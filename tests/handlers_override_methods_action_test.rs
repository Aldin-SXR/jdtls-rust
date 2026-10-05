//! Upstream OverrideMethodsActionTest with unchanged fixture, selections and prompt assertions.
mod common;
use common::jdtls::{test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::json;
#[test]
fn test_override_methods_quick_assist() {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(true);
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    for (token, expected) in [("A", true), ("String name", false)] {
        let response=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}}));
        let actions = response.as_array().expect("actions");
        let quick: Vec<_> = actions
            .iter()
            .filter(|a| a["kind"] == "quickassist")
            .collect();
        assert!(!quick.is_empty());
        assert_eq!(
            expected,
            quick
                .iter()
                .any(|a| a["command"]["command"] == "java.action.overrideMethodsPrompt")
        );
    }
}
