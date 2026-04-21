use crate::analysis::syntax::parser::JavaParser;
use crate::handlers::text_document::pos_to_char;
use dashmap::DashMap;
use tower_lsp::lsp_types::{TextDocumentContentChangeEvent, Url};
use ropey::Rope;
use std::sync::Arc;
use tree_sitter::Tree;

/// All state associated with one open document.
#[derive(Clone)]
pub struct FileState {
    pub uri: Url,
    pub content: Rope,
    pub version: i32,
    pub language_id: String,
    /// Most recently parsed tree-sitter tree (may be stale if content changed
    /// but re-parse hasn't run yet).
    pub tree: Option<Arc<Tree>>,
}

impl FileState {
    pub fn content_string(&self) -> String {
        self.content.to_string()
    }
}

/// Thread-safe in-memory registry of all open documents.
pub struct DocumentStore {
    files: Arc<DashMap<Url, FileState>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            files: Arc::new(DashMap::new()),
        }
    }

    pub fn open(&self, uri: Url, language_id: String, version: i32, text: String, parser: &mut JavaParser) {
        let rope = Rope::from_str(&text);
        let tree = parser.parse_fresh(&text).map(Arc::new);
        if self.contains(&uri) {
            tracing::debug!("Re-opening already-tracked document: {}", uri);
        }
        self.files.insert(uri.clone(), FileState {
            uri,
            content: rope,
            version,
            language_id,
            tree,
        });
    }

    pub fn close(&self, uri: &Url) {
        self.files.remove(uri);
    }

    /// Apply incremental changes from `textDocument/didChange`, re-parse.
    pub fn apply_changes(
        &self,
        uri: &Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        parser: &mut JavaParser,
    ) {
        if let Some(mut state) = self.files.get_mut(uri) {
            for change in changes {
                match change.range {
                    None => {
                        // Full replacement
                        state.content = Rope::from_str(&change.text);
                    }
                    Some(range) => {
                        let start = pos_to_char(&state.content, range.start)
                            .unwrap_or_else(|| state.content.len_chars());
                        let end = pos_to_char(&state.content, range.end)
                            .unwrap_or_else(|| state.content.len_chars());
                        state.content.remove(start..end);
                        state.content.insert(start, &change.text);
                    }
                }
            }
            state.version = version;
            let text = state.content.to_string();
            // Full re-parse: tree-sitter incremental parsing requires calling
            // old_tree.edit(InputEdit) first to mark changed ranges; without it
            // tree-sitter reuses stale node byte-ranges from the old tree that can
            // exceed the new source length → panic in utf8_text / node.byte_range().
            state.tree = parser.parse_fresh(&text).map(Arc::new);
        }
    }

    pub fn get(&self, uri: &Url) -> Option<dashmap::mapref::one::Ref<'_, Url, FileState>> {
        self.files.get(uri)
    }

    /// Snapshot of all open Java file contents, keyed by URI string.
    /// Only Java files are sent to ecj-bridge; other language files are skipped.
    pub fn all_contents(&self) -> std::collections::HashMap<String, String> {
        self.files
            .iter()
            .filter(|e| e.language_id == "java")
            .map(|e| (e.uri.to_string(), e.content.to_string()))
            .collect()
    }

    pub fn snapshots(&self) -> Vec<FileState> {
        self.files.iter().map(|entry| entry.value().clone()).collect()
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.files.contains_key(uri)
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}
