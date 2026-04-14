//! tower-lsp LanguageServer implementation.

use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::code_action as ca_conv;
use crate::analysis::semantic::completion as comp_conv;
use crate::analysis::semantic::definition as def_conv;
use crate::analysis::semantic::diagnostics as diag_conv;
use crate::analysis::semantic::hover as hover_conv;
use crate::analysis::semantic::protocol::{BridgeRange, BridgeResponse};
use crate::analysis::semantic::NavKind;
use crate::analysis::syntax::{folding, outline, snippets, tokens};
use crate::config::Config;
use crate::document_store::DocumentStore;
use crate::handlers::text_document::pos_to_offset;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
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
use tree_sitter::Parser;

pub struct JavaLanguageServer {
    client: Client,
    store: Arc<DocumentStore>,
    dispatcher: Arc<Dispatcher>,
    config: Arc<RwLock<Config>>,
    parser: Arc<Mutex<Parser>>,
}

impl JavaLanguageServer {
    pub fn new(client: Client) -> Self {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = Arc::new(DocumentStore::new());
        let dispatcher = Arc::new(Dispatcher::new(Arc::clone(&store), Arc::clone(&config)));

        let mut parser = Parser::new();
        let lang = tree_sitter_java::language();
        parser.set_language(&lang).expect("tree-sitter-java init");

        Self {
            client,
            store,
            dispatcher,
            config,
            parser: Arc::new(Mutex::new(parser)),
        }
    }

    // ── Utilities ─────────────────────────────────────────────────────────────

    async fn publish_diagnostics_for_all(&self) {
        if !self.dispatcher.is_ecj_ready().await {
            return;
        }
        match self.dispatcher.compile_all().await {
            Err(e) => error!("compile_all error: {e}"),
            Ok(BridgeResponse::Diagnostics { items, .. }) => {
                // Group by URI
                let mut by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
                for item in &items {
                    if let Some((uri, diag)) = diag_conv::to_lsp(item) {
                        by_uri.entry(uri).or_default().push(diag);
                    }
                }
                // Also publish empty diagnostics for files with no errors
                for file in self.store.all_contents().keys() {
                    if let Ok(uri) = Url::parse(file) {
                        by_uri.entry(uri).or_default();
                    }
                }
                for (uri, diags) in by_uri {
                    self.client.publish_diagnostics(uri, diags, None).await;
                }
            }
            Ok(BridgeResponse::Error { message, .. }) => {
                warn!("ECJ compile error: {message}");
            }
            _ => {}
        }
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

        // Start ecj-bridge in background
        let dispatcher = Arc::clone(&self.dispatcher);
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatcher.start_ecj().await {
                error!("Failed to start ecj-bridge: {e}");
                client.show_message(MessageType::ERROR,
                    format!("jdtls-rust: failed to start ecj-bridge: {e}")).await;
            } else {
                info!("ecj-bridge started");
            }
        });

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
                        ".".into(), "@".into(), "#".into(), " ".into(),
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
                            CodeActionKind::REFACTOR,
                            CodeActionKind::SOURCE,
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
        self.publish_diagnostics_for_all().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        {
            let mut parser = self.parser.lock().await;
            self.store.apply_changes(&uri, version, params.content_changes, &mut parser);
        }
        // Debounce: publish after a short delay is handled by the client
        // requesting a new compile; here we compile immediately.
        self.publish_diagnostics_for_all().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.store.close(&params.text_document.uri);
        // Clear diagnostics for the closed file
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics_for_all().await;
    }

    // ── Completion ────────────────────────────────────────────────────────────

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let offset = {
            match self.store.get(uri) {
                None => return Ok(None),
                Some(state) => pos_to_offset(&state.content, pos).unwrap_or(0),
            }
        };

        let mut items: Vec<CompletionItem> = Vec::new();

        // Always include static snippets
        items.extend(snippets::java_snippets());

        // Semantic completions from ECJ
        if self.dispatcher.is_ecj_ready().await {
            match self.dispatcher.complete(uri, offset).await {
                Ok(BridgeResponse::Completions { items: bridge_items, .. }) => {
                    let semantic: Vec<CompletionItem> =
                        bridge_items.iter().map(comp_conv::to_lsp).collect();
                    // Prepend semantic items so they sort first
                    items = semantic.into_iter().chain(items).collect();
                }
                Ok(BridgeResponse::Error { message, .. }) => {
                    warn!("completion ECJ error: {message}");
                }
                Err(e) => warn!("completion error: {e}"),
                _ => {}
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    // ── Hover ─────────────────────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };

        if !self.dispatcher.is_ecj_ready().await {
            return Ok(None);
        }

        match self.dispatcher.hover(uri, offset).await {
            Ok(BridgeResponse::Hover { contents, .. }) if !contents.is_empty() => {
                Ok(Some(hover_conv::to_lsp(&contents)))
            }
            Ok(BridgeResponse::Error { message, .. }) => {
                warn!("hover ECJ error: {message}");
                Ok(None)
            }
            Err(e) => { warn!("hover error: {e}"); Ok(None) }
            _ => Ok(None),
        }
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
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
        match self.dispatcher.navigate(uri, offset, NavKind::Definition).await {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() { Ok(None) }
                else { Ok(Some(GotoDefinitionResponse::Array(locs))) }
            }
            _ => Ok(None),
        }
    }

    async fn goto_declaration(&self, params: GotoDeclarationParams) -> LspResult<Option<GotoDeclarationResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
        match self.dispatcher.navigate(uri, offset, NavKind::Declaration).await {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                if locs.is_empty() { Ok(None) }
                else { Ok(Some(GotoDefinitionResponse::Array(locs))) }
            }
            _ => Ok(None),
        }
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
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
        match self.dispatcher.find_references(uri, offset).await {
            Ok(BridgeResponse::Locations { locations, .. }) => {
                let locs = def_conv::to_lsp(&locations);
                Ok(if locs.is_empty() { None } else { Some(locs) })
            }
            _ => Ok(None),
        }
    }

    // ── Document Highlight ────────────────────────────────────────────────────

    async fn document_highlight(&self, params: DocumentHighlightParams) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let offset = match self.store.get(uri) {
            None => return Ok(None),
            Some(s) => pos_to_offset(&s.content, pos).unwrap_or(0),
        };
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }
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
                Ok(if highlights.is_empty() { None } else { Some(highlights) })
            }
            _ => Ok(None),
        }
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

    // ── Code Action ────────────────────────────────────────────────────────────

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let range = &params.range;
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        let bridge_range = BridgeRange {
            start_line: range.start.line,
            start_char: range.start.character,
            end_line: range.end.line,
            end_char: range.end.character,
        };

        match self.dispatcher.code_action(uri, bridge_range).await {
            Ok(BridgeResponse::CodeActions { actions, .. }) => {
                let lsp_actions: Vec<CodeActionOrCommand> = ca_conv::to_lsp(&actions)
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect();
                Ok(if lsp_actions.is_empty() { None } else { Some(lsp_actions) })
            }
            _ => Ok(None),
        }
    }

    // ── Formatting ─────────────────────────────────────────────────────────────

    async fn formatting(&self, params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let opts = &params.options;
        if !self.dispatcher.is_ecj_ready().await { return Ok(None); }

        match self.dispatcher.format(uri, opts.tab_size as u32, opts.insert_spaces).await {
            Ok(BridgeResponse::TextEdits { edits, .. }) => {
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

    // ── Inlay Hints ───────────────────────────────────────────────────────────

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        // Inlay hints require type inference from ECJ; placeholder for now.
        Ok(None)
    }

    // ── Call Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_call_hierarchy(&self, params: CallHierarchyPrepareParams) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        Ok(None) // TODO: implement via ECJ
    }

    async fn incoming_calls(&self, params: CallHierarchyIncomingCallsParams) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(None)
    }

    async fn outgoing_calls(&self, params: CallHierarchyOutgoingCallsParams) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(None)
    }

    // ── Type Hierarchy ────────────────────────────────────────────────────────

    async fn prepare_type_hierarchy(&self, params: TypeHierarchyPrepareParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        Ok(None) // TODO: implement via ECJ
    }

    async fn supertypes(&self, params: TypeHierarchySupertypesParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        Ok(None)
    }

    async fn subtypes(&self, params: TypeHierarchySubtypesParams) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        Ok(None)
    }
}
