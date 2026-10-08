//! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceFolderChangeHandlerTest`.
//!
//! `workspaceFolderChangeHandler.update(params)` is the
//! `workspace/didChangeWorkspaceFolders` notification. The root paths of the
//! preferences decide which folders are imported, so `getRootPaths().contains`
//! is observed through the project the folder contributes to
//! `java.project.getAll`.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;

#[test]
fn test_update_workspace_folder() {
    let mut ws = Workspace::new();
    let root_folder = ws.copy_files("maven/salut");
    let root_folder_uri = dir_uri(&root_folder);
    // The server starts with another folder, so it hasn't seen this one yet.
    let other = ws.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    ws.set_roots(vec![other]);
    assert!(!ws.has_project_at(&root_folder, true));

    ws.client().notify(
        "workspace/didChangeWorkspaceFolders",
        json!({ "event": { "added": [{ "uri": root_folder_uri, "name": "test" }], "removed": [] } }),
    );
    ws.wait_for_background_jobs();
    assert!(ws.has_project_at(&root_folder, true));

    ws.client().notify(
        "workspace/didChangeWorkspaceFolders",
        json!({ "event": { "added": [], "removed": [{ "uri": root_folder_uri, "name": "test" }] } }),
    );
    ws.wait_for_background_jobs();
    assert!(!ws.has_project_at(&root_folder, true));
}
