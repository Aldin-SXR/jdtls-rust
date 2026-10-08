//! Port of `org.eclipse.jdt.ls.core.internal.commands.SourceAttachmentCommandTest`.
//!
//! `SourceAttachmentCommand.resolveSourceAttachment/updateSourceAttachment`
//! become the `java.project.resolveSourceAttachment/updateSourceAttachment`
//! commands (the Java test passes the Gson-serialized request as a string
//! argument, which is what vscode-java sends). `IClassFile.getBuffer()` is
//! observed through `java/classFileContents` (a class file without attached
//! source is decompiled), and `IClasspathEntry.getSourceAttachmentPath()`
//! through the `sourcepath` attribute JDT persists in `.classpath` (relative to the
//! project for files inside it, absolute otherwise).

mod common;
use common::jdtls::*;
use serde_json::{json, Value};

const CLASS_FILE_URI: &str = "jdt://contents/foo.jar/foo/bar.java?%3Dsource-attachment%2Ffoo.jar%3Cfoo%28bar.class";

fn set_up() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/source-attachment"]);
    ws
}

fn command(ws: &mut Workspace, command: &str, arguments: Vec<Value>) -> Value {
    ws.request("workspace/executeCommand", json!({ "command": command, "arguments": arguments }))
}

fn request(attributes: Option<Value>) -> String {
    let mut request = json!({ "classFileUri": CLASS_FILE_URI });
    if let Some(attributes) = attributes {
        request["attributes"] = attributes;
    }
    request.to_string()
}

/// `classfile.getBuffer()` has the attached source.
fn source_attached(ws: &mut Workspace) -> Option<String> {
    let contents = ws.request("java/classFileContents", json!({ "uri": CLASS_FILE_URI }));
    let contents = contents.as_str().unwrap().to_owned();
    (!contents.starts_with("// Source code is decompiled")).then_some(contents)
}

/// The `sourcepath` JDT persisted for the `foo.jar` entry.
fn persisted_source_path(ws: &Workspace) -> Option<String> {
    let classpath = std::fs::read_to_string(ws.project_root("source-attachment").join(".classpath")).unwrap();
    let doc = roxmltree::Document::parse(&classpath).unwrap();
    doc.descendants()
        .find(|n| n.has_tag_name("classpathentry") && n.attribute("path").is_some_and(|p| p.ends_with("foo.jar")))
        .and_then(|n| n.attribute("sourcepath").map(str::to_owned))
}

#[test]
fn test_resolve_source_attachment_parameter_is_missing() {
    let mut ws = set_up();
    let result = command(&mut ws, "java.project.resolveSourceAttachment", vec![]);
    assert!(result.get("errorMessage").is_some_and(|m| !m.is_null()), "{result}");
}

#[test]
fn test_resolve_source_attachment_invalid_parameter() {
    let mut ws = set_up();
    let result = command(&mut ws, "java.project.resolveSourceAttachment", vec![json!(CLASS_FILE_URI)]);
    assert!(result.get("errorMessage").is_some_and(|m| !m.is_null()), "{result}");
}

#[test]
fn test_resolve_source_attachment_call() {
    let mut ws = set_up();
    let result = command(&mut ws, "java.project.resolveSourceAttachment", vec![json!(request(None))]);
    assert!(result.get("errorMessage").is_none_or(Value::is_null), "{result}");
    let attributes = &result["attributes"];
    assert!(attributes.is_object(), "{result}");
    assert!(attributes["jarPath"].as_str().unwrap().ends_with("foo.jar"));
    assert!(attributes["sourceAttachmentPath"].is_null());
}

#[test]
fn test_update_source_attachment_parameter_is_missing() {
    let mut ws = set_up();
    let result = command(&mut ws, "java.project.updateSourceAttachment", vec![]);
    assert!(result.get("errorMessage").is_some_and(|m| !m.is_null()), "{result}");
}

#[test]
fn test_update_source_attachment_invalid_parameter() {
    let mut ws = set_up();
    let result = command(&mut ws, "java.project.updateSourceAttachment", vec![json!(CLASS_FILE_URI)]);
    assert!(result.get("errorMessage").is_some_and(|m| !m.is_null()), "{result}");
}

#[test]
fn test_update_source_attachment_empty_source_attachment_path() {
    let mut ws = set_up();
    let attributes = json!({ "sourceAttachmentEncoding": "UTF-8" });
    let result = command(&mut ws, "java.project.updateSourceAttachment", vec![json!(request(Some(attributes)))]);
    assert!(result.get("errorMessage").is_none_or(Value::is_null), "{result}");

    // Verify no source is attached to the classfile.
    assert!(source_attached(&mut ws).is_none());
}

#[test]
fn test_update_source_attachment_from_project_jar() {
    let mut ws = set_up();
    let source = ws.project_root("source-attachment").join("foo-sources.jar");
    assert!(source.exists());
    let attributes = json!({ "sourceAttachmentPath": source.to_str().unwrap(), "sourceAttachmentEncoding": "UTF-8" });
    let result = command(&mut ws, "java.project.updateSourceAttachment", vec![json!(request(Some(attributes)))]);
    assert!(result.get("errorMessage").is_none_or(Value::is_null), "{result}");

    // Verify the source is attached to the classfile.
    let buffer = source_attached(&mut ws);
    assert!(buffer.is_some_and(|b| b.contains("return sum;")));

    // Verify whether project inside jar attachment is saved with project relative path.
    let relative_path = "foo-sources.jar";
    let absolute_path = source.to_str().unwrap();
    let saved = persisted_source_path(&ws).expect("sourcepath");
    assert_eq!(relative_path, saved);
    assert_ne!(absolute_path, saved);
}

#[test]
fn test_update_source_attachment_from_external_jar() {
    let mut ws = set_up();
    // `copyFiles("eclipse/external/foo-sources.jar", false)`
    let external = ws.dir.join("external");
    std::fs::create_dir_all(&external).unwrap();
    let file = external.join("foo-sources.jar");
    std::fs::copy(fixtures_dir().join("projects/eclipse/external/foo-sources.jar"), &file).unwrap();
    let source_path = file.canonicalize().unwrap();
    let source_path = source_path.to_str().unwrap();
    let attributes = json!({ "sourceAttachmentPath": source_path, "sourceAttachmentEncoding": "UTF-8" });
    let result = command(&mut ws, "java.project.updateSourceAttachment", vec![json!(request(Some(attributes)))]);
    assert!(result.get("errorMessage").is_none_or(Value::is_null), "{result}");

    // Verify the source is attached to the classfile.
    let buffer = source_attached(&mut ws);
    assert!(buffer.is_some_and(|b| b.contains("return sum;")));

    // Verify whether external jar attachment is saved with absolute path.
    let relative_path = "foo-sources.jar";
    let saved = persisted_source_path(&ws).expect("sourcepath");
    assert_eq!(source_path, saved);
    assert_ne!(relative_path, saved);
}
