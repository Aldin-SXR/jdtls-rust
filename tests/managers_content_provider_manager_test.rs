//! Port of `org.eclipse.jdt.ls.core.internal.managers.ContentProviderManagerTest`
//! over `java/classFileContents` (`ContentProviderManager.getContent`).
//!
//! jdtls-rust has the jdt.ls default providers only: `sourceContentProvider`
//! (attached source) and `fernflowerContentProvider`.  The upstream test
//! plugin's `FakeContentProvider`/`placeholderContentProvider` extensions
//! cannot be registered, and upstream's sourceless JDK classes come from the
//! fake JDK `rtstubs.jar`, whereas the running JDK attaches `lib/src.zip`.

mod common;
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

fn setup() -> Fixture {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
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
#[ignore = "calls ContentProviderManager.getContent(null) directly; a null document URI has no LSP equivalent"]
fn test_open_nothing() {
    let mut f = setup();
    let result = f.ws.request("java/classFileContents", json!({ "uri": null }));
    assert!(result.is_null());
}

#[test]
#[ignore = "calls ContentProviderManager.getSource(null) directly; a null class file has no LSP equivalent"]
fn test_decompile_nothing() {
    let mut f = setup();
    let result = f.ws.request("java/classFileContents", json!({ "uri": null }));
    assert!(result.is_null());
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider (throwing) and the fake JDK rtstubs.jar (sourceless java.math.BigDecimal); the running JDK attaches lib/src.zip"]
fn test_throws_exception() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "requires the upstream test plugin's FakeContentProvider (throwing) and the fake JDK rtstubs.jar (sourceless java.math.BigDecimal); the running JDK attaches lib/src.zip"]
fn test_decompile_throws_exception() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar) and a log about the test plugin's duplicate providers; the running JDK attaches lib/src.zip"]
fn test_default_order() {
    let mut f = setup();
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar) and a log about the test plugin's duplicate providers; the running JDK attaches lib/src.zip"]
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
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar) and a log from the test plugin's placeholderContentProvider; the running JDK attaches lib/src.zip"]
fn test_prefer_non_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar) and a log from the test plugin's placeholderContentProvider; the running JDK attaches lib/src.zip"]
fn test_decompile_prefer_non_existing_provider_class() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["placeholderContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar); the running JDK attaches lib/src.zip"]
fn test_prefer_unknown_extension() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["unknownContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
}

#[test]
#[ignore = "expects java.math.BigDecimal to be sourceless (fake JDK rtstubs.jar); the running JDK attaches lib/src.zip"]
fn test_prefer_disassembler() {
    let mut f = setup();
    set_preferred(&mut f.ws, &["disassemblerContentProvider"]);
    let uri = f.sourceless_uri.clone();
    let result = get_content(&mut f.ws, &uri);
    assert!(result.starts_with(DECOMPILER_HEADER), "disassembler header is missing from {result}");
    assert!(result.contains("public class BigDecimal extends Number implements Comparable"), "unexpected body content {result}");
}

#[test]
#[ignore = "expects java.util.Map to be sourceless (fake JDK rtstubs.jar); the running JDK attaches lib/src.zip"]
fn test_disassemble_inner_class() {
    let mut f = setup();
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
