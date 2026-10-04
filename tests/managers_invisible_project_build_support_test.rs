//! Port of `org.eclipse.jdt.ls.core.internal.managers.InvisibleProjectBuildSupportTest`.
//!
//! `projectsManager.fileChanged` is a `workspace/didChangeWatchedFiles`
//! notification, `UpdateClasspathJob.updateClasspath(project, libraries)` a
//! `java.project.referencedLibraries` settings change; the raw classpath is
//! read from `java.project.getSettings` (which leaves the JRE container out)
//! and source attachments from `java.project.resolveSourceAttachment`.
//! `Preferences.createFrom`/`ReferencedLibraries` are checked on the Rust
//! port (`project::ReferencedLibraries`).

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CREATED: u32 = 1;
const DELETED: u32 = 3;

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

fn create_source_folder_with_missing_libs(ws: &Workspace, name: &str) -> PathBuf {
    let project_folder = ws.dir.parent().unwrap().join(format!("{name}{}", std::process::id()));
    std::fs::create_dir_all(&project_folder).unwrap();
    copy_dir(&fixtures_dir().join("projects/eclipse/source-attachment/src"), &project_folder);
    canonical(&project_folder)
}

fn add_libs(project: &Path) {
    let lib = project.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    for jar in ["foo.jar", "foo-sources.jar"] {
        std::fs::copy(fixtures_dir().join("projects/eclipse/source-attachment").join(jar), lib.join(jar)).unwrap();
    }
}

fn origin(name: &str) -> PathBuf {
    fixtures_dir().join("projects/eclipse/source-attachment").join(name)
}

/// The library entries of the project at `root`.
fn libraries(ws: &mut Workspace, root: &Path) -> Vec<Value> {
    ws.classpath_entries(root).into_iter().filter(|e| e["kind"] == json!(CPE_LIBRARY)).collect()
}

fn last_segment(v: &Value) -> String {
    Path::new(v.as_str().unwrap_or("")).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `getSourceAttachmentPath()` of the library holding `fqn`.
fn source_attachment(ws: &mut Workspace, root: &Path, fqn: &str) -> Option<String> {
    ws.wait_for_background_jobs();
    let project = invisible_project_name(root);
    let class_file = ws.class_file_uri(&project, fqn);
    let r = ws.execute("java.project.resolveSourceAttachment", vec![json!(json!({ "classFileUri": class_file }).to_string())]);
    r["attributes"]["sourceAttachmentPath"].as_str().map(str::to_owned)
}

fn set_referenced_libraries(ws: &mut Workspace, libraries: Value) {
    let mut settings = ws.settings.clone();
    settings["java"]["project"]["referencedLibraries"] = libraries;
    ws.update_settings(settings);
    ws.wait_for_background_jobs();
}

#[test]
fn test_dynamic_lib_detection() {
    let mut ws = workspace();
    let project_folder = create_source_folder_with_missing_libs(&ws, "dynamicLibDetection");
    ws.import_root_folder(&project_folder, Some("Test.java"));
    let errors = ws.error_markers(&project_folder);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));

    //Add jars to fix compilation errors
    add_libs(&project_folder);
    let lib_path = project_folder.join("lib");
    let jar = lib_path.join("foo.jar");
    ws.files_changed(&[(&jar, CREATED)]);
    {
        // raw classpath: [JRE container, source, foo.jar]
        ws.wait_for_background_jobs();
        let classpath = ws.classpath_entries(&project_folder);
        assert_eq!(2, classpath.len(), "Unexpected classpath:\n{classpath:#?}");
        assert_eq!("foo.jar", last_segment(&classpath[1]["path"]));
        assert_eq!(Some("foo-sources.jar".to_owned()), source_attachment(&mut ws, &project_folder, "foo.bar").map(|s| last_segment(&json!(s))));
    }

    //remove sources
    let sources = lib_path.join("foo-sources.jar");
    std::fs::remove_file(&sources).ok();
    ws.files_changed(&[(&sources, DELETED)]);
    {
        let classpath = ws.classpath_entries(&project_folder);
        assert_eq!(2, classpath.len(), "Unexpected classpath:\n{classpath:#?}");
        assert_eq!("foo.jar", last_segment(&classpath[1]["path"]));
        assert_eq!(None, source_attachment(&mut ws, &project_folder, "foo.bar"));
    }
    ws.assert_no_errors(&project_folder);

    //remove lib folder
    std::fs::remove_file(&jar).ok(); //lib needs to be empty
    std::fs::remove_dir(&lib_path).ok();
    ws.files_changed(&[(&lib_path, DELETED)]);
    {
        let classpath = ws.classpath_entries(&project_folder);
        assert_eq!(1, classpath.len(), "Unexpected classpath:\n{classpath:#?}");
    }
    //back to square 1
    let errors = ws.error_markers(&project_folder);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
}

#[test]
#[ignore = "asserts how often UpdateClasspathJob is scheduled (debouncing), an internal job count with no LSP-visible effect"]
fn test_debounce_jar_detection() {
    let mut ws = workspace();
    let project_folder = create_source_folder_with_missing_libs(&ws, "dynamicLibDetection");
    ws.import_root_folder(&project_folder, Some("Test.java"));
    let errors = ws.error_markers(&project_folder);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
    add_libs(&project_folder);
    let lib_path = project_folder.join("lib");
    for _ in 0..50 {
        ws.files_changed(&[(&lib_path.join("foo.jar"), CREATED), (&lib_path.join("foo-sources.jar"), CREATED)]);
    }
    let classpath = ws.classpath_entries(&project_folder);
    assert_eq!(2, classpath.len(), "Unexpected classpath:\n{classpath:#?}");
    assert_eq!("foo.jar", last_segment(&classpath[1]["path"]));
}

#[test]
#[ignore = "asserts that the lib detection request and a third-party UpdateClasspathJob request are merged into one job run (internal job count with no LSP equivalent)"]
fn test_manually_reference_libraries() {
    let mut ws = workspace();
    let project_folder = create_source_folder_with_missing_libs(&ws, "dynamicLibDetection");
    ws.import_root_folder(&project_folder, Some("Test.java"));
    let errors = ws.error_markers(&project_folder);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
}

#[test]
fn test_variable_reference_libraries() {
    let home = std::env::var("HOME").unwrap();
    let libraries = project::ReferencedLibraries::from_setting(&json!({
        "include": ["~/lib/foo.jar"],
        "exclude": ["~/lib/bar.jar"],
        "sources": { "~/library/bar.jar": "~/library/sources/bar-src.jar" }
    }))
    .unwrap();
    assert!(libraries.include[0].starts_with(&home));
    assert!(libraries.exclude[0].starts_with(&home));
    for (k, v) in &libraries.sources {
        assert!(k.starts_with(&home));
        assert!(v.starts_with(&home));
    }
    // `${java.home}` is the java.home system property of the server's JVM.
    std::env::set_var("JDTLS_JAVA_HOME", java_home());
    let jh = java_home();
    let libraries = project::ReferencedLibraries::from_setting(&json!({
        "include": ["${java.home}/lib/foo.jar"],
        "exclude": ["${java.home}/lib/bar.jar"],
        "sources": { "${java.home}/library/bar.jar": "${java.home}/library/sources/bar-src.jar" }
    }))
    .unwrap();
    assert!(libraries.include[0].starts_with(&jh));
    assert!(libraries.exclude[0].starts_with(&jh));
    for (k, v) in &libraries.sources {
        assert!(k.starts_with(&jh));
        assert!(v.starts_with(&jh));
    }
    let libraries = project::ReferencedLibraries::from_setting(&json!({ "include": ["${foo}"] })).unwrap();
    assert_eq!("${foo}", libraries.include[0]);
}

#[test]
#[ignore = "asserts the number of UpdateClasspathJob runs (1, 2, then 2 for an excluded jar), an internal job count with no LSP-visible effect"]
fn test_dynamic_reference_libraries() {
    let mut ws = workspace();
    let project_folder = create_source_folder_with_missing_libs(&ws, "dynamicLibDetection");
    ws.import_root_folder(&project_folder, Some("Test.java"));
    let errors = ws.error_markers(&project_folder);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
}

fn libs(include: &[&str], exclude: &[&str], sources: &[(&str, &str)]) -> project::ReferencedLibraries {
    let set = |v: &[&str]| -> Vec<String> {
        let mut out: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        out.sort();
        out.dedup();
        out
    };
    project::ReferencedLibraries {
        include: set(include),
        exclude: set(exclude),
        sources: sources.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

fn preferences(configuration: Value) -> project::ReferencedLibraries {
    let mut r = project::ImportSettings::from_settings(Some(&configuration)).referenced_libraries;
    r.include.sort();
    r.exclude.sort();
    r
}

#[test]
fn test_import_referenced_libraries_configuration() {
    {
        // Test import of configuration without specifying referenced libraries
        let libraries = project::ReferencedLibraries::jdtls_default();
        assert_eq!(libraries, preferences(json!({})), "Configuration with no corresponding field");
    }
    let key = |v: Value| json!({ "java.project.referencedLibraries": v });
    {
        // Test import of referenced libraries with a shortcut list
        assert_eq!(libs(&["libraries/**/*.jar"], &[], &[]), preferences(key(json!(["libraries/**/*.jar"]))), "Configuration with shortcut include array");
    }
    {
        // Test import of referenced libraries object with include
        assert_eq!(libs(&["libraries/**/*.jar"], &[], &[]), preferences(key(json!({ "include": ["libraries/**/*.jar"] }))), "Configuration with include");
    }
    {
        // Test import of referenced libraries object with include and exclude
        assert_eq!(
            libs(&["libraries/**/*.jar"], &["libraries/sources/**"], &[]),
            preferences(key(json!({ "include": ["libraries/**/*.jar"], "exclude": ["libraries/sources/**"] }))),
            "Configuration with include and exclude"
        );
    }
    let sources = [("libraries/foo.jar", "libraries/foo-src.jar")];
    {
        // Test import of referenced libraries object with include and sources
        assert_eq!(
            libs(&["libraries/**/*.jar"], &[], &sources),
            preferences(key(json!({ "include": ["libraries/**/*.jar"], "sources": { "libraries/foo.jar": "libraries/foo-src.jar" } }))),
            "Configuration with include and sources"
        );
    }
    {
        // Test import of referenced libraries object with include, exclude and sources
        assert_eq!(
            libs(&["libraries/**/*.jar"], &["libraries/sources/**"], &sources),
            preferences(key(json!({
                "include": ["libraries/**/*.jar"],
                "exclude": ["libraries/sources/**"],
                "sources": { "libraries/foo.jar": "libraries/foo-src.jar" }
            }))),
            "Configuration with include, exclude and sources"
        );
    }
    {
        // Test import of referenced libraries with exclude and sources
        assert_eq!(
            libs(&[], &["libraries/sources/**"], &sources),
            preferences(key(json!({ "exclude": ["libraries/sources/**"], "sources": { "libraries/foo.jar": "libraries/foo-src.jar" } }))),
            "Configuration with exclude and sources"
        );
    }
    {
        // Test import of referenced libraries with only exclude
        assert_eq!(libs(&[], &["libraries/sources/**"], &[]), preferences(key(json!({ "exclude": ["libraries/sources/**"] }))), "Configuration with exclude");
    }
    {
        // Test import of referenced libraries with only sources
        assert_eq!(
            libs(&[], &[], &sources),
            preferences(key(json!({ "sources": { "libraries/foo.jar": "libraries/foo-src.jar" } }))),
            "Configuration with sources"
        );
    }
}

#[test]
fn test_update_referenced_libraries() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    let uri = file_uri(&root.join("src/App.java"));
    ws.open(&uri);
    let definitions = ws.request("textDocument/definition", json!({ "textDocument": { "uri": uri }, "position": pos(0, 13) }));

    // The original mylib.jar is an empty jar, so the GTD is not available
    assert_eq!(0, definitions.as_array().map_or(0, |a| a.len()));

    // replace it which contains the class 'mylib.A'
    let project_real_path = canonical(&root);
    let new_lib_path = project_real_path.join("mylib.jar");
    let referenced_library_path = project_real_path.join("lib/mylib.jar");
    std::fs::copy(&new_lib_path, &referenced_library_path).unwrap();

    set_referenced_libraries(&mut ws, json!(["lib/**/*.jar"]));

    let definitions = ws.request("textDocument/definition", json!({ "textDocument": { "uri": uri }, "position": pos(0, 13) }));
    assert_eq!(1, definitions.as_array().map_or(0, |a| a.len()), "{definitions}");
}

#[test]
#[ignore = "needs jdt.ls' source discovery for unmanaged jars (MavenSourceDownloader: Maven Central SHA-1 search and -sources.jar download), not ported yet"]
fn test_dynamic_source_lookups() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/downloadSources", Some("UsingRemark.java"));
    ws.assert_no_errors(&root);
}
