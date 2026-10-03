//! Port of `org.eclipse.jdt.ls.core.internal.handlers.DocumentSymbolHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

const PACKAGE: u64 = 4;
const CLASS: u64 = 5;
const METHOD: u64 = 6;
const CONSTRUCTOR: u64 = 9;
const ENUM: u64 = 10;
const INTERFACE: u64 = 11;
const CONSTANT: u64 = 14;
const ENUM_MEMBER: u64 = 22;
const FIELD: u64 = 8;
const DEPRECATED_TAG: u64 = 1;

/// `setup()`: imports `maven/salut` and `eclipse/source-attachment`.  The
/// mocked `ClientPreferences` answers become client capabilities.
fn setup(hierarchical: bool, symbol_tags: bool) -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut", "eclipse/source-attachment"]);
    configure(&mut ws, hierarchical, symbol_tags);
    ws
}

fn configure(ws: &mut Workspace, hierarchical: bool, symbol_tags: bool) {
    let ds = &mut ws.capabilities["textDocument"]["documentSymbol"];
    ds["hierarchicalDocumentSymbolSupport"] = json!(hierarchical);
    if symbol_tags {
        ds["tagSupport"] = json!({ "valueSet": [1] });
    }
}

fn document_symbol(ws: &mut Workspace, uri: &str) -> Vec<Value> {
    let result = ws.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
    result.as_array().cloned().unwrap_or_default()
}

fn get_symbols(ws: &mut Workspace, project: &str, class_name: &str) -> Vec<Value> {
    let uri = ws.class_uri(project, class_name);
    let symbols = document_symbol(ws, &uri);
    for s in &symbols {
        assert!(s.get("location").is_some(), "expected SymbolInformation, got {s}");
    }
    assert!(!symbols.is_empty(), "No symbols found for {class_name}");
    symbols
}

fn get_hierarchical_document_symbols(ws: &mut Workspace, uri: &str) -> Vec<Value> {
    let symbols = document_symbol(ws, uri);
    for s in &symbols {
        assert!(s.get("selectionRange").is_some(), "expected DocumentSymbol, got {s}");
    }
    assert!(!symbols.is_empty());
    symbols
}

fn internal_get_hierarchical_symbols(ws: &mut Workspace, project: &str, class_name: &str) -> Vec<Value> {
    let uri = ws.class_uri(project, class_name);
    get_hierarchical_document_symbols(ws, &uri)
}

fn get_hierarchical_symbols(ws: &mut Workspace, class_name: &str) -> Vec<Value> {
    internal_get_hierarchical_symbols(ws, "salut", class_name)
}

fn name_detail(s: &Value) -> String {
    format!("{}{}", s["name"].as_str().unwrap_or(""), s["detail"].as_str().unwrap_or("null"))
}

/// Breadth-first traversal of every symbol tree.
fn as_stream(symbols: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for s in symbols {
        let mut queue = std::collections::VecDeque::from([s.clone()]);
        while let Some(n) = queue.pop_front() {
            if let Some(children) = n["children"].as_array() {
                queue.extend(children.iter().cloned());
            }
            out.push(n);
        }
    }
    out
}

fn assert_has_symbol(expected_type: &str, expected_parent: &str, expected_kind: u64, symbols: &[Value]) {
    let symbol = symbols
        .iter()
        .find(|s| s["name"] == expected_type && s["containerName"] == expected_parent)
        .unwrap_or_else(|| panic!("{expected_type} ({expected_parent}) is missing from {symbols:#?}"));
    assert_eq!(symbol["kind"], expected_kind, "Unexpected SymbolKind in {expected_type}");
}

fn assert_has_hierarchical_symbol(expected_type: &str, expected_parent: Option<&str>, expected_kind: u64, symbols: &[Value]) {
    let symbol = match expected_parent {
        Some(expected_parent) => {
            let all = as_stream(symbols);
            let parent = all
                .iter()
                .find(|s| name_detail(s) == expected_parent)
                .unwrap_or_else(|| panic!("Cannot find parent with name: {expected_parent}"))
                .clone();
            let children = parent["children"].as_array().cloned().unwrap_or_default();
            all.into_iter().find(|s| name_detail(s) == expected_type && children.contains(s))
        }
        None => symbols.iter().find(|s| s["name"] == expected_type).cloned(),
    };
    let symbol = symbol.unwrap_or_else(|| panic!("{expected_type} ({expected_parent:?}) is missing from {symbols:#?}"));
    assert_eq!(symbol["kind"], expected_kind, "Unexpected SymbolKind in {expected_type}");
}

fn is_valid(range: &Value) -> bool {
    ["start", "end"].iter().all(|p| range[p]["line"].as_i64().is_some_and(|l| l >= 0) && range[p]["character"].as_i64().is_some_and(|c| c >= 0))
}

fn test_class(ws: &mut Workspace, class_name: &str, hierarchical: bool) {
    if !hierarchical {
        let symbols = get_symbols(ws, "salut", class_name);
        for symbol in &symbols {
            assert!(is_valid(&symbol["location"]["range"]), "Class: {class_name}, Symbol:{} - invalid location.", symbol["name"]);
        }
    } else {
        let symbols = get_hierarchical_symbols(ws, class_name);
        for symbol in &symbols {
            assert!(
                is_valid(&symbol["range"]) && is_valid(&symbol["selectionRange"]),
                "Class: {class_name}, Symbol:{} - invalid location.",
                symbol["name"]
            );
        }
    }
}

#[test]
#[ignore = "needs jdt:// classfile support (commons-lang3 WordUtils)"]
fn test_document_symbol_handler() {
    let mut ws = setup(false, false);
    test_class(&mut ws, "org.apache.commons.lang3.text.WordUtils", false);
}

#[test]
#[ignore = "needs jdt:// classfile support (commons-lang3 WordUtils)"]
fn test_document_symbol_handler_hierarchical() {
    let mut ws = setup(true, false);
    test_class(&mut ws, "org.apache.commons.lang3.text.WordUtils", true);
}

#[test]
#[ignore = "needs jdt:// classfile support (commons-lang3 StrTokenizer)"]
fn test_synthetic_member() {
    let mut ws = setup(false, false);
    let class_name = "org.apache.commons.lang3.text.StrTokenizer";
    let symbols = get_symbols(&mut ws, "salut", class_name);
    let overloaded_method1 = "getCSVInstance(String)";
    let overloaded_method2 = "reset()";
    let (mut found1, mut found2) = (false, false);
    for symbol in &symbols {
        let name = symbol["name"].as_str().unwrap();
        assert!(is_valid(&symbol["location"]["range"]), "Class: {class_name}, Symbol:{name} - invalid location.");
        assert!(!name.starts_with("access$"), "Class: {class_name}, Symbol:{name} - invalid name");
        assert!(name != "<clinit>", "Class: {class_name}, Symbol:{name}- invalid name");
        found1 |= name == overloaded_method1;
        found2 |= name == overloaded_method2;
    }
    assert!(found1, "The {overloaded_method1} method hasn't been found");
    assert!(found2, "The {overloaded_method2} method hasn't been found");
}

#[test]
#[ignore = "needs jdt:// classfile support (commons-lang3 StrTokenizer)"]
fn test_synthetic_member_hierarchical() {
    let mut ws = setup(true, false);
    let class_name = "org.apache.commons.lang3.text.StrTokenizer";
    let symbols = as_stream(&get_hierarchical_symbols(&mut ws, class_name));
    let overloaded_method1 = "getCSVInstance(String) : StrTokenizer";
    let overloaded_method2 = "reset() : StrTokenizer";
    let (mut found1, mut found2) = (false, false);
    for symbol in &symbols {
        let name = symbol["name"].as_str().unwrap();
        assert!(
            is_valid(&symbol["range"]) && is_valid(&symbol["selectionRange"]),
            "Class: {class_name}, Symbol:{name} - invalid location."
        );
        assert!(!name.starts_with("access$"), "Class: {class_name}, Symbol:{name} - invalid name");
        assert!(name != "<clinit>", "Class: {class_name}, Symbol:{name}- invalid name");
        found1 |= name_detail(symbol) == overloaded_method1;
        found2 |= name_detail(symbol) == overloaded_method2;
    }
    assert!(found1, "The {overloaded_method1} method hasn't been found");
    assert!(found2, "The {overloaded_method2} method hasn't been found");
}

#[test]
fn test_types() {
    let mut ws = setup(false, false);
    let symbols = get_symbols(&mut ws, "salut", "org.sample.Bar");
    assert_has_symbol("Bar", "Bar.java", CLASS, &symbols);
    assert_has_symbol("main(String[])", "Bar", METHOD, &symbols);
    assert_has_symbol("MyInterface", "Bar", INTERFACE, &symbols);
    assert_has_symbol("foo()", "MyInterface", METHOD, &symbols);
    assert_has_symbol("MyClass", "Bar", CLASS, &symbols);
    assert_has_symbol("bar()", "MyClass", METHOD, &symbols);
    assert_has_symbol("Foo", "Bar", ENUM, &symbols);
    assert_has_symbol("Bar", "Foo", ENUM_MEMBER, &symbols);
    assert_has_symbol("Zoo", "Foo", ENUM_MEMBER, &symbols);
    assert_has_symbol("EMPTY", "Bar", CONSTANT, &symbols);
}

#[test]
fn test_types_hierarchical() {
    let mut ws = setup(true, false);
    let symbols = get_hierarchical_symbols(&mut ws, "org.sample.Bar");
    assert_has_hierarchical_symbol("main(String[]) : void", Some("Bar"), METHOD, &symbols);
    assert_has_hierarchical_symbol("MyInterface", Some("Bar"), INTERFACE, &symbols);
    assert_has_hierarchical_symbol("foo() : void", Some("MyInterface"), METHOD, &symbols);
    assert_has_hierarchical_symbol("MyClass", Some("Bar"), CLASS, &symbols);
    assert_has_hierarchical_symbol("bar() : void", Some("MyClass"), METHOD, &symbols);
    assert_has_hierarchical_symbol("org.sample", None, PACKAGE, &symbols);
}

#[test]
#[ignore = "needs jdt:// classfile support (commons-lang3 WordUtils)"]
fn test_package_class() {
    let mut ws = setup(true, false);
    let symbols = get_hierarchical_symbols(&mut ws, "org.apache.commons.lang3.text.WordUtils");
    assert_has_hierarchical_symbol("org.apache.commons.lang3.text", None, PACKAGE, &symbols);
}

#[test]
#[ignore = "needs jdt:// classfile support (foo.bar in source-attachment's foo.jar, no source)"]
fn test_synthetic_member_hierarchical_no_source_attached() {
    let mut ws = setup(true, false);
    let symbols = as_stream(&internal_get_hierarchical_symbols(&mut ws, "source-attachment", "foo.bar"));
    assert_has_hierarchical_symbol("bar()", Some("bar"), CONSTRUCTOR, &symbols);
    assert_has_hierarchical_symbol("add(int...) : int", Some("bar"), METHOD, &symbols);
}

#[test]
fn test_deprecated() {
    let mut ws = setup(false, true);
    let symbols = get_symbols(&mut ws, "salut", "org.sample.Bar");

    let deprecated = symbols.iter().find(|s| s["name"] == "MyInterface").expect("MyInterface");
    assert_eq!(deprecated["kind"], INTERFACE);
    assert!(deprecated["tags"].is_array());
    assert!(deprecated["tags"].as_array().unwrap().contains(&json!(DEPRECATED_TAG)), "Should have deprecated tag");

    let not_deprecated = symbols.iter().find(|s| s["name"] == "MyClass").expect("MyClass");
    assert_eq!(not_deprecated["kind"], CLASS);
    if not_deprecated["tags"].is_array() {
        // (sic) upstream re-checks the deprecated symbol's tags here.
        assert!(!deprecated["tags"].as_array().unwrap().contains(&json!(DEPRECATED_TAG)), "Should not have deprecated tag");
    }
}

#[test]
fn test_deprecated_property() {
    let mut ws = setup(false, false);
    let symbols = get_symbols(&mut ws, "salut", "org.sample.Bar");

    let deprecated = symbols.iter().find(|s| s["name"] == "MyInterface").expect("MyInterface");
    assert_eq!(deprecated["kind"], INTERFACE);
    assert!(!deprecated["deprecated"].is_null());
    assert_eq!(deprecated["deprecated"], true, "Should be deprecated");
}

#[test]
fn test_lombok() {
    if std::env::var("jdt.ls.lombok.disabled").as_deref() == Ok("true") {
        return;
    }
    let mut ws = setup(false, false);
    ws.import_projects(&["maven/mavenlombok"]);
    let class_name = "org.sample.Test";
    let symbols = get_symbols(&mut ws, "mavenlombok", class_name);
    assert!(!symbols.is_empty(), "No symbols found for {class_name}");
    assert_has_symbol("Test", "Test.java", CLASS, &symbols);
    assert!(!symbols.iter().any(|s| s["kind"] == METHOD));
}

#[test]
#[ignore = "needs Lombok-generated members in the Java model (java.symbols.includeGeneratedCode)"]
fn test_lombok_show_generated_code_symbols() {
    let mut ws = setup(false, false);
    ws.settings = json!({ "java": { "symbols": { "includeGeneratedCode": true } } });
    ws.import_projects(&["maven/mavenlombok"]);
    let class_name = "org.sample.Test";
    let symbols = get_symbols(&mut ws, "mavenlombok", class_name);
    assert!(!symbols.is_empty(), "No symbols found for {class_name}");
    assert_has_symbol("Test", "Test.java", CLASS, &symbols);
    assert!(
        symbols.iter().any(|s| s["kind"] == METHOD),
        "Generated methods should appear when java.symbols.includeGeneratedCode is true"
    );
}

#[test]
#[ignore = "needs jdt:// classfile support (decompiled org.sample.Foo from eclipse/reference)"]
fn test_decompiled_source() {
    let mut ws = setup(true, false);
    ws.import_projects(&["eclipse/reference"]);
    let symbols = internal_get_hierarchical_symbols(&mut ws, "reference", "org.sample.Foo");
    assert_eq!(2, symbols.len());
    assert_has_hierarchical_symbol("org.sample", None, PACKAGE, &symbols);
    assert_has_hierarchical_symbol("Foo", None, ENUM, &symbols);
    assert_has_hierarchical_symbol("FOO1", Some("Foo"), ENUM_MEMBER, &symbols);
    assert_has_hierarchical_symbol("value", Some("Foo"), FIELD, &symbols);
    assert_has_hierarchical_symbol("getValue() : int", Some("Foo"), METHOD, &symbols);
    assert_has_hierarchical_symbol("Foo(int)", Some("Foo"), CONSTRUCTOR, &symbols);
}

#[test]
fn test_document_symbols_on_plain_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("testDocumentSymbolsOnPlainFile.java");
    std::fs::write(&f, "public class SomeClass {\n\tint someField;\n}\n").unwrap();
    let mut ws = setup(true, false);
    let uri = tower_lsp::lsp_types::Url::from_file_path(&f).unwrap().to_string();
    let symbols = get_hierarchical_document_symbols(&mut ws, &uri);
    let class_symbol = &symbols[0];
    assert_eq!("someField", class_symbol["children"][0]["name"]);
}
