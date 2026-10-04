//! Port of `org.eclipse.jdt.ls.core.internal.managers.StandardProjectManagerTest`.
//!
//! `StandardProjectsManager.buildSupports()` is internal (no LSP surface):
//! a unit-level port of `project::BUILD_SUPPORTS`.

#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use project::BuildSupport;

#[test]
fn test_check_build_support_order() {
    let expected_list = [BuildSupport::Gradle, BuildSupport::Maven, BuildSupport::Invisible, BuildSupport::Default, BuildSupport::Eclipse];
    let actual_list = project::BUILD_SUPPORTS;
    for i in 0..expected_list.len() {
        assert_eq!(expected_list[i], actual_list[i]);
    }
}
