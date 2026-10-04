//! All seven GenerateConstructorsActionTest methods with upstream text blocks,
//! selections, prompt capabilities and generation assertions.
mod common;
use common::jdtls::{apply_edits, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(false);
    ws.init_options["extendedClientCapabilities"]["generateConstructorsPromptSupport"] =
        json!(true);
    ws
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Vec<Value> {
    ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})).as_array().expect("code actions").clone()
}
fn source_action(actions: &[Value]) -> Option<&Value> {
    actions
        .iter()
        .find(|a| a["kind"] == "source.generate.constructors")
}
fn has_prompt(actions: &[Value]) -> bool {
    actions.iter().any(|a| {
        a["kind"] == "quickassist"
            && a["command"]["command"] == "java.action.generateConstructorsPrompt"
    })
}

#[test]
fn test_generate_constructors_enabled() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\n\npublic class A {\n\tString name;\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    let constructor = source_action(&a).expect("constructor action");
    assert_eq!(
        constructor["command"]["command"],
        "java.action.generateConstructorsPrompt"
    );
}

#[test]
fn test_generate_constructors_quick_assist() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\n\npublic class A {\n\tString name;\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(has_prompt(&a));
    let a = actions(&mut ws, &uri, source, "A");
    assert!(has_prompt(&a));
}

#[test]
fn test_generate_constructors_quick_assist_with_all_static_fields() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\n\npublic class A {\n\tstatic String name;\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "static String name");
    let first = a
        .iter()
        .find(|a| a["kind"] == "quickassist")
        .expect("a quick assist");
    assert_ne!(
        first["command"]["command"],
        "java.action.generateConstructorsPrompt"
    );
    let a = actions(&mut ws, &uri, source, "class A");
    assert!(source_action(&a).is_some());
}

#[test]
fn test_generate_constructors_empty_fields() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\n\npublic class A {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "class A");
    assert!(source_action(&a).is_some());
}

#[test]
fn test_generate_constructors_disabled_interface() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\n\npublic interface A {\n\tfinal String name = \"test\";\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "String name");
    assert!(source_action(&a).is_none());
}

#[test]
fn test_generate_constructors_disabled_anonymous() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source="package p;\n\npublic class A {\n\tpublic Runnable getRunnable() {\n\t\treturn new Runnable() {\n\t\t\t@Override\n\t\t\tpublic void run() {\n\t\t\t}\n\t\t};\n\t}\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "run()");
    assert!(source_action(&a).is_none());
}

#[test]
fn test_generate_constructors_with_super_delegation() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    ws.create_cu(
        &root,
        "src",
        "p",
        "A.java",
        "package p;\n\npublic class A {\n\tpublic A() {\n\t}\n\tpublic A(String a) {\n\t}\n}\n",
    );
    let source = "package p;\n\npublic class B extends A {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let a = actions(&mut ws, &uri, source, "class B");
    assert!(source_action(&a).is_some());
    assert!(has_prompt(&a));
    let context = json!({"textDocument":{"uri":uri},"range":get_range(source,"class B"),"context":{"diagnostics":[]}});
    let response = ws.request("java/checkConstructorsStatus", context.clone());
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 2);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 0);
    let edit=ws.request("java/generateConstructors",json!({"context":context,"constructors":response["constructors"],"fields":response["fields"]}));
    assert!(!edit.is_null());
    let actual = apply_edits(source, edit["changes"][&uri].as_array().unwrap());
    assert!(actual.contains("public B() {"));
    assert!(actual.contains("public B(String a) {"));
    assert!(actual.contains("super(a);"));
}
