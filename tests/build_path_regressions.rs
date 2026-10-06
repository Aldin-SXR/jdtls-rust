//! Source-path command behavior through LSP, including persisted classpaths.
mod common;
use common::jdtls::{is_oracle, test_default_options, Workspace};
use common::projects::{file_uri, SOURCE_PATHS};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn change(ws: &mut Workspace, add: bool, path: &Path) -> Value {
    ws.execute(
        if add {
            "java.project.addToSourcePath"
        } else {
            "java.project.removeFromSourcePath"
        },
        vec![json!(file_uri(path))],
    )
}
fn list(ws: &mut Workspace) -> Value {
    ws.execute("java.project.listSourcePaths", vec![])
}
fn name(root: &Path) -> String {
    let hash = root
        .to_string_lossy()
        .replace('\\', "/")
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    format!("{}_{hash:x}", root.file_name().unwrap().to_string_lossy())
}
fn no_remove(folder: &str) -> Value {
    json!({"status":true,"message":format!("No need to remove it from source path, because the folder '{folder}' isn't on any project's source path.")})
}
fn entries(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path.join(".classpath")).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("classpathentry"))
        .map(|n| {
            let mut value = serde_json::Map::new();
            for a in n.attributes() {
                value.insert(a.name().to_owned(), json!(a.value()));
            }
            let attributes: Vec<_> = n
                .descendants()
                .filter(|c| c.has_tag_name("attribute"))
                .map(|a| json!([a.attribute("name"), a.attribute("value")]))
                .collect();
            let rules: Vec<_> = n
                .descendants()
                .filter(|c| c.has_tag_name("accessrule"))
                .map(|a| json!([a.attribute("kind"), a.attribute("pattern")]))
                .collect();
            value.insert("attributes".into(), json!(attributes));
            value.insert("accessrules".into(), json!(rules));
            Value::Object(value)
        })
        .collect()
}
fn source_entry(path: &Path, source: &str) -> Value {
    entries(path)
        .into_iter()
        .find(|e| e["kind"] == "src" && e["path"] == source)
        .unwrap()
}

#[test]
fn hidden_source_paths_are_idempotent_and_survive_restart() {
    let mut ws = Workspace::new();
    let root = ws.dir.join("plain");
    std::fs::create_dir_all(&root).unwrap();
    ws.set_roots(vec![root.clone()]);
    let metadata = ws.workspace_project_location(&name(&root));
    assert_eq!(
        no_remove("plain/src"),
        change(&mut ws, false, &root.join("src"))
    );
    assert!(
        !metadata.exists(),
        "removing an absent path must not create a project"
    );
    assert_eq!(
        json!({"status":true,"message":format!("Successfully added 'plain/src' to the project {}'s source path.",name(&root)),"sourcePaths":["src"]}),
        change(&mut ws, true, &root.join("src"))
    );
    assert!(
        !root.join("src").exists(),
        "adding a classpath entry must not create the source directory"
    );
    assert_eq!(
        vec!["org.eclipse.jdt.core.javanature"],
        ws.natures(&metadata)
    );
    let before = std::fs::read(metadata.join(".classpath")).unwrap();
    assert_eq!(
        json!({"status":true,"message":format!("No need to add it to source path again, because the folder 'plain/src' is already in the project {}'s source path.",name(&root))}),
        change(&mut ws, true, &root.join("src"))
    );
    assert_eq!(before, std::fs::read(metadata.join(".classpath")).unwrap());
    let listed = list(&mut ws);
    assert_eq!(
        json!({"status":true,"data":[{"path":root.join("src").to_string_lossy(),"displayPath":"plain/src","classpathEntry":format!("/{}/_/src",name(&root)),"projectName":name(&root),"projectType":"Workspace"}]}),
        listed
    );
    ws.restart();
    assert_eq!(listed, list(&mut ws));
    assert_eq!(
        json!({"status":true,"message":format!("Successfully removed 'plain/src' from the project {}'s source path.",name(&root)),"sourcePaths":[]}),
        change(&mut ws, false, &root.join("src"))
    );
    assert_eq!(
        no_remove("plain/src"),
        change(&mut ws, false, &root.join("src"))
    );
    ws.restart();
    assert_eq!(json!({"status":true,"data":[]}), list(&mut ws));
    assert!(
        !root.join(".classpath").exists(),
        "metadata belongs to the server workspace"
    );
}

#[test]
fn outside_workspace_folders_return_the_exact_failure_for_both_commands() {
    let mut ws = Workspace::new();
    let root = ws.dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    ws.set_roots(vec![root]);
    let outside = ws.external_dir().join("missing");
    for add in [true, false] {
        assert_eq!(
            json!({"status":false,"message":format!("The folder '{}' doesn't belong to any workspace.",outside.display())}),
            change(&mut ws, add, &outside)
        );
    }
    assert_eq!(json!({"status":true,"data":[]}), list(&mut ws));
}

#[test]
fn parent_sources_exclude_children_and_removal_clears_only_exact_filters() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    std::fs::create_dir_all(root.join("src/child")).unwrap();
    std::fs::write(root.join(".classpath"), r#"<classpath>
<classpathentry kind="src" path="src/child" output="child-bin"><attributes><attribute name="test" value="true"/></attributes></classpathentry>
<classpathentry kind="con" path="org.eclipse.jdt.launching.JRE_CONTAINER"/>
<classpathentry kind="output" path="bin"/>
</classpath>"#).unwrap();
    assert_eq!(
        json!({"status":true,"message":"Successfully added 'TestProject/src' to the project TestProject's source path."}),
        change(&mut ws, true, &root.join("src"))
    );
    assert_eq!("child/", source_entry(&root, "src")["excluding"]);
    assert_eq!("child-bin", source_entry(&root, "src/child")["output"]);
    assert_eq!(
        json!([["test", "true"]]),
        source_entry(&root, "src/child")["attributes"]
    );
    assert_eq!(
        json!({"status":false,"message":"Cannot add the folder '/TestProject/src/grandchild' to the source path because its parent folder is already in the source path of the project 'TestProject'."}),
        change(&mut ws, true, &root.join("src/grandchild"))
    );
    assert_eq!(
        json!({"status":true,"message":"Successfully removed 'TestProject/src/child' from the project TestProject's source path."}),
        change(&mut ws, false, &root.join("src/child"))
    );
    assert!(source_entry(&root, "src")["excluding"].is_null());
    assert_eq!(
        1,
        ws.project_settings(&file_uri(&root), &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
    ws.restart();
    assert_eq!(
        1,
        ws.project_settings(&file_uri(&root), &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
}

#[test]
fn removal_preserves_other_patterns_outputs_attributes_and_library_rules() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    std::fs::create_dir_all(root.join("src/child")).unwrap();
    let original = r#"<classpath>
<classpathentry kind="src" path="src" excluding="child/|other/**" including="child/|keep/**" output="custom-bin"><attributes><attribute name="optional" value="true"/></attributes></classpathentry>
<classpathentry kind="src" path="src/child"/>
<classpathentry kind="lib" path="lib/example.jar" sourcepath="lib/source.jar" exported="true"><accessrules><accessrule kind="nonaccessible" pattern="internal/**"/></accessrules><attributes><attribute name="javadoc_location" value="https://example.org/docs"/></attributes></classpathentry>
<classpathentry kind="con" path="org.eclipse.jdt.launching.JRE_CONTAINER"/>
<classpathentry kind="output" path="bin"/>
</classpath>"#;
    std::fs::write(root.join(".classpath"), original).unwrap();
    let library = entries(&root)
        .into_iter()
        .find(|e| e["kind"] == "lib")
        .unwrap();
    assert_eq!(
        true,
        change(&mut ws, false, &root.join("src/child"))["status"]
    );
    let entry = source_entry(&root, "src");
    assert_eq!("other/**", entry["excluding"]);
    assert_eq!("keep/**", entry["including"]);
    assert_eq!("custom-bin", entry["output"]);
    assert_eq!(json!([["optional", "true"]]), entry["attributes"]);
    assert_eq!(
        library,
        entries(&root)
            .into_iter()
            .find(|e| e["kind"] == "lib")
            .unwrap()
    );
    let before = std::fs::read(root.join(".classpath")).unwrap();
    assert_eq!(
        no_remove("TestProject/src/other"),
        change(&mut ws, false, &root.join("src/other"))
    );
    assert_eq!(before, std::fs::read(root.join(".classpath")).unwrap());
}

#[test]
fn workspace_root_sources_exclude_visible_projects_and_use_empty_relative_paths() {
    let mut ws = Workspace::new();
    let child = ws.copy_files("eclipse/hello");
    let root = child.parent().unwrap().to_path_buf();
    ws.set_roots(vec![root.clone()]);
    assert_eq!(
        json!({"status":true,"message":format!("Successfully added 'eclipse' to the project {}'s source path.",name(&root)),"sourcePaths":[""]}),
        change(&mut ws, true, &root)
    );
    let metadata = ws.workspace_project_location(&name(&root));
    assert_eq!("hello/", source_entry(&metadata, "_")["excluding"]);
    let listing = list(&mut ws);
    let entry = listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["projectName"] == name(&root))
        .unwrap();
    assert_eq!("eclipse/", entry["displayPath"]);
    assert_eq!(format!("{}/", root.display()), entry["path"]);
    assert_eq!(
        json!({"status":true,"message":"Successfully removed 'eclipse/hello/test' from the project hello's source path."}),
        change(&mut ws, false, &child.join("test"))
    );
    assert_eq!(
        json!({"status":true,"message":format!("Successfully removed 'eclipse' from the project {}'s source path.",name(&root)),"sourcePaths":[]}),
        change(&mut ws, false, &root)
    );
    ws.restart();
    assert_eq!(1, list(&mut ws)["data"].as_array().unwrap().len());
}

#[test]
fn source_path_changes_keep_virtual_buffer_diagnostics_and_text() {
    // The Rust server extends Eclipse's disk-backed compilation units with
    // virtual documents. Exercise that contract without creating a source file.
    if is_oracle() {
        return;
    }
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    let virtual_file: PathBuf = root.join("new-source/Virtual.java");
    let uri = file_uri(&virtual_file);
    let text = "public class Virtual { int x = \"wrong\"; }\n";
    ws.execute(
        "java.project.refreshDiagnostics",
        vec![json!(uri), json!("thisFile"), json!(false)],
    );
    ws.open_with(&uri, text);
    assert_eq!(
        true,
        change(&mut ws, true, &root.join("new-source"))["status"]
    );
    let diagnostics = ws.diagnostics(&uri);
    assert!(
        diagnostics
            .iter()
            .any(|d| d["message"] == "Type mismatch: cannot convert from String to int"),
        "{diagnostics:#?}"
    );
    assert_eq!(
        true,
        change(&mut ws, false, &root.join("new-source"))["status"]
    );
    let diagnostics = ws.diagnostics(&uri);
    assert!(
        diagnostics
            .iter()
            .any(|d| d["message"] == "Type mismatch: cannot convert from String to int"),
        "{diagnostics:#?}"
    );
    assert!(!virtual_file.exists());
    assert!(!root.join("new-source").exists());
}

#[test]
fn project_root_and_self_closing_classpaths_accept_new_sources() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    assert_eq!(true, change(&mut ws, false, &root.join("src"))["status"]);
    assert_eq!(
        json!({"status":true,"message":"Successfully added 'TestProject' to the project TestProject's source path."}),
        change(&mut ws, true, &root)
    );
    assert_eq!("src", source_entry(&root, "")["kind"]);
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    std::fs::write(root.join(".classpath"), "<classpath/>").unwrap();
    assert_eq!(
        json!({"status":true,"message":"Successfully added 'TestProject/src' to the project TestProject's source path."}),
        change(&mut ws, true, &root.join("src"))
    );
    assert_eq!("src", source_entry(&root, "src")["kind"]);
    ws.restart();
    assert_eq!(1, list(&mut ws)["data"].as_array().unwrap().len());
}

#[test]
fn default_project_source_changes_do_not_duplicate_project_handles() {
    let mut ws = Workspace::new();
    let root = ws.workspace_project_location("jdt.ls-java-project");
    let file = ws.external_dir().join("DefaultProbe.java");
    let text = "public class DefaultProbe {}\n";
    std::fs::write(&file, text).unwrap();
    ws.open_with(&file_uri(&file), text);
    ws.diagnostics(&file_uri(&file));
    assert_eq!(1, ws.all_projects(true).len());
    let before = ws.all_projects(true);
    let result = change(&mut ws, true, &root.join("additional"));
    assert_eq!(
        json!({"status":true,"message":format!("Successfully added '{}' to the project jdt.ls-java-project's source path.",root.join("additional").display())}),
        result
    );
    assert_eq!(before, ws.all_projects(true));
    assert_eq!(
        2,
        ws.project_settings(&file_uri(&root), &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
    assert_eq!(
        true,
        change(&mut ws, false, &root.join("additional"))["status"]
    );
    assert_eq!(
        1,
        ws.project_settings(&file_uri(&root), &[SOURCE_PATHS])[SOURCE_PATHS]
            .as_array()
            .unwrap()
            .len()
    );
    assert_eq!(before, ws.all_projects(true));
}

#[test]
fn imported_hidden_projects_preserve_linked_output_and_reapply_source_preferences() {
    let mut ws = Workspace::new();
    ws.settings = json!({"java":{"project":{"sourcePaths":["src"],"outputPath":"out"}}});
    let root = ws.copy_and_import_folder(
        "singlefile/lesson1",
        Some("src/org/samples/HelloWorld.java"),
    );
    assert_eq!(1, list(&mut ws)["data"].as_array().unwrap().len());
    assert_eq!(
        json!({"status":true,"message":format!("Successfully added 'lesson1/samples' to the project {}'s source path.",name(&root)),"sourcePaths":["src","samples"]}),
        change(&mut ws, true, &root.join("samples"))
    );
    let metadata = ws.workspace_project_location(&name(&root));
    assert_eq!(
        "_/out",
        entries(&metadata)
            .iter()
            .find(|e| e["kind"] == "output")
            .unwrap()["path"]
    );
    let output_key = "org.eclipse.jdt.ls.core.outputPath";
    assert_eq!(
        json!(root.join("out").to_string_lossy()),
        ws.project_settings(&file_uri(&root), &[output_key])[output_key]
    );
    ws.restart();
    assert_eq!(1, list(&mut ws)["data"].as_array().unwrap().len());
    assert_eq!(
        json!(root.join("out").to_string_lossy()),
        ws.project_settings(&file_uri(&root), &[output_key])[output_key]
    );
}
