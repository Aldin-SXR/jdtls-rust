//! Rust ports of jdt.ls request handlers, built on bridge data where JDT
//! bindings are needed.  `server.rs` only delegates here.

pub mod call_hierarchy;
pub mod client_caps;
pub mod code_lens;
pub mod document_symbol;
pub mod dom;
pub mod folding_range;
pub mod formatting;
pub mod inlay_hint_filter;
pub mod inlay_hints;
pub mod java_element;
pub mod java_model;
pub mod navigation;
pub mod preferences;
pub mod rename;
pub mod scanner;
pub mod selection_range;
pub mod semantic;
pub mod semantic_tokens;
pub mod signature_help;
pub mod type_hierarchy;
pub mod workspace_symbols;
mod ts_dump;

use tower_lsp::lsp_types::Url;

use crate::document_store::DocumentStore;

/// Text of a document: open/workspace documents from the store, otherwise a
/// plain `.java` file read from disk (jdt.ls serves files outside any project
/// through its invisible project).
pub fn source_text(store: &DocumentStore, uri: &Url) -> Option<String> {
    if let Some(state) = store.get(uri) {
        return Some(state.content_string());
    }
    if uri.scheme() == "file" {
        let path = uri.to_file_path().ok()?;
        if path.extension().is_some_and(|e| e == "java") {
            return std::fs::read_to_string(path).ok();
        }
    }
    None
}
