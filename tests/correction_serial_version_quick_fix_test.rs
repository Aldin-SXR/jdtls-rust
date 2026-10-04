//! Port of `org.eclipse.jdt.ls.core.internal.correction.SerialVersionQuickFixTest`.

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
fn test_local_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test5 {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        class X implements Serializable, Cloneable, Runnable {\n");
    buf.push_str("            private static final int x= 1;\n");
    buf.push_str("            private Object y;\n");
    buf.push_str("            public X() {\n");
    buf.push_str("            }\n");
    buf.push_str("            public void run() {}\n");
    buf.push_str("            public synchronized strictfp void bar() {}\n");
    buf.push_str("            public String bar(int x, int y) { return null; };\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test3", "Test5.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test5 {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        class X implements Serializable, Cloneable, Runnable {\n");
    buf.push_str("            private static final long serialVersionUID = 1L;\n");
    buf.push_str("            private static final int x= 1;\n");
    buf.push_str("            private Object y;\n");
    buf.push_str("            public X() {\n");
    buf.push_str("            }\n");
    buf.push_str("            public void run() {}\n");
    buf.push_str("            public synchronized strictfp void bar() {}\n");
    buf.push_str("            public String bar(int x, int y) { return null; };\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add default serial version ID", &buf);

    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test5 {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        class X implements Serializable, Cloneable, Runnable {\n");
    buf.push_str("            private static final long serialVersionUID = -4564939359985118485L;\n");
    buf.push_str("            private static final int x= 1;\n");
    buf.push_str("            private Object y;\n");
    buf.push_str("            public X() {\n");
    buf.push_str("            }\n");
    buf.push_str("            public void run() {}\n");
    buf.push_str("            public synchronized strictfp void bar() {}\n");
    buf.push_str("            public String bar(int x, int y) { return null; };\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add generated serial version ID", &buf);

    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_inner_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("\n");
    buf.push_str("public class Test2 {\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("    protected class Test1 implements Serializable {\n");
    buf.push_str("        public long var3;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "Test2.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("\n");
    buf.push_str("public class Test2 {\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("    protected class Test1 implements Serializable {\n");
    buf.push_str("        private static final long serialVersionUID = 1L;\n");
    buf.push_str("        public long var3;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add default serial version ID", &buf);

    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("\n");
    buf.push_str("public class Test2 {\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("    protected class Test1 implements Serializable {\n");
    buf.push_str("        private static final long serialVersionUID = -4023230086280104302L;\n");
    buf.push_str("        public long var3;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add generated serial version ID", &buf);

    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_outer_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "Test1.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    private static final long serialVersionUID = 1L;\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add default serial version ID", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    private static final long serialVersionUID = -2242798150684569765L;\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    protected int var2;\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add generated serial version ID", &buf);

    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_outer_class2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.util.EventObject;\n");
    buf.push_str("public class Test4 extends EventObject {\n");
    buf.push_str("    private static final int x;\n");
    buf.push_str("    private static Class[] a2;\n");
    buf.push_str("    private volatile Class a1;\n");
    buf.push_str("    static {\n");
    buf.push_str("        x= 1;\n");
    buf.push_str("    }\n");
    buf.push_str("    {\n");
    buf.push_str("        a1= null;\n");
    buf.push_str("    }\n");
    buf.push_str("    \n");
    buf.push_str("    public Test4(Object source) {\n");
    buf.push_str("        super(source);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test3", "Test4.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.util.EventObject;\n");
    buf.push_str("public class Test4 extends EventObject {\n");
    buf.push_str("    private static final long serialVersionUID = 1L;\n");
    buf.push_str("    private static final int x;\n");
    buf.push_str("    private static Class[] a2;\n");
    buf.push_str("    private volatile Class a1;\n");
    buf.push_str("    static {\n");
    buf.push_str("        x= 1;\n");
    buf.push_str("    }\n");
    buf.push_str("    {\n");
    buf.push_str("        a1= null;\n");
    buf.push_str("    }\n");
    buf.push_str("    \n");
    buf.push_str("    public Test4(Object source) {\n");
    buf.push_str("        super(source);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add default serial version ID", &buf);

    let mut buf = String::new();
    buf.push_str("package test3;\n");
    buf.push_str("import java.util.EventObject;\n");
    buf.push_str("public class Test4 extends EventObject {\n");
    buf.push_str("    private static final long serialVersionUID = -7476608308201363525L;\n");
    buf.push_str("    private static final int x;\n");
    buf.push_str("    private static Class[] a2;\n");
    buf.push_str("    private volatile Class a1;\n");
    buf.push_str("    static {\n");
    buf.push_str("        x= 1;\n");
    buf.push_str("    }\n");
    buf.push_str("    {\n");
    buf.push_str("        a1= null;\n");
    buf.push_str("    }\n");
    buf.push_str("    \n");
    buf.push_str("    public Test4(Object source) {\n");
    buf.push_str("        super(source);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add generated serial version ID", &buf);

    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_outer_class3() {
    // longer package
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package a.b.c;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    class Test1Inner {}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "a.b.c", "Test1.java", &buf);

    let mut buf = String::new();
    buf.push_str("package a.b.c;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    private static final long serialVersionUID = 1L;\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    class Test1Inner {}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add default serial version ID", &buf);

    let mut buf = String::new();
    buf.push_str("package a.b.c;\n");
    buf.push_str("import java.io.Serializable;\n");
    buf.push_str("public class Test1 implements Serializable {\n");
    buf.push_str("    private static final long serialVersionUID = -3715240305486851194L;\n");
    buf.push_str("    protected int var1;\n");
    buf.push_str("    class Test1Inner {}\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add generated serial version ID", &buf);

    t.assert_code_actions(&cu, &[e1, e2]);
}
