//! Port of `org.eclipse.jdt.ls.core.internal.handlers.DocumentHighlightHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

const READ: u64 = 2;
const WRITE: u64 = 3;

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws
}

fn request_highlights(ws: &mut Workspace, compilation_unit: &str, line: u32, character: u32) -> Vec<Value> {
    let uri = ws.class_file_uri("hello", compilation_unit);
    let result = ws.request(
        "textDocument/documentHighlight",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    let mut highlights = result.as_array().cloned().unwrap_or_default();
    // Sorting the highlights to make testing easier
    highlights.sort_by_key(|h| (h["range"]["start"]["line"].as_u64().unwrap(), h["range"]["start"]["character"].as_u64().unwrap()));
    highlights
}

/// `Lsp4jAssertions.assertRange` + kind.
fn assert_highlight(highlight: &Value, expected_line: u64, expected_start: u64, expected_end: u64, expected_kind: u64) {
    let r = &highlight["range"];
    assert_eq!(expected_line, r["start"]["line"], "{highlight}");
    assert_eq!(expected_start, r["start"]["character"], "{highlight}");
    assert_eq!(expected_line, r["end"]["line"], "{highlight}");
    assert_eq!(expected_end, r["end"]["character"], "{highlight}");
    assert_eq!(expected_kind, highlight["kind"], "{highlight}");
}

#[test]
fn test_document_highlight_exception_occurences() {
    let mut ws = setup();
    let result = request_highlights(&mut ws, "org.sample.Highlight", 8, 34);
    assert_eq!(2, result.len(), "{result:?}");
    let mut it = result.iter();
    assert_highlight(it.next().unwrap(), 8, 31, 42, READ);
    assert_highlight(it.next().unwrap(), 10, 3, 8, READ);
}

#[test]
fn test_document_highlight_method_exits() {
    let mut ws = setup();
    let result = request_highlights(&mut ws, "org.sample.Highlight", 8, 11);
    assert_eq!(4, result.len(), "{result:?}");
    let mut it = result.iter();
    assert_highlight(it.next().unwrap(), 8, 8, 14, READ);
    assert_highlight(it.next().unwrap(), 10, 3, 8, READ);
    assert_highlight(it.next().unwrap(), 14, 3, 8, READ);
    assert_highlight(it.next().unwrap(), 25, 2, 21, READ);
}

#[test]
fn test_document_highlight_break_continue_target() {
    let mut ws = setup();
    let result = request_highlights(&mut ws, "org.sample.Highlight", 19, 7);
    assert_eq!(2, result.len(), "{result:?}");
    let mut it = result.iter();
    assert_highlight(it.next().unwrap(), 16, 2, 6, READ);
    assert_highlight(it.next().unwrap(), 23, 2, 3, READ);
}

#[test]
fn test_document_highlight_implement_occurrences() {
    let mut ws = setup();
    let result = request_highlights(&mut ws, "org.sample.Highlight", 4, 38);
    assert_eq!(3, result.len(), "{result:?}");
    let mut it = result.iter();
    assert_highlight(it.next().unwrap(), 4, 34, 46, READ);
    assert_highlight(it.next().unwrap(), 34, 13, 16, READ);
    assert_highlight(it.next().unwrap(), 39, 13, 16, READ);
}

#[test]
fn test_document_highlight_occurrences() {
    let mut ws = setup();
    let result = request_highlights(&mut ws, "org.sample.Highlight", 6, 18);
    assert_eq!(9, result.len(), "{result:?}");
    let mut it = result.iter();
    assert_highlight(it.next().unwrap(), 6, 16, 19, WRITE);
    assert_highlight(it.next().unwrap(), 9, 6, 9, READ);
    assert_highlight(it.next().unwrap(), 12, 2, 5, WRITE);
    assert_highlight(it.next().unwrap(), 13, 6, 9, READ);
    assert_highlight(it.next().unwrap(), 16, 16, 19, READ);
    assert_highlight(it.next().unwrap(), 18, 8, 11, READ);
    assert_highlight(it.next().unwrap(), 24, 2, 5, WRITE);
    assert_highlight(it.next().unwrap(), 24, 22, 25, READ);
    assert_highlight(it.next().unwrap(), 25, 9, 12, READ);
}
