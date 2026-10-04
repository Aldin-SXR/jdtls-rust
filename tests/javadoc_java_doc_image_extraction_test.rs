//! Port of `org.eclipse.jdt.ls.core.internal.javadoc.JavaDocImageExtractionTest`.
//!
//! `HoverInfoProvider.computeJavadoc(JDTUtils.findElementAtSelection(cu, l, c))`
//! is the Javadoc entry of the hover at `l:c`. `testIsAbsolutePath` exercises a
//! pure function and is a unit test in `src/javadoc/path_handler.rs`
//! (`java_doc_image_extraction_test::test_is_absolute_path`).

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

/// `setupMockMavenProject(folderName)`
fn setup_mock_maven_project(folder_name: &str) -> (Workspace, std::path::PathBuf) {
    let mut ws = Workspace::new();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
    ws.import_projects(&[&format!("maven/{folder_name}")]);
    let root = ws.dir.join("maven").join(folder_name);
    (ws, root)
}

fn compute_javadoc(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> String {
    let hover = ws.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    );
    let contents = hover["contents"].as_array().cloned().unwrap_or_default();
    contents
        .iter()
        .skip(1)
        .filter_map(Value::as_str)
        .find(|s| !s.starts_with("Source: *"))
        .unwrap_or_else(|| panic!("no Javadoc in hover {hover}"))
        .to_owned()
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (reactor-core sources jar download)"]
fn test_image_extraction_with_source_jar() {
    help_test_image_extraction_with_x_jar("javadoc-image-extraction-with-sources");
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (reactor-core javadoc jar download)"]
fn test_image_extraction_with_javadoc_jar() {
    help_test_image_extraction_with_x_jar("javadoc-image-extraction-with-javadoc");
}

fn help_test_image_extraction_with_x_jar(test_folder_name: &str) {
    let (mut ws, root) = setup_mock_maven_project(test_folder_name);
    let uri = Url::from_file_path(root.join("src/main/java/foo/JavaDocJarTest.java")).unwrap().to_string();
    let final_string = compute_javadoc(&mut ws, &uri, 10, 12);
    let expected_image_markdown = "![](";
    assert!(
        final_string.contains(expected_image_markdown) && final_string.contains("reactor-core-3.2.10.RELEASE/error.svg)"),
        "Does finalString=\n\t\"{final_string}\"\nContain expectedImageMarkdown"
    );
}

#[test]
fn test_image_extraction_without_any_jars() {
    let (mut ws, root) = setup_mock_maven_project("javadoc-image-extraction-without-any");
    let uri = Url::from_file_path(root.join("src/main/java/foo/JavaDocJarTest.java")).unwrap().to_string();
    let final_string = compute_javadoc(&mut ws, &uri, 12, 22);

    let expected_image_markdown = "![](this/does/not/exist.png)";

    assert!(final_string.contains(expected_image_markdown), "Missing image from {final_string}");
}

#[test]
fn test_image_relative_to_file() {
    let (mut ws, root) = setup_mock_maven_project("relative-image");
    let file = root.join("src/main/java/foo/bar/RelativeImage.java");
    let uri = Url::from_file_path(&file).unwrap().to_string();

    // Paths.get(uri).getParent().toUri().getPath()
    let parent = file.parent().unwrap();
    let parent_export_path = format!("{}/", Url::from_file_path(parent).unwrap().path());

    let relative_export_path = "FolderWithPictures/red-hat-logo.png";
    let absolute_export_path = format!("{parent_export_path}{relative_export_path}");
    let expected_image_markdown = format!("![](file://{absolute_export_path})");

    let final_string = compute_javadoc(&mut ws, &uri, 7, 23);

    assert!(final_string.contains(&expected_image_markdown), "Missing image from {final_string}");
}

#[test]
fn test_hyperlink_image() {
    let (mut ws, root) = setup_mock_maven_project("javadoc-image-extraction-with-hyperlink");
    let uri = Url::from_file_path(root.join("src/main/java/foo/JavaDocJarTest.java")).unwrap().to_string();
    let final_string = compute_javadoc(&mut ws, &uri, 14, 22);

    let expected_image_markdown = "![](https://www.redhat.com/cms/managed-files/Logo-redhat-color-375.png)";

    assert!(final_string.contains(expected_image_markdown), "Missing image from {final_string}");
}
