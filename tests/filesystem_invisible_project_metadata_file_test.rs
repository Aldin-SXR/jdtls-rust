//! Port of `org.eclipse.jdt.ls.core.internal.filesystem.InvisibleProjectMetadataFileTest`.

mod common;

use common::jdtls::*;
use common::metadata::*;
use common::projects::*;
use serde_json::json;

fn workspace_in(fs_mode: &str) -> Workspace {
    let mut ws = Workspace::new();
    set_fs_mode(&mut ws, fs_mode);
    ws
}

#[test]
fn test_metadata_file_location() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let root = ws.copy_and_import_folder("singlefile/simple", Some("src/App.java"));
        assert!(ws.has_project_at(&root, true));
        let name = invisible_project_name(&root);
        let project = ws.workspace_project_location(&name);

        for rel in [".project", ".classpath", ".settings"] {
            let path = metadata_location(&ws, fs_mode, &project, &name, rel);
            assert!(path.exists(), "{}", path.display());
            assert_eq!(
                is_prefix_of(&project, &path),
                generates_metadata_files_at_project_root(fs_mode),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn test_project_settings() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let root = ws.copy_and_import_folder(
            "singlefile/lesson1",
            Some("src/org/samples/HelloWorld.java"),
        );
        assert!(ws.has_project_at(&root, true));
        assert_eq!(
            json!("ignore"),
            ws.java_option(
                &root,
                "org.eclipse.jdt.core.compiler.problem.missingSerialVersion"
            )
        );
        ws.assert_no_errors(&root);
    }
}

// https://github.com/eclipse/eclipse.jdt.ls/pull/1863#issuecomment-924395431
#[test]
fn test_preview_features_settings_disabled() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let root = ws.copy_and_import_folder("singlefile/java18a", Some("foo/bar/Foo.java"));
        assert!(ws.has_project_at(&root, true));
        ws.assert_no_errors(&root);
        assert_eq!(
            json!("disabled"),
            ws.java_option(
                &root,
                "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures"
            )
        );
    }
}

// https://github.com/eclipse/eclipse.jdt.ls/pull/1863#issuecomment-924395431
#[test]
#[ignore = "ECJ aborts a compilation with preview features enabled below the latest source level (26) and the server publishes that error on Foo.java; the oracle publishes no diagnostics for this invisible project"]
fn test_preview_features_setting_enabled() {
    for fs_mode in FS_MODES {
        let mut ws = workspace_in(fs_mode);
        let root = ws.copy_and_import_folder("singlefile/java18b", Some("foo/bar/Foo.java"));
        assert!(ws.has_project_at(&root, true));
        ws.assert_no_errors(&root);
        assert_eq!(
            json!("enabled"),
            ws.java_option(
                &root,
                "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures"
            )
        );
    }
}
