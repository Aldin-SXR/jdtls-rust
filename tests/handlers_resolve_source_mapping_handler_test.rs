//! Port of `org.eclipse.jdt.ls.core.internal.handlers.ResolveSourceMappingHandlerTest`.
//!
//! `ResolveSourceMappingHandler.resolveStackTraceLocation(line, projects)` is
//! the `java.project.resolveStackTraceLocation` delegate command.

mod common;

use common::jdtls::*;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/quickstart2"]);
    ws
}

fn resolve_stack_trace_location(ws: &mut Workspace, line: &str, project_names: Option<&[&str]>) -> String {
    let mut arguments: Vec<Value> = vec![json!(line)];
    if let Some(names) = project_names {
        arguments.push(json!(names));
    }
    let result = ws.request("workspace/executeCommand", json!({ "command": "java.project.resolveStackTraceLocation", "arguments": arguments }));
    result.as_str().unwrap_or_default().to_owned()
}

#[test]
fn test_resolve_source_uri() {
    let mut ws = setup();
    let uri = resolve_stack_trace_location(&mut ws, "at quickstart.AppTest.shouldAnswerWithTrue(AppTest.java:10)", Some(&["quickstart2"]));
    assert!(uri.starts_with("file://"));
    assert!(uri.contains("quickstart2/src/test/java/quickstart/AppTest.java"));
}

#[test]
fn test_resolve_kotlin_derived_sources() {
    let mut ws = setup();
    let uri = resolve_stack_trace_location(&mut ws, "at okhttp3.OkHttpClient.<init>(OkHttpClient.kt)", Some(&["quickstart2"]));
    assert!(uri.starts_with("jdt://contents/okhttp-jvm-5.3.2.jar/okhttp3/OkHttpClient.kt"), "Unexpected URI: {uri}");
    assert!(uri.contains("com%5C/squareup%5C/okhttp3%5C/okhttp-jvm%5C/5.3.2%5C/okhttp-jvm-5.3.2.jar"));
}

#[test]
fn test_resolve_scala_derived_sources() {
    let mut ws = setup();
    let uri = resolve_stack_trace_location(&mut ws, "at akka.actor.Actor.$init$(Actor.scala:492)", Some(&["quickstart2"]));
    assert!(uri.starts_with("jdt://contents/akka-actor_2.13-2.8.8.jar/akka.actor/Actor.scala"), "Unexpected URI: {uri}");
    assert!(uri.contains("com%5C/typesafe%5C/akka%5C/akka-actor_2.13%5C/2.8.8%5C/akka-actor_2.13-2.8.8.jar"));
}

#[test]
fn test_resolve_dependency_uri() {
    let mut ws = setup();
    let uri = resolve_stack_trace_location(&mut ws, "at org.junit.Assert.assertEquals(Assert.java:117)", Some(&["quickstart2"]));
    assert!(uri.starts_with("jdt://contents/junit-4.13.jar/org.junit/Assert.java"));
    assert!(uri.contains(
        "junit%5C/junit%5C/4.13%5C/junit-4.13.jar=/maven.pomderived=/true=/=/test=/true=/=/maven.groupId=/junit=/=/maven.artifactId=/junit=/=/maven.version=/4.13=/=/maven.scope=/test=/=/maven.pomderived=/true=/%3Corg.junit%28Assert.class"
    ));
}

#[test]
fn test_resolve_dependency_uri_without_giving_project_names() {
    let mut ws = setup();
    let uri = resolve_stack_trace_location(&mut ws, "at org.junit.Assert.assertEquals(Assert.java:117)", None);
    assert!(uri.starts_with("jdt://contents/junit-4.13.jar/org.junit/Assert.java"));
    assert!(uri.contains(
        "junit%5C/4.13%5C/junit-4.13.jar=/maven.pomderived=/true=/=/test=/true=/=/maven.groupId=/junit=/=/maven.artifactId=/junit=/=/maven.version=/4.13=/=/maven.scope=/test=/=/maven.pomderived=/true=/%3Corg.junit%28Assert.class"
    ));
}
