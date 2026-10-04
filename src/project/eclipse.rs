//! Eclipse project import (`.project` + `.classpath` + `.settings`).

use super::detect::FileDetector;
use super::{project_prefs, source_attachment, ImportSettings, Library, Project, ProjectKind, SourceFolder};
use std::path::{Path, PathBuf};

pub fn import(root: &Path, settings: &ImportSettings, claimed: &[PathBuf]) -> Vec<Project> {
    let mut detector = FileDetector::new(root, &[".project"])
        .add_exclusions(["**/bin"])
        .add_exclusions(&settings.exclusions);
    for c in claimed {
        detector = detector.add_exclusions([c.to_string_lossy().replace('\\', "\\\\")]);
    }
    detector
        .scan()
        .into_iter()
        .filter(|dir| !claimed.iter().any(|c| dir.starts_with(c)))
        .filter_map(|dir| load(&dir))
        .collect()
}

/// Load an Eclipse project from `dir` (must contain `.project`).
pub fn load(dir: &Path) -> Option<Project> {
    let dir = super::canonicalize_lenient(dir);
    let desc = std::fs::read_to_string(dir.join(".project")).ok()?;
    let doc = roxmltree::Document::parse(&desc).ok()?;
    let name = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("name"))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
    let is_java = doc
        .descendants()
        .any(|n| n.has_tag_name("nature") && n.text().is_some_and(|t| t.trim() == "org.eclipse.jdt.core.javanature"));

    let mut project = Project {
        name,
        root: dir.clone(),
        kind: ProjectKind::Eclipse,
        source_folders: Vec::new(),
        libraries: Vec::new(),
        project_deps: Vec::new(),
        options: project_prefs(&dir),
    };
    if is_java {
        if let Ok(cp) = std::fs::read_to_string(dir.join(".classpath")) {
            apply_classpath(&mut project, &cp);
        }
    }
    Some(project)
}

/// Apply `.classpath` entries to `project`.
pub fn apply_classpath(project: &mut Project, classpath_xml: &str) {
    let Ok(doc) = roxmltree::Document::parse(classpath_xml) else { return };
    for entry in doc.descendants().filter(|n| n.has_tag_name("classpathentry")) {
        let kind = entry.attribute("kind").unwrap_or("");
        let path = entry.attribute("path").unwrap_or("");
        let is_test = entry
            .descendants()
            .any(|a| a.has_tag_name("attribute") && a.attribute("name") == Some("test") && a.attribute("value") == Some("true"));
        match kind {
            "src" if path.starts_with('/') => {
                project.project_deps.push(path.trim_start_matches('/').to_owned());
            }
            "src" => project.source_folders.push(SourceFolder {
                path: project.root.join(path),
                is_test,
            }),
            "lib" => {
                let jar = resolve_lib_path(&project.root, path);
                let source = entry
                    .attribute("sourcepath")
                    .filter(|s| !s.is_empty())
                    .map(|s| resolve_lib_path(&project.root, s));
                project.libraries.push(Library { path: jar, source, is_test });
            }
            "var" => {
                if let Some(jar) = resolve_variable_path(path) {
                    let source = source_attachment(&jar);
                    project.libraries.push(Library { path: jar, source, is_test });
                }
            }
            _ => {}
        }
    }
}

fn resolve_lib_path(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    // Workspace-relative "/project/lib.jar" — resolve against the parent of
    // this project (sibling projects share a parent in fixtures).
    if let Some(rest) = path.strip_prefix('/') {
        if let Some(parent) = root.parent() {
            let candidate = parent.join(rest);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    root.join(path)
}

fn resolve_variable_path(path: &str) -> Option<PathBuf> {
    let (var, rest) = path.split_once('/')?;
    let base = match var {
        "M2_REPO" => dirs_home()?.join(".m2").join("repository"),
        _ => PathBuf::from(std::env::var(var).ok()?),
    };
    Some(base.join(rest))
}

pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
