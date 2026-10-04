//! Faithful ports of jdt.ls `PasteEventHandlerTest` (all 22 methods).
//! `AbstractSourceTestCase` uses the test VM's verbatim fakejdk/21/rtstubs.jar.
//! Through LSP its library replaces the JRE container, preserving the search
//! candidates of the upstream test VM. The public paste command exercises
//! the same missing-import path that the last five upstream tests call directly.

mod common;
use common::jdtls::{fixtures_dir, range, test_default_options, Workspace};
use serde_json::{json, Value};
use std::path::PathBuf;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    let mut options = test_default_options();
    for key in [
        "org.eclipse.jdt.core.compiler.source",
        "org.eclipse.jdt.core.compiler.compliance",
        "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
    ] {
        options.insert(key.into(), "21".into());
    }
    for (key, value) in [
        ("tabulation.char", "tab"),
        ("tabulation.size", "4"),
        ("lineSplit", "999"),
        ("blank_lines_before_field", "1"),
        ("blank_lines_before_method", "1"),
    ] {
        options.insert(
            format!("org.eclipse.jdt.core.formatter.{key}"),
            value.into(),
        );
    }
    let root = ws.new_empty_project(&options);
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::copy(
        fixtures_dir().join("fakejdk/21/rtstubs.jar"),
        root.join("lib/rtstubs.jar"),
    )
    .unwrap();
    std::fs::write(root.join(".classpath"), r#"<classpath><classpathentry kind="src" path="src"/><classpathentry kind="lib" path="lib/rtstubs.jar"/><classpathentry kind="output" path="bin"/></classpath>"#).unwrap();
    ws.create_cu(
        &root,
        "src",
        "p",
        "A.java",
        "package p;\n\npublic class A {\n}\n",
    );
    (ws, root)
}

fn paste(
    ws: &mut Workspace,
    uri: &str,
    selection: Value,
    text: &str,
    copied: Option<&str>,
) -> Value {
    // Upstream constructs a model CU synchronously. Establish the editor
    // working copy and finish the workspace jobs before invoking its handler.
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
    let params = json!({
        "location": { "uri": uri, "range": selection }, "text": text, "copiedDocumentUri": copied,
        "formattingOptions": { "tabSize": 4, "insertSpaces": false }
    });
    // vscode-java serializes this model: lsp4j's untyped Map is not a
    // JsonElement and JSONUtility.toLsp4jModel returns null for it.
    ws.request(
        "workspace/executeCommand",
        json!({ "command": "java.edit.handlePasteEvent", "arguments": [params.to_string()] }),
    )
}

#[test]
fn test_paste_into_empty_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 24, 2, 24), "aaa\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\n\" + //\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_with_error_on_line() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\npublic void test() {\n\tString asdf = \"\"\n}\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(3, 16, 3, 16), "aaa\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\n\" + //\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_windows_newline_in_copied_text() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 24, 2, 24), "aaa\r\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\r\\n\" + //\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_windows_newline_in_class_file() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\r\npublic class A {\r\n\tprivate String asdf = \"\";\r\n}\r\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 24, 2, 24), "aaa\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\n\" + //\r\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_before_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"asdf\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 23, 2, 23), "aaa\naaa", None);
    assert!(actual.is_null(), "{actual:#}");
}

#[test]
fn test_paste_after_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"asdf\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 29, 2, 29), "aaa\naaa", None);
    assert!(actual.is_null(), "{actual:#}");
}

#[test]
fn test_paste_beginning_of_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"asdf\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 24, 2, 24), "aaa\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\n\" + //\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_end_of_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"asdf\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 28, 2, 28), "aaa\naaa", None);
    assert_eq!(
        actual,
        json!({ "insertText": "aaa\\n\" + //\n\t\t\t\"aaa" })
    );
}

#[test]
fn test_paste_into_string_block() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String asdf = \"\"\"asdf\"\"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 30, 2, 30), "aaa\naaa", None);
    assert!(actual.is_null(), "{actual:#}");
}

#[test]
fn test_paste_chinese_characters_into_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String hello = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 25, 2, 25), "你好", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(actual["insertText"], "你好");
}

#[test]
fn test_paste_unicode_characters_into_string_literal() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String hello = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 25, 2, 25), "\\u4F60\\u597D", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(actual["insertText"], "\\\\u4F60\\\\u597D");
}

#[test]
fn test_paste_literal_backslash_n() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String nl = \"555\\n555\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 30, 2, 30), "\\n", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(actual["insertText"], "\\\\n");
}

#[test]
fn test_paste_literal_backslash_n_with_actual_newlines() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String foo = \"a\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 24, 2, 24), "b\\nc\nd\ne", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(
        actual["insertText"],
        "b\\\\nc\\n\" + //\n\t\t\t\"d\\n\" + //\n\t\t\t\"e"
    );
}

#[test]
fn test_paste_only_literal_backslash_n() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String str = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 23, 2, 23), "\\n", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(actual["insertText"], "\\\\n");
}

#[test]
fn test_paste_multiline_with_literal_backslash_n() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String text = \"\";\n}\n",
    );
    let actual = paste(
        &mut ws,
        &cu,
        range(2, 24, 2, 24),
        "hello\\nworld\nmore\ntext",
        None,
    );
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(
        actual["insertText"],
        "hello\\\\nworld\\n\" + //\n\t\t\t\"more\\n\" + //\n\t\t\t\"text"
    );
}

#[test]
fn test_paste_windows_newline_with_literal_backslash_n() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String str = \"\";\n}\n",
    );
    let actual = paste(&mut ws, &cu, range(2, 23, 2, 23), "test\\n\r\nnext", None);
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(actual["insertText"], "test\\\\n\\r\\n\" + //\n\t\t\t\"next");
}

#[test]
fn test_paste_only_actual_newlines() {
    let (mut ws, root) = setup();
    let cu = ws.create_cu(
        &root,
        "src",
        "test",
        "A.java",
        "package test;\npublic class A {\n\tprivate String multiline = \"\";\n}\n",
    );
    let actual = paste(
        &mut ws,
        &cu,
        range(2, 29, 2, 29),
        "line1\nline2\nline3",
        None,
    );
    assert!(actual.is_object(), "{actual:#}");
    assert_eq!(
        actual["insertText"],
        "line1\\n\" + //\n\t\t\t\"line2\\n\" + //\n\t\t\t\"line3"
    );
}

fn import_edit(ambiguous: bool, resolve: bool, enabled: Option<bool>) -> Value {
    let (mut ws, root) = setup();
    if let Some(value) = enabled {
        ws.settings = json!({ "java": { "updateImportsOnPaste": { "enabled": value } } });
    }
    let eol = if enabled.is_some() { "\n" } else { "\r\n" };
    let source = format!(
        "package p;{eol}{eol}public class B {{{eol}}}{}",
        if enabled.is_some() { "\n" } else { "" }
    );
    let cu = ws.create_cu(&root, "src", "p", "B.java", &source);
    if ambiguous {
        ws.create_cu(
            &root,
            "src",
            "test",
            "List.java",
            "package test;\r\n\r\npublic class List {\r\n}",
        );
    }
    let copied = resolve.then(|| ws.create_cu(&root, "src", "p", "C.java", "package p;\r\n\r\nimport java.util.List;\r\nimport java.util.Set;\r\npublic class C {\r\n\tpublic List<String> b;\r\n\tpublic Set<String> c;\r\n}"));
    let text = format!("\tpublic List<String> b;{eol}\tpublic Set<String> c;{eol}");
    let result = paste(&mut ws, &cu, range(3, 0, 3, 0), &text, copied.as_deref());
    if enabled == Some(false) {
        assert!(result.is_null(), "{result:#}");
        return result;
    }
    assert!(result.is_object(), "{result:#}");
    let changes = result["additionalEdit"]["changes"][&cu]
        .as_array()
        .expect("unit changes");
    assert_eq!(1, changes.len());
    let list = if ambiguous && !resolve {
        String::new()
    } else {
        format!("import java.util.List;{eol}")
    };
    assert_eq!(
        changes[0]["newText"],
        format!("{eol}{eol}{list}import java.util.Set;{eol}{eol}")
    );
    assert_eq!(changes[0]["range"], range(0, 10, 2, 0));
    result
}

#[test]
fn test_get_add_imports_workspace_edit() {
    import_edit(false, false, None);
}

#[test]
fn test_get_add_imports_ambigous_workspace_edit() {
    import_edit(true, false, None);
}

#[test]
fn test_get_add_imports_resolve_ambigous_workspace_edit() {
    import_edit(true, true, None);
}

#[test]
fn test_get_add_imports_workspace_edit_with_preference_disabled() {
    import_edit(false, false, Some(false));
}

#[test]
fn test_get_add_imports_workspace_edit_with_preference_enabled() {
    import_edit(false, false, Some(true));
}
