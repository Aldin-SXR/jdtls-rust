//! Port of `org.eclipse.jdt.ls.core.internal.correction.RedundantInterfaceQuickFixTest`.

mod common;

use common::jdtls::{range, test_default_options};
use common::quickfix::{Expected, QuickFixTest};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf, BTreeMap<String, String>) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.redundantSuperinterface".into(), "warning".into());
    let root = t.ws.new_empty_project(&options);
    (t, root, options)
}

#[test]
fn test_redundant_superinterface() {
    let (mut t, root, _) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class RedundantInterface implements Int1, Int2 {}\n");
    buf.push_str("interface Int1 {}\n");
    buf.push_str("interface Int2 extends Int1 {}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "RedundantInterface.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class RedundantInterface implements Int2 {}\n");
    buf.push_str("interface Int1 {}\n");
    buf.push_str("interface Int2 extends Int1 {}\n");
    let e1 = Expected::new("Remove super interface", &buf);
    let selection = range(1, 45, 1, 45);
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_ignore_redundant_superinterface() {
    let (mut t, root, mut options) = setup();
    options.insert("org.eclipse.jdt.core.compiler.problem.redundantSuperinterface".into(), "ignore".into());
    t.ws.set_project_options(&root, &options);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class RedundantInterface implements Int1, Int2 {}\n");
    buf.push_str("interface Int1 {}\n");
    buf.push_str("interface Int2 extends Int1 {}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "RedundantInterface.java", &buf);
    let selection = range(1, 45, 1, 45);
    t.set_ignored_commands(&["Generate Constructors...", "Generate Constructors"]);
    t.assert_code_action_not_exists_range(&cu, selection, "Remove super interface");
}
