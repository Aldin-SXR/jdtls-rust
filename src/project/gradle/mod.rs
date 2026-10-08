//! Gradle project import (`GradleProjectImporter` and Buildship's
//! synchronization). The build is run through the Tooling API ([`model`]) and
//! its `EclipseProject` model becomes the Java project model. Where Gradle
//! cannot be run, the build scripts are read statically ([`fallback`]).

pub mod checksums;
pub mod config;
pub mod diagnostics;
mod fallback;
pub mod model;
pub mod persistence;
pub(crate) mod sha256;
pub mod util;

use super::detect::FileDetector;
use super::metadata::{self, MetadataSettings};
use super::{
    compliance_options, normalize_java_version, ClasspathEntry, EntryKind, ImportSettings,
    Project, ProjectKind, Workspace,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const BUILD_FILES: &[&str] = &[
    "build.gradle",
    "settings.gradle",
    "build.gradle.kts",
    "settings.gradle.kts",
];

pub fn import(
    root: &Path,
    settings: &ImportSettings,
    ws: &Workspace,
    configs: Option<&[PathBuf]>,
) -> Vec<Project> {
    let dirs: Vec<PathBuf> = match configs {
        Some(files) => {
            let non_gradle: Vec<&PathBuf> = ws
                .projects
                .iter()
                .filter(|p| !p.has_nature(super::GRADLE_NATURE))
                .map(|p| &p.location)
                .collect();
            let dirs: Vec<PathBuf> = files
                .iter()
                .filter(|f| {
                    f.file_name()
                        .is_some_and(|n| BUILD_FILES.contains(&n.to_string_lossy().as_ref()))
                })
                .filter_map(|f| f.parent().map(Path::to_path_buf))
                .filter(|d| !non_gradle.iter().any(|p| *p == d))
                .collect();
            eliminate_nested_paths(dirs)
        }
        None => {
            let mut detector = FileDetector::new(root, BUILD_FILES)
                .include_nested(false)
                .add_exclusions(["**/build", "**/bin"])
                .add_exclusions(&settings.exclusions);
            for p in ws
                .projects
                .iter()
                .filter(|p| p.kind != ProjectKind::Invisible)
            {
                detector =
                    detector.add_exclusions([p.location.to_string_lossy().replace('\\', "\\\\")]);
            }
            detector.scan()
        }
    };
    let mut out = Vec::new();
    for dir in dirs {
        let dir = super::canonicalize_lenient(&dir);
        out.extend(import_dir(&dir, settings));
    }
    out
}

/// `AbstractProjectImporter.eliminateNestedPaths`.
fn eliminate_nested_paths(mut dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    dirs.sort_by_key(|d| d.components().count());
    let mut out: Vec<PathBuf> = Vec::new();
    for d in dirs {
        if !out.iter().any(|o| d.starts_with(o)) {
            out.push(d);
        }
    }
    out
}

/// `GradleProjectImporter.inferGradleJavaHome`: a Gradle that cannot run on
/// the default JDK is launched with the newest JDK it supports.
fn infer_gradle_java_home(config: &mut config::BuildConfiguration, settings: &ImportSettings) {
    let gs = &settings.gradle;
    if gs.java_home.as_deref().is_some_and(|h| !h.trim().is_empty()) {
        return;
    }
    let java_version = match config::get_java_home(gs) {
        Some(home) => super::vm_version(&home),
        None => settings.vm_version.clone(),
    };
    let Some(gradle_version) = model::requested_version(config) else {
        return;
    };
    if util::is_incompatible(Some(&gradle_version), java_version.as_deref()) {
        let highest = util::get_highest_supported_java(&gradle_version);
        let installs = util::get_all_vm_installs(settings.runtime_registry.as_ref(), &gs.runtimes);
        if let Some(home) = util::get_jdk_to_launch_daemon(&installs, highest) {
            config.java_home = Some(home);
        }
    }
}

fn import_dir(dir: &Path, settings: &ImportSettings) -> Vec<Project> {
    let mut config = config::get_build_configuration(dir, &settings.gradle);
    infer_gradle_java_home(&mut config, settings);
    match model::fetch(&config, &settings.gradle) {
        Ok(m) => {
            let projects: Vec<Project> = m
                .project
                .flatten()
                .into_iter()
                .map(|p| project_from_model(p, settings))
                .collect();
            let mut projects = projects;
            compile_other_languages(&config, settings, dir, &mut projects);
            write_metadata(&m, &projects, &config, settings);
            projects
        }
        Err(model::FetchError::Unavailable(reason)) => {
            tracing::warn!(
                "Gradle is not available, reading {} statically: {reason}",
                dir.display()
            );
            fallback::import_build(dir)
        }
        Err(model::FetchError::Failed { message, .. }) => {
            tracing::error!("Gradle synchronization of {} failed: {message}", dir.display());
            failed_import(dir, &message)
        }
    }
}

/// `GradleBuildSupport.compile`: build the Kotlin, Groovy, AspectJ and Scala
/// sources with Gradle and report the compilers' diagnostics.
fn compile_other_languages(
    config: &config::BuildConfiguration,
    settings: &ImportSettings,
    dir: &Path,
    projects: &mut [Project],
) {
    let gs = &settings.gradle;
    if !(gs.kotlin_support || gs.groovy_support || gs.aspectj_support || gs.scala_support) {
        return;
    }
    let output = match model::compile(config, gs) {
        Ok(o) if !o.tasks.is_empty() => o,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!("Gradle compile of {} failed: {e}", dir.display());
            return;
        }
    };
    let options = diagnostics::ParseOptions::for_tasks(&output.tasks);
    for marker in diagnostics::parse(&output.stderr, options, dir) {
        let file = marker.resource.clone().unwrap_or_default();
        if let Some(p) = projects
            .iter_mut()
            .filter(|p| file.starts_with(&p.location))
            .max_by_key(|p| p.location.components().count())
        {
            p.markers.push(marker);
        }
    }
}

/// A build that could not be synchronized keeps its Gradle project, with the
/// failure reported on it.
fn failed_import(dir: &Path, message: &str) -> Vec<Project> {
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut p = Project::new(name, dir, ProjectKind::Gradle);
    p.natures = vec![super::GRADLE_NATURE.to_owned()];
    p.build_files = existing_build_files(dir);
    p.markers.push(super::Marker::project(message, 1, "0"));
    vec![p]
}

fn existing_build_files(dir: &Path) -> Vec<PathBuf> {
    BUILD_FILES
        .iter()
        .map(|f| dir.join(f))
        .filter(|f| f.is_file())
        .collect()
}

fn project_from_model(p: &model::ModelProject, settings: &ImportSettings) -> Project {
    let metadata = &settings.metadata;
    let dir = super::canonicalize_lenient(&p.dir);
    let mut project = Project::new(&p.name, &dir, ProjectKind::Gradle);
    project.natures = p.natures.clone();
    if settings.gradle.scala_support {
        // `ScalaGradleSupport.cleanScalaProjects`: natures Eclipse does not know.
        project
            .natures
            .retain(|n| n == super::JAVA_NATURE || n == super::GRADLE_NATURE);
    }
    if !project.has_nature(super::GRADLE_NATURE) {
        project.natures.push(super::GRADLE_NATURE.to_owned());
    }
    project.build_files = existing_build_files(&dir);
    if project.is_java() {
        let mut options = metadata::read_preferences(metadata, &project, metadata::JDT_CORE_PREFS);
        if let Some(java) = &p.java {
            if let Some(source) = java.source.as_deref().and_then(normalize_java_version) {
                let target = java
                    .target
                    .as_deref()
                    .and_then(normalize_java_version)
                    .unwrap_or_else(|| source.clone());
                options.extend(compliance_options(&source));
                options.insert(super::TARGET.to_owned(), target);
            }
        }
        project.options = options;
        project.classpath = classpath_from_model(&project, p, &dir);
        if settings.gradle.scala_support {
            project.classpath.retain(|e| {
                e.kind != EntryKind::Container
                    || e.is_jre_container()
                    || e.path == super::GRADLE_CONTAINER
            });
        }
        project.output = Some(dir.join(default_output(p)));
    }
    project
}

/// Buildship keeps the default output folder apart from the source output folders.
fn default_output(p: &model::ModelProject) -> String {
    let output = p.output.clone().unwrap_or_else(|| "bin/default".to_owned());
    let nested = p.sources.iter().any(|s| {
        s.output
            .as_deref()
            .is_some_and(|o| o == output || o.starts_with(&format!("{output}/")))
    });
    if nested {
        format!("{output}-default")
    } else {
        output
    }
}

fn classpath_from_model(project: &Project, p: &model::ModelProject, dir: &Path) -> Vec<ClasspathEntry> {
    let mut classpath = Vec::new();
    for s in &p.sources {
        let location = if s.dir.as_os_str().is_empty() {
            dir.join(&s.path)
        } else {
            super::canonicalize_lenient(&s.dir)
        };
        let mut e = ClasspathEntry::new(EntryKind::Source, project.full_path(&location));
        e.output = s.output.as_deref().map(|o| dir.join(o));
        e.attributes = s.attributes.clone();
        e.inclusions = s.includes.clone();
        e.exclusions = s.excludes.clone();
        e.location = Some(location);
        classpath.push(e);
    }
    let jre = p
        .containers
        .iter()
        .find(|c| c.starts_with(super::JRE_CONTAINER))
        .map(|c| c.trim_end_matches('/').to_owned())
        .unwrap_or_else(|| super::JRE_CONTAINER.to_owned());
    classpath.push(ClasspathEntry::new(EntryKind::Container, jre));
    let mut container = ClasspathEntry::new(EntryKind::Container, super::GRADLE_CONTAINER);
    for d in &p.classpath {
        let mut e = ClasspathEntry::new(EntryKind::Library, d.file.to_string_lossy().into_owned());
        e.attributes = d.attributes.clone();
        e.exported = d.exported;
        e.source_attachment = d.source.clone();
        e.location = Some(d.file.clone());
        container.children.push(e);
    }
    for d in &p.project_dependencies {
        let mut e = ClasspathEntry::new(EntryKind::Project, format!("/{}", d.path.trim_start_matches('/')));
        e.attributes = d.attributes.clone();
        e.exported = d.exported;
        container.children.push(e);
    }
    classpath.push(container);
    classpath
}

/// `GradleBuildSupport.isBuildFile(resource)`: a Gradle script or
/// `gradle.properties` of a Gradle project, outside the output folder.
pub fn is_build_file(project: &Project, file: &Path) -> bool {
    let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let build_like = name == "gradle.properties"
        || name.ends_with(".gradle")
        || name.ends_with(".gradle.kts");
    if !(build_like && project.has_nature(super::GRADLE_NATURE) && file.starts_with(&project.location)) {
        return false;
    }
    if !project.is_java() {
        return true;
    }
    project.output.as_ref().is_none_or(|output| !file.starts_with(output))
}

fn relative_path(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_components)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to_components[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn distribution_value(d: &config::GradleDistribution) -> String {
    match d {
        config::GradleDistribution::Wrapper => "GRADLE_DISTRIBUTION(WRAPPER)".to_owned(),
        config::GradleDistribution::FixedVersion(v) => format!("GRADLE_DISTRIBUTION(VERSION({v}))"),
        config::GradleDistribution::Local(p) => {
            format!("GRADLE_DISTRIBUTION(LOCAL_INSTALLATION({}))", p.display())
        }
    }
}

/// The files Buildship writes for the imported projects.
fn write_metadata(
    m: &model::GradleModel,
    projects: &[Project],
    config: &config::BuildConfiguration,
    settings: &ImportSettings,
) {
    let root = projects.first().map(|p| p.location.clone());
    let filter = format!(
        "{}|{}",
        settings.resource_filters.patterns().join("|"),
        super::resource_filters::CREATED_BY_JAVA_LANGUAGE_SERVER
    );
    let filter = if settings.resource_filters.patterns().is_empty() {
        String::new()
    } else {
        filter
    };
    for (p, project) in m.project.flatten().into_iter().zip(projects) {
        let mut builders: Vec<&str> = p
            .build_commands
            .iter()
            .map(String::as_str)
            .filter(|b| !settings.gradle.scala_support || *b == "org.eclipse.jdt.core.javabuilder")
            .collect();
        builders.push("org.eclipse.buildship.core.gradleprojectbuilder");
        let comment = format!("Project {} created by Buildship.", project.name);
        let result = (|| -> std::io::Result<()> {
            metadata::write_project_file(
                &settings.metadata,
                project,
                &metadata::project_description(project, &comment, &builders, &filter),
            )?;
            if project.is_java() {
                metadata::write_classpath_file(&settings.metadata, project)?;
                write_compliance(project, settings)?;
            }
            let mut prefs: BTreeMap<String, String> = BTreeMap::new();
            if Some(&project.location) == root.as_ref() {
                prefs.insert("arguments".into(), config.arguments.join(" "));
                prefs.insert("auto.sync".into(), config.auto_sync.to_string());
                prefs.insert("build.scans.enabled".into(), "false".into());
                prefs.insert(
                    "connection.gradle.distribution".into(),
                    distribution_value(&config.distribution),
                );
                prefs.insert("connection.project.dir".into(), String::new());
                prefs.insert(
                    "gradle.user.home".into(),
                    config
                        .gradle_user_home
                        .as_ref()
                        .map(|h| h.display().to_string())
                        .unwrap_or_default(),
                );
                prefs.insert(
                    "java.home".into(),
                    config
                        .java_home
                        .as_ref()
                        .map(|h| h.display().to_string())
                        .unwrap_or_default(),
                );
                prefs.insert("jvm.arguments".into(), config.jvm_arguments.join(" "));
                prefs.insert("offline.mode".into(), config.offline_mode.to_string());
                prefs.insert(
                    "override.workspace.settings".into(),
                    config.override_workspace_settings.to_string(),
                );
                prefs.insert("show.console.view".into(), "true".into());
                prefs.insert("show.executions.view".into(), "true".into());
            } else if let Some(root) = &root {
                prefs.insert(
                    "connection.project.dir".into(),
                    relative_path(&project.location, root),
                );
            }
            metadata::write_preferences(
                &settings.metadata,
                project,
                "org.eclipse.buildship.core.prefs",
                &prefs,
            )
        })();
        if let Err(e) = result {
            tracing::warn!("Cannot write the metadata of {}: {e}", project.name);
        }
    }
}

/// The compliance settings that differ from the workspace default.
fn write_compliance(project: &Project, settings: &ImportSettings) -> std::io::Result<()> {
    let mut prefs = metadata::read_preferences(&settings.metadata, project, metadata::JDT_CORE_PREFS);
    let default = settings.vm_version.clone().or_else(|| settings.default_compliance().into());
    let mut changed = false;
    for key in [super::COMPLIANCE, super::SOURCE, super::TARGET] {
        if let Some(v) = project.options.get(key) {
            if Some(v) != default.as_ref() && prefs.get(key) != Some(v) {
                prefs.insert(key.to_owned(), v.clone());
                changed = true;
            }
        }
    }
    if changed {
        metadata::write_preferences(&settings.metadata, project, metadata::JDT_CORE_PREFS, &prefs)?;
    }
    Ok(())
}
