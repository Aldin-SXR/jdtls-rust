//! Port of `org.eclipse.jdt.ls.core.internal.handlers.NavigateToDeclarationHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

fn declaration(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Vec<Value> {
    let result = ws.request(
        "textDocument/declaration",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    assert!(!result.is_null(), "declaration must not return null");
    result.as_array().cloned().unwrap_or_default()
}

fn declaration_test(ws: &mut Workspace, class_name: &str, line: u32, column: u32) -> Vec<Value> {
    ws.import_projects(&["eclipse/declaration-test"]);
    let uri = ws.class_file_uri("declaration-test", class_name);
    declaration(ws, &uri, line, column)
}

#[test]
fn test_get_empty_declaration() {
    let mut ws = Workspace::new();
    let declarations = declaration(&mut ws, "/foo/bar", 1, 1);
    assert_eq!(0, declarations.len());
}

#[test]
fn test_get_method_declaration_same_file() {
    let mut ws = Workspace::new();
    let declarations = declaration_test(&mut ws, "TestSame", 1, 20);
    assert!(declarations[0]["uri"].is_string());
    assert_eq!(9, declarations[0]["range"]["start"]["line"]);
    assert_eq!(16, declarations[0]["range"]["start"]["character"]);
}

#[test]
fn test_get_method_declaration() {
    let mut ws = Workspace::new();
    let declarations = declaration_test(&mut ws, "Car", 4, 23);
    assert!(declarations[0]["uri"].is_string());
    assert_eq!(1, declarations[0]["range"]["start"]["line"]);
    assert_eq!(15, declarations[0]["range"]["start"]["character"]);
}

#[test]
fn test_get_field_declaration() {
    let mut ws = Workspace::new();
    let declarations = declaration_test(&mut ws, "Car", 3, 15);
    assert_eq!(0, declarations.len());
}

#[test]
fn test_custom_package() {
    let mut ws = Workspace::new();
    let declarations = declaration_test(&mut ws, "PackageTwo.Foo2", 9, 14);
    assert!(declarations[0]["uri"].is_string());
    assert_eq!(9, declarations[0]["range"]["start"]["line"]);
    assert_eq!(9, declarations[0]["range"]["start"]["character"]);
}
