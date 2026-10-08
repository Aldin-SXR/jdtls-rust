//! Port of `org.eclipse.jdt.ls.core.internal.handlers.PostfixCompletionTest`.
//!
//! `setUp` enables postfix completion; `mockLSP3Client()` is
//! `mockLSPClient(true, true, true)`: snippets, signature help, the
//! `editRange`/`insertTextFormat`/`insertTextMode` item defaults and the
//! `AdjustIndentation` insert text mode. The mocked `ClientPreferences`
//! become LSP client capabilities ([`Caps`]). Like upstream, the project
//! runs on the rtstubs test JDK. `getWorkingCopy` reconciles the unit;
//! jdt.ls does that before completion only with `java.lsp.joinOnCompletion`,
//! so the oracle runs with it.

mod common;
use common::completion::*;
use serde_json::{json, Value};

const TEST_SOURCE_PATH: &str = "src/org/sample/Test.java";

/// `AbstractCompilationUnitBasedTest.setup` + `PostfixCompletionTest.setUp`.
fn setup() -> T {
    setup_lazy(false)
}

fn setup_lazy(lazy_resolve_text_edit: bool) -> T {
    let mut t = setup_with(settings_with(true, lazy_resolve_text_edit));
    t.ws.use_upstream_test_jdk("hello");
    t.ws.oracle_java_options.push("-Djava.lsp.joinOnCompletion=true".into());
    t.caps = Caps::mock(true, true, true);
    t
}

fn first(list: &Value) -> Value {
    items(list).into_iter().next().unwrap_or_else(|| panic!("no completion items: {list:#}"))
}

fn range(sl: u64, sc: u64, el: u64, ec: u64) -> Value {
    json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } })
}

/// `item.getAdditionalTextEdits().get(0).getRange()`.
fn first_additional_range(item: &Value) -> Value {
    item["additionalTextEdits"][0]["range"].clone()
}

#[test]
fn test_cast_lazy_resolve() {
    let mut t = setup_lazy(true);
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.cast\t}\n}",
    );
    let list = t.request_completions(&unit, "a.cast");
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!("cast", s(&item["label"]));
    assert_eq!(s(&item["insertText"]), "((${1})${inner_expression})${0}");
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    assert_eq!(range(3, 2, 3, 8), first_additional_range(&item), "{item:#}");
}

#[test]
fn test_assert() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(Boolean identifier) {\n\t\tidentifier.assert\t}\n}",
    );
    let list = t.request_completions(&unit, "identifier.assert");
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!("assert", s(&item["label"]));
    assert_eq!(s(&item["insertText"]), "assert identifier;");
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    assert_eq!(range(3, 2, 3, 19), first_additional_range(&item), "{item:#}");
}

#[test]
fn test_cast() {
    let mut t = setup();
    t.caps.label_details = true;
    t.caps.insert_text_mode_default = Some(MODE_AS_IS as u32);
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.cast\t}\n}",
    );
    let list = t.request_completions(&unit, "a.cast");
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!("cast", s(&item["label"]));
    assert!(item["labelDetails"]["detail"].is_null(), "{item:#}");
    assert_eq!("Casts the expression to a new type", s(&item["labelDetails"]["description"]));
    assert_eq!(s(&item["insertText"]), "((${1})a)${0}");
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    assert_eq!(item["insertTextMode"], MODE_ADJUST_INDENTATION, "{item:#}");
    assert_eq!(range(3, 2, 3, 8), first_additional_range(&item), "{item:#}");
}

#[test]
fn test_if() {
    let mut t = setup();
    t.caps.insert_text_mode_default = Some(MODE_ADJUST_INDENTATION as u32);
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean a) {\n\t\ta.if\t}\n}",
    );
    let list = t.request_completions(&unit, "a.if");
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!("if", s(&item["label"]));
    assert_eq!(s(&item["insertText"]), "if (a) {\n\t${0}\n}");
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    assert!(item["insertTextMode"].is_null(), "{item:#}");
    assert_eq!(range(3, 2, 3, 6), first_additional_range(&item), "{item:#}");
}

/// The shape shared by most postfix tests: the first item's label, insert
/// text, snippet format and the range of the first additional edit.
fn check_first(t: &mut T, source: &str, behind: &str, label: &str, insert_text: &str, expected_range: Value) {
    let unit = t.get_working_copy(TEST_SOURCE_PATH, source);
    let list = t.request_completions(&unit, behind);
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!(label, s(&item["label"]));
    assert_eq!(s(&item["insertText"]), insert_text);
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    assert_eq!(expected_range, first_additional_range(&item), "{item:#}");
}

#[test]
fn test_else() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean a) {\n\t\ta.else\t}\n}",
        "a.else",
        "else",
        "if (!a) {\n\t${0}\n}",
        range(3, 2, 3, 8),
    );
}

#[test]
fn test_for() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] a) {\n\t\ta.for\t}\n}",
        "a.for",
        "for",
        "for (String ${1:a2} : a) {\n\t${0}\n}",
        range(3, 2, 3, 7),
    );
}

#[test]
fn test_fori() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] a) {\n\t\ta.fori\t}\n}",
        "a.fori",
        "fori",
        "for (int ${1:a2} = 0; ${1:a2} < a.length; ${1:a2}++) {\n\t${0}\n}",
        range(3, 2, 3, 8),
    );
}

#[test]
fn test_forr() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] a) {\n\t\ta.forr\t}\n}",
        "a.forr",
        "forr",
        "for (int ${1:a2} = a.length - 1; ${1:a2} >= 0; ${1:a2}--) {\n\t${0}\n}",
        range(3, 2, 3, 8),
    );
}

#[test]
fn test_nnull() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.nnull\t}\n}",
        "a.nnull",
        "nnull",
        "if (a != null) {\n\t${0}\n}",
        range(3, 2, 3, 9),
    );
}

#[test]
fn test_null() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.null\t}\n}",
        "a.null",
        "null",
        "if (a == null) {\n\t${0}\n}",
        range(3, 2, 3, 8),
    );
}

#[test]
fn test_opt() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(Object identifier) {\n\t\tidentifier.opt\t}\n}",
        "identifier.opt",
        "opt",
        "Optional.ofNullable(identifier)",
        range(3, 2, 3, 16),
    );
}

#[test]
fn test_not() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean a) {\n\t\ta.not\t}\n}",
        "a.not",
        "not",
        "!a",
        range(3, 2, 3, 7),
    );
}

#[test]
fn test_sysout() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.sysout\t}\n}",
        "a.sysout",
        "sysout",
        "System.out.println(a);${0}",
        range(3, 2, 3, 10),
    );
}

#[test]
fn test_sysout_item_defaults_enabled() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tvoid sysop(){}\n\tpublic void testMethod(Object args) {\n\t\tnew Test().syso\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "new Test().syso");
    assert!(!list.is_null());
    assert!(!list["itemDefaults"]["editRange"].is_null(), "{list:#}");
    assert_eq!(list["itemDefaults"]["insertTextFormat"], FORMAT_SNIPPET, "{list:#}");
    assert_eq!(list["itemDefaults"]["insertTextMode"], MODE_ADJUST_INDENTATION, "{list:#}");

    let ci = items(&list).into_iter().find(|item| s(&item["label"]).starts_with("sysout"));
    let ci = ci.unwrap_or_else(|| panic!("no sysout item: {list:#}"));

    assert_eq!("System.out.println(new Test());${0}", s(&ci["textEditText"]));
    //check that the fields covered by itemDefaults are set to null
    assert!(ci["textEdit"].is_null(), "{ci:#}");
    assert!(ci["insertTextFormat"].is_null(), "{ci:#}");
    assert!(ci["insertTextMode"].is_null(), "{ci:#}");
    assert_eq!(ci["kind"], KIND_SNIPPET, "{ci:#}");
}

#[test]
fn test_sysout_object() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\tBoolean foo = true;\n\t\tfoo.sysout\t}\n}",
        "foo.sysout",
        "sysout",
        "System.out.println(foo);${0}",
        range(4, 2, 4, 12),
    );
}

#[test]
fn test_sysoutv_object() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\tBoolean foo = true;\n\t\tfoo.sysoutv\t}\n}",
        "foo.sysoutv",
        "sysoutv",
        "System.out.println(\"foo = \" + foo);${0}",
        range(4, 2, 4, 13),
    );
}

#[test]
fn test_sysouf_object() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\tBoolean foo = true;\n\t\tfoo.sysouf\t}\n}",
        "foo.sysouf",
        "sysouf",
        "System.out.printf(\"\", foo);${0}",
        range(4, 2, 4, 12),
    );
}

#[test]
fn test_syserr_object() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\tBoolean foo = true;\n\t\tfoo.syserr\t}\n}",
        "foo.syserr",
        "syserr",
        "System.err.println(foo);${0}",
        range(4, 2, 4, 12),
    );
}

#[test]
fn test_format() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.format\t}\n}",
    );
    let list = t.request_completions(&unit, "a.format");
    assert!(!list.is_null());
    let item = items(&list).into_iter().find(|i| i["kind"] == KIND_SNIPPET);
    let item = item.unwrap_or_else(|| panic!("no snippet item: {list:#}"));
    assert_eq!("format", s(&item["label"]));
    assert_eq!(s(&item["textEditText"]), "String.format(a, ${0});");
    assert_eq!(range(3, 2, 3, 10), first_additional_range(&item), "{item:#}");
}

#[test]
fn test_throw() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tException e;\n\t\te.throw\t}\n}",
        "e.throw",
        "throw",
        "throw e;",
        range(4, 2, 4, 9),
    );
}

#[test]
fn test_var() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.var\t}\n}",
        "a.var",
        "var",
        "String ${1:a2} = a;${0}",
        range(3, 2, 3, 7),
    );
}

#[test]
fn test_var2() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\nimport java.util.Collections;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\tCollections.emptyList().var\t}\n}",
    );
    let list = t.request_completions(&unit, ".emptyList().var");
    assert!(!list.is_null());
    let item = first(&list);
    assert_eq!("var", s(&item["label"]));
    assert_eq!(s(&item["insertText"]), "List<Object> ${1:emptyList} = Collections.emptyList();${0}");
    assert_eq!(item["insertTextFormat"], FORMAT_SNIPPET, "{item:#}");
    let additional_text_edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert_eq!(range(4, 2, 4, 29), additional_text_edits[0]["range"], "{item:#}");
    assert!(
        additional_text_edits.iter().any(|e| s(&e["newText"]).contains("import java.util.List;")),
        "{item:#}"
    );
}

#[test]
fn test_par() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(String a) {\n\t\ta.par\t}\n}",
        "a.par",
        "par",
        "(a)",
        range(3, 2, 3, 7),
    );
}

#[test]
fn test_while() {
    let mut t = setup();
    check_first(
        &mut t,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean a) {\n\t\ta.while\t}\n}",
        "a.while",
        "while",
        "while (a) {\n\t${0}\n}",
        range(3, 2, 3, 9),
    );
}

#[test]
fn test_can_evaluate() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tSystem.\t}\n}",
    );
    let list = t.request_completions(&unit, "System.");
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|i| i["kind"] == KIND_SNIPPET), "{list:#}");
}

#[test]
fn test_can_evaluate2() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\nimport java.util.ArrayList;\nimport java.util.List;\npublic class Test {\n\tpublic void testMethod() {\n\t\tList<String> lines = new ArrayList<>();\n\t\tlines.\n\t\tif (lines.isEmpty()){return;}\n\t}\n}",
    );
    let list = t.request_completions(&unit, "\t\tlines.");
    assert!(!items(&list).is_empty(), "{list:#}");
}

#[test]
fn test_java_doc() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\npublic enum Test {\n\t/**\n\t* {@link ArrayList}\n\t* Match case for the first letter.\n\t*/\n\tFIRSTLETTER;\n}",
    );
    let list = t.request_completions(&unit, "letter.");
    assert!(items(&list).is_empty(), "{list:#}");
}

#[test]
fn test_completion_generic_anonymous_class() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\nimport java.util.ArrayList;\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\t ArrayList list = new ArrayList() {}. \n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "{}.");
    assert!(!items(&list).is_empty(), "{list:#}");
}

#[test]
fn test_postfix_completion_not_in_import_declaration() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\n\nimport static java.util.ArrayList.;\npublic class Test {}\n",
    );
    let list = t.request_completions(&unit, "import static java.util.ArrayList.");
    assert_eq!(
        0,
        items(&list).len(),
        "Postfix completion should not be triggered in import declarations:{:?}",
        items(&list)
    );
}
