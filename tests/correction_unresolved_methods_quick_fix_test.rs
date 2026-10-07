//! Port of `org.eclipse.jdt.ls.core.internal.correction.UnresolvedMethodsQuickFixTest`.
mod common;
use std::path::PathBuf;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
use serde_json::json;

/// `UnresolvedMethodsQuickFixTest.setup`: `newEmptyProject()` with the
/// test options plus `COMPILER_PB_NO_EFFECT_ASSIGNMENT = ignore` and
/// `COMPILER_PB_INDIRECT_STATIC_ACCESS = error`, and empty catch block,
/// constructor and method stub templates (`StubUtility.setCodeTemplate`).
fn setup_with(favorites: &[&str]) -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    if !favorites.is_empty() {
        t.ws.settings["java"]["completion"] = json!({ "favoriteStaticMembers": favorites });
    }
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.noEffectAssignment".into(), "ignore".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.indirectStaticAccess".into(), "error".into());
    let root = t.ws.new_empty_project(&options);
    let mut xml = String::from("<templates>");
    for key in ["catchbody", "constructorbody", "methodbody"] {
        xml.push_str(&format!("<template id=\"org.eclipse.jdt.ui.text.codetemplates.{key}\" name=\"{key}\" description=\"{key}\" context=\"{key}_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\"></template>"));
    }
    xml.push_str("</templates>");
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!("eclipse.preferences.version=1\norg.eclipse.jdt.ui.text.custom_code_templates={xml}\n"),
    )
    .unwrap();
    if !favorites.is_empty() {
        // `PreferenceManager.getPrefs(null).setJavaCompletionFavoriteMembers`
        // changes the live preferences; jdt.ls copies the favorites of the
        // *current* preferences into the JavaManipulation node, so they reach
        // it with a configuration change on the running server.
        t.ws.client();
        let settings = t.ws.settings.clone();
        t.ws.update_settings(settings);
    }
    (t, root)
}

fn setup() -> (QuickFixTest, PathBuf) {
    setup_with(&[])
}

#[test]
fn test_method_in_same_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_for_init() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    void foo() {\n        for (int i= 0, j= goo(3); i < 0; i++) {\n        }\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(int)'", "package test1;\npublic class E {\n    void foo() {\n        for (int i= 0, j= goo(3); i < 0; i++) {\n        }\n    }\n\n    private int goo(int i) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_infix_expression1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private boolean foo() {\n        return f(1) || f(2);\n    }\n}\n");
    let e1 = Expected::new("Create method 'f(int)'", "package test1;\npublic class E {\n    private boolean foo() {\n        return f(1) || f(2);\n    }\n\n    private boolean f(int i) {\n        return false;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_infix_expression2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private boolean foo() {\n        return f(1) == f(2);\n    }\n}\n");
    let e1 = Expected::new("Create method 'f(int)'", "package test1;\npublic class E {\n    private boolean foo() {\n        return f(1) == f(2);\n    }\n\n    private Object f(int i) {\n        return null;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing0_empty_lines() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing1_empty_line() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing2_empty_lines() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n    \n    \n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n    \n    \n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing_comment() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n//comment\n\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n//comment\n\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing_javadoc() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    /**\n     * javadoc\n     */\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    /**\n     * javadoc\n     */\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_spacing_non_javadoc() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    /*\n     * non javadoc\n     */\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n\n    void fred() {\n    }\n\n    /*\n     * non javadoc\n     */\n    void foo(Vector vec) {\n        int i= goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_same_type_using_this() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector vec) {\n        int i= this.goo(vec, true);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Vector, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector vec) {\n        int i= this.goo(vec, true);\n    }\n\n    private int goo(Vector vec, boolean b) {\n        return 0;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_different_class() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    void foo(X x) {\n        if (x instanceof Y) {\n            boolean i= x.goo(1, 2.1);\n        }\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n}\n");
    t.ws.create_cu(&root, "src", "test1", "Y.java", "package test1;\npublic interface Y {\n    public boolean goo(int i, double d);\n}\n");
    let e1 = Expected::new("Create method 'goo(int, double)' in type 'X'", "package test1;\npublic class X {\n\n    public boolean goo(int i, double d) {\n        return false;\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'x'", "package test1;\npublic class E {\n    void foo(X x) {\n        if (x instanceof Y) {\n            boolean i= ((Y) x).goo(1, 2.1);\n        }\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_parameter_with_type_variable() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "Bork.java", "package test1;\npublic class Bork<T> {\n    private Help help = new Help();\n    public void method() {\n        help.help(this);\n    }\n}\n\nclass Help {\n}\n");
    let e1 = Expected::new("Create method 'help(Bork<T>)' in type 'Help'", "package test1;\npublic class Bork<T> {\n    private Help help = new Help();\n    public void method() {\n        help.help(this);\n    }\n}\n\nclass Help {\n\n    public void help(Bork<T> bork) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_parameter_anonymous() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public E() {\n        foo(new Runnable() {\n            public void run() {}\n        });\n    }\n}\n");
    let e1 = Expected::new("Create method 'foo(Runnable)'", "package test1;\npublic class E {\n    public E() {\n        foo(new Runnable() {\n            public void run() {}\n        });\n    }\n\n    private void foo(Runnable runnable) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_generic_type() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X<A> {\n}\n");
    t.ws.create_cu(&root, "src", "test1", "Y.java", "package test1;\npublic interface Y<A> {\n    public boolean goo(X<A> a);\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public Y<Object> y;\n    void foo(X<String> x) {\n        boolean i= x.goo(x);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(X<String>)' in type 'X'", "package test1;\npublic class X<A> {\n\n    public boolean goo(X<String> x) {\n        return false;\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'x'", "package test1;\npublic class E {\n    public Y<Object> y;\n    void foo(X<String> x) {\n        boolean i= ((Y<Object>) x).goo(x);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_assigned_to_wildcard() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? extends Number> vec) {\n        vec.add(goo());\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo()'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? extends Number> vec) {\n        vec.add(goo());\n    }\n\n    private Object goo() {\n        return null;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_assigned_to_wildcard2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? super Number> vec) {\n        vec.add(goo());\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo()'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? super Number> vec) {\n        vec.add(goo());\n    }\n\n    private Number goo() {\n        return null;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_assigned_from_wildcard1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? super Number> vec) {\n        goo(vec.get(0));\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(Object)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void foo(Vector<? super Number> vec) {\n        goo(vec.get(0));\n    }\n\n    private void goo(Object object) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_assigned_from_wildcard2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    void testMethod(Vector<? extends Number> vec) {\n        goo(vec.get(0));\n    }\n\n    private void goo(int i) {\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(int)' to 'goo(Number)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void testMethod(Vector<? extends Number> vec) {\n        goo(vec.get(0));\n    }\n\n    private void goo(Number number) {\n    }\n}\n");
    let e2 = Expected::new("Cast argument 'vec.get(0)' to 'int'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void testMethod(Vector<? extends Number> vec) {\n        goo((int) vec.get(0));\n    }\n\n    private void goo(int i) {\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(Number)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    void testMethod(Vector<? extends Number> vec) {\n        goo(vec.get(0));\n    }\n\n    private void goo(Number number) {\n    }\n\n    private void goo(int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_method_in_generic_type_same_cu() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public class X<A> {\n    }\n    int foo(X<String> x) {\n        return x.goo(x);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(X<String>)' in type 'X'", "package test1;\npublic class E {\n    public class X<A> {\n\n        public int goo(X<String> x) {\n            return 0;\n        }\n    }\n    int foo(X<String> x) {\n        return x.goo(x);\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'x'", "package test1;\npublic class E {\n    public class X<A> {\n    }\n    int foo(X<String> x) {\n        return ((Object) x).goo(x);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_in_raw_type() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X<A> {\n}\n");
    t.ws.create_cu(&root, "src", "test1", "Y.java", "package test1;\npublic interface Y<A> {\n    public boolean goo(X<A> a);\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public Y<Object> y;\n    void foo(X x) {\n        boolean i= x.goo(x);\n    }\n}\n");
    let e1 = Expected::new("Create method 'goo(X)' in type 'X'", "package test1;\npublic class X<A> {\n\n    public boolean goo(X x) {\n        return false;\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'x'", "package test1;\npublic class E {\n    public Y<Object> y;\n    void foo(X x) {\n        boolean i= ((Y<Object>) x).goo(x);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_in_anonymous1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Change to 'foo(..)'", "package test1;\npublic class E {\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                foo();\n            }\n        };\n    }\n}\n");
    let e2 = Expected::new("Create method 'xoo()'", "package test1;\npublic class E {\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n\n            private void xoo() {\n            }\n        };\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo()' in type 'E'", "package test1;\npublic class E {\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n        };\n    }\n\n    protected void xoo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_method_in_anonymous2() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "other", "A.java", "package other;\npublic class A {\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport other.A;\npublic class E {\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                A.xoo();\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Create method 'xoo()' in type 'A'", "package other;\npublic class A {\n\n    public static void xoo() {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_method_in_anonymous3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public static void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Change to 'foo(..)'", "package test1;\npublic class E {\n    public static void foo() {\n        new Runnable() {\n            public void run() {\n                foo();\n            }\n        };\n    }\n}\n");
    let e2 = Expected::new("Create method 'xoo()'", "package test1;\npublic class E {\n    public static void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n\n            private void xoo() {\n            }\n        };\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo()' in type 'E'", "package test1;\npublic class E {\n    public static void foo() {\n        new Runnable() {\n            public void run() {\n                xoo();\n            }\n        };\n    }\n\n    protected static void xoo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_method_in_anonymous4() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public static void foo(final E e) {\n        new Runnable() {\n            public void run() {\n                e.foobar();\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Create method 'foobar()' in type 'E'", "package test1;\npublic class E {\n    public static void foo(final E e) {\n        new Runnable() {\n            public void run() {\n                e.foobar();\n            }\n        };\n    }\n\n    protected void foobar() {\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'e'", "package test1;\npublic class E {\n    public static void foo(final E e) {\n        new Runnable() {\n            public void run() {\n                ((Object) e).foobar();\n            }\n        };\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_in_anonymous_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E<T> {\n    public void foo() {\n        new Comparable<String>() {\n            public int compareTo(String s) {\n                xoo();\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Change to 'foo(..)'", "package test1;\npublic class E<T> {\n    public void foo() {\n        new Comparable<String>() {\n            public int compareTo(String s) {\n                foo();\n            }\n        };\n    }\n}\n");
    let e2 = Expected::new("Create method 'xoo()'", "package test1;\npublic class E<T> {\n    public void foo() {\n        new Comparable<String>() {\n            public int compareTo(String s) {\n                xoo();\n            }\n\n            private void xoo() {\n            }\n        };\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo()' in type 'E<T>'", "package test1;\npublic class E<T> {\n    public void foo() {\n        new Comparable<String>() {\n            public int compareTo(String s) {\n                xoo();\n            }\n        };\n    }\n\n    protected void xoo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_method_in_anonymous_covering1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run(1);\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Qualify with enclosing type 'E'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                E.this.run(1);\n            }\n        };\n    }\n}\n");
    let e2 = Expected::new("Remove argument to match 'run()'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run();\n            }\n        };\n    }\n}\n");
    let e3 = Expected::new("Change method 'run()': Add parameter 'int'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run(int i) {\n                run(1);\n            }\n        };\n    }\n}\n");
    let e4 = Expected::new("Create method 'run(int)'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run(1);\n            }\n\n            private void run(int i) {\n            }\n        };\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3, e4]);
}

#[test]
fn test_method_in_anonymous_covering2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public static void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run(1);\n            }\n        };\n    }\n}\n");
    let e1 = Expected::new("Qualify with enclosing type 'E'", "package test1;\npublic class E {\n    public static void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                E.run(1);\n            }\n        };\n    }\n}\n");
    let e2 = Expected::new("Remove argument to match 'run()'", "package test1;\npublic class E {\n    public static void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run();\n            }\n        };\n    }\n}\n");
    let e3 = Expected::new("Change method 'run()': Add parameter 'int'", "package test1;\npublic class E {\n    public static void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run(int i) {\n                run(1);\n            }\n        };\n    }\n}\n");
    let e4 = Expected::new("Create method 'run(int)'", "package test1;\npublic class E {\n    public static void run(int i) {\n    }\n    public void foo() {\n        new Runnable() {\n            public void run() {\n                run(1);\n            }\n\n            private void run(int i) {\n            }\n        };\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3, e4]);
}

#[test]
fn test_method_in_anonymous_covering3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public class Inner {\n        public void run() {\n            run(1);\n        }\n    }\n}\n");
    let e1 = Expected::new("Qualify with enclosing type 'E'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public class Inner {\n        public void run() {\n            E.this.run(1);\n        }\n    }\n}\n");
    let e2 = Expected::new("Remove argument to match 'run()'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public class Inner {\n        public void run() {\n            run();\n        }\n    }\n}\n");
    let e3 = Expected::new("Change method 'run()': Add parameter 'int'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public class Inner {\n        public void run(int i) {\n            run(1);\n        }\n    }\n}\n");
    let e4 = Expected::new("Create method 'run(int)'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public class Inner {\n        public void run() {\n            run(1);\n        }\n\n        private void run(int i) {\n        }\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3, e4]);
}

#[test]
fn test_method_in_anonymous_covering4() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public static class Inner {\n        public void run() {\n            run(1);\n        }\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'run()'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public static class Inner {\n        public void run() {\n            run();\n        }\n    }\n}\n");
    let e2 = Expected::new("Change method 'run()': Add parameter 'int'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public static class Inner {\n        public void run(int i) {\n            run(1);\n        }\n    }\n}\n");
    let e3 = Expected::new("Create method 'run(int)'", "package test1;\npublic class E {\n    public void run(int i) {\n    }\n    public static class Inner {\n        public void run() {\n            run(1);\n        }\n\n        private void run(int i) {\n        }\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_method_in_different_interface() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    void foo(X x) {\n        boolean i= x.goo(getClass());\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic interface X {\n}\n");
    let e1 = Expected::new("Create method 'goo(Class<? extends E>)' in type 'X'", "package test1;\npublic interface X {\n\n    boolean goo(Class<? extends E> class1);\n}\n");
    let e2 = Expected::new("Add cast to 'x'", "package test1;\npublic class E {\n    void foo(X x) {\n        boolean i= ((Object) x).goo(getClass());\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_in_array_access() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "p", "E.java", "package p;\n\npublic class E {\n    void foo() {\n        int i = bar()[0];\n    }\n}\n");
    let e1 = Expected::new("Create method 'bar()'", "package p;\n\npublic class E {\n    void foo() {\n        int i = bar()[0];\n    }\n\n    private int[] bar() {\n        return null;\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_parameter_mismatch_cast() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        long x= 0;\n        foo(x + 1);\n    }\n}\n");
    let e1 = Expected::new("Change method 'foo(int)' to 'foo(long)'", "package test1;\npublic class E {\n    public void foo(long l) {\n        long x= 0;\n        foo(x + 1);\n    }\n}\n");
    let e2 = Expected::new("Cast argument 'x + 1' to 'int'", "package test1;\npublic class E {\n    public void foo(int i) {\n        long x= 0;\n        foo((int) (x + 1));\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(long)'", "package test1;\npublic class E {\n    public void foo(int i) {\n        long x= 0;\n        foo(x + 1);\n    }\n\n    private void foo(long l) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_cast2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        double x= 0.0;\n        X.xoo((float) x, this);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public static void xoo(int i, Object o) {\n    }\n}\n");
    let e1 = Expected::new("Change method 'xoo(int, Object)' to 'xoo(float, Object)'", "package test1;\npublic class X {\n    public static void xoo(float x, Object o) {\n    }\n}\n");
    let e2 = Expected::new("Cast argument '(float)x' to 'int'", "package test1;\npublic class E {\n    public void foo(int i) {\n        double x= 0.0;\n        X.xoo((int) x, this);\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo(float, E)' in type 'X'", "package test1;\npublic class X {\n    public static void xoo(int i, Object o) {\n    }\n\n    public static void xoo(float x, E o) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_cast_boxing() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(Integer i) {\n        foo(1.0);\n    }\n}\n");
    let e1 = Expected::new("Change method 'foo(Integer)' to 'foo(double)'", "package test1;\npublic class E {\n    public void foo(double d) {\n        foo(1.0);\n    }\n}\n");
    let e2 = Expected::new("Cast argument '1.0' to 'int'", "package test1;\npublic class E {\n    public void foo(Integer i) {\n        foo((int) 1.0);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(double)'", "package test1;\npublic class E {\n    public void foo(Integer i) {\n        foo(1.0);\n    }\n\n    private void foo(double d) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_change_var_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        long x= 0;\n        goo(x);\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector)' to 'goo(long)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(long x) {\n    }\n    public void foo() {\n        long x= 0;\n        goo(x);\n    }\n}\n");
    let e2 = Expected::new("Change type of 'x' to 'Vector'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        Vector x= 0;\n        goo(x);\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(long)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        long x= 0;\n        goo(x);\n    }\n    private void goo(long x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_change_var_type_in_generic() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\nimport java.util.Vector;\npublic class A<T> {\n    public void goo(Vector<T> v) {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E<T> {\n    public void foo(A<Number> a, long x) {\n        a.goo(x);\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector<T>)' to 'goo(long)'", "package test1;\nimport java.util.Vector;\npublic class A<T> {\n    public void goo(long x) {\n    }\n}\n");
    let e2 = Expected::new("Change type of 'x' to 'Vector<Number>'", "package test1;\n\nimport java.util.Vector;\n\npublic class E<T> {\n    public void foo(A<Number> a, Vector<Number> x) {\n        a.goo(x);\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(long)' in type 'A'", "package test1;\nimport java.util.Vector;\npublic class A<T> {\n    public void goo(Vector<T> v) {\n    }\n\n    public void goo(long x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_keep_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\n\nimport java.util.Collections;\n\nclass E {\n    void foo(@Deprecated final String map){}\n    {foo(Collections.EMPTY_MAP);}\n}\n");
    let e1 = Expected::new("Change method 'foo(String)' to 'foo(Map)'", "package test1;\n\nimport java.util.Collections;\nimport java.util.Map;\n\nclass E {\n    void foo(@Deprecated final Map emptyMap){}\n    {foo(Collections.EMPTY_MAP);}\n}\n");
    let e2 = Expected::new("Create method 'foo(Map)'", "package test1;\n\nimport java.util.Collections;\nimport java.util.Map;\n\nclass E {\n    void foo(@Deprecated final String map){}\n    {foo(Collections.EMPTY_MAP);}\n    private void foo(Map emptyMap) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_parameter_mismatch_change_field_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    int fCount= 0;\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        goo(fCount);\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector)' to 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    int fCount= 0;\n    public void goo(int fCount2) {\n    }\n    public void foo() {\n        goo(fCount);\n    }\n}\n");
    let e2 = Expected::new("Change type of 'fCount' to 'Vector'", "package test1;\nimport java.util.Vector;\npublic class E {\n    Vector fCount= 0;\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        goo(fCount);\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    int fCount= 0;\n    public void goo(Vector v) {\n    }\n    public void foo() {\n        goo(fCount);\n    }\n    private void goo(int fCount2) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_change_field_type_in_generic() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X<A> {\n    String count= 0;\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector<String> v) {\n    }\n    public void foo(X<String> x, int y) {\n        goo(x.count);\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector<String>)' to 'goo(String)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(String count) {\n    }\n    public void foo(X<String> x, int y) {\n        goo(x.count);\n    }\n}\n");
    let e2 = Expected::new("Change type of 'count' to 'Vector<String>'", "package test1;\n\nimport java.util.Vector;\n\npublic class X<A> {\n    Vector<String> count= 0;\n}\n");
    let e3 = Expected::new("Create method 'goo(String)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector<String> v) {\n    }\n    public void foo(X<String> x, int y) {\n        goo(x.count);\n    }\n    private void goo(String count) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_change_method_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector)' to 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(int i) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e2 = Expected::new("Change return type of 'foo(..)' to 'Vector'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public Vector foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public void goo(Vector v) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n    private void goo(int foo) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_change_method_type_bug102142() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "Foo.java", "package test1;\npublic class Foo {\n    Foo(String string) {\n        System.out.println(string);\n    }  \n    private void bar() {\n        new Foo(3);\n    }\n}\n");
    let e1 = Expected::new("Change constructor 'Foo(String)' to 'Foo(int)'", "package test1;\npublic class Foo {\n    Foo(int i) {\n        System.out.println(i);\n    }  \n    private void bar() {\n        new Foo(3);\n    }\n}\n");
    let e2 = Expected::new("Create constructor 'Foo(int)'", "package test1;\npublic class Foo {\n    Foo(String string) {\n        System.out.println(string);\n    }  \n    public Foo(int i) {\n    }\n    private void bar() {\n        new Foo(3);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_parameter_mismatch_change_method_type_in_generic() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public void goo(Vector<String> v) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e1 = Expected::new("Change method 'goo(Vector<String>)' to 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public void goo(int i) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e2 = Expected::new("Change return type of 'foo(..)' to 'Vector<String>'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public void goo(Vector<String> v) {\n    }\n    public Vector<String> foo() {\n        goo(this.foo());\n        return 9;\n    }\n}\n");
    let e3 = Expected::new("Create method 'goo(int)'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public void goo(Vector<String> v) {\n    }\n    public int foo() {\n        goo(this.foo());\n        return 9;\n    }\n    private void goo(int foo) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_less_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(String s, int i, Object o) {\n        int x= 0;\n        foo(x);\n    }\n}\n");
    let e1 = Expected::new("Add arguments to match 'foo(String, int, Object)'", "package test1;\npublic class E {\n    public void foo(String s, int i, Object o) {\n        int x= 0;\n        foo(s, x, o);\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(String, int, Object)': Remove parameters 'String, Object'", "package test1;\npublic class E {\n    public void foo(int i) {\n        int x= 0;\n        foo(x);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(int)'", "package test1;\npublic class E {\n    public void foo(String s, int i, Object o) {\n        int x= 0;\n        foo(x);\n    }\n\n    private void foo(int x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_less_arguments2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        X.xoo(null);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public static void xoo(int i, Object o) {\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'xoo(int, Object)'", "package test1;\npublic class E {\n    public void foo() {\n        X.xoo(0, null);\n    }\n}\n");
    let e2 = Expected::new("Change method 'xoo(int, Object)': Remove parameter 'int'", "package test1;\npublic class X {\n    public static void xoo(Object o) {\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo(Object)' in type 'X'", "package test1;\npublic class X {\n    public static void xoo(int i, Object o) {\n    }\n\n    public static void xoo(Object object) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_less_arguments3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        X.xoo(1);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    /**\n     * @param i The int value\n     *                  More about the int value\n     * @param o The Object value\n     */\n    public static void xoo(int i, Object o) {\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'xoo(int, Object)'", "package test1;\npublic class E {\n    public void foo() {\n        X.xoo(1, null);\n    }\n}\n");
    let e2 = Expected::new("Change method 'xoo(int, Object)': Remove parameter 'Object'", "package test1;\npublic class X {\n    /**\n     * @param i The int value\n     *                  More about the int value\n     */\n    public static void xoo(int i) {\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo(int)' in type 'X'", "package test1;\npublic class X {\n    /**\n     * @param i The int value\n     *                  More about the int value\n     * @param o The Object value\n     */\n    public static void xoo(int i, Object o) {\n    }\n\n    public static void xoo(int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_less_arguments_in_generic() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic interface X<S, T extends Number> {\n    public void foo(S s, int i, T t);\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic abstract class E implements X<String, Integer> {\n    public void meth(E e, String s) {\n        int x= 0;\n        e.foo(x);\n    }\n}\n");
    let e1 = Expected::new("Add arguments to match 'foo(String, int, Integer)'", "package test1;\npublic abstract class E implements X<String, Integer> {\n    public void meth(E e, String s) {\n        int x= 0;\n        e.foo(s, x, x);\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(S, int, T)': Remove parameters 'S, T'", "package test1;\npublic interface X<S, T extends Number> {\n    public void foo(int i);\n}\n");
    let e3 = Expected::new("Create method 'foo(int)'", "package test1;\npublic abstract class E implements X<String, Integer> {\n    public void meth(E e, String s) {\n        int x= 0;\n        e.foo(x);\n    }\n\n    private void foo(int x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_super_constructor_less_arguments() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public X(Object o, int i) {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public E() {\n        super(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'X(Object, int)'", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public E() {\n        super(new Vector(), 0);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'X(Object, int)': Remove parameter 'int'", "package test1;\npublic class X {\n    public X(Object o) {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'X(Vector)'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public X(Object o, int i) {\n    }\n\n    public X(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_constructor_invocation_less_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'E(Object, int)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector(), 0);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'E(Object, int)': Remove parameter 'int'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E(Object o) {\n    }\n    public E() {\n        this(new Vector());\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'E(Vector)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector());\n    }\n    public E(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_constructor_invocation_less_arguments_in_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'E(Object, int)'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector(), 0);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'E(Object, int)': Remove parameter 'int'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public E(Object o) {\n    }\n    public E() {\n        this(new Vector());\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'E<T>(Vector)'", "package test1;\nimport java.util.Vector;\npublic class E<T> {\n    public E(Object o, int i) {\n    }\n    public E() {\n        this(new Vector());\n    }\n    public E(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_more_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(X x) {\n        x.xoo(1, 1, x.toString());\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public void xoo(int i, String o) {\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'xoo(int, String)'", "package test1;\npublic class E {\n    public void foo(X x) {\n        x.xoo(1, x.toString());\n    }\n}\n");
    let e2 = Expected::new("Change method 'xoo(int, String)': Add parameter 'int'", "package test1;\npublic class X {\n    public void xoo(int i, int j, String o) {\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo(int, int, String)' in type 'X'", "package test1;\npublic class X {\n    public void xoo(int i, String o) {\n    }\n\n    public void xoo(int i, int j, String string) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_more_arguments2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(String s) {\n        int x= 0;\n        foo(s, x);\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'foo(String)'", "package test1;\npublic class E {\n    public void foo(String s) {\n        int x= 0;\n        foo(s);\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(String)': Add parameter 'int'", "package test1;\npublic class E {\n    public void foo(String s, int x2) {\n        int x= 0;\n        foo(s, x);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(String, int)'", "package test1;\npublic class E {\n    public void foo(String s) {\n        int x= 0;\n        foo(s, x);\n    }\n\n    private void foo(String s, int x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_more_arguments3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Collections;\npublic class E {\n    public void foo(X x) {\n        x.xoo(Collections.EMPTY_SET, 1, 2);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    /**\n     * @param i The int value\n     */\n    public void xoo(int i) {\n       int j= 0;\n    }\n}\n");
    let e1 = Expected::new("Remove arguments to match 'xoo(int)'", "package test1;\nimport java.util.Collections;\npublic class E {\n    public void foo(X x) {\n        x.xoo(1);\n    }\n}\n");
    let e2 = Expected::new("Change method 'xoo(int)': Add parameters 'Set, int'", "package test1;\n\nimport java.util.Set;\n\npublic class X {\n    /**\n     * @param emptySet \n     * @param i The int value\n     * @param k \n     */\n    public void xoo(Set emptySet, int i, int k) {\n       int j= 0;\n    }\n}\n");
    let e3 = Expected::new("Create method 'xoo(Set, int, int)' in type 'X'", "package test1;\n\nimport java.util.Set;\n\npublic class X {\n    /**\n     * @param i The int value\n     */\n    public void xoo(int i) {\n       int j= 0;\n    }\n\n    public void xoo(Set emptySet, int i, int j) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_more_arguments4() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object[] o= null;\n        foo(o.length);\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'foo()'", "package test1;\npublic class E {\n    public void foo() {\n        Object[] o= null;\n        foo();\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo()': Add parameter 'int'", "package test1;\npublic class E {\n    public void foo(int length) {\n        Object[] o= null;\n        foo(o.length);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(int)'", "package test1;\npublic class E {\n    public void foo() {\n        Object[] o= null;\n        foo(o.length);\n    }\n\n    private void foo(int length) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_more_arguments_in_generic() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E<T> {\n    public void foo(X<T> x) {\n        x.xoo(x.toString(), x, 2);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X<T> {\n    /**\n     * @param i The int value\n     */\n    public void xoo(String s) {\n    }\n}\n");
    let e1 = Expected::new("Remove arguments to match 'xoo(String)'", "package test1;\npublic class E<T> {\n    public void foo(X<T> x) {\n        x.xoo(x.toString());\n    }\n}\n");
    let e2 = Expected::new("Create method 'xoo(String, X<T>, int)' in type 'X'", "package test1;\npublic class X<T> {\n    /**\n     * @param i The int value\n     */\n    public void xoo(String s) {\n    }\n\n    public void xoo(String string, X<T> x, int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_super_constructor_more_arguments() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public X() {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public E() {\n        super(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'X()'", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public E() {\n        super();\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'X()': Add parameter 'Vector'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public X(Vector vector) {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'X(Vector)'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public X() {\n    }\n\n    public X(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_constructor_invocation_more_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E() {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'E()'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E() {\n    }\n    public E(Object o, int i) {\n        this();\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'E()': Add parameter 'Vector'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E(Vector vector) {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'E(Vector)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    public E() {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n    public E(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_constructor_invocation_more_arguments2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E {\n    /**\n     * My favourite constructor.\n     */\n    public E() {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'E()'", "package test1;\nimport java.util.Vector;\npublic class E {\n    /**\n     * My favourite constructor.\n     */\n    public E() {\n    }\n    public E(Object o, int i) {\n        this();\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'E()': Add parameter 'Vector'", "package test1;\nimport java.util.Vector;\npublic class E {\n    /**\n     * My favourite constructor.\n     * @param vector \n     */\n    public E(Vector vector) {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'E(Vector)'", "package test1;\nimport java.util.Vector;\npublic class E {\n    /**\n     * My favourite constructor.\n     */\n    public E() {\n    }\n    public E(Object o, int i) {\n        this(new Vector());\n    }\n    public E(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_swap() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i, String[] o) {\n        foo(new String[] { }, i - 1);\n    }\n}\n");
    let e1 = Expected::new("Swap arguments 'new String[]{}' and 'i - 1'", "package test1;\npublic class E {\n    public void foo(int i, String[] o) {\n        foo(i - 1, new String[] { });\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(int, String[])': Swap parameters 'int, String[]'", "package test1;\npublic class E {\n    public void foo(String[] o, int i) {\n        foo(new String[] { }, i - 1);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(String[], int)'", "package test1;\npublic class E {\n    public void foo(int i, String[] o) {\n        foo(new String[] { }, i - 1);\n    }\n\n    private void foo(String[] strings, int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_swap_in_generic_type() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A<T> {\n    public void b(int i, T[] t) {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic enum E {\n    CONST1, CONST2;\n    public void foo(A<String> a) {\n        a.b(new String[1], 1);\n    }\n}\n");
    let e1 = Expected::new("Swap arguments 'new String[1]' and '1'", "package test1;\npublic enum E {\n    CONST1, CONST2;\n    public void foo(A<String> a) {\n        a.b(1, new String[1]);\n    }\n}\n");
    let e2 = Expected::new("Change method 'b(int, T[])': Swap parameters 'int, T[]'", "package test1;\npublic class A<T> {\n    public void b(T[] t, int i) {\n    }\n}\n");
    let e3 = Expected::new("Create method 'b(String[], int)' in type 'A'", "package test1;\npublic class A<T> {\n    public void b(int i, T[] t) {\n    }\n\n    public void b(String[] strings, int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_with_extra_dimensions() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "ArrayTest.java", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a);\n        }\n        private void foo(int a[]) {\n        }\n}\n");
    let e1 = Expected::new("Change method 'foo(int[])' to 'foo(String[])'", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a);\n        }\n        private void foo(String[] a) {\n        }\n}\n");
    let e2 = Expected::new("Change type of 'a' to 'int[]'", "package test1;\npublic class ArrayTest {\n        public void test(int[] a){\n                foo(a);\n        }\n        private void foo(int a[]) {\n        }\n}\n");
    let e3 = Expected::new("Create method 'foo(String[])'", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a);\n        }\n        private void foo(String[] a) {\n        }\n        private void foo(int a[]) {\n        }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_parameter_mismatch_with_var_args() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "ArrayTest.java", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a, a);\n        }\n        private void foo(int[] a, int... i) {\n        }\n}\n");
    let e1 = Expected::new("Change method 'foo(int[], int...)' to 'foo(String[], String...)'", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a, a);\n        }\n        private void foo(String[] a, String... a2) {\n        }\n}\n");
    let e2 = Expected::new("Create method 'foo(String[], String[])'", "package test1;\npublic class ArrayTest {\n        public void test(String[] a){\n                foo(a, a);\n        }\n        private void foo(String[] a, String[] a2) {\n        }\n        private void foo(int[] a, int... i) {\n        }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_parameter_mismatch_swap2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    /**\n     * @param i The int value\n     * @param o The Object value\n     * @param b The boolean value\n     *                  More about the boolean value\n     */\n    public void foo(int i, Object o, boolean b) {\n        foo(false, o, i - 1);\n    }\n}\n");
    let e1 = Expected::new("Swap arguments 'false' and 'i - 1'", "package test1;\npublic class E {\n    /**\n     * @param i The int value\n     * @param o The Object value\n     * @param b The boolean value\n     *                  More about the boolean value\n     */\n    public void foo(int i, Object o, boolean b) {\n        foo(i - 1, o, false);\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(int, Object, boolean)': Swap parameters 'int, boolean'", "package test1;\npublic class E {\n    /**\n     * @param b The boolean value\n     *                  More about the boolean value\n     * @param o The Object value\n     * @param i The int value\n     */\n    public void foo(boolean b, Object o, int i) {\n        foo(false, o, i - 1);\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(boolean, Object, int)'", "package test1;\npublic class E {\n    /**\n     * @param i The int value\n     * @param o The Object value\n     * @param b The boolean value\n     *                  More about the boolean value\n     */\n    public void foo(int i, Object o, boolean b) {\n        foo(false, o, i - 1);\n    }\n\n    private void foo(boolean b, Object o, int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_super_constructor() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E extends A {\n    public E(int i) {\n        super(i);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n}\n");
    let e1 = Expected::new("Remove argument to match 'A()'", "package test1;\npublic class E extends A {\n    public E(int i) {\n        super();\n    }\n}\n");
    let e2 = Expected::new("Create constructor 'A(int)'", "package test1;\npublic class A {\n\n    public A(int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_class_instance_creation() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(i);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n}\n");
    let e1 = Expected::new("Remove argument to match 'A()'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A();\n    }\n}\n");
    let e2 = Expected::new("Create constructor 'A(int)'", "package test1;\npublic class A {\n\n    public A(int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_class_instance_creation2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(\"test\");\n    }\n    class A {\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'A()'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A();\n    }\n    class A {\n    }\n}\n");
    let e2 = Expected::new("Create constructor 'A(String)'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(\"test\");\n    }\n    class A {\n\n        public A(String string) {\n        }\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_class_instance_creation_in_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        A<String> a= new A<String>(i);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A<T> {\n}\n");
    let e1 = Expected::new("Remove argument to match 'A<String>()'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A<String> a= new A<String>();\n    }\n}\n");
    let e2 = Expected::new("Create constructor 'A<T>(int)'", "package test1;\npublic class A<T> {\n\n    public A(int i) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_class_instance_creation_more_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(i, String.valueOf(i), true);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    public A(int i) {\n    }\n}\n");
    let e1 = Expected::new("Remove arguments to match 'A(int)'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(i);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'A(int)': Add parameters 'String, boolean'", "package test1;\npublic class A {\n    public A(int i, String string, boolean b) {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'A(int, String, boolean)'", "package test1;\npublic class A {\n    public A(int i) {\n    }\n\n    public A(int i, String valueOf, boolean b) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_class_instance_creation_more_arguments_in_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.List;\npublic class E {\n    public void foo(int i) {\n        A<List<? extends E>> a= new A<List<? extends E>>(i, String.valueOf(i), true);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A<T> {\n    public A(int i) {\n    }\n}\n");
    let e1 = Expected::new("Remove arguments to match 'A<List<? extends E>>(int)'", "package test1;\nimport java.util.List;\npublic class E {\n    public void foo(int i) {\n        A<List<? extends E>> a= new A<List<? extends E>>(i);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'A(int)': Add parameters 'String, boolean'", "package test1;\npublic class A<T> {\n    public A(int i, String string, boolean b) {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'A<T>(int, String, boolean)'", "package test1;\npublic class A<T> {\n    public A(int i) {\n    }\n\n    public A(int i, String valueOf, boolean b) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_class_instance_creation_less_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A();\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    public A(int i, String s) {\n    }\n}\n");
    let e1 = Expected::new("Add arguments to match 'A(int, String)'", "package test1;\npublic class E {\n    public void foo(int i) {\n        A a= new A(i, null);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'A(int, String)': Remove parameters 'int, String'", "package test1;\npublic class A {\n    public A() {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'A()'", "package test1;\npublic class A {\n    public A(int i, String s) {\n    }\n\n    public A() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_class_instance_creation_less_arguments_in_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.List;\npublic class E {\n    public void foo(int i) {\n        A<List<String>> a= new A<List<String>>();\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A<T> {\n    public A(int i, String s) {\n    }\n}\n");
    let e1 = Expected::new("Add arguments to match 'A<List<String>>(int, String)'", "package test1;\nimport java.util.List;\npublic class E {\n    public void foo(int i) {\n        A<List<String>> a= new A<List<String>>(i, null);\n    }\n}\n");
    let e2 = Expected::new("Change constructor 'A(int, String)': Remove parameters 'int, String'", "package test1;\npublic class A<T> {\n    public A() {\n    }\n}\n");
    let e3 = Expected::new("Create constructor 'A<T>()'", "package test1;\npublic class A<T> {\n    public A(int i, String s) {\n    }\n\n    public A() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_constructor_invocation() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public E(int i) {\n        this(i, true);\n    }\n}\n");
    let e1 = Expected::new("Create constructor 'E(int, boolean)'", "package test1;\npublic class E {\n    public E(int i) {\n        this(i, true);\n    }\n\n    public E(int i, boolean b) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_constructor_invocation_in_generic_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E<S, T> {\n    public E(int i) {\n        this(i, true);\n    }\n}\n");
    let e1 = Expected::new("Create constructor 'E<S, T>(int, boolean)'", "package test1;\npublic class E<S, T> {\n    public E(int i) {\n        this(i, true);\n    }\n\n    public E(int i, boolean b) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_super_method_invocation() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E extends A {\n    public void foo(int i) {\n        super.foo(i);\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n}\n");
    let e1 = Expected::new("Create method 'foo(int)' in type 'A'", "package test1;\npublic class A {\n\n    public void foo(int i) {\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_super_method_more_arguments() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public int foo() {\n        return 0;\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public void xoo() {\n        super.foo(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Remove argument to match 'foo()'", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public void xoo() {\n        super.foo();\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo()': Add parameter 'Vector'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public int foo(Vector vector) {\n        return 0;\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(Vector)' in type 'X'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public int foo() {\n        return 0;\n    }\n\n    public void foo(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_super_method_less_arguments() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "X.java", "package test1;\npublic class X {\n    public int foo(Object o, boolean b) {\n        return 0;\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public void xoo() {\n        super.foo(new Vector());\n    }\n}\n");
    let e1 = Expected::new("Add argument to match 'foo(Object, boolean)'", "package test1;\nimport java.util.Vector;\npublic class E extends X {\n    public void xoo() {\n        super.foo(new Vector(), false);\n    }\n}\n");
    let e2 = Expected::new("Change method 'foo(Object, boolean)': Remove parameter 'boolean'", "package test1;\npublic class X {\n    public int foo(Object o) {\n        return 0;\n    }\n}\n");
    let e3 = Expected::new("Create method 'foo(Vector)' in type 'X'", "package test1;\n\nimport java.util.Vector;\n\npublic class X {\n    public int foo(Object o, boolean b) {\n        return 0;\n    }\n\n    public void foo(Vector vector) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_missing_cast_parents1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(Object o) {\n        String x= (String) o.substring(1);\n    }\n}\n");
    let e1 = Expected::new("Add parentheses around cast", "package test1;\npublic class E {\n    public void foo(Object o) {\n        String x= ((String) o).substring(1);\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_missing_cast_parents2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(Object o) {\n        String x= (String) o.substring(1).toLowerCase();\n    }\n}\n");
    let e1 = Expected::new("Add parentheses around cast", "package test1;\npublic class E {\n    public void foo(Object o) {\n        String x= ((String) o).substring(1).toLowerCase();\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_missing_cast_parents3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private static Object obj;\n    public void foo() {\n        String x= (String) E.obj.substring(1).toLowerCase();\n    }\n}\n");
    let e1 = Expected::new("Add parentheses around cast", "package test1;\npublic class E {\n    private static Object obj;\n    public void foo() {\n        String x= ((String) E.obj).substring(1).toLowerCase();\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_array_access() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private static Object obj;\n    public String foo(Object[] array) {\n        return array.tostring();\n    }\n}\n");
    let e1 = Expected::new("Change to 'length'", "package test1;\npublic class E {\n    private static Object obj;\n    public String foo(Object[] array) {\n        return array.length;\n    }\n}\n");
    let e2 = Expected::new("Change to 'toString(..)'", "package test1;\npublic class E {\n    private static Object obj;\n    public String foo(Object[] array) {\n        return array.toString();\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_incomplete_throws_statement() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo(Object[] array) {\n        throw RuntimeException();\n    }\n}\n");
    let e1 = Expected::new("Insert 'new' keyword", "package test1;\npublic class E {\n    public void foo(Object[] array) {\n        throw new RuntimeException();\n    }\n}\n");
    let e2 = Expected::new("Create method 'RuntimeException()'", "package test1;\npublic class E {\n    public void foo(Object[] array) {\n        throw RuntimeException();\n    }\n\n    private Exception RuntimeException() {\n        return null;\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_annotation_attribute1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", "package pack;\npublic class E {\n    public @interface Annot {\n    }\n\n    @Annot(count= 1)\n    public void foo() {\n    }\n}\n");
    let e1 = Expected::new("Create attribute 'count()'", "package pack;\npublic class E {\n    public @interface Annot {\n\n        int count();\n    }\n\n    @Annot(count= 1)\n    public void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_annotation_attribute2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", "package pack;\npublic class E {\n    public @interface Annot {\n    }\n\n    @Annot(1)\n    public void foo() {\n    }\n}\n");
    let e1 = Expected::new("Create attribute 'value()'", "package pack;\npublic class E {\n    public @interface Annot {\n\n        int value();\n    }\n\n    @Annot(1)\n    public void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_static_import_favorite1() {
    let (mut t, root) = setup_with(&["java.lang.Math.*"]);
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", "package pack;\n\npublic class E {\n    private int foo() {\n        return max(1, 2);\n    }\n}\n");
    let e1 = Expected::new("Add static import for 'Math.max'", "package pack;\n\nimport static java.lang.Math.max;\n\npublic class E {\n    private int foo() {\n        return max(1, 2);\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_static_import_favorite2() {
    let (mut t, root) = setup_with(&["java.lang.Math.max"]);
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", "package pack;\n\npublic class E {\n    private int max() {\n        return max(1, 2);\n    }\n}\n");
    let e1 = Expected::new("Change to 'Math.max'", "package pack;\n\npublic class E {\n    private int max() {\n        return Math.max(1, 2);\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
#[ignore = "needs ModifierCorrectionSubProcessor.addNonAccessibleReferenceProposal (NotVisibleMethod visibility change), which is not ported yet"]
fn test_indirect_protected_method() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    protected void method() {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "B.java", "package test2;\nimport test1.A;\npublic class B extends A {\n    private void bMethod() {\n        A a = new A();\n        a.method();\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'method()' to 'public'", "package test1;\npublic class A {\n    public void method() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_static_method_in_interface1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "Snippet.java", "package test1;\ninterface Snippet {\n    public abstract String name();\n}\nclass Ref {\n    void foo(Snippet c) {\n        int[] v= Snippet.values();\n    }\n}\n");
    let e1 = Expected::new("Create method 'values()' in type 'Snippet'", "package test1;\ninterface Snippet {\n    public abstract String name();\n\n    public static int[] values() {\n        return null;\n    }\n}\nclass Ref {\n    void foo(Snippet c) {\n        int[] v= Snippet.values();\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_static_method_in_interface2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "Snippet.java", "package test1;\ninterface Snippet {\n    public abstract String name();\n}\ninterface Ref {\n   int[] v= Snippet.values();\n}\n");
    let e1 = Expected::new("Create method 'values()' in type 'Snippet'", "package test1;\ninterface Snippet {\n    public abstract String name();\n\n    public static int[] values() {\n        return null;\n    }\n}\ninterface Ref {\n   int[] v= Snippet.values();\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_static_method_in_interface3() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "XX.java", "package test1;\npublic class XX {\n    interface I {\n        int i= n();\n    }\n}\n");
    let e1 = Expected::new("Create method 'n()'", "package test1;\npublic class XX {\n    interface I {\n        int i= n();\n\n        static int n() {\n            return 0;\n        }\n    }\n}\n");
    let e2 = Expected::new("Create method 'n()' in type 'XX'", "package test1;\npublic class XX {\n    interface I {\n        int i= n();\n    }\n\n    protected static int n() {\n        return 0;\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_static_method_in_interface4() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "I.java", "package test1;\ninterface I {\n    int i= n();\n}\n");
    let e1 = Expected::new("Create method 'n()'", "package test1;\ninterface I {\n    int i= n();\n\n    static int n() {\n        return 0;\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_abstract_method_in_interface() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "Snippet.java", "package test1;\ninterface Snippet {\n    abstract String name();\n}\nclass Ref {\n    void foo(Snippet c) {\n        int[] v= c.values();\n    }\n}\n");
    let e1 = Expected::new("Create method 'values()' in type 'Snippet'", "package test1;\ninterface Snippet {\n    abstract String name();\n\n    abstract int[] values();\n}\nclass Ref {\n    void foo(Snippet c) {\n        int[] v= c.values();\n    }\n}\n");
    let e2 = Expected::new("Add cast to 'c'", "package test1;\ninterface Snippet {\n    abstract String name();\n}\nclass Ref {\n    void foo(Snippet c) {\n        int[] v= ((Object) c).values();\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}
