//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ReferencesHandlerTest`.
//!
//! Upstream builds the handler with a fresh `PreferenceManager` mock, so
//! `isClientSupportsClassFileContent()` is `false` unless a test enables it.

mod common;
use common::jdtls::{range, Workspace};
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": false } });
    ws
}

fn with_class_file_support(ws: &mut Workspace) {
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
}

fn find_references(ws: &mut Workspace, uri: &str, line: u32, character: u32, include_declaration: bool) -> Vec<Value> {
    let result = ws.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        }),
    );
    assert!(!result.is_null(), "findReferences should not return null");
    result.as_array().cloned().unwrap_or_default()
}

#[test]
fn test_empty() {
    let mut ws = setup();
    let references = find_references(&mut ws, "/foo/bar", 1, 1, false);
    assert!(references.is_empty(), "references are not empty");
}

#[test]
fn test_reference() {
    let mut ws = setup();
    let uri = ws.path_uri("eclipse/hello/src/java/Foo2.java");
    let references = find_references(&mut ws, &uri, 5, 16, false);
    assert_eq!(1, references.len(), "{references:?}");
    let referee_uri = ws.path_uri("eclipse/hello/src/java/Foo3.java");
    assert_eq!(referee_uri, references[0]["uri"]);
}

#[test]
fn test_include_accessors() {
    let mut ws = setup();
    let uri = ws.path_uri("eclipse/hello/src/org/ref/Apple.java");
    ws.update_settings(json!({ "java": { "references": { "includeAccessors": false } } }));
    let references = find_references(&mut ws, &uri, 3, 18, false);
    assert_eq!(3, references.len(), "{references:?}");
    ws.update_settings(json!({ "java": { "references": { "includeAccessors": true } } }));
    let references = find_references(&mut ws, &uri, 3, 18, false);
    assert_eq!(5, references.len(), "{references:?}");
    assert_eq!(ws.path_uri("eclipse/hello/src/org/ref/Apple.java"), references[0]["uri"]);
    assert_eq!(ws.path_uri("eclipse/hello/src/org/ref/Test.java"), references[4]["uri"]);
}

#[test]
fn test_enum_in_class_file() {
    let mut ws = setup();
    with_class_file_support(&mut ws);
    ws.import_projects(&["eclipse/reference"]);
    let file_uri = ws.class_file_uri("reference", "org.sample.Foo");
    let references = find_references(&mut ws, &file_uri, 5, 6, false);
    assert_eq!(2, references.len(), "{references:?}");
    let referee_uri = ws.path_uri("eclipse/reference/src/org/reference/Main.java");
    assert_eq!(referee_uri, references[0]["uri"]);
    assert_eq!(file_uri, references[1]["uri"]);
}

// https://github.com/redhat-developer/vscode-java/issues/2227
#[test]
fn test_potential_match() {
    let mut ws = setup();
    with_class_file_support(&mut ws);
    ws.import_projects(&["eclipse/reference"]);
    let uri = ws.path_uri("eclipse/reference/src/org/reference/User.java");
    let references = find_references(&mut ws, &uri, 1, 15, false);
    assert_eq!(0, references.len(), "{references:?}");
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/2148
#[test]
fn test_declaration_in_references() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/reference"]);
    let uri = ws.path_uri("eclipse/reference/src/org/reference/Main.java");
    let references = find_references(&mut ws, &uri, 12, 22, true);
    assert_eq!(2, references.len(), "{references:?}");
    assert_eq!(range(12, 20, 12, 32), references[0]["range"]);
    assert_eq!(range(14, 15, 14, 25), references[1]["range"]);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/2405
#[test]
#[ignore = "calls ReferencesHandler.search(IField) on java.lang.System.out directly (no LSP equivalent) and expects a match inside rtstubs.jar's System.class; searching references inside the JDK's class files is not implemented"]
fn test_references_in_jre() {
    let mut ws = setup();
    with_class_file_support(&mut ws);
    let uri = ws.class_file_uri("hello", "java.lang.System");
    // `System.out` declaration in the System class file.
    let contents = ws.request("java/classFileContents", json!({ "uri": uri }));
    let line = contents.as_str().unwrap().lines().position(|l| l.contains("public static final PrintStream out")).unwrap() as u32;
    let references = find_references(&mut ws, &uri, line, 37, false);
    let location = references
        .iter()
        .find(|r| r["uri"].as_str().unwrap().starts_with("jdt://contents/rtstubs.jar/java.lang/System.class"));
    assert!(location.is_some());
}
