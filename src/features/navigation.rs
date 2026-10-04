//! Navigation: definition, type definition, declaration, implementation,
//! references and document highlight, plus `java/classFileContents` —
//! ports of the jdt.ls handlers (`NavigateToDefinitionHandler`,
//! `NavigateToTypeDefinitionHandler`, `NavigateToDeclarationHandler`,
//! `ImplementationsHandler`, `ReferencesHandler`, `DocumentHighlightHandler`,
//! `ContentProviderManager`).
//!
//! Binding resolution is done by the bridge (`NavigationDataService`), which
//! returns raw locations (a source URI or a class file plus a range).  This
//! module resolves the request target (workspace file, virtual document or
//! `jdt://` class file), applies the jdt.ls preferences, builds `jdt://`
//! URIs and shapes the LSP results.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use once_cell::sync::Lazy;
use serde_json::Value;
use tower_lsp::lsp_types::{DocumentHighlight, DocumentHighlightKind, Location, Position, Range, Url};

use crate::analysis::dispatcher::{Dispatcher, RequestContext};
use crate::analysis::semantic::ecj_process::next_id;
use crate::analysis::semantic::protocol::{BridgeRequest, BridgeResponse, RawLocation};
use crate::classfile::{self, ClassFileDesc, ClassFileRef};
use crate::project::{ProjectKind, Workspace, DEFAULT_PROJECT_NAME};

// ── Preferences ──────────────────────────────────────────────────────────────

/// The jdt.ls preferences navigation depends on.
#[derive(Debug, Clone)]
pub struct NavPrefs {
    /// `extendedClientCapabilities.classFileContentsSupport`
    /// (`PreferenceManager.isClientSupportsClassFileContent`).
    pub class_file_contents_support: bool,
    /// `java.references.includeDecompiledSources`.
    pub include_decompiled_sources: bool,
    /// `java.references.includeAccessors`.
    pub include_accessors: bool,
}

impl Default for NavPrefs {
    fn default() -> Self {
        Self { class_file_contents_support: false, include_decompiled_sources: true, include_accessors: true }
    }
}

static PREFS: Lazy<RwLock<NavPrefs>> = Lazy::new(|| RwLock::new(NavPrefs::default()));

pub fn prefs() -> NavPrefs {
    PREFS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Read the preferences from `initializationOptions`.
pub fn init_preferences(init_options: Option<&Value>) {
    let mut p = NavPrefs::default();
    if let Some(opts) = init_options {
        let ext = opts.get("extendedClientCapabilities");
        if let Some(v) = ext.and_then(|e| e.get("classFileContentsSupport")) {
            p.class_file_contents_support = v.as_bool().unwrap_or_else(|| v.as_str() == Some("true"));
        }
        *PREFS.write().unwrap_or_else(|e| e.into_inner()) = p;
        if let Some(settings) = opts.get("settings") {
            update_settings(settings);
        }
        return;
    }
    *PREFS.write().unwrap_or_else(|e| e.into_inner()) = p;
}

/// Apply `workspace/didChangeConfiguration` settings.
pub fn update_settings(settings: &Value) {
    let refs = settings.get("java").and_then(|j| j.get("references"));
    let mut p = PREFS.write().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = refs {
        if let Some(b) = r.get("includeAccessors").and_then(Value::as_bool) {
            p.include_accessors = b;
        }
        if let Some(b) = r.get("includeDecompiledSources").and_then(Value::as_bool) {
            p.include_decompiled_sources = b;
        }
    }
}

// ── Targets ──────────────────────────────────────────────────────────────────

/// What a request's URI resolves to.
struct Target {
    uri: String,
    /// The class file when the URI is a `jdt://contents/...` URI.
    class_file: Option<(ClassFileDesc, ClassFileRef)>,
    /// Owning project name (`jdt.ls-java-project` for the default project).
    project: String,
    ctx: RequestContext,
}

fn workspace(d: &Dispatcher) -> Workspace {
    d.workspace.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn project_roots(ws: &Workspace) -> Vec<(String, PathBuf)> {
    ws.projects.iter().map(|p| (p.name.clone(), p.root.clone())).collect()
}

/// `JDTUtils.resolveClassFile`: the bridge descriptor of a class-file URI.
pub fn class_file_target(ws: &Workspace, uri: &str) -> Option<(ClassFileDesc, ClassFileRef)> {
    let r = ClassFileRef::parse(uri)?;
    let project_root = ws.project(&r.project).map(|p| p.root.clone());
    let root = classfile::resolve_root_path(&r.root_path, project_root.as_deref(), &project_roots(ws));
    let desc = ClassFileDesc {
        root: root.to_string_lossy().into_owned(),
        module: r.module.clone(),
        package_name: r.package.clone(),
        class_file_name: r.class_file.clone(),
        source_file_name: None,
    };
    Some((desc, r))
}

async fn resolve_target(d: &Dispatcher, uri: &Url) -> Option<Target> {
    let ws = workspace(d);
    if classfile::is_class_file_uri(uri) {
        let (desc, r) = class_file_target(&ws, uri.as_str())?;
        let ctx = d.context_for_project_name(&r.project).await;
        let project = ws.project(&r.project).map(|p| p.name.clone()).unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned());
        return Some(Target { uri: uri.to_string(), class_file: Some((desc, r)), project, ctx });
    }
    let content = d.store.get(uri).map(|s| s.content_string())?;
    let mut ctx = d.context_for(Some(uri)).await;
    ctx.files.insert(uri.to_string(), content);
    let project = ws.project_for_uri(uri).map(|p| p.name.clone()).unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned());
    Some(Target { uri: uri.to_string(), class_file: None, project, ctx })
}

/// Library path → source attachment, for every imported project.
pub fn source_attachments(ws: &Workspace) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for p in &ws.projects {
        for lib in &p.libraries {
            if let Some(src) = &lib.source {
                out.insert(lib.path.to_string_lossy().into_owned(), src.to_string_lossy().into_owned());
            }
        }
    }
    out
}

/// `JDTUtils.toUri(IClassFile)` for a class file seen from `project`.
pub fn class_file_uri(ws: &Workspace, project: &str, desc: &ClassFileDesc) -> String {
    let proj = ws.project(project);
    let root = PathBuf::from(&desc.root);
    let root_path = if desc.module.is_some() {
        desc.root.clone()
    } else {
        classfile::memento_root_path(&root, proj.map(|p| p.root.as_path()), &project_roots(ws))
    };
    let attributes = match proj {
        Some(p) if desc.module.is_some() => {
            // JRE container library: JDT adds the javadoc location, then the
            // container entry's own attributes.
            let mut a = Vec::new();
            if let Some(url) = classfile::jdk_javadoc_location(&root) {
                a.push(("javadoc_location".to_owned(), url));
            }
            a.extend(match p.kind {
                ProjectKind::Maven => vec![("maven.pomderived".to_owned(), "true".to_owned())],
                ProjectKind::Eclipse => classfile::eclipse_container_attributes(&p.root),
                _ => Vec::new(),
            });
            a
        }
        None if desc.module.is_some() => classfile::jdk_javadoc_location(&root)
            .map(|url| vec![("javadoc_location".to_owned(), url)])
            .unwrap_or_default(),
        Some(p) => match p.kind {
            ProjectKind::Maven => p
                .libraries
                .iter()
                .find(|l| l.path == root)
                .map(|lib| classfile::maven_attributes(&root, lib.is_test, &crate::project::maven::local_repository()))
                .unwrap_or_default(),
            ProjectKind::Eclipse => classfile::eclipse_library_attributes(&p.root, &root),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    ClassFileRef {
        project: proj.map(|p| p.name.clone()).unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned()),
        root_path,
        module: desc.module.clone(),
        attributes,
        package: desc.package_name.clone(),
        class_file: desc.class_file_name.clone(),
        source_file_name: desc.source_file_name.clone(),
    }
    .to_uri()
}

fn same_class_file(a: &ClassFileDesc, b: &ClassFileDesc) -> bool {
    Path::new(&a.root) == Path::new(&b.root)
        && a.module == b.module
        && a.package_name == b.package_name
        && a.class_file_name == b.class_file_name
}

/// Raw bridge location → LSP location (`None` for class files when the client
/// lacks class file content support, like `JDTUtils.toUri` returning null).
fn to_location(ws: &Workspace, target: &Target, raw: &RawLocation, support: bool) -> Option<Location> {
    let uri = if let Some(cf) = &raw.class_file {
        if !support {
            return None;
        }
        match &target.class_file {
            Some((desc, r)) if same_class_file(desc, cf) => {
                let mut r = r.clone();
                if r.source_file_name.is_none() {
                    r.source_file_name = cf.source_file_name.clone();
                }
                r.to_uri()
            }
            _ => class_file_uri(ws, &target.project, cf),
        }
    } else {
        raw.uri.clone()?
    };
    Some(Location {
        uri: Url::parse(&uri).ok()?,
        range: Range {
            start: Position { line: raw.start_line, character: raw.start_char },
            end: Position { line: raw.end_line, character: raw.end_char },
        },
    })
}

struct NavOutcome {
    locations: Vec<Location>,
    raw: Vec<RawLocation>,
    null_result: bool,
}

async fn nav(d: &Dispatcher, uri: &Url, pos: Position, op: &str, include_declaration: bool) -> Option<NavOutcome> {
    if !d.is_ecj_ready().await {
        return None;
    }
    let target = resolve_target(d, uri).await?;
    let ws = workspace(d);
    let p = prefs();
    let RequestContext { files, classpath, source_level, options } = clone_ctx(&target.ctx);
    let req = BridgeRequest::NavData {
        id: next_id(),
        files,
        classpath,
        source_level,
        options,
        uri: target.uri.clone(),
        op: op.to_owned(),
        line: pos.line,
        character: pos.character,
        class_file: target.class_file.as_ref().map(|(d, _)| d.clone()),
        source_attachments: source_attachments(&ws),
        include_class_files: p.class_file_contents_support,
        include_decompiled: p.include_decompiled_sources,
        include_declaration,
        include_accessors: p.include_accessors,
        libraries: None,
        skip_libraries: Vec::new(),
        search_keys: Vec::new(),
    };
    match d.send_request(req).await {
        Ok(BridgeResponse::NavData { locations, null_result, .. }) => {
            let mut out: Vec<Location> = Vec::new();
            for raw in &locations {
                if let Some(l) = to_location(&ws, &target, raw, p.class_file_contents_support) {
                    if !out.contains(&l) || op == "highlight" {
                        out.push(l);
                    }
                }
            }
            Some(NavOutcome { locations: out, raw: locations, null_result })
        }
        Ok(BridgeResponse::Error { message, .. }) => {
            tracing::warn!("navData {op} failed: {message}");
            None
        }
        Ok(other) => {
            tracing::warn!("navData {op}: unexpected response {other:?}");
            None
        }
        Err(e) => {
            tracing::warn!("navData {op} error: {e}");
            None
        }
    }
}

fn clone_ctx(c: &RequestContext) -> RequestContext {
    RequestContext {
        files: c.files.clone(),
        classpath: c.classpath.clone(),
        source_level: c.source_level.clone(),
        options: c.options.clone(),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `textDocument/definition` (`NavigateToDefinitionHandler.definition`):
/// a list, empty when nothing is found.  `None` means the bridge could not
/// answer (the caller may fall back to syntax-only navigation).
pub async fn definition(d: &Dispatcher, uri: &Url, pos: Position) -> Option<Vec<Location>> {
    if !is_resolvable(d, uri) {
        return Some(Vec::new());
    }
    nav(d, uri, pos, "definition", false).await.map(|o| o.locations)
}

/// `textDocument/typeDefinition` (`NavigateToTypeDefinitionHandler`): `null` when nothing is found.
pub async fn type_definition(d: &Dispatcher, uri: &Url, pos: Position) -> Option<Option<Vec<Location>>> {
    if !is_resolvable(d, uri) {
        return Some(None);
    }
    nav(d, uri, pos, "typeDefinition", false).await.map(|o| {
        if o.null_result || o.locations.is_empty() {
            None
        } else {
            Some(o.locations)
        }
    })
}

/// `textDocument/declaration` (`NavigateToDeclarationHandler`).
pub async fn declaration(d: &Dispatcher, uri: &Url, pos: Position) -> Option<Vec<Location>> {
    if !is_resolvable(d, uri) {
        return Some(Vec::new());
    }
    nav(d, uri, pos, "declaration", false).await.map(|o| o.locations)
}

/// `textDocument/implementation` (`ImplementationsHandler.findImplementations`).
pub async fn implementation(d: &Dispatcher, uri: &Url, pos: Position) -> Option<Vec<Location>> {
    if !is_resolvable(d, uri) {
        return Some(Vec::new());
    }
    nav(d, uri, pos, "implementation", false).await.map(|o| o.locations)
}

/// `textDocument/references` (`ReferencesHandler.findReferences`).
pub async fn references(d: &Dispatcher, uri: &Url, pos: Position, include_declaration: bool) -> Option<Vec<Location>> {
    if !is_resolvable(d, uri) {
        return Some(Vec::new());
    }
    nav(d, uri, pos, "references", include_declaration).await.map(|o| o.locations)
}

/// `textDocument/documentHighlight` (`DocumentHighlightHandler.documentHighlight`).
pub async fn document_highlight(d: &Dispatcher, uri: &Url, pos: Position) -> Option<Vec<DocumentHighlight>> {
    if !is_resolvable(d, uri) {
        return Some(Vec::new());
    }
    let o = nav(d, uri, pos, "highlight", false).await?;
    Some(
        o.raw
            .iter()
            .map(|r| DocumentHighlight {
                range: Range {
                    start: Position { line: r.start_line, character: r.start_char },
                    end: Position { line: r.end_line, character: r.end_char },
                },
                kind: Some(match r.kind {
                    3 => DocumentHighlightKind::WRITE,
                    1 => DocumentHighlightKind::TEXT,
                    _ => DocumentHighlightKind::READ,
                }),
            })
            .collect(),
    )
}

/// Whether the URI can name a type root at all (an open/workspace document
/// or a class file); anything else yields the handlers' empty answer.
fn is_resolvable(d: &Dispatcher, uri: &Url) -> bool {
    classfile::is_class_file_uri(uri) || d.store.contains(uri)
}

/// `java/classFileContents` (`ContentProviderManager.getContent`): attached
/// source, else the FernFlower-decompiled class, else "".
pub async fn class_file_contents(d: &Dispatcher, uri: &str) -> String {
    if !d.is_ecj_ready().await {
        return String::new();
    }
    let ws = workspace(d);
    let desc = if uri.starts_with("jdt:") {
        match class_file_target(&ws, uri) {
            Some((desc, _)) => desc,
            None => return String::new(),
        }
    } else {
        // A `.class` file on disk.
        let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else { return String::new() };
        if !path.is_file() || path.extension().is_none_or(|e| e != "class") {
            return String::new();
        }
        ClassFileDesc {
            root: path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            module: None,
            package_name: String::new(),
            class_file_name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            source_file_name: None,
        }
    };
    let req = BridgeRequest::ClassFileContents { id: next_id(), class_file: desc, source_attachments: source_attachments(&ws) };
    match d.send_request(req).await {
        Ok(BridgeResponse::ClassFileContents { contents, .. }) => contents,
        _ => String::new(),
    }
}

/// `ClassFileUtil.getURI(project, fqn)`: the URI of a source or binary type
/// (backs the `jdtls-rust.classFileUri` command used by the test harness).
pub async fn type_uri(d: &Dispatcher, project: &str, fqn: &str) -> Option<String> {
    if !d.is_ecj_ready().await {
        return None;
    }
    let ws = workspace(d);
    let RequestContext { files, classpath, source_level, .. } = d.context_for_project_name(project).await;
    let req = BridgeRequest::ClassFileInfo { id: next_id(), files, classpath, source_level, fqn: fqn.to_owned() };
    match d.send_request(req).await {
        Ok(BridgeResponse::ClassFileInfo { source_uri: Some(uri), .. }) => Some(uri),
        Ok(BridgeResponse::ClassFileInfo { class_file: Some(desc), .. }) => {
            let name = ws.project(project).map(|p| p.name.clone()).unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned());
            Some(class_file_uri(&ws, &name, &desc))
        }
        _ => None,
    }
}
