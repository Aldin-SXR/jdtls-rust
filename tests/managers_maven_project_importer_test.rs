//! Port of `org.eclipse.jdt.ls.core.internal.managers.MavenProjectImporterTest`.
//!
//! Jobs are observed through their `language/progressReport`s (jdt.ls
//! reports the progress of every job to a client that supports it), the
//! project model through `java.project.getAll`/`getSettings`/
//! `listSourcePaths` and the published diagnostics.  The
//! `MavenProjectImporter.applies` calls are unit-level ports of
//! `project::maven::import`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::*;
use common::maven::*;
use common::projects::*;
use serde_json::json;
use std::path::PathBuf;

const PROJECT1_PATTERN: &str = "**/project1";
const UPDATE_JOB: &str = "Update Maven project configuration";
const ENABLE_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures";
const REPORT_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures";
const PREVIEW_ERROR: &str = "Preview features enabled at an invalid source release level";

#[test]
fn test_import_simple_java_project() {
    let mut ws = workspace();
    import_simple_java_project(&mut ws);
    assert_eq!(
        0,
        jobs_named(&mut ws, UPDATE_JOB),
        "New Projects should not be updated"
    );
    assert_task_completed(&mut ws, "Importing Maven project(s)");
}

#[test]
fn test_java_import_exclusions() {
    let mut ws = workspace();
    let mut exclusions: Vec<String> = project::detect::DEFAULT_IMPORT_EXCLUSIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    exclusions.push(PROJECT1_PATTERN.to_owned());
    ws.settings["java"]["import"] = json!({ "exclusions": exclusions });
    ws.import_projects(&["maven/multi"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // project 2
    assert!(!ws.has_project_at(&ws.dir.join("maven/multi/project1"), true));
    let project2 = ws.dir.join("maven/multi/project2");
    ws.assert_is_maven_project(&project2);
}

#[test]
fn test_node_modules() {
    let mut ws = workspace();
    ws.import_projects(&["maven/salut5"]);
    let proj = ws.dir.join("maven/salut5/proj");
    ws.assert_is_maven_project(&proj);
    let scanned = progress_reports(&mut ws).iter().any(|r| {
        r["subTask"]
            .as_str()
            .is_some_and(|s| s.ends_with("node_modules/sub"))
    });
    assert!(!scanned, "node_modules has been scanned");
}

#[test]
fn test_unzipped_source_import_exclusions() {
    let mut ws = workspace();
    ws.import_projects(&["maven/unzipped-sources"]);
    let projects = ws.all_projects(true);
    assert!(projects.is_empty(), "{projects:?}");
}

#[test]
fn test_disable_maven() {
    let mut ws = workspace();
    ws.settings["java"]["import"] = json!({ "maven": { "enabled": false } });
    ws.import_projects(&["eclipse/eclipsemaven"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // 1 eclipse projects
    let eclipse = ws.dir.join("eclipse/eclipsemaven");
    assert!(ws.has_project_at(&eclipse, true));
    assert!(
        !ws.natures(&eclipse).iter().any(|n| n == MAVEN_NATURE),
        "eclipse has the Maven nature"
    );
}

/// `importExistingMavenProject(name)`: initialize the projects again
/// (a server restart on the same jdt.ls workspace) without copying.
fn import_existing_maven_project(ws: &mut Workspace, name: &str) -> PathBuf {
    ws.restart();
    let project = ws.dir.join("maven").join(name);
    ws.assert_is_maven_project(&project);
    project
}

#[test]
fn test_unchanged_project_should_not_be_updated() {
    let mut ws = workspace();
    let name = "salut";
    import_maven_project(&mut ws, name);
    assert_eq!(
        0,
        jobs_named(&mut ws, UPDATE_JOB),
        "New Project should not be updated"
    );
    import_existing_maven_project(&mut ws, name);
    assert_eq!(
        0,
        jobs_named(&mut ws, UPDATE_JOB),
        "Unchanged Project should not be updated"
    );
}

#[test]
fn test_changed_project_should_be_updated() {
    let mut ws = workspace();
    let name = "salut";
    let salut = import_maven_project(&mut ws, name);
    assert_eq!(
        0,
        jobs_named(&mut ws, UPDATE_JOB),
        "New Project should not be updated"
    );
    let pom = salut.join("pom.xml");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
    std::fs::File::options()
        .write(true)
        .open(&pom)
        .unwrap()
        .set_modified(later)
        .unwrap();
    import_existing_maven_project(&mut ws, name);
    assert_eq!(
        1,
        jobs_named(&mut ws, UPDATE_JOB),
        "Changed Project should be updated"
    );
}

#[test]
fn test_preexisting_i_project_same_name() {
    let mut ws = workspace();
    let workspace_dir = ws.dir.parent().unwrap().join("preexistingProjectTest");
    let project_dir = workspace_dir.join("TheSalutProject");
    copy_dir(&fixtures_dir().join("projects/maven/salut"), &project_dir);
    ws.import_root(&workspace_dir);
    ws.wait_for_background_jobs();
    // `updateWorkspaceFolders([workspaceDir], [])` again: the job succeeds.
    ws.client().notify(
        "workspace/didChangeWorkspaceFolders",
        json!({ "event": { "added": [{ "uri": dir_uri(&workspace_dir), "name": "preexistingProjectTest" }], "removed": [] } }),
    );
    ws.wait_for_background_jobs();
    ws.assert_is_maven_project(&project_dir);
}

fn java_project(name: &str, level: &str) {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, name);
    ws.assert_is_java_project(&project);
    assert_eq!(level, ws.java_source_level(&project));
    ws.assert_no_errors(&project);
}

fn preview_project(name: &str, level: &str, expect_preview_error: Option<bool>) {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, name);
    ws.assert_is_java_project(&project);
    assert_eq!(level, ws.java_source_level(&project));
    assert_eq!(json!("enabled"), ws.java_option(&project, ENABLE_PREVIEW));
    assert_eq!(json!("ignore"), ws.java_option(&project, REPORT_PREVIEW));
    match expect_preview_error {
        Some(true) => ws.assert_has_errors(&project, &[PREVIEW_ERROR]),
        Some(false) => ws.assert_no_errors(&project),
        None => {}
    }
}

#[test]
fn test_java9_project() {
    java_project("salut-java9", "9");
}

#[test]
fn test_java110_project() {
    java_project("salut-java110", "10");
}

#[test]
fn test_java10_project() {
    java_project("salut-java10", "10");
}

#[test]
fn test_java11_project() {
    java_project("salut-java11", "11");
}

#[test]
fn test_java12_project() {
    // https://bugs.eclipse.org/bugs/show_bug.cgi?id=549258#c9
    preview_project("salut-java12", "12", Some(true));
}

#[test]
fn test_java13_project() {
    preview_project("salut-java13", "13", Some(true));
}

#[test]
fn test_java14_project() {
    preview_project("salut-java14", "14", Some(true));
}

#[test]
fn test_java15_project() {
    preview_project("salut-java15", "15", Some(true));
}

#[test]
fn test_java16_project() {
    preview_project("salut-java16", "16", Some(true));
}

#[test]
fn test_java17_project() {
    preview_project("salut-java17", "17", Some(true));
}

#[test]
fn test_java18_project() {
    preview_project("salut-java18", "18", None);
}

#[test]
fn test_java19_project() {
    preview_project("salut-java19", "19", Some(true));
}

#[test]
fn test_java20_project() {
    preview_project("salut-java20", "20", Some(true));
}

#[test]
fn test_java21_project() {
    preview_project("salut-java21", "21", Some(true));
}

#[test]
fn test_java22_project() {
    preview_project("salut-java22", "22", Some(true));
}

#[test]
fn test_java23_project() {
    preview_project("salut-java23", "23", Some(true));
}

#[test]
fn test_java24_project() {
    preview_project("salut-java24", "24", Some(true));
}

#[test]
fn test_java25_project() {
    preview_project("salut-java25", "25", Some(true));
}

#[test]
#[ignore = "needs a Java 26 runtime: with only JDK 25 installed the oracle reports 'release 26 is not found in the system'"]
fn test_java26_project() {
    preview_project("salut-java26", "26", Some(false));
}

#[test]
fn test_annotation_processing() {
    let mut ws = workspace();
    let project = import_maven_project(&mut ws, "autovalued");
    ws.assert_is_java_project(&project);
    let autovalue_foo =
        project.join("target/generated-sources/annotations/foo/bar/AutoValue_Foo.java");
    ws.wait_for_background_jobs();
    assert!(
        autovalue_foo.exists(),
        "{} was not generated",
        autovalue_foo.display()
    );
    ws.assert_no_errors(&project);
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
    let mut settings = project::ImportSettings::jdtls_defaults();
    settings.maven_enabled = false;
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let configuration_paths = vec![root.join("pom.xml")];
    settings.maven_enabled = true;
    assert!(project::maven::import(&root, &settings, &ws, Some(&configuration_paths)).is_empty());
}

#[test]
fn avoid_import_duplicated_projects2() {
    let (_tmp, root) = fixture_copy("multi-buildtools");
    let mut settings = project::ImportSettings::jdtls_defaults();
    settings.maven_enabled = false;
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let p = ws
        .projects
        .iter()
        .find(|p| p.location == root)
        .expect("multi-buildtools");
    assert!(p.is_java());
    settings.maven_enabled = true;
    assert!(project::maven::import(&root, &settings, &ws, None).is_empty());
}

// https://github.com/redhat-developer/vscode-java/issues/2712
#[test]
fn test_null_analysis_disabled() {
    let mut ws = workspace();
    ws.settings["java"]["compile"] = json!({ "nullAnalysis": {
        "nonnull": ["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        "nullable": ["org.eclipse.jdt.annotation.Nullable", "javax.annotation.Nonnull"],
        "nonnullbydefault": ["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        "mode": "automatic"
    } });
    let project = import_maven_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    ws.build_workspace(true);
    assert_eq!(
        json!("disabled"),
        ws.java_option(
            &project,
            "org.eclipse.jdt.core.compiler.annotation.nullanalysis"
        )
    );
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/2017
#[test]
fn test_import_modules_with_same_artifact_id() {
    let mut ws = workspace();
    ws.import_projects(&["maven/multimodule-same-artifacts"]);
    let projects = ws.all_projects(true);
    assert_eq!(4, projects.len()); // three projects & parent module
                                   // See MavenProjectImporter.DUPLICATE_ARTIFACT_TEMPLATE
    let names = ws.source_path_project_names();
    assert!(
        names.iter().any(|n| n == "com.example.one-my-app"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|n| n == "com.example.two-my-app"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|n| n == "com.example.three-my-app"),
        "{names:?}"
    );
}

#[test]
fn test_preexisting_i_project_different_name() {
    let mut ws = workspace();
    let project_dir = ws.dir.parent().unwrap().join("testImportDifferentName");
    copy_dir(&fixtures_dir().join("projects/maven/salut"), &project_dir);
    ws.import_root(&project_dir);
    ws.wait_for_background_jobs();
    // `updateWorkspaceFolders([projectDir], [])` again: the job succeeds.
    ws.client().notify(
        "workspace/didChangeWorkspaceFolders",
        json!({ "event": { "added": [{ "uri": dir_uri(&project_dir), "name": "testImportDifferentName" }], "removed": [] } }),
    );
    ws.wait_for_background_jobs();
    ws.assert_is_maven_project(&project_dir);
}

// https://github.com/redhat-developer/vscode-java/issues/3639
#[test]
#[ignore = "m2e fails to configure module1 (no .classpath) and cleanInvalidJavaProjects drops its Java nature during initialization, so over LSP the oracle never shows the one Java project the upstream test sees before calling it; the Rust importer keeps module1 a Java project"]
fn test_invalid_project() {
    let mut ws = workspace();
    ws.import_projects(&["maven/invalid2"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len(), "{projects:?}"); // invalid2 & module1
    let java_projects = ws.all_projects(false);
    assert_eq!(1, java_projects.len(), "{java_projects:?}");
}
