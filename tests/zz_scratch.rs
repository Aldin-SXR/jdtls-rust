//! Scratch test for capturing oracle output (not committed).

mod common;

use common::jdtls::test_default_options;
use common::quickfix::QuickFixTest;

#[test]
fn scratch() {
    let mut t = QuickFixTest::new();
    let options = test_default_options();
    let root = t.ws.new_empty_project(&options);
    let src = std::fs::read_to_string(std::env::var("SCRATCH_SRC").unwrap()).unwrap();
    let pkg = std::env::var("SCRATCH_PKG").unwrap_or_else(|_| "test1".into());
    let name = std::env::var("SCRATCH_NAME").unwrap_or_else(|_| "E.java".into());
    let cu = t.ws.create_cu(&root, "src", &pkg, &name, &src);
    let settings = t.ws.request(
        "workspace/executeCommand",
        serde_json::json!({ "command": "java.project.getSettings", "arguments": [cu, ["org.eclipse.jdt.core.formatter.tabulation.char", "org.eclipse.jdt.core.formatter.tabulation.size", "org.eclipse.jdt.core.compiler.problem.missingSerialVersion"]] }),
    );
    println!("SETTINGS: {settings}");
    if std::env::var("SCRATCH_CONFIG").is_ok() {
        t.ws.client().request_results.insert("workspace/configuration".into(), serde_json::json!([4, true]));
    }
    let diags = t.diagnostics(&cu);
    println!("DIAGNOSTICS: {}", serde_json::to_string_pretty(&diags).unwrap());
    let actions = t.evaluate_code_actions(&cu);
    println!("ACTIONS: {}", serde_json::to_string_pretty(&actions).unwrap());
}
