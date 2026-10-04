//! Port of `org.eclipse.jdt.ls.core.internal.managers.InvisibleProjectPreferenceChangeListenerTest`.
//!
//! `listener.preferencesChange(old, new)` is a
//! `workspace/didChangeConfiguration` with the new `java.project.*`
//! settings; the classpath and output location are read with
//! `java.project.getSettings`, `client.showMessage` is the
//! `window/showMessage` notification.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::Path;

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

/// `listener.preferencesChange(preferences, newPreferences)`.
fn change_project_preferences(ws: &mut Workspace, changes: Value) {
    let mut settings = ws.settings.clone();
    for (k, v) in changes.as_object().unwrap() {
        settings["java"]["project"][k] = v.clone();
    }
    ws.update_settings(settings);
    ws.wait_for_background_jobs();
}

fn output_location(ws: &mut Workspace, root: &Path) -> String {
    ws.project_setting(&dir_uri(root), OUTPUT_PATH).as_str().unwrap_or("").to_owned()
}

fn source_entries(ws: &mut Workspace, root: &Path) -> Vec<String> {
    let root = canonical(root);
    ws.source_paths(&root)
        .iter()
        .map(|p| Path::new(p).strip_prefix(&root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| p.clone()))
        .collect()
}

fn show_messages(ws: &mut Workspace) -> Vec<Value> {
    ws.wait_idle();
    ws.client().notifications.iter().filter(|n| n["method"] == "window/showMessage").cloned().collect()
}

#[test]
fn test_update_output_path() {
    let mut ws = workspace();
    ws.settings["java"]["project"]["outputPath"] = json!("");
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    let name = invisible_project_name(&canonical(&root));
    assert_eq!(canonical(&ws.workspace_project_location(&name)).join("bin").to_string_lossy(), output_location(&mut ws, &root));

    change_project_preferences(&mut ws, json!({ "outputPath": "bin" }));

    // `/<name>/_/bin`
    assert_eq!(canonical(&root).join("bin").to_string_lossy(), output_location(&mut ws, &root));
}

#[test]
fn test_update_output_path_wont_affect_source_path() {
    let mut ws = workspace();
    ws.settings["java"]["project"]["outputPath"] = json!("");
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    let original_source_path_count = source_entries(&mut ws, &root).len();

    change_project_preferences(&mut ws, json!({ "outputPath": "bin" }));

    let new_source_path_count = source_entries(&mut ws, &root).len();
    assert_eq!(original_source_path_count, new_source_path_count);
}

#[test]
fn test_update_output_path_to_un_empty_folder() {
    let mut ws = workspace();
    ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    ws.wait_for_background_jobs();
    let before = show_messages(&mut ws).len();

    change_project_preferences(&mut ws, json!({ "outputPath": "lib" }));

    assert_eq!(1, show_messages(&mut ws).len() - before);
}

#[test]
fn test_update_source_paths() {
    let mut ws = workspace();
    ws.settings["java"]["project"]["sourcePaths"] = json!(["src"]);
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    let source_paths = source_entries(&mut ws, &root);
    assert_eq!(1, source_paths.len());
    assert!(source_paths.contains(&"src".to_owned()));

    change_project_preferences(&mut ws, json!({ "sourcePaths": ["src", "test"] }));

    let source_paths = source_entries(&mut ws, &root);
    assert_eq!(2, source_paths.len());
    assert!(source_paths.contains(&"src".to_owned()));
    assert!(source_paths.contains(&"test".to_owned()));
}

/// The root path is removed from the preferences: the invisible project no
/// longer belongs to a workspace root, so its classpath is left alone.
#[test]
fn test_when_root_path_changed() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    ws.wait_idle();
    let before = show_messages(&mut ws).len();
    ws.remove_root(&root);

    change_project_preferences(&mut ws, json!({ "sourcePaths": ["src", "src2"] }));

    assert_eq!(0, show_messages(&mut ws).len() - before);
}

#[test]
fn test_update_source_paths2() {
    let mut ws = workspace();
    ws.settings["java"]["project"]["sourcePaths"] = json!(["src1"]);
    let root = ws.copy_and_import_folder("singlefile/wrong-packagename", Some("src/mypackage/Foo.java"));
    let source_paths = source_entries(&mut ws, &root);
    assert_eq!(0, source_paths.len());

    let uri = file_uri(&root.join("src/mypackage/Foo.java"));
    let reports = |ws: &mut Workspace| -> Vec<Value> {
        ws.wait_idle();
        ws.client().notifications.iter().filter(|n| n["method"] == "textDocument/publishDiagnostics").cloned().collect()
    };
    let before = reports(&mut ws).len();
    ws.open(&uri);
    let diagnostic_reports = ws
        .client()
        .recv_until(std::time::Duration::from_secs(30), |m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri))
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(1, diagnostic_reports.len());
    let _ = before;
    ws.client().notifications.retain(|n| n["method"] != "textDocument/publishDiagnostics");

    change_project_preferences(&mut ws, json!({ "sourcePaths": ["src"] }));

    let source_paths = source_entries(&mut ws, &root);
    assert_eq!(1, source_paths.len());
    assert!(source_paths.contains(&"src".to_owned()));
    let diagnostic_reports = reports(&mut ws);
    assert!(!diagnostic_reports.is_empty());
    for report in diagnostic_reports {
        assert!(report["params"]["diagnostics"].as_array().is_some_and(|d| d.is_empty()), "{report}");
    }
}
