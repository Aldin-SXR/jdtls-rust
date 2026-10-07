//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionHandlerLazyResolveTest`.
//!
//! `setUp` turns postfix completion off and lazy text-edit resolution on;
//! `mockLSP3Client()` stubs snippet support. The mocked `ClientPreferences`
//! become LSP client capabilities ([`Caps`]). Like upstream, the project
//! runs on the rtstubs test JDK.

mod common;
use common::completion::*;
use serde_json::Value;

const TEST_SOURCE_PATH: &str = "src/org/sample/Test.java";

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionHandlerLazyResolveTest.setUp`.
fn setup() -> T {
    let mut t = setup_with(settings_with(false, true));
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { snippets: true, ..Default::default() };
    t
}

/// `JsonRpcHelpers.toOffset`.
fn to_offset(text: &str, line: u64, character: u64) -> usize {
    let line_start = if line == 0 { 0 } else { text.match_indices('\n').nth(line as usize - 1).unwrap().0 + 1 };
    let mut units = 0u64;
    for (i, c) in text[line_start..].char_indices() {
        if units >= character {
            return line_start + i;
        }
        units += c.len_utf16() as u64;
    }
    text.len()
}

fn replaced_content<'a>(unit: &'a Unit, item: &Value) -> &'a str {
    let range = &item["textEdit"]["range"];
    let start = to_offset(&unit.text, range["start"]["line"].as_u64().unwrap(), range["start"]["character"].as_u64().unwrap());
    let end = to_offset(&unit.text, range["end"]["line"].as_u64().unwrap(), range["end"]["character"].as_u64().unwrap());
    &unit.text[start..end]
}

fn resolved_text(t: &mut T, item: &Value) -> String {
    let resolved = t.resolve(item);
    assert!(!resolved["textEdit"].is_null(), "{resolved:#}");
    s(&resolved["textEdit"]["newText"]).to_owned()
}

#[test]
fn test_fully_qualified_type_completion1() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tjava.util.List\n}\n");
    let list = t.request_completions(&unit, "java.util.List");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    let filter_text = s(&item["filterText"]);
    let replaced = replaced_content(&unit, item);
    assert!(filter_text.starts_with(replaced), "{filter_text:?} vs {replaced:?}");
}

#[test]
fn test_fully_qualified_type_completion2() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\nimport java.util.List\npublic class Test {}\n");
    let list = t.request_completions(&unit, "import java.util.List");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    let filter_text = s(&item["filterText"]);
    let replaced = replaced_content(&unit, item);
    assert!(filter_text.starts_with(replaced), "{filter_text:?} vs {replaced:?}");
}

#[test]
fn test_snippet_sysout() {
    let mut t = setup();
    t.caps.insert_text_mode_default = Some(MODE_ADJUST_INDENTATION as u32);
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsysout\t}\n}");
    let list = t.request_completions(&unit, "sysout");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("sysout", s(&item["label"]));
    assert_eq!("System.out.println(${0});", s(&item["insertText"]));
    assert!(item["insertTextMode"].is_null(), "{item:#}");
}

fn find_snippet(list: &Value, label: &str) -> Option<Value> {
    items(list).into_iter().find(|i| i["kind"] == KIND_SNIPPET && i["label"] == label)
}

#[test]
fn test_snippet_sout() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsout\t}\n}");
    let list = t.request_completions(&unit, "sout");
    assert!(!list.is_null());
    let item = find_snippet(&list, "sout").expect("Failed to find snippet: 'sout'.");
    assert_eq!("System.out.println(${0});", s(&item["insertText"]));
}

#[test]
fn test_snippet_syserr() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsyserr\t}\n}");
    let list = t.request_completions(&unit, "syserr");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("syserr", s(&item["label"]));
    assert_eq!("System.err.println(${0});", s(&item["insertText"]));
}

#[test]
fn test_snippet_serr() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tserr\t}\n}");
    let list = t.request_completions(&unit, "serr");
    assert!(!list.is_null());
    let item = find_snippet(&list, "serr").expect("Failed to find snippet: 'serr'.");
    assert_eq!("System.err.println(${0});", s(&item["insertText"]));
}

#[test]
fn test_snippet_systrace() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsystrace\t}\n}");
    let list = t.request_completions(&unit, "systrace");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("systrace", s(&item["label"]));
    assert_eq!("System.out.println(\"${enclosing_type}.${enclosing_method}()\");", s(&item["insertText"]));
    assert_eq!("System.out.println(\"Test.testMethod()\");", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_soutm() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsoutm\t}\n}");
    let list = t.request_completions(&unit, "soutm");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("soutm", s(&item["label"]));
    assert_eq!("System.out.println(\"${enclosing_type}.${enclosing_method}()\");", s(&item["insertText"]));
    assert_eq!("System.out.println(\"Test.testMethod()\");", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_array_foreach() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] args) {\n\t\tforeach\t}\n}");
    let list = t.request_completions(&unit, "foreach");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("foreach", s(&item["label"]));
    assert_eq!("for (${1:iterable_type} ${2:iterable_element} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("for (${1:String} ${2:string} : ${3:args}) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_list_foreach() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\nimport java.util.List;\npublic class Test {\n\tpublic void testMethod(List<String> args) {\n\t\tforeach\t}\n}",
    );
    let list = t.request_completions(&unit, "foreach");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("foreach", s(&item["label"]));
    assert_eq!("for (${1:iterable_type} ${2:iterable_element} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("for (${1:String} ${2:string} : ${3:args}) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_list_iter() {
    let mut t = setup();
    let unit = t.get_working_copy(
        TEST_SOURCE_PATH,
        "package org.sample;\nimport java.util.List;\npublic class Test {\n\tpublic void testMethod(List<String> args) {\n\t\titer\t}\n}",
    );
    let list = t.request_completions(&unit, "iter");
    assert!(!list.is_null());
    let item = find_snippet(&list, "iter").expect("Failed to find snippet: 'iter'.");
    assert_eq!("for (${1:iterable_type} ${2:iterable_element} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("for (${1:String} ${2:string} : ${3:args}) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, &item));
}

#[test]
fn test_snippet_array_fori() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] args) {\n\t\tfori\t}\n}");
    let list = t.request_completions(&unit, "fori");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("fori", s(&item["label"]));
    assert_eq!(
        "for (${1:int} ${2:index} = ${3:0}; ${2:index} < ${4:array.length}; ${2:index}++) {\n\t$TM_SELECTED_TEXT${0}\n}",
        s(&item["insertText"])
    );
    assert_eq!(
        "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:args.length}; ${2:i}++) {\n\t$TM_SELECTED_TEXT${0}\n}",
        resolved_text(&mut t, item)
    );
}

#[test]
fn test_snippet_while() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean con) {\n\t\twhile\t}\n}");
    let list = t.request_completions(&unit, "while");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("while", s(&item["label"]));
    assert!(item["labelDetails"]["detail"].is_null());
    assert_eq!("while statement", s(&item["labelDetails"]["description"]));
    assert_eq!("while (${1:condition:var(boolean)}) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("while (${1:con}) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_dowhile() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean con) {\n\t\tdowhile\t}\n}");
    let list = t.request_completions(&unit, "dowhile");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("dowhile", s(&item["label"]));
    assert_eq!("do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:condition:var(boolean)});", s(&item["insertText"]));
    assert_eq!("do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:con});", resolved_text(&mut t, item));
}

/// Upstream sets the system property `jdt.codeCompleteSubstringMatch` to
/// `true` in the test JVM for this test.
#[test]
fn test_snippet_if() {
    let mut t = setup();
    t.ws.oracle_java_options.push("-Djdt.codeCompleteSubstringMatch=true".into());
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean con) {\n\t\tif\t}\n}");
    let list = t.request_completions(&unit, "if");
    assert!(!list.is_null());
    let has_if_snippet = items(&list).iter().any(|item| {
        item["label"] == "if" && item["insertText"] == "if (${1:condition:var(boolean)}) {\n\t$TM_SELECTED_TEXT${0}\n}"
    });
    assert!(has_if_snippet, "{list:#}");
}

#[test]
fn test_snippet_ifelse() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean con) {\n\t\tifelse\t}\n}");
    let list = t.request_completions(&unit, "ifelse");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("ifelse", s(&item["label"]));
    assert_eq!("if (${1:condition:var(boolean)}) {\n\t${2}\n} else {\n\t${0}\n}", s(&item["insertText"]));
    assert_eq!("if (${1:con}) {\n\t${2}\n} else {\n\t${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_ifnull() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(Object obj) {\n\t\tifnull\t}\n}");
    let list = t.request_completions(&unit, "ifnull");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("ifnull", s(&item["label"]));
    assert_eq!("if (${1:name:var} == null) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("if (${1:obj} == null) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_snippet_ifnotnull() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(Object obj) {\n\t\tifnotnull\t}\n}");
    let list = t.request_completions(&unit, "ifnotnull");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("ifnotnull", s(&item["label"]));
    assert_eq!("if (${1:name:var} != null) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["insertText"]));
    assert_eq!("if (${1:obj} != null) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

/// `mockClientPreferences(true, true, true)` replaces the client preferences
/// (label details off) and stubs `getCompletionItemInsertTextModeDefault`.
#[test]
fn test_snippet_while_item_defaults_enabled_generic_snippets() {
    let mut t = setup();
    t.caps = Caps::mock(true, true, true);
    t.caps.insert_text_mode_default = Some(MODE_AS_IS as u32);
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(boolean con) {\n\t\twhile\t}\n}");
    let list = t.request_completions(&unit, "while");
    assert!(!list.is_null());
    let defaults = &list["itemDefaults"];
    assert!(!defaults["editRange"].is_null(), "{list:#}");
    assert_eq!(FORMAT_SNIPPET, defaults["insertTextFormat"]);
    assert_eq!(MODE_ADJUST_INDENTATION, defaults["insertTextMode"]);

    let item = &items(&list)[0];
    assert_eq!("while", s(&item["label"]));
    assert_eq!("while (${1:condition:var(boolean)}) {\n\t$TM_SELECTED_TEXT${0}\n}", s(&item["textEditText"]));
    // check that the fields covered by itemDefaults are set to null
    assert!(item["textEdit"].is_null());
    assert!(item["insertTextFormat"].is_null());
    assert!(item["insertTextMode"].is_null());

    assert_eq!("while (${1:con}) {\n\t$TM_SELECTED_TEXT${0}\n}", resolved_text(&mut t, item));
}

#[test]
fn test_constructor_completion() {
    let mut t = setup();
    let unit = t.get_working_copy(TEST_SOURCE_PATH, "package org.sample;\npublic class Test {\n\tpublic void testMethod(String[] args) {\n\t\tnew String\t}\n}");
    let list = t.request_completions(&unit, "new String");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("String", s(&item["textEdit"]["newText"]));
}
