//! Port of `org.eclipse.jdt.ls.core.internal.correction.ReturnTypeQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.noEffectAssignment".into(), "ignore".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.indirectStaticAccess".into(), "error".into());
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

#[test]
fn test_void_method_returns_value() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        return new Object();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object foo(Object o) {\n");
    buf.push_str("        return new Object();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change method return type to 'Object'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change to 'return;'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_method_returns_void() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object foo(Object o) {\n");
    buf.push_str("        return o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return statement", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change return type to 'void'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_return_type() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Set method return type to 'void'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public E(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change to constructor", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_should_return() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object foo(Object o) {\n");
    buf.push_str("        return o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return statement", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change return type to 'void'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

