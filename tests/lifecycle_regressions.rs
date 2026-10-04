//! End-to-end regressions for working-copy validation and file events.

mod common;
use common::jdtls::*;
use serde_json::json;

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
