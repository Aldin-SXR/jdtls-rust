//! Port of `org.eclipse.jdt.ls.core.internal.handlers.NavigateToDefinitionHandlerTest`.

mod common;
use common::jdtls::{copy_dir, fixtures_dir, Workspace};
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    ws
}

fn definition(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Vec<Value> {
    let result = ws.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    assert!(!result.is_null(), "definition must not return null");
    result.as_array().cloned().unwrap_or_default()
}

fn test_class(ws: &mut Workspace, class_name: &str, line: u32, column: u32) {
    let uri = ws.class_file_uri("salut", class_name);
    let definitions = definition(ws, &uri, line, column);
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert!(definitions[0]["range"]["start"]["line"].as_i64().unwrap() >= 0);
}

fn start(l: &Value) -> (u64, u64) {
    (l["range"]["start"]["line"].as_u64().unwrap(), l["range"]["start"]["character"].as_u64().unwrap())
}

#[test]
fn test_get_empty_definition() {
    let mut ws = setup();
    let definitions = definition(&mut ws, "/foo/bar", 1, 1);
    assert_eq!(0, definitions.len());
}

#[test]
fn test_attached_source() {
    let mut ws = setup();
    test_class(&mut ws, "org.apache.commons.lang3.StringUtils", 20, 26);
}

#[test]
fn test_no_class_content_support() {
    let mut ws = setup();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": false } });
    let uri = ws.class_file_uri("salut", "org.apache.commons.lang3.StringUtils");
    let definitions = definition(&mut ws, &uri, 20, 26);
    assert_eq!(0, definitions.len());
}

#[test]
#[ignore = "expects the disassembled stub of rtstubs.jar's javax.tools.Tool (fake JDK without sources); the running JDK's javax.tools.Tool has attached source (lib/src.zip) where (6,57) is in the license header"]
fn test_disassembled_source() {
    let mut ws = setup();
    test_class(&mut ws, "javax.tools.Tool", 6, 57);
}

#[test]
#[ignore = "expects the disassembled stub of rtstubs.jar's javax.tools.Tool (fake JDK without sources); the running JDK's javax.tools.Tool has attached source (lib/src.zip) with a different layout"]
fn test_source_version() {
    let mut ws = setup();
    let class_name = "javax.tools.Tool";
    let uri = ws.class_file_uri("salut", class_name);
    let definitions = definition(&mut ws, &uri, 11, 12);
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert_eq!((3, 12), start(&definitions[0]));
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1813
#[test]
fn test_method_in_anonymous_class() {
    let mut ws = setup();
    let class_name = "org.sample.App";
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.class_file_uri("hello", class_name);
    let definitions = definition(&mut ws, &uri, 12, 28);
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert_eq!((3, 18), start(&definitions[0]));
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1813
#[test]
fn test_method_in_anonymous_class2() {
    let mut ws = setup();
    let class_name = "org.sample.App2";
    ws.import_projects(&["eclipse/java11"]);
    let uri = ws.class_file_uri("java11", class_name);
    let definitions = definition(&mut ws, &uri, 10, 24);
    assert_eq!(1, definitions.len(), "No definition found for {class_name}");
    assert!(definitions[0]["uri"].is_string());
    assert_eq!((7, 10), start(&definitions[0]));
}

#[test]
fn test_jdk_classes() {
    let mut ws = setup();
    // linkFilesToDefaultProject("singlefile/Single.java")
    let file = ws.dir.join("singlefile/Single.java");
    copy_dir(&fixtures_dir().join("projects/singlefile/Single.java"), &file);
    let single = tower_lsp::lsp_types::Url::from_file_path(&file).unwrap().to_string();
    ws.open(&single);
    let uri = ws.class_file_uri("jdt.ls-java-project", "Single");
    definition(&mut ws, &uri, 1, 31);
    test_class(&mut ws, "org.apache.commons.lang3.stringutils", 145, 30);
}

// this test should pass when starting with -javaagent:<lombok_jar>
// https://github.com/redhat-developer/vscode-java/issues/2805
#[test]
fn test_lombok() {
    let mut ws = setup();
    ws.import_projects(&["maven/mavenlombok"]);
    let main = ws.class_uri("mavenlombok", "org.sample.Main");
    let test = ws.class_uri("mavenlombok", "org.sample.Test");
    let has_errors = [main.clone(), test]
        .iter()
        .any(|u| ws.diagnostics(u).iter().any(|d| d["severity"] == 1));
    if has_errors {
        // there isn't the lombok agent
        return;
    }
    let uri = ws.class_file_uri("mavenlombok", "org.sample.Main");
    let locations = definition(&mut ws, &uri, 5, 20);
    assert_eq!(1, locations.len());
    assert_eq!(6, locations[0]["range"]["start"]["line"]);
    assert_eq!(6, locations[0]["range"]["end"]["line"]);
    assert_eq!(19, locations[0]["range"]["start"]["character"]);
    assert_eq!(23, locations[0]["range"]["end"]["character"]);
    assert!(locations[0]["uri"].as_str().unwrap().ends_with("org/sample/Test.java"));
}

#[test]
#[ignore = "Kotlin support (java.jdt.ls.kotlinSupport / Gradle Kotlin projects) is not implemented"]
fn test_kotlin() {
    let mut ws = setup();
    ws.settings = json!({ "java": { "jdt": { "ls": { "kotlinSupport": { "enabled": true } } } } });
    ws.import_projects(&["gradle/duallang"]);
    let uri = ws.class_file_uri("duallang", "com.example.MessageApp");
    let locations = definition(&mut ws, &uri, 5, 18);
    assert_eq!(1, locations.len());
    assert_eq!(2, locations[0]["range"]["start"]["line"]);
    assert_eq!(2, locations[0]["range"]["end"]["line"]);
    assert_eq!(6, locations[0]["range"]["start"]["character"]);
    assert_eq!(6, locations[0]["range"]["end"]["character"]);
    assert!(locations[0]["uri"].as_str().unwrap().ends_with("MessageService.kt"));
}

#[test]
fn test_break_continue() {
    let mut ws = setup();
    let uri = ws.class_file_uri("salut", "org.sample.TestBreakContinue");
    // continue
    let definitions = definition(&mut ws, &uri, 11, 5);
    assert_eq!(1, definitions.len());
    assert_eq!((8, 3), start(&definitions[0]));
    // outer continue
    let definitions = definition(&mut ws, &uri, 14, 5);
    assert_eq!(1, definitions.len());
    assert_eq!((6, 2), start(&definitions[0]));
    // break
    let definitions = definition(&mut ws, &uri, 17, 5);
    assert_eq!(1, definitions.len());
    assert_eq!((8, 3), start(&definitions[0]));
    // outer break
    let definitions = definition(&mut ws, &uri, 20, 5);
    assert_eq!(1, definitions.len());
    assert_eq!((6, 2), start(&definitions[0]));
}
