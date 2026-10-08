//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionInsertReplaceCapabilityTest`.
//!
//! `mockClient()` stubs snippet, signature help and insert/replace support;
//! the Mockito mock answers `false` for every other client preference. Like
//! upstream, the project runs on the rtstubs test JDK.

mod common;
use common::completion::*;

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionInsertReplaceCapabilityTest.setUp`.
fn setup() -> T {
    let mut t = setup_with(settings());
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { snippets: true, signature_help: true, insert_replace: true, ..Default::default() };
    t
}

#[test]
fn test_completion_insert_replace_edit() {
    let mut t = setup();
    t.caps.insert_replace = true;
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        &[
            "public class Test {",
            "\tpublic static void main(String[] args) {",
            "\t\tif (\"foo\".equSystem.getProperty(\"bar\")) {}",
            "\t}",
            "}",
        ]
        .join("\n"),
    );
    let list = t.request_completions(&unit, ".equ");
    let item = items(&list).into_iter().next().unwrap_or_else(|| panic!("no items: {list:#}"));
    let edit = &item["textEdit"];
    assert!(!edit["insert"].is_null() && !edit["replace"].is_null(), "not an InsertReplaceEdit: {item:#}");
    assert!(s(&edit["newText"]).starts_with("equals("), "{item:#}");
    // check insert range
    assert_position(2, 12, &edit["insert"]["start"]);
    assert_position(2, 15, &edit["insert"]["end"]);
    // check replace range
    assert_position(2, 12, &edit["replace"]["start"]);
    assert_position(2, 21, &edit["replace"]["end"]);
}
