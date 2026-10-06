//! Port of `org.eclipse.jdt.ls.core.internal.managers.ProjectsManagerTest`.
//!
//! `preferences.setProjectConfigurations` is the `projectConfigurations`
//! initialization option, `changeImportedProjects` the
//! `java.project.changeImportedProjects` command, `updateProject` the
//! `java/projectConfigurationUpdate` notification, `sendStatus` the
//! `language/status` notifications and `fileChanged` a
//! `workspace/didChangeWatchedFiles` notification.

mod common;
#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;
#[path = "common/projects_manager_fixture.rs"]
mod manager_fixture;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const BUILD_FILE_CHANGED: &str =
    "The build file has been changed and may need reload to make it effective.";

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

/// `preferences.setProjectConfigurations(paths)` + `initializeProjects([root])`.
fn initialize_with_configurations(ws: &mut Workspace, root: &Path, configurations: &[PathBuf]) {
    let uris: Vec<Value> = configurations.iter().map(|p| json!(file_uri(p))).collect();
    ws.init_options["projectConfigurations"] = Value::Array(uris);
    ws.import_root(root);
}

fn assert_projects(ws: &mut Workspace, expected: &[PathBuf]) {
    let all = ws.project_locations(true);
    assert_eq!(expected.len(), all.len(), "{all:#?}");
    for p in &all {
        assert!(
            expected.iter().any(|e| canonical(e) == *p),
            "unexpected project {}",
            p.display()
        );
    }
}

#[test]
fn test_create_default_project() {
    let mut ws = manager_fixture::workspace();
    let projects = manager_fixture::initialize_empty(&mut ws);
    assert_eq!(1, projects.len());
    let result = projects.first().expect("default project");
    assert_eq!(json!(true), result["isDefault"]);
    let location = canonical(&PathBuf::from(
        result["location"].as_str().expect("project location"),
    ));
    let expected = canonical(&ws.workspace_project_location("jdt.ls-java-project"));
    assert_eq!(expected, location);
    assert_eq!(
        json!(true), result["exists"], "the default project does not exist"
    );
    assert!(location.is_dir(), "the default project does not exist");
}

#[test]
fn test_resource_filters() {
    let mut ws = manager_fixture::workspace();
    ws.import_projects(&["maven/salut"]);
    let results = manager_fixture::filters(
        &mut ws, "salut", &["/node_modules", "/.git", "/src"],
        vec![json!({}), json!({"patterns":["node_modules", "\\.git"]}), json!({"patterns":null})],
    );
    assert_eq!(json!([false, false, false]), results[0]["filtered"]);
    assert_eq!(json!([true, true, false]), results[1]["filtered"]);
    assert_eq!(json!([false, false, false]), results[2]["filtered"]);
}

#[test]
fn test_invalid_resource_filters() {
    let mut ws = manager_fixture::workspace();
    ws.import_projects(&["maven/salut"]);
    let patterns = vec!["**/node_modules/**", "node_modules", "\\.git"];
    assert_eq!(3, patterns.len());
    let results = manager_fixture::filters(
        &mut ws, "salut", &[], vec![json!({"patterns":patterns})],
    );
    assert_eq!(
        2, results[0]["patterns"].as_array().expect("resource filters").len(),
    );
}

#[test]
fn test_cleanup_default_project() {
    let mut ws = workspace();
    // `linkFilesToDefaultProject("singlefile/WithError.java")`: a standalone
    // file outside every root, opened by the client.
    let outside = ws.dir.parent().unwrap().join("standalone");
    std::fs::create_dir_all(&outside).unwrap();
    let file = outside.join("WithError.java");
    std::fs::copy(
        fixtures_dir().join("projects/singlefile/WithError.java"),
        &file,
    )
    .unwrap();
    let uri = file_uri(&file);
    ws.open(&uri);
    ws.wait_idle();
    std::fs::remove_file(&file).unwrap();
    ws.files_changed(&[(&file, 3)]);
    let result = ws.build_workspace(true);
    assert_eq!(json!(1), result, "BuildWorkspaceStatus is: {result}.");
}

#[test]
fn dont_filter_git_like_packages() {
    //See https://github.com/eclipse/eclipse.jdt.ls/issues/2244
    let name = "gitfilter";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_no_errors(&project);
}

#[test]
fn test_import_maven_sub_module() {
    let mut ws = workspace();
    let project_dir = ws.copy_files("maven/multimodule");
    initialize_with_configurations(
        &mut ws,
        &project_dir,
        &[project_dir.join("module1/pom.xml")],
    );
    let expected = [
        project_dir.join("module1"),
        project_dir.join("module1/childmodule"),
    ];
    assert_projects(&mut ws, &expected);
}

#[test]
fn test_change_imported_maven_sub_module() {
    let mut ws = workspace();
    let project_dir = ws.copy_files("maven/multimodule");
    initialize_with_configurations(
        &mut ws,
        &project_dir,
        &[
            project_dir.join("module1/pom.xml"),
            project_dir.join("module2/pom.xml"),
        ],
    );
    let expected = [
        project_dir.join("module1"),
        project_dir.join("module1/childmodule"),
        project_dir.join("module2"),
    ];
    assert_projects(&mut ws, &expected);

    let new_build_file = project_dir.join("module3/pom.xml");
    let to_import = vec![file_uri(&new_build_file)];
    let project_to_remove = canonical(&project_dir.join("module2"));
    let to_delete = vec![dir_uri(&project_to_remove)];
    ws.execute(
        "java.project.changeImportedProjects",
        vec![json!(to_import), json!([]), json!(to_delete)],
    );
    ws.wait_for_background_jobs();
    let expected = [
        project_dir.join("module1"),
        project_dir.join("module1/childmodule"),
        project_dir.join("module3"),
    ];
    assert_projects(&mut ws, &expected);
}

#[test]
fn test_import_mixed_projects() {
    let mut ws = workspace();
    let project_dir = ws.copy_files("mixed");
    initialize_with_configurations(
        &mut ws,
        &project_dir,
        &[
            project_dir.join("hello/.project"),
            project_dir.join("simple-gradle/build.gradle"),
            project_dir.join("salut/pom.xml"),
        ],
    );
    let expected = [
        project_dir.join("hello"),
        project_dir.join("salut"),
        project_dir.join("simple-gradle"),
    ];
    assert_projects(&mut ws, &expected);
}

#[test]
fn test_import_mixed_projects_partially() {
    let mut ws = workspace();
    let project_dir = ws.copy_files("mixed");
    initialize_with_configurations(
        &mut ws,
        &project_dir,
        &[
            project_dir.join("simple-gradle/build.gradle"),
            project_dir.join("salut/pom.xml"),
        ],
    );
    let expected = [project_dir.join("salut"), project_dir.join("simple-gradle")];
    assert_projects(&mut ws, &expected);
}

/// The `language/status` notifications received after `start`.
fn statuses(ws: &mut Workspace, start: usize) -> Vec<(String, String)> {
    ws.wait_idle();
    let all: Vec<(String, String)> = ws
        .client()
        .notifications
        .iter()
        .filter(|n| n["method"] == "language/status")
        .map(|n| {
            (
                n["params"]["type"].as_str().unwrap_or("").to_owned(),
                n["params"]["message"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    all[start.min(all.len())..].to_vec()
}

fn status_count(ws: &mut Workspace) -> usize {
    statuses(ws, 0).len()
}

/// `projectsManager.updateProject(project, force)`.
fn update_project(ws: &mut Workspace, project: &Path) {
    ws.client().notify(
        "java/projectConfigurationUpdate",
        json!({ "uri": dir_uri(project) }),
    );
    ws.wait_for_background_jobs();
}

#[test]
#[ignore = "the oracle cannot update these old Gradle builds with the installed JDK 25: it reports ProjectStatus WARNING and retains the reload marker; requires a compatible Gradle VM and real Gradle model parity"]
fn test_sending_ok_project_status() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/simple-gradle"]);
    let project = ws.dir.join("gradle/simple-gradle");
    ws.wait_for_background_jobs();
    let start = status_count(&mut ws);
    update_project(&mut ws, &project);
    let status = statuses(&mut ws, start);
    assert_eq!(2, status.len(), "{status:?}");
    let last = status.last().unwrap();
    assert_eq!("ProjectStatus", last.0);
    assert_eq!("OK", last.1);
}

#[test]
#[ignore = "TODO(gradle): needs the Gradle model (the invalid build's sync error)"]
fn test_sending_warning_project_status() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/invalid"]);
    let project = ws.dir.join("gradle/invalid");
    ws.wait_for_background_jobs();
    let start = status_count(&mut ws);
    update_project(&mut ws, &project);
    let status = statuses(&mut ws, start);
    assert_eq!(2, status.len(), "{status:?}");
    let last = status.last().unwrap();
    assert_eq!("ProjectStatus", last.0);
    assert_eq!("WARNING", last.1);
}

/// `file.findMarkers(BUILD_FILE_MARKER_TYPE, false, DEPTH_ZERO)`.
fn build_file_markers(ws: &mut Workspace, file: &Path) -> usize {
    let uri = file_uri(file);
    ws.project_published_diagnostics()
        .get(&uri)
        .map(|d| {
            d.iter()
                .filter(|d| d["message"] == BUILD_FILE_CHANGED)
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn test_reload_maven_project_marker() {
    let mut ws = workspace();
    ws.import_projects(&["maven/salut"]);
    let project = canonical(&ws.dir.join("maven/salut"));
    let pom = project.join("pom.xml");
    assert_eq!(0, build_file_markers(&mut ws, &pom));

    let original_pom = std::fs::read_to_string(&pom).unwrap();
    std::fs::write(&pom, format!("{original_pom}\n")).unwrap();
    ws.files_changed(&[(&pom, 2)]);
    assert_eq!(1, build_file_markers(&mut ws, &pom));

    update_project(&mut ws, &project);
    assert_eq!(0, build_file_markers(&mut ws, &pom));
}

#[test]
#[ignore = "the oracle cannot update these old Gradle builds with the installed JDK 25: it reports ProjectStatus WARNING and retains the reload marker; requires a compatible Gradle VM and real Gradle model parity"]
fn test_reload_gradle_project_marker() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/sample"]);
    let project = canonical(&ws.dir.join("gradle/sample"));
    let gradle = project.join("settings.gradle");
    assert_eq!(0, build_file_markers(&mut ws, &gradle));

    let original_gradle = std::fs::read_to_string(&gradle).unwrap();
    std::fs::write(&gradle, format!("{original_gradle}\n")).unwrap();
    ws.files_changed(&[(&gradle, 2)]);
    assert_eq!(1, build_file_markers(&mut ws, &gradle));

    update_project(&mut ws, &project);
    assert_eq!(0, build_file_markers(&mut ws, &gradle));
}
