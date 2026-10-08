//! Port of `org.eclipse.jdt.ls.core.internal.correction.AssignToFieldQuickAssistTest`.

mod common;

use common::quickfix::{get_range, get_range_len, Expected, QuickFixTest};
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    (t, root)
}

#[test]
fn test_assign_param_to_field() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public  E(int count) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int count;\n");
    buf.push_str("\n");
    buf.push_str("    public  E(int count) {\n");
    buf.push_str("        this.count = count;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let selection = get_range(&t.ws.read(&cu), "count");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_assign_param_to_field_with_final_setting() {
    let (mut t, root) = setup();
    t.ws.settings["java"]["codeGeneration"]["addFinalForNewDeclaration"] = json!("fields");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public  E(int count) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private final int count;\n");
    buf.push_str("\n");
    buf.push_str("    public  E(int count) {\n");
    buf.push_str("        this.count = count;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let selection = get_range(&t.ws.read(&cu), "count");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_assign_param_to_field2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public  E(int count, Vector vec[]) {\n");
    buf.push_str("        super();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private Vector[] vec;\n");
    buf.push_str("\n");
    buf.push_str("    public  E(int count, Vector vec[]) {\n");
    buf.push_str("        super();\n");
    buf.push_str("        this.vec = vec;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int count;\n");
    buf.push_str("    private Vector[] vec;\n");
    buf.push_str("\n");
    buf.push_str("    public  E(int count, Vector vec[]) {\n");
    buf.push_str("        super();\n");
    buf.push_str("        this.count = count;\n");
    buf.push_str("        this.vec = vec;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Assign all parameters to new fields", &buf);

    let selection = get_range(&t.ws.read(&cu), "vec");
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}

#[test]
fn test_assign_param_to_field3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int vec;\n");
    buf.push_str("\n");
    buf.push_str("    public static void foo(int count, Vector vec[]) {\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int vec;\n");
    buf.push_str("    private static Vector[] vec2;\n");
    buf.push_str("\n");
    buf.push_str("    public static void foo(int count, Vector vec[]) {\n");
    buf.push_str("        vec2 = vec;\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int vec;\n");
    buf.push_str("    private static int count;\n");
    buf.push_str("    private static Vector[] vec2;\n");
    buf.push_str("\n");
    buf.push_str("    public static void foo(int count, Vector vec[]) {\n");
    buf.push_str("        E.count = count;\n");
    buf.push_str("        vec2 = vec;\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Assign all parameters to new fields", &buf);

    let selection = get_range(&t.ws.read(&cu), "vec");
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}

#[test]
fn test_assign_param_to_field4() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private long count;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int count) {\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private long count;\n");
    buf.push_str("    private int count2;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int count) {\n");
    buf.push_str("        count2 = count;\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private long count;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int count) {\n");
    buf.push_str("        this.count = count;\n");
    buf.push_str("        count++;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Assign parameter to field 'count'", &buf);

    let selection = get_range_len(&t.ws.read(&cu), "int count", 0);
    t.assert_code_actions_range(&cu, selection, &[e1, e2]);
}

#[test]
fn test_assign_param_to_field5() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int p1;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int p1, int p2) {\n");
    buf.push_str("        this.p1 = p1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int p1;\n");
    buf.push_str("    private int p2;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int p1, int p2) {\n");
    buf.push_str("        this.p1 = p1;\n");
    buf.push_str("        this.p2 = p2;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Assign parameter to new field", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int p1;\n");
    buf.push_str("    private int p12;\n");
    buf.push_str("    private int p2;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int p1, int p2) {\n");
    buf.push_str("        p12 = p1;\n");
    buf.push_str("        this.p1 = p1;\n");
    buf.push_str("        this.p2 = p2;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Assign all parameters to new fields", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private int p1;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo(int p1, int p2) {\n");
    buf.push_str("        this.p1 = p1;\n");
    buf.push_str("        p1 = p2;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Assign parameter to field 'p1'", &buf);

    let selection = get_range_len(&t.ws.read(&cu), "int p2", 0);
    t.assert_code_actions_range(&cu, selection, &[e1, e2, e3]);
}
