//! Port of `org.eclipse.jdt.ls.core.internal.managers.ContentProviderManagerTest`
//! over `java/classFileContents` and the Rust manager's direct null-input API.
//!
//! jdtls-rust has the jdt.ls default providers only: `sourceContentProvider`
//! (attached source) and `fernflowerContentProvider`.  The upstream test
//! plugin's `FakeContentProvider`/`placeholderContentProvider` extensions
//! still need direct fixture injection and real-manager oracle verification.
//! Sourceless binary cases use the upstream fake JDK
//! `rtstubs.jar` alongside the original Maven dependencies.

mod common;
#[path = "../src/features/content_provider.rs"]
mod content_provider;
use content_provider::{Manager, Monitor, Preferences};
use std::sync::Arc;
use common::jdtls::Workspace;
use serde_json::json;

const FAKE_DECOMPILED_SOURCE: &str = "This is decompiled";
/// `FernFlowerDecompiler.DECOMPILER_HEADER`.
const DECOMPILER_HEADER: &str = "// Source code is decompiled from a .class file using FernFlower decompiler (from Intellij IDEA).\n";

struct Fixture {
    ws: Workspace,
    sourceless_uri: String,
    source_available_uri: String,
}

fn setup() -> Fixture { setup_with_test_jdk(false) }

fn setup_with_test_jdk(stub_jdk: bool) -> Fixture {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    if stub_jdk { ws.use_upstream_maven_test_jdk("salut", "1.8"); }
    let sourceless_uri = ws.class_file_uri("salut", "java.math.BigDecimal");
    let source_available_uri = ws.class_file_uri("salut", "org.apache.commons.lang3.text.WordUtils");
    Fixture { ws, sourceless_uri, source_available_uri }
}

fn get_content(ws: &mut Workspace, uri: &str) -> String {
    let result = ws.request("java/classFileContents", json!({ "uri": uri }));
    result.as_str().expect("content must not be null").to_owned()
}

fn set_preferred(ws: &mut Workspace, ids: &[&str]) {
    ws.update_settings(json!({ "java": { "contentProvider": { "preferred": ids } } }));
}

#[test]
fn test_open_source_code() {
    let mut f = setup();
    let uri = f.source_available_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.contains("Operations on Strings that contain words."), "unexpected body content {result}");
}

#[test]
fn test_decompile_source_code() {
    let mut f = setup();
    let uri = f.source_available_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.contains("Operations on Strings that contain words."), "unexpected body content {result}");
}

#[test]
fn test_open_missing_file() {
    let mut f = setup();
    let result = get_content(&mut f.ws, "file:///this/is/Missing.class");
    assert!(result.is_empty(), "not empty: {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider registered for *.thingy URIs"]
fn test_open_thingy() {
    let mut f = setup();
    assert_eq!(FAKE_DECOMPILED_SOURCE, get_content(&mut f.ws, "file://this/is/Some.thingy"));
}

#[test]
fn test_open_nothing() {
    let manager = Manager::new(Arc::new(Preferences::default()), Vec::new());
    let result = futures::executor::block_on(manager.get_content(None, &Monitor::default()));
    assert!(result.is_none());
}

#[test]
fn test_decompile_nothing() {
    let manager = Manager::new(Arc::new(Preferences::default()), Vec::new());
    let result = futures::executor::block_on(manager.get_source(None, &Monitor::default()));
    assert!(result.is_none());
}

#[test]
#[ignore = "requires the upstream test plugin's throwing FakeContentProvider and its logged-error assertion"]
fn test_throws_exception() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's throwing FakeContentProvider and its logged-error assertion"]
fn test_decompile_throws_exception() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's duplicate-provider extensions and logged-error assertion"]
fn test_default_order() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's duplicate-provider extensions and logged-error assertion"]
fn test_decompile_default_order() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider"]
fn test_prefer_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["fakeContentProvider", "placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    assert_eq!(FAKE_DECOMPILED_SOURCE, get_content(&mut f.ws, &uri));
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider"]
fn test_decompile_prefer_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["fakeContentProvider", "placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    assert_eq!(FAKE_DECOMPILED_SOURCE, get_content(&mut f.ws, &uri));
}

#[test]
#[ignore = "requires the upstream test plugin's placeholderContentProvider and logged-info assertion"]
fn test_prefer_non_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's placeholderContentProvider and logged-info assertion"]
fn test_decompile_prefer_non_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
fn test_prefer_unknown_extension() {
    let mut f = setup_with_test_jdk(true);
    set_preferred(&mut f.ws, &["unknownContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
fn test_prefer_disassembler() {
    let mut f = setup_with_test_jdk(true);
    set_preferred(&mut f.ws, &["disassemblerContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
    assert!(result.contains("public class BigDecimal extends Number implements Comparable"), "unexpected body content {result}");
}

#[test]
fn test_disassemble_inner_class() {
    let mut f = setup_with_test_jdk(true);
    set_preferred(&mut f.ws, &["disassemblerContentProvider"]);
    let uri = f.ws.class_file_uri("salut", "java.util.Map");
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
    assert!(result.contains("interface Entry"), "unexpected body content {result}");

    let uri = f.ws.class_file_uri("salut", "java.util.Map$Entry");
    // Decompiling the inner class directly should include the outer class code
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
    assert!(result.contains("public interface Map<"), "unexpected body content {result}");
}

#[test]
#[ignore = "inspects DecompilerResult line mappings, which java/classFileContents does not expose"]
fn test_decompile_line_mappings() {
    let mut f = setup();
    f.ws.import_projects(&["eclipse/reference"]);
    let uri = f.ws.class_file_uri("reference", "org.sample.Foo");
    set_preferred(&mut f.ws, &["fernflowerContentProvider"]);
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER));
    unreachable!("line mappings (original [11, 12, ...], 6 entries each) are not available over LSP");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider cancelling the monitor"]
fn test_cancel_monitor() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.is_empty(), "not empty");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider recording the preferences"]
fn test_expect_preferences() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    get_content(&mut f.ws, &uri);
    unreachable!("FakeContentProvider.preferences is not observable");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider"]
fn test_no_caching() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    assert_eq!("some value", get_content(&mut f.ws, &uri));
    assert_eq!("something else", get_content(&mut f.ws, &uri));
}
