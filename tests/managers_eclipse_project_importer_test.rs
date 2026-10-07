//! Port of `org.eclipse.jdt.ls.core.internal.managers.EclipseProjectImporterTest`.
//!
//! Projects are inspected through the commands jdt.ls exposes
//! (`java.project.getAll`, `java.project.getSettings`, the published
//! diagnostics and the file watcher registration), so the LSP tests also run
//! against the oracle.  Tests that call `EclipseProjectImporter` methods
//! directly (`findUniqueProject`, `applies`) are unit-level ports of
//! `project::eclipse`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;
use std::path::PathBuf;

const BAR_PATTERN: &str = "**/bar";

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    // AbstractProjectsManagerBasedTest.initPreferences
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

#[test]
fn import_simple_java_project() {
    let name = "hello";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    // a test for https://github.com/redhat-developer/vscode-java/issues/244
    ws.import_projects(&[&format!("eclipse/{name}")]);
    ws.assert_is_java_project(&project);
}

#[test]
#[ignore = "the jdt.ls 1.58.0 product cannot open a project whose resource filter uses the unregistered org.eclipse.ui.ide.missingFilter matcher: the oracle (and jdtls-rust) keep it closed, so it is no Java project; upstream's test runtime opens it"]
fn ignore_missing_resource_filters() {
    let name = "ignored-filter";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_no_errors(&project);
    // The missing filter is logged to the platform log only: no message is
    // sent to the client.
    let logged: Vec<String> = ws
        .client()
        .notifications
        .iter()
        .filter(|n| n["method"] == "window/logMessage" && n["params"]["type"] == 1)
        .map(|n| n["params"]["message"].as_str().unwrap_or("").to_owned())
        .filter(|m| m.contains("Missing resource filter type"))
        .collect();
    assert!(logged.is_empty(), "Unexpected logs {logged:?}");
}

#[test]
fn import_multiple_java_project() {
    let mut ws = workspace();
    ws.import_projects(&["eclipse/multi"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len());

    let bar = ws.dir.join("eclipse/multi/bar");
    ws.assert_is_java_project(&bar);

    let foo = ws.dir.join("eclipse/multi/foo");
    ws.assert_is_java_project(&foo);
}

#[test]
fn test_use_release_flag_by_default() {
    let name = "java7";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    assert_eq!(
        json!("enabled"),
        ws.java_option(&project, "org.eclipse.jdt.core.compiler.release")
    );
}

#[test]
fn test_java_import_exclusions() {
    let mut ws = workspace();
    let mut exclusions: Vec<String> = project::detect::DEFAULT_IMPORT_EXCLUSIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    exclusions.push(BAR_PATTERN.to_owned());
    ws.settings["java"]["import"] = json!({ "exclusions": exclusions });
    ws.import_projects(&["eclipse/multi"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len());
    assert!(!ws.has_project_at(&ws.dir.join("eclipse/multi/bar"), true));
    let foo = ws.dir.join("eclipse/multi/foo");
    ws.assert_is_java_project(&foo);
}

#[test]
fn test_find_unique_project() {
    // given
    let name = "project";
    let mut existing: Vec<String> = Vec::new();
    // when
    let p = project::eclipse::find_unique_project(name, |n| existing.iter().any(|e| e == n));
    // then
    assert_eq!("project", p);

    // given
    existing.push("project".into());
    // when
    let p = project::eclipse::find_unique_project(name, |n| existing.iter().any(|e| e == n));
    // then
    assert_eq!("project (2)", p);

    // given
    existing.push("project (2)".into());
    // when
    let p = project::eclipse::find_unique_project(name, |n| existing.iter().any(|e| e == n));
    // then
    assert_eq!("project (3)", p);
}

#[test]
fn test_preview_features16() {
    let name = "java16";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_has_errors(
        &project,
        &["Syntax error on token \"sealed\", static expected"],
    );
}

#[test]
#[ignore = "needs a JavaSE-26 runtime: upstream registers fake TestVMType JREs for every execution environment, only JDK 25 is installed here (the oracle reports 'Unbound classpath container: JRE System Library [JavaSE-26]')"]
fn test_preview_features_disabled_by_default() {
    let name = "java26";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_no_errors(&project);
}

#[test]
fn test_preview_features_not_available() {
    let name = "java12";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_has_errors(
        &project,
        &[
            "Switch Expressions are supported from",
            "Arrow in case statement supported from",
        ],
    );
}

#[test]
fn test_classpath() {
    let name = "classpath";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    assert!(!ws.has_project_at(&ws.dir.join("eclipse").join(name), true));
}

fn fixture_copy(rel: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join(rel);
    copy_dir(&fixtures_dir().join("projects").join(rel), &root);
    (tmp, root)
}

#[test]
fn avoid_import_duplicated_projects() {
    let (_tmp, root) = fixture_copy("multi-buildtools");
    let settings = project::ImportSettings::jdtls_defaults();
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let configuration_paths = vec![root.join("build.gradle")];
    assert!(project::eclipse::import(&root, &settings, &ws, Some(&configuration_paths)).is_empty());
}

// https://github.com/redhat-developer/vscode-java/issues/2436
#[test]
fn import_java_project_with_root_source() {
    let name = "projectwithrootsource";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    let watchers = ws.watcher_glob_patterns();
    assert_eq!(11, watchers.len());
    let src_glob_pattern = &watchers[9];
    assert!(
        src_glob_pattern.ends_with("projectwithrootsource/**"),
        "Unexpected source glob pattern: {src_glob_pattern}"
    );
}

#[test]
#[ignore = "upstream counts 2 markers via IProject.findMarkers but jdt.ls publishes only the Main.java null type mismatch warning (the oracle reports exactly 1), so the marker count is not observable over LSP"]
fn test_null_analysis() {
    let name = "testnullable2";
    let mut ws = workspace();
    ws.settings["java"]["compile"] = json!({ "nullAnalysis": {
        "nonnull": ["org.springframework.lang.NonNull", "javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        "nullable": ["org.springframework.lang.Nullable", "org.eclipse.jdt.annotation.Nullable", "javax.annotation.Nonnull"],
        "mode": "automatic"
    } });
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.build_workspace(true);
    let markers = ws.all_markers(&project);
    assert_eq!(2, markers.len(), "{}", markers_to_string(&markers));
    let marker = markers.iter().find(|(_, d)| {
        d["severity"] == 2
            && d["message"]
                == "Null type mismatch: required '@Nonnull Test' but the provided value is null"
    });
    assert!(marker.is_some(), "{}", markers_to_string(&markers));
    let (uri, _) = marker.unwrap();
    assert!(uri.ends_with("/Main.java"));
    assert_eq!(
        json!("enabled"),
        ws.java_option(
            &project,
            "org.eclipse.jdt.core.compiler.annotation.nullanalysis"
        )
    );
    ws.assert_no_errors(&project);
}

#[test]
fn do_not_duplicate_import_project() {
    let (_tmp, root) = fixture_copy("eclipse/hello");
    let settings = project::ImportSettings::jdtls_defaults();
    let ws = project::Workspace::import(&[root.clone()], &settings);
    assert!(ws.project("hello").is_some_and(|p| p.is_java()));
    let has_unimported_projects = !project::eclipse::import(&root, &settings, &ws, None).is_empty();
    assert!(!has_unimported_projects);
}

#[test]
fn test_forbidden_reference() {
    let name = "forbiddenreference";
    let mut ws = workspace();
    ws.import_projects(&[&format!("eclipse/{name}")]);
    let project = ws.dir.join("eclipse").join(name);
    ws.assert_is_java_project(&project);
    ws.assert_no_errors(&project);
}
