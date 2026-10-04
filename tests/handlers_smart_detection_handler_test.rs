//! Faithful ports of both methods in JDT LS SmartDetectionHandlerTest.
//! The fake Java 21 test VM matches AbstractSourceTestCase; enabling the
//! preference and opening the CU exercise the same handler via its delegate.
mod common;
use common::jdtls::{fixtures_dir, test_default_options, Workspace};
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    ws.settings = json!({"java.edit.smartSemicolonDetection.enabled": true});
    let mut options = test_default_options();
    for key in [
        "org.eclipse.jdt.core.compiler.source",
        "org.eclipse.jdt.core.compiler.compliance",
        "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
    ] {
        options.insert(key.into(), "21".into());
    }
    for (key, value) in [
        ("tabulation.char", "tab"),
        ("tabulation.size", "4"),
        ("lineSplit", "999"),
        ("blank_lines_before_field", "1"),
        ("blank_lines_before_method", "1"),
    ] {
        options.insert(
            format!("org.eclipse.jdt.core.formatter.{key}"),
            value.into(),
        );
    }
    let root = ws.new_empty_project(&options);
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::copy(
        fixtures_dir().join("fakejdk/21/rtstubs.jar"),
        root.join("lib/rtstubs.jar"),
    )
    .unwrap();
    std::fs::write(root.join(".classpath"), r#"<classpath><classpathentry kind="src" path="src"/><classpathentry kind="lib" path="lib/rtstubs.jar"/><classpathentry kind="output" path="bin"/></classpath>"#).unwrap();
    (ws, root)
}

fn detect(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> serde_json::Value {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    let params = json!({"uri": uri, "position": {"line": line, "character": character}});
    ws.request(
        "workspace/executeCommand",
        json!({"command": "java.edit.smartSemicolonDetection", "arguments": [params.to_string()]}),
    )
}

#[test]
fn test_smart_semicolon_detection() {
    let (mut ws, root) = setup();
    let uri = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String str = new String()\n}\n",
    );
    assert_eq!(
        detect(&mut ws, &uri, 2, 33),
        json!({"uri": uri, "position": {"line": 2, "character": 34}})
    );
}

#[test]
fn test_smart_semicolon_detection_in_javadoc() {
    let (mut ws, root) = setup();
    let uri = ws.create_cu(&root, "src", "test", "A.java",
        "package test;\n/**\n * new String()\n */\npublic class A {\n\tprivate String str = new String()\n}\n");
    assert!(detect(&mut ws, &uri, 2, 14).is_null());
}
