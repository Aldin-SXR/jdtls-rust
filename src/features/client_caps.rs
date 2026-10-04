//! Client capabilities consulted by the ported handlers (jdt.ls
//! `ClientPreferences`).

use std::sync::RwLock;

use tower_lsp::lsp_types::ClientCapabilities;

static CAPS: RwLock<Option<ClientCapabilities>> = RwLock::new(None);

pub fn set(caps: &ClientCapabilities) {
    *CAPS.write().unwrap_or_else(|e| e.into_inner()) = Some(caps.clone());
}

fn with<T>(f: impl FnOnce(&ClientCapabilities) -> Option<T>) -> Option<T> {
    CAPS.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(f)
}

/// `ClientPreferences.isHierarchicalDocumentSymbolSupported`.
pub fn hierarchical_document_symbols() -> bool {
    with(|c| c.text_document.as_ref()?.document_symbol.as_ref()?.hierarchical_document_symbol_support).unwrap_or(false)
}

/// `ClientPreferences.isSymbolTagSupported`.
pub fn symbol_tags() -> bool {
    with(|c| {
        c.text_document.as_ref()?.document_symbol.as_ref()?.tag_support.as_ref()?;
        Some(true)
    })
    .unwrap_or(false)
}

/// `ClientPreferences.isDiagnosticTagSupported`.
pub fn diagnostic_tags() -> bool {
    with(|c| {
        c.text_document.as_ref()?.publish_diagnostics.as_ref()?.tag_support.as_ref()?;
        Some(true)
    })
    .unwrap_or(false)
}

/// `ClientPreferences.isSupportedCodeActionKind(kind)`.
pub fn supported_code_action_kind(kind: &str) -> bool {
    with(|c| {
        let set = &c.text_document.as_ref()?.code_action.as_ref()?.code_action_literal_support.as_ref()?.code_action_kind.value_set;
        Some(set.iter().any(|k| kind.starts_with(k.as_str())))
    })
    .unwrap_or(false)
}

/// `ClientPreferences.isResolveCodeActionSupported`.
pub fn resolve_code_action() -> bool {
    with(|c| {
        let ca = c.text_document.as_ref()?.code_action.as_ref()?;
        let data = ca.data_support?;
        let props = &ca.resolve_support.as_ref()?.properties;
        Some(data && props.iter().any(|p| p == "edit"))
    })
    .unwrap_or(false)
}

/// `ClientPreferences.isResourceOperationSupported`.
pub fn resource_operations() -> bool {
    use tower_lsp::lsp_types::ResourceOperationKind as K;
    with(|c| {
        let ops = c.workspace.as_ref()?.workspace_edit.as_ref()?.resource_operations.as_ref()?;
        Some(ops.contains(&K::Create) && ops.contains(&K::Rename) && ops.contains(&K::Delete))
    })
    .unwrap_or(false)
}
