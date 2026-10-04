//! Port of `org.eclipse.jdt.ls.core.internal.handlers.InitHandlerTest`.
//!
//! Upstream mocks `ClientPreferences` and the command handler; here the
//! equivalent client capabilities go into a real `initialize` request and
//! the test checks the `InitializeResult` and the `client/registerCapability`
//! requests the server sends afterwards.  jdt.ls contributes no static
//! commands, so the mocked `cmd1`…`cmd4` become the real command set.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::time::Duration;
use tower_lsp::lsp_types::Url;

/// The `workspace/executeCommand` commands jdt.ls 1.58 contributes
/// (`JDTDelegateCommandHandler`), all non-static.
const ALL_COMMANDS: &[&str] = &[
    "java.project.import",
    "java.project.changeImportedProjects",
    "java.navigate.openTypeHierarchy",
    "java.project.resolveStackTraceLocation",
    "java.edit.handlePasteEvent",
    "java.edit.stringFormatting",
    "java.project.getSettings",
    "java.project.resolveWorkspaceSymbol",
    "java.project.upgradeGradle",
    "java.project.createModuleInfo",
    "java.vm.getAllInstalls",
    "java.edit.organizeImports",
    "java.project.refreshDiagnostics",
    "java.project.removeFromSourcePath",
    "java.project.listSourcePaths",
    "java.project.updateSettings",
    "java.project.getAll",
    "java.reloadBundles",
    "java.project.isTestFile",
    "java.project.resolveText",
    "java.project.getClasspaths",
    "java.navigate.resolveTypeHierarchy",
    "java.getTroubleshootingInfo",
    "java.edit.smartSemicolonDetection",
    "java.project.updateSourceAttachment",
    "java.project.updateClassPaths",
    "java.decompile",
    "java.protobuf.generateSources",
    "java.project.resolveSourceAttachment",
    "java.project.updateJdk",
    "java.project.addToSourcePath",
    "java.completion.onDidSelect",
];

/// `initialize(dynamicRegistration)`: `didChangeConfiguration` and
/// `executeCommand` dynamic registration, an empty `textDocument`.
fn capabilities(dynamic_registration: bool) -> Value {
    json!({
        "workspace": {
            "didChangeConfiguration": { "dynamicRegistration": dynamic_registration },
            "executeCommand": { "dynamicRegistration": dynamic_registration },
        },
        "textDocument": {},
    })
}

fn initialize(ws: &mut Workspace) -> Value {
    ws.client();
    ws.initialize_result.clone()
}

/// `client/registerCapability` registrations received (waiting for the server
/// to go quiet first).
fn registrations(ws: &mut Workspace) -> Vec<Value> {
    let c = ws.client();
    c.settle(Duration::from_millis(3000), Duration::from_secs(60));
    c.server_requests
        .iter()
        .filter(|r| r["method"] == "client/registerCapability")
        .flat_map(|r| r["params"]["registrations"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn watcher_patterns(ws: &mut Workspace) -> Vec<Value> {
    let regs = registrations(ws);
    let watched: Vec<&Value> = regs.iter().filter(|r| r["method"] == "workspace/didChangeWatchedFiles").collect();
    assert_eq!(1, watched.len(), "{regs:#?}");
    watched[0]["registerOptions"]["watchers"].as_array().cloned().unwrap()
}

fn pattern_of(w: &Value) -> String {
    match &w["globPattern"] {
        Value::String(s) => s.clone(),
        p => p["pattern"].as_str().unwrap().to_owned(),
    }
}

#[test]
fn test_execute_command_provider() {
    let mut ws = Workspace::new();
    ws.capabilities = capabilities(false);
    let result = initialize(&mut ws);
    let commands = result["capabilities"]["executeCommandProvider"]["commands"].as_array().unwrap();
    assert!(!commands.is_empty());
}

#[test]
fn test_server_info() {
    let mut ws = Workspace::new();
    ws.capabilities = capabilities(false);
    let result = initialize(&mut ws);

    assert!(result["serverInfo"].is_object());
    assert!(result["serverInfo"]["version"].is_string());
    assert!(result["serverInfo"]["name"].is_string());
}

#[test]
fn test_execute_command_provider_dynamic_registration() {
    let mut ws = Workspace::new();
    ws.capabilities = capabilities(true);
    let result = initialize(&mut ws);
    assert!(result["capabilities"].get("executeCommandProvider").is_none(), "{result}");
}

/// With dynamic registration the static commands are listed in the result
/// (jdt.ls has none) and the non-static ones are registered after
/// `initialized`.
#[test]
fn test_static_command_with_dynamic_registration() {
    let mut ws = Workspace::new();
    ws.capabilities = capabilities(true);
    let result = initialize(&mut ws);

    assert!(result["capabilities"].get("executeCommandProvider").is_none(), "{result}");
    let regs = registrations(&mut ws);
    let commands: Vec<&Value> = regs.iter().filter(|r| r["method"] == "workspace/executeCommand").collect();
    assert_eq!(1, commands.len(), "{regs:#?}");
    let registered = commands[0]["registerOptions"]["commands"].as_array().unwrap();
    assert_eq!(ALL_COMMANDS.iter().map(|c| json!(c)).collect::<Vec<_>>(), *registered);
}

#[test]
fn test_static_command_without_dynamic_registration() {
    let mut ws = Workspace::new();
    ws.capabilities = capabilities(false);
    let result = initialize(&mut ws);

    let command_provider = &result["capabilities"]["executeCommandProvider"];
    assert!(command_provider.is_object());
    let commands = command_provider["commands"].as_array().unwrap();
    assert_eq!(ALL_COMMANDS.iter().map(|c| json!(c)).collect::<Vec<_>>(), *commands);
    let regs = registrations(&mut ws);
    assert!(regs.iter().all(|r| r["method"] != "workspace/executeCommand"), "{regs:#?}");
}

#[test]
fn test_will_save_and_will_save_wait_until_capabilities() {
    let mut ws = Workspace::new();
    let mut caps = capabilities(true);
    caps["textDocument"]["synchronization"] = json!({ "willSave": true, "willSaveWaitUntil": true });
    ws.capabilities = caps;
    let result = initialize(&mut ws);
    let o = &result["capabilities"]["textDocumentSync"];
    assert!(o.is_object(), "{o}");
    assert_eq!(json!(true), o["willSave"]);
    assert_eq!(json!(true), o["willSaveWaitUntil"]);
}

#[test]
fn test_register_delayed_capability() {
    let mut ws = Workspace::new();
    let d = json!({ "dynamicRegistration": true });
    ws.capabilities = json!({
        "workspace": { "symbol": d },
        "textDocument": {
            "documentSymbol": d, "codeAction": d, "definition": d, "hover": d, "references": d,
            "documentHighlight": d, "foldingRange": d, "completion": d,
        },
    });
    let result = initialize(&mut ws);
    assert!(result["capabilities"].get("documentSymbolProvider").is_none(), "{result}");
    let regs = registrations(&mut ws);
    assert_eq!(9, regs.len(), "{regs:#?}");
    let methods: Vec<&str> = regs.iter().map(|r| r["method"].as_str().unwrap()).collect();
    assert_eq!(
        vec![
            "workspace/symbol",
            "textDocument/documentSymbol",
            "textDocument/definition",
            "textDocument/hover",
            "textDocument/references",
            "textDocument/documentHighlight",
            "textDocument/completion",
            "textDocument/codeAction",
            "textDocument/foldingRange",
        ],
        methods
    );
    assert_eq!(json!({ "resolveProvider": true, "triggerCharacters": [".", "@", "#", "*", " "] }), regs[6]["registerOptions"]);
    assert_eq!(json!({ "codeActionKinds": [], "resolveProvider": false }), regs[7]["registerOptions"]);
}

#[test]
#[ignore = "configures Eclipse IVMInstall JVMs from a fake JDK and listens for default-VM change events; no LSP-visible equivalent"]
fn test_configure_jvms() {}

#[test]
#[ignore = "pure Preferences parsing; ported as the unit test features::preferences::tests::test_maven_settings"]
fn test_maven_settings() {}

#[test]
#[ignore = "pure Preferences parsing; ported as the unit test server::tests::test_java_import_exclusions"]
fn test_java_import_exclusions() {}

#[test]
fn test_watchers() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut", "gradle/simple-gradle"]);
    ws.new_empty_project(&Default::default());
    let watchers = watcher_patterns(&mut ws);
    // 9 basic + 3 project roots
    assert_eq!(12, watchers.len(), "Unexpected watchers:\n{watchers:#?}");
    let project_watchers = &watchers[9..12];
    assert!(pattern_of(&project_watchers[0]).ends_with("TestProject"));
    assert_eq!(json!(4), project_watchers[0]["kind"], "WatchKind.Delete");
    assert!(pattern_of(&project_watchers[1]).ends_with("salut"));
    assert!(pattern_of(&project_watchers[2]).ends_with("simple-gradle"));
    let base = |name: &str| format!("file:{}/", ws.project_root(name).parent().unwrap().display());
    assert_eq!(json!({ "baseUri": base("TestProject"), "pattern": "TestProject" }), project_watchers[0]["globPattern"]);
    assert_eq!(json!({ "baseUri": base("salut"), "pattern": "salut" }), project_watchers[1]["globPattern"]);

    let mut basic: Vec<String> = watchers[0..9].iter().map(pattern_of).collect();
    basic.sort();
    assert_eq!("**/*.gradle", basic[0]);
    assert_eq!("**/*.gradle.kts", basic[1]);
    assert_eq!("**/*.java", basic[2]);
    assert_eq!("**/.classpath", basic[3]);
    assert_eq!("**/.project", basic[4]);
    assert_eq!("**/.settings/*.prefs", basic[5]);
    assert_eq!("**/gradle.properties", basic[6]);
    assert_eq!("**/pom.xml", basic[7]);
    assert_eq!("**/src/**", basic[8]);

    // A resource change doesn't re-register the (unchanged) watchers.
    let salut = ws.project_root("salut");
    let resource = salut.join("src/main/resources/test.properties");
    std::fs::write(&resource, "test=test\n").unwrap();
    ws.notify_file_changed(&resource, 2);
    let regs = registrations(&mut ws);
    assert_eq!(1, regs.iter().filter(|r| r["method"] == "workspace/didChangeWatchedFiles").count(), "{regs:#?}");
}

#[test]
fn test_workspace_will_rename_files() {
    let mut ws = Workspace::new();
    ws.capabilities = json!({ "workspace": { "fileOperations": { "willRename": true } }, "textDocument": {} });
    let result = initialize(&mut ws);
    let file_op_srv_cap = &result["capabilities"]["workspace"]["fileOperations"];
    assert!(file_op_srv_cap.is_object(), "{result}");
    let file_op_ops = &file_op_srv_cap["willRename"];
    assert!(file_op_ops.is_object());
    assert_eq!("file", file_op_ops["filters"][0]["scheme"]);
    assert_eq!("file", file_op_ops["filters"][0]["pattern"]["matches"]);
    assert_eq!("**/*.java", file_op_ops["filters"][0]["pattern"]["glob"]);
    assert_eq!(
        json!({ "filters": [
            { "pattern": { "glob": "**/*.java", "matches": "file" }, "scheme": "file" },
            { "pattern": { "glob": "**", "matches": "folder" }, "scheme": "file" },
        ] }),
        *file_op_ops
    );
}

// https://github.com/redhat-developer/vscode-java/issues/2429
#[test]
fn test_settings_watchers() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": {
        "format": { "settings": { "url": "file:c:invalid" } },
        "settings": { "url": "../../formatter/settings.prefs" },
    } });
    let watchers = watcher_patterns(&mut ws);
    assert_eq!(10, watchers.len(), "Unexpected watchers:\n{watchers:#?}");
    let formatter = ws.dir.parent().unwrap().parent().unwrap().join("formatter");
    assert_eq!(json!({ "baseUri": format!("file:{}", formatter.display()), "pattern": "settings.prefs" }), watchers[9]["globPattern"]);
}

#[test]
#[ignore = "symbolic-link root of a Gradle project; the reference server cannot import Gradle 8.5 projects on this JDK (Gradle needs network/JDK <= 21)"]
fn test_init_on_symbolic_link_folder() {}

/// Without `workspaceEdit.resourceOperations` the client can't rename files,
/// so renaming a public type edits the source only.
#[test]
fn test_missing_resource_operations() {
    let mut ws = Workspace::new();
    ws.capabilities = json!({
        "workspace": { "workspaceEdit": {} },
        "textDocument": { "rename": { "prepareSupport": true } },
    });
    let root = ws.new_empty_project(&Default::default());
    let uri = ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n}\n");
    let edit = ws.request(
        "textDocument/rename",
        json!({ "textDocument": { "uri": uri }, "position": pos(1, 14), "newName": "B" }),
    );
    assert!(edit.get("documentChanges").is_none(), "{edit}");
    let changes = edit["changes"].as_object().unwrap();
    assert_eq!(vec![&uri], changes.keys().collect::<Vec<_>>());
    let _ = Url::parse(&uri).unwrap();
}

// https://github.com/eclipse-jdtls/eclipse.jdt.ls/issues/3222
#[test]
fn test_files_associations() {
    let mut ws = Workspace::new();
    // `java.associations` (`files.associations` entries mapped to `java`).
    ws.settings = json!({ "java": { "associations": { "*.maxj": "java" } } });
    let root = ws.new_empty_project(&Default::default());
    let content = "package test1;\npublic class Test {\n}\n";
    let uri = ws.create_cu(&root, "src", "test1", "Test.maxj", content);
    ws.open_with(&uri, content);
    let symbols = ws.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
    let names: Vec<&str> = symbols.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(vec!["test1", "Test"], names);

    // Without the association `Test.maxj` isn't a Java file.
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&Default::default());
    let uri = ws.create_cu(&root, "src", "test1", "Test.maxj", content);
    ws.open_with(&uri, content);
    let symbols = ws.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
    assert_eq!(json!([]), symbols);
}
