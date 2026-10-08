//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.AnonymousClassCreationToLambdaTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_convert_to_lambda1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void method();\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void bar(I i) {}\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        bar(new I() {\n");
    buf.push_str("            @Override\n");
    buf.push_str("            /*[*/public void method() {\n");
    buf.push_str("                System.out.println();\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void method();\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void bar(I i) {}\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        bar(() -> System.out.println());\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void method(int a, int b);\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void bar(I i) {}\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        bar(new I() {\n");
    buf.push_str("            /*[*/public void method(int a, int b) {\n");
    buf.push_str("                System.out.println(a+b);\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void method(int a, int b);\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void bar(I i) {}\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        bar((a, b) -> System.out.println(a+b));\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void count(int test);\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        I i = new I() {\n");
    buf.push_str("            /*[*/public void count(int test) {\n");
    buf.push_str("                System.out.println(test);\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        };\n");
    buf.push_str("        i.count(10);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    void count(int test);\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        I i = test -> System.out.println(test);\n");
    buf.push_str("        i.count(10);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda4() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    int method();\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void bar(I i) {}\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        bar(new I() {\n");
    buf.push_str("            /*[*/public int method() {\n");
    buf.push_str("                return 1;\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface I {\n");
    buf.push_str("    int method();\n");
    buf.push_str("}\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void bar(I i) {}\n");
    buf.push_str("    void foo() {\n");
    buf.push_str("        bar(() -> 1);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
#[ignore = "upstream method has no @Test annotation and is never run"]
fn test_convert_to_lambda5() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface A {\n");
    buf.push_str("    public int sayHello();\n");
    buf.push_str("}\n");
    buf.push_str("interface J {\n");
    buf.push_str("    public int method();\n");
    buf.push_str("}\n");
    buf.push_str("public class X {\n");
    buf.push_str("    static void foo(A a) { }\n");
    buf.push_str("    static void foo(B b) { }\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        foo(new A() {\n");
    buf.push_str("            @Override\n");
    buf.push_str("            /*[*/public int sayHello() {\n");
    buf.push_str("                return 0;\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "X.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface A {\n");
    buf.push_str("    public int sayHello();\n");
    buf.push_str("}\n");
    buf.push_str("interface J {\n");
    buf.push_str("    public int method();\n");
    buf.push_str("}\n");
    buf.push_str("public class X {\n");
    buf.push_str("    static void foo(A a) { }\n");
    buf.push_str("    static void foo(B b) { }\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        foo((A) () -> 0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda6() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("@FunctionalInterface\n");
    buf.push_str("interface A {\n");
    buf.push_str("    int test(int x, int y, int z);\n");
    buf.push_str("}\n");
    buf.push_str("public class B {\n");
    buf.push_str("    int i;\n");
    buf.push_str("    private void foo() {\n");
    buf.push_str("        A a = new A() {\n");
    buf.push_str("           @Override\n");
    buf.push_str("           /*[*/public int test(int x/*km*/, int i /*inches*/, int y/*yards*/) {\n");
    buf.push_str("                return x + i + y;\n");
    buf.push_str("           }/*]*/\n");
    buf.push_str("        };\n");
    buf.push_str("        a.test(1, 2, 3);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "B.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("@FunctionalInterface\n");
    buf.push_str("interface A {\n");
    buf.push_str("    int test(int x, int y, int z);\n");
    buf.push_str("}\n");
    buf.push_str("public class B {\n");
    buf.push_str("    int i;\n");
    buf.push_str("    private void foo() {\n");
    buf.push_str("        A a = (x/*km*/, i /*inches*/, y/*yards*/) -> x + i + y;\n");
    buf.push_str("        a.test(1, 2, 3);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda7() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    Runnable r1 = new Runnable() {\n");
    buf.push_str("        @Override @Deprecated\n");
    buf.push_str("        /*[*/public void run() {\n");
    buf.push_str("            System.out.println();\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("    };\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "C1.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    Runnable r1 = () -> System.out.println();\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda8() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    Runnable run = new Runnable() {\n");
    buf.push_str("        @Override\n");
    buf.push_str("        /*[*/public strictfp void run() {}/*]*/\n");
    buf.push_str("    };\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "C1.java", &buf);

    t.assert_code_action_not_exists(&cu, "Convert to lambda expression");
}

#[test]
fn test_convert_to_lambda9() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    Runnable run = new Runnable() {\n");
    buf.push_str("        @Override\n");
    buf.push_str("        /*[*/public synchronized void run() {}/*]*/\n");
    buf.push_str("    };\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "C1.java", &buf);

    t.assert_code_action_not_exists(&cu, "Convert to lambda expression");
}

#[test]
fn test_convert_to_lambda10() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface FI {\n");
    buf.push_str("    void foo(String... strs);\n");
    buf.push_str("}\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    FI fi = new  FI() {\n");
    buf.push_str("        @Override\n");
    buf.push_str("        /*[*/public void foo(String... strs) {\n");
    buf.push_str("                 System.out.println();\n");
    buf.push_str("        /*]*/}\n");
    buf.push_str("    };\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "C1.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("interface FI {\n");
    buf.push_str("    void foo(String... strs);\n");
    buf.push_str("}\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    FI fi = strs -> {\n");
    buf.push_str("             System.out.println();\n");
    buf.push_str("    /*]*/};\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda11() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("import java.lang.annotation.ElementType;\n");
    buf.push_str("import java.lang.annotation.Target;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    FI fi1 = new FI() {\n");
    buf.push_str("        @Override\n");
    buf.push_str("        /*[*/public void foo(java.util.@T ArrayList<IOException> x) {\n");
    buf.push_str("                 System.out.println();\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("    };\n");
    buf.push_str("}\n");
    buf.push_str("interface FI {\n");
    buf.push_str("    void foo(ArrayList<IOException> x);\n");
    buf.push_str("}\n");
    buf.push_str("@Target(ElementType.TYPE_USE)\n");
    buf.push_str("@interface T {\n");
    buf.push_str("    int val1() default 1;\n");
    buf.push_str("    int val2() default -1;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "C1.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("import java.lang.annotation.ElementType;\n");
    buf.push_str("import java.lang.annotation.Target;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class C1 {\n");
    buf.push_str("    FI fi1 = (java.util.@T ArrayList<IOException> x) -> System.out.println();\n");
    buf.push_str("}\n");
    buf.push_str("interface FI {\n");
    buf.push_str("    void foo(ArrayList<IOException> x);\n");
    buf.push_str("}\n");
    buf.push_str("@Target(ElementType.TYPE_USE)\n");
    buf.push_str("@interface T {\n");
    buf.push_str("    int val1() default 1;\n");
    buf.push_str("    int val2() default -1;\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

#[test]
fn test_convert_to_lambda12() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.function.Predicate;\n");
    buf.push_str("\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    void foo(ArrayList<String> list) {\n");
    buf.push_str("        list.removeIf(new Predicate<String>() {\n");
    buf.push_str("            @Override\n");
    buf.push_str("            /*[*/public boolean test(String t) {\n");
    buf.push_str("                return t.isEmpty();\n");
    buf.push_str("            }/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    void foo(ArrayList<String> list) {\n");
    buf.push_str("        list.removeIf(String::isEmpty);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e = Expected::new("Convert to lambda expression", &buf);

    t.assert_code_actions(&cu, &[e.clone()]);
}

