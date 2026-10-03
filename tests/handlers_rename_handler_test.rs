//! Port of `org.eclipse.jdt.ls.core.internal.handlers.RenameHandlerTest`.

mod common;
use common::jdtls::{apply_edits, dos2unix, pos, test_default_options, Workspace};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    ws: Workspace,
    project: PathBuf,
}

/// `setup()`: an empty project, a client without resource operation support
/// (`isResourceOperationSupported` → false) and rename enabled.
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

/// `getRenameEdit`: the WorkspaceEdit, or the ResponseError.
fn get_rename_edit(ws: &mut Workspace, uri: &str, position: &Value, new_name: &str) -> Result<Value, Value> {
    let c = ws.client();
    let id = format!("rename-{uri}-{position}-{new_name}");
    c.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/rename",
        "params": { "textDocument": { "uri": uri }, "position": position, "newName": new_name }
    }));
    let resp = c
        .recv_until(Duration::from_secs(90), |m| m["id"] == json!(id) && m.get("method").is_none())
        .expect("timed out waiting for textDocument/rename");
    match resp.get("error") {
        Some(err) => Err(err.clone()),
        None => Ok(resp["result"].clone()),
    }
}

fn changes(edit: &Value) -> &serde_json::Map<String, Value> {
    edit["changes"].as_object().expect("WorkspaceEdit.changes")
}

fn edits_for<'a>(edit: &'a Value, uri: &str) -> &'a [Value] {
    edit["changes"][uri].as_array().map(Vec::as_slice).unwrap_or_else(|| panic!("no changes for {uri} in {edit:#}"))
}

fn document_changes(edit: &Value) -> &[Value] {
    edit["documentChanges"].as_array().map(Vec::as_slice).unwrap_or_else(|| panic!("no documentChanges in {edit:#}"))
}

fn text_document_edits(change: &Value) -> &[Value] {
    assert!(change.get("textDocument").is_some(), "expected a TextDocumentEdit, got {change:#}");
    change["edits"].as_array().unwrap()
}

/// `java.util.regex` `replaceFirst("(?s)E(?!.*?E)", repl)`: replace the last `E`.
fn replace_last(s: &str, from: char, to: &str) -> String {
    match s.rfind(from) {
        Some(i) => format!("{}{}{}", &s[..i], to, &s[i + from.len_utf8()..]),
        None => s.to_owned(),
    }
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "newname").unwrap();

    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);

    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class E {\n"
            + "   public int foo(String newname) {\n"
            + "  \t\tnewname.length();\n"
            + "   }\n"
            + "   public int bar(String str) {\n"
            + "   \tstr.length();\n"
            + "   }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);
    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class E {\n"
            + "   public int bar() {\n"
            + "\t\tString str = new String();\n"
            + "   \tstr.length();\n"
            + "   }\n"
            + "   public int foo() {\n"
            + "\t\tString newname = new String();\n"
            + "   \tnewname.length()\n"
            + "   }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);
    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class E {\n"
            + "\tprivate int newname = 2;\n"
            + "   public void bar() {\n"
            + "\t\tnewname = 3;\n"
            + "   }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);
    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class E {\n"
            + "   public int newname() {\n"
            + "   }\n"
            + "   public int foo() {\n"
            + "\t\tthis.newname();\n"
            + "   }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "Newname").unwrap();
    assert!(!edit.is_null());
    let resource_changes = document_changes(&edit);

    assert_eq!(resource_changes.len(), 2);

    let resource_change = &resource_changes[1];
    assert_eq!(resource_change["kind"], "rename", "expected a RenameFile, got {resource_change:#}");
    assert_eq!(json!(cu), resource_change["oldUri"]);
    assert_eq!(json!(replace_last(&cu, 'E', "Newname")), resource_change["newUri"]);

    let test_changes = text_document_edits(&resource_changes[0]);

    let expected = "package test1;\n".to_owned()
        + "public class Newname {\n"
        + "   public Newname() {\n"
        + "   }\n"
        + "   public int bar() {\n"
        + "   }\n"
        + "   public int foo() {\n"
        + "\t\tthis.bar();\n"
        + "   }\n"
        + "}\n";

    assert_eq!(expected, apply_edits(&builder, test_changes));
}

#[test]
fn test_rename_type_with_errors() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);
    // assertThrows(ResponseErrorException.class, …)
    let result = (|| {
        let codes = ["package test1;\n", "public class Newname {\n", "   }\n", "}\n"];
        let (builder, _) = merge_code(&codes);
        ws.create_cu(&project, "src", "test1", "Newname.java", &builder);

        let codes1 = [
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
        let (builder, pos) = merge_code(&codes1);
        let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

        let edit = get_rename_edit(&mut ws, &cu, &pos, "Newname")?;
        assert!(!edit.is_null());
        let resource_changes = document_changes(&edit);

        assert_eq!(resource_changes.len(), 3);
        Ok::<_, Value>(())
    })();
    assert!(result.is_err(), "expected a ResponseError");
}

#[test]
fn test_rename_system_library() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "public class E {\n",
        "   public int bar() {\n",
        "\t\tString str = new String();\n",
        "   \tstr.len|*gth();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    assert!(get_rename_edit(&mut ws, &cu, &pos, "newname").is_err(), "expected a ResponseError");
}

#[test]
fn test_rename_multiple_files() {
    let Fixture { mut ws, project } = setup();
    let codes1 = ["package test1;\n", "public class A {\n", "   public void foo() {\n", "   }\n", "}\n"];

    let codes2 = [
        "package test1;\n",
        "public class B {\n",
        "   public void foo() {\n",
        "\t\tA a = new A();\n",
        "\t\ta.foo|*();\n",
        "   }\n",
        "}\n",
    ];
    let (builder_a, _) = merge_code(&codes1);
    let cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "test1", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);

    assert_eq!(
        apply_edits(&builder_a, edits_for(&edit, &cu_a)),
        "package test1;\n".to_owned() + "public class A {\n" + "   public void newname() {\n" + "   }\n" + "}\n"
    );

    assert_eq!(
        apply_edits(&builder_b, edits_for(&edit, &cu_b)),
        "package test1;\n".to_owned()
            + "public class B {\n"
            + "   public void foo() {\n"
            + "\t\tA a = new A();\n"
            + "\t\ta.newname();\n"
            + "   }\n"
            + "}\n"
    );
}

#[test]
fn test_rename_override_method_simple() {
    let Fixture { mut ws, project } = setup();
    let codes1 = ["package test1;\n", "public class A {\n", "   public void foo(){}\n", "}\n"];

    let codes2 = [
        "package test1;\n",
        "public class B extends A {\n",
        "\t@Override\n",
        "   public void foo|*() {\n",
        "   }\n",
        "}\n",
    ];
    let (builder_a, _) = merge_code(&codes1);
    let cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "test1", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);

    assert_eq!(
        apply_edits(&builder_a, edits_for(&edit, &cu_a)),
        "package test1;\n".to_owned() + "public class A {\n" + "   public void newname(){}\n" + "}\n"
    );

    assert_eq!(
        apply_edits(&builder_b, edits_for(&edit, &cu_b)),
        "package test1;\n".to_owned()
            + "public class B extends A {\n"
            + "\t@Override\n"
            + "   public void newname() {\n"
            + "   }\n"
            + "}\n"
    );
}

#[test]
fn test_rename_override_method_complex() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "class B extends A {\n",
        "\tpublic void foo|*() {\n",
        "\t};\n",
        "}\n",
        "abstract class A {\n",
        "\tpublic abstract void foo();\n",
        "}\n",
        "class C extends A implements D {\n",
        "\tpublic void foo() {\n",
        "\t};\n",
        "}\n",
        "interface D {\n",
        "\tvoid foo();\n",
        "}\n",
    ];

    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "A.java", &builder);

    let edit = get_rename_edit(&mut ws, &cu, &pos, "newfoo").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);

    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "class B extends A {\n"
            + "\tpublic void newfoo() {\n"
            + "\t};\n"
            + "}\n"
            + "abstract class A {\n"
            + "\tpublic abstract void newfoo();\n"
            + "}\n"
            + "class C extends A implements D {\n"
            + "\tpublic void newfoo() {\n"
            + "\t};\n"
            + "}\n"
            + "interface D {\n"
            + "\tvoid newfoo();\n"
            + "}\n"
    );
}

#[test]
fn test_rename_interface_method() {
    let Fixture { mut ws, project } = setup();
    let codes1 = ["package test1;\n", "public interface A {\n", "   public void foo();\n", "}\n"];

    let codes2 = [
        "package test1;\n",
        "public class B implements A {\n",
        "\t@Override\n",
        "   public void foo|*() {\n",
        "   }\n",
        "}\n",
    ];
    let (builder_a, _) = merge_code(&codes1);
    let cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "test1", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "newname").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);

    assert_eq!(
        apply_edits(&builder_a, edits_for(&edit, &cu_a)),
        "package test1;\n".to_owned() + "public interface A {\n" + "   public void newname();\n" + "}\n"
    );

    assert_eq!(
        apply_edits(&builder_b, edits_for(&edit, &cu_b)),
        "package test1;\n".to_owned()
            + "public class B implements A {\n"
            + "\t@Override\n"
            + "   public void newname() {\n"
            + "   }\n"
            + "}\n"
    );
}

#[test]
fn test_rename_type() {
    let Fixture { mut ws, project } = setup();
    let codes1 = [
        "package test1;\n",
        "public class A {\n",
        "   public void foo(){\n",
        "\t\tB b = new B();\n",
        "\t\tb.foo();\n",
        "\t}\n",
        "}\n",
    ];

    let codes2 = ["package test1;\n", "public class B|* {\n", "\tpublic B() {}\n", "   public void foo() {}\n", "}\n"];
    let (builder_a, _) = merge_code(&codes1);
    let cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "test1", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "NewType").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);

    assert_eq!(
        apply_edits(&builder_a, edits_for(&edit, &cu_a)),
        "package test1;\n".to_owned()
            + "public class A {\n"
            + "   public void foo(){\n"
            + "\t\tNewType b = new NewType();\n"
            + "\t\tb.foo();\n"
            + "\t}\n"
            + "}\n"
    );

    assert_eq!(
        apply_edits(&builder_b, edits_for(&edit, &cu_b)),
        "package test1;\n".to_owned()
            + "public class NewType {\n"
            + "\tpublic NewType() {}\n"
            + "   public void foo() {}\n"
            + "}\n"
    );
}

// this test should pass when starting with -javaagent:<lombok_jar> (-javagent:~/.m2/repository/org/projectlombok/lombok/1.18.28/lombok-1.18.28.jar)
// https://github.com/eclipse/eclipse.jdt.ls/issues/1775
#[test]
fn test_rename_type_lombok() {
    let Fixture { mut ws, .. } = setup();
    ws.import_projects(&["maven/mavenlombok"]);
    let file = ws.class_uri("mavenlombok", "org.sample.Test");
    let cu = file.clone();
    let p = pos(5, 15);
    let source = ws.read(&cu);
    let expected = source.replace("Test", "Test1");
    let edit = get_rename_edit(&mut ws, &cu, &p, "Test1").unwrap();
    assert!(!edit.is_null());
    assert_eq!(2, changes(&edit).len());
    assert_eq!(expected, apply_edits(&source, edits_for(&edit, &cu)));
}

// this test should pass when starting with -javaagent:<lombok_jar> (-javagent:~/.m2/repository/org/projectlombok/lombok/1.18.28/lombok-1.18.28.jar)
// https://github.com/redhat-developer/vscode-java/issues/3203
#[test]
fn test_lombok_singular() {
    let Fixture { mut ws, .. } = setup();
    ws.import_projects(&["maven/mavenlombok"]);
    let cu = ws.class_uri("mavenlombok", "org.sample.Test2");
    let p = pos(9, 18);
    let source = ws.read(&cu);
    let expected = source.replace("singulars", "singulars2");
    let edit = get_rename_edit(&mut ws, &cu, &p, "singulars2").unwrap();
    assert!(!edit.is_null());
    assert_eq!(1, changes(&edit).len());
    assert_eq!(expected, apply_edits(&source, edits_for(&edit, &cu)));
}

// this test should pass when starting with -javaagent:<lombok_jar> (-javagent:~/.m2/repository/org/projectlombok/lombok/1.18.28/lombok-1.18.28.jar)
// https://github.com/redhat-developer/vscode-java/issues/2805
#[test]
fn test_rename_method_lombok() {
    if std::env::var("jdt.ls.lombok.disabled").is_ok_and(|v| v == "true") {
        return;
    }
    let Fixture { mut ws, .. } = setup();
    ws.import_projects(&["maven/mavenlombok"]);
    let main = ws.class_uri("mavenlombok", "org.sample.Main");
    let file = ws.class_uri("mavenlombok", "org.sample.Test");
    let test2 = ws.class_uri("mavenlombok", "org.sample.Test2");
    // ResourceUtils.getErrorMarkers(project): there isn't the lombok agent.
    let has_errors = [&main, &file, &test2]
        .into_iter()
        .any(|u| ws.diagnostics(u).iter().any(|d| d["severity"] == 1));
    if has_errors {
        return;
    }
    let main_source = ws.read(&main);
    let main_expected = main_source.replace("getName", "getName1");
    let cu = file.clone();
    let p = pos(6, 23);
    let source = ws.read(&cu);
    let expected = source.replace("name", "name1");
    let edit = get_rename_edit(&mut ws, &cu, &p, "name1").unwrap();
    assert!(!edit.is_null());
    assert_eq!(2, changes(&edit).len());
    assert_eq!(expected, apply_edits(&source, edits_for(&edit, &cu)));
    assert_eq!(main_expected, apply_edits(&main_source, edits_for(&edit, &main)));
}

#[test]
fn test_rename_super_method() {
    let Fixture { mut ws, project } = setup();
    let codes = [
        "package test1;\n",
        "class A {\n",
        "   public void bar() {\n",
        "   }\n",
        "}\n",
        "class B extends A {\n",
        "   public void bar() {\n",
        "\t\tsuper|*.bar();\n",
        "   }\n",
        "}\n",
    ];
    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "E.java", &builder);

    let edit = get_rename_edit(&mut ws, &cu, &pos, "TypeA").unwrap();
    assert!(!edit.is_null());
    assert_eq!(1, changes(&edit).len());

    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "class TypeA {\n"
            + "   public void bar() {\n"
            + "   }\n"
            + "}\n"
            + "class B extends TypeA {\n"
            + "   public void bar() {\n"
            + "\t\tsuper.bar();\n"
            + "   }\n"
            + "}\n"
    );
}

#[test]
fn test_rename_constructor() {
    let Fixture { mut ws, project } = setup();
    let codes1 = [
        "package test1;\n",
        "public class A {\n",
        "   public void foo(){\n",
        "\t\tB b = new B();\n",
        "\t\tb.foo();\n",
        "\t}\n",
        "}\n",
    ];

    let codes2 = ["package test1;\n", "public class B {\n", "   public B|*() {}\n", "   public void foo() {}\n", "}\n"];
    let (builder_a, _) = merge_code(&codes1);
    let cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "test1", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "NewName").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);

    assert_eq!(
        apply_edits(&builder_a, edits_for(&edit, &cu_a)),
        "package test1;\n".to_owned()
            + "public class A {\n"
            + "   public void foo(){\n"
            + "\t\tNewName b = new NewName();\n"
            + "\t\tb.foo();\n"
            + "\t}\n"
            + "}\n"
    );

    assert_eq!(
        apply_edits(&builder_b, edits_for(&edit, &cu_b)),
        "package test1;\n".to_owned()
            + "public class NewName {\n"
            + "   public NewName() {}\n"
            + "   public void foo() {}\n"
            + "}\n"
    );
}

#[test]
fn test_rename_type_parameter() {
    let Fixture { mut ws, project } = setup();
    let codes = ["package test1;\n", "public class A<T|*> {\n", "\tprivate T t;\n", "\tpublic T get() { return t; }\n", "}\n"];

    let (builder, pos) = merge_code(&codes);
    let cu = ws.create_cu(&project, "src", "test1", "A.java", &builder);

    let edit = get_rename_edit(&mut ws, &cu, &pos, "TT").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);

    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class A<TT> {\n"
            + "\tprivate TT t;\n"
            + "\tpublic TT get() { return t; }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "UU").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);

    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class B<T> {\n"
            + "\tprivate T t;\n"
            + "\tpublic <UU extends Number> UU inspect(UU u) { return u; }\n"
            + "}\n"
    );
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

    let edit = get_rename_edit(&mut ws, &cu, &pos, "i2").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 1);
    assert_eq!(
        apply_edits(&builder, edits_for(&edit, &cu)),
        "package test1;\n".to_owned()
            + "public class E {\n"
            + "\t/**\n"
            + "\t *@param i2 int\n"
            + "\t */\n"
            + "   public int foo(int i2) {\n"
            + "\t\tE e = new E();\n"
            + "\t\te.foo();\n"
            + "   }\n"
            + "}\n"
    );
}

#[test]
fn test_rename_package() {
    let Fixture { mut ws, project } = setup();
    set_resource_operation_supported(&mut ws, true);

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
    let _cu_a = ws.create_cu(&project, "src", "test1", "A.java", &builder_a);

    let (builder_b, pos) = merge_code(&codes2);
    let cu_b = ws.create_cu(&project, "src", "parent.test2", "B.java", &builder_b);

    let edit = get_rename_edit(&mut ws, &cu_b, &pos, "parent.newpackage").unwrap();
    assert!(!edit.is_null());

    let resource_changes = document_changes(&edit);

    assert_eq!(5, resource_changes.len());

    let test_changes_a = text_document_edits(&resource_changes[0]);
    let test_changes_b = text_document_edits(&resource_changes[1]);

    let expected_a = "package test1;\n".to_owned()
        + "import parent.newpackage.B;\n"
        + "public class A {\n"
        + "   public void foo(){\n"
        + "\t\tB b = new B();\n"
        + "\t\tb.foo();\n"
        + "\t}\n"
        + "}\n";

    let expected_b = "package parent.newpackage;\n".to_owned()
        + "public class B {\n"
        + "\tpublic B() {}\n"
        + "   public void foo() {}\n"
        + "}\n";
    assert_eq!(expected_a, apply_edits(&builder_a, test_changes_a));
    assert_eq!(expected_b, apply_edits(&builder_b, test_changes_b));

    //moved package
    let resource_change = &resource_changes[2];
    assert_eq!(resource_change["kind"], "create", "expected a CreateFile, got {resource_change:#}");
    let pack2_uri = ws.path_uri(&format!("{}/src/parent/test2/", common::jdtls::TEST_PROJECT_NAME));
    let expected_create = regex_replace_first_test2(&pack2_uri, "newpackage/.temp");
    assert_eq!(json!(expected_create), resource_change["uri"]);

    //moved class B
    let resource_change2 = &resource_changes[3];
    assert_eq!(resource_change2["kind"], "rename", "expected a RenameFile, got {resource_change2:#}");
    assert_eq!(json!(cu_b), resource_change2["oldUri"]);
    assert_eq!(json!(cu_b.replace("test2", "newpackage")), resource_change2["newUri"]);
}

/// `replaceFirst("test2[/]?", repl)`.
fn regex_replace_first_test2(s: &str, repl: &str) -> String {
    let i = s.find("test2").expect("test2 in uri");
    let mut end = i + "test2".len();
    if s[end..].starts_with('/') {
        end += 1;
    }
    format!("{}{}{}", &s[..i], repl, &s[end..])
}

// https://github.com/redhat-developer/vscode-java/issues/2433
#[test]
fn test_rename_record_field() {
    let Fixture { mut ws, .. } = setup();
    let name = "java17";
    ws.import_projects(&[&format!("eclipse/{name}")]);
    // assertIsJavaProject(project)
    let root_uri = ws.project_uri(name);
    let projects = ws.request("workspace/executeCommand", json!({ "command": "java.project.getAll", "arguments": [] }));
    assert!(
        projects.as_array().unwrap().iter().any(|p| p.as_str() == Some(root_uri.as_str())),
        "{name} is not a Java project: {projects}"
    );
    // assertEquals("17", getJavaSourceLevel(project)): the fixture's JDT prefs
    // (the server exposes no project-settings request yet).
    let prefs = std::fs::read_to_string(root.join(".settings/org.eclipse.jdt.core.prefs")).unwrap();
    assert!(prefs.lines().any(|l| l.trim() == "org.eclipse.jdt.core.compiler.source=17"));
    let cu = ws.class_uri(name, "test1.Test");
    let p = pos(1, 29);
    let edit = get_rename_edit(&mut ws, &cu, &p, "value2").unwrap();
    assert!(!edit.is_null());
    assert_eq!(changes(&edit).len(), 2);
    let main_cu = ws.class_uri(name, "test1.Main");
    let change = edits_for(&edit, &main_cu);
    let text = apply_edits(&ws.read(&main_cu), change);
    let expected = "package test1;\n".to_owned()
        + "public class Main {\n"
        + "    public static void main(String[] args) {\n"
        + "        Test instance = new Test(2);\n"
        + "        System.out.println(instance.value2());\n"
        + "    }\n"
        + "}\n";
    assert_eq!(dos2unix(&expected), dos2unix(&text));
}
