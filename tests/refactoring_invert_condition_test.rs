//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.InvertConditionTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_range, get_range_len, Expected, QuickFixTest};

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_invert_less_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 >= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 < 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 < 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_greater_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 > 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 <= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 > 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 > 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_less_equals_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 <= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 > 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 <= 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 <= 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_greater_equals_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 >= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 >= 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 >= 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_equals_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 == 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 != 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 == 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 == 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_not_equals_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 != 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (3 == 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 != 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 != 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_conditional_and_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (true && true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (false || false)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true && true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true && true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_conditional_or_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (true || true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (false && false)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true || true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true || true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_and_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (true & true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (false | false)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true & true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true & true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_or_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (true | true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (false & false)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true | true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true | true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_xor_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (true ^ true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (!(true ^ true))\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true ^ true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true ^ true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_boolean_literal() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        while (true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        while (false)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "true");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "true", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_boolean_literal_with_parentheses() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        while (!(!true))\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        while (!true)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "!(!true)");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "!(!true)", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_complex_condition() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        for (int i = 4; i > 3 && i < 10; i++)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        for (int i = 4; i <= 3 || i >= 10; i++)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "i > 3 && i < 10");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "i > 3 && i < 10", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_variable() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean isValid = true;\n");
    buf.push_str("        if (isValid)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        boolean isValid = true;\n");
    buf.push_str("        if (!isValid)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "isValid");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "isValid", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_invert_method_calling() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (isValid())\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (!isValid())\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "isValid()");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "isValid()", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_conditional_operator() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        return 3 > 5 ? 0 : -1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        return 3 <= 5 ? 0 : -1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 > 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let non_selection_range = get_range_len(&t.ws.read(&cu), "3 > 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_combined_condition() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (isValid() || 3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (!isValid() && 3 >= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "isValid() || 3 < 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);

    let mut non_selection_range = get_range_len(&t.ws.read(&cu), "isValid()", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);

    non_selection_range = get_range_len(&t.ws.read(&cu), "3 < 5", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);

    non_selection_range = get_range_len(&t.ws.read(&cu), "||", 0);
    t.assert_code_actions_range(&cu, non_selection_range.clone(), &[expected.clone()]);
}

#[test]
fn test_combined_condition_with_partial_selection() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (isValid() || 3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (!isValid() || 3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "isValid()");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);
}

#[test]
fn test_combined_condition_with_partial_selection2() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (isValid() || 3 < 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public boolean isValid() {\n");
    buf.push_str("        return true;\n");
    buf.push_str("    }\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        if (isValid() || 3 >= 5)\n");
    buf.push_str("            return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Invert conditions", &buf, "refactor");
    let replaced_range = get_range(&t.ws.read(&cu), "3 < 5");
    t.assert_code_actions_range(&cu, replaced_range.clone(), &[expected.clone()]);
}

