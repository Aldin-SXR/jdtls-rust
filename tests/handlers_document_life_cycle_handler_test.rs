//! Port of `org.eclipse.jdt.ls.core.internal.handlers.DocumentLifeCycleHandlerTest`.
//!
//! The upstream test drives `DocumentLifeCycleHandler` directly and counts the
//! `publishDiagnostics` calls it makes.  Here the same steps go over LSP
//! (`didOpen`/`didChange`/`didSave`/`didClose`) and each step compares the
//! document reports the server published afterwards
//! ([`Workspace::published_diagnostics`]).  In the real server a save also
//! runs the auto-build, whose marker changes re-validate open documents
//! (`WorkspaceDiagnosticsHandler` → `triggerValidation`), so a save can
//! publish a report where the isolated handler published none; those
//! expectations follow the real jdt.ls (confirmed with `JDTLS_ORACLE=1`) and
//! are marked "server:" below.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use tower_lsp::lsp_types::Url;

/// `setup()`: the upstream test mocks `Preferences`, so
/// `isValidateAllOpenBuffersOnChanges()` is `false` (Mockito's default).
fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "edit": { "validateAllOpenBuffersOnChanges": false } } });
    ws
}

/// `ExpectedProblemReport(cu, problemCount)`.
struct Report<'a>(&'a str, usize);

/// `assertNewProblemReported(expectedReports...)`.
#[track_caller]
fn assert_new_problem_reported(ws: &mut Workspace, expected: &[Report]) {
    let diags = ws.published_diagnostics_min(expected.len());
    assert_eq!(expected.len(), diags.len(), "unexpected reports: {diags:#?}");
    for Report(uri, count) in expected {
        let filtered: Vec<&Value> = diags.iter().filter(|d| d["uri"] == *uri).collect();
        assert_eq!(1, filtered.len(), "reports for {uri}: {diags:#?}");
        let list = filtered[0]["diagnostics"].as_array().unwrap();
        let messages: Vec<&str> = list.iter().filter_map(|d| d["message"].as_str()).collect();
        assert_eq!(*count, list.len(), "{}", messages.join(", "));
    }
}

fn messages(report: &Value) -> Vec<String> {
    report["diagnostics"].as_array().unwrap().iter().map(|d| d["message"].as_str().unwrap().to_owned()).collect()
}

/// `Lsp4jAssertions.assertRange(line, start, end, range)`.
fn assert_range(line: u32, start: u32, end: u32, d: &Value) {
    assert_eq!(range(line, start, line, end), d["range"], "{d}");
}

/// `JDTUtils.toRange(cu, 0, source.length())`: the whole document.
fn whole(text: &str) -> Value {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().unwrap();
    range(0, 0, lines.len() as u32 - 1, last.encode_utf16().count() as u32)
}

fn path_of(uri: &str) -> PathBuf {
    Url::parse(uri).unwrap().to_file_path().unwrap()
}

fn file_uri(path: &std::path::Path) -> String {
    Url::from_file_path(path).unwrap().to_string()
}

/// Code actions limited to quick fixes for the first problem of `uri`
/// (`getCodeActions(cu)`): the range is the problem start, the context holds
/// all problems.
fn get_code_actions(ws: &mut Workspace, uri: &str, diagnostics: &[Value]) -> Vec<Value> {
    let start = diagnostics[0]["range"]["start"].clone();
    let result = ws.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri },
            "range": { "start": start, "end": start },
            "context": { "diagnostics": diagnostics, "only": ["quickfix"] },
        }),
    );
    result.as_array().cloned().unwrap_or_default()
}

#[test]
fn test_unimplemented_methods() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic interface E {\n    void foo();\n}\n");
    let src = "package test1;\npublic class F implements E {\n}\n";
    let uri = ws.create_cu(&root, "src", "test1", "F.java", src);
    ws.published_diagnostics();
    ws.open_with(&uri, src);
    let reports = ws.published_diagnostics_min(1);
    let diagnostics = reports.iter().find(|r| r["uri"] == uri).unwrap()["diagnostics"].as_array().cloned().unwrap();

    let code_actions = get_code_actions(&mut ws, &uri, &diagnostics);
    assert_eq!(code_actions.len(), 1, "{code_actions:#?}");
    assert_eq!(code_actions[0]["kind"], "quickfix");
}

#[test]
fn test_remove_dead_code_after_if() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src = "package test1;\npublic class E {\n    public boolean foo(boolean b1) {\n        if (false) {\n            return true;\n        }\n        return false;\n    }\n}\n";
    let uri = ws.create_cu(&root, "src", "test1", "E.java", src);
    ws.published_diagnostics();
    ws.open_with(&uri, src);
    let reports = ws.published_diagnostics_min(1);
    let diagnostics = reports.iter().find(|r| r["uri"] == uri).unwrap()["diagnostics"].as_array().cloned().unwrap();

    let code_actions = get_code_actions(&mut ws, &uri, &diagnostics);
    let has_code_action = code_actions.iter().any(|a| a["title"] == "Remove (including condition)");
    assert!(has_code_action, "Code action to remove dead code not found: {code_actions:#?}");
}

#[test]
fn test_basic_buffer_life_cycle() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src = "package test1;\npublic class E123 {\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E123.java", src);

    assert_new_problem_reported(&mut ws, &[]);

    ws.open_with(&cu1, src);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);

    let changed = "package test1;\npublic class E123 {\n  X x;\n}\n";
    ws.change(&cu1, changed);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    std::fs::write(path_of(&cu1), changed).unwrap();
    ws.save(&cu1, Some(changed));
    // server: the auto-build's new marker re-validates the open document.
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    ws.close(&cu1);
    assert_new_problem_reported(&mut ws, &[]);
}

#[test]
fn test_basic_buffer_life_cycle_without_save() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src = "package test1;\npublic class E123 {\n    public boolean foo() {\n        return x;\n    }\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E123.java", src);
    ws.published_diagnostics();

    ws.open_with(&cu1, src);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    ws.change(&cu1, "package test1;\npublic class E123 {\n    public boolean foo() {\n        return true;\n    }\n}\n");
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);

    ws.close(&cu1);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);
}

#[test]
fn test_reconcile() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src = "package test1;\npublic class E123 {\n    public void testing() {\n        int someIntegerChanged = 5;\n        int i = someInteger + 5\n    }\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E123.java", src);
    ws.published_diagnostics();
    ws.open_with(&cu1, src);
    let diagnostics_params = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostics_params.len(), "{diagnostics_params:#?}");
    let diagnostics = diagnostics_params[0]["diagnostics"].as_array().unwrap();
    assert_eq!(2, diagnostics.len(), "{diagnostics:#?}");
    ws.close(&cu1);
}

/// The document's working copy is shared while it is open (requests see the
/// buffer) and discarded on close (requests see the file again).
#[test]
fn test_working_copies() {
    let mut ws = workspace();
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.class_uri("hello", "org.sample.Foo");
    let source = std::fs::read_to_string(path_of(&uri)).unwrap();
    let buffer = format!("{source}\nclass WorkingCopyOnly {{}}\n");
    let symbols = |ws: &mut Workspace| -> Vec<String> {
        let r = ws.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
        r.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().to_owned()).collect()
    };
    ws.open_with(&uri, &buffer);
    let cu1 = symbols(&mut ws);
    let cu2 = symbols(&mut ws);
    assert_eq!(cu1, cu2);
    assert!(cu1.contains(&"WorkingCopyOnly".to_owned()), "{cu1:?}");
    ws.close(&uri);
    let cu1 = symbols(&mut ws);
    assert!(!cu1.contains(&"WorkingCopyOnly".to_owned()), "{cu1:?}");
}


#[test]
fn test_incremental_change_document() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let begin_part = "package test1;\n";
    let to_be_changed_part = "public class E123 {\n";
    let end_part = "}\n";
    let src = format!("{begin_part}{to_be_changed_part}{end_part}");
    let cu1 = ws.create_cu(&root, "src", "test1", "E123.java", &src);

    assert_new_problem_reported(&mut ws, &[]);

    ws.open_with(&cu1, &src);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);

    // JDTUtils.toRange(cu, BEGIN_PART.length(), TO_BE_CHANGED_PART.length())
    let text = format!("{to_be_changed_part}  X x;\n");
    ws.change_range(&cu1, range(1, 0, 2, 0), &text);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    let saved = format!("{begin_part}{text}{end_part}");
    std::fs::write(path_of(&cu1), &saved).unwrap();
    ws.save(&cu1, Some(&saved));
    // server: the auto-build's new marker re-validates the open document.
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    ws.close(&cu1);
    assert_new_problem_reported(&mut ws, &[]);
}

#[test]
fn test_fix_in_dependency_scenario() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src1 = "package test1;\npublic class F123 {\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "F123.java", src1);
    let src2 = "package test1;\npublic class F456 {\n  { F123.foo(); }\n}\n";
    let cu2 = ws.create_cu(&root, "src", "test1", "F456.java", src2);

    // server: the initial build reports F456's marker.
    assert_new_problem_reported(&mut ws, &[Report(&cu2, 1)]);

    ws.open_with(&cu2, src2);
    assert_new_problem_reported(&mut ws, &[Report(&cu2, 1)]);

    ws.open_with(&cu1, src1);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);

    let changed = "package test1;\npublic class F123 {\n  public static void foo() {}\n}\n";
    ws.change(&cu1, changed);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);

    ws.close(&cu2);
    ws.open_with(&cu2, src2);
    assert_new_problem_reported(&mut ws, &[Report(&cu2, 0)]);

    std::fs::write(path_of(&cu1), changed).unwrap();
    ws.save(&cu1, Some(changed));
    // server: the build removes F456's marker, which re-validates it.
    assert_new_problem_reported(&mut ws, &[Report(&cu2, 0)]);

    ws.close(&cu1);
    assert_new_problem_reported(&mut ws, &[]);

    ws.close(&cu2);
    assert_new_problem_reported(&mut ws, &[]);
}

/// A file of the default project (outside every project): syntax errors only.
#[test]
fn test_did_open_standalone_file() {
    let mut ws = workspace();
    let content = "package java;\npublic class Foo extends UnknownType {\tpublic void method1(){\n\t\tsuper.whatever();\t}\n}";
    let dir = ws.external_dir().join("java");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Foo.java"), content).unwrap();
    let cu1 = file_uri(&dir.join("Foo.java"));
    ws.published_diagnostics();

    ws.open_with(&cu1, content);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len(), "{diagnostic_reports:#?}");
    let diag_param = &diagnostic_reports[0];
    assert_eq!(1, diag_param["diagnostics"].as_array().unwrap().len(), "{diag_param:#?}");
    let d = &diag_param["diagnostics"][0];
    assert_eq!("Foo.java is a non-project file, only syntax errors are reported", d["message"]);
}

#[test]
fn test_did_open_not_on_classpath() {
    let mut ws = workspace();
    ws.import_projects(&["eclipse/hello"]);
    let uri = file_uri(&ws.project_root("hello").join("nopackage/Test2.java"));
    let source = std::fs::read_to_string(path_of(&uri)).unwrap();
    ws.published_diagnostics();
    ws.open_with(&uri, &source);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len(), "{diagnostic_reports:#?}");
    assert_eq!(2, diagnostic_reports[0]["diagnostics"].as_array().unwrap().len(), "{diagnostic_reports:#?}");
    ws.close(&uri);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len(), "{diagnostic_reports:#?}");
    assert_eq!(0, diagnostic_reports[0]["diagnostics"].as_array().unwrap().len());
}

#[test]
fn test_did_open_standalone_file_with_syntax_error() {
    let mut ws = workspace();
    let content = "package java;\npublic class Foo extends UnknownType {\n\tpublic void method1(){\n\t\tsuper.whatever()\n\t}\n}";
    let dir = ws.external_dir().join("java");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Foo.java"), content).unwrap();
    let cu1 = file_uri(&dir.join("Foo.java"));
    ws.published_diagnostics();

    ws.open_with(&cu1, content);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len());
    let diag_param = &diagnostic_reports[0];
    assert_eq!(2, diag_param["diagnostics"].as_array().unwrap().len(), "Unexpected number of errors {diag_param}");
    let d = &diag_param["diagnostics"][0];
    assert_eq!("Foo.java is a non-project file, only syntax errors are reported", d["message"]);
    assert_range(0, 0, 1, d);
    let d = &diag_param["diagnostics"][1];
    assert_eq!("Syntax error, insert \";\" to complete BlockStatements", d["message"]);
    assert_range(3, 17, 18, d);
}

#[test]
fn test_did_open_standalone_file_with_non_syntax_errors() {
    let mut ws = workspace();
    let content = "package java;\npublic class Foo {\n\tpublic static void notThis(){\n\t\tSystem.out.println(this);\n\t}\n\tpublic void method1(){\n\t}\n\tpublic void method1(){\n\t}\n}";
    let dir = ws.external_dir().join("java");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Foo.java"), content).unwrap();
    let cu1 = file_uri(&dir.join("Foo.java"));
    ws.published_diagnostics();

    ws.open_with(&cu1, content);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len());
    let diag_param = &diagnostic_reports[0];

    assert_eq!(4, diag_param["diagnostics"].as_array().unwrap().len(), "Unexpected number of errors {diag_param}");
    let d = &diag_param["diagnostics"][0];
    assert_eq!("Foo.java is a non-project file, only syntax errors are reported", d["message"]);
    assert_range(0, 0, 1, d);

    let d = &diag_param["diagnostics"][1];
    assert_eq!("Cannot use this in a static context", d["message"]);
    assert_range(3, 21, 25, d);

    let d = &diag_param["diagnostics"][2];
    assert_eq!("Duplicate method method1() in type Foo", d["message"]);
    assert_range(5, 13, 22, d);

    let d = &diag_param["diagnostics"][3];
    assert_eq!("Duplicate method method1() in type Foo", d["message"]);
    assert_range(7, 13, 22, d);
}

/// Opening a file under a root folder that has no project yet creates its
/// invisible project.  The folder gets its sources after initialization, so
/// the invisible project importer didn't apply at startup.
#[test]
fn test_did_open_lazy_loading_invisible_project() {
    let mut ws = workspace();
    let standalone_folder = ws.dir.join("singlefile/lesson1");
    std::fs::create_dir_all(&standalone_folder).unwrap();
    ws.init_options["workspaceFolders"] = json!([file_uri(&standalone_folder)]);
    ws.client();
    copy_dir(&fixtures_dir().join("projects/singlefile/lesson1"), &standalone_folder);
    let trigger_file = standalone_folder.join("src/org/samples/HelloWorld.java");
    let file_uri = file_uri(&trigger_file);
    let project_uri = format!("file:{}/", standalone_folder.display());

    let all = ws.request("workspace/executeCommand", json!({ "command": "java.project.getAll", "arguments": [] }));
    assert!(!all.as_array().unwrap().contains(&json!(project_uri)), "{all}");

    let text = std::fs::read_to_string(&trigger_file).unwrap();
    ws.open_with(&file_uri, &text);
    ws.published_diagnostics();

    let all = ws.request("workspace/executeCommand", json!({ "command": "java.project.getAll", "arguments": [] }));
    assert!(all.as_array().unwrap().contains(&json!(project_uri)), "{all}");
}

#[test]
fn test_not_expected_package() {
    let mut ws = workspace();
    let content = "package org;\npublic class Foo {}";
    let temp = ws.external_dir().join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let file = temp.join("Foo.java");
    std::fs::write(&file, content).unwrap();
    let uri = file_uri(&file);
    ws.published_diagnostics();
    ws.open_with(&uri, content);
    let reports = ws.published_diagnostics_min(1);
    assert_eq!(vec!["Foo.java is a non-project file, only syntax errors are reported"], messages(&reports[0]), "Unexpected number of errors");

    let source = content.replace("org", "org.eclipse");
    ws.change_range(&uri, whole(content), &source);
    std::fs::write(&file, &source).unwrap();
    ws.save(&uri, Some(&source));
    let reports = ws.published_diagnostics_min(1);
    let last = reports.iter().rev().find(|r| r["uri"] == uri).unwrap();
    assert_eq!(vec!["Foo.java is a non-project file, only syntax errors are reported"], messages(last), "Unexpected number of errors");
}

#[test]
fn test_create_compilation_unit() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let foo_content = "package org;\npublic class Foo {}\n";
    let bar_content = "package org;\npublic class Bar {\n  Foo test() { return null; }\n}\n";
    ws.client();
    let org = root.join("src/org");
    std::fs::create_dir_all(&org).unwrap();
    let bar = org.join("Bar.java");
    std::fs::write(&bar, bar_content).unwrap();
    ws.notify_file_changed(&bar, 1);
    let file = org.join("Foo.java");
    std::fs::write(&file, "").unwrap();
    let uri = file_uri(&file);
    ws.open_version(&uri, "", 1);
    std::fs::write(&file, foo_content).unwrap();
    ws.change_version(&uri, foo_content, 1);
    ws.save(&uri, Some(foo_content));
    ws.close(&uri);
    ws.published_diagnostics();
    let bar_uri = file_uri(&bar);
    ws.open_with(&bar_uri, bar_content);
    let reports = ws.published_diagnostics_min(1);
    let problems = &reports.iter().find(|r| r["uri"] == bar_uri).unwrap()["diagnostics"];
    assert_eq!(0, problems.as_array().unwrap().len(), "Unexpected number of errors {problems:#?}");
}

#[test]
fn test_not_expected_package2() {
    let mut ws = workspace();
    let content = "package org;\npublic class Foo {}";
    let dir = ws.external_dir().join("temp/org/eclipse");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Foo.java");
    std::fs::write(&file, content).unwrap();
    let uri = file_uri(&file);
    ws.published_diagnostics();
    ws.open_with(&uri, content);
    let reports = ws.published_diagnostics_min(1);
    assert_eq!(vec!["Foo.java is a non-project file, only syntax errors are reported"], messages(&reports[0]), "Unexpected number of errors");

    let source = content.replace("org", "org.eclipse");
    ws.change_range(&uri, whole(content), &source);
    std::fs::write(&file, &source).unwrap();
    ws.save(&uri, Some(&source));
    let reports = ws.published_diagnostics_min(1);
    let last = reports.iter().rev().find(|r| r["uri"] == uri).unwrap();
    assert_eq!(vec!["Foo.java is a non-project file, only syntax errors are reported"], messages(last), "Unexpected number of errors");

    let source2 = source.replace("org.eclipse", "org.eclipse.toto");
    ws.change_range(&uri, whole(&source), &source2);
    std::fs::write(&file, &source2).unwrap();
    ws.save(&uri, Some(&source2));
    let reports = ws.published_diagnostics_min(1);
    let last = reports.iter().rev().find(|r| r["uri"] == uri).unwrap();
    assert_eq!(2, messages(last).len(), "Unexpected number of errors {last:#?}");
}

#[test]
fn test_close_missing_resource() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    let src = "package test1;\npublic class E123 {\n    public boolean foo() {\n        return x;\n    }\n}\n";
    let cu1 = ws.create_cu(&root, "src", "test1", "E123.java", src);
    ws.published_diagnostics();

    ws.open_with(&cu1, src);
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 1)]);

    ws.change(&cu1, "package test1;\npublic class E123 {\n    public boolean foo() {\n        return true;\n    }\n}\n");
    let file = path_of(&cu1);
    std::fs::remove_file(&file).unwrap();
    ws.close(&cu1);
    // server: the change publishes before the close clears the report.
    assert_new_problem_reported(&mut ws, &[Report(&cu1, 0)]);
}



#[test]
fn test_file_rename_for_type_declaration() {
    let mut ws = workspace();
    let root = ws.new_empty_project(&Default::default());
    // jdt.ls renames the file on save when the `renameFileToType` clean-up is
    // enabled (`JDTLanguageServer.didSave`).
    ws.settings["java"]["cleanup"] = json!({ "actions": ["renameFileToType"] });
    ws.settings["java"]["saveActions"] = json!({ "cleanup": true });
    ws.client();
    let step = |ws: &mut Workspace, content: &str, expected: usize| {
        let uri = ws.create_cu(&root, "src", "test1", "Other.java", content);
        ws.open_with(&uri, content);
        ws.save(&uri, Some(content));
        ws.published_diagnostics();
        let edits = ws.client().take_server_requests("workspace/applyEdit");
        assert_eq!(expected, edits.len(), "{edits:#?}");
        ws.close(&uri);
        ws.published_diagnostics();
        edits
    };

    step(&mut ws, "package test1;\n\nclass Foo {\n}", 0);

    // RenameFile operation through 'workspace/applyEdit'
    let edits = step(&mut ws, "package test1;\n\npublic class Foo {\n}", 1);
    let uri = file_uri(&root.join("src/test1/Other.java"));
    let new_uri = file_uri(&root.join("src/test1/Foo.java"));
    assert_eq!(
        json!({ "changes": {}, "documentChanges": [{ "kind": "rename", "oldUri": uri, "newUri": new_uri }] }),
        edits[0]["params"]["edit"]
    );

    step(&mut ws, "package test1;\n\npublic interface Foo {\n}\n\nclass Bar {\n}\n\nclass Biz {\n}", 1);

    step(&mut ws, "public class Foo {\n}\n\npublic class Bar {\n}\n\npublic class Biz {\n}", 0);
}

#[test]
fn test_diagnostics_on_external_file_with_internal_project() {
    let mut ws = workspace();
    ws.init_options["extendedClientCapabilities"]["skipProjectConfiguration"] = json!(true);
    ws.settings["java"]["edit"]["validateAllOpenBuffersOnChanges"] = json!(true);
    let dir = ws.external_dir().join("testDiagnosticsOnExternalFileWithInternalProject");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("A.java");
    let content = "error public class A { }";
    std::fs::write(&file_path, content).unwrap();
    let uri = file_uri(&file_path);
    ws.published_diagnostics();
    ws.open_version(&uri, content, 0);
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert!(!diagnostic_reports.is_empty(), "No diagnostics sent on open");
    let diagnostics = messages(&diagnostic_reports[0]);
    assert!(diagnostics.iter().any(|m| m.contains("Syntax error")), "First diagnostics not sent");
    ws.change_range(&uri, range(0, 0, 0, 0), "another");
    let diagnostic_reports = ws.published_diagnostics_min(1);
    assert_eq!(1, diagnostic_reports.len(), "No diagnostics sent on change");
    let diagnostics = messages(&diagnostic_reports[0]);
    assert!(diagnostics.iter().any(|m| m.contains("Syntax error")), "diagnostics not updated upon edit");
}

// Upstream cases not ported here (no empty tests counted as ports):
// test_non_jdt_error: creates a non-JDT IMarker through the Eclipse resources API; markers from other builders have no LSP equivalent.
// test_document_monitor: DocumentMonitor is an internal API of the lifecycle handler (version check inside a request); no LSP request exposes it deterministically.
// test_document_monitor_closed_document: DocumentMonitor is an internal API of the lifecycle handler (version check inside a request); no LSP request exposes it deterministically.
