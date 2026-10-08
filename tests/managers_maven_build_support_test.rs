//! Port of `org.eclipse.jdt.ls.core.internal.managers.MavenBuildSupportTest`.
//!
//! `projectsManager.updateProject(project, false)` is the
//! `java/projectConfigurationUpdate` notification, `fileChanged` a
//! `workspace/didChangeWatchedFiles` notification, and jobs are observed
//! through their `language/progressReport`s.  `collectProjects` is a
//! unit-level port of `project::maven::collect_projects`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::*;
use common::maven::*;
use common::projects::*;
use serde_json::json;
use std::path::{Path, PathBuf};

/// `projectsManager.updateProject(project, false)`.
fn update_project(ws: &mut Workspace, project: &Path) {
    ws.client().notify(
        "java/projectConfigurationUpdate",
        json!({ "uri": dir_uri(project) }),
    );
    ws.wait_for_background_jobs();
}

#[test]
fn test_update() {
    let mut ws = workspace();
    let project = import_simple_java_project(&mut ws);

    let pom = project.join("pom.xml");

    // Remove dependencies to cause compilation errors
    let original_pom = std::fs::read_to_string(&pom).unwrap();
    let dependency_less_pom = comment(&original_pom, "<dependencies>", "</dependencies>");
    std::fs::write(&pom, dependency_less_pom).unwrap();
    ws.wait_for_background_jobs();
    // Contents changed outside the workspace, so should not change
    ws.assert_no_errors(&project);

    update_project(&mut ws, &project);

    // Giving a nudge, so that errors show up
    ws.wait_for_background_jobs();
    ws.assert_has_errors(&project, &[]);

    // Fix pom, trigger build
    std::fs::write(&pom, original_pom).unwrap();
    update_project(&mut ws, &project);
    ws.wait_for_background_jobs();
    ws.assert_no_errors(&project);
}

fn test_non_standard_compiler_id(project_name: &str) {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, project_name);
    ws.assert_is_java_project(&project);
    assert_eq!("1.8", ws.java_source_level(&project));
    ws.assert_no_errors(&project);
}

#[test]
fn test_compile_with_error_prone() {
    test_non_standard_compiler_id("compile-with-error-prone");
}

#[test]
fn test_compile_with_eclipse() {
    test_non_standard_compiler_id("compile-with-eclipse");
}

#[test]
fn test_compile_with_eclipse_tycho_jdt() {
    test_non_standard_compiler_id("compile-with-tycho-jdt");
}

fn collected_projects(name: &str) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join(name);
    copy_dir(&fixtures_dir().join("projects/maven").join(name), &root);
    let settings = project::ImportSettings::jdtls_defaults();
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let project = ws
        .projects
        .iter()
        .find(|p| p.location == root)
        .unwrap_or_else(|| panic!("project {name} was not imported"));
    project::maven::collect_projects(&ws, project)
}

#[test]
fn test_invalid_projects() {
    let projects = collected_projects("multimodule2");
    assert_eq!(projects.len(), 1);
}

#[test]
fn test_multiple_projects() {
    let projects = collected_projects("multimodule");
    assert_eq!(projects.len(), 4);
    for p in &projects {
        if "module3" == p {
            panic!("module3 exists");
        }
    }
}

#[test]
fn test_ignore_inner_pom_changes() {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, "archetyped");
    assert_eq!(
        1,
        ws.all_projects(true).len(),
        "The inner pom should not have been imported"
    );

    let inner_pom = project.join("src/main/resources/archetype-resources/pom.xml");

    ws.update_settings(json!({ "java": {
        "maven": { "downloadSources": true },
        "configuration": { "updateBuildConfiguration": "automatic" }
    } }));
    ws.files_changed(&[(&inner_pom, 2)]);
    ws.wait_for_background_jobs();
    let update_triggered = progress_reports(&mut ws).iter().any(|r| {
        r["task"]
            .as_str()
            .is_some_and(|t| t.contains("Update project"))
    });
    assert!(
        !update_triggered,
        "Update project should not have been triggered"
    );
}

#[test]
fn test_build_helper_support() {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, "buildhelped");
    ws.build_workspace(true);
    ws.assert_is_java_project(&project);
    ws.assert_no_errors(&project);
}

fn local_repository_sources(group: &str, artifact: &str, version: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap();
    PathBuf::from(home)
        .join(".m2/repository")
        .join(group.replace('.', "/"))
        .join(artifact)
        .join(version)
        .join(format!("{artifact}-{version}-sources.jar"))
}

/// `new SourceContentProvider().getSource(classFile)`, retrying after the
/// download-sources jobs have finished.
fn class_file_source(ws: &mut Workspace, uri: &str) -> Option<String> {
    let mut source = None;
    for _ in 0..30 {
        ws.wait_for_background_jobs();
        source = ws
            .request("java/classFileContents", json!({ "uri": uri }))
            .as_str()
            .map(str::to_owned);
        if source.as_deref().is_some_and(|s| !s.starts_with("// Source code is decompiled")) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    source
}

#[test]
fn test_download_sources() {
    let file = local_repository_sources("org.apache.commons", "commons-lang3", "3.18.0");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
    let mut ws = workspace();
    ws.settings = json!({ "java": { "maven": { "downloadSources": false } } });
    import_maven_project(&mut ws, "salut");
    ws.wait_for_background_jobs();
    assert!(!file.exists());
    let uri = ws.class_file_uri("salut", "org.apache.commons.lang3.StringUtils");
    let source = class_file_source(&mut ws, &uri);
    assert!(
        source.is_some(),
        "Couldn't find source for org.apache.commons.lang3.StringUtils({} {})",
        file.display(),
        if file.exists() { "exists)" } else { "is missing)" }
    );
}

#[test]
fn test_download_sources_when_sha1_search_fails() {
    let sources = local_repository_sources("org.springframework", "spring-core", "7.0.2");
    let _ = std::fs::remove_dir_all(sources.parent().unwrap());
    let mut ws = workspace();
    ws.settings = json!({ "java": { "maven": { "downloadSources": false } } });
    import_maven_project(&mut ws, "spring");
    ws.wait_for_background_jobs();
    assert!(!sources.exists());
    let uri = ws
        .try_class_file_uri("spring", "org.springframework.util.StringUtils")
        .expect("Couldn't find type org.springframework.util.StringUtils");
    let source = class_file_source(&mut ws, &uri)
        .expect("Couldn't find source for org.springframework.util.StringUtils");
    assert!(
        source.contains("This class delivers some simple functionality"),
        "Source for org.springframework.util.StringUtils should contain 'This class delivers some simple functionality'. Source: {source}"
    );
}

#[test]
fn test_update_snapshots() {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, "salut3");
    ws.wait_for_background_jobs();
    assert!(ws
        .try_class_file_uri("salut3", "org.apache.commons.lang3.StringUtils")
        .is_none());
    ws.update_settings(json!({ "java": {
        "maven": { "downloadSources": true, "updateSnapshots": false },
        "configuration": { "updateBuildConfiguration": "automatic" }
    } }));
    let pom = project.join("pom.xml");
    let mut content = std::fs::read_to_string(&pom).unwrap();
    content = content.replace(
        "<dependencies></dependencies>",
        &("<dependencies>\n".to_owned()
            + "<dependency>\n"
            + "   <groupId>org.apache.commons</groupId>\n"
            + "   <artifactId>commons-lang3</artifactId>\n"
            + "   <version>3.9</version>\n"
            + "</dependency>"
            + "</dependencies>"),
    );
    std::fs::write(&pom, content).unwrap();
    ws.files_changed(&[(&pom, 2)]);
    ws.wait_for_background_jobs();
    assert!(ws
        .try_class_file_uri("salut3", "org.apache.commons.lang3.StringUtils")
        .is_some());
}

#[test]
fn test_batch_import() {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, "batch");
    ws.wait_for_background_jobs();
    ws.assert_is_maven_project(&project);
    assert_eq!(ws.all_projects(true).len(), 13);
    let child = ws.project_root("batchchild");
    ws.assert_is_maven_project(&child);
}
