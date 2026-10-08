//! Workspace project model — Rust port of the jdt.ls project importers
//! (`GradleProjectImporter`, `MavenProjectImporter`, `EclipseProjectImporter`,
//! `InvisibleProjectImporter`) and of the parts of the Eclipse resource and
//! Java models they produce (natures, raw classpath, output location,
//! build-path markers).
//!
//! The model is purely additive: documents that are not inside any imported
//! project (including virtual documents with no file on disk, e.g.
//! `untitled:` or `inmemory://` URIs) belong to the default project, which
//! uses the configured classpath and compliance only.

pub mod detect;
pub mod download;
pub mod eclipse;
pub mod classpath;
pub mod gradle;
pub mod invisible;
pub mod jar;
pub mod jdt_defaults;
pub mod maven;
pub mod null_analysis;
pub mod prefs;
pub mod resource_filters;
pub mod runtime;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub const COMPLIANCE: &str = "org.eclipse.jdt.core.compiler.compliance";
pub const SOURCE: &str = "org.eclipse.jdt.core.compiler.source";
pub const TARGET: &str = "org.eclipse.jdt.core.compiler.codegen.targetPlatform";
pub const RELEASE: &str = "org.eclipse.jdt.core.compiler.release";
pub const ENABLE_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures";
pub const REPORT_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures";

/// Name jdt.ls gives the project holding files outside any build project.
pub const DEFAULT_PROJECT_NAME: &str = "jdt.ls-java-project";

pub const JAVA_NATURE: &str = "org.eclipse.jdt.core.javanature";
pub const MAVEN_NATURE: &str = "org.eclipse.m2e.core.maven2Nature";
pub const GRADLE_NATURE: &str = "org.eclipse.buildship.core.gradleprojectnature";
/// `UnmanagedFolderNature.NATURE_ID` (invisible projects).
pub const UNMANAGED_FOLDER_NATURE: &str = "org.eclipse.jdt.ls.unmanagedFolderNature";

/// Bridge-only environment option; imported projects use their declared JRE.
pub const INCLUDE_RUNNING_VM: &str = "jdtls.bridge.includeRunningVM";

pub const JRE_CONTAINER: &str = "org.eclipse.jdt.launching.JRE_CONTAINER";
pub const MAVEN_CONTAINER: &str = "org.eclipse.m2e.MAVEN2_CLASSPATH_CONTAINER";
pub const GRADLE_CONTAINER: &str = "org.eclipse.buildship.core.gradleclasspathcontainer";

/// `ProjectUtils.WORKSPACE_LINK`: the linked folder of an invisible project.
pub const WORKSPACE_LINK: &str = "_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    Eclipse,
    Maven,
    Gradle,
    Invisible,
    Default,
}

/// `IClasspathEntry.CPE_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Library = 1,
    Project = 2,
    Source = 3,
    Variable = 4,
    Container = 5,
}

/// One raw classpath entry (`IClasspathEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClasspathEntry {
    pub kind: EntryKind,
    /// The Eclipse path: the workspace-relative full path of a source folder
    /// (`/project/src`), of a project (`/project`) or of a library inside the
    /// workspace (`/project/lib/a.jar`); the absolute path of an external
    /// library; the container path.
    pub path: String,
    /// File-system location of a source folder or library.
    pub location: Option<PathBuf>,
    /// Specific output location (file system).
    pub output: Option<PathBuf>,
    pub source_attachment: Option<PathBuf>,
    /// Extra attributes, in declaration order.
    pub attributes: Vec<(String, String)>,
    pub inclusions: Vec<String>,
    pub exclusions: Vec<String>,
    pub exported: bool,
    /// Resolved entries of a classpath container (Maven/Gradle dependencies).
    pub children: Vec<ClasspathEntry>,
}

impl ClasspathEntry {
    pub fn new(kind: EntryKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            location: None,
            output: None,
            source_attachment: None,
            attributes: Vec::new(),
            inclusions: Vec::new(),
            exclusions: Vec::new(),
            exported: false,
            children: Vec::new(),
        }
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attribute(&mut self, name: &str, value: &str) {
        if let Some(a) = self.attributes.iter_mut().find(|(k, _)| k == name) {
            a.1 = value.to_owned();
        } else {
            self.attributes.push((name.to_owned(), value.to_owned()));
        }
    }

    /// `IClasspathEntry.isTest()`.
    pub fn is_test(&self) -> bool {
        self.attribute("test") == Some("true")
    }

    pub fn is_jre_container(&self) -> bool {
        self.kind == EntryKind::Container && self.path.starts_with(JRE_CONTAINER)
    }
}

/// A problem marker on a project or one of its build files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// `None`: the project itself; otherwise the resource (e.g. `pom.xml`).
    pub resource: Option<PathBuf>,
    pub message: String,
    /// LSP severity: 1 error, 2 warning, 3 information.
    pub severity: u8,
    /// `IJavaModelMarker.ID` (published as the diagnostic code).
    pub code: String,
    /// Zero-based `(line, start column, end column)` for markers on a file.
    pub range: Option<(u32, u32, u32)>,
    /// Computed by `Workspace::finish` (recomputed on every change).
    pub derived: bool,
}

impl Marker {
    pub fn project(message: impl Into<String>, severity: u8, code: impl Into<String>) -> Self {
        Self {
            resource: None,
            message: message.into(),
            severity,
            code: code.into(),
            range: None,
            derived: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFolder {
    pub path: PathBuf,
    pub is_test: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub path: PathBuf,
    pub source: Option<PathBuf>,
    pub is_test: bool,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    /// The project's real folder (`ProjectUtils.getProjectRealFolder`).
    pub root: PathBuf,
    /// `IProject.getLocation()`: equal to `root` except for invisible
    /// projects, which live in the jdt.ls workspace and link `root` as `_`.
    pub location: PathBuf,
    pub kind: ProjectKind,
    pub natures: Vec<String>,
    /// The raw classpath (`IJavaProject.getRawClasspath()`).
    pub classpath: Vec<ClasspathEntry>,
    /// Default output location (file system).
    pub output: Option<PathBuf>,
    /// Project-level problems (build path errors, build file markers).
    pub markers: Vec<Marker>,
    /// Build descriptor files of the project (pom.xml, build.gradle, …).
    pub build_files: Vec<PathBuf>,
    /// Derived from `classpath`: source folders.
    pub source_folders: Vec<SourceFolder>,
    /// Derived from `classpath`: libraries (container contents included).
    pub libraries: Vec<Library>,
    /// Derived from `classpath`: names of workspace projects this project depends on.
    pub project_deps: Vec<String>,
    /// JDT core options specific to this project (compliance, prefs file).
    pub options: BTreeMap<String, String>,
    /// Maven: the selected profiles (`org.eclipse.m2e.core.selectedProfiles`).
    pub selected_profiles: String,
    /// Managed resource filters, excluding the default project.
    pub resource_filters: resource_filters::ResourceFilters,
    pub runtime: Option<runtime::VmInstall>,
    /// The encoding the build sets for the project (m2e: the POM's
    /// `project.build.sourceEncoding`).
    pub encoding: Option<String>,
}

impl Project {
    pub fn new(name: impl Into<String>, root: &Path, kind: ProjectKind) -> Self {
        Self {
            name: name.into(),
            root: root.to_path_buf(),
            location: root.to_path_buf(),
            kind,
            natures: Vec::new(),
            classpath: Vec::new(),
            output: None,
            markers: Vec::new(),
            build_files: Vec::new(),
            source_folders: Vec::new(),
            libraries: Vec::new(),
            project_deps: Vec::new(),
            options: BTreeMap::new(),
            selected_profiles: String::new(),
            resource_filters: resource_filters::ResourceFilters::default(),
            runtime: None,
            encoding: None,
        }
    }

    /// `IProject.getDefaultCharset(false)`: the project's explicit encoding,
    /// from its build or `.settings/org.eclipse.core.resources.prefs`
    /// (`encoding/<project>`).
    pub fn explicit_encoding(&self) -> Option<String> {
        if let Some(e) = &self.encoding {
            return Some(e.clone());
        }
        let prefs = prefs::read_properties(&self.location.join(".settings").join("org.eclipse.core.resources.prefs"))?;
        prefs.get("encoding/<project>").filter(|e| !e.is_empty()).cloned()
    }

    pub fn is_java(&self) -> bool {
        self.natures.iter().any(|n| n == JAVA_NATURE)
    }

    pub fn has_nature(&self, nature: &str) -> bool {
        self.natures.iter().any(|n| n == nature)
    }

    pub fn compliance(&self) -> Option<&str> {
        self.options.get(COMPLIANCE).map(String::as_str)
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// The Eclipse full path (`/name/relative`) of `location` inside the project.
    pub fn full_path(&self, location: &Path) -> String {
        let (base, prefix) =
            if self.kind == ProjectKind::Invisible && location.starts_with(&self.root) {
                (
                    self.root.as_path(),
                    format!("/{}/{WORKSPACE_LINK}", self.name),
                )
            } else {
                (self.location.as_path(), format!("/{}", self.name))
            };
        match location.strip_prefix(base) {
            Ok(rel) if rel.as_os_str().is_empty() => prefix,
            Ok(rel) => format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/")),
            Err(_) => location.to_string_lossy().into_owned(),
        }
    }

    /// The file-system location of an Eclipse full path inside this project.
    pub fn location_of(&self, full_path: &str) -> Option<PathBuf> {
        let rest = full_path.strip_prefix('/')?;
        let (name, rel) = rest.split_once('/').unwrap_or((rest, ""));
        if name != self.name {
            return None;
        }
        if self.kind == ProjectKind::Invisible {
            if rel == WORKSPACE_LINK {
                return Some(self.root.clone());
            }
            if let Some(r) = rel.strip_prefix(&format!("{WORKSPACE_LINK}/")) {
                return Some(self.root.join(r));
            }
        }
        Some(if rel.is_empty() {
            self.location.clone()
        } else {
            self.location.join(rel)
        })
    }

    /// Recompute `source_folders`, `libraries` and `project_deps` from the
    /// raw classpath.
    pub fn derive_views(&mut self) {
        let mut sources = Vec::new();
        let mut libs = Vec::new();
        let mut deps = Vec::new();
        fn walk(
            entries: &[ClasspathEntry],
            sources: &mut Vec<SourceFolder>,
            libs: &mut Vec<Library>,
            deps: &mut Vec<String>,
        ) {
            for e in entries {
                match e.kind {
                    EntryKind::Source => {
                        if let Some(loc) = &e.location {
                            if !sources.iter().any(|s: &SourceFolder| &s.path == loc) {
                                sources.push(SourceFolder {
                                    path: loc.clone(),
                                    is_test: e.is_test(),
                                });
                            }
                        }
                    }
                    EntryKind::Library | EntryKind::Variable => {
                        if let Some(loc) = &e.location {
                            if !libs.iter().any(|l: &Library| &l.path == loc) {
                                libs.push(Library {
                                    path: loc.clone(),
                                    source: e.source_attachment.clone(),
                                    is_test: e.is_test(),
                                });
                            }
                        }
                    }
                    EntryKind::Project => {
                        let name = e.path.trim_start_matches('/').to_owned();
                        if !deps.contains(&name) {
                            deps.push(name);
                        }
                    }
                    EntryKind::Container => walk(&e.children, sources, libs, deps),
                }
            }
        }
        walk(&self.classpath, &mut sources, &mut libs, &mut deps);
        self.source_folders = sources;
        self.libraries = libs;
        self.project_deps = deps;
    }

    pub fn source_folder_for(&self, path: &Path) -> Option<&SourceFolder> {
        if self.is_filtered(path) {
            return None;
        }
        self.source_folders
            .iter()
            .filter(|sf| path.starts_with(&sf.path) && !self.is_excluded(path, &sf.path))
            .max_by_key(|sf| sf.path.components().count())
    }

    pub fn is_filtered(&self, path: &Path) -> bool {
        self.resource_filters.is_filtered(&self.root, path)
    }

    /// Whether `path` is excluded from the source entry at `folder`
    /// (exclusion patterns such as `bin/`, nested source folders).
    fn is_excluded(&self, path: &Path, folder: &Path) -> bool {
        let Some(entry) = self
            .classpath
            .iter()
            .find(|e| e.kind == EntryKind::Source && e.location.as_deref() == Some(folder))
        else {
            return false;
        };
        let Ok(rel) = path.strip_prefix(folder) else {
            return false;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        entry
            .exclusions
            .iter()
            .any(|ex| match ex.strip_suffix('/') {
                Some(dir) => rel == dir || rel.starts_with(&format!("{dir}/")),
                None => detect::glob_to_regex(ex).is_some_and(|re| re.is_match(&rel)),
            })
    }

    /// All `.java` files beneath this project's source folders.
    pub fn java_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for sf in &self.source_folders {
            for entry in walkdir::WalkDir::new(&sf.path)
                .follow_links(true)
                .into_iter()
                .filter_entry(|entry| !self.is_filtered(entry.path()))
                .flatten()
            {
                let p = entry.path();
                if entry.file_type().is_file()
                    && p.extension().is_some_and(|e| e == "java")
                    && self.source_folder_for(p).is_some_and(|f| f.path == sf.path)
                    && seen.insert(p.to_path_buf())
                {
                    out.push(p.to_path_buf());
                }
            }
        }
        out.sort();
        out
    }

    /// Whether the build path has errors (`The project cannot be built until
    /// build path errors are resolved`): JDT then skips compiling it.
    pub fn has_build_path_errors(&self) -> bool {
        self.markers
            .iter()
            .any(|m| m.resource.is_none() && m.severity == 1)
    }
}

/// `java.project.referencedLibraries`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferencedLibraries {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub sources: BTreeMap<String, String>,
}

/// Settings that influence import (subset of jdt.ls `Preferences`).
#[derive(Debug, Clone, Default)]
pub struct ImportSettings {
    pub exclusions: Vec<String>,
    pub maven_enabled: bool,
    pub gradle_enabled: bool,
    /// `java.project.sourcePaths` for invisible projects (`None`: infer).
    pub source_paths: Option<Vec<String>>,
    /// `java.project.outputPath` for invisible projects.
    pub output_path: Option<String>,
    /// `java.project.referencedLibraries`.
    pub referenced_libraries: ReferencedLibraries,
    /// `initializationOptions.triggerFiles`.
    pub trigger_files: Vec<PathBuf>,
    /// `initializationOptions.projectConfigurations` (`None`: scan roots).
    pub project_configurations: Option<Vec<PathBuf>>,
    /// The jdt.ls workspace (`-data`) directory.
    pub data_dir: Option<PathBuf>,
    /// The default VM's home and major version (`"25"`).
    pub vm_home: Option<PathBuf>,
    pub runtime_registry: Option<runtime::RuntimeRegistry>,
    pub vm_version: Option<String>,
    pub maven: maven::MavenSettings,
    /// `java.compile.nullAnalysis.*`.
    pub null_analysis: null_analysis::NullAnalysisSettings,
    pub resource_filters: resource_filters::ResourceFilters,
    pub gradle: gradle::config::GradleSettings,
}

impl ImportSettings {
    pub fn jdtls_defaults() -> Self {
        Self {
            exclusions: detect::DEFAULT_IMPORT_EXCLUSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            maven_enabled: true,
            gradle_enabled: true,
            source_paths: None,
            output_path: None,
            referenced_libraries: ReferencedLibraries::jdtls_default(),
            trigger_files: Vec::new(),
            project_configurations: None,
            data_dir: None,
            vm_home: None,
            runtime_registry: None,
            vm_version: None,
            maven: maven::MavenSettings::default(),
            null_analysis: null_analysis::NullAnalysisSettings {
                mode: "disabled".to_owned(),
                ..Default::default()
            },
            resource_filters: resource_filters::ResourceFilters::jdtls_default(),
            gradle: gradle::config::GradleSettings::default(),
        }
    }

    /// The workspace default compliance (that of the default VM).
    pub fn default_compliance(&self) -> String {
        self.vm_version.clone().unwrap_or_else(|| "21".to_owned())
    }

    /// Location of a project jdt.ls creates in its own workspace.
    pub fn workspace_location(&self, name: &str) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("jdtls-rust-workspace"))
            .join(name)
    }
}

/// The set of imported projects.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub projects: Vec<Project>,
    /// Whether the default project (`jdt.ls-java-project`) exists.
    pub default_project: Option<PathBuf>,
    /// The workspace root paths.
    pub roots: Vec<PathBuf>,
    /// Major version of the default VM (`"25"`).
    pub vm_version: Option<String>,
    pub runtime_registry: Option<runtime::RuntimeRegistry>,
}

impl Workspace {
    /// `IWorkspaceRoot.getProjects`, including the default project handle.
    pub fn all_projects(&self) -> Vec<Project> {
        let mut projects = self.projects.clone();
        if let Some(root) = &self.default_project {
            let mut project = eclipse::load(root).unwrap_or_else(|| default_java_project(root));
            project.kind = ProjectKind::Default;
            if let Some(vm) = self.runtime_registry.as_ref().and_then(|r| r.default_install()) {
                project.runtime = Some(vm.clone());
                for entry in project.classpath.iter_mut().filter(|e| e.is_jre_container()) {
                    entry.children = vm.classpath_entries();
                }
                project.derive_views();
                runtime::configure_project_preview(&mut project, vm);
            }
            projects.push(project);
        }
        projects.sort_by(|a, b| a.name.cmp(&b.name));
        projects
    }
    /// Materialize the default Java project created by empty-root initialization
    /// or needed by a standalone buffer. Source buffers themselves stay virtual.
    pub fn ensure_default_project(&self) -> std::io::Result<()> {
        let Some(root) = &self.default_project else {
            return Ok(());
        };
        std::fs::create_dir_all(root.join("src"))?;
        std::fs::create_dir_all(root.join("bin"))?;
        let description = root.join(".project");
        if !description.exists() {
            std::fs::write(description, format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription><name>{DEFAULT_PROJECT_NAME}</name><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>{JAVA_NATURE}</nature></natures></projectDescription>\n"))?;
        }
        let classpath = root.join(".classpath");
        if !classpath.exists() {
            std::fs::write(classpath, format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"{JRE_CONTAINER}\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>\n"))?;
        }
        Ok(())
    }

    pub fn configure_filters(&mut self, filters: &resource_filters::ResourceFilters) {
        for project in &mut self.projects {
            if project.kind != ProjectKind::Default {
                project.resource_filters = filters.clone();
            }
        }
    }
    /// Import every project found under `roots`, mirroring jdt.ls
    /// `ProjectsManager.initializeProjects`: for each root the importers run
    /// in order Gradle (300) → Maven (400) → Eclipse (1000) → Invisible
    /// (1500); each skips the folders of projects already in the workspace.
    pub fn import(roots: &[PathBuf], settings: &ImportSettings) -> Self {
        Self::import_with_previous(roots, settings, None)
    }

    /// [`Workspace::import`] keeping the invisible projects of `previous`
    /// (jdt.ls persists them in its workspace and only updates them
    /// incrementally).
    pub fn import_with_previous(
        roots: &[PathBuf],
        settings: &ImportSettings,
        previous: Option<&Workspace>,
    ) -> Self {
        let roots: Vec<PathBuf> = roots.iter().map(|r| canonicalize_lenient(r)).collect();
        let mut ws = Workspace {
            projects: Vec::new(),
            default_project: None,
            roots: roots.clone(),
            vm_version: settings.vm_version.clone(),
            runtime_registry: None,
        };
        if roots.is_empty() {
            ws.default_project = Some(settings.workspace_location(DEFAULT_PROJECT_NAME));
        }
        for root in &roots {
            if !root.is_dir() {
                continue;
            }
            let configs: Option<Vec<PathBuf>> = settings.project_configurations.as_ref().map(|c| {
                c.iter()
                    .map(|p| canonicalize_lenient(p))
                    .filter(|p| p.starts_with(root))
                    .collect()
            });
            if settings.gradle_enabled {
                for p in gradle::import(root, settings, &ws, configs.as_deref()) {
                    ws.add(p);
                }
            }
            if settings.maven_enabled {
                for p in maven::import(root, settings, &ws, configs.as_deref()) {
                    ws.add(p);
                }
            }
            for p in eclipse::import(root, settings, &ws, configs.as_deref()) {
                ws.add(p);
            }
            if let Some(prev) = previous.and_then(|w| {
                w.projects
                    .iter()
                    .find(|p| p.kind == ProjectKind::Invisible && p.root == *root)
            }) {
                if ws.visible_projects_under(root).is_empty()
                    || !prev.has_nature(UNMANAGED_FOLDER_NATURE)
                {
                    ws.default_project = previous
                        .and_then(|w| w.default_project.clone())
                        .or(ws.default_project.take());
                    ws.add(prev.clone());
                    continue;
                }
            }
            if let Some(restored) = invisible::restore_project(root, settings) {
                if ws.visible_projects_under(root).is_empty()
                    || !restored.has_nature(UNMANAGED_FOLDER_NATURE)
                {
                    // A trigger re-runs the importer on initialization, applying
                    // source/output preferences even when the project exists.
                    let imported = if configs.is_none() {
                        settings.trigger_files.iter().map(|t| canonicalize_lenient(t))
                            .find(|t| t.starts_with(root))
                            .and_then(|t| invisible::load_invisible_project(&t, root, settings, &ws))
                    } else { None };
                    ws.add(imported.unwrap_or(restored));
                    continue;
                }
            }
            if configs.is_none() && !settings.trigger_files.is_empty() {
                ws.default_project
                    .get_or_insert_with(|| settings.workspace_location(DEFAULT_PROJECT_NAME));
                if let Some(trigger) = settings
                    .trigger_files
                    .iter()
                    .map(|t| canonicalize_lenient(t))
                    .find(|t| t.starts_with(root))
                {
                    if let Some(p) =
                        invisible::load_invisible_project(&trigger, root, settings, &ws)
                    {
                        ws.add(p);
                    }
                }
            }
        }
        if let Some(registry) = &settings.runtime_registry {
            registry.apply_to_workspace(&mut ws);
        } else {
            ws.finish();
        }
        // `projectsBuildFinished`: the annotation-based null analysis options.
        let vm = ws.vm_version.clone();
        for p in &mut ws.projects {
            null_analysis::update_project(p, &settings.null_analysis, vm.as_deref());
        }
        ws
    }

    /// Workspace-level validation once every project is known: missing
    /// required projects, then JDT's "cannot be built" marker on every
    /// project with build path errors.
    pub fn finish(&mut self) {
        const CANNOT_BUILD: &str =
            "The project cannot be built until build path errors are resolved";
        let names: Vec<(String, bool)> = self
            .projects
            .iter()
            .map(|p| (p.name.clone(), p.is_java()))
            .collect();
        let vm = self.vm_version.clone();
        for p in &mut self.projects {
            p.markers.retain(|m| !m.derived);
            if !p.is_java() {
                continue;
            }
            for mut m in eclipse::classpath_problems(p) {
                m.derived = true;
                p.markers.push(m);
            }
            let missing: Vec<String> = p
                .classpath
                .iter()
                .filter(|e| e.kind == EntryKind::Project)
                .map(|e| e.path.trim_start_matches('/').to_owned())
                .filter(|n| !names.iter().any(|(name, java)| name == n && *java))
                .collect();
            for m in missing {
                let mut marker = Marker::project(
                    format!(
                        "Project '{}' is missing required Java project: '{m}'",
                        p.name
                    ),
                    1,
                    "964",
                );
                marker.derived = true;
                p.markers.push(marker);
            }
            if let Some(vm) = p.runtime.as_ref().and_then(|v| v.major_version()).or_else(|| vm.clone()) {
                for mut m in jre_markers(p, &vm) {
                    m.derived = true;
                    p.markers.push(m);
                }
            }
            if p.markers.iter().any(|m| {
                m.resource.is_none() && m.severity == 1 && (m.code == "964" || m.code == "963")
            }) {
                let mut marker = Marker::project(CANNOT_BUILD, 1, "0");
                marker.derived = true;
                p.markers.push(marker);
            }
        }
    }

    /// Add `p`, replacing a project with the same name.
    pub fn add(&mut self, mut p: Project) {
        p.derive_views();
        if let Some(existing) = self.projects.iter_mut().find(|e| e.name == p.name) {
            *existing = p;
        } else {
            self.projects.push(p);
        }
    }

    pub fn project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// `ProjectUtils.getVisibleProjects(root)`: projects located in a root
    /// folder (all but invisible and default projects).
    pub fn visible_projects_under(&self, root: &Path) -> Vec<&Project> {
        self.projects
            .iter()
            .filter(|p| p.kind != ProjectKind::Invisible && p.location.starts_with(root))
            .collect()
    }

    /// The project owning `path`: the deepest project root containing it.
    pub fn project_for_path(&self, path: &Path) -> Option<&Project> {
        self.projects
            .iter()
            .filter(|p| p.contains_path(path) && p.kind != ProjectKind::Default && p.is_java())
            .max_by_key(|p| p.root.components().count())
    }

    pub fn project_for_uri(&self, uri: &Url) -> Option<&Project> {
        let path = uri_to_path(uri)?;
        self.project_for_path(&path)
    }

    /// `project` plus the transitive closure of its project dependencies.
    pub fn project_closure<'a>(&'a self, project: &'a Project) -> Vec<&'a Project> {
        let mut out = vec![project];
        let mut seen: HashSet<&str> = HashSet::from([project.name.as_str()]);
        let mut i = 0;
        while i < out.len() {
            for dep in &out[i].project_deps {
                if let Some(p) = self.project(dep) {
                    if seen.insert(p.name.as_str()) {
                        out.push(p);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// Map of every workspace `.java` file to the name of its project.
    pub fn java_files(&self) -> HashMap<PathBuf, String> {
        let mut out = HashMap::new();
        for p in &self.projects {
            if !p.is_java() {
                continue;
            }
            for f in p.java_files() {
                out.entry(f).or_insert_with(|| p.name.clone());
            }
        }
        out
    }

    /// Workspace projects sorted by name (`IWorkspaceRoot.getProjects()`).
    pub fn sorted_projects(&self) -> Vec<&Project> {
        let mut v: Vec<&Project> = self.projects.iter().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

/// The default project's Java model, shared by the manager and project commands.
pub fn default_java_project(location: &Path) -> Project {
    let mut project = Project::new(DEFAULT_PROJECT_NAME, location, ProjectKind::Default);
    project.natures = vec![JAVA_NATURE.to_owned()];
    let mut source = ClasspathEntry::new(EntryKind::Source, format!("/{DEFAULT_PROJECT_NAME}/src"));
    source.location = Some(location.join("src"));
    project.classpath = vec![source, ClasspathEntry::new(EntryKind::Container, JRE_CONTAINER)];
    project.output = Some(location.join("bin"));
    project
}

/// `java.io.File.toURI().toString()`: `file:` + absolute path (trailing `/`
/// for directories), quoting only characters illegal in a URI path — the
/// exact form jdt.ls returns from commands such as `java.project.getAll`.
pub fn java_file_uri(path: &Path, is_dir: bool) -> String {
    let mut p = path.to_string_lossy().replace('\\', "/");
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if is_dir && !p.ends_with('/') {
        p.push('/');
    }
    let mut out = String::from("file:");
    for c in p.chars() {
        let legal = c.is_ascii_alphanumeric()
            || "-_.!~*'()/:@&=+$,;".contains(c)
            || (!c.is_ascii() && !c.is_control() && !c.is_whitespace());
        if legal {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// `JDTUtils.getFileURI(resource)` (`ResourceUtils.fixURI`): `file:///path`.
pub fn resource_uri(path: &Path) -> String {
    let s = java_file_uri(path, false);
    format!("file://{}", s.trim_start_matches("file:"))
}

pub fn uri_to_path(uri: &Url) -> Option<PathBuf> {
    if uri.scheme() != "file" {
        return None;
    }
    let path = uri.to_file_path().ok()?;
    Some(canonicalize_lenient(&path))
}

/// Canonicalize the existing prefix of `path` (so `/tmp` vs `/private/tmp`
/// compare equal on macOS) while keeping nonexistent tails intact — virtual
/// `file:` URIs never touch the disk beyond this.
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let mut tail = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(name) = cur.file_name().map(|n| n.to_os_string()) {
        if !cur.pop() {
            break;
        }
        tail.push(name);
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for t in tail.iter().rev() {
                out.push(t);
            }
            return out;
        }
    }
    path.to_path_buf()
}

/// Normalise a Java version string the way JDT does ("8" → "1.8", "1.11" → "11").
pub fn normalize_java_version(v: &str) -> Option<String> {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
    let v = v
        .strip_prefix("JavaVersion.VERSION_")
        .map(|s| s.replace('_', "."))
        .unwrap_or_else(|| v.to_owned());
    let v = v.as_str();
    let n: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if n.is_empty() {
        return None;
    }
    let parts: Vec<&str> = n.split('.').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        ["1", minor, ..] => {
            let m: u32 = minor.parse().ok()?;
            Some(if m <= 8 {
                format!("1.{m}")
            } else {
                m.to_string()
            })
        }
        [major, ..] => {
            let m: u32 = major.parse().ok()?;
            Some(if m <= 8 {
                format!("1.{m}")
            } else {
                m.to_string()
            })
        }
        _ => None,
    }
}

pub(crate) fn compliance_options(version: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(COMPLIANCE.to_owned(), version.to_owned());
    m.insert(SOURCE.to_owned(), version.to_owned());
    m.insert(TARGET.to_owned(), version.to_owned());
    m
}

/// Default JDT core options jdt.ls applies on top of `JavaCore.getOptions()`
/// (see `PreferenceManager.initialize`).
pub fn jdtls_default_options() -> BTreeMap<String, String> {
    let pairs: &[(&str, &str)] = &[
        (
            "org.eclipse.jdt.core.codeComplete.visibilityCheck",
            "enabled",
        ),
        ("org.eclipse.jdt.core.compiler.release", "enabled"),
        (
            "org.eclipse.jdt.core.compiler.problem.unhandledWarningToken",
            "ignore",
        ),
        (
            "org.eclipse.jdt.core.compiler.problem.redundantSuperinterface",
            "warning",
        ),
        ("org.eclipse.jdt.core.codeComplete.subwordMatch", "disabled"),
        (
            "org.eclipse.jdt.core.compiler.problem.missingSerialVersion",
            "ignore",
        ),
        ("org.eclipse.jdt.core.circularClasspath", "warning"),
        (
            "org.eclipse.jdt.core.compiler.ignoreUnnamedModuleForSplitPackage",
            "enabled",
        ),
        (
            "org.eclipse.jdt.core.compiler.problem.unusedLambdaParameter",
            "ignore",
        ),
        (
            "org.eclipse.jdt.core.compiler.problem.forbiddenReference",
            "ignore",
        ),
        (
            "org.eclipse.jdt.core.compiler.doc.comment.support",
            "enabled",
        ),
    ];
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Read `<root>/.settings/org.eclipse.jdt.core.prefs` if present.
pub(crate) fn project_prefs(root: &Path) -> BTreeMap<String, String> {
    prefs::read_properties(&root.join(".settings").join("org.eclipse.jdt.core.prefs"))
        .map(|mut m| {
            m.remove("eclipse.preferences.version");
            m
        })
        .unwrap_or_default()
}

/// Locate a jar's `-sources.jar` sibling (`ProjectUtils.detectSources`).
pub(crate) fn source_attachment(jar: &Path) -> Option<PathBuf> {
    let name = jar.file_name()?.to_string_lossy();
    let stem = name.strip_suffix(".jar").unwrap_or(&name);
    let src = jar.with_file_name(format!("{stem}-sources.jar"));
    src.is_file().then_some(src)
}

/// Major version of the JDK at `home` (from its `release` file): `"25"`, `"1.8"`.
pub fn vm_version(home: &Path) -> Option<String> {
    let release = prefs::read_properties(&home.join("release"))?;
    let v = release.get("JAVA_VERSION")?;
    normalize_java_version(v.trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::normalize_java_version as n;

    #[test]
    fn java_file_uri_matches_file_to_uri() {
        use std::path::Path;
        assert_eq!(
            super::java_file_uri(Path::new("/a/b c"), true),
            "file:/a/b%20c/"
        );
        assert_eq!(
            super::java_file_uri(Path::new("/a/Foo.java"), false),
            "file:/a/Foo.java"
        );
    }

    #[test]
    fn normalizes_versions() {
        assert_eq!(n("1.8").as_deref(), Some("1.8"));
        assert_eq!(n("8").as_deref(), Some("1.8"));
        assert_eq!(n("17").as_deref(), Some("17"));
        assert_eq!(n("JavaVersion.VERSION_11").as_deref(), Some("11"));
        assert_eq!(n("JavaVersion.VERSION_1_8").as_deref(), Some("1.8"));
        assert_eq!(n("'1.7'").as_deref(), Some("1.7"));
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/projects")
            .join(rel)
    }

    #[test]
    fn imports_fixture_projects() {
        let s = ImportSettings::jdtls_defaults();
        for rel in [
            "maven/salut",
            "eclipse/hello",
            "gradle/simple-gradle",
            "maven/multimodule",
            "eclipse/reference",
        ] {
            let ws = Workspace::import(&[fixture(rel)], &s);
            for p in &ws.projects {
                eprintln!(
                    "{rel}: {} {:?} compliance={:?} src={:?} libs={:?} deps={:?}",
                    p.name,
                    p.kind,
                    p.compliance(),
                    p.source_folders
                        .iter()
                        .map(|s| s
                            .path
                            .strip_prefix(&p.root)
                            .unwrap_or(&s.path)
                            .to_path_buf())
                        .collect::<Vec<_>>(),
                    p.libraries
                        .iter()
                        .map(|l| l.path.file_name().unwrap().to_owned())
                        .collect::<Vec<_>>(),
                    p.project_deps
                );
            }
        }
        let ws = Workspace::import(&[fixture("maven/salut")], &s);
        assert_eq!(ws.projects[0].name, "salut");
        assert_eq!(ws.projects[0].compliance(), Some("1.8"));
    }
}

// ─── Preferences (`Preferences.createFrom`/`updateFrom` for import keys) ────

/// `MapFlattener.getValue`: a dotted key, or the chain of nested maps.
pub fn pref_value<'a>(
    configuration: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(v) = configuration.get(key).filter(|v| !v.is_null()) {
        return Some(v);
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut cur = configuration;
    for (i, part) in parts.iter().enumerate() {
        let v = cur.get(*part)?;
        if i == parts.len() - 1 {
            return (!v.is_null()).then_some(v);
        }
        if !v.is_object() {
            return None;
        }
        cur = v;
    }
    None
}

fn pref_bool(c: &serde_json::Value, key: &str) -> Option<bool> {
    match pref_value(c, key)? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => Some(s == "true"),
        _ => None,
    }
}

fn pref_string(c: &serde_json::Value, key: &str) -> Option<String> {
    pref_value(c, key).and_then(|v| v.as_str().map(str::to_owned))
}

fn pref_list(c: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    let arr = pref_value(c, key)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    )
}

impl ReferencedLibraries {
    /// `Preferences.JAVA_PROJECT_REFERENCED_LIBRARIES_DEFAULT`.
    pub fn jdtls_default() -> Self {
        Self {
            include: vec!["lib/**".to_owned()],
            ..Default::default()
        }
    }

    /// `java.project.referencedLibraries`: a shortcut include array, or an
    /// object with `include`, `exclude` and `sources`; paths are expanded
    /// (`ResourceUtils.expandPath`).
    pub fn from_setting(v: &serde_json::Value) -> Option<Self> {
        let strings = |v: Option<&serde_json::Value>| -> Option<Vec<String>> {
            match v {
                None => Some(Vec::new()),
                Some(serde_json::Value::Array(a)) => {
                    let mut out: Vec<String> = Vec::new();
                    for s in a
                        .iter()
                        .filter_map(|s| s.as_str())
                        .map(invisible::expand_path)
                    {
                        if !out.contains(&s) {
                            out.push(s);
                        }
                    }
                    Some(out)
                }
                Some(_) => None,
            }
        };
        match v {
            serde_json::Value::Object(m) => {
                let include = strings(m.get("include"))?;
                let exclude = strings(m.get("exclude"))?;
                let mut sources = BTreeMap::new();
                if let Some(s) = m.get("sources") {
                    for (k, v) in s.as_object()? {
                        sources.insert(
                            invisible::expand_path(k),
                            invisible::expand_path(v.as_str()?),
                        );
                    }
                }
                Some(Self {
                    include,
                    exclude,
                    sources,
                })
            }
            serde_json::Value::Array(_) => Some(Self {
                include: strings(Some(v))?,
                ..Default::default()
            }),
            _ => None,
        }
    }
}

impl ImportSettings {
    /// Import settings from a jdt.ls `settings` object (on top of the jdt.ls defaults).
    pub fn from_settings(settings: Option<&serde_json::Value>) -> Self {
        let mut s = Self::jdtls_defaults();
        let Some(c) = settings else { return s };
        s.gradle = gradle::config::GradleSettings::from_settings(c);
        if let Some(ex) = pref_list(c, "java.import.exclusions") {
            s.exclusions = ex;
        }
        s.resource_filters = s.resource_filters.updated_from_settings(c);
        if let Some(b) = pref_bool(c, "java.import.maven.enabled") {
            s.maven_enabled = b;
        }
        if let Some(b) = pref_bool(c, "java.import.gradle.enabled") {
            s.gradle_enabled = b;
        }
        if let Some(sp) = pref_list(c, "java.project.sourcePaths") {
            s.source_paths = Some(sp);
        }
        if let Some(op) = pref_string(c, "java.project.outputPath") {
            s.output_path = Some(op);
        }
        if let Some(libs) = pref_value(c, "java.project.referencedLibraries")
            .and_then(ReferencedLibraries::from_setting)
        {
            s.referenced_libraries = libs;
        }
        if let Some(b) = pref_bool(c, "java.import.maven.offline.enabled") {
            s.maven.offline = b;
        }
        if let Some(b) = pref_bool(c, "java.maven.downloadSources") {
            s.maven.download_sources = b;
        }
        if let Some(b) = pref_bool(c, "java.maven.updateSnapshots") {
            s.maven.update_snapshots = b;
        }
        if let Some(p) =
            pref_string(c, "java.configuration.maven.userSettings").filter(|p| !p.is_empty())
        {
            s.maven.user_settings = Some(PathBuf::from(invisible::expand_path(&p)));
        }
        if let Some(v) = pref_list(c, "java.compile.nullAnalysis.nonnull") {
            s.null_analysis.nonnull = v;
        }
        if let Some(v) = pref_list(c, "java.compile.nullAnalysis.nullable") {
            s.null_analysis.nullable = v;
        }
        if let Some(v) = pref_list(c, "java.compile.nullAnalysis.nonnullbydefault") {
            s.null_analysis.nonnullbydefault = v;
        }
        if let Some(v) = pref_string(c, "java.compile.nullAnalysis.mode") {
            if ["automatic", "interactive", "disabled"].contains(&v.as_str()) {
                s.null_analysis.mode = v;
            }
        }
        if let Some(p) =
            pref_string(c, "java.configuration.maven.globalSettings").filter(|p| !p.is_empty())
        {
            s.maven.global_settings = Some(PathBuf::from(invisible::expand_path(&p)));
        }
        s
    }
}

/// `IJavaProject.getOption(key, true)`: the project's own option, else the
/// jdt.ls workspace default (compliance keys follow the default VM).
pub fn effective_option(project: &Project, key: &str, vm_version: Option<&str>) -> Option<String> {
    if let Some(v) = project.options.get(key) {
        return Some(v.clone());
    }
    if let Some(vm) = vm_version {
        if key == COMPLIANCE || key == SOURCE || key == TARGET {
            return Some(vm.to_owned());
        }
    }
    jdt_defaults::WORKSPACE_DEFAULTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| (*v).to_owned())
}

/// The build supports in `StandardProjectsManager.buildSupports()` order;
/// the first that applies to a project manages it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildSupport {
    Gradle,
    Maven,
    Invisible,
    Default,
    Eclipse,
}

pub const BUILD_SUPPORTS: [BuildSupport; 5] = [
    BuildSupport::Gradle,
    BuildSupport::Maven,
    BuildSupport::Invisible,
    BuildSupport::Default,
    BuildSupport::Eclipse,
];

impl BuildSupport {
    /// `IBuildSupport.applies(project)`.
    pub fn applies(self, p: &Project) -> bool {
        match self {
            BuildSupport::Gradle => p.has_nature(GRADLE_NATURE),
            BuildSupport::Maven => p.has_nature(MAVEN_NATURE),
            BuildSupport::Invisible => p.kind == ProjectKind::Invisible,
            BuildSupport::Default => p.kind == ProjectKind::Default,
            BuildSupport::Eclipse => true,
        }
    }

    /// `IBuildSupport.buildToolName()`.
    pub fn build_tool_name(self) -> &'static str {
        match self {
            BuildSupport::Gradle => "Gradle",
            BuildSupport::Maven => "Maven",
            BuildSupport::Invisible => "INVISIBLE",
            BuildSupport::Default => "DEFAULT",
            BuildSupport::Eclipse => "ECLIPSE",
        }
    }

    /// `BuildSupportManager.find(project)`.
    pub fn of(p: &Project) -> BuildSupport {
        BUILD_SUPPORTS
            .into_iter()
            .find(|b| b.applies(p))
            .unwrap_or(BuildSupport::Eclipse)
    }
}

/// Compare two JDT version strings (`"1.8"` < `"11"`).
pub fn compare_java_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |v: &str| -> (u32, u32) {
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        match parts.as_slice() {
            [1, minor, ..] => (*minor, 0),
            [major, ..] => (*major, 0),
            _ => (0, 0),
        }
    };
    key(a).cmp(&key(b))
}

/// The execution environment of a JRE container path
/// (`.../StandardVMType/JavaSE-11` → `"11"`, `JavaSE-1.8` → `"1.8"`).
pub fn jre_container_ee(path: &str) -> Option<String> {
    let last = path.rsplit('/').next()?;
    let v = last.strip_prefix("JavaSE-")?;
    Some(v.to_owned())
}

/// JDT launching's JRE container validation for `project` with the default
/// VM at `vm` (a single installed JRE).
fn jre_markers(project: &Project, vm: &str) -> Vec<Marker> {
    let mut out = Vec::new();
    let Some(jre) = project.classpath.iter().find(|e| e.is_jre_container()) else {
        return out;
    };
    let release = effective_option(project, RELEASE, Some(vm)).unwrap_or_default();
    let compliance = effective_option(project, COMPLIANCE, Some(vm)).unwrap_or_default();
    if let Some(ee) = jre_container_ee(&jre.path) {
        if compare_java_versions(&ee, vm) == std::cmp::Ordering::Greater {
            out.push(Marker::project(
                format!("Unbound classpath container: 'JRE System Library [JavaSE-{ee}]' in project '{}'", project.name),
                1,
                "963",
            ));
            return out;
        }
        if ee != vm && release == "disabled" {
            out.push(Marker::project(
                format!("The compiler compliance specified is {compliance} but a JRE {vm} is used"),
                2,
                "0",
            ));
            out.push(Marker::project(
                format!("Build path specifies execution environment JavaSE-{ee}. There are no JREs installed in the workspace that are strictly compatible with this environment."),
                2,
                "0",
            ));
        }
    }
    if release == "enabled" && compare_java_versions(&compliance, vm) == std::cmp::Ordering::Greater
    {
        out.push(Marker::project(
            format!("The project was not built due to \"release {compliance} is not found in the system\". Fix the problem, then try refreshing this project and building it since it may be inconsistent"),
            1,
            "0",
        ));
    }
    out
}
