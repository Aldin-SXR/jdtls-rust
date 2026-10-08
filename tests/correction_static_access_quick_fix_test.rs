//! Port of `org.eclipse.jdt.ls.core.internal.correction.StaticAccessQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_non_static_access_to_static_field() {
    let (mut t, root) = setup();
    //Testing IProblem.NonStaticAccessToStaticField;
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public enum EnumA {\n");
    bufA.push_str("  B1,\n");
    bufA.push_str("  B2;\n");
    bufA.push_str("  public void foo(){}\n");
    bufA.push_str("}");
    t.ws.create_cu(&root, "src", "test1", "EnumA.java", &bufA);
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public enum EnumA {\n");
    bufA.push_str("  B1,\n");
    bufA.push_str("  B2;\n");
    bufA.push_str("  public void foo(){}\n");
    bufA.push_str("}");
    let mut bufB = String::new();
    bufB.push_str("package test1;\n");
    bufB.push_str("public class ClassC {\n");
    bufB.push_str("  void bar() {\n");
    bufB.push_str("    EnumA.B1.B1.foo();\n");
    bufB.push_str("    EnumA.B1.B2.foo();\n");
    bufB.push_str("  }\n");
    bufB.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "ClassC.java", &bufB);
    let mut bufB = String::new();
    bufB.push_str("package test1;\n");
    bufB.push_str("public class ClassC {\n");
    bufB.push_str("  void bar() {\n");
    bufB.push_str("    EnumA.B1.foo();\n");
    bufB.push_str("    EnumA.B1.B2.foo();\n");
    bufB.push_str("  }\n");
    bufB.push_str("}");
    let e1 = Expected::new("Remove 'static' modifier of 'B1'", &bufA);
    let e2 = Expected::new("Change access to static using 'EnumA' (declaring type)", &bufB);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_non_static_access_to_static_method() {
    let (mut t, root) = setup();
    //Testing IProblem.NonStaticAccessToStaticMethod;
    // Referenced from: https://www.intertech.com/Blog/a-static-method-should-be-accessed-in-a-static-way/
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public class HelloWorld {\n");
    bufA.push_str("  public static void sayHello() {\n");
    bufA.push_str("    System.out.println(\"Hello!\");\n");
    bufA.push_str("  }\n");
    bufA.push_str("  public static void main(String[] args) {\n");
    bufA.push_str("    HelloWorld hw = new HelloWorld();\n");
    bufA.push_str("    hw.sayHello();\n");
    bufA.push_str("  }\n");
    bufA.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "HelloWorld.java", &bufA);
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public class HelloWorld {\n");
    bufA.push_str("  public void sayHello() {\n");
    bufA.push_str("    System.out.println(\"Hello!\");\n");
    bufA.push_str("  }\n");
    bufA.push_str("  public static void main(String[] args) {\n");
    bufA.push_str("    HelloWorld hw = new HelloWorld();\n");
    bufA.push_str("    hw.sayHello();\n");
    bufA.push_str("  }\n");
    bufA.push_str("}");
    let e1 = Expected::new("Remove 'static' modifier of 'sayHello()'", &bufA);
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public class HelloWorld {\n");
    bufA.push_str("  public static void sayHello() {\n");
    bufA.push_str("    System.out.println(\"Hello!\");\n");
    bufA.push_str("  }\n");
    bufA.push_str("  public static void main(String[] args) {\n");
    bufA.push_str("    HelloWorld hw = new HelloWorld();\n");
    bufA.push_str("    HelloWorld.sayHello();\n");
    bufA.push_str("  }\n");
    bufA.push_str("}");
    let e2 = Expected::new("Change access to static using 'HelloWorld' (declaring type)", &bufA);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_non_static_or_alien_type_receiver() {
    let (mut t, root) = setup();
    //Testing IProblem.NonStaticOrAlienTypeReceiver;
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public class X {\n");
    bufA.push_str("  public interface I {\n");
    bufA.push_str("    private static void foo(){};\n");
    bufA.push_str("    void bar();");
    bufA.push_str("  }\n");
    bufA.push_str("  public static void main(String[] args) {\n");
    bufA.push_str("    I i = () -> {};\n");
    bufA.push_str("    i.foo();\n");
    bufA.push_str("  }\n");
    bufA.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "X.java", &bufA);
    let mut bufA = String::new();
    bufA.push_str("package test1;\n");
    bufA.push_str("public class X {\n");
    bufA.push_str("  public interface I {\n");
    bufA.push_str("    private static void foo(){};\n");
    bufA.push_str("    void bar();");
    bufA.push_str("  }\n");
    bufA.push_str("  public static void main(String[] args) {\n");
    bufA.push_str("    I i = () -> {};\n");
    bufA.push_str("    I.foo();\n");
    bufA.push_str("  }\n");
    bufA.push_str("}");
    let e1 = Expected::new("Change access to static using 'I' (declaring type)", &bufA);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_indirect_access_to_static_field() {
    let (mut t, root) = setup();
    // Eclipse Core FieldAccessTest#test002()
    // Cannot get code action to trigger, same in Eclipse
    // Same with IProblem.IndirectAccessToStaticMethod
    // To test, uncomment assert at bottom
    //Testing IProblem.IndirectAccessToStaticField;
    let mut bufA = String::new();
    bufA.push_str("package foo;\n");
    bufA.push_str("public class BaseFoo {\n");
    bufA.push_str(" public static final int VAL = 0;\n");
    bufA.push_str("}");
    t.ws.create_cu(&root, "src", "foo", "BaseFoo.java", &bufA);
    let mut bufB = String::new();
    bufB.push_str("package foo;\n");
    bufB.push_str("public class NextFoo extends BaseFoo {\n");
    bufB.push_str("}");
    t.ws.create_cu(&root, "src", "foo", "NextFoo.java", &bufB);
    let mut bufC = String::new();
    bufC.push_str("package bar;\n");
    bufC.push_str("public class Bar {\n");
    bufC.push_str(" int v = foo.NextFoo.VAL;\n");
    bufC.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "bar", "Bar.java", &bufC);
    let mut bufC = String::new();
    bufC.push_str("package bar;\n");
    bufC.push_str("public class Bar {\n");
    bufC.push_str(" int v = BaseFoo.VAL;\n");
    bufC.push_str("}");
    let e1 = Expected::new("Change access to static using 'I' (declaring type)", &bufC);
    //assertCodeActions(cu, e1);
}

