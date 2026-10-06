//! `OrganizeImportsCommand`: select source compilation units, organize their
//! editor buffers and return WorkspaceEdit.changes. The server owns applyEdit.

pub(crate) mod operation;
mod scope;

use super::formatting::FormatEnv;
use crate::project::{uri_to_path, Project, Workspace};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use tower_lsp::lsp_types::{Url, WorkspaceEdit};

pub async fn command(env: &FormatEnv<'_>, arguments: &[Value]) -> anyhow::Result<WorkspaceEdit> {
    let mut changes = HashMap::new();
    if let Some(value) = arguments.first().and_then(Value::as_str) {
        let uri = Url::parse(value).map_err(|_| anyhow::anyhow!("URI is not found"))?;
        let units = compilation_units(env, &uri).await?;
        for uri in units {
            let options = env.jdt_options(Some(&uri)).await;
            match operation::organize(env.dispatcher, &uri, &options).await {
                Ok(Some(change)) => {
                    let edits = crate::correction::edit::tree_to_text_edits(
                        &change.ast.source,
                        change.edits.as_ref().expect("import edits"),
                    );
                    if !edits.is_empty() {
                        changes.insert(uri, edits);
                    }
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(%uri, %error, "Problem organize imports"),
            }
        }
    }
    Ok(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

async fn compilation_units(env: &FormatEnv<'_>, uri: &Url) -> anyhow::Result<BTreeSet<Url>> {
    // The upstream resource model accepts file URIs only. Editor-only working
    // copies extend that contract without requiring a file to exist on disk.
    if env.dispatcher.store.get(uri).is_some() {
        return Ok(BTreeSet::from([uri.clone()]));
    }
    let path = uri_to_path(uri).ok_or_else(|| anyhow::anyhow!("URI is not found"))?;
    let ws = env
        .dispatcher
        .workspace
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(project) = resource_project(&ws, &path) else {
        return Ok(BTreeSet::new());
    };
    if path.is_file() {
        return Ok((path.extension().is_some_and(|ext| ext == "java")
            && project.source_folder_for(&path).is_some())
        .then(|| uri.clone())
        .into_iter()
        .collect());
    }
    if !path.is_dir() || path != project.location {
        // IWorkspaceRoot.getFileForLocation returns an IFile handle for any
        // descendant path, even a folder. The pinned delegate consequently
        // calls organizeImportsInFile for directories and returns no changes.
        // Only a project location reaches its container/project branch.
        return Ok(BTreeSet::new());
    }
    let mut units: BTreeSet<_> = project
        .java_files()
        .into_iter()
        .filter_map(|path| Url::from_file_path(path).ok())
        .collect();
    // Include unsaved buffers in the selected project as well as disk CUs.
    let context = env.dispatcher.context_for(Some(uri)).await;
    units.extend(
        context
            .files
            .keys()
            .filter_map(|u| Url::parse(u).ok())
            .filter(|u| {
                uri_to_path(u).is_some_and(|p| {
                    resource_project(&ws, &p).is_some_and(|owner| owner.name == project.name)
                        && project.source_folder_for(&p).is_some()
                })
            }),
    );
    let units: Vec<_> = units
        .into_iter()
        .map(|uri| {
            let name = uri_to_path(&uri)
                .and_then(|path| {
                    project
                        .source_folder_for(&path)
                        .and_then(|root| path.parent().map(|parent| package(parent, &root.path)))
                })
                .unwrap_or_default();
            (uri, name)
        })
        .collect();
    Ok(scope::collect_compilation_units(&units, None)
        .into_iter()
        .collect())
}

fn package(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join(".")
}

fn resource_project<'a>(ws: &'a Workspace, path: &Path) -> Option<&'a Project> {
    ws.projects
        .iter()
        .filter(|project| {
            project.is_java()
                && (path.starts_with(&project.location) || path.starts_with(&project.root))
        })
        .max_by_key(|project| project.root.components().count())
}
