mod common;
use common::jdtls::*;
use serde_json::{json, Value};

const SOURCE: &str = "package p;\nclass A {\n    boolean test(String value) {\n        return value.equals(\"text\");\n    }\n}\n";
const EXPECTED: &str = "package p;\nclass A {\n    boolean test(String value) {\n        return \"text\".equals(value);\n    }\n}\n";

fn setup(internal: bool, settings: Value, ui: Option<&str>) -> (Workspace, String) {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    ws.settings = settings;
    ws.init_options["extendedClientCapabilities"]["canUseInternalSettings"] = json!(internal);
    if let Some(ui) = ui {
        std::fs::write(root.join(".settings/org.eclipse.jdt.ui.prefs"), ui).unwrap();
    }
    let uri = ws.create_cu(&root, "src", "p", "A.java", SOURCE);
    (ws, uri)
}

fn save(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.open(uri);
    ws.request(
        "textDocument/willSaveWaitUntil",
        json!({"textDocument": {"uri": uri}, "reason": 1}),
    )
}

fn manual(ws: &mut Workspace, uri: &str) -> Value {
    ws.request("java/cleanup", json!({"uri": uri}))["changes"][uri].clone()
}

fn applied(source: &str, edits: &Value) -> String {
    apply_edits(source, edits.as_array().unwrap())
}

#[test]
fn manual_cleanup_is_independent_of_the_save_enable_switch() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}, "saveActions": {"cleanup": false}}}),
        None,
    );
    assert_eq!(json!([]), save(&mut ws, &uri));
    let edits = manual(&mut ws, &uri);
    assert_eq!(1, edits.as_array().unwrap().len());
    assert_eq!(range(0, 0, 6, 0), edits[0]["range"]);
    assert_eq!(EXPECTED, applied(SOURCE, &edits));
    assert_eq!(SOURCE, ws.read(&uri));
    // Neither save nor manual requests apply the edits to the server buffer.
    assert_eq!(EXPECTED, applied(SOURCE, &manual(&mut ws, &uri)));
}

#[test]
fn cleanup_on_save_obeys_configuration_updates() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}}}),
        None,
    );
    assert_eq!(json!([]), save(&mut ws, &uri));
    ws.update_settings(json!({"java": {"saveActions": {"cleanup": true}}}));
    assert_eq!(EXPECTED, applied(SOURCE, &save(&mut ws, &uri)));
    ws.update_settings(json!({"java": {"saveActions": {"cleanup": false}}}));
    assert_eq!(json!([]), save(&mut ws, &uri));
}

#[test]
fn internal_cleanup_preferences_replace_lsp_settings() {
    let ui = "editor_save_participant_org.eclipse.jdt.ui.postsavelistener.cleanup=true\nsp_cleanup.invert_equals=true\n";
    let (mut ws, uri) = setup(
        true,
        json!({"java": {"cleanup": {"actions": []}, "saveActions": {"cleanup": false}}}),
        Some(ui),
    );
    assert_eq!(EXPECTED, applied(SOURCE, &save(&mut ws, &uri)));
    assert_eq!(EXPECTED, applied(SOURCE, &manual(&mut ws, &uri)));
    let (mut ws, uri) = setup(
        true,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}, "saveActions": {"cleanup": true}}}),
        None,
    );
    assert_eq!(json!([]), save(&mut ws, &uri));
    assert_eq!(json!([]), manual(&mut ws, &uri));
}

#[test]
fn internal_cleanup_requires_save_participant_and_ignores_disabled_keys() {
    for ui in ["sp_cleanup.invert_equals=true\n", "editor_save_participant_org.eclipse.jdt.ui.postsavelistener.cleanup=false\nsp_cleanup.invert_equals=true\n", "editor_save_participant_org.eclipse.jdt.ui.postsavelistener.cleanup=true\nsp_cleanup.invert_equals=false\n"] {
        let (mut ws, uri) = setup(true, json!({"java": {"cleanup": {"actions": ["invertEquals"]}, "saveActions": {"cleanup": true}}}), Some(ui));
        assert_eq!(json!([]), save(&mut ws, &uri));
        assert_eq!(json!([]), manual(&mut ws, &uri));
    }
}

#[test]
fn internal_organize_imports_does_not_require_the_cleanup_participant() {
    let (mut ws, uri) = setup(
        true,
        json!({"java": {"saveActions": {"organizeImports": false}}}),
        Some("sp_cleanup.organize_imports=true\n"),
    );
    let source = "package p;\n\nimport java.util.List;\n\nclass A {}\n";
    ws.open_with(&uri, source);
    let edits = ws.request(
        "textDocument/willSaveWaitUntil",
        json!({"textDocument": {"uri": uri}, "reason": 2}),
    );
    assert_eq!("package p;\n\nclass A {}\n", applied(source, &edits));
}

#[test]
fn cleanup_deprecated_setting_and_identifier_aliases_work() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": [], "actionsOnSave": ["cleanup.invert_equals"]}, "saveActions": {"cleanup": true}}}),
        None,
    );
    assert_eq!(EXPECTED, applied(SOURCE, &save(&mut ws, &uri)));
}

#[test]
fn cleanup_composes_reparsed_operations_and_deduplicates_aliases() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals", "cleanup.invert_equals", "organizeImports", "cleanup.organize_imports", "unknownCleanup", "renameFileToType"]}}}),
        None,
    );
    let source = SOURCE.replacen(
        "package p;\n",
        "package p;\n\nimport java.util.List;\n\n",
        1,
    );
    ws.open_with(&uri, &source);
    let edits = manual(&mut ws, &uri);
    assert_eq!(1, edits.as_array().unwrap().len());
    assert_eq!(
        EXPECTED.replacen("package p;\n", "package p;\n\n", 1),
        applied(&source, &edits)
    );
    assert_eq!(SOURCE, ws.read(&uri));
}

#[test]
fn unknown_and_rename_cleanups_return_empty_edits_without_side_effects() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["unknown", "renameFileToType"]}, "saveActions": {"cleanup": true}}}),
        None,
    );
    assert_eq!(json!([]), save(&mut ws, &uri));
    assert_eq!(json!([]), manual(&mut ws, &uri));
    assert_eq!(SOURCE, ws.read(&uri));
}

#[test]
fn save_and_manual_cleanup_support_virtual_working_copies() {
    if is_oracle() {
        return;
    } // Eclipse does not resolve editor-only non-file CUs.
    let (mut ws, _) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}, "saveActions": {"cleanup": true}}}),
        None,
    );
    let absent = ws.external_dir().join("A.java");
    for uri in [
        "untitled:A.java".to_owned(),
        "inmemory://p/A.java".to_owned(),
        common::projects::file_uri(&absent),
    ] {
        ws.open_with(&uri, SOURCE);
        let edits = ws.request(
            "textDocument/willSaveWaitUntil",
            json!({"textDocument": {"uri": uri}, "reason": 1}),
        );
        assert_eq!(EXPECTED, applied(SOURCE, &edits), "{uri}");
        assert_eq!(EXPECTED, applied(SOURCE, &manual(&mut ws, &uri)), "{uri}");
        assert!(!absent.exists());
    }
}

#[test]
fn invert_equals_uses_resolved_signatures_and_non_null_arguments() {
    let source = "package p;\nclass A {\n    enum E { ONE }\n    static final String TEXT = \"text\";\n    boolean equals(String value) { return false; }\n    boolean test(String value, Object object, int number, E e) {\n        boolean a = value.equals(TEXT);\n        boolean b = value.equals(null);\n        boolean c = object.equals(number);\n        boolean d = e.equals(E.ONE);\n        boolean f = (value).equals(\"a\" + number);\n        boolean g = this.equals(\"text\");\n        boolean h = new A().equals(\"text\");\n        boolean i = \"text\".equals(value);\n        boolean j = (\"a\" + value).equals(\"text\");\n        boolean k = object.equals(this);\n        return a;\n    }\n}\n";
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}}}),
        None,
    );
    ws.open_with(&uri, source);
    let expected = source
        .replace("value.equals(TEXT)", "TEXT.equals(value)")
        .replace("e.equals(E.ONE)", "E.ONE.equals(e)")
        .replace(
            "(value).equals(\"a\" + number)",
            "(\"a\" + number).equals(value)",
        )
        .replace("object.equals(this)", "this.equals(object)");
    assert_eq!(expected, applied(source, &manual(&mut ws, &uri)));
}

#[test]
fn save_organize_imports_never_prompts_or_applies_edits() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"saveActions": {"organizeImports": true}}}),
        None,
    );
    ws.init_options["extendedClientCapabilities"]["advancedOrganizeImportsSupport"] = json!(true);
    ws.init_options["extendedClientCapabilities"]["executeClientCommandSupport"] = json!(true);
    let root = ws.project_root("TestProject");
    ws.create_cu(
        &root,
        "src",
        "p1",
        "C.java",
        "package p1; public class C {}",
    );
    ws.create_cu(
        &root,
        "src",
        "p2",
        "C.java",
        "package p2; public class C {}",
    );
    let source = "package p;\n\nclass A { C c; Set<String> s; }\n";
    ws.open_with(&uri, source);
    let edits = ws.request(
        "textDocument/willSaveWaitUntil",
        json!({"textDocument": {"uri": uri}, "reason": 1}),
    );
    assert_eq!(
        "package p;\n\nimport java.util.Set;\n\nclass A { C c; Set<String> s; }\n",
        applied(source, &edits)
    );
    let requests = &ws.client().server_requests;
    assert!(!requests
        .iter()
        .any(|request| request["method"] == "workspace/applyEdit"
            || request["method"] == "workspace/executeClientCommand"
                && request["params"]["command"] == "java.action.organizeImports.chooseImports"));
    assert_eq!(SOURCE, ws.read(&uri));
}

#[test]
fn cleanup_full_document_edits_preserve_crlf_and_utf16_ranges() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}}}),
        None,
    );
    let source = "package p;\r\nclass A {\r\n    // 😀\r\n    boolean test(String value) { return value.equals(\"😀\"); }\r\n}";
    ws.open_with(&uri, source);
    let edits = manual(&mut ws, &uri);
    assert_eq!(1, edits.as_array().unwrap().len());
    assert_eq!(range(0, 0, 4, 1), edits[0]["range"]);
    assert_eq!(
        source.replace("value.equals(\"😀\")", "\"😀\".equals(value)"),
        applied(source, &edits)
    );
}

#[test]
fn cleanup_list_updates_do_not_revive_an_old_deprecated_setting() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actionsOnSave": ["invertEquals"]}, "saveActions": {"cleanup": true}}}),
        None,
    );
    assert_eq!(EXPECTED, applied(SOURCE, &save(&mut ws, &uri)));
    ws.update_settings(json!({"java": {"cleanup": {"actions": []}}}));
    assert_eq!(json!([]), save(&mut ws, &uri));
    assert_eq!(json!([]), manual(&mut ws, &uri));
    ws.update_settings(json!({"java": {"cleanup": {"actionsOnSave": ["invertEquals"]}}}));
    assert_eq!(EXPECTED, applied(SOURCE, &save(&mut ws, &uri)));
    ws.update_settings(json!({"java": {"cleanup": {"actions": ["unknownCleanup"]}}}));
    assert_eq!(json!([]), save(&mut ws, &uri));
}

#[test]
fn enum_constants_are_recognized_only_at_the_original_expression() {
    let (mut ws, uri) = setup(
        false,
        json!({"java": {"cleanup": {"actions": ["invertEquals"]}}}),
        None,
    );
    let source = "package p;\nclass A {\n    enum E { ONE }\n    boolean test(E e) {\n        boolean a = e.equals(E.ONE);\n        boolean b = e.equals((E.ONE));\n        boolean c = e.equals(((E)e).ONE);\n        return a;\n    }\n}\n";
    ws.open_with(&uri, source);
    assert_eq!(
        source.replace("e.equals(E.ONE)", "E.ONE.equals(e)"),
        applied(source, &manual(&mut ws, &uri))
    );
}
