//! Port of `org.eclipse.jdt.ls.core.internal.preferences.NullAnalysisTest`.
//!
//! Upstream sets the null-analysis preferences on the preference manager
//! before importing; over LSP they are the `java.compile.nullAnalysis.*`
//! settings the server starts with. Upstream then calls
//! `Preferences.updateAnnotationNullAnalysisOptions()` and rebuilds when it
//! reports a change; jdt.ls makes that call itself once the projects are
//! imported (`projectsBuildFinished`), so the tests force a workspace build
//! and observe the markers (published diagnostics) and project options
//! (`java.project.getSettings`). Where upstream asserts the method's result
//! (`testMissingNonNull`), the observable is whether the project's
//! `annotation.nullanalysis` option was switched on.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const COMPILER_ANNOTATION_NULL_ANALYSIS: &str = "org.eclipse.jdt.core.compiler.annotation.nullanalysis";
const COMPILER_NONNULL_BY_DEFAULT_ANNOTATION_NAME: &str = "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault";
const COMPILER_COMPLIANCE: &str = "org.eclipse.jdt.core.compiler.compliance";
const COMPILER_CODEGEN_TARGET_PLATFORM: &str = "org.eclipse.jdt.core.compiler.codegen.targetPlatform";
const COMPILER_SOURCE: &str = "org.eclipse.jdt.core.compiler.source";

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    // AbstractProjectsManagerBasedTest.initPreferences
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

/// `setNonnullTypes`, `setNullableTypes`, `setNonnullbydefaultTypes` and
/// (when given) `setNullAnalysisMode`.
fn set_null_analysis(ws: &mut Workspace, nonnull: &[&str], nullable: &[&str], nonnullbydefault: &[&str], mode: Option<&str>) {
    let mut null_analysis = json!({
        "nonnull": nonnull,
        "nullable": nullable,
        "nonnullbydefault": nonnullbydefault,
    });
    if let Some(mode) = mode {
        null_analysis["mode"] = json!(mode);
    }
    ws.settings["java"]["compile"] = json!({ "nullAnalysis": null_analysis });
}

/// `AbstractGradleBasedTest.importGradleProject(name)`.
fn import_gradle_project(ws: &mut Workspace, name: &str) -> PathBuf {
    ws.import_projects(&[&format!("gradle/{name}")]);
    let project = ws.dir.join("gradle").join(name);
    let natures = ws.natures(&project);
    assert!(natures.iter().any(|n| n == GRADLE_NATURE), "{name} is missing the Gradle nature");
    project
}

/// `AbstractProjectsManagerBasedTest.getWarningMarker(project, message)`.
fn get_warning_marker(ws: &mut Workspace, project: &Path, message: &str) -> Option<(String, Value)> {
    ws.warning_markers(project).into_iter().find(|(_, d)| d["message"] == message)
}

/// `((IFile) marker.getResource()).getFullPath().lastSegment()`.
fn last_segment(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap()
}

/// `if (updateAnnotationNullAnalysisOptions()) buildWorkspace(true)`.
fn update_annotation_null_analysis_options(ws: &mut Workspace) {
    ws.build_workspace(true);
}

#[test]
fn test_null_analysis_with_javax() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["javax.annotation.Nullable", "org.eclipse.jdt.annotation.Nullable"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        Some("automatic"),
    );
    let project = import_gradle_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    update_annotation_null_analysis_options(&mut ws);
    let marker = get_warning_marker(&mut ws, &project, "Null type mismatch: required '@Nonnull String' but the provided value is null");
    assert!(marker.is_some(), "{}", markers_to_string(&ws.warning_markers(&project)));
    let marker = marker.unwrap();
    assert_eq!("TestJavax.java", last_segment(&marker.0));
    let marker1 = get_warning_marker(&mut ws, &project, "Potential null pointer access: The method nullable() may return null");
    assert_eq!("TestJavax.java", last_segment(&marker1.unwrap().0));
    let marker2 = get_warning_marker(
        &mut ws,
        &project,
        "The return type is incompatible with '@Nonnull String' returned from TestJavax.A.nonnullMethod() (mismatching null constraints)",
    );
    assert_eq!("TestJavax.java", last_segment(&marker2.unwrap().0));
    let marker3 = get_warning_marker(
        &mut ws,
        &project,
        "Null type mismatch: required '@Nonnull List<String>' but the provided value is specified as @Nullable",
    );
    assert_eq!("TestJavax.java", last_segment(&marker3.unwrap().0));
    // See https://github.com/redhat-developer/vscode-java/issues/3255
    let marker4 = get_warning_marker(&mut ws, &project, "Potential null pointer access: The field obj is specified as @Nullable");
    assert!(marker4.is_none());
    ws.assert_no_errors(&project);
}

#[test]
fn test_mixed_null_analysis() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["org.eclipse.jdt.annotation.Nullable", "javax.annotation.Nonnull"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        Some("automatic"),
    );
    let project = import_gradle_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    update_annotation_null_analysis_options(&mut ws);
    let marker = get_warning_marker(&mut ws, &project, "Null type mismatch: required '@Nonnull String' but the provided value is null");
    assert!(marker.is_some(), "{}", markers_to_string(&ws.warning_markers(&project)));
    assert_eq!("TestJavax.java", last_segment(&marker.unwrap().0));
    let marker1 = get_warning_marker(&mut ws, &project, "Potential null pointer access: The method nullable() may return null");
    assert_eq!("TestJDT.java", last_segment(&marker1.unwrap().0));
    let marker2 = get_warning_marker(
        &mut ws,
        &project,
        "The return type is incompatible with '@Nonnull String' returned from TestJavax.A.nonnullMethod() (mismatching null constraints)",
    );
    assert!(marker2.is_some());
    assert_eq!("TestJavax.java", last_segment(&marker2.unwrap().0));
    let marker3 = get_warning_marker(
        &mut ws,
        &project,
        "Null type safety: The expression of type 'List<String>' needs unchecked conversion to conform to '@Nonnull List<String>'",
    );
    assert!(marker3.is_some());
    assert_eq!("TestJavax.java", last_segment(&marker3.unwrap().0));
    ws.assert_no_errors(&project);
}

#[test]
fn test_null_analysis_disabled() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["javax.annotation.Nullable", "org.eclipse.jdt.annotation.Nullable"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        Some("disabled"),
    );
    // ResourceUtils.getWarningMarkers counts the project's "no explicit
    // encoding set" marker, which jdt.ls only publishes
    // (WorkspaceDiagnosticsHandler.isIgnored) when `java.project.encoding`
    // is `warning`.
    ws.settings["java"]["project"] = json!({ "encoding": "warning" });
    let project = import_gradle_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    update_annotation_null_analysis_options(&mut ws);
    let warning_markers = ws.warning_markers(&project);
    assert_eq!(3, warning_markers.len(), "{}", markers_to_string(&warning_markers));
    ws.assert_no_errors(&project);
}

#[test]
fn test_keep_existing_project_options() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["javax.annotation.Nullable", "org.eclipse.jdt.annotation.Nullable"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        None,
    );
    let project = import_gradle_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    update_annotation_null_analysis_options(&mut ws);
    // sourceCompatibility = '11' defined in project null-analysis build.gradle
    assert_eq!(json!("11"), ws.java_option(&project, COMPILER_COMPLIANCE));
    assert_eq!(json!("11"), ws.java_option(&project, COMPILER_CODEGEN_TARGET_PLATFORM));
    assert_eq!(json!("11"), ws.java_option(&project, COMPILER_SOURCE));
}

#[test]
fn test_nonnullby_default() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["org.eclipse.jdt.annotation.Nullable", "javax.annotation.Nonnull"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        Some("automatic"),
    );
    let project = import_gradle_project(&mut ws, "null-analysis");
    ws.assert_is_java_project(&project);
    update_annotation_null_analysis_options(&mut ws);
    assert_eq!(
        json!("org.eclipse.jdt.annotation.NonNullByDefault"),
        ws.java_option(&project, COMPILER_NONNULL_BY_DEFAULT_ANNOTATION_NAME)
    );
    let file = project.join("src/main/java/org/sample/Test.java");
    assert!(file.exists());
    let file_uri = canonical(&file);
    let markers_of = |ws: &mut Workspace| {
        ws.all_markers(&project)
            .into_iter()
            .filter(|(uri, _)| tower_lsp::lsp_types::Url::parse(uri).unwrap().to_file_path().map(|p| canonical(&p)).ok() == Some(file_uri.clone()))
            .collect::<Vec<_>>()
    };
    let markers = markers_of(&mut ws);
    assert_eq!(1, markers.len(), "{}", markers_to_string(&markers));
    let marker = get_warning_marker(&mut ws, &project, "The @Nonnull field count may not have been initialized");
    assert!(marker.is_some());
    let package_info = project.join("src/main/java/org/sample/package-info.java");
    assert!(package_info.exists());
    let contents = "package org.sample;\n";
    std::fs::write(&package_info, contents).unwrap();
    ws.files_changed(&[(&package_info, 2)]);
    ws.build_workspace(true);
    let markers = markers_of(&mut ws);
    assert_eq!(0, markers.len(), "{}", markers_to_string(&markers));
    let marker = get_warning_marker(&mut ws, &project, "The @Nonnull field count may not have been initialized");
    assert!(marker.is_none());
    ws.assert_no_errors(&project);
}

// https://github.com/redhat-developer/vscode-java/issues/3387
#[test]
fn test_missing_non_null() {
    let mut ws = workspace();
    set_null_analysis(
        &mut ws,
        &["javax.annotation.Nonnull", "org.eclipse.jdt.annotation.NonNull"],
        &["javax.annotation.Nullable", "org.eclipse.jdt.annotation.Nullable"],
        &["org.eclipse.jdt.annotation.NonNullByDefault", "javax.annotation.ParametersAreNonnullByDefault"],
        Some("automatic"),
    );
    ws.import_projects(&["eclipse/testnullable3"]);
    let project = ws.dir.join("eclipse").join("testnullable3");
    ws.assert_is_java_project(&project);
    ws.wait_for_background_jobs();
    let updated = ws.java_option(&project, COMPILER_ANNOTATION_NULL_ANALYSIS) == json!("enabled");
    assert!(!updated);
    ws.import_projects(&["eclipse/testnullable2"]);
    let project = ws.dir.join("eclipse").join("testnullable2");
    // jdt.ls runs updateAnnotationNullAnalysisOptions() for every project
    // once the projects are imported at startup (projectsBuildFinished), not
    // when a workspace folder is added later: restart the server on the same
    // workspace so that the call covers the newly imported project.
    ws.restart();
    ws.assert_is_java_project(&project);
    let updated = ws.java_option(&project, COMPILER_ANNOTATION_NULL_ANALYSIS) == json!("enabled");
    assert!(updated);
}
