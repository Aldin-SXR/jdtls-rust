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
    let contents = match &hover["contents"] {
        Value::Array(a) => a.clone(),
        v => vec![v.clone()],
    };
    contents
        .iter()
        .skip(1)
        .filter_map(Value::as_str)
        .find(|s| !s.starts_with("Source: *"))
        .unwrap_or_else(|| panic!("no Javadoc in hover {hover}"))
        .to_owned()
}

#[test]
fn test_image_extraction_with_source_jar() {
    help_test_image_extraction_with_x_jar("javadoc-image-extraction-with-sources");
}

#[test]
fn test_image_extraction_with_javadoc_jar() {
    help_test_image_extraction_with_x_jar("javadoc-image-extraction-with-javadoc");
}

/// `JavaDocHTMLPathHandler.EXTRACTED_JAR_IMAGES_FOLDER` of the server under test.
fn extracted_jar_images_folder(ws: &Workspace) -> std::path::PathBuf {
    if common::jdtls::is_oracle() {
        // the plugin state location in the oracle's workspace data
        ws.dir.parent().unwrap().join("oracle-data/workspace/.metadata/.plugins/org.eclipse.jdt.ls.core/extracted-jar-images")
    } else {
        std::env::var_os("JDTLS_DATA_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("jdtls-rust"))
            .join("extracted-jar-images")
    }
}

/// `ensureSourceOfClassIsDownloaded`: the class file has attached source.
fn ensure_source_of_class_is_downloaded(ws: &mut Workspace, project: &str, classpath_name: &str) {
    let uri = ws.class_file_uri(project, classpath_name);
    let source = ws.request("java/classFileContents", json!({ "uri": uri }));
    let source = source.as_str().unwrap_or_default();
    assert!(!source.is_empty() && !source.starts_with("// Source code is decompiled"), "no source for {classpath_name}");
}

fn help_test_image_extraction_with_x_jar(test_folder_name: &str) {
    let (mut ws, root) = setup_mock_maven_project(test_folder_name);
    ensure_source_of_class_is_downloaded(&mut ws, test_folder_name, "reactor.core.publisher.Mono");

    let uri = Url::from_file_path(root.join("src/main/java/foo/JavaDocJarTest.java")).unwrap().to_string();

    let exported_file_location = extracted_jar_images_folder(&ws).join("reactor-core-3.2.10.RELEASE/error.svg");
    let test_export_path = Url::from_file_path(&exported_file_location).unwrap().to_string();

    let expected_image_markdown = format!("![]({test_export_path})");

    let final_string = compute_javadoc(&mut ws, &uri, 10, 12);

    assert!(
        final_string.contains(&expected_image_markdown),
        "Does finalString=\n\t\"{final_string}\"\nContain expectedImageMarkdown=\n\t\"{expected_image_markdown}\""
    );

    assert!(exported_file_location.exists());
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
