//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConvertSwitchExpressionQuickAssistTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn compliance_options(version: &str) -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), version.to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    options
}

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    // The upstream test preferences indent with tabs.
    t.ws.settings["java"]["format"] = serde_json::json!({ "insertSpaces": false });
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    t.ws.set_project_options(&root, &compliance_options("14"));
    (t, root)
}

#[test]
fn test_convert_to_switch_expression1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class Cls {\n");
    buf.push_str("\tpublic int foo(Day day) {\n");
    buf.push_str("\t\t// return variable\n");
    buf.push_str("\t\tint i;\n");
    buf.push_str("\t\tswitch (day) {\n");
    buf.push_str("\t\t\tcase SATURDAY:\n");
    buf.push_str("\t\t\tcase SUNDAY: i = 5; break;\n");
    buf.push_str("\t\t\tcase MONDAY:\n");
    buf.push_str("\t\t\tcase TUESDAY, WEDNESDAY: i = 7; break;\n");
    buf.push_str("\t\t\tcase THURSDAY:\n");
    buf.push_str("\t\t\tcase FRIDAY: i = 14; break;\n");
    buf.push_str("\t\t\tdefault :\n");
    buf.push_str("\t\t\t\ti = 22;\n");
    buf.push_str("\t\t\t\tbreak;\n");
    buf.push_str("\t\t}\n");
    buf.push_str("\t\treturn i;\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    buf.push_str("\n");
    buf.push_str("enum Day {\n");
    buf.push_str("    MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY, SATURDAY, SUNDAY;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class Cls {\n");
    buf.push_str("\tpublic int foo(Day day) {\n");
    buf.push_str("\t\t// return variable\n");
    buf.push_str("\t\tint i = switch (day) {\n");
    buf.push_str("\t\t\tcase SATURDAY, SUNDAY -> 5;\n");
    buf.push_str("\t\t\tcase MONDAY, TUESDAY, WEDNESDAY -> 7;\n");
    buf.push_str("\t\t\tcase THURSDAY, FRIDAY -> 14;\n");
    buf.push_str("\t\t\tdefault -> 22;\n");
    buf.push_str("\t\t};\n");
    buf.push_str("\t\treturn i;\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    buf.push_str("\n");
    buf.push_str("enum Day {\n");
    buf.push_str("    MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY, SATURDAY, SUNDAY;\n");
    buf.push_str("}\n");

    let e = Expected::new("Convert to switch expression", &buf);
    let selection = get_range(&t.ws.read(&cu), "switch");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_no_convert_to_switch_expression1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class Cls {\n");
    buf.push_str("\tstatic int i;\n");
    buf.push_str("\tstatic {\n");
    buf.push_str("\t\t// var comment\n");
    buf.push_str("\t\tint j = 4;\n");
    buf.push_str("\t\t// logic comment\n");
    buf.push_str("\t\tswitch (j) {\n");
    buf.push_str("\t\t\tcase 0: break; // no statements\n");
    buf.push_str("\t\t\tcase 1: i = 5; break;\n");
    buf.push_str("\t\t\tcase 2:\n");
    buf.push_str("\t\t\tcase 3:\n");
    buf.push_str("\t\t\tcase 4: System.out.println(\"here\"); i = 7; break;\n");
    buf.push_str("\t\t\tcase 5:\n");
    buf.push_str("\t\t\tcase 6: i = 14; break;\n");
    buf.push_str("\t\t\tdefault: i = 22; break;\n");
    buf.push_str("\t\t}\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &buf);
    let selection = get_range(&t.ws.read(&cu), "switch");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to switch expression");
}
