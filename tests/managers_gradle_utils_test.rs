//! Port of `org.eclipse.jdt.ls.core.internal.managers.GradleUtilsTest`.
//! `GradleUtils` is an internal API, so the test runs against `project::gradle::util`.

mod common;

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use common::jdtls::java_home;
use project::gradle::util::*;
use project::runtime::RuntimeRegistry;
use std::path::Path;

#[test]
fn test_get_jdk_to_launch_daemon() {
    assert_eq!("17", get_major_java_version("17.0.8"));
    assert_eq!("1.8", get_major_java_version("1.8.0_202"));
}

#[test]
fn test_get_major_java_version() {
    let registry = RuntimeRegistry::with_default_home(Path::new(&java_home()));
    let vm_installs = get_all_vm_installs(Some(&registry), &[]);
    assert!(!vm_installs.is_empty());
    for (k, v) in &vm_installs {
        let java_home = get_jdk_to_launch_daemon(&vm_installs, k);
        assert_eq!(
            Some(v),
            java_home.as_ref(),
            "javaHome={}",
            java_home
                .as_ref()
                .map_or_else(String::new, |h| h.display().to_string())
        );
    }
}

#[test]
fn test_compatiblity() {
    let gradle_version = GradleVersion::version("9.1").unwrap();
    assert_eq!("25", get_highest_supported_java(&gradle_version));
}
