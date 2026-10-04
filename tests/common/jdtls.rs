//! Test harness replicating the eclipse.jdt.ls test fixtures
//! (`AbstractProjectsManagerBasedTest`, `AbstractQuickFixTest`, …) on top of
//! the LSP protocol: fixtures are copied from `tests/fixtures/projects` into a
//! temporary "working projects" directory, the server is started on them, and
//! requests go over stdio exactly like a real client.

#![allow(dead_code)]

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Url;

pub const TEST_PROJECT_NAME: &str = "TestProject";

/// `true` when tests run against the reference Java jdt.ls (`JDTLS_ORACLE=1`).
pub fn is_oracle() -> bool {
    std::env::var("JDTLS_ORACLE").is_ok_and(|v| !v.is_empty() && v != "0")
}

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn java_home() -> String {
    std::env::var("JAVA_HOME").unwrap_or_else(|_| {
        let out = Command::new("/usr/libexec/java_home").output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_owned(),
            _ => String::new(),
        }
    })
}

// ─── Wire client ──────────────────────────────────────────────────────────────

pub struct LspClient {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
    /// Notifications received while waiting for something else.
    pub notifications: Vec<Value>,
    /// Canned results for server→client requests, by method
    /// (e.g. `workspace/executeClientCommand`).
    pub request_results: BTreeMap<String, Value>,
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> std::io::Result<Value> {
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed"));
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length: ") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

impl LspClient {
    /// Spawn the server under test.  With `JDTLS_ORACLE=1` the reference
    /// Java eclipse.jdt.ls (`scripts/oracle-jdtls.sh`) is spawned instead, so
    /// the same test can be run against upstream to confirm expectations.
    pub fn spawn() -> Self {
        Self::spawn_in(None)
    }

    pub fn spawn_in(data_dir: Option<&Path>) -> Self {
        let mut cmd = if is_oracle() {
            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/oracle-jdtls.sh");
            let data = data_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::temp_dir().join(format!("jdtls-oracle-{}", std::process::id())));
            let mut c = Command::new(script);
            c.arg(data);
            c
        } else {
            Command::new(env!("CARGO_BIN_EXE_jdtls-rust"))
        };
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var("JDTLS_TEST_STDERR").is_ok() { Stdio::inherit() } else { Stdio::null() })
            .spawn()
            .expect("spawn jdtls-rust");
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(m) = read_message(&mut stdout) {
                if tx.send(m).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin, rx, next_id: 1, notifications: Vec::new(), request_results: BTreeMap::new() }
    }

    pub fn send(&mut self, msg: &Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Handle a server→client request (respond with `null`/defaults).
    fn answer_server_request(&mut self, msg: &Value) {
        let id = msg["id"].clone();
        let canned = msg["method"].as_str().and_then(|m| self.request_results.get(m)).cloned();
        let result = match msg["method"].as_str() {
            _ if canned.is_some() => canned.unwrap(),
            Some("workspace/configuration") => {
                let n = msg["params"]["items"].as_array().map_or(0, |a| a.len());
                Value::Array(vec![Value::Null; n])
            }
            _ => Value::Null,
        };
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn recv_until(&mut self, timeout: Duration, mut pred: impl FnMut(&Value) -> bool) -> Option<Value> {
        // Check buffered notifications first.
        if let Some(i) = self.notifications.iter().position(|m| pred(m)) {
            return Some(self.notifications.remove(i));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let msg = self.rx.recv_timeout(remaining).ok()?;
            if msg.get("method").is_some() && msg.get("id").is_some() {
                self.answer_server_request(&msg);
                continue;
            }
            if pred(&msg) {
                return Some(msg);
            }
            if msg.get("method").is_some() {
                self.notifications.push(msg);
            }
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let resp = self
            .recv_until(Duration::from_secs(90), |m| m["id"] == json!(id) && m.get("method").is_none())
            .unwrap_or_else(|| panic!("timed out waiting for {method}"));
        if let Some(err) = resp.get("error") {
            panic!("{method} failed: {err}");
        }
        resp["result"].clone()
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

// ─── Workspace fixture ────────────────────────────────────────────────────────

/// One test's workspace: the `target/workingProjects` equivalent plus the
/// server serving it.  The server starts lazily on the first request so that
/// project setup (import/creation) happens before `initialize`, like jdt.ls
/// tests which call the handlers after `importProjects`.
pub struct Workspace {
    _tmp: tempfile::TempDir,
    pub dir: PathBuf,
    roots: Vec<PathBuf>,
    client: Option<LspClient>,
    pub settings: Value,
    pub init_options: Value,
    pub capabilities: Value,
    versions: BTreeMap<String, i32>,
}

impl Workspace {
    pub fn new() -> Self {
        let tmp = tempfile::Builder::new().prefix("jdtls-ws").tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap().join("workingProjects");
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            _tmp: tmp,
            dir,
            roots: Vec::new(),
            client: None,
            settings: json!({ "java": {} }),
            // `AbstractProjectsManagerBasedTest.initPreferenceManager(true)`:
            // the client supports class file contents (jdt:// URIs).
            init_options: json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } }),
            capabilities: default_client_capabilities(),
            versions: BTreeMap::new(),
        }
    }

    /// `AbstractProjectsManagerBasedTest.importProjects`: copy
    /// `projects/<path>` to the working directory and import it.
    pub fn import_projects(&mut self, paths: &[&str]) -> &mut Self {
        for path in paths {
            let from = fixtures_dir().join("projects").join(path);
            let to = self.dir.join(path);
            if to.exists() {
                std::fs::remove_dir_all(&to).ok();
            }
            copy_dir(&from, &to);
            self.add_root(to);
        }
        self
    }

    fn add_root(&mut self, root: PathBuf) {
        if self.roots.contains(&root) {
            return;
        }
        self.roots.push(root.clone());
        if let Some(c) = self.client.as_mut() {
            let uri = Url::from_file_path(&root).unwrap().to_string();
            let name = root.file_name().unwrap().to_string_lossy().into_owned();
            c.notify(
                "workspace/didChangeWorkspaceFolders",
                json!({ "event": { "added": [{ "uri": uri, "name": name }], "removed": [] } }),
            );
            self.wait_idle();
        }
    }

    /// `AbstractProjectsManagerBasedTest.newEmptyProject`: an Eclipse Java
    /// project named `TestProject` with source folder `src`.
    pub fn new_empty_project(&mut self, options: &BTreeMap<String, String>) -> PathBuf {
        self.new_project(TEST_PROJECT_NAME, options)
    }

    pub fn new_project(&mut self, name: &str, options: &BTreeMap<String, String>) -> PathBuf {
        let root = self.dir.join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join(".project"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription>\n\t<name>{name}</name>\n\t<comment></comment>\n\t<projects>\n\t</projects>\n\t<buildSpec>\n\t\t<buildCommand>\n\t\t\t<name>org.eclipse.jdt.core.javabuilder</name>\n\t\t\t<arguments>\n\t\t\t</arguments>\n\t\t</buildCommand>\n\t</buildSpec>\n\t<natures>\n\t\t<nature>org.eclipse.jdt.core.javanature</nature>\n\t</natures>\n</projectDescription>\n"
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".classpath"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<classpath>\n\t<classpathentry kind=\"src\" path=\"src\"/>\n\t<classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/>\n\t<classpathentry kind=\"output\" path=\"bin\"/>\n</classpath>\n",
        )
        .unwrap();
        self.set_project_options(&root, options);
        self.add_root(root.clone());
        root
    }

    /// `IJavaProject.setOptions`: persisted as the project's JDT core prefs.
    pub fn set_project_options(&mut self, root: &Path, options: &BTreeMap<String, String>) {
        let settings = root.join(".settings");
        std::fs::create_dir_all(&settings).unwrap();
        let mut text = String::from("eclipse.preferences.version=1\n");
        for (k, v) in options {
            text.push_str(&format!("{k}={}\n", v.replace('\\', "\\\\").replace('\n', "\\n")));
        }
        let prefs = settings.join("org.eclipse.jdt.core.prefs");
        std::fs::write(&prefs, text).unwrap();
        if self.client.is_some() {
            self.file_changed(&prefs, 2);
        }
    }

    /// `IPackageFragment.createCompilationUnit`: write a source file into
    /// `<project>/<source folder>/<package path>/<name>` and tell the server.
    pub fn create_cu(&mut self, project_root: &Path, src: &str, package: &str, name: &str, content: &str) -> String {
        let mut path = project_root.join(src);
        for seg in package.split('.').filter(|s| !s.is_empty()) {
            path.push(seg);
        }
        std::fs::create_dir_all(&path).unwrap();
        path.push(name);
        let existed = path.exists();
        std::fs::write(&path, content).unwrap();
        let uri = Url::from_file_path(&path).unwrap().to_string();
        if self.client.is_some() {
            self.file_changed(&path, if existed { 2 } else { 1 });
        }
        uri
    }

    fn file_changed(&mut self, path: &Path, typ: u32) {
        let uri = Url::from_file_path(path).unwrap().to_string();
        let c = self.client.as_mut().unwrap();
        c.notify("workspace/didChangeWatchedFiles", json!({ "changes": [{ "uri": uri, "type": typ }] }));
        self.wait_idle();
    }

    /// Round-trip a cheap request so preceding notifications are processed.
    pub fn wait_idle(&mut self) {
        let c = self.client.as_mut().unwrap();
        c.request("workspace/executeCommand", json!({ "command": "java.project.getAll", "arguments": [] }));
    }

    /// Start the server (if needed) and return the client.
    pub fn client(&mut self) -> &mut LspClient {
        if self.client.is_none() {
            let mut c = LspClient::spawn_in(Some(&self.dir.parent().unwrap().join("oracle-data")));
            let folders: Vec<Value> = self
                .roots
                .iter()
                .map(|r| json!({ "uri": Url::from_file_path(r).unwrap().to_string(), "name": r.file_name().unwrap().to_string_lossy() }))
                .collect();
            let mut init = self.init_options.clone();
            if init.get("javaHome").is_none() {
                init["javaHome"] = json!(java_home());
            }
            init["settings"] = self.settings.clone();
            c.request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": Url::from_file_path(&self.dir).unwrap().to_string(),
                    "workspaceFolders": folders,
                    "capabilities": self.capabilities,
                    "initializationOptions": init,
                }),
            );
            c.notify("initialized", json!({}));
            c.recv_until(Duration::from_secs(120), |m| {
                m["method"] == "language/status" && m["params"]["type"] == "ServiceReady"
            })
            .expect("server never reported ServiceReady");
            self.client = Some(c);
        }
        self.client.as_mut().unwrap()
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.client().request(method, params)
    }

    /// Whether the server has been started.
    pub fn client_started(&self) -> bool {
        self.client.is_some()
    }

    // ── Lookup ───────────────────────────────────────────────────────────────

    /// `ClassFileUtil.getURI(project, fqn)` for source types: the file under
    /// the project whose package path + name matches `fqn`.
    pub fn class_uri(&self, project: &str, fqn: &str) -> String {
        let rel = format!("{}.java", fqn.replace('.', "/"));
        let root = self.project_root(project);
        let found = walk_files(&root)
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(&format!("/{rel}")))
            .min_by_key(|p| p.components().count())
            .unwrap_or_else(|| panic!("type {fqn} not found in project {project}"));
        Url::from_file_path(found).unwrap().to_string()
    }

    /// `ClassFileUtil.getURI(project, fqn)`: the URI of a source or binary
    /// type (`java.util.Map$Entry` for member types), looked up by the server
    /// like JDT's type-name search (case-insensitive exact match).  Binary
    /// types get jdt.ls `jdt://contents/<jar>/<package>/<SourceFile>?<handle>`
    /// URIs.  `project` is a project name (`jdt.ls-java-project` for the
    /// default project).
    pub fn class_file_uri(&mut self, project: &str, fqn: &str) -> String {
        self.try_class_file_uri(project, fqn)
            .unwrap_or_else(|| panic!("type {fqn} not found in project {project}"))
    }

    pub fn try_class_file_uri(&mut self, project: &str, fqn: &str) -> Option<String> {
        if is_oracle() {
            return self.oracle_type_uri(project, fqn);
        }
        let v = self.request(
            "workspace/executeCommand",
            json!({ "command": "jdtls-rust.classFileUri", "arguments": [project, fqn] }),
        );
        v.as_str().map(str::to_owned)
    }

    /// Real jdt.ls has no `jdtls-rust.classFileUri`; find the type through
    /// `workspace/symbol` (which reports binary types with jdt:// URIs).
    fn oracle_type_uri(&mut self, project: &str, fqn: &str) -> Option<String> {
        let dotted = fqn.replace('$', ".");
        let (container, simple) = dotted.rsplit_once('.').unwrap_or(("", dotted.as_str()));
        let result = self.request("workspace/symbol", json!({ "query": simple }));
        let symbols = result.as_array().cloned().unwrap_or_default();
        let matches = |s: &Value, exact: bool| {
            let name = s["name"].as_str().unwrap_or("");
            let cont = s["containerName"].as_str().unwrap_or("");
            if exact { name == simple && cont == container } else { name.eq_ignore_ascii_case(simple) && cont.eq_ignore_ascii_case(container) }
        };
        let root = self
            .roots
            .iter()
            .flat_map(|r| std::iter::once(r.clone()).chain(walk_dirs(r)))
            .find(|d| project_name_of(d).as_deref() == Some(project))
            .map(|d| Url::from_file_path(d).unwrap().to_string() + "/");
        let in_project = |s: &Value| {
            let uri = s["location"]["uri"].as_str().unwrap_or("");
            if uri.starts_with("jdt:") {
                uri.contains(&format!("={project}/"))
            } else {
                root.as_ref().is_some_and(|r| uri.starts_with(r.as_str()))
            }
        };
        for exact in [true, false] {
            if let Some(s) = symbols.iter().find(|s| matches(s, exact) && in_project(s)).or_else(|| symbols.iter().find(|s| matches(s, exact))) {
                return s["location"]["uri"].as_str().map(str::to_owned);
            }
        }
        None
    }

    /// `workspace/didChangeConfiguration` with `settings` (also kept as the
    /// initial settings when the server hasn't started yet).
    pub fn update_settings(&mut self, settings: Value) {
        self.settings = settings.clone();
        if self.client.is_some() {
            self.client().notify("workspace/didChangeConfiguration", json!({ "settings": settings }));
            self.wait_idle();
        }
    }

    /// Root directory of the project named `name` (Eclipse `.project` name,
    /// Maven artifactId, or directory name).
    pub fn project_root(&self, name: &str) -> PathBuf {
        for root in &self.roots {
            for dir in std::iter::once(root.clone()).chain(walk_dirs(root)) {
                if project_name_of(&dir).as_deref() == Some(name) {
                    return dir;
                }
            }
        }
        panic!("project {name} not found");
    }

    /// `File.toURI()` form of a directory, as returned by `java.project.getAll`.
    pub fn project_uri(&self, name: &str) -> String {
        let root = self.project_root(name);
        let mut s = String::from("file:");
        s.push_str(&root.to_string_lossy().replace(' ', "%20"));
        s.push('/');
        s
    }

    pub fn path_uri(&self, rel: &str) -> String {
        Url::from_file_path(self.dir.join(rel)).unwrap().to_string()
    }

    // ── Documents ────────────────────────────────────────────────────────────

    pub fn open(&mut self, uri: &str) {
        let text = std::fs::read_to_string(Url::parse(uri).unwrap().to_file_path().unwrap()).unwrap();
        self.open_with(uri, &text);
    }

    pub fn open_with(&mut self, uri: &str, text: &str) {
        self.versions.insert(uri.to_owned(), 1);
        self.client().notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "java", "version": 1, "text": text } }),
        );
    }

    pub fn change(&mut self, uri: &str, text: &str) {
        let v = self.versions.entry(uri.to_owned()).or_insert(1);
        *v += 1;
        let v = *v;
        self.client().notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": v }, "contentChanges": [{ "text": text }] }),
        );
    }

    pub fn close(&mut self, uri: &str) {
        self.client().notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }));
    }

    /// Latest `publishDiagnostics` for `uri` after a fresh build.
    pub fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        let c = self.client();
        c.notifications.retain(|m| !(m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri));
        // jdt.ls `DiagnosticsCommand.refreshDiagnostics(uri, scope, syntaxOnly)`.
        c.request(
            "workspace/executeCommand",
            json!({ "command": "java.project.refreshDiagnostics", "arguments": [uri, "thisFile", false] }),
        );
        let msg = c
            .recv_until(Duration::from_secs(60), |m| {
                m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri
            })
            .unwrap_or_else(|| panic!("no diagnostics for {uri}"));
        msg["params"]["diagnostics"].as_array().cloned().unwrap_or_default()
    }

    /// Current file content on disk.
    pub fn read(&self, uri: &str) -> String {
        std::fs::read_to_string(Url::parse(uri).unwrap().to_file_path().unwrap()).unwrap()
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_client_capabilities() -> Value {
    json!({
        "workspace": {
            "applyEdit": true,
            "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create", "rename", "delete"] },
            "didChangeWatchedFiles": { "dynamicRegistration": true },
            "executeCommand": { "dynamicRegistration": true },
            "configuration": true,
            "workspaceFolders": true
        },
        "textDocument": {
            "synchronization": { "willSave": true, "willSaveWaitUntil": true, "didSave": true },
            "completion": {
                "completionItem": {
                    "snippetSupport": true,
                    "documentationFormat": ["markdown", "plaintext"],
                    "resolveSupport": { "properties": ["documentation", "detail", "additionalTextEdits"] },
                    "insertReplaceSupport": false,
                    "labelDetailsSupport": true
                }
            },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "signatureHelp": { "signatureInformation": { "documentationFormat": ["markdown", "plaintext"], "parameterInformation": { "labelOffsetSupport": true }, "activeParameterSupport": true } },
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "codeAction": {
                "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"] } },
                "resolveSupport": { "properties": ["edit"] },
                "dataSupport": true
            },
            "foldingRange": { "lineFoldingOnly": true },
            "semanticTokens": { "requests": { "full": true }, "tokenTypes": [], "tokenModifiers": [], "formats": ["relative"] },
            "rename": { "prepareSupport": true }
        }
    })
}

// ─── Fixture utilities ────────────────────────────────────────────────────────

pub fn copy_dir(from: &Path, to: &Path) {
    if from.is_file() {
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::copy(from, to).unwrap();
        return;
    }
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap_or_else(|_| panic!("fixture {} missing", from.display())).flatten() {
        let p = e.path();
        let target = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &target);
        } else {
            std::fs::copy(&p, &target).unwrap();
        }
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn walk_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(p.clone());
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

fn project_name_of(dir: &Path) -> Option<String> {
    if let Ok(s) = std::fs::read_to_string(dir.join(".project")) {
        if let Some(start) = s.find("<name>") {
            let rest = &s[start + 6..];
            if let Some(end) = rest.find("</name>") {
                return Some(rest[..end].trim().to_owned());
            }
        }
    }
    if let Ok(s) = std::fs::read_to_string(dir.join("pom.xml")) {
        // artifactId directly under <project> (skip the <parent> block).
        let without_parent = match (s.find("<parent>"), s.find("</parent>")) {
            (Some(a), Some(b)) if a < b => format!("{}{}", &s[..a], &s[b..]),
            _ => s.clone(),
        };
        if let Some(start) = without_parent.find("<artifactId>") {
            let rest = &without_parent[start + 12..];
            if let Some(end) = rest.find("</artifactId>") {
                return Some(rest[..end].trim().to_owned());
            }
        }
    }
    if dir.join("build.gradle").exists() || dir.join("settings.gradle").exists() || dir.join("build.gradle.kts").exists() {
        return dir.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    None
}

// ─── Assertion helpers (ports of jdt.ls test utilities) ──────────────────────

pub fn pos(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

pub fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
    json!({ "start": pos(sl, sc), "end": pos(el, ec) })
}

/// `ResourceUtils.dos2Unix`.
pub fn dos2unix(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Apply LSP text edits to `text` (jdt.ls `TextEditUtil.apply`).
pub fn apply_edits(text: &str, edits: &[Value]) -> String {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let to_offset = |p: &Value| -> usize {
        let line = p["line"].as_u64().unwrap() as usize;
        let ch = p["character"].as_u64().unwrap() as usize;
        let Some(&start) = line_starts.get(line) else { return text.len() };
        let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        let mut units = 0;
        for (i, c) in text[start..line_end].char_indices() {
            if units >= ch {
                return start + i;
            }
            units += c.len_utf16();
        }
        line_end
    };
    let mut spans: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|e| (to_offset(&e["range"]["start"]), to_offset(&e["range"]["end"]), e["newText"].as_str().unwrap_or("").to_owned()))
        .collect();
    // Stable: equal starts keep request order.
    let mut indexed: Vec<(usize, (usize, usize, String))> = spans.drain(..).enumerate().collect();
    indexed.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.0.cmp(&a.0)));
    let mut out = text.to_owned();
    for (_, (s, e, t)) in indexed {
        out.replace_range(s..e, &t);
    }
    out
}

/// jdt.ls `TestOptions.getDefaultOptions()` (on top of JavaCore defaults).
pub fn test_default_options() -> BTreeMap<String, String> {
    let pairs = [
        ("org.eclipse.jdt.core.compiler.problem.localVariableHiding", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.fieldHiding", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedLocal", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.rawTypeReference", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedWarningToken", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.deadCode", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedImport", "error"),
        ("org.eclipse.jdt.core.formatter.tabulation.char", "space"),
        ("org.eclipse.jdt.core.formatter.tabulation.size", "4"),
        ("org.eclipse.jdt.core.compiler.compliance", "1.8"),
        ("org.eclipse.jdt.core.compiler.source", "1.8"),
        ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "1.8"),
    ];
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}
