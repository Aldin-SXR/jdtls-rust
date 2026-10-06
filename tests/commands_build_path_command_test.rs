//! Port of `org.eclipse.jdt.ls.core.internal.commands.BuildPathCommandTest`.
mod common;
use common::jdtls::Workspace;
use common::projects::SOURCE_PATHS;
use serde_json::{json, Value};
use std::path::Path;

fn change(ws: &mut Workspace, command: &str, path: &Path) -> Value {
    let uri = tower_lsp::lsp_types::Url::from_file_path(path)
        .unwrap()
        .to_string();
    ws.execute(command, vec![json!(uri)])
}
fn list(ws: &mut Workspace) -> Value {
    ws.execute("java.project.listSourcePaths", vec![])
}

#[test]
fn test_build_path_operation_in_workspace_project() {
    let mut ws = Workspace::new();
    let root = ws.copy_files("singlefile/lesson1");
    ws.set_roots(vec![root.clone()]);
    let added = change(&mut ws, "java.project.addToSourcePath", &root.join("src"));
    assert_eq!(true, added["status"], "{added:#?}");
    let hash = root
        .to_string_lossy()
        .replace('\\', "/")
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let project = ws.workspace_project_location(&format!("lesson1_{hash:x}"));
    assert!(project.is_dir(), "the invisible project must exist");
    assert!(ws.has_project_at(&project, true));
    assert!(ws
        .natures(&project)
        .iter()
        .any(|n| n == "org.eclipse.jdt.core.javanature"));
    let nested = change(
        &mut ws,
        "java.project.addToSourcePath",
        &root.join("src/main/java"),
    );
    assert_eq!(false, nested["status"]);
    let samples = change(
        &mut ws,
        "java.project.addToSourcePath",
        &root.join("samples"),
    );
    assert_eq!(true, samples["status"]);
    let result = list(&mut ws);
    assert_eq!(true, result["status"]);
    let paths = result["data"]
        .as_array()
        .expect("source paths must not be null");
    assert_eq!(2, paths.len(), "{result:#?}");
    assert_eq!("lesson1/src", paths[0]["displayPath"]);
    assert_eq!("lesson1/samples", paths[1]["displayPath"]);
}

#[test]
fn test_build_path_operation_in_eclipse_project() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    let root = ws.project_root("hello");
    let result = list(&mut ws);
    assert_eq!(true, result["status"]);
    let paths = result["data"]
        .as_array()
        .expect("source paths must not be null");
    assert_eq!(2, paths.len());
    assert_eq!("hello/src", paths[0]["displayPath"]);
    assert_eq!("hello/test", paths[1]["displayPath"]);
    assert_eq!(
        false,
        change(
            &mut ws,
            "java.project.addToSourcePath",
            &root.join("src/java")
        )["status"]
    );
    assert_eq!(
        true,
        change(
            &mut ws,
            "java.project.addToSourcePath",
            &root.join("nopackage")
        )["status"]
    );
    let uri = tower_lsp::lsp_types::Url::from_file_path(&root)
        .unwrap()
        .to_string();
    assert_eq!(
        3,
        ws.project_settings(&uri, &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
    assert_eq!(
        true,
        change(
            &mut ws,
            "java.project.removeFromSourcePath",
            &root.join("nopackage")
        )["status"]
    );
    assert_eq!(
        2,
        ws.project_settings(&uri, &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
}

#[test]
fn test_build_path_operation_in_maven_project() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    let root = ws.project_root("salut");
    let result = list(&mut ws);
    assert_eq!(true, result["status"]);
    assert_eq!(
        6,
        result["data"]
            .as_array()
            .expect("source paths must not be null")
            .len()
    );
    for (command, folder) in [
        ("java.project.addToSourcePath", "src"),
        ("java.project.removeFromSourcePath", "src/main/java"),
    ] {
        let result = change(&mut ws, command, &root.join(folder));
        assert_eq!(false, result["status"]);
        assert_eq!("Unsupported operation. Please use pom.xml file to manage the source directories of maven project.",result["message"]);
    }
}

#[test]
fn test_build_path_operation_in_gradle_project() {
    let mut ws = Workspace::new();
    ws.import_projects(&["gradle/simple-gradle"]);
    let root = ws.project_root("simple-gradle");
    let result = list(&mut ws);
    assert_eq!(true, result["status"]);
    assert_eq!(
        2,
        result["data"]
            .as_array()
            .expect("source paths must not be null")
            .len(),
        "{result:#?}"
    );
    for (command, folder) in [
        ("java.project.addToSourcePath", "src"),
        ("java.project.removeFromSourcePath", "src/main/java"),
    ] {
        let result = change(&mut ws, command, &root.join(folder));
        assert_eq!(false, result["status"]);
        assert_eq!("Unsupported operation. Please use build.gradle file to manage the source directories of gradle project.",result["message"]);
    }
}
