//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CodeLensHandlerTest`.
//!
//! The mocked `PreferenceManager` answers become `initializationOptions.settings`
//! (`java.referencesCodeLens.enabled`, `java.implementationCodeLens`).

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

const REFERENCES_TYPE: &str = "references";
const IMPLEMENTATION_TYPE: &str = "implementations";

/// `setup()`: `importProjects(List.of("eclipse/hello", "eclipse/java21"))`.
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello", "eclipse/java21"]);
    ws
}

fn file_uri(ws: &Workspace, project: &str, file: &str) -> String {
    Url::from_file_path(ws.project_root(project).join(file)).unwrap().to_string()
}

/// `createCodeLensSymbolsRequest(file)` + `handler.getCodeLensSymbols(uri)`.
fn get_code_lens_symbols(ws: &mut Workspace, uri: &str) -> Vec<Value> {
    let result = ws.request("textDocument/codeLens", json!({ "textDocument": { "uri": uri } }));
    result.as_array().cloned().unwrap_or_default()
}

/// `CODELENS_REFERENCES_TEMPLATE` / `CODELENS_IMPLEMENTATIONS_TEMPLATE`.
fn code_lens_request(uri: &str, line: u32, start: u32, end: u32, kind: &str) -> Value {
    json!({
        "range": {
            "start": { "line": line, "character": start },
            "end": { "line": line, "character": end }
        },
        "data": [uri, { "line": line, "character": start }, kind]
    })
}

fn resolve(ws: &mut Workspace, lens: &Value) -> Value {
    ws.request("codeLens/resolve", lens.clone())
}

/// `Lsp4jAssertions.assertRange(line, start, end, range)`.
fn assert_range(line: u64, start: u64, end: u64, range: &Value) {
    assert_eq!(range["start"]["line"], line, "start line of {range}");
    assert_eq!(range["start"]["character"], start, "start character of {range}");
    assert_eq!(range["end"]["line"], line, "end line of {range}");
    assert_eq!(range["end"]["character"], end, "end character of {range}");
}

fn set_pref(ws: &mut Workspace, key: &str, value: Value) {
    let mut cur = &mut ws.settings;
    let parts: Vec<&str> = key.split('.').collect();
    for p in &parts[..parts.len() - 1] {
        if cur.get(*p).is_none() {
            cur[*p] = json!({});
        }
        cur = &mut cur[*p];
    }
    cur[parts[parts.len() - 1]] = value;
}

#[test]
fn test_get_code_lens_symbols() {
    let mut ws = setup();
    let uri = file_uri(&ws, "hello", "src/java/Foo.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);

    assert_eq!(result.len(), 3, "Found {result:?}");
    // CodeLens on main method
    assert_range(7, 20, 24, &result[0]["range"]);
    // CodeLens on foo method
    assert_range(15, 13, 16, &result[1]["range"]);
    // CodeLens on Foo type
    assert_range(5, 13, 16, &result[2]["range"]);
}

#[test]
#[ignore = "needs jdt:// classfile support (code lenses of java.lang.Runnable's class file)"]
fn test_get_code_lens_symbols_for_class() {
    let mut ws = setup();
    set_pref(&mut ws, "java.implementationCodeLens", json!("types"));
    let uri = "jdt://contents/java.base/java.lang/Runnable.class";
    let lenses = get_code_lens_symbols(&mut ws, uri);
    assert_eq!(lenses.len(), 2, "Found {lenses:?}");
    let data = lenses[0]["data"].as_array().unwrap();
    assert!(data.contains(&json!(REFERENCES_TYPE)), "Unexpected type {data:?}");
    let data = lenses[1]["data"].as_array().unwrap();
    assert!(data.contains(&json!(IMPLEMENTATION_TYPE)), "Unexpected type {data:?}");
}

#[test]
fn test_get_code_lense_boundaries() {
    let mut ws = setup();
    // `getCodeLensSymbols(null)`: LSP has no null document; the closest is
    // a document the server does not know.
    let result = get_code_lens_symbols(&mut ws, "file:///");
    assert_eq!(result.len(), 0);

    let uri = file_uri(&ws, "hello", "src/java/Missing.java");
    let result = get_code_lens_symbols(&mut ws, &uri);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_disable_code_lens_symbols() {
    let mut ws = setup();
    set_pref(&mut ws, "java.referencesCodeLens.enabled", json!(false));
    let uri = file_uri(&ws, "hello", "src/java/IFoo.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_enable_implementations_code_lens_symbols() {
    let mut ws = setup();
    set_pref(&mut ws, "java.implementationCodeLens", json!("types"));
    let uri = file_uri(&ws, "hello", "src/java/IFoo.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);
    assert_eq!(result.len(), 2);
    let lens = &result[1];
    let ty = lens["data"][2].as_str().unwrap();
    assert_eq!(ty, "implementations");
}

#[test]
fn test_enable_implementations_code_lens_symbols_for_base_types() {
    let mut ws = setup();
    set_pref(&mut ws, "java.implementationCodeLens", json!("all"));
    let uri = file_uri(&ws, "hello", "src/java/Foo.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);
    assert_eq!(result.len(), 6);
    let implementations = result.iter().filter(|cl| cl["data"][2] == "implementations").count();
    assert_eq!(implementations, 3);
}

#[test]
fn test_disable_implementations_code_lens_symbols() {
    let mut ws = setup();
    // The upstream test resets the mock to preferences that only disable the
    // references lens (the implementations preference is not kept).
    set_pref(&mut ws, "java.referencesCodeLens.enabled", json!(false));
    let uri = file_uri(&ws, "hello", "src/java/IFoo.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_resolve_implementations_code_lens() {
    let mut ws = setup();
    let source = file_uri(&ws, "hello", "src/java/IFoo.java");
    let lens = code_lens_request(&source, 5, 17, 21, IMPLEMENTATION_TYPE);
    assert_range(5, 17, 21, &lens["range"]);

    let result = resolve(&mut ws, &lens);
    assert!(!result.is_null());

    // Check if command found
    let command = &result["command"];
    assert!(!command.is_null());
    assert_eq!(command["title"], "2 implementations");
    assert_eq!(command["command"], "java.show.implementations");

    // Check codelens args
    let args = command["arguments"].as_array().unwrap();
    assert_eq!(args.len(), 3);

    // Check we point to the Bar class
    let source_uri = args[0].as_str().unwrap();
    assert!(source_uri.ends_with("IFoo.java"));

    // CodeLens position
    assert_eq!(args[1]["line"], 5);
    assert_eq!(args[1]["character"], 17);

    // Reference location
    let locations = args[2].as_array().unwrap();
    assert_eq!(locations.len(), 2);
    let loc = locations.iter().find(|l| l["uri"].as_str().unwrap().contains("Foo2")).unwrap();
    assert!(loc["uri"].as_str().unwrap().ends_with("src/java/Foo2.java"));
    assert_range(5, 13, 17, &loc["range"]);
}

#[test]
fn test_resolve_implementations_interface_method_code_lens() {
    let mut ws = setup();
    let source = file_uri(&ws, "hello", "src/java/ITest.java");
    let lens = code_lens_request(&source, 4, 16, 28, IMPLEMENTATION_TYPE);
    assert_range(4, 16, 28, &lens["range"]);

    let result = resolve(&mut ws, &lens);
    assert!(!result.is_null());

    // Check if command found
    let command = &result["command"];
    assert!(!command.is_null());
    assert_eq!(command["title"], "2 implementations");
    assert_eq!(command["command"], "java.show.implementations");

    // Check codelens args
    let args = command["arguments"].as_array().unwrap();
    assert_eq!(args.len(), 3);

    // Check we point to the ITest interface
    let source_uri = args[0].as_str().unwrap();
    assert!(source_uri.ends_with("ITest.java"));

    // CodeLens position
    assert_eq!(args[1]["line"], 4);
    assert_eq!(args[1]["character"], 16);

    // Reference location (just checking implementation in Test.java)
    let locations = args[2].as_array().unwrap();
    assert_eq!(locations.len(), 2);
    let loc = locations.iter().find(|l| l["uri"].as_str().unwrap().contains("Test")).unwrap();
    assert!(loc["uri"].as_str().unwrap().ends_with("src/java/Test.java"));
    assert_range(5, 13, 23, &loc["range"]);
}

#[test]
fn test_resolve_implementations_abstract_method_code_lens() {
    let mut ws = setup();
    let source = file_uri(&ws, "hello", "src/java/Test.java");
    let lens = code_lens_request(&source, 7, 25, 45, IMPLEMENTATION_TYPE);
    assert_range(7, 25, 45, &lens["range"]);

    let result = resolve(&mut ws, &lens);
    assert!(!result.is_null());

    // Check if command found
    let command = &result["command"];
    assert!(!command.is_null());
    assert_eq!(command["title"], "1 implementation");
    assert_eq!(command["command"], "java.show.implementations");

    // Check codelens args
    let args = command["arguments"].as_array().unwrap();
    assert_eq!(args.len(), 3);

    // Check we point to the Test class
    let source_uri = args[0].as_str().unwrap();
    assert!(source_uri.ends_with("Test.java"));

    // CodeLens position
    assert_eq!(args[1]["line"], 7);
    assert_eq!(args[1]["character"], 25);

    // Reference location (just checking implementation in Test.java)
    let locations = args[2].as_array().unwrap();
    assert_eq!(locations.len(), 1);
    let loc = locations.iter().find(|l| l["uri"].as_str().unwrap().contains("Ext")).unwrap();
    assert!(loc["uri"].as_str().unwrap().ends_with("src/java/Ext.java"));
    assert_range(8, 13, 31, &loc["range"]);
}

#[test]
fn test_resolve_code_lense() {
    let mut ws = setup();
    let source = "src/java/Foo.java";
    let uri = file_uri(&ws, "hello", source);
    let lens = code_lens_request(&uri, 5, 13, 16, REFERENCES_TYPE);
    assert_range(5, 13, 16, &lens["range"]);

    let result = resolve(&mut ws, &lens);
    assert!(!result.is_null());

    // Check if command found
    let command = &result["command"];
    assert!(!command.is_null());
    assert_eq!(command["title"], "1 reference");
    assert_eq!(command["command"], "java.show.references");

    // Check codelens args
    let args = command["arguments"].as_array().unwrap();
    assert_eq!(args.len(), 3);

    // Check we point to the Bar class
    let source_uri = args[0].as_str().unwrap();
    assert!(source_uri.ends_with(source));

    // CodeLens position
    assert_eq!(args[1]["line"], 5);
    assert_eq!(args[1]["character"], 13);

    // Reference location
    let locations = args[2].as_array().unwrap();
    assert_eq!(locations.len(), 1);
    let loc = &locations[0];
    assert!(loc["uri"].as_str().unwrap().ends_with("src/java/Bar.java"));
    assert_range(5, 25, 28, &loc["range"]);
}

#[test]
fn test_resolve_code_lense_boundaries() {
    let mut ws = setup();
    // `handler.resolve(null)` returns null; LSP requires a lens, so there is
    // no wire equivalent of the null argument.

    let uri = file_uri(&ws, "hello", "src/java/Missing.java");
    let lens = code_lens_request(&uri, 5, 13, 16, REFERENCES_TYPE);
    let result = resolve(&mut ws, &lens);
    // assertSame(lens, result): the same lens comes back, with a command
    assert_eq!(result["range"], lens["range"]);
    assert_eq!(result["data"], lens["data"]);
    assert!(!result["command"].is_null());
}

#[test]
fn test_ignore_lombok_code_lens_symbols() {
    let mut ws = setup();
    let uri = file_uri(&ws, "hello", "src/java/Bar.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);

    assert_eq!(result.len(), 4, "Found {result:?}");
    // CodeLens on constructor
    assert_range(7, 11, 14, &result[0]["range"]);
    // CodeLens on somethingFromJPAModelGen
    assert_range(16, 16, 40, &result[1]["range"]);
    // CodeLens on foo
    assert_range(22, 16, 19, &result[2]["range"]);
    // CodeLens on Bar type
    assert_range(5, 13, 16, &result[3]["range"]);
}

#[test]
fn test_no_reference_code_lens_for_unnamed_classes() {
    let mut ws = setup();
    let uri = file_uri(&ws, "java21", "src/main/java/UnnamedWithString.java");
    assert!(!uri.is_empty());
    let result = get_code_lens_symbols(&mut ws, &uri);

    assert_eq!(result.len(), 2, "Found {result:?}");
    // CodeLens on foo()
    assert_range(0, 7, 10, &result[0]["range"]);
    // CodeLens on main()
    assert_range(4, 5, 9, &result[1]["range"]);

    let cl = resolve(&mut ws, &code_lens_request(&uri, 0, 7, 10, REFERENCES_TYPE));
    assert!(!cl["command"].is_null());
    assert_eq!(cl["command"]["title"], "1 reference");

    let cl = resolve(&mut ws, &code_lens_request(&uri, 4, 5, 9, REFERENCES_TYPE));
    assert!(!cl["command"].is_null());
    assert_eq!(cl["command"]["title"], "0 references");
}
