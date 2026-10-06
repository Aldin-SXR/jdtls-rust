//! Faithful direct API ports of ContentProviderManagerTest, including the
//! upstream fake extensions, log assertions and FernFlower line mappings.
//! Oracle mode invokes the unmodified Eclipse manager via a test-only fragment.

mod common;
#[path = "../src/features/content_provider.rs"]
mod content_provider;
#[path = "common/content_provider_fixture.rs"]
mod fixture;

use content_provider::DECOMPILER_HEADER;
use serde_json::{json, Value};

const FAKE_DECOMPILED_SOURCE: &str = "This is decompiled";

struct Fixture {
    ws: fixture::TestWorkspace,
    sourceless_uri: String,
    source_available_uri: String,
}
impl Fixture {
    fn new() -> Self {
        Self::with_projects(&[])
    }
    fn with_projects(projects: &[&str]) -> Self {
        let mut ws = fixture::workspace();
        // Import synchronously during initialize, before resolving any classes.
        // Eclipse's workspace-folder notification queues an asynchronous import.
        ws.import_projects(projects);
        let sourceless_uri = ws.class_file_uri("salut", "java.math.BigDecimal");
        let source_available_uri =
            ws.class_file_uri("salut", "org.apache.commons.lang3.text.WordUtils");
        Self {
            ws,
            sourceless_uri,
            source_available_uri,
        }
    }
    fn call(
        &mut self,
        api: &str,
        uri: Option<&str>,
        preferred: &[&str],
        kind: &str,
        value: Option<&str>,
    ) -> Value {
        fixture::run(
            &mut self.ws,
            preferred,
            vec![fixture::operation(api, uri, kind, value)],
        )
        .remove(0)
    }
    fn sourceless(
        &mut self,
        api: &str,
        preferred: &[&str],
        kind: &str,
        value: Option<&str>,
    ) -> Value {
        self.call(
            api,
            Some(&self.sourceless_uri.clone()),
            preferred,
            kind,
            value,
        )
    }
}
fn content(result: &Value) -> &str {
    result["content"]
        .as_str()
        .expect("content must not be null")
}
fn assert_decompiled(result: &Value) {
    assert!(
        content(result).starts_with(DECOMPILER_HEADER),
        "disassembler header is missing from {result}"
    );
}
fn expect_log(result: &Value, level: &str, expected: &str) {
    assert!(
        result[level]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains(expected)),
        "expected {level} containing {expected}: {result}"
    );
}

#[test]
fn test_open_source_code() {
    let mut f = Fixture::new();
    let result = f.call(
        "content",
        Some(&f.source_available_uri.clone()),
        &[],
        "null",
        None,
    );
    assert!(
        content(&result).contains("Operations on Strings that contain words."),
        "unexpected body content {result}"
    );
}
#[test]
fn test_decompile_source_code() {
    let mut f = Fixture::new();
    let result = f.call(
        "source",
        Some(&f.source_available_uri.clone()),
        &[],
        "null",
        None,
    );
    assert!(
        content(&result).contains("Operations on Strings that contain words."),
        "unexpected body content {result}"
    );
}
#[test]
fn test_open_missing_file() {
    let mut f = Fixture::new();
    let result = f.call(
        "content",
        Some("file:///this/is/Missing.class"),
        &[],
        "null",
        None,
    );
    assert!(content(&result).is_empty(), "not empty: {result}");
}
#[test]
fn test_open_thingy() {
    let mut f = Fixture::new();
    let result = f.call(
        "content",
        Some("file://this/is/Some.thingy"),
        &[],
        "text",
        Some(FAKE_DECOMPILED_SOURCE),
    );
    assert_eq!(FAKE_DECOMPILED_SOURCE, content(&result));
}
#[test]
fn test_open_nothing() {
    let mut ws = fixture::workspace();
    let result = fixture::run(
        &mut ws,
        &[],
        vec![fixture::operation("content", None, "null", None)],
    )
    .remove(0);
    assert!(result["content"].is_null());
}
#[test]
fn test_decompile_nothing() {
    let mut ws = fixture::workspace();
    let result = fixture::run(
        &mut ws,
        &[],
        vec![fixture::operation("source", None, "null", None)],
    )
    .remove(0);
    assert!(result["content"].is_null());
}
#[test]
fn test_throws_exception() {
    let mut f = Fixture::new();
    let result = f.sourceless(
        "content",
        &[],
        "exception",
        Some("Something bad happened here"),
    );
    assert_decompiled(&result);
    expect_log(&result, "errors", "Something bad happened here");
}
#[test]
fn test_decompile_throws_exception() {
    let mut f = Fixture::new();
    let result = f.sourceless(
        "source",
        &[],
        "exception",
        Some("Something bad happened here"),
    );
    assert_decompiled(&result);
    expect_log(&result, "errors", "Something bad happened here");
}
#[test]
fn test_default_order() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &[], "null", None);
    assert_decompiled(&result);
    expect_log(
        &result,
        "errors",
        "You have more than one content provider installed:",
    );
}
#[test]
fn test_decompile_default_order() {
    let mut f = Fixture::new();
    let result = f.sourceless("source", &[], "null", None);
    assert_decompiled(&result);
    expect_log(
        &result,
        "errors",
        "You have more than one content provider installed:",
    );
}
#[test]
fn test_prefer_existing_provider_class() {
    let mut f = Fixture::new();
    let result = f.sourceless(
        "content",
        &["fakeContentProvider", "placeholderContentProvider"],
        "text",
        Some(FAKE_DECOMPILED_SOURCE),
    );
    assert_eq!(FAKE_DECOMPILED_SOURCE, content(&result));
    assert!(result["errors"].as_array().unwrap().is_empty(), "{result}");
}
#[test]
fn test_decompile_prefer_existing_provider_class() {
    let mut f = Fixture::new();
    let result = f.sourceless(
        "source",
        &["fakeContentProvider", "placeholderContentProvider"],
        "text",
        Some(FAKE_DECOMPILED_SOURCE),
    );
    assert_eq!(FAKE_DECOMPILED_SOURCE, content(&result));
    assert!(result["errors"].as_array().unwrap().is_empty(), "{result}");
}
#[test]
fn test_prefer_non_existing_provider_class() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &["placeholderContentProvider"], "null", None);
    assert_decompiled(&result);
    expect_log(
        &result,
        "infos",
        "placeholderContentProvider doesn't match IContentProvider. Skipping.",
    );
}
#[test]
fn test_decompile_prefer_non_existing_provider_class() {
    let mut f = Fixture::new();
    let result = f.sourceless("source", &["placeholderContentProvider"], "null", None);
    assert_decompiled(&result);
    expect_log(
        &result,
        "infos",
        "placeholderContentProvider doesn't match IDecompiler. Skipping.",
    );
}
#[test]
fn test_prefer_unknown_extension() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &["unknownContentProvider"], "null", None);
    assert_decompiled(&result);
}
#[test]
fn test_prefer_disassembler() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &["disassemblerContentProvider"], "null", None);
    assert_decompiled(&result);
    assert!(
        content(&result).contains("public class BigDecimal extends Number implements Comparable"),
        "unexpected body content {result}"
    );
}
#[test]
fn test_disassemble_inner_class() {
    let mut f = Fixture::new();
    let uri = f.ws.class_file_uri("salut", "java.util.Map");
    let result = f.call(
        "content",
        Some(&uri),
        &["disassemblerContentProvider"],
        "null",
        None,
    );
    assert_decompiled(&result);
    assert!(
        content(&result).contains("interface Entry"),
        "unexpected body content {result}"
    );
    let uri = f.ws.class_file_uri("salut", "java.util.Map$Entry");
    let result = f.call(
        "content",
        Some(&uri),
        &["disassemblerContentProvider"],
        "null",
        None,
    );
    assert_decompiled(&result);
    assert!(
        content(&result).contains("public interface Map<"),
        "unexpected body content {result}"
    );
}
#[test]
fn test_decompile_line_mappings() {
    let mut f = Fixture::with_projects(&["eclipse/reference"]);
    let uri = fixture::class_file_uri(&mut f.ws, "reference", "org.sample.Foo");
    let result = f.call(
        "result",
        Some(&uri),
        &["fernflowerContentProvider"],
        "null",
        None,
    );
    assert!(!result["content"].is_null());
    let original = result["originalLineMappings"]
        .as_array()
        .expect("original mappings must not be null");
    assert_eq!(6, original.len());
    assert_eq!(json!(11), original[0]);
    assert_eq!(json!(12), original[1]);
    let decompiled = result["decompiledLineMappings"]
        .as_array()
        .expect("decompiled mappings must not be null");
    assert_eq!(6, decompiled.len());
}
#[test]
fn test_cancel_monitor() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &[], "cancel", None);
    assert_eq!(json!(true), result["canceled"]);
    assert!(content(&result).is_empty(), "not empty: {result}");
}
#[test]
fn test_expect_preferences() {
    let mut f = Fixture::new();
    let result = f.sourceless("content", &[], "null", None);
    assert_eq!(
        json!(true),
        result["preferencesMatch"],
        "preferences not set"
    );
}
#[test]
fn test_no_caching() {
    let mut f = Fixture::new();
    let results = fixture::run(
        &mut f.ws,
        &[],
        vec![
            fixture::operation(
                "content",
                Some(&f.sourceless_uri),
                "text",
                Some("some value"),
            ),
            fixture::operation(
                "content",
                Some(&f.sourceless_uri),
                "text",
                Some("something else"),
            ),
        ],
    );
    assert_eq!("some value", content(&results[0]));
    assert_eq!("something else", content(&results[1]));
}
