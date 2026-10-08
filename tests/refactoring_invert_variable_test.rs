//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.InvertVariableTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_range_len, Expected, QuickFixTest};

const INVERT_BOOLEAN_VARIABLE: &str = "Invert local variable";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_adding_not_prefix_when_invert_variable() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean lie = 3 == 5;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean notLie = 3 != 5;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INVERT_BOOLEAN_VARIABLE, &buf, "refactor");
    let replaced_range = get_range_len(&t.ws.read(&cu), "lie", 0);
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);
}

#[test]
fn test_removing_not_prefix_when_invert_variable() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean notLie = 3 != 5;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean lie = 3 == 5;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INVERT_BOOLEAN_VARIABLE, &buf, "refactor");
    let replaced_range = get_range_len(&t.ws.read(&cu), "notLie", 0);
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);
}

#[test]
fn test_complex_invert_variable() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean foo() {\n");
    buf.push_str("        boolean a = 3 != 5;\n");
    buf.push_str("        boolean b = !a;\n");
    buf.push_str("        boolean c = bar(a);\n");
    buf.push_str("        return a;\n");
    buf.push_str("    }\n");
    buf.push_str("    public boolean bar(boolean value) {\n");
    buf.push_str("        return !value;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean foo() {\n");
    buf.push_str("        boolean notA = 3 == 5;\n");
    buf.push_str("        boolean b = notA;\n");
    buf.push_str("        boolean c = bar(!notA);\n");
    buf.push_str("        return !notA;\n");
    buf.push_str("    }\n");
    buf.push_str("    public boolean bar(boolean value) {\n");
    buf.push_str("        return !value;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INVERT_BOOLEAN_VARIABLE, &buf, "refactor");
    let replaced_range = get_range_len(&t.ws.read(&cu), "a = 3 != 5", 0);
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);
}

