//! Port of `org.eclipse.jdt.ls.core.internal.filesystem.MavenProjectMetadataFileTest`.
//!
//! Each `@ParameterizedTest` runs once per `data()` entry; the system
//! property is a JVM option of the server under test.

mod common;

use common::jdtls::*;
use common::maven::*;
use common::metadata::*;
use common::projects::*;
use serde_json::json;
use std::path::PathBuf;

const INVALID: &str = "invalid";
const MAVEN_INVALID: &str = "maven/invalid";

fn workspace_in(fs_mode: &str) -> Workspace {
    let mut ws = workspace();
    set_fs_mode(&mut ws, fs_mode);
    ws
}

fn update_project(ws: &mut Workspace, project: &std::path::Path) {
    ws.client().notify(
        "java/projectConfigurationUpdate",
        json!({ "uri": dir_uri(project) }),
    );
    ws.wait_for_background_jobs();
}

fn assert_metadata_file(
    ws: &Workspace,
    fs_mode: &str,
    project: &std::path::Path,
    name: &str,
    rel: &str,
) -> PathBuf {
    let path = metadata_location(ws, fs_mode, project, name, rel);
    assert!(path.exists(), "{} does not exist", path.display());
    assert_eq!(
        is_prefix_of(project, &path),
        generates_metadata_files_at_project_root(fs_mode),
        "{}",
        path.display()
    );
    path
}

#[test]
fn test_metadata_file_location() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "salut";
        ws.import_projects(&[&format!("maven/{name}")]);
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));

        assert_metadata_file(&ws, fs_mode, &project, name, ".project");
        assert_metadata_file(&ws, fs_mode, &project, name, ".classpath");
        assert_metadata_file(&ws, fs_mode, &project, name, ".settings");
    }
}

#[test]
fn test_metadata_file_sync() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "quickstart2";
        ws.import_projects(&[&format!("maven/{name}")]);
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));

        let pom = project.join("pom.xml");
        let content = std::fs::read_to_string(&pom).unwrap();
        let content = content.replace(">11<", ">1.8<");
        std::fs::write(&pom, content).unwrap();
        update_project(&mut ws, &project);

        let classpath = metadata_location(&ws, fs_mode, &project, name, ".classpath");
        let classpath_content = std::fs::read_to_string(classpath).unwrap();
        assert!(classpath_content.contains("StandardVMType/JavaSE-1.8"), "{classpath_content}");
    }
}

#[test]
fn test_invalid_project() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        ws.import_projects(&[MAVEN_INVALID]);
        let projects = ws.all_projects(true);
        assert_eq!(1, projects.len());
        let invalid = ws.project_root(INVALID);
        ws.assert_is_maven_project(&invalid);
        let project_file = metadata_location(&ws, fs_mode, &invalid, INVALID, ".project");
        assert!(project_file.exists());
        // `invalid.close(..)`: the server keeps no open handle on the file.
        assert!(project_file.exists());
        std::fs::remove_file(&project_file).unwrap();
        assert!(!project_file.exists());
        ws.restart();
        ws.import_projects(&[MAVEN_INVALID]);
        let projects = ws.all_projects(true);
        assert_eq!(1, projects.len());
        let invalid = ws.project_root(INVALID);
        ws.assert_is_maven_project(&invalid);
    }
}

#[test]
fn test_delete_classpath() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "salut";
        ws.import_projects(&[&format!("maven/{name}")]);
        let project = ws.project_root(name);
        ws.assert_is_java_project(&project);
        ws.assert_is_maven_project(&project);
        let dot_classpath = metadata_location(&ws, fs_mode, &project, name, ".classpath");
        assert!(dot_classpath.exists());
        std::fs::remove_file(&dot_classpath).unwrap();
        ws.files_changed(&[(&dot_classpath, 3)]);
        let project = ws.project_root(name);
        let bin = project.join("bin");
        assert!(!bin.exists());
        // `dotClasspath.exists()` asks the resource tree, which only the
        // project-root file is refreshed from; the redirected file stays
        // deleted on disk while the project keeps its classpath.
        if generates_metadata_files_at_project_root(fs_mode) {
            assert!(metadata_location(&ws, fs_mode, &project, name, ".classpath").exists());
        }
        ws.assert_is_java_project(&project);
        ws.assert_is_maven_project(&project);
    }
}

#[test]
fn test_factory_path_file_location() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "autovalued";
        ws.import_projects(&[&format!("maven/{name}")]);
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));

        assert_metadata_file(&ws, fs_mode, &project, name, ".project");
        assert_metadata_file(&ws, fs_mode, &project, name, ".classpath");
        assert_metadata_file(&ws, fs_mode, &project, name, ".settings");
        assert_metadata_file(&ws, fs_mode, &project, name, ".factorypath");
    }
}

#[test]
fn test_multiple_metadata_file() {
    for fs_mode in FS_MODES {
        if generates_metadata_files_at_project_root(fs_mode) {
            continue;
        }
        let mut ws = workspace_in(fs_mode);
        let name = "quickstart2";
        ws.import_projects(&[&format!("maven/{name}")]);
        ws.wait_for_background_jobs();
        let project = ws.project_root(name);
        let project_description = metadata_location(&ws, fs_mode, &project, name, ".project");
        let classpath_file = metadata_location(&ws, fs_mode, &project, name, ".classpath");
        let preferences_file = metadata_location(&ws, fs_mode, &project, name, ".settings");
        std::fs::copy(&project_description, project.join(".project")).unwrap();
        std::fs::copy(&classpath_file, project.join(".classpath")).unwrap();
        copy_dir(&preferences_file, &project.join(".settings"));

        update_project(&mut ws, &project);
        ws.assert_no_errors(&project);

        let pom = project.join("pom.xml");
        let content = std::fs::read_to_string(&pom).unwrap();
        let content = content.replace(">11<", ">1.8<");
        let content = content.replace(">11<", ">1.8<");
        std::fs::write(&pom, content).unwrap();
        update_project(&mut ws, &project);

        // if the metadata file stores both at project root & workspace, the file at project root wins.
        let new_content = std::fs::read_to_string(project.join(".classpath")).unwrap();
        assert!(new_content.contains("StandardVMType/JavaSE-1.8"), "{new_content}");
    }
}
