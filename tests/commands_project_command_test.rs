//! Port of `org.eclipse.jdt.ls.core.internal.commands.ProjectCommandTest`.
//!
//! `ProjectCommand`'s static methods are reached through the delegate
//! commands that expose them (`java.project.getSettings`, `getClasspaths`,
//! `isTestFile`, `getAll`, `updateClassPaths`, `updateSettings`,
//! `updateJdk`, `resolveWorkspaceSymbol`, `java.vm.getAllInstalls`).
//! `getJavaProjectFromUri` is observed through `getSettings`, which resolves
//! the project the same way. `IJavaProject.setRawClasspath` goes through
//! `java.project.updateClassPaths`. `updateSourcePaths` has no command; its
//! test is a unit test in `src/features/project_commands.rs`.

mod common;
use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// `IResource.getLocationURI().toString()`: `file:/abs/path`.
fn location_uri(p: &Path) -> String {
    format!("file:{}", p.to_string_lossy().replace(' ', "%20"))
}

fn get_project_settings(ws: &mut Workspace, uri: &str, keys: &[&str]) -> Value {
    ws.execute("java.project.getSettings", vec![json!(uri), json!(keys)])
}

fn get_classpaths(ws: &mut Workspace, uri: &str, scope: &str) -> Value {
    ws.execute(
        "java.project.getClasspaths",
        vec![json!(uri), json!(json!({ "scope": scope }).to_string())],
    )
}

fn is_test_file(ws: &mut Workspace, uri: &str) -> bool {
    ws.execute("java.project.isTestFile", vec![json!(uri)])
        .as_bool()
        .expect("isTestFile returns a boolean")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected an array: {v}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

fn salut(ws: &mut Workspace, name: &str) -> PathBuf {
    ws.import_projects(&[&format!("maven/{name}")]);
    ws.project_root(name)
}

#[test]
fn test_get_project_nature_ids() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let uri_string = location_uri(&project);
    let options = get_project_settings(&mut ws, &uri_string, &["org.eclipse.jdt.ls.core.natureIds"]);
    let nature_ids = strings(&options["org.eclipse.jdt.ls.core.natureIds"]);
    assert_eq!(2, nature_ids.len());
    assert!(nature_ids.iter().any(|n| n == JAVA_NATURE));
    assert!(nature_ids.iter().any(|n| n == MAVEN_NATURE));
}

#[test]
fn test_get_project_settings_for_maven_java7() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let uri_string = location_uri(&project.join("src/main/java/Foo.java"));
    let setting_keys = ["org.eclipse.jdt.core.compiler.compliance", "org.eclipse.jdt.core.compiler.source"];
    let options = get_project_settings(&mut ws, &uri_string, &setting_keys);

    assert_eq!(setting_keys.len(), options.as_object().unwrap().len());
    assert_eq!("1.8", options["org.eclipse.jdt.core.compiler.compliance"]);
    assert_eq!("1.8", options["org.eclipse.jdt.core.compiler.source"]);
}

#[test]
fn test_get_project_settings_for_maven_java8() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut2");
    let uri_string = location_uri(&project.join("src/main/java/foo/Bar.java"));
    let setting_keys = ["org.eclipse.jdt.core.compiler.compliance", "org.eclipse.jdt.core.compiler.source"];
    let options = get_project_settings(&mut ws, &uri_string, &setting_keys);

    assert_eq!(setting_keys.len(), options.as_object().unwrap().len());
    assert_eq!("11", options["org.eclipse.jdt.core.compiler.compliance"]);
    assert_eq!("11", options["org.eclipse.jdt.core.compiler.source"]);
}

#[test]
fn test_get_project_vm_installation() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut2");
    let uri_string = location_uri(&project.join("src/main/java/foo/Bar.java"));
    let options = get_project_settings(&mut ws, &uri_string, &[VM_LOCATION]);

    // `JavaRuntime.getVMInstall(javaProject)`: no runtime is configured for
    // JavaSE-11, so the project's JRE container resolves to the workspace
    // default VM, the `javaHome` the harness starts the server with.
    let location = PathBuf::from(java_home());
    assert!(!location.as_os_str().is_empty());
    assert_eq!(json!(location.to_string_lossy()), options[VM_LOCATION], "{options}");
}

fn simple(ws: &mut Workspace) -> PathBuf {
    ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"))
}

#[test]
fn test_get_project_source_paths() {
    let mut ws = Workspace::new();
    let linked = simple(&mut ws);
    let linked_folder = location_uri(&linked);
    let options = get_project_settings(&mut ws, &linked_folder, &[SOURCE_PATHS]);
    let actual_source_paths = strings(&options[SOURCE_PATHS]);
    assert_eq!(2, actual_source_paths.len(), "{options}");
    assert!(actual_source_paths.iter().any(|source_path| {
        *source_path == linked.join("src").to_string_lossy()
            || *source_path == linked.join("test").to_string_lossy()
    }));
}

#[test]
fn test_get_project_output_path() {
    let mut ws = Workspace::new();
    let linked = simple(&mut ws);
    let linked_folder = location_uri(&linked);
    let options = get_project_settings(&mut ws, &linked_folder, &[OUTPUT_PATH]);
    let actual_output_path = options[OUTPUT_PATH].clone();
    let project = ws.workspace_project_location(&invisible_project_name(&linked));
    let expected_output_path = project.join("bin");
    assert_eq!(json!(expected_output_path.to_string_lossy()), actual_output_path);
}

#[test]
fn test_get_project_referenced_library_paths() {
    let mut ws = Workspace::new();
    let linked = simple(&mut ws);
    let linked_folder = location_uri(&linked);
    let options = get_project_settings(&mut ws, &linked_folder, &[REFERENCED_LIBRARIES]);
    let actual_referenced_library_paths = strings(&options[REFERENCED_LIBRARIES]);
    let expected_referenced_library_path = linked.join("lib").join("mylib.jar");
    assert_eq!(1, actual_referenced_library_paths.len());
    assert_eq!(
        expected_referenced_library_path.to_string_lossy(),
        actual_referenced_library_paths[0]
    );
}

fn classpath_entries(ws: &mut Workspace, uri: &str) -> Vec<Value> {
    let options = get_project_settings(ws, uri, &[CLASSPATH_ENTRIES]);
    assert!(!options[CLASSPATH_ENTRIES].is_null());
    options[CLASSPATH_ENTRIES].as_array().unwrap().clone()
}

#[test]
fn test_get_classpath_entries() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut2");
    let uri_string = location_uri(&project.join("src/main/java/foo/Bar.java"));
    let entries = classpath_entries(&mut ws, &uri_string);
    assert!(!entries.is_empty());
    assert!(entries.iter().any(|entry| entry["kind"] == CPE_SOURCE));
    assert!(entries.iter().any(|entry| entry["kind"] == CPE_LIBRARY));
}

#[test]
fn test_get_classpath_entries_with_non_exist_lib() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut2");
    let uri_string = location_uri(&project.join("src/main/java/foo/Bar.java"));
    // `javaProject.setRawClasspath(rawClasspath + newLibraryEntry("/foo/bar/a.jar"))`.
    let mut classpath_entries_ = classpath_entries(&mut ws, &uri_string);
    classpath_entries_.push(json!({ "kind": CPE_LIBRARY, "path": "/foo/bar/a.jar" }));
    ws.execute(
        "java.project.updateClassPaths",
        vec![json!(uri_string), json!(json!({ "classpathEntries": classpath_entries_ }).to_string())],
    );

    let entries = classpath_entries(&mut ws, &uri_string);
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .any(|entry| entry["kind"] == CPE_LIBRARY && entry["path"] == "/foo/bar/a.jar"),
        "{entries:#?}"
    );
}

#[test]
fn test_update_classpath_entries() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut2");
    let uri_string = location_uri(&project.join("src/main/java/foo/Bar.java"));
    let mut entries = classpath_entries(&mut ws, &uri_string);

    let size = entries.len();
    entries.remove(size - 1);
    ws.execute(
        "java.project.updateClassPaths",
        vec![json!(uri_string), json!(json!({ "classpathEntries": entries }).to_string())],
    );

    let new_entries = classpath_entries(&mut ws, &uri_string);
    assert_eq!(size - 1, new_entries.len(), "{new_entries:#?}");
}

#[test]
fn test_get_maven_project_from_uri() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let java_source = location_uri(&project.join("src/main/java/Foo.java"));
    let java_project = ws.try_execute("java.project.getSettings", vec![json!(java_source), json!([NATURE_IDS])]);
    assert!(java_project.is_ok(), "Can get project from java file uri");

    let project_uri = location_uri(&project);
    let java_project = ws.try_execute("java.project.getSettings", vec![json!(project_uri), json!([NATURE_IDS])]);
    assert!(java_project.is_ok(), "Can get project from project uri");
}

/// `getJavaProjectFromUri(uri).getElementName()`: the name of the project
/// that owns the source paths `getSettings` reports for `uri`.
fn java_project_name(ws: &mut Workspace, uri: &str) -> String {
    let source_paths = strings(&get_project_settings(ws, uri, &[SOURCE_PATHS])[SOURCE_PATHS]);
    let listed = ws.list_source_paths();
    let names: Vec<String> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| {
            let p = d["path"].as_str().unwrap().trim_end_matches('/');
            source_paths.iter().any(|s| s.trim_end_matches('/') == p)
        })
        .map(|d| d["projectName"].as_str().unwrap().to_owned())
        .collect();
    assert!(!names.is_empty(), "{listed}");
    assert!(names.iter().all(|n| *n == names[0]), "{names:?}");
    names[0].clone()
}

#[test]
fn test_get_multi_module_maven_project_from_uri() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/multimodule3"]);
    let project = ws.project_root("this_is_a_very_long_module_name");
    let java_source = location_uri(&project.join("src/main/org/eclipse/App.java"));
    assert_eq!("this_is_a_very_long_module_name", java_project_name(&mut ws, &java_source));

    let project_uri = location_uri(&project);
    assert_eq!("this_is_a_very_long_module_name", java_project_name(&mut ws, &project_uri));
}

#[test]
fn test_get_invisible_project_from_uri() {
    let mut ws = Workspace::new();
    let linked = simple(&mut ws);
    let linked_folder = location_uri(&linked);
    let java_project = ws.try_execute("java.project.getSettings", vec![json!(linked_folder), json!([NATURE_IDS])]);
    assert!(java_project.is_ok(), "Can get project from linked folder uri");
}

fn assert_classpaths(result: &Value, classpaths: usize, modulepaths: usize) {
    assert_eq!(classpaths, result["classpaths"].as_array().unwrap().len(), "{result:#}");
    assert_eq!(modulepaths, result["modulepaths"].as_array().unwrap().len(), "{result:#}");
}

fn contains_junit(result: &Value) -> bool {
    strings(&result["classpaths"]).iter().any(|e| e.contains("junit"))
}

#[test]
fn test_get_classpaths_for_maven() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "classpathtest");
    let uri_string = location_uri(&project.join("src/main/java/main/App.java"));
    let result = get_classpaths(&mut ws, &uri_string, "runtime");
    assert_classpaths(&result, 1, 0);
    assert!(!result["classpaths"][0].as_str().unwrap().contains("junit"));

    let result = get_classpaths(&mut ws, &uri_string, "test");
    assert_classpaths(&result, 4, 0);
    assert!(contains_junit(&result));
}

#[test]
fn test_get_classpaths_for_maven_when_updating() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "classpathtest");
    let uri_string = location_uri(&project.join("src/main/java/main/App.java"));

    // `projectsManager.updateProject(project, true)`.
    ws.client().notify(
        "java/projectConfigurationUpdate",
        json!({ "uri": file_uri(&project.join("pom.xml")) }),
    );

    for _ in 0..10 {
        let result = get_classpaths(&mut ws, &uri_string, "test");
        assert_classpaths(&result, 4, 0);
        assert!(contains_junit(&result));
    }
}

#[test]
fn test_get_classpaths_for_gradle() {
    let mut ws = Workspace::new();
    ws.import_projects(&["gradle/simple-gradle"]);
    let project = ws.project_root("simple-gradle");
    let uri_string = location_uri(&project.join("src/main/java/Library.java"));
    let result = get_classpaths(&mut ws, &uri_string, "runtime");
    assert_classpaths(&result, 3, 0);
    assert!(!result["classpaths"][0].as_str().unwrap().contains("junit"));

    let result = get_classpaths(&mut ws, &uri_string, "test");
    assert_classpaths(&result, 5, 0);
    assert!(contains_junit(&result));
}

#[test]
fn test_get_classpaths_for_maven_modular() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "modular-project");
    let uri_string = location_uri(&project.join("src/main/java/modular/Main.java"));
    let result = get_classpaths(&mut ws, &uri_string, "test");
    assert_classpaths(&result, 0, 1);
}

#[test]
fn test_get_classpaths_for_eclipse() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    let project = ws.project_root("hello");
    let uri_string = location_uri(&project.join("src/java/Bar.java"));
    let result = get_classpaths(&mut ws, &uri_string, "runtime");
    assert_classpaths(&result, 1, 0);

    let result = get_classpaths(&mut ws, &uri_string, "test");
    assert_classpaths(&result, 2, 0);
}

#[test]
fn test_is_test_file_for_maven() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "classpathtest");
    let src_uri = location_uri(&project.join("src/main/java/main/App.java"));
    let test_uri = location_uri(&project.join("src/test/java/test/AppTest.java"));
    assert!(!is_test_file(&mut ws, &src_uri));
    assert!(is_test_file(&mut ws, &test_uri));
}

#[test]
fn test_is_test_file_for_gradle() {
    let mut ws = Workspace::new();
    ws.import_projects(&["gradle/simple-gradle"]);
    let project = ws.project_root("simple-gradle");
    let src_uri = location_uri(&project.join("src/main/java/Library.java"));
    let test_uri = location_uri(&project.join("src/test/java/LibraryTest.java"));
    assert!(!is_test_file(&mut ws, &src_uri));
    assert!(is_test_file(&mut ws, &test_uri));
}

#[test]
fn get_all_java_project() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/multimodule"]);
    let projects = ws.execute("java.project.getAll", vec![]);
    assert_eq!(3, projects.as_array().unwrap().len(), "{projects}");
}

/// `START_OF_DOCUMENT`.
fn start_of_document() -> Value {
    range(0, 0, 0, 0)
}

fn build_class_symbol(ws: &mut Workspace, project: &str, fq_class_name: &str) -> Value {
    let uri_string = ws.class_file_uri(project, fq_class_name);
    json!({
        "location": { "uri": uri_string, "range": start_of_document() },
        "name": &fq_class_name[fq_class_name.rfind('.').map_or(0, |i| i + 1)..],
        // `SymbolKind.Class`: `JSONUtility.toModel` uses a plain Gson, which
        // reads enums by name.
        "kind": "Class",
    })
}

fn resolve_workspace_symbol(ws: &mut Workspace, requested: &Value) -> Value {
    ws.execute(
        "java.project.resolveWorkspaceSymbol",
        vec![json!(requested.to_string())],
    )
}

#[test]
fn test_resolve_class_symbol() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut-java11"]);

    let requested_symbol = build_class_symbol(&mut ws, "salut-java11", "org.apache.commons.lang3.StringUtils");
    let resolved_symbol = resolve_workspace_symbol(&mut ws, &requested_symbol);
    assert_eq!(range(124, 13, 124, 24), resolved_symbol["location"]["range"], "{resolved_symbol}");
}

#[test]
fn test_resolve_nested_class_symbol() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut-java11"]);

    let mut requested_symbol = build_class_symbol(&mut ws, "salut-java11", "org.sample.Bar");
    requested_symbol["name"] = json!("MyClass");
    let resolved_symbol = resolve_workspace_symbol(&mut ws, &requested_symbol);
    assert_eq!(resolved_symbol["location"]["range"], range(17, 21, 17, 28), "{resolved_symbol}");
}

#[test]
fn test_update_project_jdk() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let all_vm_installs = ws.execute("java.vm.getAllInstalls", vec![]);
    let vm_install = all_vm_installs
        .as_array()
        .unwrap()
        .iter()
        .find(|vm| !vm["typeName"].as_str().unwrap().contains("org.eclipse.jdt.ls.core.internal.TestVMType"))
        .unwrap()
        .clone();
    let project_uri = location_uri(&project);
    ws.execute(
        "java.project.updateJdk",
        vec![json!(project_uri), vm_install["path"].clone()],
    );
    let vm = get_project_settings(&mut ws, &project_uri, &[VM_LOCATION]);
    assert_eq!(vm_install["path"], vm[VM_LOCATION], "{all_vm_installs}");
}

#[test]
fn test_update_invalid_project_jdk() {
    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let update_project_jdk = ws.execute(
        "java.project.updateJdk",
        vec![json!(location_uri(&project)), json!("invalid/path")],
    );
    assert_eq!(false, update_project_jdk["success"], "{update_project_jdk}");
}

#[test]
fn test_update_maven_profiles() {
    const KEY: &str = "org.eclipse.m2e.core.selectedProfiles";

    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let uri_string = location_uri(&project);
    let options = get_project_settings(&mut ws, &uri_string, &[KEY]);

    assert_eq!("", options[KEY]);

    let update_options = json!({ KEY: "my profile" });
    ws.execute("java.project.updateSettings", vec![json!(uri_string), json!(update_options.to_string())]);

    let options = get_project_settings(&mut ws, &uri_string, &[KEY]);
    assert_eq!("my profile", options[KEY]);
}

#[test]
fn test_update_project_options() {
    // `JavaCore.COMPILER_CODEGEN_METHOD_PARAMETERS_ATTR`.
    const KEY: &str = "org.eclipse.jdt.core.compiler.codegen.methodParameters";

    let mut ws = Workspace::new();
    let project = salut(&mut ws, "salut");
    let uri_string = location_uri(&project);
    let options = get_project_settings(&mut ws, &uri_string, &[KEY]);

    // `JavaCore.DO_NOT_GENERATE`.
    assert_eq!("do not generate", options[KEY]);

    let update_options = json!({ KEY: "generate" });
    ws.execute("java.project.updateSettings", vec![json!(uri_string), json!(update_options.to_string())]);

    let options = get_project_settings(&mut ws, &uri_string, &[KEY]);
    // `JavaCore.GENERATE`.
    assert_eq!("generate", options[KEY]);
}
