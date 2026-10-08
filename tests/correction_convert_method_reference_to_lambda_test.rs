//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConvertMethodReferenceToLambdaTest`.

mod common;

use common::jdtls::{range, test_default_options};
use common::quickfix::QuickFixTest;
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.init_options["extendedClientCapabilities"]["advancedExtractRefactoringSupport"] = json!(true);
    let root = t.ws.new_empty_project(&test_default_options());
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
    (t, root)
}

#[test]
fn test_method_reference_to_lambda() {
    let (mut t, root) = setup();
    t.set_ignored_commands(&[
        "Assign statement to new field",
        "Extract to constant",
        "Extract to field",
        "Extract to local variable (replace all occurrences)",
        "Extract to local variable",
        "Introduce Parameter...",
        "Generate Constructors...",
        "Generate Constructors",
    ]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.stream.Stream;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private Stream<String> asHex(Stream<Integer> stream) {\n");
    buf.push_str("        return stream.map(Integer::toHexString);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let actions = t.evaluate_code_actions_range(&cu, range(4, 34, 4, 34));
    assert_eq!(2, actions.len(), "{actions:#?}");
    let action = &actions[0];
    assert_eq!("quickassist", action["kind"]);
    assert_eq!("Convert to lambda expression", action["title"]);
    assert!(!action["edit"].is_null());
}

#[test]
fn test_lambda_to_method_reference() {
    let (mut t, root) = setup();
    t.set_ignored_commands(&[
        "Assign statement to new field",
        "Extract to constant",
        "Extract to field",
        "Extract to local variable (replace all occurrences)",
        "Extract to local variable",
        "Introduce Parameter...",
        "Generate Constructors...",
        "Generate Constructors",
        "Extract lambda body to method",
        "Convert to method reference",
    ]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.stream.Stream;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private Stream<String> asHex(Stream<Integer> stream) {\n");
    buf.push_str("        return stream.map(t -> Integer.toHexString(t));\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let actions = t.evaluate_code_actions_range(&cu, range(4, 39, 4, 39));
    assert_eq!(2, actions.len(), "{actions:#?}");
    let action = &actions[1];
    assert_eq!("quickassist", action["kind"]);
    assert_eq!("Clean up lambda expression", action["title"]);
    assert!(!action["edit"].is_null());
}
