//! Port of `org.eclipse.jdt.ls.core.internal.managers.MavenClasspathTest`.
//!
//! The `DiagnosticsHandler` reconcile of a working copy is the diagnostics
//! the server publishes for the opened document.

mod common;

use common::jdtls::*;
use common::maven::*;

fn count_errors(problems: &[serde_json::Value]) -> usize {
    problems.iter().filter(|p| p["severity"] == 1).count()
}

#[test]
fn test_main() {
    let mut ws = workspace();
    import_maven_project(&mut ws, "classpathtest");
    let uri = ws.class_uri("classpathtest", "main.App");
    let source = std::fs::read_to_string(url::Url::parse(&uri).unwrap().to_file_path().unwrap()).unwrap();
    ws.open_with(&uri, &source);
    let problems = ws.diagnostics(&uri);
    assert_eq!(1, problems.len(), "There aren't any problems");
}

#[test]
fn test_test() {
    let mut ws = workspace();
    import_maven_project(&mut ws, "classpathtest");
    let uri = ws.class_uri("classpathtest", "test.AppTest");
    let source = std::fs::read_to_string(url::Url::parse(&uri).unwrap().to_file_path().unwrap()).unwrap();
    ws.open_with(&uri, &source);
    let problems = ws.diagnostics(&uri);
    assert_eq!(0, problems.len(), "There is a problem");
}

#[test]
fn typemismatch_test() {
    let mut ws = workspace();
    import_maven_project(&mut ws, "typemismatch");
    let uri = ws.class_uri("typemismatch", "test.Test");
    let mut source = std::fs::read_to_string(url::Url::parse(&uri).unwrap().to_file_path().unwrap()).unwrap();
    ws.open_with(&uri, &source);
    let problems = ws.diagnostics(&uri);
    assert_eq!(0, count_errors(&problems), "There is an error");
    for (from, to) in [
        ("Test {", "Test { "),
        ("Test { ", "Test {  "),
        ("Test {  ", "Test {   "),
        ("Test {   ", "Test {"),
    ] {
        source = source.replace(from, to);
        ws.change(&uri, &source);
        let problems = ws.diagnostics(&uri);
        assert_eq!(0, count_errors(&problems), "There is an error");
    }
}
