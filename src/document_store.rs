use crate::analysis::syntax::parser::JavaParser;
use crate::handlers::text_document::pos_to_char;
use dashmap::DashMap;
use tower_lsp::lsp_types::{TextDocumentContentChangeEvent, Url};
use ropey::Rope;
use std::sync::{Arc, Mutex};
use tree_sitter::Tree;

/// All state associated with one document known to the server.
#[derive(Clone)]
pub struct FileState {
    pub uri: Url,
    pub content: Rope,
    pub version: i32,
    pub language_id: String,
    /// Most recently parsed tree-sitter tree (may be stale if content changed
    /// but re-parse hasn't run yet).
    pub tree: Option<Arc<Tree>>,
    /// `true` when the client opened the document (`didOpen`); `false` for
    /// workspace files loaded from disk on demand.
    pub open: bool,
}

impl FileState {
    pub fn content_string(&self) -> String {
        self.content.to_string()
    }
}

/// Thread-safe registry of documents.
///
/// Two layers:
/// * documents opened by the client — authoritative, and may be *virtual*
///   (no backing file: `untitled:`, `inmemory://`, or a nonexistent path);
/// * workspace `.java` files discovered by project import, read from disk
///   lazily and refreshed when the client reports file changes.
pub struct DocumentStore {
    files: Arc<DashMap<Url, FileState>>,
    /// Known workspace source files (from project import).
    workspace: Arc<DashMap<Url, ()>>,
    parser: Mutex<JavaParser>,
    /// CoreASTProvider's active Java element; closing a buffer does not clear it.
    active_java_uri: Mutex<Option<Url>>,
    /// `BaseDocumentLifeCycleHandler.documentVersions`: the client's version
    /// of each open document (`didOpen`/`didChange`), removed on `didClose`.
    document_versions: DashMap<Url, i32>,
}

/// `BaseDocumentLifeCycleHandler.DocumentMonitor`: detects that a document
/// changed while a request was being computed.
pub struct DocumentMonitor<'a> {
    store: &'a DocumentStore,
    uri: Url,
    initial_version: Option<i32>,
}

impl DocumentMonitor<'_> {
    /// `true` if the document has changed since the creation of this monitor.
    /// A document that is not open (no version before and now) is assumed not
    /// to have changed.
    pub fn has_changed(&self) -> bool {
        self.initial_version != self.store.document_version(&self.uri)
    }

    /// `checkChanged()`: a `ContentModified` response error if the document
    /// [`has_changed`](Self::has_changed).
    pub fn check_changed(&self) -> Result<(), tower_lsp::jsonrpc::Error> {
        if self.has_changed() {
            return Err(tower_lsp::jsonrpc::Error {
                code: tower_lsp::jsonrpc::ErrorCode::ServerError(-32801),
                message: "Document changed, request invalid".into(),
                data: None,
            });
        }
        Ok(())
    }
}

impl DocumentStore {
    /// `new DocumentMonitor(uri)`.
    pub fn monitor(&self, uri: &Url) -> DocumentMonitor<'_> {
        DocumentMonitor { store: self, uri: uri.clone(), initial_version: self.document_version(uri) }
    }

    fn document_version(&self, uri: &Url) -> Option<i32> {
        self.document_versions.get(uri).map(|v| *v)
    }

    pub fn new() -> Self {
        Self {
            files: Arc::new(DashMap::new()),
            workspace: Arc::new(DashMap::new()),
            parser: Mutex::new(JavaParser::new()),
            active_java_uri: Mutex::new(None),
            document_versions: DashMap::new(),
        }
    }

    pub fn set_active_java_uri(&self, uri: &Url) {
        *self.active_java_uri.lock().unwrap_or_else(|e| e.into_inner()) = Some(uri.clone());
    }

    pub fn is_active_java_uri(&self, uri: &Url) -> bool {
        self.active_java_uri.lock().unwrap_or_else(|e| e.into_inner()).as_ref() == Some(uri)
    }

    pub fn open(&self, uri: Url, language_id: String, version: i32, text: String, parser: &mut JavaParser) {
        self.document_versions.insert(uri.clone(), version);
        let rope = Rope::from_str(&text);
        let tree = parser.parse_fresh(&text).map(Arc::new);
        if self.files.get(&uri).is_some_and(|s| s.open) {
            tracing::debug!("Re-opening already-tracked document: {}", uri);
        }
        self.files.insert(uri.clone(), FileState {
            uri,
            content: rope,
            version,
            language_id,
            tree,
            open: true,
        });
    }

    /// Close a client document.  Workspace files fall back to their disk
    /// content; virtual documents are forgotten.
    pub fn close(&self, uri: &Url) {
        self.document_versions.remove(uri);
        self.files.remove(uri);
        if self.workspace.contains_key(uri) {
            self.load_from_disk(uri);
        }
    }

    /// Forget a document entirely (file deleted).
    pub fn remove(&self, uri: &Url) {
        self.files.remove(uri);
        self.workspace.remove(uri);
    }

    /// A saved file or directory was deleted. Forget the disk-backed units
    /// below it, preserving working copies until the client closes them.
    pub fn remove_workspace_path(&self, uri: &Url) {
        let path = uri.to_file_path().ok();
        let removed: Vec<Url> = self.workspace.iter().map(|e| e.key().clone()).filter(|candidate| {
            candidate == uri || path.as_ref().is_some_and(|root| {
                candidate.to_file_path().is_ok_and(|p| p.starts_with(root))
            })
        }).collect();
        for candidate in removed {
            self.workspace.remove(&candidate);
            if self.files.get(&candidate).is_some_and(|s| !s.open) {
                self.files.remove(&candidate);
            }
        }
    }

    pub fn rename(&self, old_uri: &Url, new_uri: Url) {
        let was_workspace = self.workspace.remove(old_uri).is_some();
        if let Some((_, mut state)) = self.files.remove(old_uri) {
            state.uri = new_uri.clone();
            self.files.insert(new_uri.clone(), state);
        }
        if was_workspace {
            self.workspace.insert(new_uri, ());
        }
    }

    /// Replace the set of known workspace source files.
    pub fn set_workspace_files(&self, uris: impl IntoIterator<Item = Url>) {
        let new: std::collections::HashSet<Url> = uris.into_iter().collect();
        let stale: Vec<Url> = self
            .workspace
            .iter()
            .map(|e| e.key().clone())
            .filter(|u| !new.contains(u))
            .collect();
        for u in stale {
            self.workspace.remove(&u);
            if self.files.get(&u).is_some_and(|s| !s.open) {
                self.files.remove(&u);
            }
        }
        for u in new {
            self.workspace.insert(u, ());
        }
    }

    /// Register one new workspace file (created on disk).
    pub fn add_workspace_file(&self, uri: Url) {
        self.workspace.insert(uri.clone(), ());
        if self.files.get(&uri).is_some_and(|s| !s.open) {
            self.files.remove(&uri);
        }
    }

    /// The file changed on disk: drop the cached content unless it is open.
    pub fn invalidate_disk(&self, uri: &Url) {
        if self.files.get(uri).is_some_and(|s| !s.open) {
            self.files.remove(uri);
        }
    }

    pub fn is_workspace_file(&self, uri: &Url) -> bool {
        self.workspace.contains_key(uri)
    }

    fn load_from_disk(&self, uri: &Url) -> bool {
        let Some(path) = uri.to_file_path().ok() else { return false };
        let Ok(text) = std::fs::read_to_string(&path) else { return false };
        let tree = self.parser.lock().ok().and_then(|mut p| p.parse_fresh(&text)).map(Arc::new);
        // Never replace a document opened meanwhile (loads race with didOpen).
        self.files.entry(uri.clone()).or_insert(FileState {
            uri: uri.clone(),
            content: Rope::from_str(&text),
            version: 0,
            language_id: "java".to_owned(),
            tree,
            open: false,
        });
        true
    }

    /// Apply incremental changes from `textDocument/didChange`, re-parse.
    pub fn apply_changes(
        &self,
        uri: &Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        parser: &mut JavaParser,
    ) {
        self.document_versions.insert(uri.clone(), version);
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

    /// Look up a document: open documents first, then workspace files (loaded
    /// from disk on first access).
    pub fn get(&self, uri: &Url) -> Option<dashmap::mapref::one::Ref<'_, Url, FileState>> {
        if let Some(r) = self.files.get(uri) {
            return Some(r);
        }
        if self.workspace.contains_key(uri) && self.load_from_disk(uri) {
            return self.files.get(uri);
        }
        None
    }

    fn ensure_workspace_loaded(&self) {
        let missing: Vec<Url> = self
            .workspace
            .iter()
            .map(|e| e.key().clone())
            .filter(|u| !self.files.contains_key(u))
            .collect();
        for u in missing {
            self.load_from_disk(&u);
        }
    }

    /// Snapshot of all Java file contents (open + workspace), keyed by URI string.
    /// Only Java files are sent to ecj-bridge; other language files are skipped.
    pub fn all_contents(&self) -> std::collections::HashMap<String, String> {
        self.ensure_workspace_loaded();
        self.files
            .iter()
            .filter(|e| e.language_id == "java")
            .map(|e| (e.uri.to_string(), e.content.to_string()))
            .collect()
    }

    /// Contents of every workspace file as saved on disk: open buffers are
    /// ignored (what a build of the saved files sees).
    pub fn disk_contents(&self) -> std::collections::HashMap<String, String> {
        self.ensure_workspace_loaded();
        let mut out = std::collections::HashMap::new();
        for e in self.workspace.iter() {
            let uri = e.key();
            let text = match self.files.get(uri) {
                Some(f) if !f.open => Some(f.content.to_string()),
                _ => uri.to_file_path().ok().and_then(|p| std::fs::read_to_string(p).ok()),
            };
            if let Some(t) = text {
                out.insert(uri.to_string(), t);
            }
        }
        out
    }

    /// URIs of the documents the client has open.
    pub fn open_uris(&self) -> Vec<Url> {
        let mut out: Vec<Url> = self.files.iter().filter(|e| e.open).map(|e| e.key().clone()).collect();
        out.sort();
        out
    }

    /// Snapshot of documents.  Includes workspace files loaded from disk.
    pub fn snapshots(&self) -> Vec<FileState> {
        self.ensure_workspace_loaded();
        self.files.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Snapshot of client-opened documents only.
    pub fn open_snapshots(&self) -> Vec<FileState> {
        self.files.iter().filter(|e| e.open).map(|entry| entry.value().clone()).collect()
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.files.contains_key(uri) || self.workspace.contains_key(uri)
    }

    pub fn is_open(&self, uri: &Url) -> bool {
        self.files.get(uri).is_some_and(|s| s.open)
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod document_life_cycle_handler_test {
    //! The `DocumentMonitor` cases of
    //! `org.eclipse.jdt.ls.core.internal.handlers.DocumentLifeCycleHandlerTest`
    //! (the others are in `tests/handlers_document_life_cycle_handler_test.rs`).

    use super::*;

    fn open_document(store: &DocumentStore, uri: &Url, content: &str, version: i32) {
        store.open(uri.clone(), "java".into(), version, content.into(), &mut JavaParser::new());
    }

    fn change_document_full(store: &DocumentStore, uri: &Url, content: &str, version: i32) {
        let change = TextDocumentContentChangeEvent { range: None, range_length: None, text: content.into() };
        store.apply_changes(uri, version, vec![change], &mut JavaParser::new());
    }

    fn assert_changed(monitor: &DocumentMonitor<'_>) {
        let error = monitor.check_changed().expect_err("ResponseErrorException expected");
        assert_eq!(tower_lsp::jsonrpc::ErrorCode::ServerError(-32801), error.code);
        assert_eq!("Document changed, request invalid", error.message);
    }

    fn cu() -> Url {
        Url::parse("file:///tmp/TestProject/src/foo/Foo.java").unwrap()
    }

    #[test]
    fn test_document_monitor() {
        let store = DocumentStore::new();
        let content = "package foo;\n";
        let cu = cu();

        open_document(&store, &cu, content, 1);
        let document_monitor = store.monitor(&cu);
        document_monitor.check_changed().unwrap();
        change_document_full(&store, &cu, content, 2);
        assert_changed(&document_monitor);
        store.close(&cu);
    }

    #[test]
    fn test_document_monitor_closed_document() {
        let store = DocumentStore::new();
        let content = "package foo;\n";
        let cu = cu();

        let document_monitor_before_open = store.monitor(&cu);
        open_document(&store, &cu, content, 1);
        change_document_full(&store, &cu, content, 2);
        assert_changed(&document_monitor_before_open); // Version changed (null -> 2)
        store.close(&cu);

        let document_monitor_after_close = store.monitor(&cu);
        document_monitor_after_close.check_changed().unwrap(); // Version not changed (null -> null)
    }
}
