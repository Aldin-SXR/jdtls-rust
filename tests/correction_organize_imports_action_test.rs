//! Verbatim sources and assertions from OrganizeImportsActionTest.
mod common;
use common::jdtls::*;
use common::quickfix::quickfix_client_capabilities;
use serde_json::{json, Value};

fn setup() -> (Workspace, std::path::PathBuf) {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    let root = ws.new_empty_project(&test_default_options());
    ws.use_upstream_test_jdk("TestProject");
    (ws, root)
}
fn organize(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.open(uri);
    let actions = ws.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": uri}, "range": range(0,0,0,0),
            "context": {"diagnostics": [], "only": ["source"]}
        }),
    );
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Organize imports")
        .unwrap_or_else(|| panic!("{actions:#}"));
    assert_eq!(action["kind"], "source.organizeImports");
    action["edit"].clone()
}
fn result(source: &str, uri: &str, edit: &Value) -> String {
    edit["changes"][uri]
        .as_array()
        .map(|edits| apply_edits(source, edits))
        .unwrap_or_else(|| source.to_owned())
}

#[test]
fn test_organize_imports_module_info() {
    let (mut ws, _root) = setup();
    ws.import_projects(&["eclipse/java9"]);
    let root = ws.project_root("java9");
    ws.use_upstream_test_jdk("java9");
    let source = "import foo.bar.MyDriverAction;\nimport java.sql.DriverAction;\nimport java.sql.SQLException;\n\nmodule mymodule.nine {\n\trequires java.sql;\n\texports foo.bar;\n\tprovides DriverAction with MyDriverAction;\n}\n";
    let cu = ws.create_cu(&root, "src/main/java", "", "module-info.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "import java.sql.DriverAction;\n\nimport foo.bar.MyDriverAction;\n\nmodule mymodule.nine {\n\trequires java.sql;\n\texports foo.bar;\n\tprovides DriverAction with MyDriverAction;\n}\n");
}

#[test]
fn test_organize_imports_unused() {
    let (mut ws, root) = setup();
    let source = "package test1;\n\nimport java.util.ArrayList;\n\npublic class E {\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(
        result(source, &cu, &edit),
        "package test1;\n\npublic class E {\n}\n"
    );
}

#[test]
fn test_organize_imports_sort() {
    let (mut ws, root) = setup();
    let source = "package test1;\n\nimport java.util.HashMap;\nimport java.util.ArrayList;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.HashMap;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
}

#[test]
fn test_organize_imports_on_demand_threshold() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java": {"sources": {"organizeImports": {"starThreshold": 2}}}});
    let source = "package test1;\n\nimport java.util.HashMap;\nimport java.util.ArrayList;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport java.util.*;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
}

#[test]
fn test_organize_imports_static_on_demand_threshold() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java": {"sources": {"organizeImports": {"staticStarThreshold": 2}}}});
    let source = "package test1;\n\nimport static java.lang.Math.pow;\nimport static java.lang.Math.sqrt;\n\npublic class E {\n\n    public E() {\n        double d1 = sqrt(4);\n        double d2 = pow(2, 2);\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport static java.lang.Math.*;\n\npublic class E {\n\n    public E() {\n        double d1 = sqrt(4);\n        double d2 = pow(2, 2);\n    }\n}\n");
}

#[test]
fn test_organize_imports_automatically_resolve() {
    let (mut ws, root) = setup();
    let source = "package test1;\n\nimport java.util.ArrayList;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.HashMap;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
}
