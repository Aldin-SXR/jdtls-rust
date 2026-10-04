//! Port of `org.eclipse.jdt.ls.core.internal.managers.EclipseBuildSupportTest`.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;

#[test]
fn test_update_jar() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws.import_projects(&["eclipse/updatejar"]);
    let project = ws.dir.join("eclipse/updatejar");
    ws.assert_is_java_project(&project);
    let errors = ws.error_markers(&project);
    assert_eq!(2, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
    let valid_foo_jar = project.join("foo.jar");
    let dest_lib = project.join("lib");
    std::fs::create_dir_all(&dest_lib).unwrap();
    let new_jar = dest_lib.join("foo.jar");
    std::fs::copy(&valid_foo_jar, &new_jar).unwrap();
    ws.files_changed(&[(&new_jar, 1)]);
    let errors = ws.error_markers(&project);
    assert_eq!(0, errors.len(), "Unexpected errors {}", markers_to_string(&errors));
}
