//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionHandlerChainTest`.
//!
//! `setUp` turns postfix completion off and chain completion on, and
//! `increaseChainCompletionTimeout` sets the project preference
//! `recommenders.chain.timeout` to 5 (written to the project's
//! `org.eclipse.jdt.ls.core` preference node before the server starts).
//! `mockLSP3Client()` stubs snippet support. Like upstream, the project runs
//! on the rtstubs test JDK. Completion requests carry
//! `context.triggerKind = 1` like upstream's request template.

mod common;
use common::completion::*;
use serde_json::{json, Value};

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionHandlerChainTest.setUp`.
fn setup() -> T {
    let mut settings = settings_with(false, false);
    settings["java"]["completion"]["chain"] = json!({ "enabled": true });
    let mut t = setup_with(settings);
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { snippets: true, ..Default::default() };
    increase_chain_completion_timeout(&t);
    t
}

/// `increaseChainCompletionTimeout`: if they don't finish within 5secs we might have a performance issue.
fn increase_chain_completion_timeout(t: &T) {
    let settings = t.ws.project_root("hello").join(".settings");
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(settings.join("org.eclipse.jdt.ls.core.prefs"), "eclipse.preferences.version=1\nrecommenders.chain.timeout=5\n").unwrap();
}

fn request_completions(t: &mut T, unit: &Unit, behind: &str) -> Value {
    let (line, character) = find_completion_location(&unit.text, behind, 0);
    t.ws.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": unit.uri },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": 1 }
        }),
    )
}

fn labels_containing(list: &Value, part: &str) -> Vec<Value> {
    items(list).into_iter().filter(|i| s(&i["label"]).contains(part)).collect()
}

fn first_label_starting(list: &Value, prefix: &str) -> Option<Value> {
    items(list).into_iter().find(|i| s(&i["label"]).starts_with(prefix))
}

#[test]
fn test_chain_completions_on_parameter() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.stream.Stream;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tStream.of(\"1\").collect()\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "collect(");
    let completion_items = labels_containing(&list, "toList");
    assert_eq!(1, completion_items.len(), "toList completion count: {list:#}");

    let completion_item = &completion_items[0];
    assert_eq!("Collectors.toList()", s(&completion_item["textEdit"]["newText"]), "Completion getTextEditText");
    assert_eq!("Collectors.toList() : Collector<T,?,List<T>>", s(&completion_item["label"]), "Completion Label");
    assert_eq!(
        "java.util.stream.Collectors.Collectors.toList() : Collector<T,?,List<T>>",
        s(&completion_item["detail"]),
        "Completion Details"
    );
    let edits = completion_item["additionalTextEdits"].as_array().expect("additional edits");
    assert_eq!(1, edits.len(), "Additional edits count");
    assert_eq!("import java.util.stream.Collectors;\n", s(&edits[0]["newText"]), "Import");
}

#[test]
fn test_chain_completions_on_variable() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names =\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names =");
    let completion_items = labels_containing(&list, "emptyList");
    assert_eq!(1, completion_items.len(), "emptyList completion count: {list:#}");

    let completion_item = &completion_items[0];
    assert_eq!("Collections.emptyList()", s(&completion_item["textEdit"]["newText"]), "Completion getTextEditText");
    let edits = completion_item["additionalTextEdits"].as_array().expect("additional edits");
    assert_eq!(1, edits.len(), "Additional edits count");
    assert_eq!("import java.util.Collections;\n", s(&edits[0]["newText"]), "Import");
}

#[test]
fn test_chain_completions_on_variable_with_new_keyword_expect_no_chains() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names = new\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names = new");
    let completion_items: Vec<Value> = items(&list).into_iter().filter(|i| s(&i["label"]).ends_with("emptyList() <T>")).collect();
    assert_eq!(0, completion_items.len(), "emptyList completion count");
}

#[test]
fn test_chain_completions_on_variable_completing_constructor_expect_no_chains() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names = new Arr\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names = new Arr");
    let completion_items: Vec<Value> = items(&list).into_iter().filter(|i| s(&i["label"]).ends_with("emptyList() <T>")).collect();
    assert_eq!(0, completion_items.len(), "emptyList completion count");
}

#[test]
fn test_chain_completions_on_chains_from_visible_variables() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n\tpublic class Stream {\n\t\tpublic List<String> toList() {\n\t\t\treturn null;\n\t\t}\n\t}\n\n    public static void main(String[] args) {\n\t\tStream stream = new Stream();\n\t\tStream[] streams = new Stream[0];\n\t\tList<String> names =\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names =");
    let item = first_label_starting(&list, "stream.").unwrap_or_else(|| panic!("completion: {list:#}"));
    assert_eq!("stream.toList() : List<String>", s(&item["label"]), "completion label");
    assert_eq!("stream.toList()", s(&item["textEdit"]["newText"]), "completion edit text");

    let item = first_label_starting(&list, "streams[").unwrap_or_else(|| panic!("array completion: {list:#}"));
    assert_eq!("streams[].toList() : List<String>", s(&item["label"]), "array completion label");
    assert_eq!("streams[${1:i}].toList()", s(&item["textEdit"]["newText"]), "array completion edit text");
}

#[test]
fn test_chain_completions_on_chains_correct_snippet_placeholders() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n\tpublic class Stream {\n\t\tpublic List<String> toList(int size) {\n\t\t\treturn null;\n\t\t}\n\t}\n\n    public static void main(String[] args) {\n\t\tStream stream = new Stream();\n\t\tStream[] streams = new Stream[0];\n\t\tList<String> names =\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names =");
    let item = first_label_starting(&list, "streams[].").unwrap_or_else(|| panic!("completion: {list:#}"));
    assert_eq!("streams[].toList(int size) : List<String>", s(&item["label"]), "completion label");
    assert_eq!("streams[${1:i}].toList(${2:size})", s(&item["textEdit"]["newText"]), "completion edit text");
}

#[test]
fn test_chain_completions_on_primitive_variable_expect_no_completions() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static boo(IntChain chain) {\n\t\tInteger variable = ;\n    }\n\n\tstatic class IntChain {\n\t\tpublic Integer newInt() {\n\t\t\treturn 1;\n\t\t}\n\t}\n}\n",
    );
    let list = request_completions(&mut t, &unit, "variable = ");
    assert_eq!(0, labels_containing(&list, "newInt").len(), "emptyList completion count");
}

#[test]
fn test_chain_completions_on_string_variable_expect_no_completions() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static boo(StringChain chain) {\n\t\tString variable = //\n\t\t\"variable\".concat(\"\");\n    }\n\n\tstatic class StringChain {\n\t\tpublic String newString() {\n\t\t\treturn \"\";\n\t\t}\n\t}\n}\n",
    );
    let list = request_completions(&mut t, &unit, "variable = ");
    assert_eq!(0, labels_containing(&list, "newString").len(), "emptyList completion count [binding]");

    let list = request_completions(&mut t, &unit, "\"variable\".concat(");
    assert_eq!(0, labels_containing(&list, "newString").len(), "emptyList completion count [type]");
}

#[test]
fn test_chain_completions_on_object_variable_expect_no_completions() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static boo(ObjectChain chain) {\n\t\tObject variable = //\n\t\tchain.equals(variable);\n    }\n\n\tstatic class ObjectChain {\n\t\tpublic Object newObject() {\n\t\t\treturn new Object();\n\t\t}\n\t}\n}\n",
    );
    let list = request_completions(&mut t, &unit, "variable = ");
    assert_eq!(0, labels_containing(&list, "newObject").len(), "emptyList completion count");

    let list = request_completions(&mut t, &unit, "chain.equals(");
    assert_eq!(0, labels_containing(&list, "newObject").len(), "emptyList completion count [type]");
}

#[test]
fn test_chain_completions_with_token_expect_replace_token() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names = empty\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names = empty");
    let completion_items = labels_containing(&list, "emptyList");
    assert_eq!(1, completion_items.len(), "emptyList completion count: {list:#}");

    let completion_item = &completion_items[0];
    assert_eq!("Collections.emptyList()", s(&completion_item["textEdit"]["newText"]), "Completion getTextEditText");
    let (line, character) = find_completion_location(&unit.text, "names = empty", 0);
    assert_position(line as u64, (character - "empty".len() as u32) as u64, &completion_item["textEdit"]["range"]["start"]);
}

#[test]
fn test_chain_completions_on_variable_with_token_matching_edge() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names = emptyL\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names = emptyL");
    assert_eq!(2, items(&list).len(), "{list:#}");
    assert!(items(&list).iter().any(|i| regex_full_match(r".*\.emptyList().*", s(&i["label"]))), "emptyList");
    assert!(items(&list).iter().any(|i| regex_full_match(r".*\.EMPTY_LIST.*", s(&i["label"]))), "EMPTY_LIST");
}

#[test]
fn test_chain_completions_on_variable_with_token_matching_start() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\npublic class Foo {\n    public static void main(String[] args) {\n\t\tList<String> names = Coll\n    }\n}\n",
    );
    let list = request_completions(&mut t, &unit, "names = Coll");
    assert!(!items(&list).is_empty());
    assert!(items(&list).iter().any(|i| regex_full_match(r"Collections\..*", s(&i["label"]))), "All Collections.*");
}
