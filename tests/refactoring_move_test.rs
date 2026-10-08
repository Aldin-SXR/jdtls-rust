//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.MoveTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_range_len, get_title, QuickFixTest};
use serde_json::{json, Value};

const MOVE: &str = "Move...";
const APPLY_REFACTORING_COMMAND_ID: &str = "java.action.applyRefactoringCommand";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    t.ws.init_options["extendedClientCapabilities"]["moveRefactoringSupport"] = json!(true);
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    (t, root)
}

/// `assertCodeActions(cu, range, new Expected(MOVE, "", JavaCodeActionKind.REFACTOR_MOVE))` where
/// `evaluateCodeActionCommand` checks the apply refactoring command.
fn assert_move(t: &mut QuickFixTest, cu: &str, range: Value) {
    let actions = t.evaluate_code_actions_range(cu, range);
    let titles: Vec<String> = actions.iter().map(get_title).collect();
    let action = actions.iter().find(|a| get_title(a) == MOVE).unwrap_or_else(|| panic!("Should prompt code action: {MOVE} in {titles:?}"));
    assert_eq!("refactor.move", action["kind"], "{MOVE} has the wrong kind ");
    let command = if action["command"].is_string() { action } else { &action["command"] };
    assert_eq!(APPLY_REFACTORING_COMMAND_ID, command["command"]);
    assert!(!command["arguments"].is_null());
}

#[test]
fn test_move_class() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "E {", 0);
    assert_move(&mut t, &cu, range.clone());
}

#[test]
fn test_move_static_method() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public static void foo() {\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "foo() {", 0);
    assert_move(&mut t, &cu, range.clone());
}

#[test]
fn test_move_static_field() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public static String bar;\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "bar;", 0);
    assert_move(&mut t, &cu, range.clone());
}

#[test]
fn test_move_inner_class() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public class Inner {\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "Inner {", 0);
    assert_move(&mut t, &cu, range.clone());
}

#[test]
fn test_move_no_show_in_class_body() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("	// body");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "// body", 0);
    assert_move(&mut t, &cu, range.clone());

    t.set_only(&[]);
    t.assert_code_actions_range(&cu, range.clone(), &[]);
}

#[test]
fn test_move_no_show_in_method_body() {
    let (mut t, root) = setup();

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public static void foo() {\n");
    buf.push_str("		// body");
    buf.push_str("	}\n");
    buf.push_str("}\n");

    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let range = get_range_len(&t.ws.read(&cu), "// body", 0);
    assert_move(&mut t, &cu, range.clone());

    t.set_only(&[]);
    t.assert_code_actions_range(&cu, range.clone(), &[]);
}

