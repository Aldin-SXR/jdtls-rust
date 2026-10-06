//! Port of `org.eclipse.jdt.ls.core.internal.JVMConfiguratorTest`.
mod common;
#[path = "common/jvm_fixture.rs"]
mod fixture;
#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;
use serde_json::json;

#[test]
fn test_default_vm() {
    let mut f = fixture::Fixture::new(false);
    let home = f.fake.join("9");
    let result = f.command(json!({"api":"default","home":home}));
    assert_eq!(true, result["changed"], "A VM hasn't been changed");
    assert_eq!(true, result["different"]);
    assert_eq!("9", result["id"]);
}
#[test]
fn test_jvm() {
    let mut f = fixture::Fixture::new(false);
    let result = f.command(json!({"api":"configure","path":f.native,"name":"JavaSE-21","javadoc":"file:///javadoc","default":true,"dispose":true}));
    assert_eq!(true, result["valid"]);
    assert_eq!(true, result["directory"]);
    assert_eq!(true, result["validated"]);
    assert_eq!(true, result["changed"], "A VM hasn't been changed");
    assert_eq!(true, result["present"]);
    assert_eq!(true, result["vm2"]);
    assert!(result["version"].as_str().unwrap().starts_with("21"));
    assert_eq!(true, result["librariesNotNull"]);
    for library in result["libraries"].as_array().unwrap() {
        assert_eq!(result["javadoc"], library["javadoc"]);
    }
    assert_eq!(true, result["different"]);
    assert_eq!(true, result["defaultSame"]);
    assert_eq!(true, result["environmentPresent"]);
    assert_eq!(true, result["environmentSame"]);
    assert_eq!(true, result["disposedAbsent"]);
}
#[test]
fn test_invalid_javadoc() {
    let mut f = fixture::Fixture::new(false);
    let result = f.command(
        json!({"api":"javadoc","path":f.native,"name":"JavaSE-21","javadoc":f.native.join("doc")}),
    );
    assert_eq!(true, result["valid"]);
    assert!(!result["javadoc"].is_null());
}
#[test]
fn test_preview_feature_settings() {
    let mut f = fixture::Fixture::new(false);
    let root =
        f.ws.copy_and_import_folder("singlefile/java13", Some("foo/bar/Foo.java"));
    let versions = ["21", "26", "12", "21"];
    let results = f.preview(&root, &versions);
    for (i, version) in versions.iter().enumerate() {
        for project in results[i].as_array().unwrap() {
            assert_eq!(*version, project["compliance"]);
            assert_eq!(
                if *version == "26" {
                    "enabled"
                } else {
                    "disabled"
                },
                project["preview"]
            );
        }
    }
}
#[test]
fn test_invalid_runtime_with_actionable_notification() {
    let mut f = fixture::Fixture::new(true);
    let path = f.native.parent().unwrap().join("11a_nonexist");
    let result = f.command(json!({"api":"configure","path":path,"name":"JavaSE-21"}));
    let notices = f.notices(&result, "language/actionableNotification");
    assert_eq!(1, notices.len(), "{notices:#?}");
    assert_eq!(1, notices[0]["severity"]);
    assert_eq!(1, notices[0]["commands"].as_array().unwrap().len());
}
#[test]
fn test_invalid_runtime() {
    let mut f = fixture::Fixture::new(false);
    let path = f.native.join("bin");
    let result = f.command(json!({"api":"configure","path":path,"name":"JavaSE-21"}));
    let notices = f.notices(&result, "window/showMessage");
    assert_eq!(1, notices.len(), "{notices:#?}");
    assert_eq!(1, notices[0]["type"]);
    assert_eq!(
        format!(
            "Invalid runtime for JavaSE-21: 'bin' should be removed from the path ({}).",
            path.display()
        ),
        notices[0]["message"]
    );
}
#[test]
fn test_java_runtimes_do_not_leak() {
    let mut f = fixture::Fixture::new(false);
    f.ws.import_projects(&["maven/salut-java11"]);
    let root = f.ws.project_root("salut-java11");
    f.setup();
    f.ws.assert_is_java_project(&root);
    assert_eq!("11", f.ws.java_option(&root, project::SOURCE));
    let result =
        f.ws.request("workspace/symbol", json!({"query":"java.lang.Object"}));
    let objects = result
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["containerName"] == "java.lang" && s["name"] == "Object")
        .count();
    assert_eq!(1, objects);
}
