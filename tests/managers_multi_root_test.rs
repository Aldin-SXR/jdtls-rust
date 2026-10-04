//! Port of `org.eclipse.jdt.ls.core.internal.managers.MultiRootTest`.
//!
//! `importProjects(folders)` is a server (re)start with those workspace
//! folders, `updateWorkspaceFolders(added, removed)` a
//! `workspace/didChangeWorkspaceFolders` notification; the workspace projects
//! are those `java.project.getAll` (`includeNonJava`) reports.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;
use std::path::PathBuf;

const ECLIPSE_FOLDER: &str = "eclipse/hello";
const MAVEN_FOLDER: &str = "maven/salut";
const MAVEN_MULTI_FOLDER: &str = "maven/multi";
const GRADLE_FOLDER: &str = "gradle/simple-gradle";

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "maven": { "downloadSources": true } } });
    ws
}

fn location(ws: &Workspace, rel: &str) -> PathBuf {
    ws.dir.join(rel)
}

/// `WorkspaceHelper.getProject(name) != null` for the project at `rel`.
fn has(ws: &mut Workspace, rel: &str) -> bool {
    let p = location(ws, rel);
    ws.has_project_at(&p, true)
}

/// `importProjects(folders)`: copy the folders and initialize the projects.
fn import_projects(ws: &mut Workspace, folders: &[&str]) {
    let mut roots = Vec::new();
    for f in folders {
        roots.push(ws.copy_files(f));
    }
    ws.restart();
    ws.set_roots(roots);
}

/// `updateProjects(added, removed)`.
fn update_projects(ws: &mut Workspace, added: &[&str], removed: &[&str]) {
    for a in added {
        let to = ws.dir.join(a);
        if !to.exists() {
            ws.copy_files(a);
        }
        ws.import_root(&to);
    }
    for r in removed {
        let root = ws.dir.join(r);
        ws.remove_root(&root);
    }
    ws.wait_for_background_jobs();
}

#[test]
fn test_initialize_with_multi_folders() {
    let mut ws = workspace();
    {
        import_projects(&mut ws, &[ECLIPSE_FOLDER, MAVEN_FOLDER]);
        assert_eq!(2, ws.all_projects(true).len());
        assert!(has(&mut ws, "eclipse/hello"));
        assert!(has(&mut ws, "maven/salut"));
    }
    // simulate a new start with a different set of projects
    {
        import_projects(&mut ws, &[MAVEN_MULTI_FOLDER, ECLIPSE_FOLDER]);

        assert_eq!(3, ws.all_projects(true).len());
        assert!(has(&mut ws, "eclipse/hello"));
        assert!(!has(&mut ws, "maven/salut"));
        assert!(has(&mut ws, "maven/multi/project1"));
        assert!(has(&mut ws, "maven/multi/project2"));
    }
}

#[test]
fn test_update_multi_folders() {
    let mut ws = workspace();
    {
        import_projects(&mut ws, &[ECLIPSE_FOLDER, MAVEN_FOLDER]);
        assert_eq!(2, ws.all_projects(true).len());
        assert!(has(&mut ws, "eclipse/hello"));
        assert!(has(&mut ws, "maven/salut"));
    }
    {
        // add a folder that contains 2 projects
        update_projects(&mut ws, &[MAVEN_MULTI_FOLDER], &[MAVEN_FOLDER]);

        assert_eq!(3, ws.all_projects(true).len());
        assert!(has(&mut ws, "eclipse/hello"));
        assert!(!has(&mut ws, "maven/salut"));
        assert!(has(&mut ws, "maven/multi/project1"));
        assert!(has(&mut ws, "maven/multi/project2"));
    }
    {
        // add a folder that existed before
        // remove a folder that contains 2 projects
        update_projects(&mut ws, &[MAVEN_FOLDER], &[MAVEN_MULTI_FOLDER]);

        assert_eq!(2, ws.all_projects(true).len());
        assert!(has(&mut ws, "eclipse/hello"));
        assert!(has(&mut ws, "maven/salut"));
        assert!(!has(&mut ws, "maven/multi/project1"));
        assert!(!has(&mut ws, "maven/multi/project2"));
    }
    {
        // add a gradle folder
        // remove a folder that contains 2 projects
        update_projects(&mut ws, &[GRADLE_FOLDER], &[ECLIPSE_FOLDER, MAVEN_FOLDER]);

        assert_eq!(1, ws.all_projects(true).len());
        assert!(!has(&mut ws, "eclipse/hello"));
        assert!(!has(&mut ws, "maven/salut"));
        assert!(has(&mut ws, "gradle/simple-gradle"));
    }
}
