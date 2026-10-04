//! Port of `org.eclipse.jdt.ls.core.internal.correction.UnnecessaryCastQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_range, Expected, QuickFixTest};

#[test]
fn test_unnecessary_cast() {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unnecessaryTypeCheck".into(), "error".into());
    let root = t.ws.new_empty_project(&options);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class Driver {\n");
    buf.push_str("}");
    t.ws.create_cu(&root, "src", "test", "Driver.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class BusDriver extends Driver {\n");
    buf.push_str("  public void drive() {\n");
    buf.push_str("    Driver d = (Driver) this;\n");
    buf.push_str("  }\n");
    buf.push_str("}");
    let source = buf.clone();
    let cu = t.ws.create_cu(&root, "src", "test", "BusDriver.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class BusDriver extends Driver {\n");
    buf.push_str("  public void drive() {\n");
    buf.push_str("    Driver d = this;\n");
    buf.push_str("  }\n");
    buf.push_str("}");
    let e1 = Expected::new("Remove cast", &buf);

    let selection = get_range(&source, "FOO");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}
