//! All ten upstream OverrideMethodsTestCase methods, through the public list/add endpoints.
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

fn base() -> (Workspace, PathBuf) {
    let (mut ws, root) = setup();
    for (file,body) in [
        ("A.java","public abstract class A {\npublic abstract void a();\npublic abstract void b(java.util.Vector<java.util.Date> v);\n}\n"),
        ("B.java","public interface B {\nvoid c(java.util.Hashtable h);\n}\n"),
        ("C.java","public abstract class C {\npublic void c(java.util.Hashtable h) {\n}\npublic abstract java.util.Enumeration d(java.util.Hashtable h) {\n}\n}\n"),
        ("D.java","public abstract class D extends C {\npublic abstract void c(java.util.Hashtable h);\n}\n"),
        ("E.java","public interface E {\nvoid c(java.util.Hashtable h);\nvoid e() throws java.util.NoSuchElementException;\n}\n"),
    ] { ws.create_cu(&root,"src","p",file,&format!("package p;\n\n{body}")); }
    (ws, root)
}
fn params(uri: &str, source: &str, token: &str) -> Value {
    json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})
}
fn list(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Value {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    ws.request("java/listOverridableMethods", params(uri, source, token))
}
fn check(response: &Value, expected: &[&str], unimplemented: Option<bool>, absent: bool) {
    let methods = response["methods"].as_array().unwrap();
    let signatures: Vec<_> = methods
        .iter()
        .filter(|m| unimplemented.is_none_or(|u| m["unimplemented"] == u))
        .map(|m| {
            format!(
                "{}({})",
                m["name"].as_str().unwrap(),
                m["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect();
    for e in expected {
        assert_eq!(
            !absent,
            signatures.contains(&e.to_string()),
            "expected {e} absent={absent}: {response}"
        );
    }
}
fn generate(
    ws: &mut Workspace,
    uri: &str,
    source: &str,
    token: Option<&str>,
    methods: Value,
) -> String {
    let mut context = params(uri, source, "public class");
    if let Some(token) = token {
        context["range"] = get_range(source, token);
    } else {
        // The direct operation's null cursor appends to the requested type.
        // Select its name so the public handler takes that same append path,
        // including units with several top-level types.
        let name = source
            .split_once("public class ")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        let mut range = get_range(source, &format!("public class {name}"));
        range["start"] = range["end"].clone();
        range["start"]["character"] =
            json!(range["end"]["character"].as_u64().unwrap() - name.encode_utf16().count() as u64);
        context["range"] = range;
    }
    let edit = ws.request(
        "java/addOverridableMethods",
        json!({"context":context,"overridableMethods":methods}),
    );
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .unwrap_or_else(|| panic!("overridable edits: {edit}")),
    )
}
fn check_result(
    ws: &mut Workspace,
    uri: &str,
    source: &str,
    name: &str,
    expected: &[&str],
    imports: Option<&[&str]>,
) {
    ws.change(uri, source);
    let symbols = ws.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    let top = symbols
        .as_array()
        .unwrap_or_else(|| panic!("{symbols}: {source}"))
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("{symbols}"));
    let actual: Vec<_> = top["children"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == 6)
        .map(|s| s["name"].as_str().unwrap().split('(').next().unwrap())
        .collect();
    assert_eq!(actual.len(), expected.len(), "{symbols}");
    for e in expected {
        assert!(actual.contains(e), "method {e}: {symbols}");
    }
    let Some(imports) = imports else {
        return;
    };
    let actual: Vec<_> = source
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("import ")
                .and_then(|l| l.strip_suffix(';'))
        })
        .collect();
    assert_eq!(actual.len(), imports.len(), "{source}");
    for i in imports {
        assert!(actual.contains(i), "import {i}: {source}");
    }
}

#[test]
fn test1() {
    let (mut ws, root) = base();
    let source = "package p;\n\npublic class Test1 extends A implements B {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test1.java", source);
    let response = list(&mut ws, &uri, source, "class Test1");
    check(
        &response,
        &["a()", "b(Vector<Date>)", "c(Hashtable)"],
        Some(true),
        false,
    );
    let edited = generate(&mut ws, &uri, source, None, response["methods"].clone());
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test1",
        &[
            "a", "b", "c", "equals", "clone", "toString", "finalize", "hashCode",
        ],
        Some(&["java.util.Date", "java.util.Hashtable", "java.util.Vector"]),
    );
}

#[test]
fn test2() {
    let (mut ws, root) = base();
    let source = "package p;\n\npublic class Test2 extends C implements B {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test2.java", source);
    let response = list(&mut ws, &uri, source, "class Test2");
    check(&response, &["d(Hashtable)"], Some(true), false);
    check(&response, &["c(Hashtable)"], Some(false), false);
    let edited = generate(&mut ws, &uri, source, None, response["methods"].clone());
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test2",
        &[
            "c", "d", "equals", "clone", "toString", "finalize", "hashCode",
        ],
        Some(&["java.util.Enumeration", "java.util.Hashtable"]),
    );
}

#[test]
fn test3() {
    let (mut ws, root) = base();
    let source = "package p;\n\npublic class Test3 extends D {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test3.java", source);
    let response = list(&mut ws, &uri, source, "class Test3");
    check(
        &response,
        &["c(Hashtable)", "d(Hashtable)"],
        Some(true),
        false,
    );
    let edited = generate(&mut ws, &uri, source, None, response["methods"].clone());
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test3",
        &[
            "c", "d", "equals", "clone", "toString", "finalize", "hashCode",
        ],
        Some(&["java.util.Hashtable", "java.util.Enumeration"]),
    );
}

#[test]
fn test4() {
    let (mut ws, root) = base();
    let source = "package p;\n\npublic class Test4 implements B, E {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test4.java", source);
    let response = list(&mut ws, &uri, source, "class Test4");
    check(&response, &["c(Hashtable)", "e()"], Some(true), false);
    let edited = generate(&mut ws, &uri, source, None, response["methods"].clone());
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test4",
        &[
            "c", "e", "equals", "clone", "toString", "finalize", "hashCode",
        ],
        Some(&["java.util.Hashtable", "java.util.NoSuchElementException"]),
    );
}

#[test]
fn test_cloneable() {
    let (mut ws, root) = base();
    let source = "package p;\n\npublic class Test4 implements Cloneable {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test4.java", source);
    let response = list(&mut ws, &uri, source, "class Test4");
    check(&response, &["clone()"], Some(true), false);
    let edited = generate(&mut ws, &uri, source, None, response["methods"].clone());
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test4",
        &["equals", "clone", "toString", "finalize", "hashCode"],
        Some(&[]),
    );
}

#[test]
fn test_bug119171() {
    let (mut ws, root) = base();
    ws.create_cu(&root,"src","p","F.java","package p;\nimport java.util.Properties;\npublic interface F {\n    public void b(Properties p);\n}\n");
    ws.create_cu(
        &root,
        "src",
        "p",
        "Properties.java",
        "package p;\npublic class Properties {\n    public int get() {return 0;}\n}\n",
    );
    let source="package p;\n\npublic class Test5 implements F {\n    public void foo() {\n        Properties p= new Properties();\n        p.get();\n    }\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test5.java", source);
    let response = list(&mut ws, &uri, source, "class Test5");
    let selected = response["methods"].clone();
    let edited = generate(&mut ws, &uri, source, None, selected);
    check_result(
        &mut ws,
        &uri,
        &edited,
        "Test5",
        &[
            "foo", "b", "clone", "equals", "finalize", "hashCode", "toString",
        ],
        Some(&[]),
    );
}

#[test]
fn test_bug297183() {
    let (mut ws, root) = base();
    ws.create_cu(&root,"src","p","Shape.java","package p;\ninterface Shape {\r\n  int getX();\r\n  int getY();\r\n  int getEdges();\r\n  int getArea();\r\n}\r\n");
    ws.create_cu(
        &root,
        "src",
        "p",
        "Circle.java",
        "package p;\ninterface Circle extends Shape {\r\n  int getR();\r\n}\r\n\r\n",
    );
    let source = "package p;\n\npublic class DefaultCircle implements Circle {\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "DefaultCircle.java", source);
    let response = list(&mut ws, &uri, source, "class DefaultCircle");
    check(
        &response,
        &["getX()", "getY()", "getEdges()", "getArea()", "getR()"],
        Some(true),
        false,
    );
    let selected = json!(response["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["unimplemented"] == true)
        .cloned()
        .collect::<Vec<_>>());
    let edited = generate(&mut ws, &uri, source, None, selected);
    check_result(
        &mut ws,
        &uri,
        &edited,
        "DefaultCircle",
        &["getX", "getY", "getEdges", "getArea", "getR"],
        Some(&[]),
    );
}

#[test]
fn test_bug480682() {
    let (mut ws, root) = base();
    let source="public class Test480682 extends Base {\n}\nabstract class Base implements I {\n    @Override\n    public final void method1() {}\n}\ninterface I {\n    void method1();\n    void method2();\n}\n";
    let uri = ws.create_cu(&root, "src", "p", "Test480682.java", source);
    let response = list(&mut ws, &uri, source, "class Test480682");
    check(&response, &["method2()"], Some(true), false);
    check(&response, &["method1()"], None, true);
    let selected = json!(response["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["unimplemented"] == true)
        .cloned()
        .collect::<Vec<_>>());
    let edited = generate(&mut ws, &uri, source, None, selected);
    check_result(&mut ws, &uri, &edited, "Test480682", &["method2"], None);
}

#[test]
fn test_generate_after_cursor_position() {
    let (mut ws, root) = base();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("afterCursor");
    let source="package p;\r\n\r\npublic class Test implements Cloneable {\r\n\tfinal String field1 = null;/*|*/\r\n\tpublic Test() {\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "Test.java", source);
    let response = list(&mut ws, &uri, source, "class Test");
    check(&response, &["clone()"], Some(true), false);
    let selected = json!(response["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["name"] == "clone")
        .cloned()
        .collect::<Vec<_>>());
    let edited = generate(&mut ws, &uri, source, Some("/*|*/"), selected);
    let expected="package p;\r\n\r\npublic class Test implements Cloneable {\r\n\tfinal String field1 = null;/*|*/\r\n\t@Override\r\n\tprotected Object clone() throws CloneNotSupportedException {\r\n\t\t// TODO Auto-generated method stub\r\n\t\treturn super.clone();\r\n\t}\r\n\tpublic Test() {\r\n\t}\r\n}";
    assert_eq!(
        edited.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}

#[test]
fn test_generate_before_cursor_position() {
    let (mut ws, root) = base();
    ws.settings["java.codeGeneration.insertionLocation"] = json!("beforeCursor");
    let source="package p;\r\n\r\npublic class Test implements Cloneable {\r\n\tfinal String field1 = null;/*|*/\r\n\tpublic Test() {\r\n\t}\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "Test.java", source);
    let response = list(&mut ws, &uri, source, "class Test");
    check(&response, &["clone()"], Some(true), false);
    let selected = json!(response["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["name"] == "clone")
        .cloned()
        .collect::<Vec<_>>());
    let edited = generate(&mut ws, &uri, source, Some("/*|*/"), selected);
    let expected="package p;\r\n\r\npublic class Test implements Cloneable {\r\n\t@Override\r\n\tprotected Object clone() throws CloneNotSupportedException {\r\n\t\t// TODO Auto-generated method stub\r\n\t\treturn super.clone();\r\n\t}\r\n\tfinal String field1 = null;/*|*/\r\n\tpublic Test() {\r\n\t}\r\n}";
    assert_eq!(
        edited.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
}
