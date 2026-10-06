//! `BuildPathCommand` and `ProjectUtils.addSourcePath` / `removeSourcePath`.
//! Build-tool projects own their source paths; general projects can edit them.

use crate::project::{
    self, invisible, ClasspathEntry, EntryKind, ImportSettings, ProjectKind, Workspace,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub struct Change {
    pub result: Value,
    pub changed: bool,
}

fn result(status: bool, message: String) -> Change {
    Change {
        result: json!({"status":status,"message":message}),
        changed: false,
    }
}

fn display(folder: &Path, roots: &[PathBuf]) -> String {
    roots
        .iter()
        .find(|r| folder.starts_with(r))
        .and_then(|r| r.parent())
        .and_then(|base| folder.strip_prefix(base).ok())
        .unwrap_or(folder)
        .to_string_lossy()
        .into_owned()
}

fn not_present(folder: &str) -> Change {
    result(true, format!("No need to remove it from source path, because the folder '{folder}' isn't on any project's source path."))
}

pub fn change(
    ws: &mut Workspace,
    settings: &ImportSettings,
    roots: &[PathBuf],
    uri: &str,
    add: bool,
) -> Change {
    let Some(folder) = Url::parse(uri).ok().and_then(|u| project::uri_to_path(&u)) else {
        return result(false, "Invalid source folder URI.".to_owned());
    };
    let display = display(&folder, roots);
    // JDT chooses the deepest physical project location, not the linked root.
    let mut projects = ws.all_projects();
    projects.sort_by(|a, b| {
        b.location
            .as_os_str()
            .len()
            .cmp(&a.location.as_os_str().len())
    });
    let found = projects
        .into_iter()
        .find(|p| p.is_java() && folder.starts_with(&p.location));
    let mut exclusions = Vec::new();
    let mut target = if let Some(project) = found {
        project
    } else {
        let Some(root) = roots.iter().find(|root| folder.starts_with(root)) else {
            return result(
                false,
                format!("The folder '{display}' doesn't belong to any workspace."),
            );
        };
        let name = invisible::project_name(root);
        let target = match ws.project(&name) {
            Some(p) => p.clone(),
            None if !add => return not_present(&display),
            None => invisible::new_project(root, settings),
        };
        exclusions = ws
            .visible_projects_under(root)
            .iter()
            .map(|p| target.full_path(&p.location))
            .collect();
        target
    };
    let unsupported = if target.has_nature(project::GRADLE_NATURE) {
        Some("Unsupported operation. Please use build.gradle file to manage the source directories of gradle project.")
    } else if target.has_nature(project::MAVEN_NATURE) {
        Some("Unsupported operation. Please use pom.xml file to manage the source directories of maven project.")
    } else {
        None
    };
    if let Some(message) = unsupported {
        return result(false, message.to_owned());
    }
    let source = target.full_path(&folder);
    let exists = target
        .classpath
        .iter()
        .any(|e| e.kind == EntryKind::Source && e.path == source);
    if add && exists {
        return result(true, format!("No need to add it to source path again, because the folder '{display}' is already in the project {}'s source path.", target.name));
    }
    if !add && !exists {
        return not_present(&display);
    }
    if add {
        let mut entry = ClasspathEntry::new(EntryKind::Source, &source);
        entry.location = Some(folder);
        for old in target
            .classpath
            .iter()
            .filter(|e| e.kind == EntryKind::Source)
        {
            if Path::new(&source).starts_with(&old.path) {
                return result(false, format!("Cannot add the folder '{source}' to the source path because its parent folder is already in the source path of the project '{}'.", target.name));
            }
            if let Ok(child) = Path::new(&old.path).strip_prefix(&source) {
                entry
                    .exclusions
                    .push(format!("{}/", child.to_string_lossy()));
            }
        }
        for excluded in &exclusions {
            if let Ok(child) = Path::new(excluded).strip_prefix(&source) {
                if !child.as_os_str().is_empty() {
                    entry
                        .exclusions
                        .push(format!("{}/", child.to_string_lossy()));
                }
            }
        }
        target.classpath.push(entry);
    } else {
        target
            .classpath
            .retain(|e| e.kind != EntryKind::Source || e.path != source);
        for entry in target
            .classpath
            .iter_mut()
            .filter(|e| e.kind == EntryKind::Source)
        {
            entry
                .inclusions
                .retain(|p| Path::new(&entry.path).join(p) != Path::new(&source));
            entry
                .exclusions
                .retain(|p| Path::new(&entry.path).join(p) != Path::new(&source));
        }
    }
    if let Err(error) = project::classpath::persist_sources(&target) {
        return result(false, error.to_string());
    }
    let verb = if add { "added" } else { "removed" };
    let mut change = result(
        true,
        format!(
            "Successfully {verb} '{display}' {} the project {}'s source path.",
            if add { "to" } else { "from" },
            target.name
        ),
    );
    if target.kind == ProjectKind::Invisible {
        let prefix = format!("/{}/{}", target.name, project::WORKSPACE_LINK);
        let sources: Vec<_> = target
            .classpath
            .iter()
            .filter(|e| e.kind == EntryKind::Source)
            .map(|e| {
                e.path
                    .strip_prefix(&prefix)
                    .unwrap_or(&e.path)
                    .trim_start_matches('/')
                    .to_owned()
            })
            .collect();
        change.result["sourcePaths"] = json!(sources);
    }
    if target.kind != ProjectKind::Default {
        ws.add(target);
    }
    ws.finish();
    change.changed = true;
    change
}
