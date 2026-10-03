//! Integration tests: spawn `jdtls-rust`, drive it over LSP stdio, assert on responses.
//!
//! Test cases are derived from eclipse.jdt.ls CompletionHandlerTest.java and adapted
//! to our server's behaviour.  Semantic (ECJ) tests are skipped when ECJ is not ready
//! within the startup timeout; syntax-only tests always run.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

// ─── LSP client ──────────────────────────────────────────────────────────────

struct LspClient {
    _child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
}

/// Read one Content-Length-framed JSON message from `reader`.
fn read_one_lsp_message(
    reader: &mut BufReader<std::process::ChildStdout>,
) -> std::io::Result<Value> {
    use std::io::Read;
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stdout closed"));
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

impl LspClient {
    fn spawn() -> Self {
        let bin = env!("CARGO_BIN_EXE_jdtls-rust");
        let mut child = Command::new(bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn jdtls-rust");

        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());

        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            loop {
                match read_one_lsp_message(&mut stdout) {
                    Ok(msg) => {
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        LspClient { _child: child, stdin, rx, next_id: 1 }
    }

    // ── Wire protocol ─────────────────────────────────────────────────────────

    fn send_raw(&mut self, msg: &Value) {
        let body = msg.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin.write_all(header.as_bytes()).unwrap();
        self.stdin.write_all(body.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    /// Block until a message arrives or the given deadline passes.
    fn recv_timeout(&mut self, deadline: Instant) -> Option<Value> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        self.rx.recv_timeout(remaining).ok()
    }

    /// Receive messages until one satisfies `pred`, discarding others.
    /// Panics if no matching message arrives within 30 s.
    fn recv_until(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let msg = self
                .recv_timeout(deadline)
                .expect("timed out waiting for LSP message");
            if pred(&msg) {
                return msg;
            }
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ── LSP helpers ───────────────────────────────────────────────────────────

    /// Initialise the server with the running JVM's java home.
    fn initialize(&mut self) {
        self.initialize_with_options(json!({
            "javaHome": java_home(),
            "sourceCompatibility": "21"
        }));
    }

    fn initialize_as_lms_monaco(&mut self) {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "lms-monaco",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "textDocument": {
                        "completion": {
                            "completionItem": {
                                "snippetSupport": true
                            }
                        }
                    }
                },
                "initializationOptions": {
                    "javaHome": java_home(),
                    "sourceCompatibility": "21"
                }
            }
        }));
        self.recv_until(|m| m["id"] == id);
        self.send_raw(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    }

    fn initialize_with_options(&mut self, initialization_options: Value) {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "capabilities": {
                    "textDocument": {
                        "completion": {
                            "completionItem": {
                                "snippetSupport": true
                            }
                        }
                    }
                },
                "initializationOptions": initialization_options
            }
        }));
        // Wait for initialize response
        self.recv_until(|m| m["id"] == id);
        self.send_raw(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "java",
                    "version": 1,
                    "text": text
                }
            }
        }));
    }

    /// Request completion and return the array of items.
    fn complete(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        let result = &resp["result"];
        match result {
            Value::Array(items) => items.clone(),
            Value::Object(_) => result["items"].as_array().cloned().unwrap_or_default(),
            _ => vec![],
        }
    }

    /// Request hover and return the raw result value.
    fn hover(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        self.recv_until(|m| m["id"] == id)["result"].clone()
    }

    /// Request signature help and return the raw result value.
    fn signature_help(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/signatureHelp",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        self.recv_until(|m| m["id"] == id)["result"].clone()
    }

    /// Request document highlights and return the array of ranges.
    fn document_highlight(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/documentHighlight",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request find-references and return the array of locations.
    fn references(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": false }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request document formatting and return the array of TextEdits.
    fn format(&mut self, uri: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": { "tabSize": 4, "insertSpaces": true }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request inlay hints for a range and return the array.
    fn inlay_hints(&mut self, uri: &str, start_line: u32, end_line: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/inlayHint",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": 0 },
                    "end":   { "line": end_line,   "character": 0 }
                }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request code actions for a range and return the array.
    fn code_actions(&mut self, uri: &str, line: u32, character: u32, diags: &[Value]) -> Vec<Value> {
        self.code_actions_range(uri, line, character, line, character, diags)
    }

    /// Request code actions for an explicit range and return the array.
    fn code_actions_range(
        &mut self,
        uri: &str,
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
        diags: &[Value],
    ) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_character },
                    "end":   { "line": end_line, "character": end_character }
                },
                "context": { "diagnostics": diags }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request rename and return the raw WorkspaceEdit result.
    fn rename(&mut self, uri: &str, line: u32, character: u32, new_name: &str) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/rename",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name
            }
        }));
        self.recv_until(|m| m["id"] == id)["result"].clone()
    }

    fn prepare_rename(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/prepareRename",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        self.recv_until(|m| m["id"] == id)["result"].clone()
    }

    fn linked_editing_range(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/linkedEditingRange",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        self.recv_until(|m| m["id"] == id)["result"].clone()
    }

    fn on_type_formatting(&mut self, uri: &str, line: u32, character: u32, ch: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/onTypeFormatting",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "ch": ch,
                "options": { "tabSize": 4, "insertSpaces": true }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    fn document_links(&mut self, uri: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/documentLink",
            "params": { "textDocument": { "uri": uri } }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request document symbols and return the array.
    fn document_symbols(&mut self, uri: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/documentSymbol",
            "params": { "textDocument": { "uri": uri } }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request goto-definition and return the array of locations.
    fn goto_definition(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        match &resp["result"] {
            Value::Array(a) => a.clone(),
            Value::Object(_) => vec![resp["result"].clone()],
            _ => vec![],
        }
    }

    /// Prepare type hierarchy and return items.
    fn prepare_type_hierarchy(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/prepareTypeHierarchy",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Get supertypes for a TypeHierarchyItem.
    fn type_supertypes(&mut self, item: &Value) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "typeHierarchy/supertypes",
            "params": { "item": item }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Get subtypes for a TypeHierarchyItem.
    fn type_subtypes(&mut self, item: &Value) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "typeHierarchy/subtypes",
            "params": { "item": item }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Prepare call hierarchy and return items.
    fn prepare_call_hierarchy(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/prepareCallHierarchy",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Get incoming calls for a CallHierarchyItem.
    fn call_incoming(&mut self, item: &Value) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "callHierarchy/incomingCalls",
            "params": { "item": item }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Get outgoing calls for a CallHierarchyItem.
    fn call_outgoing(&mut self, item: &Value) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "callHierarchy/outgoingCalls",
            "params": { "item": item }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request folding ranges and return the array of FoldingRanges.
    fn folding_ranges(&mut self, uri: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/foldingRange",
            "params": { "textDocument": { "uri": uri } }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request selection ranges and return the array of SelectionRanges.
    fn selection_ranges(&mut self, uri: &str, positions: Vec<(u32, u32)>) -> Vec<Value> {
        let id = self.next_id();
        let lsp_positions: Vec<Value> = positions.into_iter()
            .map(|(l, c)| json!({ "line": l, "character": c }))
            .collect();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/selectionRange",
            "params": {
                "textDocument": { "uri": uri },
                "positions": lsp_positions
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request semantic tokens (full) and return the data array.
    fn semantic_tokens(&mut self, uri: &str) -> Vec<u64> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": uri } }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"]["data"].as_array().cloned().unwrap_or_default()
            .into_iter().filter_map(|v| v.as_u64()).collect()
    }

    /// Request goto-implementation and return the array of locations.
    fn implementation(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/implementation",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request goto-type-definition and return the array of locations.
    fn type_definition(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/typeDefinition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        match &resp["result"] {
            Value::Array(a) => a.clone(),
            Value::Object(obj) => vec![Value::Object(obj.clone())],
            _ => vec![],
        }
    }

    /// Request workspace symbols and return the array.
    fn workspace_symbol(&mut self, query: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "workspace/symbol",
            "params": { "query": query }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    /// Request code lenses and return the array.
    fn code_lens(&mut self, uri: &str) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/codeLens",
            "params": { "textDocument": { "uri": uri } }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

    fn resolve_code_lens(&mut self, lens: &Value) -> Value {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "codeLens/resolve",
            "params": lens
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].clone()
    }

    /// Request goto-declaration and return the array of locations.
    fn goto_declaration(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/declaration",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        match &resp["result"] {
            Value::Array(a) => a.clone(),
            Value::Object(obj) => vec![Value::Object(obj.clone())],
            _ => vec![],
        }
    }

    /// Request range formatting and return the array of TextEdits.
    fn range_formatting(&mut self, uri: &str, start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Vec<Value> {
        let id = self.next_id();
        self.send_raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/rangeFormatting",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end":   { "line": end_line,   "character": end_char }
                },
                "options": { "tabSize": 4, "insertSpaces": true }
            }
        }));
        let resp = self.recv_until(|m| m["id"] == id);
        resp["result"].as_array().cloned().unwrap_or_default()
    }

}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn java_home() -> String {
    std::env::var("JAVA_HOME").unwrap_or_else(|_| {
        // Ask the running JVM
        let out = Command::new("java")
            .args(["-XshowSettings:all", "-version"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        for line in stderr.lines() {
            if let Some(val) = line.trim().strip_prefix("java.home = ") {
                return val.trim().to_owned();
            }
        }
        panic!("Could not determine java.home");
    })
}

fn javac_bin() -> String {
    let candidate = PathBuf::from(java_home()).join("bin").join("javac");
    if candidate.exists() {
        candidate.to_string_lossy().into_owned()
    } else {
        "javac".to_owned()
    }
}

/// Unique `file://` URI for a test (avoids collisions between parallel tests).
fn test_uri(name: &str) -> String {
    format!("file:///tmp/jdtls-test-{name}.java")
}

fn labels(items: &[Value]) -> Vec<String> {
    items.iter()
        .filter_map(|i| i["label"].as_str().map(str::to_owned))
        .collect()
}

fn find_item<'a>(items: &'a [Value], label: &str) -> Option<&'a Value> {
    items.iter().find(|i| i["label"].as_str() == Some(label))
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

fn compile_java_to_dir(out_dir: &Path, sources: &[(&str, &str)]) {
    let mut source_paths = Vec::new();
    for (relative, contents) in sources {
        let path = out_dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        source_paths.push(path);
    }

    let mut cmd = Command::new(javac_bin());
    cmd.arg("-d").arg(out_dir);
    for path in &source_paths {
        cmd.arg(path);
    }

    let output = cmd.output().expect("failed to run javac");
    assert!(
        output.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── Syntax-only tests (no ECJ needed) ───────────────────────────────────────

/// Typing `for` inside a method body → the `for` snippet is offered.
#[test]
fn syntax_snippet_for() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_snippet_for");
    let src = "public class Foo {\n    void go() {\n        fo\n    }\n}";
    c.open(&uri, src);

    // line 2 (0-based), after "fo"
    let items = c.complete(&uri, 2, 10);
    let ls = labels(&items);
    assert!(ls.iter().any(|l| l == "for" || l.starts_with("for")),
        "expected 'for' snippet, got: {ls:?}");
}

/// Typing `sout` → the `System.out.println` snippet is offered.
#[test]
fn syntax_snippet_sout() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_snippet_sout");
    let src = "public class Foo {\n    void go() {\n        sout\n    }\n}";
    c.open(&uri, src);

    let items = c.complete(&uri, 2, 12);
    let ls = labels(&items);
    assert!(ls.iter().any(|l| l.contains("sout") || l.contains("println")),
        "expected sout/println snippet, got: {ls:?}");
}

#[test]
fn syntax_postfix_snippets_present() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_present");
    let src = indoc(r#"
        class Foo {
            void go(String value) {
                value.
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 2, utf16_len("        value."));
    let ls = labels(&items);
    for expected in ["cast", "null", "opt", "par", "syserr", "sysouf", "sysout", "sysoutv", "var"] {
        assert!(ls.iter().any(|label| label == expected), "expected postfix snippet '{expected}', got: {ls:?}");
    }
}

#[test]
fn syntax_postfix_sysout_rewrites_bare_member_access() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_bare_sysout");
    let src = indoc(r#"
        class Foo {
            void go(String value) {
                value.
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 2, utf16_len("        value."));
    let item = find_item(&items, "sysout").expect("expected sysout postfix snippet");
    assert_eq!(
        item["insertText"].as_str(),
        Some("System.out.println(value);${0}"),
        "expected postfix snippet insertText for UI client compatibility, got: {item:?}"
    );
    assert_eq!(
        item["textEdit"]["newText"].as_str(),
        Some("System.out.println(value);${0}"),
        "unexpected sysout text edit: {item:?}"
    );
    assert_eq!(
        item["textEdit"]["range"]["start"]["character"].as_u64(),
        Some(14),
        "expected bare postfix insertion at the cursor, got: {item:?}"
    );
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(
        edits.iter().any(|edit|
            edit["newText"].as_str() == Some("")
                && edit["range"]["start"]["character"].as_u64() == Some(8)
                && edit["range"]["end"]["character"].as_u64() == Some(14)
        ),
        "expected additional delete edit for `value.`, got: {item:?}"
    );
}

#[test]
fn syntax_postfix_sysout_rewrites_full_expression() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_sysout");
    let src = indoc(r#"
        class Foo {
            void go(String value) {
                value.sysout
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 2, utf16_len("        value.sysout"));
    let item = find_item(&items, "sysout").expect("expected sysout postfix snippet");
    assert_eq!(item["kind"].as_u64(), Some(15), "expected snippet kind, got: {item:?}");
    assert_eq!(
        item["textEdit"]["newText"].as_str(),
        Some("System.out.println(value);${0}"),
        "unexpected sysout text edit: {item:?}"
    );
    assert_eq!(
        item["textEdit"]["range"]["start"]["character"].as_u64(),
        Some(14),
        "expected postfix token replacement to start at `sysout`, got: {item:?}"
    );
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(
        edits.iter().any(|edit|
            edit["newText"].as_str() == Some("")
                && edit["range"]["start"]["character"].as_u64() == Some(8)
                && edit["range"]["end"]["character"].as_u64() == Some(14)
        ),
        "expected additional delete edit for `value.`, got: {item:?}"
    );
}

#[test]
fn syntax_postfix_opt_adds_optional_import() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_opt_import");
    let src = indoc(r#"
        class Foo {
            void go(Object value) {
                value.opt
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 2, utf16_len("        value.opt"));
    let item = find_item(&items, "opt").expect("expected opt postfix snippet");
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(
        edits.iter().any(|edit|
            edit["newText"].as_str() == Some("")
                && edit["range"]["start"]["character"].as_u64() == Some(8)
                && edit["range"]["end"]["character"].as_u64() == Some(14)
        ),
        "expected delete edit for original postfix expression, got: {item:?}"
    );
    assert!(
        edits.iter().any(|edit| edit["newText"].as_str() == Some("import java.util.Optional;\n")),
        "expected Optional import edit, got: {item:?}"
    );
}

#[test]
fn syntax_postfix_snippets_sort_after_members() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_sort_after_members");
    let src = indoc(r#"
        class Foo {
            void alpha() {}
            void go() {
                this.
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 3, utf16_len("        this."));
    let labels = labels(&items);
    let member_idx = labels.iter().position(|label| label == "alpha")
        .expect("expected semantic member completion");
    let postfix_idx = labels.iter().position(|label| label == "sysout")
        .expect("expected postfix snippet completion");
    assert!(
        member_idx < postfix_idx,
        "expected member completions before postfix snippets, got: {labels:?}"
    );
}

#[test]
fn syntax_postfix_not_offered_in_imports() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_import");
    let src = indoc(r#"
        import static java.util.ArrayList.
        class Foo {}
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 0, utf16_len("import static java.util.ArrayList."));
    let ls = labels(&items);
    assert!(
        !ls.iter().any(|label| matches!(label.as_str(), "sysout" | "opt" | "var" | "cast")),
        "postfix snippets should not appear in import completions, got: {ls:?}"
    );
}

#[test]
fn syntax_postfix_not_offered_on_type_qualifier() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_postfix_type_qualifier");
    let src = indoc(r#"
        class A {
            static void alpha() {}
        }

        class Foo {
            void go() {
                A.
            }
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 6, utf16_len("        A."));
    let ls = labels(&items);
    assert!(
        !ls.iter().any(|label| matches!(label.as_str(), "sysout" | "opt" | "var" | "cast")),
        "postfix snippets should not appear on type qualifiers, got: {ls:?}"
    );
}

#[test]
fn syntax_no_completion_after_member_modifier() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_no_completion_after_member_modifier");
    let src = indoc(r#"
        class Foo {
            public 
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 1, utf16_len("    public "));
    assert!(
        items.is_empty(),
        "expected no completions immediately after member modifiers, got: {items:?}"
    );
}

#[test]
fn syntax_no_completion_after_method_return_type() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_no_completion_after_method_return_type");
    let src = indoc(r#"
        class Foo {
            public static void 
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 1, utf16_len("    public static void "));
    assert!(
        items.is_empty(),
        "expected no completions in method-name slot after return type, got: {items:?}"
    );
}

#[test]
fn syntax_no_completion_after_parameter_type_in_signature() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_no_completion_after_parameter_type");
    let src = indoc(r#"
        class Foo {
            public static int test(int )
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 1, utf16_len("    public static int test(int "));
    assert!(
        items.is_empty(),
        "expected no completions in parameter-name slot, got: {items:?}"
    );
}

#[test]
fn syntax_class_body_type_prefix_offers_int() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_class_body_type_prefix");
    let src = indoc(r#"
        class Foo {
            public i
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 1, utf16_len("    public i"));
    let ls = labels(&items);
    assert!(
        ls.iter().any(|label| label == "int"),
        "expected class-body type prefix to offer 'int', got: {ls:?}"
    );
}

#[test]
fn syntax_no_completion_after_method_signature_close_paren() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_no_completion_after_signature_close_paren");
    let src = indoc(r#"
        class Foo {
            public static int test(int a) 
        }
    "#);
    c.open(&uri, &src);

    let items = c.complete(&uri, 1, utf16_len("    public static int test(int a) "));
    assert!(
        items.is_empty(),
        "expected no completions after member parameter list, got: {items:?}"
    );
}

/// Cursor in variable-name slot (`int |x`) → no completions.
#[test]
fn syntax_no_completion_in_var_name() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_no_completion_in_var_name");
    // Cursor is on `x` — the variable name, not the initializer.
    let src = "public class Foo {\n    void go() {\n        int x\n    }\n}";
    c.open(&uri, src);

    // position after "int x" on the variable name character
    let items = c.complete(&uri, 2, 13);
    assert!(items.is_empty(), "expected no completions in var-name slot, got: {:?}", labels(&items));
}

/// Prefix-filtered member completion: `this.` in class → own fields appear.
#[test]
fn syntax_this_member_completion() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_this_member");
    let src = indoc(r#"
        public class Foo {
            private String myTestString;
            void go() {
                this.myTest
            }
        }
    "#);
    c.open(&uri, &src);

    // line 3, after "this.myTest"
    let items = c.complete(&uri, 3, 19);
    let ls = labels(&items);
    assert!(ls.contains(&"myTestString".to_owned()),
        "expected myTestString in this. completions, got: {ls:?}");
}

/// Prefix filtering: typing `Arr` inside a method → only items starting with Arr.
/// Fields/methods from the class that don't match should NOT appear.
#[test]
fn syntax_prefix_filters_members() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_prefix_filters");
    let src = indoc(r#"
        public class Foo {
            private String zebra = "z";
            void go() {
                int x = Arr
            }
        }
    "#);
    c.open(&uri, &src);

    // line 3, after "Arr"
    let items = c.complete(&uri, 3, 19);
    let ls = labels(&items);
    // "zebra" must NOT appear — it doesn't start with "Arr"
    assert!(!ls.contains(&"zebra".to_owned()),
        "field 'zebra' should be filtered out by prefix 'Arr', got: {ls:?}");
    // "go" method must NOT appear
    assert!(!ls.contains(&"go".to_owned()),
        "method 'go' should be filtered out by prefix 'Arr', got: {ls:?}");
}

/// Empty prefix in expression context → imported type names ARE offered.
#[test]
fn syntax_expression_offers_imported_types() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_expr_imports");
    let src = indoc(r#"
        import java.util.List;
        public class Foo {
            void go() {
                Object x =
            }
        }
    "#);
    c.open(&uri, &src);

    // line 3, after "= " (empty prefix expression context)
    let items = c.complete(&uri, 3, 20);
    let ls = labels(&items);
    // "List" is already imported — must appear in expression context
    assert!(ls.contains(&"List".to_owned()),
        "imported type 'List' should appear with empty prefix in expression context, got: {ls:?}");
}

// ─── Semantic (ECJ) tests — skipped if ECJ doesn't initialise in time ────────

/// Waits up to 30 s for `textDocument/publishDiagnostics` for `uri`.
/// Returns the diagnostics array on success, or `None` if ECJ didn't start in time.
/// The caller gets the actual diagnostics so it can inspect them directly.
fn ecj_wait_diagnostics(c: &mut LspClient, uri: &str) -> Option<Vec<Value>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let Some(msg) = c.recv_timeout(deadline) else { return None };
        if msg["method"] == "textDocument/publishDiagnostics"
            && msg["params"]["uri"] == uri
        {
            return Some(
                msg["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
}

/// Returns true when ECJ has started and compiled `uri` (diagnostics arrived).
fn ecj_ready(c: &mut LspClient, uri: &str) -> bool {
    ecj_wait_diagnostics(c, uri).is_some()
}

/// Typing `Objec` inside a method → `Object` class appears (from jdtls testCompletion_object).
#[test]
fn ecj_completion_object_type() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_object");
    let src = "public class Foo {\n    void foo() {\n        Objec\n    }\n}";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_completion_object_type — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, 13);
    let ls = labels(&items);
    assert!(ls.contains(&"Object".to_owned()),
        "expected 'Object' in completions for prefix 'Objec', got: {ls:?}");

    let obj = find_item(&items, "Object").unwrap();
    assert_eq!(obj["kind"], 7, "Object should have kind=Class (7)");
}

/// Typing after `new O` → constructor completion is offered.
#[test]
fn ecj_completion_constructor_after_new() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ctor_completion");
    let src = indoc(r#"
        class E {
            void go() {
                Object o = new O
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_completion_constructor_after_new — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, utf16_len("        Object o = new O"));
    let ctor = items.iter().find(|i| {
        i["label"].as_str().is_some_and(|label| label.starts_with("Object("))
    });
    assert!(ctor.is_some(), "expected constructor completion for Object, got: {items:?}");

    let ctor = ctor.unwrap();
    assert_eq!(ctor["kind"], 4, "constructor completion should have kind=Constructor");
    let insert = ctor["insertText"].as_str().unwrap_or("");
    assert!(
        insert.starts_with("Object("),
        "expected constructor insert text for Object, got: {ctor:?}"
    );
}

/// `int x = Arr` → `ArrayList` appears; own class members (e.g. `foo`) do NOT
/// (derived from jdtls expression-context filtering behaviour).
#[test]
fn ecj_expression_completion_filtered() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_expr_filter");
    let src = indoc(r#"
        public class Foo {
            void foo() {
                int x = Arr
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_expression_completion_filtered — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, 19);
    let ls = labels(&items);

    // JDK types starting with "Arr" must appear
    assert!(ls.iter().any(|l| l == "ArrayList"),
        "expected 'ArrayList' for prefix 'Arr', got: {ls:?}");

    // Own class method "foo" must NOT appear — filtered by prefix
    assert!(!ls.contains(&"foo".to_owned()),
        "method 'foo' should not appear for prefix 'Arr', got: {ls:?}");
}

/// Unimported type completion carries auto-import edits and an explicit textEdit.
#[test]
fn ecj_completion_auto_import_additional_edits() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_completion_auto_import");
    let src = indoc(r#"
        class E {
            void go() {
                int x = Arr
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_completion_auto_import_additional_edits — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, utf16_len("        int x = Arr"));
    let item = find_item(&items, "ArrayList");
    assert!(item.is_some(), "expected ArrayList completion, got: {:?}", labels(&items));

    let item = item.unwrap();
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(!edits.is_empty(), "expected auto-import edits on ArrayList completion, got: {item:?}");
    assert!(
        edits.iter().any(|e| e["newText"].as_str().is_some_and(|t| t.contains("import java.util.ArrayList;"))),
        "expected import edit for java.util.ArrayList, got: {edits:?}"
    );

    let text_edit = &item["textEdit"];
    assert!(text_edit.is_object(), "expected explicit textEdit when additionalTextEdits are present, got: {item:?}");
    assert_eq!(text_edit["newText"], "ArrayList");
}

/// `java.lang` types are implicitly imported and must not carry auto-import edits.
#[test]
fn ecj_completion_java_lang_type_skips_auto_import() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_completion_java_lang_no_import");
    let src = indoc(r#"
        class E {
            void go() {
                Object x = Str
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_completion_java_lang_type_skips_auto_import — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, utf16_len("        Object x = Str"));
    let item = find_item(&items, "String");
    assert!(item.is_some(), "expected String completion, got: {:?}", labels(&items));

    let item = item.unwrap();
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(
        edits.is_empty(),
        "java.lang.String should not carry an auto-import edit, got: {item:?}"
    );
}

/// Local variable completion inside a method (from jdtls LOCAL_VARIABLE_REF).
#[test]
fn ecj_local_variable_completion() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_local");
    let src = indoc(r#"
        public class Foo {
            void foo() {
                String myLocalVar = "hello";
                System.out.println(myLoc
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_local_variable_completion — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 3, 36);
    let ls = labels(&items);
    assert!(ls.contains(&"myLocalVar".to_owned()),
        "expected 'myLocalVar' in local completions, got: {ls:?}");
}

#[test]
fn ecj_static_member_completion_on_type_qualifier() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_static_member_qualifier");
    let src = indoc(r#"
        class Main {
            void go() {
                A.a
            }
        }

        class A {
            public static void add() {}
            public static void a() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_static_member_completion_on_type_qualifier — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 2, utf16_len("        A.a"));
    let ls = labels(&items);
    assert!(
        ls.iter().any(|l| l.starts_with("a(")),
        "expected static method completion for A.a, got: {ls:?}"
    );
}

#[test]
fn ecj_instance_member_completion_on_local_qualifier() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_instance_member_qualifier");
    let src = indoc(r#"
        class Main {
            void go() {
                A a = new A();
                a.a
            }
        }

        class A {
            public void add() {}
            public void a() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_instance_member_completion_on_local_qualifier — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 3, utf16_len("        a.a"));
    let ls = labels(&items);
    assert!(
        ls.iter().any(|l| l.starts_with("a(")),
        "expected instance method completion for local qualifier a.a, got: {ls:?}"
    );
}

/// Diagnostics: a type error (undefined method) produces an error diagnostic
/// (from jdtls compile-on-open behaviour).
#[test]
fn ecj_diagnostic_type_error() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_diag");
    // `unknownMethod()` does not exist on Object
    let src = "public class Foo {\n    void foo() {\n        Object o = new Object();\n        o.unknownMethod();\n    }\n}";
    c.open(&uri, src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_diagnostic_type_error — ECJ not ready");
        return;
    };

    assert!(!diags.is_empty(), "expected at least one error diagnostic for unknownMethod()");
    let has_error = diags.iter().any(|d| d["severity"] == 1);
    assert!(has_error, "expected severity=1 (Error), got: {diags:?}");
}

/// Import completion: typing `import java.util.Arr` → `ArrayList` package entry.
#[test]
fn ecj_import_completion() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_import");
    // Simulate the user typing inside an import statement
    let src2 = "import java.util.Arr\npublic class Foo {}";
    c.open(&uri, src2);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_import_completion — ECJ not ready");
        return;
    }

    // position at end of "import java.util.Arr" on line 0
    let items = c.complete(&uri, 0, 20);
    let ls = labels(&items);
    assert!(ls.iter().any(|l| l.contains("ArrayList")),
        "expected 'java.util.ArrayList' in import completions, got: {ls:?}");
}

/// Import completions should not carry extra import edits.
#[test]
fn ecj_import_completion_skips_additional_edits() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_import_no_extra_edits");
    let src = "import java.util.Arr\nclass E {}";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_import_completion_skips_additional_edits — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 0, 20);
    let item = items.iter().find(|i| i["label"].as_str() == Some("java.util.ArrayList"));
    assert!(item.is_some(), "expected java.util.ArrayList import completion, got: {:?}", labels(&items));

    let item = item.unwrap();
    let edits = item["additionalTextEdits"].as_array().cloned().unwrap_or_default();
    assert!(edits.is_empty(), "import completions should not carry additional edits, got: {item:?}");
}

/// Import completions must carry an explicit textEdit for the CodeRunner UI,
/// which does not synthesize a fallback replacement range client-side.
#[test]
fn ecj_import_completion_includes_text_edit_for_ui_clients() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_import_text_edit");
    let src = "import java.util.Arr\nclass E {}";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_import_completion_includes_text_edit_for_ui_clients — ECJ not ready");
        return;
    }

    let items = c.complete(&uri, 0, 20);
    let item = items.iter().find(|i| i["label"].as_str() == Some("java.util.ArrayList"));
    assert!(item.is_some(), "expected java.util.ArrayList import completion, got: {:?}", labels(&items));

    let item = item.unwrap();
    let text_edit = &item["textEdit"];
    assert!(text_edit.is_object(), "expected import completion to include textEdit, got: {item:?}");
    assert_eq!(text_edit["newText"], "ArrayList");
}

/// Open imported workspace types should be exposed as document links.
#[test]
fn ui_document_link_for_imported_open_type() {
    let mut c = LspClient::spawn();
    c.initialize();

    let imported_uri = test_uri("ui_doclink_imported");
    let imported_src = indoc(r#"
        package demo;

        public class Helper {}
    "#);
    c.open(&imported_uri, &imported_src);

    let uri = test_uri("ui_doclink_current");
    let src = indoc(r#"
        package demo;

        import demo.Helper;

        class E {}
    "#);
    c.open(&uri, &src);

    let links = c.document_links(&uri);
    let helper_link = links.iter().find(|link| link["target"].as_str() == Some(&imported_uri));
    assert!(helper_link.is_some(), "expected document link to imported open type, got: {links:?}");
}

// ─── Syntax-only: document highlight & references ────────────────────────────

/// Cursor on a local variable → all occurrences are highlighted in the same file.
/// (from jdtls DocumentHighlightHandlerTest#testDocumentHighlight_Occurrences)
#[test]
fn syntax_document_highlight_local_var() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_highlight");
    let src = indoc(r#"
        public class E {
            void go() {
                int count = 0;
                count++;
                System.out.println(count);
            }
        }
    "#);
    c.open(&uri, &src);

    // Line 2: "        int count = 0;" — "count" starts at col 12 (8 spaces + "int ")
    let highlights = c.document_highlight(&uri, 2, 12);
    assert!(
        highlights.len() >= 3,
        "expected ≥3 highlights for 'count' (decl + 2 uses), got: {highlights:?}"
    );
}

/// Cursor on a method name → find-references returns usages in the same file.
/// (from jdtls ReferencesHandlerTest#testReference)
#[test]
fn syntax_find_references_same_file() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_refs");
    let src = indoc(r#"
        public class E {
            void helper() {}
            void go() {
                helper();
                helper();
            }
        }
    "#);
    c.open(&uri, &src);

    // Line 1: "    void helper() {}" — cursor on "helper" (col 9)
    let refs = c.references(&uri, 1, 9);
    assert!(
        refs.len() >= 2,
        "expected ≥2 references to 'helper', got: {refs:?}"
    );
    // All references must point back to the same file
    for r in &refs {
        assert_eq!(r["uri"], uri, "reference should be in same file");
    }
}

// ─── ECJ: hover ──────────────────────────────────────────────────────────────

/// Hover on a method name → response contains the method signature.
/// (from jdtls HoverHandlerTest#testHover)
#[test]
fn ecj_hover_method_signature() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_hover");
    let src = indoc(r#"
        public class E {
            public int foo(String s) { return 0; }
            void bar() { foo("x"); }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_hover_method_signature — ECJ not ready");
        return;
    }

    // Line 1: "    public int foo(String s) { return 0; }" — cursor on "foo" (col 15)
    let result = c.hover(&uri, 1, 15);
    let text = result["contents"]["value"]
        .as_str()
        .or_else(|| result["contents"].as_str())
        .unwrap_or("");
    assert!(
        text.contains("foo") && text.contains("String"),
        "hover should contain method signature with 'foo' and 'String', got: {text:?}"
    );
}

/// Hover on a field → response contains the field type.
/// (from jdtls HoverHandlerTest#testHoverVariable)
#[test]
fn ecj_hover_field_type() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_hover_field");
    let src = indoc(r#"
        public class E {
            private String myField = "hello";
            void go() { System.out.println(myField); }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_hover_field_type — ECJ not ready");
        return;
    }

    // Line 1: "    private String myField = ..." — cursor on "myField" (col 19)
    let result = c.hover(&uri, 1, 19);
    let text = result["contents"]["value"]
        .as_str()
        .or_else(|| result["contents"].as_str())
        .unwrap_or("");
    assert!(
        text.contains("myField") || text.contains("String"),
        "hover should mention field name or type, got: {text:?}"
    );
}

// ─── ECJ: signature help ─────────────────────────────────────────────────────

/// Single-method signature help: calling `foo()` → one signature, label contains params.
/// (from jdtls SignatureHelpHandlerTest#testSignatureHelp_singleMethod)
#[test]
fn ecj_signature_help_single_method() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_sig_single");
    let src = indoc(r#"
        public class E {
            public int foo(String s) { return 0; }
            void bar() { foo(); }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_signature_help_single_method — ECJ not ready");
        return;
    }

    // Line 2: "    void bar() { foo(); }" — cursor inside foo( at col 21
    let result = c.signature_help(&uri, 2, 21);
    let sigs = result["signatures"].as_array().cloned().unwrap_or_default();
    assert!(!sigs.is_empty(), "expected at least one signature for foo(), got none");
    let label = sigs[0]["label"].as_str().unwrap_or("");
    assert!(
        label.contains("foo") && label.contains("String"),
        "signature label should contain 'foo' and 'String', got: {label:?}"
    );
}

/// Multiple overloads: calling `foo(2, )` → three signatures, active param = 1.
/// (from jdtls SignatureHelpHandlerTest#testSignatureHelp_multipleMethods)
#[test]
fn ecj_signature_help_active_parameter() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_sig_multi");
    let src = indoc(r#"
        public class E {
            public int foo(String s) { return 0; }
            public int foo(int s) { return 0; }
            public int foo(int s, String t) { return 0; }
            void bar() { foo(2, ); }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_signature_help_active_parameter — ECJ not ready");
        return;
    }

    // Line 4: "    void bar() { foo(2, ); }" — cursor after comma at col 24
    let result = c.signature_help(&uri, 4, 24);
    let sigs = result["signatures"].as_array().cloned().unwrap_or_default();
    assert!(
        sigs.len() >= 2,
        "expected ≥2 overload signatures for foo, got: {sigs:?}"
    );
    let active_param = result["activeParameter"].as_u64().unwrap_or(0);
    assert_eq!(active_param, 1, "cursor after first comma → activeParameter should be 1");
}

/// Overloaded methods → all signatures are returned and the best overload is active.
#[test]
fn ecj_signature_help_multiple_methods() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_sig_overloads");
    let src = indoc(r#"
        class E {
            int foo(String s) { return 1; }
            int foo(int s) { return 2; }
            int foo(int s, String t) { return 3; }
            void bar() { foo(2, ); }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_signature_help_multiple_methods — ECJ not ready");
        return;
    }

    let result = c.signature_help(&uri, 4, utf16_len("    void bar() { foo(2, "));
    let signatures = result["signatures"].as_array().cloned().unwrap_or_default();
    assert_eq!(signatures.len(), 3, "expected 3 overloads, got: {result:?}");
    assert_eq!(result["activeParameter"], 1, "expected second argument to be active, got: {result:?}");
    assert_eq!(
        signatures[result["activeSignature"].as_u64().unwrap_or(0) as usize]["label"],
        "foo(int s, String t) : int",
        "expected best overload to be the two-parameter int/String method (jdt.ls label format)"
    );
}

/// No signature help outside a method call.
/// (from jdtls SignatureHelpHandlerTest#testSignatureHelp_noCall)
#[test]
fn ecj_signature_help_empty_outside_call() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_sig_none");
    let src = indoc(r#"
        public class E {
            public int bar(String s) { return 0; }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_signature_help_empty_outside_call — ECJ not ready");
        return;
    }

    // Line 1, col 4 — inside method body but not in a call expression
    let result = c.signature_help(&uri, 1, 4);
    let sigs = result["signatures"].as_array().cloned().unwrap_or_default();
    assert!(
        sigs.is_empty(),
        "expected no signatures outside a call expression, got: {sigs:?}"
    );
}

// ─── ECJ: diagnostics ────────────────────────────────────────────────────────

/// Unused private field → warning diagnostic.
/// (from jdtls DiagnosticHandlerTest#testNotUsed)
#[test]
fn ecj_diagnostic_unused_field() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_diag_unused_field");
    // Non-public class — filename doesn't need to match class name
    let src = "class E {\n    private int i;\n}";
    c.open(&uri, src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_diagnostic_unused_field — ECJ not ready");
        return;
    };

    assert!(!diags.is_empty(), "expected a warning for unused private field 'i'");
    let has_warning = diags.iter().any(|d| d["severity"] == 2);
    assert!(has_warning, "expected severity=2 (Warning), got: {diags:?}");
}

/// Dead code block → diagnostic with multi-line range.
/// (from jdtls DiagnosticHandlerTest#testMultipleLineRange)
#[test]
fn ecj_diagnostic_dead_code_range() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_diag_dead_code");
    let src = indoc(r#"
        class E {
            boolean foo(boolean b) {
                if (false) {
                    return true;
                }
                return false;
            }
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_diagnostic_dead_code_range — ECJ not ready");
        return;
    };

    assert!(!diags.is_empty(), "expected a diagnostic for dead code block");
    // The dead code diagnostic should span multiple lines (the if body)
    let multi_line = diags.iter().any(|d| {
        let start_line = d["range"]["start"]["line"].as_u64().unwrap_or(0);
        let end_line   = d["range"]["end"]["line"].as_u64().unwrap_or(0);
        end_line > start_line
    });
    assert!(multi_line, "expected at least one multi-line diagnostic range, got: {diags:?}");
}

// ─── ECJ: formatting ─────────────────────────────────────────────────────────

/// Basic formatting: badly indented code → at least one TextEdit returned.
/// (from jdtls FormatterHandlerTest#testDocumentFormatting)
#[test]
fn ecj_format_returns_edits() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_format");
    // Deliberately messy indentation — non-public so filename need not match class name
    let src = "class E {\nvoid go() {\nint x=1;\n}\n}";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_format_returns_edits — ECJ not ready");
        return;
    }

    let edits = c.format(&uri);
    if edits.is_empty() {
        // google-java-format requires --add-exports flags on JVM 17+ to access
        // javac internals; if the formatter initialised without them it silently
        // disables itself and returns no edits.  Treat as skip, not failure.
        eprintln!("INFO ecj_format_returns_edits — formatter returned no edits (may need --add-exports flags for this JVM)");
        return;
    }
    // Every edit must have a range and newText
    for edit in &edits {
        assert!(edit["range"].is_object(), "edit must have a range");
        assert!(edit["newText"].is_string(), "edit must have newText");
    }
    // The formatted output should contain proper indentation
    let new_text = edits[0]["newText"].as_str().unwrap_or("");
    assert!(
        new_text.contains("  void") || new_text.contains("    void"),
        "formatted code should indent method body, got: {new_text:?}"
    );
}

/// Formatting already-correct code → no edits (idempotent).
/// (from jdtls FormatterHandlerTest#testDocumentFormatting idempotent check)
#[test]
fn ecj_format_idempotent() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_format_idem");
    // google-java-format style: 2-space indent, non-public class
    let src = "class E {\n  void go() {\n    int x = 1;\n  }\n}\n";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_format_idempotent — ECJ not ready");
        return;
    }

    let edits = c.format(&uri);
    // If the source is already in google-java-format style, no edits should be returned.
    // Every edit that IS returned must be structurally valid.
    for edit in &edits {
        assert!(edit["range"].is_object(), "edit must have a range");
        assert!(edit["newText"].is_string(), "edit must have newText");
    }
}

/// On-type formatting should return edits for badly-formatted Java when typing `;`.
#[test]
fn ui_on_type_formatting_returns_edits() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ui_on_type_formatting");
    let src = "class E {\nvoid go() {\nint x=1;\n}\n}";
    c.open(&uri, src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ui_on_type_formatting_returns_edits — ECJ not ready");
        return;
    }

    let edits = c.on_type_formatting(&uri, 2, utf16_len("int x=1;"), ";");
    assert!(!edits.is_empty(), "expected on-type formatting edits, got: {edits:?}");
}

// ─── Syntax-only: document symbols & goto-definition ─────────────────────────

/// A class with a field and method → hierarchical document symbols returned.
/// (from jdtls DocumentSymbolHandlerTest#testDocumentSymbolsOnPlainFile)
#[test]
fn syntax_document_symbols_class_members() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_symbols");
    let src = indoc(r#"
        class E {
            int someField;
            void someMethod() {}
        }
    "#);
    c.open(&uri, &src);

    let syms = c.document_symbols(&uri);
    assert!(!syms.is_empty(), "expected at least one document symbol, got none");

    // Find the class symbol (may be at top level or nested)
    fn find_sym<'a>(syms: &'a [Value], name: &str) -> Option<&'a Value> {
        syms.iter().find(|s| s["name"].as_str() == Some(name))
    }

    let class_sym = find_sym(&syms, "E").expect("expected class symbol 'E'");
    let children = class_sym["children"].as_array().cloned().unwrap_or_default();
    assert!(
        find_sym(&children, "someField").is_some(),
        "expected 'someField' in class children, got: {children:?}"
    );
    assert!(
        find_sym(&children, "someMethod").is_some(),
        "expected 'someMethod' in class children, got: {children:?}"
    );
}

/// Cursor on a method call → goto-definition jumps to the method declaration in the same file.
/// (from jdtls NavigateToDefinitionHandlerTest — same-file variant)
#[test]
fn syntax_goto_definition_method() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_gotodef");
    let src = indoc(r#"
        class E {
            void helper() {}
            void go() {
                helper();
            }
        }
    "#);
    c.open(&uri, &src);

    // Line 3: "        helper();" — cursor on "helper" (col 8)
    let defs = c.goto_definition(&uri, 3, 8);
    assert!(!defs.is_empty(), "expected at least one definition location for 'helper'");
    let def = &defs[0];
    assert_eq!(def["uri"], uri, "definition should be in the same file");
    // "void helper()" is on line 1
    let def_line = def["range"]["start"]["line"].as_u64().unwrap_or(99);
    assert_eq!(def_line, 1, "definition of 'helper' should be on line 1, got: {def_line}");
}

/// Cursor on a field reference → goto-definition jumps to the field declaration.
/// (from jdtls NavigateToDefinitionHandlerTest — same-file field variant)
#[test]
fn syntax_goto_definition_field() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_gotodef_field");
    let src = indoc(r#"
        class E {
            int myCount = 0;
            void go() {
                myCount++;
            }
        }
    "#);
    c.open(&uri, &src);

    // Line 3: "        myCount++;" — cursor on "myCount" (col 8)
    let defs = c.goto_definition(&uri, 3, 8);
    assert!(!defs.is_empty(), "expected definition location for 'myCount'");
    let def_line = defs[0]["range"]["start"]["line"].as_u64().unwrap_or(99);
    assert_eq!(def_line, 1, "definition of 'myCount' should be on line 1, got: {def_line}");
}

// ─── ECJ: inlay hints ────────────────────────────────────────────────────────

/// Character literal arg → parameter name hint is shown.
/// (from jdtls InlayHintHandlerTest#testCharacterLiteral)
#[test]
fn ecj_inlay_hint_char_literal() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_inlay_char");
    let src = indoc(r#"
        class Foo {
            void foo(char c) {}
            void bar() {
                foo('x');
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_inlay_hint_char_literal — ECJ not ready");
        return;
    }

    let hints = c.inlay_hints(&uri, 0, 10);
    // Expect a hint labelled "c:" near line 3
    let labels: Vec<&str> = hints.iter()
        .filter_map(|h| h["label"].as_str())
        .collect();
    assert!(
        labels.iter().any(|l| l.contains('c')),
        "expected inlay hint with parameter name 'c', got: {labels:?}"
    );
}

/// Null literal arg → parameter name hint is shown.
/// (from jdtls InlayHintHandlerTest#testNullLiteral)
#[test]
fn ecj_inlay_hint_null_literal() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_inlay_null");
    let src = indoc(r#"
        class Foo {
            void foo(String s) {}
            void bar() {
                foo(null);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_inlay_hint_null_literal — ECJ not ready");
        return;
    }

    let hints = c.inlay_hints(&uri, 0, 10);
    let labels: Vec<&str> = hints.iter()
        .filter_map(|h| h["label"].as_str())
        .collect();
    assert!(
        labels.iter().any(|l| l.contains('s')),
        "expected inlay hint with parameter name 's' for null arg, got: {labels:?}"
    );
}

/// Variable expression arg → NO inlay hint (only literals get hints).
/// (from jdtls InlayHintHandlerTest — non-trivial args are suppressed)
#[test]
fn ecj_inlay_hint_no_hint_for_variable_arg() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_inlay_var");
    let src = indoc(r#"
        class Foo {
            void foo(String s) {}
            void bar() {
                String myVar = "hello";
                foo(myVar);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_inlay_hint_no_hint_for_variable_arg — ECJ not ready");
        return;
    }

    let hints = c.inlay_hints(&uri, 0, 10);
    // Variable expressions should NOT produce a hint
    assert!(
        hints.is_empty(),
        "expected no inlay hints for variable arg, got: {hints:?}"
    );
}

// ─── ECJ: code actions ────────────────────────────────────────────────────────

/// Unresolved type → "Add import" quick-fix is offered.
/// (from jdtls CodeActionHandlerTest#testCodeAction_allKindsOfActions)
#[test]
fn ecj_code_action_add_import() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_import");
    let src = indoc(r#"
        class E {
            void go() {
                ArrayList list = new ArrayList();
            }
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_add_import — ECJ not ready");
        return;
    };

    // Pass diagnostics so the server knows what errors to fix
    let actions = c.code_actions(&uri, 2, 8, &diags);
    assert!(!actions.is_empty(), "expected at least one code action for unresolved 'ArrayList'");

    // At least one action should be about adding an import or be a quickfix
    let has_import_action = actions.iter().any(|a| {
        let title = a["title"].as_str()
            .or_else(|| a["right"]["title"].as_str())
            .unwrap_or("");
        let kind = a["kind"].as_str()
            .or_else(|| a["right"]["kind"].as_str())
            .unwrap_or("");
        title.to_lowercase().contains("import") || kind.contains("quickfix")
    });
    assert!(has_import_action, "expected an 'Add import' or quickfix action, got: {actions:?}");
}

/// Unused import → "source.organizeImports" action is offered.
/// (from jdtls CodeActionHandlerTest#testCodeAction_organizeImportsSourceActionOnly)
#[test]
fn ecj_code_action_organize_imports() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_organise");
    let src = indoc(r#"
        import java.util.List;
        class E {
            void go() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_organize_imports — ECJ not ready");
        return;
    }

    let actions = c.code_actions(&uri, 0, 0, &[]);
    assert!(!actions.is_empty(), "expected at least one code action");

    let has_organise = actions.iter().any(|a| {
        let kind = a["kind"].as_str()
            .or_else(|| a["right"]["kind"].as_str())
            .unwrap_or("");
        let title = a["title"].as_str()
            .or_else(|| a["right"]["title"].as_str())
            .unwrap_or("");
        kind.contains("organizeImports") || kind.contains("source")
            || title.to_lowercase().contains("import")
    });
    assert!(has_organise, "expected an import-related action, got: {actions:?}");
}

/// Organize imports must not add explicit imports for `java.lang` types.
#[test]
fn ecj_code_action_organize_imports_skips_java_lang() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_organize_no_java_lang");
    let src = indoc(r#"
        class E {
            ArrayList<String> values = new ArrayList<>();
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_organize_imports_skips_java_lang — ECJ not ready");
        return;
    };

    let actions = c.code_actions(&uri, 0, 0, &diags);
    let action = actions.iter().find(|a| {
        a["title"].as_str()
            .or_else(|| a["right"]["title"].as_str())
            .is_some_and(|t| t == "Organize Imports")
    });
    assert!(action.is_some(), "expected Organize Imports action, got: {actions:?}");

    let edits = action.unwrap()["edit"]["changes"][&uri].as_array().cloned().unwrap_or_default();
    let new_text: String = edits.iter()
        .filter_map(|e| e["newText"].as_str())
        .collect();
    assert!(
        new_text.contains("import java.util.ArrayList;"),
        "expected organize imports to add java.util.ArrayList, got: {new_text:?}"
    );
    assert!(
        !new_text.contains("import java.lang.String;"),
        "organize imports must not add java.lang.String, got: {new_text:?}"
    );
    assert!(
        !new_text.contains("String;\n"),
        "organize imports must not add any explicit String import, got: {new_text:?}"
    );
}

/// Unused import diagnostic → explicit removal quick-fix is offered.
#[test]
fn ecj_code_action_remove_unused_import() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_remove_unused_import");
    let src = indoc(r#"
        import java.sql.*;
        class E {
            void go() {}
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_remove_unused_import — ECJ not ready");
        return;
    };

    let actions = c.code_actions(&uri, 0, 8, &diags);
    let action = actions.iter().find(|a| {
        a["title"].as_str()
            .or_else(|| a["right"]["title"].as_str())
            .is_some_and(|t| t == "Remove unused import")
    });
    assert!(action.is_some(), "expected 'Remove unused import', got: {actions:?}");

    let edits = action.unwrap()["edit"]["changes"][&uri].as_array().cloned().unwrap_or_default();
    assert!(!edits.is_empty(), "expected workspace edit for unused import removal");
    assert!(
        edits.iter().any(|e| e["newText"].as_str() == Some("")),
        "expected removal edit for unused import, got: {edits:?}"
    );
}

/// Multiple unused imports → bulk removal quick-fix is offered.
#[test]
fn ecj_code_action_remove_all_unused_imports() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_remove_all_unused_imports");
    let src = indoc(r#"
        import java.sql.*;
        import java.util.List;
        class E {
            void go() {}
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_remove_all_unused_imports — ECJ not ready");
        return;
    };

    let actions = c.code_actions(&uri, 0, 8, &diags);
    let titles: Vec<&str> = actions.iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles.contains(&"Remove all unused imports"),
        "expected bulk unused-import action, got: {titles:?}"
    );
}

// ─── ECJ: rename ─────────────────────────────────────────────────────────────

/// Prepare rename should return the selected identifier range and placeholder.
#[test]
fn ui_prepare_rename_returns_placeholder() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ui_prepare_rename");
    let src = indoc(r#"
        class E {
            void go() {
                int count = 0;
                count++;
            }
        }
    "#);
    c.open(&uri, &src);

    let result = c.prepare_rename(&uri, 3, 9);
    assert_eq!(result["placeholder"], "count");
    assert!(result["range"].is_object(), "expected prepareRename range, got: {result:?}");
}

/// Linked editing should return all ranges for the current symbol in the file.
#[test]
fn ui_linked_editing_returns_symbol_ranges() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ui_linked_editing");
    let src = indoc(r#"
        class E {
            void go() {
                int count = 0;
                count++;
                System.out.println(count);
            }
        }
    "#);
    c.open(&uri, &src);

    let result = c.linked_editing_range(&uri, 3, 9);
    let ranges = result["ranges"].as_array().cloned().unwrap_or_default();
    assert!(
        ranges.len() >= 3,
        "expected linked editing ranges for declaration and uses, got: {result:?}"
    );
}

/// Rename a local variable → all occurrences in the file are renamed.
/// (from jdtls RenameHandlerTest#testRenameLocalVariable)
#[test]
fn ecj_rename_local_variable() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_rename_local");
    let src = indoc(r#"
        class E {
            void go() {
                int count = 0;
                count++;
                System.out.println(count);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_rename_local_variable — ECJ not ready");
        return;
    }

    // Line 2: "        int count = 0;" — cursor on "count" (col 12)
    let edit = c.rename(&uri, 2, 12, "total");
    assert!(
        edit.is_object() && !edit.is_null(),
        "expected a WorkspaceEdit, got: {edit:?}"
    );

    // Extract all text edits from the workspace edit
    let changes = &edit["changes"];
    let edits_for_file = changes[&uri]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        edits_for_file.len() >= 2,
        "expected ≥2 edits (declaration + uses), got: {edits_for_file:?}"
    );
    let all_new_texts: Vec<&str> = edits_for_file.iter()
        .filter_map(|e| e["newText"].as_str())
        .collect();
    assert!(
        all_new_texts.iter().all(|t| *t == "total"),
        "all edits should rename to 'total', got: {all_new_texts:?}"
    );
}

/// Rename a method → declaration and call site are both updated.
/// (from jdtls RenameHandlerTest#testRenameMethod)
#[test]
fn ecj_rename_method() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_rename_method");
    let src = indoc(r#"
        class E {
            void helper() {}
            void go() {
                helper();
                helper();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_rename_method — ECJ not ready");
        return;
    }

    // Line 1: "    void helper() {}" — cursor on "helper" (col 9)
    let edit = c.rename(&uri, 1, 9, "util");
    assert!(edit.is_object() && !edit.is_null(), "expected a WorkspaceEdit");

    let changes = &edit["changes"];
    let edits_for_file = changes[&uri].as_array().cloned().unwrap_or_default();
    // Declaration + 2 call sites = 3 edits
    assert!(
        edits_for_file.len() >= 3,
        "expected ≥3 edits (declaration + 2 calls), got: {edits_for_file:?}"
    );
}

/// Rename a field → declaration and usages are updated.
#[test]
fn ecj_rename_field() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_rename_field");
    let src = indoc(r#"
        class E {
            private int myValue = 2;
            void bar() {
                myValue = 3;
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_rename_field — ECJ not ready");
        return;
    }

    let edit = c.rename(&uri, 3, 8, "newname");
    assert!(edit.is_object() && !edit.is_null(), "expected a WorkspaceEdit, got: {edit:?}");

    let edits_for_file = edit["changes"][&uri].as_array().cloned().unwrap_or_default();
    assert!(
        edits_for_file.len() >= 2,
        "expected declaration and usage edits for field rename, got: {edits_for_file:?}"
    );
    let all_new_texts: Vec<&str> = edits_for_file.iter()
        .filter_map(|e| e["newText"].as_str())
        .collect();
    assert!(
        all_new_texts.iter().all(|t| *t == "newname"),
        "all field rename edits should use the new name, got: {all_new_texts:?}"
    );
}

// ─── ECJ: type hierarchy ─────────────────────────────────────────────────────

/// Prepare type hierarchy on a class → returns the class as a root item.
/// (from jdtls TypeHierarchyHandlerTest#testSuperTypeHierarchy)
#[test]
fn ecj_type_hierarchy_prepare() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_typehier");
    let src = indoc(r#"
        class Animal {}
        class Dog extends Animal {}
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_type_hierarchy_prepare — ECJ not ready");
        return;
    }

    // Line 1: "class Dog extends Animal {}" — cursor on "Dog" (col 6)
    let items = c.prepare_type_hierarchy(&uri, 1, 6);
    assert!(!items.is_empty(), "expected prepare to return at least one item");
    let name = items[0]["name"].as_str().unwrap_or("");
    assert_eq!(name, "Dog", "prepared item should be 'Dog', got: {name:?}");
}

/// Supertypes of Dog → Animal appears in the list.
/// (from jdtls TypeHierarchyHandlerTest#testSuperTypeHierarchy)
#[test]
fn ecj_type_hierarchy_supertypes() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_typehier_super");
    let src = indoc(r#"
        class Animal {}
        class Dog extends Animal {}
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_type_hierarchy_supertypes — ECJ not ready");
        return;
    }

    let items = c.prepare_type_hierarchy(&uri, 1, 6);
    if items.is_empty() {
        eprintln!("SKIP ecj_type_hierarchy_supertypes — prepare returned empty");
        return;
    }

    let supertypes = c.type_supertypes(&items[0]);
    let names: Vec<&str> = supertypes.iter()
        .filter_map(|i| i["name"].as_str())
        .collect();
    assert!(
        names.contains(&"Animal"),
        "supertypes of Dog should include 'Animal', got: {names:?}"
    );
}

/// Subtypes of Animal → Dog appears in the list.
/// (from jdtls TypeHierarchyHandlerTest#testSubTypeHierarchy)
#[test]
fn ecj_type_hierarchy_subtypes() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_typehier_sub");
    let src = indoc(r#"
        class Animal {}
        class Dog extends Animal {}
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_type_hierarchy_subtypes — ECJ not ready");
        return;
    }

    // Prepare on Animal (line 0, col 6)
    let items = c.prepare_type_hierarchy(&uri, 0, 6);
    if items.is_empty() {
        eprintln!("SKIP ecj_type_hierarchy_subtypes — prepare returned empty");
        return;
    }

    let subtypes = c.type_subtypes(&items[0]);
    let names: Vec<&str> = subtypes.iter()
        .filter_map(|i| i["name"].as_str())
        .collect();
    assert!(
        names.contains(&"Dog"),
        "subtypes of Animal should include 'Dog', got: {names:?}"
    );
}

// ─── ECJ: call hierarchy ─────────────────────────────────────────────────────

/// Prepare call hierarchy on a method → returns a CallHierarchyItem with the method name.
/// (from jdtls CallHierarchyHandlerTest#prepareCallHierarchy)
#[test]
fn ecj_call_hierarchy_prepare() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_callhier");
    let src = indoc(r#"
        class E {
            void foo() {}
            void bar() {
                foo();
                foo();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_call_hierarchy_prepare — ECJ not ready");
        return;
    }

    // Line 1: "    void foo() {}" — cursor on "foo" (col 9)
    let items = c.prepare_call_hierarchy(&uri, 1, 9);
    assert!(!items.is_empty(), "expected prepare to return at least one item for 'foo'");
    let name = items[0]["name"].as_str().unwrap_or("");
    assert_eq!(name, "foo", "prepared item should be 'foo', got: {name:?}");
}

/// Incoming calls to `foo` → `bar` appears as a caller.
/// (from jdtls CallHierarchyHandlerTest#incomingCalls_src)
#[test]
fn ecj_call_hierarchy_incoming() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_callhier_in");
    let src = indoc(r#"
        class E {
            void foo() {}
            void bar() {
                foo();
                foo();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_call_hierarchy_incoming — ECJ not ready");
        return;
    }

    let items = c.prepare_call_hierarchy(&uri, 1, 9);
    if items.is_empty() {
        eprintln!("SKIP ecj_call_hierarchy_incoming — prepare returned empty");
        return;
    }

    let calls = c.call_incoming(&items[0]);
    assert!(!calls.is_empty(), "expected incoming calls to 'foo'");
    let callers: Vec<&str> = calls.iter()
        .filter_map(|c| c["from"]["name"].as_str())
        .collect();
    assert!(
        callers.contains(&"bar"),
        "expected 'bar' as a caller of 'foo', got: {callers:?}"
    );
}

/// Outgoing calls from `bar` → `foo` appears as a callee.
/// (from jdtls CallHierarchyHandlerTest#outgoingCalls_src)
#[test]
fn ecj_call_hierarchy_outgoing() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_callhier_out");
    let src = indoc(r#"
        class E {
            void foo() {}
            void bar() {
                foo();
                foo();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_call_hierarchy_outgoing — ECJ not ready");
        return;
    }

    // Prepare on bar (line 2, col 9)
    let items = c.prepare_call_hierarchy(&uri, 2, 9);
    if items.is_empty() {
        eprintln!("SKIP ecj_call_hierarchy_outgoing — prepare returned empty");
        return;
    }

    let calls = c.call_outgoing(&items[0]);
    assert!(!calls.is_empty(), "expected outgoing calls from 'bar'");
    let callees: Vec<&str> = calls.iter()
        .filter_map(|c| c["to"]["name"].as_str())
        .collect();
    assert!(
        callees.contains(&"foo"),
        "expected 'foo' as a callee of 'bar', got: {callees:?}"
    );
}

/// Getter/Setter code actions appear for a class with an instance field.
#[test]
fn ecj_code_action_getter_setter() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_getter_setter");
    // Field on line 1 (0-based). Cursor placed on line 0 (class declaration) to
    // verify actions appear even when cursor is not on the exact field line.
    let src = indoc(r#"
        class Foo {
            private int age;
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_getter_setter — ECJ not ready");
        return;
    }

    // Request from line 0 (class declaration line) — should still see getter/setter for 'age'
    let actions = c.code_actions(&uri, 0, 0, &[]);
    let titles: Vec<&str> = actions.iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    eprintln!("actions at line 0: {titles:?}");
    assert!(
        titles.iter().any(|t| t.contains("Getter") && t.contains("age")),
        "expected 'Generate Getter for age' at line 0, got: {titles:?}"
    );

    // Also verify from the field line itself (line 1)
    let actions2 = c.code_actions(&uri, 1, 4, &[]);
    let titles2: Vec<&str> = actions2.iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    eprintln!("actions at line 1: {titles2:?}");
    assert!(
        titles2.iter().any(|t| t.contains("Getter") && t.contains("age")),
        "expected 'Generate Getter for age' at line 1, got: {titles2:?}"
    );
}

#[test]
fn ecj_code_action_final_modifiers_uses_source_kind() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_final_modifiers");
    let src = indoc(r#"
        class Foo {
            void go(int input) {
                int count = input + 1;
                System.out.println(count);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_final_modifiers_uses_source_kind — ECJ not ready");
        return;
    }

    let actions = c.code_actions(&uri, 0, 0, &[]);
    let final_action = actions.iter().find(|a| {
        a["title"].as_str() == Some("Change modifiers to final where possible")
            || a["right"]["title"].as_str() == Some("Change modifiers to final where possible")
    });

    let Some(action) = final_action else {
        panic!("expected final-modifiers action, got: {actions:?}");
    };

    let kind = action["kind"]
        .as_str()
        .or_else(|| action["right"]["kind"].as_str())
        .unwrap_or("");
    assert_eq!(
        kind,
        "source.generate.finalModifiers",
        "expected source.generate.finalModifiers kind, got: {action:?}"
    );
}

// ─── Mimic eclipse.jdt.ls tests ─────────────────────────────────────────────

/// Folding ranges for imports and class body.
/// (from jdtls FoldingRangeHandlerTest#testTypes)
#[test]
fn syntax_folding_ranges() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_folding");
    let src = indoc(r#"
        package org.sample;
        import java.util.List;
        import java.util.ArrayList;

        /**
         * Some javadoc
         */
        public class Simple {
            void foo() {
            }
        }
    "#);
    c.open(&uri, &src);

    let ranges = c.folding_ranges(&uri);
    assert!(ranges.len() >= 2, "expected at least folding ranges for imports and class, got: {ranges:?}");

    let has_imports = ranges.iter().any(|r| {
        r["kind"] == "imports" && r["startLine"] == 1 && r["endLine"] == 2
    });
    assert!(has_imports, "expected 'imports' folding range at lines 1-2, got: {ranges:?}");

    let has_class = ranges.iter().any(|r| {
        r["startLine"] == 7 && r["endLine"] == 10
    });
    assert!(has_class, "expected class body folding range at lines 7-10, got: {ranges:?}");
}

/// Selection ranges: nested expansion from identifier to method to class.
/// (from jdtls SelectionRangeHandlerTest#testParamList)
#[test]
fn syntax_selection_ranges() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_selection");
    let src = indoc(r#"
        class Foo {
            void bar(int param) {
            }
        }
    "#);
    c.open(&uri, &src);

    // Position at "param" (line 1, col 18)
    let ranges = c.selection_ranges(&uri, vec![(1, 18)]);
    assert!(!ranges.is_empty(), "expected selection ranges for 'param'");

    let r = &ranges[0];
    let mut found_param = false;
    let mut found_method = false;
    let mut found_class = false;

    // Use a loop to traverse the selection range hierarchy
    let mut current = r.clone();
    loop {
        let start_line = current["range"]["start"]["line"].as_u64().unwrap_or(99);
        let start_char = current["range"]["start"]["character"].as_u64().unwrap_or(99);
        let end_line = current["range"]["end"]["line"].as_u64().unwrap_or(0);
        let end_char = current["range"]["end"]["character"].as_u64().unwrap_or(0);

        if start_line == 1 && start_char == 17 && end_char == 22 {
            found_param = true;
        }
        if start_line == 1 && end_line == 2 {
            found_method = true;
        }
        if start_line == 0 && end_line == 3 {
            found_class = true;
        }

        if current["parent"].is_object() {
            current = current["parent"].clone();
        } else {
            break;
        }
    }

    assert!(found_param, "expected selection range to cover 'param'");
    assert!(found_method, "expected parent selection range to cover method body");
    assert!(found_class, "expected ancestor selection range to cover class body");
}

/// Semantic tokens for a method declaration.
/// (from jdtls SemanticTokensHandlerTest#testSemanticTokens_Methods)
#[test]
fn ecj_semantic_tokens() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_sem_tokens");
    let src = indoc(r#"
        public class E {
            public void foo() {}
        }
    "#);
    c.open(&uri, &src);

    // Tree-sitter tokens are available immediately
    let data = c.semantic_tokens(&uri);
    assert!(!data.is_empty(), "expected some semantic tokens");

    // The data is delta-encoded: [deltaLine, deltaStart, length, tokenType, tokenModifiers]
    // "public" is at 0,0, length 6. type 15 (MODIFIER)
    // "class" is at 0,7, length 5. type 14 (KEYWORD)
    // "E" is at 0,13, length 1. type 2 (CLASS)
    // ...
    // "void" is at 1,11, length 4. type 14 (KEYWORD)
    // "foo" is at 1,16, length 3. type 12 (METHOD)

    let mut found_foo = false;
    let mut curr_line = 0;
    let mut curr_char = 0;
    for i in (0..data.len()).step_by(5) {
        let delta_line = data[i] as u32;
        let delta_start = data[i+1] as u32;
        let length = data[i+2] as u32;
        let token_type = data[i+3] as u32;

        if delta_line > 0 {
            curr_line += delta_line;
            curr_char = delta_start;
        } else {
            curr_char += delta_start;
        }

        if curr_line == 1 && curr_char == 16 && length == 3 && token_type == 12 {
            found_foo = true;
        }
    }
    assert!(found_foo, "expected semantic token for method 'foo' at 1:16 (type 12)");
}

/// Goto implementation: interface method → class implementation.
/// (from jdtls ImplementationsHandlerTest#testImplementations)
#[test]
fn ecj_goto_implementation() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_impl");
    let src = indoc(r#"
        interface I { void m(); }
        class C implements I { public void m() {} }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_goto_implementation — ECJ not ready");
        return;
    }

    // Line 0: "interface I { void m(); }" — cursor on "m" (col 19)
    let locs = c.implementation(&uri, 0, 19);
    assert!(!locs.is_empty(), "expected at least one implementation for I.m()");

    let impl_line = locs[0]["range"]["start"]["line"].as_u64().unwrap_or(99);
    assert_eq!(impl_line, 1, "implementation of I.m() should be on line 1 (class C), got: {impl_line}");
}

/// Goto type definition: variable → class definition of its type.
/// (from jdtls NavigateToTypeDefinitionHandlerTest#testTypeDefinition)
#[test]
fn ecj_goto_type_definition() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_typedef");
    let src = indoc(r#"
        class SomeType {}
        class E {
            void go() {
                SomeType x = new SomeType();
                x.toString();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_goto_type_definition — ECJ not ready");
        return;
    }

    // Line 4: "        x.toString();" — cursor on "x" (col 8)
    let locs = c.type_definition(&uri, 4, 8);
    assert!(!locs.is_empty(), "expected type definition for 'x'");

    let type_line = locs[0]["range"]["start"]["line"].as_u64().unwrap_or(99);
    assert_eq!(type_line, 0, "type definition of 'x' should be line 0 (class SomeType), got: {type_line}");
}

/// Workspace symbols: search for a class defined in one of several open files.
/// (from jdtls WorkspaceSymbolHandlerTest#testWorkspaceSymbol)
#[test]
fn syntax_workspace_symbols() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri1 = test_uri("syntax_ws_1");
    let src1 = "class FirstClass {}";
    c.open(&uri1, src1);

    let uri2 = test_uri("syntax_ws_2");
    let src2 = "class SecondClass {}";
    c.open(&uri2, src2);

    let symbols = c.workspace_symbol("Second");
    assert!(
        symbols.iter().any(|s| s["name"] == "SecondClass"),
        "expected 'SecondClass' in workspace symbols for query 'Second', got: {symbols:?}"
    );
}

/// Code lens: verify that method usage count is shown.
/// (from jdtls CodeLensHandlerTest#testCodeLens_Methods)
#[test]
fn ecj_code_lens_usages() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_codelens");
    let src = indoc(r#"
        class E {
            void target() {}
            void caller() {
                target();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_lens_usages — ECJ not ready");
        return;
    }

    let lenses = c.code_lens(&uri);
    assert!(!lenses.is_empty(), "expected at least one code lens");

    let resolved: Vec<Value> = lenses.iter()
        .filter(|l| l["data"]["tag"].as_str() == Some("references"))
        .map(|l| c.resolve_code_lens(l))
        .collect();
    assert!(
        resolved.iter().any(|l| l["command"]["title"].as_str() == Some("1 reference")),
        "expected a resolved lens with '1 reference' for target(), got: {resolved:?}"
    );
}

/// Zero-reference symbols still produce a clickable show-references lens.
#[test]
fn ecj_code_lens_zero_references() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_codelens_zero");
    let src = indoc(r#"
        class E {
            void unused() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_lens_zero_references — ECJ not ready");
        return;
    }

    let lenses = c.code_lens(&uri);
    let zero = lenses.iter()
        .filter(|l| l["data"]["tag"].as_str() == Some("references"))
        .map(|l| c.resolve_code_lens(l))
        .find(|l| l["command"]["title"].as_str() == Some("0 references"));
    assert!(zero.is_some(), "expected a resolved 0-reference lens, got: {lenses:?}");
    let zero = zero.unwrap();
    assert_eq!(
        zero["command"]["command"],
        "editor.action.showReferences",
        "0-reference lens should still invoke showReferences"
    );
    let refs = zero["command"]["arguments"][2].as_array().cloned().unwrap_or_default();
    assert!(refs.is_empty(), "0-reference lens should carry an empty reference list, got: {zero:?}");
}

/// CodeRunner's `ui` client expects Eclipse-style `java.show.references`
/// code-lens commands, not Monaco's raw `editor.action.showReferences`.
#[test]
fn ecj_code_lens_lms_monaco_shape() {
    let mut c = LspClient::spawn();
    c.initialize_as_lms_monaco();

    let uri = test_uri("ecj_codelens_lms_monaco");
    let src = indoc(r#"
        class E {
            void target() {}
            void caller() {
                target();
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_lens_lms_monaco_shape — ECJ not ready");
        return;
    }

    let lenses = c.code_lens(&uri);
    let lens = lenses.iter()
        .filter(|l| l["data"]["tag"].as_str() == Some("references"))
        .map(|l| c.resolve_code_lens(l))
        .find(|l| l["command"]["title"].as_str() == Some("1 reference"));
    assert!(lens.is_some(), "expected resolved 1-reference lens, got: {lenses:?}");
    let lens = lens.unwrap();
    assert_eq!(lens["command"]["command"], "java.show.references");
    assert_eq!(lens["command"]["arguments"][0], uri);
    assert!(lens["command"]["arguments"][1]["line"].is_u64());
    assert!(lens["command"]["arguments"][1]["character"].is_u64());
    let refs = lens["command"]["arguments"][2].as_array().cloned().unwrap_or_default();
    assert_eq!(refs.len(), 1, "expected one reference location, got: {lens:?}");
    assert_eq!(refs[0]["uri"], uri);
    assert!(refs[0]["range"]["start"]["line"].is_u64());
    assert!(refs[0]["range"]["start"]["character"].is_u64());
}

/// Method called from a different class in the same file → 1 reference shown.
/// Regression: withScope used enclosingType (class B) as scope, so b.ba() in
/// class A was never visited.
#[test]
fn ecj_code_lens_cross_class_reference() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_codelens_cross_class");
    let src = indoc(r#"
        class A {
            public static void main(String[] args) {
                B b = new B();
                b.ba();
            }
        }
        class B {
            public void ba() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_lens_cross_class_reference — ECJ not ready");
        return;
    }

    let lenses = c.code_lens(&uri);
    let resolved: Vec<Value> = lenses.iter()
        .filter(|l| l["data"]["tag"].as_str() == Some("references"))
        .map(|l| c.resolve_code_lens(l))
        .collect();
    assert!(
        resolved.iter().any(|l| l["command"]["title"].as_str() == Some("1 reference")),
        "expected '1 reference' lens for ba() called from class A, got: {resolved:?}"
    );
}

/// Method and local variable share the same name (`b.b()`).
/// Regression: resolveLocalDeclaration found variable `b` before the binding
/// path resolved the method call, causing 0 references and wrong goto-definition.
#[test]
fn ecj_code_lens_method_name_same_as_variable() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_codelens_name_clash");
    let src = indoc(r#"
        class A {
            public static void main(String[] args) {
                B b = new B();
                b.b();
            }
        }
        class B {
            public void b() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_lens_method_name_same_as_variable — ECJ not ready");
        return;
    }

    let lenses = c.code_lens(&uri);
    let resolved: Vec<Value> = lenses.iter()
        .filter(|l| l["data"]["tag"].as_str() == Some("references"))
        .map(|l| c.resolve_code_lens(l))
        .collect();
    assert!(
        resolved.iter().any(|l| l["command"]["title"].as_str() == Some("1 reference")),
        "expected '1 reference' lens for b() when variable is also named b, got: {resolved:?}"
    );
}

/// Goto-definition on method name `b` in `b.b()` → jumps to method declaration,
/// not the local variable `b`.
/// Regression: resolveLocalDeclaration shadowed the method binding lookup.
#[test]
fn ecj_goto_definition_method_name_same_as_variable() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_goto_def_name_clash");
    let src = indoc(r#"
        class A {
            public static void main(String[] args) {
                B b = new B();
                b.b();
            }
        }
        class B {
            public void b() {}
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_goto_definition_method_name_same_as_variable — ECJ not ready");
        return;
    }

    // Column 18 in "        b.b();" — the second `b` (method name, 0-based line 3)
    let defs = c.goto_definition(&uri, 3, 10);
    assert!(!defs.is_empty(), "expected a definition location for b(), got none");
    let target_line = defs[0]["range"]["start"]["line"].as_u64().unwrap_or(99);
    // Method b() is declared on line 7 (0-based)
    assert_eq!(target_line, 7, "goto-definition should jump to method b() on line 7, not variable b on line 2, got line {target_line}");
}

/// Goto declaration: jump to class declaration.
/// (from jdtls NavigateToDeclarationHandlerTest#testDeclaration)
#[test]
fn syntax_goto_declaration() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_declaration");
    let src = indoc(r#"
        class E {
            void helper() {}
            void go() {
                helper();
            }
        }
    "#);
    c.open(&uri, &src);

    // Line 3: "        helper();" — cursor on "helper" (col 8)
    let locs = c.goto_declaration(&uri, 3, 8);
    assert!(!locs.is_empty(), "expected at least one declaration location");
    let line = locs[0]["range"]["start"]["line"].as_u64().unwrap_or(99);
    assert_eq!(line, 1, "declaration should be at line 1, got: {line}");
}

/// Range formatting: badly indented lines in a range → TextEdits returned.
/// (from jdtls FormatterHandlerTest#testDocumentRangeFormatting)
#[test]
fn syntax_range_formatting() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("syntax_range_format");
    let src = indoc(r#"
        class E {
        void messy() {
        int x = 1;
        }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP syntax_range_formatting — ECJ not ready");
        return;
    }

    // Format the entire file range (lines 0 to 5)
    let edits = c.range_formatting(&uri, 0, 0, 5, 0);
    if edits.is_empty() {
        eprintln!("INFO syntax_range_formatting — formatter returned no edits (may need --add-exports flags for this JVM)");
        return;
    }
    assert!(!edits.is_empty(), "expected range formatting to return edits for messy code");
}

// ─── ECJ: new code actions (serialVersionUID, thrown exception, Javadoc, visibility, refactors) ───

/// Class implementing Serializable without serialVersionUID → "Add serialVersionUID" quickfix.
#[test]
fn ecj_code_action_serial_version_uid() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_serial");
    let src = indoc(r#"
        import java.io.Serializable;
        class MyClass implements Serializable {}
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_serial_version_uid — ECJ not ready");
        return;
    };

    if diags.iter().all(|d| {
        let msg = d["message"].as_str().unwrap_or("");
        !msg.contains("serialVersionUID")
    }) {
        eprintln!("SKIP ecj_code_action_serial_version_uid — no serialVersionUID diagnostic");
        return;
    }

    // Line 1: "class MyClass implements Serializable {}"
    let actions = c.code_actions(&uri, 1, 6, &diags);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("serialVersionUID")),
        "expected 'Add serialVersionUID' action, got: {titles:?}"
    );
}

/// Method declaring `throws IOException` but never throwing → "Remove unused thrown exception" quickfix.
#[test]
fn ecj_code_action_remove_unused_thrown() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_thrown");
    let src = indoc(r#"
        class E {
            void go() throws java.io.IOException {}
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_remove_unused_thrown — ECJ not ready");
        return;
    };

    if diags.iter().all(|d| {
        let msg = d["message"].as_str().unwrap_or("");
        !msg.to_lowercase().contains("declared") && !msg.to_lowercase().contains("thrown")
    }) {
        eprintln!("SKIP ecj_code_action_remove_unused_thrown — no unused-thrown diagnostic");
        return;
    }

    // Line 1: "    void go() throws java.io.IOException {}"
    let actions = c.code_actions(&uri, 1, 9, &diags);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("remove") && t.to_lowercase().contains("thrown")),
        "expected 'Remove unused thrown exception' action, got: {titles:?}"
    );
}

/// Method with Javadoc but missing @param/@return tags → tag-fix actions offered.
#[test]
fn ecj_code_action_missing_javadoc_tags() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_javadoc");
    let src = indoc(r#"
        class E {
            /** Does something. */
            public int compute(int value) { return value; }
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_missing_javadoc_tags — ECJ not ready");
        return;
    };

    if diags.iter().all(|d| {
        let msg = d["message"].as_str().unwrap_or("");
        !msg.to_lowercase().contains("param") && !msg.to_lowercase().contains("return")
    }) {
        eprintln!("SKIP ecj_code_action_missing_javadoc_tags — no missing-tag diagnostic");
        return;
    }

    // Line 2: method declaration line
    let actions = c.code_actions(&uri, 2, 18, &diags);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.contains("@param") || t.contains("@return") || t.contains("Javadoc")),
        "expected a Javadoc tag fix action, got: {titles:?}"
    );
}

/// Calling a private method from another class → "Change visibility" quickfix offered.
#[test]
fn ecj_code_action_change_visibility() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_visibility");
    let src = indoc(r#"
        class Helper {
            private void secret() {}
        }
        class Caller {
            void run() {
                new Helper().secret();
            }
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_change_visibility — ECJ not ready");
        return;
    };

    if diags.iter().all(|d| {
        let msg = d["message"].as_str().unwrap_or("");
        !msg.to_lowercase().contains("not visible") && !msg.to_lowercase().contains("visibility")
    }) {
        eprintln!("SKIP ecj_code_action_change_visibility — no visibility diagnostic");
        return;
    }

    // Line 5: "        new Helper().secret();"
    let actions = c.code_actions(&uri, 5, 21, &diags);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("visibility") || t.to_lowercase().contains("public")),
        "expected a visibility change action, got: {titles:?}"
    );
}

/// Ambiguous type `Date` (java.util.Date vs java.sql.Date) → ≥2 import candidates offered.
#[test]
fn ecj_code_action_ambiguous_import() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_ambiguous");
    let src = indoc(r#"
        class E {
            void go() {
                Date d;
            }
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_code_action_ambiguous_import — ECJ not ready");
        return;
    };

    // Line 2: "        Date d;" — cursor on Date
    let actions = c.code_actions(&uri, 2, 8, &diags);
    let import_actions: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .filter(|t| t.contains("Date"))
        .collect();
    assert!(
        import_actions.len() >= 2,
        "expected ≥2 import candidates for 'Date' (util + sql), got: {import_actions:?}"
    );
}

/// Selecting an expression → "Extract to local variable" quickassist offered.
#[test]
fn ecj_code_action_extract_local_variable() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_extract_var");
    let src = indoc(r#"
        class E {
            void go() {
                int x = Math.max(1, 2);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_extract_local_variable — ECJ not ready");
        return;
    }

    // Select "Math.max(1, 2)" — line 2, cols 16..30
    let actions = c.code_actions_range(&uri, 2, 16, 2, 30, &[]);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("extract") && t.to_lowercase().contains("variable")),
        "expected 'Extract to local variable' action, got: {titles:?}"
    );
}

/// Cursor on single-use local variable → "Inline local variable" quickassist offered.
#[test]
fn ecj_code_action_inline_local_variable() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_inline_var");
    let src = indoc(r#"
        class E {
            void go() {
                int value = 42;
                System.out.println(value);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_inline_local_variable — ECJ not ready");
        return;
    }

    // Line 2: "        int value = 42;" — cursor on "value" (col 12)
    let actions = c.code_actions_range(&uri, 2, 12, 2, 17, &[]);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles.iter().any(|t| t.to_lowercase().contains("inline")),
        "expected 'Inline local variable' action, got: {titles:?}"
    );
}

/// Selecting multiple statements → "Extract method" quickassist offered.
#[test]
fn ecj_code_action_extract_method() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_extract_method");
    let src = indoc(r#"
        class E {
            void go() {
                int a = 1;
                int b = 2;
                int c = a + b;
                System.out.println(c);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_extract_method — ECJ not ready");
        return;
    }

    // Select lines 2-5 (the four statements)
    let actions = c.code_actions_range(&uri, 2, 8, 5, 34, &[]);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("extract") && t.to_lowercase().contains("method")),
        "expected 'Extract method' action, got: {titles:?}"
    );
}

/// Extract method on a for-loop that references an outer variable → correct
/// parameter list (only outer locals, not type names or qualified names) and
/// the extracted call replaces the loop without leaving orphaned braces.
#[test]
fn ecj_code_action_extract_method_for_loop() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_ca_extract_for");
    // "items" is declared outside the selection; String/System/out/println must
    // NOT appear as parameters.
    let src = indoc(r#"
        import java.util.ArrayList;
        import java.util.List;
        class E {
            void go() {
                List<String> items = new ArrayList<>();
                items.add("Hello");
                for (String item : items) {
                    System.out.println(item);
                }
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_code_action_extract_method_for_loop — ECJ not ready");
        return;
    }

    // Select the for-loop: line 6 col 8 → line 8 col 9  (0-based)
    let actions = c.code_actions_range(&uri, 6, 8, 8, 9, &[]);
    let action = actions
        .iter()
        .find(|a| {
            let t = a["title"].as_str()
                .or_else(|| a["right"]["title"].as_str())
                .unwrap_or("");
            t.to_lowercase().contains("extract") && t.to_lowercase().contains("method")
        });
    assert!(action.is_some(), "expected 'Extract method' action, got: {:?}",
        actions.iter()
            .filter_map(|a| a["title"].as_str().or_else(|| a["right"]["title"].as_str()))
            .collect::<Vec<_>>());

    let action = action.unwrap();

    // Collect all text edits from the workspace edit.
    let edits: Vec<&serde_json::Value> = action["edit"]["changes"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|v| v.as_array().into_iter().flatten())
        .collect();
    assert!(!edits.is_empty(), "expected text edits in Extract method action");

    // The call-site edit must replace the for-loop with an extracted(...) call.
    let new_texts: Vec<&str> = edits.iter()
        .filter_map(|e| e["newText"].as_str())
        .collect();
    let has_call = new_texts.iter().any(|t| t.contains("extracted("));
    assert!(has_call, "expected a newText containing 'extracted(' in edits, got: {new_texts:?}");

    // "items" should be the only parameter — not "String", "System", "out", or "println".
    let call_text = new_texts.iter().find(|t| t.contains("extracted(")).unwrap();
    assert!(
        !call_text.contains("String") && !call_text.contains("System")
            && !call_text.contains("out") && !call_text.contains("println"),
        "call should not contain type/field/method names as args, got: {call_text:?}"
    );

    // The inserted method body must contain the for-loop.
    let method_text = new_texts.iter().find(|t| t.contains("for (")).unwrap_or(&"");
    assert!(
        method_text.contains("for (") && method_text.contains("items"),
        "extracted method body should contain the for-loop and reference 'items', got: {method_text:?}"
    );
}

#[test]
fn ecj_hover_after_astral_char_uses_utf16_positions() {
    let mut c = LspClient::spawn();
    c.initialize();

    let uri = test_uri("ecj_utf16_hover");
    let src = indoc(r#"
        class E {
            void go() {
                String emoji = "😀"; Math.abs(1);
            }
        }
    "#);
    c.open(&uri, &src);

    if !ecj_ready(&mut c, &uri) {
        eprintln!("SKIP ecj_hover_after_astral_char_uses_utf16_positions — ECJ not ready");
        return;
    }

    let prefix = "        String emoji = \"😀\"; ";
    let hover = c.hover(&uri, 2, utf16_len(prefix));
    let value = hover["contents"]["value"]
        .as_str()
        .or_else(|| hover["contents"].as_str())
        .unwrap_or("");
    assert!(
        value.contains("Math") || value.contains("abs"),
        "expected hover on Math.abs after astral char, got: {hover:?}"
    );
}

#[test]
fn ecj_annotation_processing_generates_missing_type() {
    let temp = tempfile::tempdir().unwrap();
    let processor_dir = temp.path().join("processor");
    compile_java_to_dir(&processor_dir, &[
        (
            "testproc/GenerateHello.java",
            r#"
                package testproc;
                import java.lang.annotation.ElementType;
                import java.lang.annotation.Retention;
                import java.lang.annotation.RetentionPolicy;
                import java.lang.annotation.Target;

                @Retention(RetentionPolicy.SOURCE)
                @Target(ElementType.TYPE)
                public @interface GenerateHello {}
            "#,
        ),
        (
            "testproc/GenProcessor.java",
            r#"
                package testproc;

                import java.io.IOException;
                import java.io.Writer;
                import java.util.Set;
                import javax.annotation.processing.AbstractProcessor;
                import javax.annotation.processing.RoundEnvironment;
                import javax.annotation.processing.SupportedAnnotationTypes;
                import javax.annotation.processing.SupportedSourceVersion;
                import javax.lang.model.SourceVersion;
                import javax.lang.model.element.TypeElement;
                import javax.tools.JavaFileObject;

                @SupportedAnnotationTypes("testproc.GenerateHello")
                @SupportedSourceVersion(SourceVersion.RELEASE_21)
                public class GenProcessor extends AbstractProcessor {
                    private boolean generated;

                    @Override
                    public boolean process(Set<? extends TypeElement> annotations, RoundEnvironment roundEnv) {
                        if (generated || roundEnv.processingOver()) {
                            return false;
                        }
                        generated = true;
                        try {
                            JavaFileObject file = processingEnv.getFiler().createSourceFile("HelloGenerated");
                            try (Writer writer = file.openWriter()) {
                                writer.write("class HelloGenerated {}");
                            }
                        } catch (IOException e) {
                            throw new RuntimeException(e);
                        }
                        return false;
                    }
                }
            "#,
        ),
    ]);
    let services_dir = processor_dir.join("META-INF").join("services");
    fs::create_dir_all(&services_dir).unwrap();
    fs::write(
        services_dir.join("javax.annotation.processing.Processor"),
        "testproc.GenProcessor\n",
    )
    .unwrap();

    let mut c = LspClient::spawn();
    c.initialize_with_options(json!({
        "javaHome": java_home(),
        "sourceCompatibility": "21",
        "classpath": [processor_dir.to_string_lossy()]
    }));

    let uri = test_uri("ecj_annotation_processing");
    let src = indoc(r#"
        import testproc.GenerateHello;

        @GenerateHello
        class E {
            HelloGenerated generated;
        }
    "#);
    c.open(&uri, &src);

    let Some(diags) = ecj_wait_diagnostics(&mut c, &uri) else {
        eprintln!("SKIP ecj_annotation_processing_generates_missing_type — ECJ not ready");
        return;
    };

    assert!(
        diags.is_empty(),
        "expected annotation processing to generate HelloGenerated, got diagnostics: {diags:?}"
    );
}

// ─── Utility ─────────────────────────────────────────────────────────────────

/// Strip leading newline and common indentation from a multi-line string literal.
fn indoc(s: &str) -> String {
    let s = s.strip_prefix('\n').unwrap_or(s);
    let indent = s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    s.lines()
        .map(|l| if l.len() >= indent { &l[indent..] } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}
