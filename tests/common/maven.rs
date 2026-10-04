//! `AbstractMavenBasedTest` support.

use super::jdtls::*;
use super::projects::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// `AbstractProjectsManagerBasedTest`: a workspace with the upstream test
/// preferences (`initPreferences`) and a client that supports progress
/// reports (`clientPreferences.isProgressReportSupported()`).
pub fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws.init_options["extendedClientCapabilities"]["progressReportProvider"] = json!(true);
    ws
}

/// `importMavenProject(name)`: import `maven/<name>` and assert it is a
/// Maven project; returns its location.
pub fn import_maven_project(ws: &mut Workspace, name: &str) -> PathBuf {
    ws.import_projects(&[&format!("maven/{name}")]);
    let project = ws.dir.join("maven").join(name);
    ws.assert_is_maven_project(&project);
    project
}

/// `importSimpleJavaProject()`.
pub fn import_simple_java_project(ws: &mut Workspace) -> PathBuf {
    let project = import_maven_project(ws, "salut");
    ws.assert_is_java_project(&project);
    assert_eq!("1.8", ws.java_source_level(&project));
    ws.assert_no_errors(&project);
    project
}

/// `comment(s, from, to)`.
pub fn comment(s: &str, from: &str, to: &str) -> String {
    s.replace(from, &format!("<!--{from}")).replace(to, &format!("{to}-->"))
}

/// The `language/progressReport` notifications received so far.
pub fn progress_reports(ws: &mut Workspace) -> Vec<Value> {
    ws.wait_idle();
    ws.client().notifications.iter().filter(|n| n["method"] == "language/progressReport").map(|n| n["params"].clone()).collect()
}

/// `assertTaskCompleted(taskName)`.
pub fn assert_task_completed(ws: &mut Workspace, task_name: &str) {
    let reports = progress_reports(ws);
    assert!(!reports.is_empty(), "No progress report were sent to the client");
    let mut completed_task = false;
    let mut tasks: Vec<String> = Vec::new();
    let mut task_id: Option<String> = None;
    for report in &reports {
        let id = report["id"].as_str().expect("report id").to_owned();
        let task = report["task"].as_str().unwrap_or("").to_owned();
        if !tasks.contains(&task) {
            tasks.push(task.clone());
        }
        if task == task_name {
            task_id = Some(id.clone());
        }
        if task_id.as_deref() == Some(id.as_str()) && report["complete"] == json!(true) {
            completed_task = true;
        }
    }
    assert!(task_id.is_some(), "'{task_name}' was not found among {tasks:?}");
    assert!(completed_task, "'{task_name}' was not completed");
}

/// Number of jobs named `name` that reported progress (`JobChangeAdapter.scheduled`).
pub fn jobs_named(ws: &mut Workspace, name: &str) -> usize {
    let mut ids: Vec<String> = progress_reports(ws)
        .iter()
        .filter(|r| r["task"] == json!(name))
        .filter_map(|r| r["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    ids.len()
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}
