//! Port of `org.eclipse.jdt.ls.core.internal.managers.GradleProjectImporterTest`.
//!
//! The project model is observed through `java.project.getAll`/`getSettings`/
//! `listSourcePaths`, the published diagnostics and the Buildship preference
//! files, like the other importer tests. The calls into `GradleProjectImporter`,
//! `GradleUtils` and `BuildConfiguration` are unit-level ports of
//! `project::gradle`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::gradle::*;
use common::jdtls::*;
use common::maven::assert_task_completed;
use common::projects::*;
use project::gradle::config::{self, GradleDistribution, GradleSettings};
use project::gradle::util;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const GRADLE1_PATTERN: &str = "**/gradle1";
const ENABLE_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures";
const REPORT_PREVIEW: &str = "org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures";

fn project_dir(ws: &mut Workspace, name: &str) -> Option<PathBuf> {
    ws.project_locations(true)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name))
}

fn root_dir() -> PathBuf {
    std::env::temp_dir()
}

#[test]
fn import_simple_gradle_project() {
    let mut ws = workspace();
    import_simple_java_project(&mut ws);
    assert_task_completed(&mut ws, "Importing Gradle project(s)");
}

#[test]
fn import_nested_gradle_project() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/nested"]);
    let projects = ws.all_projects(true);
    assert_eq!(3, projects.len()); // 3 gradle projects
    let gradle1 = project_dir(&mut ws, "gradle1").unwrap();
    assert_is_gradle_project(&mut ws, &gradle1);
    let gradle2 = project_dir(&mut ws, "gradle2").unwrap();
    assert_is_gradle_project(&mut ws, &gradle2);
    let gradle3 = project_dir(&mut ws, "gradle3").unwrap();
    assert_is_gradle_project(&mut ws, &gradle3);
    assert!(!ws.natures(&gradle3).iter().any(|n| n == JAVA_NATURE));
}

#[test]
fn test_delete_invalid_projects() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/nested/gradle1", "gradle/nested/gradle2"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len()); // 2 gradle projects
    let gradle1 = project_dir(&mut ws, "gradle1").unwrap();
    assert_is_gradle_project(&mut ws, &gradle1);
    let gradle2 = project_dir(&mut ws, "gradle2").unwrap();
    assert_is_gradle_project(&mut ws, &gradle2);

    ws.remove_root(&gradle2);
    ws.import_projects(&["gradle/nested/gradle1"]);
    ws.wait_for_background_jobs();
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len());
    assert!(project_dir(&mut ws, "gradle1").is_some());
    assert!(project_dir(&mut ws, "gradle2").is_none());
}

#[test]
fn test_java_import_exclusions() {
    let mut ws = workspace();
    let mut exclusions: Vec<String> = project::detect::DEFAULT_IMPORT_EXCLUSIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    exclusions.push(GRADLE1_PATTERN.to_owned());
    ws.settings["java"]["import"] = json!({ "exclusions": exclusions });
    ws.import_projects(&["gradle/nested"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len()); // 2 gradle projects
    assert!(project_dir(&mut ws, "gradle1").is_none());
    let gradle2 = project_dir(&mut ws, "gradle2").unwrap();
    assert_is_gradle_project(&mut ws, &gradle2);
    let gradle3 = project_dir(&mut ws, "gradle3").unwrap();
    assert_is_gradle_project(&mut ws, &gradle3);
}

#[test]
fn test_disable_gradle_wrapper() {
    let mut gradle = GradleSettings::default();
    let required_version = "8.5";
    let file = fixtures_dir().join("projects/gradle/simple-gradle");
    assert!(file.is_dir());
    let distribution = config::get_gradle_distribution(&file, &gradle);
    assert_eq!(GradleDistribution::Wrapper, distribution);
    gradle.wrapper_enabled = false;
    let distribution = config::get_gradle_distribution(&file, &gradle);
    if let Some(home) = config::get_gradle_home_file_default(&gradle) {
        assert_eq!(GradleDistribution::Local(home), distribution);
    } else {
        assert_eq!(config::default_distribution(), distribution);
    }
    gradle.version = Some(required_version.to_owned());
    let distribution = config::get_gradle_distribution(&file, &gradle);
    assert_eq!(
        GradleDistribution::FixedVersion(required_version.to_owned()),
        distribution
    );

    let mut ws = workspace();
    ws.settings["java"]["import"] = json!({ "gradle": {
        "wrapper": { "enabled": false }, "version": required_version } });
    ws.import_projects(&["eclipse/eclipsegradle"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // 1 eclipse project
    let eclipse = project_dir(&mut ws, "eclipsegradle").unwrap();
    assert_is_gradle_project(&mut ws, &eclipse);
}

#[test]
fn test_gradle_user_home() {
    let mut ws = workspace();
    let gradle_user_home = tempfile::Builder::new()
        .prefix("gradleUserHome")
        .tempdir()
        .unwrap();
    let home = gradle_user_home.path().canonicalize().unwrap();
    ws.settings["java"]["import"] =
        json!({ "gradle": { "user": { "home": home.to_string_lossy() } } });
    ws.import_projects(&["gradle/simple-gradle"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // 1 eclipse project
    let project = project_dir(&mut ws, "simple-gradle").unwrap();
    assert_is_gradle_project(&mut ws, &project);
    assert!(home.exists());
    let configuration = load_project_configuration(&project);
    assert_eq!(
        Some(home.to_string_lossy().into_owned()),
        configuration.gradle_user_home
    );
}

#[test]
fn test_java_home() {
    let mut prefs = GradleSettings::default();
    let registry = project::runtime::RuntimeRegistry::with_default_home(Path::new(&java_home()));
    let vm = registry.default_install().unwrap().clone();
    prefs.default_vm = Some(vm.home.clone());
    let java_home = fixtures_dir().join("fakejdk").join("11");
    prefs.java_home_preference = Some(java_home.to_string_lossy().into_owned());
    let root_folder = root_dir().join("projects/gradle/simple-gradle");
    let build = config::get_build_configuration(&root_folder, &prefs);
    assert_eq!(Some(vm.home), build.java_home);
}

#[test]
fn test_gradle_java_home() {
    let mut prefs = GradleSettings::default();
    let registry = project::runtime::RuntimeRegistry::with_default_home(Path::new(&java_home()));
    let vm = registry.default_install().unwrap().clone();
    prefs.default_vm = Some(vm.home.clone());
    let java_home = fixtures_dir()
        .join("fakejdk")
        .join("1.8")
        .to_string_lossy()
        .into_owned();
    prefs.java_home = Some(java_home.clone());
    let root_folder = root_dir().join("projects/gradle/simple-gradle");
    let build = config::get_build_configuration(&root_folder, &prefs);
    assert_eq!(
        Some(java_home),
        build.java_home.map(|h| h.to_string_lossy().into_owned())
    );
}

#[test]
fn test_disable_import_gradle() {
    let mut ws = workspace();
    ws.settings["java"]["import"] = json!({ "gradle": { "enabled": false } });
    ws.import_projects(&["eclipse/eclipsegradle"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // 1 eclipse projects
    let eclipse = ws.project_root("eclipse");
    assert!(ws.has_project_at(&eclipse, true));
    assert!(
        !ws.natures(&eclipse).iter().any(|n| n == GRADLE_NATURE),
        "eclipse has the Gradle nature"
    );
}

fn fixture_copy(rel: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join(rel);
    copy_dir(&fixtures_dir().join("projects").join(rel), &root);
    (tmp, root)
}

#[test]
fn test_gradle_persistence() {
    let (tmp, root) = fixture_copy("gradle/nested");
    let state = tmp.path().canonicalize().unwrap().join("state");
    let mut settings = project::ImportSettings::jdtls_defaults();
    settings.gradle.scripts_dir = Some(state.join("scripts"));
    let workspace = project::Workspace::import(&[root.clone()], &settings);
    let gradle_projects: Vec<&project::Project> = workspace
        .projects
        .iter()
        .filter(|p| p.has_nature(project::GRADLE_NATURE))
        .collect();
    assert!(!gradle_projects.is_empty());
    for p in &gradle_projects {
        assert!(
            project::gradle::persistence::should_synchronize(&workspace, &p.location, &state, &settings.metadata),
            "{} should synchronize",
            p.name
        );
    }
    project::gradle::persistence::save_models(&workspace, &state);
    for p in &gradle_projects {
        assert!(
            !project::gradle::persistence::should_synchronize(&workspace, &p.location, &state, &settings.metadata),
            "{} should not synchronize",
            p.name
        );
    }
    let gradle1 = workspace.project("gradle1").unwrap();
    let gradle_build = gradle1.location.join("build.gradle");
    std::fs::File::options()
        .write(true)
        .open(&gradle_build)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(1))
        .unwrap();
    assert!(project::gradle::persistence::should_synchronize(
        &workspace,
        &gradle1.location,
        &state,
        &settings.metadata
    ));
}

#[test]
fn test_workspace_settings() {
    let env: HashMap<String, String> = HashMap::new();
    let mut sysprops: HashMap<String, String> = HashMap::new();
    let prefs = GradleSettings::default();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().canonicalize().unwrap().join("fakeGradleHome");
    sysprops.insert(
        config::GRADLE_HOME.to_owned(),
        file.to_string_lossy().into_owned(),
    );
    let override_workspace_settings = config::get_gradle_home_file(&prefs, &env, &sysprops).is_some();
    assert!(!override_workspace_settings);
    std::fs::create_dir(&file).unwrap();
    let override_workspace_settings = config::get_gradle_home_file(&prefs, &env, &sysprops).is_some();
    assert!(override_workspace_settings);
}

#[test]
fn test_gradle_home() {
    let env: HashMap<String, String> = HashMap::new();
    let mut sysprops: HashMap<String, String> = HashMap::new();
    let mut prefs = GradleSettings::default();
    prefs.default_vm = None;
    let tmp = tempfile::tempdir().unwrap();
    let root_file = tmp.path().canonicalize().unwrap();
    let file = root_file.join("fakeGradleHome");
    sysprops.insert(
        config::GRADLE_HOME.to_owned(),
        file.to_string_lossy().into_owned(),
    );
    let override_workspace_settings = config::get_gradle_home_file(&prefs, &env, &sysprops).is_some();
    assert!(!override_workspace_settings);
    std::fs::create_dir(&file).unwrap();
    let override_workspace_settings = config::get_gradle_home_file(&prefs, &env, &sysprops).is_some();
    assert!(override_workspace_settings);
    let project_file = root_file.join("fakeProject");
    std::fs::create_dir(&project_file).unwrap();
    let build = config::get_build_configuration(&file, &prefs);
    assert!(build.gradle_user_home.is_none());
}

#[test]
fn test_build_file() {
    let (_tmp, root) = fixture_copy("gradle/simple-gradle");
    let settings = project::ImportSettings::jdtls_defaults();
    let workspace = project::Workspace::import(&[root.clone()], &settings);
    let project = workspace.project("simple-gradle").unwrap();
    assert_eq!("1.8", project.compliance().unwrap());
    let file = project.location.join("target-default/build.gradle");
    assert!(!project::gradle::is_build_file(project, &file));

    let (_tmp2, root) = fixture_copy("gradle/gradle-withoutjava");
    let workspace = project::Workspace::import(&[root.clone()], &settings);
    let project = workspace.projects.first().unwrap();
    let file = project.location.join("build.gradle");
    assert!(project::gradle::is_build_file(project, &file));
}

#[test]
fn test_gradle_properties_file() {
    let (_tmp, root) = fixture_copy("gradle/simple-gradle");
    let settings = project::ImportSettings::jdtls_defaults();
    let workspace = project::Workspace::import(&[root.clone()], &settings);
    let project = workspace.project("simple-gradle").unwrap();
    let file = project.location.join("target-default/gradle.properties");
    assert!(!project::gradle::is_build_file(project, &file));

    let (_tmp2, root) = fixture_copy("gradle/gradle-withoutjava");
    let workspace = project::Workspace::import(&[root.clone()], &settings);
    let project = workspace.projects.first().unwrap();
    let file = project.location.join("gradle.properties");
    assert!(project::gradle::is_build_file(project, &file));
}

#[test]
fn test_gradle_home_preference() {
    let env: HashMap<String, String> = HashMap::new();
    let sysprops: HashMap<String, String> = HashMap::new();
    let mut prefs = GradleSettings::default();
    prefs.home = None;
    assert_eq!(None, config::get_gradle_home_file(&prefs, &env, &sysprops));

    prefs.home = Some("/gradle/home".to_owned());
    assert_eq!(
        Some(PathBuf::from("/gradle/home")),
        config::get_gradle_home_file(&prefs, &env, &sysprops)
    );
}

#[test]
fn test_gradle_arguments() {
    let mut prefs = GradleSettings::default();
    let root_path = root_dir();
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(!build.arguments.is_empty());
    assert_eq!(2, build.arguments.len());
    assert!(build.arguments.contains(&"--init-script".to_owned()));

    prefs.arguments = vec!["-Pproperty=value".to_owned(), "--stacktrace".to_owned()];
    let build = config::get_build_configuration(&root_path, &prefs);
    assert_eq!(4, build.arguments.len());
    assert!(build.arguments.contains(&"-Pproperty=value".to_owned()));
    assert!(build.arguments.contains(&"--stacktrace".to_owned()));
    assert!(build.arguments.contains(&"--init-script".to_owned()));
}

#[test]
fn test_gradle_offline_mode() {
    let mut prefs = GradleSettings::default();
    let root_path = root_dir();
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(!build.offline_mode);
    prefs.offline = true;
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(build.offline_mode);
}

#[test]
fn test_gradle_auto_sync() {
    let mut prefs = GradleSettings::default();
    let root_path = root_dir();
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(!build.auto_sync);
    prefs.update_build_configuration = "automatic".to_owned();
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(build.auto_sync);
}

#[test]
fn test_gradle_jvm_arguments() {
    let mut prefs = GradleSettings::default();
    let root_path = root_dir();
    let build = config::get_build_configuration(&root_path, &prefs);
    assert!(build.jvm_arguments.is_empty());

    prefs.jvm_arguments = vec!["-Djavax.net.ssl.trustStore=truststore.jks".to_owned()];
    let build = config::get_build_configuration(&root_path, &prefs);
    assert_eq!(1, build.jvm_arguments.len());
    assert!(build
        .jvm_arguments
        .contains(&"-Djavax.net.ssl.trustStore=truststore.jks".to_owned()));
}

#[test]
fn test_java11_project() {
    let mut ws = workspace();
    let project = import_gradle_project(&mut ws, "gradle-11");
    ws.assert_is_java_project(&project);
    assert_eq!("11", ws.java_source_level(&project));
    ws.assert_no_errors(&project);
}

fn java_project_with_preview_features(java_version: &str, enabled: bool, severity: &str) {
    let mut ws = workspace();
    let project = import_gradle_project(&mut ws, &format!("gradle-{java_version}"));
    ws.assert_is_java_project(&project);
    assert_eq!(java_version, ws.java_source_level(&project));
    assert_eq!(
        json!(if enabled { "enabled" } else { "disabled" }),
        ws.java_option(&project, ENABLE_PREVIEW)
    );
    assert_eq!(json!(severity), ws.java_option(&project, REPORT_PREVIEW));
}

#[test]
fn test_java12_project() {
    java_project_with_preview_features("12", false, "warning");
}

#[test]
fn test_java13_project() {
    // The project has enabled preview features in the jdt setting.
    java_project_with_preview_features("13", true, "ignore");
}

#[test]
fn test_java14_project() {
    // The project has enabled preview features in the jdt setting.
    java_project_with_preview_features("14", true, "ignore");
}

#[test]
fn test_subprojects() {
    let mut ws = workspace();
    // force overrideWorkspace
    ws.settings["java"]["import"] = json!({ "gradle": { "arguments": ["--stacktrace"] } });
    ws.import_projects(&["gradle/subprojects"]);
    let projects = ws.all_projects(true);
    assert_eq!(3, projects.len()); // 3 gradle projects
    let root = project_dir(&mut ws, "subprojects").unwrap();
    assert_is_gradle_project(&mut ws, &root);
    let project1 = project_dir(&mut ws, "project1").unwrap();
    assert_is_gradle_project(&mut ws, &project1);
    let project2 = project_dir(&mut ws, "project2").unwrap();
    assert_is_gradle_project(&mut ws, &project2);
    update_project(&mut ws, &root);
    update_project(&mut ws, &project1);
    update_project(&mut ws, &project2);
    ws.wait_for_background_jobs();
    let configuration = load_build_configuration_at(&root);
    // check the children .settings/org.eclipse.buildship.core.prefs
    assert!(configuration.override_workspace_settings);
    assert_eq!(3, configuration.arguments.len());
    let configuration = load_build_configuration_at(&project1);
    assert!(!configuration.override_workspace_settings);
    let configuration = load_build_configuration_at(&project2);
    assert!(!configuration.override_workspace_settings);
    let configuration = load_project_configuration(&project1);
    assert!(configuration.override_workspace_settings);
    assert_eq!(3, configuration.arguments.len());
    let configuration = load_project_configuration(&project2);
    assert!(configuration.override_workspace_settings);
    assert_eq!(3, configuration.arguments.len());
    ws.update_settings(json!({ "java": { "import": { "gradle": { "arguments": [] } } } }));
    update_project(&mut ws, &root);
    ws.wait_for_background_jobs();
    let configuration = load_project_configuration(&root);
    // the configuration contains two arguments about jdt.ls init script
    assert!(configuration.override_workspace_settings);
    assert_eq!(2, configuration.arguments.len());
    // check that the children are updated
    let configuration = load_project_configuration(&project1);
    assert!(configuration.override_workspace_settings);
    assert_eq!(2, configuration.arguments.len());
    let configuration = load_project_configuration(&project2);
    assert!(configuration.override_workspace_settings);
    assert_eq!(2, configuration.arguments.len());
}

#[test]
fn import_gradle_kts_project() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/kradle"]);
    let projects = ws.all_projects(true);
    assert_eq!(1, projects.len()); // gradle kts projects
    let kradle = project_dir(&mut ws, "kradle").unwrap();
    assert_is_gradle_project(&mut ws, &kradle);
    ws.assert_no_errors(&kradle);
    let app = ws.try_class_file_uri("kradle", "org.sample.App");
    assert!(app.is_some());
    let app_test = ws.try_class_file_uri("kradle", "org.sample.AppTest");
    assert!(app_test.is_some());
}

#[test]
fn avoid_import_duplicated_projects() {
    let (_tmp, root) = fixture_copy("multi-buildtools");
    let mut settings = project::ImportSettings::jdtls_defaults();
    settings.gradle_enabled = false;
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let configuration_paths = vec![root.join("build.gradle")];
    settings.gradle_enabled = true;
    assert!(project::gradle::import(&root, &settings, &ws, Some(&configuration_paths)).is_empty());
}

#[test]
fn avoid_import_duplicated_projects2() {
    let (_tmp, root) = fixture_copy("multi-buildtools");
    let mut settings = project::ImportSettings::jdtls_defaults();
    settings.gradle_enabled = false;
    let ws = project::Workspace::import(&[root.clone()], &settings);
    let p = ws
        .projects
        .iter()
        .find(|p| p.name == "multi-build-tools")
        .expect("multi-build-tools");
    assert!(p.is_java());
    settings.gradle_enabled = true;
    assert!(project::gradle::import(&root, &settings, &ws, None).is_empty());
}

fn protobuf_source(ws: &mut Workspace, project: &Path, source_set: &str) -> bool {
    let expected = project.join(format!("build/generated/source/proto/{source_set}/java"));
    ws.classpath_entries(project)
        .iter()
        .any(|e| e["path"].as_str().is_some_and(|p| Path::new(p) == expected))
}

/// `IJavaProject.getRawClasspath().length`: the source folders plus the JRE and Gradle containers.
fn raw_classpath_length(ws: &mut Workspace, project: &Path) -> usize {
    ws.classpath_entries(project)
        .iter()
        .filter(|e| e["kind"] == json!(CPE_SOURCE))
        .count()
        + 2
}

#[test]
fn test_proto_buf_support() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "protobufSupport": { "enabled": true } } });
    let project = import_gradle_project(&mut ws, "protobuf");
    assert!(protobuf_source(&mut ws, &project, "main"));
    assert!(protobuf_source(&mut ws, &project, "test"));
}

#[test]
fn test_proto_buf_support_changed() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "protobufSupport": { "enabled": true } } });
    let project = import_gradle_project(&mut ws, "protobuf");
    assert_eq!(5, raw_classpath_length(&mut ws, &project));
    assert!(protobuf_source(&mut ws, &project, "main"));
    assert!(protobuf_source(&mut ws, &project, "test"));

    ws.update_settings(json!({ "java": { "jdt": { "ls": { "protobufSupport": { "enabled": false } } } } }));
    update_project(&mut ws, &project);
    ws.wait_for_background_jobs();

    assert_eq!(3, raw_classpath_length(&mut ws, &project));
}

/// The names of the workspace projects by location (`.project` files).
fn names_by_location(ws: &mut Workspace) -> Vec<(PathBuf, String)> {
    ws.project_locations(true)
        .into_iter()
        .filter_map(|p| project_name_in_description(&p).map(|n| (p, n)))
        .collect()
}

fn project_name_in_description(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".project")).ok()?;
    let start = text.find("<name>")? + 6;
    let end = text[start..].find("</name>")? + start;
    Some(text[start..end].to_owned())
}

#[test]
fn test_name_conflict_project() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/nameConflict"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len());
    let root = ws.dir.join("gradle/nameConflict");
    assert_is_gradle_project(&mut ws, &root);
    let sub_project = root.join("nameconflict");
    assert_is_gradle_project(&mut ws, &sub_project);
    let names = names_by_location(&mut ws);
    assert!(names.contains(&(canonical(&root), "nameConflict".to_owned())), "{names:?}");
    assert!(
        names.contains(&(canonical(&sub_project), "nameConflict-nameconflict".to_owned())),
        "{names:?}"
    );
}

// https://github.com/eclipse-jdtls/eclipse.jdt.ls/issues/1743
#[test]
fn test_name_conflict_project2() {
    let mut ws = workspace();
    ws.import_projects(&["gradle/nameconflict2"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len());
    let names = names_by_location(&mut ws);
    let project1 = ws.dir.join("gradle/nameconflict2/initial");
    let project2 = ws.dir.join("gradle/nameconflict2/complete");
    assert!(names.contains(&(canonical(&project1), "rest-service-initial".to_owned())), "{names:?}");
    assert!(names.contains(&(canonical(&project2), "rest-service-complete".to_owned())), "{names:?}");
    assert_is_gradle_project(&mut ws, &project1);
    assert_is_gradle_project(&mut ws, &project2);
}

fn android_sdk_installed() -> bool {
    std::env::var_os("ANDROID_HOME").is_some() || std::env::var_os("ANDROID_SDK_ROOT").is_some()
}

#[test]
fn test_android_project_support() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "androidSupport": { "enabled": true } } });
    ws.import_projects(&["gradle/android"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len());
    let android_app_project = project_dir(&mut ws, "app").unwrap();
    let classpath_entries = ws.classpath_entries(&android_app_project);
    let paths: Vec<String> = classpath_entries
        .iter()
        .filter_map(|e| e["path"].as_str().map(str::to_owned))
        .collect();
    let has = |suffix: &str| paths.iter().any(|p| p.ends_with(suffix));
    if !android_sdk_installed() {
        // android SDK is not detected, plugin will do nothing
        assert_eq!(2, raw_classpath_length(&mut ws, &android_app_project));
    } else {
        // android SDK is detected, android project should be imported successfully
        assert_eq!(6, raw_classpath_length(&mut ws, &android_app_project));
        // main sourceSet are added to classpath correctly
        assert!(has("/app/src/main/java"));
        // test sourceSet are added to classpath correctly
        assert!(has("/app/src/test/java"));
        // androidTest sourceSet are added to classpath correctly
        assert!(has("/app/src/androidTest/java"));
        // buildConfig files are added to classpath correctly
        assert!(has("/app/build/generated/source/buildConfig/standard/debug"));
        // dataBinding files are added to classpath correctly
        assert!(has(
            "/app/build/generated/data_binding_base_class_source_out/standardDebug/out"
        ));
    }
}

#[test]
fn test_android_project_support_changed() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "androidSupport": { "enabled": true } } });
    ws.import_projects(&["gradle/android"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len());
    let android_app_project = project_dir(&mut ws, "app").unwrap();
    if !android_sdk_installed() {
        // android SDK is not detected, plugin will do nothing
        assert_eq!(2, raw_classpath_length(&mut ws, &android_app_project));
    } else {
        // android SDK is detected, android project should be imported successfully
        assert_eq!(6, raw_classpath_length(&mut ws, &android_app_project));
    }
    ws.update_settings(json!({ "java": { "jdt": { "ls": { "androidSupport": { "enabled": false } } } } }));
    for project in ws.project_locations(true) {
        update_project(&mut ws, &project);
    }
    ws.wait_for_background_jobs();
    // regardless of ANDROID_HOME, the number of cpe is 2 since android support is disabled
    assert_eq!(2, raw_classpath_length(&mut ws, &android_app_project));
}

#[test]
fn test_need_replace_content() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("test.txt");
    std::fs::write(&f, b"").unwrap();
    let digest = project::gradle::sha256::digest(&std::fs::read(&f).unwrap());
    assert!(util::need_replace_content(&f, &digest).unwrap());

    std::fs::write(&f, b"modification").unwrap();
    assert!(util::need_replace_content(&f, &digest).unwrap());
}

#[test]
fn test_annotation_processing() {
    let mut ws = workspace();
    let project = import_gradle_project(&mut ws, "apt");
    ws.wait_for_background_jobs();
    let prefs = std::fs::read_to_string(project.join(".settings/org.eclipse.jdt.apt.core.prefs"))
        .unwrap_or_default();
    let value = |key: &str| {
        prefs
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
    };
    assert_eq!(Some("true".to_owned()), value("org.eclipse.jdt.apt.aptEnabled"));
    assert_eq!(
        Some("true".to_owned()),
        value("org.eclipse.jdt.apt.processorOptions/mapstruct.suppressGeneratorTimestamp")
    );
    assert_eq!(
        Some("apt".to_owned()),
        value("org.eclipse.jdt.apt.processorOptions/test.arg")
    );
}

#[test]
fn test_get_gradle_distribution() {
    let project_root = fixtures_dir().join("projects/gradle/no-gradlew");
    let distribution = config::get_gradle_distribution(&project_root, &GradleSettings::default());
    assert_eq!(GradleDistribution::Wrapper, distribution);
}

fn language_project(
    name: &str,
    support: &str,
    enabled: bool,
    ws: &mut Workspace,
) -> PathBuf {
    ws.settings["java"]["jdt"] = json!({ "ls": { support: { "enabled": enabled } } });
    import_gradle_project(ws, name)
}

fn wait_for_other_langs(ws: &mut Workspace) {
    ws.wait_for_background_jobs();
}

#[test]
fn test_aspect_support_disabled() {
    let mut ws = workspace();
    let project = language_project("aspect", "aspectjSupport", false, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_has_errors(&project, &[]);
}

#[test]
fn test_aspect_support_enabled() {
    let mut ws = workspace();
    let project = language_project("aspect", "aspectjSupport", true, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_no_errors(&project);
    let demo_aspect = ws.try_class_file_uri("aspect", "io.freefair.DemoAspect");
    assert!(demo_aspect.is_some());
}

#[test]
fn test_kotlin_support_disabled() {
    let mut ws = workspace();
    let project = language_project("kotlin", "kotlinSupport", false, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_has_errors(&project, &[]);
}

#[test]
fn test_kotlin_support_enabled() {
    let mut ws = workspace();
    let project = language_project("kotlin", "kotlinSupport", true, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_no_errors(&project);
}

#[test]
fn test_groovy_support_disabled() {
    let mut ws = workspace();
    let project = language_project("groovy", "groovySupport", false, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_has_errors(&project, &[]);
}

#[test]
fn test_groovy_support_enabled() {
    let mut ws = workspace();
    let project = language_project("groovy", "groovySupport", true, &mut ws);
    wait_for_other_langs(&mut ws);
    ws.assert_no_errors(&project);
}

#[test]
fn test_scala_support_enabled() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "scalaSupport": { "enabled": true } } });
    ws.import_projects(&["gradle/scala"]);
    wait_for_other_langs(&mut ws);
    let project = project_dir(&mut ws, "app").unwrap();
    assert_is_gradle_project(&mut ws, &project);
    ws.assert_no_errors(&project);
}

#[test]
fn test_scala_support_disabled() {
    let mut ws = workspace();
    ws.settings["java"]["jdt"] = json!({ "ls": { "scalaSupport": { "enabled": false } } });
    ws.import_projects(&["gradle/scala"]);
    wait_for_other_langs(&mut ws);
    let project = project_dir(&mut ws, "app").unwrap();
    assert_is_gradle_project(&mut ws, &project);
    ws.assert_has_errors(&project, &[]);
}

#[test]
fn test_clean_up_build_server_footprint() {
    let mut ws = workspace();
    let project = import_gradle_project(&mut ws, "gradle-build-server");
    assert!(!ws
        .natures(&project)
        .iter()
        .any(|n| n == "com.microsoft.gradle.bs.importer.GradleBuildServerProjectNature"));
    let description = std::fs::read_to_string(project.join(".project")).unwrap_or_default();
    for builder in [
        "com.microsoft.gradle.bs.importer.builder.BuildServerBuilder",
        "java.bs.JavaProblemChecker",
    ] {
        assert!(
            !description.contains(builder),
            "Build server builders should have been removed"
        );
    }
}
