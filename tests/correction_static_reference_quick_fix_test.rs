//! Port of `org.eclipse.jdt.ls.core.internal.correction.StaticReferenceQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_range, Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

#[test]
fn test_static_method_requested() {
    let (mut t, root) = setup();
    // Testing IProblem.StaticMethodRequested;
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tpublic static void main(String[] args) {\n");
    buf.push_str("\t\tb();\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("\t");
    buf.push_str("\tpublic void b() {\n");
    // non static
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tpublic static void main(String[] args) {\n");
    buf.push_str("\t\tb();\n");
    buf.push_str("\t}\n");
    buf.push_str("\t");
    buf.push_str("\tpublic static void b() {\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change 'b()' to 'static'", &buf);
    let selection = get_range(&t.ws.read(&cu), "FOOBAR");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_non_static_field_from_static_invocation() {
    let (mut t, root) = setup();
    // Testing IProblem.NonStaticFieldFromStaticInvocation;
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tint x = 0;");
    // is not static
    buf.push_str("\tpublic static void main(String[] args) {\n");
    buf.push_str("\t\tSystem.out.println(x);\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tstatic int x = 0;");
    // added static
    buf.push_str("\tpublic static void main(String[] args) {\n");
    buf.push_str("\t\tSystem.out.println(x);\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change 'x' to 'static'", &buf);
    let selection = get_range(&t.ws.read(&cu), "FOOBAR");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_instance_method_during_constructor_invocation() {
    let (mut t, root) = setup();
    // Testing IProblem.NonStaticFieldFromStaticInvocation;
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tA (long x) {\n");
    buf.push_str("\t\tthis(test());\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("\t\n");
    buf.push_str("\tint test() {\n");
    buf.push_str("\t\treturn 0;\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tA (long x) {\n");
    buf.push_str("\t\tthis(test());\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("\t\n");
    buf.push_str("\tstatic int test() {\n");
    buf.push_str("\t\treturn 0;\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change 'test()' to 'static'", &buf);
    let selection = get_range(&t.ws.read(&cu), "FOOBAR");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_instance_field_during_constructor_invocation() {
    let (mut t, root) = setup();
    // Testing IProblem.NonStaticFieldFromStaticInvocation;
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tint i;");
    buf.push_str("\tA () {\n");
    buf.push_str("\t\tthis(i);\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tstatic int i;");
    buf.push_str("\tA () {\n");
    buf.push_str("\t\tthis(i);\n");
    // referencing in static context
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change 'i' to 'static'", &buf);
    let selection = get_range(&t.ws.read(&cu), "FOOBAR");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

