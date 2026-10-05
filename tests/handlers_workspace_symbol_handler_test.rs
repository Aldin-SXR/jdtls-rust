//! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceSymbolHandlerTest`.
//!
//! The mocked preferences become client settings: class file content support
//! (`AbstractProjectsManagerBasedTest` defaults it to `true`) is
//! `extendedClientCapabilities.classFileContentsSupport`, symbol tag support
//! is `textDocument.documentSymbol.tagSupport`, and
//! `includeSourceMethodDeclarations` is `java.symbols.includeSourceMethodDeclarations`.
//! `WorkspaceSymbolHandler.search(query, maxResults, projectName, sourceOnly)`
//! is the `java/searchSymbols` request.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};
use std::collections::HashSet;

const INTERFACE: u64 = 11;
const METHOD: u64 = 6;
const DEPRECATED_TAG: u64 = 1;

/// `setup()`: `importProjects("eclipse/hello")`.
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    set_class_file_content_support(&mut ws, true);
    ws
}

fn set_class_file_content_support(ws: &mut Workspace, supported: bool) {
    ws.init_options["extendedClientCapabilities"] = json!({ "classFileContentsSupport": supported });
}

/// `WorkspaceSymbolHandler.search(query, monitor)`.
fn search(ws: &mut Workspace, query: Option<&str>) -> Vec<Value> {
    search_with(ws, query, 0, None, false)
}

/// `WorkspaceSymbolHandler.search(query, maxResults, projectName, sourceOnly, monitor)`.
fn search_with(ws: &mut Workspace, query: Option<&str>, max_results: u64, project_name: Option<&str>, source_only: bool) -> Vec<Value> {
    let result = ws.request(
        "java/searchSymbols",
        json!({ "query": query, "maxResults": max_results, "projectName": project_name, "sourceOnly": source_only }),
    );
    assert!(result.is_array(), "expected a list, got {result}");
    result.as_array().cloned().unwrap()
}

/// `JDTUtils.newRange()`.
fn default_range() -> Value {
    json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } })
}

fn name(s: &Value) -> &str {
    s["name"].as_str().unwrap_or("")
}

fn container(s: &Value) -> &str {
    s["containerName"].as_str().unwrap_or("")
}

#[test]
fn test_search_with_empty_results() {
    let mut ws = setup();
    let results = search(&mut ws, None);
    assert_eq!(results.len(), 0);

    let results = search(&mut ws, Some("  "));
    assert_eq!(results.len(), 0);

    let results = search(&mut ws, Some("Abracadabra"));
    assert_eq!(results.len(), 0);
}

#[test]
fn test_workspace_search_no_class_content_support() {
    let mut ws = setup();
    set_class_file_content_support(&mut ws, false);
    //No classes from binaries can be found
    let results = search(&mut ws, Some("Array"));
    assert_eq!(results.len(), 0, "Unexpected results");

    //... but workspace classes can still be found
    workspace_search_on_file_in_workspace(&mut ws);
}

#[test]
fn test_workspace_search() {
    let mut ws = setup();
    ws.use_upstream_test_jdk("hello");
    let query = "Array";
    let results = search(&mut ws, Some(query));
    assert_eq!(results.len(), 11, "Unexpected results");
    for symbol in &results {
        assert!(!symbol["kind"].is_null(), "Kind is missing");
        assert!(!symbol["containerName"].is_null(), "ContainerName is missing");
        assert!(name(symbol).starts_with(query));
        let location = &symbol["location"];
        assert_eq!(location["range"], default_range());
        //No class in the workspace project starts with Array, so everything comes from the JDK
        let uri = location["uri"].as_str().unwrap();
        assert!(uri.starts_with("jdt://"), "Unexpected uri {uri}");
    }
}

#[test]
fn test_workspace_search_on_file_in_workspace() {
    let mut ws = setup();
    workspace_search_on_file_in_workspace(&mut ws);
}

fn workspace_search_on_file_in_workspace(ws: &mut Workspace) {
    let query = "Baz";
    let results = search(ws, Some(query));
    assert_eq!(results.len(), 2, "Unexpected results");
    for symbol in &results {
        assert!(!symbol["kind"].is_null(), "Kind is missing");
        assert!(!symbol["containerName"].is_null(), "ContainerName is missing");
        assert!(name(symbol).starts_with(query));
        let location = &symbol["location"];
        assert_ne!(location["range"], default_range(), "Range should not equal the default range");
        let uri = location["uri"].as_str().unwrap();
        assert!(uri.starts_with("file://"), "Unexpected uri {uri}");
    }
}

#[test]
fn test_project_search() {
    let mut ws = setup();
    let query = "IFoo";
    let results = search(&mut ws, Some(query));
    assert_eq!(results.len(), 2, "Found {} results", results.len());
    assert!(results.iter().any(|s| container(s) == "org.sample"));
    assert!(results.iter().any(|s| container(s) == "java"));
    let symbol = &results[0];
    assert_eq!(symbol["kind"], INTERFACE);
    assert_eq!(name(symbol), query);
    let location = &symbol["location"];
    assert_ne!(location["range"], default_range(), "Range should not equal the default range");
    let uri = location["uri"].as_str().unwrap();
    assert!(uri.ends_with("Foo.java"), "Unexpected uri {uri}");
}

#[test]
fn test_camel_case_search() {
    let mut ws = setup();
    let results = search(&mut ws, Some("NPE"));
    assert!(!results.is_empty());
    assert!(results.iter().any(|s| name(s) == "NullPointerException"));

    let results = search(&mut ws, Some("HaMa"));
    let class_name = "HashMap";
    let found_class = results.iter().any(|s| name(s) == class_name);
    assert!(found_class, "Did not find {class_name}");
}

#[test]
fn test_camel_case_fuzzy_search() {
    let mut ws = setup();
    ws.use_upstream_test_jdk("hello");
    let expected: HashSet<&str> = ["BufferedInputStream", "BufferedOutputStream", "StringBufferInputStream"].into_iter().collect();
    let results = search(&mut ws, Some("BuffStream"));
    assert!(!results.is_empty());
    assert!(results.iter().all(|s| expected.contains(name(s))), "{results:?}");

    let results = search(&mut ws, Some("inkSet"));
    let class_name = "LinkedHashSet";
    let found_class = results.iter().any(|s| name(s) == class_name);
    assert!(found_class, "Did not find {class_name}");
}

#[test]
fn test_search_source_only() {
    let mut ws = setup();
    let query = "B*";
    let results = search_with(&mut ws, Some(query), 0, Some("hello"), true);
    assert_eq!(results.len(), 6, "Found {}result", results.len());
    let class_name = "BaseTest";
    let found_class = results.iter().any(|s| name(s) == class_name);
    assert!(found_class, "Did not find {class_name}");
}

#[test]
fn test_search_return_max_results() {
    let mut ws = setup();
    let query = "B*";
    let results = search_with(&mut ws, Some(query), 2, Some("hello"), true);
    assert_eq!(results.len(), 2, "Found {}result", results.len());
}

#[test]
fn test_empty_names() {
    let mut ws = setup();
    ws.import_projects(&["maven/reactor"]);
    let query = "Mono";
    let results = search_with(&mut ws, Some(query), 0, Some("reactor"), false);
    assert_eq!(results.len(), 119, "Found ");
    let has_empty_name = results.iter().any(|s| name(s).is_empty());
    assert!(!has_empty_name, "Found empty name");
}

#[test]
fn test_search_qualified_type_no_wildcards() {
    let mut ws = setup();
    let results = search(&mut ws, Some("java.io.file"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s).starts_with("File") && container(s) == "java.io"));

    let results = search(&mut ws, Some("java.util.array"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s).starts_with("Array") && container(s) == "java.util"));
}

#[test]
fn test_search_qualified_type_with_wildcards() {
    let mut ws = setup();
    let results = search(&mut ws, Some("java.util.*list*"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s) == "List" && container(s) == "java.util"));

    let results = search(&mut ws, Some("*.lang*.*exception"));
    assert!(results.len() > 1);
    assert!(results.iter().all(|s| name(s).ends_with("Exception") && container(s).contains(".lang")), "{results:?}");
}

#[test]
fn test_search_all_types_of_package() {
    let mut ws = setup();
    let results = search(&mut ws, Some("java.io"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s) == "File" && container(s) == "java.io"));

    let results = search(&mut ws, Some("java.lang"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s) == "Exception" && container(s) == "java.lang"));
}

#[test]
fn test_search_partial_package() {
    let mut ws = setup();
    let results = search(&mut ws, Some("util.Array"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s) == "ArrayList" && container(s) == "java.util"));

    let results = search(&mut ws, Some("util.Pattern"));
    assert!(results.len() > 1);
    assert!(results.iter().any(|s| name(s) == "Pattern" && container(s) == "java.util.regex"));
}

#[test]
fn test_search_without_duplicate() {
    let mut ws = setup();
    let results = search(&mut ws, Some("*"));
    let results_set: HashSet<String> = results.iter().map(|s| s.to_string()).collect();
    assert_eq!(results.len(), results_set.len());
}

#[test]
fn test_search_source_method_declarations() {
    let mut ws = setup();
    ws.settings["java"]["symbols"] = json!({ "includeSourceMethodDeclarations": true });
    let results = search_with(&mut ws, Some("deleteSomething"), 0, Some("hello"), true);
    assert_eq!(results.len(), 1, "Found {} result", results.len());
    let res = &results[0];
    assert_eq!(res["kind"], METHOD);
    assert_eq!(container(res), "org.sample.Baz");

    let results = search_with(&mut ws, Some("main"), 0, Some("hello"), true);
    assert_eq!(results.len(), 11, "Found {} result", results.len());
    let all_methods = results.iter().all(|s| s["kind"] == METHOD);
    assert!(all_methods, "Found a non-method symbol");
}

#[test]
fn test_deprecated() {
    let mut ws = setup();
    ws.capabilities["textDocument"]["documentSymbol"]["tagSupport"] = json!({ "valueSet": [1] });

    let results = search(&mut ws, Some("Certificate"));

    let deprecated = results.iter().find(|s| container(s) == "java.security");
    assert!(deprecated.is_some(), "{results:?}");
    let deprecated = deprecated.unwrap();
    assert!(deprecated["tags"].is_array());
    assert!(deprecated["tags"].as_array().unwrap().contains(&json!(DEPRECATED_TAG)), "Should have deprecated tag");

    let not_deprecated = results.iter().find(|s| container(s) == "java.security.cert");
    assert!(not_deprecated.is_some());
    let not_deprecated = not_deprecated.unwrap();
    if let Some(tags) = not_deprecated["tags"].as_array() {
        assert!(!tags.contains(&json!(DEPRECATED_TAG)), "Should not have deprecated tag");
    }
}

#[test]
fn test_deprecated_property() {
    let mut ws = setup();
    ws.capabilities["textDocument"]["documentSymbol"]
        .as_object_mut()
        .map(|o| o.remove("tagSupport"));

    let results = search(&mut ws, Some("Certificate"));

    let deprecated = results.iter().find(|s| container(s) == "java.security");
    assert!(deprecated.is_some());
    let deprecated = deprecated.unwrap();
    assert!(!deprecated["deprecated"].is_null());
    assert_eq!(deprecated["deprecated"], true, "Should be deprecated");
}

#[test]
fn test_workspace_search_with_class_content_support() {
    let mut ws = setup();
    ws.use_upstream_test_jdk("hello");
    set_class_file_content_support(&mut ws, true);
    //Classes will be found with jar container path.
    let results = search(&mut ws, Some("Array"));
    assert_ne!(results.len(), 0, "Unexpected results");
    // sample just the first symbol
    assert!(!results[0]["location"].is_null(), "Location is null");
    assert!(!results[0]["location"]["uri"].is_null(), "Location URI is null");
    assert!(results[0]["location"]["uri"].as_str().unwrap().contains("rtstubs.jar/"), "Wrong location URI");
}
