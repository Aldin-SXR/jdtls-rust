//! Port of `org.eclipse.jdt.ls.core.internal.managers.WrapperValidatorTest`.
//! `WrapperValidator` is an internal API, so the test runs against
//! `project::gradle::checksums`. The validator state is process-global, as in
//! the upstream plugin, so the tests run one at a time.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::fixtures_dir;
use project::gradle::checksums::{self, WrapperValidator};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

struct Property<'a>(MutexGuard<'a, ()>);

impl Drop for Property<'_> {
    fn drop(&mut self) {
        checksums::set_checksum_cache_dir(None);
    }
}

fn set_property() -> Property<'static> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    checksums::set_checksum_cache_dir(Some(&PathBuf::from("target/gradle/checksums")));
    checksums::clear();
    Property(guard)
}

fn source_project_directory() -> PathBuf {
    fixtures_dir().join("projects")
}

#[test]
fn test_gradle_wrapper() {
    let _property = set_property();
    let file = source_project_directory().join("gradle/simple-gradle");
    assert!(file.is_dir());
    let sha256_directory = checksums::get_sha256_cache_file();
    assert!(sha256_directory.is_dir());
    let result = WrapperValidator::new(100)
        .check_wrapper(&file.to_string_lossy())
        .unwrap();
    assert!(result.is_valid());
    // test cache
    assert!(sha256_directory.is_dir());
    let file_name = "gradle-6.3-wrapper.jar.sha256";
    let mut sha256 = None;
    for json in checksums::internal_checksums() {
        let wrapper_checksum_url = json["wrapperChecksumUrl"].as_str();
        if wrapper_checksum_url.is_some_and(|u| u.ends_with(&format!("/{file_name}"))) {
            sha256 = json["sha256"].as_str().map(str::to_owned);
            break;
        }
    }
    assert_eq!(
        Some("1cef53de8dc192036e7b0cc47584449b0cf570a00d560bfaa6c9eabe06e1fc06"),
        sha256.as_deref()
    );
}

#[test]
fn test_missing_sha256() {
    let _property = set_property();
    let wrapper_validator = WrapperValidator::new(100);
    let allowed = checksums::get_allowed();
    let disallowed = checksums::get_disallowed();
    let file = source_project_directory().join("gradle/gradle-4.0");
    let file_path = file.to_string_lossy().into_owned();
    wrapper_validator.check_wrapper(&file_path).unwrap();
    let size = checksums::size();
    let sha256 = vec!["41c8aa7a337a44af18d8cda0d632ebba469aef34f3041827624ef5c1a4e4419d".to_owned()];
    let run = || {
        checksums::clear();
        checksums::disallow(sha256.clone());
        assert!(file.is_dir());
        let result = wrapper_validator.check_wrapper(&file_path).unwrap();
        assert!(!result.is_valid());
        assert!(!result.checksum.is_empty());
        checksums::clear();
        checksums::allow(sha256.clone());
        let result = wrapper_validator.check_wrapper(&file_path).unwrap();
        assert!(result.is_valid());
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
    checksums::clear();
    checksums::allow(allowed.snapshot());
    checksums::disallow(disallowed.snapshot());
    wrapper_validator.check_wrapper(&file_path).unwrap();
    assert_eq!(size, checksums::size());
    if let Err(e) = outcome {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn test_preferences() {
    let _property = set_property();
    let wrapper_validator = WrapperValidator::new(100);
    let allowed = checksums::get_allowed();
    let disallowed = checksums::get_disallowed();
    let file = source_project_directory().join("gradle/gradle-4.0");
    let file_path = file.to_string_lossy().into_owned();
    wrapper_validator.check_wrapper(&file_path).unwrap();
    let size = checksums::size();
    let list = vec![json!({
        "sha256": "41c8aa7a337a44af18d8cda0d632ebba469aef34f3041827624ef5c1a4e4419d",
        "allowed": true
    })];
    let run = || {
        let result = wrapper_validator.check_wrapper(&file_path).unwrap();
        assert!(!result.is_valid());
        assert!(!result.checksum.is_empty());
        checksums::clear();
        checksums::put_sha256(&list);
        let result = wrapper_validator.check_wrapper(&file_path).unwrap();
        assert!(result.is_valid());
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
    checksums::clear();
    checksums::allow(allowed.snapshot());
    checksums::disallow(disallowed.snapshot());
    wrapper_validator.check_wrapper(&file_path).unwrap();
    assert_eq!(size, checksums::size());
    if let Err(e) = outcome {
        std::panic::resume_unwind(e);
    }
}
