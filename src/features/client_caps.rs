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

/// `ClientPreferences.isWorkspaceChangeWatchedFilesDynamicRegistered`.
pub fn watched_files_dynamic_registration() -> bool {
    with(|c| c.workspace.as_ref()?.did_change_watched_files.as_ref()?.dynamic_registration).unwrap_or(false)
}
