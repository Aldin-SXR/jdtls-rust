//! Port of `org.eclipse.jdt.ls.core.internal.javadoc.JavadocContentTest`.
//!
//! Upstream calls `HoverInfoProvider.computeSignature(element)` and
//! `computeJavadoc(element)` on Java model elements found with
//! `IJavaProject.findType` / `getField` / `getMethod`. Over LSP the same two
//! values are the first two hover contents on the element's declaration
//! name (`computeHover` is exactly `[computeSignature, computeJavadoc,
//! sourceInfo]`).

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

const FILE: &str = "src/org/sample/TestJavadoc.java";

fn setup() -> (Workspace, String) {
    let mut ws = Workspace::new();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
    ws.import_projects(&["eclipse/hello"]);
    let uri = Url::from_file_path(ws.project_root("hello").join(FILE)).unwrap().to_string();
    (ws, uri)
}

/// Hover contents on the declaration name at `line:character`.
fn element(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Vec<Value> {
    let hover = ws.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    // lsp4j writes a one-element list as that element
    match &hover["contents"] {
        Value::Array(a) => a.clone(),
        v @ (Value::String(_) | Value::Object(_)) => vec![v.clone()],
        _ => panic!("no hover contents: {hover}"),
    }
}

/// `computeSignature(element).getValue()`
fn signature(contents: &[Value]) -> String {
    assert_eq!(contents[0]["language"], "java");
    contents[0]["value"].as_str().unwrap().to_owned()
}

/// `computeJavadoc(element)`: the Javadoc entry, or `None` when the hover
/// has only signature and source.
fn javadoc(contents: &[Value]) -> Option<String> {
    let docs: Vec<&str> = contents[1..]
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| !s.starts_with("Source: *"))
        .collect();
    assert!(docs.len() <= 1, "unexpected hover contents {contents:?}");
    docs.first().map(|s| s.to_string())
}

#[test]
fn test_class_javadoc() {
    let (mut ws, uri) = setup();
    // public class TestJavadoc<K, V> {
    let c = element(&mut ws, &uri, 12, 14);
    assert_eq!("org.sample.TestJavadoc<K, V>", signature(&c));
    let expected_javadoc = "Test javadoc class\n\
\n\
* **Type Parameters:**\n  * **\\<K\\>** the type of keys\n  * **\\<V\\>** the type of values\n\
* **Author:**\n  * Some dude\n  * Another one\n\
* **See Also:**\n  * some.pkg.SomeClass\n  * some.pkg.SomeClass.someMethod()";
    assert_eq!(Some(expected_javadoc.to_owned()), javadoc(&c));
}

#[test]
fn test_field_javadoc() {
    let (mut ws, uri) = setup();
    // public int fooField;
    let c = element(&mut ws, &uri, 17, 13);
    assert_eq!("int fooField", signature(&c));
    assert_eq!(Some("Foo field".to_owned()), javadoc(&c));
}

#[test]
fn test_method_javadoc() {
    let (mut ws, uri) = setup();
    // private String foo(String input, int count) {
    let c = element(&mut ws, &uri, 26, 17);
    assert_eq!("String org.sample.TestJavadoc.foo(String input, int count)", signature(&c));
    let expected_javadoc = "Foo method\n\
\n\
* **Parameters:**\n  * **input** some input\n  * **count** some count\n\
* **Returns:**\n  * some string";
    assert_eq!(Some(expected_javadoc.to_owned()), javadoc(&c));
}

#[test]
fn test_literal_code_javadoc() {
    let (mut ws, uri) = setup();
    // public void anotherMethod() {
    let c = element(&mut ws, &uri, 44, 15);
    assert_eq!("void org.sample.TestJavadoc.anotherMethod()", signature(&c));
    let expected_javadoc = "\n      interface Service {\n         @LookupIfProperty(name = \"service.foo.enabled\", stringValue = \"true\")\n         String name();\n      }\n      \n";
    assert_eq!(Some(expected_javadoc.to_owned()), javadoc(&c));
}

#[test]
fn test_null_javadoc() {
    let (mut ws, uri) = setup();
    // public class Inner {
    let c = element(&mut ws, &uri, 31, 15);
    assert_eq!("org.sample.TestJavadoc.Inner", signature(&c));
    // getMarkdownContentReader(inner) == null && getMarkdownContent(inner) == null
    assert_eq!(None, javadoc(&c));
}
