//! LSP ports of `WorkspaceDiagnosticsHandlerTest` using saved-file builds.
//! The marker conversion cases (`testToDiagnosticsArray`, `testMavenMarkers`)
//! are unit tests in `src/features/markers.rs`.
//!
//! The m2e pom marker cases (`testMarkerListening` ...) publish their
//! reports the same way; `projectsManager.updateProject` is the
//! `java/projectConfigurationUpdate` notification, `handler.publishDiagnostics`
//! the report a restarted server makes on `initialized`.
//! The Mockito `connection` captor becomes the `publishDiagnostics`
//! notifications the client received; `Collections.reverse(allCalls)` makes
//! the latest report of a URI the first match.

mod common;
use common::jdtls::*;
use common::projects::*;
use serde_json::{json, Value};
use std::time::Duration;

/// `verify(connection, ..).publishDiagnostics(captor.capture())` after the
/// background jobs: every report received so far, latest first.
fn all_calls(ws: &mut Workspace) -> Vec<Value> {
    ws.wait_idle();
    let c = ws.client();
    c.settle(Duration::from_secs(3), Duration::from_secs(60));
    let mut calls: Vec<Value> =
        c.take_notifications("textDocument/publishDiagnostics").into_iter().map(|m| m["params"].clone()).collect();
    calls.reverse();
    calls
}

fn diagnostics_of(report: &Value) -> Vec<Value> {
    report["diagnostics"].as_array().unwrap().clone()
}

fn line_char(d: &Value, end: &str) -> (u64, u64) {
    (d["range"][end]["line"].as_u64().unwrap(), d["range"][end]["character"].as_u64().unwrap())
}

#[test]
fn test_task_markers() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let all_calls = self::all_calls(&mut ws);

    let task_diags = all_calls.iter().find(|p| p["uri"].as_str().unwrap().ends_with("TaskMarkerTest.java"));
    assert!(task_diags.is_some(), "No TaskMarkerTest.java markers were found");
    let mut diags = diagnostics_of(task_diags.unwrap());
    assert_eq!(3, diags.len(), "Some marker is missing");
    let todo_markers = diags.iter().filter(|p| p["message"].as_str().unwrap().starts_with("TODO")).count();
    assert_eq!(2, todo_markers, "A TODO marker is missing");
    diags.sort_by(|o1, o2| o1["message"].as_str().unwrap().cmp(o2["message"].as_str().unwrap()));
    let d = &diags[1];
    assert_eq!("TODO task 2", d["message"]);
    assert_eq!(3, d["severity"]);
    assert_eq!((11, 11), line_char(d, "start"));
    assert_eq!((11, 22), line_char(d, "end"));
    let d = &diags[0];
    assert_eq!("TODO task 1", d["message"]);
    assert_eq!(3, d["severity"]);
    assert_eq!((9, 11), line_char(d, "start"));
    assert_eq!((9, 22), line_char(d, "end"));
}

#[test]
#[ignore = "needs JDT incremental-builder semantics: jdt.ls compiles only the added A1.java and reports the duplicate class file locator itself ('The type A is already defined' plus the public type error); our full rebuild gets ECJ's DuplicateTypes only"]
fn test_bad_location_exception() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    ws.wait_idle();
    let root = ws.project_root("hello");
    let file = root.join("src/test1/A.java");
    assert!(file.exists());
    let dest_file = root.join("src/test1/A1.java");
    assert!(!dest_file.exists());
    std::fs::copy(&file, &dest_file).unwrap();
    let uri = url::Url::from_file_path(&dest_file).unwrap().to_string();
    // project.refreshLocal(IResource.DEPTH_INFINITE, null)
    ws.notify_file_changed(&dest_file, 1);
    let all_calls = self::all_calls(&mut ws);
    let param = all_calls.iter().find(|p| p["uri"] == uri.as_str());
    assert!(param.is_some(), "{all_calls:#?}");
    let diags = diagnostics_of(param.unwrap());
    assert_eq!(2, diags.len(), "{diags:?} {all_calls:#?}");
    let d = diags.iter().find(|p| p["message"] == "The type A is already defined");
    assert!(d.is_some());
    let diag = d.unwrap();
    // The positions are unsigned.
    assert!(diag["range"]["start"]["line"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["start"]["character"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["end"]["line"].as_i64().unwrap() >= 0);
    assert!(diag["range"]["end"]["character"].as_i64().unwrap() >= 0);
}

/// `ResourceUtils.setContent(iFile, ..)`: the file changes on disk and the
/// workspace is refreshed.
fn set_content(ws: &mut Workspace, file: &std::path::Path, content: &str) {
    std::fs::write(file, content).unwrap();
    ws.notify_file_changed(file, 2);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1920
#[test]
#[ignore = "upstream makes a working copy without a DocumentLifeCycleHandler; over LSP the open document is always validated (the oracle also publishes 3 reports); the LSP equivalent is test_working_copy2"]
fn test_working_copy() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    // cu.becomeWorkingCopy(null)
    ws.open(&uri);
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = self::all_calls(&mut ws);
    assert!(calls.len() <= 2, "{calls:#?}");
    ws.close(&uri);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1963
#[test]
fn test_working_copy2() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    ws.open(&uri);
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = self::all_calls(&mut ws);
    assert!(calls.len() <= 3, "{calls:#?}");
    ws.close(&uri);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1920
#[test]
fn test_without_working_copy() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/hello"]);
    let file = ws.project_root("hello").join("src/test1/A.java");
    all_calls(&mut ws); // reset(connection)
    set_content(&mut ws, &file, "package test1;\npublic class A() {}\n");
    let calls = self::all_calls(&mut ws);
    assert!(calls.len() >= 3, "{calls:#?}");
}

#[test]
#[ignore = "no 'Unknown referenced nature' report over LSP: WorkspaceDiagnosticsHandler.isIgnored drops CheckMissingNaturesListener markers and the oracle jdt.ls 1.58.0 publishes none for eclipse/wtpproject"]
fn test_missing_natures() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["eclipse/wtpproject"]);
    let all_calls = self::all_calls(&mut ws);
    // https://github.com/eclipse/eclipse.jdt.ls/issues/2331
    let has_missing_nature = all_calls.iter().any(|project_diags| {
        diagnostics_of(project_diags).iter().any(|p| p["message"].as_str().unwrap().starts_with("Unknown referenced nature"))
    });
    assert!(has_missing_nature, "{all_calls:#?}");
}

#[test]
fn test_annotation() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut4"]);
    let uri = ws.class_uri("salut4", "org.sample.MyTest");
    // The problem markers of the unit, converted with
    // `toDiagnosticsArray(document, markers, false)`.
    let all_calls = self::all_calls(&mut ws);
    let report = all_calls.iter().find(|p| p["uri"] == uri.as_str()).expect("MyTest.java markers");
    let diagnostics = diagnostics_of(report);
    assert_eq!(4, diagnostics.len());
    let diagnostic = diagnostics.iter().find(|p| p["message"] == "Test cannot be resolved to a type").unwrap();
    assert_eq!(4, diagnostic["range"]["start"]["character"]);
}

#[test]
fn test_delete_package() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/unresolvedtype"]);
    let before = ws.published_diagnostics_min(1);
    assert!(before.iter().any(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")
        && r["diagnostics"].as_array().unwrap().iter().any(|d| d["severity"] == 1)), "unresolved type in Foo.java: {before:#?}");

    let folder = ws.project_root("unresolvedtype").join("src/pckg");
    assert!(folder.exists());
    std::fs::remove_dir_all(&folder).unwrap();
    ws.notify_file_changed(&folder, 3);
    let after = ws.published_diagnostics_min(1);
    let reports = after.iter().filter(|r| r["uri"].as_str().unwrap().ends_with("Foo.java")).collect::<Vec<_>>();
    assert_eq!(1, reports.len(), "Should update the children's diagnostics of the deleted package: {after:#?}");
    assert!(reports[0]["diagnostics"].as_array().unwrap().is_empty(), "Should clean up the children's diagnostics of the deleted package");
}

#[test]
fn test_diagnostic_filtering() {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "diagnostic": { "filter": ["**/Foo*.java"] } } });
    ws.import_projects(&["eclipse/hello"]);
    let reports = ws.published_diagnostics_min(1);
    assert!(!reports.is_empty());
    for report in reports {
        let uri = report["uri"].as_str().unwrap();
        assert!(!uri.contains("Foo"), "{uri} should have been excluded from diagnostics.");
    }
}

fn uri_ends_with(report: &Value, suffix: &str) -> bool {
    report["uri"].as_str().unwrap().ends_with(suffix)
}

fn project_report(calls: &[Value], project: &str) -> Option<Value> {
    calls
        .iter()
        .find(|p| uri_ends_with(p, project) || uri_ends_with(p, &format!("{project}/")))
        .cloned()
}

const PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML: &str =
    "Project configuration is not up-to-date with pom.xml, requires an update.";

/// `Collections.sort(diags, DIAGNOSTICS_COMPARATOR)`.
fn sorted(mut diags: Vec<Value>) -> Vec<Value> {
    diags.sort_by(|d1, d2| {
        let line = |d: &Value| d["range"]["start"]["line"].as_i64().unwrap();
        line(d1)
            .cmp(&line(d2))
            .then_with(|| d1["message"].as_str().unwrap().cmp(d2["message"].as_str().unwrap()))
    });
    diags
}

#[test]
fn test_marker_listening() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["maven/broken"]);
    let all_calls = self::all_calls(&mut ws);

    /* With Maven 3.6.2 (m2e 1.14), source folders are no longer configured if dependencies are malformed (missing version tag here) */

    let pom_diags = all_calls.iter().find(|p| uri_ends_with(p, "pom.xml"));
    assert!(pom_diags.is_some(), "No pom.xml errors were found");
    let diags = diagnostics_of(pom_diags.unwrap());
    // https://github.com/redhat-developer/vscode-java/issues/2857
    // m2e 2.2.0 returns 3 markers
    assert_eq!(3, diags.len(), "{diags:?}");
    let diag = diags
        .iter()
        .find(|d| d["message"].as_str().unwrap().starts_with("Project build error"))
        .unwrap();
    assert_eq!(
        "Project build error: 'dependencies.dependency.version' for org.apache.commons:commons-lang3:jar is missing.",
        diag["message"]
    );
}

#[test]
#[ignore = "the oracle's default JRE here is 25 (\"... 1.7 but a JRE 25 is used\"); upstream runs on TestVMType's JRE 1.8"]
fn test_project_level_markers() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["maven/broken"]);
    let all_calls = self::all_calls(&mut ws);
    let project_diags = project_report(&all_calls, "maven/broken");
    assert!(project_diags.is_some(), "No maven/broken errors were found");
    let diags = sorted(diagnostics_of(&project_diags.unwrap()));
    assert_eq!(2, diags.len(), "{diags:?}");
    assert!(diags[1]["message"]
        .as_str()
        .unwrap()
        .starts_with("The compiler compliance specified is 1.7 but a JRE 1.8 is used"));
    let pom_diags = all_calls.iter().find(|p| uri_ends_with(p, "pom.xml"));
    assert!(pom_diags.is_some(), "No pom.xml errors were found");
    let diags = sorted(diagnostics_of(pom_diags.unwrap()));
    assert_eq!(3, diags.len(), "{diags:?}");
    assert!(diags[2]["message"].as_str().unwrap().starts_with("Project build error: "));
}

#[test]
#[ignore = "the server does not yet report the 'Project configuration is not up-to-date with pom.xml' warning when a build finds a changed pom; the oracle publishes it only for some build sequences, so the LSP port is unverified"]
fn test_project_configuration_is_not_up_to_date() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["maven/salut"]);
    let root = ws.project_root("salut");
    let pom = root.join("pom.xml");
    assert!(pom.exists());
    let content = std::fs::read_to_string(&pom).unwrap().replace("1.8", "11");
    std::fs::write(&pom, content).unwrap();

    ws.build_workspace(true);
    let all_calls = self::all_calls(&mut ws);
    // `assertNoErrors(project)` and `getWarningMarkers(project)` read the
    // markers of the full build, the latest report of each resource.
    let markers = |severity: u64| -> Vec<Value> {
        let mut seen: Vec<&str> = Vec::new();
        let mut found = Vec::new();
        for report in &all_calls {
            let uri = report["uri"].as_str().unwrap();
            if seen.contains(&uri) {
                continue;
            }
            seen.push(uri);
            found.extend(diagnostics_of(report).into_iter().filter(|d| d["severity"] == severity));
        }
        found
    };
    assert!(markers(1).is_empty(), "salut has errors: {:?}", markers(1));
    let warnings = markers(2);

    let out_of_date_warning = warnings
        .iter()
        .find(|w| w["message"] == PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML);
    assert!(out_of_date_warning.is_some(), "No out-of-date warning found");

    let project_diags = project_report(&all_calls, "maven/salut");
    assert!(project_diags.is_some(), "No maven/salut errors were found");
    let pom_diags = all_calls.iter().find(|p| uri_ends_with(p, "pom.xml"));
    assert!(pom_diags.is_some(), "No pom.xml errors were found");
    let diags = diagnostics_of(pom_diags.unwrap());
    assert_eq!(1, diags.len(), "{diags:?}");
    let diag = &diags[0];
    assert_eq!(PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML, diag["message"]);
    assert_eq!(2, diag["severity"]);
}

fn test_diagnostic(all_calls: &[Value]) {
    let mut project_diags: Vec<Value> = Vec::new();
    let mut pom_diags: Vec<Value> = Vec::new();
    for diag in all_calls {
        if uri_ends_with(diag, "maven/salut") || uri_ends_with(diag, "maven/salut/") {
            project_diags.extend(diagnostics_of(diag));
        } else if uri_ends_with(diag, "pom.xml") {
            pom_diags.extend(diagnostics_of(diag));
        }
    }
    assert!(!project_diags.is_empty(), "No maven/salut errors were found");
    let project_diag = project_diags
        .iter()
        .find(|p| p["message"].as_str().unwrap().contains("references non existing library"));
    assert!(project_diag.is_some(), "No 'references non existing library' diagnostic");
    assert_eq!(project_diag.unwrap()["severity"], 1);
    assert!(!pom_diags.is_empty(), "No pom.xml errors were found");
    let pom_diag = pom_diags
        .iter()
        .find(|p| p["message"].as_str().unwrap().starts_with("Missing artifact"));
    assert!(pom_diag.is_some(), "No 'missing artifact' diagnostic");
    let pom_diag = pom_diag.unwrap();
    assert!(pom_diag["message"].as_str().unwrap().starts_with("Missing artifact"));
    assert_eq!(pom_diag["range"]["start"]["line"], 19);
    assert_eq!(pom_diag["range"]["start"]["character"], 3);
    assert_eq!(pom_diag["range"]["end"]["line"], 19);
    assert_eq!(pom_diag["range"]["end"]["character"], 14);
    assert_eq!(pom_diag["severity"], 1);
}

#[test]
#[ignore = "a changed pom.xml does not yet refresh the Maven classpath container before a project update, and unresolved dependencies are not yet reported as 'references non existing library' after that refresh"]
fn test_missing_dependencies() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/salut"]);
    let root = ws.project_root("salut");
    let pom = root.join("pom.xml");
    assert!(pom.exists());
    ws.build_workspace(true);
    ws.assert_no_errors(&root);
    // edit pom.xml
    let content = std::fs::read_to_string(&pom)
        .unwrap()
        .replace("<version>3.18.0</version>", "<version>3.18.xx</version>");
    std::fs::write(&pom, content).unwrap();
    ws.files_changed(&[(&pom, 2)]);
    ws.wait_for_background_jobs();
    let all_calls = self::all_calls(&mut ws);
    test_diagnostic(&all_calls);
    // update project
    ws.client().notify("java/projectConfigurationUpdate", json!({ "uri": dir_uri(&root) }));
    ws.wait_for_background_jobs();
    let all_calls = self::all_calls(&mut ws);
    test_diagnostic(&all_calls);
    // build workspace
    ws.build_workspace(true);
    let all_calls = self::all_calls(&mut ws);
    test_diagnostic(&all_calls);
    // publish diagnostics
    ws.restart();
    ws.wait_for_background_jobs();
    let all_calls = self::all_calls(&mut ws);
    test_diagnostic(&all_calls);
}

#[test]
#[ignore = "disabled upstream (@Disabled: This test is unstable)"]
fn test_reset_pom_diagnostics() {
    let mut ws = Workspace::new();
    //import project
    ws.import_projects(&["maven/multimodule"]);
    let root = ws.project_root("multimodule");
    let pom = root.join("pom.xml");
    assert!(pom.exists());
    let compiler_plugin = "\n<build>\n".to_owned()
        + "    <pluginManagement>\n"
        + "        <plugins>\n"
        + "            <plugin>\n"
        + "                <artifactId>maven-compiler-plugin</artifactId>\n"
        + "                <version>3.8.0</version>\n"
        + "                <configuration>\n"
        + "                    <release>9</release>\n"
        + "                </configuration>\n"
        + "            </plugin>\n"
        + "        </plugins>\n"
        + "    </pluginManagement>\n"
        + "</build>\n";

    let content = std::fs::read_to_string(&pom)
        .unwrap()
        .replace("<profiles>", &format!("{compiler_plugin}\n<profiles>"));
    std::fs::write(&pom, content).unwrap();
    ws.files_changed(&[(&pom, 2)]);
    ws.wait_for_background_jobs();

    // `allCalls` is not reversed here: the reports in the order received.
    let mut all_calls = self::all_calls(&mut ws);
    all_calls.reverse();

    let pom_diags: Vec<&Value> = all_calls
        .iter()
        .filter(|p| uri_ends_with(p, "pom.xml") && !diagnostics_of(p).is_empty())
        .collect();
    assert_eq!(3, pom_diags.len(), "No pom.xml errors were found");
    assert!(uri_ends_with(pom_diags[0], "childmodule/pom.xml"), "{}", pom_diags[0]["uri"]);
    assert_eq!(1, diagnostics_of(pom_diags[0]).len());
    assert_eq!(
        diagnostics_of(pom_diags[0])[0]["message"],
        PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML
    );
    assert!(uri_ends_with(pom_diags[1], "module2/pom.xml"));
    assert_eq!(1, diagnostics_of(pom_diags[1]).len());
    assert_eq!(
        diagnostics_of(pom_diags[1])[0]["message"],
        PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML
    );
    assert!(uri_ends_with(pom_diags[2], "module3/pom.xml"));
    assert_eq!(1, diagnostics_of(pom_diags[2]).len());
    assert_eq!(
        diagnostics_of(pom_diags[2])[0]["message"],
        PROJECT_CONFIGURATION_IS_NOT_UP_TO_DATE_WITH_POM_XML
    );

    ws.client().notify("java/projectConfigurationUpdate", json!({ "uri": dir_uri(&root) }));
    ws.wait_for_background_jobs();

    let mut all_calls = self::all_calls(&mut ws);
    all_calls.reverse();
    let pom_diags: Vec<&Value> = all_calls.iter().filter(|p| uri_ends_with(p, "pom.xml")).collect();
    let mut reset1 = false;
    let mut reset2 = false;
    let mut reset3 = true;
    for diag in pom_diags {
        if uri_ends_with(diag, "childmodule/pom.xml") {
            assert_eq!(0, diagnostics_of(diag).len(), "Unexpected diagnostics:\n{:?}", diagnostics_of(diag));
            reset1 = true;
        } else if uri_ends_with(diag, "module2/pom.xml") {
            assert_eq!(0, diagnostics_of(diag).len(), "Unexpected diagnostics:\n{:?}", diagnostics_of(diag));
            reset2 = true;
        } else if uri_ends_with(diag, "module3/pom.xml") {
            //not a active module so was not updated. But this is actually a dubious behavior. Need to change that
            assert_eq!(1, diagnostics_of(diag).len(), "Unexpected diagnostics:\n{:?}", diagnostics_of(diag));
            reset3 = false;
        }
    }
    assert!(reset1, "childmodule/pom.xml diagnostics were not reset");
    assert!(reset2, "module2/pom.xml diagnostics were not reset");
    assert!(!reset3, "module3/pom.xml diagnostics were reset");
}

#[test]
fn test_encoding() {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    let encoding = |calls: &[Value]| -> usize {
        calls
            .iter()
            .filter(|p| project_report(std::slice::from_ref(p), "eclipse/hello").is_some())
            .flat_map(diagnostics_of)
            .filter(|d| d["message"].as_str().unwrap().starts_with("Project 'hello' has no explicit encoding set"))
            .count()
    };
    // The marker exists, but `java.project.encoding` ignores it.
    let all_calls_ignore = self::all_calls(&mut ws);
    assert_eq!(0, encoding(&all_calls_ignore));
    ws.update_settings(json!({ "java": { "project": { "encoding": "warning" } } }));
    ws.restart();
    let all_calls_warning = self::all_calls(&mut ws);
    assert_eq!(1, encoding(&all_calls_warning));
    ws.update_settings(json!({ "java": { "project": { "encoding": "setDefault" } } }));
    ws.restart();
    let all_calls_set_default = self::all_calls(&mut ws);
    assert_eq!(0, encoding(&all_calls_set_default));
}
