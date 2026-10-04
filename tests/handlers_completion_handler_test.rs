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
#[ignore = "JDK-dependent: the test JDK (11+) has TimeUnit.of(ChronoUnit), a tenth proposal; upstream runs on a Java 8 stub JRE (the oracle on JDK 25 also returns 10)"]
fn test_completion_import_static() {
    let mut t = setup();
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
#[ignore = "JDK-dependent: upstream's stub JRE yields 6 Method* type proposals before the `method` snippet (items[6]); a real JDK yields 50 (the list is identical to jdt.ls 1.58 on the same JDK)"]
fn test_snippet_interface_method() {
    let mut t = setup();
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
