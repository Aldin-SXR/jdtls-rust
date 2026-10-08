//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.AdvancedExtractTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_selection_range, QuickFixTest};
use serde_json::{json, Value};

const APPLY_REFACTORING_COMMAND_ID: &str = "java.action.applyRefactoringCommand";

/// `isAdvancedExtractRefactoringSupported()` (and `isMoveRefactoringSupported()` for the move tests).
fn setup(move_support: bool) -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    t.ws.init_options["extendedClientCapabilities"]["advancedExtractRefactoringSupport"] = json!(true);
    if move_support {
        t.ws.init_options["extendedClientCapabilities"]["moveRefactoringSupport"] = json!(true);
    }
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    (t, root)
}

/// `CodeActionHandlerTest.getCommand`.
fn command(action: &Value) -> &Value {
    if action["command"].is_string() {
        action
    } else {
        &action["command"]
    }
}

/// `CodeActionHandlerTest.findAction(codeActions, kind)`.
fn find_action<'a>(actions: &'a [Value], kind: &str) -> Option<&'a Value> {
    actions.iter().find(|a| if a["command"].is_string() { a["command"] == kind } else { a["kind"] == kind })
}

fn evaluate(t: &mut QuickFixTest, cu: &str) -> Vec<Value> {
    let selection = get_selection_range(&t.ws.read(cu));
    t.evaluate_code_actions_range(cu, selection)
}

fn assert_command(command: &Value, first_argument: &str, arguments: usize) {
    assert_eq!(APPLY_REFACTORING_COMMAND_ID, command["command"]);
    assert!(!command["arguments"].is_null());
    assert_eq!(arguments, command["arguments"].as_array().expect("arguments").len());
    assert_eq!(first_argument, command["arguments"][0]);
}

#[test]
fn test_extract_variable() {
    let (mut t, root) = setup(false);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let extract_code_actions: Vec<Value> = code_actions.iter().filter(|a| a["kind"].as_str().is_some_and(|k| k.starts_with("refactor.extract"))).cloned().collect();
    assert_eq!(5, extract_code_actions.len());
    let find = |argument: &str| extract_code_actions.iter().map(command).find(|c| c["arguments"][0] == argument).cloned();

    let extract_constant_command = find("extractConstant").expect("extract constant");
    assert_command(&extract_constant_command, "extractConstant", 2);

    let extract_field_command = find("extractField").expect("extract field");
    assert_command(&extract_field_command, "extractField", 3);

    let extract_method_command = find("extractMethod").expect("extract method");
    assert_command(&extract_method_command, "extractMethod", 2);

    let extract_variable_all_command = find("extractVariableAllOccurrence").expect("extract variable all occurrence");
    assert_command(&extract_variable_all_command, "extractVariableAllOccurrence", 2);

    let extract_variable_command = find("extractVariable").expect("extract variable");
    assert_command(&extract_variable_command, "extractVariable", 2);
}

#[test]
fn test_extract_method() {
    let (mut t, root) = setup(false);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo(boolean b1, boolean b2) {\n");
    buf.push_str("        int n = 0;\n");
    buf.push_str("        int i = 0;\n");
    buf.push_str("        /*[*/\n");
    buf.push_str("        if (b1)\n");
    buf.push_str("            i = 1;\n");
    buf.push_str("        if (b2)\n");
    buf.push_str("            n = n + i;\n");
    buf.push_str("        /*]*/\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let extract_method_action = find_action(&code_actions, "refactor.extract.function").expect("extract method action");
    assert_command(command(extract_method_action), "extractMethod", 2);
}

#[test]
fn test_move_file() {
    let (mut t, root) = setup(true);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public /*[*/class E /*]*/{\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let move_action = find_action(&code_actions, "refactor.move").expect("move action");
    let move_command = command(move_action);
    assert_command(move_command, "moveFile", 3);
    assert!(move_command["arguments"][2].is_object());
    assert_eq!(cu, move_command["arguments"][2]["uri"]);
}

#[test]
fn test_move_instance_method() {
    let (mut t, root) = setup(true);
    t.ws.create_cu(
        &root,
        "src",
        "test1",
        "Second.java",
        &("package test1;\n".to_owned() + "\n" + "public class Second {\n" + "    public void bar() {\n" + "    }\n" + "}"),
    );
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    Second s;\n");
    buf.push_str("    public void print() {\n");
    buf.push_str("        /*[*//*]*/s.bar();\n");
    buf.push_str("    }\n");
    buf.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let move_action = find_action(&code_actions, "refactor.move").expect("move action");
    let move_command = command(move_action);
    assert_command(move_command, "moveInstanceMethod", 3);
    assert!(move_command["arguments"][2].is_object());
    assert_eq!("print()", move_command["arguments"][2]["displayName"]);
}

#[test]
fn test_move_static_member() {
    let (mut t, root) = setup(true);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void print() {\n");
    buf.push_str("        /*[*//*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let move_action = find_action(&code_actions, "refactor.move").expect("move action");
    let move_command = command(move_action);
    assert_command(move_command, "moveStaticMember", 3);
    let info = &move_command["arguments"][2];
    assert!(info.is_object());
    let project_name = root.file_name().expect("project name").to_str().expect("utf-8");
    assert_eq!(project_name, info["projectName"]);
    assert_eq!("print()", info["displayName"]);
    // ASTNode.METHOD_DECLARATION
    assert_eq!(31, info["memberType"]);
    assert_eq!("test1.E", info["enclosingTypeName"]);
}

#[test]
fn test_move_inner_type() {
    let (mut t, root) = setup(true);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    class Inner {\n");
    buf.push_str("        /*[*//*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let code_actions = evaluate(&mut t, &cu);
    let move_action = find_action(&code_actions, "refactor.move").expect("move action");
    let move_command = command(move_action);
    assert_command(move_command, "moveType", 3);
    let info = &move_command["arguments"][2];
    assert!(info.is_object());
    let project_name = root.file_name().expect("project name").to_str().expect("utf-8");
    assert_eq!(project_name, info["projectName"]);
    assert_eq!("Inner", info["displayName"]);
    assert_eq!("test1.E", info["enclosingTypeName"]);
    assert!(!info["supportedDestinationKinds"].as_array().expect("destination kinds").is_empty());
}
