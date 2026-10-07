//! Port of jdt.ls `AbstractQuickFixTest`, `AbstractSelectionTest` and
//! `CodeActionUtil` on top of the LSP harness.
//!
//! * [`QuickFixTest::new`] sets up the client like the jdt.ls test
//!   `PreferenceManager` mock: every code action kind supported, no
//!   `codeAction/resolve`, no resource operations, the prompt-based source
//!   actions supported, `java.codeGeneration.generateComments = true` and
//!   `java.quickfix.showAt = problem`.
//! * [`QuickFixTest::evaluate_code_actions`] requests code actions once per
//!   problem of the unit (range = the problem start, with all the unit's
//!   diagnostics as context), [`QuickFixTest::evaluate_code_actions_range`]
//!   once for a range; ignored kinds (default `source.*`), ignored command
//!   titles and `only` kinds are applied like upstream.
//! * [`QuickFixTest::assert_code_actions`] & co. compare titles, kinds and
//!   the resulting document text exactly like `assertCodeActions`.

#![allow(dead_code)]

use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

use super::jdtls::{apply_edits, dos2unix, Workspace};

/// `AbstractQuickFixTest.Expected`.
#[derive(Clone, Debug)]
pub struct Expected {
    pub name: String,
    pub content: String,
    pub kind: String,
}

impl Expected {
    /// `new Expected(name, content)` (any kind).
    pub fn new(name: &str, content: &str) -> Self {
        Expected { name: name.to_owned(), content: content.to_owned(), kind: "*".to_owned() }
    }

    /// `new Expected(name, content, kind)`.
    pub fn with_kind(name: &str, content: &str, kind: &str) -> Self {
        Expected { name: name.to_owned(), content: content.to_owned(), kind: kind.to_owned() }
    }
}

/// `IProblem.UndefinedType`.
const UNDEFINED_TYPE: &str = "16777218";

pub struct QuickFixTest {
    pub ws: Workspace,
    ignored_commands: Vec<String>,
    ignored_kinds: Vec<String>,
    only: Option<Vec<String>>,
    opened: Vec<String>,
    /// `AbstractSelectionTest`: `getRange(cu, problem)` is the marked
    /// `/*[*/ ... /*]*/` selection instead of the problem start.
    selection: bool,
}

impl Default for QuickFixTest {
    fn default() -> Self {
        Self::new()
    }
}

/// Client capabilities of the jdt.ls test `ClientPreferences` mock.
pub fn quickfix_client_capabilities() -> Value {
    let mut caps = super::jdtls::default_client_capabilities();
    caps["workspace"]["workspaceEdit"] = json!({ "documentChanges": true });
    // `isWorkspaceConfigurationSupported()` is false in the mock.
    caps["workspace"]["configuration"] = json!(false);
    caps["textDocument"]["codeAction"] = json!({
        "codeActionLiteralSupport": { "codeActionKind": { "valueSet": [""] } }
    });
    caps["textDocument"]["publishDiagnostics"] = json!({ "tagSupport": { "valueSet": [1, 2] } });
    caps
}

impl QuickFixTest {
    pub fn new() -> Self {
        let mut ws = Workspace::new();
        ws.capabilities = quickfix_client_capabilities();
        ws.init_options["extendedClientCapabilities"] = json!({
            "classFileContentsSupport": true,
            "overrideMethodsPromptSupport": true,
            "hashCodeEqualsPromptSupport": true,
            "generateToStringPromptSupport": true,
            "advancedGenerateAccessorsSupport": true,
            "generateConstructorsPromptSupport": true,
            "generateDelegateMethodsPromptSupport": true
        });
        ws.settings = json!({ "java": { "codeGeneration": { "generateComments": true }, "quickfix": { "showAt": "problem" } } });
        QuickFixTest { ws, ignored_commands: Vec::new(), ignored_kinds: vec!["source.*".to_owned()], only: None, opened: Vec::new(), selection: false }
    }

    /// `AbstractSelectionTest`: code actions are requested for the marked
    /// selection (`CodeActionUtil.getRange(cu)`).
    pub fn set_selection_test(&mut self) {
        self.selection = true;
    }

    /// `setIgnoredCommands(..)` (regular expressions on titles).
    pub fn set_ignored_commands(&mut self, commands: &[&str]) {
        self.ignored_commands = commands.iter().map(|s| s.to_string()).collect();
    }

    /// `setIgnoredKind(..)` (regular expressions on kinds).
    pub fn set_ignored_kind(&mut self, kinds: &[&str]) {
        self.ignored_kinds = kinds.iter().map(|s| s.to_string()).collect();
    }

    /// `setOnly(..)`.
    pub fn set_only(&mut self, kinds: &[&str]) {
        self.only = Some(kinds.iter().map(|s| s.to_string()).collect());
    }

    /// The unit's diagnostics (`DiagnosticsHandler.toDiagnosticsArray` of
    /// the AST problems).
    pub fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        if !self.opened.iter().any(|u| u == uri) {
            self.opened.push(uri.to_owned());
            self.ws.open(uri);
        }
        self.ws.diagnostics(uri)
    }

    /// `AbstractQuickFixTest.evaluateCodeActions(cu)`.
    pub fn evaluate_code_actions(&mut self, uri: &str) -> Vec<Value> {
        let diagnostics = self.diagnostics(uri);
        let text = self.ws.read(uri);
        let mut result = Vec::new();
        if diagnostics.is_empty() {
            let range = if self.selection { get_selection_range(&text) } else { json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }) };
            result.extend(self.request_code_actions(uri, range, &diagnostics));
        } else {
            for d in &diagnostics {
                let range = if self.selection { get_selection_range(&text) } else { problem_range(&text, d) };
                result.extend(self.request_code_actions(uri, range, &diagnostics));
            }
        }
        result
    }

    /// `AbstractQuickFixTest.evaluateCodeActions(cu, range)`.
    pub fn evaluate_code_actions_range(&mut self, uri: &str, range: Value) -> Vec<Value> {
        let diagnostics = self.diagnostics(uri);
        self.request_code_actions(uri, range, &diagnostics)
    }

    fn request_code_actions(&mut self, uri: &str, range: Value, diagnostics: &[Value]) -> Vec<Value> {
        let mut context = json!({ "diagnostics": diagnostics });
        if let Some(only) = &self.only {
            context["only"] = json!(only);
        }
        let result = self.ws.request("textDocument/codeAction", json!({ "textDocument": { "uri": uri }, "range": range, "context": context }));
        let mut actions: Vec<Value> = result.as_array().cloned().unwrap_or_default();
        if let Some(only) = &self.only {
            for a in &actions {
                let kind = a["kind"].as_str().unwrap_or("");
                assert!(only.iter().any(|k| !kind.is_empty() && kind.starts_with(k.as_str())), "{} has kind {} but only {:?} are accepted", a["title"], kind, only);
            }
        }
        actions.retain(|a| {
            if is_command(a) {
                return true;
            }
            let kind = a["kind"].as_str().unwrap_or("");
            !self.ignored_kinds.iter().any(|k| full_match(k, kind))
        });
        actions.retain(|a| {
            let title = get_title(a);
            !self.ignored_commands.iter().any(|c| full_match(c, &title))
        });
        actions
    }

    /// `assertCodeActions(cu, expecteds...)`.
    pub fn assert_code_actions(&mut self, uri: &str, expected: &[Expected]) {
        let actions = self.evaluate_code_actions(uri);
        self.assert_code_actions_list(&actions, expected);
    }

    /// `assertCodeActions(cu, range, expecteds...)`.
    pub fn assert_code_actions_range(&mut self, uri: &str, range: Value, expected: &[Expected]) {
        let actions = self.evaluate_code_actions_range(uri, range);
        self.assert_code_actions_list(&actions, expected);
    }

    /// `assertCodeActions(codeActions, expecteds...)`.
    pub fn assert_code_actions_list(&mut self, actions: &[Value], expected: &[Expected]) {
        if actions.len() < expected.len() {
            let res: Vec<String> = actions.iter().map(|a| format!("'{}'", get_title(a))).collect();
            assert_eq!(expected.len(), actions.len(), "Number of code actions: {}", res.join(","));
        }
        for e in expected {
            let action = actions.iter().find(|a| get_title(a) == e.name);
            let action = action.unwrap_or_else(|| {
                panic!("Should prompt code action: {}\nactual: {:?}", e.name, actions.iter().map(get_title).collect::<Vec<_>>())
            });
            self.assert_equivalent(e, action);
        }
        let mut a_str = String::new();
        let mut e_str = String::new();
        for a in actions {
            let title = get_title(a);
            if let Some(e) = expected.iter().find(|e| e.name == title) {
                let actual = self.evaluate_code_action_command(a);
                let content = dos2unix(&e.content);
                if content != actual {
                    a_str.push_str(&format!("\n{title}\n{actual}"));
                    e_str.push_str(&format!("\n{}\n{}", e.name, content));
                }
            }
        }
        assert_eq!(e_str, a_str);
    }

    /// `Expected.assertEquivalent(action)`.
    fn assert_equivalent(&mut self, e: &Expected, action: &Value) {
        let title = get_title(action);
        assert_eq!(e.name, title, "Unexpected command :");
        if e.kind != "*" && !is_command(action) {
            assert_eq!(e.kind, action["kind"].as_str().unwrap_or(""), "{title} has the wrong kind ");
        }
        let actual = self.evaluate_code_action_command(action);
        assert_eq!(dos2unix(&e.content), actual, "{title} has the wrong content ");
    }

    /// `assertCodeActionExists(cu, expected)`.
    pub fn assert_code_action_exists_expected(&mut self, uri: &str, expected: &Expected) {
        let actions = self.evaluate_code_actions(uri);
        if let Some(a) = actions.iter().find(|a| get_title(a) == expected.name) {
            let a = a.clone();
            self.assert_equivalent(expected, &a);
            return;
        }
        let all: Vec<String> = actions.iter().map(get_title).collect();
        panic!("{} not found in {}", expected.name, all.join("\n"));
    }

    /// `assertCodeActionExists(cu, label)`.
    pub fn assert_code_action_exists(&mut self, uri: &str, label: &str) {
        let actions = self.evaluate_code_actions(uri);
        assert!(actions.iter().any(|a| get_title(a) == label), "'{label}' should exist within the code actions");
    }

    /// `assertCodeActionExists(cu, labels[])`.
    pub fn assert_code_actions_exist(&mut self, uri: &str, labels: &[&str]) {
        let actions = self.evaluate_code_actions(uri);
        for label in labels {
            assert!(actions.iter().any(|a| get_title(a) == *label), "'{label}' should exist within the code actions");
        }
    }

    /// `assertCodeActionNotExists(cu, label)`.
    pub fn assert_code_action_not_exists(&mut self, uri: &str, label: &str) {
        let actions = self.evaluate_code_actions(uri);
        assert!(!actions.iter().any(|a| get_title(a) == label), "'{label}' should not be added to the code actions");
    }

    /// `assertCodeActionNotExists(cu, range, label)`.
    pub fn assert_code_action_not_exists_range(&mut self, uri: &str, range: Value, label: &str) {
        let actions = self.evaluate_code_actions_range(uri, range);
        assert!(!actions.iter().any(|a| get_title(a) == label), "'{label}' should not be added to the code actions");
    }

    /// `evaluateCodeActionCommand(codeAction)`.
    pub fn evaluate_code_action_command(&mut self, action: &Value) -> String {
        assert!(!is_command(action), "Expected CodeAction, got Command: {action}");
        let edit = &action["edit"];
        assert!(!edit.is_null(), "No edits generated: {action}");
        evaluate_workspace_edit(&self.ws, edit).unwrap_or_default()
    }
}

/// `AbstractQuickFixTest.evaluateWorkspaceEdit(edit)`: the single modified
/// document's new content.
pub fn evaluate_workspace_edit(ws: &Workspace, edit: &Value) -> Option<String> {
    if edit.is_null() {
        return None;
    }
    if let Some(changes) = edit["documentChanges"].as_array() {
        let docs: Vec<&Value> = changes.iter().filter(|c| c.get("textDocument").is_some()).collect();
        assert!(!docs.is_empty(), "No edits generated");
        let mut uris: Vec<&str> = docs.iter().filter_map(|d| d["textDocument"]["uri"].as_str()).collect();
        uris.dedup();
        assert_eq!(1, uris.len(), "Only one resource should be modified");
        let edits: Vec<Value> = docs.iter().flat_map(|d| d["edits"].as_array().cloned().unwrap_or_default()).collect();
        return Some(evaluate_changes(ws, uris[0], &edits));
    }
    let changes = edit["changes"].as_object().expect("changes");
    let mut it = changes.iter();
    let (uri, edits) = it.next().expect("No edits generated");
    assert!(it.next().is_none(), "More than one resource modified");
    Some(evaluate_changes(ws, uri, edits.as_array().map(Vec::as_slice).unwrap_or(&[])))
}

fn evaluate_changes(ws: &Workspace, uri: &str, edits: &[Value]) -> String {
    assert!(!edits.is_empty(), "No edits generated: {edits:?}");
    let path = Url::parse(uri).unwrap().to_file_path().unwrap();
    let source = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = ws;
    dos2unix(&apply_edits(&source, edits))
}

pub fn is_command(a: &Value) -> bool {
    a["command"].is_string()
}

/// `getTitle(codeAction)`.
pub fn get_title(a: &Value) -> String {
    let title = if is_command(a) {
        a["title"].as_str()
    } else if a["command"].is_object() {
        a["command"]["title"].as_str()
    } else {
        a["title"].as_str()
    };
    dos2unix(title.unwrap_or(""))
}

fn full_match(pattern: &str, s: &str) -> bool {
    regex::Regex::new(&format!("^(?:{pattern})$")).is_ok_and(|r| r.is_match(s))
}

/// `getRange(cu, problem)`: the problem start (`problem.getSourceStart()`).
fn problem_range(text: &str, d: &Value) -> Value {
    let mut start = d["range"]["start"].clone();
    if d["code"].as_str() == Some(UNDEFINED_TYPE) {
        // The diagnostic of an undefined annotation type starts at `@`.
        let line = start["line"].as_u64().unwrap_or(0) as usize;
        let ch = start["character"].as_u64().unwrap_or(0) as usize;
        if let Some(l) = text.lines().nth(line) {
            let units: Vec<u16> = l.encode_utf16().collect();
            if units.get(ch) == Some(&(b'@' as u16)) {
                let mut c = ch + 1;
                while units.get(c).is_some_and(|u| *u == b' ' as u16 || *u == b'\t' as u16) {
                    c += 1;
                }
                start["character"] = json!(c);
            }
        }
    }
    // Diagnostics already carry positions in the current working copy. A
    // round-trip through disk text moves the cursor when an unsaved buffer has
    // different lines (AbstractQuickFixTest.getRange uses that working copy).
    json!({ "start": start, "end": start })
}

fn line_starts(text: &str) -> Vec<usize> {
    let v: Vec<u16> = text.encode_utf16().collect();
    let mut starts = vec![0];
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'\r' as u16 {
            if v.get(i + 1) == Some(&(b'\n' as u16)) {
                i += 1;
            }
            starts.push(i + 1);
        } else if v[i] == b'\n' as u16 {
            starts.push(i + 1);
        }
        i += 1;
    }
    starts
}

fn to_offset(text: &str, pos: &Value) -> usize {
    let starts = line_starts(text);
    let line = pos["line"].as_u64().unwrap_or(0) as usize;
    starts.get(line).copied().unwrap_or(0) + pos["character"].as_u64().unwrap_or(0) as usize
}

fn to_line(text: &str, offset: i64) -> Option<(usize, usize)> {
    let len = text.encode_utf16().count() as i64;
    if offset < 0 || offset > len {
        return None;
    }
    let starts = line_starts(text);
    let offset = offset as usize;
    let line = match starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i - 1,
    };
    Some((line, offset - starts[line]))
}

/// `JDTUtils.toRange(cu, offset, length)`.
pub fn to_range(text: &str, offset: i64, length: i64) -> Value {
    let (mut s, mut e) = ((0, 0), (0, 0));
    if offset > 0 || length > 0 {
        s = to_line(text, offset).unwrap_or((0, 0));
        e = to_line(text, offset + length).unwrap_or((0, 0));
    }
    json!({ "start": { "line": s.0, "character": s.1 }, "end": { "line": e.0, "character": e.1 } })
}

fn index_of16(text: &str, search: &str, last: bool) -> i64 {
    let t: Vec<u16> = text.encode_utf16().collect();
    let s: Vec<u16> = search.encode_utf16().collect();
    if s.len() > t.len() {
        return -1;
    }
    let range: Box<dyn Iterator<Item = usize>> = if last { Box::new((0..=t.len() - s.len()).rev()) } else { Box::new(0..=t.len() - s.len()) };
    for i in range {
        if t[i..i + s.len()] == s[..] {
            return i as i64;
        }
    }
    -1
}

/// `CodeActionUtil.getRange(unit, search)` (last occurrence).
pub fn get_range(text: &str, search: &str) -> Value {
    get_range_len(text, search, search.encode_utf16().count() as i64)
}

/// `CodeActionUtil.getRange(unit, search, length)`.
pub fn get_range_len(text: &str, search: &str, length: i64) -> Value {
    let start = index_of16(text, search, true);
    to_range(text, start, length)
}

/// `CodeActionUtil.getSelection(source)`: `[start, length]` of the
/// `/*[*/ ... /*]*/` markers.
pub fn get_selection(source: &str) -> (i64, i64) {
    const OPEN: &str = "/*[*/";
    const CLOSE: &str = "/*]*/";
    let mut including_start = index_of16(source, OPEN, false);
    let mut excluding_start = index_of16(source, CLOSE, false);
    let mut including_end = index_of16(source, CLOSE, true);
    let mut excluding_end = index_of16(source, OPEN, true);
    if including_start > excluding_start && excluding_start != -1 {
        including_start = -1;
    } else if excluding_start > including_start && including_start != -1 {
        excluding_start = -1;
    }
    if including_end < excluding_end {
        including_end = -1;
    } else if excluding_end < including_end {
        excluding_end = -1;
    }
    let start = if including_start != -1 { including_start } else { excluding_start + 5 };
    let end = if excluding_end != -1 { excluding_end } else { including_end + 5 };
    (start, end - start)
}

/// `CodeActionUtil.getRange(cu)`: the marked selection.
pub fn get_selection_range(text: &str) -> Value {
    let (s, l) = get_selection(text);
    to_range(text, s, l)
}
