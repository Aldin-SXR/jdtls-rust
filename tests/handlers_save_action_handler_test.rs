//! Original SaveActionHandlerTest fixtures, sources and expected results.
mod common;
use common::jdtls::*;
use common::projects::file_uri;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.use_upstream_test_jdk("hello");
    ws.init_options["extendedClientCapabilities"]["canUseInternalSettings"] = json!(false);
    ws.settings = json!({"java": {"saveActions": {"organizeImports": true}}});
    ws
}

fn will_save(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.open(uri);
    ws.request(
        "textDocument/willSaveWaitUntil",
        json!({"textDocument": {"uri": uri}, "reason": 1}),
    )
}

#[test]
fn test_will_save_wait_until() {
    let mut ws = setup();
    let uri = file_uri(&ws.project_root("hello").join("src/java/Foo4.java"));
    let source = ws.read(&uri);
    let edits = will_save(&mut ws, &uri);
    assert_eq!(
        "package java;\n\npublic class Foo4 {\n}\n",
        dos2unix(&apply_edits(&source, edits.as_array().unwrap()))
    );
}

#[test]
fn test_static_will_save_wait_until() {
    let mut ws = setup();
    let favorites = json!({"java": {"completion": {"favoriteStaticMembers": ["java.lang.Math.*", "java.util.stream.Collectors.*"]}}});
    ws.settings["java"]["completion"] = favorites["java"]["completion"].clone();
    ws.client();
    ws.update_settings(favorites);
    let uri = file_uri(&ws.project_root("hello").join("src/org/sample/Foo6.java"));
    let source = ws.read(&uri);
    let edits = will_save(&mut ws, &uri);
    let expected = "package org.sample;\n\nimport static java.lang.Math.PI;\nimport static java.lang.Math.abs;\nimport static java.util.stream.Collectors.toList;\n\nimport java.util.List;\n\npublic class Foo6 {\n    List list = List.of(1).stream().collect(toList());\n    double i = abs(-1);\n    double pi = PI;\n}\n";
    assert_eq!(
        expected,
        dos2unix(&apply_edits(&source, edits.as_array().unwrap()))
    );
}

#[test]
fn test_missing_formatter_url() {
    let mut ws = setup();
    ws.settings["java"]["format"] = json!({"settings": {"url": "xxxx"}});
    let path = ws.project_root("hello").join("src/java/Foo4.java");
    ws.notify_file_changed(&path, 2);
    // The original assertion is that handling this file change does not throw.
    // A completed build/request barrier also detects a crashed wire server.
    ws.wait_for_background_jobs();
}

#[test]
fn test_no_conflict_between_lsp_and_jdtui() {
    let mut ws = setup();
    ws.settings["java"]["cleanup"] = json!({"actions": ["invertEquals"]});
    ws.settings["java"]["saveActions"]["cleanup"] = json!(true);
    let root = ws.project_root("hello");
    std::fs::write(root.join(".settings/org.eclipse.jdt.ui.prefs"), "editor_save_participant_org.eclipse.jdt.ui.postsavelistener.cleanup=true\nsp_cleanup.make_variable_declarations_final=true").unwrap();
    let source = "package test1;\npublic class NoConflictWithLSP {\n    public void test() {\n        String MESSAGE = \"This is a message.\";\n        if (MESSAGE.equals(\"message\"))\n        }\n    }\n}\n";
    let uri = ws.create_cu(&root, "src", "test1", "NoConflictWithLSP.java", source);
    let edits = will_save(&mut ws, &uri);
    let expected = "package test1;\npublic class NoConflictWithLSP {\n    public void test() {\n        String MESSAGE = \"This is a message.\";\n        if (\"message\".equals(MESSAGE))\n        }\n    }\n}\n";
    assert_eq!(expected, apply_edits(source, edits.as_array().unwrap()));
}
