use crate::analysis::syntax::parser::JavaParser;
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
                        let start = lsp_pos_to_char(&state.content, range.start);
                        let end = lsp_pos_to_char(&state.content, range.end);
                        state.content.remove(start..end);
                        state.content.insert(start, &change.text);
                    }
                }
            }
            state.version = version;
            let text = state.content.to_string();
            let old_tree = state.tree.as_ref().map(|t| t.as_ref());
            // tree-sitter can use old tree for incremental parsing, but we
            // need InputEdit which requires computing byte offsets — for now
            // do a full re-parse (still fast, ~1ms for typical files).
            state.tree = parser.parse(&text, old_tree).map(Arc::new);
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

fn lsp_pos_to_char(rope: &Rope, pos: tower_lsp::lsp_types::Position) -> usize {
    let line = pos.line as usize;
    let col = pos.character as usize;
    let line_start = rope.line_to_char(line.min(rope.len_lines().saturating_sub(1)));
    line_start + col
}
