//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionHandlerTest`.
//!
//! `setup()` imports `eclipse/hello` with the test preferences of
//! `AbstractProjectsManagerBasedTest.initPreferences`
//! (`codeGeneration.generateComments = true`) and the `setUp` of the test
//! class (`postfix` completion and lazy text-edit resolution off).  The
//! mocked `ClientPreferences` become LSP client capabilities ([`Caps`]): a
//! Mockito mock answers `false` for everything not stubbed, so each test
//! starts from the capabilities `mockLSP3Client()` stubs (snippets and
//! signature help) and adds what the test stubs.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};

// ─── Client capabilities (mocked ClientPreferences) ─────────────────────────

#[derive(Clone, Default)]
struct Caps {
    snippets: bool,
    signature_help: bool,
    label_details: bool,
    resolve_documentation: bool,
    resolve_additional_text_edits: bool,
    markdown: bool,
    insert_replace: bool,
    item_defaults: Vec<&'static str>,
    insert_text_mode_adjust_indentation: bool,
    /// `getCompletionItemInsertTextModeDefault`: 1 AsIs, 2 AdjustIndentation.
    insert_text_mode_default: Option<u32>,
    tag_support: bool,
}

impl Caps {
    /// `mockLSP3Client()`.
    fn lsp3() -> Self {
        Caps { snippets: true, signature_help: true, ..Default::default() }
    }
    /// `mockLSP2Client()`.
    fn lsp2() -> Self {
        Caps::default()
    }
    /// `mockClientPreferences(snippets, signatureHelp, itemDefaults)`.
    fn mock(snippets: bool, signature_help: bool, item_defaults: bool) -> Self {
        Caps {
            snippets,
            signature_help,
            item_defaults: if item_defaults { vec!["editRange", "insertTextFormat", "insertTextMode"] } else { vec![] },
            insert_text_mode_adjust_indentation: true,
            ..Default::default()
        }
    }

    fn to_json(&self) -> Value {
        let mut caps = default_client_capabilities();
        let mut props = Vec::new();
        if self.resolve_documentation {
            props.push("documentation");
        }
        if self.resolve_additional_text_edits {
            props.push("additionalTextEdits");
        }
        let mut item = json!({
            "snippetSupport": self.snippets,
            "labelDetailsSupport": self.label_details,
            "insertReplaceSupport": self.insert_replace,
            "documentationFormat": if self.markdown { json!(["markdown", "plaintext"]) } else { json!(["plaintext"]) },
            "resolveSupport": { "properties": props },
        });
        if self.insert_text_mode_adjust_indentation {
            item["insertTextModeSupport"] = json!({ "valueSet": [1, 2] });
        }
        if self.tag_support {
            item["tagSupport"] = json!({ "valueSet": [1] });
        }
        let mut completion = json!({ "completionItem": item });
        if !self.item_defaults.is_empty() {
            completion["completionList"] = json!({ "itemDefaults": self.item_defaults });
        }
        if let Some(m) = self.insert_text_mode_default {
            completion["insertTextMode"] = json!(m);
        }
        caps["textDocument"]["completion"] = completion;
        if !self.signature_help {
            caps["textDocument"].as_object_mut().unwrap().remove("signatureHelp");
        }
        caps
    }
}

// ─── Fixture ─────────────────────────────────────────────────────────────────

struct T {
    ws: Workspace,
    caps: Caps,
    /// `project`: the project `getWorkingCopy` paths are relative to.
    project: &'static str,
}

fn settings() -> Value {
    json!({
        "java": {
            "completion": { "postfix": { "enabled": false }, "lazyResolveTextEdit": { "enabled": false } },
            "codeGeneration": { "generateComments": true },
            // Preserve the code-template store used by the upstream mocked
            // PreferenceManager; a real configuration update clears it.
            "templates": { "typeComment": ["/**", " * ${type_name}", " * ${tags}", " */"] },
            "format": { "insertSpaces": false, "tabSize": 4 },
            "maven": { "defaultMojoExecutionAction": "ignore" }
        }
    })
}

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionHandlerTest.setUp`.
fn setup() -> T {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.settings = settings();
    T { ws, caps: Caps::lsp3(), project: "hello" }
}

#[derive(Clone)]
struct Unit {
    uri: String,
    text: String,
}

/// `findCompletionLocation`: after the last (or first after `from`) occurrence.
fn find_completion_location(text: &str, behind: &str, from: usize) -> (u32, u32) {
    let idx = if from > 0 { from + text[from..].find(behind).unwrap() } else { text.rfind(behind).unwrap_or_else(|| panic!("{behind:?} not in source")) };
    let offset = idx + behind.len();
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = text[line_start..offset].encode_utf16().count() as u32;
    (line, character)
}

impl T {
    fn start(&mut self) {
        self.ws.capabilities = self.caps.to_json();
        self.ws.client();
    }

    /// `getWorkingCopy(path, source)`: the unit `path` of project `hello` with `source`.
    fn get_working_copy(&mut self, path: &str, source: &str) -> Unit {
        let root = self.ws.project_root(self.project);
        let uri = url::Url::from_file_path(root.join(path)).unwrap().to_string();
        self.ws.capabilities = self.caps.to_json();
        self.ws.open_with(&uri, source);
        Unit { uri, text: source.to_owned() }
    }

    /// `getWorkingCopy` of an existing unit `uri`.
    fn get_working_copy_uri(&mut self, uri: &str, source: &str) -> Unit {
        self.ws.capabilities = self.caps.to_json();
        self.ws.open_with(uri, source);
        Unit { uri: uri.to_owned(), text: source.to_owned() }
    }

    fn change(&mut self, unit: &mut Unit, text: &str) {
        self.ws.change(&unit.uri, text);
        unit.text = text.to_owned();
    }

    fn completion_at(&mut self, uri: &str, line: u32, character: u32) -> Value {
        self.ws.request(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        )
    }

    fn request_completions(&mut self, unit: &Unit, behind: &str) -> Value {
        self.request_completions_from(unit, behind, 0)
    }

    fn request_completions_from(&mut self, unit: &Unit, behind: &str, from: usize) -> Value {
        let (line, character) = find_completion_location(&unit.text, behind, from);
        self.completion_at(&unit.uri, line, character)
    }

    fn resolve(&mut self, item: &Value) -> Value {
        self.ws.request("completionItem/resolve", item.clone())
    }

    fn set_preference(&mut self, path: &[&str], value: Value) {
        let mut v = &mut self.ws.settings;
        for p in &path[..path.len() - 1] {
            if v.get(*p).is_none() {
                v[*p] = json!({});
            }
            v = v.get_mut(*p).unwrap();
        }
        v[path[path.len() - 1]] = value;
        if self.ws.client_started() {
            let s = self.ws.settings.clone();
            self.ws.client().notify("workspace/didChangeConfiguration", json!({ "settings": s }));
            self.ws.wait_idle();
        }
    }
}

fn items(list: &Value) -> Vec<Value> {
    list["items"].as_array().cloned().unwrap_or_default()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

/// `Lsp4jAssertions.assertTextEdit`.
fn assert_text_edit(line: u64, start: u64, end: u64, text: &str, edit: &Value) {
    assert!(!edit.is_null(), "no text edit");
    assert_eq!(text, s(&edit["newText"]), "{edit:#}");
    assert_eq!(line, edit["range"]["start"]["line"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(start, edit["range"]["start"]["character"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(line, edit["range"]["end"]["line"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(end, edit["range"]["end"]["character"].as_u64().unwrap(), "{edit:#}");
}

fn assert_position(line: u64, character: u64, pos: &Value) {
    assert_eq!(line, pos["line"].as_u64().unwrap(), "{pos}");
    assert_eq!(character, pos["character"].as_u64().unwrap(), "{pos}");
}

const KIND_TEXT: u64 = 1;
const KIND_METHOD: u64 = 2;
const KIND_CONSTRUCTOR: u64 = 4;
const KIND_FIELD: u64 = 5;
const KIND_VARIABLE: u64 = 6;
const KIND_CLASS: u64 = 7;
const KIND_INTERFACE: u64 = 8;
const KIND_MODULE: u64 = 9;
const KIND_PROPERTY: u64 = 10;
const KIND_ENUM: u64 = 13;
const KIND_KEYWORD: u64 = 14;
const KIND_SNIPPET: u64 = 15;
const KIND_ENUM_MEMBER: u64 = 20;
const KIND_CONSTANT: u64 = 21;
const KIND_STRUCT: u64 = 22;
const FORMAT_PLAIN_TEXT: u64 = 1;
const FORMAT_SNIPPET: u64 = 2;
const MODE_AS_IS: u64 = 1;
const MODE_ADJUST_INDENTATION: u64 = 2;

fn regex_full_match(pattern: &str, input: &str) -> bool {
    regex::Regex::new(&format!("^(?:{pattern})$")).unwrap().is_match(input)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// `testCompletion_javadoc`: a fresh `ClientPreferences` mock with only
/// `isCompletionResolveDocumentSupport`.
#[test]
fn test_completion_javadoc() {
    let mut t = setup();
    t.ws.oracle_java_options.push("-Djava.lsp.joinOnCompletion=true".into());
    t.caps = Caps { resolve_documentation: true, ..Default::default() };
    let uri = t.ws.class_uri("hello", "org.sample.TestJavadoc");
    let source = t.ws.read(&uri);
    let mut unit = t.get_working_copy_uri(&uri, &source);
    let loc = find_completion_location(&unit.text, "inner.", 0);
    t.change(&mut unit, &source);
    t.change(&mut unit, &source);
    let list = t.completion_at(&unit.uri, loc.0, loc.1);
    let resolved = t.resolve(&items(&list)[0]);
    assert_eq!("Test", s(&resolved["documentation"]));
}

#[test]
fn test_completion_javadoc_markdown() {
    let mut t = setup();
    t.ws.oracle_java_options.push("-Djava.lsp.joinOnCompletion=true".into());
    t.caps = Caps { resolve_documentation: true, markdown: true, ..Default::default() };
    let uri = t.ws.class_uri("hello", "org.sample.TestJavadoc");
    let source = t.ws.read(&uri);
    let mut unit = t.get_working_copy_uri(&uri, &source);
    let loc = find_completion_location(&unit.text, "inner.", 0);
    t.change(&mut unit, &source);
    t.change(&mut unit, &source);
    let list = t.completion_at(&unit.uri, loc.0, loc.1);
    let resolved = t.resolve(&items(&list)[0]);
    let markup = &resolved["documentation"];
    assert!(markup.is_object(), "{resolved:#}");
    assert_eq!("markdown", s(&markup["kind"]));
    assert_eq!("Test", s(&markup["value"]));
}

#[test]
fn test_completion_nojavadoc() {
    let mut t = setup();
    t.caps = Caps { markdown: true, ..Default::default() };
    let uri = t.ws.class_uri("hello", "org.sample.Foo5");
    let source = t.ws.read(&uri);
    let unit = t.get_working_copy_uri(&uri, &source);
    let list = t.request_completions(&unit, "nam");
    let resolved = t.resolve(&items(&list)[0]);
    assert!(resolved.get("documentation").is_none_or(Value::is_null), "{resolved:#}");
}

#[test]
fn test_completion_object() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n\t\tObjec\n\t}\n}\n");
    let list = t.request_completions(&unit, "Objec");
    assert!(!list.is_null());
    let items = items(&list);
    assert!(!items.is_empty(), "No proposals were found");
    for item in &items {
        assert!(!s(&item["label"]).trim().is_empty());
        assert!(!item["kind"].is_null());
        assert!(!s(&item["sortText"]).trim().is_empty());
        //text edits are set during calls to "completion"
        assert!(!item["textEdit"].is_null(), "{item:#}");
        assert!(!s(&item["insertText"]).trim().is_empty(), "{item:#}");
        assert!(!item["filterText"].is_null());
        assert!(!s(&item["filterText"]).contains(' '));
        assert!(s(&item["label"]).starts_with(s(&item["insertText"])), "{item:#}");
        assert!(s(&item["filterText"]).contains("Objec"));
        //Check contains data used for completionItem resolution
        let data = &item["data"];
        assert!(!data.is_null());
        assert!(!s(&data["pid"]).trim().is_empty());
        assert!(!s(&data["rid"]).trim().is_empty());
    }
}

#[test]
fn test_completion_constructor() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n\t\tObject o = new O\n\t}\n}\n");
    let list = t.request_completions(&unit, "new O");
    assert!(!items(&list).is_empty(), "No proposals were found");
    let mut items = items(&list);
    items.sort_by(|a, b| s(&a["sortText"]).cmp(s(&b["sortText"])));
    let ctor = &items[0];
    assert_eq!("Object", s(&ctor["label"]));
    // createMethodProposalLabel
    assert_eq!("()", s(&ctor["labelDetails"]["detail"]));
    assert!(ctor["labelDetails"]["description"].is_null());
    assert_eq!("java.lang.Object.Object()", s(&ctor["detail"]));
    assert_eq!("Object", s(&ctor["insertText"]));

    let resolved = t.resolve(ctor);
    let te = &resolved["textEdit"];
    assert!(!te.is_null());
    assert_eq!("Object()", s(&te["newText"]));
    let range = &te["range"];
    assert_eq!(2, range["start"]["line"]);
    assert_eq!(17, range["start"]["character"]);
    assert_eq!(2, range["end"]["line"]);
    assert_eq!(18, range["end"]["character"]);
}

#[test]
fn test_completion_import_package() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/java/Foo.java", "import java.sq \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n");
    let list = t.request_completions(&unit, "java.sq");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    // Check completion item
    assert!(item["insertText"].is_null());
    assert_eq!("java.sql", s(&item["label"]));
    // createPackageProposalLabel
    assert!(item["labelDetails"]["detail"].is_null());
    assert_eq!("(package)", s(&item["labelDetails"]["description"]));
    assert_eq!("(package) java.sql", s(&item["detail"]));
    assert_eq!(KIND_MODULE, item["kind"]);
    assert_eq!("999999215", s(&item["sortText"]));
    let te = &item["textEdit"];
    assert!(!te.is_null());
    assert_eq!("java.sql.${0:*};", s(&te["newText"]));
    let range = &te["range"];
    assert_eq!(0, range["start"]["line"]);
    assert_eq!(7, range["start"]["character"]);
    assert_eq!(0, range["end"]["line"]);
    //Not checking the range end character
}

#[test]
fn test_completion_javadoc_comment() {
    let mut t = setup();
    t.caps = Caps::mock(true, true, true);
    t.caps.insert_text_mode_default = Some(2);
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\t/** */ \n\tvoid foo(int i, String s) {\n\t}\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert!(item["insertText"].is_null());
    assert_eq!("Javadoc comment", s(&item["label"]));
    assert_eq!(KIND_SNIPPET, item["kind"]);
    assert_eq!("999999999", s(&item["sortText"]));
    assert_eq!(FORMAT_SNIPPET, item["insertTextFormat"]);
    assert!(item["insertTextMode"].is_null());
    assert!(!item["textEdit"].is_null());
    assert_eq!("\n * ${0}\n * @param i\n * @param s\n", s(&item["textEdit"]["newText"]));
    let range = &item["textEdit"]["range"];
    assert_eq!(1, range["start"]["line"]);
    assert_eq!(4, range["start"]["character"]);
    assert_eq!(1, range["end"]["line"]);
    assert_eq!(" * @param i\n * @param s\n", s(&item["documentation"]));
}

#[test]
fn test_completion_javadoc_comment_no_snippet() {
    let mut t = setup();
    t.caps = Caps { snippets: false, insert_text_mode_adjust_indentation: true, insert_text_mode_default: Some(1), ..Default::default() };
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\t/** */ \n\tvoid foo(int i, String s) {\n\t}\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert!(item["insertText"].is_null());
    assert_eq!("Javadoc comment", s(&item["label"]));
    assert_eq!(KIND_SNIPPET, item["kind"]);
    assert_eq!("999999999", s(&item["sortText"]));
    assert_eq!(FORMAT_PLAIN_TEXT, item["insertTextFormat"]);
    assert_eq!(MODE_ADJUST_INDENTATION, item["insertTextMode"]);
    assert!(!item["textEdit"].is_null());
    assert_eq!("\n * @param i\n * @param s\n", s(&item["textEdit"]["newText"]));
    let range = &item["textEdit"]["range"];
    assert_eq!(1, range["start"]["line"]);
    assert_eq!(4, range["start"]["character"]);
    assert_eq!(1, range["end"]["line"]);
    assert_eq!(" * @param i\n * @param s\n", s(&item["documentation"]));
}

#[test]
fn test_completion_javadoc_comment_partial() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\t/** \n\t * @int \n\t*/ \n\tvoid foo(int i, String s) {\n\t}\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(0, items(&list).len(), "{list:#}");
}

#[test]
fn test_completion_javadoc_comment_regular() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\t/* */ \n\tvoid foo(int i, String s) {\n\t}\n}\n");
    let list = t.request_completions(&unit, "/*");
    assert_eq!(0, items(&list).len(), "{list:#}");
}

#[test]
fn test_completion_javadoc_comment_no_param() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\t/** */ \n\tvoid foo() {\n\t}\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(0, items(&list).len(), "{list:#}");
}

fn java16_unit(t: &mut T, source: &str) -> Unit {
    t.ws.import_projects(&["eclipse/java16"]);
    let uri = t.ws.class_uri("java16", "foo.bar.Foo");
    let original = t.ws.read(&uri);
    let mut unit = t.get_working_copy_uri(&uri, &original);
    t.change(&mut unit, source);
    unit
}

#[test]
fn test_completion_javadoc_comment_record() {
    let mut t = setup();
    let unit = java16_unit(&mut t, "package foo.bar;\n/** */ \npublic record Foo(String name, int age) {\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert!(item["insertText"].is_null());
    assert_eq!("Javadoc comment", s(&item["label"]));
    assert_eq!(KIND_SNIPPET, item["kind"]);
    assert_eq!("999999999", s(&item["sortText"]));
    assert_eq!(FORMAT_SNIPPET, item["insertTextFormat"]);
    assert!(!item["textEdit"].is_null());
    assert_eq!("\n * ${0}\n * Foo\n * @param name\n * @param age\n", s(&item["textEdit"]["newText"]));
    let range = &item["textEdit"]["range"];
    assert_eq!(1, range["start"]["line"]);
    assert_eq!(3, range["start"]["character"]);
    assert_eq!(1, range["end"]["line"]);
    assert_eq!(" * Foo\n * @param name\n * @param age\n", s(&item["documentation"]));
}

#[test]
fn test_completion_javadoc_comment_record_no_snippet() {
    let mut t = setup();
    t.caps = Caps::default();
    let unit = java16_unit(&mut t, "package foo.bar;\n/** */ \npublic record Foo(String name, int age) {\n}\n");
    let list = t.request_completions(&unit, "/**");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert!(item["insertText"].is_null());
    assert_eq!("Javadoc comment", s(&item["label"]));
    assert_eq!(KIND_SNIPPET, item["kind"]);
    assert_eq!("999999999", s(&item["sortText"]));
    assert_eq!(FORMAT_PLAIN_TEXT, item["insertTextFormat"]);
    assert!(!item["textEdit"].is_null());
    assert_eq!("\n * Foo\n * @param name\n * @param age\n", s(&item["textEdit"]["newText"]));
    let range = &item["textEdit"]["range"];
    assert_eq!(1, range["start"]["line"]);
    assert_eq!(3, range["start"]["character"]);
    assert_eq!(1, range["end"]["line"]);
    assert_eq!(" * Foo\n * @param name\n * @param age\n", s(&item["documentation"]));
}

#[test]
fn test_completion_import_static() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/java/Foo.java", "import static java.util.concurrent.TimeUnit. \npublic class Foo {\n\tvoid foo() {\n\t}\n}\n");
    let list = t.request_completions(&unit, "java.util.concurrent.TimeUnit.");
    let items = items(&list);
    assert_eq!(9, items.len(), "{list:#}");

    //// .DAYS - enum value
    let days = &items[0];
    // Check completion item
    assert_eq!("DAYS", s(&days["insertText"]));
    // createLabelWithTypeAndDeclaration
    assert_eq!("DAYS", s(&days["label"]));
    assert_eq!("TimeUnit", s(&days["labelDetails"]["description"]));
    assert!(days["labelDetails"]["detail"].is_null());
    assert_eq!(KIND_ENUM_MEMBER, days["kind"]);
    assert_eq!("999999210", s(&days["sortText"]));
    let te = &days["textEdit"];
    assert_eq!("DAYS;", s(&te["newText"]));
    assert_eq!(0, te["range"]["start"]["line"]);
    assert_eq!(44, te["range"]["start"]["character"]);
    assert_eq!(0, te["range"]["end"]["line"]);

    //Check other fields are listed alphabetically
    assert_eq!("HOURS;", s(&items[1]["textEdit"]["newText"]));
    assert_eq!("MICROSECONDS;", s(&items[2]["textEdit"]["newText"]));
    assert_eq!("MILLISECONDS;", s(&items[3]["textEdit"]["newText"]));
    assert_eq!("MINUTES;", s(&items[4]["textEdit"]["newText"]));
    assert_eq!("NANOSECONDS;", s(&items[5]["textEdit"]["newText"]));
    assert_eq!("SECONDS;", s(&items[6]["textEdit"]["newText"]));

    //// .values() - static method
    let values = &items[7];
    assert_eq!("valueOf", s(&values["insertText"]));
    assert_eq!("valueOf", s(&values["label"]));
    assert_eq!("(String)", s(&values["labelDetails"]["detail"]));
    assert_eq!("TimeUnit", s(&values["labelDetails"]["description"]));
    assert_eq!(KIND_METHOD, values["kind"]);
    assert_eq!("999999211", s(&values["sortText"]));
    let te = &values["textEdit"];
    assert_eq!("valueOf;", s(&te["newText"]));
    assert_eq!(0, te["range"]["start"]["line"]);
    assert_eq!(44, te["range"]["start"]["character"]);
    assert_eq!(0, te["range"]["end"]["line"]);
}

const MAP_SOURCE: &str = "public class Foo {\n\tvoid foo() {\nSystem.out.print(\"Hello\");\nSystem.out.println(\" World!\");\nHashMap<String, String> map = new HashMap<>();\nmap.pu\n\t}\n}\n";

#[test]
fn test_completion_method_with_lspv2() {
    let mut t = setup();
    t.caps = Caps::lsp2();
    let unit = t.get_working_copy("src/java/Foo.java", MAP_SOURCE);
    let list = t.request_completions(&unit, "map.pu");
    let ci = items(&list)
        .into_iter()
        .find(|i| regex_full_match(r"put\(String \w+, String \w+\) : String", s(&i["label"])))
        .unwrap_or_else(|| panic!("no put in {list:#}"));
    assert_eq!("put", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999019", s(&ci["sortText"]));
    assert_text_edit(5, 4, 6, "put", &ci["textEdit"]);
    let edits = ci["additionalTextEdits"].as_array().unwrap_or_else(|| panic!("no additional edits in {ci:#}"));
    assert_eq!(2, edits.len(), "{ci:#}");
}

#[test]
fn test_completion_method_with_lspv3() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", MAP_SOURCE);
    let list = t.request_completions(&unit, "map.pu");
    let ci = items(&list)
        .into_iter()
        .find(|i| regex_full_match(r"put\(String \w+, String \w+\) : String", s(&i["label"])))
        .unwrap_or_else(|| panic!("no put in {list:#}"));
    assert_eq!("put", s(&ci["insertText"]));
    assert!(regex_full_match(r"java.util.HashMap.put\(String \w+, String \w+\) : String", s(&ci["detail"])), "{ci:#}");
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999019", s(&ci["sortText"]));
    let te = &ci["textEdit"];
    if s(&te["newText"]) == "put(${1:key}, ${2:value})" {
        assert_text_edit(5, 4, 6, "put(${1:key}, ${2:value})", te);
    } else {
        //In case the JDK has no sources
        assert_text_edit(5, 4, 6, "put(${1:arg0}, ${2:arg1})", te);
    }
    let edits = ci["additionalTextEdits"].as_array().unwrap_or_else(|| panic!("no additional edits in {ci:#}"));
    assert_eq!(2, edits.len());
}

const GUESS_SOURCE: &str = "public class Foo {\n\tstatic void test(String name, int i) {}\n\tpublic static void main(String[] args) {\n\t\tString str = \"x\";\n\t\tint  x = 0;\n\t\ttes\n\t}\n\n}\n";

fn guess_method_arguments(mode: &str, expected: &str) {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!(mode));
    let unit = t.get_working_copy("src/java/Foo.java", GUESS_SOURCE);
    let list = t.request_completions(&unit, "tes");
    let ci = items(&list).into_iter().find(|i| i["label"] == "test(String name, int i) : void").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("test", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999163", s(&ci["sortText"]));
    assert_text_edit(5, 2, 5, expected, &ci["textEdit"]);
}

#[test]
fn test_completion_method_insert_parameter_names() {
    guess_method_arguments("insertParameterNames", "test(${1:name}, ${2:i});");
}

#[test]
fn test_completion_method_insert_best_guessed_arguments() {
    guess_method_arguments("insertBestGuessedArguments", "test(${1:str}, ${2:x});");
}

#[test]
fn test_completion_method_guess_method_arguments2() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!("insertBestGuessedArguments"));
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n\tstatic void test(String name, int i) {}\n\tpublic static void main(String[] args) {\n\t\tString str = \"x\";\n\t\ttes\n\t}\n\n}\n",
    );
    let list = t.request_completions(&unit, "tes");
    let ci = items(&list).into_iter().find(|i| i["label"] == "test(String name, int i) : void").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("test", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999163", s(&ci["sortText"]));
    assert_text_edit(4, 2, 5, "test(${1:str}, ${2:0});", &ci["textEdit"]);
}

#[test]
fn test_completion_method_guess_method_arguments3() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!("insertBestGuessedArguments"));
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n\tstatic void test(int i, int j) {}\n\tpublic static void main(String[] args) {\n\t\tint one=1;\n\t\tint two=2;\n\t\ttes\n\t}\n\n}\n",
    );
    let list = t.request_completions(&unit, "tes");
    let ci = items(&list).into_iter().find(|i| i["label"] == "test(int i, int j) : void").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("test", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999163", s(&ci["sortText"]));
    assert_text_edit(5, 2, 5, "test(${1:one}, ${2:two});", &ci["textEdit"]);
}

#[test]
fn test_completion_method_guess_method_arguments_constructor() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!("insertBestGuessedArguments"));
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n\tpublic static void main(String[] args) {\n\t\tString str = \"x\";\n\t\tnew A\n\t}\n\tprivate static class A { public A(String name){} }\n}\n",
    );
    let list = t.request_completions(&unit, "new A");
    let ci = items(&list).into_iter().find(|i| i["label"] == "A(String name)").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("A", s(&ci["insertText"]));
    assert_eq!(KIND_CONSTRUCTOR, ci["kind"]);
    assert_eq!("999999051", s(&ci["sortText"]));
    assert_text_edit(3, 6, 7, "A(${1:str})", &ci["textEdit"]);
}

#[test]
fn test_completion_method_turn_off_guess_method_arguments() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!("off"));
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n\tstatic void test(int i, int j) {}\n\tpublic static void main(String[] args) {\n\t\tint one=1;\n\t\tint two=2;\n\t\ttes\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "tes");
    let ci = items(&list).into_iter().find(|i| i["label"] == "test(int i, int j) : void").unwrap_or_else(|| panic!("{list:#}"));
    assert_text_edit(5, 2, 5, "test(${0});", &ci["textEdit"]);
}

#[test]
fn test_completion_constructor_turn_off_guess_method_arguments() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "guessMethodArguments"], json!("off"));
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tpublic static void main(String[] args) {\n\t\tString s = new String\n\t}\n}\n");
    let list = t.request_completions(&unit, "new String");
    let ci = items(&list).into_iter().find(|i| i["label"] == "String - java.lang").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("String(${0})", s(&ci["textEdit"]["newText"]));
}

#[test]
fn test_completion_constructor_inner_class() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "import java.util.List;\nimport java.util.ArrayList;\npublic class Foo {\n\tpublic void test() {\n\t\tList<String> a = new MyC\n\t}\n\tpublic class MyClass {\n\t\tstatic class MyList<E> extends ArrayList<E> { }\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "new MyC");
    let ci = items(&list).into_iter().find(|i| i["label"] == "MyClass - java.Foo").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("java.Foo.MyClass", s(&ci["detail"]));
}

const FIELD_SOURCE: &str = "import java.sq \npublic class Foo {\nprivate String myTestString;\n\tvoid foo() {\n   this.myTestS\n\t}\n}\n";

#[test]
fn test_completion_field() {
    let mut t = setup();
    t.caps.insert_text_mode_default = Some(1);
    let unit = t.get_working_copy("src/java/Foo.java", FIELD_SOURCE);
    let list = t.request_completions(&unit, "this.myTestS");
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert_eq!(KIND_FIELD, item["kind"]);
    assert_eq!("myTestString", s(&item["insertText"]));
    assert_eq!("Foo.myTestString : String", s(&item["detail"]));
    assert_text_edit(4, 8, 15, "myTestString", &item["textEdit"]);
    assert_eq!(MODE_ADJUST_INDENTATION, item["insertTextMode"]);
}

#[test]
fn test_completion_field_item_defaults_enabled() {
    let mut t = setup();
    t.caps = Caps::mock(true, true, true);
    t.caps.insert_text_mode_default = Some(1);
    let unit = t.get_working_copy("src/java/Foo.java", FIELD_SOURCE);
    let list = t.request_completions(&unit, "this.myTestS");
    let defaults = &list["itemDefaults"];
    assert!(!defaults["editRange"].is_null(), "{list:#}");
    assert_eq!(FORMAT_SNIPPET, defaults["insertTextFormat"]);
    assert_eq!(MODE_ADJUST_INDENTATION, defaults["insertTextMode"]);
    assert!(!defaults["data"].is_null());
    let kinds = defaults["data"]["completionKinds"].as_array().unwrap();
    assert!(kinds.contains(&json!(2)), "FIELD_REF in {kinds:?}");

    assert_eq!(1, items(&list).len());
    let item = &items(&list)[0];
    assert_eq!(KIND_FIELD, item["kind"]);
    assert_eq!("myTestString", s(&item["insertText"]));
    assert_eq!("Foo.myTestString : String", s(&item["detail"]));
    //check that the fields covered by itemDefaults are set to null
    assert!(item["textEdit"].is_null());
    assert!(item["insertTextFormat"].is_null());
    assert!(item["insertTextMode"].is_null());
}

#[test]
fn test_completion_field_item_defaults_enabled_adjust_indentation() {
    let mut t = setup();
    t.caps = Caps::mock(true, true, true);
    t.caps.insert_text_mode_default = Some(2);
    let unit = t.get_working_copy("src/java/Foo.java", FIELD_SOURCE);
    let list = t.request_completions(&unit, "this.myTestS");
    assert!(list["itemDefaults"]["insertTextMode"].is_null(), "{list:#}");
    assert_eq!(1, items(&list).len());
    assert!(items(&list)[0]["insertTextMode"].is_null());
}

#[test]
fn test_completion_import_type() {
    let mut t = setup();
    t.caps.insert_text_mode_default = Some(2);
    let unit = t.get_working_copy("src/java/Foo.java", "import java.sq \npublic class Foo {\n\tvoid foo() {\n   java.util.Ma\n\t}\n}\n");
    let list = t.request_completions(&unit, "java.util.Ma");
    assert!(!items(&list).is_empty());
    let item = &items(&list)[0];
    assert_eq!(KIND_INTERFACE, item["kind"]);
    assert_eq!("Map", s(&item["insertText"]));
    assert_text_edit(3, 3, 15, "java.util.Map", &item["textEdit"]);
    assert!(s(&item["filterText"]).starts_with("java.util.Ma"));
    assert!(item["insertTextMode"].is_null());
}

#[test]
fn test_completion_no_package() {
    let mut t = setup();
    let unit = t.get_working_copy("src/NoPackage.java", "public class NoPackage {\n    NoP}\n");
    let list = t.request_completions(&unit, "    NoP");
    assert!(!items(&list).is_empty(), "No proposals were found");
    assert_eq!("NoPackage", s(&items(&list)[0]["label"]));
}

#[test]
fn test_completion_package() {
    let mut t = setup();
    t.caps = Caps::default();
    let unit = t.get_working_copy("src/org/sample/Baz.java", "package opublic class Baz {\n}\n");
    let list = t.request_completions(&unit, "package o");
    let mut items = items(&list);
    assert!(!items.is_empty());
    items.sort_by(|a, b| s(&a["sortText"]).cmp(s(&b["sortText"])));
    let item = &items[0];
    // current package should appear 1st
    assert_eq!("org.sample", s(&item["label"]));
    let resolved = t.resolve(item);
    assert!(!resolved.is_null());
    let te = &item["textEdit"];
    assert_eq!("org.sample", s(&te["newText"]));
    let range = &te["range"];
    assert_eq!(0, range["start"]["line"]);
    assert_eq!(8, range["start"]["character"]);
    assert_eq!(0, range["end"]["line"]);
    assert_eq!(15, range["end"]["character"]);
}

#[test]
fn test_skip_additional_edit_for_import() {
    let mut t = setup();
    let source = "package org.sample;\nimport public class Test {\n}";
    let mut unit = t.get_working_copy("src/org/sample/Test.java", source);
    // mock the user's input behavior
    let edited = "package org.sample;\nimport jpublic class Test {\n}";
    t.change(&mut unit, edited);
    let list = t.request_completions(&unit, "import j");
    let item = &items(&list)[0];
    let resolved = t.resolve(item);
    assert!(resolved["additionalTextEdits"].is_null(), "{resolved:#}");
}

#[test]
fn test_skip_additional_edit_for_import2() {
    let mut t = setup();
    let source = "package org.sample;\nimport public class Test {\n}";
    let mut unit = t.get_working_copy("src/org/sample/Test.java", source);
    let edited = "package org.sample;\nimport java.util.Arrpublic class Test {\n}";
    t.change(&mut unit, edited);
    let list = t.request_completions(&unit, "java.util.Arr");
    let item = &items(&list)[0];
    let resolved = t.resolve(item);
    assert!(resolved["additionalTextEdits"].is_null(), "{resolved:#}");
}

fn sorted(mut items: Vec<Value>) -> Vec<Value> {
    items.sort_by(|a, b| s(&a["sortText"]).cmp(s(&b["sortText"])));
    items
}

fn new_text(item: &Value) -> &str {
    s(&item["textEdit"]["newText"])
}

/// `importProjects("eclipse/records"); project = getProject("records")`.
fn use_records(t: &mut T) {
    t.ws.import_projects(&["eclipse/records"]);
    t.project = "records";
}

#[test]
fn test_snippet_non_lazy_resolve() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "lazyResolveTextEdit", "enabled"], json!(false));
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\tpublic void testMethod() {\n\t\tsysout\t}\n}",
    );
    let list = t.request_completions(&unit, "sysout");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("sysout", s(&item["label"]));
    assert_eq!("System.out.println(${0});", new_text(item));
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1800
#[test]
fn test_snippet_ifelse2() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\tprivate void test(String s, int i) {\n  if (i > 2) {\n  } else {\n    s.\n    System.out.println(\"b\");\n}\n}",
    );
    let list = t.request_completions(&unit, "s.");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1800
#[test]
fn test_snippet_if2() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "public class Test {\n  private boolean flag;\n  private void test(List<String> c) {\n    if (flag) {\n      \n      List<String> scs = c.subList(0, 1);\n    }\n  }\n  String test() {\n    return null;\n  } \n}",
    );
    let list = t.request_completions(&unit, "      ");
    assert!(!list.is_null());
    assert!(items(&list).len() > 1);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1811
#[test]
fn test_snippet_multiline_string() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n  public void test () {\n    String foo = \"\"\"\n    test1\n    test2\n    test3\n    \"\"\".;\n  }\n}",
    );
    let list = t.request_completions(&unit, "\".");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
}

#[test]
fn test_snippet_ctor() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class MainClass {\n}class AnotherClass {\nctor\n}");
    let list = t.request_completions(&unit, "ctor");
    assert!(!list.is_null());
    let item = &items(&list)[0];
    assert_eq!("ctor", s(&item["label"]));
    assert_eq!("${1|public,protected,private|} AnotherClass(${2}) {\n\t${3:super();}${0}\n}", new_text(item));
}

#[test]
fn test_snippet_interface() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "");
    let list = t.request_completions(&unit, "");
    assert!(!list.is_null());
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[1];
    assert_eq!("interface", s(&item["label"]));
    assert_eq!("package org.sample;\n\n/**\n * Test\n */\npublic interface Test {\n\n\t${0}\n}", dos2unix(s(&item["insertText"])));
    //check resolution doesn't blow up (https://github.com/eclipse/eclipse.jdt.ls/issues/675)
    assert_eq!(*item, t.resolve(item));
}

#[test]
fn test_snippet_interface_with_package() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n");
    let list = t.request_completions(&unit, "package org.sample;\n");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[1];
    assert_eq!("interface", s(&item["label"]));
    assert_eq!("/**\n * Test\n */\npublic interface Test {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_inner_interface() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic interface Test {}\n");
    let list = t.request_completions(&unit, "package org.sample;\npublic interface Test {}\n");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[1];
    assert_eq!("interface", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest}\n */\npublic interface ${1:InnerTest} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_sibling_inner_interface() {
    let mut t = setup();
    let src = "package org.sample;\npublic interface Test {}\npublic interface InnerTest{}\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[1];
    assert_eq!("interface", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic interface ${1:InnerTest_1} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_nested_inner_interface() {
    let mut t = setup();
    let src = "package org.sample;\npublic interface Test {}\npublic interface InnerTest{\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    let items = sorted(items(&list).into_iter().filter(|i| i["sortText"].is_string()).collect());
    assert!(!items.is_empty());
    let item = &items[15];
    assert_eq!("interface", s(&item["label"]), "{items:#?}");
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic interface ${1:InnerTest_1} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_nested_inner_interface_nosnippet() {
    let mut t = setup();
    t.caps = Caps::lsp2();
    let src = "package org.sample;\npublic interface Test {}\npublic interface InnerTest{\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|ci| ci["kind"] == KIND_SNIPPET), "No snippets should be returned");
}

#[test]
fn test_snippet_interface_method() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic interface Test {\nmethod\n}");
    let list = t.request_completions(&unit, "method");
    assert!(!list.is_null());
    let items = items(&list);
    let item_one = &items[6];
    let item_two = &items[7];
    assert_eq!("method", s(&item_one["label"]), "{items:#?}");
    assert_eq!("static_method", s(&item_two["label"]));
    assert_eq!("${1|public,private|} ${2:void} ${3:name}(${4});", new_text(item_one));
    assert_eq!("${1|public,private|} static ${2:void} ${3:name}(${4}) {\n\t${0}\n}", new_text(item_two));
}

#[test]
fn test_snippet_interface_no_ctor() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic interface Test {\nctor\n}");
    let list = t.request_completions(&unit, "ctor");
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|i| i["label"] == "ctor"), "No ctor snippet should be available");
}

#[test]
fn test_snippet_class() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "");
    let list = t.request_completions(&unit, "");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[0];
    assert_eq!("class", s(&item["label"]));
    assert_eq!("package org.sample;\n\n/**\n * Test\n */\npublic class Test {\n\n\t${0}\n}", dos2unix(s(&item["insertText"])));
}

#[test]
fn test_snippet_class_with_package() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n");
    let list = t.request_completions(&unit, "package org.sample;\n");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[0];
    assert_eq!("class", s(&item["label"]));
    assert_eq!("/**\n * Test\n */\npublic class Test {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_inner_class() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {}\n");
    let list = t.request_completions(&unit, "");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[0];
    assert_eq!("class", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest}\n */\npublic class ${1:InnerTest} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_inner_class_item_defaults_enabled_type_definition() {
    let mut t = setup();
    t.caps = Caps::mock(true, true, true);
    t.caps.insert_text_mode_default = Some(1);
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {}\n");
    let list = t.request_completions(&unit, "");
    assert!(!list.is_null());
    assert_eq!(FORMAT_SNIPPET, list["itemDefaults"]["insertTextFormat"], "{list:#}");
    assert_eq!(MODE_ADJUST_INDENTATION, list["itemDefaults"]["insertTextMode"]);
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[0];
    assert_eq!("class", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest}\n */\npublic class ${1:InnerTest} {\n\n\t${0}\n}", s(&item["textEditText"]));
    //check that the fields covered by itemDefaults are set to null
    assert!(item["textEdit"].is_null());
    assert!(item["insertTextFormat"].is_null());
    assert!(item["insertTextMode"].is_null());
}

#[test]
fn test_snippet_sibling_inner_class() {
    let mut t = setup();
    let src = "package org.sample;\npublic class Test {}\npublic class InnerTest{}\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[0];
    assert_eq!("class", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic class ${1:InnerTest_1} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_sibling_inner_class_nosnippets() {
    let mut t = setup();
    t.caps = Caps::lsp2();
    let src = "package org.sample;\npublic class Test {}\npublic class InnerTest{}\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|ci| ci["kind"] == KIND_SNIPPET), "No snippets should be returned");
}

#[test]
fn test_snippet_nested_inner_class() {
    let mut t = setup();
    let src = "package org.sample;\npublic class Test {}\npublic class InnerTest{\n";
    let unit = t.get_working_copy("src/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    let items = sorted(items(&list).into_iter().filter(|i| i["sortText"].is_string()).collect());
    assert!(!items.is_empty());
    let item = &items[14];
    assert_eq!("class", s(&item["label"]), "{items:#?}");
    assert!(item["insertText"].is_string());
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic class ${1:InnerTest_1} {\n\n\t${0}\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_class_no_static_method() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\nstatic_method\n}");
    let list = t.request_completions(&unit, "static_method");
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|i| i["label"] == "static_method"), "No static_method snippet should be available");
}

#[test]
fn test_snippet_no_record() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "");
    let list = t.request_completions(&unit, "");
    assert!(!list.is_null());
    //Not a Java 14 project => no snippet
    assert!(!items(&list).iter().any(|i| i["label"] == "record"), "No record snippet should be available");
}

#[test]
fn test_snippet_record() {
    let mut t = setup();
    use_records(&mut t);
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", "");
    let list = t.request_completions(&unit, "");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[2];
    assert_eq!("record", s(&item["label"]), "{items:#?}");
    assert_eq!("package org.sample;\n\n/**\n * Test\n */\npublic record Test(${0}) {\n}", dos2unix(s(&item["insertText"])));
    //check resolution doesn't blow up (https://github.com/eclipse/eclipse.jdt.ls/issues/675)
    assert_eq!(*item, t.resolve(item));
}

#[test]
fn test_snippet_record_with_package() {
    let mut t = setup();
    use_records(&mut t);
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", "package org.sample;\n");
    let list = t.request_completions(&unit, "package org.sample;\n");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[2];
    assert_eq!("record", s(&item["label"]));
    assert_eq!("/**\n * Test\n */\npublic record Test(${0}) {\n}", s(&item["insertText"]));
}

#[test]
fn test_snippet_nested_inner_record_nosnippet() {
    let mut t = setup();
    use_records(&mut t);
    t.caps = Caps::lsp2();
    let src = "package org.sample;\npublic record Test() {}\npublic record InnerTest(){\n";
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    assert!(!list.is_null());
    assert!(!items(&list).iter().any(|ci| ci["kind"] == KIND_SNIPPET), "No snippets should be returned");
}

// ─── Completion response data ────────────────────────────────────────────────

const OBJEC_SOURCE: &str = "public class Foo {\n\tvoid foo() {\n\t\tObjec\n\t}\n}\n";

/// `testCompletion_dataFieldURI`: upstream reads the response's common `uri`
/// data from the server-internal `CompletionResponses` cache.  Over LSP that
/// data is what `completionItem/resolve` resolves the compilation unit from
/// (`CompletionResolveHandler` throws when the response is missing or the
/// `uri` matches no unit), so resolving the item must succeed.
#[test]
fn test_completion_data_field_uri() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", OBJEC_SOURCE);
    let list = t.request_completions(&unit, "Objec");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let item = &items(&list)[0];
    let rid: i64 = s(&item["data"]["rid"]).parse().unwrap();
    assert!(rid >= 0);
    assert!(regex_full_match(r"file://.*/src/java/Foo\.java", &unit.uri), "unexpected URI prefix: {}", unit.uri);
    let resp = t.ws.client().request_response("completionItem/resolve", item.clone());
    assert!(resp.get("error").is_none(), "{resp:#}");
}

/// `testCompletion_dataFieldExecutionTime`: upstream reads the
/// `COMPLETION_EXECUTION_TIME` common data of the cached response.  Over LSP
/// it is observable through `java.completion.onDidSelect`, which copies it
/// into the selected item's data and fails when the response is missing.
#[test]
fn test_completion_data_field_execution_time() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", OBJEC_SOURCE);
    let list = t.request_completions(&unit, "Objec");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let item = &items(&list)[0];
    let rid: i64 = s(&item["data"]["rid"]).parse().unwrap();
    let pid = s(&item["data"]["pid"]).to_owned();
    let resp = t.ws.client().request_response(
        "workspace/executeCommand",
        json!({ "command": "java.completion.onDidSelect", "arguments": [rid.to_string(), pid] }),
    );
    assert!(resp.get("error").is_none(), "{resp:#}");
}

// ─── Records (disabled upstream) ─────────────────────────────────────────────

#[test]
#[ignore = "@Disabled upstream: cu.getAllTypes() returns an empty array in tests, so the inner record name is not computed"]
fn test_snippet_inner_record() {
    let mut t = setup();
    use_records(&mut t);
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", "package org.sample;\npublic record Test() {}\n");
    let list = t.request_completions(&unit, "package org.sample;\npublic record Test() {");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[7];
    assert_eq!("record", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest}\n */\npublic record ${1:InnerTest}(${0}) {\n}", s(&item["insertText"]));
}

#[test]
#[ignore = "@Disabled upstream: cu.getAllTypes() returns an empty array in tests, so the inner record name is not computed"]
fn test_snippet_sibling_inner_record() {
    let mut t = setup();
    use_records(&mut t);
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", "package org.sample;\npublic record Test() {}\npublic record InnerTest(){}\n");
    let list = t.request_completions(&unit, "package org.sample;\npublic record Test {}\npublic record InnerTest(){}\n");
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[7];
    assert_eq!("record", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic record ${1:InnerTest_1}(${0) {\n}", s(&item["insertText"]));
}

#[test]
#[ignore = "@Disabled upstream: cu.getAllTypes() returns an empty array in tests, so the inner record name is not computed"]
fn test_snippet_nested_inner_record() {
    let mut t = setup();
    use_records(&mut t);
    let src = "package org.sample;\npublic record Test() {}\npublic record InnerTest(){\n";
    let unit = t.get_working_copy("src/main/java/org/sample/Test.java", src);
    let list = t.request_completions(&unit, src);
    let items = sorted(items(&list));
    assert!(!items.is_empty());
    let item = &items[24];
    assert_eq!("record", s(&item["label"]));
    assert_eq!("/**\n * ${1:InnerTest_1}\n */\npublic record ${1:InnerTest_1}(${0}) {\n}", s(&item["insertText"]));
}

// ─── Overrides ───────────────────────────────────────────────────────────────

fn override_items(list: &Value) -> Vec<Value> {
    items(list)
        .into_iter()
        .filter(|i| i["detail"].as_str().is_some_and(|d| d.starts_with("Override method in")))
        .collect()
}

/// `importProjects("eclipse/<name>"); project = getProject(name)` unless the
/// current project already is `name`.
fn use_project(t: &mut T, name: &'static str) {
    if t.project != name {
        t.ws.import_projects(&[&format!("eclipse/{name}")]);
        t.project = name;
    }
}

fn class_method_override(project: &'static str, support_snippets: bool, overrides_super_class: bool) {
    let mut t = setup();
    use_project(&mut t, project);
    t.caps = Caps::mock(support_snippets, true, false);
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n    toStr}\n");
    let list = t.request_completions(&unit, " toStr");
    assert!(!list.is_null());
    let filtered = override_items(&list);
    assert!(!filtered.is_empty(), "No override proposals: {list:#}");
    let oride = &filtered[0];
    assert_eq!("toString", s(&oride["insertText"]));
    assert!(!oride["textEdit"].is_null());
    let text = s(&oride["textEdit"]["newText"]);
    let mut expected = String::new();
    if overrides_super_class {
        expected.push_str("@Override\n");
    }
    expected.push_str("public String toString() {\n\t");
    if support_snippets {
        expected.push_str("${0:");
    }
    expected.push_str("// TODO Auto-generated method stub\n\t");
    expected.push_str("return super.toString();");
    if support_snippets {
        expected.push('}');
    }
    expected.push_str("\n}");
    assert_eq!(expected, text);
}

fn interface_method_override(project: &'static str, support_snippets: bool, overrides_interface: bool) {
    let mut t = setup();
    use_project(&mut t, project);
    t.caps = Caps::mock(support_snippets, true, false);
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo implements Runnable{\n    ru}\n");
    let list = t.request_completions(&unit, " ru");
    assert!(!list.is_null());
    let filtered = override_items(&list);
    assert!(!filtered.is_empty(), "No override proposals: {list:#}");
    let oride = &filtered[0];
    assert_eq!("run", s(&oride["insertText"]));
    assert!(!oride["textEdit"].is_null());
    let text = s(&oride["textEdit"]["newText"]);
    let mut expected = String::new();
    if overrides_interface {
        expected.push_str("@Override\n");
    }
    expected.push_str("public void run() {\n\t");
    if support_snippets {
        expected.push_str("${0:");
    }
    expected.push_str("// TODO Auto-generated method stub\n\t");
    if support_snippets {
        expected.push('}');
    }
    expected.push_str("\n}");
    assert_eq!(expected, text);
}

#[test]
fn test_completion_method_override() {
    class_method_override("hello", true, true);
}

#[test]
fn test_completion_interface_method_override() {
    interface_method_override("hello", true, true);
}

#[test]
fn test_completion_class_method_override_no_snippet() {
    class_method_override("hello", false, true);
}

#[test]
fn test_completion_interface_method_override_no_snippet() {
    interface_method_override("hello", false, true);
}

#[test]
fn test_completion_class_method_override_java4() {
    class_method_override("java11", true, true);
}

#[test]
fn test_completion_interface_method_override_java4() {
    interface_method_override("java11", true, true);
}

#[test]
fn test_completion_class_method_override_java5() {
    class_method_override("java11", true, true);
}

#[test]
fn test_completion_interface_method_override_java5() {
    interface_method_override("java11", true, true);
}

#[test]
fn test_completion_method_override_with_params() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n\npublic class Test extends Baz {\n    getP}\n");
    let list = t.request_completions(&unit, " getP");
    assert!(!list.is_null());
    let filtered = override_items(&list);
    assert_eq!(1, filtered.len(), "No override proposals: {list:#}");
    let oride = &filtered[0];
    assert_eq!("getParent", s(&oride["insertText"]));
    assert!(!oride["textEdit"].is_null());
    let text = s(&oride["textEdit"]["newText"]);
    let expected = "@Override\nprotected File getParent(File file, int depth) {\n\t${0:// TODO Auto-generated method stub\n\treturn super.getParent(file, depth);}\n}";
    assert_eq!(expected, text);
    let edits = oride["additionalTextEdits"].as_array().unwrap();
    assert_eq!(1, edits.len(), "Missing required imports");
    assert_eq!("\n\nimport java.io.File;\n\n", s(&edits[0]["newText"]));
    assert_position(0, 19, &edits[0]["range"]["start"]);
    assert_position(2, 0, &edits[0]["range"]["end"]);
}

#[test]
fn test_completion_method_override_with_exception() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n\npublic class Test extends Baz {\n    dele}\n");
    let list = t.request_completions(&unit, " dele");
    assert!(!list.is_null());
    let filtered = override_items(&list);
    assert_eq!(1, filtered.len(), "No override proposals: {list:#}");
    let oride = &filtered[0];
    assert_eq!("deleteSomething", s(&oride["insertText"]));
    assert!(!oride["textEdit"].is_null());
    let text = s(&oride["textEdit"]["newText"]);
    assert_eq!(s(&oride["label"]), "deleteSomething");
    assert_eq!(s(&oride["labelDetails"]["detail"]), "()");
    assert_eq!(s(&oride["labelDetails"]["description"]), "void");
    let expected = "@Override\nprotected void deleteSomething() throws IOException {\n\t${0:// TODO Auto-generated method stub\n\tsuper.deleteSomething();}\n}";
    assert_eq!(expected, text);
    let edits = oride["additionalTextEdits"].as_array().unwrap();
    assert_eq!(1, edits.len(), "Missing required imports");
    assert_eq!("\n\nimport java.io.IOException;\n\n", s(&edits[0]["newText"]));
    assert_position(0, 19, &edits[0]["range"]["start"]);
    assert_position(2, 0, &edits[0]["range"]["end"]);
}

// ─── Getters and setters ─────────────────────────────────────────────────────

fn find_label_prefix(list: &Value, prefix: &str) -> Value {
    items(list)
        .into_iter()
        .find(|i| s(&i["label"]).starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix:?} in {list:#}"))
}

#[test]
fn test_completion_getter() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n    private String strField;\n    get}\n");
    let list = t.request_completions(&unit, "get");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "getStrField() : String");
    assert_eq!("getStrField", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999979", s(&ci["sortText"]));
    assert_text_edit(
        2,
        4,
        7,
        "/**\n * @return the strField\n */\npublic String getStrField() {\n\treturn strField;\n}",
        &ci["textEdit"],
    );
}

#[test]
fn test_completion_getter_no_javadoc() {
    let mut t = setup();
    t.set_preference(&["java", "codeGeneration", "generateComments"], json!(false));
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n    private String strField;\n    get}\n");
    let list = t.request_completions(&unit, "get");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "getStrField() : String");
    assert_eq!("getStrField", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999979", s(&ci["sortText"]));
    assert_text_edit(2, 4, 7, "public String getStrField() {\n\treturn strField;\n}", &ci["textEdit"]);
}

#[test]
fn test_completion_booleangetter() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n    private boolean boolField;\n    is\n}\n");
    let list = t.request_completions(&unit, "is");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "isBoolField() : boolean");
    assert_eq!("isBoolField", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999979", s(&ci["sortText"]));
    assert_text_edit(
        2,
        4,
        6,
        "/**\n * @return the boolField\n */\npublic boolean isBoolField() {\n\treturn boolField;\n}",
        &ci["textEdit"],
    );
}

#[test]
fn test_completion_setter() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n    private String strField;\n    set}\n");
    let list = t.request_completions(&unit, "set");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "setStrField(String strField) : void");
    assert_eq!("setStrField", s(&ci["insertText"]));
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("999999979", s(&ci["sortText"]));
    assert_text_edit(
        2,
        4,
        7,
        "/**\n * @param strField the strField to set\n */\npublic void setStrField(String strField) {\n\tthis.strField = strField;\n}",
        &ci["textEdit"],
    );
}

// ─── Anonymous types ─────────────────────────────────────────────────────────

#[test]
fn test_completion_anonymous_type() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps.label_details = true;
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static void main(String[] args) {\n        IFoo foo = new \n    } \n    interface IFoo {\n        String getName();\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "new ");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "Foo.IFoo");
    assert_eq!("Foo.IFoo", s(&ci["insertText"]));
    assert_eq!(KIND_CONSTRUCTOR, ci["kind"]);
    // createAnonymousTypeLabel
    assert_eq!("Foo.IFoo", s(&ci["label"]));
    assert_eq!("()", s(&ci["labelDetails"]["detail"]));
    assert_eq!("Anonymous Inner Type", s(&ci["labelDetails"]["description"]));
    assert_eq!("999998684", s(&ci["sortText"]));
    assert_text_edit(2, 23, 23, "IFoo() {\n\t${0}\n};", &ci["textEdit"]);
}

#[test]
fn test_completion_anonymous_type_more_methods() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static void main(String[] args) {\n        IFoo foo = new \n    } \n    interface IFoo {\n        String getName();\n        void setName(String name);\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "new ");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "Foo.IFoo()  Anonymous Inner Type");
    assert_eq!("Foo.IFoo", s(&ci["insertText"]));
    assert_eq!(KIND_CONSTRUCTOR, ci["kind"]);
    assert_eq!("999998684", s(&ci["sortText"]));
    assert_text_edit(2, 23, 23, "IFoo() {\n\t${0}\n};", &ci["textEdit"]);
}

fn anonymous_declaration(caps: Caps, source: &str, behind: &str, line: u64, start: u64, end: u64, text: &str) {
    let mut t = setup();
    t.caps = caps;
    let unit = t.get_working_copy("src/java/Foo.java", source);
    let list = t.request_completions(&unit, behind);
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "Runnable()  Anonymous Inner Type");
    assert_eq!("Runnable", s(&ci["insertText"]));
    assert_eq!(KIND_CLASS, ci["kind"]);
    assert_eq!("999999372", s(&ci["sortText"]));
    assert_text_edit(line, start, end, text, &ci["textEdit"]);
}

#[test]
fn test_completion_anonymous_declaration_type() {
    anonymous_declaration(
        Caps::lsp3(),
        "public class Foo {\n    public static void main(String[] args) {\n        new Runnable()\n    }\n}\n",
        "Runnable(",
        2,
        20,
        22,
        "() {\n\t${0}\n}",
    );
}

#[test]
fn test_completion_anonymous_declaration_type2() {
    anonymous_declaration(
        Caps::lsp3(),
        "public class Foo {\n    public static void main(String[] args) {\n        new Runnable(  )\n    }\n}\n",
        "Runnable( ",
        2,
        20,
        24,
        "() {\n\t${0}\n}",
    );
}

#[test]
fn test_completion_anonymous_declaration_type3() {
    anonymous_declaration(
        Caps::lsp3(),
        "public class Foo {\n    public static void main(String[] args) {\n        run(\"name\", new Runnable(, 1);\n    }\n    void run(String name, Runnable runnable, int i) {\n    }\n}\n",
        "Runnable(",
        2,
        33,
        37,
        "() {\n\t${0}\n}",
    );
}

#[test]
fn test_completion_anonymous_declaration_type4() {
    anonymous_declaration(
        Caps::lsp3(),
        "public class Foo {\n    public static void main(String[] args) {\n        run(\"name\", new Runnable(\n        , 1);\n    }\n    void run(String name, Runnable runnable, int i) {\n    }\n}\n",
        "Runnable(",
        3,
        8,
        12,
        "() {\n\t${0}\n}",
    );
}

#[test]
fn test_completion_anonymous_declaration_type5() {
    anonymous_declaration(
        Caps::lsp3(),
        "public class Foo {\n    public static void main(String[] args) {\n        run(\"name\", new Runnable(",
        "Runnable(",
        2,
        33,
        33,
        "() {\n\t${0}\n}",
    );
}

#[test]
fn test_completion_anonymous_declaration_type_no_snippet() {
    // A fresh mock: only isCompletionSnippetsSupported (false) is stubbed.
    anonymous_declaration(
        Caps::default(),
        "public class Foo {\n    public static void main(String[] args) {\n        new Runnable()\n    }\n}\n",
        "Runnable(",
        2,
        20,
        22,
        "() {\n\n}",
    );
}

// ─── Types ───────────────────────────────────────────────────────────────────

#[test]
fn test_completion_type() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Foo.java",
        "public class Foo {\n    public static void main(String[] args) {\n        ArrayList\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "ArrayList");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "ArrayList");
    assert_eq!("ArrayList", s(&ci["insertText"]));
    assert_eq!(KIND_CLASS, ci["kind"]);
    assert_eq!("ArrayList - java.util", s(&ci["label"]));
    assert_eq!("java.util.ArrayList", s(&ci["detail"]));
    assert_eq!("999999116", s(&ci["sortText"]));
    assert!(!ci["textEdit"].is_null());
}

const DOLLAR_SOURCE: &str = "public class Foo$Bar {\n    public static void main(String[] args) {\n        new Foo\n    }\n}\n";

/// `testCompletion_class_name_contains_$`.
#[test]
fn test_completion_class_name_contains_dollar() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/org/sample/Foo$Bar.java", DOLLAR_SOURCE);
    let list = t.request_completions(&unit, "new Foo");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "Foo$Bar");
    assert_eq!("Foo$Bar", s(&ci["insertText"]));
    assert_eq!(KIND_CONSTRUCTOR, ci["kind"]);
    assert_eq!("999999115", s(&ci["sortText"]));
    assert_text_edit(2, 12, 15, "Foo\\$Bar()", &ci["textEdit"]);
}

/// `testCompletion_class_name_contains_$withoutSnippetSupport`.
#[test]
fn test_completion_class_name_contains_dollar_without_snippet_support() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps.snippets = false;
    let unit = t.get_working_copy("src/org/sample/Foo$Bar.java", DOLLAR_SOURCE);
    let list = t.request_completions(&unit, "new Foo");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "Foo$Bar");
    assert_eq!("Foo$Bar", s(&ci["insertText"]));
    assert_eq!(KIND_CONSTRUCTOR, ci["kind"]);
    assert_eq!("999999115", s(&ci["sortText"]));
    assert_text_edit(2, 12, 15, "Foo$Bar", &ci["textEdit"]);
}

#[test]
fn test_completion_test_classes_dont_leak_into_main_code() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n\npublic class Test extends AbstractTe {\n}\n");
    let list = t.request_completions(&unit, " AbstractTe");
    assert_eq!(0, items(&list).len(), "Test proposals leaked:\n{list:#}");
}

#[test]
fn test_completion_test_method_with_params() {
    let mut t = setup();
    t.caps = Caps { resolve_documentation: true, ..Default::default() };
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\tfo\n\t\tSystem.out.println(\"Hello World!\");\n\t}\n\n\t/**\n\t* This method has Javadoc\n\t*/\n\tpublic static void foo(String bar) {\n\t}\n\t/**\n\t* Another Javadoc\n\t*/\n\tpublic static void foo() {\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "\t\tfo");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "foo(String bar) : void");
    let resolved = t.resolve(&ci);
    assert!(!resolved.is_null());
    assert_eq!(s(&resolved["documentation"]), " This method has Javadoc ");
    let ci = find_label_prefix(&list, "foo() : void");
    let resolved = t.resolve(&ci);
    assert!(!resolved.is_null());
    assert_eq!(s(&resolved["documentation"]), " Another Javadoc ");
}

#[test]
fn test_completion_test_classes_available_into_test_code() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("test/foo/bar/BaseTest.java", "package foo.bar;\n\npublic class BaseTest extends AbstractTe {\n}\n");
    let list = t.request_completions(&unit, " AbstractTe");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "Test proposals missing from :\n{list:#}");
    assert_eq!("AbstractTest - foo.bar", s(&items(&list)[0]["label"]));
}

/// `getCompletionOverwriteReplaceUnit`.
const OVERWRITE_SOURCE: &str = "package foo.bar;\n\npublic class BaseTest {\n    public int testInt;\n\n    public boolean method(int x, int y, int z) {\n        return true;\n    } \n\n    public void update() {\n        BaseTest t = new BaseTest();\n        t.method(t.this.testInt, this.testInt);\n    }\n}\n";

fn completion_overwrite_replace(overwrite: bool, expected: &str) {
    let mut t = setup();
    if !overwrite {
        t.set_preference(&["java", "completion", "overwrite"], json!(false));
    }
    let unit = t.get_working_copy("test/foo/bar/BaseTest.java", OVERWRITE_SOURCE);
    let list = t.request_completions(&unit, "method(t.");
    assert!(!list.is_null());
    let ci = find_label_prefix(&list, "testInt : int");
    assert_eq!("testInt", s(&ci["insertText"]));
    assert_eq!(KIND_FIELD, ci["kind"]);
    assert_eq!("999998554", s(&ci["sortText"]));
    assert!(!ci["textEdit"].is_null());
    let returned = apply_edits(&unit.text, &[ci["textEdit"].clone()]);
    assert_eq!(returned, expected);
}

#[test]
fn test_completion_overwrite() {
    completion_overwrite_replace(
        true,
        "package foo.bar;\n\npublic class BaseTest {\n    public int testInt;\n\n    public boolean method(int x, int y, int z) {\n        return true;\n    } \n\n    public void update() {\n        BaseTest t = new BaseTest();\n        t.method(t.testInt.testInt, this.testInt);\n    }\n}\n",
    );
}

#[test]
fn test_completion_insert() {
    completion_overwrite_replace(
        false,
        "package foo.bar;\n\npublic class BaseTest {\n    public int testInt;\n\n    public boolean method(int x, int y, int z) {\n        return true;\n    } \n\n    public void update() {\n        BaseTest t = new BaseTest();\n        t.method(t.testIntthis.testInt, this.testInt);\n    }\n}\n",
    );
}

// ─── Snippet contexts ────────────────────────────────────────────────────────

fn find_snippet(list: &Value, label: &str) -> Option<Value> {
    items(list).into_iter().find(|i| i["label"] == label && i["kind"] == KIND_SNIPPET)
}

#[test]
fn test_snippet_with_public() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic ");
    let list = t.request_completions(&unit, "public ");
    assert!(!list.is_null());
    let ci = find_snippet(&list, "class").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("class Test {\n\n\t${0}\n}", s(&ci["insertText"]));
    let ci = find_snippet(&list, "interface").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("interface Test {\n\n\t${0}\n}", s(&ci["insertText"]));
}

#[test]
fn test_snippet_context_javadoc() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n/**\n */");
    let list = t.request_completions(&unit, "/**");
    assert!(!list.is_null());
    assert!(find_snippet(&list, "class").is_none(), "{list:#}");
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

#[test]
fn test_snippet_context_package() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n");
    let list = t.request_completions(&unit, "package ");
    assert!(!list.is_null());
    assert!(find_snippet(&list, "class").is_none(), "{list:#}");
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

#[test]
fn test_snippet_context_method1() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t}\n}\n");
    let list = t.request_completions(&unit, "{\n\n");
    assert!(!list.is_null());
    let ci = find_snippet(&list, "class").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("class ${1:InnerTest} {\n\n\t${0}\n}", s(&ci["insertText"]));
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

#[test]
fn test_snippet_context_method2() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tif (c\n\t}\n}\n");
    let list = t.request_completions(&unit, "if (c");
    assert!(!list.is_null());
    assert!(find_snippet(&list, "class").is_none(), "{list:#}");
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

#[test]
fn test_snippet_context_method3() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tint \n\t}\n}\n");
    let list = t.request_completions(&unit, "int ");
    assert!(!list.is_null());
    assert!(find_snippet(&list, "class").is_none(), "{list:#}");
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

#[test]
fn test_snippet_context_static() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tstatic {\n\t}\n}\n");
    let list = t.request_completions(&unit, "static {\n");
    assert!(!list.is_null());
    let ci = find_snippet(&list, "class").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("class ${1:InnerTest} {\n\n\t${0}\n}", s(&ci["insertText"]));
    assert!(find_snippet(&list, "interface").is_none(), "{list:#}");
}

// ─── Static imports and result limits ────────────────────────────────────────

/// `-Dcompletion.timeout=60000`, set by the static import tests.
fn long_completion_timeout(t: &mut T) {
    t.ws.oracle_java_options.push("-Dcompletion.timeout=60000".into());
}

#[test]
fn test_static_imports1() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    long_completion_timeout(&mut t);
    t.set_preference(&["java", "completion", "favoriteStaticMembers"], json!(["test1.A.foo"]));
    let unit = t.get_working_copy("src/test1/B.java", "package test1;\n\npublic class B {\n    public void bar() {\n        fo\n    }\n}\n");
    let list = t.request_completions(&unit, "fo");
    assert!(!list.is_null());
    assert_eq!(Some(false), list["isIncomplete"].as_bool());
    assert!(!items(&list).is_empty());
    assert_eq!("foo() : void", s(&items(&list)[0]["label"]), "no proposal for foo()");
}

fn no_snippets(items: Vec<Value>) -> Vec<Value> {
    items.into_iter().filter(|i| i["kind"] != KIND_SNIPPET).collect()
}

#[test]
fn test_limit_completion_results() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/test1/B.java", "package test1;\n\npublic class B {\n    public void bar() {\n        d\n    }\n}\n");
    //Completion should limit results to maxCompletionResults (excluding snippets)
    let list = t.request_completions(&unit, "d");
    assert!(!list.is_null());
    assert_eq!(Some(true), list["isIncomplete"].as_bool());
    let completion_only = no_snippets(items(&list));
    assert_eq!(50, completion_only.len());
    assert!(s(&completion_only[0]["sortText"]) < s(&completion_only[completion_only.len() - 1]["sortText"]));

    //Set max results to 1 to double check
    t.set_preference(&["java", "completion", "maxResults"], json!(1));
    let list = t.request_completions(&unit, "d");
    assert!(!list.is_null());
    assert_eq!(Some(true), list["isIncomplete"].as_bool());
    assert_eq!(1, no_snippets(items(&list)).len());

    //when maxCompletionResults is set to 0, limit is disabled, completion should be complete
    t.set_preference(&["java", "completion", "maxResults"], json!(0));
    let list = t.request_completions(&unit, "d");
    assert!(!list.is_null());
    assert_eq!(Some(false), list["isIncomplete"].as_bool());
    let completion_only = no_snippets(items(&list));
    assert!(completion_only.len() > 50, "Expected way than {}", completion_only.len());
    assert!(s(&completion_only[0]["sortText"]) < s(&completion_only[completion_only.len() - 1]["sortText"]));
}

#[test]
fn test_static_imports2() {
    let mut t = setup();
    long_completion_timeout(&mut t);
    t.set_preference(&["java", "completion", "favoriteStaticMembers"], json!([]));
    let unit = t.get_working_copy(
        "src/test1/B.java",
        // conflicting method, no static import possible
        "package test1;\n\npublic class B {\n    public void bar() {\n        /* */fo\n    }\n    public void foo(int x) {\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "/* */fo");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    for it in items(&list) {
        assert_ne!("foo() : void", s(&it["label"]), "there is a proposal for foo()");
    }
}

#[test]
fn test_star_imports() {
    let mut t = setup();
    long_completion_timeout(&mut t);
    t.set_preference(&["java", "completion", "favoriteStaticMembers"], json!(["java.lang.Math.*"]));
    t.set_preference(&["java", "sources", "organizeImports", "starThreshold"], json!(2));
    t.set_preference(&["java", "sources", "organizeImports", "staticStarThreshold"], json!(2));
    let unit = t.get_working_copy(
        "src/test1/B.java",
        "package test1;\nimport static java.lang.Math.sqrt;\nimport java.util.List;\npublic class B {\n    List<String> list = new ArrayL\n    public static void main(String[] args) {\n        double d1 = sqrt(4);\n        double d2 = abs\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "new ArrayL");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    let item = items(&list).into_iter().find(|i| i["label"] == "ArrayList()").unwrap_or_else(|| panic!("{list:#}"));
    let edits = item["additionalTextEdits"].as_array().unwrap_or_else(|| panic!("{item:#}"));
    assert_eq!(1, edits.len());
    assert_eq!("\n\nimport java.util.*;", s(&edits[0]["newText"]));
    let list = t.request_completions(&unit, "= abs");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    let item = find_label_prefix(&list, "abs(double");
    let edits = item["additionalTextEdits"].as_array().unwrap_or_else(|| panic!("{item:#}"));
    assert_eq!(1, edits.len());
    assert_eq!("import static java.lang.Math.*;\n\n", s(&edits[0]["newText"]));
}

#[test]
fn test_completion_links_in_markdown() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { markdown: true, resolve_documentation: true, ..Default::default() };
    t.ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n    public void foo(){\n      this.zz \n    }\n    \n\t/**\n\t * @see Baz\n\t */\n    public Baz zzzzzzz(){ \n      return null;\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "this.zz");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "{list:#}");
    let ci = &items(&list)[0];
    assert_eq!("zzzzzzz() : Baz", s(&ci["label"]));
    let resolved = t.resolve(ci);
    assert!(resolved["documentation"].is_object(), "{resolved:#}");
    let doc = s(&resolved["documentation"]["value"]);
    assert!(doc.contains("* [Baz](file:/"), "Unexpected documentation content in {doc}");
}

#[test]
fn test_completion_additional_text_edit() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tprivate Object o;\n\tvoid foo() {\n\t\to.toStr\n\t}\n}\n");
    let list = t.request_completions(&unit, "o.toStr");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let ci = &items(&list)[0];
    assert!(ci["additionalTextEdits"].is_null(), "{ci:#}");
    assert_eq!("toString() : String", s(&ci["label"]));
    let resolved = t.resolve(ci);
    assert!(resolved["additionalTextEdits"].is_null(), "{resolved:#}");
}

#[test]
fn test_completion_resolve_additional_text_edits() {
    let mut t = setup();
    t.caps = Caps { resolve_additional_text_edits: true, ..Default::default() };
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n\t\tHashMa\n\t}\n}\n");
    let list = t.request_completions(&unit, "HashMa");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let ci = &items(&list)[0];
    assert!(ci["additionalTextEdits"].is_null(), "{ci:#}");
    assert_eq!("HashMap - java.util", s(&ci["label"]));
    let resolved = t.resolve(ci);
    let edits = resolved["additionalTextEdits"].as_array().unwrap_or_else(|| panic!("{resolved:#}"));
    assert_eq!(1, edits.len());
    assert_eq!("import java.util.HashMap;\n\n", s(&edits[0]["newText"]));
}

#[test]
fn test_completion_enum() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n   enum Zenum{A,B}\n\tvoid test() {\n\n      Zenu\n\t}\n}\n");
    let list = t.request_completions(&unit, "   Zenu");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert_eq!(KIND_ENUM, item["kind"]);
    assert_eq!("Zenum", s(&item["insertText"]));
}

#[test]
fn test_completion_constant() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tchar c = java.io.File.pathSeparatorC \n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "pathSeparatorC");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert_eq!(KIND_CONSTANT, item["kind"]);
    assert_eq!("pathSeparatorChar", s(&item["insertText"]));
}

// ─── Type filters ────────────────────────────────────────────────────────────

fn set_filtered_types(t: &mut T, types: &[&str]) {
    t.set_preference(&["java", "completion", "filteredTypes"], json!(types));
}

fn has_detail(list: &Value, detail: &str) -> bool {
    items(list).iter().any(|i| i["detail"] == detail)
}

#[test]
fn test_completion_filter_types() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tList l; \n\t}\n}\n");
    let list = t.request_completions(&unit, "List");
    assert!(!list.is_null());
    assert!(has_detail(&list, "java.util.List"), "{list:#}");
    let present = items(&list).iter().any(|i| i["label"] == "List - java.util");
    assert!(present, "The 'List - java.util' proposal hasn't been found");
    set_filtered_types(&mut t, &["java.util.*"]);
    let list = t.request_completions(&unit, "List");
    assert!(!list.is_null());
    assert!(!has_detail(&list, "java.util.List"), "{list:#}");
}

#[test]
fn test_completion_filter_packages() {
    let mut t = setup();
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\n\npublic class Test {\n\tvoid test() {\n\t\tjava.util \n\t}\n}\n");
    let list = t.request_completions(&unit, "java.util");
    assert!(!list.is_null());
    let packages: Vec<String> = items(&list).iter().map(|i| s(&i["label"]).to_owned()).collect();
    assert!(packages.len() > 1, "{list:#}");
    assert_eq!("java.util", packages[0]);
}

#[test]
fn test_completion_filter_types_keep_methods() {
    let mut t = setup();
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tjava.util.List l; \n       l.clea \n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "l.clea");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "Missing completion: {list:#}");
    assert_eq!("clear() : void", s(&items(&list)[0]["label"]));
}

#[test]
fn test_completion_filter_types_keep_methods2() {
    let mut t = setup();
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\nimport java.util.List;public class Test {\n\n\tvoid test() {\n\n\t\tList l; \n\t\tl.clea \n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "l.clea");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "Missing completion: {list:#}");
    assert_eq!("clear() : void", s(&items(&list)[0]["label"]));
}

#[test]
fn test_completion_filter_methods_when_type_is_missing() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\npublic class Test {\n\n\tvoid test() {\n\n\t\tList l; \n\t\tl.clea \n\t}\n}\n");
    let list = t.request_completions(&unit, "l.clea");
    assert!(!list.is_null());
    assert_eq!(0, items(&list).len(), "{list:#}");
}

fn ignore_type_filter_when_imported(import: &str) {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let src = format!("package org.sample;\n{import}public class Test {{\n\n\tvoid test() {{\n\n\t\tList\n\t}}\n}}\n");
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy("src/org/sample/Test.java", &src);
    // getWorkingCopy's makeConsistent: the unit is reconciled before the request.
    t.ws.diagnostics(&unit.uri);
    let list = t.request_completions(&unit, "\t\tList");
    assert!(!list.is_null());
    assert!(has_detail(&list, "java.util.List"), "{list:#}");
}

#[test]
fn test_completion_ignore_type_filter_when_imported1() {
    ignore_type_filter_when_imported("import java.util.List;");
}

#[test]
fn test_completion_ignore_type_filter_when_imported2() {
    ignore_type_filter_when_imported("import java.util.*;");
}

#[test]
fn test_completion_ignore_type_filter_when_imported3() {
    ignore_type_filter_when_imported("import static java.util.List.*;");
}

#[test]
fn test_completion_ignore_type_filter_when_imported4() {
    ignore_type_filter_when_imported("import static java.util.List.DUMMY;");
}

#[test]
fn test_completion_ignore_type_filter_when_imported5() {
    let mut t = setup();
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\nimport java.util.List;\npublic class Test {\n}");
    let list = t.request_completions(&unit, "java.util.");
    assert!(!list.is_null());
}

#[test]
fn test_completion_ignore_type_filter_when_imported6() {
    let mut t = setup();
    // The preference is set before the server starts: it is in place before the request.
    set_filtered_types(&mut t, &["java.util.*"]);
    let unit = t.get_working_copy("src/org/sample/Test.java", "package org.sample;\nimport java.util.\npublic class Test {\n}");
    let list = t.request_completions(&unit, "java.util.");
    assert!(!list.is_null());
}

#[test]
fn test_completion_auto_add_static_import_as_favorite_import() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    // The preference is set before the server starts: it is in place before the request.
    t.set_preference(&["java", "completion", "favoriteStaticMembers"], json!(["org.junit.Assert.*"]));
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\nimport static java.util.Arrays.sort;\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\tasList\n\t}\n}",
    );
    // getWorkingCopy's makeConsistent: the unit is reconciled before the request.
    t.ws.diagnostics(&unit.uri);
    let list = t.request_completions(&unit, "asList");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    let item = items(&list)
        .into_iter()
        .find(|i| i["detail"].as_str().is_some_and(|d| d.starts_with("java.util.Arrays.asList(")))
        .unwrap_or_else(|| panic!("{list:#}"));
    assert!(!item.is_null());
}

// ─── Documentation ───────────────────────────────────────────────────────────

#[test]
#[ignore = "environment: needs com.aspose:aspose-words:15.12.0 from repository.aspose.com, which this machine cannot reach (the oracle fails the same way)"]
fn test_completion_invalid_javadoc() {
    let mut t = setup();
    t.ws.import_projects(&["maven/aspose"]);
    let uri = t.ws.class_uri("aspose", "org.sample.TestJavadoc");
    let source = t.ws.read(&uri);
    let unit = t.get_working_copy_uri(&uri, &source);
    let list = t.request_completions(&unit, "doc.");
    let ci = items(&list).into_iter().find(|i| i["label"] == "accept(DocumentVisitor visitor) : boolean");
    assert!(ci.is_some(), "{list:#}");
}

#[test]
fn test_completion_constant_default_value() {
    let mut t = setup();
    t.caps = Caps { resolve_documentation: true, ..Default::default() };
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\npublic class Test {\n\n\tprivate int one = IConstantDefault.\n\t@IConstantDefault()\n\tvoid test() {\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "IConstantDefault.");
    assert!(!list.is_null());
    assert_eq!(3, items(&list).len(), "{list:#}");
    let ci = &items(&list)[0];
    assert_eq!(KIND_CONSTANT, ci["kind"]);
    assert_eq!("ONE : int", s(&ci["label"]));
    let resolved = t.resolve(ci);
    assert_eq!(KIND_CONSTANT, resolved["kind"]);
    assert_eq!("Value: 1", s(&resolved["documentation"]));

    let ci = &items(&list)[1];
    assert_eq!(KIND_CONSTANT, ci["kind"]);
    assert_eq!("TEST : double", s(&ci["label"]));

    let list = t.request_completions(&unit, "@IConstantDefault(");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "{list:#}");
    let ci = &items(&list)[0];
    assert_eq!(KIND_FIELD, ci["kind"]);
    assert_eq!("someMethod : String", s(&ci["label"]));
    let resolved = t.resolve(ci);
    assert_eq!(KIND_FIELD, resolved["kind"]);
    assert_eq!("Default: \"test\"", s(&resolved["documentation"]));
}

// See https://github.com/redhat-developer/vscode-java/issues/1258
#[test]
fn test_completion_javadoc_original() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { resolve_documentation: true, ..Default::default() };
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\nimport java.util.List;\nimport java.util.LinkedList;\npublic class Test {\n\n\tvoid test() {\n\t\tMyList<String> l = new LinkedList<>();\n\t\tl.add\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "l.add");
    assert!(!list.is_null());
    assert_eq!(4, items(&list).len(), "{list:#}");
    let ci = &items(&list)[0];
    assert_eq!(KIND_METHOD, ci["kind"]);
    assert_eq!("add(String e) : boolean", s(&ci["label"]));
    let resolved = t.resolve(ci);
    assert_eq!(KIND_METHOD, resolved["kind"]);
    assert_eq!(" Test ", s(&resolved["documentation"]));
}

// See https://github.com/redhat-developer/vscode-java/issues/2034
#[test]
fn test_completion_anonymous() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps.label_details = true;
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample;\nimport java.util.Arrays;\npublic class Test {\n\n\tpublic static void main(String[] args) {\n\t\tnew Runnable() {\n\t\t\t@Override\n\t\t\tpublic void run() {\n\t\t\t\tboolean equals = Arrays.equals(new Object[0], new Object[0]);\n\t\t\t}\n\t\t};\n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "= A");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    let ci = items(&list).into_iter().find(|i| i["label"] == "Arrays").unwrap_or_else(|| panic!("{list:#}"));
    // createTypeProposalLabel
    assert_eq!("Arrays", s(&ci["label"]));
    assert!(ci["labelDetails"]["detail"].is_null());
    assert_eq!("java.util", s(&ci["labelDetails"]["description"]));
    assert_eq!(KIND_CLASS, ci["kind"]);
    assert_eq!("java.util.Arrays", s(&ci["detail"]));
}

#[test]
fn test_completion_nullable() {
    let mut t = setup();
    t.ws.import_projects(&["eclipse/testnullable"]);
    // JavaCore.createCompilationUnitFrom(file): the unit is not opened.
    let uri = t.ws.class_uri("testnullable", "org.sample.Main");
    let text = t.ws.read(&uri);
    t.start();
    let (line, character) = find_completion_location(&text, "ru", 0);
    let list = t.completion_at(&uri, line, character);
    assert!(!list.is_null());
    let ci = items(&list).into_iter().find(|i| i["label"] == "run() : void").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!("public void run() {};", s(&ci["textEdit"]["newText"]));
}

const DEPRECATED_SOURCE: &str = "public class Main {\n\t@Deprecated\n\tpublic static final class DeprecatedClass {}\n\tDeprecatedCl\n\t/**\n\t * @deprecated\n\t */\n\tpublic static void deprecatedMethod() {\n\t\tdeprecatedMe\n\t}\n\tpublic static void notDeprecated() {\n\t\tnotDepr\n\t}\n}";

#[test]
fn test_completion_deprecated() {
    let mut t = setup();
    t.caps.tag_support = true;
    let unit = t.get_working_copy("src/org/sample/Test.java", DEPRECATED_SOURCE);

    let deprecated_class = items(&t.request_completions(&unit, "\tDeprecatedCl"))[0].clone();
    assert_eq!(KIND_CLASS, deprecated_class["kind"]);
    let tags = deprecated_class["tags"].as_array().unwrap_or_else(|| panic!("{deprecated_class:#}"));
    assert!(tags.contains(&json!(1)), "Should have deprecated tag");

    let deprecated_method = items(&t.request_completions(&unit, "\t\tdeprecatedMe"))[0].clone();
    assert_eq!(KIND_METHOD, deprecated_method["kind"]);
    let tags = deprecated_method["tags"].as_array().unwrap_or_else(|| panic!("{deprecated_method:#}"));
    assert!(tags.contains(&json!(1)), "Should have deprecated tag");

    let not_deprecated = items(&t.request_completions(&unit, "\t\tnotDepr"))[0].clone();
    assert_eq!(KIND_METHOD, not_deprecated["kind"]);
    if let Some(tags) = not_deprecated["tags"].as_array() {
        assert!(!tags.contains(&json!(1)), "Should not have deprecated tag");
    }
}

#[test]
fn test_completion_deprecated_property() {
    let mut t = setup();
    t.caps.tag_support = false;
    let unit = t.get_working_copy("src/org/sample/Test.java", "public class Main {\n\t@Deprecated\n\tpublic static final class DeprecatedClass {}\n\tDeprecatedCl\n}");
    let deprecated_class = items(&t.request_completions(&unit, "\tDeprecatedCl"))[0].clone();
    assert_eq!(KIND_CLASS, deprecated_class["kind"]);
    assert_eq!(Some(true), deprecated_class["deprecated"].as_bool(), "Should be deprecated: {deprecated_class:#}");
}

// ─── Lambdas and constructors ────────────────────────────────────────────────

#[test]
fn test_completion_lambda() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "import java.util.function.Consumer;\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\tConsumer c = \n\t}\n}",
    );
    let list = t.request_completions(&unit, "c = ");
    assert!(!list.is_null());
    let lambda = items(&list)
        .into_iter()
        .find(|i| regex_full_match(r"\(Object \w+\) ->", s(&i["label"])) && i["kind"] == KIND_METHOD)
        .unwrap_or_else(|| panic!("{list:#}"));
    assert!(regex_full_match(r"\$\{1:\w+\} -> \$\{0\}", s(&lambda["textEdit"]["newText"])), "{lambda:#}");
    let label = s(&lambda["label"]);
    // In case the JDK has no sources: "(Object arg0) ->"
    assert!(label == "(Object t) ->" || label == "(Object arg0) ->", "{label}");
    assert!(lambda["labelDetails"]["detail"].is_null());
    assert_eq!(s(&lambda["labelDetails"]["description"]), "void");
}

/// A completion request with `CompletionContext(TriggerCharacter, " ")`.
fn request_with_space_trigger(t: &mut T, unit: &Unit, behind: &str) -> Value {
    let (line, character) = find_completion_location(&unit.text, behind, 0);
    t.ws.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": unit.uri },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": 2, "triggerCharacter": " " }
        }),
    )
}

#[test]
fn test_completion_after_new() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "public class Test {\n\tpublic static void main(String[] args) {\n\t\tString s = new \n\t}\n}");
    let list = request_with_space_trigger(&mut t, &unit, "new ");
    assert_eq!(Some(true), list["isIncomplete"].as_bool(), "{list:#}");
    assert!(s(&items(&list)[0]["label"]).starts_with("String("), "{list:#}");
}

#[test]
fn test_completion_ignore_space_without_new() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "public class Test {\n\tpublic static void main(String[] args) {\n\t\tString s \n\t}\n}");
    let list = request_with_space_trigger(&mut t, &unit, "String s ");
    assert!(items(&list).is_empty(), "{list:#}");
}

#[test]
fn test_completion_ignore_variable_with_new_postfix() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "public class Test {\n\tpublic static void main(String[] args) {\n\t\tString val_new;\n\t\tnew String(val_new );\n\t}\n}",
    );
    let list = request_with_space_trigger(&mut t, &unit, "val_new ");
    assert!(items(&list).is_empty(), "{list:#}");
}

#[test]
fn test_completion_ignore_string_literal_new() {
    let mut t = setup();
    let unit = t.get_working_copy("src/org/sample/Test.java", "public class Test {\n\tpublic static void main(String[] args) {\n\t\tString s = \"new \";\n\t}\n}");
    let list = request_with_space_trigger(&mut t, &unit, "\"new ");
    assert!(items(&list).is_empty(), "{list:#}");
}

// https://github.com/redhat-developer/vscode-java/issues/2534
#[test]
fn test_completion_qualified_name() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\t java.util.List<String> list = new Array\n\t}\n}",
    );
    let list = t.request_completions(&unit, "new Array");
    assert!(!items(&list).is_empty());
    assert_eq!("ArrayList<>()", s(&items(&list)[0]["textEdit"]["newText"]), "{list:#}");
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/2147
#[test]
fn test_completion_qualified_name2() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\t  List<String> list = new java.util.ArrayL\n\t}\n}",
    );
    let list = t.request_completions(&unit, "ArrayL");
    assert!(!items(&list).is_empty());
    assert!(s(&items(&list)[0]["filterText"]).starts_with("java.util.ArrayList"), "{list:#}");
}

#[test]
fn test_completion_with_conflicting_type_names() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps { resolve_additional_text_edits: true, ..Default::default() };
    t.get_working_copy("src/java/List.java", "package util;\npublic class List {\n}\n");
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "package util;\npublic class Foo {\n\tvoid foo() {\n \t\tObject list = new List();\n\t\tList \n\t}\n}\n",
    );
    // CoreASTProvider.getAST(unit, WAIT_YES): the unit is reconciled before the request.
    t.ws.diagnostics(&unit.uri);
    let from = unit.text.find("List()").unwrap() + 6;
    let list = t.request_completions_from(&unit, "List", from);
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    assert!(has_detail(&list, "java.util.List"), "java.util.List not found: {list:#}");
    let resolved = t.resolve(&items(&list)[0]);
    assert_eq!("java.util.List", s(&resolved["textEdit"]["newText"]), "{resolved:#}");
}

fn lambda_items(list: &Value) -> Vec<Value> {
    items(list).into_iter().filter(|p| p["label"].as_str().is_some_and(|l| l.contains("->"))).collect()
}

#[test]
fn test_completion_lambda_with_no_param() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n \t\tRunnable r = \n\t}\n}\n");
    let list = t.request_completions(&unit, "= ");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let items = lambda_items(&list);
    assert!(!items.is_empty(), "Lambda not found");
    assert!(regex_full_match(r"\(\) -> \$\{0\}", s(&items[0]["textEdit"]["newText"])), "{:#}", items[0]);
}

#[test]
fn test_completion_lambda_with_multiple_params() {
    let mut t = setup();
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n\tvoid foo() {\n \t\tjava.util.function.BiConsumer<Integer, Long> bc = \n\t}\n}\n",
    );
    let list = t.request_completions(&unit, "= ");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");
    let items = lambda_items(&list);
    assert!(!items.is_empty(), "Lambda not found");
    assert!(regex_full_match(r"\(\$\{1:\w+\}\, \$\{2:\w+\}\) -> \$\{0\}", s(&items[0]["textEdit"]["newText"])), "{:#}", items[0]);
}

// ─── Case matching ───────────────────────────────────────────────────────────

fn first_upper(item: &Value) -> bool {
    s(&item["label"]).chars().next().is_some_and(char::is_uppercase)
}

const MATCH_CASE_SOURCE: &str = "package org.sample\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\ti\n\t}\n}";

#[test]
fn test_completion_match_case_off() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    let unit = t.get_working_copy("src/org/sample/Test.java", MATCH_CASE_SOURCE);
    let list = t.request_completions(&unit, "\t\ti");
    assert!(!items(&list).is_empty());
    assert!(items(&list).iter().any(first_upper), "{list:#}");
}

#[test]
fn test_completion_match_case_first_letter() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.set_preference(&["java", "completion", "matchCase"], json!("firstLetter"));
    let unit = t.get_working_copy("src/org/sample/Test.java", MATCH_CASE_SOURCE);
    let list = t.request_completions(&unit, "\t\ti");
    assert!(!items(&list).is_empty());
    assert!(!items(&list).iter().any(|i| i["kind"] != KIND_SNIPPET && first_upper(i)), "{list:#}");
}

#[test]
fn test_completion_match_case_first_letter_for_constructor() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.set_preference(&["java", "completion", "matchCase"], json!("firstLetter"));
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample\npublic class Test {\n\tpublic static void main(String[] args) {\n\t\tString a = new S\n\t}\n}",
    );
    let list = t.request_completions(&unit, "new S");
    assert!(!items(&list).is_empty());
    assert!(items(&list).iter().all(first_upper), "{list:#}");
    assert!(items(&list).iter().any(|i| s(&i["label"]).starts_with("String")), "{list:#}");
}

// https://github.com/eclipse-jdtls/eclipse.jdt.ls/issues/2884
#[test]
fn test_completion_match_case_first_letter_for_method_override() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.set_preference(&["java", "completion", "matchCase"], json!("firstLetter"));
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample\npublic class Test {\n\tpublic void testMethod(int a, int b){}\n\tpublic void testMethod(int b){}\n}\nclass TestOverride extends Test{\n\tt\n}",
    );
    let list = t.request_completions(&unit, "t");
    assert!(!items(&list).is_empty());
    assert!(s(&items(&list)[0]["label"]).starts_with("testMethod(int b"), "{list:#}");
    assert!(s(&items(&list)[1]["label"]).starts_with("testMethod(int a"), "{list:#}");
}

// ─── Snippet items ───────────────────────────────────────────────────────────

/// `testCompletion_selectSnippetItem` (https://github.com/eclipse/eclipse.jdt.ls/issues/2376):
/// upstream asserts the item's response is still cached in
/// `CompletionResponses`.  Over LSP, `java.completion.onDidSelect` fails with
/// "Cannot get completion responses." when it is not.
#[test]
fn test_completion_select_snippet_item() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n \t\tsysout\n\t}\n}\n");
    let list = t.request_completions(&unit, "sysout");
    let completion_item = &items(&list)[0];
    let data = &completion_item["data"];
    let request_id: i64 = s(&data["rid"]).parse().unwrap();
    let resp = t.ws.client().request_response(
        "workspace/executeCommand",
        json!({ "command": "java.completion.onDidSelect", "arguments": [request_id.to_string(), s(&data["pid"])] }),
    );
    assert!(resp.get("error").is_none(), "{resp:#}");
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/2387
#[test]
fn test_completion_multi_line_range() {
    let mut t = setup();
    t.caps.insert_replace = true;
    let unit = t.get_working_copy(
        "src/java/Foo.java",
        "public class Foo {\n    public static void main(String[] args) {\n        if (true) {\n            java.util.List<String> list = new java.util.ArrayList<>();\n            list.add\n            (\"test\"\n            );\n        }\n    }\n}\n",
    );
    let list = t.request_completions(&unit, "list.");
    let completion_items: Vec<Value> = items(&list).into_iter().filter(|i| s(&i["label"]).starts_with("add")).collect();
    assert!(!completion_items.is_empty(), "{list:#}");
    for completion_item in completion_items {
        let te = &completion_item["textEdit"];
        assert!(!te.is_null());
        let replace = if te.get("replace").is_some() {
            &te["replace"]
        } else if te.get("range").is_some() {
            &te["range"]
        } else {
            &te["insert"]
        };
        assert_eq!(replace["start"]["line"], replace["end"]["line"], "{completion_item:#}");
    }
}

fn edit_range_of(item: &Value) -> Value {
    let te = &item["textEdit"];
    if te.get("range").is_some() {
        te["range"].clone()
    } else {
        te["replace"].clone()
    }
}

#[test]
fn test_completion_syserr_snipper() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "lazyResolveTextEdit", "enabled"], json!(false));
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid f() {\n\t\tsyser\n\t}\n};\n");
    let list = t.request_completions(&unit, "syser");
    assert!(!list.is_null());
    assert_eq!(1, items(&list).len(), "{list:#}");
    let item = &items(&list)[0];
    assert_eq!("syserr", s(&item["label"]));
    assert_eq!(range(2, 2, 2, 7), edit_range_of(item));
}

#[test]
fn test_completion_print_snippets() {
    let mut t = setup();
    t.ws.use_upstream_test_jdk("hello");
    t.set_preference(&["java", "completion", "lazyResolveTextEdit", "enabled"], json!(false));
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid f() {\n\t\tprin\n\t}\n};\n");
    let list = t.request_completions(&unit, "prin");
    assert!(!list.is_null());
    let out_item = &items(&list)[3];
    let err_item = &items(&list)[4];
    assert_eq!("System.out.println()", s(&out_item["label"]), "{list:#}");
    assert_eq!("System.err.println()", s(&err_item["label"]), "{list:#}");
    assert_eq!(range(2, 2, 2, 6), edit_range_of(out_item));
    assert_eq!(range(2, 2, 2, 6), edit_range_of(err_item));
}

#[test]
fn test_completion_publicmain_snippet() {
    let mut t = setup();
    t.set_preference(&["java", "completion", "lazyResolveTextEdit", "enabled"], json!(false));
    t.set_preference(&["java", "completion", "matchCase"], json!("firstLetter"));
    let unit = t.get_working_copy("src/java/Foo.java", "class Foo {\n\tpublic\n};\n");
    let list = t.request_completions(&unit, "public");
    assert!(!list.is_null());
    assert_eq!(2, items(&list).len(), "{list:#}");
    let item = &items(&list)[1];
    assert_eq!("public static void main(String[] args)", s(&item["label"]));
}

// ─── Array creations ─────────────────────────────────────────────────────────

fn array_type_receiver(source: &str, insert_text: &str, label: &str) {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Arr.java", source);
    let list = t.request_completions(&unit, "new ");
    let completion_item = &items(&list)[0];
    assert_eq!(insert_text, s(&completion_item["insertText"]), "Array type completion EditText");
    assert_eq!(label, s(&completion_item["label"]), "Array type completion Label");
}

#[test]
fn test_completion_for_non_primitive_array_type_receivers() {
    array_type_receiver("public class Arr {\n\tvoid foo() {\n \t\tString[] names = new S\n\t}\n}\n", "String[]", "String[] - java.lang");
}

#[test]
fn test_completion_for_primitive_array_type_receivers() {
    array_type_receiver("public class Arr {\n\tvoid foo() {\n \t\tint[] ages = new i\n\t}\n}\n", "int[]", "int[]");
}

#[test]
fn test_completion_for_enclosing_type_array_type_receivers() {
    array_type_receiver("public class Arr {\n\tvoid foo() {\n\t\tArr[] ages = new A\n\t}\n}\n", "Arr[]", "Arr[] - java");
}

// ─── Lombok ──────────────────────────────────────────────────────────────────

/// The `when(...)` stubs of the lombok tests on top of `mockLSP3Client()`.
fn lombok_caps() -> Caps {
    Caps {
        insert_replace: true,
        item_defaults: vec!["editRange", "insertTextFormat", "insertTextMode"],
        insert_text_mode_adjust_indentation: true,
        ..Caps::lsp3()
    }
}

fn lombok_completions(source: &str) -> Value {
    let mut t = setup();
    // -javaagent:~/.m2/repository/org/projectlombok/lombok/<version>/lombok-<version>.jar
    let home = std::env::var("HOME").unwrap_or_default();
    t.ws.oracle_java_options.push(format!("-javaagent:{home}/.m2/repository/org/projectlombok/lombok/1.18.32/lombok-1.18.32.jar"));
    t.caps = lombok_caps();
    t.ws.import_projects(&["maven/mavenlombok"]);
    let uri = t.ws.class_uri("mavenlombok", "org.sample.Test");
    let original = t.ws.read(&uri);
    let mut unit = t.get_working_copy_uri(&uri, &original);
    t.change(&mut unit, source);
    t.request_completions(&unit, " = ")
}

// this test should pass when starting with -javaagent:<lombok_jar>
// https://github.com/eclipse/eclipse.jdt.ls/issues/2669
#[test]
#[ignore = "needs the Lombok javaagent in the compiler; the ECJ bridge has no Lombok support (passes on the oracle started with -javaagent)"]
fn test_completion_lombok() {
    let list = lombok_completions(
        "package org.sample;\nimport lombok.Builder;\nimport lombok.Data;\nimport lombok.Builder.Default;\n@Data\n@Builder\npublic class Test {\n      @Default\n      private Integer offset = ;\n}\n",
    );
    assert!(!list.is_null());
    assert_eq!(6, items(&list).len(), "{list:#}");
    let item_defaults = &list["itemDefaults"];
    assert!(!item_defaults.is_null());
    assert!(item_defaults["insertTextFormat"].is_null(), "{item_defaults:#}");
    assert!(item_defaults["editRange"].is_null(), "{item_defaults:#}");
}

// this test should pass when starting with -javaagent:<lombok_jar>
// https://github.com/eclipse/eclipse.jdt.ls/issues/2669
#[test]
#[ignore = "needs the Lombok javaagent in the compiler; the ECJ bridge has no Lombok support (passes on the oracle started with -javaagent)"]
fn test_completion_lombok2() {
    let list = lombok_completions(
        "package org.sample;\nimport lombok.Builder;\nimport lombok.Data;\nimport lombok.Builder.Default;\n@Data\n@Builder\npublic class Test {\n      private Integer offset = ;\n}\n",
    );
    assert!(!list.is_null());
    assert_eq!(19, items(&list).len(), "{list:#}");
    let item_defaults = &list["itemDefaults"];
    assert!(!item_defaults.is_null());
    assert!(!item_defaults["editRange"].is_null(), "{item_defaults:#}");
}

// ─── Java 17 ─────────────────────────────────────────────────────────────────

#[test]
fn test_completion_record() {
    let mut t = setup();
    use_project(&mut t, "java17");
    let unit = t.get_working_copy(
        "src/foo/bar/Foo.java",
        "package foo.bar;\n\npublic class Foo() {\n\n\tstatic record MyRecordKind(int i){}\n\n\tprivate MyRecordKin\n}\n",
    );
    let list = t.request_completions(&unit, "private MyRecordKin");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    let item = &items(&list)[0];
    assert_eq!(KIND_STRUCT, item["kind"]);
    assert_eq!("MyRecordKind", s(&item["insertText"]));
}

#[test]
fn test_completion_annotation_param() {
    let mut t = setup();
    use_project(&mut t, "java17");
    let unit = t.get_working_copy("src/foo/bar/Foo.java", "package foo.bar;\n\n@Deprecated()\npublic class Foo() {\n}\n");
    let list = t.request_completions(&unit, "@Deprecated(");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty());
    for item in items(&list) {
        assert_eq!(KIND_FIELD, item["kind"], "{item:#}");
    }
}

// ─── Overload order and collapsing ───────────────────────────────────────────

const OVERLOADS_SOURCE: &str = "package org.sample\npublic class Test {\n\tpublic void test(String x){}\n\tpublic void test(String x, int y){}\n\tpublic void test(String x, int y, boolean z){}\n\tpublic static void main(String[] args) {\n\t\t  Test obj = new Test();\n\t\t  obj.test\n\t}\n}";

#[test]
fn test_completion_order() {
    let mut t = setup();
    t.caps.label_details = true;
    let unit = t.get_working_copy("src/org/sample/Test.java", OVERLOADS_SOURCE);
    let list = t.request_completions(&unit, "obj.test");
    assert!(!items(&list).is_empty());
    assert!(s(&items(&list)[0]["filterText"]).starts_with("test(String x)"), "{list:#}");
    assert!(s(&items(&list)[1]["filterText"]).starts_with("test(String x, int y)"), "{list:#}");
    assert!(s(&items(&list)[2]["filterText"]).starts_with("test(String x, int y, boolean z)"), "{list:#}");
}

#[test]
fn test_completion_collapse() {
    let mut t = setup();
    t.caps.label_details = true;
    t.set_preference(&["java", "completion", "collapseCompletionItems"], json!(true));
    let unit = t.get_working_copy("src/org/sample/Test.java", OVERLOADS_SOURCE);
    let list = t.request_completions(&unit, "obj.test");
    assert!(!items(&list).is_empty());
    assert!(s(&items(&list)[0]["labelDetails"]["detail"]).starts_with("(...)"), "{list:#}");
    assert!(s(&items(&list)[0]["labelDetails"]["description"]).starts_with("3 overloads"), "{list:#}");
}

#[test]
fn test_completion_collapse_extends() {
    let mut t = setup();
    t.caps.label_details = true;
    t.set_preference(&["java", "completion", "collapseCompletionItems"], json!(true));
    let unit = t.get_working_copy(
        "src/org/sample/Test.java",
        "package org.sample\nclass Test extends TestSuper {\n\tpublic void test(String x){}\n\tpublic static void main(String[] args) {\n\t\t  Test obj = new Test();\n\t\t  obj.test\n\t}\n}\npublic class TestSuper {\n\tpublic void test(String x, int y){}\n}",
    );
    let list = t.request_completions(&unit, "obj.test");
    assert!(!items(&list).is_empty());
    assert!(s(&items(&list)[0]["labelDetails"]["detail"]).starts_with("(...)"), "{list:#}");
    assert!(s(&items(&list)[0]["labelDetails"]["description"]).starts_with("2 overloads"), "{list:#}");
}
