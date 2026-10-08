//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConvertVarQuickFixTest`.

mod common;

use common::quickfix::{get_range_len, get_title, Expected, QuickFixTest};
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.import_projects(&["eclipse/java10"]);
    let root = t.ws.project_root("java10");
    (t, root)
}

fn create_cu(t: &mut QuickFixTest, root: &PathBuf, source: &str) -> String {
    t.ws.create_cu(root, "src/main/java", "foo.bar", "Test.java", source)
}

#[test]
fn test_convert_var_type_to_resolved_type() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        var name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = create_cu(&mut t, &root, &buf);
    let code_actions = t.evaluate_code_actions(&cu);
    let code_action = code_actions.iter().find(|c| get_title(c) == "Change type of 'name' to 'String'");
    assert!(code_action.is_some());
}

#[test]
fn test_convert_var_type_to_resolved_type2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        var/*cursor*/ name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = create_cu(&mut t, &root, &buf);

    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        String name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Change type of 'name' to 'String'", &buf, "refactor");
    let range = get_range_len(&t.ws.read(&cu), "/*cursor*/", 0);
    t.assert_code_actions_range(&cu, range, &[expected]);
}

#[test]
fn test_convert_resolved_type_to_var() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        String name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = create_cu(&mut t, &root, &buf);
    let commands = t.evaluate_code_actions(&cu);
    let code_action = commands.iter().find(|c| get_title(c) == "Change type of 'name' to 'var'");
    assert!(code_action.is_some());
}

#[test]
fn test_convert_resolved_type_to_var2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        String/*cursor*/ name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = create_cu(&mut t, &root, &buf);

    let mut buf = String::new();
    buf.push_str("package foo.bar;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    public void test() {\n");
    buf.push_str("        var name = \"test\";\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");

    let expected = Expected::with_kind("Change type of 'name' to 'var'", &buf, "refactor");
    let range = get_range_len(&t.ws.read(&cu), "/*cursor*/", 0);
    t.assert_code_actions_range(&cu, range, &[expected]);
}
