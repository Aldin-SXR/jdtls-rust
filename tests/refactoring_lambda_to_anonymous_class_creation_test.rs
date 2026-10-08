//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.LambdaToAnonymousClassCreationTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotation".to_owned(), "warning".to_owned());
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

#[test]
fn test_convert_to_anonymous_class_creation() {
    let (mut t, root) = setup();
    let mut builder = String::new();
    builder.push_str("package test1;\n");
    builder.push_str("interface I {\n");
    builder.push_str("    void method();\n");
    builder.push_str("}\n");
    builder.push_str("public class E {\n");
    builder.push_str("    void bar(I i) {\n");
    builder.push_str("    }\n");
    builder.push_str("    void foo() {\n");
    builder.push_str("        bar(() /*[*//*]*/-> {\n");
    builder.push_str("            System.out.println();\n");
    builder.push_str("            System.out.println();\n");
    builder.push_str("        });\n");
    builder.push_str("    }\n");
    builder.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &builder);

    let mut builder = String::new();
    builder.push_str("package test1;\n");
    builder.push_str("interface I {\n");
    builder.push_str("    void method();\n");
    builder.push_str("}\n");
    builder.push_str("public class E {\n");
    builder.push_str("    void bar(I i) {\n");
    builder.push_str("    }\n");
    builder.push_str("    void foo() {\n");
    builder.push_str("        bar(new I() {\n");
    builder.push_str("            @Override\n");
    builder.push_str("            public void method() {\n");
    builder.push_str("                System.out.println();\n");
    builder.push_str("                System.out.println();\n");
    builder.push_str("            }\n");
    builder.push_str("        });\n");
    builder.push_str("    }\n");
    builder.push_str("}\n");
    let e = Expected::new("Convert to anonymous class creation", &builder);

    t.assert_code_actions(&cu, &[e]);
}
