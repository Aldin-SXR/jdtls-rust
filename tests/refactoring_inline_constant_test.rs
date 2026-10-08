//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.InlineConstantTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

const INLINE_CONSTANT: &str = "Inline Constant";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_inline_constant_declaration_selected() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private static final String /*]*/LOGGER_NAME/*[*/ = \"TEST.E\";\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        System.out.println(LOGGER_NAME);\n");
    buf.push_str("    }\n");
    buf.push_str("    public void bar() {\n");
    buf.push_str("        String value = LOGGER_NAME;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        System.out.println(\"TEST.E\");\n");
    buf.push_str("    }\n");
    buf.push_str("    public void bar() {\n");
    buf.push_str("        String value = \"TEST.E\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INLINE_CONSTANT, &buf, "refactor.inline");
    t.assert_code_actions(&cu, &[expected.clone()]);
}

#[test]
fn test_inline_constant_invocation_selected() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private static final String LOGGER_NAME = \"TEST.E\";\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        System.out.println(/*]*/LOGGER_NAME/*[*/);\n");
    buf.push_str("    }\n");
    buf.push_str("    public void bar() {\n");
    buf.push_str("        String value = LOGGER_NAME;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private static final String LOGGER_NAME = \"TEST.E\";\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        System.out.println(/*]*/\"TEST.E\"/*[*/);\n");
    buf.push_str("    }\n");
    buf.push_str("    public void bar() {\n");
    buf.push_str("        String value = LOGGER_NAME;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind(INLINE_CONSTANT, &buf, "refactor.inline");
    t.assert_code_actions(&cu, &[expected.clone()]);
}

#[test]
fn test_inline_constant_no_reference() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private static final String /*]*/LOGGER_NAME/*[*/ = \"TEST.E\";\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);
    t.assert_code_action_not_exists(&cu, INLINE_CONSTANT);
}

#[test]
fn test_inline_constant_reference_in_imports() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("import java.util.HashMap;\n");
    buf.push_str("import java.util.Map;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static final Map /*]*/map/*[*/ = new HashMap<>();\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);
    t.assert_code_action_not_exists(&cu, INLINE_CONSTANT);
}

