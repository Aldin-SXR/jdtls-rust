//! Port of `org.eclipse.jdt.ls.core.internal.managers.GradleBuildSupportTest`.

mod common;

use common::gradle::*;
use common::jdtls::*;
use common::projects::*;
use serde_json::json;

#[test]
fn test_update() {
    let mut ws = workspace();
    let project = import_simple_java_project(&mut ws);

    let gradle = project.join("build.gradle");
    let original_gradle = std::fs::read_to_string(&gradle).unwrap();

    let new_gradle_path = project.join("build2.gradle");

    // Remove dependencies to cause compilation errors
    let new_gradle = std::fs::read_to_string(&new_gradle_path).unwrap();
    std::fs::write(&gradle, new_gradle).unwrap();
    ws.wait_for_background_jobs();
    // Contents changed outside the workspace, so should not change
    ws.assert_no_errors(&project);

    // Giving a nudge, so that errors show up
    update_project(&mut ws, &project);

    ws.wait_for_background_jobs();
    ws.assert_has_errors(&project, &[]);
    assert_eq!("1.8", ws.java_source_level(&project));

    // Fix gradle file, trigger build
    std::fs::write(&gradle, original_gradle).unwrap();
    update_project(&mut ws, &project);
    ws.wait_for_background_jobs();
    ws.assert_no_errors(&project);
    assert_eq!("1.8", ws.java_source_level(&project));
}

// https://github.com/redhat-developer/vscode-java/issues/3893
#[test]
fn test_update_module() {
    let mut ws = workspace();
    ws.settings["java"]["configuration"] = json!({ "updateBuildConfiguration": "disabled" });
    ws.import_projects(&["gradle/sample"]);
    let projects = ws.all_projects(true);
    assert_eq!(2, projects.len()); // app, sample
    let root = ws.dir.join("gradle/sample");
    assert_is_gradle_project(&mut ws, &root);
    let project = root.join("app");
    assert_is_gradle_project(&mut ws, &project);
    ws.assert_is_java_project(&project);
    let type_name = "org.apache.commons.lang3.StringUtils";
    assert!(ws.try_class_file_uri("app", type_name).is_none());
    let build2 = std::fs::read_to_string(project.join("build.gradle2")).unwrap();
    let build = project.join("build.gradle");
    std::fs::write(&build, build2).unwrap();
    ws.files_changed(&[(&build, 2)]);
    ws.wait_for_background_jobs();
    assert!(ws.try_class_file_uri("app", type_name).is_none());
    update_project(&mut ws, &project);
    ws.wait_for_background_jobs();
    assert!(ws.try_class_file_uri("app", type_name).is_some());
}
