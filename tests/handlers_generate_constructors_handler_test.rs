//! All six GenerateConstructorsHandlerTest methods, preserving sources,
//! selections, visible signatures, fields and complete expected compilation units.
mod common;
use common::jdtls::{apply_edits, fixtures_dir, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::PathBuf;
fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    ws.settings = json!({"java.codeGeneration.generateComments": false});
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

fn params(uri: &str, range: Value) -> Value {
    json!({"textDocument":{"uri":uri},"range":range,"context":{"diagnostics":[]}})
}
fn status(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Value {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    ws.request(
        "java/checkConstructorsStatus",
        params(uri, get_range(source, token)),
    )
}
fn eof(source: &str) -> Value {
    let line = source.bytes().filter(|&b| b == b'\n').count();
    let character = source
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count();
    json!({"start":{"line":line,"character":character},"end":{"line":line,"character":character}})
}
fn generated(ws: &mut Workspace, uri: &str, source: &str, status: &Value, range: Value) -> String {
    let edit=ws.request("java/generateConstructors",json!({"context":params(uri,range),"constructors":status["constructors"],"fields":status["fields"]}));
    assert!(!edit.is_null());
    apply_edits(
        source,
        edit["changes"][uri].as_array().expect("constructor edits"),
    )
}

#[test]
fn test_check_constructor_status() {
    let (mut ws, root) = setup();
    ws.create_cu(&root,"src","p","B.java","package p;\r\n\r\npublic class B {\r\n\tpublic B(String name) {\r\n\t}\r\n\tpublic B(String name, int id) {\r\n\t}\r\n\tprivate B() {\r\n\t}\r\n}");
    let source="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;\r\n\tprivate String address;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    let response = status(&mut ws, &uri, source, "String address");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 2);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    assert_eq!(response["constructors"][0]["name"], "B");
    assert_eq!(response["constructors"][0]["parameters"], json!(["String"]));
    assert_eq!(response["constructors"][1]["name"], "B");
    assert_eq!(
        response["constructors"][1]["parameters"],
        json!(["String", "int"])
    );
    assert_eq!(response["fields"][0]["name"], "instance");
    assert_eq!(response["fields"][0]["type"], "String");
    assert_eq!(response["fields"][0]["isSelected"], false);
    assert_eq!(response["fields"][1]["name"], "address");
    assert_eq!(response["fields"][1]["type"], "String");
    assert_eq!(response["fields"][1]["isSelected"], true);
}

#[test]
fn test_check_constructor_status_enum() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic enum B {\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "enum B");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 1);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 0);
    assert_eq!(response["constructors"][0]["name"], "Object");
    assert!(response["constructors"][0]["parameters"].is_array());
    assert_eq!(
        response["constructors"][0]["parameters"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_generate_constructors() {
    let (mut ws, root) = setup();
    ws.create_cu(&root,"src","p","B.java","package p;\r\n\r\npublic class B {\r\n\tpublic B(String name) {\r\n\t}\r\n\tpublic B(String name, int id) {\r\n\t}\r\n\tprivate B() {\r\n\t}\r\n}");
    let source="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;\r\n\tprivate String address;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    let response = status(&mut ws, &uri, source, "String address");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 2);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    let expected="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;\r\n\tprivate String address;\r\n\tpublic C(String name, String instance, String address) {\r\n\t\tsuper(name);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n\tpublic C(String name, int id, String instance, String address) {\r\n\t\tsuper(name, id);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n}";
    let range = eof(source);
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_constructors_enum() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic enum B {\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "enum B");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 1);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 0);
    let expected = "package p;\r\n\r\npublic enum B {\r\n\t;\r\n\r\n\tprivate B() {\r\n\t}\r\n}";
    let range = eof(source);
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_constructors_after_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("afterCursor");
    ws.create_cu(&root,"src","p","B.java","package p;\r\n\r\npublic class B {\r\n\tpublic B(String name) {\r\n\t}\r\n\tpublic B(String name, int id) {\r\n\t}\r\n\tprivate B() {\r\n\t}\r\n}");
    let source="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;/*|*/\r\n\tprivate String address;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    let response = status(&mut ws, &uri, source, "String address");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 2);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    let expected="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;/*|*/\r\n\tpublic C(String name, String instance, String address) {\r\n\t\tsuper(name);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n\tpublic C(String name, int id, String instance, String address) {\r\n\t\tsuper(name, id);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n\tprivate String address;\r\n}";
    let range = get_range(source, "/*|*/");
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_constructors_before_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("beforeCursor");
    ws.create_cu(&root,"src","p","B.java","package p;\r\n\r\npublic class B {\r\n\tpublic B(String name) {\r\n\t}\r\n\tpublic B(String name, int id) {\r\n\t}\r\n\tprivate B() {\r\n\t}\r\n}");
    let source="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tprivate final String instance;/*|*/\r\n\tprivate String address;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    let response = status(&mut ws, &uri, source, "String address");
    assert!(response["constructors"].is_array());
    assert_eq!(response["constructors"].as_array().unwrap().len(), 2);
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    let expected="package p;\r\n\r\npublic class C extends B {\r\n\tprivate static String logger;\r\n\tprivate final String uuid = \"123\";\r\n\tpublic C(String name, String instance, String address) {\r\n\t\tsuper(name);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n\tpublic C(String name, int id, String instance, String address) {\r\n\t\tsuper(name, id);\r\n\t\tthis.instance = instance;\r\n\t\tthis.address = address;\r\n\t}\r\n\tprivate final String instance;/*|*/\r\n\tprivate String address;\r\n}";
    let range = get_range(source, "/*|*/");
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}
