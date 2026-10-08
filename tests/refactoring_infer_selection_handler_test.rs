//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.InferSelectionHandlerTest`.
//!
//! `InferSelectionHandler.inferSelectionsForRefactor` is exercised through the
//! `java/inferSelection` request.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{to_range, QuickFixTest};
use serde_json::{json, Value};

const VERTICAL_BAR: &str = "/*|*/";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    (t, root)
}

/// `getVerticalBarRange(cu)`: the last position of the vertical bar comment.
fn vertical_bar_range(text: &str) -> Value {
    let index = text.find(VERTICAL_BAR).expect("vertical bar");
    let offset = text[..index].encode_utf16().count() + VERTICAL_BAR.encode_utf16().count();
    to_range(text, offset as i64, 0)
}

/// `InferSelectionHandler.inferSelectionsForRefactor(new InferSelectionParams(command, params))`.
fn infer_selection(t: &mut QuickFixTest, cu: &str, command: &str) -> Value {
    let diagnostics = t.diagnostics(cu);
    let range = vertical_bar_range(&t.ws.read(cu));
    t.ws.request(
        "java/inferSelection",
        json!({
            "command": command,
            "context": { "textDocument": { "uri": cu }, "range": range, "context": { "diagnostics": diagnostics, "only": ["refactor"] } }
        }),
    )
}

#[test]
fn test_infer_selection_when_extract_method() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        boolean b1 = true;\n");
    buf.push_str("        boolean b2 = false;\n");
    buf.push_str("        boolean b3 = true && !b2;\n");
    buf.push_str("        if (b1 && (/*|*/b2 || b3))\n");
    buf.push_str("            return 1;\n");
    buf.push_str("        \n");
    buf.push_str("        return 0;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let infos = infer_selection(&mut t, &cu, "extractMethod");
    let infos = infos.as_array().expect("selection infos");
    assert_eq!(infos.len(), 3);
    assert_eq!(infos[0]["name"], "b2");
    assert_eq!(infos[0]["length"], 2);
    assert_eq!(infos[1]["name"], "b2 || b3");
    assert_eq!(infos[1]["length"], 8);
    assert_eq!(infos[2]["name"], "b1 && (b2 || b3)");
    assert_eq!(infos[2]["length"], 21);
}

#[test]
fn test_infer_selection_when_extract_variable() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        boolean b1 = true;\n");
    buf.push_str("        boolean b2 = false;\n");
    buf.push_str("        boolean b3 = true && !b2;\n");
    buf.push_str("        boolean b4 = b3 || /*|*/b2 && b1;\n");
    buf.push_str("        if ((b1||b4) && (b2||b3))\n");
    buf.push_str("            return 1;\n");
    buf.push_str("        \n");
    buf.push_str("        return 0;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let infos = infer_selection(&mut t, &cu, "extractVariable");
    let infos = infos.as_array().expect("selection infos");
    assert_eq!(infos.len(), 3);
    assert_eq!(infos[0]["name"], "b2");
    assert_eq!(infos[0]["length"], 2);
    assert_eq!(infos[1]["name"], "b2 && b1");
    assert_eq!(infos[1]["length"], 8);
    assert_eq!(infos[2]["name"], "b3 || b2 && b1");
    assert_eq!(infos[2]["length"], 19);
}

#[test]
fn test_infer_selection_when_extract_constant() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        boolean b1 = true;\n");
    buf.push_str("        boolean b2 = false;\n");
    buf.push_str("        boolean b3 = /*|*/true || false;\n");
    buf.push_str("        boolean b4 = b3 || b2 && b1;\n");
    buf.push_str("        if ((b1||b4) && (b2||b3))\n");
    buf.push_str("            return 1;\n");
    buf.push_str("        \n");
    buf.push_str("        return 0;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let infos = infer_selection(&mut t, &cu, "extractConstant");
    let infos = infos.as_array().expect("selection infos");
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0]["name"], "true");
    assert_eq!(infos[0]["length"], 4);
    assert_eq!(infos[1]["name"], "true || false");
    assert_eq!(infos[1]["length"], 13);
}

#[test]
fn test_infer_selection_when_extract_field() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private String test = \"test\";\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        int hashCode = this./*|*/test.hashCode();\n");
    buf.push_str("        return 0;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let infos = infer_selection(&mut t, &cu, "extractField");
    let infos = infos.as_array().expect("selection infos");
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0]["name"], "this.test");
    assert_eq!(infos[0]["length"], 14);
    assert_eq!(infos[1]["name"], "this.test.hashCode()");
    assert_eq!(infos[1]["length"], 25);
}

