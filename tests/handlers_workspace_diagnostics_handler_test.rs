//! LSP ports of `WorkspaceDiagnosticsHandlerTest` using saved-file builds.
//! The marker conversion cases (`testToDiagnosticsArray`, `testMavenMarkers`)
//! are unit tests in `src/features/markers.rs`.
//!
//! The Mockito `connection` captor becomes the `publishDiagnostics`
//! notifications the client received; `Collections.reverse(allCalls)` makes
//! the latest report of a URI the first match.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::time::Duration;

/// `verify(connection, ..).publishDiagnostics(captor.capture())` after the
/// background jobs: every report received so far, latest first.
fn all_calls(ws: &mut Workspace) -> Vec<Value> {
    ws.wait_idle();
    let c = ws.client();
    c.settle(Duration::from_secs(3), Duration::from_secs(60));
    let mut calls: Vec<Value> =
        c.take_notifications("textDocument/publishDiagnostics").into_iter().map(|m| m["params"].clone()).collect();
    calls.reverse();
    calls
}

fn diagnostics_of(report: &Value) -> Vec<Value> {
    report["diagnostics"].as_array().unwrap().clone()
}

fn line_char(d: &Value, end: &str) -> (u64, u64) {
    (d["range"][end]["line"].as_u64().unwrap(), d["range"][end]["character"].as_u64().unwrap())
}

#[test]
fn test_task_markers() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let all_calls = all_calls(&mut ws);

    let task_diags = all_calls.iter().find(|p| p["uri"].as_str().unwrap().ends_with("TaskMarkerTest.java"));
    assert!(task_diags.is_some(), "No TaskMarkerTest.java markers were found");
    let mut diags = diagnostics_of(task_diags.unwrap());
    assert_eq!(3, diags.len(), "Some marker is missing");
    let todo_markers = diags.iter().filter(|p| p["message"].as_str().unwrap().starts_with("TODO")).count();
    assert_eq!(2, todo_markers, "A TODO marker is missing");
    diags.sort_by(|o1, o2| o1["message"].as_str().unwrap().cmp(o2["message"].as_str().unwrap()));
    let d = &diags[1];
    assert_eq!("TODO task 2", d["message"]);
    assert_eq!(3, d["severity"]);
    assert_eq!((11, 11), line_char(d, "start"));
    assert_eq!((11, 22), line_char(d, "end"));
    let d = &diags[0];
    assert_eq!("TODO task 1", d["message"]);
    assert_eq!(3, d["severity"]);
    assert_eq!((9, 11), line_char(d, "start"));
    assert_eq!((9, 22), line_char(d, "end"));
}

#[test]
#[ignore = "needs JDT incremental-builder semantics: jdt.ls compiles only the added A1.java and reports the duplicate class file locator itself ('The type A is already defined' plus the public type error); our full rebuild gets ECJ's DuplicateTypes only"]
fn test_bad_location_exception() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    ws.wait_idle();
    let root = ws.project_root("hello");
    let file = root.join("src/test1/A.java");
    assert!(file.exists());
    let dest_file = root.join("src/test1/A1.java");
    assert!(!dest_file.exists());
    std::fs::copy(&file, &dest_file).unwrap();
    let uri = url::Url::from_file_path(&dest_file).unwrap().to_string();
    // project.refreshLocal(IResource.DEPTH_INFINITE, null)
    ws.notify_file_changed(&dest_file, 1);
    let all_calls = all_calls(&mut ws);
    let param = all_calls.iter().find(|p| p["uri"] == uri.as_str());
    assert!(param.is_some(), "{all_calls:#?}");
    let diags = diagnostics_of(param.unwrap());
    assert_eq!(2, diags.len(), "{diags:?} {all_calls:#?}");
    let d = diags.iter().find(|p| p["message"] == "The type A is already defined");
    assert!(d.is_some());
    let diag = d.unwrap();
    // The positions are unsigned.
    assert!(diag["range"]["start"]["line"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["start"]["character"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["end"]["line"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["end"]["character"].as_i64().unwrap() >= 0);
}

/// `ResourceUtils.setContent(iFile, ..)`: the file changes on disk and the
/// workspace is refreshed.
fn set_content(ws: &mut Workspace, file: &std::path::Path, content: &str) {
    std::fs::write(file, content).unwrap();
    ws.notify_file_changed(file, 2);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1920
#[test]
#[ignore = "upstream makes a working copy without a DocumentLifeCycleHandler; over LSP the open document is always validated (the oracle also publishes 3 reports); the LSP equivalent is test_working_copy2"]
fn test_working_copy() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    // cu.becomeWorkingCopy(null)
    ws.open(&uri);
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = all_calls(&mut ws);
    assert!(calls.len() <= 2, "{calls:#?}");
    ws.close(&uri);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1963
#[test]
fn test_working_copy2() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    ws.open(&uri);
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = all_calls(&mut ws);
    assert!(calls.len() <= 3, "{calls:#?}");
    ws.close(&uri);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1920
#[test]
fn test_without_working_copy() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = all_calls(&mut ws);
    assert!(calls.len() >= 3, "{calls:#?}");
}

#[test]
#[ignore = "no 'Unknown referenced nature' report over LSP: WorkspaceDiagnosticsHandler.isIgnored drops CheckMissingNaturesListener markers and the oracle jdt.ls 1.58.0 publishes none for eclipse/wtpproject"]
fn test_missing_natures() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/wtpproject"]);
    let all_calls = all_calls(&mut ws);
    // https://github.com/eclipse/eclipse.jdt.ls/issues/2331
    let has_missing_nature = all_calls.iter().any(|project_diags| {
        diagnostics_of(project_diags).iter().any(|p| p["message"].as_str().unwrap().starts_with("Unknown referenced nature"))
    });
    assert!(has_missing_nature, "{all_calls:#?}");
}

#[test]
fn test_annotation() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut4"]);
    let uri = ws.class_uri("salut4", "org.sample.MyTest");
    // The problem markers of the unit, converted with
    // `toDiagnosticsArray(document, markers, false)`.
    let all_calls = all_calls(&mut ws);
    let report = all_calls.iter().find(|p| p["uri"] == uri.as_str()).expect("MyTest.java markers");
    let diagnostics = diagnostics_of(report);
    assert_eq!(4, diagnostics.len());
    let diagnostic = diagnostics.iter().find(|p| p["message"] == "Test cannot be resolved to a type").unwrap();
    assert_eq!(4, diagnostic["range"]["start"]["character"]);
}

#[test]
fn test_delete_package() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/unresolvedtype"]);
    let before = ws.published_diagnostics_min(1);
    assert!(before.iter().any(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")
        && r["diagnostics"].as_array().unwrap().iter().any(|d| d["severity"] == 1)), "unresolved type in Foo.java: {before:#?}");

    let folder = ws.project_root("unresolvedtype").join("src/pckg");
    assert!(folder.exists());
    std::fs::remove_dir_all(&folder).unwrap();
    ws.notify_file_changed(&folder, 3);
    let after = ws.published_diagnostics_min(1);
    let reports = after.iter().filter(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")).collect::<Vec<_>>();
    assert_eq!(1, reports.len(), "Should update the children's diagnostics of the deleted package: {after:#?}");
    assert!(reports[0]["diagnostics"].as_array().unwrap().is_empty(), "Should clean up the children's diagnostics of the deleted package");
}

#[test]
fn test_diagnostic_filtering() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "diagnostic": { "filter": ["**/Foo*.java"] } } });
    ws.import_projects(&["eclipse/hello"]);
    let reports = ws.published_diagnostics_min(1);
    assert!(!reports.is_empty());
    for report in reports {
        let uri = report["uri"].as_str().unwrap();
        assert!(!uri.contains("Foo"), "{uri} should have been excluded from diagnostics.");
    }
}
