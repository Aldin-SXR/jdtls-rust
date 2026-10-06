//! NonProjectFixTest's original source, action order, command titles and args.
//! The default-project working copy is opened as an external standalone file.
mod common;
use common::jdtls::*;
use common::quickfix::quickfix_client_capabilities;
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

const SOURCE: &str = "package java;\npublic class Foo extends UnknownType {\n\tpublic void method1(){\n\t\tsuper.whatever()\n\t}\n}";
fn actions(syntax_only: bool) -> (Workspace, Value) {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    ws.settings = json!({"java":{"edit":{"validateAllOpenBuffersOnChanges":false}}});
    let dir = ws.external_dir().join("java");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Foo.java");
    std::fs::write(&path, SOURCE).unwrap();
    let uri = Url::from_file_path(path).unwrap().to_string();
    ws.request("workspace/executeCommand",json!({"command":"java.project.refreshDiagnostics", "arguments":[uri,"anyNonProjectFile",syntax_only]}));
    ws.open_with(&uri, SOURCE);
    let reports = ws.published_diagnostics_min(1);
    let diagnostics = reports.iter().find(|r| r["uri"] == uri).unwrap()["diagnostics"]
        .as_array()
        .unwrap();
    assert!(!diagnostics.is_empty());
    let problem = &diagnostics[0];
    assert_eq!("16", problem["code"]);
    let start = problem["range"]["start"].clone();
    let result = ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},
        "range":{"start":start,"end":start}, "context":{"diagnostics":[problem],"only":["quickfix"]}}));
    (ws, result)
}
fn assert_actions(result: Value, titles: [&str; 2], syntax_only: bool) {
    let actions = result.as_array().unwrap();
    assert_eq!(2, actions.len(), "{result:#}");
    for (action, (title, scope)) in actions
        .iter()
        .zip(titles.into_iter().zip(["thisFile", "anyNonProjectFile"]))
    {
        assert_eq!("quickfix", action["kind"]);
        assert_eq!(title, action["command"]["title"]);
        let args = action["command"]["arguments"].as_array().unwrap();
        assert_eq!(3, args.len());
        assert_eq!(scope, args[1]);
        assert_eq!(syntax_only, args[2]);
    }
}
#[test]
fn test_report_all_errors_fix_for_non_project_file() {
    let (_ws, result) = actions(true);
    assert_actions(
        result,
        [
            "Report compilation errors for this file",
            "Report compilation errors for any non-project file in the current session",
        ],
        false,
    );
}
#[test]
fn test_report_syntax_errors_fix_for_non_project_file() {
    let (_ws, result) = actions(false);
    assert_actions(
        result,
        [
            "Only report syntax errors for this file",
            "Only report syntax errors for any non-project file in the current session",
        ],
        true,
    );
}
