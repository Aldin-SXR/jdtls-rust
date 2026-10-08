//! Port of `org.eclipse.jdt.ls.core.internal.correction.StaticImportQuickAssistTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    (t, root)
}

#[test]
fn test_convert_to_static_field_import() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tpublic static final String FOO = \"BAR\";\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test", "A.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic String bar = A.FOO;\n");
    buf.push_str("\tpublic String bar1 = A.FOO;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "B.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("\n");
    buf.push_str("import static test.A.FOO;\n");
    buf.push_str("\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic String bar = A.FOO;\n");
    buf.push_str("\tpublic String bar1 = FOO;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Convert to static import", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("\n");
    buf.push_str("import static test.A.FOO;\n");
    buf.push_str("\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic String bar = FOO;\n");
    buf.push_str("\tpublic String bar1 = FOO;\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Convert to static import (replace all occurrences)", &buf);

    let selection = get_range(&t.ws.read(&cu), "FOO");
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}

#[test]
fn test_convert_to_static_method_import() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tpublic static void foo() {\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test", "A.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic void bar() {\n");
    buf.push_str("\t\tA.foo();\n");
    buf.push_str("\t}\n");
    buf.push_str("\tpublic void bar1() {\n");
    buf.push_str("\t\tA.foo();\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "B.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("\n");
    buf.push_str("import static test.A.foo;\n");
    buf.push_str("\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic void bar() {\n");
    buf.push_str("\t\tA.foo();\n");
    buf.push_str("\t}\n");
    buf.push_str("\tpublic void bar1() {\n");
    buf.push_str("\t\tfoo();\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Convert to static import", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("\n");
    buf.push_str("import static test.A.foo;\n");
    buf.push_str("\n");
    buf.push_str("public class B {\n");
    buf.push_str("\tpublic void bar() {\n");
    buf.push_str("\t\tfoo();\n");
    buf.push_str("\t}\n");
    buf.push_str("\tpublic void bar1() {\n");
    buf.push_str("\t\tfoo();\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Convert to static import (replace all occurrences)", &buf);

    let selection = get_range(&t.ws.read(&cu), "foo");
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}

#[test]
// https://github.com/eclipse/eclipse.jdt.ls/issues/1203
fn test_convert_to_static_import_preserves_existing() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class T {\n");
    buf.push_str("    public static void foo() { };\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "T.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("import test1.T;\n");
    buf.push_str("\n");
    buf.push_str("public class S {\n");
    buf.push_str("    public S() {\n");
    buf.push_str("        T.foo();\n");
    buf.push_str("        System.out.println(T.class);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "S.java", &buf);

    let selection = get_range(&t.ws.read(&cu), "foo");

    let mut expectation = String::new();
    expectation.push_str("package test2;\n");
    expectation.push_str("\n");
    expectation.push_str("import static test1.T.foo;\n");
    expectation.push_str("\n");
    expectation.push_str("import test1.T;\n");
    expectation.push_str("\n");
    expectation.push_str("public class S {\n");
    expectation.push_str("    public S() {\n");
    expectation.push_str("        foo();\n");
    expectation.push_str("        System.out.println(T.class);\n");
    expectation.push_str("    }\n");
    expectation.push_str("}\n");

    let e1 = Expected::new("Convert to static import", &expectation);
    let e2 = Expected::new("Convert to static import (replace all occurrences)", &expectation);
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}
