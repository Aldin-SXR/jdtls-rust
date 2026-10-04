//! Port of `org.eclipse.jdt.ls.core.internal.managers.InvisibleProjectImporterTest`.
//!
//! The invisible project is inspected over LSP: `java.project.getAll`
//! (whose URI is the project's real folder), `java.project.getSettings`
//! (natures, source paths, output path, classpath entries) and the
//! published diagnostics.  The static `InvisibleProjectImporter` helpers
//! (`getPackageName`, `JavaFileDetector`) and the exclusion patterns of the
//! source entries (not part of `ProjectClasspathEntry`) are checked on the
//! Rust port in `project::invisible`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

/// `AbstractInvisibleProjectBasedTest.createSourceFolderWithLibs(name, srcDir, addLibs)`.
fn create_source_folder_with_libs(ws: &Workspace, name: &str, src_dir: Option<&str>, add_libs: bool) -> PathBuf {
    let project_folder = ws.dir.parent().unwrap().join(format!("{name}{}", std::process::id()));
    std::fs::create_dir_all(&project_folder).unwrap();
    let source_folder = match src_dir {
        Some(d) if !d.trim().is_empty() => project_folder.join(d),
        _ => project_folder.clone(),
    };
    copy_dir(&fixtures_dir().join("projects/eclipse/source-attachment/src"), &source_folder);
    if add_libs {
        add_libs_to(&project_folder);
    }
    project_folder
}

/// `AbstractInvisibleProjectBasedTest.addLibs`.
fn add_libs_to(project: &Path) {
    let lib = project.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    for jar in ["foo.jar", "foo-sources.jar"] {
        std::fs::copy(fixtures_dir().join("projects/eclipse/source-attachment").join(jar), lib.join(jar)).unwrap();
    }
}

/// Paths of the source entries relative to the linked folder
/// (`entry.getPath().makeRelativeTo(linkFolder.getFullPath())`).
fn relative_source_paths(ws: &mut Workspace, root: &Path) -> Vec<String> {
    let root = canonical(root);
    ws.source_paths(&root)
        .iter()
        .map(|p| PathBuf::from(p))
        .map(|p| p.strip_prefix(&root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string_lossy().into_owned()))
        .collect()
}

#[test]
fn import_incomplete_folder() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("maven/salut/src/main/java/org/sample", Some("Bar.java"));
    assert!(!ws.has_project_at(&root, true));
}

#[test]
fn import_complete_folder() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/lesson1", Some("src/org/samples/HelloWorld.java"));
    assert!(ws.has_project_at(&root, true));
    assert!(ws.natures(&root).iter().any(|n| n == UNMANAGED_FOLDER_NATURE));
    let source_path = canonical(&root.join("src"));
    assert!(ws.source_paths(&root).iter().any(|p| Path::new(p) == source_path));
}

#[test]
fn import_complete_folder_without_trigger_file() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/lesson1", None);
    assert!(!ws.has_project_at(&root, true));
}

#[test]
fn import_partial_maven_folder() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("maven/salut-java11");
    let root = project_folder.join("src");
    ws.import_root_folder(&root, Some("main/java/org/sample/Bar.java"));
    assert!(!ws.has_project_at(&root, true));
}

#[test]
fn import_partial_gradle_folder() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("gradle/gradle-11");
    let root = project_folder.join("src");
    ws.import_root_folder(&root, Some("main/java/foo/bar/Foo.java"));
    assert!(!ws.has_project_at(&root, true));
}

#[test]
fn automatic_jar_detection_lib_under_source() {
    let mut ws = workspace();
    let project_folder = create_source_folder_with_libs(&ws, "automaticJarDetectionLibUnderSource", None, true);
    ws.import_root_folder(&project_folder, Some("Test.java"));
    ws.assert_no_errors(&project_folder);

    // The raw classpath is [JRE container, source, foo.jar]; the settings
    // leave the JRE container out.
    let classpath = ws.classpath_entries(&project_folder);
    assert_eq!(2, classpath.len(), "Unexpected classpath:\n{classpath:#?}");
    assert_eq!(json!(CPE_LIBRARY), classpath[1]["kind"]);
    assert!(classpath[1]["path"].as_str().unwrap().ends_with("/foo.jar"), "{classpath:#?}");
    let class_file = ws.class_file_uri(&invisible_project_name(&canonical(&project_folder)), "foo.bar");
    let attachment = ws.execute("java.project.resolveSourceAttachment", vec![json!(json!({ "classFileUri": class_file }).to_string())]);
    let source = attachment["attributes"]["sourceAttachmentPath"].as_str().unwrap_or("").to_owned();
    assert!(source.ends_with("/foo-sources.jar"), "{attachment}");

    let watchers = ws.watcher_glob_patterns();
    assert_eq!(12, watchers.len(), "{watchers:#?}"); // basic(9) + project(1) + library(1)
    let src_glob_pattern = watchers.iter().find(|w| *w == "**/src/**").unwrap();
    assert!(src_glob_pattern == "**/src/**", "Unexpected source glob pattern: {src_glob_pattern}");
    let name = project_folder.file_name().unwrap().to_string_lossy().into_owned();
    let proj_glob_pattern = watchers.iter().find(|w| w.ends_with(&format!("{name}/**"))).unwrap();
    assert!(proj_glob_pattern.ends_with(&format!("{name}/**")), "Unexpected project glob pattern: {proj_glob_pattern}");
    let lib_glob_pattern = watchers.iter().find(|w| w.ends_with(&format!("{name}/lib/**"))).unwrap();
    assert!(lib_glob_pattern.ends_with(&format!("{name}/lib/**")), "Unexpected library glob pattern: {lib_glob_pattern}");
}

#[test]
fn get_package_name_from_relative_path_of_empty_file() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("singlefile");
    ws.import_root_folder(&project_folder, Some("lesson1/Test.java"));
    assert!(ws.has_project_at(&project_folder, true));
    let root = canonical(&project_folder);
    let java_file = root.join("lesson1/Test.java");
    assert_eq!("lesson1", project::invisible::package_name(&java_file, &root));
}

#[test]
fn get_package_name_from_nearby_non_empty_file() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("singlefile");
    ws.import_root_folder(&project_folder, Some("lesson1/samples/Empty.java"));
    assert!(ws.has_project_at(&project_folder, true));
    let root = canonical(&project_folder);
    let java_file = root.join("lesson1/samples/Empty.java");
    assert_eq!("samples", project::invisible::package_name(&java_file, &root));
}

#[test]
fn get_package_name_in_src_empty_file() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("singlefile");
    ws.import_root_folder(&project_folder, Some("lesson1/src/main/java/demosamples/Empty1.java"));
    assert!(ws.has_project_at(&project_folder, true));
    let root = canonical(&project_folder);
    let java_file = root.join("lesson1/src/main/java/demosamples/Empty1.java");
    assert_eq!("main.java.demosamples", project::invisible::package_name(&java_file, &root));
}

#[test]
fn get_package_name() {
    let mut ws = workspace();
    let project_folder = ws.copy_files("singlefile");
    ws.import_root_folder(&project_folder, Some("Single.java"));
    assert!(ws.has_project_at(&project_folder, true));
    let root = canonical(&project_folder);
    let java_file = root.join("Single.java");
    assert_eq!("", project::invisible::package_name(&java_file, &root));
}

#[test]
#[ignore = "needs a Java 26 default VM (TestVMType.setTestJREAsDefault(\"26\")): only JDK 25 is installed here"]
fn test_preview_features_enabled_by_default() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    assert!(ws.has_project_at(&root, true));
    ws.assert_no_errors(&root);
    assert_eq!(json!("enabled"), ws.java_option(&root, "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures"));
    assert_eq!(json!("ignore"), ws.java_option(&root, "org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures"));
}

/// The second-to-last Java version JDT knows (25) is the installed JDK, so
/// the default VM is already the one `setTestJREAsDefault` selects.
#[test]
fn test_preview_features_disabled_for_not_latest_jdk() {
    let mut ws = workspace();
    let root = ws.copy_and_import_folder("singlefile/lesson1", Some("src/org/samples/HelloWorld.java"));
    assert!(ws.has_project_at(&root, true));
    ws.assert_no_errors(&root);
    assert_eq!(json!("disabled"), ws.java_option(&root, "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures"));
}

fn set_project(ws: &mut Workspace, key: &str, value: Value) {
    ws.settings["java"]["project"][key] = value;
}

#[test]
fn test_specifying_output_path() {
    let mut ws = workspace();
    set_project(&mut ws, "outputPath", json!("output"));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    // `/<name>/_/output`
    let output = ws.project_setting(&dir_uri(&root), OUTPUT_PATH);
    assert_eq!(canonical(&root).join("output").to_string_lossy(), output.as_str().unwrap());
}

/// The source entries' exclusion patterns are not part of the project
/// settings: checked on the Rust model.
#[test]
fn test_specifying_output_path_inside_source_path() {
    let (_tmp, root) = fixture_copy("singlefile/java14");
    let mut settings = import_settings();
    settings.output_path = Some("output".into());
    settings.trigger_files = vec![root.join("foo/bar/Foo.java")];
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let p = ws.projects.iter().find(|p| p.kind == project::ProjectKind::Invisible).expect("invisible project");
    let mut is_output_excluded = false;
    for e in p.classpath.iter().filter(|e| e.kind == project::EntryKind::Source) {
        if e.exclusions.iter().any(|x| x == "output/") {
            is_output_excluded = true;
        }
    }
    assert!(is_output_excluded, "Output path should be excluded from source path");
}

#[test]
fn test_specifying_output_path_equal_to_source_path() {
    let mut ws = workspace();
    set_project(&mut ws, "outputPath", json!("src"));
    ws.copy_and_import_folder("singlefile/simple2", Some("src/App.java"));
    ws.wait_idle();
}

#[test]
fn test_specifying_absolute_output_path() {
    // `assertThrows(CoreException)`: the import fails.
    let (_tmp, root) = fixture_copy("singlefile/simple");
    let mut settings = import_settings();
    settings.output_path = Some(fixtures_dir().join("projects").to_string_lossy().into_owned());
    let ws = project::Workspace::default();
    let r = project::invisible::try_load_invisible_project(&root.join("src/App.java"), &root, &settings, &ws);
    assert_eq!(Err(project::invisible::InvisibleError::AbsoluteOutputPath), r.map(|p| p.is_some()));
}

#[test]
fn test_specifying_empty_output_path() {
    let mut ws = workspace();
    set_project(&mut ws, "outputPath", json!(""));
    let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
    let name = invisible_project_name(&canonical(&root));
    let output = ws.project_setting(&dir_uri(&root), OUTPUT_PATH);
    assert_eq!(canonical(&ws.workspace_project_location(&name)).join("bin").to_string_lossy(), output.as_str().unwrap());
}

#[test]
fn test_specifying_source_paths() {
    let mut ws = workspace();
    set_project(&mut ws, "sourcePaths", json!(["foo", "bar"]));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    let source_paths = relative_source_paths(&mut ws, &root);
    assert_eq!(1, source_paths.len());
    assert!(source_paths.contains(&"foo".to_owned()));
}

#[test]
fn test_specifying_empty_source_paths() {
    let mut ws = workspace();
    set_project(&mut ws, "sourcePaths", json!([]));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    let source_paths = relative_source_paths(&mut ws, &root);
    assert_eq!(0, source_paths.len());
}

#[test]
fn test_specifying_nested_source_paths() {
    let mut ws = workspace();
    set_project(&mut ws, "sourcePaths", json!(["foo", "foo/bar"]));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    let source_paths = relative_source_paths(&mut ws, &root);
    assert_eq!(2, source_paths.len());
    assert!(source_paths.contains(&"foo".to_owned()));
    assert!(source_paths.contains(&"foo/bar".to_owned()));
}

#[test]
fn test_specifying_duplicated_source_paths() {
    let mut ws = workspace();
    set_project(&mut ws, "sourcePaths", json!(["foo", "foo"]));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    let source_paths = relative_source_paths(&mut ws, &root);
    assert_eq!(1, source_paths.len());
    assert!(source_paths.contains(&"foo".to_owned()));
}

#[test]
fn test_specifying_root_as_source_paths() {
    let mut ws = workspace();
    set_project(&mut ws, "sourcePaths", json!([""]));
    let root = ws.copy_and_import_folder("singlefile/java14", Some("foo/bar/Foo.java"));
    let source_paths = relative_source_paths(&mut ws, &root);
    assert_eq!(1, source_paths.len());
    assert!(source_paths.contains(&"".to_owned()));
}

#[test]
fn test_specifying_absolute_source_path() {
    // `assertThrows(CoreException)`: the import fails.
    let (_tmp, root) = fixture_copy("singlefile/simple");
    let mut settings = import_settings();
    settings.source_paths = Some(vec![fixtures_dir().join("projects").to_string_lossy().into_owned()]);
    let ws = project::Workspace::default();
    let r = project::invisible::try_load_invisible_project(&root.join("src/App.java"), &root, &settings, &ws);
    assert_eq!(Err(project::invisible::InvisibleError::AbsoluteSourcePath), r.map(|p| p.is_some()));
}

/// The exclusion patterns of the source entries are checked on the Rust model.
#[test]
fn test_specifying_source_paths_containing_output_path() {
    let (_tmp, root) = fixture_copy("singlefile/java14");
    let mut settings = import_settings();
    settings.source_paths = Some(vec!["".into()]);
    settings.output_path = Some("bin".into());
    settings.trigger_files = vec![root.join("foo/bar/Foo.java")];
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let p = ws.projects.iter().find(|p| p.kind == project::ProjectKind::Invisible).expect("invisible project");
    for e in p.classpath.iter().filter(|e| e.kind == project::EntryKind::Source) {
        assert_eq!("bin/", e.exclusions[0]);
    }
}

fn source_root_count(ws: &mut Workspace, root: &Path) -> usize {
    ws.classpath_entries(root).iter().filter(|e| e["kind"] == json!(CPE_SOURCE)).count()
}

#[test]
fn test_infer_source_root() {
    let mut ws = workspace();
    ws.settings["java"]["import"] = json!({ "exclusions": ["**/excluded"] });
    let root = ws.copy_and_import_folder("singlefile/inferSourceRoot", Some("lesson1/Lesson1.java"));
    ws.wait_idle();
    assert_eq!(3, source_root_count(&mut ws, &root));

    // `InvisibleProjectImporter.inferSourceRoot` runs when the file is opened.
    let undiscovered = root.join("a/very/deep/path/Source.java");
    let uri = file_uri(&undiscovered);
    ws.open(&uri);
    ws.wait_idle();
    assert_eq!(4, source_root_count(&mut ws, &root));

    let markers = ws.error_markers(&root);
    assert_eq!(0, markers.len(), "{}", markers_to_string(&markers));
}

#[test]
fn test_infer_source_root2() {
    let mut ws = workspace();
    ws.settings["java"]["import"] = json!({ "exclusions": ["**/excluded"] });
    let root = ws.copy_and_import_folder("singlefile/inferSourceRoot", Some("Main.java"));
    ws.wait_idle();
    assert_eq!(3, source_root_count(&mut ws, &root));

    let undiscovered = root.join("a/very/deep/path/Source.java");
    let uri = file_uri(&undiscovered);
    ws.open(&uri);
    ws.wait_idle();
    assert_eq!(4, source_root_count(&mut ws, &root));

    let markers = ws.error_markers(&root);
    assert_eq!(0, markers.len(), "{}", markers_to_string(&markers));
}

#[test]
fn java_file_detector_test() {
    // `createMockProject()`: a project "mock" linking
    // `invisibleFileDetector/other-project/Other.java`.
    let root = fixtures_dir().join("projects/singlefile/invisibleFileDetector");
    let mock_link_parent = root.join("other-project");
    let mut folders_to_search: Vec<PathBuf> =
        std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders_to_search.sort();
    let detector = project::invisible::JavaFileDetector::with_exclusions(vec!["**/excluded".into()], vec![mock_link_parent]);
    let trigger_files = detector.scan(&folders_to_search);
    assert_eq!(0, trigger_files.len(), "{trigger_files:?}");
}

fn import_settings() -> project::ImportSettings {
    let mut s = project::ImportSettings::jdtls_defaults();
    s.data_dir = Some(std::env::temp_dir().join("jdtls-rust-invisible-test"));
    s
}

fn fixture_copy(rel: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join(rel);
    copy_dir(&fixtures_dir().join("projects").join(rel), &root);
    (tmp, root)
}
