//! Port of `org.eclipse.jdt.ls.core.internal.handlers.FileEventHandlerTest`
//! over `workspace/willRenameFiles` (`FileEventHandler.handleWillRenameFiles`).
//! The harness client supports resource operations
//! (`clientPreferences.isResourceOperationSupported()`).

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use tower_lsp::lsp_types::Url;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&Default::default());
    (ws, root)
}

/// `sourceFolder.createPackageFragment(name, ...)`.
fn create_package(root: &std::path::Path, name: &str) -> PathBuf {
    let mut dir = root.join("src");
    for seg in name.split('.') {
        dir.push(seg);
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `JDTUtils.getFileURI(folder)`: `file:///…` without a trailing slash.
fn folder_uri(dir: &std::path::Path) -> String {
    Url::from_file_path(dir).unwrap().to_string()
}

fn will_rename(ws: &mut Workspace, files: &[(&str, &str)]) -> Value {
    let files: Vec<Value> = files.iter().map(|(o, n)| json!({ "oldUri": o, "newUri": n })).collect();
    ws.request("workspace/willRenameFiles", json!({ "files": files }))
}

fn document_changes(edit: &Value) -> Vec<Value> {
    edit["documentChanges"].as_array().cloned().unwrap_or_else(|| panic!("no documentChanges: {edit}"))
}

/// `documentChanges.get(i).isLeft()`: a `TextDocumentEdit`.
fn is_left(change: &Value) -> bool {
    change.get("textDocument").is_some() && change.get("edits").is_some()
}

fn edits(change: &Value) -> Vec<Value> {
    change["edits"].as_array().cloned().unwrap()
}

#[test]
fn test_rename_files() {
    let (mut ws, root) = setup();
    create_package(&root, "test1");
    let builder_a = "package test1;\npublic class ObjectA {\n\tpublic void foo() {\n\t}\n}\n";
    let cu_a = ws.create_cu(&root, "src", "test1", "ObjectA.java", builder_a);
    let builder_b = "package test1;\npublic class B {\n\tpublic void foo() {\n\t\tObjectA a = new ObjectA();\n\t\ta.foo();\n\t}\n}\n";
    let cu_b = ws.create_cu(&root, "src", "test1", "B.java", builder_b);

    let uri_a = cu_a.clone();
    let new_uri_a = uri_a.replace("ObjectA", "ObjectA1");
    let edit = will_rename(&mut ws, &[(&uri_a, &new_uri_a)]);
    assert!(!edit.is_null());
    let changes = document_changes(&edit);
    assert_eq!(2, changes.len(), "{edit:#}");

    assert!(is_left(&changes[0]));
    assert_eq!(changes[0]["textDocument"]["uri"], cu_b);
    assert_eq!(
        apply_edits(builder_b, &edits(&changes[0])),
        "package test1;\npublic class B {\n\tpublic void foo() {\n\t\tObjectA1 a = new ObjectA1();\n\t\ta.foo();\n\t}\n}\n"
    );

    assert!(is_left(&changes[1]));
    assert_eq!(changes[1]["textDocument"]["uri"], uri_a);
    assert_eq!(apply_edits(builder_a, &edits(&changes[1])), "package test1;\npublic class ObjectA1 {\n\tpublic void foo() {\n\t}\n}\n");
}

#[test]
fn test_rename_files_refactoring_exists() {
    let (mut ws, root) = setup();
    create_package(&root, "test1");
    let builder_a = "package test1;\npublic class ObjectA1 {\n\tpublic void foo() {\n\t}\n}\n";
    let cu_a = ws.create_cu(&root, "src", "test1", "ObjectA.java", builder_a);

    let uri_a = cu_a.clone();
    let new_uri_a = uri_a.replace("ObjectA", "ObjectA1");
    let edit = will_rename(&mut ws, &[(&uri_a, &new_uri_a)]);
    assert!(edit.is_null(), "{edit}");
}

const CODE_A: &str = "package parent.pack1;\nimport parent.pack2.B;\npublic class A {\n\tpublic void foo() {\n\t\tB b = new B();\n\t\tb.foo();\n\t}\n}\n";
const CODE_B: &str = "package parent.pack2;\npublic class B {\n\tpublic B() {}\n\tpublic void foo() {}\n}\n";

// Test renaming package from "parent.pack2" to "parent.newpack2"
#[test]
fn test_rename_package() {
    let (mut ws, root) = setup();
    create_package(&root, "parent.pack1");
    let pack2 = create_package(&root, "parent.pack2");
    let cu_a = ws.create_cu(&root, "src", "parent.pack1", "A.java", CODE_A);
    let cu_b = ws.create_cu(&root, "src", "parent.pack2", "B.java", CODE_B);

    let pack2_uri = folder_uri(&pack2);
    let new_pack2_uri = pack2_uri.replace("pack2", "newpack2");
    let edit = will_rename(&mut ws, &[(&pack2_uri, &new_pack2_uri)]);
    assert!(!edit.is_null());
    let document_changes = document_changes(&edit);
    assert_eq!(2, document_changes.len(), "{edit:#}");

    assert!(is_left(&document_changes[0]));
    assert_eq!(document_changes[0]["textDocument"]["uri"], cu_a);
    assert_eq!(
        apply_edits(CODE_A, &edits(&document_changes[0])),
        "package parent.pack1;\nimport parent.newpack2.B;\npublic class A {\n\tpublic void foo() {\n\t\tB b = new B();\n\t\tb.foo();\n\t}\n}\n"
    );

    assert!(is_left(&document_changes[1]));
    assert_eq!(document_changes[1]["textDocument"]["uri"], cu_b);
    assert_eq!(
        apply_edits(CODE_B, &edits(&document_changes[1])),
        "package parent.newpack2;\npublic class B {\n\tpublic B() {}\n\tpublic void foo() {}\n}\n"
    );
}

// Test renaming package from "parent.pack2" to "newparent.newpack2"
#[test]
fn test_rename_package2() {
    let (mut ws, root) = setup();
    create_package(&root, "parent.pack1");
    let pack2 = create_package(&root, "parent.pack2");
    let cu_a = ws.create_cu(&root, "src", "parent.pack1", "A.java", CODE_A);
    let cu_b = ws.create_cu(&root, "src", "parent.pack2", "B.java", CODE_B);

    let pack2_uri = folder_uri(&pack2);
    let new_pack2_uri = pack2_uri.replace("pack2", "newpack2").replace("parent", "newparent");
    let edit = will_rename(&mut ws, &[(&pack2_uri, &new_pack2_uri)]);
    assert!(!edit.is_null());
    let document_changes = document_changes(&edit);
    assert_eq!(2, document_changes.len(), "{edit:#}");

    assert!(is_left(&document_changes[0]));
    assert_eq!(document_changes[0]["textDocument"]["uri"], cu_a);
    assert_eq!(
        apply_edits(CODE_A, &edits(&document_changes[0])),
        "package parent.pack1;\nimport newparent.newpack2.B;\npublic class A {\n\tpublic void foo() {\n\t\tB b = new B();\n\t\tb.foo();\n\t}\n}\n"
    );

    assert!(is_left(&document_changes[1]));
    assert_eq!(document_changes[1]["textDocument"]["uri"], cu_b);
    assert_eq!(
        apply_edits(CODE_B, &edits(&document_changes[1])),
        "package newparent.newpack2;\npublic class B {\n\tpublic B() {}\n\tpublic void foo() {}\n}\n"
    );
}

#[test]
fn test_rename_sub_package() {
    let (mut ws, root) = setup();
    let parent_pack = create_package(&root, "parent");
    create_package(&root, "parent.pack1");
    create_package(&root, "parent.pack2");
    let cu_a = ws.create_cu(&root, "src", "parent.pack1", "A.java", CODE_A);
    let cu_b = ws.create_cu(&root, "src", "parent.pack2", "B.java", CODE_B);

    let parent_pack_uri = folder_uri(&parent_pack);
    let new_parent_pack_uri = parent_pack_uri.replace("parent", "newparent");
    let edit = will_rename(&mut ws, &[(&parent_pack_uri, &new_parent_pack_uri)]);
    assert!(!edit.is_null());
    let document_changes = document_changes(&edit);
    assert_eq!(3, document_changes.len(), "{edit:#}");

    assert!(is_left(&document_changes[0]));
    assert_eq!(document_changes[0]["textDocument"]["uri"], cu_a);
    assert!(is_left(&document_changes[1]));
    assert_eq!(document_changes[1]["textDocument"]["uri"], cu_a);
    let mut all = edits(&document_changes[0]);
    all.extend(edits(&document_changes[1]));
    assert_eq!(
        apply_edits(CODE_A, &all),
        "package newparent.pack1;\nimport newparent.pack2.B;\npublic class A {\n\tpublic void foo() {\n\t\tB b = new B();\n\t\tb.foo();\n\t}\n}\n"
    );

    assert!(is_left(&document_changes[2]));
    assert_eq!(document_changes[2]["textDocument"]["uri"], cu_b);
    assert_eq!(
        apply_edits(CODE_B, &edits(&document_changes[2])),
        "package newparent.pack2;\npublic class B {\n\tpublic B() {}\n\tpublic void foo() {}\n}\n"
    );
}

const UNIT_A: &str = "package jdtls.test1;\r\n\r\npublic class A {\r\n\tprivate B b = new B();\r\n}";
const UNIT_B: &str = "package jdtls.test1;\r\n\r\npublic class B {\r\n}";
const UNIT_C: &str = "package jdtls.test1;\r\n\r\npublic class C {\r\n\tprivate B b = new B();\r\n}";

#[test]
fn test_move_multi_files() {
    let (mut ws, root) = setup();
    create_package(&root, "jdtls.test1");
    let unit_a = ws.create_cu(&root, "src", "jdtls.test1", "A.java", UNIT_A);
    let unit_b = ws.create_cu(&root, "src", "jdtls.test1", "B.java", UNIT_B);
    ws.create_cu(&root, "src", "jdtls.test1", "C.java", UNIT_C);
    create_package(&root, "jdtls.test2");

    let new_uri_a = unit_a.replace("test1", "test2");
    let new_uri_b = unit_b.replace("test1", "test2");
    let edit = will_rename(&mut ws, &[(&unit_a, &new_uri_a), (&unit_b, &new_uri_b)]);

    assert!(!edit.is_null());
    let changes = document_changes(&edit);
    assert_eq!(3, changes.len(), "{edit:#}");

    let expected = "package jdtls.test1;\r\n\r\nimport jdtls.test2.B;\r\n\r\npublic class C {\r\n\tprivate B b = new B();\r\n}";
    assert_eq!(expected, apply_edits(UNIT_C, &edits(&changes[0])));

    let expected = "package jdtls.test2;\r\n\r\npublic class B {\r\n}";
    assert_eq!(expected, apply_edits(UNIT_B, &edits(&changes[1])));

    let expected = "package jdtls.test2;\r\n\r\npublic class A {\r\n\tprivate B b = new B();\r\n}";
    assert_eq!(expected, apply_edits(UNIT_A, &edits(&changes[2])));
}

#[test]
fn test_move_multi_files_different_destination() {
    let (mut ws, root) = setup();
    create_package(&root, "jdtls.test1");
    let unit_a = ws.create_cu(&root, "src", "jdtls.test1", "A.java", UNIT_A);
    let unit_b = ws.create_cu(&root, "src", "jdtls.test1", "B.java", UNIT_B);
    ws.create_cu(&root, "src", "jdtls.test1", "C.java", UNIT_C);
    create_package(&root, "jdtls.test2");
    create_package(&root, "jdtls.test3");

    let new_uri_a = unit_a.replace("test1", "test2");
    let new_uri_b = unit_b.replace("test1", "test3");
    let edit = will_rename(&mut ws, &[(&unit_a, &new_uri_a), (&unit_b, &new_uri_b)]);
    assert!(edit.is_null(), "{edit}");
}

#[test]
fn test_move_non_classpath_file() {
    let (mut ws, root) = setup();
    create_package(&root, "jdtls.test1");
    let file = root.join("Bar.java");
    let contents = "public class Bar {\r\n}";
    std::fs::write(&file, contents).unwrap();
    ws.client();
    ws.notify_file_changed(&file, 1);

    let uri = Url::from_file_path(&file).unwrap().to_string();
    let new_uri = uri.replace("Bar.java", "src/jdtls/test1/Bar.java");
    let edit = will_rename(&mut ws, &[(&uri, &new_uri)]);

    assert!(!edit.is_null());
    let changes = document_changes(&edit);
    assert_eq!(1, changes.len(), "{edit:#}");

    let expected = "package jdtls.test1;\r\npublic class Bar {\r\n}";
    assert_eq!(expected, apply_edits(contents, &edits(&changes[0])));
}
