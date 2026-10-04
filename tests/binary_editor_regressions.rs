//! Binary editor behavior verified against the Eclipse JDT LS oracle.
mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

fn workspace(attached: bool, hierarchical: bool) -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/source-attachment"]);
    ws.capabilities["textDocument"]["documentSymbol"]["hierarchicalDocumentSymbolSupport"] =
        json!(hierarchical);
    ws.settings = json!({ "java": { "implementationCodeLens": "types", "referencesCodeLens": { "enabled": true } } });
    if attached {
        let cp = ws.project_root("source-attachment").join(".classpath");
        let text = std::fs::read_to_string(&cp).unwrap().replace(
            "path=\"foo.jar\"",
            "path=\"foo.jar\" sourcepath=\"foo-sources.jar\"",
        );
        std::fs::write(cp, text).unwrap();
    }
    ws
}

fn request(ws: &mut Workspace, method: &str, uri: &str) -> Value {
    ws.request(method, json!({ "textDocument": { "uri": uri } }))
}

fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
    json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } })
}

fn outline(attached: bool) -> Value {
    let (
        package_range,
        package_selection,
        type_range,
        type_selection,
        constructor_range,
        constructor_selection,
        method_range,
        method_selection,
    ) = if attached {
        (
            range(0, 0, 0, 6),
            range(0, 0, 0, 6),
            range(2, 0, 10, 1),
            range(2, 13, 2, 16),
            range(2, 0, 10, 1),
            range(2, 13, 2, 16),
            range(3, 4, 9, 5),
            range(3, 22, 3, 25),
        )
    } else {
        (
            range(1, 0, 1, 12),
            range(1, 8, 1, 11),
            range(3, 0, 16, 1),
            range(3, 13, 3, 16),
            range(4, 3, 5, 4),
            range(4, 10, 4, 13),
            range(7, 3, 15, 4),
            range(7, 21, 7, 24),
        )
    };
    let mut package = json!({ "name": "foo", "detail": "", "kind": 4, "range": package_range, "selectionRange": package_selection });
    if attached {
        package["children"] = json!([]);
    }
    json!([package, {
        "name": "bar", "detail": "", "kind": 5, "range": type_range, "selectionRange": type_selection,
        "children": [
            { "name": "bar()", "detail": "", "kind": 9, "range": constructor_range, "selectionRange": constructor_selection },
            { "name": "add(int...)", "detail": " : int", "kind": 6, "range": method_range, "selectionRange": method_selection }
        ]
    }])
}

fn assert_lenses(ws: &mut Workspace, uri: &str, attached: bool) -> Value {
    let lenses = request(ws, "textDocument/codeLens", uri);
    let declarations = if attached {
        vec![
            (range(3, 22, 3, 25), "references"),
            (range(2, 13, 2, 16), "references"),
            (range(2, 13, 2, 16), "implementations"),
        ]
    } else {
        vec![
            (range(0, 0, 0, 0), "references"),
            (range(0, 0, 0, 0), "implementations"),
        ]
    };
    let expected: Vec<Value> = declarations
        .into_iter()
        .map(|(range, kind)| {
            json!({
                "data": [uri, range["start"], kind], "range": range
            })
        })
        .collect();
    assert_eq!(lenses, json!(expected));
    lenses
}

#[test]
fn attached_class_file_editor() {
    let mut ws = workspace(true, true);
    let uri = ws.class_file_uri("source-attachment", "foo.bar");
    assert_eq!(
        request(&mut ws, "textDocument/documentSymbol", &uri),
        outline(true)
    );
    let lenses = assert_lenses(&mut ws, &uri, true);
    let resolved = ws.request("codeLens/resolve", lenses[0].clone());
    assert_eq!(
        resolved["command"],
        json!({
            "command": "java.show.references", "title": "1 reference",
            "arguments": [uri, { "line": 3, "character": 22 }, [{ "uri": ws.class_uri("source-attachment", "Test"), "range": range(5, 16, 5, 28) }]]
        })
    );
    let selection = ws.request(
        "textDocument/selectionRange",
        json!({ "textDocument": { "uri": uri }, "positions": [{ "line": 3, "character": 22 }] }),
    );
    assert_eq!(
        selection,
        json!([{
            "range": range(3, 22, 3, 25), "parent": { "range": range(3, 4, 9, 5), "parent": {
                "range": range(2, 0, 10, 1), "parent": { "range": range(0, 0, 11, 0) }
            }}
        }])
    );
}

#[test]
fn decompiled_class_file_editor() {
    let mut ws = workspace(false, true);
    let uri = ws.class_file_uri("source-attachment", "foo.bar");
    let text = ws.request("java/classFileContents", json!({ "uri": uri }));
    assert!(text
        .as_str()
        .unwrap()
        .starts_with("// Source code is decompiled"));
    assert_eq!(
        request(&mut ws, "textDocument/documentSymbol", &uri),
        outline(false)
    );
    assert_lenses(&mut ws, &uri, false);
    assert_eq!(
        request(&mut ws, "textDocument/semanticTokens/full", &uri),
        json!({ "data": [] })
    );
    assert_eq!(
        ws.request(
            "textDocument/selectionRange",
            json!({ "textDocument": { "uri": uri }, "positions": [{ "line": 7, "character": 21 }] })
        ),
        json!([])
    );
}

fn flat_outline(attached: bool) {
    let mut ws = workspace(attached, false);
    let uri = ws.class_file_uri("source-attachment", "foo.bar");
    let ty_range = if attached {
        range(2, 13, 2, 16)
    } else {
        range(0, 0, 0, 0)
    };
    let method_range = if attached {
        range(3, 22, 3, 25)
    } else {
        range(0, 0, 0, 0)
    };
    assert_eq!(
        request(&mut ws, "textDocument/documentSymbol", &uri),
        json!([
            { "name": "bar()", "kind": 9, "containerName": "bar", "location": { "uri": uri, "range": ty_range } },
            { "name": "add(int...)", "kind": 6, "containerName": "bar", "location": { "uri": uri, "range": method_range } },
            { "name": "bar", "kind": 5, "containerName": "bar.class", "location": { "uri": uri, "range": ty_range } }
        ])
    );
}

#[test]
fn attached_flat_outline() {
    flat_outline(true);
}

#[test]
fn decompiled_flat_outline() {
    flat_outline(false);
}

#[test]
fn binary_call_hierarchy_without_workspace_sources() {
    let mut ws = workspace(true, true);
    std::fs::remove_dir_all(ws.project_root("source-attachment").join("src")).unwrap();
    let uri = ws.class_file_uri("source-attachment", "foo.bar");
    let items = ws.request(
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": uri }, "position": { "line": 3, "character": 22 }
        }),
    );
    assert_eq!(
        items,
        json!([{
            "name": "add(int...) : int", "kind": 6, "detail": "foo.bar", "uri": uri,
            "range": range(3, 4, 9, 5), "selectionRange": range(3, 22, 3, 25)
        }])
    );
    assert_eq!(
        ws.request("callHierarchy/outgoingCalls", json!({ "item": items[0] })),
        json!([])
    );
}
