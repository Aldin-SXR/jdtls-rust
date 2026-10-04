//! Port of jdt.ls `InitHandler.registerCapabilities` (the `initialize`
//! result), `JDTLanguageServer.registerCapabilities` /
//! `syncCapabilitiesToSettings` (the `client/registerCapability` requests
//! sent after `initialized` and on configuration changes) and
//! `StandardProjectsManager.registerWatchers`.
//!
//! The capabilities are computed from the raw `initialize` params, the way
//! `ClientPreferences` reads lsp4j's `ClientCapabilities`, and substituted
//! for tower-lsp's typed result by [`InitializeResultRewrite`] (lsp-types
//! can't express every jdt.ls field, e.g. `typeHierarchyProvider`).

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use serde_json::{json, Value};
use tower_lsp::jsonrpc::{Request, Response};
use tower_lsp::lsp_types::Registration;
use tower_service::Service;

/// The `workspace/executeCommand` commands jdt.ls 1.58 contributes through
/// `JDTDelegateCommandHandler` (all non-static), in the order jdt.ls lists
/// them (its `HashSet` iteration order).
pub const JDTLS_COMMANDS: &[&str] = &[
    "java.project.import",
    "java.project.changeImportedProjects",
    "java.navigate.openTypeHierarchy",
    "java.project.resolveStackTraceLocation",
    "java.edit.handlePasteEvent",
    "java.edit.stringFormatting",
    "java.project.getSettings",
    "java.project.resolveWorkspaceSymbol",
    "java.project.upgradeGradle",
    "java.project.createModuleInfo",
    "java.vm.getAllInstalls",
    "java.edit.organizeImports",
    "java.project.refreshDiagnostics",
    "java.project.removeFromSourcePath",
    "java.project.listSourcePaths",
    "java.project.updateSettings",
    "java.project.getAll",
    "java.reloadBundles",
    "java.project.isTestFile",
    "java.project.resolveText",
    "java.project.getClasspaths",
    "java.navigate.resolveTypeHierarchy",
    "java.getTroubleshootingInfo",
    "java.edit.smartSemicolonDetection",
    "java.project.updateSourceAttachment",
    "java.project.updateClassPaths",
    "java.decompile",
    "java.protobuf.generateSources",
    "java.project.resolveSourceAttachment",
    "java.project.updateJdk",
    "java.project.addToSourcePath",
    "java.completion.onDidSelect",
];

/// jdt.ls has no static (always registered) delegate commands.
pub const JDTLS_STATIC_COMMANDS: &[&str] = &[];

/// Commands this server handles beyond jdt.ls's (test support and the
/// `lms-monaco` client); accepted but only advertised to `lms-monaco`.
pub const EXTRA_COMMANDS: &[&str] = &["jdtls-rust.classFileUri", "jdtls-rust.refreshDiagnostics", "java.project.rebuild"];

/// `ClientPreferences` over the raw `initialize` params.
#[derive(Clone, Debug, Default)]
pub struct ClientPrefs {
    caps: Value,
    ext: Value,
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

impl ClientPrefs {
    pub fn from_params(params: &Value) -> Self {
        Self {
            caps: params.get("capabilities").cloned().unwrap_or(Value::Null),
            ext: params.pointer("/initializationOptions/extendedClientCapabilities").cloned().unwrap_or(Value::Null),
        }
    }

    fn td(&self, key: &str) -> &Value {
        self.caps.get("textDocument").and_then(|t| t.get(key)).unwrap_or(&Value::Null)
    }

    fn ws(&self, key: &str) -> &Value {
        self.caps.get("workspace").and_then(|t| t.get(key)).unwrap_or(&Value::Null)
    }

    /// `v3supported`: the client sent `textDocument` capabilities.
    pub fn v3(&self) -> bool {
        self.caps.get("textDocument").is_some_and(|t| !t.is_null())
    }

    fn dynamic(cap: &Value) -> bool {
        cap.get("dynamicRegistration").is_some_and(truthy)
    }

    /// `is<Feature>DynamicRegistered` for a `textDocument` capability.
    pub fn td_dynamic(&self, key: &str) -> bool {
        self.v3() && Self::dynamic(self.td(key))
    }

    /// `is<Feature>DynamicRegistered` for a `workspace` capability.
    pub fn ws_dynamic(&self, key: &str) -> bool {
        self.v3() && Self::dynamic(self.ws(key))
    }

    pub fn ext(&self, key: &str) -> bool {
        self.ext.get(key).is_some_and(truthy)
    }

    pub fn is_completion_item_label_details_support(&self) -> bool {
        self.v3() && self.td("completion").pointer("/completionItem/labelDetailsSupport").is_some_and(truthy)
    }

    /// `isSupportedCodeActionKind(kind)`: a value-set entry prefixes `kind`.
    pub fn is_supported_code_action_kind(&self, kind: &str) -> bool {
        self.v3()
            && self
                .td("codeAction")
                .pointer("/codeActionLiteralSupport/codeActionKind/valueSet")
                .and_then(Value::as_array)
                .is_some_and(|set| set.iter().filter_map(Value::as_str).any(|k| kind.starts_with(k)))
    }

    pub fn is_resolve_code_action_supported(&self) -> bool {
        let ca = self.td("codeAction");
        self.v3()
            && ca.get("dataSupport").is_some_and(truthy)
            && ca
                .pointer("/resolveSupport/properties")
                .and_then(Value::as_array)
                .is_some_and(|p| p.iter().any(|v| v == "edit"))
    }

    pub fn is_will_save_registered(&self) -> bool {
        self.v3() && self.td("synchronization").get("willSave").is_some_and(truthy)
    }

    pub fn is_will_save_wait_until_registered(&self) -> bool {
        self.v3() && self.td("synchronization").get("willSaveWaitUntil").is_some_and(truthy)
    }

    pub fn is_workspace_will_rename_files_supported(&self) -> bool {
        self.v3() && self.ws("fileOperations").get("willRename").is_some_and(truthy)
    }

    pub fn is_workspace_apply_edit_supported(&self) -> bool {
        truthy(self.ws("applyEdit"))
    }

    pub fn is_workspace_change_watched_files_dynamic_registered(&self) -> bool {
        self.ws_dynamic("didChangeWatchedFiles")
    }
}

/// `CompletionHandler.getDefaultCompletionOptions`.
fn completion_options(prefs: &ClientPrefs) -> Value {
    let mut o = json!({ "resolveProvider": true, "triggerCharacters": [".", "@", "#", "*", " "] });
    if prefs.is_completion_item_label_details_support() {
        o["completionItem"] = json!({ "labelDetailsSupport": true });
    }
    o
}

/// `CodeActionHandler.createOptions`.
fn code_action_options(prefs: &ClientPrefs) -> Value {
    let kinds = ["quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"];
    let supported: Vec<&str> = kinds.iter().copied().filter(|k| prefs.is_supported_code_action_kind(k)).collect();
    json!({ "codeActionKinds": supported, "resolveProvider": prefs.is_resolve_code_action_supported() })
}

fn on_type_formatting_options() -> Value {
    json!({ "firstTriggerCharacter": ";", "moreTriggerCharacter": ["\n", "}"] })
}

fn signature_help_options() -> Value {
    json!({ "triggerCharacters": ["(", ","] })
}

fn rename_options() -> Value {
    json!({ "prepareProvider": true })
}

/// `InitHandler.registerCapabilities`: the `initialize` result.  `lms_monaco`
/// additionally advertises the providers that client relies on beyond
/// jdt.ls's (document links, linked editing, file-operation notifications).
pub fn initialize_result(params: &Value, lms_monaco: bool) -> Value {
    let prefs = ClientPrefs::from_params(params);
    let mut c = serde_json::Map::new();
    let mut set = |k: &str, v: Value| {
        c.insert(k.to_owned(), v);
    };
    if !prefs.td_dynamic("completion") {
        set("completionProvider", completion_options(&prefs));
    }
    if !prefs.td_dynamic("formatting") {
        set("documentFormattingProvider", json!(true));
    }
    if !prefs.td_dynamic("rangeFormatting") {
        set("documentRangeFormattingProvider", json!(true));
    }
    if !prefs.td_dynamic("onTypeFormatting") {
        set("documentOnTypeFormattingProvider", on_type_formatting_options());
    }
    if !prefs.td_dynamic("codeLens") {
        set("codeLensProvider", json!({ "resolveProvider": true }));
    }
    if !prefs.td_dynamic("signatureHelp") {
        set("signatureHelpProvider", signature_help_options());
    }
    if !prefs.td_dynamic("rename") {
        set("renameProvider", rename_options());
    }
    if !prefs.td_dynamic("codeAction") {
        set("codeActionProvider", code_action_options(&prefs));
    }
    let mut all: Vec<&str> = JDTLS_COMMANDS.to_vec();
    if lms_monaco {
        all.extend_from_slice(EXTRA_COMMANDS);
    }
    if !prefs.ws_dynamic("executeCommand") {
        set("executeCommandProvider", json!({ "commands": all }));
    } else if !JDTLS_STATIC_COMMANDS.is_empty() {
        set("executeCommandProvider", json!({ "commands": JDTLS_STATIC_COMMANDS }));
    }
    if !prefs.ws_dynamic("symbol") {
        set("workspaceSymbolProvider", json!(true));
    }
    if !prefs.ext("clientDocumentSymbolProvider") && !prefs.td_dynamic("documentSymbol") {
        set("documentSymbolProvider", json!(true));
    }
    for (cap, key) in [
        ("definition", "definitionProvider"),
        ("declaration", "declarationProvider"),
        ("typeDefinition", "typeDefinitionProvider"),
    ] {
        if !prefs.td_dynamic(cap) {
            set(key, json!(true));
        }
    }
    if !prefs.ext("clientHoverProvider") && !prefs.td_dynamic("hover") {
        set("hoverProvider", json!(true));
    }
    for (cap, key) in [
        ("references", "referencesProvider"),
        ("documentHighlight", "documentHighlightProvider"),
        ("foldingRange", "foldingRangeProvider"),
        ("implementation", "implementationProvider"),
        ("selectionRange", "selectionRangeProvider"),
        ("inlayHint", "inlayHintProvider"),
        ("typeHierarchy", "typeHierarchyProvider"),
    ] {
        if !prefs.td_dynamic(cap) {
            set(key, json!(true));
        }
    }
    set("callHierarchyProvider", json!(true));

    let mut sync = json!({ "openClose": true, "save": { "includeText": true }, "change": 2 });
    if prefs.is_will_save_registered() {
        sync["willSave"] = json!(true);
    }
    if prefs.is_will_save_wait_until_registered() {
        sync["willSaveWaitUntil"] = json!(true);
    }
    set("textDocumentSync", sync);

    let mut workspace = json!({ "workspaceFolders": { "supported": true, "changeNotifications": true } });
    if prefs.is_workspace_will_rename_files_supported() {
        workspace["fileOperations"] = json!({ "willRename": { "filters": [
            { "pattern": { "glob": "**/*.java", "matches": "file" }, "scheme": "file" },
            { "pattern": { "glob": "**", "matches": "folder" }, "scheme": "file" },
        ] } });
    }
    if lms_monaco {
        let filters = json!({ "filters": [{ "scheme": "file", "pattern": { "glob": "**/*.java", "matches": "file" } }] });
        let ops = workspace.as_object_mut().unwrap().entry("fileOperations").or_insert_with(|| json!({}));
        ops["didCreate"] = filters.clone();
        ops["didRename"] = filters.clone();
        ops["didDelete"] = filters;
    }
    set("workspace", workspace);

    let legend = serde_json::to_value(crate::features::semantic_tokens::legend()).unwrap_or(Value::Null);
    set(
        "semanticTokensProvider",
        json!({
            "full": { "delta": false },
            "range": false,
            "documentSelector": [{ "language": "java", "scheme": "file" }, { "language": "java", "scheme": "jdt" }],
            "legend": legend,
        }),
    );
    if lms_monaco {
        set("documentLinkProvider", json!({ "resolveProvider": false }));
        set("linkedEditingRangeProvider", json!(true));
    }

    json!({
        "capabilities": Value::Object(c),
        "serverInfo": { "name": "jdtls-rust", "version": env!("CARGO_PKG_VERSION") },
    })
}

// ─── Dynamic registrations ──────────────────────────────────────────────────

/// `UUID.randomUUID().toString()`.
pub fn random_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// `Preferences.*_ID`: one random registration id per capability, fixed for
/// the server's lifetime.
fn registration_id(method: &str) -> String {
    static IDS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, id)) = ids.iter().find(|(m, _)| m == method) {
        return id.clone();
    }
    let id = random_uuid();
    ids.push((method.to_owned(), id.clone()));
    id
}

/// `BaseJDTLanguageServer.registeredCapabilities`.
static REGISTERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// A registration change to send to the client.
pub enum RegistrationChange {
    Register(Registration),
    Unregister { id: String, method: String },
}

fn register(out: &mut Vec<RegistrationChange>, method: &str, options: Option<Value>) {
    let id = registration_id(method);
    let mut reg = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    if reg.get_or_insert_with(HashSet::new).insert(id.clone()) {
        out.push(RegistrationChange::Register(Registration { id, method: method.to_owned(), register_options: options }));
    }
}

fn unregister(out: &mut Vec<RegistrationChange>, method: &str) {
    let id = registration_id(method);
    let mut reg = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    if reg.get_or_insert_with(HashSet::new).remove(&id) {
        out.push(RegistrationChange::Unregister { id, method: method.to_owned() });
    }
}

fn toggle(out: &mut Vec<RegistrationChange>, enabled: bool, method: &str, options: Option<Value>) {
    if enabled {
        register(out, method, options);
    } else {
        unregister(out, method);
    }
}

/// `JDTLanguageServer.registerCapabilities` (once, after `initialized`).
pub fn initial_registrations(prefs: &ClientPrefs) -> Vec<RegistrationChange> {
    let mut out = Vec::new();
    if prefs.ws_dynamic("symbol") {
        register(&mut out, "workspace/symbol", None);
    }
    if !prefs.ext("clientDocumentSymbolProvider") && prefs.td_dynamic("documentSymbol") {
        register(&mut out, "textDocument/documentSymbol", None);
    }
    for (cap, method) in [
        ("definition", "textDocument/definition"),
        ("declaration", "textDocument/declaration"),
        ("typeDefinition", "textDocument/typeDefinition"),
    ] {
        if prefs.td_dynamic(cap) {
            register(&mut out, method, None);
        }
    }
    if !prefs.ext("clientHoverProvider") && prefs.td_dynamic("hover") {
        register(&mut out, "textDocument/hover", None);
    }
    for (cap, method) in [
        ("references", "textDocument/references"),
        ("documentHighlight", "textDocument/documentHighlight"),
        ("implementation", "textDocument/implementation"),
        ("inlayHint", "textDocument/inlayHint"),
    ] {
        if prefs.td_dynamic(cap) {
            register(&mut out, method, None);
        }
    }
    out
}

fn pref_bool(key: &str, default: bool) -> bool {
    crate::features::preferences::get_bool(key).unwrap_or(default)
}

/// `JDTLanguageServer.syncCapabilitiesToSettings`: (un)register the
/// capabilities that follow user preferences.
pub fn sync_capabilities_to_settings(prefs: &ClientPrefs, lms_monaco: bool) -> Vec<RegistrationChange> {
    let mut out = Vec::new();
    if prefs.td_dynamic("completion") {
        toggle(&mut out, pref_bool("java.completion.enabled", true), "textDocument/completion", Some(completion_options(prefs)));
    }
    let format = pref_bool("java.format.enabled", true);
    if prefs.td_dynamic("formatting") {
        toggle(&mut out, format, "textDocument/formatting", None);
    }
    if prefs.td_dynamic("rangeFormatting") {
        toggle(&mut out, format, "textDocument/rangeFormatting", None);
    }
    if prefs.td_dynamic("onTypeFormatting") {
        toggle(&mut out, pref_bool("java.format.onType.enabled", false), "textDocument/onTypeFormatting", Some(on_type_formatting_options()));
    }
    if prefs.td_dynamic("codeLens") {
        toggle(&mut out, crate::features::preferences::code_lens_enabled(), "textDocument/codeLens", Some(json!({ "resolveProvider": true })));
    }
    if prefs.td_dynamic("signatureHelp") {
        toggle(&mut out, pref_bool("java.signatureHelp.enabled", false), "textDocument/signatureHelp", Some(signature_help_options()));
    }
    if prefs.td_dynamic("rename") {
        toggle(&mut out, pref_bool("java.rename.enabled", true), "textDocument/rename", Some(rename_options()));
    }
    if prefs.ws_dynamic("executeCommand") {
        let mut commands: Vec<&str> = JDTLS_COMMANDS.iter().copied().filter(|c| !JDTLS_STATIC_COMMANDS.contains(c)).collect();
        if lms_monaco {
            commands.extend_from_slice(EXTRA_COMMANDS);
        }
        toggle(&mut out, pref_bool("java.executeCommand.enabled", true), "workspace/executeCommand", Some(json!({ "commands": commands })));
    }
    if prefs.td_dynamic("codeAction") {
        toggle(&mut out, true, "textDocument/codeAction", Some(code_action_options(prefs)));
    }
    if prefs.td_dynamic("foldingRange") {
        toggle(&mut out, pref_bool("java.foldingRange.enabled", true), "textDocument/foldingRange", None);
    }
    if prefs.td_dynamic("selectionRange") {
        toggle(&mut out, pref_bool("java.selectionRange.enabled", true), "textDocument/selectionRange", None);
    }
    out
}

// ─── File watchers ──────────────────────────────────────────────────────────

/// `StandardProjectsManager.basicWatchers` followed by the Gradle and Maven
/// build supports' watch patterns.
const BASIC_WATCHERS: &[&str] = &[
    "**/*.java",
    "**/.project",
    "**/.classpath",
    "**/.settings/*.prefs",
    "**/src/**",
    "**/*.gradle",
    "**/*.gradle.kts",
    "**/gradle.properties",
    "**/pom.xml",
];

/// `File.toURI()` of `path` (a trailing `/` when it is an existing directory).
fn file_to_uri(path: &Path) -> String {
    crate::project::java_file_uri(path, path.is_dir())
}

/// `ResourceUtils.toGlobPattern(path, false)` for an absolute path: a
/// relative pattern on the parent folder.
fn relative_pattern(path: &Path) -> Value {
    let parent = path.parent().unwrap_or(path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    json!({ "baseUri": file_to_uri(parent), "pattern": name })
}

/// Collapse `.`/`..` segments the way Eclipse `Path` does.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `StandardProjectsManager.getURIs(url)` + `addWatcher`: the files a
/// `java.format.settings.url`/`java.settings.url` value names.
fn url_watch_paths(url: &str, roots: &[PathBuf]) -> Vec<PathBuf> {
    if url.trim().is_empty() {
        return Vec::new();
    }
    let mut url = url.to_owned();
    if let Ok(u) = url::Url::parse(&url) {
        // `file:c:invalid` is an opaque URI: `new File(uri)` rejects it.
        if u.scheme() == "file" && !url[5..].starts_with('/') {
            return Vec::new();
        }
        if u.scheme() == "file" {
            match u.to_file_path() {
                Ok(p) => url = p.to_string_lossy().into_owned(),
                // `file:c:invalid` is opaque: not a hierarchical file URI.
                Err(_) => return Vec::new(),
            }
        } else if u.scheme().len() > 1 {
            return Vec::new();
        }
    }
    if url.starts_with('/') && Path::new(&url).is_file() {
        return vec![normalize(Path::new(&url))];
    }
    roots.iter().map(|r| normalize(&r.join(&url))).collect()
}

/// The watcher patterns (`FileSystemWatcher`s) jdt.ls registers for the
/// workspace: basic and build-file globs, non-standard source folders and
/// libraries, the formatter/settings files, then a delete watcher on every
/// project folder (sorted by project name).
pub fn watchers(ws: &crate::project::Workspace, roots: &[PathBuf]) -> Vec<Value> {
    let mut patterns: Vec<Value> = BASIC_WATCHERS.iter().map(|p| json!(p)).collect();
    let push = |patterns: &mut Vec<Value>, v: Value| {
        if !patterns.contains(&v) {
            patterns.push(v);
        }
    };
    let mut projects: Vec<&crate::project::Project> = ws.projects.iter().collect();
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    let mut sources: Vec<PathBuf> = Vec::new();
    for p in &projects {
        for sf in &p.source_folders {
            let s = sf.path.to_string_lossy().replace('\\', "/");
            if !s.contains("/src/") && !s.ends_with("/src") && sf.path.exists() && !sources.iter().any(|x| sf.path.starts_with(x)) {
                sources.push(sf.path.clone());
            }
        }
    }
    for s in sources {
        let pattern = if s.is_file() { s.to_string_lossy().into_owned() } else { format!("{}/**", s.to_string_lossy().trim_end_matches('/')) };
        push(&mut patterns, json!(pattern));
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for key in ["java.format.settings.url", "java.settings.url"] {
        if let Some(url) = crate::features::preferences::get_string(key) {
            for p in url_watch_paths(&url, roots) {
                if !files.contains(&p) {
                    files.push(p);
                }
            }
        }
    }
    for f in files {
        push(&mut patterns, relative_pattern(&f));
    }
    let mut out: Vec<Value> = patterns.into_iter().map(|g| json!({ "globPattern": g })).collect();
    for p in &projects {
        if p.kind != crate::project::ProjectKind::Invisible && p.root.exists() {
            out.push(json!({ "globPattern": relative_pattern(&p.root), "kind": 4 }));
        }
    }
    out
}

/// The watchers last registered (`StandardProjectsManager.watchers`).
static WATCHERS: Mutex<Option<Vec<Value>>> = Mutex::new(None);

/// `registerWatchers`: the (re-)registration to send when the watchers
/// changed (and the client registers file watchers dynamically).
pub fn watcher_registration(prefs: &ClientPrefs, watchers: Vec<Value>) -> Vec<RegistrationChange> {
    if !prefs.is_workspace_change_watched_files_dynamic_registered() {
        return Vec::new();
    }
    let mut last = WATCHERS.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref() == Some(&watchers) {
        return Vec::new();
    }
    *last = Some(watchers.clone());
    drop(last);
    let mut out = Vec::new();
    unregister(&mut out, "workspace/didChangeWatchedFiles");
    register(&mut out, "workspace/didChangeWatchedFiles", Some(json!({ "watchers": watchers })));
    out
}

// ─── initialize result substitution ─────────────────────────────────────────

/// Service wrapper replacing the `initialize` result with
/// [`initialize_result`] computed from the raw request params.
pub struct InitializeResultRewrite<S> {
    inner: S,
}

impl<S> InitializeResultRewrite<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S> Service<Request> for InitializeResultRewrite<S>
where
    S: Service<Request, Response = Option<Response>>,
    S::Future: Send + 'static,
    S::Error: 'static,
{
    type Response = Option<Response>;
    type Error = S::Error;
    type Future = BoxFuture<Result<Option<Response>, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if req.method() != "initialize" {
            return Box::pin(self.inner.call(req));
        }
        let params = req.params().cloned().unwrap_or(Value::Null);
        let lms_monaco = params.pointer("/clientInfo/name").and_then(Value::as_str) == Some("lms-monaco");
        let fut = self.inner.call(req);
        Box::pin(async move {
            let resp = fut.await?;
            Ok(resp.map(|r| {
                let (id, result) = r.into_parts();
                match result {
                    Ok(_) => Response::from_ok(id, initialize_result(&params, lms_monaco)),
                    Err(e) => Response::from_error(id, e),
                }
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape() {
        let u = random_uuid();
        assert_eq!(36, u.len());
        assert_eq!(&u[14..15], "4");
        assert_ne!(u, random_uuid());
    }

    #[test]
    fn dynamic_execute_command_omits_provider() {
        let params = json!({ "capabilities": { "workspace": { "executeCommand": { "dynamicRegistration": true } }, "textDocument": {} } });
        let r = initialize_result(&params, false);
        assert!(r["capabilities"].get("executeCommandProvider").is_none());
        let params = json!({ "capabilities": { "workspace": { "executeCommand": { "dynamicRegistration": true } } } });
        // Without `textDocument` capabilities (not v3) nothing is dynamic.
        let r = initialize_result(&params, false);
        assert!(r["capabilities"]["executeCommandProvider"]["commands"].is_array());
    }

    #[test]
    fn settings_url_watch_paths() {
        let roots = vec![PathBuf::from("/a/b/ws")];
        assert_eq!(vec![PathBuf::from("/a/formatter/settings.prefs")], url_watch_paths("../../formatter/settings.prefs", &roots));
        assert!(url_watch_paths("file:c:invalid", &roots).is_empty());
    }
}
