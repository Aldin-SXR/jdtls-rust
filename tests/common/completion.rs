//! Shared support for the ports of the jdt.ls completion test classes
//! (`CompletionHandlerTest` and its siblings): the mocked
//! `ClientPreferences` as LSP client capabilities ([`Caps`]), the
//! `AbstractCompilationUnitBasedTest` fixture ([`T`]) and assertion helpers.

#![allow(dead_code)]

use serde_json::{json, Value};

use super::jdtls::*;

// ─── Client capabilities (mocked ClientPreferences) ─────────────────────────

#[derive(Clone, Default)]
pub struct Caps {
    pub snippets: bool,
    pub signature_help: bool,
    pub label_details: bool,
    pub resolve_documentation: bool,
    pub resolve_additional_text_edits: bool,
    pub markdown: bool,
    pub insert_replace: bool,
    pub item_defaults: Vec<&'static str>,
    pub insert_text_mode_adjust_indentation: bool,
    /// `getCompletionItemInsertTextModeDefault`: 1 AsIs, 2 AdjustIndentation.
    pub insert_text_mode_default: Option<u32>,
    pub tag_support: bool,
}

impl Caps {
    /// `mockLSP3Client()`.
    pub fn lsp3() -> Self {
        Caps { snippets: true, signature_help: true, ..Default::default() }
    }
    /// `mockLSP2Client()`.
    pub fn lsp2() -> Self {
        Caps::default()
    }
    /// `mockClientPreferences(snippets, signatureHelp, itemDefaults)`.
    pub fn mock(snippets: bool, signature_help: bool, item_defaults: bool) -> Self {
        Caps {
            snippets,
            signature_help,
            item_defaults: if item_defaults { vec!["editRange", "insertTextFormat", "insertTextMode"] } else { vec![] },
            insert_text_mode_adjust_indentation: true,
            ..Default::default()
        }
    }

    pub fn to_json(&self) -> Value {
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

pub struct T {
    pub ws: Workspace,
    pub caps: Caps,
    /// `project`: the project `getWorkingCopy` paths are relative to.
    pub project: &'static str,
}

pub fn settings() -> Value {
    settings_with(false, false)
}

/// The settings with `postfix` completion and lazy text-edit resolution set.
pub fn settings_with(postfix: bool, lazy_resolve_text_edit: bool) -> Value {
    json!({
        "java": {
            "completion": { "postfix": { "enabled": postfix }, "lazyResolveTextEdit": { "enabled": lazy_resolve_text_edit } },
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
pub fn setup() -> T {
    setup_with(settings())
}

/// `AbstractCompilationUnitBasedTest.setup` with the given settings.
pub fn setup_with(settings: Value) -> T {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.settings = settings;
    T { ws, caps: Caps::lsp3(), project: "hello" }
}

#[derive(Clone)]
pub struct Unit {
    pub uri: String,
    pub text: String,
}

/// `findCompletionLocation`: after the last (or first after `from`) occurrence.
pub fn find_completion_location(text: &str, behind: &str, from: usize) -> (u32, u32) {
    let idx = if from > 0 { from + text[from..].find(behind).unwrap() } else { text.rfind(behind).unwrap_or_else(|| panic!("{behind:?} not in source")) };
    let offset = idx + behind.len();
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = text[line_start..offset].encode_utf16().count() as u32;
    (line, character)
}

impl T {
    pub fn start(&mut self) {
        self.ws.capabilities = self.caps.to_json();
        self.ws.client();
    }

    /// `getWorkingCopy(path, source)`: the unit `path` of project `hello` with `source`.
    pub fn get_working_copy(&mut self, path: &str, source: &str) -> Unit {
        let root = self.ws.project_root(self.project);
        let uri = url::Url::from_file_path(root.join(path)).unwrap().to_string();
        self.ws.capabilities = self.caps.to_json();
        self.ws.open_with(&uri, source);
        Unit { uri, text: source.to_owned() }
    }

    /// `getWorkingCopy` of an existing unit `uri`.
    pub fn get_working_copy_uri(&mut self, uri: &str, source: &str) -> Unit {
        self.ws.capabilities = self.caps.to_json();
        self.ws.open_with(uri, source);
        Unit { uri: uri.to_owned(), text: source.to_owned() }
    }

    pub fn change(&mut self, unit: &mut Unit, text: &str) {
        self.ws.change(&unit.uri, text);
        unit.text = text.to_owned();
    }

    pub fn completion_at(&mut self, uri: &str, line: u32, character: u32) -> Value {
        self.ws.request(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        )
    }

    pub fn request_completions(&mut self, unit: &Unit, behind: &str) -> Value {
        self.request_completions_from(unit, behind, 0)
    }

    pub fn request_completions_from(&mut self, unit: &Unit, behind: &str, from: usize) -> Value {
        let (line, character) = find_completion_location(&unit.text, behind, from);
        self.completion_at(&unit.uri, line, character)
    }

    pub fn resolve(&mut self, item: &Value) -> Value {
        self.ws.request("completionItem/resolve", item.clone())
    }

    pub fn set_preference(&mut self, path: &[&str], value: Value) {
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

pub fn items(list: &Value) -> Vec<Value> {
    list["items"].as_array().cloned().unwrap_or_default()
}

pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

/// `Lsp4jAssertions.assertTextEdit`.
pub fn assert_text_edit(line: u64, start: u64, end: u64, text: &str, edit: &Value) {
    assert!(!edit.is_null(), "no text edit");
    assert_eq!(text, s(&edit["newText"]), "{edit:#}");
    assert_eq!(line, edit["range"]["start"]["line"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(start, edit["range"]["start"]["character"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(line, edit["range"]["end"]["line"].as_u64().unwrap(), "{edit:#}");
    assert_eq!(end, edit["range"]["end"]["character"].as_u64().unwrap(), "{edit:#}");
}

pub fn assert_position(line: u64, character: u64, pos: &Value) {
    assert_eq!(line, pos["line"].as_u64().unwrap(), "{pos}");
    assert_eq!(character, pos["character"].as_u64().unwrap(), "{pos}");
}

pub const KIND_TEXT: u64 = 1;
pub const KIND_METHOD: u64 = 2;
pub const KIND_CONSTRUCTOR: u64 = 4;
pub const KIND_FIELD: u64 = 5;
pub const KIND_VARIABLE: u64 = 6;
pub const KIND_CLASS: u64 = 7;
pub const KIND_INTERFACE: u64 = 8;
pub const KIND_MODULE: u64 = 9;
pub const KIND_PROPERTY: u64 = 10;
pub const KIND_ENUM: u64 = 13;
pub const KIND_KEYWORD: u64 = 14;
pub const KIND_SNIPPET: u64 = 15;
pub const KIND_ENUM_MEMBER: u64 = 20;
pub const KIND_CONSTANT: u64 = 21;
pub const KIND_STRUCT: u64 = 22;
pub const FORMAT_PLAIN_TEXT: u64 = 1;
pub const FORMAT_SNIPPET: u64 = 2;
pub const MODE_AS_IS: u64 = 1;
pub const MODE_ADJUST_INDENTATION: u64 = 2;

pub fn regex_full_match(pattern: &str, input: &str) -> bool {
    regex::Regex::new(&format!("^(?:{pattern})$")).unwrap().is_match(input)
}

