//! tower-lsp LanguageServer implementation.

use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::code_action as ca_conv;
use crate::analysis::semantic::definition as def_conv;
use crate::analysis::semantic::diagnostics as diag_conv;
use crate::analysis::semantic::protocol::{
    BridgeCallHierarchyItem, BridgeDiagnostic, BridgeRange, BridgeResponse, BridgeTypeHierarchyItem,
};
use crate::analysis::semantic::NavKind;
use crate::analysis::syntax::parser::JavaParser;
use crate::analysis::syntax::{
    completion as syntax_completion, diagnostics as syntax_diagnostics,
    navigation as syntax_navigation, outline, snippets,
};
use crate::config::Config;
use crate::document_store::DocumentStore;
use crate::features::formatting;
use crate::features::navigation;
use crate::handlers::text_document::pos_to_offset;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    request::{
        GotoDeclarationParams, GotoDeclarationResponse, GotoImplementationParams,
        GotoImplementationResponse, GotoTypeDefinitionParams, GotoTypeDefinitionResponse,
    },
    *,
};
use tower_lsp::{Client, LanguageServer};
use tracing::{error, info, warn};

fn to_bridge_diag(uri: &Url, d: &Diagnostic) -> BridgeDiagnostic {
    BridgeDiagnostic {
        uri: uri.to_string(),
        start_line: d.range.start.line,
        start_char: d.range.start.character,
        end_line: d.range.end.line,
        end_char: d.range.end.character,
        severity: match d.severity {
            Some(DiagnosticSeverity::ERROR) => 1,
            Some(DiagnosticSeverity::WARNING) => 2,
            Some(DiagnosticSeverity::INFORMATION) => 3,
            Some(DiagnosticSeverity::HINT) => 4,
            _ => 1,
        },
        message: d.message.clone(),
        code: match &d.code {
            Some(NumberOrString::String(s)) => Some(s.clone()),
            Some(NumberOrString::Number(n)) => Some(n.to_string()),
            None => None,
        },
        category_id: 0,
        tags: None,
        ..Default::default()
    }
}

/// URIs whose last published diagnostics were non-empty (so a later build
/// that clears them publishes an empty list, like jdt.ls' marker deltas).
static PUBLISHED: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashSet<Url>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Collect tree-sitter and ECJ diagnostics for every open document and push
/// them to the client.  Shared by `spawn_compile_loop` and `publish_diagnostics_for_all`.
/// Returns whether any workspace project has error markers
/// (`BuildWorkspaceStatus.WITH_ERROR`).
async fn publish_diagnostics(
    store: &DocumentStore,
    dispatcher: &Dispatcher,
    client: &Client,
) -> bool {
    let snapshots = store.snapshots();
    let mut by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    let mut has_errors = false;

    // Run ECJ first — it is the authoritative source for Java diagnostics.
    // Track which URIs ECJ produced diagnostics for; tree-sitter diagnostics
    // are suppressed for those files to avoid inaccurate large-range squiggles
    // from tree-sitter's error-recovery nodes conflicting with ECJ's precise ones.
    let mut ecj_covered: std::collections::HashSet<Url> = std::collections::HashSet::new();
    let tag_support = crate::features::client_caps::diagnostic_tags();
    let mut docs: HashMap<String, Option<diag_conv::Doc16>> = HashMap::new();
    match dispatcher.compile_all().await {
        Ok(BridgeResponse::Diagnostics { items, .. }) => {
            for item in &items {
                let doc = docs.entry(item.uri.clone()).or_insert_with(|| {
                    Url::parse(&item.uri)
                        .ok()
                        .and_then(|u| crate::features::source_text(store, &u))
                        .map(|t| diag_conv::Doc16::new(&t))
                });
                if let Some((uri, diag)) = diag_conv::to_lsp(item, doc.as_ref(), tag_support) {
                    ecj_covered.insert(uri.clone());
                    by_uri.entry(uri).or_default().push(diag);
                }
            }
        }
        Ok(BridgeResponse::Error { message, .. }) => warn!("ECJ compile error: {message}"),
        Err(e) => error!("compile_all error: {e}"),
        _ => {}
    }

    // Fall back to tree-sitter only for files ECJ has no diagnostics for.
    for state in &snapshots {
        if !ecj_covered.contains(&state.uri) {
            let diags = state
                .tree
                .as_ref()
                .map(|t| syntax_diagnostics::collect(t))
                .unwrap_or_default();
            if !diags.is_empty() {
                by_uri.entry(state.uri.clone()).or_default().extend(diags);
            }
        }
    }

    // Ensure every open document gets an entry (clears stale diagnostics).
    for state in snapshots.iter().filter(|s| s.open) {
        by_uri.entry(state.uri.clone()).or_default();
    }
    {
        let ws = dispatcher
            .workspace
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for (uri, diags) in &by_uri {
            if ws.project_for_uri(uri).is_some()
                && diags
                    .iter()
                    .any(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            {
                has_errors = true;
            }
        }
        // Project-level markers (build path problems, build files).
        for (uri, diags) in crate::features::project_commands::project_marker_diagnostics(&ws) {
            if diags.iter().any(|d| d["severity"] == 1) {
                has_errors = true;
            }
            if let Ok(u) = Url::parse(&uri) {
                let d: Vec<Diagnostic> = diags
                    .into_iter()
                    .filter_map(|d| serde_json::from_value(d).ok())
                    .collect();
                by_uri.entry(u).or_default().extend(d);
            }
        }
    }
    // Clear what was reported before and is clean now.
    let previous: Vec<Url> = PUBLISHED.lock().unwrap().iter().cloned().collect();
    for uri in previous {
        by_uri.entry(uri).or_default();
    }
    for (uri, diags) in by_uri {
        {
            let mut published = PUBLISHED.lock().unwrap();
            if diags.is_empty() {
                published.remove(&uri);
            } else {
                published.insert(uri.clone());
            }
        }
        client.publish_diagnostics(uri, diags, None).await;
    }
    has_errors
}

/// Current project and preference watchers (`StandardProjectsManager.registerWatchers`).
fn project_watchers(
    ws: &crate::project::Workspace,
    roots: &[std::path::PathBuf],
    libraries: &[String],
) -> Vec<Value> {
    let mut watchers = crate::features::init::watchers(ws, roots);
    if let Some(extra) = crate::features::project_commands::watcher_registration(ws, libraries)
        .get("watchers")
        .and_then(Value::as_array)
    {
        for watcher in extra {
            if !watchers.contains(watcher) {
                watchers.push(watcher.clone());
            }
        }
    }
    watchers
}

/// jdt.ls `language/status` notification.
enum LanguageStatus {}

#[derive(serde::Serialize, serde::Deserialize)]
struct LanguageStatusParams {
    #[serde(rename = "type")]
    typ: String,
    message: String,
}

impl tower_lsp::lsp_types::notification::Notification for LanguageStatus {
    type Params = LanguageStatusParams;
    const METHOD: &'static str = "language/status";
}

pub struct JavaLanguageServer {
    client: Client,
    store: Arc<DocumentStore>,
    dispatcher: Arc<Dispatcher>,
    config: Arc<RwLock<Config>>,
    parser: Arc<Mutex<JavaParser>>,
    client_flavor: Arc<RwLock<ClientFlavor>>,
    workspace_folders: Arc<RwLock<Vec<WorkspaceFolder>>>,
    /// Sends a signal that source changed; background task debounces and compiles.
    compile_tx: watch::Sender<u64>,
    /// Workspace root folders used for project import.
    roots: Arc<RwLock<Vec<std::path::PathBuf>>>,
    /// Client capabilities relevant to rename (resource operations).
    rename_client: Arc<RwLock<crate::features::rename::RenameClient>>,
    /// Document life cycle and workspace diagnostics (jdt.ls clients).
    lifecycle: Arc<crate::features::lifecycle::Lifecycle>,
    /// `ClientPreferences` over the raw `initialize` params.
    client_prefs: Arc<std::sync::RwLock<crate::features::init::ClientPrefs>>,
    /// `ServiceStatus.ServiceReady` was sent.
    service_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Standalone files opened under a root that created an invisible
    /// project (`DocumentLifeCycleHandler.resolveCompilationUnit`).
    extra_triggers: Arc<RwLock<Vec<std::path::PathBuf>>>,
    /// Serializes workspace (re-)imports (jdt.ls runs them as workspace jobs
    /// holding the workspace rule).
    import_lock: Arc<Mutex<()>>,
}

#[path = "server_projects.rs"]
mod projects;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ClientFlavor {
    #[default]
    Default,
    LmsMonaco,
}

impl JavaLanguageServer {
    /// jdt.ls `java/searchSymbols`.
    pub async fn search_symbols(
        &self,
        params: crate::features::workspace_symbols::SearchSymbolParams,
    ) -> LspResult<Vec<SymbolInformation>> {
        let p = params;
        Ok(crate::features::workspace_symbols::search(
            &self.dispatcher,
            p.query.as_deref(),
            p.max_results,
            p.project_name.as_deref(),
            p.source_only,
        )
        .await)
    }

    pub fn new(client: Client) -> Self {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = Arc::new(DocumentStore::new());
        let dispatcher = Arc::new(Dispatcher::new(Arc::clone(&store), Arc::clone(&config)));

        let (compile_tx, _) = watch::channel(0u64);
        let lifecycle = crate::features::lifecycle::Lifecycle::new(
            client.clone(),
            Arc::clone(&store),
            Arc::clone(&dispatcher),
        );
        crate::features::completion::set_env(crate::features::completion::Env {
            dispatcher: Arc::clone(&dispatcher),
            store: Arc::clone(&store),
            client: client.clone(),
            config: Arc::clone(&config),
        });

        Self {
            lifecycle,
            client_prefs: Arc::new(std::sync::RwLock::new(Default::default())),
            service_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            client,
            store,
            dispatcher,
            config,
            parser: Arc::new(Mutex::new(JavaParser::new())),
            client_flavor: Arc::new(RwLock::new(ClientFlavor::Default)),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
            compile_tx,
            roots: Arc::new(RwLock::new(Vec::new())),
            rename_client: Arc::new(RwLock::new(Default::default())),
            extra_triggers: Arc::new(RwLock::new(Vec::new())),
            import_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Spawn the background debounce-compile loop.  Called once after ECJ is ready.
    fn spawn_compile_loop(&self) {
        let mut rx = self.compile_tx.subscribe();
        let dispatcher = Arc::clone(&self.dispatcher);
        let store = Arc::clone(&self.store);
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                // Wait for a change notification
                if rx.changed().await.is_err() {
                    break;
                }

                // Debounce: wait 400 ms, draining any additional signals that arrive
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(400)) => break,
                        res = rx.changed() => { if res.is_err() { return; } }
                    }
                }

                // If ECJ isn't ready yet, poll until it is rather than discarding this signal
                while !dispatcher.is_ecj_ready().await {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }

                publish_diagnostics(&store, &dispatcher, &client).await;
            }
        });
    }

    /// `java/buildWorkspace` (`BuildWorkspaceHandler.buildWorkspace`): the
    /// `BuildWorkspaceStatus` ordinal.  The parameter is `forceRebuild`,
    /// possibly wrapped in an array.
    pub async fn build_workspace(&self, _force_rebuild: Value) -> LspResult<Value> {
        drop(self.import_lock.lock().await);
        let errors = self.lifecycle.build(None).await;
        Ok(json!(crate::features::lifecycle::build_status(errors)))
    }

    /// `java/buildProjects` (`BuildWorkspaceHandler.buildProjects`).
    pub async fn build_projects(&self, params: Value) -> LspResult<Value> {
        let uris: Vec<String> = params
            .get("identifiers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.get("uri").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!(self.lifecycle.build_projects(&uris).await))
    }

    /// `java/classFileContents` (jdt.ls extension).
    pub async fn class_file_contents(&self, params: Value) -> LspResult<String> {
        let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
        Ok(navigation::class_file_contents(&self.dispatcher, uri).await)
    }

    // ── Utilities ─────────────────────────────────────────────────────────────

    /// (Re-)import all projects under the workspace roots and register their
    /// source files with the document store.
    async fn reimport_workspace(&self) {
        let _guard = self.import_lock.lock().await;
        let roots = self.roots.read().await.clone();
        let settings = self.current_import_settings().await;
        let previous = self.workspace_snapshot();
        let import_progress = self.begin_maven_import_progress(&roots, &settings).await;
        let ws = tokio::task::spawn_blocking(move || {
            crate::project::Workspace::import_with_previous(&roots, &settings, Some(&previous))
        })
        .await
        .unwrap_or_default();
        self.complete_maven_import_progress(&ws, import_progress)
            .await;
        for p in &ws.projects {
            info!(
                "Imported {:?} project '{}' at {}",
                p.kind,
                p.name,
                p.root.display()
            );
        }
        let files: Vec<Url> = ws
            .java_files()
            .into_keys()
            .filter_map(|p| Url::from_file_path(p).ok())
            .collect();
        self.store.set_workspace_files(files);
        *self
            .dispatcher
            .workspace
            .write()
            .unwrap_or_else(|e| e.into_inner()) = ws;
        self.record_build_file_digests();
        if self.service_ready.load(std::sync::atomic::Ordering::SeqCst) {
            self.register_watchers().await;
        }
    }

    async fn format_env(&self) -> formatting::FormatEnv<'_> {
        let cfg = self.config.read().await;
        formatting::FormatEnv {
            dispatcher: &self.dispatcher,
            client: &self.client,
            settings: cfg.format.clone(),
            roots: cfg.root_paths.clone(),
            extended_client_capabilities: cfg.extended_client_capabilities.clone(),
        }
    }

    fn request_compile(&self) {
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    pub async fn resolve_unimplemented_accessors(
        &self,
        params: crate::features::accessors::AccessorParams,
    ) -> LspResult<Vec<crate::features::accessors::AccessorField>> {
        Ok(crate::features::accessors::resolve(&self.dispatcher, params).await)
    }

    pub async fn generate_accessors(
        &self,
        params: crate::features::accessors::GenerateAccessorsParams,
    ) -> LspResult<Option<WorkspaceEdit>> {
        let format = self.format_env().await;
        let env = crate::correction::edit::Env {
            dispatcher: &self.dispatcher,
            format: &format,
            lifecycle: &self.lifecycle,
        };
        Ok(crate::features::accessors::generate(&env, params).await)
    }

    /// The `lms-monaco` client keeps the original diagnostics pipeline:
    /// every document compiled with full diagnostics, republished after
    /// each change.
    async fn legacy_diagnostics(&self) -> bool {
        *self.client_flavor.read().await == ClientFlavor::LmsMonaco
    }

    fn client_prefs(&self) -> crate::features::init::ClientPrefs {
        self.client_prefs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Send `client/registerCapability` / `client/unregisterCapability`.
    async fn send_registrations(
        client: &Client,
        changes: Vec<crate::features::init::RegistrationChange>,
    ) {
        use crate::features::init::RegistrationChange;
        for change in changes {
            match change {
                RegistrationChange::Register(r) => {
                    if let Err(e) = client.register_capability(vec![r]).await {
                        warn!("registerCapability failed: {e}");
                    }
                }
                RegistrationChange::Unregister { id, method } => {
                    if let Err(e) = client
                        .unregister_capability(vec![Unregistration { id, method }])
                        .await
                    {
                        warn!("unregisterCapability failed: {e}");
                    }
                }
            }
        }
    }

    /// `registerWatchers` for the current workspace.
    async fn register_watchers(&self) {
        let ws = self
            .dispatcher
            .workspace
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let roots = self.roots.read().await.clone();
        let settings = self.current_import_settings().await;
        let watchers = project_watchers(&ws, &roots, &settings.referenced_libraries.include);
        let changes = crate::features::init::watcher_registration(&self.client_prefs(), watchers);
        Self::send_registrations(&self.client, changes).await;
    }

    async fn send_status(client: &Client, typ: &str, message: &str) {
        client
            .send_notification::<LanguageStatus>(LanguageStatusParams {
                typ: typ.into(),
                message: message.into(),
            })
            .await;
    }

    /// Compile all open files and publish diagnostics to the client immediately.
    /// Used on demand (e.g. after a workspace-wide action); the background loop
    /// in `spawn_compile_loop` handles the normal debounced case.
    async fn publish_diagnostics_for_all(&self) -> bool {
        publish_diagnostics(&self.store, &self.dispatcher, &self.client).await
    }

    fn dedupe_completion_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
        let mut seen = std::collections::HashSet::new();
        items
            .into_iter()
            .filter(|item| {
                // Deduplicate by (label, kind) so that the same variable/method
                // offered by both tree-sitter and the ECJ bridge doesn't appear twice.
                seen.insert((
                    item.label.clone(),
                    item.kind.map(|kind| format!("{kind:?}")),
                ))
            })
            .collect()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for JavaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        *self.client_flavor.write().await = detect_client_flavor(params.client_info.as_ref());
        *self.client_prefs.write().unwrap_or_else(|e| e.into_inner()) =
            crate::features::init::ClientPrefs::from_params(
                &serde_json::to_value(&params).unwrap_or_default(),
            );
        let legacy = self.legacy_diagnostics().await;
        crate::features::client_caps::set(&params.capabilities);
        crate::features::preferences::init(&params);
        crate::features::completion::prefs::init(&params);
        *self.workspace_folders.write().await =
            params.workspace_folders.clone().unwrap_or_default();
        navigation::init_preferences(params.initialization_options.as_ref());
        *self.rename_client.write().await =
            crate::features::rename::RenameClient::from_capabilities(&params.capabilities);

        // Parse initializationOptions
        if let Some(opts) = params.initialization_options {
            let mut cfg: Config = serde_json::from_value::<Config>(opts)
                .unwrap_or_default()
                .with_defaults();
            if let Some(settings) = cfg.settings.clone() {
                merge_config_settings(&mut cfg, &settings);
            }
            cfg.completion_documentation_markdown = completion_markdown(&params.capabilities);
            *self.config.write().await = cfg;
        } else {
            let mut cfg = Config::default().with_defaults();
            cfg.completion_documentation_markdown = completion_markdown(&params.capabilities);
            *self.config.write().await = cfg;
        }
        self.config.write().await.inlay_hint_refresh_support = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.inlay_hint.as_ref())
            .and_then(|i| i.refresh_support)
            .unwrap_or(false);
        {
            let mut cfg = self.config.write().await;
            #[allow(deprecated)]
            let root_paths = formatting::options::jdtls_root_paths(
                cfg.workspace_folders.as_deref(),
                params.root_uri.as_ref(),
                params.root_path.as_deref(),
            );
            cfg.root_paths = root_paths;
        }

        // Import workspace projects (Gradle → Maven → Eclipse → invisible).
        // `BaseInitHandler.handleInitializationOptions`: the root paths are
        // `initializationOptions.workspaceFolders`, else `rootUri`/`rootPath`,
        // else the jdt.ls workspace location.
        let mut roots: Vec<std::path::PathBuf> = self.config.read().await.root_paths.clone();
        if roots.is_empty() {
            roots.push(crate::project::canonicalize_lenient(&data_dir()));
        }
        *self.roots.write().await = roots;
        if legacy {
            self.reimport_workspace().await;
            self.report_projects_status().await;
        }

        // Start ecj-bridge in background, then kick the compile loop
        let dispatcher = Arc::clone(&self.dispatcher);
        let store = Arc::clone(&self.store);
        let client = self.client.clone();
        let compile_tx = self.compile_tx.clone();
        let prefs = self.client_prefs();
        let roots = self.roots.read().await.clone();
        let settings = self.current_import_settings().await;
        tokio::spawn(async move {
            if let Err(e) = dispatcher.start_ecj().await {
                error!("Failed to start ecj-bridge: {e}");
                client
                    .show_message(
                        MessageType::ERROR,
                        format!("jdtls-rust: failed to start ecj-bridge: {e}"),
                    )
                    .await;
            } else if !legacy {
                info!("ecj-bridge started");
            } else {
                info!("ecj-bridge started");
                // Initial build of the imported workspace, then report readiness
                // the way jdt.ls does (`language/status` ServiceReady).
                publish_diagnostics(&store, &dispatcher, &client).await;
                client
                    .send_notification::<LanguageStatus>(LanguageStatusParams {
                        typ: "ServiceReady".into(),
                        message: "ServiceReady".into(),
                    })
                    .await;
                let ws = dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let watchers =
                    project_watchers(&ws, &roots, &settings.referenced_libraries.include);
                Self::send_registrations(
                    &client,
                    crate::features::init::watcher_registration(&prefs, watchers),
                )
                .await;
                // Trigger a compile for anything opened meanwhile — use a fresh
                // increment so the watch always fires.
                let next = (*compile_tx.borrow()).wrapping_add(1);
                let _ = compile_tx.send(next);
            }
        });
        if legacy {
            self.spawn_compile_loop();
        } else {
            self.lifecycle.start();
        }

        let token_legend = crate::features::semantic_tokens::legend();

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                        ..Default::default()
                    },
                )),
                // jdt.ls `CompletionHandler.getDefaultCompletionOptions`.
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".into(),
                        "@".into(),
                        "#".into(),
                        "*".into(),
                        " ".into(),
                    ]),
                    resolve_provider: Some(true),
                    completion_item: crate::features::completion::prefs::Client::load()
                        .label_details
                        .then(|| CompletionOptionsCompletionItem {
                            label_details_support: Some(true),
                        }),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                definition_provider: Some(OneOf::Left(true)),
                declaration_provider: Some(DeclarationCapability::Simple(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                // `CodeActionHandler.createOptions`.
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(
                            [
                                "quickfix",
                                "refactor",
                                "refactor.extract",
                                "refactor.inline",
                                "refactor.rewrite",
                                "source",
                                "source.organizeImports",
                            ]
                            .into_iter()
                            .filter(|k| crate::features::client_caps::supported_code_action_kind(k))
                            .map(CodeActionKind::from)
                            .collect(),
                        ),
                        resolve_provider: Some(crate::features::client_caps::resolve_code_action()),
                        work_done_progress_options: Default::default(),
                    },
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: ";".to_owned(),
                    more_trigger_character: Some(vec!["}".to_owned(), "\n".to_owned()]),
                }),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: token_legend,
                            full: Some(SemanticTokensFullOptions::Delta { delta: Some(false) }),
                            range: Some(false),
                            work_done_progress_options: Default::default(),
                        },
                    ),
                ),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "java.project.getAll".to_owned(),
                        "jdtls-rust.classFileUri".to_owned(),
                        "jdtls-rust.refreshDiagnostics".to_owned(),
                        "java.project.refreshDiagnostics".to_owned(),
                        "java.project.rebuild".to_owned(),
                        "java.project.getSettings".to_owned(),
                        "java.project.getClasspaths".to_owned(),
                        "java.project.isTestFile".to_owned(),
                        "java.project.listSourcePaths".to_owned(),
                        "java.project.resolveSourceAttachment".to_owned(),
                        "java.project.changeImportedProjects".to_owned(),
                        "java.project.import".to_owned(),
                        "java.edit.stringFormatting".to_owned(),
                        "java.navigate.openTypeHierarchy".to_owned(),
                        "java.navigate.resolveTypeHierarchy".to_owned(),
                    ],
                    work_done_progress_options: Default::default(),
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                        did_create: Some(java_file_operation_registration_options()),
                        did_rename: Some(java_file_operation_registration_options()),
                        did_delete: Some(java_file_operation_registration_options()),
                        ..Default::default()
                    }),
                }),
                linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(
                    true,
                )),
                // type_hierarchy is not in ServerCapabilities for lsp-types 0.94
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "jdtls-rust".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Client initialized");
        if self.legacy_diagnostics().await {
            return;
        }
        // Custom notifications are permitted after initialize completes.
        self.reimport_workspace().await;
        // `InitHandler.triggerInitialization` + `JDTLanguageServer.initialized`.
        let client = self.client.clone();
        let dispatcher = Arc::clone(&self.dispatcher);
        let lifecycle = Arc::clone(&self.lifecycle);
        let prefs = self.client_prefs();
        let ready = Arc::clone(&self.service_ready);
        let ws = self
            .dispatcher
            .workspace
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let roots = self.roots.read().await.clone();
        let settings = self.current_import_settings().await;
        let watchers = project_watchers(&ws, &roots, &settings.referenced_libraries.include);
        tokio::spawn(async move {
            Self::send_status(&client, "Starting", "Init...").await;
            Self::send_status(&client, "Starting", "0% Starting Java Language Server").await;
            Self::send_status(&client, "ProjectStatus", "OK").await;
            Self::send_status(&client, "Starting", "100% Starting Java Language Server").await;
            Self::send_status(&client, "Started", "Ready").await;
            while !dispatcher.is_ecj_ready().await {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let mut changes = crate::features::init::initial_registrations(&prefs);
            changes.extend(crate::features::init::sync_capabilities_to_settings(
                &prefs, false,
            ));
            Self::send_registrations(&client, changes).await;
            lifecycle.build(None).await;
            Self::send_registrations(
                &client,
                crate::features::init::watcher_registration(&prefs, watchers),
            )
            .await;
            ready.store(true, std::sync::atomic::Ordering::SeqCst);
            Self::send_status(&client, "ServiceReady", "ServiceReady").await;
        });
    }

    async fn shutdown(&self) -> LspResult<()> {
        self.dispatcher.shutdown_ecj().await;
        Ok(())
    }

    // ── Text document lifecycle ───────────────────────────────────────────────

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        {
            let mut parser = self.parser.lock().await;
            self.store.open(
                doc.uri.clone(),
                doc.language_id,
                doc.version,
                doc.text,
                &mut parser,
            );
        }
        self.on_document_opened(&doc.uri).await;
        if !self.legacy_diagnostics().await {
            let roots = self.config.read().await.root_paths.clone();
            self.lifecycle.did_open(&doc.uri, &roots).await;
            return;
        }
        if crate::features::lifecycle::is_java_like(&doc.uri) {
            self.store.set_active_java_uri(&doc.uri);
        }
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        if self.store.is_open(&uri) && !params.content_changes.is_empty() {
            crate::correction::handler::document_changed(&uri);
        }
        {
            let mut parser = self.parser.lock().await;
            self.store
                .apply_changes(&uri, version, params.content_changes, &mut parser);
        }
        if !self.legacy_diagnostics().await {
            self.lifecycle.did_change(&uri);
            return;
        }
        if self.store.is_open(&uri) && crate::features::lifecycle::is_java_like(&uri) {
            self.store.set_active_java_uri(&uri);
        }
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if !self.legacy_diagnostics().await {
            self.lifecycle.did_close(&uri).await;
            return;
        }
        self.store.close(&uri);
        if self.store.is_workspace_file(&uri) {
            // Reverts to the on-disk content; diagnostics follow the build.
            self.request_compile();
        } else {
            // Clear diagnostics for the closed virtual/external file
            self.client.publish_diagnostics(uri, vec![], None).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if !self.legacy_diagnostics().await {
            let apply_edit = self.client_prefs().is_workspace_apply_edit_supported();
            self.lifecycle
                .did_save(&params.text_document.uri, apply_edit)
                .await;
            return;
        }
        // Publish diagnostics immediately on save rather than waiting for the
        // debounce loop — gives the user instant feedback after an explicit save.
        if self.dispatcher.is_ecj_ready().await {
            self.publish_diagnostics_for_all().await;
        } else {
            // ECJ not ready yet; fall back to the debounce loop.
            let next = (*self.compile_tx.borrow()).wrapping_add(1);
            let _ = self.compile_tx.send(next);
        }
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let old_import_settings = self.current_import_settings().await;
        navigation::update_settings(&params.settings);
        crate::features::preferences::update(&params.settings);
        if self.service_ready.load(std::sync::atomic::Ordering::SeqCst) {
            let changes =
                crate::features::init::sync_capabilities_to_settings(&self.client_prefs(), false);
            Self::send_registrations(&self.client, changes).await;
        }
        let (restart_ecj, refresh_inlay_hints) = {
            let mut config = self.config.write().await;
            let old_inlay_hints = config.inlay_hints.clone();
            let restart = merge_config_settings(&mut config, &params.settings);
            (
                restart,
                config.inlay_hint_refresh_support
                    && old_inlay_hints.needs_refresh(&config.inlay_hints),
            )
        };
        if refresh_inlay_hints {
            let client = self.client.clone();
            tokio::spawn(async move {
                let _ = client.inlay_hint_refresh().await;
            });
        }

        if restart_ecj {
            if let Err(e) = self.dispatcher.restart_ecj().await {
                error!("Failed to restart ecj-bridge after config change: {e}");
            }
        }

        let new_import_settings = self.current_import_settings().await;
        self.on_import_settings_changed(&old_import_settings, &new_import_settings)
            .await;
        if !self.legacy_diagnostics().await {
            return;
        }
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        {
            let mut folders = self.workspace_folders.write().await;
            folders.retain(|folder| {
                !params
                    .event
                    .removed
                    .iter()
                    .any(|removed| removed.uri == folder.uri)
            });
            for added in &params.event.added {
                if !folders.iter().any(|folder| folder.uri == added.uri) {
                    folders.push(added.clone());
                }
            }
            let mut roots = self.roots.write().await;
            for removed in &params.event.removed {
                if let Ok(p) = removed.uri.to_file_path() {
                    roots.retain(|r| r != &p);
                }
            }
            for added in &params.event.added {
                if let Ok(p) = added.uri.to_file_path() {
                    if !roots.contains(&p) {
                        roots.push(p);
                    }
                }
            }
        }
        self.reimport_workspace().await;
        if !self.legacy_diagnostics().await {
            self.lifecycle.build(None).await;
            self.register_watchers().await;
            return;
        }
        self.request_compile();
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        let mut uris = Vec::new();
        for file in params.files {
            let Ok(uri) = Url::parse(&file.uri) else {
                continue;
            };
            if crate::features::lifecycle::is_java_like(&uri)
                && self.dispatcher.owns_source_path(&uri)
            {
                self.store.add_workspace_file(uri.clone());
            }
            uris.push(uri);
        }
        if self.legacy_diagnostics().await {
            self.request_compile();
        } else {
            self.lifecycle.files_changed(&uris).await;
        }
    }

    async fn will_rename_files(
        &self,
        params: RenameFilesParams,
    ) -> LspResult<Option<WorkspaceEdit>> {
        let files = params
            .files
            .into_iter()
            .map(|f| (f.old_uri, f.new_uri))
            .collect::<Vec<_>>();
        let edit =
            crate::features::file_events::will_rename_files(&self.dispatcher, &self.store, &files)
                .await;
        edit.map(serde_json::from_value).transpose().map_err(|e| {
            warn!("Invalid file refactoring edit: {e}");
            tower_lsp::jsonrpc::Error::internal_error()
        })
    }

    async fn did_rename_files(&self, params: RenameFilesParams) {
        let mut uris = Vec::new();
        for rename in params.files {
            let Ok(old_uri) = Url::parse(&rename.old_uri) else {
                continue;
            };
            let Ok(new_uri) = Url::parse(&rename.new_uri) else {
                continue;
            };
            self.store.rename(&old_uri, new_uri.clone());
            uris.extend([old_uri.clone(), new_uri]);
            self.client.publish_diagnostics(old_uri, vec![], None).await;
        }

        if self.legacy_diagnostics().await {
            self.request_compile();
        } else {
            self.lifecycle.files_changed(&uris).await;
        }
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        let legacy = self.legacy_diagnostics().await;
        let mut uris = Vec::new();
        for deleted in params.files {
            let Ok(uri) = Url::parse(&deleted.uri) else {
                continue;
            };
            if legacy {
                self.store.remove(&uri);
                self.client.publish_diagnostics(uri, vec![], None).await;
            } else {
                self.store.remove_workspace_path(&uri);
                uris.push(uri);
            }
        }
        if legacy {
            self.request_compile();
        } else {
            self.lifecycle.files_changed(&uris).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let mut should_recompile = false;
        let mut reimport = false;
        let legacy = self.legacy_diagnostics().await;
        let mut file_changes: Vec<Url> = Vec::new();
        let changed_paths: Vec<std::path::PathBuf> = params
            .changes
            .iter()
            .filter_map(|c| crate::project::uri_to_path(&c.uri))
            .collect();
        for change in params.changes {
            file_changes.push(change.uri.clone());
            let name = change
                .uri
                .path()
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_owned();
            if matches!(
                name.as_str(),
                "pom.xml"
                    | "build.gradle"
                    | "settings.gradle"
                    | "build.gradle.kts"
                    | "settings.gradle.kts"
            ) {
                // `StandardProjectsManager.fileChanged` → build support.
                if let Some(path) = crate::project::uri_to_path(&change.uri) {
                    self.on_build_file_changed(&path).await;
                }
                continue;
            }
            if is_build_descriptor(&name) {
                reimport = true;
                continue;
            }
            match change.typ {
                FileChangeType::DELETED => {
                    if legacy {
                        self.store.remove(&change.uri);
                        self.client
                            .publish_diagnostics(change.uri, vec![], None)
                            .await;
                    } else {
                        self.store.remove_workspace_path(&change.uri);
                    }
                    should_recompile = true;
                }
                FileChangeType::CREATED => {
                    if name.ends_with(".java") && self.dispatcher.owns_source_path(&change.uri) {
                        self.store.add_workspace_file(change.uri);
                    }
                    should_recompile = true;
                }
                FileChangeType::CHANGED => {
                    self.store.invalidate_disk(&change.uri);
                    should_recompile = true;
                }
                _ => {}
            }
        }

        if reimport {
            self.reimport_workspace().await;
            should_recompile = true;
        } else if self.on_files_changed(&changed_paths).await {
            should_recompile = true;
        }
        if !legacy {
            if reimport {
                self.lifecycle.build(None).await;
                self.register_watchers().await;
            } else if !file_changes.is_empty() {
                self.lifecycle.files_changed(&file_changes).await;
            }
            return;
        }
        if should_recompile {
            self.request_compile();
        }
    }

    // Syntax completion remains available while the JDT bridge starts.
    // Once ready, CompletionService handles requests with the JDT engine.
    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        if !crate::features::completion::prefs::Prefs::load().enabled {
            return Ok(Some(CompletionResponse::Array(vec![])));
        }
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let trigger_char: Option<&str> = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());

        // Wait until the stored content is up-to-date around the cursor.
        // A plain line-length check is not enough: if the cursor sits before an
        // existing delimiter like `;`, the stale document can still be "long
        // enough" while missing the just-typed identifier or trigger character.
        {
            let mut change_rx = self.compile_tx.subscribe();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(150);
            loop {
                let up_to_date = self
                    .store
                    .get(uri)
                    .map(|state| {
                        completion_store_is_fresh(&state.content_string(), pos, trigger_char)
                    })
                    .unwrap_or(true); // document not open yet → don't spin

                if up_to_date || tokio::time::Instant::now() >= deadline {
                    break;
                }
                tokio::select! {
                    _ = change_rx.changed() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {}
                }
            }
        }

        let (offset, content, tree) = {
            match self.store.get(uri) {
                None => return Ok(None),
                Some(state) => (
                    pos_to_offset(&state.content, pos).unwrap_or(0),
                    state.content_string(),
                    state.tree.clone(),
                ),
            }
        };

        let import_prefix: Option<String> =
            detect_import_prefix(&content, pos.line, pos.character, trigger_char);
        let in_import = import_prefix.is_some();
        let in_member_access = is_member_access_context(&content, offset);

        // Suppress completions when the cursor is in a variable/parameter name slot.
        if let Some(tree) = tree.as_ref() {
            if syntax_completion::is_in_declaration_name(tree, &content, offset) {
                return Ok(Some(CompletionResponse::Array(vec![])));
            }
        }
        // Also suppress when the cursor sits right after a type name on the same line
        // (e.g. `int |`, `final int myV|`) — the user is about to type a *new* name.
        if syntax_completion::is_awaiting_declaration_name(&content, offset) {
            return Ok(Some(CompletionResponse::Array(vec![])));
        }
        // Suppress auto-trigger right after `= ` — user hasn't started typing yet.
        if syntax_completion::is_after_assignment_operator(&content, offset) {
            return Ok(Some(CompletionResponse::Array(vec![])));
        }
        if is_after_numeric_literal_dot(&content, offset) {
            return Ok(Some(CompletionResponse::Array(vec![])));
        }
        if let Some(tree) = tree.as_ref() {
            if !syntax_completion::is_inside_method_body(tree, offset)
                && syntax_completion::is_inside_class_body(tree, offset)
            {
                if syntax_completion::is_after_member_modifiers(&content, offset)
                    || syntax_completion::is_in_member_param_name_slot(&content, offset)
                    || syntax_completion::is_after_member_parameter_list(&content, offset)
                {
                    return Ok(Some(CompletionResponse::Array(vec![])));
                }
            }
        }

        let mut items: Vec<CompletionItem> = Vec::new();

        if !in_import && !in_member_access {
            let prefix = syntax_completion::current_prefix(&content, offset);
            let in_params = tree
                .as_ref()
                .map(|t| syntax_completion::is_in_parameter_declaration(t, offset))
                .unwrap_or(false);
            let (in_method, in_class) = tree
                .as_ref()
                .map(|t| {
                    let m = syntax_completion::is_inside_method_body(t, offset);
                    let c = m || syntax_completion::is_inside_class_body(t, offset);
                    (m, c)
                })
                .unwrap_or((false, false));

            if in_params {
                // Parameter declaration: only type names are valid.
                // But if the cursor is in the parameter-name slot (type already written),
                // suppress all Rust-side completions — the ECJ bridge handles it too.
                let in_param_name_slot = syntax_completion::is_in_param_name_slot(&content, offset);
                if !in_param_name_slot {
                    if let Some(tree) = tree.as_ref() {
                        items.extend(syntax_completion::import_type_completions(
                            tree, &content, &prefix,
                        ));
                    }
                }
            } else {
                // Local variables, parameters, and imported types are only valid
                // inside a class body — never at the file top level.
                if in_class {
                    if let Some(tree) = tree.as_ref() {
                        items.extend(syntax_completion::local_completions(tree, &content, offset));
                        items.extend(syntax_completion::import_type_completions(
                            tree, &content, &prefix,
                        ));
                    }
                }

                if is_expression_context(&content, offset) {
                    items.extend(snippets::expression_keywords());
                } else if in_method {
                    items.extend(snippets::method_body_snippets());
                } else if in_class {
                    items.extend(snippets::class_body_keywords());
                    items.extend(snippets::class_body_snippets());
                }
                // Top level: nothing added from Rust side; ECJ bridge handles it.
            }
        } else if in_member_access && !in_import {
            // Syntax-level this. completion (ECJ will override with full semantic results)
            if let Some(tree) = tree.as_ref() {
                items.extend(syntax_completion::this_member_completions(
                    tree, &content, offset,
                ));
            }
            items.extend(snippets::postfix_snippets(&content, offset, pos));
        }

        // Compute the word range at the cursor. The CodeRunner Monaco adapter in
        // `ui/` relies on the server to provide explicit replacement ranges for
        // import-path completions and other items, while the simpler `web/`
        // demo synthesizes its own range client-side.
        let word_range = word_range_at(&content, pos);

        if word_range.is_some() {
            for item in &mut items {
                attach_completion_text_edit(item, word_range.clone());
            }
        }

        Ok(Some(CompletionResponse::Array(
            Self::dedupe_completion_items(items),
        )))
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> LspResult<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(state) => state,
        };
        let content = state.content_string();
        drop(state);

        let type_targets = open_java_type_targets(&self.store);
        let mut links = import_document_links(&content, &type_targets);
        links.extend(external_url_links(&content));

        Ok(Some(links))
    }

    // ── Hover ─────────────────────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let cfg = self.config.read().await.clone();
        Ok(
            crate::features::hover::handle(&self.dispatcher, &cfg, &params)
                .await
                .flatten(),
        )
    }

    // ── Signature Help ────────────────────────────────────────────────────────

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        use crate::features::signature_help;
        let settings =
            signature_help::Settings::from_settings(self.config.read().await.settings.as_ref());
        let doc = params.text_document_position_params;
        Ok(Some(
            signature_help::signature_help(
                &self.dispatcher,
                &doc.text_document.uri,
                doc.position,
                settings,
            )
            .await,
        ))
    }

    // ── Definition / Declaration / Type Definition / Implementation ───────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        if let Some(locs) = navigation::definition(&self.dispatcher, uri, pos).await {
            return Ok(Some(GotoDefinitionResponse::Array(locs)));
        }
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self
                .dispatcher
                .navigate(uri, offset, NavKind::Definition)
                .await
            {
                Ok(BridgeResponse::Locations { locations, .. }) => {
                    let locs = def_conv::to_lsp(&locations);
                    if !locs.is_empty() {
                        return Ok(Some(GotoDefinitionResponse::Array(locs)));
                    }
                }
                _ => {}
            }
        }

        Ok(tree
            .as_ref()
            .and_then(|tree| syntax_navigation::definition_range(tree, &content, offset))
            .map(|range| {
                GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                })
            }))
    }

    async fn goto_declaration(
        &self,
        params: GotoDeclarationParams,
    ) -> LspResult<Option<GotoDeclarationResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        if let Some(locs) = navigation::declaration(&self.dispatcher, uri, pos).await {
            return Ok(Some(GotoDefinitionResponse::Array(locs)));
        }
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self
                .dispatcher
                .navigate(uri, offset, NavKind::Declaration)
                .await
            {
                Ok(BridgeResponse::Locations { locations, .. }) => {
                    let locs = def_conv::to_lsp(&locations);
                    if !locs.is_empty() {
                        return Ok(Some(GotoDefinitionResponse::Array(locs)));
                    }
                }
                _ => {}
            }
        }

        Ok(tree
            .as_ref()
            .and_then(|tree| syntax_navigation::definition_range(tree, &content, offset))
            .map(|range| {
                GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                })
            }))
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> LspResult<Option<GotoTypeDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        if let Some(locs) = navigation::type_definition(&self.dispatcher, uri, pos).await {
            return Ok(locs.map(GotoDefinitionResponse::Array));
        }
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }
        match self
            .dispatcher
            .navigate(uri, offset, NavKind::TypeDefinition)
            .await
        {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(GotoDefinitionResponse::Array(locs)))
                }
            }
            _ => Ok(None),
        }
    }

    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> LspResult<Option<GotoImplementationResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        if let Some(locs) = navigation::implementation(&self.dispatcher, uri, pos).await {
            return Ok(Some(GotoDefinitionResponse::Array(locs)));
        }
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }
        match self
            .dispatcher
            .navigate(uri, offset, NavKind::Implementation)
            .await
        {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(GotoDefinitionResponse::Array(locs)))
                }
            }
            _ => Ok(None),
        }
    }

    // ── References ────────────────────────────────────────────────────────────

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        if let Some(locs) = navigation::references(
            &self.dispatcher,
            uri,
            pos,
            params.context.include_declaration,
        )
        .await
        {
            return Ok(Some(locs));
        }
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.find_references(uri, offset).await {
                Ok(BridgeResponse::Locations { locations, .. }) => {
                    let locs = def_conv::to_lsp(&locations);
                    if !locs.is_empty() {
                        return Ok(Some(locs));
                    }
                }
                _ => {}
            }
        }

        let locs = tree
            .as_ref()
            .map(|tree| syntax_navigation::references(tree, &content, offset))
            .unwrap_or_default()
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect::<Vec<_>>();

        Ok(if locs.is_empty() { None } else { Some(locs) })
    }

    // ── Document Highlight ────────────────────────────────────────────────────

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        if let Some(highlights) = navigation::document_highlight(&self.dispatcher, uri, pos).await {
            return Ok(Some(highlights));
        }
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.find_references(uri, offset).await {
                Ok(BridgeResponse::Locations { locations, .. }) => {
                    // Only keep references in the same file
                    let uri_str = uri.to_string();
                    let highlights: Vec<DocumentHighlight> = locations
                        .iter()
                        .filter(|l| l.uri == uri_str)
                        .map(|l| DocumentHighlight {
                            range: Range {
                                start: Position {
                                    line: l.start_line,
                                    character: l.start_char,
                                },
                                end: Position {
                                    line: l.end_line,
                                    character: l.end_char,
                                },
                            },
                            kind: Some(DocumentHighlightKind::READ),
                        })
                        .collect();
                    if !highlights.is_empty() {
                        return Ok(Some(highlights));
                    }
                }
                _ => {}
            }
        }

        let highlights = tree
            .as_ref()
            .map(|tree| syntax_navigation::document_highlights(tree, &content, offset))
            .unwrap_or_default();

        Ok(if highlights.is_empty() {
            None
        } else {
            Some(highlights)
        })
    }

    // ── Document Symbols (Outline) ─────────────────────────────────────────────

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        if !crate::features::lifecycle::is_java_like(uri) {
            // Not a compilation unit (`JDTUtils.resolveCompilationUnit` is null).
            return Ok(Some(DocumentSymbolResponse::Nested(Vec::new())));
        }
        if crate::classfile::is_class_file_uri(uri) {
            let Some((text, attached)) =
                crate::features::navigation::class_file_document(&self.dispatcher, uri.as_str())
                    .await
            else {
                return Ok(Some(DocumentSymbolResponse::Nested(Vec::new())));
            };
            return Ok(Some(crate::features::document_symbol::class_file_symbols(
                uri, &text, attached,
            )));
        }
        let Some(text) = crate::features::document_text(&self.dispatcher, uri).await else {
            return Ok(Some(DocumentSymbolResponse::Nested(Vec::new())));
        };
        Ok(Some(crate::features::document_symbol::document_symbols(
            uri, &text,
        )))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        Ok(Some(
            crate::features::workspace_symbols::search(
                &self.dispatcher,
                Some(&params.query),
                0,
                None,
                false,
            )
            .await,
        ))
    }

    // ── Code Action ────────────────────────────────────────────────────────────

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }

        let bridge_range = BridgeRange {
            start_line: params.range.start.line,
            start_char: params.range.start.character,
            end_line: params.range.end.line,
            end_char: params.range.end.character,
        };

        let bridge_diags = params
            .context
            .diagnostics
            .iter()
            .filter(|d| d.source.as_deref() == Some(diag_conv::SERVER_SOURCE_ID))
            .map(|d| to_bridge_diag(uri, d))
            .collect::<Vec<_>>();
        let has_java_problems = !bridge_diags.is_empty();

        // jdt.ls `CodeActionHandler` (Rust port).
        let env = self.format_env().await;
        let cenv = crate::correction::edit::Env {
            dispatcher: &self.dispatcher,
            format: &env,
            lifecycle: &self.lifecycle,
        };
        let mut lsp_actions = crate::correction::handler::code_actions(&cenv, &params).await;

        // Corrections not ported to Rust yet still come from the bridge.
        if crate::features::client_caps::supported_code_action_kind(
            CodeActionKind::QUICKFIX.as_str(),
        ) {
            if let Ok(BridgeResponse::CodeActions { actions, .. }) = self
                .dispatcher
                .code_action(uri, bridge_range, bridge_diags)
                .await
            {
                let titles: std::collections::HashSet<String> = lsp_actions
                    .iter()
                    .map(|a| match a {
                        CodeActionOrCommand::CodeAction(c) => c.title.clone(),
                        CodeActionOrCommand::Command(c) => c.title.clone(),
                    })
                    .collect();
                lsp_actions.extend(
                    ca_conv::to_lsp(&actions)
                        .into_iter()
                        .filter(|a| {
                            a.kind.as_ref().is_some_and(|kind| {
                                (kind.as_str() != CodeActionKind::QUICKFIX.as_str()
                                    || has_java_problems)
                                    && params.context.only.as_ref().is_none_or(|only| {
                                        only.is_empty()
                                            || only.iter().any(|requested| {
                                                kind.as_str().starts_with(requested.as_str())
                                            })
                                    })
                            })
                        })
                        .filter(|a| {
                            !titles.contains(&a.title)
                                && !crate::correction::handler::is_superseded_legacy_action(
                                    &a.title,
                                )
                        })
                        .map(CodeActionOrCommand::CodeAction),
                );
            }
        }
        Ok(Some(lsp_actions))
    }

    async fn code_action_resolve(&self, params: CodeAction) -> LspResult<CodeAction> {
        let env = self.format_env().await;
        let cenv = crate::correction::edit::Env {
            dispatcher: &self.dispatcher,
            format: &env,
            lifecycle: &self.lifecycle,
        };
        Ok(crate::correction::handler::resolve(&cenv, params).await)
    }

    // ── Formatting ─────────────────────────────────────────────────────────────

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let env = self.format_env().await;
        Ok(Some(
            formatting::format(&env, &params.text_document.uri, &params.options, None).await,
        ))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let env = self.format_env().await;
        Ok(Some(
            formatting::format(
                &env,
                &params.text_document.uri,
                &params.options,
                Some(params.range),
            )
            .await,
        ))
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let env = self.format_env().await;
        let pos = &params.text_document_position;
        Ok(Some(
            formatting::on_type_format(
                &env,
                &pos.text_document.uri,
                &params.options,
                pos.position,
                &params.ch,
            )
            .await,
        ))
    }

    // ── Rename ────────────────────────────────────────────────────────────────

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let Some(text) = self.store.get(uri).map(|s| s.content_string()) else {
            return Ok(Some(WorkspaceEdit {
                changes: Some(HashMap::new()),
                ..Default::default()
            }));
        };
        let client = *self.rename_client.read().await;
        let enabled =
            crate::features::rename::rename_enabled(self.config.read().await.settings.as_ref());
        crate::features::rename::rename(
            &self.dispatcher,
            uri,
            &text,
            params.text_document_position.position,
            &params.new_name,
            client,
            enabled,
        )
        .await
        .map(Some)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let Some(text) = self.store.get(uri).map(|s| s.content_string()) else {
            return Err(tower_lsp::jsonrpc::Error {
                code: tower_lsp::jsonrpc::ErrorCode::InvalidRequest,
                message: "Renaming this element is not supported.".into(),
                data: None,
            });
        };
        crate::features::rename::prepare_rename(&self.dispatcher, uri, &text, params.position)
            .await
            .map(|range| Some(PrepareRenameResponse::Range(range)))
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> LspResult<Option<LinkedEditingRanges>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(state) => state,
        };
        let content = state.content_string();
        let tree = state.tree.clone();
        drop(state);

        let Some((current_range, placeholder)) = identifier_range_and_text_at(&content, pos) else {
            return Ok(None);
        };
        if is_java_keyword(&placeholder) {
            return Ok(None);
        }

        let mut ranges = if let (Some(tree), Some(offset)) =
            (tree.as_ref(), pos_to_offset_from_text(&content, pos))
        {
            let refs = syntax_navigation::references(tree, &content, offset);
            if refs.is_empty() {
                vec![current_range]
            } else {
                refs
            }
        } else {
            vec![current_range]
        };

        ranges.sort_by_key(|r| (r.start.line, r.start.character, r.end.line, r.end.character));
        ranges.dedup_by_key(|r| (r.start.line, r.start.character, r.end.line, r.end.character));

        Ok(Some(LinkedEditingRanges {
            ranges,
            word_pattern: Some("[A-Za-z_$][A-Za-z0-9_$]*".to_owned()),
        }))
    }

    // ── Folding Ranges ────────────────────────────────────────────────────────

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let Some(text) =
            crate::features::document_text(&self.dispatcher, &params.text_document.uri).await
        else {
            return Ok(Some(Vec::new()));
        };
        Ok(Some(crate::features::folding_range::folding_ranges(&text)))
    }

    // ── Semantic Tokens ────────────────────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let empty = || {
            Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: Vec::new(),
            })))
        };
        // jdt.ls waits for the document life-cycle jobs; wait for the bridge.
        for _ in 0..600 {
            if self.dispatcher.is_ecj_ready().await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let Some(text) = crate::features::document_text(&self.dispatcher, uri).await else {
            return empty();
        };
        let Ok(BridgeResponse::AstBindings {
            strings,
            nodes,
            bindings,
            ..
        }) = self.dispatcher.ast_bindings(uri).await
        else {
            return empty();
        };
        let ast = crate::features::semantic_tokens::Ast::from_bridge(&strings, &nodes, &bindings);
        let data = crate::features::semantic_tokens::semantic_tokens(&text, &ast);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> LspResult<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;
        let text = if crate::classfile::is_class_file_uri(uri) {
            match crate::features::navigation::class_file_document(&self.dispatcher, uri.as_str())
                .await
            {
                Some((text, true)) => text,
                _ => return Ok(Some(Vec::new())),
            }
        } else {
            let Some(text) = crate::features::source_text(&self.store, uri) else {
                return Ok(Some(Vec::new()));
            };
            text
        };
        Ok(Some(crate::features::selection_range::selection_ranges(
            &text,
            &params.positions,
        )))
    }

    // ── Inlay Hints ───────────────────────────────────────────────────────────

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let prefs = self.config.read().await.inlay_hints.clone();
        Ok(Some(
            crate::features::inlay_hints::inlay_hint(&self.dispatcher, &prefs, &params).await,
        ))
    }

    // ── Code Lenses ───────────────────────────────────────────────────────────

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        if crate::classfile::is_class_file_uri(&params.text_document.uri) {
            let Some((text, attached)) = crate::features::navigation::class_file_document(
                &self.dispatcher,
                params.text_document.uri.as_str(),
            )
            .await
            else {
                return Ok(Some(Vec::new()));
            };
            return Ok(Some(crate::features::code_lens::class_file_code_lenses(
                &params.text_document.uri,
                &text,
                attached,
            )));
        }
        Ok(Some(crate::features::code_lens::code_lenses(
            &self.store,
            &params.text_document.uri,
        )))
    }

    async fn code_lens_resolve(&self, lens: CodeLens) -> LspResult<CodeLens> {
        Ok(crate::features::code_lens::resolve(&self.dispatcher, lens).await)
    }

    // ── Call Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        let p = params.text_document_position_params;
        Ok(crate::features::call_hierarchy::prepare(
            &self.dispatcher,
            &p.text_document.uri,
            p.position,
        )
        .await)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(crate::features::call_hierarchy::incoming(&self.dispatcher, &params.item).await)
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(crate::features::call_hierarchy::outgoing(&self.dispatcher, &params.item).await)
    }

    // ── Type Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        let p = params.text_document_position_params;
        Ok(Some(
            crate::features::type_hierarchy::prepare(
                &self.dispatcher,
                &p.text_document.uri,
                p.position,
            )
            .await,
        ))
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        use crate::features::type_hierarchy::{resolve_items, Direction};
        Ok(Some(
            resolve_items(&self.dispatcher, &params.item, Direction::Parents).await,
        ))
    }

    async fn subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        use crate::features::type_hierarchy::{resolve_items, Direction};
        Ok(Some(
            resolve_items(&self.dispatcher, &params.item, Direction::Children).await,
        ))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<Value>> {
        match params.command.as_str() {
            "java.edit.smartSemicolonDetection" => {
                if !crate::features::preferences::get_bool("java.edit.smartSemicolonDetection.enabled")
                    .unwrap_or(false)
                {
                    return Ok(None);
                }
                let request = params
                    .arguments
                    .first()
                    .and_then(json_model)
                    .and_then(|v| serde_json::from_value(v).ok());
                let Some(request) = request else {
                    return Ok(None);
                };
                Ok(crate::features::smart_detection::handle(&self.dispatcher, request)
                    .await
                    .map(|location| serde_json::to_value(location).expect("serializable smart location")))
            }
            "java.edit.handlePasteEvent" => {
                let request = params
                    .arguments
                    .first()
                    .and_then(json_model)
                    .and_then(|v| serde_json::from_value(v).ok());
                let Some(request) = request else {
                    return Err(tower_lsp::jsonrpc::Error::invalid_params("Invalid paste event"));
                };
                Ok(crate::features::paste::handle(&self.dispatcher, request)
                    .await
                    .map(|edit| serde_json::to_value(edit).expect("serializable paste edit")))
            }
            "java.project.resolveText" => {
                let path = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let content = params
                    .arguments
                    .get(1)
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                Ok(crate::features::paste::file_paste(&ws, path, content).map(Value::String))
            }
            "java.completion.onDidSelect" => {
                let request_id = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        crate::features::completion::protocol_error(
                            "Cannot get completion responses.",
                        )
                    })?;
                let proposal_id =
                    params
                        .arguments
                        .get(1)
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            crate::features::completion::protocol_error(
                                "Cannot get completion responses.",
                            )
                        })?;
                if let Some(env) = crate::features::completion::env() {
                    crate::features::completion::handler::on_did_select(
                        &env,
                        request_id,
                        proposal_id,
                    )
                    .await?;
                }
                Ok(Some(json!({})))
            }
            "java.project.refreshDiagnostics" if !self.legacy_diagnostics().await => {
                // (uri, scope, syntaxOnly), each possibly JSON-encoded.
                let arg = |i: usize| -> Option<Value> {
                    let v = params.arguments.get(i)?;
                    match v {
                        Value::String(s) => {
                            Some(serde_json::from_str(s).unwrap_or_else(|_| v.clone()))
                        }
                        other => Some(other.clone()),
                    }
                };
                let uri = arg(0).and_then(|v| v.as_str().map(str::to_owned));
                let scope = arg(1).and_then(|v| v.as_str().map(str::to_owned));
                let syntax_only = arg(2).and_then(|v| v.as_bool()).unwrap_or(false);
                self.lifecycle
                    .refresh_diagnostics(uri.as_deref(), scope.as_deref(), syntax_only)
                    .await;
                Ok(None)
            }
            "jdtls-rust.refreshDiagnostics" | "java.project.rebuild"
                if !self.legacy_diagnostics().await =>
            {
                self.lifecycle.build(None).await;
                Ok(None)
            }
            "jdtls-rust.refreshDiagnostics"
            | "java.project.refreshDiagnostics"
            | "java.project.rebuild" => {
                if self.dispatcher.is_ecj_ready().await {
                    self.publish_diagnostics_for_all().await;
                } else {
                    let next = (*self.compile_tx.borrow()).wrapping_add(1);
                    let _ = self.compile_tx.send(next);
                }
                Ok(None)
            }
            "jdtls-rust.classFileUri" => {
                // Test support: `ClassFileUtil.getURI(project, fqn)`.
                let arg = |i: usize| {
                    params
                        .arguments
                        .get(i)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                };
                let uri = navigation::type_uri(&self.dispatcher, &arg(0), &arg(1)).await;
                Ok(Some(uri.map(Value::String).unwrap_or(Value::Null)))
            }
            "java.project.getAll" => {
                // jdt.ls `ProjectCommand.getAllJavaProjects` / `getAllProjects`
                // (`{"includeNonJava": true}`): `File.toURI()` of every
                // project's real folder, in workspace (name) order.
                let include_non_java = params
                    .arguments
                    .first()
                    .and_then(json_model)
                    .and_then(|v| v.get("includeNonJava").and_then(Value::as_bool))
                    .unwrap_or(false);
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                Ok(Some(crate::features::project_commands::get_all(
                    &ws,
                    include_non_java,
                )))
            }
            "java.project.getClasspaths" => {
                let uri = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let scope = params
                    .arguments
                    .get(1)
                    .and_then(json_model)
                    .and_then(|v| v.get("scope").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_else(|| "runtime".to_owned());
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                crate::features::project_commands::get_classpaths(&ws, &uri, &scope)
                    .map(Some)
                    .map_err(internal_error)
            }
            "java.project.changeImportedProjects" => {
                // `ProjectCommand.changeImportedProjects(args[0], args[1], args[2])`
                // forwards to `ProjectsManager.changeImportedProjects(toImport,
                // toUpdate, toDelete)`.
                let list = |i: usize| -> Vec<String> {
                    params
                        .arguments
                        .get(i)
                        .and_then(json_model)
                        .and_then(|v| v.as_array().cloned())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                self.change_imported_projects(&list(0), &list(1), &list(2))
                    .await;
                Ok(None)
            }
            "java.project.import" => {
                // `ProjectsManager.importProjects`: scan the root paths again.
                self.config.write().await.project_configurations = None;
                self.reimport_workspace().await;
                self.request_compile();
                Ok(None)
            }
            "java.project.resolveSourceAttachment" => {
                let request = params.arguments.first().and_then(json_model);
                let class_file = request
                    .as_ref()
                    .and_then(|r| r.get("classFileUri"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                Ok(Some(
                    crate::features::project_commands::resolve_source_attachment(
                        &ws,
                        class_file.as_deref(),
                    ),
                ))
            }
            "java.project.isTestFile" => {
                let uri = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                crate::features::project_commands::is_test_file(&ws, &uri)
                    .map(|b| Some(Value::Bool(b)))
                    .map_err(internal_error)
            }
            "java.project.listSourcePaths" => {
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let cfg = self.config.read().await.clone();
                let env = crate::features::project_commands::Env {
                    ws: &ws,
                    vm_home: vm_home(&cfg),
                    root_paths: &cfg.root_paths,
                };
                Ok(Some(crate::features::project_commands::list_source_paths(
                    &env,
                )))
            }
            "java.edit.stringFormatting" => {
                // (content, options map or null, version)
                let args = &params.arguments;
                let content = args.first().and_then(Value::as_str).unwrap_or_default();
                let options = args.get(1).and_then(Value::as_object).map(|m| {
                    m.iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                v.as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| v.to_string()),
                            )
                        })
                        .collect()
                });
                let version = args.get(2).and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse().ok())
                        .or(v.as_i64().map(|n| n as i32))
                });
                let Some(version) = version else {
                    return Err(tower_lsp::jsonrpc::Error::invalid_params(
                        "version must be an int",
                    ));
                };
                let env = self.format_env().await;
                Ok(Some(Value::String(
                    formatting::string_formatting(&env, content, options, version).await,
                )))
            }
            "java.project.getSettings" => {
                // `ProjectCommand.getProjectSettings(uri, keys)`.
                let uri = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let keys: Vec<String> = params
                    .arguments
                    .get(1)
                    .and_then(json_model)
                    .and_then(|v| v.as_array().cloned())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let ws = self
                    .dispatcher
                    .workspace
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let cfg = self.config.read().await.clone();
                let vm = vm_home(&cfg);
                let vm_version = vm.as_deref().and_then(crate::project::vm_version);
                let formatter =
                    formatting::options::workspace_formatter_options(&cfg.format, &cfg.root_paths);
                let env = crate::features::project_commands::Env {
                    ws: &ws,
                    vm_home: vm,
                    root_paths: &cfg.root_paths,
                };
                let option = |p: &crate::project::Project, key: &str| -> Option<String> {
                    if let Some(v) = p.options.get(key) {
                        return Some(v.clone());
                    }
                    if let Some(v) = cfg.compiler_options.get(key) {
                        return Some(v.clone());
                    }
                    if let Some(v) = formatter.get(key) {
                        return Some(v.clone());
                    }
                    crate::project::effective_option(p, key, vm_version.as_deref())
                };
                crate::features::project_commands::get_settings(&env, &uri, &keys, option)
                    .map(Some)
                    .map_err(internal_error)
            }
            "java.navigate.openTypeHierarchy" => {
                Ok(crate::features::type_hierarchy::open_type_hierarchy(
                    &self.dispatcher,
                    &params.arguments,
                )
                .await)
            }
            "java.navigate.resolveTypeHierarchy" => {
                Ok(crate::features::type_hierarchy::resolve_type_hierarchy(
                    &self.dispatcher,
                    &params.arguments,
                )
                .await)
            }
            other => {
                // `WorkspaceExecuteCommandHandler.executeCommand`.
                warn!("Unsupported workspace/executeCommand request: {other}");
                Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::MethodNotFound,
                    message: format!("No delegateCommandHandler for {other}").into(),
                    data: None,
                })
            }
        }
    }
}

fn bridge_type_hierarchy_item_to_lsp(item: &BridgeTypeHierarchyItem) -> TypeHierarchyItem {
    let uri = Url::parse(&item.uri).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap());
    TypeHierarchyItem {
        name: item.name.clone(),
        kind: match item.kind {
            10 => SymbolKind::ENUM,
            11 => SymbolKind::INTERFACE,
            _ => SymbolKind::CLASS,
        },
        tags: None,
        detail: item.detail.clone(),
        uri,
        range: Range {
            start: Position {
                line: item.start_line,
                character: item.start_char,
            },
            end: Position {
                line: item.end_line,
                character: item.end_char,
            },
        },
        selection_range: Range {
            start: Position {
                line: item.sel_start_line,
                character: item.sel_start_char,
            },
            end: Position {
                line: item.sel_end_line,
                character: item.sel_end_char,
            },
        },
        data: item
            .data
            .as_ref()
            .map(|s| serde_json::Value::String(s.clone())),
    }
}

fn bridge_call_hierarchy_item_to_lsp(item: &BridgeCallHierarchyItem) -> CallHierarchyItem {
    let uri = Url::parse(&item.uri).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap());
    CallHierarchyItem {
        name: item.name.clone(),
        kind: match item.kind {
            9 => SymbolKind::CONSTRUCTOR,
            5 => SymbolKind::CLASS,
            _ => SymbolKind::METHOD,
        },
        tags: None,
        detail: item.detail.clone(),
        uri,
        range: Range {
            start: Position {
                line: item.start_line,
                character: item.start_char,
            },
            end: Position {
                line: item.end_line,
                character: item.end_char,
            },
        },
        selection_range: Range {
            start: Position {
                line: item.sel_start_line,
                character: item.sel_start_char,
            },
            end: Position {
                line: item.sel_end_line,
                character: item.sel_end_char,
            },
        },
        data: None,
    }
}

fn flatten_workspace_symbols(
    out: &mut Vec<SymbolInformation>,
    uri: &Url,
    container_name: Option<&str>,
    document_symbols: &[DocumentSymbol],
    query: &str,
) {
    for symbol in document_symbols {
        if query.is_empty() || symbol.name.to_ascii_lowercase().contains(query) {
            #[allow(deprecated)]
            out.push(SymbolInformation {
                name: symbol.name.clone(),
                kind: symbol.kind,
                tags: symbol.tags.clone(),
                deprecated: None,
                location: Location {
                    uri: uri.clone(),
                    range: symbol.selection_range,
                },
                container_name: container_name.map(str::to_owned),
            });
        }

        if let Some(children) = symbol.children.as_ref() {
            flatten_workspace_symbols(out, uri, Some(&symbol.name), children, query);
        }
    }
}

fn java_file_operation_registration_options() -> FileOperationRegistrationOptions {
    FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: Some("file".to_owned()),
            pattern: FileOperationPattern {
                glob: "**/*.java".to_owned(),
                matches: Some(FileOperationPatternKind::File),
                options: None,
            },
        }],
    }
}

fn is_build_descriptor(name: &str) -> bool {
    matches!(
        name,
        "pom.xml"
            | ".classpath"
            | ".project"
            | "build.gradle"
            | "settings.gradle"
            | "build.gradle.kts"
            | "settings.gradle.kts"
            | "org.eclipse.jdt.core.prefs"
    )
}

/// `JSONUtility.toModel`: a JSON value, or a JSON-encoded string.
fn json_model(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) => serde_json::from_str(s).ok(),
        Value::Null => None,
        other => Some(other.clone()),
    }
}

/// A `CoreException` thrown from a delegate command.
fn internal_error(message: String) -> tower_lsp::jsonrpc::Error {
    tower_lsp::jsonrpc::Error {
        code: tower_lsp::jsonrpc::ErrorCode::InternalError,
        message: message.into(),
        data: None,
    }
}

/// The jdt.ls workspace directory (`-data`), or a per-process temporary one.
pub(crate) fn data_dir() -> std::path::PathBuf {
    crate::config::DATA_DIR.get().cloned().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("jdtls-rust-workspace-{}", std::process::id()))
    })
}

/// The default VM's home (`JavaRuntime.getDefaultVMInstall()`).
pub(crate) fn vm_home(cfg: &Config) -> Option<std::path::PathBuf> {
    let from = |h: &str| {
        let p = std::path::PathBuf::from(h);
        p.join("bin")
            .join("java")
            .is_file()
            .then(|| crate::project::canonicalize_lenient(&p))
    };
    cfg.java_home
        .as_deref()
        .and_then(from)
        .or_else(|| std::env::var("JAVA_HOME").ok().as_deref().and_then(from))
        .or_else(|| {
            let out = std::process::Command::new("/usr/libexec/java_home")
                .output()
                .ok()?;
            from(String::from_utf8_lossy(&out.stdout).trim())
        })
}

/// Project import settings: the jdt.ls `settings` plus the initialization
/// options that drive import (`triggerFiles`, `projectConfigurations`).
fn import_settings(cfg: &Config) -> crate::project::ImportSettings {
    let mut s = crate::project::ImportSettings::from_settings(cfg.settings.as_ref());
    let to_paths = |uris: &[String]| -> Vec<std::path::PathBuf> {
        uris.iter()
            .filter_map(|u| Url::parse(u).ok())
            .filter_map(|u| u.to_file_path().ok())
            .map(|p| crate::project::canonicalize_lenient(&p))
            .collect()
    };
    s.trigger_files = cfg
        .trigger_files
        .as_deref()
        .map(to_paths)
        .unwrap_or_default();
    s.project_configurations = cfg.project_configurations.as_deref().map(to_paths);
    s.data_dir = Some(data_dir());
    s.vm_home = vm_home(cfg);
    s.vm_version = s.vm_home.as_deref().and_then(crate::project::vm_version);
    s
}

fn merge_config_settings(config: &mut Config, settings: &Value) -> bool {
    let mut restart_ecj = false;
    config.inlay_hints.update_from(settings);
    config.settings = Some(settings.clone());

    let updated_java_home = setting_string(settings, &["javaHome"])
        .or_else(|| setting_string(settings, &["java", "javaHome"]))
        .or_else(|| setting_string(settings, &["java", "home"]))
        .or_else(|| setting_string(settings, &["java", "jdt", "ls", "java", "home"]));
    if let Some(java_home) = updated_java_home {
        if config.java_home.as_deref() != Some(java_home.as_str()) {
            config.java_home = Some(java_home);
            restart_ecj = true;
        }
    }

    if let Some(source_compatibility) = setting_string(settings, &["sourceCompatibility"])
        .or_else(|| setting_string(settings, &["java", "sourceCompatibility"]))
    {
        config.source_compatibility = source_compatibility;
    }

    if let Some(classpath) = setting_string_array(settings, &["classpath"])
        .or_else(|| setting_string_array(settings, &["java", "classpath"]))
    {
        config.classpath = classpath;
    }

    config.format.update_from(settings);

    if let Some(max_completions) = setting_usize(settings, &["maxCompletions"])
        .or_else(|| setting_usize(settings, &["java", "maxCompletions"]))
    {
        config.max_completions = max_completions;
    }

    *config = config.clone().with_defaults();
    restart_ecj
}

fn completion_markdown(caps: &ClientCapabilities) -> bool {
    caps.text_document
        .as_ref()
        .and_then(|t| t.completion.as_ref())
        .and_then(|c| c.completion_item.as_ref())
        .and_then(|i| i.documentation_format.as_ref())
        .is_some_and(|f| f.contains(&MarkupKind::Markdown))
}

fn setting_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn setting_string(value: &Value, path: &[&str]) -> Option<String> {
    setting_value(value, path)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn setting_string_array(value: &Value, path: &[&str]) -> Option<Vec<String>> {
    let arr = setting_value(value, path)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    )
}

fn setting_usize(value: &Value, path: &[&str]) -> Option<usize> {
    setting_value(value, path)?.as_u64().map(|n| n as usize)
}

fn open_java_type_targets(store: &DocumentStore) -> HashMap<String, Url> {
    let mut targets = HashMap::new();
    for state in store.snapshots() {
        let package = parse_package_name(&state.content_string());
        let Some(tree) = state.tree.as_ref() else {
            continue;
        };
        let content = state.content_string();
        let symbols = outline::document_symbols(tree, &content);
        let Some(symbol) = symbols.iter().find(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT
            )
        }) else {
            continue;
        };

        let fqn = if package.is_empty() {
            symbol.name.clone()
        } else {
            format!("{package}.{}", symbol.name)
        };
        targets.insert(fqn, state.uri);
    }
    targets
}

fn parse_package_name(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            return rest.trim_end_matches(';').trim().to_owned();
        }
        if !trimmed.is_empty() && !trimmed.starts_with("//") {
            break;
        }
    }
    String::new()
}

fn import_document_links(content: &str, type_targets: &HashMap<String, Url>) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import ") else {
            continue;
        };
        if rest.starts_with("static ") {
            continue;
        }
        let imported = rest.trim_end_matches(';').trim();
        if imported.is_empty() || imported.ends_with(".*") {
            continue;
        }
        let Some(target) = type_targets.get(imported) else {
            continue;
        };
        let Some(start_byte) = line.find(imported) else {
            continue;
        };
        let end_byte = start_byte + imported.len();
        let start_char = utf16_len(&line[..start_byte]) as u32;
        let end_char = utf16_len(&line[..end_byte]) as u32;
        links.push(DocumentLink {
            range: Range {
                start: Position {
                    line: line_index as u32,
                    character: start_char,
                },
                end: Position {
                    line: line_index as u32,
                    character: end_char,
                },
            },
            target: Some(target.clone()),
            tooltip: Some("Open imported type".to_owned()),
            data: None,
        });
    }

    links
}

fn external_url_links(content: &str) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let mut search_from = 0usize;
        while let Some(relative_start) = line[search_from..]
            .find("https://")
            .or_else(|| line[search_from..].find("http://"))
        {
            let start = search_from + relative_start;
            let end = line[start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '}'))
                .map(|offset| start + offset)
                .unwrap_or(line.len());
            let candidate = &line[start..end];
            if let Ok(target) = Url::parse(candidate) {
                let start_char = utf16_len(&line[..start]) as u32;
                let end_char = utf16_len(&line[..end]) as u32;
                links.push(DocumentLink {
                    range: Range {
                        start: Position {
                            line: line_index as u32,
                            character: start_char,
                        },
                        end: Position {
                            line: line_index as u32,
                            character: end_char,
                        },
                    },
                    target: Some(target),
                    tooltip: None,
                    data: None,
                });
            }
            search_from = end.max(start + 1);
        }
    }

    links
}

fn identifier_range_and_text_at(content: &str, pos: Position) -> Option<(Range, String)> {
    let line_index = pos.line as usize;
    let line = content.lines().nth(line_index)?;
    let mut start = utf16_col_to_byte(line, pos.character as usize).min(line.len());

    while start > 0 {
        let ch = line[..start].chars().next_back()?;
        if is_java_ident_part(ch) {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }

    let mut end = utf16_col_to_byte(line, pos.character as usize).min(line.len());
    if end == start {
        let ch = line[end..].chars().next()?;
        if !is_java_ident_part(ch) {
            return None;
        }
        end += ch.len_utf8();
    }
    while end < line.len() {
        let Some(ch) = line[end..].chars().next() else {
            break;
        };
        if !is_java_ident_part(ch) {
            break;
        }
        end += ch.len_utf8();
    }

    if start >= end {
        return None;
    }

    let text = line[start..end].to_owned();
    let start_char = utf16_len(&line[..start]) as u32;
    let end_char = utf16_len(&line[..end]) as u32;
    Some((
        Range {
            start: Position {
                line: pos.line,
                character: start_char,
            },
            end: Position {
                line: pos.line,
                character: end_char,
            },
        },
        text,
    ))
}

fn pos_to_offset_from_text(content: &str, pos: Position) -> Option<usize> {
    let mut offset = 0usize;
    for (index, line) in content.lines().enumerate() {
        if index == pos.line as usize {
            return Some(offset + utf16_col_to_byte(line, pos.character as usize).min(line.len()));
        }
        offset += line.len() + 1;
    }
    None
}

fn is_java_keyword(text: &str) -> bool {
    matches!(
        text,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "record"
            | "sealed"
            | "permits"
            | "var"
    )
}

/// Convert a UTF-16 column offset (as used in LSP positions) to a UTF-8 byte offset.
fn utf16_col_to_byte(s: &str, utf16_col: usize) -> usize {
    let mut units = 0usize;
    for (byte_pos, ch) in s.char_indices() {
        if units >= utf16_col {
            return byte_pos;
        }
        units += ch.len_utf16();
    }
    s.len()
}

/// Heuristic freshness check for completion requests.
///
/// Besides ensuring the line is long enough for the cursor, this also catches
/// the common stale-content case where the user typed immediately before an
/// existing delimiter (`;`, `)`, `,`, …). In that case the stored line can
/// still be long enough while missing the newly-typed identifier or trigger
/// character.
fn completion_store_is_fresh(
    content: &str,
    pos: tower_lsp::lsp_types::Position,
    trigger_char: Option<&str>,
) -> bool {
    let line = pos.line as usize;
    let Some(line_text) = content.lines().nth(line) else {
        return false;
    };
    let line_utf16_len: usize = line_text.chars().map(char::len_utf16).sum();
    if pos.character as usize > line_utf16_len {
        return false;
    }
    let byte_col = utf16_col_to_byte(line_text, pos.character as usize);
    if byte_col > line_text.len() {
        return false;
    }

    let before = &line_text[..byte_col];
    if let Some(tc) = trigger_char {
        return before.ends_with(tc);
    }

    if byte_col == 0 || byte_col == line_text.len() {
        return true;
    }

    let prev = before.chars().next_back();
    let next = line_text[byte_col..].chars().next();
    if trigger_char.is_none() && next.is_some_and(is_java_ident_part) {
        return false;
    }
    !matches!(
        (prev, next),
        (Some(prev), Some(next))
            if (!is_java_ident_part(prev) && is_completion_boundary(next))
                || (is_completion_boundary(prev) && next.is_whitespace())
    )
}

fn attach_completion_text_edit(
    item: &mut CompletionItem,
    replacement_range: Option<tower_lsp::lsp_types::Range>,
) {
    if item.text_edit.is_some() {
        return;
    }

    let Some(range) = replacement_range else {
        return;
    };

    let new_text = item
        .insert_text
        .clone()
        .unwrap_or_else(|| item.label.clone());
    item.text_edit = Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(
        tower_lsp::lsp_types::TextEdit { range, new_text },
    ));
}

fn detect_client_flavor(client_info: Option<&ClientInfo>) -> ClientFlavor {
    match client_info.map(|info| info.name.as_str()) {
        Some("lms-monaco") => ClientFlavor::LmsMonaco,
        _ => ClientFlavor::Default,
    }
}

fn code_lens_command(
    client_flavor: ClientFlavor,
    command: &str,
    title: &str,
    args: Option<Vec<Value>>,
    range: Range,
) -> Command {
    let (command, arguments) = match client_flavor {
        ClientFlavor::LmsMonaco if command == "editor.action.showReferences" => (
            "java.show.references".to_owned(),
            Some(show_references_args_for_lms_monaco(args, range)),
        ),
        _ => (command.to_owned(), args),
    };

    Command {
        title: title.to_owned(),
        command,
        arguments,
    }
}

fn show_references_args_for_lms_monaco(args: Option<Vec<Value>>, range: Range) -> Vec<Value> {
    let Some(args) = args else {
        return vec![
            Value::String(String::new()),
            json!({ "line": range.start.line, "character": range.start.character }),
            Value::Array(Vec::new()),
        ];
    };

    let uri = args
        .first()
        .and_then(uri_string_from_monaco_arg)
        .map(Value::String)
        .unwrap_or_else(|| Value::String(String::new()));
    let position = args
        .get(1)
        .and_then(position_from_show_references_arg)
        .unwrap_or_else(|| json!({ "line": range.start.line, "character": range.start.character }));
    let references = args
        .get(2)
        .and_then(|v| v.as_array())
        .map(|refs| {
            refs.iter()
                .filter_map(location_from_show_references_arg)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    vec![uri, position, Value::Array(references)]
}

fn position_from_show_references_arg(value: &Value) -> Option<Value> {
    if let Some(obj) = value.as_object() {
        if obj.get("line").and_then(Value::as_u64).is_some()
            && obj.get("character").and_then(Value::as_u64).is_some()
        {
            return Some(json!({
                "line": obj.get("line")?.as_u64()? as u32,
                "character": obj.get("character")?.as_u64()? as u32,
            }));
        }

        let line = obj.get("lineNumber").and_then(Value::as_u64)?;
        let column = obj.get("column").and_then(Value::as_u64)?;
        return Some(json!({
            "line": line as u32 - 1,
            "character": column as u32 - 1,
        }));
    }

    None
}

fn location_from_show_references_arg(value: &Value) -> Option<Value> {
    location_from_lsp_arg(value).or_else(|| location_from_monaco_arg(value))
}

fn uri_string_from_monaco_arg(value: &Value) -> Option<String> {
    if let Some(uri) = value.as_str() {
        return Some(uri.to_owned());
    }

    let obj = value.as_object()?;
    let scheme = obj.get("scheme")?.as_str()?;
    let authority = obj.get("authority").and_then(Value::as_str).unwrap_or("");
    let path = obj.get("path").and_then(Value::as_str).unwrap_or("");
    let query = obj.get("query").and_then(Value::as_str).unwrap_or("");
    let fragment = obj.get("fragment").and_then(Value::as_str).unwrap_or("");

    let mut uri = format!("{scheme}://{authority}{path}");
    if !query.is_empty() {
        uri.push('?');
        uri.push_str(query);
    }
    if !fragment.is_empty() {
        uri.push('#');
        uri.push_str(fragment);
    }
    Some(uri)
}

fn location_from_lsp_arg(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let uri = uri_string_from_monaco_arg(obj.get("uri")?)?;
    let range = obj.get("range")?.as_object()?;
    let start = range.get("start")?.as_object()?;
    let end = range.get("end")?.as_object()?;

    Some(json!({
        "uri": uri,
        "range": {
            "start": {
                "line": start.get("line")?.as_u64()? as u32,
                "character": start.get("character")?.as_u64()? as u32,
            },
            "end": {
                "line": end.get("line")?.as_u64()? as u32,
                "character": end.get("character")?.as_u64()? as u32,
            }
        }
    }))
}

fn location_from_monaco_arg(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let uri = uri_string_from_monaco_arg(obj.get("uri")?)?;
    let range = obj.get("range")?.as_object()?;

    Some(json!({
        "uri": uri,
        "range": {
            "start": {
                "line": range.get("startLineNumber")?.as_u64()? as u32 - 1,
                "character": range.get("startColumn")?.as_u64()? as u32 - 1,
            },
            "end": {
                "line": range.get("endLineNumber")?.as_u64()? as u32 - 1,
                "character": range.get("endColumn")?.as_u64()? as u32 - 1,
            }
        }
    }))
}

/// Returns the LSP Range covering the Java identifier immediately before the cursor.
/// This is attached as `textEdit` on completion items so Monaco-based clients
/// can reliably replace the current token and apply any `additionalTextEdits`.
fn word_range_at(
    content: &str,
    pos: tower_lsp::lsp_types::Position,
) -> Option<tower_lsp::lsp_types::Range> {
    let line = pos.line as usize;
    let line_text = content.lines().nth(line)?;
    let col_bytes = utf16_col_to_byte(line_text, pos.character as usize);
    let col_bytes = col_bytes.min(line_text.len());
    let text_before = &line_text[..col_bytes];

    // Walk back to find the start of the identifier
    let word_start_bytes = text_before
        .char_indices()
        .rev()
        .find_map(|(i, c)| {
            if !c.is_alphanumeric() && c != '_' {
                Some(i + c.len_utf8())
            } else {
                None
            }
        })
        .unwrap_or(0);

    let word_start_col = utf16_len(&line_text[..word_start_bytes]) as u32;
    Some(tower_lsp::lsp_types::Range {
        start: tower_lsp::lsp_types::Position {
            line: pos.line,
            character: word_start_col,
        },
        end: pos,
    })
}

/// Compute the UTF-16 length of a UTF-8 string slice.
fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

fn is_java_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn is_completion_boundary(c: char) -> bool {
    matches!(c, ';' | ')' | ',' | ']' | '}' | '\n' | '\r')
}

fn is_member_access_context(content: &str, offset: usize) -> bool {
    let bytes = content.as_bytes();
    let mut i = offset.min(bytes.len());
    while i > 0 && matches!(bytes[i - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$') {
        i -= 1;
    }
    i > 0 && bytes[i - 1] == b'.'
}

fn is_after_numeric_literal_dot(content: &str, offset: usize) -> bool {
    let end = offset.min(content.len());
    if end == 0 || content.as_bytes()[end - 1] != b'.' {
        return false;
    }

    let mut start = end - 1;
    while start > 0 {
        let ch = content[..start].chars().next_back().unwrap();
        if ch.is_ascii_alphanumeric() || ch == '_' {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }

    let token = &content[start..end - 1];
    if token.is_empty() {
        return false;
    }

    is_numeric_literal_token(token)
}

fn is_numeric_literal_token(token: &str) -> bool {
    let stripped = token
        .strip_suffix(['l', 'L', 'f', 'F', 'd', 'D'])
        .unwrap_or(token);
    if let Some(rest) = stripped
        .strip_prefix("0x")
        .or_else(|| stripped.strip_prefix("0X"))
    {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    }
    if let Some(rest) = stripped
        .strip_prefix("0b")
        .or_else(|| stripped.strip_prefix("0B"))
    {
        return !rest.is_empty() && rest.chars().all(|c| matches!(c, '0' | '1' | '_'));
    }
    !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit() || c == '_')
}

/// Returns true if the cursor is in an expression position: after `=`, `(`, `,`,
/// arithmetic/bitwise operators, or the `return` keyword.  In these positions
/// statement-level snippets (for/while/class/abstract/…) are not valid and
/// should be suppressed.
fn is_expression_context(content: &str, offset: usize) -> bool {
    let bytes = content.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = offset.min(bytes.len()).saturating_sub(1);
    // Skip whitespace backwards
    while i > 0 && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i -= 1;
    }
    match bytes[i] {
        b'=' | b'+' | b'-' | b'*' | b'/' | b'%' | b'|' | b'&' | b'^' | b'(' | b',' => true,
        _ => {
            // Check if last non-whitespace token is the `return` keyword
            let prefix = &content[..=i];
            let trimmed = prefix.trim_end();
            trimmed.ends_with("return")
                && trimmed
                    .as_bytes()
                    .get(trimmed.len().wrapping_sub(7))
                    .map_or(true, |&b| !b.is_ascii_alphanumeric() && b != b'_')
        }
    }
}

/// Detect if the cursor is inside a Java import statement and return the typed prefix.
///
/// Uses the LSP line number directly rather than computing backward from a byte offset,
/// so it is robust against the didChange/completion race: when the trigger character
/// (e.g. '.') has not yet been applied to the stored content, `col` will exceed the
/// stored line length — in that case we use the full stored line and then append the
/// trigger character.
///
/// Returns `Some("java.")` for `import java.|`, `Some("java.util.")` for
/// `import java.util.|`, etc., or `None` if not in an import context.
pub fn detect_import_prefix(
    content: &str,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
) -> Option<String> {
    let line = line as usize;
    let col = col as usize;

    // Find the line in the stored content using the LSP line number.
    // This avoids the off-by-one that occurs when computing backward from `offset`
    // and the stored content is one character shorter than the real content.
    let stored_line = content.lines().nth(line).unwrap_or("");

    // `col` is a UTF-16 code-unit offset (LSP spec).  Convert to a UTF-8 byte
    // offset before slicing so we never land in the middle of a multi-byte char.
    let byte_col = utf16_col_to_byte(stored_line, col);
    let line_up_to_col = &stored_line[..byte_col];

    // Append the trigger character if it isn't already present at the end.
    let effective: std::borrow::Cow<str> = match trigger_char {
        Some(tc) if !line_up_to_col.ends_with(tc) => {
            std::borrow::Cow::Owned(format!("{line_up_to_col}{tc}"))
        }
        _ => std::borrow::Cow::Borrowed(line_up_to_col),
    };

    let trimmed = effective.trim_start();
    let rest = trimmed.strip_prefix("import ")?;
    // Handle static imports: `import static java.util.Arrays.`
    let rest = rest.strip_prefix("static ").unwrap_or(rest);
    // Only accept word characters and dots (no semicolons, spaces, etc.)
    if rest
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        Some(rest.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{completion_store_is_fresh, detect_import_prefix, is_after_numeric_literal_dot};
    use tower_lsp::lsp_types::Position;

    /// Port of `InitHandlerTest.testJavaImportExclusions`.
    #[test]
    fn test_java_import_exclusions() {
        let initialization_options = serde_json::json!({ "settings": { "java": { "import": { "exclusions": ["**/test/**"] } } } });
        let prefs =
            crate::project::ImportSettings::from_settings(initialization_options.get("settings"));
        assert_eq!("**/test/**", prefs.exclusions[0]);
    }

    // ── Normal (non-stale) cases ──────────────────────────────────────────────

    #[test]
    fn detects_top_level_package() {
        let src = "import java\n";
        assert_eq!(detect_import_prefix(src, 0, 11, None), Some("java".into()));
    }

    #[test]
    fn detects_after_dot_in_stored_content() {
        // Content already has the dot (no race).
        let src = "import java.\n";
        assert_eq!(detect_import_prefix(src, 0, 12, None), Some("java.".into()));
    }

    #[test]
    fn detects_partial_class_name() {
        let src = "import java.util.Arr\n";
        assert_eq!(
            detect_import_prefix(src, 0, 20, None),
            Some("java.util.Arr".into())
        );
    }

    #[test]
    fn detects_static_import() {
        let src = "import static java.util.\n";
        assert_eq!(
            detect_import_prefix(src, 0, 24, None),
            Some("java.util.".into())
        );
    }

    #[test]
    fn returns_none_outside_import() {
        let src = "public class Foo {\n";
        assert_eq!(detect_import_prefix(src, 0, 10, None), None);
    }

    #[test]
    fn returns_none_for_complete_import_with_semicolon() {
        // Semicolon means the import is already finished — not a completion context.
        let src = "import java.util.ArrayList;\n";
        assert_eq!(detect_import_prefix(src, 0, 27, None), None);
    }

    // ── Race-condition cases: trigger char not yet in stored content ──────────

    #[test]
    fn detects_dot_trigger_when_dot_not_in_store() {
        // Stored content has "import java" (no dot), col=12 is past the stored line.
        // Trigger char "." should be appended automatically.
        let src = "import java.util.ArrayList;\nimport java.util.List;\nimport java\npublic class Foo {}\n";
        // Line 2 (0-indexed) is "import java", col 12 is one past the end.
        assert_eq!(
            detect_import_prefix(src, 2, 12, Some(".")),
            Some("java.".into())
        );
    }

    #[test]
    fn detects_dot_trigger_mid_package_not_in_store() {
        // "import java.util" in store, dot trigger adds "."
        let src = "import java.util\n";
        assert_eq!(
            detect_import_prefix(src, 0, 17, Some(".")),
            Some("java.util.".into())
        );
    }

    #[test]
    fn trigger_char_not_appended_if_already_present() {
        // Dot already in content — don't double-append.
        let src = "import java.\n";
        assert_eq!(
            detect_import_prefix(src, 0, 12, Some(".")),
            Some("java.".into())
        );
    }

    #[test]
    fn dot_trigger_outside_import_stays_none() {
        // Typing "." on a non-import line must not produce a false positive.
        let src = "    System.out\n";
        assert_eq!(detect_import_prefix(src, 0, 14, Some(".")), None);
    }

    #[test]
    fn completion_wait_detects_stale_insert_before_semicolon() {
        let stale = "class T { void m() { int x = ; } }\n";
        assert!(!completion_store_is_fresh(
            stale,
            Position {
                line: 0,
                character: 30
            },
            None,
        ));
    }

    #[test]
    fn completion_wait_accepts_updated_insert_before_semicolon() {
        let fresh = "class T { void m() { int x = A; } }\n";
        assert!(completion_store_is_fresh(
            fresh,
            Position {
                line: 0,
                character: 30
            },
            None,
        ));
    }

    #[test]
    fn completion_wait_detects_stale_member_access_suffix() {
        let stale = "class T { void m() { value. } }\n";
        assert!(!completion_store_is_fresh(
            stale,
            Position {
                line: 0,
                character: 37
            },
            None,
        ));
    }

    #[test]
    fn completion_wait_detects_stale_edit_inside_existing_identifier() {
        let stale = "class T { public double test() { return 0.0; } }\n";
        assert!(!completion_store_is_fresh(
            stale,
            Position {
                line: 0,
                character: 18
            },
            None,
        ));
    }

    #[test]
    fn suppresses_completion_after_numeric_literal_dot() {
        assert!(is_after_numeric_literal_dot("return 0.", 9));
        assert!(is_after_numeric_literal_dot("return 123_456.", 15));
        assert!(is_after_numeric_literal_dot("return 0x1f.", 12));
        assert!(!is_after_numeric_literal_dot("return value.", 13));
        assert!(!is_after_numeric_literal_dot("return this.", 12));
    }
}
