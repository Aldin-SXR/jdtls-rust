//! All seven GenerateToStringActionTest methods with upstream sources and assertions.
mod common;
use common::jdtls::{test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(false);
    ws.init_options["extendedClientCapabilities"]["generateToStringPromptSupport"] = json!(true);
    ws
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Vec<Value> {
    ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})).as_array().expect("actions").clone()
}
fn source_action(a: &[Value]) -> Option<&Value> {
    a.iter().find(|a| a["kind"] == "source.generate.toString")
}
fn has_prompt(a: &[Value]) -> bool {
    a.iter().any(|a| {
        a["kind"] == "quickassist"
            && a["command"]["command"] == "java.action.generateToStringPrompt"
    })
}
fn has_direct(a: &[Value]) -> bool {
    a.iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Generate toString()")
}

#[test]
fn test_generate_to_string_enabled() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    let action = source_action(&a).expect("source action");
    assert_eq!(
        action["command"]["command"],
        "java.action.generateToStringPrompt"
    );
}

#[test]
fn test_generate_to_string_enabled_empty_fields() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tprivate static String name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    let action = source_action(&a).expect("source action");
    assert!(!action["edit"].is_null());
}

#[test]
fn test_generate_to_string_disabled_interface() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic interface A {\r\n\tpublic final String name = \"test\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(source_action(&a).is_none());
}

#[test]
fn test_generate_to_string_disabled_enum() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source="package p;\r\n\r\npublic enum A {\r\n\tMONDAY,\r\n\tTUESDAY;\r\n\tprivate String name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(source_action(&a).is_none());
}

#[test]
fn test_generate_to_string_quick_assist() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "A");
    assert!(has_prompt(&a));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(!has_prompt(&a));
}

#[test]
fn test_generate_to_string_quick_assist_empty_fields() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tprivate static String name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "A");
    assert!(has_direct(&a));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(!has_direct(&a));
}

#[test]
fn test_no_generate_to_string_quick_assist() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source="package p;\r\n\r\npublic class A {\r\n\tString name;\r\n   public String toString() {\r\n\t\treturn this.name;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "A");
    assert!(!a
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Generate toString()..."));
}
