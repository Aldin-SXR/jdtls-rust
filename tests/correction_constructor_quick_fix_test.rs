//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConstructorQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.deadCode".into(), "warning".into());
    let root = t.ws.new_empty_project(&options);
    t.set_ignored_commands(&["Extract.*"]);
    (t, root)
}

#[test]
#[ignore = "ConstructorFromSuperclassProposal not ported yet (the generated constructor differs: no TODO body comment, position)"]
fn test_undefined_constructor_from_super_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class F {\n");
    buf.push_str("    public F(Runnable runnable) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "F.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("\n");
    buf.push_str("    public E(Runnable runnable) {\n");
    buf.push_str("        super(runnable);\n");
    buf.push_str("        //TODO Auto-generated constructor stub\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add constructor 'E(Runnable)'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "ConstructorFromSuperclassProposal not ported yet (thrown exceptions, imports and body differ)"]
fn test_multiple_undefined_constructor_from_super_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("public class F {\n");
    buf.push_str("    public F(Runnable runnable) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public F(int i, Runnable runnable) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "F.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("\n");
    buf.push_str("    public E(Runnable runnable) throws IOException {\n");
    buf.push_str("        super(runnable);\n");
    buf.push_str("        //TODO Auto-generated constructor stub\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add constructor 'E(Runnable)'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("\n");
    buf.push_str("    public E(int i, Runnable runnable) {\n");
    buf.push_str("        super(i, runnable);\n");
    buf.push_str("        //TODO Auto-generated constructor stub\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add constructor 'E(int,Runnable)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "NotVisibleConstructor super-constructor proposals not ported yet"]
fn test_not_visible_constructor_from_super_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class F {\n");
    buf.push_str("    private F() {\n");
    buf.push_str("    }\n");
    buf.push_str("    public F(Runnable runnable) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "F.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends F {\n");
    buf.push_str("\n");
    buf.push_str("    public E(Runnable runnable) {\n");
    buf.push_str("        super(runnable);\n");
    buf.push_str("        //TODO Auto-generated constructor stub\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add constructor 'E(Runnable)'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

