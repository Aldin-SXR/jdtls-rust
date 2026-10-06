//! Original AdvancedOrganizeImportsHandlerTest fixtures, chooser assertions and
//! resulting sources, exercised through java/organizeImports.
mod common;
use common::jdtls::*;
use common::projects::file_uri;
use serde_json::{json, Value};

fn setup() -> (Workspace, std::path::PathBuf) {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    ws.use_upstream_test_jdk("TestProject");
    ws.init_options["extendedClientCapabilities"] = json!({"executeClientCommandSupport": true});
    (ws, root)
}
fn organize(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.open(uri);
    ws.request(
        "java/organizeImports",
        json!({"textDocument": {"uri": uri},
        "range": range(0,0,0,0), "context": {"diagnostics": []}}),
    )
}
fn edited(source: &str, uri: &str, edit: &Value) -> String {
    assert!(!edit.is_null(), "expected import edit");
    apply_edits(source, edit["changes"][uri].as_array().unwrap())
}
fn favorites(ws: &mut Workspace, names: &[&str], threshold: u32) {
    let settings = json!({"java": {"completion": {"favoriteStaticMembers": names},
        "sources": {"organizeImports": {"staticStarThreshold": threshold}}}});
    ws.settings = settings.clone();
    ws.client();
    // Configuration notification updates JDT UI's static-favorite preference
    // store after its initial project preferences have been established.
    ws.update_settings(settings);
    ws.client()
        .request_results
        .insert("workspace/executeClientCommand".into(), json!([]));
}

#[test]
fn test_choose_import() {
    let (mut ws, root) = setup();
    ws.create_cu(
        &root,
        "src",
        "p1",
        "C.java",
        "package p1;\r\n\r\npublic class C {\r\n}",
    );
    ws.create_cu(
        &root,
        "src",
        "p2",
        "C.java",
        "package p2;\r\n\r\npublic class C {\r\n}",
    );
    let source = "package p;\r\n\r\npublic class B {\r\n\tC c;\r\n}";
    let uri = ws.create_cu(&root, "src", "p", "B.java", source);
    ws.client().request_handlers.insert(
        "workspace/executeClientCommand".into(),
        Box::new(|msg| {
            let selections = msg["params"]["arguments"][1].as_array().unwrap();
            assert_eq!(1, selections.len());
            let selection = &selections[0];
            assert_eq!(2, selection["candidates"].as_array().unwrap().len());
            assert_eq!("p1.C", selection["candidates"][0]["fullyQualifiedName"]);
            assert_eq!("p2.C", selection["candidates"][1]["fullyQualifiedName"]);
            assert_eq!(range(3, 1, 3, 2), selection["range"]);
            json!([selection["candidates"][0]])
        }),
    );
    let edit = organize(&mut ws, &uri);
    assert_eq!(
        "package p;\r\n\r\nimport p1.C;\r\n\r\npublic class B {\r\n\tC c;\r\n}",
        edited(source, &uri, &edit)
    );
}

#[test]
fn test_static_imports() {
    let (mut ws, root) = setup();
    let source = "package p1;\n\npublic class C {\n    List list = List.of(1).stream().collect(toList());\n    double i = abs(-1);\n    double pi = PI;\n}\n";
    let uri = ws.create_cu(&root, "src", "p1", "C.java", source);
    favorites(
        &mut ws,
        &["java.lang.Math.*", "java.util.stream.Collectors.*"],
        99,
    );
    let edit = organize(&mut ws, &uri);
    assert_eq!("package p1;\n\nimport static java.lang.Math.PI;\nimport static java.lang.Math.abs;\nimport static java.util.stream.Collectors.toList;\n\nimport java.util.List;\n\npublic class C {\n    List list = List.of(1).stream().collect(toList());\n    double i = abs(-1);\n    double pi = PI;\n}\n", edited(source, &uri, &edit));
}

#[test]
fn test_ambiguous_static_imports() {
    let (mut ws, _) = setup();
    ws.import_projects(&["maven/salut4"]);
    let uri = file_uri(
        &ws.project_root("salut4")
            .join("src/test/java/org/sample/MyTest.java"),
    );
    let source = ws.read(&uri);
    favorites(
        &mut ws,
        &[
            "org.junit.jupiter.api.Assertions.*",
            "org.junit.jupiter.api.Assumptions.*",
            "org.junit.jupiter.api.DynamicContainer.*",
            "org.junit.jupiter.api.DynamicTest.*",
            "org.hamcrest.MatcherAssert.*",
            "org.hamcrest.Matchers.*",
            "org.mockito.Mockito.*",
            "org.mockito.ArgumentMatchers.*",
            "org.mockito.Answers.*",
            "org.mockito.hamcrest.MockitoHamcrest.*",
            "org.mockito.ArgumentMatchers.*",
        ],
        99,
    );
    // MavenBuildSupport.applies delegates to the Maven-project nature check.
    let project = ws.project_root("salut4");
    ws.assert_is_maven_project(&project);
    let edit = organize(&mut ws, &uri);
    assert_eq!("package org.sample;\n\nimport static org.hamcrest.MatcherAssert.assertThat;\nimport static org.hamcrest.Matchers.any;\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nimport org.junit.jupiter.api.Test;\n\npublic class MyTest {\n    @Test\n    public void test() {\n        assertEquals(1, 1, \"message\");\n        assertThat(\"test\", true);\n        any();\n    }\n}\n", edited(&source, &uri, &edit));
}

#[test]
fn test_duplicate_static_imports() {
    let (mut ws, _) = setup();
    ws.import_projects(&["maven/salut6"]);
    let root = ws.project_root("salut6");
    let uri = file_uri(&root.join("src/test/java/org/sample/MyTest.java"));
    let source = ws.read(&uri);
    assert_eq!(
        2,
        source.lines().filter(|l| l.starts_with("import ")).count()
    );
    favorites(&mut ws, &["org.assertj.core.api.Assertions.*"], 99);
    ws.assert_is_maven_project(&root);
    assert!(organize(&mut ws, &uri).is_null());
    let uri = file_uri(&root.join("src/test/java/org/sample/MyTest2.java"));
    let source = ws.read(&uri);
    let edit = organize(&mut ws, &uri);
    let result = edited(&source, &uri, &edit);
    assert_eq!(
        2,
        result.lines().filter(|l| l.starts_with("import ")).count()
    );
    assert!(result
        .lines()
        .any(|l| l == "import static org.hamcrest.MatcherAssert.assertThat;"));
}

#[test]
fn test_remove_static_imports() {
    let (mut ws, _) = setup();
    ws.import_projects(&["maven/salut4"]);
    let uri = file_uri(
        &ws.project_root("salut4")
            .join("src/test/java/org/sample/Test1.java"),
    );
    assert_eq!(
        2,
        ws.read(&uri)
            .lines()
            .filter(|l| l.starts_with("import "))
            .count()
    );
    favorites(&mut ws, &["org.sample.Test2.*"], 1);
    let project = ws.project_root("salut4");
    ws.assert_is_maven_project(&project);
    assert!(organize(&mut ws, &uri).is_null());
}
