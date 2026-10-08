//! Port of `org.eclipse.jdt.ls.core.internal.handlers.DiagnosticHandlerTest`.
//!
//! The reconciled problems of a working copy, converted by
//! `DiagnosticsHandler.toDiagnosticsArray(cu, problems, true)`, are the
//! diagnostics the server publishes for the open document; the client
//! declares diagnostic tag support (the `true` argument).

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const COMPILER_PB_DEAD_CODE: &str = "org.eclipse.jdt.core.compiler.problem.deadCode";
const COMPILER_PB_UNUSED_LAMBDA_PARAMETER: &str = "org.eclipse.jdt.core.compiler.problem.unusedLambdaParameter";

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.capabilities["textDocument"]["publishDiagnostics"] = json!({ "tagSupport": { "valueSet": [1, 2] } });
    ws
}

/// `pack1.createCompilationUnit(name, contents)` in `newEmptyProject()`,
/// then its working copy's problems.
fn diagnostics_of(ws: &mut Workspace, options: &BTreeMap<String, String>, name: &str, contents: &str) -> Vec<Value> {
    let root = ws.new_empty_project(options);
    let uri = ws.create_cu(&root, "src", "test1", name, contents);
    ws.open(&uri);
    ws.diagnostics(&uri)
}

#[test]
fn test_multiple_line_range() {
    let mut ws = workspace();
    let mut options = BTreeMap::new();
    options.insert(COMPILER_PB_DEAD_CODE.to_owned(), "warning".to_owned());
    let buf = concat!(
        "package test1;\n",
        "public class E {\n",
        "    public boolean foo(boolean b1) {\n",
        "        if (false) {\n",
        "            return true;\n",
        "        }\n",
        "        return false;\n",
        "    }\n",
        "}\n",
    );
    let diagnostics = diagnostics_of(&mut ws, &options, "E.java", buf);
    assert_eq!(1, diagnostics.len());
    let range = &diagnostics[0]["range"];
    assert_ne!(range["start"]["line"], range["end"]["line"]);
}

#[test]
fn test_task() {
    let mut ws = workspace();
    let buf = concat!("package test1;\n", "public class E {\n", "    // TODO task\n", "}\n");
    let diagnostics = diagnostics_of(&mut ws, &BTreeMap::new(), "E.java", buf);
    assert_eq!(diagnostics.len(), 1);
    // DiagnosticSeverity.Information
    assert_eq!(diagnostics[0]["severity"], 3);
}

#[test]
fn test_not_used() {
    let mut ws = workspace();
    let buf = concat!("package test1;\n", "public class E {\n", "    private int i;\n", "}\n");
    let diagnostics = diagnostics_of(&mut ws, &BTreeMap::new(), "E.java", buf);
    assert_eq!(diagnostics.len(), 1);
    // DiagnosticSeverity.Warning
    assert_eq!(diagnostics[0]["severity"], 2);
}

#[test]
fn test_deprecated() {
    let mut ws = workspace();
    let buf = concat!(
        "package test1;\n",
        "import java.security.Certificate;\n",
        "public interface E extends Certificate {}\n",
    );
    let diagnostics = diagnostics_of(&mut ws, &BTreeMap::new(), "E.java", buf);
    assert_eq!(1, diagnostics.len());
    let tags = diagnostics[0]["tags"].as_array().unwrap();
    assert_eq!(1, tags.len());
    // DiagnosticTag.Deprecated
    assert_eq!(2, tags[0]);
}

#[test]
fn test_unnecessary() {
    let mut ws = workspace();
    let buf = concat!("package test1;\n", "import java.security.*;\n");
    let diagnostics = diagnostics_of(&mut ws, &BTreeMap::new(), "E.java", buf);
    assert_eq!(1, diagnostics.len());
    let tags = diagnostics[0]["tags"].as_array().unwrap();
    assert_eq!(1, tags.len());
    // DiagnosticTag.Unnecessary
    assert_eq!(1, tags[0]);
}

// test regression https://github.com/eclipse/eclipse.jdt.ls/issues/1781
#[test]
fn test_static_reference() {
    let mut ws = workspace();
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.class_uri("hello", "org.sample.HelloWorld");
    // The markers of the unit: what the build published for it, if anything.
    let reports = ws.published_diagnostics_min(1);
    let markers: Vec<&Value> = reports
        .iter()
        .filter(|r| r["uri"] == uri.as_str())
        .flat_map(|r| r["diagnostics"].as_array().unwrap())
        .collect();
    assert_eq!(0, markers.len(), "{markers:?}");
}

#[test]
fn test_unused_lambda_parameter_warning_disabled_by_default() {
    let mut ws = workspace();
    let contents = r#"package test1;
import java.util.Map;
public class LambdaWarning {
    void foo() {
        Map.of("foo", "bar").forEach((key, value) -> {
            System.out.println(key);
        });
    }
}
"#;
    let root = ws.new_empty_project(&BTreeMap::new());
    let uri = ws.create_cu(&root, "src", "test1", "LambdaWarning.java", contents);
    let project_uri = url::Url::from_file_path(&root).unwrap().to_string();
    let settings = ws.request(
        "workspace/executeCommand",
        json!({ "command": "java.project.getSettings", "arguments": [project_uri, [COMPILER_PB_UNUSED_LAMBDA_PARAMETER]] }),
    );
    assert_eq!(
        "ignore", settings[COMPILER_PB_UNUSED_LAMBDA_PARAMETER],
        "Unused lambda parameter warning should be disabled by default"
    );

    ws.open(&uri);
    for problem in ws.diagnostics(&uri) {
        let message = problem["message"].as_str().unwrap().to_lowercase();
        assert!(
            !(message.contains("lambda parameter") && message.contains("not used")),
            "Should not have unused lambda parameter warning: {message}"
        );
    }
}
