//! All ten upstream HashCodeEqualsHandlerTest methods, with unchanged sources,
//! options, cursor selections, discovery assertions and whole-unit expectations.
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
    ws.request("java/checkHashCodeEqualsStatus", params(uri, source, token))
}
fn generated(
    ws: &mut Workspace,
    uri: &str,
    source: &str,
    status: &Value,
    token: &str,
    regenerate: bool,
) -> String {
    let edit=ws.request("java/generateHashCodeEquals",json!({"context":params(uri,source,token),"fields":status["fields"],"regenerate":regenerate}));
    assert!(!edit.is_null());
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .expect("hashCode/equals edits"),
    )
}

#[test]
fn test_check_hash_code_equals_status() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    assert_eq!(response["type"], "B");
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    assert!(
        response["existingMethods"].is_null()
            || response["existingMethods"].as_array().unwrap().is_empty()
    );
}

#[test]
fn test_check_hash_code_equals_status_methods_exist() {
    let (mut ws, root) = setup();
    let source="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n   public int hashCode() {\r\n\t}\r\n\tpublic boolean equals(Object a) {\r\n\t\treturn true;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    assert_eq!(response["type"], "B");
    assert!(response["fields"].is_array());
    assert_eq!(response["fields"].as_array().unwrap().len(), 2);
    assert_eq!(response["existingMethods"].as_array().unwrap().len(), 2);
}

#[test]
fn test_generate_hash_code_equals() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": false, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + ((name == null) ? 0 : name.hashCode());\r\n\t\tresult = prime * result + id;\r\n\t\tlong temp;\r\n\t\ttemp = Double.doubleToLongBits(rate);\r\n\t\tresult = prime * result + (int) (temp ^ (temp >>> 32));\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + ((aList == null) ? 0 : aList.hashCode());\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\tif (name == null) {\r\n\t\t\tif (other.name != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!name.equals(other.name))\r\n\t\t\treturn false;\r\n\t\tif (id != other.id)\r\n\t\t\treturn false;\r\n\t\tif (Double.doubleToLongBits(rate) != Double.doubleToLongBits(other.rate))\r\n\t\t\treturn false;\r\n\t\tif (!Arrays.deepEquals(anArray, other.anArray))\r\n\t\t\treturn false;\r\n\t\tif (aList == null) {\r\n\t\t\tif (other.aList != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!aList.equals(other.aList))\r\n\t\t\treturn false;\r\n\t\treturn true;\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_use_java7_objects() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": true, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\nimport java.util.Objects;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + Objects.hash(name, id, rate, aList);\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\treturn Objects.equals(name, other.name) && id == other.id && Double.doubleToLongBits(rate) == Double.doubleToLongBits(other.rate) && Arrays.deepEquals(anArray, other.anArray) && Objects.equals(aList, other.aList);\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_use_instanceof() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": false, "java.codeGeneration.hashCodeEquals.useInstanceof": true, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + ((name == null) ? 0 : name.hashCode());\r\n\t\tresult = prime * result + id;\r\n\t\tlong temp;\r\n\t\ttemp = Double.doubleToLongBits(rate);\r\n\t\tresult = prime * result + (int) (temp ^ (temp >>> 32));\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + ((aList == null) ? 0 : aList.hashCode());\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (!(obj instanceof B))\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\tif (name == null) {\r\n\t\t\tif (other.name != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!name.equals(other.name))\r\n\t\t\treturn false;\r\n\t\tif (id != other.id)\r\n\t\t\treturn false;\r\n\t\tif (Double.doubleToLongBits(rate) != Double.doubleToLongBits(other.rate))\r\n\t\t\treturn false;\r\n\t\tif (!Arrays.deepEquals(anArray, other.anArray))\r\n\t\t\treturn false;\r\n\t\tif (aList == null) {\r\n\t\t\tif (other.aList != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!aList.equals(other.aList))\r\n\t\t\treturn false;\r\n\t\treturn true;\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_use_blocks() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": false, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": true, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + ((name == null) ? 0 : name.hashCode());\r\n\t\tresult = prime * result + id;\r\n\t\tlong temp;\r\n\t\ttemp = Double.doubleToLongBits(rate);\r\n\t\tresult = prime * result + (int) (temp ^ (temp >>> 32));\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + ((aList == null) ? 0 : aList.hashCode());\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj) {\r\n\t\t\treturn true;\r\n\t\t}\r\n\t\tif (obj == null) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tif (getClass() != obj.getClass()) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tB other = (B) obj;\r\n\t\tif (name == null) {\r\n\t\t\tif (other.name != null) {\r\n\t\t\t\treturn false;\r\n\t\t\t}\r\n\t\t} else if (!name.equals(other.name)) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tif (id != other.id) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tif (Double.doubleToLongBits(rate) != Double.doubleToLongBits(other.rate)) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tif (!Arrays.deepEquals(anArray, other.anArray)) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\tif (aList == null) {\r\n\t\t\tif (other.aList != null) {\r\n\t\t\t\treturn false;\r\n\t\t\t}\r\n\t\t} else if (!aList.equals(other.aList)) {\r\n\t\t\treturn false;\r\n\t\t}\r\n\t\treturn true;\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_generate_comments() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": false, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": true});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + ((name == null) ? 0 : name.hashCode());\r\n\t\tresult = prime * result + id;\r\n\t\tlong temp;\r\n\t\ttemp = Double.doubleToLongBits(rate);\r\n\t\tresult = prime * result + (int) (temp ^ (temp >>> 32));\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + ((aList == null) ? 0 : aList.hashCode());\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\tif (name == null) {\r\n\t\t\tif (other.name != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!name.equals(other.name))\r\n\t\t\treturn false;\r\n\t\tif (id != other.id)\r\n\t\t\treturn false;\r\n\t\tif (Double.doubleToLongBits(rate) != Double.doubleToLongBits(other.rate))\r\n\t\t\treturn false;\r\n\t\tif (!Arrays.deepEquals(anArray, other.anArray))\r\n\t\t\treturn false;\r\n\t\tif (aList == null) {\r\n\t\t\tif (other.aList != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!aList.equals(other.aList))\r\n\t\t\treturn false;\r\n\t\treturn true;\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_regenerate() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "lastMember", "java.codeGeneration.hashCodeEquals.useJava7Objects": false, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tpublic int hashCode() {\r\n\t\treturn 0;\r\n\t}\r\n\tpublic boolean equals(Object obj) {\r\n\t\treturn false;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + ((name == null) ? 0 : name.hashCode());\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\tif (name == null) {\r\n\t\t\tif (other.name != null)\r\n\t\t\t\treturn false;\r\n\t\t} else if (!name.equals(other.name))\r\n\t\t\treturn false;\r\n\t\treturn true;\r\n\t}\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", true);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_after_cursor() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "afterCursor", "java.codeGeneration.hashCodeEquals.useJava7Objects": true, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\nimport java.util.Objects;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + Objects.hash(name, id, rate, aList);\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\treturn Objects.equals(name, other.name) && id == other.id && Double.doubleToLongBits(rate) == Double.doubleToLongBits(other.rate) && Arrays.deepEquals(anArray, other.anArray) && Objects.equals(aList, other.aList);\r\n\t}\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_hash_code_equals_before_cursor() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java.codeGeneration.insertionLocation": "beforeCursor", "java.codeGeneration.hashCodeEquals.useJava7Objects": true, "java.codeGeneration.hashCodeEquals.useInstanceof": false, "java.codeGeneration.useBlocks": false, "java.codeGeneration.generateComments": false});
    let source="package p;\r\n\r\nimport java.util.List;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let response = status(&mut ws, &uri, source, "String name");
    let expected="package p;\r\n\r\nimport java.util.Arrays;\r\nimport java.util.List;\r\nimport java.util.Objects;\r\n\r\npublic class B {\r\n\tprivate static String UUID = \"23434343\";\r\n\t@Override\r\n\tpublic int hashCode() {\r\n\t\tfinal int prime = 31;\r\n\t\tint result = 1;\r\n\t\tresult = prime * result + Arrays.deepHashCode(anArray);\r\n\t\tresult = prime * result + Objects.hash(name, id, rate, aList);\r\n\t\treturn result;\r\n\t}\r\n\t@Override\r\n\tpublic boolean equals(Object obj) {\r\n\t\tif (this == obj)\r\n\t\t\treturn true;\r\n\t\tif (obj == null)\r\n\t\t\treturn false;\r\n\t\tif (getClass() != obj.getClass())\r\n\t\t\treturn false;\r\n\t\tB other = (B) obj;\r\n\t\treturn Objects.equals(name, other.name) && id == other.id && Double.doubleToLongBits(rate) == Double.doubleToLongBits(other.rate) && Arrays.deepEquals(anArray, other.anArray) && Objects.equals(aList, other.aList);\r\n\t}\r\n\tString name;\r\n\tint id;\r\n\tdouble rate;\r\n\tCloneable[] anArray;\r\n\tList<String> aList;\r\n}";
    let actual = generated(&mut ws, &uri, source, &response, "String name", false);
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}
