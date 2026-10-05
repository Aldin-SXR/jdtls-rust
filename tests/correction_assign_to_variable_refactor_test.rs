//! Faithful ports of AssignToVariableRefactorTest (advanced client commands).
mod common;
use common::jdtls::{range, test_default_options};
use common::quickfix::QuickFixTest;
use serde_json::json;

fn check(field: bool) {
    let mut t = QuickFixTest::new();
    t.ws.init_options["extendedClientCapabilities"]["advancedExtractRefactoringSupport"] =
        json!(true);
    t.set_ignored_kind(&[
        "refactor",
        "source.overrideMethods",
        "source.generate.toString",
        "source.generate.constructors",
        "source.generate.finalModifiers",
        "refactor.extract.field",
        "refactor.extract.variable",
        "refactor.introduce.parameter",
        "refactor.inline",
    ]);
    t.set_ignored_commands(&[
        if field {
            "Assign statement to new local variable"
        } else {
            "Assign statement to new field"
        },
        "Generate Constructors...",
        "Generate Constructors",
        "Add Javadoc comment",
    ]);
    let root = t.ws.new_empty_project(&test_default_options());
    let source = "package test1;\npublic class E {\n    public static void main(String[] args) {\n        E test = new E();\n        test.foo();\n    }\n    public int foo() {\n        return 1;\n    }\n}\n";
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", source);
    for column in [10, 15] {
        let actions = t.evaluate_code_actions_range(&uri, range(4, column, 4, column));
        assert_eq!(actions.len(), 1, "{actions:#?}");
        let action = &actions[0];
        assert_eq!(
            action["kind"],
            if field {
                "refactor.assign.field"
            } else {
                "refactor.assign.variable"
            }
        );
        assert_eq!(
            action["title"],
            if field {
                "Assign statement to new field"
            } else {
                "Assign statement to new local variable"
            }
        );
        let command = &action["command"];
        assert_eq!(command["command"], "java.action.applyRefactoringCommand");
        assert!(command["arguments"].is_array());
        assert_eq!(
            command["arguments"][0],
            if field {
                "assignField"
            } else {
                "assignVariable"
            }
        );
    }
}
#[test]
fn test_assign_statement_to_variable() {
    check(false);
}
#[test]
fn test_assign_statement_to_field() {
    check(true);
}
