//! Port of `org.eclipse.jdt.ls.core.internal.handlers.FoldingRangeHandlerTest`.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/foldingRange"]);
    ws
}

fn get_folding_ranges(ws: &mut Workspace, class_name: &str) -> Vec<Value> {
    let uri = ws.class_uri("foldingRange", class_name);
    let result = ws.request("textDocument/foldingRange", json!({ "textDocument": { "uri": uri } }));
    result.as_array().cloned().unwrap_or_default()
}

fn assert_has_folding_range(start_line: u64, end_line: u64, expected_kind: Option<&str>, ranges: &[Value]) {
    let range = ranges
        .iter()
        .find(|r| r["startLine"] == start_line && r["endLine"] == end_line)
        .unwrap_or_else(|| panic!("no folding range {start_line}-{end_line} in {ranges:#?}"));
    assert_eq!(range["kind"].as_str(), expected_kind, "Expected type{expected_kind:?}");
}

fn test_class_for_valid_range(class_name: &str, ranges: &[Value]) {
    for r in ranges {
        assert!(
            r["startLine"].as_u64() <= r["endLine"].as_u64(),
            "Class: {class_name}, FoldingRange:{:?} - invalid location.",
            r["kind"]
        );
    }
}

#[test]
fn test_folding_ranges() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.apache.commons.lang3.text.WordUtils");
    assert_has_folding_range(18, 22, Some("imports"), &ranges);
    test_class_for_valid_range("org.apache.commons.lang3.text.WordUtils", &ranges);
}

#[test]
fn test_types() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.SimpleFoldingRange");
    assert_eq!(8, ranges.len());
    assert_has_folding_range(2, 3, Some("imports"), &ranges);
    assert_has_folding_range(5, 7, Some("comment"), &ranges);
    assert_has_folding_range(8, 26, None, &ranges);
    assert_has_folding_range(10, 14, Some("comment"), &ranges);
    assert_has_folding_range(19, 24, None, &ranges);
    assert_has_folding_range(20, 22, None, &ranges);
    assert_has_folding_range(28, 30, Some("comment"), &ranges);
}

#[test]
fn test_error_types() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.UnmatchFoldingRange");
    assert_eq!(3, ranges.len());
    assert_has_folding_range(2, 12, None, &ranges);
    assert_has_folding_range(3, 10, None, &ranges);
    assert_has_folding_range(5, 7, None, &ranges);
}

#[test]
fn test_invalid_input() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.InvalidInputRange");
    assert_eq!(3, ranges.len());
    assert_has_folding_range(2, 4, Some("comment"), &ranges);
    assert_has_folding_range(5, 10, None, &ranges);
    assert_has_folding_range(7, 9, None, &ranges);
}

#[test]
fn test_region_folding_ranges() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.RegionFoldingRange");
    assert_eq!(7, ranges.len());
    assert_has_folding_range(7, 15, Some("region"), &ranges);
    assert_has_folding_range(17, 23, Some("region"), &ranges);
    assert_has_folding_range(18, 20, Some("region"), &ranges);
}

#[test]
fn test_statement_folding_ranges() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.StatementFoldingRange");
    assert_eq!(18, ranges.len());
    assert_has_folding_range(2, 4, Some("comment"), &ranges);
    assert_has_folding_range(5, 53, None, &ranges);
    assert_has_folding_range(7, 52, None, &ranges);

    // First switch statement
    assert_has_folding_range(10, 23, None, &ranges);
    assert_has_folding_range(11, 18, None, &ranges);
    assert_has_folding_range(19, 20, None, &ranges);
    assert_has_folding_range(21, 22, None, &ranges);

    // Try catch:
    assert_has_folding_range(12, 13, None, &ranges);
    assert_has_folding_range(14, 16, None, &ranges);

    // If statement:
    assert_has_folding_range(26, 27, None, &ranges);
    assert_has_folding_range(28, 29, None, &ranges);
    assert_has_folding_range(30, 32, None, &ranges);

    // Second switch statement:
    assert_has_folding_range(36, 51, None, &ranges);
    assert_has_folding_range(37, 40, None, &ranges);
    assert_has_folding_range(41, 47, None, &ranges);
    assert_has_folding_range(48, 50, None, &ranges);
}

#[test]
fn test_nested_switch_folding_ranges() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.NestedSwitchFoldingRange");
    assert_eq!(10, ranges.len());
    assert_has_folding_range(2, 32, None, &ranges);
    assert_has_folding_range(11, 31, None, &ranges);

    // First switch statement
    assert_has_folding_range(16, 30, None, &ranges);
    assert_has_folding_range(17, 25, None, &ranges);
    assert_has_folding_range(26, 29, None, &ranges);

    // Nested switch statement:
    assert_has_folding_range(19, 24, None, &ranges);
    assert_has_folding_range(20, 21, None, &ranges);
    assert_has_folding_range(22, 23, None, &ranges);
}

// https://github.com/eclipse-jdtls/eclipse.jdt.ls/issues/2865
#[test]
fn test_curly_braces_own_line() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.NestedSwitchFoldingRange");
    assert_eq!(10, ranges.len());
    assert_has_folding_range(4, 9, None, &ranges);
    assert_has_folding_range(7, 8, None, &ranges);
}

#[test]
fn test_static_block_folding_range() {
    let mut ws = setup();
    let ranges = get_folding_ranges(&mut ws, "org.sample.StaticBlockFoldingRange");
    assert_eq!(5, ranges.len());
    assert_has_folding_range(2, 18, None, &ranges);
    assert_has_folding_range(4, 5, None, &ranges);
    assert_has_folding_range(7, 12, None, &ranges);
    assert_has_folding_range(14, 15, None, &ranges);
    assert_has_folding_range(17, 17, None, &ranges);
}
