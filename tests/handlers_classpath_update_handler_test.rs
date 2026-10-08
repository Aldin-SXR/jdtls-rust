//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ClasspathUpdateHandlerTest`.
//!
//! `connection.sendEventNotification` is the `language/eventNotification`
//! notification; `reset(connection)` drops the notifications received so far.
//! Against jdt.ls, `test_classpath_update_for_gradle` cannot pass: Buildship
//! re-syncs on workspace resource changes, which a file written outside the
//! server does not cause.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::Path;

const CHANGED: u32 = 2;
const CREATED: u32 = 1;

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "configuration": { "updateBuildConfiguration": "automatic" } } });
    ws
}

fn event_notifications(ws: &mut Workspace) -> Vec<Value> {
    ws.client().settle(std::time::Duration::from_secs(5), std::time::Duration::from_secs(120));
    ws.client().take_notifications("language/eventNotification")
}

/// jdt.ls attaches the `ClasspathUpdateHandler` at the end of its
/// initialization job, which publishes the workspace diagnostics just before.
fn wait_for_listener(ws: &mut Workspace) {
    let c = ws.client();
    c.recv_until(std::time::Duration::from_secs(120), |m| m["method"] == "textDocument/publishDiagnostics");
    c.settle(std::time::Duration::from_secs(5), std::time::Duration::from_secs(60));
}

fn reset_connection(ws: &mut Workspace) {
    ws.wait_idle();
    event_notifications(ws);
}

fn assert_classpath_updated(events: &[Value], expected: &Path) {
    assert_eq!(1, events.len(), "{events:#?}");
    let params = &events[0]["params"];
    assert_eq!(json!(100), params["eventType"]);
    let data = params["data"].as_str().expect("data");
    let uri = tower_lsp::lsp_types::Url::parse(data).unwrap();
    assert_eq!(canonical(expected), canonical(Path::new(uri.path())));
}

fn replace_in(file: &Path, from: &str, to: &str) {
    let content = std::fs::read_to_string(file).unwrap();
    std::fs::write(file, content.replace(from, to)).unwrap();
}

#[test]
fn test_classpath_update_for_maven() {
    let mut ws = workspace();
    ws.import_projects(&["maven/salut"]);
    wait_for_listener(&mut ws);
    let project = ws.dir.join("maven/salut");
    let pom = project.join("pom.xml");
    assert!(pom.exists());
    replace_in(&pom, "<version>3.18.0</version>", "<version>3.6</version>");

    reset_connection(&mut ws);
    ws.files_changed(&[(&pom, CHANGED)]);

    let events = event_notifications(&mut ws);
    assert_classpath_updated(&events, &project);
}

#[test]
fn test_classpath_update_for_gradle() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/simple-gradle"]);
    wait_for_listener(&mut ws);
    let project = ws.dir.join("gradle/simple-gradle");
    let build_gradle = project.join("build.gradle");
    assert!(build_gradle.exists());
    replace_in(&build_gradle, "org.slf4j:slf4j-api:1.7.21", "org.slf4j:slf4j-api:1.7.20");

    reset_connection(&mut ws);
    ws.files_changed(&[(&build_gradle, CHANGED)]);

    let events = event_notifications(&mut ws);
    assert_classpath_updated(&events, &project);
}

#[test]
fn test_classpath_update_for_eclipse() {
    let mut ws = workspace();
    ws.import_projects(&["eclipse/updatejar"]);
    wait_for_listener(&mut ws);
    let project = ws.dir.join("eclipse/updatejar");
    let classpath = project.join(".classpath");
    assert!(classpath.exists());
    replace_in(&classpath, "<classpathentry kind=\"lib\" path=\"lib/foo.jar\"/>", "");

    reset_connection(&mut ws);
    ws.files_changed(&[(&classpath, CHANGED)]);

    let events = event_notifications(&mut ws);
    assert_classpath_updated(&events, &project);
}

#[test]
fn test_classpath_update_for_invisble() {
    let mut ws = workspace();
    let project_folder = ws
        .dir
        .parent()
        .unwrap()
        .join(format!("dynamicLibDetection{}", std::process::id()));
    std::fs::create_dir_all(&project_folder).unwrap();
    copy_dir(&fixtures_dir().join("projects/eclipse/source-attachment/src"), &project_folder);
    let project_folder = canonical(&project_folder);
    ws.import_root_folder(&project_folder, Some("Test.java"));
    wait_for_listener(&mut ws);

    let lib = project_folder.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    for jar in ["foo.jar", "foo-sources.jar"] {
        std::fs::copy(fixtures_dir().join("projects/eclipse/source-attachment").join(jar), lib.join(jar)).unwrap();
    }
    reset_connection(&mut ws);
    ws.files_changed(&[(&lib.join("foo.jar"), CREATED)]);

    let events = event_notifications(&mut ws);
    assert_classpath_updated(&events, &project_folder);
}
