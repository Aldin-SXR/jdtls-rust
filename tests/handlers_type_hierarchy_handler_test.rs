//! Port of `org.eclipse.jdt.ls.core.internal.handlers.TypeHierarchyHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

const SYMBOL_KIND_CLASS: u64 = 5;
const SYMBOL_KIND_INTERFACE: u64 = 11;
const SYMBOL_KIND_NULL: u64 = 21;

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

fn prepare(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Vec<Value> {
    let r = ws.request(
        "textDocument/prepareTypeHierarchy",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    r.as_array().cloned().expect("prepareTypeHierarchy returned null")
}

fn supertypes(ws: &mut Workspace, item: &Value) -> Vec<Value> {
    let r = ws.request("typeHierarchy/supertypes", json!({ "item": item }));
    r.as_array().cloned().expect("supertypes returned null")
}

fn subtypes(ws: &mut Workspace, item: &Value) -> Vec<Value> {
    let r = ws.request("typeHierarchy/subtypes", json!({ "item": item }));
    r.as_array().cloned().expect("subtypes returned null")
}

#[test]
fn test_super_type_hierarchy() {
    let mut ws = setup();
    let uri = file_uri(&ws, "salut", "src/main/java/org/sample/CallHierarchy.java");
    let items = prepare(&mut ws, &uri, 7, 27);
    assert_eq!(1, items.len());
    assert_eq!(items[0]["name"], "CallHierarchy$FooBuilder");
    let supertypes_items = supertypes(&mut ws, &items[0]);
    assert_eq!(2, supertypes_items.len());
    assert_eq!(supertypes_items[0]["name"], "Builder");
    assert_eq!(supertypes_items[0]["kind"], SYMBOL_KIND_INTERFACE);
    assert_eq!(supertypes_items[1]["name"], "Object");
    assert_eq!(supertypes_items[1]["kind"], SYMBOL_KIND_CLASS);
}

#[test]
fn test_sub_type_hierarchy() {
    let mut ws = setup();
    let uri = file_uri(&ws, "salut", "src/main/java/org/sample/CallHierarchy.java");
    let items = prepare(&mut ws, &uri, 2, 43);
    assert_eq!(1, items.len());
    assert_eq!(items[0]["name"], "Builder");
    let subtypes_items = subtypes(&mut ws, &items[0]);
    assert_eq!(10, subtypes_items.len());
}

// https://github.com/redhat-developer/vscode-java/issues/2871
#[test]
fn test_multiple_projects() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/gh2871"]);
    let uri = file_uri(&ws, "project1", "src/org/sample/First.java");
    let items = prepare(&mut ws, &uri, 1, 22);
    assert_eq!(1, items.len());
    assert_eq!("First", items[0]["name"]);
    let subtypes_items = subtypes(&mut ws, &items[0]);
    assert_eq!(1, subtypes_items.len());
    assert_eq!("Second", subtypes_items[0]["name"]);
}

#[test]
fn test_method_hierarchy() {
    let mut ws = setup();
    ws.import_projects(&["maven/type-hierarchy"]);
    let uri = file_uri(&ws, "type-hierarchy", "src/main/java/org/example/Zero.java");
    let zero_items = prepare(&mut ws, &uri, 3, 17); // public void f[o]o()
    assert_eq!(1, zero_items.len());
    assert_eq!("Zero", zero_items[0]["name"]);
    assert_eq!(SYMBOL_KIND_CLASS, zero_items[0]["kind"]);

    let supertypes_items = supertypes(&mut ws, &zero_items[0]);
    // do not show java.lang.Object if target method isn't from there
    assert_eq!(0, supertypes_items.len());

    let subtypes_items = subtypes(&mut ws, &zero_items[0]);
    assert_eq!(SYMBOL_KIND_CLASS, subtypes_items[0]["kind"]); // one
    assert_eq!("One", subtypes_items[0]["name"]); // one
    assert_eq!(SYMBOL_KIND_NULL, subtypes_items[1]["kind"]); // two
    assert_eq!("Two", subtypes_items[1]["name"]); // two

    let one = subtypes_items[0].clone();
    let two = subtypes_items[1].clone();
    let subtypes_items = subtypes(&mut ws, &one);
    assert_eq!(SYMBOL_KIND_NULL, subtypes_items[1]["kind"]); // three
    assert_eq!("Three", subtypes_items[1]["name"]); // three
    assert_eq!(SYMBOL_KIND_CLASS, subtypes_items[0]["kind"]); // four
    assert_eq!("Four", subtypes_items[0]["name"]); // four

    let subtypes_items = subtypes(&mut ws, &two);
    assert_eq!(SYMBOL_KIND_NULL, subtypes_items[0]["kind"]); // five
    assert_eq!("Five", subtypes_items[0]["name"]); // five
    assert_eq!(SYMBOL_KIND_CLASS, subtypes_items[1]["kind"]); // six
    assert_eq!("Six", subtypes_items[1]["name"]); // six
}
