//! Port of the jdt.ls document life cycle and workspace diagnostics:
//! `BaseDocumentLifeCycleHandler` / `DocumentLifeCycleHandler` (open,
//! change, save and close of working copies, their validation and
//! `publishDiagnostics`), `DiagnosticsHandler` (non-project files),
//! `DiagnosticsState`, `DiagnosticsCommand.refreshDiagnostics` and
//! `WorkspaceDiagnosticsHandler` (problem markers of the saved files, from
//! builds).
//!
//! Two kinds of problems exist, like in jdt.ls:
//! * *markers*: the build of a project's saved files (open buffers ignored).
//!   A file whose markers change is republished, unless it is open: then its
//!   working copy is re-validated instead.
//! * *working-copy problems*: an open document reconciled against the other
//!   open buffers, published after `didOpen`/`didChange` (debounced).
//!
//! Files outside every project belong to the default project (as do virtual
//! documents with no file on disk); they and project files outside source
//! folders only report syntax-like problems unless
//! `java.project.refreshDiagnostics` asks for all of them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Notify;
use tower_lsp::lsp_types::{Diagnostic, Url};
use tower_lsp::Client;

use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::diagnostics as diag_conv;
use crate::analysis::semantic::protocol::BridgeDiagnostic;
use crate::document_store::DocumentStore;
use crate::project::{Project, ProjectKind, Workspace};

// `IProblem` ids and categories.
const SYNTAX: u32 = 0x4000_0000;
const TYPE_RELATED: u32 = 0x0100_0000;
const IMPORT_RELATED: u32 = 0x1000_0000;
const PACKAGE_IS_NOT_EXPECTED_PACKAGE: u32 = 536_871_240;
const PUBLIC_CLASS_MUST_MATCH_FILE_NAME: u32 = 16_777_541;
/// Problems `BaseDiagnosticsHandler.isSyntaxLikeError` never reports in
/// syntax mode.
const NOT_SYNTAX_LIKE: &[u32] = &[
    67_109_264,  // AbstractMethodMustBeImplemented
    67_108_966,  // AmbiguousMethod
    603_979_903, // DanglingReference
    67_109_498,  // MethodMustOverrideOrImplement
    16_777_327,  // MissingReturnType
    134_217_857, // MissingTypeInConstructor
    67_109_135,  // MissingTypeInLambda
    67_108_984,  // MissingTypeInMethod
    134_217_858, // UndefinedConstructor
    33_554_502,  // UndefinedField
    67_108_964,  // UndefinedMethod
    570_425_394, // UndefinedName
    33_554_515,  // UnresolvedVariable
    67_108_979,  // ParameterMismatch
];
/// `BaseDiagnosticsHandler.NON_PROJECT_JAVA_FILE` / `NOT_ON_CLASSPATH`.
const NON_PROJECT_JAVA_FILE: u32 = 0x10;
const NOT_ON_CLASSPATH: u32 = 0x20;

/// `BaseDiagnosticsHandler.isSyntaxLikeError`.
pub fn is_syntax_like_error(id: u32, is_default_project: bool) -> bool {
    if id & SYNTAX != 0 {
        return true;
    }
    if !is_default_project && id == PACKAGE_IS_NOT_EXPECTED_PACKAGE {
        return false;
    }
    if id & TYPE_RELATED != 0 || id & IMPORT_RELATED != 0 {
        return false;
    }
    !NOT_SYNTAX_LIKE.contains(&id)
}

/// Where a document belongs (`JDTUtils.resolveCompilationUnit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// In a source folder of `project`, in package `package`.
    OnClasspath { project: String, package: String },
    /// Inside `project` but outside its source folders.
    NotOnClasspath { project: String },
    /// Outside every project (or a virtual document): the default project.
    Default,
}

/// Dotted package of `file` relative to source folder `root`.
fn folder_package(root: &Path, file: &Path) -> String {
    let Some(dir) = file.parent() else {
        return String::new();
    };
    let Ok(rel) = dir.strip_prefix(root) else {
        return String::new();
    };
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(".")
}

pub fn classify(ws: &Workspace, uri: &Url) -> UnitKind {
    let Some(path) = crate::project::uri_to_path(uri) else {
        return UnitKind::Default;
    };
    let Some(project) = ws.project_for_path(&path) else {
        return UnitKind::Default;
    };
    match project.source_folder_for(&path) {
        Some(sf) => UnitKind::OnClasspath {
            project: project.name.clone(),
            package: folder_package(&sf.path, &path),
        },
        None => UnitKind::NotOnClasspath {
            project: project.name.clone(),
        },
    }
}

/// `JDTUtils.isJavaLikeFileName` (`.java` plus `java.associations`) for
/// `file:` URIs.  Virtual documents (other schemes) are Java documents of
/// the default project whatever their name.
pub fn is_java_like(uri: &Url) -> bool {
    if uri.scheme() != "file" {
        return true;
    }
    let name = uri.path().rsplit('/').next().unwrap_or("");
    if name.ends_with(".java") {
        return true;
    }
    file_associations()
        .iter()
        .any(|ext| name.ends_with(&format!(".{ext}")))
}

/// `Preferences.getFilesAssociations`: the `*.ext` keys of
/// `java.associations` mapped to `java`.
pub fn file_associations() -> Vec<String> {
    let Some(Value::Object(map)) = crate::features::preferences::get("java.associations") else {
        return Vec::new();
    };
    map.iter()
        .filter(|(k, v)| {
            v.as_str() == Some("java")
                && k.starts_with("*.")
                && k.len() > 2
                && !k[2..].contains(['*', '?', '/', '['])
        })
        .map(|(k, _)| k[2..].to_owned())
        .collect()
}

/// The max & init value of the adaptive debounce time of the validation job
/// (`BaseDocumentLifeCycleHandler.DOCUMENT_LIFECYCLE_MAX_DEBOUNCE`, ms).
const DOCUMENT_LIFECYCLE_MAX_DEBOUNCE: i64 = 400;

/// Port of `org.eclipse.jdt.ls.core.internal.MovingAverage`.
pub struct MovingAverage {
    /// The average value.
    pub value: i64,
    n: i64,
}

impl MovingAverage {
    /// `new MovingAverage(initValue)`.
    pub fn new(init_value: i64) -> Self {
        MovingAverage { value: init_value, n: 1 }
    }

    /// `update(value)`: Java `long` arithmetic (division truncates).
    pub fn update(&mut self, value: i64) -> &mut Self {
        self.value += (value - self.value) / self.n;
        self.n += 1;
        self
    }
}

#[derive(Default)]
struct State {
    /// Problem markers per file, from the last build of its project.
    markers: HashMap<Url, Vec<Diagnostic>>,
    /// `toValidate`, in insertion order.
    to_validate: Vec<Url>,
    /// `DiagnosticsState`: per-document error level (`true` = syntax only).
    error_levels: HashMap<Url, bool>,
    /// `DiagnosticsState.globalErrorLevel` (`true` = syntax only).
    global_syntax_only: Option<bool>,
    /// Package a default-project file is linked under (`src/<package>/`).
    linked_packages: HashMap<Url, String>,
}

pub struct Lifecycle {
    client: Client,
    store: Arc<DocumentStore>,
    dispatcher: Arc<Dispatcher>,
    state: Mutex<State>,
    validate: Arc<Notify>,
    /// `movingAverageForValidation`.
    validation_average: Mutex<MovingAverage>,
    build_lock: tokio::sync::Mutex<()>,
    started: std::sync::atomic::AtomicBool,
}

impl Lifecycle {
    pub fn new(
        client: Client,
        store: Arc<DocumentStore>,
        dispatcher: Arc<Dispatcher>,
    ) -> Arc<Self> {
        Arc::new(Self {
            client,
            store,
            dispatcher,
            state: Mutex::new(State::default()),
            validate: Arc::new(Notify::new()),
            validation_average: Mutex::new(MovingAverage::new(DOCUMENT_LIFECYCLE_MAX_DEBOUNCE)),
            build_lock: tokio::sync::Mutex::new(()),
            started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn workspace(&self) -> Workspace {
        self.dispatcher
            .workspace
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Start the debounced validation job (`validationTimer` +
    /// `PublishDiagnosticJob`).
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                this.validate.notified().await;
                // `validationTimer.schedule(getDocumentLifecycleDelay())`: every
                // trigger cancels and reschedules the job.
                loop {
                    let delay = this.document_lifecycle_delay();
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(delay)) => break,
                        _ = this.validate.notified() => {}
                    }
                }
                while !this.dispatcher.is_ecj_ready().await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                let start = std::time::Instant::now();
                this.publish_pending().await;
                let elapsed = start.elapsed().as_millis() as i64;
                this.validation_average.lock().unwrap_or_else(|e| e.into_inner()).update(elapsed);
            }
        });
    }

    /// `getDocumentLifecycleDelay()`.
    fn document_lifecycle_delay(&self) -> u64 {
        let average = self.validation_average.lock().unwrap_or_else(|e| e.into_inner()).value;
        DOCUMENT_LIFECYCLE_MAX_DEBOUNCE.min((1.5 * average as f64).round() as i64).max(0) as u64
    }

    // ── Validation of working copies ────────────────────────────────────────

    /// `triggerValidation(cu)`.
    pub fn trigger_validation(&self, uri: &Url) {
        self.store.set_active_java_uri(uri);
        {
            let mut st = self.state();
            if !st.to_validate.contains(uri) {
                st.to_validate.push(uri.clone());
            }
        }
        self.validate.notify_one();
    }

    /// `publishDiagnostics(monitor)`: validate the queued working copies.
    async fn publish_pending(&self) {
        let copy = {
            let mut st = self.state();
            // Upstream snapshots toValidate before adding the other working
            // copies. They remain queued until the next validation trigger.
            let copy = std::mem::take(&mut st.to_validate);
            if crate::features::preferences::get_bool("java.edit.validateAllOpenBuffersOnChanges")
                .unwrap_or(true)
            {
                for u in self.store.open_uris() {
                    if is_java_like(&u) && !copy.contains(&u) && !st.to_validate.contains(&u) {
                        st.to_validate.push(u);
                    }
                }
            }
            copy
        };
        for uri in copy {
            if self.store.is_open(&uri) {
                self.publish_unit(&uri).await;
            }
        }
    }

    /// Whether only syntax-like problems are reported for `uri`
    /// (`DiagnosticsState.isOnlySyntaxReported`).
    pub fn is_only_syntax_reported(&self, uri: &Url) -> bool {
        let st = self.state();
        st.error_levels
            .get(uri)
            .copied()
            .unwrap_or(st.global_syntax_only.unwrap_or(true))
    }

    /// Reconcile `uri` (its open buffer, or its saved content when closed)
    /// and publish the problems (`DiagnosticsHandler.endReporting`).
    pub async fn publish_unit(&self, uri: &Url) {
        if let Some(diags) = self.reconcile(uri).await {
            if !matches_diagnostic_filter(uri) {
                self.client
                    .publish_diagnostics(uri.clone(), diags, None)
                    .await;
            }
        }
    }

    /// The problems of a working copy (`ICompilationUnit.reconcile` with
    /// a `DiagnosticsHandler` problem requestor); `None` when the document
    /// doesn't resolve to a compilation unit.
    async fn reconcile(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        if !is_java_like(uri) {
            return None;
        }
        let ws = self.workspace();
        let kind = classify(&ws, uri);
        let content = crate::features::source_text(&self.store, uri)?;
        let key = uri.to_string();
        let file_name = uri.path().rsplit('/').next().unwrap_or("").to_owned();
        let file_name = crate::classfile::percent_decode(&file_name);

        let (mut ctx, project_name, expected) = match &kind {
            UnitKind::OnClasspath { project, package } => (
                self.dispatcher.context_for(Some(uri)).await,
                Some(project.clone()),
                Some(package.clone()),
            ),
            UnitKind::NotOnClasspath { project } => (
                self.dispatcher.context_for(Some(uri)).await,
                Some(project.clone()),
                None,
            ),
            UnitKind::Default => {
                let linked = self.linked_package(uri, &content);
                (
                    self.dispatcher.context_for(Some(uri)).await,
                    None,
                    Some(linked),
                )
            }
        };
        ctx.files.insert(key.clone(), content.clone());
        let mut expected_packages = HashMap::new();
        if let Some(p) = expected {
            expected_packages.insert(key.clone(), p);
        }
        let items = match self
            .dispatcher
            .compile_units(ctx, Some(vec![key.clone()]), expected_packages)
            .await
        {
            Ok(items) => items,
            Err(e) => {
                tracing::warn!("reconcile {uri}: {e}");
                return None;
            }
        };

        let non_project_file = !matches!(kind, UnitKind::OnClasspath { .. });
        let is_default = kind == UnitKind::Default;
        let syntax_mode = non_project_file && self.is_only_syntax_reported(uri);
        let mut problems: Vec<BridgeDiagnostic> = Vec::new();
        if non_project_file {
            // `DiagnosticsHandler.createNonProjectProblem`.
            let (message, id) = match (syntax_mode, is_default) {
                (true, true) => (format!("{file_name} is a non-project file, only syntax errors are reported"), NON_PROJECT_JAVA_FILE),
                (true, false) => (
                    format!("{file_name} is not on the classpath of project {}, only syntax errors are reported", project_name.as_deref().unwrap_or("")),
                    NOT_ON_CLASSPATH,
                ),
                (false, true) => (format!("{file_name} is a non-project file, only JDK classes are added to its build path"), NON_PROJECT_JAVA_FILE),
                (false, false) => (
                    format!("{file_name} is not on the classpath of project {}, it will not be compiled to a .class file", project_name.as_deref().unwrap_or("")),
                    NOT_ON_CLASSPATH,
                ),
            };
            problems.push(BridgeDiagnostic {
                uri: key.clone(),
                start_line: 0,
                start_char: 0,
                end_line: 0,
                end_char: 1,
                severity: 2,
                message,
                code: Some(id.to_string()),
                category_id: 0,
                tags: None,
                problem_id: None,
                source_start: None,
                source_end: None,
                source_line: None,
                arguments: None,
            });
        }
        for d in items.into_iter().filter(|d| d.uri == key) {
            let id: u32 = d.code.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
            if !syntax_mode || is_syntax_like_error(id, is_default) {
                problems.push(d);
            }
        }
        let doc = diag_conv::Doc16::new(&content);
        let tag_support = crate::features::client_caps::diagnostic_tags();
        Some(
            problems
                .iter()
                .filter_map(|d| diag_conv::to_lsp(d, Some(&doc), tag_support))
                .map(|(_, d)| d)
                .collect(),
        )
    }

    // ── Default-project package links ───────────────────────────────────────

    /// The package a default-project file is linked under
    /// (`JDTUtils.getFakeCompilationUnit`: the package its saved content
    /// declares, else the open buffer's).
    fn linked_package(&self, uri: &Url, content: &str) -> String {
        if let Some(p) = self.state().linked_packages.get(uri) {
            return p.clone();
        }
        let disk = uri
            .to_file_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok());
        let pkg = crate::project::invisible::declared_package(disk.as_deref().unwrap_or(content));
        self.state()
            .linked_packages
            .insert(uri.clone(), pkg.clone());
        pkg
    }

    /// `checkPackageDeclaration`: when a default-project file declares a
    /// package other than the one it is linked under, relink it unless its
    /// folder already matches the linked package.  Returns whether it was
    /// relinked.
    fn check_package_declaration(&self, uri: &Url, content: &str) -> bool {
        if classify(&self.workspace(), uri) != UnitKind::Default {
            return false;
        }
        let linked = self.linked_package(uri, content);
        let declared = crate::project::invisible::declared_package(content);
        if declared == linked {
            return false;
        }
        let folder = uri
            .to_file_path()
            .ok()
            .and_then(|p| {
                p.parent()
                    .map(|d| d.to_string_lossy().replace(['/', '\\'], "."))
            })
            .unwrap_or_default();
        if folder.ends_with(&linked) {
            return false;
        }
        self.state().linked_packages.insert(uri.clone(), declared);
        true
    }

    // ── Document events ─────────────────────────────────────────────────────

    /// `didOpen` (the document is already in the store).
    pub async fn did_open(&self, uri: &Url, _roots: &[PathBuf]) {
        if !is_java_like(uri) {
            return;
        }
        self.register_new_file(uri);
        if let Some(text) = self.store.get(uri).map(|s| s.content_string()) {
            self.linked_package(uri, &text);
            self.check_package_declaration(uri, &text);
        }
        self.trigger_validation(uri);
    }

    /// A document in a source folder that the workspace doesn't know yet
    /// (created after the import) becomes a workspace file
    /// (`handleOpen` refreshes the new resource).
    fn register_new_file(&self, uri: &Url) {
        if matches!(
            classify(&self.workspace(), uri),
            UnitKind::OnClasspath { .. }
        ) && !self.store.is_workspace_file(uri)
        {
            self.store.add_workspace_file(uri.clone());
        }
    }

    /// `didChange` (already applied to the store).
    pub fn did_change(&self, uri: &Url) {
        if is_java_like(uri) && self.store.is_open(uri) {
            self.trigger_validation(uri);
        }
    }

    /// `didClose` (`handleClosed`); called before the store forgets the
    /// buffer.
    pub async fn did_close(&self, uri: &Url) {
        self.state().to_validate.retain(|u| u != uri);
        if !is_java_like(uri) {
            self.store.close(uri);
            return;
        }
        let kind = classify(&self.workspace(), uri);
        let path = uri.to_file_path().ok();
        let exists = path.as_ref().is_some_and(|p| p.is_file());
        let buffer = self.store.get(uri).map(|s| s.content_string());
        self.store.close(uri);
        if !matches!(kind, UnitKind::OnClasspath { .. }) || !exists {
            // Syntax-mode units and deleted files: clear their problems.
            if !matches_diagnostic_filter(uri) {
                self.client
                    .publish_diagnostics(uri.clone(), Vec::new(), None)
                    .await;
            }
        } else {
            let disk = path.and_then(|p| std::fs::read_to_string(p).ok());
            if buffer.is_some() && disk.is_some() && buffer != disk {
                // Unsaved changes are discarded: report the saved content.
                self.store.invalidate_disk(uri);
                self.publish_unit(uri).await;
            }
        }
        if kind == UnitKind::Default && !exists {
            self.state().linked_packages.remove(uri);
        }
    }

    /// `didSave`: the saved file changed on disk, so its project is rebuilt
    /// (the auto-build) after `handleSaved`; with the `renameFileToType`
    /// clean-up the file is renamed after its public type.
    pub async fn did_save(&self, uri: &Url, apply_edit_supported: bool) {
        if !is_java_like(uri) {
            return;
        }
        let text = self.store.get(uri).map(|s| s.content_string());
        if let Some(text) = &text {
            if self.check_package_declaration(uri, text) {
                self.trigger_validation(uri);
            }
        }
        self.register_new_file(uri);
        self.store.invalidate_disk(uri);
        if let Some(project) = self
            .workspace()
            .project_for_uri(uri)
            .map(|p| p.name.clone())
        {
            self.build(Some(&[project])).await;
        }
        if rename_file_to_type_enabled() {
            self.handle_file_rename_for_type_declaration(uri, apply_edit_supported)
                .await;
        }
    }

    /// `BaseDocumentLifeCycleHandler.handleFileRenameForTypeDeclaration`.
    async fn handle_file_rename_for_type_declaration(&self, uri: &Url, apply_edit_supported: bool) {
        let Some(content) = crate::features::source_text(&self.store, uri) else {
            return;
        };
        let kind = classify(&self.workspace(), uri);
        let mut ctx = self.dispatcher.context_for(Some(uri)).await;
        ctx.files.insert(uri.to_string(), content.clone());
        let mut expected = HashMap::new();
        if let UnitKind::OnClasspath { package, .. } = &kind {
            expected.insert(uri.to_string(), package.clone());
        }
        let Ok(items) = self
            .dispatcher
            .compile_units(ctx, Some(vec![uri.to_string()]), expected)
            .await
        else {
            return;
        };
        let problem = items.iter().find(|d| {
            d.uri == uri.as_str()
                && d.code.as_deref().and_then(|c| c.parse::<u32>().ok())
                    == Some(PUBLIC_CLASS_MUST_MATCH_FILE_NAME)
        });
        let Some(problem) = problem else { return };
        if public_top_level_type_count(&content) != 1 {
            return;
        }
        // "The public type {1} must be defined in its own file"
        let Some(new_name) = problem
            .message
            .strip_prefix("The public type ")
            .and_then(|r| r.split(' ').next())
        else {
            return;
        };
        let old_name = uri.path().rsplit('/').next().unwrap_or("").to_owned();
        let extension = old_name
            .rfind('.')
            .filter(|&i| i > 0)
            .map_or(".java".to_owned(), |i| old_name[i..].to_owned());
        let document_uri = uri.to_string();
        let new_uri = document_uri.replace(&old_name, &format!("{new_name}{extension}"));
        if apply_edit_supported {
            let edit = json!({ "changes": {}, "documentChanges": [{ "kind": "rename", "oldUri": document_uri, "newUri": new_uri }] });
            let client = self.client.clone();
            tokio::spawn(async move {
                if let Ok(edit) = serde_json::from_value(edit) {
                    let _ = client.apply_edit(edit).await;
                }
            });
        }
    }

    // ── Workspace diagnostics (markers) ─────────────────────────────────────

    /// Build `projects` (every project when `None`) from the saved files and
    /// publish the marker changes (`WorkspaceDiagnosticsHandler.visit`).
    /// Returns the error markers of the built projects.
    pub async fn build(&self, projects: Option<&[String]>) -> usize {
        let _guard = self.build_lock.lock().await;
        while !self.dispatcher.is_ecj_ready().await {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let ws = self.workspace();
        // A rebuilt project's dependents are rebuilt too.
        let mut targets: Vec<&Project> = match projects {
            None => ws.projects.iter().collect(),
            Some(names) => ws
                .projects
                .iter()
                .filter(|p| {
                    names.contains(&p.name)
                        || ws
                            .project_closure(p)
                            .iter()
                            .any(|c| names.contains(&c.name))
                })
                .collect(),
        };
        targets.retain(|p| p.kind != ProjectKind::Default);
        let disk = self.store.disk_contents();
        let mut built: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
        let mut built_files: HashSet<Url> = HashSet::new();
        let mut marker_workspace = ws.clone();
        marker_workspace
            .projects
            .retain(|p| targets.iter().any(|t| t.name == p.name));
        for (uri, values) in
            crate::features::project_commands::project_marker_diagnostics(&marker_workspace)
        {
            if let Ok(uri) = Url::parse(&uri) {
                let diagnostics: Vec<Diagnostic> = values
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect();
                built_files.insert(uri.clone());
                built.insert(uri, diagnostics);
            }
        }
        for project in &targets {
            if !project.is_java() || project.has_build_path_errors() {
                continue;
            }
            let scopes: &[bool] = if project.has_test_scope() { &[true, false] } else { &[false] };
            for &main_only in scopes {
                let ctx = self
                    .dispatcher
                    .context_scoped(&ws, Some(&project.name), false, disk.clone(), main_only)
                    .await;
                let mut roots = Vec::new();
                let mut expected = HashMap::new();
                for f in project.java_files() {
                    let Ok(u) = Url::from_file_path(&f) else {
                        continue;
                    };
                    if !ctx.files.contains_key(u.as_str()) {
                        continue;
                    }
                    if project.has_test_scope() && project.is_main_source(&f) != main_only {
                        continue;
                    }
                    if let Some(sf) = project.source_folder_for(&f) {
                        expected.insert(u.to_string(), folder_package(&sf.path, &f));
                    }
                    roots.push(u.to_string());
                    built_files.insert(u);
                }
                if roots.is_empty() {
                    continue;
                }
                let own: HashSet<String> = roots.iter().cloned().collect();
                match self.dispatcher.build_units(ctx, roots, expected).await {
                    Ok((items, generated_sources)) => {
                        if let Some(folder) = project
                            .classpath
                            .iter()
                            .find(|e| e.attribute("m2e-apt") == Some("true") && !e.is_test())
                            .and_then(|e| e.location.as_ref())
                        {
                            for (relative, source) in generated_sources {
                                let relative = std::path::Path::new(&relative);
                                if relative
                                    .components()
                                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                                {
                                    continue;
                                }
                                let path = folder.join(relative);
                                if let Some(parent) = path.parent() {
                                    if let Err(e) = std::fs::create_dir_all(parent)
                                        .and_then(|_| std::fs::write(&path, source))
                                    {
                                        tracing::warn!("generated source {}: {e}", path.display());
                                    }
                                }
                            }
                        }
                        // WorkspaceDiagnosticsHandler: the build's problem and
                        // task markers of each file.
                        let tag_support = crate::features::client_caps::diagnostic_tags();
                        let mut docs: HashMap<String, diag_conv::Doc16> = HashMap::new();
                        for d in items.into_iter().filter(|d| own.contains(&d.uri)) {
                            let (Some(problem), Some(text), Ok(u)) =
                                (diag_conv::RawProblem::from_bridge(&d), disk.get(&d.uri), Url::parse(&d.uri))
                            else {
                                if let Some((u, diag)) = diag_conv::to_lsp(&d, None, tag_support) {
                                    built.entry(u).or_default().push(diag);
                                }
                                continue;
                            };
                            let doc = docs.entry(d.uri.clone()).or_insert_with(|| diag_conv::Doc16::new(text));
                            let marker = crate::features::markers::Marker::from_problem(&problem);
                            built
                                .entry(u)
                                .or_default()
                                .extend(crate::features::markers::to_diagnostics_array(doc, &[Some(&marker)], tag_support));
                        }
                    }
                    Err(e) => tracing::warn!("build of {}: {e}", project.name),
                }
            }
        }
        let target_names: HashSet<&str> = targets.iter().map(|p| p.name.as_str()).collect();
        let mut changed: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
        {
            let mut st = self.state();
            // Files of the rebuilt projects that are gone (or left the source
            // folders) lose their markers.
            let stale: Vec<Url> = st
                .markers
                .keys()
                .filter(|u| !built_files.contains(*u))
                .filter(|u| {
                    ws.project_for_uri(u)
                        .map_or(true, |p| target_names.contains(p.name.as_str()))
                })
                .cloned()
                .collect();
            for u in stale {
                if let Some(old) = st.markers.remove(&u) {
                    if !old.is_empty() {
                        changed.push((u, Vec::new()));
                    }
                }
            }
            for u in &built_files {
                let new = built.remove(u).unwrap_or_default();
                let old = st.markers.get(u).cloned().unwrap_or_default();
                if old != new {
                    changed.push((u.clone(), new.clone()));
                }
                st.markers.insert(u.clone(), new);
            }
        }
        changed.sort_by(|a, b| a.0.cmp(&b.0));
        for (uri, diags) in changed {
            if self.store.is_open(&uri) {
                self.trigger_validation(&uri);
            } else if !matches_diagnostic_filter(&uri) {
                self.client.publish_diagnostics(uri, diags, None).await;
            }
        }
        let st = self.state();
        st.markers
            .iter()
            .filter(|(u, _)| built_files.contains(*u))
            .map(|(_, d)| {
                d.iter()
                    .filter(|x| x.severity == Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR))
                    .count()
            })
            .sum()
    }

    /// `BuildWorkspaceHandler.buildProjects`: build the projects owning
    /// `uris` (project folders or files in them); `CANCELLED` when none.
    pub async fn build_projects(&self, uris: &[String]) -> u32 {
        let ws = self.workspace();
        let mut names: Vec<String> = Vec::new();
        for u in uris {
            let Some(path) = Url::parse(u)
                .ok()
                .and_then(|u| crate::project::uri_to_path(&u))
            else {
                continue;
            };
            let path = crate::project::canonicalize_lenient(&path);
            if let Some(p) = ws
                .projects
                .iter()
                .find(|p| p.root == path)
                .or_else(|| ws.project_for_path(&path))
            {
                if !names.contains(&p.name) {
                    names.push(p.name.clone());
                }
            }
        }
        if names.is_empty() {
            return BUILD_CANCELLED;
        }
        let errors = self.build(Some(&names)).await;
        build_status(errors)
    }

    /// A file changed on disk (`didChangeWatchedFiles`): its project is
    /// rebuilt.
    pub async fn files_changed(&self, uris: &[Url]) {
        let ws = self.workspace();
        let mut projects: Vec<String> = Vec::new();
        for u in uris {
            if let Some(p) = ws.project_for_uri(u) {
                if !projects.contains(&p.name) {
                    projects.push(p.name.clone());
                }
            } else {
                // The file left its project: clear it.
                let had_markers = self.state().markers.remove(u).is_some();
                if had_markers {
                    self.client
                        .publish_diagnostics(u.clone(), Vec::new(), None)
                        .await;
                }
            }
        }
        if !projects.is_empty() {
            // WorkspaceDiagnosticsHandler.visit: the resource delta of the
            // change, then the delta of the build, each report the markers of
            // the projects they contain.
            self.publish_project_markers(&projects).await;
            self.build(Some(&projects)).await;
            self.publish_project_markers(&projects).await;
        }
    }

    /// `WorkspaceDiagnosticsHandler.publishMarkers(project, markers)` for the
    /// projects named `names`: the project's own markers, then those of its
    /// `pom.xml` and Gradle wrapper properties when they exist (reports are
    /// sent even when empty).
    async fn publish_project_markers(&self, names: &[String]) {
        let ws = self.workspace();
        for p in ws.projects.iter().filter(|p| p.kind != ProjectKind::Default && names.contains(&p.name)) {
            let project_uri = crate::project::resource_uri(&p.location);
            let Ok(uri) = Url::parse(&project_uri) else { continue };
            if matches_diagnostic_filter(&uri) {
                continue;
            }
            let mut single = ws.clone();
            single.projects.retain(|q| q.name == p.name);
            let mut reports: HashMap<String, Vec<Value>> =
                crate::features::project_commands::project_marker_diagnostics(&single).into_iter().collect();
            let mut publish = |u: Url, values: Vec<Value>| {
                let diags: Vec<Diagnostic> = values.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
                (u, diags)
            };
            let mut out = vec![publish(uri, reports.remove(&project_uri).unwrap_or_default())];
            for build_file in ["pom.xml", "gradle/wrapper/gradle-wrapper.properties"] {
                let path = p.location.join(build_file);
                if path.is_file() {
                    let file_uri = crate::project::resource_uri(&path);
                    if let Ok(u) = Url::parse(&file_uri) {
                        out.push(publish(u, reports.remove(&file_uri).unwrap_or_default()));
                    }
                }
            }
            for (u, diags) in out {
                self.client.publish_diagnostics(u, diags, None).await;
            }
        }
    }

    /// The markers of a closed file (what a build reported for it).
    pub fn markers(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        self.state().markers.get(uri).cloned()
    }

    // ── java.project.refreshDiagnostics ─────────────────────────────────────

    /// `DiagnosticsCommand.refreshDiagnostics(uri, scope, syntaxOnly)`.
    pub async fn refresh_diagnostics(
        &self,
        uri: Option<&str>,
        scope: Option<&str>,
        syntax_only: bool,
    ) {
        let target = uri.and_then(|u| Url::parse(u).ok());
        let refresh_all = match scope {
            Some("thisFile") => {
                if let Some(t) = &target {
                    self.state().error_levels.insert(t.clone(), syntax_only);
                }
                false
            }
            Some("anyNonProjectFile") => {
                let mut st = self.state();
                st.global_syntax_only = Some(syntax_only);
                st.error_levels.clear();
                true
            }
            _ => false,
        };
        if refresh_all {
            let ws = self.workspace();
            for u in self.store.open_uris() {
                if is_java_like(&u) && !matches!(classify(&ws, &u), UnitKind::OnClasspath { .. }) {
                    self.store.set_active_java_uri(&u);
                    self.publish_unit(&u).await;
                }
            }
        } else if let Some(t) = target {
            self.store.set_active_java_uri(&t);
            self.publish_unit(&t).await;
        }
    }
}

/// `BuildWorkspaceStatus` ordinals.
pub const BUILD_SUCCEED: u32 = 1;
pub const BUILD_WITH_ERROR: u32 = 2;
pub const BUILD_CANCELLED: u32 = 3;

/// The status of a build that left `errors` error markers.
pub fn build_status(errors: usize) -> u32 {
    if errors == 0 {
        BUILD_SUCCEED
    } else {
        BUILD_WITH_ERROR
    }
}

/// `java.cleanup.actions` (or the deprecated `java.cleanup.actionsOnSave`)
/// contains `renameFileToType` and `java.saveActions.cleanup` is on.
fn rename_file_to_type_enabled() -> bool {
    let actions = crate::features::preferences::cleanup_actions();
    crate::features::preferences::get_bool("java.saveActions.cleanup").unwrap_or(false)
        && actions.iter().any(|a| a == "renameFileToType")
}

/// Public top-level types of `content` (`cu.getTypes()` with `public`).
fn public_top_level_type_count(content: &str) -> usize {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_java::language()).is_err() {
        return 0;
    }
    let Some(tree) = parser.parse(content, None) else {
        return 0;
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .filter(|n| {
            n.kind().ends_with("_declaration")
                && n.kind() != "package_declaration"
                && n.kind() != "import_declaration"
        })
        .filter(|n| {
            let mut c = n.walk();
            let modifiers = n.children(&mut c).find(|ch| ch.kind() == "modifiers");
            modifiers.is_some_and(|m| {
                m.utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .split_whitespace()
                    .any(|w| w == "public")
            })
        })
        .count()
}

/// `BaseDiagnosticsHandler.matchesDiagnosticFilter` (`java.diagnostic.filter`).
pub fn matches_diagnostic_filter(uri: &Url) -> bool {
    let Some(filters) = crate::features::preferences::get("java.diagnostic.filter") else {
        return false;
    };
    let Some(filters) = filters.as_array() else {
        return false;
    };
    let Some(path) = crate::project::uri_to_path(uri) else {
        return false;
    };
    let path = path.to_string_lossy().replace('\\', "/");
    filters.iter().filter_map(Value::as_str).any(|f| {
        crate::project::detect::glob_to_regex(f)
            .is_some_and(|re| re.is_match(&path) || re.is_match(path.trim_start_matches('/')))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_like_errors() {
        // Syntax errors are always reported.
        assert!(is_syntax_like_error(1_610_612_976, true));
        // UndefinedType (type related) is not.
        assert!(!is_syntax_like_error(16_777_218, true));
        assert!(!is_syntax_like_error(67_108_964, true));
        // Package mismatch only counts in the default project.
        assert!(is_syntax_like_error(PACKAGE_IS_NOT_EXPECTED_PACKAGE, true));
        assert!(!is_syntax_like_error(
            PACKAGE_IS_NOT_EXPECTED_PACKAGE,
            false
        ));
    }

    #[test]
    fn counts_public_top_level_types() {
        assert_eq!(
            1,
            public_top_level_type_count("package a;\npublic interface Foo {}\nclass Bar {}\n")
        );
        assert_eq!(
            3,
            public_top_level_type_count("public class A {}\npublic class B {}\npublic class C {}")
        );
        assert_eq!(0, public_top_level_type_count("class A {}"));
    }

    #[test]
    fn folder_packages() {
        assert_eq!(
            "a.b",
            folder_package(Path::new("/p/src"), Path::new("/p/src/a/b/X.java"))
        );
        assert_eq!(
            "",
            folder_package(Path::new("/p/src"), Path::new("/p/src/X.java"))
        );
    }
}

#[cfg(test)]
mod moving_average_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.MovingAverageTest`.

    use super::MovingAverage;

    #[test]
    fn test_update() {
        let mut average = MovingAverage::new(400);

        // initialize to 400 at first
        assert_eq!(400, average.value);

        average.update(200);
        // the first input value takes over the initial value
        assert_eq!(200, average.value);

        average.update(100);
        // (200 + 100) / 2
        assert_eq!(150, average.value);
    }
}
