//! Wire-level chooser negotiation, cancellation, identities and virtual buffers.
mod common;
use common::jdtls::*;
use common::quickfix::quickfix_client_capabilities;
use serde_json::{json, Value};

fn setup(advanced: bool, execute: bool) -> (Workspace, std::path::PathBuf) {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    ws.init_options["extendedClientCapabilities"] = json!({
        "advancedOrganizeImportsSupport": advanced, "executeClientCommandSupport": execute});
    let root = ws.new_empty_project(&test_default_options());
    ws.use_upstream_test_jdk("TestProject");
    for package in ["p1", "p2"] {
        for name in ["C", "D"] {
            ws.create_cu(
                &root,
                "src",
                package,
                &format!("{name}.java"),
                &format!("package {package}; public class {name} {{}}"),
            );
        }
    }
    (ws, root)
}
fn params(uri: &str, only: &[&str]) -> Value {
    json!({"textDocument": {"uri": uri}, "range": range(0,0,0,0),
        "context": {"diagnostics": [], "only": only}})
}
fn request(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.request("java/organizeImports", params(uri, &[]))
}
fn result(source: &str, uri: &str, edit: &Value) -> String {
    edit["changes"]
        .as_object()
        .and_then(|changes| {
            changes.iter().find_map(|(key, value)| {
                (tower_lsp::lsp_types::Url::parse(key).ok()
                    == tower_lsp::lsp_types::Url::parse(uri).ok())
                .then_some(value)
            })
        })
        .and_then(Value::as_array)
        .map(|e| apply_edits(source, e))
        .unwrap_or_else(|| source.to_owned())
}
fn calls(ws: &mut Workspace) -> Vec<Value> {
    ws.client()
        .server_requests
        .iter()
        .filter(|r| {
            r["method"] == "workspace/executeClientCommand"
                && r["params"]["command"] == "java.action.organizeImports.chooseImports"
        })
        .cloned()
        .collect()
}
fn chooser(ws: &mut Workspace, f: impl FnMut(&Value) -> Value + Send + 'static) {
    ws.client()
        .request_handlers
        .insert("workspace/executeClientCommand".into(), Box::new(f));
}
fn source_action(ws: &mut Workspace, uri: &str, title: &str) -> Value {
    let actions = ws.request("textDocument/codeAction", params(uri, &["source"]));
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("{actions:#}"));
    if action["data"].is_null() {
        action.clone()
    } else {
        ws.request("codeAction/resolve", action.clone())
    }
}
const SOURCE: &str =
    "package p;\n\nimport java.util.Set;\n\npublic class B { C c; ArrayList<String> list; }\n";

#[test]
fn null_reply_cancels_removal_and_unique_imports() {
    let (mut ws, root) = setup(true, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |_| Value::Null);
    assert!(request(&mut ws, &uri).is_null());
    assert_eq!(1, calls(&mut ws).len());
    assert_eq!(SOURCE, ws.read(&uri));
}
#[test]
fn empty_reply_keeps_unique_edits() {
    let (mut ws, root) = setup(true, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |_| json!([]));
    let edit = request(&mut ws, &uri);
    assert_eq!(
        SOURCE.replace("java.util.Set", "java.util.ArrayList"),
        result(SOURCE, &uri, &edit)
    );
}
#[test]
fn only_known_ids_select_imports_and_names_are_not_trusted() {
    let (mut ws, root) = setup(true, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |msg| {
        let selected = &msg["params"]["arguments"][1][0]["candidates"][1];
        json!([null, {"id":"unknown", "fullyQualifiedName":"p1.C"},
            {"id":selected["id"], "fullyQualifiedName":"forged.Name"}])
    });
    let edit = request(&mut ws, &uri);
    assert_eq!(
        SOURCE.replace(
            "import java.util.Set;",
            "import java.util.ArrayList;\n\nimport p2.C;"
        ),
        result(SOURCE, &uri, &edit)
    );
}
#[test]
fn missing_execute_support_cancels_without_sending_a_request() {
    let (mut ws, root) = setup(true, false);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    assert!(request(&mut ws, &uri).is_null());
    assert!(calls(&mut ws).is_empty());
}
#[test]
fn direct_handler_uses_chooser_without_advanced_source_action_capability() {
    let (mut ws, root) = setup(false, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |msg| {
        json!([msg["params"]["arguments"][1][0]["candidates"][0]])
    });
    let edit = request(&mut ws, &uri);
    assert!(result(SOURCE, &uri, &edit).contains("import p1.C;"));
    assert_eq!(1, calls(&mut ws).len());
}
#[test]
fn source_action_is_noninteractive_without_advanced_capability() {
    let (mut ws, root) = setup(false, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    let action = source_action(&mut ws, &uri, "Organize imports");
    assert_eq!(
        SOURCE.replace("java.util.Set", "java.util.ArrayList"),
        result(SOURCE, &uri, &action["edit"])
    );
    assert!(calls(&mut ws).is_empty());
}
#[test]
fn deferred_source_action_chooses_on_resolve() {
    let (mut ws, root) = setup(true, true);
    ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |msg| {
        json!([msg["params"]["arguments"][1][0]["candidates"][0]])
    });
    let actions = ws.request(
        "textDocument/codeAction",
        params(&uri, &["source.organizeImports"]),
    );
    assert!(calls(&mut ws).is_empty());
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Organize imports")
        .unwrap();
    assert!(action["edit"].is_null());
    let action = ws.request("codeAction/resolve", action.clone());
    assert!(result(SOURCE, &uri, &action["edit"]).contains("import p1.C;"));
    let sent = calls(&mut ws);
    assert_eq!(1, sent.len());
    assert_eq!(
        json!(uri.replacen("file://", "file:", 1)),
        sent[0]["params"]["arguments"][0]
    );
    assert_eq!(false, sent[0]["params"]["arguments"][2]);
}
#[test]
fn add_missing_imports_retains_existing_imports_and_sets_restore_flag() {
    let (mut ws, root) = setup(true, true);
    let uri = ws.create_cu(&root, "src", "p", "B.java", SOURCE);
    ws.open(&uri);
    chooser(&mut ws, |msg| {
        json!([msg["params"]["arguments"][1][0]["candidates"][0]])
    });
    let action = source_action(&mut ws, &uri, "Add all missing imports");
    assert_eq!("source", action["kind"]);
    assert_eq!(
        SOURCE.replace(
            "import java.util.Set;",
            "import java.util.ArrayList;\nimport java.util.Set;\n\nimport p1.C;"
        ),
        result(SOURCE, &uri, &action["edit"])
    );
    let sent = calls(&mut ws);
    assert_eq!(2, sent.len());
    assert_eq!(false, sent[0]["params"]["arguments"][2]);
    assert_eq!(true, sent[1]["params"]["arguments"][2]);
}
#[test]
fn multiple_ambiguities_have_utf16_ranges_and_one_prompt() {
    let (mut ws, root) = setup(true, true);
    let source = "package p;\r\n\r\npublic class B { /* 😀 */ C c; D d; C again; }\r\n";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.open(&uri);
    chooser(&mut ws, |msg| {
        let selections = msg["params"]["arguments"][1].as_array().unwrap();
        assert_eq!(2, selections.len());
        let mut selected = Vec::new();
        for s in selections {
            let first = &s["candidates"][0];
            let expected = match first["fullyQualifiedName"].as_str().unwrap() {
                "p1.C" => range(2, 26, 2, 27),
                "p1.D" => range(2, 31, 2, 32),
                name => panic!("{name}"),
            };
            assert_eq!(expected, s["range"]);
            assert_ne!(s["candidates"][0]["id"], s["candidates"][1]["id"]);
            selected.push(first.clone());
        }
        json!(selected)
    });
    let edit = request(&mut ws, &uri);
    assert_eq!(
        source.replace(
            "\r\n\r\npublic",
            "\r\n\r\nimport p1.C;\r\nimport p1.D;\r\n\r\npublic"
        ),
        result(source, &uri, &edit)
    );
    assert_eq!(1, calls(&mut ws).len());
}
#[test]
fn no_ambiguity_does_not_require_client_command_support() {
    let (mut ws, root) = setup(true, false);
    let source = "package p;\npublic class B { ArrayList<String> list; }\n";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.open(&uri);
    let edit = request(&mut ws, &uri);
    assert!(result(source, &uri, &edit).contains("import java.util.ArrayList;"));
    assert!(calls(&mut ws).is_empty());
    let source = "package p;\npublic class B {}\n";
    ws.change(&uri, source);
    assert!(request(&mut ws, &uri).is_null());
}
#[test]
fn add_missing_imports_is_offered_only_for_undefined_types() {
    let (mut ws, root) = setup(false, false);
    let source = "package p;\npublic class B { int i = \"bad\"; }\n";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.open(&uri);
    let actions = ws.request("textDocument/codeAction", params(&uri, &["source"]));
    assert!(!actions
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["title"] == "Add all missing imports"));
}
#[test]
fn virtual_buffers_add_jdk_imports_without_creating_source_files() {
    if is_oracle() {
        return;
    } // Required Rust extension to Eclipse's resource model.
    for uri in [
        "untitled:Virtual.java",
        "inmemory:///Virtual.java",
        "file:///absent/import-choice/Virtual.java",
    ] {
        let mut ws = Workspace::new();
        let source = "public class Virtual { ArrayList<String> list; }\n";
        ws.open_with(uri, source);
        let edit = request(&mut ws, uri);
        assert_eq!(
            format!("import java.util.ArrayList;\n\n{source}"),
            result(source, uri, &edit)
        );
        assert!(!std::path::Path::new("/absent/import-choice/Virtual.java").exists());
    }
}

#[test]
fn standalone_warning_does_not_prevent_type_errors_or_jdk_import_fixes() {
    let mut ws = Workspace::new();
    ws.capabilities = quickfix_client_capabilities();
    let path = ws.external_dir().join("Standalone.java");
    let uri = tower_lsp::lsp_types::Url::from_file_path(&path)
        .unwrap()
        .to_string();
    let source =
        "public class Standalone { int x = \"this is not an int\"; ArrayList<String> items; }\n";
    std::fs::write(path, source).unwrap();
    // Set the mode before didOpen, so its asynchronous initial diagnostic
    // report is already in full-validation mode on both servers.
    ws.request(
        "workspace/executeCommand",
        json!({"command":"java.project.refreshDiagnostics",
        "arguments":[uri,"anyNonProjectFile",false]}),
    );
    ws.open_with(&uri, source);
    let diagnostics = ws.diagnostics(&uri);
    let warning = diagnostics
        .iter()
        .find(|d| d["code"] == "16")
        .unwrap_or_else(|| panic!("{diagnostics:#?}"));
    assert_eq!(2, warning["severity"]);
    assert_eq!(
        "Standalone.java is a non-project file, only JDK classes are added to its build path",
        warning["message"]
    );
    assert!(diagnostics
        .iter()
        .any(|d| d["message"] == "Type mismatch: cannot convert from String to int"));
    let missing = diagnostics
        .iter()
        .find(|d| d["message"] == "ArrayList cannot be resolved to a type")
        .unwrap();
    let actions = ws.request(
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},
        "range":missing["range"], "context":{"only":["quickfix"],"diagnostics":[missing]}}),
    );
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Import 'ArrayList' (java.util)")
        .unwrap_or_else(|| panic!("{actions:#}"));
    let with_import = result(source, &uri, &action["edit"]);
    assert!(with_import.contains("import java.util.ArrayList;"));
    let fixed = with_import.replace("\"this is not an int\"", "42");
    ws.change(&uri, &fixed);
    let diagnostics = ws.diagnostics(&uri);
    assert_eq!(1, diagnostics.len(), "{diagnostics:#?}");
    assert_eq!("16", diagnostics[0]["code"]);
    assert_eq!(2, diagnostics[0]["severity"]);
    assert_eq!(source, ws.read(&uri)); // Client edits never save the file implicitly.
}

#[test]
fn source_action_chooser_uses_java_location_uri_for_unicode_and_spaces() {
    let (mut ws, _) = setup(true, true);
    let root = ws.new_project("Unicode Project", &test_default_options());
    ws.use_upstream_test_jdk("Unicode Project");
    for package in ["p1", "p2"] {
        ws.create_cu(
            &root,
            "src",
            package,
            "C.java",
            &format!("package {package}; public class C {{}}"),
        );
    }
    let source = "package p;\npublic class Café { C c; }\n";
    let uri = ws.create_cu(&root, "src", "p", "Café.java", source);
    ws.open(&uri);
    assert!(uri.contains("Unicode%20Project"));
    let expected = uri
        .replacen("file://", "file:", 1)
        .replace("Caf%C3%A9.java", "Café.java");
    chooser(&mut ws, move |msg| {
        assert_eq!(expected, msg["params"]["arguments"][0]);
        json!([msg["params"]["arguments"][1][0]["candidates"][0]])
    });
    let action = source_action(&mut ws, &uri, "Organize imports");
    assert!(
        result(source, &uri, &action["edit"]).contains("import p1.C;"),
        "{action:#}"
    );
}
