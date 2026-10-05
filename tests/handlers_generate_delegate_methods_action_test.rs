//! All three upstream GenerateDelegateMethodsActionTest methods.
mod common;
use common::jdtls::{test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::json;

#[test]
fn test_generate_delegate_methods_enabled() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,"String name"),"context":{"diagnostics":[]}}));
    let a = actions
        .as_array()
        .expect("code actions")
        .iter()
        .find(|a| a["kind"] == "source.generate.delegateMethods");
    let a = a.expect("delegate source action");
    assert_eq!(
        a["command"]["command"],
        "java.action.generateDelegateMethodsPrompt"
    );
}

#[test]
fn test_generate_delegate_methods_disabled() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,"class A"),"context":{"diagnostics":[]}}));
    let a = actions
        .as_array()
        .expect("code actions")
        .iter()
        .find(|a| a["kind"] == "source.generate.delegateMethods");
    assert!(a.is_none());
}

#[test]
fn test_generate_delegate_methods_disabled_interface() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic interface A {\r\n\tpublic final String name = \"test\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,"String name"),"context":{"diagnostics":[]}}));
    let a = actions
        .as_array()
        .expect("code actions")
        .iter()
        .find(|a| a["kind"] == "source.generate.delegateMethods");
    assert!(a.is_none());
}
