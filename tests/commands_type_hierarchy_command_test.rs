//! Port of `org.eclipse.jdt.ls.core.internal.commands.TypeHierarchyCommandTest`
//! (the legacy `java.navigate.openTypeHierarchy` / `resolveTypeHierarchy`
//! commands, driven through `workspace/executeCommand` exactly as
//! `JDTDelegateCommandHandler` unpacks them).

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

const SYMBOL_KIND_CLASS: u64 = 5;
const SYMBOL_KIND_NULL: u64 = 21;

/// lsp4j `TypeHierarchyDirection`.
const CHILDREN: i64 = 0;
const PARENTS: i64 = 1;
const BOTH: i64 = 2;

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    ws
}

/// `IFile.getLocationURI().toString()` of a project-relative file.
fn file_uri(ws: &Workspace, project: &str, rel: &str) -> String {
    let path = ws.project_root(project).join(rel);
    format!("file:{}", path.to_string_lossy().replace(' ', "%20"))
}

fn type_hierarchy(ws: &mut Workspace, uri: &str, line: u32, character: u32, direction: i64, resolve: i64) -> Value {
    ws.request(
        "workspace/executeCommand",
        json!({
            "command": "java.navigate.openTypeHierarchy",
            // vscode-java passes every argument through `JSON.stringify`.
            "arguments": [
                json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }).to_string(),
                direction.to_string(),
                resolve.to_string()
            ]
        }),
    )
}

fn resolve_type_hierarchy(ws: &mut Workspace, item: &Value, direction: i64, resolve: i64) -> Value {
    ws.request(
        "workspace/executeCommand",
        json!({
            "command": "java.navigate.resolveTypeHierarchy",
            "arguments": [item.to_string(), direction.to_string(), resolve.to_string()]
        }),
    )
}

fn list(v: &Value) -> &Vec<Value> {
    v.as_array().expect("expected a list")
}

#[test]
fn test_type_hierarchy() {
    let mut ws = setup();
    let uri = file_uri(&ws, "salut", "src/main/java/org/sample/TestJavadoc.java");
    let item = type_hierarchy(&mut ws, &uri, 4, 20, BOTH, 1);
    assert!(!item.is_null());
    assert_eq!(item["name"], "TestJavadoc");
    assert!(!item["children"].is_null());
    assert_eq!(list(&item["children"]).len(), 0);
    assert!(!item["parents"].is_null());
    assert_eq!(list(&item["parents"]).len(), 1);
    assert_eq!(item["parents"][0]["name"], "Object");
}

#[test]
fn test_super_type_hierarchy() {
    let mut ws = setup();
    let uri = file_uri(&ws, "salut", "src/main/java/org/sample/CallHierarchy.java");
    let item = type_hierarchy(&mut ws, &uri, 7, 27, PARENTS, 1);
    assert!(!item.is_null());
    assert_eq!(item["name"], "CallHierarchy$FooBuilder");
    assert!(item["children"].is_null());
    assert_eq!(list(&item["parents"]).len(), 2);
    let builder = &item["parents"][0];
    assert!(!builder.is_null());
    assert_eq!(builder["name"], "Builder");
    assert!(builder["parents"].is_null());
    let object = &item["parents"][1];
    assert!(!object.is_null());
    assert_eq!(object["name"], "Object");
    assert!(object["parents"].is_null());
}

#[test]
fn test_sub_type_hierarchy() {
    let mut ws = setup();
    let uri = file_uri(&ws, "salut", "src/main/java/org/sample/CallHierarchy.java");
    let item = type_hierarchy(&mut ws, &uri, 2, 43, CHILDREN, 2);
    assert!(!item.is_null());
    assert_eq!(item["name"], "Builder");
    assert!(item["parents"].is_null());
    assert_eq!(10, list(&item["children"]).len());
    for child in list(&item["children"]) {
        let sub_child = &child["children"];
        assert!(!sub_child.is_null());
        if list(sub_child).len() == 1 {
            assert_eq!(sub_child[0]["name"], "ReflectionToStringBuilder");
        }
    }
}

// https://github.com/redhat-developer/vscode-java/issues/2871
#[test]
fn test_multiple_projects() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/gh2871"]);
    let uri = file_uri(&ws, "project1", "src/org/sample/First.java");
    let item = type_hierarchy(&mut ws, &uri, 1, 22, BOTH, 1);
    assert!(!item.is_null());
    assert_eq!(item["name"], "First");
    assert!(!item["children"].is_null());
    assert_eq!(list(&item["children"]).len(), 1);
    assert_eq!(item["children"][0]["name"], "Second");
}

#[test]
fn test_method_hierarchy() {
    let mut ws = setup();
    ws.import_projects(&["maven/type-hierarchy"]);
    let uri = file_uri(&ws, "type-hierarchy", "src/main/java/org/example/Zero.java");
    let zero = type_hierarchy(&mut ws, &uri, 3, 17, BOTH, 1); // public void f[o]o()

    // do not show java.lang.Object if target method isn't from there
    assert_eq!(0, list(&zero["parents"]).len());

    assert_eq!(SYMBOL_KIND_CLASS, zero["kind"]); // zero
    assert_eq!(SYMBOL_KIND_CLASS, zero["children"][0]["kind"]); // one
    assert_eq!(SYMBOL_KIND_NULL, zero["children"][1]["kind"]); // two

    let one = resolve_type_hierarchy(&mut ws, &zero["children"][0], BOTH, 1);
    assert_eq!(SYMBOL_KIND_NULL, one["children"][1]["kind"]); // three
    assert_eq!(SYMBOL_KIND_CLASS, one["children"][0]["kind"]); // four

    let two = resolve_type_hierarchy(&mut ws, &zero["children"][1], BOTH, 1);
    assert_eq!(SYMBOL_KIND_NULL, two["children"][0]["kind"]); // five
    assert_eq!(SYMBOL_KIND_CLASS, two["children"][1]["kind"]); // six
}
