//! Port of `org.eclipse.jdt.ls.core.internal.filesystem.EclipseProjectMetadataFileTest`.

mod common;

use common::jdtls::*;
use common::metadata::*;
use common::projects::*;

fn workspace_in(fs_mode: &str) -> Workspace {
    let mut ws = Workspace::new();
    set_fs_mode(&mut ws, fs_mode);
    ws
}

#[test]
fn test_metadata_file_location() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "hello";
        ws.import_projects(&[&format!("eclipse/{name}")]);
        let project = ws.project_root(name);
        ws.assert_is_java_project(&project);

        let project_description = metadata_location(&ws, fs_mode, &project, name, ".project");
        assert!(project_description.exists());
        assert!(is_prefix_of(&project, &project_description));

        let classpath = metadata_location(&ws, fs_mode, &project, name, ".classpath");
        assert!(classpath.exists());
        assert!(is_prefix_of(&project, &classpath));

        let preferences = metadata_location(&ws, fs_mode, &project, name, ".settings");
        assert!(preferences.exists());
        assert!(is_prefix_of(&project, &preferences));
    }
}

#[test]
fn test_delete_classpath() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "classpath2";
        ws.import_projects(&[&format!("eclipse/{name}")]);
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));
        let dot_classpath = metadata_location(&ws, fs_mode, &project, name, ".classpath");
        assert!(dot_classpath.exists());
        std::fs::remove_file(&dot_classpath).unwrap();
        ws.files_changed(&[(&dot_classpath, 3)]);
        ws.wait_for_background_jobs();
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));
        assert!(!ws.natures(&project).iter().any(|n| n == JAVA_NATURE));
        let bin = project.join("bin");
        assert!(!bin.exists());
    }
}

#[test]
fn test_delete_non_metadata_classpath() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let name = "classpath3";
        ws.import_projects(&[&format!("eclipse/{name}")]);
        let project = ws.project_root(name);
        assert!(ws.has_project_at(&project, true));
        let dot_classpath = project.join("resources/.classpath");
        assert!(dot_classpath.exists());
        std::fs::remove_file(&dot_classpath).unwrap();
        ws.files_changed(&[(&dot_classpath, 3)]);
        ws.wait_for_background_jobs();
        let project = ws.project_root(name);
        assert!(ws.natures(&project).iter().any(|n| n == JAVA_NATURE));
    }
}
