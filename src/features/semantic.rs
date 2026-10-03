//! Client for the bridge's binding-resolved semantic index
//! (`SemanticIndexService.java`): element selection (`codeSelect`), the JDT
//! search engine's reference matching, callees, implementations and type
//! hierarchies.  The bridge only returns data (element descriptors and
//! matches); the features in this module tree shape it into LSP results.
//!
//! Every query runs per project: each imported project (and the default
//! project of virtual documents) is indexed with its own classpath and
//! compliance, and only its own files are indexed (the other files of its
//! closure are visible for binding resolution).  Workspace-wide queries
//! merge the per-project answers.

use crate::analysis::dispatcher::{Dispatcher, RequestContext};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use tower_lsp::lsp_types::{Position, Range, Url};

/// A range in LSP coordinates as sent by the bridge.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub struct Rng {
    pub sl: u32,
    pub sc: u32,
    pub el: u32,
    pub ec: u32,
}

impl Rng {
    pub fn to_lsp(self) -> Range {
        Range {
            start: Position { line: self.sl, character: self.sc },
            end: Position { line: self.el, character: self.ec },
        }
    }
}

/// Element descriptor: a JDT `IMember` (type, method, constructor, field,
/// enum constant or initializer), from source or from a class file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Elem {
    pub key: String,
    pub kind: String,
    pub name: String,
    pub type_kind: Option<String>,
    pub flags: i64,
    pub deprecated: bool,
    pub from_source: bool,
    pub anonymous: bool,
    pub local: bool,
    pub implicit_type: bool,
    pub default_constructor: bool,
    pub fqn: Option<String>,
    pub declaring_type_key: Option<String>,
    pub declaring_type_fqn: Option<String>,
    pub package_name: Option<String>,
    pub type_chain: Vec<String>,
    pub params: Vec<String>,
    pub param_sigs: Vec<String>,
    pub varargs: bool,
    pub return_type: Option<String>,
    pub type_params: Vec<String>,
    pub occurrence: u32,
    pub uri: Option<String>,
    pub range: Option<Rng>,
    pub name_range: Option<Rng>,
    pub archive: Option<String>,
    pub module: Option<String>,
    pub class_file: Option<String>,
    pub superclass_key: Option<String>,
    pub interface_keys: Vec<String>,
    pub overrides: Vec<String>,
    pub constructor_keys: Vec<String>,
}

pub const ACC_PRIVATE: i64 = 0x0002;
pub const ACC_STATIC: i64 = 0x0008;
pub const ACC_FINAL: i64 = 0x0010;
pub const ACC_ABSTRACT: i64 = 0x0400;

impl Elem {
    pub fn is_type(&self) -> bool {
        self.kind == "type"
    }
    pub fn is_method_like(&self) -> bool {
        self.kind == "method" || self.kind == "constructor"
    }
    pub fn is_interface(&self) -> bool {
        matches!(self.type_kind.as_deref(), Some("interface") | Some("annotation"))
    }
    pub fn has_flag(&self, flag: i64) -> bool {
        self.flags & flag != 0
    }
    /// A source element whose location is known (its file was indexed).
    pub fn has_source_location(&self) -> bool {
        self.uri.is_some() && self.range.is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectResult {
    pub select: Vec<Elem>,
    pub selected_kind: Option<String>,
    pub enclosing: Option<Elem>,
    pub primary_type: Option<Elem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Match {
    pub uri: String,
    pub range: Rng,
    pub accurate: bool,
    pub javadoc: bool,
    pub enclosing: Option<String>,
    pub enclosing_kind: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ReferencesResult {
    matches: Vec<Match>,
    elements: HashMap<String, Elem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Call {
    callee: String,
    range: Option<Rng>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CalleesResult {
    calls: Vec<Call>,
    elements: HashMap<String, Elem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ElementsResult {
    elements: Vec<Elem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ElementResult {
    element: Option<Elem>,
}

/// One type of a hierarchy, with the matching method for method hierarchies.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HierarchyEntry {
    #[serde(rename = "type")]
    pub ty: Elem,
    pub method: Option<Elem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HierarchyResult {
    items: Vec<HierarchyEntry>,
}

/// A project's search context.
pub struct ProjectContext {
    pub project: Option<String>,
    pub ctx: RequestContext,
    pub owned: Vec<String>,
}

pub struct Semantic<'a> {
    pub dispatcher: &'a Dispatcher,
    contexts: Vec<ProjectContext>,
}

impl<'a> Semantic<'a> {
    pub async fn new(dispatcher: &'a Dispatcher) -> Semantic<'a> {
        let contexts = dispatcher
            .project_contexts()
            .await
            .into_iter()
            .filter(|(_, _, owned)| !owned.is_empty())
            .map(|(project, ctx, owned)| ProjectContext { project, ctx, owned })
            .collect();
        Semantic { dispatcher, contexts }
    }

    pub fn contexts(&self) -> &[ProjectContext] {
        &self.contexts
    }

    /// The context whose project owns `uri`.
    pub fn context_of(&self, uri: &str) -> Option<&ProjectContext> {
        self.contexts.iter().find(|c| c.owned.iter().any(|u| u == uri))
    }

    fn context_named(&self, project: Option<&str>) -> Option<&ProjectContext> {
        self.contexts.iter().find(|c| c.project.as_deref() == project)
    }

    async fn query(&self, pc: &ProjectContext, mut query: Value) -> Option<Value> {
        query["owned"] = json!(pc.owned);
        match self.dispatcher.semantic_search(&pc.ctx, query).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("semantic search failed: {e}");
                None
            }
        }
    }

    /// `ICodeAssist.codeSelect` plus `ITypeRoot.getElementAt` at `pos`.
    pub async fn select(&self, uri: &Url, pos: Position) -> Option<(SelectResult, Option<String>)> {
        let pc = self.context_of(uri.as_str())?;
        let v = self
            .query(pc, json!({ "op": "select", "uri": uri.as_str(), "line": pos.line, "character": pos.character }))
            .await?;
        let mut r: SelectResult = serde_json::from_value(v).ok()?;
        for e in r.select.iter_mut() {
            self.complete(e).await;
        }
        Some((r, pc.project.clone()))
    }

    /// Fill in the location of a source element declared in a file another
    /// project owns.
    pub async fn complete(&self, e: &mut Elem) {
        if !e.from_source || e.has_source_location() {
            return;
        }
        for pc in &self.contexts {
            if let Some(v) = self.query(pc, json!({ "op": "element", "key": e.key })).await {
                if let Ok(ElementResult { element: Some(found) }) = serde_json::from_value::<ElementResult>(v) {
                    if found.has_source_location() {
                        *e = found;
                        return;
                    }
                }
            }
        }
    }

    pub async fn element(&self, key: &str, project: Option<&str>) -> Option<Elem> {
        let mut order: Vec<&ProjectContext> = Vec::new();
        if let Some(pc) = self.context_named(project) {
            order.push(pc);
        }
        order.extend(self.contexts.iter().filter(|c| c.project.as_deref() != project));
        let mut fallback = None;
        for pc in order {
            if let Some(v) = self.query(pc, json!({ "op": "element", "key": key })).await {
                if let Ok(ElementResult { element: Some(found) }) = serde_json::from_value::<ElementResult>(v) {
                    if !found.from_source || found.has_source_location() {
                        return Some(found);
                    }
                    fallback.get_or_insert(found);
                }
            }
        }
        fallback
    }

    /// Search matches for `target` in every project, sorted the way the JDT
    /// search engine reports them (by document path, then offset), with the
    /// descriptors of their enclosing members.  `mode` is `references` or
    /// `constructorsOf` (constructor references of a type).
    pub async fn references(&self, target: &Elem, mode: &str) -> (Vec<Match>, HashMap<String, Elem>) {
        let mut matches = Vec::new();
        let mut elements = HashMap::new();
        for pc in &self.contexts {
            let Some(v) = self.query(pc, json!({ "op": "references", "key": target.key, "mode": mode })).await else {
                continue;
            };
            let Ok(r) = serde_json::from_value::<ReferencesResult>(v) else { continue };
            matches.extend(r.matches);
            elements.extend(r.elements);
        }
        matches.sort_by(|a, b| {
            path_of(&a.uri)
                .cmp(&path_of(&b.uri))
                .then((a.range.sl, a.range.sc).cmp(&(b.range.sl, b.range.sc)))
        });
        (matches, elements)
    }

    /// Calls made from `member` in source order: (callee, call range).
    pub async fn callees(&self, member: &Elem) -> Vec<(Elem, Option<Rng>)> {
        let Some(uri) = member.uri.as_deref() else { return Vec::new() };
        let Some(pc) = self.context_of(uri) else { return Vec::new() };
        let Some(v) = self.query(pc, json!({ "op": "callees", "key": member.key })).await else {
            return Vec::new();
        };
        let Ok(r) = serde_json::from_value::<CalleesResult>(v) else { return Vec::new() };
        let mut out = Vec::new();
        for c in r.calls {
            if let Some(e) = r.elements.get(&c.callee) {
                let mut e = e.clone();
                self.complete(&mut e).await;
                out.push((e, c.range));
            }
        }
        out
    }

    /// `ImplementationCollector`: all subtypes of a type, or the
    /// implementations of a method, across projects.
    pub async fn implementations(&self, target: &Elem) -> Vec<Elem> {
        let mut out: Vec<Elem> = Vec::new();
        for pc in &self.contexts {
            let Some(v) = self.query(pc, json!({ "op": "implementations", "key": target.key })).await else {
                continue;
            };
            let Ok(r) = serde_json::from_value::<ElementsResult>(v) else { continue };
            for e in r.elements {
                if !out.iter().any(|o| o.key == e.key) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// Direct supertypes of `ty` (interfaces first, then the superclass), as
    /// `ITypeHierarchy.getSupertypes` orders them.
    pub async fn supertypes(&self, ty: &Elem, method: Option<&Elem>, project: Option<&str>) -> Vec<HierarchyEntry> {
        let pc = ty
            .uri
            .as_deref()
            .and_then(|u| self.context_of(u))
            .or_else(|| self.context_named(project))
            .or_else(|| self.contexts.first());
        let Some(pc) = pc else { return Vec::new() };
        let mut q = json!({ "op": "supertypes", "key": ty.key });
        if let Some(m) = method {
            q["methodKey"] = json!(m.key);
        }
        let Some(v) = self.query(pc, q).await else { return Vec::new() };
        let mut items = serde_json::from_value::<HierarchyResult>(v).map(|r| r.items).unwrap_or_default();
        for it in items.iter_mut() {
            self.complete(&mut it.ty).await;
            if let Some(m) = it.method.as_mut() {
                self.complete(m).await;
            }
        }
        items
    }

    /// Direct subtypes of `ty` in every project's sources and, once, in the
    /// libraries.
    pub async fn subtypes(&self, ty: &Elem, method: Option<&Elem>) -> Vec<HierarchyEntry> {
        let mut out: Vec<HierarchyEntry> = Vec::new();
        let mut libraries_done: Vec<Vec<String>> = Vec::new();
        for pc in &self.contexts {
            let libraries = !libraries_done.contains(&pc.ctx.classpath);
            libraries_done.push(pc.ctx.classpath.clone());
            let mut q = json!({ "op": "subtypes", "key": ty.key, "libraries": libraries });
            if let Some(m) = method {
                q["methodKey"] = json!(m.key);
            }
            let Some(v) = self.query(pc, q).await else { continue };
            let Ok(r) = serde_json::from_value::<HierarchyResult>(v) else { continue };
            for it in r.items {
                if !out.iter().any(|o| o.ty.key == it.ty.key) {
                    out.push(it);
                }
            }
        }
        out
    }

    /// Binary type listings of `archives` (and of the JDK when `jdk`).
    pub async fn list_types(&self, archives: &[String], jdk: bool) -> Option<Value> {
        let pc = self.contexts.first();
        let empty = RequestContext {
            files: HashMap::new(),
            classpath: Vec::new(),
            source_level: "21".into(),
            options: BTreeMap::new(),
        };
        let ctx = pc.map(|p| &p.ctx).unwrap_or(&empty);
        let q = json!({ "op": "listTypes", "archives": archives, "jdk": jdk });
        match self.dispatcher.semantic_search(&RequestContext {
            files: HashMap::new(),
            classpath: Vec::new(),
            source_level: ctx.source_level.clone(),
            options: BTreeMap::new(),
        }, q).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("listTypes failed: {e}");
                None
            }
        }
    }
}

fn path_of(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_owned())
}
