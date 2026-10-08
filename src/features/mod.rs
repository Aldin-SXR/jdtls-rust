//! Rust ports of jdt.ls request handlers, built on bridge data where JDT
//! bindings are needed.  `server.rs` only delegates here.

pub mod accessors;
pub mod build_path;
pub mod constructors;
pub mod content_provider;
pub mod call_hierarchy;
pub mod client_caps;
pub mod code_lens;
pub mod completion;
pub mod document_symbol;
pub mod dom;
pub mod client_connection;
pub mod configuration;
pub mod create_module_info;
pub mod execute_command;
pub mod file_events;
pub mod folding_range;
pub mod formatting;
pub mod hover;
pub mod hashcode;
pub mod delegates;
pub mod overrides;
pub mod organize_imports;
pub mod save_actions;
pub mod cleanup;
pub mod init;
pub mod inlay_hint_filter;
pub mod inlay_hints;
pub mod java_element;
pub mod java_model;
pub mod lifecycle;
pub mod markers;
pub mod navigation;
pub mod paste;
pub mod preferences;
pub mod progress;
pub mod project_commands;
pub mod rename;
pub mod resolve_source_mapping;
pub mod scanner;
pub mod selection_range;
pub mod semantic;
pub mod semantic_tokens;
pub mod signature_help;
pub mod smart_detection;
pub mod tostring;
mod ts_dump;
pub mod type_hierarchy;
pub mod workspace_symbols;

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

/// Editor text for a compilation unit or a read-only class-file document.
/// Keep binary source out of the document store: it must not be compiled as
/// another workspace compilation unit.
pub async fn document_text(
    d: &crate::analysis::dispatcher::Dispatcher,
    uri: &Url,
) -> Option<String> {
    if crate::classfile::is_class_file_uri(uri) {
        let text = navigation::class_file_contents(d, uri.as_str()).await;
        return (!text.is_empty()).then_some(text);
    }
    source_text(&d.store, uri)
}
