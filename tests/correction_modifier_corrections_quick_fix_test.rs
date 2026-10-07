//! Port of `org.eclipse.jdt.ls.core.internal.correction.ModifierCorrectionsQuickFixTest`.
mod common;
use std::collections::BTreeMap;
use std::path::PathBuf;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

const PROPOSAL: &str = "Remove invalid modifiers";

/// `setup`: `newEmptyProject()` with `TestOptions.getDefaultOptions()`.
///
/// The upstream test's mocked `PreferenceManager` never pushes the
/// `java.format.insertSpaces` default into the JavaCore options, so units
/// without project formatter options indent with the JavaCore default (tabs).
fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.settings["java"]["format"] = serde_json::json!({ "insertSpaces": false });
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

/// `setup` followed by `fJProject.setOptions(options)` where `options` only
/// holds `JavaModelUtil.setComplianceOptions(options, version)`
/// (`JavaCore.setComplianceOptions`).
fn setup_compliance(version: &str) -> (QuickFixTest, PathBuf) {
    let (mut t, root) = setup();
    let mut options = BTreeMap::new();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), version.to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    t.ws.set_project_options(&root, &options);
    (t, root)
}

#[test]
fn test_invalid_interface_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic static interface E {\n    public void foo();\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    public void foo();\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_member_interface_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    private interface Inner {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    interface Inner {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_interface_field_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    public native int i;\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    public int i;\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_interface_method_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    private strictfp void foo();\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    void foo();\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_class_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic volatile class E {\n    public void foo() {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic class E {\n    public void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_member_class_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    private class Inner {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    class Inner {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_local_class_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic class E {\n    private void foo() {\n        static class Local {\n        }\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic class E {\n    private void foo() {\n        class Local {\n        }\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_class_field_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic class E {\n    strictfp public native int i;\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic class E {\n    public int i;\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_class_method_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic abstract class E {\n    volatile abstract void foo();\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic abstract class E {\n    abstract void foo();\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_constructor_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "Bug.java", "package test;\n\npublic class Bug {\n    public static Bug() {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\n\npublic class Bug {\n    public Bug() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_param_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic class E {\n    private void foo(private int x) {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic class E {\n    private void foo(int x) {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_variable_modifiers() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic class E {\n    private void foo() {\n        native int x;\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic class E {\n    private void foo() {\n        int x;\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_enum_modifier() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\n\nprivate final strictfp enum E {\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\n\nstrictfp enum E {\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_enum_modifier2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\n\npublic abstract enum E {\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\n\npublic enum E {\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_enum_constructor_modifier() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\n\nenum E {\n\tWHITE(1);\n\n\tpublic final E(int foo) {\n\t}\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\n\nenum E {\n\tWHITE(1);\n\n\tE(int foo) {\n\t}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_member_enum_modifier() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\n\nclass E {\n\tfinal enum A {\n\t}\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\n\nclass E {\n\tenum A {\n\t}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_argument_modifier() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    public void foo(static String a);\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    public void foo(String a);\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_modifier_combination_final_volatile_for_field() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    public final volatile String A;\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    public final String A;\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_static_modifier_for_field() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    private static String A = \"Test\";\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    static String A = \"Test\";\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_static_modifier_for_method() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic interface E {\n    private static void foo() {\n    }\n}\n");
    let e1 = Expected::new(PROPOSAL, "package test;\npublic interface E {\n    static void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_override_static_method() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    public static void foo() {\n    }\n}\npublic class E extends C {\n    public void foo() {\n    }\n}\n");
    let e1 = Expected::new("Remove 'static' modifier of 'C.foo'(..)", "package test;\npublic class C {\n    public void foo() {\n    }\n}\npublic class E extends C {\n    public void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_override_final_method() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    protected final void foo() {\n    }\n}\npublic class E extends C {\n    protected void foo() {\n    }\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'C.foo'(..)", "package test;\npublic class C {\n    protected void foo() {\n    }\n}\npublic class E extends C {\n    protected void foo() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "needs GetterSetterCorrectionSubProcessor (SelfEncapsulateFieldRefactoring, \"Create getter and setter for ...\"), the scope of GetterSetterQuickFixTest"]
fn test_invisible_field_requested_in_same_package1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class C {\n    private int test;\n}\npublic class E {\n    public void foo (C c) {\n         c.test = 1;\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'test' to 'package'", "package test1;\npublic class C {\n    int test;\n}\npublic class E {\n    public void foo (C c) {\n         c.test = 1;\n    }\n}\n");
    let e2 = Expected::new("Create getter and setter for 'test'...", "package test1;\npublic class C {\n    private int test;\n\n    public int getTest() {\n        return test;\n        \n    }\n\n    public void setTest(int test) {\n        this.test = test;\n        \n    }\n}\npublic class E {\n    public void foo (C c) {\n         c.setTest(1);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "needs GetterSetterCorrectionSubProcessor (GetterSetterQuickFixTest scope): 'Create getter and setter for' proposal"]
fn test_invisible_field_requested_in_same_package2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class C {\n    private int test;\n}\npublic class E extends C {\n    public void foo () {\n         test = 1;\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'test' to 'protected'", "package test1;\npublic class C {\n    protected int test;\n}\npublic class E extends C {\n    public void foo () {\n         test = 1;\n    }\n}\n");
    let e2 = Expected::new("Create local variable 'test'", "package test1;\npublic class C {\n    private int test;\n}\npublic class E extends C {\n    public void foo () {\n         int test = 1;\n    }\n}\n");
    let e3 = Expected::new("Create field 'test'", "package test1;\npublic class C {\n    private int test;\n}\npublic class E extends C {\n    private int test;\n\n    public void foo () {\n         test = 1;\n    }\n}\n");
    let e4 = Expected::new("Create parameter 'test'", "package test1;\npublic class C {\n    private int test;\n}\npublic class E extends C {\n    public void foo (int test) {\n         test = 1;\n    }\n}\n");
    let e5 = Expected::new("Remove assignment", "package test1;\npublic class C {\n    private int test;\n}\npublic class E extends C {\n    public void foo () {\n    }\n}\n");
    let e6 = Expected::new("Create getter and setter for 'test'...", "package test1;\npublic class C {\n    private int test;\n\n    public int getTest() {\n        return test;\n        \n    }\n\n    public void setTest(int test) {\n        this.test = test;\n        \n    }\n}\npublic class E extends C {\n    public void foo () {\n         setTest(1);\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1, e2, e3, e4, e5, e6]);
}

#[test]
fn test_invisible_method_requested_in_other_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    private void test() {}\n}\nclass D {\n    public void foo () {\n        C c = new C();\n        c.test();\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'test()' to 'package'", "package test;\npublic class C {\n    void test() {}\n}\nclass D {\n    public void foo () {\n        C c = new C();\n        c.test();\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_method_requested_in_other_package() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    private void add() {\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport test.C;\npublic class E {\n    public void foo (C c) {\n         c.add();\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'add()' to 'public'", "package test;\npublic class C {\n    public void add() {\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_constructor_requested_in_other_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    private C() {}\n}\nclass D {\n    public void foo () {\n        C c = new C();\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'C()' to 'package'", "package test;\npublic class C {\n    C() {}\n}\nclass D {\n    public void foo () {\n        C c = new C();\n    }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_constructor_requested_in_in_super_type() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test", "C.java", "package test;\npublic class C {\n    private C() {}\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\nclass D extends C{\n    public D() {\n        super();\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'C()' to 'protected'", "package test;\npublic class C {\n    protected C() {}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_type_requested_in_other_package() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\nclass A {}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "B.java", "package test2;\npublic class B {\n    public void foo () {\n        test1.A a = null;\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'A' to 'public'", "package test1;\npublic class A {}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_type_requested_in_generic_type() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\nclass A<T> {}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "B.java", "package test2;\npublic class B {\n    public void foo () {\n        test1.A<String> a = null;\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'A' to 'public'", "package test1;\npublic class A<T> {}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invisible_type_requested_from_super_class() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test", "A.java", "package test;\npublic class A {\n    private class InnerA {}\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "B.java", "package test;\npublic class B extends A{\n    public void foo () {\n        InnerA a = null;\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'InnerA' to 'package'", "package test;\npublic class A {\n    class InnerA {}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_non_blank_final_local_assignment() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\npublic class X {\n  void foo() {\n    final String s = \"\";\n    if (false) {\n      s = \"\";\n    }\n  }\n}");
    let e1 = Expected::new("Remove 'final' modifier of 's'", "package test;\npublic class X {\n  void foo() {\n    String s = \"\";\n    if (false) {\n      s = \"\";\n    }\n  }\n}");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_duplicate_final_local_initialization() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\npublic class X {\n   private int a;\tpublic X (int a) {\n\t\tthis.a = a;\n\t}\n\tpublic int returnA () {\n\t\treturn a;\n\t}\n\tpublic static boolean comparison (X x, int val) {\n\t\treturn (x.returnA() == val);\n\t}\n\tpublic void foo() {\n\t\tfinal X abc;\n\t\tboolean comp = X.comparison((abc = new X(2)), (abc = new X(1)).returnA());\n\t}\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'abc'", "package test;\npublic class X {\n   private int a;\tpublic X (int a) {\n\t\tthis.a = a;\n\t}\n\tpublic int returnA () {\n\t\treturn a;\n\t}\n\tpublic static boolean comparison (X x, int val) {\n\t\treturn (x.returnA() == val);\n\t}\n\tpublic void foo() {\n\t\tX abc;\n\t\tboolean comp = X.comparison((abc = new X(2)), (abc = new X(1)).returnA());\n\t}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_final_field_assignment() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\npublic class X {\n\tfinal int contents;\n\t\n\tX() {\n\t\tcontents = 3;\n\t}\n\tX(X other) {\n\t\tother.contents = 5;\n\t}\n\t\n\tpublic static void main(String[] args) {\n\t\tX one = new X();\n\t\tSystem.out.println(\"one.contents: \" + one.contents);\n\t\tX two = new X(one);\n\t\tSystem.out.println(\"one.contents: \" + one.contents);\n\t\tSystem.out.println(\"two.contents: \" + two.contents);\n\t}\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'contents'", "package test;\npublic class X {\n\tint contents;\n\t\n\tX() {\n\t\tcontents = 3;\n\t}\n\tX(X other) {\n\t\tother.contents = 5;\n\t}\n\t\n\tpublic static void main(String[] args) {\n\t\tX one = new X();\n\t\tSystem.out.println(\"one.contents: \" + one.contents);\n\t\tX two = new X(one);\n\t\tSystem.out.println(\"one.contents: \" + one.contents);\n\t\tSystem.out.println(\"two.contents: \" + two.contents);\n\t}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_duplicate_blank_final_field_initialization() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\npublic class X {\n   private int a;\n\tfinal int x;\n\t{\n\t\tx = new X(x = 2).returnA();\t}\n\tpublic X (int a) {\n\t\tthis.a = a;\n\t}\n\tpublic int returnA () {\n\t\treturn a;\n\t}\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'x'", "package test;\npublic class X {\n   private int a;\n\tint x;\n\t{\n\t\tx = new X(x = 2).returnA();\t}\n\tpublic X (int a) {\n\t\tthis.a = a;\n\t}\n\tpublic int returnA () {\n\t\treturn a;\n\t}\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_anonymous_class_cannot_extend_final_class() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\nimport java.io.Serializable;\npublic final class X implements Serializable {\n    class SMember extends String {}  \n    @Annot(value = new SMember())\n     void bar() {}\n    @Annot(value = \n            new X(){\n                    ZorkAnonymous1 z;\n                    void foo() {\n                            this.bar();\n                            Zork2 z;\n                    }\n            })\n\tvoid foo() {}\n}\n@interface Annot {\n        String value();\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'X'", "package test;\nimport java.io.Serializable;\npublic class X implements Serializable {\n    class SMember extends String {}  \n    @Annot(value = new SMember())\n     void bar() {}\n    @Annot(value = \n            new X(){\n                    ZorkAnonymous1 z;\n                    void foo() {\n                            this.bar();\n                            Zork2 z;\n                    }\n            })\n\tvoid foo() {}\n}\n@interface Annot {\n        String value();\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_class_extend_final_class() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", "package test;\nimport java.io.Serializable;\n\npublic final class X implements Serializable {\n\n        void bar() {}\n\n        interface IM {}\n        class SMember extends String {}\n\n        class Member extends X {  \n                ZorkMember z;\n                void foo() {\n                        this.bar();\n                        Zork1 z;\n                } \n        }\n\n        void foo() {\n                new X().new IM();\n                class Local extends X { \n                        ZorkLocal z;\n                        void foo() {\n                                this.bar();\n                                Zork3 z;\n                        }\n                }\n                new X() {\n                        ZorkAnonymous2 z;                       \n                        void foo() {\n                                this.bar();\n                                Zork4 z;\n                        }\n                };\n        }\n}\n");
    let e1 = Expected::new("Remove 'final' modifier of 'X'", "package test;\nimport java.io.Serializable;\n\npublic class X implements Serializable {\n\n        void bar() {}\n\n        interface IM {}\n        class SMember extends String {}\n\n        class Member extends X {  \n                ZorkMember z;\n                void foo() {\n                        this.bar();\n                        Zork1 z;\n                } \n        }\n\n        void foo() {\n                new X().new IM();\n                class Local extends X { \n                        ZorkLocal z;\n                        void foo() {\n                                this.bar();\n                                Zork3 z;\n                        }\n                }\n                new X() {\n                        ZorkAnonymous2 z;                       \n                        void foo() {\n                                this.bar();\n                                Zork4 z;\n                        }\n                };\n        }\n}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_add_sealed_missing_class_modifier_proposal() {
    let (mut t, root) = setup_compliance("17");
    t.ws.assert_no_errors(&root);
    let cu = t.ws.create_cu(&root, "src", "test", "Shape.java", "package test;\n\npublic sealed class Shape permits Square {}\n\nclass Square extends Shape {}\n");
    let e1 = Expected::new("Change 'Square' to 'final'", "package test;\n\npublic sealed class Shape permits Square {}\n\nfinal class Square extends Shape {}\n");
    t.assert_code_actions(&cu, &[e1]);
    let e2 = Expected::new("Change 'Square' to 'non-sealed'", "package test;\n\npublic sealed class Shape permits Square {}\n\nnon-sealed class Square extends Shape {}\n");
    t.assert_code_actions(&cu, &[e2]);
    let e3 = Expected::new("Change 'Square' to 'sealed'", "package test;\n\npublic sealed class Shape permits Square {}\n\nsealed class Square extends Shape {}\n");
    t.assert_code_actions(&cu, &[e3]);
}

#[test]
fn test_add_sealed_as_direct_super_class() {
    let (mut t, root) = setup_compliance("17");
    t.ws.assert_no_errors(&root);
    let cu = t.ws.create_cu(&root, "src", "test", "Shape.java", "package test;\n\npublic sealed class Shape permits Square {}\n\nfinal class Square {}\n");
    let e1 = Expected::new("Declare 'Shape' as direct super class of 'Square'", "package test;\n\npublic sealed class Shape permits Square {}\n\nfinal class Square extends Shape {}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_add_permits_to_direct_super_class() {
    let (mut t, root) = setup_compliance("17");
    t.ws.assert_no_errors(&root);
    t.ws.create_cu(&root, "src", "test", "Shape.java", "package test;\n\npublic sealed class Shape {}\n\n");
    let cu = t.ws.create_cu(&root, "src", "test", "Square.java", "package test;\n\nfinal class Square extends Shape {}\n\n");
    let e1 = Expected::new("Declare 'Square' as permitted subtype of 'Shape'", "package test;\n\npublic sealed class Shape permits Square {}\n\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_add_permitted_sub_type_for_switch() {
    let (mut t, root) = setup_compliance("21");
    t.ws.assert_no_errors(&root);
    let cu = t.ws.create_cu(&root, "src", "test", "SealedInSwitch.java", "package test;\npublic class SealedInSwitch {\n\tpublic static void test() {\n\t\tShape input = null;\n\t\tswitch (input) {\n\t\t}\n\t}\n\tpublic sealed class Shape permits Oval, Triangle, Rectangle {}\n\tpublic final class Oval extends Shape {}\n\tpublic final class Triangle extends Shape {}\n\tpublic final class Rectangle extends Shape {}}\n");
    let e1 = Expected::new("Add permitted type cases", "package test;\npublic class SealedInSwitch {\n\tpublic static void test() {\n\t\tShape input = null;\n\t\tswitch (input) {\n\t\t\tcase SealedInSwitch.Oval s -> {}\n\t\t\tcase SealedInSwitch.Triangle s2 -> {}\n\t\t\tcase SealedInSwitch.Rectangle s3 -> {}\n\t\t\tcase null -> {}\n\t\t\tdefault -> {}\n\t\t}\n\t}\n\tpublic sealed class Shape permits Oval, Triangle, Rectangle {}\n\tpublic final class Oval extends Shape {}\n\tpublic final class Triangle extends Shape {}\n\tpublic final class Rectangle extends Shape {}}\n");
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_create_method_in_super_type() {
    let (mut t, root) = setup_compliance("17");
    t.ws.assert_no_errors(&root);
    t.ws.create_cu(&root, "src", "test", "Base.java", "package test;\npublic class Base {\n}");
    let cu = t.ws.create_cu(&root, "src", "test", "Common.java", "package test;\npublic class Common extends Base {\n    @Override\n    public void foo() {\n    }\n}");
    let e1 = Expected::new("Create 'foo()' in super type 'Base'", "package test;\npublic class Base {\n\n\tpublic void foo() {\n\t\t// TODO Auto-generated method stub\n\t\tthrow new UnsupportedOperationException(\"Unimplemented method 'foo'\");\n\t}\n}");
    t.assert_code_actions(&cu, &[e1]);
}

