//! Ports of `org.eclipse.jdt.ls.core.internal.handlers.CodeActionHandlerTest`.

mod common;
use common::jdtls::*;
use common::quickfix::{get_range, QuickFixTest};
use serde_json::{json, Value};

fn setup(source: &str, path: &str) -> (Workspace, String) {
    let mut ws = QuickFixTest::new().ws;
    ws.import_projects(&["eclipse/hello"]);
    let settings = ws.dir.join("settings.prefs");
    std::fs::write(&settings, "").unwrap();
    ws.settings["java"]["settings"]["url"] =
        json!(tower_lsp::lsp_types::Url::from_file_path(settings)
            .unwrap()
            .to_string());
    let uri = ws.path_uri(&format!("eclipse/hello/{path}"));
    ws.open_with(&uri, source);
    (ws, uri)
}

fn diagnostic(code: &str, range: &Value) -> Value {
    json!({"code":code,"range":range,"severity":1,"message":"Test Diagnostic","source":"Java"})
}

fn actions(
    ws: &mut Workspace,
    uri: &str,
    range: Value,
    diagnostics: Vec<Value>,
    only: Option<&str>,
) -> Vec<Value> {
    let mut context = json!({"diagnostics":diagnostics});
    if let Some(kind) = only {
        context["only"] = json!([kind]);
    }
    let result = ws.request(
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},"range":range,"context":context}),
    );
    result
        .as_array()
        .expect("non-null code action response")
        .clone()
}

fn quote_actions() {
    let source = "public class Foo {\n\tvoid foo() {\nString s = \"some str\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "some str");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("1610612995", &range)],
        None,
    );
    assert!(!result.is_empty());
    assert_eq!("quickfix", result[0]["kind"]);
    assert!(!result[0]["edit"].is_null());
}

#[test]
fn test_code_action_remove_unterminated_string() {
    quote_actions();
}

#[test]
fn test_code_action_literal_remove_unterminated_string() {
    quote_actions();
}

#[test]
fn test_code_action_superfluous_semicolon() {
    let source = "public class Foo {\n\tvoid foo() {\n;\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, ";");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536871092", &range)],
        None,
    );
    assert!(!result.is_empty());
    assert_eq!("quickfix", result[0]["kind"]);
    let edits = result[0]["edit"]["changes"][&uri].as_array().unwrap();
    assert_eq!(1, edits.len());
    assert_eq!("", edits[0]["newText"]);
    assert_eq!(range, edits[0]["range"]);
}

#[test]
fn test_code_action_organize_imports_source_action_only() {
    let source = "import java.util.List;\npublic class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536870973", &range)],
        Some("source.organizeImports"),
    );
    assert!(!result.is_empty(), "No organize imports actions were found");
    for action in result {
        assert!(action["kind"]
            .as_str()
            .unwrap()
            .starts_with("source.organizeImports"));
    }
}

#[test]
fn test_code_action_organize_imports_quick_fix() {
    let source = "import java.util.List;\npublic class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let result = actions(&mut ws, &uri, get_range(source, "util"), vec![], None);
    assert!(result
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Organize imports"));
    let result = actions(&mut ws, &uri, get_range(source, "String bar"), vec![], None);
    assert!(!result
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Organize imports"));
}

#[test]
fn test_code_action_refactor_actions_only() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536870973", &range)],
        Some("refactor"),
    );
    assert!(!result.is_empty(), "No refactor actions were found");
    for action in result {
        assert!(action["kind"].as_str().unwrap().starts_with("refactor"));
    }
}

#[test]
fn test_code_action_error_from_other_sources() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tInteger bar = 2000;\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let diag = json!({"code":"MagicNumberCheck","range":range,"severity":1,"message":"'2000' is a magic number.","source":"Checkstyle"});
    let result = actions(&mut ws, &uri, range, vec![diag], Some("refactor"));
    assert!(!result.is_empty(), "No refactor actions were found");
    for action in result {
        assert!(action["kind"].as_str().unwrap().starts_with("refactor"));
    }
}

#[test]
fn test_code_action_exception() {
    let mut ws = QuickFixTest::new().ws;
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.path_uri("eclipse/hello/nopackage/Test.java");
    ws.open(&uri);
    actions(&mut ws, &uri, range(0, 17, 0, 17), vec![], None);
}

#[test]
fn test_no_unnecessary_code_actions() {
    let source = "package org.sample;\n\npublic class Foo {\n\tprivate String foo;\n\tpublic String getFoo() {\n\t  return foo;\n\t}\n   \n\tpublic void setFoo(String newFoo) {\n\t  foo = newFoo;\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/org/sample/Foo.java");
    let result = actions(
        &mut ws,
        &uri,
        get_range(source, "String foo;"),
        vec![],
        None,
    );
    assert!(
        !result.iter().any(|a| a["kind"] == "source.organizeImports"),
        "No need for organize imports action"
    );
    assert!(
        !result
            .iter()
            .any(|a| a["kind"] == "source.generate.accessors"),
        "No need for generate getter and setter action"
    );
}

fn unused_import_actions(with_other_diagnostic: bool) {
    let source = "import java.sql.*; \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let selected = get_range(source, "java.sql");
    let mut diagnostics = vec![];
    if with_other_diagnostic {
        diagnostics.push(json!({"range":range(0,0,0,1),"message":"fake dignostic without code"}));
    }
    diagnostics.push(diagnostic("268435844", &selected));
    let result = actions(&mut ws, &uri, selected, diagnostics, None);
    assert!(result.len() >= 3, "{result:#?}");
    assert!(result.iter().any(|a| a["kind"] == "quickfix"));
    assert_eq!(
        1,
        result
            .iter()
            .filter(|a| a["kind"] == "source.organizeImports")
            .count()
    );
    assert!(!result[0]["edit"].is_null());
}

#[test]
fn test_code_action_remove_unused_import() {
    unused_import_actions(false);
}

#[test]
fn test_code_action_ignoring_other_diagnostic_without_code() {
    unused_import_actions(true);
}
