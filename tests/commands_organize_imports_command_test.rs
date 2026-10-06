//! Port of `org.eclipse.jdt.ls.core.internal.commands.OrganizeImportsCommandTest`.
mod common;
#[path = "../src/features/organize_imports/scope.rs"]
mod scope;
use common::jdtls::{apply_edits, test_default_options, Workspace};
use common::projects::{dir_uri, file_uri};
use serde_json::{json, Value};
use std::path::PathBuf;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    // The original calls OrganizeImportsCommand directly. Disable the delegate's
    // applyEdit branch so the wire response contains that same WorkspaceEdit.
    ws.capabilities["workspace"]["applyEdit"] = json!(false);
    let root = ws.new_empty_project(&test_default_options());
    ws.use_upstream_test_jdk("TestProject");
    (ws, root)
}
fn organize(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.execute("java.edit.organizeImports", vec![json!(uri)])
}
fn result(source: &str, uri: &str, edit: &Value) -> String {
    edit["changes"][uri]
        .as_array()
        .map(|edits| apply_edits(source, edits))
        .unwrap_or_else(|| source.to_owned())
}

#[test]
fn test_generic_organize_imports_call_invalid_file() {
    let (mut ws, _) = setup();
    ws.import_projects(&["eclipse/hello"]);
    assert!(ws
        .try_execute(
            "java.edit.organizeImports",
            vec![json!("no/such/file.java")]
        )
        .is_err());
}
#[test]
fn test_generic_organize_imports_call_null() {
    let (mut ws, _) = setup();
    ws.import_projects(&["eclipse/hello"]);
    // ExecuteCommandParams omits arguments for the original null-list branch.
    let edit = ws.request(
        "workspace/executeCommand",
        json!({"command": "java.edit.organizeImports"}),
    );
    assert!(!edit.is_null());
}
#[test]
fn test_generic_organize_imports_call() {
    let (mut ws, _) = setup();
    ws.import_projects(&["eclipse/hello"]);
    let root = ws.project_root("hello");
    let edit = organize(&mut ws, &file_uri(&root.join("src/java/Foo4.java")));
    assert!(edit.is_object(), "{edit:#}");
    let changes = edit["changes"].as_object().expect("workspace changes");
    assert!(!changes.is_empty());
    let first = &changes.values().next().unwrap()[0];
    assert_eq!(first["range"]["start"]["line"], 0);
    assert_eq!(first["range"]["end"]["line"], 4);
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
fn test_organize_imports_automatically_resolve() {
    let (mut ws, root) = setup();
    let source = "package test1;\n\nimport java.util.ArrayList;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.HashMap;\n\npublic class E {\n\n    public E() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
}

#[test]
fn test_organize_imports_filter_types() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java": {"completion": {"filteredTypes": ["java.util.*"]}}});
    let source = "package test1;\n\npublic class E {\n\n    public E() {\n        List list = new ArrayList();\n    }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", source);
    let edit = organize(&mut ws, &cu);
    assert_eq!(edit["changes"].as_object().unwrap().len(), 0);
    ws.update_settings(json!({"java": {"completion": {"filteredTypes": []}}}));
    let edit = organize(&mut ws, &cu);
    assert!(!edit["changes"].as_object().unwrap().is_empty());
    assert_eq!(result(source, &cu, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.List;\n\npublic class E {\n\n    public E() {\n        List list = new ArrayList();\n    }\n}\n");
}

#[test]
fn test_organize_imports_in_package() {
    let (mut ws, root) = setup();
    let source1 = "package test1;\n\nimport java.util.ArrayList;\n\npublic class E {\n}\n";
    let source2 = "package test1;\n\nimport java.util.HashMap;\nimport java.util.ArrayList;\n\npublic class F {\n\n    public F() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E.java", source1);
    let cu2 = ws.create_cu(&root, "src", "test1", "F.java", source2);
    // The original invokes organizeImportsInPackageFragment directly. Its
    // public delegate routes folder resources through the file branch, so use
    // the production package collector and the same per-CU compiler operation.
    let units = vec![
        (
            tower_lsp::lsp_types::Url::parse(&cu1).unwrap(),
            "test1".into(),
        ),
        (
            tower_lsp::lsp_types::Url::parse(&cu2).unwrap(),
            "test1".into(),
        ),
    ];
    let mut edit = json!({"changes": {}});
    for unit in scope::collect_compilation_units(&units, Some("test1")) {
        let organized = organize(&mut ws, unit.as_str());
        edit["changes"]
            .as_object_mut()
            .unwrap()
            .extend(organized["changes"].as_object().unwrap().clone());
    }
    assert_eq!(
        result(source1, &cu1, &edit),
        "package test1;\n\npublic class E {\n}\n"
    );
    assert_eq!(result(source2, &cu2, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.HashMap;\n\npublic class F {\n\n    public F() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
}

#[test]
fn test_organize_imports_in_project() {
    let (mut ws, root) = setup();
    let source1 = "package test1;\n\nimport java.util.ArrayList;\n\npublic class E {\n}\n";
    let source2 = "package test1;\n\nimport java.util.HashMap;\nimport java.util.ArrayList;\n\npublic class F {\n\n    public F() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E.java", source1);
    let cu2 = ws.create_cu(&root, "src", "test1", "F.java", source2);
    let edit = organize(&mut ws, &dir_uri(&root));
    assert_eq!(
        result(source1, &cu1, &edit),
        "package test1;\n\npublic class E {\n}\n"
    );
    assert_eq!(result(source2, &cu2, &edit), "package test1;\n\nimport java.util.ArrayList;\nimport java.util.HashMap;\n\npublic class F {\n\n    public F() {\n        ArrayList list = new ArrayList();\n        HashMap<String, String> map = new HashMap<String, String>();\n    }\n}\n");
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
