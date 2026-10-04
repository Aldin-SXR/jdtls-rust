//! `textDocument/rename` and `textDocument/prepareRename`, ported from
//! jdt.ls `RenameHandler` / `PrepareRenameHandler`.
//!
//! jdt.ls renames through the JDT refactorings (`RenameSupport`).  Here the
//! bridge only resolves bindings: the element at the cursor and, for the
//! identifiers involved, every name occurrence in the workspace with its
//! binding key (plus method override relations).  Selecting the occurrences
//! to rename, the refactoring checks and the `WorkspaceEdit` shape
//! (`changes`, or `documentChanges` with resource operations when the client
//! supports them, like `ChangeUtil.convertToWorkspaceEdit`) are computed here.

use crate::analysis::dispatcher::{Dispatcher, RequestContext};
use crate::analysis::semantic::protocol::{
    BridgeRenameElement, BridgeRenameFile, BridgeRenameMethod, BridgeResponse,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tower_lsp::jsonrpc::{Error, ErrorCode, Result as LspResult};
use tower_lsp::lsp_types::*;

const NOT_SUPPORTED: &str = "Renaming this element is not supported.";

/// `ResponseErrorCode.InvalidRequest` with `message`.
fn invalid_request(message: impl Into<String>) -> Error {
    Error { code: ErrorCode::InvalidRequest, message: Cow::Owned(message.into()), data: None }
}

/// Client capabilities relevant to rename.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenameClient {
    /// `ClientPreferences.isResourceOperationSupported`: the client supports
    /// the `create`, `rename` and `delete` resource operations.
    pub resource_operations: bool,
}

impl RenameClient {
    pub fn from_capabilities(caps: &ClientCapabilities) -> Self {
        let ops = caps
            .workspace
            .as_ref()
            .and_then(|w| w.workspace_edit.as_ref())
            .and_then(|e| e.resource_operations.as_ref());
        let resource_operations = ops.is_some_and(|ops| {
            ops.contains(&ResourceOperationKind::Create)
                && ops.contains(&ResourceOperationKind::Rename)
                && ops.contains(&ResourceOperationKind::Delete)
        });
        Self { resource_operations }
    }
}

/// `java.rename.enabled` (default `true`).
pub fn rename_enabled(settings: Option<&serde_json::Value>) -> bool {
    settings
        .and_then(|s| s.pointer("/java/rename/enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

// ─── Text positions (UTF-16, like JDT offsets) ───────────────────────────────

struct LineIndex<'a> {
    text: &'a str,
    /// (byte offset, utf16 offset) of every line start.
    starts: Vec<(usize, usize)>,
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![(0, 0)];
        let mut utf16 = 0usize;
        let mut it = text.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            utf16 += c.len_utf16();
            match c {
                '\n' => starts.push((i + 1, utf16)),
                '\r' => {
                    if let Some(&(_, '\n')) = it.peek() {
                        let (j, _) = it.next().unwrap();
                        utf16 += 1;
                        starts.push((j + 1, utf16));
                    } else {
                        starts.push((i + 1, utf16));
                    }
                }
                _ => {}
            }
        }
        Self { text, starts }
    }

    /// UTF-16 offset of an LSP position (clamped to the line).
    fn offset(&self, pos: Position) -> usize {
        let line = (pos.line as usize).min(self.starts.len() - 1);
        let (byte, utf16) = self.starts[line];
        let end = self.starts.get(line + 1).map_or(self.text.len(), |s| s.0);
        let mut units = 0usize;
        for c in self.text[byte..end].chars() {
            if units >= pos.character as usize || c == '\n' || c == '\r' {
                break;
            }
            units += c.len_utf16();
        }
        utf16 + units
    }

    fn position(&self, offset: usize) -> Position {
        let line = match self.starts.binary_search_by(|s| s.1.cmp(&offset)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        Position { line: line as u32, character: (offset - self.starts[line].1) as u32 }
    }

    fn range(&self, start: usize, length: usize) -> Range {
        Range { start: self.position(start), end: self.position(start + length) }
    }
}

// ─── Names ───────────────────────────────────────────────────────────────────

const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const",
    "continue", "default", "do", "double", "else", "enum", "extends", "final", "finally", "float",
    "for", "goto", "if", "implements", "import", "instanceof", "int", "interface", "long", "native",
    "new", "package", "private", "protected", "public", "return", "short", "static", "strictfp",
    "super", "switch", "synchronized", "this", "throw", "throws", "transient", "try", "void",
    "volatile", "while", "true", "false", "null", "_",
];

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !KEYWORDS.contains(&name)
}

/// `Checks.checkName` / `JavaConventions.validate*Name`: a fatal error for
/// names that are not Java identifiers.
fn check_identifier(name: &str) -> Result<(), Error> {
    if is_identifier(name) {
        Ok(())
    } else {
        Err(invalid_request(format!("'{name}' is not a valid Java identifier")))
    }
}

fn check_package_name(name: &str) -> Result<(), Error> {
    if !name.is_empty() && name.split('.').all(is_identifier) {
        Ok(())
    } else {
        Err(invalid_request(format!("'{name}' is not a valid Java identifier")))
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `NamingConventions` base name of a boolean field (`isFoo` → `foo`).
fn boolean_base(name: &str) -> &str {
    match name.strip_prefix("is") {
        Some(rest) if rest.chars().next().is_some_and(char::is_uppercase) => rest,
        _ => name,
    }
}

/// `GetterSetterUtil.getGetterName` candidates (primary first).
fn getter_names(field: &str, is_boolean: bool) -> Vec<String> {
    if is_boolean {
        let base = boolean_base(field);
        vec![format!("is{}", capitalize(base)), format!("get{}", capitalize(base))]
    } else {
        vec![format!("get{}", capitalize(field))]
    }
}

fn setter_name(field: &str, is_boolean: bool) -> String {
    let base = if is_boolean { boolean_base(field) } else { field };
    format!("set{}", capitalize(base))
}

fn file_name(uri: &str) -> String {
    let path = Url::parse(uri).map(|u| u.path().to_owned()).unwrap_or_else(|_| uri.to_owned());
    path.rsplit('/').next().unwrap_or("").to_owned()
}

fn file_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok().filter(|u| u.scheme() == "file").and_then(|u| u.to_file_path().ok())
}

fn file_uri(path: &Path) -> Option<Url> {
    Url::from_file_path(path).ok()
}

// ─── Bridge access ───────────────────────────────────────────────────────────

struct Target {
    select: Option<BridgeRenameElement>,
    prepare: Option<BridgeRenameElement>,
    package_name: String,
}

async fn resolve_target(dispatcher: &Dispatcher, uri: &Url, offset: usize, ctx: &RequestContext) -> Option<Target> {
    match dispatcher.rename_target(uri, offset, ctx).await {
        Ok(BridgeResponse::RenameTarget { select, prepare, package_name, .. }) => {
            Some(Target { select, prepare, package_name: package_name.unwrap_or_default() })
        }
        Ok(BridgeResponse::Error { message, .. }) => {
            tracing::warn!("rename: bridge error resolving target: {message}");
            None
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("rename: bridge request failed: {e}");
            None
        }
    }
}

/// Occurrence data merged over the owning project and its dependents.
#[derive(Default)]
struct Occurrences {
    files: BTreeMap<String, BridgeRenameFile>,
    methods: HashMap<String, BridgeRenameMethod>,
    relations: Vec<(String, String)>,
    /// Text of every file seen, for offset conversion.
    texts: HashMap<String, String>,
}

impl Occurrences {
    /// Methods related to `start` through overriding (`RippleMethodFinder`).
    fn ripple(&self, start: &str) -> HashSet<String> {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in &self.relations {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let mut seen: HashSet<String> = HashSet::from([start.to_owned()]);
        let mut queue = VecDeque::from([start.to_owned()]);
        while let Some(k) = queue.pop_front() {
            for n in adj.get(k.as_str()).into_iter().flatten() {
                if seen.insert((*n).to_owned()) {
                    queue.push_back((*n).to_owned());
                }
            }
        }
        seen
    }
}

async fn collect_occurrences(
    dispatcher: &Dispatcher,
    contexts: &[RequestContext],
    names: &[String],
    package: Option<&str>,
    only: Option<&str>,
) -> Occurrences {
    let mut out = Occurrences::default();
    // A package reference contains its last segment.
    let needles: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .chain(package.and_then(|p| p.rsplit('.').next()))
        .collect();
    for ctx in contexts {
        let mut uris: Vec<String> = ctx
            .files
            .iter()
            .filter(|(u, text)| {
                !out.files.contains_key(*u)
                    && only.is_none_or(|o| o == u.as_str())
                    && needles.iter().any(|n| text.contains(n))
            })
            .map(|(u, _)| u.clone())
            .collect();
        uris.sort();
        if uris.is_empty() {
            continue;
        }
        for u in &uris {
            out.texts.insert(u.clone(), ctx.files[u].clone());
        }
        match dispatcher.rename_occurrences(ctx, uris, names.to_vec(), package.map(str::to_owned)).await {
            Ok(BridgeResponse::RenameOccurrences { files, methods, relations, .. }) => {
                for f in files {
                    out.files.entry(f.uri.clone()).or_insert(f);
                }
                for m in methods {
                    out.methods.entry(m.key.clone()).or_insert(m);
                }
                for r in relations {
                    if let [a, b] = r.as_slice() {
                        out.relations.push((a.clone(), b.clone()));
                    }
                }
            }
            Ok(BridgeResponse::Error { message, .. }) => tracing::warn!("rename: bridge error: {message}"),
            Ok(_) => {}
            Err(e) => tracing::warn!("rename: bridge request failed: {e}"),
        }
    }
    out
}

async fn contexts_for(dispatcher: &Dispatcher, uri: &Url, text: &str) -> Vec<RequestContext> {
    let mut ctx = dispatcher.context_for(Some(uri)).await;
    ctx.files.entry(uri.to_string()).or_insert_with(|| text.to_owned());
    let mut out = vec![ctx];
    out.extend(dispatcher.dependent_contexts(uri).await);
    out
}

// ─── prepareRename ───────────────────────────────────────────────────────────

/// `PrepareRenameHandler.prepareRename`: the range of the name at the
/// position, or an `InvalidRequest` error when it cannot be renamed.
pub async fn prepare_rename(dispatcher: &Dispatcher, uri: &Url, text: &str, pos: Position) -> LspResult<Range> {
    if !dispatcher.is_ecj_ready().await {
        return Err(invalid_request(NOT_SUPPORTED));
    }
    let index = LineIndex::new(text);
    let offset = index.offset(pos);
    let mut ctx = dispatcher.context_for(Some(uri)).await;
    ctx.files.entry(uri.to_string()).or_insert_with(|| text.to_owned());
    let target = resolve_target(dispatcher, uri, offset, &ctx).await;
    let Some(el) = target.and_then(|t| t.prepare) else {
        return Err(invalid_request(NOT_SUPPORTED));
    };
    if el.name_start < 0 || !is_rename_available(&el) {
        return Err(invalid_request(NOT_SUPPORTED));
    }
    Ok(index.range(el.name_start as usize, el.name_length as usize))
}

/// `PrepareRenameHandler.isBinaryOrPackage` negated:
/// `RefactoringAvailabilityTesterCore.isRenameElementAvailable`.
fn is_rename_available(el: &BridgeRenameElement) -> bool {
    if el.recovered {
        return false;
    }
    match el.kind.as_deref() {
        Some("local") => true,
        Some("typeParameter") | Some("field") | Some("enumConstant") => el.from_source,
        Some("type") => el.from_source && !el.anonymous && el.package_name.as_deref() != Some("java.lang"),
        Some("method") => el.from_source && !is_to_string(el),
        _ => false,
    }
}

/// `RefactoringAvailabilityTesterCore.isRenameProhibited(IMethod)`.
fn is_to_string(el: &BridgeRenameElement) -> bool {
    el.name.as_deref() == Some("toString")
        && el.param_count == 0
        && el.type_name.as_deref() == Some("java.lang.String")
}

// ─── rename ──────────────────────────────────────────────────────────────────

/// Text edits per file (UTF-16 start, length, new text) plus resource
/// operations, in `ChangeUtil` order.
#[derive(Default)]
struct RenameChange {
    /// Edits of the refactoring's `TextChangeManager`.
    edits: HashMap<String, Vec<(usize, usize, String)>>,
    /// Text edits converted from resource changes (package declarations),
    /// emitted after the text changes.
    resource_edits: Vec<(String, Vec<(usize, usize, String)>)>,
    operations: Vec<ResourceOp>,
}

impl RenameChange {
    fn add(&mut self, uri: &str, start: usize, length: usize, text: &str) {
        let v = self.edits.entry(uri.to_owned()).or_default();
        if !v.iter().any(|e| e.0 == start) {
            v.push((start, length, text.to_owned()));
        }
    }
}

/// `RenameHandler.rename`.
pub async fn rename(
    dispatcher: &Dispatcher,
    uri: &Url,
    text: &str,
    pos: Position,
    new_name: &str,
    client: RenameClient,
    enabled: bool,
) -> LspResult<WorkspaceEdit> {
    let empty = WorkspaceEdit { changes: Some(HashMap::new()), ..Default::default() };
    if !enabled || !dispatcher.is_ecj_ready().await {
        return Ok(empty);
    }
    let index = LineIndex::new(text);
    let offset = index.offset(pos);
    let contexts = contexts_for(dispatcher, uri, text).await;
    let Some(target) = resolve_target(dispatcher, uri, offset, &contexts[0]).await else {
        return Ok(empty);
    };
    let Some(el) = target.select.clone() else { return Ok(empty) };
    let (Some(kind), Some(key), Some(name)) = (el.kind.clone(), el.key.clone(), el.name.clone()) else {
        return Ok(empty);
    };
    let uri_s = uri.to_string();

    let change = match kind.as_str() {
        "local" | "typeParameter" => {
            check_new_name(&name, new_name)?;
            if !el.from_source {
                return Err(read_only(&name));
            }
            let occ = collect_occurrences(dispatcher, &contexts[..1], &[name.clone()], None, Some(&uri_s)).await;
            let mut change = RenameChange::default();
            for f in occ.files.values() {
                for o in &f.occurrences {
                    if o.kind.as_deref() == Some(kind.as_str()) && o.key.as_deref() == Some(key.as_str()) {
                        change.add(&f.uri, o.start, o.length, new_name);
                    }
                }
            }
            (change, occ.texts)
        }
        "field" | "enumConstant" => rename_field(dispatcher, &contexts, &el, &key, &name, new_name).await?,
        "method" => rename_method(dispatcher, &contexts, &el, &key, &name, new_name).await?,
        "type" => rename_type(dispatcher, &contexts, &el, &key, &name, new_name, client).await?,
        "package" => {
            let pkg = el.package_name.clone().unwrap_or(name.clone());
            rename_package(dispatcher, &contexts, uri, &target.package_name, &pkg, new_name, client).await?
        }
        _ => return Ok(empty),
    };
    Ok(to_workspace_edit(change.0, &change.1, client))
}

fn read_only(name: &str) -> Error {
    invalid_request(format!("'{name}' is read only."))
}

/// `checkNewElementName`: a valid identifier different from the current name.
fn check_new_name(current: &str, new_name: &str) -> Result<(), Error> {
    check_identifier(new_name)?;
    if current == new_name {
        return Err(invalid_request("Choose another name."));
    }
    Ok(())
}

/// `RenameFieldProcessor` (with `UPDATE_GETTER_METHOD | UPDATE_SETTER_METHOD`)
/// and `RenameEnumConstProcessor`.
async fn rename_field(
    dispatcher: &Dispatcher,
    contexts: &[RequestContext],
    el: &BridgeRenameElement,
    key: &str,
    name: &str,
    new_name: &str,
) -> Result<(RenameChange, HashMap<String, String>), Error> {
    check_new_name(name, new_name)?;
    if !el.from_source {
        return Err(read_only(name));
    }
    let is_enum = el.kind.as_deref() == Some("enumConstant");
    let is_boolean = el.type_name.as_deref() == Some("boolean");
    let accessors = !is_enum && !el.record_component;
    let mut names = vec![name.to_owned()];
    if accessors {
        names.extend(getter_names(name, is_boolean));
        names.push(setter_name(name, is_boolean));
    }
    let occ = collect_occurrences(dispatcher, contexts, &names, None, None).await;
    let declaring = el.declaring_type_key.as_deref();
    let mut change = RenameChange::default();
    for f in occ.files.values() {
        for o in &f.occurrences {
            if matches!(o.kind.as_deref(), Some("field") | Some("enumConstant")) && o.key.as_deref() == Some(key) {
                change.add(&f.uri, o.start, o.length, new_name);
            }
        }
    }
    let declared = |m: &&BridgeRenameMethod, n: &str| m.declaring_type_key.as_deref() == declaring && m.name.as_deref() == Some(n);
    let mut method_renames: Vec<(HashSet<String>, String)> = Vec::new();
    if accessors {
        // GetterSetterUtil.getGetter / getSetter
        let getter = getter_names(name, is_boolean).into_iter().enumerate().find_map(|(i, g)| {
            occ.methods
                .values()
                .find(|m| declared(m, &g) && m.param_types.as_ref().is_none_or(|p| p.is_empty()))
                .map(|m| (m.key.clone(), i))
        });
        if let Some((gk, i)) = getter {
            let new_getter = getter_names(new_name, is_boolean).swap_remove(i);
            method_renames.push((occ.ripple(&gk), new_getter));
        }
        let setter = setter_name(name, is_boolean);
        let field_type = el.type_name.clone().unwrap_or_default();
        if let Some(m) = occ.methods.values().find(|m| {
            declared(m, &setter) && m.param_types.as_ref().is_some_and(|p| p.len() == 1 && p[0] == field_type)
        }) {
            method_renames.push((occ.ripple(&m.key), setter_name(new_name, is_boolean)));
        }
    } else if el.record_component {
        // Record accessor (implicit or explicit) and the methods it implements.
        let mut keys = HashSet::new();
        for m in occ.methods.values().filter(|m| declared(m, name) && m.param_types.as_ref().is_none_or(|p| p.is_empty())) {
            keys.extend(occ.ripple(&m.key));
        }
        for f in occ.files.values() {
            for o in &f.occurrences {
                if o.kind.as_deref() == Some("method")
                    && o.declaring_type_key.as_deref() == declaring
                    && o.name.as_deref() == Some(name)
                    && o.param_count == 0
                {
                    if let Some(k) = &o.key {
                        keys.insert(k.clone());
                    }
                }
            }
        }
        method_renames.push((keys, new_name.to_owned()));
    }
    for (keys, to) in &method_renames {
        let keys: HashSet<&String> = keys.iter().filter(|k| occ.methods.get(*k).is_none_or(|m| m.from_source)).collect();
        for f in occ.files.values() {
            for o in &f.occurrences {
                if o.kind.as_deref() == Some("method") && o.key.as_ref().is_some_and(|k| keys.contains(k)) {
                    change.add(&f.uri, o.start, o.length, to);
                }
            }
        }
    }
    Ok((change, occ.texts))
}

/// `RenameVirtualMethodProcessor` / `RenameNonVirtualMethodProcessor`.
async fn rename_method(
    dispatcher: &Dispatcher,
    contexts: &[RequestContext],
    el: &BridgeRenameElement,
    key: &str,
    name: &str,
    new_name: &str,
) -> Result<(RenameChange, HashMap<String, String>), Error> {
    if !el.from_source {
        return Err(read_only(name));
    }
    check_new_name(name, new_name)?;
    let occ = collect_occurrences(dispatcher, contexts, &[name.to_owned()], None, None).await;
    let methods = if el.is_static || el.is_private { HashSet::from([key.to_owned()]) } else { occ.ripple(key) };
    let mut binary: Vec<&BridgeRenameMethod> =
        methods.iter().filter_map(|k| occ.methods.get(k)).filter(|m| !m.from_source).collect();
    binary.sort_by(|a, b| a.key.cmp(&b.key));
    if let Some(m) = binary.first() {
        return Err(invalid_request(format!(
            "Related method '{}' (declared in '{}') is binary. Refactoring cannot be performed.",
            m.name.as_deref().unwrap_or(name),
            m.declaring_type_name.as_deref().unwrap_or("")
        )));
    }
    let mut change = RenameChange::default();
    for f in occ.files.values() {
        for o in &f.occurrences {
            if o.kind.as_deref() == Some("method") && o.key.as_ref().is_some_and(|k| methods.contains(k)) {
                change.add(&f.uri, o.start, o.length, new_name);
            }
        }
    }
    Ok((change, occ.texts))
}

/// `RenameTypeProcessor` (jdt.ls `RenameSupport.create(IType, …)`): the type,
/// its references and constructors, and the compilation unit when the type
/// is its primary type.
async fn rename_type(
    dispatcher: &Dispatcher,
    contexts: &[RequestContext],
    el: &BridgeRenameElement,
    key: &str,
    name: &str,
    new_name: &str,
    client: RenameClient,
) -> Result<(RenameChange, HashMap<String, String>), Error> {
    if !el.from_source || el.anonymous {
        return Err(read_only(name));
    }
    check_new_name(name, new_name)?;
    let occ = collect_occurrences(dispatcher, contexts, &[name.to_owned()], None, None).await;
    let mut change = RenameChange::default();
    let mut declaring_uri = None;
    for f in occ.files.values() {
        for o in &f.occurrences {
            if o.kind.as_deref() == Some("type") && o.key.as_deref() == Some(key) {
                if o.role.as_deref() == Some("decl") {
                    declaring_uri.get_or_insert_with(|| f.uri.clone());
                }
                change.add(&f.uri, o.start, o.length, new_name);
            }
        }
    }
    // The compilation unit is renamed with its primary type.
    if let Some(cu) = declaring_uri.filter(|u| el.top_level && file_name(u) == format!("{name}.java")) {
        let new_cu = format!("{new_name}.java");
        if let Some(path) = file_path(&cu) {
            let new_path = path.with_file_name(&new_cu);
            let new_uri = file_uri(&new_path);
            let exists_in_workspace = new_uri
                .as_ref()
                .is_some_and(|u| contexts.iter().any(|c| c.files.contains_key(&u.to_string())));
            if new_path.exists() || exists_in_workspace {
                return Err(invalid_request(format!("Compilation unit '{new_cu}' already exists")));
            }
            if client.resource_operations {
                if let (Ok(old_uri), Some(new_uri)) = (Url::parse(&cu), new_uri) {
                    change.operations.push(ResourceOp::Rename(RenameFile {
                        old_uri,
                        new_uri,
                        options: None,
                        annotation_id: None,
                    }));
                }
            }
        }
    }
    Ok((change, occ.texts))
}

/// `RenamePackageProcessor` (without subpackages) plus jdt.ls
/// `ChangeUtil.convertRenamePackcageChange`.
async fn rename_package(
    dispatcher: &Dispatcher,
    contexts: &[RequestContext],
    uri: &Url,
    unit_package: &str,
    pkg: &str,
    new_name: &str,
    client: RenameClient,
) -> Result<(RenameChange, HashMap<String, String>), Error> {
    check_package_name(new_name)?;
    if pkg == new_name {
        return Err(invalid_request("Choose another name."));
    }
    let occ = collect_occurrences(dispatcher, contexts, &[], Some(pkg), None).await;
    // The package fragment: the unit's own package when it is the one
    // selected, else the first source folder declaring it.
    let units_of = |p: &str| -> Vec<&BridgeRenameFile> {
        occ.files.values().filter(|f| f.package_name.as_deref() == Some(p)).collect()
    };
    let declaring = units_of(pkg);
    let pkg_dir = if unit_package == pkg {
        file_path(uri.as_str()).and_then(|p| p.parent().map(Path::to_path_buf))
    } else {
        declaring.iter().filter_map(|f| file_path(&f.uri)).filter_map(|p| p.parent().map(Path::to_path_buf)).next()
    };
    if unit_package != pkg && declaring.is_empty() {
        return Err(read_only(pkg));
    }
    if !units_of(new_name).is_empty() {
        return Err(invalid_request(format!("Package '{new_name}' already exists in this project.")));
    }
    let mut change = RenameChange::default();
    for f in occ.files.values() {
        for o in &f.occurrences {
            if o.kind.as_deref() == Some("package") && o.key.as_deref() == Some(pkg) && o.role.as_deref() != Some("packageDecl") {
                change.add(&f.uri, o.start, o.length, new_name);
            }
        }
    }
    if !client.resource_operations {
        return Ok((change, occ.texts));
    }
    let Some(pkg_dir) = pkg_dir else { return Ok((change, occ.texts)) };
    let mut units: Vec<&BridgeRenameFile> = declaring
        .into_iter()
        .filter(|f| file_path(&f.uri).is_some_and(|p| p.parent() == Some(pkg_dir.as_path())))
        .collect();
    units.sort_by_key(|f| file_name(&f.uri));
    // Package declarations of the moved units.
    for f in &units {
        let edits: Vec<(usize, usize, String)> = f
            .occurrences
            .iter()
            .filter(|o| o.kind.as_deref() == Some("package") && o.role.as_deref() == Some("packageDecl"))
            .map(|o| (o.start, o.length, new_name.to_owned()))
            .collect();
        if !edits.is_empty() {
            change.resource_edits.push((f.uri.clone(), edits));
        }
    }
    let old_segments = pkg.split('.').count();
    let mut new_dir = pkg_dir.clone();
    for _ in 0..old_segments {
        new_dir.pop();
    }
    for seg in new_name.split('.') {
        new_dir.push(seg);
    }
    let temp = new_dir.join(".temp");
    if let Some(temp_uri) = file_uri(&temp) {
        change.operations.push(ResourceOp::Create(CreateFile {
            uri: temp_uri.clone(),
            options: Some(CreateFileOptions { overwrite: Some(false), ignore_if_exists: Some(true) }),
            annotation_id: None,
        }));
        for f in &units {
            let (Ok(old_uri), Some(new_uri)) = (Url::parse(&f.uri), file_uri(&new_dir.join(file_name(&f.uri)))) else {
                continue;
            };
            change.operations.push(ResourceOp::Rename(RenameFile { old_uri, new_uri, options: None, annotation_id: None }));
        }
        change.operations.push(ResourceOp::Delete(DeleteFile {
            uri: temp_uri,
            options: Some(DeleteFileOptions { recursive: Some(false), ignore_if_not_exists: Some(true), annotation_id: None }),
        }));
    }
    Ok((change, occ.texts))
}

/// `ChangeUtil.convertToWorkspaceEdit`: text changes sorted by compilation
/// unit name (`TextChangeManager.getAllChanges`), then the resource changes.
fn to_workspace_edit(change: RenameChange, texts: &HashMap<String, String>, client: RenameClient) -> WorkspaceEdit {
    let to_text_edits = |uri: &str, mut edits: Vec<(usize, usize, String)>| -> Vec<TextEdit> {
        edits.sort_by_key(|e| e.0);
        let text = texts.get(uri).map(String::as_str).unwrap_or("");
        let index = LineIndex::new(text);
        edits.into_iter().map(|(s, l, t)| TextEdit { range: index.range(s, l), new_text: t }).collect()
    };
    let mut files: Vec<(String, Vec<(usize, usize, String)>)> = change.edits.into_iter().filter(|(_, e)| !e.is_empty()).collect();
    files.sort_by(|a, b| file_name(&a.0).cmp(&file_name(&b.0)).then_with(|| a.0.cmp(&b.0)));

    let mut edit = WorkspaceEdit { changes: Some(HashMap::new()), ..Default::default() };
    if client.resource_operations {
        let mut ops: Vec<DocumentChangeOperation> = Vec::new();
        for (uri, edits) in files.into_iter().chain(change.resource_edits) {
            let Ok(url) = Url::parse(&uri) else { continue };
            let edits = to_text_edits(&uri, edits);
            ops.push(DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri: url, version: None },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            }));
        }
        ops.extend(change.operations.into_iter().map(DocumentChangeOperation::Op));
        if !ops.is_empty() {
            edit.document_changes = Some(DocumentChanges::Operations(ops));
        }
    } else {
        let changes = edit.changes.as_mut().unwrap();
        for (uri, edits) in files {
            let Ok(url) = Url::parse(&uri) else { continue };
            let edits = to_text_edits(&uri, edits);
            changes.entry(url).or_default().extend(edits);
        }
    }
    edit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_round_trips_utf16() {
        let text = "a\r\nb€c\nd";
        let idx = LineIndex::new(text);
        assert_eq!(idx.offset(Position { line: 1, character: 2 }), 5);
        assert_eq!(idx.position(5), Position { line: 1, character: 2 });
        assert_eq!(idx.position(7), Position { line: 2, character: 0 });
    }

    #[test]
    fn accessor_names() {
        assert_eq!(getter_names("myValue", false), vec!["getMyValue"]);
        assert_eq!(getter_names("isDone", true), vec!["isDone", "getDone"]);
        assert_eq!(setter_name("isDone", true), "setDone");
        assert!(is_identifier("newname") && !is_identifier("1a") && !is_identifier("class"));
    }
}
