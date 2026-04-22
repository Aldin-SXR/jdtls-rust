//! Routes analysis requests to the appropriate layer (tree-sitter or ECJ bridge).

use crate::config::Config;
use crate::document_store::DocumentStore;
use super::semantic::{BridgeRequest, BridgeResponse, EcjProcess, NavKind};
use super::semantic::protocol::{BridgeRange, BridgeDiagnostic};
use super::semantic::ecj_process::next_id;
use anyhow::{anyhow, Result};
use tower_lsp::lsp_types::Url;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Central dispatcher: owns the ECJ process and the document store.
pub struct Dispatcher {
    pub store: Arc<DocumentStore>,
    ecj: Arc<RwLock<Option<EcjProcess>>>,
    config: Arc<RwLock<Config>>,
}

impl Dispatcher {
    pub fn new(store: Arc<DocumentStore>, config: Arc<RwLock<Config>>) -> Self {
        Self {
            store,
            ecj: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// Start the ecj-bridge subprocess (called after initialize).
    pub async fn start_ecj(&self) -> Result<()> {
        let cfg = self.config.read().await;
        let jar = ecj_jar_path()?;
        let proc = EcjProcess::spawn(jar, &cfg.java_binary()).await?;
        *self.ecj.write().await = Some(proc);
        Ok(())
    }

    pub async fn restart_ecj(&self) -> Result<()> {
        let old = {
            let mut guard = self.ecj.write().await;
            guard.take()
        };
        if let Some(ecj) = old {
            ecj.shutdown().await;
        }
        self.start_ecj().await
    }

    pub async fn is_ecj_ready(&self) -> bool {
        self.ecj.read().await.is_some()
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    async fn send(&self, req: BridgeRequest) -> Result<BridgeResponse> {
        let guard = self.ecj.read().await;
        let ecj = guard.as_ref().ok_or_else(|| anyhow!("ecj-bridge not started"))?;
        ecj.send(req).await
    }

    async fn classpath_and_level(&self) -> (Vec<String>, String) {
        let cfg = self.config.read().await;
        (cfg.classpath.clone(), cfg.source_compatibility.clone())
    }

    // ── Public analysis ops ──────────────────────────────────────────────────

    pub async fn compile_all(&self) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::Compile {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
        }).await
    }

    /// `content_snapshot` is the content of `uri` at the time `offset` was computed.
    /// It overrides the store entry so ECJ sees the same content the offset was derived from,
    /// avoiding a race between `didChange` and `completion`.
    pub async fn complete(
        &self,
        uri: &Url,
        offset: usize,
        import_prefix: Option<String>,
        content_snapshot: String,
    ) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        let mut files = self.store.all_contents();
        files.insert(uri.to_string(), content_snapshot);
        self.send(BridgeRequest::Complete {
            id: next_id(),
            files,
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
            import_prefix,
        }).await
    }

    pub async fn hover(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::Hover {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn navigate(&self, uri: &Url, offset: usize, kind: NavKind) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::Navigate {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
            kind,
        }).await
    }

    pub async fn find_references(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::FindReferences {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn code_action(&self, uri: &Url, range: BridgeRange, diagnostics: Vec<BridgeDiagnostic>) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::CodeAction {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            range,
            diagnostics,
        }).await
    }

    pub async fn signature_help(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::SignatureHelp {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn rename(&self, uri: &Url, offset: usize, new_name: String) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::Rename {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
            new_name,
        }).await
    }

    pub async fn organize_imports(&self, uri: &Url) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::OrganizeImports {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
        }).await
    }

    pub async fn code_lens(&self, uri: &Url) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::CodeLens {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
        }).await
    }

    pub async fn inlay_hints(&self, uri: &Url) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::InlayHints {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
        }).await
    }

    pub async fn format(&self, uri: &Url, tab_size: u32, insert_spaces: bool) -> Result<BridgeResponse> {
        let state = self.store.get(uri).ok_or_else(|| anyhow!("document not open"))?;
        self.send(BridgeRequest::Format {
            id: next_id(),
            source: state.content_string(),
            uri: uri.to_string(),
            tab_size,
            insert_spaces,
        }).await
    }

    pub async fn type_hierarchy_prepare(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::TypeHierarchyPrepare {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn type_hierarchy_supertypes(&self, data: String) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::TypeHierarchySupertypes {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            data,
        }).await
    }

    pub async fn type_hierarchy_subtypes(&self, data: String) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::TypeHierarchySubtypes {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            data,
        }).await
    }

    pub async fn call_hierarchy_prepare(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::CallHierarchyPrepare {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn call_hierarchy_incoming(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::CallHierarchyIncoming {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn call_hierarchy_outgoing(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let (classpath, source_level) = self.classpath_and_level().await;
        self.send(BridgeRequest::CallHierarchyOutgoing {
            id: next_id(),
            files: self.store.all_contents(),
            classpath,
            source_level,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn shutdown_ecj(&self) {
        if let Some(ecj) = self.ecj.read().await.as_ref() {
            ecj.shutdown().await;
        }
    }
}

/// Resolve the path to the ecj-bridge JAR.
///
/// Priority:
/// 1. `JDTLS_ECJ_JAR` env var (explicit override — useful for development)
/// 2. Embedded JAR extracted from the binary (normal production path)
fn ecj_jar_path() -> Result<&'static std::path::Path> {
    // 1. Explicit override
    if let Ok(p) = std::env::var("JDTLS_ECJ_JAR") {
        // Leak so we can return &'static Path
        let path: &'static std::path::Path =
            Box::leak(Box::new(std::path::PathBuf::from(p))).as_path();
        if path.exists() { return Ok(path); }
    }

    // 2. Extract embedded JAR
    crate::embedded_jar::jar_path()
}
