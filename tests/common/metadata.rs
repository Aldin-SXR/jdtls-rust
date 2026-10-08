//! Support for the `filesystem.*ProjectMetadataFileTest` ports.

use super::jdtls::*;
use super::projects::*;
use std::path::{Path, PathBuf};

pub const GENERATES_METADATA_FILES_AT_PROJECT_ROOT: &str =
    "java.import.generatesMetadataFilesAtProjectRoot";

/// The `data()` parameters: `System.setProperty(GENERATES_METADATA_FILES_AT_PROJECT_ROOT, fsMode)`.
pub const FS_MODES: [&str; 2] = ["false", "true"];

pub fn set_fs_mode(ws: &mut Workspace, fs_mode: &str) {
    ws.oracle_java_options
        .push(format!("-D{GENERATES_METADATA_FILES_AT_PROJECT_ROOT}={fs_mode}"));
}

/// `JLSFsUtils.generatesMetadataFilesAtProjectRoot()`.
pub fn generates_metadata_files_at_project_root(fs_mode: &str) -> bool {
    fs_mode.eq_ignore_ascii_case("true")
}

/// `FileUtil.toPath(project.getFile(rel).getLocationURI())`: the file system
/// location of a metadata file of the project at `location`.
pub fn metadata_location(ws: &Workspace, fs_mode: &str, location: &Path, name: &str, rel: &str) -> PathBuf {
    let at_root = location.join(rel);
    if generates_metadata_files_at_project_root(fs_mode) || at_root.exists() {
        return at_root;
    }
    if rel.starts_with(".settings/") && location.join(".settings").exists() {
        return at_root;
    }
    ws.server_workspace_dir()
        .join(".metadata/.plugins/org.eclipse.core.resources/.projects")
        .join(name)
        .join(rel)
}

/// `project.getLocation().isPrefixOf(path)`.
pub fn is_prefix_of(location: &Path, path: &Path) -> bool {
    canonical(path).starts_with(canonical(location))
}
