//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ImportNewProjectsTest`.
//!
//! `projectsManager.importProjects` is the `java.project.import` command,
//! `changeImportedProjects` the `java.project.changeImportedProjects`
//! command, and the mocked `sendEventNotification` the
//! `language/eventNotification` messages the client received.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::Path;

const PROJECTS_IMPORTED: u64 = 200;

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

fn project_count(ws: &mut Workspace) -> usize {
    ws.project_locations(true).len()
}

/// The `language/eventNotification` messages received so far.
fn events(ws: &mut Workspace) -> Vec<Value> {
    ws.wait_idle();
    ws.client()
        .notifications
        .iter()
        .filter(|n| n["method"] == "language/eventNotification")
        .map(|n| n["params"].clone())
        .collect()
}

fn add_module4(project_base_path: &Path) {
    let pom = project_base_path.join("pom.xml");
    let content = std::fs::read_to_string(&pom).unwrap().replace(
        "<module>module1</module>",
        "<module>module1</module>\n<module>module4</module>",
    );
    std::fs::write(&pom, content).unwrap();
    let sub_module_path = project_base_path.join("module4");
    std::fs::create_dir_all(&sub_module_path).unwrap();
    let build_file = sub_module_path.join("pom.xml");
    let xml = "\n".to_owned()
        + "<project xmlns=\"http://maven.apache.org/POM/4.0.0\""
        + "xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\""
        + "xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd\">"
        + "<modelVersion>4.0.0</modelVersion>"
        + "<parent>"
        + "<groupId>foo.bar</groupId>"
        + "<artifactId>multimodule</artifactId>"
        + "<version>0.0.1-SNAPSHOT</version>"
        + "</parent>"
        + "<artifactId>module4</artifactId>"
        + "</project>";
    std::fs::write(&build_file, xml).unwrap();
}

#[test]
fn test_import_new_maven_projects() {
    let mut ws = workspace();
    ws.import_projects(&["maven/multimodule"]);
    ws.wait_for_background_jobs();
    assert_eq!(5, project_count(&mut ws));

    // Add new sub-module
    let project_base_path = ws.project_root("multimodule");
    add_module4(&project_base_path);

    // Verify no projects imported
    assert_eq!(5, project_count(&mut ws));

    // Verify import projects
    let before = events(&mut ws).len();
    ws.execute("java.project.import", vec![]);
    ws.wait_for_background_jobs();
    let module4 = ws.dir.join("maven/multimodule/module4");
    assert!(ws.has_project_at(&module4, true));
    let count = project_count(&mut ws);
    assert_eq!(6, count);

    // The mocked connection only receives the notification of the import
    // job, the last one: the progressive reporter reports to the real client.
    let events = events(&mut ws);
    let argument = events[before..].last().expect("a ProjectsImported event");
    assert_eq!(json!(PROJECTS_IMPORTED), argument["eventType"]);
    assert_eq!(count, argument["data"].as_array().unwrap().len());
}

#[test]
fn test_manual_import_new_maven_projects() {
    let mut ws = workspace();
    ws.import_projects(&["maven/multimodule"]);
    ws.wait_for_background_jobs();
    assert_eq!(5, project_count(&mut ws));

    // Add new sub-module
    let project_base_path = ws.project_root("multimodule");
    add_module4(&project_base_path);
    let parent_pom = project_base_path.join("pom.xml");
    let new_module_pom = project_base_path.join("module4/pom.xml");

    // Verify no projects imported
    assert_eq!(5, project_count(&mut ws));

    // Manual import the new maven module
    let before = events(&mut ws).len();
    ws.execute(
        "java.project.changeImportedProjects",
        vec![
            json!([file_uri(&new_module_pom)]),
            json!([file_uri(&parent_pom)]),
            json!([]),
        ],
    );
    ws.wait_for_background_jobs();
    let module4 = project_base_path.join("module4");
    assert!(ws.has_project_at(&module4, true), "New module is imported");
    assert_eq!(6, project_count(&mut ws));

    let events = events(&mut ws);
    let argument = &events[before..];
    assert_eq!(1, argument.len(), "{argument:?}");
    assert_eq!(json!(PROJECTS_IMPORTED), argument[0]["eventType"]);
    assert_eq!(1, argument[0]["data"].as_array().unwrap().len());
}

#[test]
fn test_import_new_gradle_projects() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/multi-module"]);
    ws.wait_for_background_jobs();
    assert_eq!(3, project_count(&mut ws));
    // Add new sub-module
    let project_base_path = ws.project_root("multi-module");
    let settings = project_base_path.join("settings.gradle");
    let mut content = std::fs::read_to_string(&settings).unwrap();
    content.push_str("\ninclude 'test'");
    std::fs::write(&settings, content).unwrap();
    let sub_module_path = project_base_path.join("test");
    std::fs::create_dir_all(&sub_module_path).unwrap();
    std::fs::write(sub_module_path.join("build.gradle"), "").unwrap();

    // Verify no projects imported
    assert_eq!(3, project_count(&mut ws));

    // Verify import projects
    let before = events(&mut ws).len();
    ws.execute("java.project.import", vec![]);
    ws.wait_for_background_jobs();
    assert!(ws.has_project_at(&sub_module_path, true));
    let count = project_count(&mut ws);
    assert_eq!(4, count);

    // The mocked connection only receives the notification of the import
    // job, the last one: the progressive reporter reports to the real client.
    let events = events(&mut ws);
    let argument = events[before..].last().expect("a ProjectsImported event");
    assert_eq!(json!(PROJECTS_IMPORTED), argument["eventType"]);
    assert_eq!(count, argument["data"].as_array().unwrap().len());
}

#[test]
fn test_import_mixed_projects() {
    let mut ws = workspace();
    assert_eq!(0, project_count(&mut ws));
    ws.import_projects(&["mixed"]);
    ws.wait_for_background_jobs();
    assert_eq!(3, project_count(&mut ws));
    let hello = ws.project_root("hello");
    ws.assert_is_java_project(&hello);
}
