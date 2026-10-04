//! LSP ports of `WorkspaceDiagnosticsHandlerTest` using saved-file builds.
//! Marker conversion, Maven/build-path markers, tasks, encoding and missing
//! natures remain unported; this file does not claim coverage of those cases.

mod common;
use common::jdtls::*;
use serde_json::json;

#[test]
fn test_delete_package() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/unresolvedtype"]);
    let before = ws.published_diagnostics_min(1);
    assert!(before.iter().any(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")
        && r["diagnostics"].as_array().unwrap().iter().any(|d| d["severity"] == 1)), "unresolved type in Foo.java: {before:#?}");

    let folder = ws.project_root("unresolvedtype").join("src/pckg");
    assert!(folder.exists());
    std::fs::remove_dir_all(&folder).unwrap();
    ws.notify_file_changed(&folder, 3);
    let after = ws.published_diagnostics_min(1);
    let reports = after.iter().filter(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")).collect::<Vec<_>>();
    assert_eq!(1, reports.len(), "Should update the children's diagnostics of the deleted package: {after:#?}");
    assert!(reports[0]["diagnostics"].as_array().unwrap().is_empty(), "Should clean up the children's diagnostics of the deleted package");
}

#[test]
fn test_diagnostic_filtering() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "diagnostic": { "filter": ["**/Foo*.java"] } } });
    ws.import_projects(&["eclipse/hello"]);
    let reports = ws.published_diagnostics_min(1);
    assert!(!reports.is_empty());
    for report in reports {
        let uri = report["uri"].as_str().unwrap();
        assert!(!uri.contains("Foo"), "{uri} should have been excluded from diagnostics.");
    }
}
