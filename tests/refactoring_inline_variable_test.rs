//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.InlineVariableTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

const INLINE_LOCAL_VARIABLE: &str = "Inline local variable";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_inline_local_variable() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(String[] parameters, int j) {\n");
    buf.push_str("        int /*]*/temp/*[*/ = parameters.length + j;\n");
    buf.push_str("        int temp1 = temp;\n");
    buf.push_str("        System.out.println(temp);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(String[] parameters, int j) {\n");
    buf.push_str("        int temp1 = parameters.length + j;\n");
    buf.push_str("        System.out.println(parameters.length + j);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INLINE_LOCAL_VARIABLE, &buf, "refactor.inline");
    t.assert_code_actions(&cu, &[expected.clone()]);
}

#[test]
fn test_inline_local_variable_with_no_references() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(String[] parameters, int j) {\n");
    buf.push_str("        int temp = parameters.length + j;\n");
    buf.push_str("        int /*]*/temp1/*[*/ = temp;\n");
    buf.push_str("        System.out.println(temp);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);
    t.assert_code_action_not_exists(&cu, INLINE_LOCAL_VARIABLE);
}

