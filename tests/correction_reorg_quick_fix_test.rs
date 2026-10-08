//! Port of `org.eclipse.jdt.ls.core.internal.correction.ReorgQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{get_title, Expected, QuickFixTest};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] = json!(["create", "rename", "delete"]);
    let root = t.ws.new_empty_project(&test_default_options());
    (t, root)
}

fn find_action<'a>(actions: &'a [Value], title: &str) -> Option<&'a Value> {
    actions.iter().find(|a| get_title(a) == title)
}

fn assert_rename_file_operation(action: &Value, new_uri: &str) {
    let changes = action["edit"]["documentChanges"].as_array().expect("documentChanges");
    assert_eq!(1, changes.len());
    assert_eq!("rename", changes[0]["kind"]);
    assert_eq!(new_uri, changes[0]["newUri"]);
}

fn source_uri(root: &Path, relative: &str) -> String {
    url::Url::from_file_path(root.join("src").join(relative)).unwrap().to_string()
}

#[test]
fn test_unused_imports() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove unused import", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Organize imports", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_remove_all_unused_imports() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove all unused imports", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_unused_imports_in_default_package() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove unused import", &buf);
    let mut buf = String::new();
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Organize imports", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_unused_import_on_demand() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("import java.net.*;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str(" Vector v;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str(" Vector v;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove unused import", &buf);
    let e2 = Expected::new("Organize imports", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_colliding_imports() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.security.Permission;\n");
    buf.push_str("import java.security.acl.Permission;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str(" Permission p;\n");
    buf.push_str(" Vector v;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.security.Permission;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str(" Permission p;\n");
    buf.push_str(" Vector v;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove unused import", &buf);
    let e2 = Expected::new("Organize imports", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_wrong_package_statement() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let actions = t.evaluate_code_actions(&cu);
    let action = find_action(&actions, "Correct package declaration").expect("Correct package declaration");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    assert_eq!(buf, t.evaluate_code_action_command(action));
    let action = find_action(&actions, "Move 'E.java' to package 'test2'").expect("Move 'E.java' to package 'test2'");
    assert_rename_file_operation(action, &source_uri(&root, "test2/E.java"));
}

#[test]
fn test_wrong_package_statement_in_enum() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("public enum E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let actions = t.evaluate_code_actions(&cu);
    let action = find_action(&actions, "Correct package declaration").expect("Correct package declaration");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public enum E {\n");
    buf.push_str("}\n");
    assert_eq!(buf, t.evaluate_code_action_command(action));
    let action = find_action(&actions, "Move 'E.java' to package 'test2'").expect("Move 'E.java' to package 'test2'");
    assert_rename_file_operation(action, &source_uri(&root, "test2/E.java"));
}

#[test]
fn test_wrong_package_statement_from_default() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "", "E.java", &buf);
    let actions = t.evaluate_code_actions(&cu);
    let action = find_action(&actions, "Correct package declaration").expect("Correct package declaration");
    let mut buf = String::new();
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    assert_eq!(buf, t.evaluate_code_action_command(action));
    let action = find_action(&actions, "Move 'E.java' to package 'test2'").expect("Move 'E.java' to package 'test2'");
    assert_rename_file_operation(action, &source_uri(&root, "test2/E.java"));
}

#[test]
fn test_wrong_default_package_statement() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "E.java", &buf);
    let actions = t.evaluate_code_actions(&cu);
    let action = find_action(&actions, "Correct package declaration").expect("Correct package declaration");
    let mut buf = String::new();
    buf.push_str("package test2;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    assert_eq!(buf, t.evaluate_code_action_command(action));
    let action = find_action(&actions, "Move 'E.java' to the default package").expect("Move 'E.java' to the default package");
    assert_rename_file_operation(action, &source_uri(&root, "E.java"));
}

#[test]
fn test_wrong_type_name() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "X.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'X'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_wrong_type_name_bug180330() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package p;\n");
    buf.push_str("public class \\u0042 {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "p", "C.java", &buf);
    let mut buf = String::new();
    buf.push_str("package p;\n");
    buf.push_str("public class C {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'C'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_wrong_type_name_but_colliding() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'E'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_wrong_type_name_with_constructor() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str(" public X() {\n");
    buf.push_str(" X other;\n");
    buf.push_str(" }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str(" public E() {\n");
    buf.push_str(" E other;\n");
    buf.push_str(" }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'E'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_wrong_type_name_in_enum() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public enum X {\n");
    buf.push_str(" A;\n");
    buf.push_str(" X() {\n");
    buf.push_str(" }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class X {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public enum E {\n");
    buf.push_str(" A;\n");
    buf.push_str(" E() {\n");
    buf.push_str(" }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'E'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_wrong_type_name_in_annot() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public @interface X {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public @interface X {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "X.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public @interface E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Rename type to 'E'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_no_java_apply_workspace_edit_command() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Vector;\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let actions = t.evaluate_code_actions(&cu);
    assert!(
        !actions.iter().any(|a| a["command"] == "java.apply.workspaceEdit"),
        "Should not return legacy java.apply.workspaceEdit Command"
    );
    assert!(
        !actions.iter().any(|a| a["command"]["command"] == "java.apply.workspaceEdit"),
        "Should not embed legacy java.apply.workspaceEdit as CodeAction.command"
    );
    let organize = actions.iter().find(|a| a["title"] == "Organize imports").expect("Expected 'Organize imports' code action");
    assert!(!organize["edit"].is_null(), "'Organize imports' should carry WorkspaceEdit");
}

#[test]
fn test_bump_required_compliance14() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E");
    buf.push_str("\tpublic void foo(int a) {\n");
    buf.push_str("\t\tswitch (a) {\n");
    buf.push_str("\t\t\tcase 1,2 -> System.out.println(\"abc\");\n");
    buf.push_str("\t\t\tdefault -> System.out.println(\"def\");\n");
    buf.push_str("\t\t}\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    t.assert_code_action_exists(&cu, "Change project compiler compliance to 14");
}

#[test]
fn test_bump_required_compliance15() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E");
    buf.push_str("\tpublic void foo(int a) {\n");
    buf.push_str("\t\tString s = \"\"\"\n");
    buf.push_str("\t\t\tabcdefg\n");
    buf.push_str("\t\t\t\"\"\";\n");
    buf.push_str("\t\t}\n");
    buf.push_str("\t}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    t.assert_code_action_exists(&cu, "Change project compiler compliance to 15");
}

