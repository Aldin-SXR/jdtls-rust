//! Port of `org.eclipse.jdt.ls.core.internal.correction.VariableQuickFixTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::path::PathBuf;

const QUICK_ASSIST: &str = "quickassist";

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    t.set_only(&["quickfix"]);
    (t, root)
}

#[test]
fn test_split_variable_expect_declaration_and_assignment() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!(
        "package test1;\r\n",
        "public class V {\r\n",
        "    public void foo() {\r\n",
        "        int maxCount = 10;\r\n",
        "    }\r\n",
        "}",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "V.java", contents);
    let expected = concat!(
        "package test1;\r\n",
        "public class V {\r\n",
        "    public void foo() {\r\n",
        "        int maxCount;\r\n",
        "        maxCount = 10;\r\n",
        "    }\r\n",
        "}",
    );
    let range = get_range(&t.ws.read(&cu), "maxCount");
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Split variable declaration", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&code_actions, &[e1]);
}

#[test]
fn test_join_variable_expect_declaration_and_assignment() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!(
        "package test1;\r\n",
        "public class V {\r\n",
        "    public void foo() {\r\n",
        "        int maxCount;\r\n",
        "        maxCount = 10;\r\n",
        "    }\r\n",
        "}",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "V.java", contents);
    let expected = concat!(
        "package test1;\r\n",
        "public class V {\r\n",
        "    public void foo() {\r\n",
        "        int maxCount = 10;\r\n",
        "    }\r\n",
        "}",
    );
    let range = get_range(&t.ws.read(&cu), "maxCount");
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Join variable declaration", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&code_actions, &[e1]);
}

#[test]
fn test_invert_equals_expect_variables_swapped() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!(
        "package test1;\n",
        "public class E {\n",
        "    public void foo() {\n",
        "\t\tString name1 = \"John\";\n",
        "\t\tString name2 = \"Doe\";\n",
        "\t\tif (name1.equals(name2)) {\n",
        "\t\t}\n",
        "    }\n",
        "}",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", contents);
    let expected = concat!(
        "package test1;\n",
        "public class E {\n",
        "    public void foo() {\n",
        "\t\tString name1 = \"John\";\n",
        "\t\tString name2 = \"Doe\";\n",
        "\t\tif (name2.equals(name1)) {\n",
        "\t\t}\n",
        "    }\n",
        "}",
    );
    let range = get_range(&t.ws.read(&cu), "name1.equals(name2)");
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Invert equals", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&code_actions, &[e1]);
}
