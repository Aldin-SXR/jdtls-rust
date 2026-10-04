//! Public-command regressions beyond PasteEventHandlerTest's 22 ports.
mod common;
use common::jdtls::{apply_edits, range, test_default_options, Workspace};
use serde_json::{json, Value};
use std::path::Path;

fn paste(
    ws: &mut Workspace,
    uri: &str,
    selection: Value,
    text: &str,
    copied: Option<&str>,
    spaces: bool,
) -> Value {
    let params = json!({ "location": { "uri": uri, "range": selection }, "text": text,
        "copiedDocumentUri": copied, "formattingOptions": { "tabSize": 2, "insertSpaces": spaces } });
    ws.request(
        "workspace/executeCommand",
        json!({ "command": "java.edit.handlePasteEvent", "arguments": [params.to_string()] }),
    )
}
fn resolve(ws: &mut Workspace, folder: &Path, text: &str) -> Value {
    ws.request("workspace/executeCommand", json!({ "command": "java.project.resolveText", "arguments": [folder.to_str().unwrap(), text] }))
}
fn position(source: &str, byte: usize) -> Value {
    let before = &source[..byte];
    json!({ "line": before.matches('\n').count(), "character": before.rsplit('\n').next().unwrap().encode_utf16().count() })
}
fn selection(source: &str, start: usize, end: usize) -> Value {
    json!({ "start": position(source, start), "end": position(source, end) })
}
fn created(ws: &mut Workspace, source: &str) -> String {
    let root = ws.new_empty_project(&test_default_options());
    ws.create_cu(&root, "src", "p", "A.java", source)
}

#[test]
fn spaces_utf16_controls_and_encoded_parameters() {
    let mut ws = Workspace::new();
    let source = "package p;\npublic class A {\n  String s = /* 😀 */ \"ab\";\n}\n";
    let uri = created(&mut ws, source);
    let start = source.find("ab").unwrap();
    let actual = paste(
        &mut ws,
        &uri,
        selection(source, start, start + 2),
        "\"\\\t\u{8}\u{c}\r你好😀",
        None,
        true,
    );
    assert_eq!(
        actual,
        json!({ "insertText": "\\\"\\\\\\t\\b\\f\\r\" + //\n      \"你好😀" })
    );
    // Asking for an edit leaves the authoritative buffer and disk unchanged.
    assert_eq!(ws.read(&uri), source);
    let again = paste(
        &mut ws,
        &uri,
        selection(source, start, start),
        "x\ny",
        None,
        true,
    );
    assert_eq!(again, json!({ "insertText": "x\\n\" + //\n      \"y" }));
}

#[test]
fn selection_edges_comments_and_disabled_import_preference() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "updateImportsOnPaste": { "enabled": false } } });
    let source = "package p;\npublic class A {\n  // \"comment\"\n  String s = \"text\";\n}\n";
    let uri = created(&mut ws, source);
    let quote = source.find("\"text").unwrap();
    for (start, end) in [
        (quote, quote),
        (quote + 1, quote + 6),
        (
            source.find("comment").unwrap(),
            source.find("comment").unwrap(),
        ),
    ] {
        assert!(paste(
            &mut ws,
            &uri,
            selection(source, start, end),
            "x\ny",
            None,
            false
        )
        .is_null());
    }
    assert_eq!(
        paste(
            &mut ws,
            &uri,
            selection(source, quote + 1, quote + 1),
            "x\ny",
            Some(&uri),
            false
        ),
        json!({ "insertText": "x\\n\" + //\n  \t\t\"y" })
    );
}

#[test]
fn retains_imports_and_only_imports_type_references() {
    let mut ws = Workspace::new();
    let source = "package p;\n\nimport java.util.Set;\n\npublic class A {\n}\n";
    let uri = created(&mut ws, source);
    let text = "  java.util.Map<String, String> map;\n  ArrayList<String> list;\n  String fake = \"HashMap\"; // TreeSet\n";
    let actual = paste(&mut ws, &uri, range(5, 0, 5, 0), text, None, false);
    assert_eq!(actual["insertText"], text);
    let edits = actual["additionalEdit"]["changes"][&uri]
        .as_array()
        .expect("import edit");
    let imported = apply_edits(source, edits);
    assert_eq!(
        imported,
        "package p;\n\nimport java.util.ArrayList;\nimport java.util.Set;\n\npublic class A {\n}\n"
    );
    assert_eq!(ws.read(&uri), source);
    assert!(paste(&mut ws, &uri, range(5, 0, 5, 0), text, Some(&uri), false).is_null());
    // Copied import names are Java tokens. A package beginning with
    // "static" and comments in an import must not be mistaken for a keyword.
    let root = ws.project_root("TestProject");
    ws.create_cu(
        &root,
        "src",
        "staticish",
        "List.java",
        "package staticish; public class List<T> {}\n",
    );
    let copied = ws.create_cu(
        &root,
        "src",
        "p",
        "Copied.java",
        "package p;\nimport staticish./* source */List;\nclass Copied {}\n",
    );
    let selected = paste(
        &mut ws,
        &uri,
        range(5, 0, 5, 0),
        "  List<String> values;\n",
        Some(&copied),
        false,
    );
    let edits = selected["additionalEdit"]["changes"][&uri]
        .as_array()
        .unwrap_or_else(|| panic!("{selected:#}"));
    assert_eq!(
        apply_edits(source, edits),
        "package p;\n\nimport java.util.Set;\n\nimport staticish.List;\n\npublic class A {\n}\n"
    );
}

#[test]
fn imports_only_inside_the_primary_type() {
    let mut ws = Workspace::new();
    let source = "package p;\npublic class A {\n}\nclass Other {\n}\n";
    let uri = created(&mut ws, source);
    for selection in [
        range(0, 0, 0, 0),
        range(1, 0, 1, 0),
        range(4, 0, 4, 0),
        range(2, 0, 3, 0),
    ] {
        assert!(paste(
            &mut ws,
            &uri,
            selection,
            "ArrayList<String> list;\n",
            None,
            false
        )
        .is_null());
    }
    assert!(paste(
        &mut ws,
        &uri,
        range(2, 0, 2, 0),
        "ArrayList<String> list;\n",
        None,
        false
    )
    .is_object());
}

#[test]
fn static_favorites_add_an_import_without_changing_the_buffer() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "completion": { "favoriteStaticMembers": ["java.util.Collections.emptyList"] } } });
    let source = "package p;\n\npublic class A {\n  void f() {\n  }\n}\n";
    let uri = created(&mut ws, source);
    let text = "    emptyList();\n";
    assert!(paste(&mut ws, &uri, range(4, 0, 4, 0), text, None, false).is_null());
    // Preferences.setJavaCompletionFavoriteMembers writes the *previous*
    // preference manager value to JavaManipulation's completion preferences.
    // Applying configuration again makes that value visible to import search.
    let settings = ws.settings.clone();
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": settings }),
    );
    ws.wait_idle();
    let actual = paste(&mut ws, &uri, range(4, 0, 4, 0), text, None, false);
    let edits = actual["additionalEdit"]["changes"][&uri]
        .as_array()
        .unwrap_or_else(|| panic!("{actual:#}"));
    assert_eq!(apply_edits(source, edits), "package p;\n\nimport static java.util.Collections.emptyList;\n\npublic class A {\n  void f() {\n  }\n}\n");
    assert_eq!(ws.read(&uri), source);
}

#[test]
fn file_paste_selects_the_first_type_and_available_name() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    let folder = root.join("src");
    std::fs::write(folder.join("First.java"), "class First {}\n").unwrap();
    std::fs::write(folder.join("First1.java"), "class First1 {}\n").unwrap();
    std::fs::write(folder.join("First3.java"), "class First3 {}\n").unwrap();
    assert_eq!(
        resolve(&mut ws, &folder, "class First {} public class Second {}"),
        folder.join("First2.java").to_str().unwrap()
    );
    assert!(!folder.join("First2.java").exists());
    assert_eq!(
        resolve(&mut ws, &folder, "public interface I {}"),
        folder.join("I.java").to_str().unwrap()
    );
    for text in [
        "enum E {}",
        "@interface Anno {}",
        "record R(int x) {}",
        "import java.util.List;",
    ] {
        assert_eq!(
            resolve(&mut ws, &folder, text),
            folder.join("Untitled.java").to_str().unwrap(),
            "{text}"
        );
    }
    assert!(resolve(&mut ws, &folder, "some ordinary text").is_null());
}

#[test]
fn file_paste_matches_exact_prefix_and_source_root_packages() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    std::fs::create_dir_all(root.join("src/p/deep")).unwrap();
    ws.create_cu(
        &root,
        "src",
        "p",
        "Existing.java",
        "package p; public class Existing {}\n",
    );
    let suggested = root.join("suggested");
    for package in ["p", "p.deep", "p.deep.more", "unmatched.sub"] {
        let expected = root
            .join("src")
            .join(package.replace('.', "/"))
            .join("Pasted.java");
        assert_eq!(
            resolve(
                &mut ws,
                &suggested,
                &format!("package {package}; public class Pasted {{}}")
            ),
            expected.to_str().unwrap()
        );
        assert!(!expected.exists());
    }
}

#[test]
fn file_paste_uses_the_supplied_folder_when_no_packages_exist() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    // A Java keyword is not a source package fragment.
    std::fs::create_dir_all(root.join("src/class")).unwrap();
    let folder = root.join("new-folder");
    assert_eq!(
        resolve(&mut ws, &folder, "package newpkg.deep;"),
        folder.join("newpkg/deep/Untitled.java").to_str().unwrap()
    );
}

#[test]
fn virtual_documents_support_string_and_import_paste() {
    // The oracle cannot resolve untitled/inmemory CUs. This regression covers
    // the Rust server's required virtual-document architecture.
    if common::jdtls::is_oracle() {
        return;
    }
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "completion": { "filteredTypes": ["java.awt.*"] } } });
    for (uri, name) in [
        ("untitled:A.java", "A"),
        ("untitled:Untitled-1", "A"),
        ("inmemory://paste/A.java", "A"),
        ("inmemory://paste/%C3%89.java", "É"),
        ("file:///nonexistent-paste-regression/A.java", "A"),
    ] {
        let content = "public class A {\n  String value = \"\";\n}\n"
            .replace("class A", &format!("class {name}"));
        let source = content.as_str();
        ws.open_with(uri, source);
        let start = source.find("\"\"").unwrap() + 1;
        assert_eq!(
            paste(
                &mut ws,
                uri,
                selection(source, start, start),
                "a\nb",
                None,
                false
            ),
            json!({ "insertText": "a\\n\" + //\n  \t\t\"b" })
        );
        let result = paste(
            &mut ws,
            uri,
            range(2, 0, 2, 0),
            "  Set<String> values;\n",
            None,
            false,
        );
        let edits = result["additionalEdit"]["changes"][uri]
            .as_array()
            .unwrap_or_else(|| panic!("{result:#}"));
        assert_eq!(
            apply_edits(source, edits),
            format!("import java.util.Set;\n\n{source}")
        );
        assert_eq!(
            paste(
                &mut ws,
                uri,
                selection(source, start, start),
                "a\nb",
                None,
                false
            )["insertText"],
            "a\\n\" + //\n  \t\t\"b"
        );
    }
}
