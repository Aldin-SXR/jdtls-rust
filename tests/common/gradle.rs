//! `AbstractGradleBasedTest` support.

use super::jdtls::*;
use super::projects::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A workspace with the upstream test preferences and a client that supports
/// progress reports.
pub fn workspace() -> Workspace {
    super::maven::workspace()
}

/// `importGradleProject(name)`: import `gradle/<name>` and assert it is a
/// Gradle project; returns its location.
pub fn import_gradle_project(ws: &mut Workspace, name: &str) -> PathBuf {
    ws.import_projects(&[&format!("gradle/{name}")]);
    let project = ws.dir.join("gradle").join(name);
    assert_is_gradle_project(ws, &project);
    project
}

/// `importSimpleJavaProject()`.
pub fn import_simple_java_project(ws: &mut Workspace) -> PathBuf {
    let project = import_gradle_project(ws, "simple-gradle");
    ws.assert_is_java_project(&project);
    assert_eq!("1.8", ws.java_source_level(&project));
    project
}

/// `assertIsGradleProject(project)`.
pub fn assert_is_gradle_project(ws: &mut Workspace, root: &Path) {
    assert!(
        ws.has_project_at(root, true),
        "{} is not a project",
        root.display()
    );
    let natures = ws.natures(root);
    assert!(
        natures.iter().any(|n| n == GRADLE_NATURE),
        "{} is missing the Gradle nature",
        root.display()
    );
}

/// `projectsManager.updateProject(project, force)`.
pub fn update_project(ws: &mut Workspace, project: &Path) {
    ws.client().notify(
        "java/projectConfigurationUpdate",
        json!({ "uri": dir_uri(project) }),
    );
    ws.wait_for_background_jobs();
}

pub const BUILDSHIP_PREFS: &str = ".settings/org.eclipse.buildship.core.prefs";

/// The Buildship preferences of the project at `project`, empty when absent.
pub fn buildship_prefs(project: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(project.join(BUILDSHIP_PREFS)).unwrap_or_default();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Buildship's `BuildConfiguration` stored for a root project.
#[derive(Debug, Default)]
pub struct StoredBuildConfiguration {
    pub override_workspace_settings: bool,
    pub arguments: Vec<String>,
    pub gradle_user_home: Option<String>,
}

fn load_build_configuration(root: &Path) -> StoredBuildConfiguration {
    let prefs = buildship_prefs(root);
    StoredBuildConfiguration {
        override_workspace_settings: prefs.get("override.workspace.settings").is_some_and(|v| v == "true"),
        arguments: prefs
            .get("arguments")
            .map(|a| a.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
        gradle_user_home: prefs
            .get("gradle.user.home")
            .filter(|h| !h.is_empty())
            .cloned(),
    }
}

/// `CorePlugin.configurationManager().loadProjectConfiguration(project).getBuildConfiguration()`:
/// the build configuration of the project's root project.
pub fn load_project_configuration(project: &Path) -> StoredBuildConfiguration {
    let dir = buildship_prefs(project)
        .get("connection.project.dir")
        .cloned()
        .unwrap_or_default();
    if dir.is_empty() {
        load_build_configuration(project)
    } else {
        load_build_configuration(&canonical(&project.join(dir)))
    }
}

/// `CorePlugin.configurationManager().loadBuildConfiguration(location)`.
pub fn load_build_configuration_at(location: &Path) -> StoredBuildConfiguration {
    load_build_configuration(location)
}
