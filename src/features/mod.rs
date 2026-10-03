//! Rust ports of jdt.ls request handlers.  `server.rs` only delegates here.

pub mod client_caps;
pub mod document_symbol;
pub mod dom;
pub mod folding_range;
pub mod java_model;
pub mod scanner;
pub mod selection_range;
pub mod semantic_tokens;

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
mod ts_dump;
