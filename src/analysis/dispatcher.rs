//! Routes analysis requests to the appropriate layer (tree-sitter or ECJ bridge).

use crate::config::Config;
use crate::document_store::DocumentStore;
use super::semantic::{BridgeRequest, BridgeResponse, EcjProcess, NavKind};
use super::semantic::protocol::{BridgeRange, BridgeDiagnostic};
use super::semantic::ecj_process::next_id;
use anyhow::{anyhow, Result};
use tower_lsp::lsp_types::Url;
use std::sync::Arc;
use std::collections::{BTreeMap, HashMap};
use crate::project::Workspace;
use tokio::sync::RwLock;

/// Central dispatcher: owns the ECJ process and the document store.
pub struct RequestContext {
    pub files: HashMap<String, String>,
    pub classpath: Vec<String>,
    pub source_level: String,
    pub options: BTreeMap<String, String>,
}

/// Result of [`Dispatcher::inlay_hint_data`].
pub struct InlayHintData {
    pub response: BridgeResponse,
    /// The document text the bridge offsets refer to.
    pub source: String,
    /// Package implied by the document's location in a source folder (the
    /// JDT package fragment), when it lies in one.
    pub folder_package: Option<String>,
}

pub struct Dispatcher {
    pub store: Arc<DocumentStore>,
    pub workspace: Arc<std::sync::RwLock<Workspace>>,
    ecj: Arc<RwLock<Option<EcjProcess>>>,
    config: Arc<RwLock<Config>>,
}

impl Dispatcher {
    pub fn new(store: Arc<DocumentStore>, config: Arc<RwLock<Config>>) -> Self {
        Self {
            store,
            workspace: Arc::new(std::sync::RwLock::new(Workspace::default())),
            ecj: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// Connect to the shared ecj-bridge daemon, starting it if not running.
    pub async fn start_ecj(&self) -> Result<()> {
        let cfg = self.config.read().await;
        let jar = ecj_jar_path()?;
        let socket = crate::embedded_jar::socket_path();
        let proc = EcjProcess::ensure_started(jar, &cfg.java_binary(), socket).await?;
        *self.ecj.write().await = Some(proc);
        Ok(())
    }

    /// Drop the current connection and reconnect (or start a fresh daemon if
    /// the socket is stale).  Other clients already connected to the daemon
    /// are unaffected.
    pub async fn restart_ecj(&self) -> Result<()> {
        let old = {
            let mut guard = self.ecj.write().await;
            guard.take()
        };
        if let Some(ecj) = old {
            ecj.shutdown().await;
        }
        // Remove the socket file so ensure_started spawns a fresh daemon
        // rather than reconnecting to a potentially unhealthy one.
        let socket = crate::embedded_jar::socket_path();
        let _ = std::fs::remove_file(socket);
        self.start_ecj().await
    }

    /// Whether `uri` lies in a source folder of an imported project.
    pub fn owns_source_path(&self, uri: &Url) -> bool {
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner());
        crate::project::uri_to_path(uri)
            .is_some_and(|p| ws.project_for_path(&p).is_some_and(|proj| proj.source_folder_for(&p).is_some()))
    }

    pub async fn is_ecj_ready(&self) -> bool {
        self.ecj.read().await.is_some()
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Send a raw bridge request (used by feature modules that build their
    /// own requests, e.g. `features::navigation`).
    pub async fn send_request(&self, req: BridgeRequest) -> Result<BridgeResponse> {
        self.send(req).await
    }

    async fn send(&self, req: BridgeRequest) -> Result<BridgeResponse> {
        let guard = self.ecj.read().await;
        let ecj = guard.as_ref().ok_or_else(|| anyhow!("ecj-bridge not started"))?;
        ecj.send(req).await
    }

    /// Build the ECJ request context for `uri`: the sources, classpath,
    /// compliance and compiler options of the project owning it.  Documents
    /// outside every imported project (including virtual documents) share the
    /// default project, which uses only the configured classpath/compliance.
    pub async fn context_for(&self, uri: Option<&Url>) -> RequestContext {
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
        let project = uri.and_then(|u| ws.project_for_uri(u)).map(|p| p.name.clone());
        self.context_for_project(&ws, project.as_deref(), uri.is_none()).await
    }

    /// Context for the workspace project named `name` (the default project
    /// when no such project exists).
    pub async fn context_for_project_name(&self, name: &str) -> RequestContext {
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
        let project = ws.project(name).map(|p| p.name.clone());
        self.context_for_project(&ws, project.as_deref(), false).await
    }

    /// Contexts of the other projects whose closure includes the project
    /// owning `uri` (they may reference its elements).
    pub async fn dependent_contexts(&self, uri: &Url) -> Vec<RequestContext> {
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(owner) = ws.project_for_uri(uri).map(|p| p.name.clone()) else { return Vec::new() };
        let mut out = Vec::new();
        for p in &ws.projects {
            if p.name != owner && ws.project_closure(p).iter().any(|c| c.name == owner) {
                out.push(self.context_for_project(&ws, Some(&p.name), false).await);
            }
        }
        out
    }

    /// Context for the named project, or for the default project when `None`
    /// (`everything`: include all documents, used for workspace-wide queries).
    async fn context_for_project(&self, ws: &Workspace, project: Option<&str>, everything: bool) -> RequestContext {
        let cfg = self.config.read().await.clone();
        let mut all = self.store.all_contents();
        let Some(project) = project.and_then(|n| ws.project(n)) else {
            if !everything {
                all.retain(|u, _| Url::parse(u).ok().map_or(true, |u| ws.project_for_uri(&u).is_none()));
            }
            let mut options = crate::project::jdtls_default_options();
            options.extend(cfg.compiler_options.clone());
            return RequestContext {
                files: all,
                classpath: cfg.classpath.clone(),
                source_level: cfg.source_compatibility.clone(),
                options,
            };
        };
        let closure = ws.project_closure(project);
        let names: std::collections::HashSet<&str> = closure.iter().map(|p| p.name.as_str()).collect();
        all.retain(|u, _| {
            Url::parse(u)
                .ok()
                .and_then(|u| ws.project_for_uri(&u))
                .is_some_and(|p| names.contains(p.name.as_str()))
        });
        let mut classpath: Vec<String> = Vec::new();
        for p in &closure {
            for lib in &p.libraries {
                let s = lib.path.to_string_lossy().into_owned();
                if !classpath.contains(&s) {
                    classpath.push(s);
                }
            }
        }
        classpath.extend(cfg.classpath.iter().cloned());
        let mut options = crate::project::jdtls_default_options();
        options.extend(cfg.compiler_options.clone());
        options.extend(project.options.clone());
        let source_level = project
            .compliance()
            .map(str::to_owned)
            .unwrap_or_else(|| cfg.source_compatibility.clone());
        RequestContext { files: all, classpath, source_level, options }
    }

    // ── Public analysis ops ──────────────────────────────────────────────────

    /// Compile every project (and the default project) separately and
    /// return the merged diagnostics; each project only reports diagnostics
    /// for its own files.
    pub async fn compile_all(&self) -> Result<BridgeResponse> {
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
        let mut targets: Vec<Option<String>> = ws.projects.iter().map(|p| Some(p.name.clone())).collect();
        targets.push(None);
        let mut items = Vec::new();
        let mut last_id = 0;
        for target in targets {
            let RequestContext { files, classpath, source_level, options } =
                self.context_for_project(&ws, target.as_deref(), false).await;
            if files.is_empty() {
                continue;
            }
            let own: std::collections::HashSet<String> = files
                .keys()
                .filter(|u| {
                    let owner = Url::parse(u).ok().and_then(|u| ws.project_for_uri(&u)).map(|p| p.name.clone());
                    owner == target
                })
                .cloned()
                .collect();
            let id = next_id();
            last_id = id;
            match self.send(BridgeRequest::Compile { id, files, classpath, source_level, options }).await? {
                BridgeResponse::Diagnostics { items: diags, .. } => {
                    items.extend(diags.into_iter().filter(|d| own.contains(&d.uri)));
                }
                other => return Ok(other),
            }
        }
        Ok(BridgeResponse::Diagnostics { id: last_id, items })
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
        let RequestContext { mut files, classpath, source_level, options } = self.context_for(None).await;
        files.insert(uri.to_string(), content_snapshot);
        self.send(BridgeRequest::Complete {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
            import_prefix,
        }).await
    }

    pub async fn hover(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::Hover {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
        }).await
    }

    /// Resolved DOM and bindings of `uri` (data for semantic tokens).
    pub async fn ast_bindings(&self, uri: &Url) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::AstBindings { id: next_id(), files, classpath, source_level, options, uri: uri.to_string() }).await
    }

    pub async fn navigate(&self, uri: &Url, offset: usize, kind: NavKind) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::Navigate {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
            kind,
        }).await
    }

    pub async fn find_references(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::FindReferences {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn code_action(&self, uri: &Url, range: BridgeRange, diagnostics: Vec<BridgeDiagnostic>) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::CodeAction {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            range,
            diagnostics,
        }).await
    }

    pub async fn signature_help_data(
        &self,
        uri: &Url,
        search_offset: Option<usize>,
        context_offset: Option<usize>,
        fallback_name: Option<String>,
        description: bool,
    ) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::SignatureHelpData {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            search_offset: search_offset.map_or(-1, |o| o as i64),
            context_offset: context_offset.map_or(-1, |o| o as i64),
            fallback_name,
            description,
        }).await
    }

    /// The element at `offset` (UTF-16) in `uri`, resolved against the
    /// project owning `uri`.
    pub async fn rename_target(&self, uri: &Url, offset: usize, ctx: &RequestContext) -> Result<BridgeResponse> {
        self.send(BridgeRequest::RenameTarget {
            id: next_id(),
            files: ctx.files.clone(),
            classpath: ctx.classpath.clone(),
            source_level: ctx.source_level.clone(),
            options: ctx.options.clone(),
            uri: uri.to_string(),
            offset,
        }).await
    }

    /// Occurrences of `names` (and of `package_name`) in `uris`.
    pub async fn rename_occurrences(
        &self,
        ctx: &RequestContext,
        uris: Vec<String>,
        names: Vec<String>,
        package_name: Option<String>,
    ) -> Result<BridgeResponse> {
        self.send(BridgeRequest::RenameOccurrences {
            id: next_id(),
            files: ctx.files.clone(),
            classpath: ctx.classpath.clone(),
            source_level: ctx.source_level.clone(),
            options: ctx.options.clone(),
            uris,
            names,
            package_name,
        }).await
    }

    pub async fn organize_imports(&self, uri: &Url) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::OrganizeImports {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
        }).await
    }

    pub async fn code_lens(&self, uri: &Url) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::CodeLens {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
        }).await
    }

    /// Inlay-hint binding data for `uri` (see `features::inlay_hints`).
    pub async fn inlay_hint_data(&self, uri: &Url, format_parameters: bool) -> Result<InlayHintData> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        let source = files.get(uri.as_str()).cloned().unwrap_or_default();
        let (sourcepath, folder_package) = {
            let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner());
            let path = crate::project::uri_to_path(uri);
            match path.as_deref().and_then(|p| ws.project_for_path(p).map(|proj| (p, proj))) {
                Some((path, project)) => {
                    let sourcepath = ws
                        .project_closure(project)
                        .iter()
                        .flat_map(|p| p.source_folders.iter())
                        .filter(|sf| sf.path.is_dir())
                        .map(|sf| sf.path.to_string_lossy().into_owned())
                        .collect();
                    let package = project.source_folder_for(path).and_then(|sf| {
                        let dir = path.parent()?.strip_prefix(&sf.path).ok()?;
                        let segments: Vec<String> =
                            dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                        Some(segments.join("."))
                    });
                    (sourcepath, package)
                }
                None => (Vec::new(), None),
            }
        };
        let response = self
            .send(BridgeRequest::InlayHints {
                id: next_id(),
                files,
                classpath,
                source_level,
                options,
                uri: uri.to_string(),
                sourcepath,
                format_parameters,
            })
            .await?;
        Ok(InlayHintData { response, source, folder_package })
    }

    /// Run the Eclipse formatter on `source` (see `BridgeRequest::Format`).
    /// `Ok(None)` when the formatter returned `null`.
    pub async fn format_source(
        &self,
        source: &str,
        format_kind: i32,
        offset: usize,
        length: usize,
        line_separator: &str,
        options: BTreeMap<String, String>,
    ) -> Result<Option<Vec<super::semantic::protocol::BridgeFormatEdit>>> {
        match self.send(BridgeRequest::Format {
            id: next_id(),
            source: source.to_owned(),
            format_kind,
            offset,
            length,
            indentation_level: 0,
            line_separator: line_separator.to_owned(),
            options,
        }).await? {
            BridgeResponse::FormatEdits { edits, .. } => Ok(edits),
            BridgeResponse::Error { message, .. } => Err(anyhow!(message)),
            other => Err(anyhow!("unexpected format response: {other:?}")),
        }
    }

    /// The JDT options (and source level) of the project owning `uri`,
    /// without collecting its sources: `context_for(..).options`.
    pub async fn options_for(&self, uri: Option<&Url>) -> (BTreeMap<String, String>, String) {
        let cfg = self.config.read().await.clone();
        let ws = self.workspace.read().unwrap_or_else(|e| e.into_inner());
        let mut options = crate::project::jdtls_default_options();
        options.extend(cfg.compiler_options.clone());
        match uri.and_then(|u| ws.project_for_uri(u)) {
            Some(project) => {
                options.extend(project.options.clone());
                let level = project.compliance().map(str::to_owned).unwrap_or_else(|| cfg.source_compatibility.clone());
                (options, level)
            }
            None => (options, cfg.source_compatibility.clone()),
        }
    }

    pub async fn type_hierarchy_prepare(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::TypeHierarchyPrepare {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn type_hierarchy_supertypes(&self, data: String) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(None).await;
        self.send(BridgeRequest::TypeHierarchySupertypes {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            data,
        }).await
    }

    pub async fn type_hierarchy_subtypes(&self, data: String) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(None).await;
        self.send(BridgeRequest::TypeHierarchySubtypes {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            data,
        }).await
    }

    pub async fn call_hierarchy_prepare(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::CallHierarchyPrepare {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn call_hierarchy_incoming(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::CallHierarchyIncoming {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
            uri: uri.to_string(),
            offset,
        }).await
    }

    pub async fn call_hierarchy_outgoing(&self, uri: &Url, offset: usize) -> Result<BridgeResponse> {
        let RequestContext { files, classpath, source_level, options } = self.context_for(Some(uri)).await;
        self.send(BridgeRequest::CallHierarchyOutgoing {
            id: next_id(),
            files,
            classpath,
            source_level,
            options,
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
