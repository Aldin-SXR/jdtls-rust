//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ProjectConfigurationUpdateHandlerTest`.
//!
//! The upstream test mocks `ProjectsManager` and verifies that
//! `updateProjects(projects, true)` is called once; `updateProjects` announces
//! itself with the `Updating project configurations...` status message, which
//! is counted here.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;

const UPDATING: &str = "Updating project configurations...";

fn update_calls(ws: &mut Workspace) -> usize {
    ws.wait_for_background_jobs();
    ws.client()
        .notifications
        .iter()
        .filter(|n| n["method"] == "language/status" && n["params"]["message"] == UPDATING)
        .count()
}

#[test]
fn test_update_configuration() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/multimodule"]);
    let project = ws.dir.join("maven/multimodule");
    ws.client();
    let before = update_calls(&mut ws);

    ws.client().notify("java/projectConfigurationUpdate", json!({ "uri": dir_uri(&project) }));

    assert_eq!(before + 1, update_calls(&mut ws));
}

#[test]
fn test_update_configurations() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/multimodule"]);
    let module1 = ws.dir.join("maven/multimodule/module1");
    let module2 = ws.dir.join("maven/multimodule/module2");
    ws.client();
    let before = update_calls(&mut ws);

    ws.client().notify(
        "java/projectConfigurationsUpdate",
        json!({ "identifiers": [{ "uri": dir_uri(&module1) }, { "uri": dir_uri(&module2) }] }),
    );

    assert_eq!(before + 1, update_calls(&mut ws));
}
