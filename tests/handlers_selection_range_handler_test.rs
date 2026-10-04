//! Port of `org.eclipse.jdt.ls.core.internal.handlers.SelectionRangeHandlerTest`.

mod common;
use common::jdtls::{pos, range, Workspace};
use serde_json::{json, Value};

fn type_decl_range() -> Value {
    range(2, 0, 31, 1)
}

fn comp_unit_range() -> Value {
    range(0, 0, 32, 0)
}

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    ws
}

fn get_selection_range(ws: &mut Workspace, class_name: &str, position: Value) -> Value {
    let uri = ws.class_uri("salut", class_name);
    let result = ws.request(
        "textDocument/selectionRange",
        json!({ "textDocument": { "uri": uri }, "positions": [position] }),
    );
    result[0].clone()
}

fn validate_selection_range(range: &Value, ranges: &[Value]) -> bool {
    let mut range = Some(range);
    let mut iterator = ranges.iter();
    let mut next = iterator.next();
    while let (Some(r), Some(expected)) = (range, next) {
        if r["range"] != *expected {
            return false;
        }
        range = r.get("parent").filter(|p| !p.is_null());
        next = iterator.next();
    }
    range.is_none() && next.is_none()
}

fn assert_selection_range(range: &Value, ranges: &[Value]) {
    assert!(validate_selection_range(range, ranges), "unexpected selection range {range:#}\nexpected {ranges:#?}");
}

#[test]
fn test_javadoc() {
    let mut ws = setup();
    let range = get_selection_range(&mut ws, "org.sample.Foo4", pos(9, 31));
    assert_selection_range(
        &range,
        &[
            common::jdtls::range(9, 4, 9, 40), // text element
            common::jdtls::range(9, 4, 9, 40), // tag element
            common::jdtls::range(8, 1, 10, 4), // javadoc
            common::jdtls::range(8, 1, 16, 2), // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );
}

#[test]
fn test_comments() {
    let mut ws = setup();
    // line comment
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(12, 57));
    assert_selection_range(
        &r,
        &[
            range(12, 43, 12, 66), // line comment
            range(11, 8, 16, 2),   // block
            range(8, 1, 16, 2),    // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );

    // block comment
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(14, 17));
    assert_selection_range(
        &r,
        &[
            range(14, 2, 14, 29), // block comment
            range(11, 8, 16, 2),  // block
            range(8, 1, 16, 2),   // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );

    // block comment in param list
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(18, 42));
    assert_selection_range(
        &r,
        &[
            range(18, 27, 18, 68), // block comment
            range(18, 1, 30, 2),   // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );
}

#[test]
fn test_string_literal() {
    let mut ws = setup();
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(12, 30));
    assert_selection_range(
        &r,
        &[
            range(12, 21, 12, 40), // string literal
            range(12, 2, 12, 41),  // method invocation
            range(12, 2, 12, 42),  // expression statement
            range(11, 8, 16, 2),   // block
            range(8, 1, 16, 2),    // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );
}

#[test]
fn test_param_list() {
    let mut ws = setup();
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(18, 24));
    assert_selection_range(
        &r,
        &[
            range(18, 21, 18, 27), // simple name
            range(18, 17, 18, 27), // single variable declaration
            range(18, 1, 30, 2),   // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );
}

#[test]
fn test_switch() {
    let mut ws = setup();
    let r = get_selection_range(&mut ws, "org.sample.Foo4", pos(22, 27));
    assert_selection_range(
        &r,
        &[
            range(22, 24, 22, 30), // simple name
            range(22, 5, 22, 31),  // method invocation
            range(22, 5, 22, 32),  // expression statement
            range(20, 3, 26, 4),   // switch statement
            range(19, 6, 27, 3),   // block
            range(19, 2, 29, 3),   // try statement
            range(18, 85, 30, 2),  // block
            range(18, 1, 30, 2),   // method declaration
            type_decl_range(),
            comp_unit_range(),
        ],
    );
}
