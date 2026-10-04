//! End-to-end correction integration regressions, also run against jdt.ls.

mod common;
use common::jdtls::*;
use common::quickfix::quickfix_client_capabilities;
use serde_json::{json, Value};

fn organize(ws: &mut Workspace, uri: &str) -> Value {
    let result = ws.request(
        "textDocument/codeAction",
        json!({
            "textDocument":{"uri":uri}, "range":range(0,0,0,0),
            "context":{"diagnostics":[],"only":["source.organizeImports"]}
        }),
    );
    let actions = result.as_array().unwrap();
    assert_eq!(1, actions.len(), "{result:#?}");
    assert_eq!("Organize imports", actions[0]["title"]);
    actions[0].clone()
}

#[test]
fn deferred_organize_imports_resolves_and_expires_on_document_change() {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    let root = ws.new_empty_project(&test_default_options());
    let source = "import java.util.List;\npublic class E {}\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    ws.open_with(&uri, source);
    let action = organize(&mut ws, &uri);
    assert!(action["edit"].is_null(), "{action:#?}");
    assert_eq!(json!([]), action["diagnostics"]);
    assert!(action["data"]["rid"].is_string());
    assert!(action["data"]["pid"].is_string());
    let resolved = ws.request("codeAction/resolve", action.clone());
    let edits = resolved["edit"]["changes"][&uri]
        .as_array()
        .unwrap_or_else(|| panic!("{resolved:#?}"));
    assert_eq!("public class E {}\n", apply_edits(source, edits));
    assert!(resolved["data"].is_null());
    let action = organize(&mut ws, &uri);
    let current = "import java.util.Set;\npublic class E {}\n";
    ws.change(&uri, current);
    let stale = ws.request("codeAction/resolve", action.clone());
    assert_eq!(action, stale);
    let fresh = organize(&mut ws, &uri);
    let resolved = ws.request("codeAction/resolve", fresh);
    let edits = resolved["edit"]["changes"][&uri]
        .as_array()
        .unwrap_or_else(|| panic!("{resolved:#?}"));
    assert_eq!("public class E {}\n", apply_edits(current, edits));
}

#[test]
fn non_project_quick_fixes_toggle_the_actual_diagnostics_mode() {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.path_uri("Virtual.java");
    std::fs::write(ws.dir.join("Virtual.java"), "public class Virtual {}\n").unwrap();
    ws.open_with(&uri, "public class Virtual {}\n");
    let params = json!({"textDocument":{"uri":uri},"range":range(0,0,0,0),"context":{
        "only":["quickfix"],"diagnostics":[{"range":range(0,0,0,0),"code":"16","source":"Java","message":"Non-project file","severity":2}]
    }});
    let actions = ws.request("textDocument/codeAction", params.clone());
    let actions = actions.as_array().unwrap();
    let fixes: Vec<_> = actions
        .iter()
        .filter(|a| a["command"]["command"] == "java.project.refreshDiagnostics")
        .collect();
    assert_eq!(2, fixes.len(), "{actions:#?}");
    assert_eq!("Report compilation errors for this file", fixes[0]["title"]);
    assert_eq!(
        json!([uri, "thisFile", false]),
        fixes[0]["command"]["arguments"]
    );
    assert_eq!(
        json!([uri, "anyNonProjectFile", false]),
        fixes[1]["command"]["arguments"]
    );
    ws.request("workspace/executeCommand", fixes[0]["command"].clone());
    let actions = ws.request("textDocument/codeAction", params);
    let fixes: Vec<_> = actions
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["command"]["command"] == "java.project.refreshDiagnostics")
        .collect();
    assert_eq!(2, fixes.len());
    assert_eq!("Only report syntax errors for this file", fixes[0]["title"]);
    assert_eq!(
        json!([uri, "thisFile", true]),
        fixes[0]["command"]["arguments"]
    );
    assert_eq!(
        json!([uri, "anyNonProjectFile", true]),
        fixes[1]["command"]["arguments"]
    );
}

#[test]
fn organize_imports_combines_removals_and_missing_imports_in_one_edit() {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    let root = ws.new_empty_project(&test_default_options());
    let source = "import java.util.Set;\npublic class E { ArrayList<String> values; }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    ws.open_with(&uri, source);
    let action = organize(&mut ws, &uri);
    let edits = action["edit"]["changes"][&uri].as_array().unwrap();
    assert_eq!(1, edits.len());
    assert_eq!(
        "import java.util.ArrayList;\npublic class E { ArrayList<String> values; }\n",
        apply_edits(source, edits)
    );
}
