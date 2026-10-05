//! All five upstream GenerateDelegateMethodsHandlerTest methods, with verbatim fixtures and expectations.
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

#[test]
fn test_check_delegate_methods_status() {
    let (mut ws, root) = setup();
    ws.create_cu(&root,"src","p","B.java", "package p;\r\n\r\npublic class B {\r\n\tprivate String name;\r\n\tpublic String getName() {\r\n\t\treturn this.name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}");
    let source = "package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let response = ws.request(
        "java/checkDelegateMethodsStatus",
        params(&uri, source, "B b;"),
    );
    assert!(response["delegateFields"].is_array());
    assert_eq!(response["delegateFields"].as_array().unwrap().len(), 1);
    let field = &response["delegateFields"][0];
    assert_eq!(field["field"]["name"], "b");
    assert!(field["delegateMethods"].is_array());
    assert_eq!(field["delegateMethods"].as_array().unwrap().len(), 5);
    assert_eq!(field["delegateMethods"][0]["name"], "getName");
    assert_eq!(field["delegateMethods"][1]["name"], "setName");
    for (i, n) in ["equals", "hashCode", "toString"].iter().enumerate() {
        assert_eq!(field["delegateMethods"][i + 2]["name"], *n);
    }
}

#[test]
fn test_check_delegate_methods_status_exclude_exists() {
    let (mut ws, root) = setup();
    ws.create_cu(&root,"src","p","B.java", "package p;\r\n\r\npublic class B {\r\n\tprivate String name;\r\n\tpublic String getName() {\r\n\t\treturn this.name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}");
    let source = "package p;\r\n\r\npublic class C {\r\n\tprivate B b;\r\n\tpublic String getName() {\r\n\t\treturn b.getName();\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let response = ws.request(
        "java/checkDelegateMethodsStatus",
        params(&uri, source, "B b;"),
    );
    assert!(response["delegateFields"].is_array());
    assert_eq!(response["delegateFields"].as_array().unwrap().len(), 1);
    let field = &response["delegateFields"][0];
    assert_eq!(field["field"]["name"], "b");
    assert!(field["delegateMethods"].is_array());
    assert_eq!(field["delegateMethods"].as_array().unwrap().len(), 4);
    assert_eq!(field["delegateMethods"][0]["name"], "setName");
}

#[test]
fn test_generate_delegate_methods() {
    let (mut ws, root) = setup();
    ws.create_cu(&root,"src","p","B.java", "package p;\r\n\r\npublic class B {\r\n\tprivate String name;\r\n\tpublic String getName() {\r\n\t\treturn this.name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}");
    let source = "package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let response = ws.request(
        "java/checkDelegateMethodsStatus",
        params(&uri, source, "B b;"),
    );
    assert!(response["delegateFields"].is_array());
    assert_eq!(response["delegateFields"].as_array().unwrap().len(), 1);
    let field = &response["delegateFields"][0];
    assert_eq!(field["field"]["name"], "b");
    assert!(field["delegateMethods"].is_array());
    assert_eq!(field["delegateMethods"].as_array().unwrap().len(), 5);
    assert_eq!(field["delegateMethods"][0]["name"], "getName");
    assert_eq!(field["delegateMethods"][1]["name"], "setName");
    let entries = json!([{"field":field["field"],"delegateMethod":field["delegateMethods"][0]},{"field":field["field"],"delegateMethod":field["delegateMethods"][1]}]);
    // A zero-width EOF range exercises the public endpoint's append path,
    // corresponding to the helper's null cursor in the upstream test.
    let mut context = params(&uri, source, "B b;");
    let line = source.lines().count() - 1;
    let character = source.lines().last().unwrap().encode_utf16().count();
    context["range"] = json!({"start":{"line":line,"character":character},"end":{"line":line,"character":character}});
    let edit = ws.request(
        "java/generateDelegateMethods",
        json!({"context":context,"delegateEntries":entries}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("delegate edits"),
    );
    let expected="package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;\r\n\tpublic String getName() {\r\n\t\treturn b.getName();\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tb.setName(name);\r\n\t}\r\n}";
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_delegate_methods_after_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("afterCursor");
    ws.create_cu(&root,"src","p","B.java", "package p;\r\n\r\npublic class B {\r\n\tprivate String name;\r\n\tpublic String getName() {\r\n\t\treturn this.name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}");
    let source = "package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;/*|*/\r\n\tpublic C(B b) {\r\n\t\tthis.b = b;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let response = ws.request(
        "java/checkDelegateMethodsStatus",
        params(&uri, source, "B b;"),
    );
    assert!(response["delegateFields"].is_array());
    assert_eq!(response["delegateFields"].as_array().unwrap().len(), 1);
    let field = &response["delegateFields"][0];
    assert_eq!(field["field"]["name"], "b");
    assert!(field["delegateMethods"].is_array());
    assert_eq!(field["delegateMethods"].as_array().unwrap().len(), 5);
    assert_eq!(field["delegateMethods"][0]["name"], "getName");
    assert_eq!(field["delegateMethods"][1]["name"], "setName");
    let entries = json!([{"field":field["field"],"delegateMethod":field["delegateMethods"][0]},{"field":field["field"],"delegateMethod":field["delegateMethods"][1]}]);
    let context = params(&uri, source, "/*|*/");
    let edit = ws.request(
        "java/generateDelegateMethods",
        json!({"context":context,"delegateEntries":entries}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("delegate edits"),
    );
    let expected="package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;/*|*/\r\n\tpublic String getName() {\r\n\t\treturn b.getName();\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tb.setName(name);\r\n\t}\r\n\tpublic C(B b) {\r\n\t\tthis.b = b;\r\n\t}\r\n}";
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_delegate_methods_before_cursor_position() {
    let (mut ws, root) = setup();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("beforeCursor");
    ws.create_cu(&root,"src","p","B.java", "package p;\r\n\r\npublic class B {\r\n\tprivate String name;\r\n\tpublic String getName() {\r\n\t\treturn this.name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}");
    let source = "package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tprivate B b;/*|*/\r\n\tpublic C(B b) {\r\n\t\tthis.b = b;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "C.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let response = ws.request(
        "java/checkDelegateMethodsStatus",
        params(&uri, source, "B b;"),
    );
    assert!(response["delegateFields"].is_array());
    assert_eq!(response["delegateFields"].as_array().unwrap().len(), 1);
    let field = &response["delegateFields"][0];
    assert_eq!(field["field"]["name"], "b");
    assert!(field["delegateMethods"].is_array());
    assert_eq!(field["delegateMethods"].as_array().unwrap().len(), 5);
    assert_eq!(field["delegateMethods"][0]["name"], "getName");
    assert_eq!(field["delegateMethods"][1]["name"], "setName");
    let entries = json!([{"field":field["field"],"delegateMethod":field["delegateMethods"][0]},{"field":field["field"],"delegateMethod":field["delegateMethods"][1]}]);
    let context = params(&uri, source, "/*|*/");
    let edit = ws.request(
        "java/generateDelegateMethods",
        json!({"context":context,"delegateEntries":entries}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("delegate edits"),
    );
    let expected="package p;\r\n\r\npublic class C {\r\n\tprivate int id;\r\n\tprivate int B[] array;\r\n\tpublic String getName() {\r\n\t\treturn b.getName();\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tb.setName(name);\r\n\t}\r\n\tprivate B b;/*|*/\r\n\tpublic C(B b) {\r\n\t\tthis.b = b;\r\n\t}\r\n}";
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}
