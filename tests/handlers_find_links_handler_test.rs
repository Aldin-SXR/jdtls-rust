//! Port of `org.eclipse.jdt.ls.core.internal.handlers.FindLinksHandlerTest`.
//!
//! `FindLinksHandler.findLinks(type, position)` is the `java/findLinks` request.

mod common;

use common::jdtls::*;
use serde_json::{json, Value};
use std::path::PathBuf;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    let project = ws.new_empty_project(&test_default_options());
    (ws, project)
}

fn find_links(ws: &mut Workspace, link_type: &str, uri: &str, line: u32, character: u32) -> Value {
    ws.request(
        "java/findLinks",
        json!({
            "type": link_type,
            "position": { "textDocument": { "uri": uri }, "position": { "line": line, "character": character } },
        }),
    )
}

#[test]
fn test_find_super_method() {
    let (mut ws, project) = setup();
    let unit_a = ws.create_cu(
        &project,
        "src",
        "test1",
        "A.java",
        "package test1;\n\npublic class A {\n\tpublic void run() {\n\t}\n}",
    );
    let unit_b = ws.create_cu(
        &project,
        "src",
        "test1",
        "B.java",
        "package test1;\n\npublic class B extends A {\n\tpublic void run() {\n\t}\n}",
    );

    let response = find_links(&mut ws, "superImplementation", &unit_b, 3, 14);
    let response = response.as_array().expect("links");
    assert_eq!(1, response.len());
    let location = &response[0];
    assert_eq!("test1.A.run", location["displayName"]);
    assert_eq!("method", location["kind"]);
    assert_eq!(unit_a, location["uri"].as_str().unwrap());
    let range = &location["range"];
    assert_eq!(3, range["start"]["line"]);
    assert_eq!(13, range["start"]["character"]);
    assert_eq!(3, range["end"]["line"]);
    assert_eq!(16, range["end"]["character"]);
}

#[test]
fn test_find_nearest_super_method() {
    let (mut ws, project) = setup();
    let unit_a = ws.create_cu(
        &project,
        "src",
        "test1",
        "A.java",
        "package test1;\n\npublic class A {\n\tpublic void run() {\n\t}\n}",
    );
    ws.create_cu(&project, "src", "test1", "B.java", "package test1;\n\npublic class B extends A {\n}");
    let unit_c = ws.create_cu(
        &project,
        "src",
        "test1",
        "C.java",
        "package test1;\n\npublic class C extends B {\n\tpublic void run() {\n\t}\n}",
    );

    let response = find_links(&mut ws, "superImplementation", &unit_c, 3, 14);
    let response = response.as_array().expect("links");
    assert_eq!(1, response.len());
    let location = &response[0];
    assert_eq!("test1.A.run", location["displayName"]);
    assert_eq!("method", location["kind"]);
    assert_eq!(unit_a, location["uri"].as_str().unwrap());
    let range = &location["range"];
    assert_eq!(3, range["start"]["line"]);
    assert_eq!(13, range["start"]["character"]);
    assert_eq!(3, range["end"]["line"]);
    assert_eq!(16, range["end"]["character"]);
}

#[test]
fn test_no_super_method() {
    let (mut ws, project) = setup();
    let unit_a = ws.create_cu(
        &project,
        "src",
        "test1",
        "A.java",
        "package test1;\n\npublic class A {\n\tpublic void run() {\n\t}\n}",
    );

    let response = find_links(&mut ws, "superImplementation", &unit_a, 3, 14);
    assert!(response.is_null() || response.as_array().is_some_and(Vec::is_empty));
}
