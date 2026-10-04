//! All eleven GenerateAccessorsActionTest methods, using the original sources,
//! selection strings, client capability and assertions.
mod common;
use common::jdtls::{test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    // The upstream ClientPreferences mock returns false for resolve support.
    ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(false);
    ws.init_options["extendedClientCapabilities"]["advancedGenerateAccessorsSupport"] = json!(true);
    ws
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Vec<Value> {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    ws.request("textDocument/codeAction", json!({"textDocument": {"uri": uri}, "range": get_range(source, token), "context": {"diagnostics": []}})).as_array().expect("code actions").clone()
}
fn accessors(actions: &[Value]) -> Option<&Value> {
    actions
        .iter()
        .find(|a| a["kind"] == "source.generate.accessors")
}
fn has(actions: &[Value], title: &str) -> bool {
    actions
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == title)
}

#[test]
fn test_generate_accessors_enabled() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "String name");
    let action = accessors(&actual).expect("accessor action");
    assert!(action["edit"].is_object());
}

#[test]
fn test_advanced_generate_accessors_enabled() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic class A {\r\n\tstatic String name;\r\n\tString address;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "String name");
    let action = accessors(&actual).expect("accessor action");
    assert_eq!(
        action["command"]["command"],
        "java.action.generateAccessorsPrompt"
    );
}

#[test]
fn test_generate_accessors_disabled_empty_fields() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "class A");
    assert!(accessors(&actual).is_none());
}

#[test]
fn test_generate_accessors_disabled_interface() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic interface A {\r\n\tpublic final String name = \"test\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "String name");
    assert!(accessors(&actual).is_none());
}

#[test]
fn test_generate_accessors_for_record_enabled() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/java16"]);
    let root = ws.project_root("java16");
    let source = "package p;\r\n\r\npublic record A(String name, int age) {\r\n}";
    let uri = ws.create_cu(&root, "src/main/java", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "A");
    let action = accessors(&actual).expect("accessor action");
    assert_eq!(
        action["command"]["command"],
        "java.action.generateAccessorsPrompt"
    );
}

#[test]
fn test_generate_accessors_quick_assist_for_type_declaration() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "A");
    assert!(has(&actual, "Generate Getters and Setters"));
    assert!(has(&actual, "Generate Getters"));
    assert!(has(&actual, "Generate Setters"));
}

#[test]
fn test_generate_accessors_quick_assist_for_field_declaration() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "String name");
    assert!(has(&actual, "Generate Getter and Setter for 'name'"));
    assert!(has(&actual, "Generate Getter for 'name'"));
    assert!(has(&actual, "Generate Setter for 'name'"));
}

#[test]
fn test_generate_accessors_quick_assist_at_line() {
    let mut ws = setup();
    ws.settings["java.quickfix.showAt"] = json!("line");
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "public S");
    assert!(has(&actual, "Generate Getter and Setter for 'name'"));
    assert!(has(&actual, "Generate Getter for 'name'"));
    assert!(has(&actual, "Generate Setter for 'name'"));
}

#[test]
fn test_generate_accessors_quick_assist_at_line1() {
    let mut ws = setup();
    ws.settings["java.quickfix.showAt"] = json!("line");
    let root = ws.new_empty_project(&test_default_options());
    let source =
        "package p;\r\n\r\npublic class A {\r\n\tpublic java.lang.String name = \"name\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, ".lang.");
    assert!(has(&actual, "Generate Getter and Setter for 'name'"));
    assert!(has(&actual, "Generate Getter for 'name'"));
    assert!(has(&actual, "Generate Setter for 'name'"));
}

#[test]
fn test_generate_accessors_quick_assist_for_multiple_field_declaration() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(
        &mut ws,
        &uri,
        source,
        "String name = \"name\";\r\n\tpublic String pet = \"pet\";",
    );
    assert!(has(&actual, "Generate Getters and Setters"));
    assert!(has(&actual, "Generate Getters"));
    assert!(has(&actual, "Generate Setters"));
}

#[test]
fn test_generate_accessors_quick_assist_for_final_field() {
    let mut ws = setup();
    let root = ws.new_empty_project(&test_default_options());
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic final String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    let actual = actions(&mut ws, &uri, source, "String name");
    assert!(!has(&actual, "Generate Getter and Setter for 'name'"));
    assert!(has(&actual, "Generate Getter for 'name'"));
    assert!(!has(&actual, "Generate Setter for 'name'"));
}
