//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionResolveHandlerTest`.
//!
//! `setUp` turns postfix completion off and stubs
//! `isCompletionResolveDocumentSupport`; the Mockito mock answers `false`
//! for every other client preference. Like upstream, the project runs on
//! the rtstubs test JDK.

mod common;
use common::completion::*;

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionResolveHandlerTest.setUp`.
/// `getWorkingCopy` reconciles the unit; jdt.ls does that before completion
/// only with `java.lsp.joinOnCompletion`, so the oracle runs with it.
fn setup() -> T {
    let mut t = setup_with(settings_with(false, false));
    t.ws.oracle_java_options.push("-Djava.lsp.joinOnCompletion=true".into());
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { resolve_documentation: true, ..Default::default() };
    t
}

#[test]
fn test_snippet_while_item_defaults_enabled_generic_snippets() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\t/**\n\t * This is a test.\n\t */\n\tprivate int a;\n\tpublic void test(int a) {\n\t\ta\n\t}\n}",
    );
    let list = t.request_completions(&unit, "\ta");

    assert!(!list.is_null());

    let item = items(&list).into_iter().find(|i| i["insertText"] == "this.a");
    let item = item.unwrap_or_else(|| panic!("no this.a item: {list:#}"));

    let resolved = t.resolve(&item);
    assert_eq!("This is a test.", s(&resolved["documentation"]).trim(), "{resolved:#}");
}

#[test]
#[ignore = "@Disabled upstream: requires a real JDK, instead of stubbed JRE with no module info"]
fn test_module_completion_resolve_shows_documentation() {
    let mut t = setup();
    t.ws.import_projects(&["eclipse/java25"]);
    let root = t.ws.project_root("java25");
    let uri = url::Url::from_file_path(root.join("src/main/java/org/sample/ImportModule.java")).unwrap().to_string();
    let source = "package org.sample;\n\nimport module java.\n\npublic class ImportModule {}\n";
    let unit = t.get_working_copy_uri(&uri, source);

    let list = t.request_completions(&unit, "java.");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "Expected module completions after 'import module java.'");

    let java_sql_item = items(&list).into_iter().filter(|i| i["kind"] == KIND_MODULE).find(|i| i["label"] == "java.base");
    assert!(java_sql_item.is_some(), "Expected 'java.base' module in completion list");

    let resolved = t.resolve(&java_sql_item.unwrap());
    assert!(!resolved["documentation"].is_null(), "Resolved module completion should have documentation");
    let doc = &resolved["documentation"];
    let doc_text = doc.as_str().or_else(|| doc["value"].as_str());
    assert!(doc_text.is_some(), "Documentation content should not be null");
    assert!(!doc_text.unwrap().trim().is_empty(), "Module documentation should not be empty");
}
