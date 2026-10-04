//! Eclipse project import (`EclipseProjectImporter`): `.project` +
//! `.classpath` + `.settings`, read the way the Eclipse resource and Java
//! models read them.

use super::detect::FileDetector;
use super::{
    project_prefs, source_attachment, ClasspathEntry, EntryKind, ImportSettings, Marker, Project,
    ProjectKind, Workspace, JAVA_NATURE,
};
use std::path::{Path, PathBuf};

/// `IProjectDescription.DESCRIPTION_FILE_NAME`.
pub const DESCRIPTION_FILE: &str = ".project";
/// `IJavaProject.CLASSPATH_FILE_NAME`.
pub const CLASSPATH_FILE: &str = ".classpath";

/// `EclipseProjectImporter.applies` + `importToWorkspace` for `root`.
/// `configs`: `projectConfigurations` under `root` (`applies(buildFiles)`).
pub fn import(
    root: &Path,
    settings: &ImportSettings,
    ws: &Workspace,
    configs: Option<&[PathBuf]>,
) -> Vec<Project> {
    let dirs: Vec<PathBuf> = match configs {
        Some(files) => {
            let imported: Vec<&Path> = ws.projects.iter().map(|p| p.location.as_path()).collect();
            files
                .iter()
                .filter(|f| f.file_name().is_some_and(|n| n == DESCRIPTION_FILE))
                .filter_map(|f| f.parent().map(Path::to_path_buf))
                .filter(|d| !imported.contains(&d.as_path()))
                .collect()
        }
        None => {
            let mut detector = FileDetector::new(root, &[DESCRIPTION_FILE])
                .add_exclusions(["**/bin"])
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
    let mut out: Vec<Project> = Vec::new();
    for dir in dirs.into_iter().filter(|d| d.join(CLASSPATH_FILE).exists()) {
        let Some(desc) = read_description(&dir) else {
            continue;
        };
        if !desc.natures.iter().any(|n| n == JAVA_NATURE) {
            continue;
        }
        let dir = super::canonicalize_lenient(&dir);
        // `findUniqueProject` when a project of that name lives elsewhere.
        let taken = |n: &str| {
            ws.projects
                .iter()
                .chain(out.iter())
                .any(|p| p.name == n && p.location != dir)
        };
        let name = find_unique_project(&desc.name, taken);
        if desc.has_missing_filter {
            // `project.open()` fails on a resource filter whose matcher is not
            // registered (jdt.ls has no `org.eclipse.ui.ide` matchers): the
            // project is created but stays closed.
            let mut closed = Project::new(&name, &dir, ProjectKind::Eclipse);
            closed.build_files = vec![dir.join(DESCRIPTION_FILE)];
            out.push(closed);
            continue;
        }
        out.push(load_with(&dir, &name, desc.natures));
    }
    out
}

/// `EclipseProjectImporter.findUniqueProject`: `name`, `name (2)`, `name (3)`…
pub fn find_unique_project(basename: &str, exists: impl Fn(&str) -> bool) -> String {
    let mut i = 1;
    loop {
        let name = if i < 2 {
            basename.to_owned()
        } else {
            format!("{basename} ({i})")
        };
        if !exists(&name) {
            return name;
        }
        i += 1;
    }
}

pub struct Description {
    pub name: String,
    pub natures: Vec<String>,
    /// A resource filter uses a matcher that is not registered.
    pub has_missing_filter: bool,
}

/// Resource filter matchers available in jdt.ls (`org.eclipse.core.resources`).
const KNOWN_FILTER_MATCHERS: &[&str] = &["org.eclipse.core.resources.regexFilterMatcher"];

/// `IWorkspace.loadProjectDescription(<dir>/.project)`.
pub fn read_description(dir: &Path) -> Option<Description> {
    let text = std::fs::read_to_string(dir.join(DESCRIPTION_FILE)).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let root = doc.root_element();
    let name = root
        .children()
        .find(|n| n.has_tag_name("name"))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            dir.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let natures = root
        .children()
        .filter(|n| n.has_tag_name("natures"))
        .flat_map(|n| n.children().filter(|c| c.has_tag_name("nature")))
        .filter_map(|n| n.text().map(|t| t.trim().to_owned()))
        .filter(|t| !t.is_empty())
        .collect();
    let has_missing_filter = root
        .children()
        .filter(|n| n.has_tag_name("filteredResources"))
        .flat_map(|n| n.descendants().filter(|m| m.has_tag_name("matcher")))
        .filter_map(|m| {
            m.children()
                .find(|c| c.has_tag_name("id"))
                .and_then(|c| c.text())
        })
        .any(|id| !KNOWN_FILTER_MATCHERS.contains(&id.trim()));
    Some(Description {
        name,
        natures,
        has_missing_filter,
    })
}

/// Load an Eclipse project from `dir` (must contain `.project`).
pub fn load(dir: &Path) -> Option<Project> {
    let dir = super::canonicalize_lenient(dir);
    let desc = read_description(&dir)?;
    let name = desc.name.clone();
    Some(load_with(&dir, &name, desc.natures))
}

fn load_with(dir: &Path, name: &str, natures: Vec<String>) -> Project {
    let mut project = Project::new(name, dir, ProjectKind::Eclipse);
    project.natures = natures;
    project.options = project_prefs(dir);
    project.build_files = vec![dir.join(DESCRIPTION_FILE), dir.join(CLASSPATH_FILE)];
    if project.is_java() {
        match std::fs::read_to_string(dir.join(CLASSPATH_FILE)) {
            Ok(cp) => apply_classpath(&mut project, &cp),
            Err(_) => {
                // JavaProject.defaultClasspath(): the project folder is the source folder.
                let mut src = ClasspathEntry::new(EntryKind::Source, format!("/{name}"));
                src.location = Some(dir.to_path_buf());
                project.classpath.push(src);
                project.classpath.push(ClasspathEntry::new(
                    EntryKind::Container,
                    super::JRE_CONTAINER,
                ));
                project.output = Some(dir.join("bin"));
            }
        }
    }
    project
}

fn attributes_of(entry: roxmltree::Node) -> Vec<(String, String)> {
    entry
        .children()
        .filter(|n| n.has_tag_name("attributes"))
        .flat_map(|n| n.children().filter(|a| a.has_tag_name("attribute")))
        .filter_map(|a| {
            Some((
                a.attribute("name")?.to_owned(),
                a.attribute("value").unwrap_or("").to_owned(),
            ))
        })
        .collect()
}

fn patterns(attr: Option<&str>) -> Vec<String> {
    attr.map(|s| {
        s.split('|')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

/// Apply `.classpath` entries to `project` (`JavaProject.decodeClasspath`).
pub fn apply_classpath(project: &mut Project, classpath_xml: &str) {
    let Ok(doc) = roxmltree::Document::parse(classpath_xml) else {
        return;
    };
    let root = project.location.clone();
    let name = project.name.clone();
    for node in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("classpathentry"))
    {
        let kind = node.attribute("kind").unwrap_or("");
        let path = node.attribute("path").unwrap_or("");
        let attributes = attributes_of(node);
        let output = node
            .attribute("output")
            .map(|o| resolve_project_path(&root, &name, o));
        let exported = node.attribute("exported") == Some("true");
        match kind {
            "src" if path.starts_with('/') => {
                let mut e = ClasspathEntry::new(EntryKind::Project, path);
                e.attributes = attributes;
                e.exported = exported;
                project.classpath.push(e);
            }
            "src" => {
                let full = if path.is_empty() || path == "." {
                    format!("/{name}")
                } else {
                    format!("/{name}/{}", path.trim_end_matches('/'))
                };
                let mut e = ClasspathEntry::new(EntryKind::Source, full);
                e.location = Some(if path.is_empty() || path == "." {
                    root.clone()
                } else {
                    root.join(path)
                });
                e.output = output;
                e.attributes = attributes;
                e.inclusions = patterns(node.attribute("including"));
                e.exclusions = patterns(node.attribute("excluding"));
                project.classpath.push(e);
            }
            "lib" => {
                let (eclipse_path, location) = library_path(&root, &name, path);
                let mut e = ClasspathEntry::new(EntryKind::Library, eclipse_path);
                e.source_attachment = node
                    .attribute("sourcepath")
                    .filter(|s| !s.is_empty())
                    .map(|s| library_path(&root, &name, s).1);
                e.location = Some(location);
                e.attributes = attributes;
                e.exported = exported;
                project.classpath.push(e);
            }
            "var" => {
                let mut e = ClasspathEntry::new(EntryKind::Variable, path);
                e.location = resolve_variable_path(path);
                e.source_attachment = e.location.as_deref().and_then(source_attachment);
                e.attributes = attributes;
                e.exported = exported;
                project.classpath.push(e);
            }
            "con" => {
                let mut e = ClasspathEntry::new(EntryKind::Container, path);
                e.attributes = attributes;
                e.exported = exported;
                project.classpath.push(e);
            }
            "output" => project.output = Some(resolve_project_path(&root, &name, path)),
            _ => {}
        }
    }
    if project.output.is_none() {
        project.output = Some(root.join("bin"));
    }
}

/// A project-relative path (or `/project/...` workspace path) as a location.
fn resolve_project_path(root: &Path, name: &str, path: &str) -> PathBuf {
    match path.strip_prefix(&format!("/{name}")) {
        Some(rest) => root.join(rest.trim_start_matches('/')),
        None if path.starts_with('/') => root
            .parent()
            .map(|p| p.join(path.trim_start_matches('/')))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => root.join(path),
    }
}

/// `(Eclipse path, location)` of a library entry path from `.classpath`.
fn library_path(root: &Path, name: &str, path: &str) -> (String, PathBuf) {
    let p = Path::new(path);
    if p.is_absolute() {
        // Absolute: an external file if it exists, else a workspace path.
        if p.exists() {
            return (path.to_owned(), p.to_path_buf());
        }
        if let Some(rest) = path.strip_prefix(&format!("/{name}/")) {
            return (path.to_owned(), root.join(rest));
        }
        // Workspace-relative "/project/lib.jar": sibling projects share a parent in fixtures.
        if let Some(parent) = root.parent() {
            let candidate = parent.join(path.trim_start_matches('/'));
            if candidate.exists() {
                return (path.to_owned(), candidate);
            }
        }
        return (path.to_owned(), p.to_path_buf());
    }
    (format!("/{name}/{path}"), root.join(path))
}

fn resolve_variable_path(path: &str) -> Option<PathBuf> {
    let (var, rest) = path.split_once('/')?;
    let base = match var {
        "M2_REPO" => super::maven::local_repository(),
        "JRE_LIB" | "JRE_SRC" | "JRE_SRCROOT" => return None,
        _ => PathBuf::from(std::env::var(var).ok()?),
    };
    Some(base.join(rest))
}

/// `ClasspathEntry.validateClasspath` problems reported as build path
/// markers on the project.
pub fn validate_classpath(project: &mut Project) {
    let problems = classpath_problems(project);
    project.markers.extend(problems);
}

/// The build path problems of `project`'s raw classpath: missing libraries
/// and required (non-optional) source folders.
pub fn classpath_problems(project: &Project) -> Vec<Marker> {
    let mut out = Vec::new();
    let name = project.name.clone();
    let mut errors = Vec::new();
    for e in &project.classpath {
        match e.kind {
            EntryKind::Library | EntryKind::Variable => {
                if e.location.as_deref().is_some_and(|l| !l.exists()) || e.location.is_none() {
                    let shown = e
                        .path
                        .strip_prefix(&format!("/{name}/"))
                        .unwrap_or(&e.path)
                        .to_owned();
                    errors.push(format!(
                        "Project '{name}' is missing required library: '{shown}'"
                    ));
                }
            }
            EntryKind::Source => {
                if e.location.as_deref().is_some_and(|l| !l.exists())
                    && e.attribute("optional") != Some("true")
                {
                    let shown = e
                        .path
                        .strip_prefix(&format!("/{name}/"))
                        .unwrap_or(&e.path)
                        .to_owned();
                    errors.push(format!(
                        "Project '{name}' is missing required source folder: '{shown}'"
                    ));
                }
            }
            _ => {}
        }
    }
    for msg in errors {
        out.push(Marker::project(msg, 1, "964"));
    }
    out
}

pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
