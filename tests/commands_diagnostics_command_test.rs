//! Port of `org.eclipse.jdt.ls.core.internal.commands.DiagnosticsCommandTest`.
//! Default-project units are represented by files outside the workspace roots.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

const SOURCE: &str = "package java;\npublic class Foo extends UnknownType {\n\tpublic void method1(){\n\t\tsuper.whatever()\n\t}\n}";
const SYNTAX_MESSAGE: &str = "Foo.java is a non-project file, only syntax errors are reported";
const FULL_MESSAGE: &str = "Foo.java is a non-project file, only JDK classes are added to its build path";
const MISSING_SEMICOLON: &str = "Syntax error, insert \";\" to complete BlockStatements";

fn setup(syntax_only: bool) -> (Workspace, String) {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "edit": { "validateAllOpenBuffersOnChanges": false } } });
    let dir = ws.external_dir().join("java");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Foo.java");
    std::fs::write(&file, SOURCE).unwrap();
    let uri = Url::from_file_path(file).unwrap().to_string();
    ws.published_diagnostics();
    if !syntax_only {
        // The Java test sets DiagnosticsState.globalErrorLevel directly.
        // The public command provides the same state before the file opens.
        refresh(&mut ws, &uri, "anyNonProjectFile", false);
        assert!(ws.published_diagnostics().is_empty());
    }
    ws.open_with(&uri, SOURCE);
    (ws, uri)
}

fn refresh(ws: &mut Workspace, uri: &str, scope: &str, syntax_only: bool) {
    let result = ws.request("workspace/executeCommand", json!({
        "command": "java.project.refreshDiagnostics",
        "arguments": [uri, scope, syntax_only],
    }));
    assert!(result.is_null());
}

fn report(ws: &mut Workspace, uri: &str) -> Vec<Value> {
    let reports = ws.published_diagnostics_min(1);
    assert_eq!(1, reports.len(), "{reports:#?}");
    assert_eq!(uri, reports[0]["uri"]);
    reports[0]["diagnostics"].as_array().unwrap().clone()
}

fn assert_syntax(diagnostics: &[Value]) {
    assert_eq!(2, diagnostics.len(), "{diagnostics:#?}");
    assert_eq!(SYNTAX_MESSAGE, diagnostics[0]["message"]);
    assert_eq!(MISSING_SEMICOLON, diagnostics[1]["message"]);
}

fn assert_full(diagnostics: &[Value]) {
    assert_eq!(4, diagnostics.len(), "{diagnostics:#?}");
    assert_eq!(FULL_MESSAGE, diagnostics[0]["message"]);
    assert_eq!("UnknownType cannot be resolved to a type", diagnostics[1]["message"]);
    assert_eq!("UnknownType cannot be resolved to a type", diagnostics[2]["message"]);
    assert_eq!(MISSING_SEMICOLON, diagnostics[3]["message"]);
}

#[test]
fn test_refresh_diagnostics_with_report_all_errors() {
    let (mut ws, uri) = setup(true);
    assert_syntax(&report(&mut ws, &uri));
    refresh(&mut ws, &uri, "thisFile", false);
    assert_full(&report(&mut ws, &uri));
}

#[test]
fn test_refresh_diagnostics_with_report_syntax_errors() {
    let (mut ws, uri) = setup(false);
    assert_full(&report(&mut ws, &uri));
    refresh(&mut ws, &uri, "thisFile", true);
    assert_syntax(&report(&mut ws, &uri));
}
