//! All eight GenerateToStringHandlerTest methods, retaining upstream fixtures,
//! settings, cursor selections, discovery assertions and whole-unit expectations.
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

fn params(uri: &str, source: &str, token: &str) -> Value {
    json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})
}
fn status(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Value {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    ws.request("java/checkToStringStatus", params(uri, source, token))
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
    let context = json!({"textDocument":{"uri":uri},"range":range,"context":{"diagnostics":[]}});
    let edit = ws.request(
        "java/generateToString",
        json!({"context":context,"fields":status["fields"]}),
    );
    assert!(!edit.is_null());
    apply_edits(
        source,
        edit["changes"][uri].as_array().expect("toString edits"),
    )
}

#[test]
fn test_generate_to_string_status() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    assert_eq!(response["type"], "B");
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 5);
    assert_eq!(response["exists"], false);
}

#[test]
fn test_check_to_string_status_methods_exist() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tpublic String toString() {\r\n\t\treturn \"B[]\";\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    assert_eq!(response["type"], "B");
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 5);
    assert_eq!(response["exists"], true);
}

#[test]
fn test_generate_to_string() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;\r\n\tString[] arrays;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "B");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;\r\n\tString[] arrays;\r\n\t@Override\r\n\tpublic String toString() {\r\n\t\treturn \"B [name=\" + name + \", id=\" + id + \", aList=\" + aList + \", arrays=\" + Arrays.toString(arrays) + \", getClass()=\" + getClass() + \", hashCode()=\" + hashCode() + \", toString()=\" + super.toString() + \"]\";\r\n\t}\r\n}";
    let range = eof(source);
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_to_string_order() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\npublic class B {\r\n\tpublic String stringField;\r\n\tpublic int intField;\r\n\tpublic static int staticIntField;\r\n\tpublic boolean booleanField;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String stringField");
    assert_eq!(response["type"], "B");
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 6);
    assert_eq!(response["exists"], false);
    assert_eq!(response["fields"][0]["name"], "stringField");
    assert_eq!(response["fields"][1]["name"], "intField");
    assert_eq!(response["fields"][2]["name"], "booleanField");
    let expected="package p;\r\n\r\npublic class B {\r\n\tpublic String stringField;\r\n\tpublic int intField;\r\n\tpublic static int staticIntField;\r\n\tpublic boolean booleanField;\r\n\t@Override\r\n\tpublic String toString() {\r\n\t\treturn \"B [stringField=\" + stringField + \", intField=\" + intField + \", booleanField=\" + booleanField + \", getClass()=\" + getClass() + \", hashCode()=\" + hashCode() + \", toString()=\" + super.toString() + \"]\";\r\n\t}\r\n}";
    let range = eof(source);
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_to_string_customized_settings() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.generateComments":true,"java.codeGeneration.useBlocks":true,"java.codeGeneration.toString.codeStyle":"STRING_BUILDER_CHAINED","java.codeGeneration.toString.skipNullValues":true,"java.codeGeneration.toString.listArrayContents":false,"java.codeGeneration.toString.limitElements":10});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;\r\n\tString[] arrays;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "B");
    let expected="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;\r\n\tString[] arrays;\r\n\t@Override\r\n\tpublic String toString() {\r\n\t\tfinal int maxLen = 10;\r\n\t\tStringBuilder builder = new StringBuilder();\r\n\t\tbuilder.append(\"B [\");\r\n\t\tif (name != null) {\r\n\t\t\tbuilder.append(\"name=\").append(name).append(\", \");\r\n\t\t}\r\n\t\tbuilder.append(\"id=\").append(id).append(\", \");\r\n\t\tif (aList != null) {\r\n\t\t\tbuilder.append(\"aList=\").append(aList.subList(0, Math.min(aList.size(), maxLen))).append(\", \");\r\n\t\t}\r\n\t\tif (arrays != null) {\r\n\t\t\tbuilder.append(\"arrays=\").append(arrays).append(\", \");\r\n\t\t}\r\n\t\tif (getClass() != null) {\r\n\t\t\tbuilder.append(\"getClass()=\").append(getClass()).append(\", \");\r\n\t\t}\r\n\t\tbuilder.append(\"hashCode()=\").append(hashCode()).append(\", \");\r\n\t\tif (super.toString() != null) {\r\n\t\t\tbuilder.append(\"toString()=\").append(super.toString());\r\n\t\t}\r\n\t\tbuilder.append(\"]\");\r\n\t\treturn builder.toString();\r\n\t}\r\n}";
    let range = eof(source);
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_to_string_after_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("afterCursor");
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;/*|*/\r\n\tString[] arrays;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "B");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;/*|*/\r\n\t@Override\r\n\tpublic String toString() {\r\n\t\treturn \"B [name=\" + name + \", id=\" + id + \", aList=\" + aList + \", arrays=\" + Arrays.toString(arrays) + \", getClass()=\" + getClass() + \", hashCode()=\" + hashCode() + \", toString()=\" + super.toString() + \"]\";\r\n\t}\r\n\tString[] arrays;\r\n}";
    let range = get_range(source, "/*|*/");
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_to_string_before_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("beforeCursor");
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tList<String> aList;/*|*/\r\n\tString[] arrays;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "B");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\t@Override\r\n\tpublic String toString() {\r\n\t\treturn \"B [name=\" + name + \", id=\" + id + \", aList=\" + aList + \", arrays=\" + Arrays.toString(arrays) + \", getClass()=\" + getClass() + \", hashCode()=\" + hashCode() + \", toString()=\" + super.toString() + \"]\";\r\n\t}\r\n\tList<String> aList;/*|*/\r\n\tString[] arrays;\r\n}";
    let range = get_range(source, "/*|*/");
    let actual = generated(&mut ws, &uri, source, &response, range);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_inherited_fields_and_methods() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    let root = ws.dir.join("eclipse/hello");
    let path = root.join("src/org/sample/Child.java");
    let source = std::fs::read_to_string(&path).unwrap();
    let uri = tower_lsp::lsp_types::Url::from_file_path(path)
        .unwrap()
        .to_string();
    let response = status(&mut ws, &uri, &source, "Child");
    assert_eq!(response["fields"].as_array().unwrap().len(), 5);
    assert_eq!(response["fields"][0]["name"], "name");
    assert_eq!(response["fields"][0]["isField"], true);
    assert_eq!(response["fields"][0]["isSelected"], true);
    assert_eq!(response["fields"][1]["name"], "parentName");
    assert_eq!(response["fields"][1]["isField"], true);
    assert_eq!(response["fields"][1]["isSelected"], false);
    assert_eq!(response["fields"][2]["name"], "getClass");
    assert_eq!(response["fields"][2]["isField"], false);
    assert_eq!(response["fields"][1]["isSelected"], false);
}
