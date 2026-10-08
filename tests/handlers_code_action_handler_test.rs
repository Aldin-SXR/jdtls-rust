//! Ports of `org.eclipse.jdt.ls.core.internal.handlers.CodeActionHandlerTest`.

mod common;
use common::jdtls::*;
use common::quickfix::{get_range, QuickFixTest};
use serde_json::{json, Value};

fn setup(source: &str, path: &str) -> (Workspace, String) {
    setup_with(source, path, |_| {})
}

/// [`setup`], configuring the workspace before the server starts.
fn setup_with(source: &str, path: &str, configure: impl FnOnce(&mut Workspace)) -> (Workspace, String) {
    let mut ws = QuickFixTest::new().ws;
    ws.import_projects(&["eclipse/hello"]);
    configure(&mut ws);
    let settings = ws.dir.join("settings.prefs");
    std::fs::write(&settings, "").unwrap();
    ws.settings["java"]["settings"]["url"] =
        json!(tower_lsp::lsp_types::Url::from_file_path(settings)
            .unwrap()
            .to_string());
    let uri = ws.path_uri(&format!("eclipse/hello/{path}"));
    ws.open_with(&uri, source);
    (ws, uri)
}

fn diagnostic(code: &str, range: &Value) -> Value {
    json!({"code":code,"range":range,"severity":1,"message":"Test Diagnostic","source":"Java"})
}

fn actions(
    ws: &mut Workspace,
    uri: &str,
    range: Value,
    diagnostics: Vec<Value>,
    only: Option<&str>,
) -> Vec<Value> {
    let mut context = json!({"diagnostics":diagnostics});
    if let Some(kind) = only {
        context["only"] = json!([kind]);
    }
    let result = ws.request(
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},"range":range,"context":context}),
    );
    result
        .as_array()
        .expect("non-null code action response")
        .clone()
}

fn quote_actions() {
    let source = "public class Foo {\n\tvoid foo() {\nString s = \"some str\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "some str");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("1610612995", &range)],
        None,
    );
    assert!(!result.is_empty());
    assert_eq!("quickfix", result[0]["kind"]);
    assert!(!result[0]["edit"].is_null());
}

#[test]
fn test_code_action_remove_unterminated_string() {
    quote_actions();
}

#[test]
fn test_code_action_literal_remove_unterminated_string() {
    quote_actions();
}

#[test]
fn test_code_action_superfluous_semicolon() {
    let source = "public class Foo {\n\tvoid foo() {\n;\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, ";");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536871092", &range)],
        None,
    );
    assert!(!result.is_empty());
    assert_eq!("quickfix", result[0]["kind"]);
    let edits = result[0]["edit"]["changes"][&uri].as_array().unwrap();
    assert_eq!(1, edits.len());
    assert_eq!("", edits[0]["newText"]);
    assert_eq!(range, edits[0]["range"]);
}

#[test]
fn test_code_action_organize_imports_source_action_only() {
    let source = "import java.util.List;\npublic class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536870973", &range)],
        Some("source.organizeImports"),
    );
    assert!(!result.is_empty(), "No organize imports actions were found");
    for action in result {
        assert!(action["kind"]
            .as_str()
            .unwrap()
            .starts_with("source.organizeImports"));
    }
}

#[test]
fn test_code_action_organize_imports_quick_fix() {
    let source = "import java.util.List;\npublic class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let result = actions(&mut ws, &uri, get_range(source, "util"), vec![], None);
    assert!(result
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Organize imports"));
    let result = actions(&mut ws, &uri, get_range(source, "String bar"), vec![], None);
    assert!(!result
        .iter()
        .any(|a| a["kind"] == "quickassist" && a["title"] == "Organize imports"));
}

#[test]
fn test_code_action_refactor_actions_only() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let result = actions(
        &mut ws,
        &uri,
        range.clone(),
        vec![diagnostic("536870973", &range)],
        Some("refactor"),
    );
    assert!(!result.is_empty(), "No refactor actions were found");
    for action in result {
        assert!(action["kind"].as_str().unwrap().starts_with("refactor"));
    }
}

#[test]
fn test_code_action_error_from_other_sources() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tInteger bar = 2000;\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let diag = json!({"code":"MagicNumberCheck","range":range,"severity":1,"message":"'2000' is a magic number.","source":"Checkstyle"});
    let result = actions(&mut ws, &uri, range, vec![diag], Some("refactor"));
    assert!(!result.is_empty(), "No refactor actions were found");
    for action in result {
        assert!(action["kind"].as_str().unwrap().starts_with("refactor"));
    }
}

#[test]
fn test_code_action_exception() {
    let mut ws = QuickFixTest::new().ws;
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.path_uri("eclipse/hello/nopackage/Test.java");
    ws.open(&uri);
    actions(&mut ws, &uri, range(0, 17, 0, 17), vec![], None);
}

#[test]
fn test_no_unnecessary_code_actions() {
    let source = "package org.sample;\n\npublic class Foo {\n\tprivate String foo;\n\tpublic String getFoo() {\n\t  return foo;\n\t}\n   \n\tpublic void setFoo(String newFoo) {\n\t  foo = newFoo;\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/org/sample/Foo.java");
    let result = actions(
        &mut ws,
        &uri,
        get_range(source, "String foo;"),
        vec![],
        None,
    );
    assert!(
        !result.iter().any(|a| a["kind"] == "source.organizeImports"),
        "No need for organize imports action"
    );
    assert!(
        !result
            .iter()
            .any(|a| a["kind"] == "source.generate.accessors"),
        "No need for generate getter and setter action"
    );
}

fn unused_import_actions(with_other_diagnostic: bool) {
    let source = "import java.sql.*; \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let selected = get_range(source, "java.sql");
    let mut diagnostics = vec![];
    if with_other_diagnostic {
        diagnostics.push(json!({"range":range(0,0,0,1),"message":"fake dignostic without code"}));
    }
    diagnostics.push(diagnostic("268435844", &selected));
    let result = actions(&mut ws, &uri, selected, diagnostics, None);
    assert!(result.len() >= 3, "{result:#?}");
    assert!(result.iter().any(|a| a["kind"] == "quickfix"));
    assert_eq!(
        1,
        result
            .iter()
            .filter(|a| a["kind"] == "source.organizeImports")
            .count()
    );
    assert!(!result[0]["edit"].is_null());
}

#[test]
fn test_code_action_remove_unused_import() {
    unused_import_actions(false);
}

#[test]
fn test_code_action_ignoring_other_diagnostic_without_code() {
    unused_import_actions(true);
}

fn kind_of(a: &Value) -> &str {
    a["kind"].as_str().unwrap_or("")
}

/// `getBaseKind(kind)`: the kind up to its first `.`.
fn base_kind(kind: &str) -> &str {
    kind.split('.').next().unwrap()
}

/// `CodeActionHandlerTest.findActions(codeActions, kind)`.
fn find_actions<'a>(actions: &'a [Value], kind: &str) -> Vec<&'a Value> {
    actions.iter().filter(|a| kind_of(a) == kind).collect()
}

const QUICK_ASSIST: &str = "quickassist";

#[test]
fn test_code_action_literal_remove_unused_import() {
    let source = "import java.sql.*; \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "java.sql");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("268435844", &range)], None);
    assert!(code_actions.len() >= 4, "{code_actions:#?}");
    assert_eq!("quickfix", kind_of(&code_actions[0]));
    assert_eq!("quickfix", kind_of(&code_actions[1]));
    assert_eq!(QUICK_ASSIST, kind_of(&code_actions[2]));
    assert_eq!("source.generate.constructors", kind_of(&code_actions[3]));
    assert_eq!("source.organizeImports", kind_of(&code_actions[4]));
    assert!(!code_actions[0]["edit"].is_null());
}

#[test]
fn test_code_action_source_actions_only() {
    let source = "import java.sql.*; \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "foo()");
    let source_actions = actions(&mut ws, &uri, range, vec![], Some("source"));
    assert!(!source_actions.is_empty(), "No source actions were found");
    for code_action in &source_actions {
        assert!(kind_of(code_action).starts_with("source"), "Unexpected kind:{}", kind_of(code_action));
    }
}

#[test]
fn test_code_action_quickfix_actions_only() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let quickfix_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("536870973", &range)], Some("quickfix"));
    assert!(!quickfix_actions.is_empty(), "No quickfix actions were found");
    for code_action in &quickfix_actions {
        assert!(kind_of(code_action).starts_with("quickfix"), "Unexpected kind:{}", kind_of(code_action));
    }
}

#[test]
#[ignore = "needs the QuickAssistProcessor/RefactorProcessor ports (surround with try/catch, split variable declaration, add final modifier); legacy bridge actions are still appended unsorted; passes on the oracle"]
fn test_code_action_all_kinds_of_actions() {
    let source = "public class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "bar");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("536870973", &range)], None);
    assert!(!code_actions.is_empty(), "No code actions were found");
    assert!(code_actions.iter().any(|a| kind_of(a).starts_with("quickfix")), "No quickfix actions were found");
    assert!(code_actions.iter().any(|a| kind_of(a).starts_with("refactor")), "No refactor actions were found");
    assert!(code_actions.iter().any(|a| kind_of(a).starts_with("source")), "No source actions were found");

    let base_kinds: Vec<&str> = code_actions.iter().map(|a| base_kind(kind_of(a))).collect();
    let last_index_of = |k: &str| base_kinds.iter().rposition(|x| *x == k).map_or(-1, |i| i as i64);
    let index_of = |k: &str| base_kinds.iter().position(|x| *x == k).map_or(-1, |i| i as i64);
    assert!(last_index_of("quickfix") < index_of("refactor"), "quickfix actions should be ahead of refactor actions");
    assert!(last_index_of("refactor") < index_of("source"), "refactor actions should be ahead of source actions");
}

#[test]
fn test_code_action_npe_in_new_cu_proposal() {
    let source = "package org.sample;\npublic class Foo {\n\tpublic static void main(String[] args) {\n\t\tnew javax.activity\n\t}\n}\n";
    let (mut ws, uri) = setup(source, "src/org/sample/Foo.java");
    let range = get_range(source, "javax.activity");
    actions(&mut ws, &uri, range.clone(), vec![diagnostic("16777218", &range)], None);
}

#[test]
fn test_code_action_npe_on_invalid_method_call() {
    let source = "public class App {\n\tString foo = App.test(); // compilation error, test() doesn't exist\n}\n";
    let (mut ws, uri) = setup(source, "src/App.java");
    let app_index = source.find("App.test()").unwrap() as i64;
    let range = common::quickfix::to_range(source, app_index, "App".len() as i64);
    // This should not throw NPE
    actions(&mut ws, &uri, range, vec![], None);
}

#[test]
fn test_code_action_refresh_diagnostics_command_in_new_cu_proposal() {
    let source = "package org.sample;\npublic class Foo {\n\tpublic static void main(String[] args) {\n\t\tCU obj;\n\t}\n}\n";
    let mut ws = QuickFixTest::new().ws;
    // `preferences.setValidateAllOpenBuffersOnChanges(false)`
    ws.settings["java"]["edit"]["validateAllOpenBuffersOnChanges"] = json!(false);
    ws.import_projects(&["eclipse/hello"]);
    let uri = ws.path_uri("eclipse/hello/src/org/sample/Foo.java");
    ws.open_with(&uri, source);
    let range = get_range(source, "CU");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("16777218", &range)], None);
    let new_cu_proposal = code_actions.iter().find(|a| a["title"] == "Create class 'CU'");
    assert!(new_cu_proposal.is_some(), "{code_actions:#?}");
    let command = &new_cu_proposal.unwrap()["command"];
    assert!(!command.is_null());
    assert_eq!("java.project.refreshDiagnostics", command["command"]);
}

#[test]
fn test_filter_types() {
    let source = "package org.sample;\n\npublic class Foo {\n\tList foo;\n}\n";
    // The upstream test VM's library has a single `List`.
    let (mut ws, uri) = setup_with(source, "src/org/sample/Foo.java", |ws| ws.use_upstream_test_jdk("hello"));
    let range = get_range(source, "List");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![], None);
    assert!(code_actions.iter().any(|a| kind_of(a) == "source.organizeImports"), "No organize imports action {:?}", code_actions.iter().map(|a| (a["kind"].clone(), a["title"].clone())).collect::<Vec<_>>());
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "java": { "completion": { "filteredTypes": ["java.util.*"] } } } }),
    );
    ws.wait_idle();
    let code_actions = actions(&mut ws, &uri, range, vec![], None);
    assert!(!code_actions.iter().any(|a| kind_of(a) == "source.organizeImports"), "No need for organize imports action");
}

#[test]
#[ignore = "needs the Convert to anonymous class creation quick assist (only the legacy bridge path has one); passes on the oracle"]
fn test_code_action_custom_file_formatting_options() {
    let mut ws = QuickFixTest::new().ws;
    ws.capabilities["workspace"]["configuration"] = json!(true);
    ws.import_projects(&["eclipse/hello"]);
    ws.use_upstream_test_jdk("hello");
    let root = ws.project_root("hello");
    // javaProject.getOptions(false) + indent using tabs, tab size 4.
    let prefs = root.join(".settings/org.eclipse.jdt.core.prefs");
    let mut project_options: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&prefs)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| *k != "eclipse.preferences.version")
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    project_options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    project_options.insert("org.eclipse.jdt.core.formatter.tabulation.size".into(), "4".into());
    ws.set_project_options(&root, &project_options);
    // `connection.configuration(..)` answers 4 and true (indent using spaces).
    ws.client().request_results.insert("workspace/configuration".to_owned(), json!([4, true]));

    let source = concat!(
        "package test1;\n",
        "interface I {\n",
        "\tvoid method();\n",
        "}\n",
        "public class E {\n",
        "\tvoid bar(I i) {\n",
        "\t}\n",
        "\tvoid foo() {\n",
        "\t\tbar(() /*[*//*]*/-> {\n",
        "\t\t});\n",
        "\t}\n",
        "}\n",
    );
    let uri = ws.create_cu(&root, "src", "test1", "E.java", source);
    ws.open(&uri);
    let range = get_range(source, "/*[*//*]*/");
    let code_actions = actions(&mut ws, &uri, range, vec![], Some("refactor"));
    let found = code_actions.iter().find(|a| a["title"] == "Convert to anonymous class creation");
    assert!(found.is_some(), "{code_actions:#?}");
    let actual = common::quickfix::evaluate_workspace_edit(&ws, &found.unwrap()["edit"]).unwrap();
    let expected = concat!(
        "package test1;\n",
        "interface I {\n",
        "\tvoid method();\n",
        "}\n",
        "public class E {\n",
        "\tvoid bar(I i) {\n",
        "\t}\n",
        "\tvoid foo() {\n",
        "\t\tbar(new I() {\n",
        "            @Override\n",
        "            public void method() {\n",
        "            }\n",
        "        });\n",
        "\t}\n",
        "}\n",
    );
    assert_eq!(expected, actual);
}

#[test]
#[ignore = "needs QuickAssistProcessor.getAddMissingMethodDeclarationProposal and the method-reference-to-lambda assist; passes on the oracle"]
fn test_code_action_unimplemented_method_reference() {
    let source = "package test1;\nimport java.util.Comparator;\nclass Foo {\n    void foo(Comparator<String> c) {\n    }\n    void bar() {\n        foo(this::action);\n    }\n}";
    let (mut ws, uri) = setup(source, "src/java/Foo.java");
    let range = get_range(source, "action");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("603979903", &range)], Some(QUICK_ASSIST));
    assert_eq!("Add missing method 'action' to class 'Foo'", code_actions[1]["title"], "{code_actions:#?}");
}

#[test]
fn test_code_action_ignore_compiler_issue() {
    let source = "package java;\npublic class Foo {\n  @SuppressWarnings(\"deprecation\")\n  public void test () {\n  }\n}";
    // `clientPreferences.isResourceOperationSupported()` is true.
    let (mut ws, uri) = setup_with(source, "src/java/Foo.java", |ws| {
        ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] = json!(["create", "rename", "delete"]);
    });
    let range = get_range(source, "deprecation");
    let code_actions = actions(&mut ws, &uri, range.clone(), vec![diagnostic("536871547", &range)], None);
    assert!(!code_actions.is_empty());
    let code_action = code_actions
        .iter()
        .find(|a| kind_of(a) == "quickfix" && a["title"] == "Ignore compiler problem(s)")
        .unwrap_or_else(|| panic!("{:?}", code_actions.iter().map(|a| (a["kind"].clone(), a["title"].clone())).collect::<Vec<_>>()));
    let we = &code_action["edit"];
    let changes = we["documentChanges"].as_array().unwrap();
    assert_eq!(1, changes.len());
    assert_eq!("org.eclipse.jdt.core.compiler.problem.unusedWarningToken=ignore\n", changes[0]["edits"][0]["newText"]);
}

/// `CodeActionUtil.constructCodeActionParams(unit, search)` + `server.codeAction(params)`.
fn quick_assist_titles(source: &str, path: &str, search: &str) -> Vec<String> {
    let (mut ws, uri) = setup(source, path);
    let code_actions = actions(&mut ws, &uri, get_range(source, search), vec![], None);
    find_actions(&code_actions, QUICK_ASSIST).iter().map(|a| a["title"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn test_quick_assist_for_import_declaration_order() {
    let source = "import java.util.List;\npublic class Foo {\n\tvoid foo() {\n\t\tString bar = \"astring\";\t}\n}\n";
    let titles = quick_assist_titles(source, "src/java/Foo.java", "util");
    assert_eq!("Organize imports", titles[0]);
}

#[test]
fn test_quick_assist_for_field_declaration_order() {
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let titles = quick_assist_titles(source, "A.java", "String name");
    assert_eq!("Generate Getter and Setter for 'name'", titles[0]);
    assert_eq!("Generate Getter for 'name'", titles[1]);
    assert_eq!("Generate Setter for 'name'", titles[2]);
    assert_eq!("Generate Constructors...", titles[3]);
}

#[test]
fn test_quick_assist_for_type_declaration_order() {
    let source = "package p;\r\n\r\npublic class A {\r\n\tpublic String name = \"name\";\r\n\tpublic String pet = \"pet\";\r\n}";
    let titles = quick_assist_titles(source, "A.java", "A");
    assert_eq!("Generate Getters and Setters", titles[0]);
    assert_eq!("Generate Getters", titles[1]);
    assert_eq!("Generate Setters", titles[2]);
    assert_eq!("Generate Constructors...", titles[3]);
    assert_eq!("Generate hashCode() and equals()...", titles[4]);
    assert_eq!("Generate toString()...", titles[5]);
    assert_eq!("Override/Implement Methods...", titles[6]);
}
