//! All seven GenerateAccessorsHandlerTest methods, preserving their source,
//! selected fields, insertion preferences and expected class text. Project
//! templates install AbstractSourceTestCase's template patterns through JDT's
//! ProjectTemplateStore, so the public protocol matches its direct calls.
mod common;
use common::jdtls::{apply_edits, fixtures_dir, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    ws.settings = json!({"java.codeGeneration.generateComments": true});
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
    install_templates(&root);
    (ws, root)
}

fn install_templates(root: &Path) {
    let templates = [
        (
            "gettercomment",
            "gettercomment_context",
            "/**\r\n * @return Returns the ${bare_field_name}.\r\n */",
        ),
        (
            "settercomment",
            "settercomment_context",
            "/**\r\n * @param ${param} The ${bare_field_name} to set.\r\n */",
        ),
        ("getterbody", "getterbody_context", "return ${field};"),
        ("setterbody", "setterbody_context", "${field} = ${param};"),
    ];
    let mut xml = String::from("<templates>");
    for (id, context, pattern) in templates {
        xml.push_str(&format!("<template id=\"org.eclipse.jdt.ui.text.codetemplates.{id}\" name=\"{id}\" description=\"{id}\" context=\"{context}\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{pattern}</template>"));
    }
    xml.push_str("</templates>");
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!(
            "eclipse.preferences.version=1\norg.eclipse.jdt.ui.text.custom_code_templates={}\n",
            xml.replace('\r', "\\r").replace('\n', "\\n")
        ),
    )
    .unwrap();
}
fn params(uri: &str, selection: Value) -> Value {
    json!({"textDocument": {"uri": uri}, "range": selection, "context": {"diagnostics": []}})
}
fn resolve(ws: &mut Workspace, uri: &str, source: &str, kind: &str) -> Value {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    let mut p = params(uri, get_range(source, "class B"));
    p["kind"] = json!(kind);
    ws.request("java/resolveUnimplementedAccessors", p)
}
fn field(name: &str, stat: bool, get: bool, set: bool, ty: &str) -> Value {
    json!({"fieldName": name, "isStatic": stat, "generateGetter": get, "generateSetter": set, "typeName": ty})
}

#[test]
fn test_resolve_unimplemented_accessors() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;\r\n\tString name;\r\n\tList<String> names;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let actual = resolve(&mut ws, &uri, source, "BOTH");
    assert_eq!(
        actual,
        json!([
            field("staticField", true, true, true, "String"),
            field("finalField", false, true, false, "String"),
            field("name", false, true, true, "String"),
            field("names", false, true, true, "List<String>")
        ])
    );
}

#[test]
fn test_resolve_unimplemented_getters() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;\r\n\tString name;\r\n\tpublic String getName() { return this.name; }}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let actual = resolve(&mut ws, &uri, source, "GETTER");
    assert_eq!(
        actual,
        json!([
            field("staticField", true, true, false, "String"),
            field("finalField", false, true, false, "String")
        ])
    );
}

#[test]
fn test_resolve_unimplemented_setters() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;\r\n\tString name;\r\n\tpublic String getName() { return this.name; }}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let actual = resolve(&mut ws, &uri, source, "SETTER");
    assert_eq!(
        actual,
        json!([
            field("staticField", true, false, true, "String"),
            field("name", false, false, true, "String")
        ])
    );
}

#[test]
fn test_resolve_unimplemented_accessors_methods_exist() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tString name;\r\n\tint id;\r\n\tpublic String getName() {\r\n\t\treturn name;\r\n\t}\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n\tpublic int getId() {\r\n\t\treturn id;\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    let actual = resolve(&mut ws, &uri, source, "BOTH");
    assert_eq!(actual, json!([field("id", false, false, true, "int")]));
}

#[test]
fn test_generate_accessors() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.settings["java.codeGeneration.insertionLocation"] = json!("lastMember");
    let accessors = resolve(&mut ws, &uri, source, "BOTH");
    let edit = ws.request(
        "java/generateAccessors",
        json!({"context": params(&uri, get_range(source, "class B")), "accessors": accessors}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("accessor edit"),
    );
    let expected = "public class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;\r\n\tString name;\r\n\t/**\r\n\t * @return Returns the staticField.\r\n\t */\r\n\tpublic static String getStaticField() {\r\n\t\treturn staticField;\r\n\t}\r\n\t/**\r\n\t * @param staticField The staticField to set.\r\n\t */\r\n\tpublic static void setStaticField(String staticField) {\r\n\t\tB.staticField = staticField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the finalField.\r\n\t */\r\n\tpublic String getFinalField() {\r\n\t\treturn finalField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the name.\r\n\t */\r\n\tpublic String getName() {\r\n\t\treturn name;\r\n\t}\r\n\t/**\r\n\t * @param name The name to set.\r\n\t */\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n}";
    let actual = &actual[actual.find("public class B").unwrap()..];
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_accessors_after_cursor_position() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;/*|*/\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.settings["java.codeGeneration.insertionLocation"] = json!("afterCursor");
    let accessors = resolve(&mut ws, &uri, source, "BOTH");
    let edit = ws.request(
        "java/generateAccessors",
        json!({"context": params(&uri, get_range(source, "/*|*/")), "accessors": accessors}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("accessor edit"),
    );
    let expected = "public class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;/*|*/\r\n\t/**\r\n\t * @return Returns the staticField.\r\n\t */\r\n\tpublic static String getStaticField() {\r\n\t\treturn staticField;\r\n\t}\r\n\t/**\r\n\t * @param staticField The staticField to set.\r\n\t */\r\n\tpublic static void setStaticField(String staticField) {\r\n\t\tB.staticField = staticField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the finalField.\r\n\t */\r\n\tpublic String getFinalField() {\r\n\t\treturn finalField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the name.\r\n\t */\r\n\tpublic String getName() {\r\n\t\treturn name;\r\n\t}\r\n\t/**\r\n\t * @param name The name to set.\r\n\t */\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n\tString name;\r\n}";
    let actual = &actual[actual.find("public class B").unwrap()..];
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_accessors_before_cursor_position() {
    let (mut ws, root) = setup();
    let source = "package p;\r\n\r\npublic class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\tprivate final String finalField;/*|*/\r\n\tString name;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.settings["java.codeGeneration.insertionLocation"] = json!("beforeCursor");
    let accessors = resolve(&mut ws, &uri, source, "BOTH");
    let edit = ws.request(
        "java/generateAccessors",
        json!({"context": params(&uri, get_range(source, "/*|*/")), "accessors": accessors}),
    );
    assert!(!edit.is_null());
    let actual = apply_edits(
        source,
        edit["changes"][&uri].as_array().expect("accessor edit"),
    );
    let expected = "public class B {\r\n\tprivate static String staticField = \"23434343\";\r\n\t/**\r\n\t * @return Returns the staticField.\r\n\t */\r\n\tpublic static String getStaticField() {\r\n\t\treturn staticField;\r\n\t}\r\n\t/**\r\n\t * @param staticField The staticField to set.\r\n\t */\r\n\tpublic static void setStaticField(String staticField) {\r\n\t\tB.staticField = staticField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the finalField.\r\n\t */\r\n\tpublic String getFinalField() {\r\n\t\treturn finalField;\r\n\t}\r\n\t/**\r\n\t * @return Returns the name.\r\n\t */\r\n\tpublic String getName() {\r\n\t\treturn name;\r\n\t}\r\n\t/**\r\n\t * @param name The name to set.\r\n\t */\r\n\tpublic void setName(String name) {\r\n\t\tthis.name = name;\r\n\t}\r\n\tprivate final String finalField;/*|*/\r\n\tString name;\r\n}";
    let actual = &actual[actual.find("public class B").unwrap()..];
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}
