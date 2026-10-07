//! Port of `org.eclipse.jdt.ls.core.internal.correction.GetterSetterQuickFixTest`.
mod common;

use std::path::PathBuf;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

/// `setup`: `newEmptyProject()` with `TestOptions.getDefaultOptions()`.
///
/// The upstream test's mocked `PreferenceManager` never pushes the
/// `java.format.insertSpaces` default into the JavaCore options.
fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.settings["java"]["format"] = serde_json::json!({ "insertSpaces": false });
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

fn lines(l: &[&str]) -> String {
    l.concat()
}

const D_TAIL: &[&str] = &["}\n", "class D {\n", "    public void foo(){\n", "        C c = new C();\n"];

fn invisible_field(statement: &str, replaced: &str) {
    let (mut t, root) = setup();
    let mut src = vec!["package test;\n", "public class C {\n", "    private int test;\n"];
    src.extend_from_slice(D_TAIL);
    src.extend_from_slice(&[statement, "    }\n", "}\n"]);
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", &lines(&src));

    let mut e1 = vec!["package test;\n", "public class C {\n", "    int test;\n"];
    e1.extend_from_slice(D_TAIL);
    e1.extend_from_slice(&[statement, "    }\n", "}\n"]);
    let e1 = Expected::new("Change visibility of 'test' to 'package'", &lines(&e1));

    let mut e2 = vec![
        "package test;\n",
        "public class C {\n",
        "    private int test;\n",
        "\n",
        "    public int getTest() {\n",
        "        return test;\n",
        "        \n",
        "    }\n",
        "\n",
        "    public void setTest(int test) {\n",
        "        this.test = test;\n",
        "        \n",
        "    }\n",
    ];
    e2.extend_from_slice(D_TAIL);
    e2.extend_from_slice(&[replaced, "    }\n", "}\n"]);
    let e2 = Expected::new("Create getter and setter for 'test'...", &lines(&e2));
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "SEF proposal not offered yet: field binding is_from_source() is false for the bridge field binding (availability check)"]
fn test_invisible_field_to_getter_setter() {
    invisible_field("        ++c.test;\n", "        c.setTest(c.getTest() + 1);\n");
}

#[test]
#[ignore = "SEF proposal not offered yet: field binding is_from_source() is false for the bridge field binding (availability check)"]
fn test_invisible_field_to_getter_setter_2() {
    invisible_field("        c.test += 1 + 2;\n", "        c.setTest(c.getTest() + (1 + 2));\n");
}

#[test]
#[ignore = "SEF proposal not offered yet: field binding is_from_source() is false for the bridge field binding (availability check)"]
fn test_invisible_field_to_getter_setter_3() {
    invisible_field("        c.test -= 1 + 2;\n", "        c.setTest(c.getTest() - (1 + 2));\n");
}

#[test]
#[ignore = "SEF proposal not offered yet: field binding is_from_source() is false for the bridge field binding (availability check)"]
fn test_invisible_field_to_getter_setter_4() {
    invisible_field("        c.test *= 1 + 2;\n", "        c.setTest(c.getTest() * (1 + 2));\n");
}

#[test]
fn test_invisible_field_to_getter_setter_5() {
    let (mut t, root) = setup();
    let accessors = [
        "\n",
        "    /**\n",
        "     * @return the test\n",
        "     */\n",
        "    public int getTest() {\n",
        "        return test;\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @param test the test to set\n",
        "     */\n",
        "    public void setTest(int test) {\n",
        "        this.test = test;\n",
        "    }\n",
    ];
    let unit = |field: &str, statement: &str| {
        let mut v = vec!["package test;\n", "public class C {\n", field];
        v.extend_from_slice(&accessors);
        v.extend_from_slice(D_TAIL);
        v.extend_from_slice(&[statement, "    }\n", "}\n"]);
        lines(&v)
    };
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", &unit("    private int test;\n", "        ++c.test;\n"));
    let e1 = Expected::new("Change visibility of 'test' to 'package'", &unit("    int test;\n", "        ++c.test;\n"));
    let e2 = Expected::new("Replace c.test with setter", &unit("    private int test;\n", "        c.setTest(c.getTest() + 1);\n"));
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "SEF proposal not offered yet: field binding is_from_source() is false for the bridge field binding (availability check)"]
fn test_create_field_using_sef() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        &lines(&[
            "package test;\n",
            "public class A {\n",
            "    private int t;\n",
            "    {\n",
            "        System.out.println(t);\n",
            "    }\n",
            "}\n",
            "class B {\n",
            "    {\n",
            "        new A().t = 5;\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e1 = Expected::new(
        "Change visibility of 't' to 'package'",
        &lines(&[
            "package test;\n",
            "public class A {\n",
            "    int t;\n",
            "    {\n",
            "        System.out.println(t);\n",
            "    }\n",
            "}\n",
            "class B {\n",
            "    {\n",
            "        new A().t = 5;\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e2 = Expected::new(
        "Create getter and setter for 't'...",
        &lines(&[
            "package test;\n",
            "public class A {\n",
            "    private int t;\n",
            "    {\n",
            "        System.out.println(getT());\n",
            "    }\n",
            "    public int getT() {\n",
            "        return t;\n",
            "        \n",
            "    }\n",
            "    public void setT(int t) {\n",
            "        this.t = t;\n",
            "        \n",
            "    }\n",
            "}\n",
            "class B {\n",
            "    {\n",
            "        new A().setT(5);\n",
            "    }\n",
            "}\n",
        ]),
    );
    t.assert_code_actions(&cu, &[e1, e2]);
}
