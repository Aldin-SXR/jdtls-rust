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
    pub(crate) next_id: u64,
    /// Notifications received while waiting for something else.
    pub notifications: Vec<Value>,
    /// Canned results for server→client requests, by method
    /// (e.g. `workspace/executeClientCommand`).
    pub request_results: BTreeMap<String, Value>,
    /// Dynamic replies for requests with server-generated opaque identities.
    pub request_handlers: BTreeMap<String, Box<dyn FnMut(&Value) -> Value + Send>>,
    /// Every server→client request received (e.g. `client/registerCapability`,
    /// `workspace/applyEdit`), in arrival order.
    pub server_requests: Vec<Value>,
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> std::io::Result<Value> {
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "closed",
            ));
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
    serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

impl LspClient {
    /// Spawn the server under test.  With `JDTLS_ORACLE=1` the reference
    /// Java eclipse.jdt.ls (`scripts/oracle-jdtls.sh`) is spawned instead, so
    /// the same test can be run against upstream to confirm expectations.
    pub fn spawn() -> Self {
        Self::spawn_in(None)
    }

    pub fn spawn_in(data_dir: Option<&Path>) -> Self {
        Self::spawn_in_with_java_options(data_dir, &[], None)
    }

    fn spawn_in_with_java_options(data_dir: Option<&Path>, java_options: &[String], oracle_home: Option<&Path>) -> Self {
        let mut cmd = if is_oracle() {
            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/oracle-jdtls.sh");
            let data = data_dir.map(Path::to_path_buf).unwrap_or_else(|| {
                std::env::temp_dir().join(format!("jdtls-oracle-{}", std::process::id()))
            });
            let mut c = Command::new(script);
            c.arg(data);
            if let Some(home) = oracle_home {
                c.env("JDTLS_ORACLE_HOME", home);
            }
            c
        } else {
            let mut c = Command::new(env!("CARGO_BIN_EXE_jdtls-rust"));
            // Like jdt.ls: `-data <workspace>` is the server's metadata area.
            if let Some(d) = data_dir {
                c.arg("-data").arg(d.join("workspace"));
            }
            c
        };
        if is_oracle() && !java_options.is_empty() {
            let existing = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
            cmd.env(
                "JAVA_TOOL_OPTIONS",
                format!("{existing} {}", java_options.join(" ")),
            );
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var("JDTLS_TEST_STDERR").is_ok() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
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
        Self {
            child,
            stdin,
            rx,
            next_id: 1,
            notifications: Vec::new(),
            request_results: BTreeMap::new(),
            request_handlers: BTreeMap::new(),
            server_requests: Vec::new(),
        }
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
        self.server_requests.push(msg.clone());
        let id = msg["id"].clone();
        let canned = msg["method"]
            .as_str()
            .and_then(|m| self.request_results.get(m))
            .cloned();
        let result = match msg["method"].as_str() {
            Some(method) if self.request_handlers.contains_key(method) => {
                self.request_handlers.get_mut(method).unwrap()(msg)
            }
            _ if canned.is_some() => canned.unwrap(),
            Some("workspace/configuration") => {
                let n = msg["params"]["items"].as_array().map_or(0, |a| a.len());
                Value::Array(vec![Value::Null; n])
            }
            _ => Value::Null,
        };
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn recv_until(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&Value) -> bool,
    ) -> Option<Value> {
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
        let resp = self.request_response(method, params);
        if let Some(err) = resp.get("error") {
            panic!("{method} failed: {err}");
        }
        resp["result"].clone()
    }

    /// Send a request and return the whole response message (`result` or
    /// `error`), for tests that assert on response errors.
    pub fn request_response(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.recv_until(Duration::from_secs(90), |m| {
            m["id"] == json!(id) && m.get("method").is_none()
        })
        .unwrap_or_else(|| panic!("timed out waiting for {method}"))
    }

    /// Wait until no message has arrived for `quiet` (at most `max`),
    /// buffering notifications and answering server requests meanwhile.
    pub fn settle(&mut self, quiet: Duration, max: Duration) {
        let deadline = Instant::now() + max;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            let Ok(msg) = self.rx.recv_timeout(quiet.min(remaining)) else {
                return;
            };
            if msg.get("method").is_some() && msg.get("id").is_some() {
                self.answer_server_request(&msg);
            } else if msg.get("method").is_some() {
                self.notifications.push(msg);
            }
        }
    }

    /// Remove and return the buffered notifications named `method`.
    pub fn take_notifications(&mut self, method: &str) -> Vec<Value> {
        let (taken, kept) = std::mem::take(&mut self.notifications)
            .into_iter()
            .partition(|m| m["method"] == method);
        self.notifications = kept;
        taken
    }

    /// Remove and return the recorded server→client requests named `method`.
    pub fn take_server_requests(&mut self, method: &str) -> Vec<Value> {
        let (taken, kept) = std::mem::take(&mut self.server_requests)
            .into_iter()
            .partition(|m| m["method"] == method);
        self.server_requests = kept;
        taken
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
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) client: Option<LspClient>,
    pub settings: Value,
    pub init_options: Value,
    pub capabilities: Value,
    /// JVM properties set directly by the upstream test (oracle only).
    pub oracle_java_options: Vec<String>,
    /// Isolated oracle product with test-only extensions, when needed.
    pub oracle_home: Option<PathBuf>,
    /// The `initialize` result, once the server has started.
    pub initialize_result: Value,
    versions: BTreeMap<String, i32>,
}

impl Workspace {
    pub fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("jdtls-ws")
            .tempdir()
            .unwrap();
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
            oracle_java_options: Vec::new(),
            oracle_home: None,
            initialize_result: Value::Null,
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

    pub(crate) fn add_root(&mut self, root: PathBuf) {
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

    /// Use the upstream TestVMType library matching the fixture's JRE
    /// container, without source attachments. Keep its other entries.
    pub fn use_upstream_test_jdk(&mut self, project: &str) {
        assert!(self.client.is_none(), "configure the test JDK before starting the server");
        let root = self.project_root(project);
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let path = root.join(".classpath");
        let mut text = std::fs::read_to_string(&path).unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        let ranges: Vec<_> = doc.descendants().filter(|n| n.tag_name().name() == "classpathentry"
            && n.attribute("kind") == Some("con")
            && n.attribute("path").is_some_and(|p| p.starts_with("org.eclipse.jdt.launching.JRE_CONTAINER")))
            .map(|n| n.range()).collect();
        assert_eq!(ranges.len(), 1, "missing or ambiguous JRE container: {}",path.display());
        let container = doc.descendants().find(|n| n.range() == ranges[0]).unwrap();
        let version = container.attribute("path").unwrap().rsplit('/').next().unwrap()
            .strip_prefix("JavaSE-").unwrap_or("21");
        std::fs::copy(fixtures_dir().join(format!("fakejdk/{version}/rtstubs.jar")), root.join("lib/rtstubs.jar")).unwrap();
        text.replace_range(ranges[0].clone(), "<classpathentry kind=\"lib\" path=\"lib/rtstubs.jar\"/>");
        std::fs::write(path,text).unwrap();
    }

    /// m2e retains explicit libraries in .classpath. Make the test VM's
    /// sourceless classes available alongside the unchanged Maven dependencies.
    pub fn use_upstream_maven_test_jdk(&mut self, project: &str, version: &str) {
        assert!(self.client.is_none(), "configure the test JDK before starting the server");
        let root = self.project_root(project);
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::copy(fixtures_dir().join(format!("fakejdk/{version}/rtstubs.jar")), root.join("lib/rtstubs.jar")).unwrap();
        let path = root.join(".classpath");
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "<classpath><classpathentry kind=\"src\" path=\"src/main/java\"/><classpathentry kind=\"src\" path=\"src/test/java\"/><classpathentry kind=\"con\" path=\"org.eclipse.m2e.MAVEN2_CLASSPATH_CONTAINER\"/><classpathentry kind=\"output\" path=\"target/classes\"/></classpath>".to_owned());
        let end = text.rfind("</classpath>").unwrap();
        text.insert_str(end, "<classpathentry kind=\"lib\" path=\"lib/rtstubs.jar\"/>");
        std::fs::write(path,text).unwrap();
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
            text.push_str(&format!(
                "{k}={}\n",
                v.replace('\\', "\\\\").replace('\n', "\\n")
            ));
        }
        let prefs = settings.join("org.eclipse.jdt.core.prefs");
        std::fs::write(&prefs, text).unwrap();
        if self.client.is_some() {
            self.file_changed(&prefs, 2);
        }
    }

    /// `IJavaProject.setOption`.
    pub fn set_project_option(&mut self, root: &Path, key: &str, value: &str) {
        let prefs = root.join(".settings").join("org.eclipse.jdt.core.prefs");
        let mut options = BTreeMap::new();
        for line in std::fs::read_to_string(&prefs).unwrap_or_default().lines() {
            if let Some((k, v)) = line.split_once('=') {
                if k != "eclipse.preferences.version" {
                    options.insert(k.to_owned(), v.replace("\\n", "\n").replace("\\\\", "\\"));
                }
            }
        }
        options.insert(key.to_owned(), value.to_owned());
        self.set_project_options(root, &options);
    }

    /// `JavaProjectHelper.addLibrary`: a library jar copied into the project.
    pub fn add_library(&mut self, root: &Path, jar: &Path) {
        assert!(self.client.is_none(), "add libraries before the server starts");
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        let name = jar.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::copy(jar, lib.join(&name)).unwrap();
        let path = root.join(".classpath");
        let mut text = std::fs::read_to_string(&path).unwrap();
        let end = text.rfind("</classpath>").unwrap();
        text.insert_str(end, &format!("\t<classpathentry kind=\"lib\" path=\"lib/{name}\"/>\n"));
        std::fs::write(path, text).unwrap();
    }

    /// `IPackageFragment.createCompilationUnit`: write a source file into
    /// `<project>/<source folder>/<package path>/<name>` and tell the server.
    pub fn create_cu(
        &mut self,
        project_root: &Path,
        src: &str,
        package: &str,
        name: &str,
        content: &str,
    ) -> String {
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
        c.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": [{ "uri": uri, "type": typ }] }),
        );
        self.wait_idle();
    }

    /// Round-trip a cheap request so preceding notifications are processed.
    pub fn wait_idle(&mut self) {
        let c = self.client();
        c.request(
            "workspace/executeCommand",
            json!({ "command": "java.project.getAll", "arguments": [] }),
        );
    }

    /// Start the server (if needed) and return the client.
    pub fn client(&mut self) -> &mut LspClient {
        if self.client.is_none() {
            let mut c = LspClient::spawn_in_with_java_options(
                Some(&self.dir.parent().unwrap().join("oracle-data")),
                &self.oracle_java_options,
                self.oracle_home.as_deref(),
            );
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
            // vscode-java sends the workspace folders in the initialization
            // options too; jdt.ls takes its root paths from there
            // (`BaseInitHandler`), falling back to `rootUri`.
            if init.get("workspaceFolders").is_none() {
                let uris: Vec<Value> = self
                    .roots
                    .iter()
                    .map(|r| json!(Url::from_file_path(r).unwrap().to_string()))
                    .collect();
                init["workspaceFolders"] = Value::Array(uris);
            }
            self.initialize_result = c.request(
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
            if exact {
                name == simple && cont == container
            } else {
                name.eq_ignore_ascii_case(simple) && cont.eq_ignore_ascii_case(container)
            }
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
            // The fixture explicitly supplies the upstream test VM library.
            // Maven also retains the host VM, so select the fixture's class
            // file when workspace/symbol reports both binary roots.
            if let Some(s) = symbols.iter().find(|s| matches(s, exact) && in_project(s)
                && s["location"]["uri"].as_str().is_some_and(|uri| uri.starts_with("jdt://contents/rtstubs.jar/"))) {
                return s["location"]["uri"].as_str().map(str::to_owned);
            }
            if let Some(s) = symbols
                .iter()
                .find(|s| matches(s, exact) && in_project(s))
                .or_else(|| symbols.iter().find(|s| matches(s, exact)))
            {
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
            self.client().notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": settings }),
            );
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
        let text =
            std::fs::read_to_string(Url::parse(uri).unwrap().to_file_path().unwrap()).unwrap();
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
        self.client().notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        );
    }

    /// Latest `publishDiagnostics` for `uri` after a fresh build.
    pub fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        let c = self.client();
        c.notifications.retain(|m| {
            !(m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        });
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
        msg["params"]["diagnostics"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Wait for the server to go quiet (no message for `QUIET`), then return
    /// the `publishDiagnostics` params received for Java documents since the
    /// last call.  jdt.ls publishes asynchronously (debounced validation
    /// jobs), so tests compare what was published after each step, like the
    /// upstream `clientRequests.get("publishDiagnostics")`.  Project-level
    /// reports (project folder and build-file URIs, from the workspace
    /// diagnostics handler) are not document reports and are left out.
    pub fn published_diagnostics(&mut self) -> Vec<Value> {
        self.published_diagnostics_min(0)
    }

    /// [`Self::published_diagnostics`], first waiting (up to a minute) until
    /// at least `min` document reports have arrived.
    pub fn published_diagnostics_min(&mut self, min: usize) -> Vec<Value> {
        let is_doc_report = |m: &Value| {
            m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"]
                    .as_str()
                    .is_some_and(|u| u.ends_with(".java"))
        };
        let c = self.client();
        let deadline = Instant::now() + Duration::from_secs(60);
        while c.notifications.iter().filter(|m| is_doc_report(m)).count() < min
            && Instant::now() < deadline
        {
            c.settle(Duration::from_millis(200), Duration::from_millis(200));
        }
        c.settle(Duration::from_millis(3000), Duration::from_secs(60));
        let (taken, kept): (Vec<Value>, Vec<Value>) = std::mem::take(&mut c.notifications)
            .into_iter()
            .partition(|m| m["method"] == "textDocument/publishDiagnostics");
        c.notifications = kept;
        taken
            .into_iter()
            .filter(|m| is_doc_report(m))
            .map(|m| m["params"].clone())
            .collect()
    }

    pub fn save(&mut self, uri: &str, text: Option<&str>) {
        let mut params = json!({ "textDocument": { "uri": uri } });
        if let Some(t) = text {
            params["text"] = json!(t);
        }
        self.client().notify("textDocument/didSave", params);
    }

    /// `didChange` with a ranged (incremental) content change.
    pub fn change_range(&mut self, uri: &str, range: Value, text: &str) {
        let v = self.versions.entry(uri.to_owned()).or_insert(1);
        *v += 1;
        let v = *v;
        self.client().notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": v }, "contentChanges": [{ "range": range, "text": text }] }),
        );
    }

    /// `didOpen` with an explicit version.
    pub fn open_version(&mut self, uri: &str, text: &str, version: i32) {
        self.versions.insert(uri.to_owned(), version);
        self.client().notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "java", "version": version, "text": text } }),
        );
    }

    /// `didChange` (full content) with an explicit version.
    pub fn change_version(&mut self, uri: &str, text: &str, version: i32) {
        self.versions.insert(uri.to_owned(), version);
        self.client().notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }),
        );
    }

    /// Tell the server about a file change on disk (`workspace/didChangeWatchedFiles`,
    /// 1 = created, 2 = changed, 3 = deleted).
    pub fn notify_file_changed(&mut self, path: &Path, typ: u32) {
        self.client();
        self.file_changed(path, typ);
    }

    /// A directory outside every workspace root (files there belong to the
    /// default project).
    pub fn external_dir(&self) -> PathBuf {
        let d = self.dir.parent().unwrap().join("external");
        std::fs::create_dir_all(&d).unwrap();
        d
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
    for e in std::fs::read_dir(from)
        .unwrap_or_else(|_| panic!("fixture {} missing", from.display()))
        .flatten()
    {
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
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
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
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
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
    if dir.join("build.gradle").exists()
        || dir.join("settings.gradle").exists()
        || dir.join("build.gradle.kts").exists()
    {
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
        let Some(&start) = line_starts.get(line) else {
            return text.len();
        };
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
        .map(|e| {
            (
                to_offset(&e["range"]["start"]),
                to_offset(&e["range"]["end"]),
                e["newText"].as_str().unwrap_or("").to_owned(),
            )
        })
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

/// jdt.ls `TestOptions.getDefaultOptions()`: `JavaCore.getDefaultOptions()`
/// (JDT 3.46 defaults, generated from the oracle's jars) with the
/// `TestOptions` overrides and 1.8 compliance.
pub fn test_default_options() -> BTreeMap<String, String> {
    let pairs: &[(&str, &str)] = &[
        ("org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "disabled"),
        ("org.eclipse.jdt.core.compiler.annotation.missingNonNullByDefaultAnnotation", "ignore"),
        ("org.eclipse.jdt.core.compiler.annotation.nonnull", "org.eclipse.jdt.annotation.NonNull"),
        ("org.eclipse.jdt.core.compiler.annotation.nonnull.secondary", ""),
        ("org.eclipse.jdt.core.compiler.annotation.nonnullbydefault", "org.eclipse.jdt.annotation.NonNullByDefault"),
        ("org.eclipse.jdt.core.compiler.annotation.nonnullbydefault.secondary", ""),
        ("org.eclipse.jdt.core.compiler.annotation.notowning", "org.eclipse.jdt.annotation.NotOwning"),
        ("org.eclipse.jdt.core.compiler.annotation.nullable", "org.eclipse.jdt.annotation.Nullable"),
        ("org.eclipse.jdt.core.compiler.annotation.nullable.secondary", ""),
        ("org.eclipse.jdt.core.compiler.annotation.nullanalysis", "disabled"),
        ("org.eclipse.jdt.core.compiler.annotation.owning", "org.eclipse.jdt.annotation.Owning"),
        ("org.eclipse.jdt.core.compiler.annotation.resourceanalysis", "disabled"),
        ("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode", "enabled"),
        ("org.eclipse.jdt.core.compiler.codegen.lambda.genericSignature", "do not generate"),
        ("org.eclipse.jdt.core.compiler.codegen.methodParameters", "do not generate"),
        ("org.eclipse.jdt.core.compiler.codegen.shareCommonFinallyBlocks", "disabled"),
        ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "1.8"),
        ("org.eclipse.jdt.core.compiler.codegen.unusedLocal", "preserve"),
        ("org.eclipse.jdt.core.compiler.codegen.useStringConcatFactory", "enabled"),
        ("org.eclipse.jdt.core.compiler.codegen.validateOperandStack", "enabled"),
        ("org.eclipse.jdt.core.compiler.compliance", "1.8"),
        ("org.eclipse.jdt.core.compiler.debug.lineNumber", "generate"),
        ("org.eclipse.jdt.core.compiler.debug.localVariable", "generate"),
        ("org.eclipse.jdt.core.compiler.debug.sourceFile", "generate"),
        ("org.eclipse.jdt.core.compiler.doc.comment.support", "enabled"),
        ("org.eclipse.jdt.core.compiler.emulateJavacBug8031744", "enabled"),
        ("org.eclipse.jdt.core.compiler.generateClassFiles", "enabled"),
        ("org.eclipse.jdt.core.compiler.ignoreUnnamedModuleForSplitPackage", "disabled"),
        ("org.eclipse.jdt.core.compiler.maxProblemPerUnit", "100"),
        ("org.eclipse.jdt.core.compiler.problem.APILeak", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.annotatedTypeArgumentToUnannotated", "info"),
        ("org.eclipse.jdt.core.compiler.problem.annotationSuperInterface", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.assertIdentifier", "error"),
        ("org.eclipse.jdt.core.compiler.problem.autoboxing", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.comparingIdentical", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.deadCode", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.deadCodeInTrivialIfStatement", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.deprecation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.deprecationInDeprecatedCode", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.deprecationWhenOverridingDeprecatedMethod", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.discouragedReference", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.emptyStatement", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.enumIdentifier", "error"),
        ("org.eclipse.jdt.core.compiler.problem.explicitlyClosedAutoCloseable", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.fallthroughCase", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.fatalOptionalError", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.fieldHiding", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.finalParameterBound", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.finallyBlockNotCompletingNormally", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.forbiddenReference", "error"),
        ("org.eclipse.jdt.core.compiler.problem.hiddenCatchBlock", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.includeNullInfoFromAsserts", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.incompatibleNonInheritedInterfaceMethod", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.incompatibleOwningContract", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.incompleteEnumSwitch", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.indirectStaticAccess", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.insufficientResourceAnalysis", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.invalidJavadoc", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.invalidJavadocTags", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.invalidJavadocTagsDeprecatedRef", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.invalidJavadocTagsNotVisibleRef", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.invalidJavadocTagsVisibility", "public"),
        ("org.eclipse.jdt.core.compiler.problem.localVariableHiding", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.memberOfDeprecatedTypeNotDeprecated", "info"),
        ("org.eclipse.jdt.core.compiler.problem.methodWithConstructorName", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.missingDefaultCase", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingDeprecatedAnnotation", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingEnumCaseDespiteDefault", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingHashCodeMethod", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocComments", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocCommentsOverriding", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocCommentsVisibility", "public"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocTagDescription", "return_tag"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocTags", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocTagsMethodTypeParameters", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocTagsOverriding", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingJavadocTagsVisibility", "public"),
        ("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotation", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingSerialVersion", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.missingSynchronizedOnInheritedMethod", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.noEffectAssignment", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.noImplicitStringConversion", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.nonExternalizedStringLiteral", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.nonnullParameterAnnotationDropped", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.nonnullTypeVariableFromLegacyInvocation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.nullAnnotationInferenceConflict", "error"),
        ("org.eclipse.jdt.core.compiler.problem.nullReference", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.nullSpecViolation", "error"),
        ("org.eclipse.jdt.core.compiler.problem.nullUncheckedConversion", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.overridingMethodWithoutSuperInvocation", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.overridingPackageDefaultMethod", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.parameterAssignment", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.pessimisticNullAnalysisForFreeTypeVariables", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.possibleAccidentalBooleanAssignment", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.potentialNullReference", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.potentiallyUnclosedCloseable", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.rawTypeReference", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.redundantNullAnnotation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.redundantNullCheck", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.redundantSpecificationOfTypeArguments", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.redundantSuperinterface", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.reportMethodCanBePotentiallyStatic", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.reportMethodCanBeStatic", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.specialParameterHidingField", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.staticAccessReceiver", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.suppressOptionalErrors", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.suppressWarnings", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.suppressWarningsNotFullyAnalysed", "info"),
        ("org.eclipse.jdt.core.compiler.problem.syntacticNullAnalysisForFields", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.syntheticAccessEmulation", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.tasks", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.terminalDeprecation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.typeParameterHiding", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unavoidableGenericTypeProblems", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.uncheckedTypeOperation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unclosedCloseable", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.undocumentedEmptyBlock", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unhandledWarningToken", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.uninternedIdentityComparison", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.unlikelyCollectionMethodArgumentType", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unlikelyCollectionMethodArgumentTypeStrict", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.unlikelyEqualsArgumentType", "info"),
        ("org.eclipse.jdt.core.compiler.problem.unnecessaryElse", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unnecessaryTypeCheck", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unqualifiedFieldAccess", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unstableAutoModuleName", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionExemptExceptionAndThrowable", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionIncludeDocCommentReference", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionWhenOverriding", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedExceptionParameter", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedImport", "error"),
        ("org.eclipse.jdt.core.compiler.problem.unusedLabel", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unusedLambdaParameter", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unusedLocal", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedParameter", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedParameterIncludeDocCommentReference", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedParameterWhenImplementingAbstract", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedParameterWhenOverridingConcrete", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedTypeArgumentsForMethodInvocation", "warning"),
        ("org.eclipse.jdt.core.compiler.problem.unusedTypeParameter", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.unusedWarningToken", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.varargsArgumentNeedCast", "warning"),
        ("org.eclipse.jdt.core.compiler.processAnnotations", "disabled"),
        ("org.eclipse.jdt.core.compiler.release", "disabled"),
        ("org.eclipse.jdt.core.compiler.source", "1.8"),
        ("org.eclipse.jdt.core.compiler.storeAnnotations", "disabled"),
        ("org.eclipse.jdt.core.compiler.taskCaseSensitive", "enabled"),
        ("org.eclipse.jdt.core.compiler.taskPriorities", "NORMAL,HIGH,NORMAL"),
        ("org.eclipse.jdt.core.compiler.taskTags", "TODO,FIXME,XXX"),
        ("org.eclipse.jdt.core.formatter.align_arrows_in_switch_on_columns", "false"),
        ("org.eclipse.jdt.core.formatter.align_assignment_statements_on_columns", "false"),
        ("org.eclipse.jdt.core.formatter.align_fields_grouping_blank_lines", "2147483647"),
        ("org.eclipse.jdt.core.formatter.align_selector_in_method_invocation_on_expression_first_line", "true"),
        ("org.eclipse.jdt.core.formatter.align_type_members_on_columns", "false"),
        ("org.eclipse.jdt.core.formatter.align_variable_declarations_on_columns", "false"),
        ("org.eclipse.jdt.core.formatter.align_with_spaces", "false"),
        ("org.eclipse.jdt.core.formatter.alignment_for_additive_operator", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_enum_constant", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_field", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_local_variable", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_method", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_package", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_parameter", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_annotations_on_type", "49"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_allocation_expression", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_annotation", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_enum_constant", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_explicit_constructor_call", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_method_invocation", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_arguments_in_qualified_allocation_expression", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_assertion_message", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_assignment", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_bitwise_operator", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_compact_if", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_compact_loops", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_conditional_expression", "80"),
        ("org.eclipse.jdt.core.formatter.alignment_for_conditional_expression_chain", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_enum_constants", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_array_initializer", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_for_loop_header", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_switch_case_with_arrow", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_switch_case_with_colon", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_logical_operator", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_method_declaration", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_module_statements", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_multiple_fields", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_multiplicative_operator", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_parameterized_type_references", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_parameters_in_constructor_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_parameters_in_method_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_permitted_types_in_type_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_record_components", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_relational_operator", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_resources_in_try", "80"),
        ("org.eclipse.jdt.core.formatter.alignment_for_selector_in_method_invocation", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_shift_operator", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_string_concatenation", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_superclass_in_type_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_enum_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_record_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_type_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_switch_case_with_arrow", "20"),
        ("org.eclipse.jdt.core.formatter.alignment_for_throws_clause_in_constructor_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_throws_clause_in_method_declaration", "16"),
        ("org.eclipse.jdt.core.formatter.alignment_for_type_annotations", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_type_arguments", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_type_parameters", "0"),
        ("org.eclipse.jdt.core.formatter.alignment_for_union_type_in_multicatch", "16"),
        ("org.eclipse.jdt.core.formatter.blank_lines_after_imports", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_after_last_class_body_declaration", "0"),
        ("org.eclipse.jdt.core.formatter.blank_lines_after_package", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_abstract_method", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_field", "0"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_first_class_body_declaration", "0"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_imports", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_member_type", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_method", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_new_chunk", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_before_package", "0"),
        ("org.eclipse.jdt.core.formatter.blank_lines_between_import_groups", "1"),
        ("org.eclipse.jdt.core.formatter.blank_lines_between_statement_group_in_switch", "0"),
        ("org.eclipse.jdt.core.formatter.blank_lines_between_type_declarations", "1"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_annotation_type_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_anonymous_type_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_array_initializer", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_block", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_block_in_case", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_block_in_case_after_arrow", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_constructor_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_enum_constant", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_enum_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_lambda_body", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_method_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_record_constructor", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_record_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_switch", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.brace_position_for_type_declaration", "end_of_line"),
        ("org.eclipse.jdt.core.formatter.comment.align_tags_descriptions_grouped", "true"),
        ("org.eclipse.jdt.core.formatter.comment.align_tags_names_descriptions", "false"),
        ("org.eclipse.jdt.core.formatter.comment.clear_blank_lines_in_block_comment", "false"),
        ("org.eclipse.jdt.core.formatter.comment.clear_blank_lines_in_javadoc_comment", "false"),
        ("org.eclipse.jdt.core.formatter.comment.count_line_length_from_starting_position", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_block_comments", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_header", "false"),
        ("org.eclipse.jdt.core.formatter.comment.format_html", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_javadoc_comments", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_line_comments", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_markdown_comments", "true"),
        ("org.eclipse.jdt.core.formatter.comment.format_source_code", "true"),
        ("org.eclipse.jdt.core.formatter.comment.indent_parameter_description", "false"),
        ("org.eclipse.jdt.core.formatter.comment.indent_root_tags", "false"),
        ("org.eclipse.jdt.core.formatter.comment.indent_tag_description", "false"),
        ("org.eclipse.jdt.core.formatter.comment.insert_new_line_before_root_tags", "insert"),
        ("org.eclipse.jdt.core.formatter.comment.insert_new_line_between_different_tags", "do not insert"),
        ("org.eclipse.jdt.core.formatter.comment.insert_new_line_for_parameter", "do not insert"),
        ("org.eclipse.jdt.core.formatter.comment.javadoc_do_not_separate_block_tags", "false"),
        ("org.eclipse.jdt.core.formatter.comment.line_length", "80"),
        ("org.eclipse.jdt.core.formatter.comment.new_lines_at_block_boundaries", "true"),
        ("org.eclipse.jdt.core.formatter.comment.new_lines_at_javadoc_boundaries", "true"),
        ("org.eclipse.jdt.core.formatter.comment.preserve_white_space_between_code_and_line_comments", "false"),
        ("org.eclipse.jdt.core.formatter.compact_else_if", "true"),
        ("org.eclipse.jdt.core.formatter.continuation_indentation", "2"),
        ("org.eclipse.jdt.core.formatter.continuation_indentation_for_array_initializer", "2"),
        ("org.eclipse.jdt.core.formatter.disabling_tag", "@formatter:off"),
        ("org.eclipse.jdt.core.formatter.enabling_tag", "@formatter:on"),
        ("org.eclipse.jdt.core.formatter.format_guardian_clause_on_one_line", "false"),
        ("org.eclipse.jdt.core.formatter.format_line_comment_starting_on_first_column", "false"),
        ("org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_annotation_declaration_header", "true"),
        ("org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_enum_constant_header", "true"),
        ("org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_enum_declaration_header", "true"),
        ("org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_record_header", "true"),
        ("org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_type_header", "true"),
        ("org.eclipse.jdt.core.formatter.indent_breaks_compare_to_cases", "true"),
        ("org.eclipse.jdt.core.formatter.indent_empty_lines", "false"),
        ("org.eclipse.jdt.core.formatter.indent_statements_compare_to_block", "true"),
        ("org.eclipse.jdt.core.formatter.indent_statements_compare_to_body", "true"),
        ("org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_cases", "true"),
        ("org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_switch", "false"),
        ("org.eclipse.jdt.core.formatter.indentation.size", "4"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_enum_constant", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_field", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_local_variable", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_method", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_package", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_parameter", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_type", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_label", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_opening_brace_in_array_initializer", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_type_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_at_end_of_file_if_missing", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_before_catch_in_try_statement", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_before_closing_brace_in_array_initializer", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_before_else_in_if_statement", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_before_finally_in_try_statement", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_before_while_in_do_statement", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_additive_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_and_in_type_parameter", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_arrow_in_switch_case", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_arrow_in_switch_default", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_assignment_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_at_in_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_at_in_annotation_type_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_bitwise_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_closing_angle_bracket_in_type_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_closing_angle_bracket_in_type_parameters", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_closing_brace_in_block", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_closing_paren_in_cast", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_colon_in_assert", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_colon_in_case", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_colon_in_conditional", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_colon_in_for", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_colon_in_labeled_statement", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_allocation_expression", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_annotation", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_array_initializer", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_declaration_parameters", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_declaration_throws", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_enum_constant_arguments", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_enum_declarations", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_explicitconstructorcall_arguments", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_for_increments", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_for_inits", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_declaration_parameters", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_declaration_throws", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_invocation_arguments", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_multiple_field_declarations", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_multiple_local_declarations", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_parameterized_type_reference", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_permitted_types", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_record_components", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_superinterfaces", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_switch_case_expressions", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_type_arguments", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_comma_in_type_parameters", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_ellipsis", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_lambda_arrow", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_logical_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_multiplicative_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_not_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_angle_bracket_in_parameterized_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_angle_bracket_in_type_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_angle_bracket_in_type_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_brace_in_array_initializer", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_bracket_in_array_allocation_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_bracket_in_array_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_cast", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_catch", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_constructor_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_enum_constant", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_for", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_if", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_method_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_method_invocation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_parenthesized_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_record_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_switch", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_synchronized", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_try", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_while", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_postfix_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_prefix_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_question_in_conditional", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_question_in_wildcard", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_relational_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_semicolon_in_for", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_semicolon_in_try_resources", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_shift_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_string_concatenation", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_after_unary_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_additive_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_and_in_type_parameter", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_arrow_in_switch_case", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_arrow_in_switch_default", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_assignment_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_at_in_annotation_type_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_bitwise_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_angle_bracket_in_parameterized_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_angle_bracket_in_type_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_angle_bracket_in_type_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_brace_in_array_initializer", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_bracket_in_array_allocation_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_bracket_in_array_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_cast", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_catch", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_constructor_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_enum_constant", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_for", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_if", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_method_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_method_invocation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_parenthesized_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_record_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_switch", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_synchronized", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_try", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_while", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_assert", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_case", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_conditional", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_default", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_for", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_colon_in_labeled_statement", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_allocation_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_array_initializer", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_declaration_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_declaration_throws", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_enum_constant_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_enum_declarations", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_explicitconstructorcall_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_for_increments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_for_inits", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_throws", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_invocation_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_multiple_field_declarations", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_multiple_local_declarations", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_parameterized_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_permitted_types", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_record_components", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_superinterfaces", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_switch_case_expressions", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_type_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_comma_in_type_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_ellipsis", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_lambda_arrow", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_logical_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_multiplicative_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_angle_bracket_in_parameterized_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_angle_bracket_in_type_arguments", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_angle_bracket_in_type_parameters", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_annotation_type_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_anonymous_type_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_array_initializer", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_block", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_constructor_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_enum_constant", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_enum_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_method_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_record_constructor", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_record_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_switch", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_type_declaration", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_bracket_in_array_allocation_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_bracket_in_array_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_bracket_in_array_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_annotation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_annotation_type_member_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_catch", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_constructor_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_enum_constant", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_for", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_if", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_invocation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_parenthesized_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_record_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_switch", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_synchronized", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_try", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_while", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_parenthesized_expression_in_return", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_parenthesized_expression_in_throw", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_postfix_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_prefix_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_question_in_conditional", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_question_in_wildcard", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_relational_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_semicolon", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_semicolon_in_for", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_semicolon_in_try_resources", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_shift_operator", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_string_concatenation", "insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_before_unary_operator", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_brackets_in_array_type_reference", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_braces_in_array_initializer", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_brackets_in_array_allocation_expression", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_annotation_type_member_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_constructor_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_enum_constant", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_method_declaration", "do not insert"),
        ("org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_method_invocation", "do not insert"),
        ("org.eclipse.jdt.core.formatter.join_line_comments", "false"),
        ("org.eclipse.jdt.core.formatter.join_lines_in_comments", "true"),
        ("org.eclipse.jdt.core.formatter.join_wrapped_lines", "true"),
        ("org.eclipse.jdt.core.formatter.keep_annotation_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_anonymous_type_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_code_block_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_else_statement_on_same_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_empty_array_initializer_on_one_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_enum_constant_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_enum_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_if_then_body_block_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_imple_if_on_one_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_lambda_body_block_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_loop_body_block_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_method_body_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_record_constructor_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_record_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_simple_do_while_body_on_same_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_simple_for_body_on_same_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_simple_getter_setter_on_one_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_simple_while_body_on_same_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_switch_body_block_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_switch_case_with_arrow_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.keep_then_statement_on_same_line", "false"),
        ("org.eclipse.jdt.core.formatter.keep_type_declaration_on_one_line", "one_line_never"),
        ("org.eclipse.jdt.core.formatter.lineSplit", "120"),
        ("org.eclipse.jdt.core.formatter.never_indent_block_comments_on_first_column", "false"),
        ("org.eclipse.jdt.core.formatter.never_indent_line_comments_on_first_column", "false"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_after_code_block", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_at_beginning_of_code_block", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_at_beginning_of_method_body", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_at_end_of_code_block", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_at_end_of_method_body", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_blank_lines_before_code_block", "0"),
        ("org.eclipse.jdt.core.formatter.number_of_empty_lines_to_preserve", "1"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_annotation", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_catch_clause", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_enum_constant_declaration", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_for_statment", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_if_while_statement", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_lambda_declaration", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_method_delcaration", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_method_invocation", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_record_declaration", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_switch_statement", "common_lines"),
        ("org.eclipse.jdt.core.formatter.parentheses_positions_in_try_clause", "common_lines"),
        ("org.eclipse.jdt.core.formatter.put_empty_statement_on_new_line", "true"),
        ("org.eclipse.jdt.core.formatter.tabulation.char", "space"),
        ("org.eclipse.jdt.core.formatter.tabulation.size", "4"),
        ("org.eclipse.jdt.core.formatter.text_block_indentation", "0"),
        ("org.eclipse.jdt.core.formatter.use_on_off_tags", "true"),
        ("org.eclipse.jdt.core.formatter.use_tabs_only_for_leading_indentations", "false"),
        ("org.eclipse.jdt.core.formatter.wrap_before_additive_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_assertion_message_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_assignment_operator", "false"),
        ("org.eclipse.jdt.core.formatter.wrap_before_bitwise_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_conditional_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_logical_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_multiplicative_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_or_operator_multicatch", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_relational_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_shift_operator", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_string_concatenation", "true"),
        ("org.eclipse.jdt.core.formatter.wrap_before_switch_case_arrow_operator", "false"),
        ("org.eclipse.jdt.core.formatter.wrap_outer_expressions_when_nested", "true"),
    ];
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
