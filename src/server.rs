//! tower-lsp LanguageServer implementation.

use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::code_action as ca_conv;
use crate::analysis::semantic::completion as comp_conv;
use crate::analysis::semantic::definition as def_conv;
use crate::analysis::semantic::diagnostics as diag_conv;
use crate::analysis::semantic::hover as hover_conv;
use crate::analysis::semantic::protocol::{BridgeCallHierarchyItem, BridgeTypeHierarchyItem, BridgeRange, BridgeResponse, BridgeDiagnostic};
use crate::analysis::semantic::NavKind;
use crate::analysis::syntax::{
    completion as syntax_completion, diagnostics as syntax_diagnostics, folding, outline,
    navigation as syntax_navigation, selection, snippets, tokens,
};
use crate::analysis::syntax::parser::JavaParser;
use crate::config::Config;
use crate::document_store::DocumentStore;
use crate::handlers::text_document::pos_to_offset;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    request::{
        GotoDeclarationParams, GotoDeclarationResponse,
        GotoTypeDefinitionParams, GotoTypeDefinitionResponse,
        GotoImplementationParams, GotoImplementationResponse,
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
    }
}

/// Collect tree-sitter and ECJ diagnostics for every open document and push
/// them to the client.  Shared by `spawn_compile_loop` and `publish_diagnostics_for_all`.
async fn publish_diagnostics(store: &DocumentStore, dispatcher: &Dispatcher, client: &Client) {
    let snapshots = store.snapshots();
    let mut by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();

    for state in &snapshots {
        let diags = state.tree.as_ref()
            .map(|t| syntax_diagnostics::collect(t))
            .unwrap_or_default();
        by_uri.insert(state.uri.clone(), diags);
    }

    match dispatcher.compile_all().await {
        Ok(BridgeResponse::Diagnostics { items, .. }) => {
            for item in &items {
                if let Some((uri, diag)) = diag_conv::to_lsp(item) {
                    by_uri.entry(uri).or_default().push(diag);
                }
            }
        }
        Ok(BridgeResponse::Error { message, .. }) => warn!("ECJ compile error: {message}"),
        Err(e) => error!("compile_all error: {e}"),
        _ => {}
    }

    // Ensure every open document gets an entry (clears stale diagnostics).
    for state in &snapshots {
        by_uri.entry(state.uri.clone()).or_default();
    }
    for (uri, diags) in by_uri {
        client.publish_diagnostics(uri, diags, None).await;
    }
}

pub struct JavaLanguageServer {
    client: Client,
    store: Arc<DocumentStore>,
    dispatcher: Arc<Dispatcher>,
    config: Arc<RwLock<Config>>,
    parser: Arc<Mutex<JavaParser>>,
    /// Sends a signal that source changed; background task debounces and compiles.
    compile_tx: watch::Sender<u64>,
}

impl JavaLanguageServer {
    pub fn new(client: Client) -> Self {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = Arc::new(DocumentStore::new());
        let dispatcher = Arc::new(Dispatcher::new(Arc::clone(&store), Arc::clone(&config)));

        let (compile_tx, _) = watch::channel(0u64);

        Self {
            client,
            store,
            dispatcher,
            config,
            parser: Arc::new(Mutex::new(JavaParser::new())),
            compile_tx,
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
                if rx.changed().await.is_err() { break; }

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

    // ── Utilities ─────────────────────────────────────────────────────────────

    /// Compile all open files and publish diagnostics to the client immediately.
    /// Used on demand (e.g. after a workspace-wide action); the background loop
    /// in `spawn_compile_loop` handles the normal debounced case.
    async fn publish_diagnostics_for_all(&self) {
        publish_diagnostics(&self.store, &self.dispatcher, &self.client).await;
    }

    fn dedupe_completion_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
        let mut seen = std::collections::HashSet::new();
        items.into_iter()
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
        // Parse initializationOptions
        if let Some(opts) = params.initialization_options {
            let cfg: Config = serde_json::from_value::<Config>(opts)
                .unwrap_or_default()
                .with_defaults();
            *self.config.write().await = cfg;
        } else {
            *self.config.write().await = Config::default().with_defaults();
        }

        // Start ecj-bridge in background, then kick the compile loop
        let dispatcher = Arc::clone(&self.dispatcher);
        let client = self.client.clone();
        let compile_tx = self.compile_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatcher.start_ecj().await {
                error!("Failed to start ecj-bridge: {e}");
                client.show_message(MessageType::ERROR,
                    format!("jdtls-rust: failed to start ecj-bridge: {e}")).await;
            } else {
                info!("ecj-bridge started");
                // Trigger an initial compile — use a fresh increment so the watch
                // always fires even if did_open already sent a signal earlier.
                let next = (*compile_tx.borrow()).wrapping_add(1);
                let _ = compile_tx.send(next);
            }
        });
        self.spawn_compile_loop();

        let token_legend = tokens::legend();

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
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".into(), "@".into(), "#".into(),
                    ]),
                    resolve_provider: Some(false),
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
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::from("quickassist"),
                            CodeActionKind::REFACTOR,
                            CodeActionKind::SOURCE,
                            CodeActionKind::from("source.generate.accessors"),
                        ]),
                        resolve_provider: Some(false),
                        work_done_progress_options: Default::default(),
                    },
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(false),
                    work_done_progress_options: Default::default(),
                })),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: token_legend,
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            work_done_progress_options: Default::default(),
                        },
                    ),
                ),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
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
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        {
            let mut parser = self.parser.lock().await;
            self.store.apply_changes(&uri, version, params.content_changes, &mut parser);
        }
        let next = (*self.compile_tx.borrow()).wrapping_add(1);
        let _ = self.compile_tx.send(next);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.store.close(&params.text_document.uri);
        // Clear diagnostics for the closed file
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
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

    // ── Completion ────────────────────────────────────────────────────────────

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
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
            let deadline = tokio::time::Instant::now()
                + std::time::Duration::from_millis(150);
            loop {
                let up_to_date = self.store.get(uri).map(|state| {
                    completion_store_is_fresh(&state.content_string(), pos, trigger_char)
                }).unwrap_or(true); // document not open yet → don't spin

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

        let mut items: Vec<CompletionItem> = Vec::new();

        if !in_import && !in_member_access {
            let prefix = syntax_completion::current_prefix(&content, offset);
            if let Some(tree) = tree.as_ref() {
                items.extend(syntax_completion::local_completions(tree, &content, offset));
                items.extend(syntax_completion::import_type_completions(tree, &content, &prefix));
            }
            if is_expression_context(&content, offset) {
                items.extend(snippets::expression_keywords());
            } else {
                let in_method = tree.as_ref()
                    .map(|t| syntax_completion::is_inside_method_body(t, offset))
                    .unwrap_or(false);
                items.extend(snippets::method_body_snippets());
                if !in_method {
                    items.extend(snippets::class_body_snippets());
                }
            }
        } else if in_member_access {
            // Syntax-level this. completion (ECJ will override with full semantic results)
            if let Some(tree) = tree.as_ref() {
                items.extend(syntax_completion::this_member_completions(tree, &content, offset));
            }
        }

        // Compute the word range at the cursor — needed so that completion items
        // with additionalTextEdits (auto-import) also carry an explicit textEdit.
        // Monaco only applies additionalTextEdits when the item has a textEdit.
        let word_range = word_range_at(&content, pos);

        // Semantic completions from ECJ
        let in_expr = is_expression_context(&content, offset);
        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.complete(uri, offset, import_prefix, content.clone()).await {
                Ok(BridgeResponse::Completions { items: bridge_items, .. }) => {
                    let semantic: Vec<CompletionItem> = bridge_items.iter().filter_map(|c| {
                        // In expression context (after `=`, `return`, etc.) void methods
                        // cannot produce a value — suppress them.
                        if in_expr
                            && c.kind == 2  // METHOD
                            && c.label.ends_with(": void")
                        {
                            return None;
                        }
                        let mut item = comp_conv::to_lsp(c);
                        // For items that carry auto-import edits, attach an explicit
                        // textEdit so Monaco applies the additionalTextEdits.
                        if item.additional_text_edits.as_ref().map_or(false, |e| !e.is_empty()) {
                            if let Some(range) = word_range {
                                let new_text = item.insert_text.clone().unwrap_or_else(|| item.label.clone());
                                item.text_edit = Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(
                                    tower_lsp::lsp_types::TextEdit {
                                        range,
                                        new_text,
                                    }
                                ));
                            }
                        }
                        Some(item)
                    }).collect();
                    if in_import {
                        items = semantic;
                    } else {
                        // Prepend semantic items so they sort first
                        items = semantic.into_iter().chain(items).collect();
                    }
                }
                Ok(BridgeResponse::Error { message, .. }) => {
                    warn!("completion ECJ error: {message}");
                }
                Err(e) => warn!("completion error: {e}"),
                _ => {}
            }
        }

        Ok(Some(CompletionResponse::Array(Self::dedupe_completion_items(items))))
    }

    // ── Hover ─────────────────────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.hover(uri, offset).await {
                Ok(BridgeResponse::Hover { contents, .. }) if !contents.is_empty() => {
                    return Ok(Some(hover_conv::to_lsp(&contents)));
                }
                Ok(BridgeResponse::Error { message, .. }) => {
                    warn!("hover ECJ error: {message}");
                }
                Err(e) => warn!("hover error: {e}"),
                _ => {}
            }
        }

        Ok(tree
            .as_ref()
            .and_then(|tree| syntax_navigation::hover_markdown(tree, &content, offset))
            .map(|markdown| hover_conv::to_lsp(&markdown)))
    }

    // ── Signature Help ────────────────────────────────────────────────────────

    async fn signature_help(&self, params: SignatureHelpParams) -> LspResult<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.signature_help(uri, offset).await {
            Ok(BridgeResponse::SignatureHelp { signatures, active_signature, active_parameter, .. }) => {
                let sigs: Vec<SignatureInformation> = signatures.iter().map(|s| {
                    SignatureInformation {
                        label: s.label.clone(),
                        documentation: s.documentation.as_ref().map(|d| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: d.clone(),
                            })
                        }),
                        parameters: Some(s.parameters.iter().map(|p| ParameterInformation {
                            label: ParameterLabel::Simple(p.label.clone()),
                            documentation: p.documentation.as_ref().map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d.clone(),
                                })
                            }),
                        }).collect()),
                        active_parameter: None,
                    }
                }).collect();
                Ok(Some(SignatureHelp {
                    signatures: sigs,
                    active_signature: Some(active_signature),
                    active_parameter: Some(active_parameter),
                }))
            }
            _ => Ok(None),
        }
    }

    // ── Definition / Declaration / Type Definition / Implementation ───────────

    async fn goto_definition(&self, params: GotoDefinitionParams) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.navigate(uri, offset, NavKind::Definition).await {
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
            .map(|range| GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range,
            })))
    }

    async fn goto_declaration(&self, params: GotoDeclarationParams) -> LspResult<Option<GotoDeclarationResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let (offset, content, tree) = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => (
                pos_to_offset(&s.content, pos).unwrap_or(0),
                s.content_string(),
                s.tree.clone(),
            ),
        };

        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.navigate(uri, offset, NavKind::Declaration).await {
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
            .map(|range| GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range,
            })))
    }

    async fn goto_type_definition(&self, params: GotoTypeDefinitionParams) -> LspResult<Option<GotoTypeDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
        match self.dispatcher.navigate(uri, offset, NavKind::TypeDefinition).await {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() { Ok(None) }
                else { Ok(Some(GotoDefinitionResponse::Array(locs))) }
            }
            _ => Ok(None),
        }
    }

    async fn goto_implementation(&self, params: GotoImplementationParams) -> LspResult<Option<GotoImplementationResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
        match self.dispatcher.navigate(uri, offset, NavKind::Implementation).await {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() { Ok(None) }
                else { Ok(Some(GotoDefinitionResponse::Array(locs))) }
            }
            _ => Ok(None),
        }
    }

    // ── References ────────────────────────────────────────────────────────────

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
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

    async fn document_highlight(&self, params: DocumentHighlightParams) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
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
                                start: Position { line: l.start_line, character: l.start_char },
                                end: Position { line: l.end_line, character: l.end_char },
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

        Ok(if highlights.is_empty() { None } else { Some(highlights) })
    }

    // ── Document Symbols (Outline) ─────────────────────────────────────────────

    async fn document_symbol(&self, params: DocumentSymbolParams) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => s,
        };
        let tree = match &state.tree {
            None => return Ok(None),
            Some(t) => t.clone(),
        };
        let content = state.content_string();
        drop(state);

        let syms = outline::document_symbols(&tree, &content);
        Ok(Some(DocumentSymbolResponse::Nested(syms)))
    }

    async fn symbol(&self, params: WorkspaceSymbolParams) -> LspResult<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_ascii_lowercase();
        let mut symbols = Vec::new();

        for state in self.store.snapshots() {
            let Some(tree) = state.tree.as_ref() else {
                continue;
            };
            let content = state.content_string();
            let document_symbols = outline::document_symbols(tree, &content);
            flatten_workspace_symbols(&mut symbols, &state.uri, None, &document_symbols, &query);
        }

        Ok(Some(symbols))
    }

    // ── Code Action ────────────────────────────────────────────────────────────

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        let bridge_range = BridgeRange {
            start_line: params.range.start.line,
            start_char: params.range.start.character,
            end_line: params.range.end.line,
            end_char: params.range.end.character,
        };

        let bridge_diags = params.context.diagnostics.iter()
            .map(|d| to_bridge_diag(uri, d))
            .collect();

        let mut lsp_actions: Vec<CodeActionOrCommand> = Vec::new();

        if let Ok(BridgeResponse::CodeActions { actions, .. }) =
            self.dispatcher.code_action(uri, bridge_range, bridge_diags).await
        {
            lsp_actions.extend(
                ca_conv::to_lsp(&actions)
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction),
            );
        }

        // Organize Imports — always offered (ECJ code action also includes it, but this
        // ensures it appears even when the ECJ organizeImports call returns no edits).
        if let Ok(BridgeResponse::WorkspaceEdit { changes, .. }) =
            self.dispatcher.organize_imports(uri).await
        {
            let org_edit = ca_conv::workspace_edit_from_bridge(&changes);
            lsp_actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Organize Imports".to_owned(),
                kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
                edit: Some(org_edit),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }

        Ok(if lsp_actions.is_empty() { None } else { Some(lsp_actions) })
    }

    // ── Formatting ─────────────────────────────────────────────────────────────

    async fn formatting(&self, params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let opts = &params.options;
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.format(uri, opts.tab_size as u32, opts.insert_spaces).await {
            Ok(BridgeResponse::TextEdits { uri: response_uri, edits, .. }) => {
                if response_uri != uri.as_str() {
                    warn!("format response URI {response_uri} does not match request URI {uri}");
                }
                let text_edits: Vec<TextEdit> = edits.iter().map(|e| TextEdit {
                    range: Range {
                        start: Position { line: e.start_line, character: e.start_char },
                        end: Position { line: e.end_line, character: e.end_char },
                    },
                    new_text: e.new_text.clone(),
                }).collect();
                Ok(if text_edits.is_empty() { None } else { Some(text_edits) })
            }
            _ => Ok(None),
        }
    }

    async fn range_formatting(&self, params: DocumentRangeFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        // Delegate to full-file formatting for now; ECJ formatter can be extended later
        self.formatting(DocumentFormattingParams {
            text_document: params.text_document,
            options: params.options,
            work_done_progress_params: Default::default(),
        }).await
    }

    // ── Rename ────────────────────────────────────────────────────────────────

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.rename(uri, offset, params.new_name).await {
            Ok(BridgeResponse::WorkspaceEdit { changes, .. }) => {
                let mut lsp_changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
                for fe in &changes {
                    if let Ok(u) = Url::parse(&fe.uri) {
                        let edits: Vec<TextEdit> = fe.edits.iter().map(|e| TextEdit {
                            range: Range {
                                start: Position { line: e.start_line, character: e.start_char },
                                end: Position { line: e.end_line, character: e.end_char },
                            },
                            new_text: e.new_text.clone(),
                        }).collect();
                        lsp_changes.entry(u).or_default().extend(edits);
                    }
                }
                Ok(Some(WorkspaceEdit { changes: Some(lsp_changes), ..Default::default() }))
            }
            _ => Ok(None),
        }
    }

    // ── Folding Ranges ────────────────────────────────────────────────────────

    async fn folding_range(&self, params: FoldingRangeParams) -> LspResult<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => s,
        };
        let tree = match &state.tree {
            None => return Ok(None),
            Some(t) => t.clone(),
        };
        let content = state.content_string();
        drop(state);

        Ok(Some(folding::folding_ranges(&tree, &content)))
    }

    // ── Semantic Tokens ────────────────────────────────────────────────────────

    async fn semantic_tokens_full(&self, params: SemanticTokensParams) -> LspResult<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => s,
        };
        let tree = match &state.tree {
            None => return Ok(None),
            Some(t) => t.clone(),
        };
        let content = state.content_string();
        drop(state);

        let token_vec = tokens::semantic_tokens_full(&tree, &content);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: token_vec,
        })))
    }

    async fn selection_range(&self, params: SelectionRangeParams) -> LspResult<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;
        let state = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => s,
        };
        let tree = match &state.tree {
            None => return Ok(None),
            Some(t) => t.clone(),
        };

        let mut ranges = Vec::with_capacity(params.positions.len());
        for position in params.positions {
            let offset = pos_to_offset(&state.content, position).unwrap_or(0);
            ranges.push(selection::selection_range(&tree, offset).unwrap_or(SelectionRange {
                range: Range {
                    start: position,
                    end: position,
                },
                parent: None,
            }));
        }

        Ok(Some(ranges))
    }

    // ── Inlay Hints ───────────────────────────────────────────────────────────

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }
        match self.dispatcher.inlay_hints(uri).await {
            Ok(BridgeResponse::InlayHints { hints, .. }) => {
                let items = hints
                    .iter()
                    .map(|h| InlayHint {
                        position: Position { line: h.line, character: h.character },
                        label: InlayHintLabel::String(h.label.clone()),
                        kind: Some(match h.kind {
                            1 => InlayHintKind::TYPE,
                            _ => InlayHintKind::PARAMETER,
                        }),
                        tooltip: None,
                        padding_left: Some(false),
                        padding_right: Some(true),
                        text_edits: None,
                        data: None,
                    })
                    .collect();
                Ok(Some(items))
            }
            Ok(BridgeResponse::Error { message, .. }) => {
                warn!("inlay hints ECJ error: {message}");
                Ok(None)
            }
            Err(e) => {
                warn!("inlay hints error: {e}");
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    // ── Code Lenses ───────────────────────────────────────────────────────────

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;
        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }
        match self.dispatcher.code_lens(uri).await {
            Ok(BridgeResponse::CodeLenses { lenses, .. }) => {
                let items = lenses.iter().map(|l| {
                    let range = Range {
                        start: Position { line: l.start_line, character: l.start_char },
                        end: Position { line: l.end_line, character: l.end_char },
                    };
                    let command = l.command.as_ref().map(|cmd| Command {
                        title: l.title.clone(),
                        command: cmd.clone(),
                        arguments: l.args.clone(),
                    });
                    // Informational lenses (no command) still need a title in the command field.
                    let command = command.unwrap_or_else(|| Command {
                        title: l.title.clone(),
                        command: String::new(),
                        arguments: None,
                    });
                    CodeLens { range, command: Some(command), data: None }
                }).collect();
                Ok(Some(items))
            }
            Ok(BridgeResponse::Error { message, .. }) => {
                warn!("code lens ECJ error: {message}");
                Ok(None)
            }
            Err(e) => {
                warn!("code lens error: {e}");
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    // ── Call Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_call_hierarchy(&self, params: CallHierarchyPrepareParams) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.call_hierarchy_prepare(uri, offset).await {
            Ok(BridgeResponse::CallHierarchyPrepare { items, .. }) if !items.is_empty() => {
                Ok(Some(items.iter().map(bridge_call_hierarchy_item_to_lsp).collect()))
            }
            _ => Ok(None),
        }
    }

    async fn incoming_calls(&self, params: CallHierarchyIncomingCallsParams) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        let item = &params.item;
        let uri = &item.uri;
        let pos = item.selection_range.start;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.call_hierarchy_incoming(uri, offset).await {
            Ok(BridgeResponse::CallHierarchyIncomingCalls { calls, .. }) => {
                let result = calls.iter().map(|c| CallHierarchyIncomingCall {
                    from: bridge_call_hierarchy_item_to_lsp(&c.from),
                    from_ranges: c.from_ranges.iter().map(|r| Range {
                        start: Position { line: r.start_line, character: r.start_char },
                        end: Position { line: r.end_line, character: r.end_char },
                    }).collect(),
                }).collect();
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    async fn outgoing_calls(&self, params: CallHierarchyOutgoingCallsParams) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        let item = &params.item;
        let uri = &item.uri;
        let pos = item.selection_range.start;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.call_hierarchy_outgoing(uri, offset).await {
            Ok(BridgeResponse::CallHierarchyOutgoingCalls { calls, .. }) => {
                let result = calls.iter().map(|c| CallHierarchyOutgoingCall {
                    to: bridge_call_hierarchy_item_to_lsp(&c.to),
                    from_ranges: c.from_ranges.iter().map(|r| Range {
                        start: Position { line: r.start_line, character: r.start_char },
                        end: Position { line: r.end_line, character: r.end_char },
                    }).collect(),
                }).collect();
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    // ── Type Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_type_hierarchy(&self, params: TypeHierarchyPrepareParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.type_hierarchy_prepare(uri, offset).await {
            Ok(BridgeResponse::TypeHierarchyPrepare { items, .. }) if !items.is_empty() => {
                Ok(Some(items.iter().map(bridge_type_hierarchy_item_to_lsp).collect()))
            }
            _ => Ok(None),
        }
    }

    async fn supertypes(&self, params: TypeHierarchySupertypesParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        let data = match params.item.data.as_ref().and_then(|v| v.as_str()) {
            Some(s) => s.to_owned(),
            None => return Ok(None),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.type_hierarchy_supertypes(data).await {
            Ok(BridgeResponse::TypeHierarchySupertypes { items, .. }) => {
                Ok(Some(items.iter().map(bridge_type_hierarchy_item_to_lsp).collect()))
            }
            _ => Ok(None),
        }
    }

    async fn subtypes(&self, params: TypeHierarchySubtypesParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        let data = match params.item.data.as_ref().and_then(|v| v.as_str()) {
            Some(s) => s.to_owned(),
            None => return Ok(None),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.type_hierarchy_subtypes(data).await {
            Ok(BridgeResponse::TypeHierarchySubtypes { items, .. }) => {
                Ok(Some(items.iter().map(bridge_type_hierarchy_item_to_lsp).collect()))
            }
            _ => Ok(None),
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
            start: Position { line: item.start_line, character: item.start_char },
            end: Position { line: item.end_line, character: item.end_char },
        },
        selection_range: Range {
            start: Position { line: item.sel_start_line, character: item.sel_start_char },
            end: Position { line: item.sel_end_line, character: item.sel_end_char },
        },
        data: item.data.as_ref().map(|s| serde_json::Value::String(s.clone())),
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
            start: Position { line: item.start_line, character: item.start_char },
            end: Position { line: item.end_line, character: item.end_char },
        },
        selection_range: Range {
            start: Position { line: item.sel_start_line, character: item.sel_start_char },
            end: Position { line: item.sel_end_line, character: item.sel_end_char },
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
    !matches!(
        (prev, next),
        (Some(prev), Some(next))
            if (!is_java_ident_part(prev) && is_completion_boundary(next))
                || (is_completion_boundary(prev) && next.is_whitespace())
    )
}

/// Returns the LSP Range covering the Java identifier immediately before the cursor.
/// This is attached as `textEdit` on completion items that also carry
/// `additionalTextEdits` (auto-import), because Monaco only applies
/// `additionalTextEdits` when the item has an explicit `textEdit`.
fn word_range_at(content: &str, pos: tower_lsp::lsp_types::Position) -> Option<tower_lsp::lsp_types::Range> {
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
        start: tower_lsp::lsp_types::Position { line: pos.line, character: word_start_col },
        end:   pos,
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
    while i > 0
        && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r')
    {
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
    if rest.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
        Some(rest.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{completion_store_is_fresh, detect_import_prefix};
    use tower_lsp::lsp_types::Position;

    // ── Normal (non-stale) cases ──────────────────────────────────────────────

    #[test]
    fn detects_top_level_package() {
        let src = "import java\n";
        assert_eq!(
            detect_import_prefix(src, 0, 11, None),
            Some("java".into())
        );
    }

    #[test]
    fn detects_after_dot_in_stored_content() {
        // Content already has the dot (no race).
        let src = "import java.\n";
        assert_eq!(
            detect_import_prefix(src, 0, 12, None),
            Some("java.".into())
        );
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
        assert!(
            !completion_store_is_fresh(
                stale,
                Position { line: 0, character: 30 },
                None,
            )
        );
    }

    #[test]
    fn completion_wait_accepts_updated_insert_before_semicolon() {
        let fresh = "class T { void m() { int x = A; } }\n";
        assert!(
            completion_store_is_fresh(
                fresh,
                Position { line: 0, character: 30 },
                None,
            )
        );
    }
}
