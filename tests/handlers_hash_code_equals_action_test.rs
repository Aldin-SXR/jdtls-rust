//! All six HashCodeEqualsActionTest methods, preserving sources and assertions.
mod common;
use common::jdtls::{test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
fn actions(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Vec<Value> {
    ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})).as_array().expect("code actions").clone()
}
fn source_action(a: &[Value]) -> bool {
    a.iter()
        .any(|a| a["kind"] == "source.generate.hashCodeEquals")
}
fn quick_actions(a: &[Value]) -> Vec<&Value> {
    a.iter().filter(|a| a["kind"] == "quickassist").collect()
}
fn has_prompt(a: &[&Value]) -> bool {
    a.iter()
        .any(|a| a["command"]["command"] == "java.action.hashCodeEqualsPrompt")
}

#[test]
fn test_hash_code_equals_enabled() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(source_action(&a));
}

#[test]
fn test_hash_code_equals_disabled_empty_fields() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic static String name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(!source_action(&a));
}

#[test]
fn test_hash_code_equals_disabled_interface() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic interface A {\r\n\tpublic final String name = \"test\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(!source_action(&a));
}

#[test]
fn test_hash_code_equals_disabled_enum() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source="package p;\r\n\r\npublic enum A {\r\n\tMONDAY,\r\n\tTUESDAY;\r\n\tprivate String name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(!source_action(&a));
}

#[test]
fn test_hash_code_equals_quick_assist() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic class A {\r\n\tpublic final String name = \"test\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "A");
    let quick = quick_actions(&a);
    assert!(!quick.is_empty());
    assert!(has_prompt(&quick));
    let a = actions(&mut ws, &uri, source, "String name");
    let quick = quick_actions(&a);
    assert!(!quick.is_empty());
    assert!(!has_prompt(&quick));
}

#[test]
fn test_no_hash_code_equals_quick_assist() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source="package p;\r\n\r\npublic class A {\r\n\tString name;\r\n   public int hashCode() {\r\n\t}\r\n\tpublic boolean equals(Object a) {\r\n\t\treturn true;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "A");
    assert!(
        !a.iter()
            .any(|a| a["kind"] == "quickassist"
                && a["title"] == "Generate hashCode() and equals()...")
    );
}
