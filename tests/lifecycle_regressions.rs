//! End-to-end regressions for working-copy validation and file events.

mod common;
use common::jdtls::*;
use serde_json::json;
use tower_lsp::lsp_types::Url;

#[test]
fn full_diagnostics_for_unsaved_buffers_report_and_clear_type_errors() {
    for scheme in ["file", "untitled", "inmemory"] {
        let mut ws = Workspace::new();
        let missing = ws.external_dir().join("Main.java");
        let uri = if scheme == "file" {
            Url::from_file_path(&missing).unwrap().to_string()
        } else {
            format!("{scheme}:///Main.java")
        };
        let source = "public class Main {\n    void run() {\n        int x = \"this is not an int\";\n        System.out.println(x);\n    }\n}\n";
        // Like the web client, set full validation before didOpen. No file or
        // imported project is needed, and edits must keep that validation mode.
        ws.published_diagnostics();
        ws.request("workspace/executeCommand", json!({
            "command": "java.project.refreshDiagnostics",
            "arguments": [uri, "thisFile", false],
        }));
        assert!(ws.published_diagnostics().is_empty());
        ws.open_with(&uri, source);
        let reports = ws.published_diagnostics_min(1);
        let report = reports.iter().find(|r| r["uri"] == uri).unwrap();
        let diagnostics = report["diagnostics"].as_array().unwrap();
        let error = diagnostics.iter().find(|d| d["severity"] == 1).unwrap();
        assert_eq!("Type mismatch: cannot convert from String to int", error["message"]);
        assert_eq!(range(2, 16, 2, 36), error["range"]);

        ws.change(&uri, &source.replace("\"this is not an int\"", "42"));
        let reports = ws.published_diagnostics_min(1);
        let report = reports.iter().find(|r| r["uri"] == uri).unwrap();
        assert!(report["diagnostics"].as_array().unwrap().iter().all(|d| d["severity"] != 1), "{report:#?}");
        assert!(!missing.exists(), "validation must use the open buffer");
    }
}

#[test]
fn other_open_buffers_are_validated_on_the_next_trigger_like_jdtls() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "edit": { "validateAllOpenBuffersOnChanges": true } } });
    let root = ws.new_empty_project(&Default::default());
    let dependency = "package test1;\npublic class Dependency {}\n";
    let dependent = "package test1;\npublic class Dependent { void run() { Dependency.run(); } }\n";
    let dependency_uri = ws.create_cu(&root, "src", "test1", "Dependency.java", dependency);
    let dependent_uri = ws.create_cu(&root, "src", "test1", "Dependent.java", dependent);
    ws.published_diagnostics();
    ws.open_with(&dependency_uri, dependency);
    ws.open_with(&dependent_uri, dependent);
    let before = ws.published_diagnostics_min(2);
    assert!(before.iter().any(|r| r["uri"] == dependent_uri && r["diagnostics"].as_array().unwrap().len() == 1), "{before:#?}");

    ws.change(&dependency_uri, "package test1;\npublic class Dependency { public static void run() {} }\n");
    let first = ws.published_diagnostics_min(1);
    assert_eq!(1, first.len(), "{first:#?}");
    assert_eq!(dependency_uri, first[0]["uri"]);
    assert!(first[0]["diagnostics"].as_array().unwrap().is_empty());

    // BaseDocumentLifeCycleHandler.publishDiagnostics snapshots the queue
    // before adding other working copies, so a second change processes them.
    ws.change(&dependency_uri, "package test1;\npublic class Dependency { public static void run() {} }\n\n");
    let after = ws.published_diagnostics_min(2);
    assert!(after.iter().any(|r| r["uri"] == dependent_uri && r["diagnostics"].as_array().unwrap().is_empty()), "{after:#?}");
    assert!(after.iter().any(|r| r["uri"] == dependency_uri && r["diagnostics"].as_array().unwrap().is_empty()), "{after:#?}");
}

#[test]
fn resource_filters_keep_virtual_buffers_in_the_default_project() {
    use common::jdtls::*;
    use tower_lsp::lsp_types::Url;
    for scheme in ["file", "untitled", "inmemory"] {
        let mut ws = Workspace::new();
        ws.settings = json!({"java":{"project":{"resourceFilters":[".*"]}}});
        let path = ws.external_dir().join("Main.java");
        let uri = if scheme == "file" {
            Url::from_file_path(&path).unwrap().to_string()
        } else {
            format!("{scheme}:///Main.java")
        };
        let source = "public class Main { int x = \"bad\"; }\n";
        ws.request(
            "workspace/executeCommand",
            json!({
                "command":"java.project.refreshDiagnostics", "arguments":[uri, "thisFile", false]
            }),
        );
        ws.open_with(&uri, source);
        let reports = ws.published_diagnostics_min(1);
        let report = reports
            .iter()
            .find(|r| r["uri"] == uri)
            .unwrap_or_else(|| panic!("virtual buffer diagnostics for {uri}"));
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["severity"] == 1
                    && d["message"] == "Type mismatch: cannot convert from String to int"),
            "{report:#?}"
        );
        ws.change(&uri, &source.replace("\"bad\"", "42"));
        let reports = ws.published_diagnostics_min(1);
        let report = reports
            .iter()
            .find(|r| r["uri"] == uri)
            .expect("updated virtual diagnostics");
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d["severity"] != 1),
            "{report:#?}"
        );
        assert!(
            !path.exists(),
            "virtual buffers must not be written to disk"
        );
    }
}
