//! Port of `org.eclipse.jdt.ls.core.internal.handlers.NavigateToTypeDefinitionHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    ws
}

fn type_definition(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Value {
    ws.request(
        "textDocument/typeDefinition",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    )
}

fn test_class(ws: &mut Workspace, class_name: &str, line: u32, column: u32) {
    let uri = ws.class_file_uri("salut", class_name);
    let definitions = type_definition(ws, &uri, line, column);
    assert!(!definitions.is_null());
    let definitions = definitions.as_array().unwrap();
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert!(definitions[0]["range"]["start"]["line"].as_i64().unwrap() >= 0);
}

#[test]
fn test_get_empty_definition() {
    let mut ws = setup();
    let definitions = type_definition(&mut ws, "/foo/bar", 1, 1);
    assert!(definitions.is_null());
}

#[test]
fn test_attached_source() {
    let mut ws = setup();
    test_class(&mut ws, "org.apache.commons.lang3.StringUtils", 20, 26);
}

#[test]
fn test_local_variable() {
    let mut ws = setup();
    test_class(&mut ws, "java.Foo3", 18, 24);
}

#[test]
#[ignore = "expects the disassembled stub of rtstubs.jar's javax.tools.Tool (fake JDK without sources); the running JDK's javax.tools.Tool has attached source (lib/src.zip) with a different layout"]
fn test_disassembled_source() {
    let mut ws = setup();
    let class_name = "javax.tools.Tool";
    let uri = ws.class_file_uri("salut", class_name);
    let definitions = type_definition(&mut ws, &uri, 11, 12);
    let definitions = definitions.as_array().unwrap();
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert_eq!(3, definitions[0]["range"]["start"]["line"]);
    assert_eq!(12, definitions[0]["range"]["start"]["character"]);
}

#[test]
fn test_class_field() {
    let mut ws = setup();
    test_class(&mut ws, "java.Foo3", 17, 30);
}

#[test]
fn test_external_class_field() {
    let mut ws = setup();
    test_class(&mut ws, "java.Foo3", 17, 11);
}

#[test]
fn test_no_class_content_support() {
    let mut ws = setup();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": false } });
    let uri = ws.class_file_uri("salut", "org.apache.commons.lang3.StringUtils");
    let definitions = type_definition(&mut ws, &uri, 20, 26);
    assert!(definitions.is_null());
}
