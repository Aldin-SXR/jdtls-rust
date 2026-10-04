//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CallHierarchyHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

const CLASS: u64 = 5;
const METHOD: u64 = 6;
const FIELD: u64 = 8;
const CONSTRUCTOR: u64 = 9;
const DEPRECATED_TAG: u64 = 1;
/// `JavaElementLabelsCore.DECL_STRING`
const DECL_STRING: &str = " : ";

/// `setup()`: `importProjects(Arrays.asList("eclipse/hello", "maven/salut"))`.
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello", "maven/salut"]);
    ws
}

/// `assertItem(item, name, kind, detail, deprecated, selectionStartLine)`.
fn assert_item(item: &Value, name: &str, kind: u64, detail: &str, deprecated: bool, selection_start_line: u64) {
    assert!(!item.is_null());
    assert_eq!(item["name"], name, "name of {item}");
    assert_eq!(item["kind"], kind, "kind of {item}");
    assert_eq!(item["detail"], detail, "detail of {item}");
    let tags: Vec<Value> = item["tags"].as_array().cloned().unwrap_or_default();
    if deprecated {
        assert!(item["tags"].is_array(), "tags of {item}");
        assert!(tags.iter().any(|t| *t == DEPRECATED_TAG), "tags of {item}");
    } else {
        assert!(tags.is_empty() || !tags.iter().any(|t| *t == DEPRECATED_TAG), "tags of {item}");
    }
    assert_eq!(item["selectionRange"]["start"]["line"], selection_start_line, "selection start of {item}");
}

fn prepare(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Value {
    ws.request(
        "textDocument/prepareCallHierarchy",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    )
}

fn get_incoming_calls(ws: &mut Workspace, item: &Value) -> Value {
    ws.request("callHierarchy/incomingCalls", json!({ "item": item }))
}

fn get_outgoings(ws: &mut Workspace, item: &Value) -> Value {
    ws.request("callHierarchy/outgoingCalls", json!({ "item": item }))
}

fn get_uri_from_jar_project(ws: &Workspace, class_name: &str) -> String {
    ws.class_uri("salut", class_name)
}

fn get_uri_from_src_project(ws: &Workspace, class_name: &str) -> String {
    ws.class_uri("hello", class_name)
}

fn items(v: &Value) -> Vec<Value> {
    assert!(v.is_array(), "expected a list, got {v}");
    v.as_array().cloned().unwrap()
}

#[test]
fn prepare_call_hierarchy_no_item_at_location() {
    let mut ws = setup();
    // Line 16 from `CallHierarchy`
    //<|>/*nothing*/
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchy");
    assert!(prepare(&mut ws, &uri, 15, 0).is_null());
}

#[test]
fn prepare_call_hierarchy() {
    let mut ws = setup();
    // Line 15 from `CallHierarchy`
    //    protected int <|>protectedField = 200;
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchy");
    let items = items(&prepare(&mut ws, &uri, 14, 18));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], "protectedField", FIELD, "org.sample.CallHierarchy$Base", false, 14);
}

#[test]
fn prepare_call_hierarchy_src_enclosing() {
    let mut ws = setup();
    // Line 20 from `CallHierarchy`
    // /*should resolve to enclosing "method/constructor/initializer"*/
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchy");
    let items = items(&prepare(&mut ws, &uri, 19, 0));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], "Base()", CONSTRUCTOR, "org.sample.CallHierarchy$Base", false, 18);
}

#[test]
fn incoming_calls_src() {
    let mut ws = setup();
    // Line 27 from `CallHierarchy`
    //    public void <|>bar() {
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchy");
    let items = items(&prepare(&mut ws, &uri, 26, 16));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], &format!("bar(){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy$Base", false, 26);

    let calls = items_of(&get_incoming_calls(&mut ws, &items[0]));
    assert_eq!(calls.len(), 3);
    assert_item(&calls[0]["from"], "Child()", CONSTRUCTOR, "org.sample.CallHierarchy$Child", false, 45);
    assert_item(&calls[1]["from"], &format!("main(String[]){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy", true, 7);
    assert_item(&calls[2]["from"], &format!("method_1(){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy$Base", false, 35);
}

fn items_of(v: &Value) -> Vec<Value> {
    items(v)
}

#[test]
fn test_selection_range() {
    let mut ws = setup();
    // Line  from `org.sample.Foo`
    //    public void <|>someMethod() {}
    let uri = get_uri_from_src_project(&ws, "org.sample.Foo");
    let items = items(&prepare(&mut ws, &uri, 8, 13));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], &format!("someMethod(){DECL_STRING}void"), METHOD, "org.sample.Foo", false, 8);

    let calls = items_of(&get_incoming_calls(&mut ws, &items[0]));
    assert_eq!(calls.len(), 4);
    assert_item(&calls[2]["from"], "main(String[]) : void", METHOD, "org.sample.Call", false, 7);
    assert_item(&calls[3]["from"], "main(String[]) : void", METHOD, "org.sample.Call", false, 10);
    let selection_range = &calls[2]["from"]["selectionRange"];
    assert_eq!(*selection_range, json!({ "start": { "line": 7, "character": 18 }, "end": { "line": 7, "character": 30 } }));
    let selection_range = &calls[3]["from"]["selectionRange"];
    assert_eq!(*selection_range, json!({ "start": { "line": 10, "character": 18 }, "end": { "line": 10, "character": 30 } }));
}

#[test]
#[ignore = "upstream rtstubs has no source; the real JDK locates currentThread in src.zip, identically on the oracle"]
fn outgoing_calls_src() {
    let mut ws = setup();
    // Line 34 from `CallHierarchy`
    //    protected void <|>method_1() {
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchy");
    let items = items(&prepare(&mut ws, &uri, 33, 19));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], &format!("method_1(){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy$Base", false, 33);

    let calls = items_of(&get_outgoings(&mut ws, &items[0]));
    assert_eq!(calls.len(), 2);
    assert_item(&calls[0]["to"], &format!("foo(){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy$Base", false, 22);
    assert_item(&calls[1]["to"], &format!("bar(){DECL_STRING}void"), METHOD, "org.sample.CallHierarchy$Base", false, 26);

    let call1_calls = items_of(&get_outgoings(&mut ws, &calls[1]["to"]));
    assert_eq!(call1_calls.len(), 4);
    assert_item(&call1_calls[0]["to"], "Child()", CONSTRUCTOR, "org.sample.CallHierarchy$Child", false, 42);
    assert_item(&call1_calls[2]["to"], &format!("currentThread(){DECL_STRING}Thread"), METHOD, "java.lang.Thread", false, 0);
}

#[test]
fn incoming_calls_maven() {
    let mut ws = setup();
    // Line 12 from `CallHierarchyOther`
    //  @Deprecated public static class <|>X {
    let uri = get_uri_from_jar_project(&ws, "org.sample.CallHierarchyOther");
    let items = items(&prepare(&mut ws, &uri, 11, 34));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], "X", CLASS, "org.sample.CallHierarchyOther", true, 11);

    let calls = items_of(&get_incoming_calls(&mut ws, &items[0]));
    assert_eq!(calls.len(), 1);
    assert_item(&calls[0]["from"], "FooBuilder()", CONSTRUCTOR, "org.sample.CallHierarchy$FooBuilder", false, 10);

    let call0_calls = items_of(&get_incoming_calls(&mut ws, &calls[0]["from"]));
    assert_eq!(call0_calls.len(), 3);
    assert_item(&call0_calls[0]["from"], "{...}", CONSTRUCTOR, "org.sample.CallHierarchyOther", false, 5);
}

#[test]
fn outgoing_jar() {
    let mut ws = setup();
    // Line 15 from `CallHierarchy`
    //    public Object <|>build() {
    let uri = get_uri_from_jar_project(&ws, "org.sample.CallHierarchy");
    let items = items(&prepare(&mut ws, &uri, 14, 18));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], &format!("build(){DECL_STRING}Object"), METHOD, "org.sample.CallHierarchy$FooBuilder", false, 14);

    let calls = items_of(&get_outgoings(&mut ws, &items[0]));
    assert_eq!(calls.len(), 2);
    assert_item(&calls[0]["to"], &format!("capitalize(String){DECL_STRING}String"), METHOD, "org.apache.commons.lang3.text.WordUtils", false, 61);

    let call0_calls = items_of(&get_outgoings(&mut ws, &calls[0]["to"]));
    assert_eq!(call0_calls.len(), 1);
    assert_item(&call0_calls[0]["to"], &format!("capitalize(String, char...){DECL_STRING}String"), METHOD, "org.apache.commons.lang3.text.WordUtils", false, 94);

    let jar_uri = call0_calls[0]["to"]["uri"].as_str().unwrap().to_owned();
    assert!(jar_uri.starts_with("jdt://"));
    assert!(jar_uri.contains("org.apache.commons.lang3.text/WordUtils.java?"));
    assert!(jar_uri.contains("org.apache.commons.lang3.text%28WordUtils.class"));
}

#[test]
fn incoming_calls_on_interface_method() {
    let mut ws = setup();
    // Line 27 from `CallHierarchy`
    //    public void <|>bar() {
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchyGH2771");
    let items = items(&prepare(&mut ws, &uri, 3, 17));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], &format!("name(){DECL_STRING}Opt<String>"), METHOD, "org.sample.CallHierarchyGH2771", false, 3);

    let calls = items_of(&get_incoming_calls(&mut ws, &items[0]));
    assert_eq!(calls.len(), 0);
}

#[test]
fn incoming_calls_on_interface_method_return_type() {
    let mut ws = setup();
    // Line 27 from `CallHierarchy`
    //    public void <|>bar() {
    let uri = get_uri_from_src_project(&ws, "org.sample.CallHierarchyGH2771");
    let items = items(&prepare(&mut ws, &uri, 3, 6));
    assert_eq!(items.len(), 1);
    assert_item(&items[0], "Opt<T>", CLASS, "org.sample.CallHierarchyGH2771", false, 5);

    let calls = items_of(&get_incoming_calls(&mut ws, &items[0]));
    assert_eq!(calls.len(), 1);
}
