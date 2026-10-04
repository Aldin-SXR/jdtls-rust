//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ImplementationsHandlerTest`.
//!
//! Upstream builds the handler with a fresh `PreferenceManager` mock, so
//! `isClientSupportsClassFileContent()` is `false` unless a test enables it.

mod common;
use common::jdtls::{pos, range, Workspace};
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": false } });
    ws
}

fn file_uri(ws: &Workspace, rel: &str) -> String {
    ws.path_uri(&format!("eclipse/hello/{rel}"))
}

fn find_implementations(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Vec<Value> {
    let result = ws.request(
        "textDocument/implementation",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    assert!(!result.is_null(), "findImplementations should not return null");
    result.as_array().cloned().unwrap_or_default()
}

fn find(implementations: &[Value], path: &str) -> Value {
    implementations
        .iter()
        .find(|i| i["uri"].as_str().unwrap().contains(path))
        .cloned()
        .unwrap_or_else(|| panic!("no implementation in {path}: {implementations:?}"))
}

#[test]
fn test_empty() {
    let mut ws = setup();
    let implementations = find_implementations(&mut ws, "/foo/bar", 1, 1);
    assert!(implementations.is_empty(), "implementations are not empty");
}

#[test]
fn test_interface_implementation() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/IFoo.java");
    let implementations = find_implementations(&mut ws, &uri, 2, 20); // Position over IFoo
    assert_eq!(2, implementations.len());
    let foo2 = find(&implementations, "org/sample/Foo2.java");
    assert_eq!(range(2, 13, 2, 17), foo2["range"]);
    let foo3 = find(&implementations, "org/sample/Foo3.java");
    assert_eq!(range(5, 13, 5, 17), foo3["range"]);
}

#[test]
fn test_class_implementation() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/Foo2.java");
    let implementations = find_implementations(&mut ws, &uri, 2, 14); // Position over Foo2
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo3 = &implementations[0];
    assert!(foo3["uri"].as_str().unwrap().contains("org/sample/Foo3.java"), "Unexpected implementation : {}", foo3["uri"]);
    assert_eq!(range(5, 13, 5, 17), foo3["range"]);
}

#[test]
fn test_method_implementation() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/IFoo.java");
    let implementations = find_implementations(&mut ws, &uri, 4, 14); // Position over IFoo#someMethod
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo2 = &implementations[0];
    assert!(foo2["uri"].as_str().unwrap().contains("org/sample/Foo2.java"), "Unexpected implementation : {}", foo2["uri"]);
    // check range points to someMethod() position
    assert_eq!(pos(4, 16), foo2["range"]["start"]);
    assert_eq!(pos(4, 26), foo2["range"]["end"]);
}

#[test]
fn test_method_invocation_implementation() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooService.java");
    let implementations = find_implementations(&mut ws, &uri, 6, 14); // Position over foo.someMethod
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo2 = &implementations[0];
    assert!(foo2["uri"].as_str().unwrap().contains("org/sample/Foo2.java"), "Unexpected implementation : {}", foo2["uri"]);
    assert_eq!(pos(4, 16), foo2["range"]["start"]);
    assert_eq!(pos(4, 26), foo2["range"]["end"]);
}

#[test]
fn test_method_super_invocation_implementation() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooChild.java");
    let implementations = find_implementations(&mut ws, &uri, 5, 14); // Position over super.someMethod
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo = &implementations[0];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/Foo.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(8, 13), foo["range"]["start"]);
    assert_eq!(pos(8, 23), foo["range"]["end"]);
}

#[test]
fn test_class_implementation_include_definition() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooService.java");
    let implementations = find_implementations(&mut ws, &uri, 10, 20); // Position over new Foo()
    assert_eq!(2, implementations.len(), "{implementations:?}");
    let foo = &implementations[0];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/Foo.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(2, 13), foo["range"]["start"]);
    assert_eq!(pos(2, 16), foo["range"]["end"]);
    let foo = &implementations[1];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/FooChild.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(2, 13), foo["range"]["start"]);
    assert_eq!(pos(2, 21), foo["range"]["end"]);
}

#[test]
fn test_method_implementation_include_definition() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooService.java");
    let implementations = find_implementations(&mut ws, &uri, 11, 13); // Position over someMethod()
    assert_eq!(2, implementations.len(), "{implementations:?}");
    let foo = &implementations[0];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/Foo.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(8, 13), foo["range"]["start"]);
    assert_eq!(pos(8, 23), foo["range"]["end"]);
    let foo = &implementations[1];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/FooChild.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(4, 13), foo["range"]["start"]);
    assert_eq!(pos(4, 23), foo["range"]["end"]);
}

#[test]
fn test_unimplemented_class_implementation_include_definition() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooService.java");
    let implementations = find_implementations(&mut ws, &uri, 14, 13); // Position over AbstractFoo.
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo = &implementations[0];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/AbstractFoo.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(2, 22), foo["range"]["start"]);
    assert_eq!(pos(2, 33), foo["range"]["end"]);
}

#[test]
fn test_unimplemented_method_implementation_include_definition() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/FooService.java");
    let implementations = find_implementations(&mut ws, &uri, 15, 13); // Position over someMethod()
    assert_eq!(1, implementations.len(), "{implementations:?}");
    let foo = &implementations[0];
    assert!(foo["uri"].as_str().unwrap().contains("org/sample/AbstractFoo.java"), "Unexpected implementation : {}", foo["uri"]);
    assert_eq!(pos(4, 15), foo["range"]["start"]);
    assert_eq!(pos(4, 25), foo["range"]["end"]);
}

fn get_runnable_implementations(ws: &mut Workspace) -> Vec<Value> {
    let uri = file_uri(ws, "src/org/sample/RunnableTest.java");
    find_implementations(ws, &uri, 2, 42) // implementations of java.lang.Runnable
}

#[test]
fn test_implementation_from_binary_type_without_class_content_support() {
    let mut ws = setup();
    // Only workspace implementation returned
    let implementations = get_runnable_implementations(&mut ws);
    assert_eq!(1, implementations.len(), "{implementations:?}");
    assert!(
        implementations[0]["uri"].as_str().unwrap().contains("org/sample/RunnableTest.java"),
        "Unexpected implementation : {}",
        implementations[0]["uri"]
    );
    assert_eq!(range(2, 13, 2, 25), implementations[0]["range"]);
}

#[test]
#[ignore = "expects exactly the 8 Runnable implementations of the fake JDK rtstubs.jar; the running JDK has hundreds, and binary subtype search over the JDK is not implemented"]
fn test_implementation_from_binary_type_with_class_content_support() {
    let mut ws = setup();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
    // workspace + binary implementations returned
    let implementations = get_runnable_implementations(&mut ws);
    // only jdk classes are expected to implement Runnable
    assert_eq!(8, implementations.len(), "{implementations:?}");
    let default_range = range(0, 0, 0, 0);
    let contains_runnable_test = implementations.iter().any(|i| i["uri"].as_str().unwrap().contains("org/sample/RunnableTest.java"));
    assert!(contains_runnable_test, "Implementation not found : org/sample/RunnableTest.java");
    for implem in implementations.iter().filter(|i| !i["uri"].as_str().unwrap().contains("org/sample/RunnableTest.java")) {
        assert!(implem["uri"].as_str().unwrap().contains("rtstubs.jar"), "Unexpected implementation : {}", implem["uri"]);
        assert_eq!(default_range, implem["range"], "Expected default location "); // no jdk sources available
    }
}

#[test]
fn test_invalid_element() {
    let mut ws = setup();
    let uri = file_uri(&ws, "src/org/sample/Foo4.java");
    let implementations = find_implementations(&mut ws, &uri, 3, 34); // Position over T
    assert_eq!(0, implementations.len(), "{implementations:?}");
}
