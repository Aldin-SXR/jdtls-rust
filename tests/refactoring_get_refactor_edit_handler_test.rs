//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.GetRefactorEditHandlerTest`.
//!
//! `GetRefactorEditHandler.getEditsForRefactor` is exercised through the
//! `java/getRefactorEdit` request.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{evaluate_workspace_edit, get_selection_range, QuickFixTest};
use serde_json::{json, Value};

const RENAME_COMMAND: &str = "java.action.rename";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    (t, root)
}

/// `GetRefactorEditHandler.getEditsForRefactor(new GetRefactorEditParams(command, arguments, params))`.
fn refactor_edit(t: &mut QuickFixTest, cu: &str, command: &str, arguments: Value) -> Value {
    let diagnostics = t.diagnostics(cu);
    let range = get_selection_range(&t.ws.read(cu));
    t.ws.request(
        "java/getRefactorEdit",
        json!({
            "command": command,
            "commandArguments": arguments,
            "context": { "textDocument": { "uri": cu }, "range": range, "context": { "diagnostics": diagnostics, "only": ["refactor"] } }
        }),
    )
}

/// `AbstractSourceTestCase.compareSource`.
fn compare_source(expected: &str, actual: &str) {
    let mut e = expected.lines();
    let mut a = actual.lines();
    let mut line = 1;
    loop {
        match (a.next(), e.next()) {
            (None, None) => return,
            (x, y) => {
                assert_eq!(y, x, "Content not as expected: line {line}");
            }
        }
        line += 1;
    }
}

fn assert_rename_command(result: &Value) {
    assert!(result["command"].is_object(), "{result:#}");
    assert_eq!(RENAME_COMMAND, result["command"]["command"]);
    assert_eq!(1, result["command"]["arguments"].as_array().expect("arguments").len());
}

#[test]
fn test_extract_variable() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractVariable", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_variable_final() {
    let (mut t, root) = setup();
    t.ws.settings["java"]["codeGeneration"]["addFinalForNewDeclaration"] = json!("all");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractVariable", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		final int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_variable_all_occurrence() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractVariableAllOccurrence", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_variable_all_occurrence_final() {
    let (mut t, root) = setup();
    t.ws.settings["java"]["codeGeneration"]["addFinalForNewDeclaration"] = json!("all");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractVariableAllOccurrence", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		final int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_field() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractField", json!(["Current method"]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	private int i;\n\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		this.i = 0;\n");
    buf.push_str("        int x= /*]*/this.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_constant() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractConstant", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	private static final int _0 = /*]*/0/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		int x= _0;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

#[test]
fn test_extract_method() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo(boolean b1, boolean b2) {\n");
    buf.push_str("        int n = 0;\n");
    buf.push_str("        int i = 0;\n");
    buf.push_str("        /*[*/\n");
    buf.push_str("        if (b1)\n");
    buf.push_str("            i = 1;\n");
    buf.push_str("        if (b2)\n");
    buf.push_str("            n = n + i;\n");
    buf.push_str("        /*]*/\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let refactor_edit = refactor_edit(&mut t, &cu, "extractMethod", json!([]));
    assert!(!refactor_edit.is_null());
    assert!(!refactor_edit["edit"].is_null(), "{refactor_edit:#}");
    let actual = evaluate_workspace_edit(&t.ws, &refactor_edit["edit"]).expect("edit");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo(boolean b1, boolean b2) {\n");
    buf.push_str("        int n = 0;\n");
    buf.push_str("        int i = 0;\n");
    buf.push_str("        n = extracted(b1, b2, n, i);\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private int extracted(boolean b1, boolean b2, int n, int i) {\n");
    buf.push_str("        /*[*/\n");
    buf.push_str("        if (b1)\n");
    buf.push_str("            i = 1;\n");
    buf.push_str("        if (b2)\n");
    buf.push_str("            n = n + i;\n");
    buf.push_str("        /*]*/\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    compare_source(&buf, &actual);
    assert_rename_command(&refactor_edit);
}

