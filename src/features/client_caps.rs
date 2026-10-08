//! Client capabilities consulted by the ported handlers: a port of jdt.ls
//! `ClientPreferences`, the wrapper around the `ClientCapabilities` the
//! client sent with `initialize`.

use std::sync::RwLock;

use serde_json::{Map, Value};
use tower_lsp::lsp_types::{ClientCapabilities, MarkupKind, ResourceOperationKind, TextDocumentClientCapabilities};

/// `ClientPreferences`.
#[derive(Debug, Clone)]
pub struct ClientPreferences {
    capabilities: ClientCapabilities,
    v3supported: bool,
    extended_client_capabilities: Map<String, Value>,
}

/// `isDynamicRegistrationSupported(capability)`: the capability is present
/// and its `dynamicRegistration` is `true`.
macro_rules! dynamic_registration {
    ($cap:expr) => {
        $cap.as_ref().and_then(|c| c.dynamic_registration).unwrap_or(false)
    };
}

// The whole `ClientPreferences` API is ported; not every predicate has a
// Rust caller yet.
#[allow(dead_code)]
impl ClientPreferences {
    /// `new ClientPreferences(caps)`; `None` is the Java `null`, which
    /// throws `IllegalArgumentException`.
    pub fn new(caps: Option<ClientCapabilities>) -> Result<Self, String> {
        Self::with_extended(caps, None)
    }

    /// `new ClientPreferences(caps, extendedClientCapabilities)`.
    pub fn with_extended(caps: Option<ClientCapabilities>, extended: Option<&Value>) -> Result<Self, String> {
        let Some(capabilities) = caps else {
            return Err("ClientCapabilities can not be null".to_owned());
        };
        let v3supported = capabilities.text_document.is_some();
        let extended_client_capabilities = extended.and_then(Value::as_object).cloned().unwrap_or_default();
        Ok(Self { capabilities, v3supported, extended_client_capabilities })
    }

    /// `capabilities.getTextDocument()`, only reached when `v3supported`.
    fn text(&self) -> Option<&TextDocumentClientCapabilities> {
        self.capabilities.text_document.as_ref().filter(|_| self.v3supported)
    }

    fn text_dynamic(&self, f: impl FnOnce(&TextDocumentClientCapabilities) -> bool) -> bool {
        self.text().is_some_and(f)
    }

    pub fn is_signature_help_supported(&self) -> bool {
        self.text().is_some_and(|t| t.signature_help.is_some())
    }

    pub fn is_workspace_folders_supported(&self) -> bool {
        self.capabilities.workspace.as_ref().and_then(|w| w.workspace_folders).unwrap_or(false)
    }

    pub fn is_workspace_will_rename_files_supported(&self) -> bool {
        self.v3supported
            && self
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.file_operations.as_ref())
                .and_then(|f| f.will_rename)
                .unwrap_or(false)
    }

    pub fn is_completion_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.completion))
    }

    pub fn is_completion_snippets_supported(&self) -> bool {
        self.text()
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .and_then(|i| i.snippet_support)
            .unwrap_or(false)
    }

    pub fn is_v3_supported(&self) -> bool {
        self.v3supported
    }

    pub fn is_formatting_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.formatting))
    }

    pub fn is_range_formatting_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.range_formatting))
    }

    pub fn is_on_type_formatting_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.on_type_formatting))
    }

    pub fn is_code_lens_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.code_lens))
    }

    pub fn is_signature_help_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.signature_help))
    }

    pub fn is_rename_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.rename))
    }

    pub fn is_execute_command_dynamic_registration_supported(&self) -> bool {
        self.v3supported && self.capabilities.workspace.as_ref().is_some_and(|w| dynamic_registration!(w.execute_command))
    }

    pub fn is_workspace_symbol_dynamic_registered(&self) -> bool {
        self.v3supported && self.capabilities.workspace.as_ref().is_some_and(|w| dynamic_registration!(w.symbol))
    }

    pub fn is_workspace_change_watched_files_dynamic_registered(&self) -> bool {
        self.v3supported
            && self.capabilities.workspace.as_ref().is_some_and(|w| dynamic_registration!(w.did_change_watched_files))
    }

    pub fn is_workspace_configuration_supported(&self) -> bool {
        self.v3supported && self.capabilities.workspace.as_ref().and_then(|w| w.configuration).unwrap_or(false)
    }

    pub fn is_document_symbol_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.document_symbol))
    }

    pub fn is_code_action_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.code_action))
    }

    pub fn is_definition_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.definition))
    }

    pub fn is_declaration_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.declaration))
    }

    pub fn is_type_definition_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.type_definition))
    }

    pub fn is_hover_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.hover))
    }

    pub fn is_references_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.references))
    }

    pub fn is_document_highlight_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.document_highlight))
    }

    pub fn is_folding_range_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.folding_range))
    }

    pub fn is_implementation_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.implementation))
    }

    pub fn is_selection_range_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.selection_range))
    }

    pub fn is_inlay_hint_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.inlay_hint))
    }

    pub fn is_call_hierarchy_dynamic_registered(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.call_hierarchy))
    }

    pub fn is_type_hierarchy_dynamic_registration_supported(&self) -> bool {
        self.text_dynamic(|t| dynamic_registration!(t.type_hierarchy))
    }

    pub fn is_will_save_registered(&self) -> bool {
        self.text().and_then(|t| t.synchronization.as_ref()).and_then(|s| s.will_save).unwrap_or(false)
    }

    pub fn is_will_save_wait_until_registered(&self) -> bool {
        self.text().and_then(|t| t.synchronization.as_ref()).and_then(|s| s.will_save_wait_until).unwrap_or(false)
    }

    pub fn is_work_done_progress_supported(&self) -> bool {
        self.v3supported && self.capabilities.window.as_ref().and_then(|w| w.work_done_progress).unwrap_or(false)
    }

    pub fn is_workspace_apply_edit_supported(&self) -> bool {
        self.capabilities.workspace.as_ref().and_then(|w| w.apply_edit).unwrap_or(false)
    }

    /// `Boolean.parseBoolean(extendedClientCapabilities.getOrDefault(name, "false").toString())`.
    pub fn extended_flag(&self, name: &str) -> bool {
        match self.extended_client_capabilities.get(name) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
            _ => false,
        }
    }

    pub fn is_class_file_content_supported(&self) -> bool {
        self.extended_flag("classFileContentsSupport")
    }

    pub fn is_supports_completion_documentation_markdown(&self) -> bool {
        self.text()
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .and_then(|i| i.documentation_format.as_ref())
            .is_some_and(|f| f.contains(&MarkupKind::Markdown))
    }

    pub fn is_resource_operation_supported(&self) -> bool {
        self.capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.workspace_edit.as_ref())
            .and_then(|e| e.resource_operations.as_ref())
            .is_some_and(|ops| {
                ops.contains(&ResourceOperationKind::Create)
                    && ops.contains(&ResourceOperationKind::Rename)
                    && ops.contains(&ResourceOperationKind::Delete)
            })
    }

    /// `true` only when the client explicitly set
    /// `textDocument.documentSymbol.hierarchicalDocumentSymbolSupport`.
    pub fn is_hierarchical_document_symbol_supported(&self) -> bool {
        self.text()
            .and_then(|t| t.document_symbol.as_ref())
            .and_then(|d| d.hierarchical_document_symbol_support)
            .unwrap_or(false)
    }

    /// The client listed a prefix of `kind` in
    /// `textDocument.codeAction.codeActionLiteralSupport.codeActionKind.valueSet`.
    pub fn is_supported_code_action_kind(&self, kind: &str) -> bool {
        self.text()
            .and_then(|t| t.code_action.as_ref())
            .and_then(|c| c.code_action_literal_support.as_ref())
            .is_some_and(|l| l.code_action_kind.value_set.iter().any(|k| kind.starts_with(k.as_str())))
    }

    pub fn is_diagnostic_tag_supported(&self) -> bool {
        self.text().and_then(|t| t.publish_diagnostics.as_ref()).is_some_and(|d| d.tag_support.is_some())
    }

    pub fn is_resolve_code_action_supported(&self) -> bool {
        self.text().and_then(|t| t.code_action.as_ref()).is_some_and(|c| {
            c.data_support.unwrap_or(false)
                && c.resolve_support.as_ref().is_some_and(|r| r.properties.iter().any(|p| p == "edit"))
        })
    }

    pub fn is_completion_item_tag_supported(&self) -> bool {
        self.text()
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .is_some_and(|i| i.tag_support.is_some())
    }

    pub fn is_symbol_tag_supported(&self) -> bool {
        self.text().and_then(|t| t.document_symbol.as_ref()).is_some_and(|d| d.tag_support.is_some())
    }

    pub fn is_completion_insert_replace_support(&self) -> bool {
        self.text()
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .and_then(|i| i.insert_replace_support)
            .unwrap_or(false)
    }

    pub fn is_property_supported_for_completion_resolve(&self, property: &str) -> bool {
        self.text()
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .and_then(|i| i.resolve_support.as_ref())
            .is_some_and(|r| r.properties.iter().any(|p| p == property))
    }

    /// Upstream dereferences `capabilities.getWorkspace()` unchecked; a
    /// missing `workspace` reads as unsupported here.
    pub fn is_inlay_hint_refresh_supported(&self) -> bool {
        self.v3supported
            && self
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.inlay_hint.as_ref())
                .and_then(|i| i.refresh_support)
                .unwrap_or(false)
    }

    pub fn is_code_lens_refresh_supported(&self) -> bool {
        self.v3supported
            && self
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.code_lens.as_ref())
                .and_then(|c| c.refresh_support)
                .unwrap_or(false)
    }

    pub fn is_change_annotation_support(&self) -> bool {
        self.v3supported
            && self
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.workspace_edit.as_ref())
                .is_some_and(|e| e.change_annotation_support.is_some())
    }
}

static PREFS: RwLock<Option<ClientPreferences>> = RwLock::new(None);

/// `PreferenceManager.updateClientPrefences`.
pub fn set(caps: &ClientCapabilities) {
    *PREFS.write().unwrap_or_else(|e| e.into_inner()) = ClientPreferences::new(Some(caps.clone())).ok();
}

fn with(f: impl FnOnce(&ClientPreferences) -> bool) -> bool {
    PREFS.read().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(f)
}

/// `ClientPreferences.isHierarchicalDocumentSymbolSupported`.
pub fn hierarchical_document_symbols() -> bool {
    with(ClientPreferences::is_hierarchical_document_symbol_supported)
}

/// `ClientPreferences.isSymbolTagSupported`.
pub fn symbol_tags() -> bool {
    with(ClientPreferences::is_symbol_tag_supported)
}

/// `ClientPreferences.isWorkspaceConfigurationSupported`.
pub fn workspace_configuration() -> bool {
    with(ClientPreferences::is_workspace_configuration_supported)
}

/// `ClientPreferences.isDiagnosticTagSupported`.
pub fn diagnostic_tags() -> bool {
    with(ClientPreferences::is_diagnostic_tag_supported)
}

/// `ClientPreferences.isSupportedCodeActionKind(kind)`.
pub fn supported_code_action_kind(kind: &str) -> bool {
    with(|p| p.is_supported_code_action_kind(kind))
}

/// `ClientPreferences.isResolveCodeActionSupported`.
pub fn resolve_code_action() -> bool {
    with(ClientPreferences::is_resolve_code_action_supported)
}

/// `ClientPreferences.isResourceOperationSupported`.
pub fn resource_operations() -> bool {
    with(ClientPreferences::is_resource_operation_supported)
}

/// Port of `org.eclipse.jdt.ls.core.internal.preferences.ClientPreferencesTest`.
///
/// The Mockito mocks become capabilities built in place: `cap` is a
/// `ClientCapabilities` whose `textDocument` and `workspace` are present but
/// empty (every getter of the mocked `text`/`workspace` returns `null`), and
/// `when(text.getX()).thenReturn(v)` sets that field on the wrapped
/// capabilities, which `ClientPreferences` reads on every call.
#[cfg(test)]
mod client_preferences_test {
    use super::*;
    use tower_lsp::lsp_types::{
        CodeLensClientCapabilities, CompletionClientCapabilities, CompletionItemCapability,
        CompletionItemCapabilityResolveSupport, DocumentFormattingClientCapabilities,
        DocumentRangeFormattingClientCapabilities, DocumentSymbolClientCapabilities, InlayHintClientCapabilities,
        InlayHintWorkspaceClientCapabilities, RenameClientCapabilities, SignatureHelpClientCapabilities, TagSupport,
        WorkspaceClientCapabilities,
    };

    /// `setup()`.
    fn setup() -> ClientPreferences {
        let cap = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities::default()),
            workspace: Some(WorkspaceClientCapabilities::default()),
            ..Default::default()
        };
        ClientPreferences::new(Some(cap)).unwrap()
    }

    /// The mocked `text`.
    fn text(prefs: &mut ClientPreferences) -> &mut TextDocumentClientCapabilities {
        prefs.capabilities.text_document.as_mut().unwrap()
    }

    /// The mocked `workspace`.
    fn workspace(prefs: &mut ClientPreferences) -> &mut WorkspaceClientCapabilities {
        prefs.capabilities.workspace.as_mut().unwrap()
    }

    #[test]
    fn test_client_preferences() {
        let _prefs = setup();
        assert!(ClientPreferences::new(None).is_err());
    }

    #[test]
    fn test_is_v3_supported() {
        let mut prefs = setup();
        assert!(prefs.is_v3_supported());

        prefs = ClientPreferences::new(Some(ClientCapabilities::default())).unwrap();
        assert!(!prefs.is_v3_supported());
    }

    #[test]
    fn test_is_execute_command_dynamic_registration_supported() {
        let prefs = setup();
        assert!(prefs.is_v3_supported());
        assert!(!prefs.is_execute_command_dynamic_registration_supported());
        assert!(!prefs.is_workspace_symbol_dynamic_registered());
        assert!(!prefs.is_workspace_change_watched_files_dynamic_registered());
        assert!(!prefs.is_workspace_folders_supported());
    }

    #[test]
    fn test_is_signature_help_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_signature_help_supported());
        text(&mut prefs).signature_help = Some(SignatureHelpClientCapabilities::default());
        assert!(prefs.is_signature_help_supported());
    }

    #[test]
    fn test_is_completion_snippets_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_completion_snippets_supported());
        text(&mut prefs).completion = Some(CompletionClientCapabilities::default());
        assert!(!prefs.is_completion_snippets_supported());
        // new CompletionCapabilities(new CompletionItemCapabilities(true))
        text(&mut prefs).completion = Some(CompletionClientCapabilities {
            completion_item: Some(CompletionItemCapability { snippet_support: Some(true), ..Default::default() }),
            ..Default::default()
        });
        assert!(prefs.is_completion_snippets_supported());
    }

    #[test]
    fn test_is_formatting_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_formatting_dynamic_registration_supported());
        text(&mut prefs).formatting = Some(DocumentFormattingClientCapabilities::default());
        assert!(!prefs.is_formatting_dynamic_registration_supported());
        text(&mut prefs).formatting = Some(DocumentFormattingClientCapabilities { dynamic_registration: Some(true) });
        assert!(prefs.is_formatting_dynamic_registration_supported());
    }

    #[test]
    fn test_is_range_formatting_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_range_formatting_dynamic_registration_supported());
        text(&mut prefs).range_formatting = Some(DocumentRangeFormattingClientCapabilities::default());
        assert!(!prefs.is_range_formatting_dynamic_registration_supported());
        text(&mut prefs).range_formatting =
            Some(DocumentRangeFormattingClientCapabilities { dynamic_registration: Some(true) });
        assert!(prefs.is_range_formatting_dynamic_registration_supported());
    }

    #[test]
    fn test_is_code_lens_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_code_lens_dynamic_registration_supported());
        text(&mut prefs).code_lens = Some(CodeLensClientCapabilities::default());
        assert!(!prefs.is_code_lens_dynamic_registration_supported());
        text(&mut prefs).code_lens = Some(CodeLensClientCapabilities { dynamic_registration: Some(true) });
        assert!(prefs.is_code_lens_dynamic_registration_supported());
    }

    #[test]
    fn test_is_signature_help_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_signature_help_dynamic_registration_supported());
        text(&mut prefs).signature_help = Some(SignatureHelpClientCapabilities::default());
        assert!(!prefs.is_signature_help_dynamic_registration_supported());
        text(&mut prefs).signature_help =
            Some(SignatureHelpClientCapabilities { dynamic_registration: Some(true), ..Default::default() });
        assert!(prefs.is_signature_help_dynamic_registration_supported());
    }

    #[test]
    fn test_is_rename_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_rename_dynamic_registration_supported());
        text(&mut prefs).rename = Some(RenameClientCapabilities::default());
        assert!(!prefs.is_rename_dynamic_registration_supported());
        text(&mut prefs).rename = Some(RenameClientCapabilities { dynamic_registration: Some(true), ..Default::default() });
        assert!(prefs.is_rename_dynamic_registration_supported());
    }

    #[test]
    fn test_is_inlay_hint_dynamic_registration_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_inlay_hint_dynamic_registered());
        text(&mut prefs).inlay_hint =
            Some(InlayHintClientCapabilities { dynamic_registration: Some(true), ..Default::default() });
        assert!(prefs.is_inlay_hint_dynamic_registered());
    }

    #[test]
    fn test_is_inlay_hint_refresh_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_inlay_hint_refresh_supported());
        workspace(&mut prefs).inlay_hint = Some(InlayHintWorkspaceClientCapabilities { refresh_support: Some(true) });
        assert!(prefs.is_inlay_hint_refresh_supported());
    }

    #[test]
    fn test_is_hierarchical_document_symbol_supported() {
        let mut prefs = setup();
        let mut capabilities = DocumentSymbolClientCapabilities::default();
        assert!(!prefs.is_hierarchical_document_symbol_supported());
        text(&mut prefs).document_symbol = Some(capabilities.clone());
        assert!(!prefs.is_hierarchical_document_symbol_supported());
        capabilities.hierarchical_document_symbol_support = Some(false);
        text(&mut prefs).document_symbol = Some(capabilities.clone());
        assert!(!prefs.is_hierarchical_document_symbol_supported());
        capabilities.hierarchical_document_symbol_support = Some(true);
        text(&mut prefs).document_symbol = Some(capabilities.clone());
        assert!(prefs.is_hierarchical_document_symbol_supported());
    }

    #[test]
    fn test_is_completion_item_tag_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_completion_item_tag_supported());
        let mut item_capabilities = CompletionItemCapability::default();
        item_capabilities.tag_support = Some(TagSupport { value_set: vec![] });
        text(&mut prefs).completion =
            Some(CompletionClientCapabilities { completion_item: Some(item_capabilities), ..Default::default() });
        assert!(prefs.is_completion_item_tag_supported());
    }

    #[test]
    fn test_is_property_supported_for_completion_resolve() {
        let mut prefs = setup();
        let property = "property";
        assert!(!prefs.is_property_supported_for_completion_resolve(property));
        let mut item_capabilities = CompletionItemCapability::default();
        item_capabilities.resolve_support =
            Some(CompletionItemCapabilityResolveSupport { properties: vec![property.to_owned()] });
        text(&mut prefs).completion =
            Some(CompletionClientCapabilities { completion_item: Some(item_capabilities), ..Default::default() });
        assert!(prefs.is_property_supported_for_completion_resolve(property));
    }

    #[test]
    fn test_is_symbol_tag_supported() {
        let mut prefs = setup();
        assert!(!prefs.is_symbol_tag_supported());
        let capabilities =
            DocumentSymbolClientCapabilities { tag_support: Some(TagSupport { value_set: vec![] }), ..Default::default() };
        text(&mut prefs).document_symbol = Some(capabilities);
        assert!(prefs.is_symbol_tag_supported());
    }
}
