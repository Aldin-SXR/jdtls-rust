//! Direct upstream manager fixture. Java is used only for class-file facts;
//! ordering, extension selection, fallback, logging and cancellation use Rust.
//! In oracle mode a test-only fragment calls the actual Eclipse manager.

use crate::common::jdtls::{is_oracle, Workspace};
use crate::content_provider::{
    self, ContentProvider, DecompilerResult, Descriptor, Event, Manager, Monitor, Preferences,
    Source,
};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

#[path = "../../src/classfile/mod.rs"]
mod classfile;
#[path = "../../src/embedded_jar.rs"]
mod embedded_jar;

static ORACLE_WORKSPACE: Mutex<Option<Workspace>> = Mutex::new(None);

// JUnit runs this class in one OSGI runtime, retaining its real source-discovery
// cache. Retain the same runtime here; the manager and fake state are reset by
// each command. Explicitly drop it at process exit to kill the server and remove
// its temporary workspace. The C callback owns no borrowed test data.
extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}
extern "C" fn cleanup_oracle() {
    ORACLE_WORKSPACE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
}
pub enum TestWorkspace {
    Rust(Workspace),
    Oracle(MutexGuard<'static, Option<Workspace>>),
}
impl Deref for TestWorkspace {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        match self {
            Self::Rust(ws) => ws,
            Self::Oracle(ws) => ws.as_ref().unwrap(),
        }
    }
}
impl DerefMut for TestWorkspace {
    fn deref_mut(&mut self) -> &mut Workspace {
        match self {
            Self::Rust(ws) => ws,
            Self::Oracle(ws) => ws.as_mut().unwrap(),
        }
    }
}
pub fn workspace() -> TestWorkspace {
    if !is_oracle() {
        return TestWorkspace::Rust(new_workspace());
    }
    let mut workspace = ORACLE_WORKSPACE.lock().unwrap_or_else(|e| e.into_inner());
    if workspace.is_none() {
        static CLEANUP: OnceLock<()> = OnceLock::new();
        CLEANUP.get_or_init(|| assert_eq!(0, unsafe { atexit(cleanup_oracle) }));
        *workspace = Some(new_workspace());
    }
    TestWorkspace::Oracle(workspace)
}
fn new_workspace() -> Workspace {
    let mut ws = Workspace::new();
    if is_oracle() {
        static PRODUCT: OnceLock<PathBuf> = OnceLock::new();
        ws.oracle_home = Some(
            PRODUCT
                .get_or_init(|| {
                    let output = Command::new("python3")
                        .arg(
                            Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("scripts/prepare-oracle-fixture.py"),
                        )
                        .arg("content-provider")
                        .output()
                        .expect("build oracle test fragment");
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
                })
                .clone(),
        );
        // ContentProviderManagerTest.setupOnce sets this before any test call.
        ws.oracle_java_options.push("-Djdt.ls.debug=true".into());
    }
    ws.import_projects(&["maven/salut"]);
    ws.use_upstream_maven_test_jdk("salut", "1.8");
    ws
}

/// `importProjects` is synchronous upstream; the LSP folder notification queues
/// a job. Wait for its class to resolve before exercising the manager API.
pub fn class_file_uri(ws: &mut Workspace, project: &str, fqn: &str) -> String {
    let started = Instant::now();
    loop {
        if let Some(uri) = ws.try_class_file_uri(project, fqn) {
            return uri;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "type {fqn} not found in project {project}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn operation(api: &str, uri: Option<&str>, fake_kind: &str, fake_value: Option<&str>) -> Value {
    json!({"api":api, "uri":uri, "fakeKind":fake_kind, "fakeValue":fake_value})
}

pub fn run(ws: &mut Workspace, preferred: &[&str], operations: Vec<Value>) -> Vec<Value> {
    if is_oracle() {
        return ws
            .request(
                "workspace/executeCommand",
                json!({
                    "command":"jdtls.test.contentProvider",
                    "arguments":[{"preferred":preferred, "operations":operations}]
                }),
            )
            .as_array()
            .expect("oracle manager results")
            .clone();
    }
    let state = Arc::new(Mutex::new(FakeState::default()));
    let facts = Arc::new(Mutex::new(None));
    let projects = vec![
        ("salut".into(), ws.project_root("salut")),
        ("reference".into(), ws.dir.join("eclipse/reference")),
    ];
    let mut descriptors = Vec::new();
    for (id, priority, pattern, provider) in [
        ("sourceContentProvider", "0", None, "source"),
        (
            "fernflowerContentProvider",
            "2147483647",
            Some(r".+\.class.*"),
            "fernflower",
        ),
    ] {
        let facts = facts.clone();
        let projects = projects.clone();
        descriptors.push(
            Descriptor::new(id, Some(priority), pattern, move || {
                Ok(Some(Box::new(FactProvider {
                    facts: facts.clone(),
                    projects: projects.clone(),
                    provider,
                })))
            })
            .unwrap(),
        );
    }
    // Upstream's placeholder extension refers to a missing class.
    descriptors.push(
        Descriptor::new(
            "placeholderContentProvider",
            None,
            Some(r".+\.class"),
            || Err("org.eclipse.jdt.ls.core.internal.PlaceHolder: ClassNotFoundException".into()),
        )
        .unwrap(),
    );
    for (id, pattern) in [
        ("fakeContentProvider", None),
        ("fakeContentProvider2", None),
        ("thingyContentProvider", Some(r".+\.thingy")),
    ] {
        let state = state.clone();
        descriptors.push(
            Descriptor::new(id, None, pattern, move || {
                Ok(Some(Box::new(FakeProvider(state.clone()))))
            })
            .unwrap(),
        );
    }
    let preferences = Arc::new(Preferences {
        preferred: (!preferred.is_empty()).then(|| preferred.iter().map(|s| (*s).into()).collect()),
    });
    let manager = Manager::new(preferences.clone(), descriptors);
    let monitor = Monitor::default();
    let mut results = Vec::new();
    for operation in operations {
        monitor.set_canceled(false);
        {
            let mut state = state.lock().unwrap();
            state.kind = operation["fakeKind"].as_str().unwrap().to_owned();
            state.value = operation["fakeValue"].as_str().map(str::to_owned);
        }
        let first_event = manager.events().len();
        let uri = operation["uri"].as_str();
        let mut result = match operation["api"].as_str().unwrap() {
            "content" => {
                json!({"content":futures::executor::block_on(manager.get_content(uri, &monitor))})
            }
            "source" => {
                json!({"content":futures::executor::block_on(manager.get_source(uri, &monitor))})
            }
            "result" => futures::executor::block_on(manager.get_source_result(uri, &monitor))
                .map(|r| serde_json::to_value(r).unwrap())
                .unwrap_or(json!({"content":null})),
            api => panic!("unknown manager API: {api}"),
        };
        let mut errors = Vec::new();
        let mut infos = Vec::new();
        for event in manager.events().into_iter().skip(first_event) {
            match event {
                Event::Error(e) => errors.push(e),
                Event::Info(i) => infos.push(i),
            }
        }
        result["errors"] = json!(errors);
        result["infos"] = json!(infos);
        result["canceled"] = json!(monitor.is_canceled());
        result["preferencesMatch"] = json!(state
            .lock()
            .unwrap()
            .preferences
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &preferences)));
        results.push(result);
    }
    results
}

#[derive(Default)]
struct FakeState {
    kind: String,
    value: Option<String>,
    preferences: Option<Arc<Preferences>>,
}
struct FakeProvider(Arc<Mutex<FakeState>>);
impl ContentProvider for FakeProvider {
    fn is_decompiler(&self) -> bool {
        true
    }
    fn set_preferences(&mut self, preferences: Arc<Preferences>) {
        self.0.lock().unwrap().preferences = Some(preferences);
    }
    fn provide<'a>(
        &'a mut self,
        _: Source<'a>,
        monitor: &'a Monitor,
    ) -> BoxFuture<'a, Result<Option<DecompilerResult>, String>> {
        Box::pin(async move {
            let state = self.0.lock().unwrap();
            match state.kind.as_str() {
                "exception" => Err(format!(
                    "FakeContentProvider error: {}",
                    state.value.as_deref().unwrap()
                )),
                "cancel" => {
                    monitor.set_canceled(true);
                    Ok(Some(DecompilerResult::text("Canceled")))
                }
                "text" => Ok(Some(DecompilerResult::text(
                    state.value.as_deref().unwrap(),
                ))),
                _ => Ok(None),
            }
        })
    }
}

struct FactProvider {
    facts: Arc<Mutex<Option<Bridge>>>,
    projects: Vec<(String, PathBuf)>,
    provider: &'static str,
}
impl ContentProvider for FactProvider {
    fn is_decompiler(&self) -> bool {
        true
    }
    fn provide<'a>(
        &'a mut self,
        source: Source<'a>,
        _: &'a Monitor,
    ) -> BoxFuture<'a, Result<Option<DecompilerResult>, String>> {
        Box::pin(async move {
            let Some(class_file) = classfile::ClassFileRef::parse(source.uri()) else {
                return Ok(None);
            };
            let project_root = self
                .projects
                .iter()
                .find(|(p, _)| p == &class_file.project)
                .map(|(_, r)| r.as_path());
            let root =
                classfile::resolve_root_path(&class_file.root_path, project_root, &self.projects);
            assert!(root.exists(), "fixture library missing: {}", root.display());
            let mut attachments = serde_json::Map::new();
            if let Some(stem) = root.file_stem().and_then(|s| s.to_str()) {
                let attached = root.with_file_name(format!("{stem}-sources.jar"));
                if attached.exists() {
                    attachments.insert(root.to_string_lossy().into_owned(), json!(attached));
                }
            }
            let request = json!({"method":"classFileContents", "id":1, "provider":self.provider,
                "classFile":{"root":root, "module":class_file.module,
                    "packageName":class_file.package, "classFileName":class_file.class_file},
                "sourceAttachments":attachments, "dumpOriginalLines":true});
            let mut facts = self.facts.lock().unwrap();
            let response = facts.get_or_insert_with(Bridge::new).request(request);
            if response["available"] != json!(true) {
                return Ok(None);
            }
            let contents = response["contents"]
                .as_str()
                .expect("raw provider content")
                .to_owned();
            Ok(Some(if self.provider == "fernflower" {
                let mappings: Option<Vec<i32>> =
                    serde_json::from_value(response["rawLineMappings"].clone()).unwrap();
                content_provider::fernflower_result(contents, mappings.as_deref())
            } else {
                DecompilerResult::text(contents)
            }))
        })
    }
}

struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}
impl Bridge {
    fn new() -> Self {
        let java = std::env::var_os("JAVA_HOME")
            .map(|h| PathBuf::from(h).join("bin/java"))
            .unwrap_or_else(|| "java".into());
        let mut child = Command::new(java)
            .arg("-jar")
            .arg(embedded_jar::jar_path().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("class-file fact bridge");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }
    fn request(&mut self, request: Value) -> Value {
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut response = String::new();
        self.stdout.read_line(&mut response).unwrap();
        let response: Value = serde_json::from_str(&response).expect("bridge response");
        assert!(
            response.get("error").is_none_or(Value::is_null),
            "{response}"
        );
        response
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
