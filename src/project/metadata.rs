//! `JLSFsUtils` and the project metadata files (`.project`, `.classpath`,
//! `.factorypath`, `.settings/*.prefs`) jdt.ls keeps for the projects it
//! creates: at the project root, or in the workspace's metadata area when
//! `java.import.generatesMetadataFilesAtProjectRoot` is false.

use super::resource_filters::{ResourceFilters, CREATED_BY_JAVA_LANGUAGE_SERVER};
use super::{classpath, prefs, EntryKind, Project, ProjectKind};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const GENERATES_METADATA_FILES_AT_PROJECT_ROOT: &str =
    "java.import.generatesMetadataFilesAtProjectRoot";

const SETTINGS: &str = ".settings";
const JDT_CORE_PREFS: &str = ".settings/org.eclipse.jdt.core.prefs";
const M2E_CORE_PREFS: &str = ".settings/org.eclipse.m2e.core.prefs";
const APT_CORE_PREFS: &str = ".settings/org.eclipse.jdt.apt.core.prefs";
const RESOURCES_PREFS: &str = ".settings/org.eclipse.core.resources.prefs";
const PROCESSOR_SERVICE: &str = "META-INF/services/javax.annotation.processing.Processor";

static PROPERTY: Mutex<Option<String>> = Mutex::new(None);
static AREA: Mutex<Option<PathBuf>> = Mutex::new(None);
static RESOURCE_PATTERNS: Mutex<Option<ResourceFilters>> = Mutex::new(None);

/// `System.setProperty` / `clearProperty` of `GENERATES_METADATA_FILES_AT_PROJECT_ROOT`.
pub fn set_property(value: Option<String>) {
    *PROPERTY.lock().unwrap_or_else(|e| e.into_inner()) = value;
}

/// `JLSFsUtils.generatesMetadataFilesAtProjectRoot`.
pub fn generates_metadata_files_at_project_root() -> bool {
    match &*PROPERTY.lock().unwrap_or_else(|e| e.into_inner()) {
        None => true,
        Some(value) => value.eq_ignore_ascii_case("true"),
    }
}

/// The folder holding the redirected files, one folder per project
/// (`JLSFsUtils.METADATA_FOLDER_PATH`).
pub fn set_metadata_area(workspace: &Path) {
    *AREA.lock().unwrap_or_else(|e| e.into_inner()) = Some(
        workspace
            .join(".metadata/.plugins/org.eclipse.core.resources/.projects"),
    );
}

/// `JDTLSFilesystemActivator.setResourcePatterns`.
pub fn set_resource_patterns(filters: Option<ResourceFilters>) {
    *RESOURCE_PATTERNS.lock().unwrap_or_else(|e| e.into_inner()) = filters;
}

/// `JLSFsUtils.isExcluded`.
pub fn is_excluded(path: &Path) -> bool {
    match &*RESOURCE_PATTERNS.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(filters) => filters.is_filtered(Path::new("/"), path),
        None => true,
    }
}

/// Where the metadata file `rel` (`.classpath`, `.settings/x.prefs`) of the
/// project at `location` lives: at the root when it (or, for preferences,
/// the `.settings` folder) already exists there, else in the metadata area
/// unless files are generated at the root.
pub fn resolve(location: &Path, name: &str, rel: &str) -> PathBuf {
    let at_root = location.join(rel);
    if generates_metadata_files_at_project_root() || at_root.exists() {
        return at_root;
    }
    if rel.starts_with(".settings/") && location.join(SETTINGS).exists() {
        return at_root;
    }
    match &*AREA.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(area) => area.join(name).join(rel),
        None => at_root,
    }
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    let parent = path.parent().expect("metadata file has a parent");
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content.as_bytes())?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn write_prefs(project: &Project, rel: &str, updates: BTreeMap<String, String>) -> io::Result<()> {
    let path = resolve(&project.location, &project.name, rel);
    let mut values = prefs::read_properties(&path).unwrap_or_default();
    values.extend(updates);
    values.insert("eclipse.preferences.version".into(), "1".into());
    let mut text = String::new();
    for (key, value) in &values {
        text.push_str(&format!("{key}={value}\n"));
    }
    write_if_changed(&path, &text)
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn project_description(project: &Project, filters: &ResourceFilters) -> String {
    let mut text = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription>\n");
    text.push_str(&format!("\t<name>{}</name>\n\t<comment></comment>\n\t<projects>\n\t</projects>\n", xml(&project.name)));
    text.push_str("\t<buildSpec>\n");
    let mut builders = vec!["org.eclipse.jdt.core.javabuilder"];
    if project.has_nature(super::MAVEN_NATURE) {
        builders.push("org.eclipse.m2e.core.maven2Builder");
    }
    for builder in builders {
        text.push_str(&format!(
            "\t\t<buildCommand>\n\t\t\t<name>{builder}</name>\n\t\t\t<arguments>\n\t\t\t</arguments>\n\t\t</buildCommand>\n"
        ));
    }
    text.push_str("\t</buildSpec>\n\t<natures>\n");
    for nature in &project.natures {
        text.push_str(&format!("\t\t<nature>{}</nature>\n", xml(nature)));
    }
    text.push_str("\t</natures>\n");
    if !filters.patterns().is_empty() {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let arguments = format!("{}|{CREATED_BY_JAVA_LANGUAGE_SERVER}", filters.patterns().join("|"));
        text.push_str(&format!(
            "\t<filteredResources>\n\t\t<filter>\n\t\t\t<id>{id}</id>\n\t\t\t<name></name>\n\t\t\t<type>30</type>\n\t\t\t<matcher>\n\t\t\t\t<id>org.eclipse.core.resources.regexFilterMatcher</id>\n\t\t\t\t<arguments>{}</arguments>\n\t\t\t</matcher>\n\t\t</filter>\n\t</filteredResources>\n",
            xml(&arguments)
        ));
    }
    text.push_str("</projectDescription>\n");
    text
}

fn is_annotation_processor_jar(jar: &Path) -> bool {
    super::jar::entry_names(jar).is_some_and(|names| names.iter().any(|n| n == PROCESSOR_SERVICE))
}

/// The jars m2e-apt puts on the factory path: the dependencies of the
/// classpath container that are not test-scoped.
fn factory_path_jars(project: &Project) -> Vec<PathBuf> {
    project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Container && e.path == super::MAVEN_CONTAINER)
        .flat_map(|c| &c.children)
        .filter(|e| e.kind == EntryKind::Library && !e.is_test())
        .filter_map(|e| e.location.clone())
        .collect()
}

fn factory_path(jars: &[PathBuf]) -> String {
    let repository = super::maven::local_repository();
    let mut text = String::from("<factorypath>\n");
    for jar in jars {
        let id = match jar.strip_prefix(&repository) {
            Ok(relative) => format!("M2_REPO/{}", relative.to_string_lossy().replace('\\', "/")),
            Err(_) => jar.to_string_lossy().into_owned(),
        };
        let kind = if jar.starts_with(&repository) { "VARJAR" } else { "EXTJAR" };
        text.push_str(&format!(
            "    <factorypathentry kind=\"{kind}\" id=\"{}\" enabled=\"true\" runInBatchMode=\"false\"/>\n",
            xml(&id)
        ));
    }
    text.push_str("</factorypath>\n");
    text
}

fn persist_maven(project: &Project, filters: &ResourceFilters) -> io::Result<()> {
    let name = &project.name;
    let location = &project.location;
    let description = resolve(location, name, ".project");
    if !description.exists() {
        write_if_changed(&description, &project_description(project, filters))?;
    }
    let source = project.classpath.iter().any(|e| e.kind == EntryKind::Source);
    if source {
        write_if_changed(
            &resolve(location, name, ".classpath"),
            &classpath::formatted_classpath(project),
        )?;
    }

    let mut compiler = BTreeMap::new();
    for key in [
        super::SOURCE,
        super::COMPLIANCE,
        super::TARGET,
        super::RELEASE,
        super::ENABLE_PREVIEW,
        super::REPORT_PREVIEW,
        "org.eclipse.jdt.core.compiler.problem.forbiddenReference",
        "org.eclipse.jdt.core.compiler.codegen.methodParameters",
        "org.eclipse.jdt.core.compiler.problem.missingSerialVersion",
    ] {
        if let Some(value) = project.options.get(key) {
            compiler.insert(key.to_owned(), value.clone());
        }
    }
    let processors = factory_path_jars(project).iter().any(|jar| is_annotation_processor_jar(jar));
    compiler.insert(
        "org.eclipse.jdt.core.compiler.processAnnotations".into(),
        if processors { "enabled" } else { "disabled" }.into(),
    );
    write_prefs(project, JDT_CORE_PREFS, compiler)?;

    let mut m2e = BTreeMap::new();
    m2e.insert("activeProfiles".to_owned(), project.selected_profiles.clone());
    m2e.insert("resolveWorkspaceProjects".to_owned(), "true".to_owned());
    m2e.insert("version".to_owned(), "1".to_owned());
    write_prefs(project, M2E_CORE_PREFS, m2e)?;

    let mut apt = BTreeMap::new();
    apt.insert("org.eclipse.jdt.apt.aptEnabled".to_owned(), processors.to_string());
    if processors {
        apt.insert(
            "org.eclipse.jdt.apt.genSrcDir".to_owned(),
            "target/generated-sources/annotations".to_owned(),
        );
        apt.insert(
            "org.eclipse.jdt.apt.genTestSrcDir".to_owned(),
            "target/generated-test-sources/test-annotations".to_owned(),
        );
        write_if_changed(
            &resolve(location, name, ".factorypath"),
            &factory_path(&factory_path_jars(project)),
        )?;
    }
    write_prefs(project, APT_CORE_PREFS, apt)?;

    if let Some(encoding) = &project.encoding {
        let mut resources = BTreeMap::new();
        resources.insert("encoding/<project>".to_owned(), encoding.clone());
        if let Some(source) = project.source_folders.iter().find(|f| !f.is_test) {
            if let Ok(relative) = source.path.strip_prefix(location) {
                resources.insert(
                    format!("encoding//{}", relative.to_string_lossy().replace('\\', "/")),
                    encoding.clone(),
                );
            }
        }
        write_prefs(project, RESOURCES_PREFS, resources)?;
    }
    Ok(())
}

fn persist_invisible(project: &Project) -> io::Result<()> {
    let name = &project.name;
    let location = &project.location;
    std::fs::create_dir_all(location)?;
    let mut compiler = BTreeMap::new();
    compiler.insert(
        super::ENABLE_PREVIEW.to_owned(),
        project
            .options
            .get(super::ENABLE_PREVIEW)
            .cloned()
            .unwrap_or_else(|| "disabled".to_owned()),
    );
    write_prefs(project, JDT_CORE_PREFS, compiler)?;
    let mut resources = BTreeMap::new();
    resources.insert("encoding/<project>".to_owned(), "UTF-8".to_owned());
    write_prefs(project, RESOURCES_PREFS, resources)?;
    classpath::persist_invisible_files(project, &resolve(location, name, ".project"), &resolve(location, name, ".classpath"))
}

/// Writes the metadata files of a project the importers created.
pub fn persist(project: &Project, filters: &ResourceFilters) {
    let result = match project.kind {
        ProjectKind::Maven if project.is_java() => persist_maven(project, filters),
        ProjectKind::Invisible => persist_invisible(project),
        _ => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!("Unable to write the metadata files of {}: {error}", project.name);
    }
}

#[cfg(test)]
mod jls_fs_utils_test {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    struct ClearProperty;
    impl Drop for ClearProperty {
        fn drop(&mut self) {
            set_property(None);
        }
    }

    fn guard() -> (std::sync::MutexGuard<'static, ()>, ClearProperty) {
        (SERIAL.lock().unwrap_or_else(|e| e.into_inner()), ClearProperty)
    }

    #[test]
    fn test_generates_metadata_files_at_project_root() {
        let _guard = guard();
        set_property(Some("true".into()));
        assert!(generates_metadata_files_at_project_root());
    }

    #[test]
    fn test_not_generates_metadata_files_at_project_root() {
        let _guard = guard();
        set_property(Some("false".into()));
        assert!(!generates_metadata_files_at_project_root());
    }

    #[test]
    fn test_generates_metadata_files_at_project_root_when_not_set() {
        let _guard = guard();
        assert!(generates_metadata_files_at_project_root());
    }

    #[test]
    fn test_excluded() {
        let _guard = guard();
        let path = Path::new("/project/node_modules");
        assert!(is_excluded(path));
    }
}
