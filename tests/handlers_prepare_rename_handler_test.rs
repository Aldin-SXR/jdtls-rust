//! Port of `org.eclipse.jdt.ls.core.internal.handlers.PrepareRenameHandlerTest`.

mod common;
use common::jdtls::{pos, test_default_options, Workspace};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    ws: Workspace,
    project: PathBuf,
}

/// `setup()`: an empty project, no resource operation support, rename enabled.
fn setup() -> Fixture {
    let mut ws = Workspace::new();
    set_resource_operation_supported(&mut ws, false);
    let project = ws.new_empty_project(&test_default_options());
    Fixture { ws, project }
}

fn set_resource_operation_supported(ws: &mut Workspace, supported: bool) {
    ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        if supported { json!(["create", "rename", "delete"]) } else { json!([]) };
}

/// `mergeCode`: join the lines, recording the position of `|*`.
fn merge_code(codes: &[&str]) -> (String, Value) {
    let mut builder = String::new();
    let mut position = Value::Null;
    for (i, code) in codes.iter().enumerate() {
        if let Some(ind) = code.find("|*").filter(|&ind| ind > 0) {
            position = pos(i as u32, ind as u32);
        }
        builder.push_str(&code.replace("|*", ""));
    }
    (builder, position)
}

/// `prepareRename`: `Either.forLeft(range)`, or the ResponseError.
fn prepare_rename(ws: &mut Workspace, uri: &str, position: &Value, _new_name: &str) -> Result<Value, Value> {
    let c = ws.client();
    let id = format!("prepareRename-{uri}-{position}");
    c.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/prepareRename",
        "params": { "textDocument": { "uri": uri }, "position": position }
    }));
    let resp = c
        .recv_until(Duration::from_secs(90), |m| m["id"] == json!(id) && m.get("method").is_none())
        .expect("timed out waiting for textDocument/prepareRename");
    match resp.get("error") {
        Some(err) => Err(err.clone()),
        None => Ok(resp["result"].clone()),
    }
}

/// `result.getLeft()`: the plain `Range` form of the result.
fn left(result: &Value) -> &Value {
    assert!(result.get("start").is_some() && result.get("end").is_some(), "expected a Range, got {result}");
    result
}

fn start_line(range: &Value) -> u64 {
    range["start"]["line"].as_u64().unwrap()
}

/// `assertThrows(ResponseErrorException.class, …)`.
fn assert_response_error(result: Result<Value, Value>) {
    let err = result.expect_err("expected a ResponseError");
    assert!(err.get("code").is_some(), "malformed ResponseError {err}");
}

#[test]
fn test_rename_parameter() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "   public int foo(String str) {\n",
        "  \t\tstr|*.length();\n",
        "   }\n",
        "   public int bar(String str) {\n",
        "   \tstr.length();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "newname").unwrap();

    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_local_variable() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "   public int bar() {\n",
        "\t\tString str = new String();\n",
        "   \tstr.length();\n",
        "   }\n",
        "   public int foo() {\n",
        "\t\tString str = new String();\n",
        "   \tstr|*.length()\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_field() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "\tprivate int myValue = 2;\n",
        "   public void bar() {\n",
        "\t\tmyValue|* = 3;\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_method() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "   public int bar() {\n",
        "   }\n",
        "   public int foo() {\n",
        "\t\tthis.bar|*();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_type_with_resource_changes() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);

    let codes = [
        "package test1;\n",
        "public class E|* {\n",
        "   public E() {\n",
        "   }\n",
        "   public int bar() {\n",
        "   }\n",
        "   public int foo() {\n",
        "\t\tthis.bar();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "Newname").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_type_parameter() {
    let Fixture { mut ws, project } = setup();
    let codes = ["package test1;\n", "public class A<T|*> {\n", "\tprivate T t;\n", "\tpublic T get() { return t; }\n", "}\n"];

    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "A.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "TT").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_type_parameter_in_method() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class B<T> {\n",
        "\tprivate T t;\n",
        "\tpublic <U|* extends Number> U inspect(U u) { return u; }\n",
        "}\n",
    ];

    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "B.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "UU").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_lambda_parameter() {
    let Fixture { mut ws, project } = setup();

    let codes = [
        "package test1;\n",
        "import java.util.function.Function;\n",
        "public class Test {\n",
        "    Function<Integer, String> f = i|* -> \"\" + i;\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "Test.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "j").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_javadoc() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "\t/**\n",
        "\t *@param i int\n",
        "\t */\n",
        "   public int foo(int i|*) {\n",
        "\t\tE e = new E();\n",
        "\t\te.foo();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let result = prepare_rename(&mut ws, &cu, &pos, "i2").unwrap();
    assert!(!left(&result).is_null());
    assert!(start_line(left(&result)) > 0);
}

#[test]
fn test_rename_package() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);
    assert_response_error((|| {
        let codes1 = [
            "package test1;\n",
            "import parent.test2.B;\n",
            "public class A {\n",
            "   public void foo(){\n",
            "\t\tB b = new B();\n",
            "\t\tb.foo();\n",
            "\t}\n",
            "}\n",
        ];

        let codes2 = ["package parent.test2|*;\n", "public class B {\n", "\tpublic B() {}\n", "   public void foo() {}\n", "}\n"];
        let (builder_a, _) = merge_code(&codes1);
        ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

        let (builder_b, pos) = merge_code(&codes2);
        let cu_b = ws.create_cu(&project, "src", "parent.test2", "B.java", &builder_b);

        prepare_rename(&mut ws, &cu_b, &pos, "parent.newpackage")
    })());
}

#[test]
fn test_rename_middle_of_package() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);
    assert_response_error((|| {
        let content = ["package |*ex.amples;\n", "public class A {}\n"];
        let (builder, pos) = merge_code(&content);
        let cu = ws.create_cu(&project, "src", "ex.amples", "A.java", &builder);

        prepare_rename(&mut ws, &cu, &pos, "ex.am.ple")?;

        let content2 = ["package ex.|*amples;\n", "public class A {}\n"];
        let (builder, pos) = merge_code(&content2);
        let cu = ws.create_cu(&project, "src", "ex.amples", "A.java", &builder);

        prepare_rename(&mut ws, &cu, &pos, "ex.am.ple")
    })());
}

#[test]
fn test_rename_class_file() {
    assert_response_error(test_rename_class_file_with("Ex|*ception"));
}

#[test]
fn test_rename_fqcn_class_file() {
    assert_response_error(test_rename_class_file_with("java.lang.Ex|*ception"));
}

#[test]
fn test_rename_binary_package() {
    assert_response_error(test_rename_class_file_with("java.net|*.URI"));
}

#[test]
fn test_rename_import_declaration() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);
    assert_response_error((|| {
        let content = ["package ex.amples;\n", "import java.ne|*t.URI;\n", "public class A {}\n"];
        let (builder, pos) = merge_code(&content);
        let cu = ws.create_cu(&project, "src", "ex.amples", "A.java", &builder);

        prepare_rename(&mut ws, &cu, &pos, "")
    })());
}

/// `testRenameClassFile(String type)`.
fn test_rename_class_file_with(type_: &str) -> Result<Value, Value> {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);

    let line = format!("public class A extends {type_}{{}}\n");
    let content = ["package ex.amples;\n", line.as_str()];
    let (builder, pos) = merge_code(&content);
    let cu = ws.create_cu(&project, "src", "ex.amples", "A.java", &builder);

    prepare_rename(&mut ws, &cu, &pos, "MyException")
}
