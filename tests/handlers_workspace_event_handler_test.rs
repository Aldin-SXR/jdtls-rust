//! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceEventHandlerTest`.
//!
//! `new WorkspaceEventsHandler(..).handleFileEvents(events)` is a
//! `workspace/didChangeWatchedFiles` notification; `waitForBackgroundJobs`
//! is a workspace build round trip.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;
use tower_lsp::lsp_types::Url;

const CREATED: u32 = 1;
const CHANGED: u32 = 2;
const DELETED: u32 = 3;

fn handle_file_events(ws: &mut Workspace, events: &[(&str, u32)]) {
    let changes: Vec<Value> = events.iter().map(|(uri, typ)| json!({ "uri": uri, "type": typ })).collect();
    ws.client().notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }));
}

fn document_symbols(ws: &mut Workspace, uri: &str) -> usize {
    let result = ws.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
    result.as_array().map_or(0, Vec::len)
}

fn assert_ends_with(target: &str, suffix: &str) {
    let target = target.strip_suffix('/').unwrap_or(target);
    assert!(target.ends_with(suffix), "{target} does not end with {suffix}");
}

#[test]
#[ignore = "writes no class files: the server has no Java builder output folder (jdt.ls 1.58.0 does not rewrite Foo.class after the watched-file event over LSP either)"]
fn test_change_working_copy() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.wait_for_background_jobs();
    let project = ws.dir.join("eclipse/hello");
    let java_file = project.join("src/org/sample/Foo.java");
    let uri = Url::from_file_path(&java_file).unwrap().to_string();
    let source = std::fs::read_to_string(&java_file).unwrap();
    ws.open_with(&uri, &source);
    let class_file = project.join("bin/org/sample/Foo.class");
    let last_modified = std::fs::metadata(&class_file).and_then(|m| m.modified()).unwrap().duration_since(UNIX_EPOCH).unwrap();
    assert!(last_modified.as_millis() > 0);
    let source = source.replace("world", "world2");
    std::fs::write(&java_file, source).unwrap();

    handle_file_events(&mut ws, &[(&uri, CHANGED)]);
    ws.wait_for_background_jobs();

    let modified = std::fs::metadata(&class_file).and_then(|m| m.modified()).unwrap().duration_since(UNIX_EPOCH).unwrap();
    assert!(modified > last_modified);
}

#[test]
fn test_discard_stale_working_copies() {
    let mut ws = Workspace::new();
    let project = ws.new_empty_project(&test_default_options());
    let contents = "package mypack;\npublic class Foo {}\n";
    let unit = ws.create_cu(&project, "src", "mypack", "Foo.java", contents);
    ws.open_with(&unit, contents);
    assert!(document_symbols(&mut ws, &unit) > 0);

    let src = project.join("src");
    let old_uri = dir_uri(&src.join("mypack"));
    let parent_uri = dir_uri(&src);
    let new_uri = old_uri.replace("mypack", "mynewpack");
    std::fs::rename(src.join("mypack"), src.join("mynewpack")).unwrap();
    assert!(document_symbols(&mut ws, &unit) > 0);

    handle_file_events(&mut ws, &[(&new_uri, CREATED), (&parent_uri, CHANGED), (&old_uri, DELETED)]);
    ws.wait_for_background_jobs();

    assert_eq!(0, document_symbols(&mut ws, &unit));
}

#[test]
fn test_delete_project_folder() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/multimodule3"]);
    let module2 = ws.dir.join("maven/multimodule3/module2");
    assert!(ws.has_project_at(&module2, true));

    let project_uri = dir_uri(&module2);
    std::fs::remove_dir_all(&module2).unwrap();
    assert!(ws.has_project_at(&module2, true));

    ws.wait_for_background_jobs();
    ws.client().settle(std::time::Duration::from_secs(3), std::time::Duration::from_secs(30));
    ws.client().take_notifications("textDocument/publishDiagnostics");
    handle_file_events(&mut ws, &[(&project_uri, DELETED)]);
    ws.wait_for_background_jobs();
    ws.client().settle(std::time::Duration::from_secs(3), std::time::Duration::from_secs(30));
    assert!(!ws.has_project_at(&module2, true));

    let diags: Vec<Value> = ws.client().notifications.iter().filter(|n| n["method"] == "textDocument/publishDiagnostics").cloned().collect();
    assert_eq!(7, diags.len());
    let uri = |i: usize| diags[i]["params"]["uri"].as_str().unwrap().to_owned();
    let count = |i: usize| diags[i]["params"]["diagnostics"].as_array().unwrap().len();
    assert_ends_with(&uri(0), "/module2");
    assert_ends_with(&uri(1), "/multimodule3");
    assert_ends_with(&uri(2), "/multimodule3/pom.xml");
    assert_ends_with(&uri(3), "/module2/pom.xml");
    assert_eq!(0, count(3));
    assert_ends_with(&uri(4), "/module2");
    assert_eq!(0, count(4));
    assert_ends_with(&uri(5), "/App.java");
    assert_eq!(0, count(5));
    assert_ends_with(&uri(6), "/AppTest.java");
    assert_eq!(0, count(6));
}
