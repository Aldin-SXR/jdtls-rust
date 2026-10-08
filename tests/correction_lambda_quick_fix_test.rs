//! Port of `org.eclipse.jdt.ls.core.internal.correction.LambdaQuickFixTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::path::PathBuf;

const QUICK_ASSIST: &str = "quickassist";

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    t.set_only(&["refactor", "quickfix"]);
    (t, root)
}

#[test]
fn test_clean_up_lambda_convert_lambda_block_to_expression() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!(
        "package test1;\r\n",
        "interface F1 {\r\n",
        "    int foo1(int a);\r\n",
        "}\r\n",
        "public class E {\r\n",
        "    public void foo(int a) {\r\n",
        "        F1 k = (e) -> {\r\n",
        "            return a;\r\n",
        "        };\r\n",
        "        k.foo1(5);\r\n",
        "    }\r\n",
        "}",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", contents);
    let expected = concat!(
        "package test1;\r\n",
        "interface F1 {\r\n",
        "    int foo1(int a);\r\n",
        "}\r\n",
        "public class E {\r\n",
        "    public void foo(int a) {\r\n",
        "        F1 k = e -> a;\r\n",
        "        k.foo1(5);\r\n",
        "    }\r\n",
        "}",
    );
    let range = common::jdtls::range(7, 16, 7, 16);
    let actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Clean up lambda expression", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&actions, &[e1]);
}

#[test]
fn test_clean_up_lambda_convert_lambda_block_to_expression_add_parenthesis() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!(
        "package test1;\r\n",
        "interface F1 {\r\n",
        "    int foo1(int a);\r\n",
        "}\r\n",
        "public class E {\r\n",
        "    public void foo(int a) {\r\n",
        "        F1 k = (e) -> {\r\n",
        "            return a + 1;\r\n",
        "        };\r\n",
        "        k.foo1(5);\r\n",
        "    }\r\n",
        "}",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", contents);
    let expected = concat!(
        "package test1;\r\n",
        "interface F1 {\r\n",
        "    int foo1(int a);\r\n",
        "}\r\n",
        "public class E {\r\n",
        "    public void foo(int a) {\r\n",
        "        F1 k = e -> (a + 1);\r\n",
        "        k.foo1(5);\r\n",
        "    }\r\n",
        "}",
    );
    let range = common::jdtls::range(7, 16, 7, 16);
    let actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Clean up lambda expression", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&actions, &[e1]);
}

fn lambda_test(t: &mut QuickFixTest, root: &PathBuf, contents: &str, expected: &str, search: &str, title: &str) {
    t.set_only(&[QUICK_ASSIST]);
    let cu = t.ws.create_cu(root, "src", "test1", "L.java", contents);
    let range = get_range(&t.ws.read(&cu), search);
    let actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind(title, expected, QUICK_ASSIST);
    t.assert_code_actions_list(&actions, &[e1]);
}

#[test]
fn test_add_inferred_lambda_parameter_types_expect_types() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (String a, String b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "(a, b)", "Add inferred lambda parameter types");
}

#[test]
fn test_add_var_lambda_parameter_types_expect_var_keyword() {
    let (mut t, root) = setup();
    let mut options = common::jdtls::test_default_options();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "11".to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    t.ws.set_project_options(&root, &options);
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (var a, var b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "(a, b)", "Add 'var' lambda parameter types");
}

#[test]
fn test_remove_var_or_inferred_lambda_parameter_types_expect_no_type() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (String a, String b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "(String a, String b) ->", "Remove lambda parameter types");
}

#[test]
fn test_remove_var_or_inferred_lambda_parameter_types_expect_no_var_keyword() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (var a, var b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "    public void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "    }\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "(var a, var b) ->", "Remove lambda parameter types");
}

#[test]
fn test_change_lambda_body_to_block_expect_block() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tFunc f = (a, b) -> {\n",
        "            System.out.println(a + b);\n",
        "        };\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "->", "Change body expression to block");
}

#[test]
fn test_change_lambda_body_to_expression_expect_expression() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tFunc f = (a, b) -> {\n",
        "\t\t\tSystem.out.println(a + b);\n",
        "\t\t};\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tFunc f = (a, b) -> System.out.println(a + b);\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(String a, String b);\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "->", "Change body block to expression");
}

#[test]
fn test_convert_lambda_to_method_reference_expect_method_ref() {
    let (mut t, root) = setup();
    let contents = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tthis.consume(a -> a.print());\n",
        "\t}\n",
        "\n",
        "\tpublic void consume(Func f) {\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(Obj a);\n",
        "\t}\n",
        "\n",
        "\tpublic class Obj {\n",
        "\t\tpublic void print() {}\n",
        "\t}\n",
        "}",
    );
    let expected = concat!(
        "package test1;\n",
        "public class L {\n",
        "\tpublic void foo() {\n",
        "\t\tthis.consume(Obj::print);\n",
        "\t}\n",
        "\n",
        "\tpublic void consume(Func f) {\n",
        "\t}\n",
        "\n",
        "\tpublic interface Func {\n",
        "\t\tvoid foo(Obj a);\n",
        "\t}\n",
        "\n",
        "\tpublic class Obj {\n",
        "\t\tpublic void print() {}\n",
        "\t}\n",
        "}",
    );
    lambda_test(&mut t, &root, contents, expected, "->", "Convert to method reference");
}
