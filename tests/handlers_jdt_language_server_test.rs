//! Port of `org.eclipse.jdt.ls.core.internal.handlers.JDTLanguageServerTest`.
//!
//! The mocked `ClientPreferences` become client capabilities: only the
//! dynamic registrations `setDynamicCapabilities` enables are declared.
//! `verify(client, times(n)).registerCapability(any())` counts the
//! `client/registerCapability` requests the server sends.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::time::Duration;

const REFERENCES_CODE_LENS_ENABLED_KEY: &str = "java.referencesCodeLens.enabled";
const JAVA_FORMAT_ENABLED_KEY: &str = "java.format.enabled";
const SIGNATURE_HELP_ENABLED_KEY: &str = "java.signatureHelp.enabled";
const EXECUTE_COMMAND_ENABLED_KEY: &str = "java.executeCommand.enabled";
const JAVA_FORMAT_ON_TYPE_ENABLED_KEY: &str = "java.format.onType.enabled";
const AUTOBUILD_ENABLED_KEY: &str = "java.autobuild.enabled";

/// `setDynamicCapabilities(enable)`.
fn capabilities(enable: bool) -> Value {
    let d = json!({ "dynamicRegistration": enable });
    json!({
        "workspace": { "executeCommand": d },
        "textDocument": {
            "codeLens": d, "formatting": d, "rangeFormatting": d, "signatureHelp": d, "onTypeFormatting": d,
        },
    })
}

/// `new JDTLanguageServer(..)` + `connectClient(client)`: the server has
/// answered `initialize` but hasn't been told `initialized`.
fn connect(ws: &Workspace, caps: Value) -> LspClient {
    let mut c = LspClient::spawn_in(Some(&ws.dir.parent().unwrap().join("oracle-data")));
    c.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": url::Url::from_file_path(&ws.dir).unwrap().to_string(),
            "capabilities": caps,
            "initializationOptions": { "javaHome": java_home(), "settings": { "java": {} } },
        }),
    );
    c
}

/// `server.initialized(null)` and the initialization jobs.
fn initialized(c: &mut LspClient) {
    c.notify("initialized", json!({}));
    c.recv_until(Duration::from_secs(120), |m| m["method"] == "language/status" && m["params"]["type"] == "ServiceReady")
        .expect("server never reported ServiceReady");
}

fn did_change_configuration(c: &mut LspClient, settings: Value) {
    c.notify("workspace/didChangeConfiguration", json!({ "settings": settings }));
    c.settle(Duration::from_millis(2000), Duration::from_secs(60));
}

/// `reset(client)` / `verify(client, times(n)).<method>(any())`.
fn take_calls(c: &mut LspClient, method: &str) -> usize {
    c.settle(Duration::from_millis(1000), Duration::from_secs(60));
    c.take_server_requests(method).len()
}

#[test]
fn test_autobuilding() {
    // `ResourcesPlugin.getWorkspace().getDescription().isAutoBuilding()` is
    // observed through the build: with autobuilding off, a changed file on
    // disk isn't built (no markers are published for it).
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&Default::default());
    let uri = ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {}\n");
    ws.client();
    ws.wait_idle();
    ws.client().settle(Duration::from_millis(2000), Duration::from_secs(60));
    ws.client().take_notifications("textDocument/publishDiagnostics");

    // Autobuilding is on: a broken file gets its markers.
    ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E { int x = ; }\n");
    ws.client().settle(Duration::from_millis(2000), Duration::from_secs(60));
    let reports = ws.client().take_notifications("textDocument/publishDiagnostics");
    assert!(reports.iter().any(|r| r["params"]["uri"] == uri.as_str()), "Autobuilding is off");

    let mut map = serde_json::Map::new();
    map.insert(AUTOBUILD_ENABLED_KEY.into(), json!(false));
    ws.client().notify("workspace/didChangeConfiguration", json!({ "settings": map }));
    ws.wait_idle();
    ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E { int y = ; }\n");
    ws.client().settle(Duration::from_millis(2000), Duration::from_secs(60));
    let reports = ws.client().take_notifications("textDocument/publishDiagnostics");
    assert!(!reports.iter().any(|r| r["params"]["uri"] == uri.as_str()), "Autobuilding is on: {reports:#?}");
}

#[test]
fn test_register_dynamic_capabilities() {
    let ws = Workspace::new();
    let mut client = connect(&ws, capabilities(true));

    let mut map = serde_json::Map::new();
    map.insert(REFERENCES_CODE_LENS_ENABLED_KEY.into(), json!(true));
    map.insert(JAVA_FORMAT_ENABLED_KEY.into(), json!(true));
    map.insert(SIGNATURE_HELP_ENABLED_KEY.into(), json!(true));
    map.insert(EXECUTE_COMMAND_ENABLED_KEY.into(), json!(true));
    map.insert(JAVA_FORMAT_ON_TYPE_ENABLED_KEY.into(), json!(true));

    // If initialized jobs are not finished, won't register capabilities
    did_change_configuration(&mut client, Value::Object(map.clone()));
    assert_eq!(0, take_calls(&mut client, "client/registerCapability"));

    initialized(&mut client);
    did_change_configuration(&mut client, Value::Object(map.clone()));
    assert_eq!(6, take_calls(&mut client, "client/registerCapability"));

    //On 2nd call, no registration call should be emitted
    did_change_configuration(&mut client, Value::Object(map.clone()));
    assert_eq!(0, take_calls(&mut client, "client/registerCapability"));

    // unregister capabilities
    map.insert(REFERENCES_CODE_LENS_ENABLED_KEY.into(), json!(false));
    map.insert(JAVA_FORMAT_ENABLED_KEY.into(), json!(false));
    map.insert(SIGNATURE_HELP_ENABLED_KEY.into(), json!(false));
    map.insert(EXECUTE_COMMAND_ENABLED_KEY.into(), json!(false));
    map.insert(JAVA_FORMAT_ON_TYPE_ENABLED_KEY.into(), json!(false));
    did_change_configuration(&mut client, Value::Object(map.clone()));
    assert_eq!(6, take_calls(&mut client, "client/unregisterCapability"));

    //On 2nd call, no unregistration calls should be emitted
    did_change_configuration(&mut client, Value::Object(map));
    assert_eq!(0, take_calls(&mut client, "client/unregisterCapability"));
}

#[test]
fn test_no_dynamic_capabilities() {
    let ws = Workspace::new();
    let mut client = connect(&ws, capabilities(false));

    let mut map = serde_json::Map::new();
    map.insert(REFERENCES_CODE_LENS_ENABLED_KEY.into(), json!(true));
    map.insert(JAVA_FORMAT_ENABLED_KEY.into(), json!(true));
    map.insert(SIGNATURE_HELP_ENABLED_KEY.into(), json!(true));
    map.insert(EXECUTE_COMMAND_ENABLED_KEY.into(), json!(true));
    did_change_configuration(&mut client, Value::Object(map.clone()));
    assert_eq!(0, take_calls(&mut client, "client/registerCapability"));

    // unregister capabilities
    map.insert(REFERENCES_CODE_LENS_ENABLED_KEY.into(), json!(false));
    map.insert(JAVA_FORMAT_ENABLED_KEY.into(), json!(false));
    map.insert(SIGNATURE_HELP_ENABLED_KEY.into(), json!(false));
    map.insert(EXECUTE_COMMAND_ENABLED_KEY.into(), json!(false));
    did_change_configuration(&mut client, Value::Object(map));
    assert_eq!(0, take_calls(&mut client, "client/unregisterCapability"));
}
