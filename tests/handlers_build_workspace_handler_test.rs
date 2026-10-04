//! Port of `org.eclipse.jdt.ls.core.internal.handlers.BuildWorkspaceHandlerTest`
//! over `java/buildWorkspace` and `java/buildProjects`, whose result is the
//! `BuildWorkspaceStatus` ordinal.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};

/// `BuildWorkspaceStatus`.
const SUCCEED: i64 = 1;
const WITH_ERROR: i64 = 2;

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut2"]);
    ws
}

fn build_workspace(ws: &mut Workspace, force_rebuild: bool) -> Value {
    ws.request("java/buildWorkspace", json!(force_rebuild))
}

fn all_projects(ws: &mut Workspace) -> Vec<String> {
    let all = ws.request("workspace/executeCommand", json!({ "command": "java.project.getAll", "arguments": [] }));
    all.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()
}

#[test]
fn test_succeed_case() {
    let mut ws = setup();
    let result = build_workspace(&mut ws, false);
    assert_eq!(json!(SUCCEED), result, "BuildWorkspaceStatus is: {result}.");
}

#[test]
fn test_failed_case() {
    let mut ws = setup();
    ws.client();
    let code_with_error = "package foo;\n\tpublic class Single2 {\n\tpublic static void main(String[] args){\n\t\tint ss = 1;;\n\t}\n}";
    let file = ws.project_root("salut2").join("src/main/java/foo/Bar.java");
    std::fs::write(&file, code_with_error).unwrap();
    ws.notify_file_changed(&file, 2);
    let result = build_workspace(&mut ws, false);
    assert_eq!(json!(WITH_ERROR), result);
}


#[test]
fn test_parallel_build_for_eclipse_projects() {
    let mut ws = setup();
    ws.settings = json!({ "java": { "maxConcurrentBuilds": 4 } });
    ws.import_projects(&["eclipse/multi"]);
    ws.client();
    let projects: Vec<String> = all_projects(&mut ws).into_iter().filter(|p| p.contains("/eclipse/multi/")).collect();
    assert_eq!(2, projects.len(), "{projects:?}");

    let result = build_workspace(&mut ws, false);
    assert_eq!(json!(SUCCEED), result, "BuildWorkspaceStatus is: {result}.");
}

#[test]
fn test_parallel_build_support() {
    let mut ws = setup();
    ws.settings = json!({ "java": { "maxConcurrentBuilds": 4 } });
    ws.import_projects(&["maven/multimodule"]);
    ws.client();
    // `importProjects` returns the 5 Eclipse projects; `java.project.getAll`
    // lists the 3 with the Java nature (the parent poms aren't).
    let projects: Vec<String> = all_projects(&mut ws).into_iter().filter(|p| p.contains("/maven/multimodule/")).collect();
    assert_eq!(3, projects.len(), "{projects:?}");

    let result = build_workspace(&mut ws, false);
    assert_eq!(json!(SUCCEED), result, "BuildWorkspaceStatus is: {result}.");
}

#[test]
fn test_build_projects() {
    let mut ws = setup();
    ws.import_projects(&["maven/multimodule"]);
    ws.client();
    // The 5 imported projects' `IProject.getLocationURI()` (no trailing slash).
    let identifiers: Vec<Value> = ["", "/module1", "/module1/childmodule", "/module2", "/module3"]
        .iter()
        .map(|m| json!({ "uri": format!("file:{}/maven/multimodule{m}", ws.dir.display()) }))
        .collect();
    assert_eq!(5, identifiers.len());
    let result = ws.request("java/buildProjects", json!({ "identifiers": identifiers, "isFullBuild": true }));
    assert_eq!(json!(SUCCEED), result, "BuildWorkspaceStatus is: {result}.");
}

// Upstream cases not ported here (no empty tests counted as ports):
// test_canceled_case: needs a pre-cancelled IProgressMonitor; over LSP a $/cancelRequest races the build and yields a RequestCancelled error instead of CANCELLED.
