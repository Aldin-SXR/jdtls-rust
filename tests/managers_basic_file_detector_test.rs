//! Port of `org.eclipse.jdt.ls.core.internal.managers.BasicFileDetectorTest`.
//!
//! `BasicFileDetector` is an internal class with no LSP surface, so this is a
//! unit-level port of the Rust `project::detect::FileDetector` (it cannot run
//! against the oracle).  Upstream runs with the working directory at the test
//! plugin, so `projects/buildfiles` is `tests/fixtures/projects/buildfiles`
//! here; the constructor's preference exclusions (`java.import.exclusions`
//! defaults) are passed explicitly.

#[path = "../src/project/detect.rs"]
#[allow(dead_code)]
mod detect;

use detect::{FileDetector, DEFAULT_IMPORT_EXCLUSIONS};
use std::path::{Path, PathBuf};

const PROJECTS: &str = "tests/fixtures/projects";

fn p(rel: &str) -> String {
    format!("{PROJECTS}/{rel}")
}

/// `new BasicFileDetector(root, "buildfile")` with the default preferences.
fn detector(root: &str) -> FileDetector {
    detector_with(
        root,
        DEFAULT_IMPORT_EXCLUSIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

fn detector_with(root: &str, exclusions: Vec<String>) -> FileDetector {
    FileDetector::new(Path::new(root), &["buildfile"]).add_exclusions(exclusions)
}

fn strings(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter()
        .map(|d| d.to_string_lossy().into_owned())
        .collect()
}

fn assert_found(dirs: &[PathBuf], expected: &[String]) {
    let found = strings(dirs);
    let mut missing: Vec<String> = expected.to_vec();
    for d in &found {
        missing.retain(|m| m != d);
    }
    assert_eq!(0, missing.len(), "Directories were not detected{missing:?}");
}

fn default_exclusions() -> Vec<String> {
    DEFAULT_IMPORT_EXCLUSIONS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn test_scan_build_file_at_root_excluding_nested_dirs() {
    let dirs = detector(&p("buildfiles")).include_nested(false).scan();
    assert_eq!(1, dirs.len(), "Found {dirs:?}"); // .metadata is ignored
    assert_eq!(p("buildfiles"), dirs[0].to_string_lossy());
}

#[test]
fn test_scan_build_file_at_root_including_nested_dirs() {
    let dirs = detector(&format!("{}/", p("buildfiles"))).scan();
    assert_eq!(6, dirs.len(), "Found {dirs:?}");
    assert_found(
        &dirs,
        &[
            p("buildfiles"),
            p("buildfiles/parent/1_0/0_2_0"),
            p("buildfiles/parent/1_0/0_2_1"),
            p("buildfiles/parent/1_1"),
            p("buildfiles/parent/1_1/1_2_0"),
            p("buildfiles/parent/1_1/1_2_1"),
        ]
        .iter()
        .map(|s| s.trim_end_matches('/').to_owned())
        .collect::<Vec<_>>(),
    );
}

#[test]
fn test_scan_excluding_nested_build_files_depth3() {
    let dirs = detector(&p("buildfiles/parent"))
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(3, dirs.len(), "Found {dirs:?}");
    assert_found(
        &dirs,
        &[
            p("buildfiles/parent/1_1"),
            p("buildfiles/parent/1_0/0_2_0"),
            p("buildfiles/parent/1_0/0_2_1"),
        ],
    );
}

#[test]
fn test_inclusions() {
    let mut inclusions = default_exclusions();
    inclusions.push("**/parent/**".into());
    inclusions.push("!**/parent".into());
    inclusions.push("!**/parent/1_0".into());
    inclusions.push("!**/parent/1_0/*".into());
    let dirs = detector_with(&p("buildfiles/parent"), inclusions.clone())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(2, dirs.len(), "Found {dirs:?} ,exclusions={inclusions:?}");
    assert_found(
        &dirs,
        &[
            p("buildfiles/parent/1_0/0_2_0"),
            p("buildfiles/parent/1_0/0_2_1"),
        ],
    );
    let dirs = detector_with(&p("buildfiles/parent"), default_exclusions())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(3, dirs.len(), "Found {dirs:?}");
}

#[test]
fn test_inclusions2() {
    let mut inclusions = default_exclusions();
    inclusions.push("**/parent/**".into());
    inclusions.push("!**/parent".into());
    inclusions.push("!**/parent/1_0".into());
    inclusions.push("!**/parent/1_0/0_2_0".into());
    let dirs = detector_with(&p("buildfiles/parent"), inclusions.clone())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(1, dirs.len(), "Found  ,exclusions={inclusions:?}{dirs:?}");
    assert_found(&dirs, &[p("buildfiles/parent/1_0/0_2_0")]);
}

#[test]
fn test_inclusions3() {
    let mut inclusions = default_exclusions();
    inclusions.push("!**/parent".into());
    inclusions.push("!**/parent/1_0".into());
    inclusions.push("!**/parent/1_0/0_2_0".into());
    inclusions.push("**/parent/**".into());
    let dirs = detector_with(&p("buildfiles/parent"), inclusions.clone())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(0, dirs.len(), "Found  ,exclusions={inclusions:?}{dirs:?}");
}

#[test]
fn test_inclusions4() {
    let mut inclusions = default_exclusions();
    inclusions.push("**".into());
    inclusions.push("!**/parent/1_0/**".into());
    let dirs = detector_with(&p("buildfiles/parent"), inclusions.clone())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(2, dirs.len(), "Found  ,exclusions={inclusions:?}{dirs:?}");
}

#[test]
fn test_inclusions5() {
    let mut inclusions = default_exclusions();
    inclusions.push("!**/parent/1_0/**".into());
    inclusions.push("**".into());
    let dirs = detector_with(&p("buildfiles/parent"), inclusions.clone())
        .include_nested(false)
        .max_depth(3)
        .scan();
    assert_eq!(0, dirs.len(), "Found  ,exclusions={inclusions:?}{dirs:?}");
}

#[test]
fn test_scan_nested_build_files_depth2() {
    let dirs = detector(&p("buildfiles/parent"))
        .include_nested(false)
        .max_depth(2)
        .scan();
    assert_eq!(1, dirs.len(), "Found {dirs:?}");
    assert_eq!(p("buildfiles/parent/1_1"), dirs[0].to_string_lossy());
}

fn random() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
        % 10000
}

#[test]
fn test_scan_symbolic_links() {
    let temp_directory = std::env::temp_dir().join(format!("projects_symbolic_link-{}", random()));
    std::fs::create_dir_all(&temp_directory).unwrap();
    let target_link_folder = temp_directory.join("buildfiles");
    let result = std::panic::catch_unwind(|| {
        std::os::unix::fs::symlink(
            std::fs::canonicalize(p("buildfiles")).unwrap(),
            &target_link_folder,
        )
        .unwrap();
        let dirs = detector(&temp_directory.to_string_lossy())
            .include_nested(false)
            .max_depth(2)
            .scan();
        assert_eq!(1, dirs.len(), "Found {dirs:?}"); // .metadata is ignored
        assert_eq!(
            target_link_folder.to_string_lossy(),
            dirs[0].to_string_lossy()
        );
    });
    std::fs::remove_file(&target_link_folder).ok();
    std::fs::remove_dir_all(&temp_directory).ok();
    result.unwrap();
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

#[test]
fn test_scan_circular_symbolic_links() {
    let origin_directory = PathBuf::from(p("buildfiles"));
    let temp_directory =
        std::env::temp_dir().join(format!("circular_symbolic_link_ws-{}", random()));
    let circular_symbolic_link = temp_directory.join("circular_symbolic_link");
    let result = std::panic::catch_unwind(|| {
        copy_dir(&origin_directory, &temp_directory);
        std::os::unix::fs::symlink(&temp_directory, &circular_symbolic_link).unwrap();
        let dirs = detector(&temp_directory.to_string_lossy()).scan();
        assert_eq!(6, dirs.len(), "Found {dirs:?}");
        let relative: Vec<PathBuf> = dirs
            .iter()
            .map(|d| d.strip_prefix(&temp_directory).unwrap().to_path_buf())
            .collect();
        assert_found(
            &relative,
            &[
                "",
                "parent/1_0/0_2_0",
                "parent/1_0/0_2_1",
                "parent/1_1",
                "parent/1_1/1_2_0",
                "parent/1_1/1_2_1",
            ]
            .map(String::from),
        );
    });
    std::fs::remove_file(&circular_symbolic_link).ok();
    std::fs::remove_dir_all(&temp_directory).ok();
    result.unwrap();
}

#[test]
fn test_scan_not_found_directory() {
    let not_found_directory = std::env::temp_dir().join(format!("foo_bar_not_found_{}", random()));
    let dirs = detector(&not_found_directory.to_string_lossy()).scan();
    assert_eq!(0, dirs.len(), "Found {dirs:?}"); // No uncaught exception occurs
}
