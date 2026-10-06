//! Port of jdt.ls `WorkspaceSymbolHandler` (`workspace/symbol` and the
//! `java/searchSymbols` extension) over the Rust type index.

use super::code_lens::position;
use super::preferences;
use super::semantic::Semantic;
use crate::analysis::dispatcher::Dispatcher;
use crate::index::type_index::{
    self, name_matches, pattern_match, validate_rule, MatchRule, MethodEntry, TypeEntry, TypeOrigin, ACC_ANNOTATION,
    ACC_DEPRECATED, ACC_ENUM, ACC_INTERFACE,
};
use crate::project::{Workspace, DEFAULT_PROJECT_NAME};
use serde::Deserialize;
use std::collections::HashSet;
use tower_lsp::lsp_types::{Location, Range, SymbolInformation, SymbolKind, SymbolTag, Url};

/// `WorkspaceSymbolHandler.SearchSymbolParams` (`java/searchSymbols`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchSymbolParams {
    pub query: Option<String>,
    pub project_name: Option<String>,
    pub source_only: bool,
    pub max_results: usize,
}

/// `WorkspaceSymbolHandler.search(query, maxResults, projectName, sourceOnly)`.
pub async fn search(dispatcher: &Dispatcher, query: Option<&str>, max_results: usize, project_name: Option<&str>, source_only: bool) -> Vec<SymbolInformation> {
    let mut out = Results::new(max_results);
    let Some(query) = query else { return out.items };
    if query.trim().is_empty() {
        return out.items;
    }
    let ws = dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let scope = scope(dispatcher, &ws, project_name, source_only).await;

    let t_query = query.trim();
    let mut qualifier: Option<String> = None;
    let mut type_name = t_query.to_owned();
    let mut fuzzy: Option<String> = None;
    if let Some(i) = t_query.rfind('.') {
        let q = &t_query[..i];
        type_name = t_query[i + 1..].to_owned();
        qualifier = Some(if !q.contains('*') && !q.contains('?') { format!("*{q}*") } else { q.to_owned() });
    } else {
        let mut term = String::new();
        let mut prev: Option<char> = None;
        for c in t_query.chars() {
            if let Some(p) = prev {
                if p.is_lowercase() && c.is_uppercase() {
                    term.push('*');
                }
            }
            term.push(c);
            prev = Some(c);
        }
        if term != t_query {
            fuzzy = Some(format!("*{term}*"));
        }
    }
    let type_rule = if type_name.contains('*') || type_name.contains('?') { MatchRule::Pattern } else { MatchRule::CamelCase };

    if !type_name.is_empty() {
        let pattern = fuzzy.clone().unwrap_or_else(|| type_name.clone());
        let rule = validate_rule(&pattern, type_rule);
        for t in &scope.types {
            if out.full() {
                break;
            }
            if qualifier.as_deref().is_some_and(|q| !pattern_match(q, &t.container())) {
                continue;
            }
            if name_matches(&pattern, rule, &t.name) {
                out.add_type(t, &scope, source_only);
            }
        }
    }
    // qualifier = qualifierName.typeName, type = null
    for t in &scope.types {
        if out.full() {
            break;
        }
        if pattern_match(t_query, &t.container()) {
            out.add_type(t, &scope, source_only);
        }
    }

    if preferences::include_source_method_declarations() {
        let source_scope = if source_only { None } else { Some(self::scope(dispatcher, &ws, project_name, true).await) };
        let methods = source_scope.as_ref().map_or(&scope.methods, |s| &s.methods);
        let rule = validate_rule(t_query, type_rule);
        for m in methods {
            if out.full() {
                break;
            }
            if name_matches(t_query, rule, &m.name) {
                out.add_method(m, &scope);
            }
        }
    }
    out.items
}

struct Results {
    items: Vec<SymbolInformation>,
    seen: HashSet<String>,
    max: usize,
}

impl Results {
    fn new(max: usize) -> Self {
        Results { items: Vec::new(), seen: HashSet::new(), max }
    }

    fn full(&self) -> bool {
        self.max > 0 && self.items.len() >= self.max
    }

    fn push(&mut self, s: SymbolInformation) {
        if self.full() {
            return;
        }
        let key = serde_json::to_string(&s).unwrap_or_default();
        if self.seen.insert(key) {
            self.items.push(s);
        }
    }

    fn add_type(&mut self, t: &TypeEntry, scope: &SearchScope, source_only: bool) {
        if t.name.is_empty() {
            return;
        }
        let location = match &t.origin {
            TypeOrigin::Source { uri, name } => {
                let Some(text) = scope.text(uri) else { return };
                let Ok(uri) = Url::parse(uri) else { return };
                Location { uri, range: Range { start: position(text, name.0), end: position(text, name.1) } }
            }
            TypeOrigin::Binary { archive, module, class_file, source_file_name } => {
                if source_only {
                    return;
                }
                let desc = crate::classfile::ClassFileDesc {
                    package_name: t.package.clone(),
                    root: archive.clone(),
                    module: module.clone(),
                    class_file_name: class_file.clone(),
                    source_file_name: source_file_name.clone(),
                };
                let Ok(uri) = Url::parse(&super::navigation::class_file_uri(&scope.workspace, &scope.project, &desc)) else {
                    return;
                };
                Location { uri, range: Range::default() }
            }
        };
        let deprecated = t.modifiers & ACC_DEPRECATED != 0;
        #[allow(deprecated)]
        self.push(SymbolInformation {
            name: t.name.clone(),
            kind: map_kind(t.modifiers),
            tags: (deprecated && preferences::symbol_tags_supported()).then(|| vec![SymbolTag::DEPRECATED]),
            deprecated: (deprecated && !preferences::symbol_tags_supported()).then_some(true),
            location,
            container_name: Some(t.container()),
        });
    }

    fn add_method(&mut self, m: &MethodEntry, scope: &SearchScope) {
        let Some(text) = scope.text(&m.uri) else { return };
        let Ok(uri) = Url::parse(&m.uri) else { return };
        let deprecated = m.modifiers & ACC_DEPRECATED != 0;
        #[allow(deprecated)]
        self.push(SymbolInformation {
            name: m.name.clone(),
            kind: SymbolKind::METHOD,
            tags: (deprecated && preferences::symbol_tags_supported()).then(|| vec![SymbolTag::DEPRECATED]),
            deprecated: (deprecated && !preferences::symbol_tags_supported()).then_some(true),
            location: Location { uri, range: Range { start: position(text, m.name_span.0), end: position(text, m.name_span.1) } },
            container_name: Some(m.declaring_type.clone()),
        });
    }
}

/// `WorkspaceSymbolTypeRequestor.mapKind`.
fn map_kind(flags: u32) -> SymbolKind {
    if flags & ACC_INTERFACE != 0 {
        SymbolKind::INTERFACE
    } else if flags & ACC_ANNOTATION != 0 {
        SymbolKind::PROPERTY
    } else if flags & ACC_ENUM != 0 {
        SymbolKind::ENUM
    } else {
        SymbolKind::CLASS
    }
}

/// The types (and source methods) of a search scope.
struct SearchScope {
    workspace: Workspace,
    project: String,
    types: Vec<TypeEntry>,
    methods: Vec<MethodEntry>,
    texts: std::collections::HashMap<String, String>,
}

impl SearchScope {
    fn text(&self, uri: &str) -> Option<&str> {
        self.texts.get(uri).map(String::as_str)
    }
}

/// `WorkspaceSymbolHandler.createSearchScope`: the sources of the project
/// (or all projects) and their referenced projects, plus their libraries and
/// the JDK when the client can open class files.
async fn scope(dispatcher: &Dispatcher, ws: &Workspace, project_name: Option<&str>, source_only: bool) -> SearchScope {
    let target = project_name.and_then(|n| ws.project(n));
    let projects: Vec<&crate::project::Project> = match target {
        Some(p) => ws.project_closure(p),
        None => ws.projects.iter().collect(),
    };
    let names: HashSet<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    let exclude_tests = preferences::search_scope_main();
    let mut texts = std::collections::HashMap::new();
    let mut types = Vec::new();
    let mut methods = Vec::new();
    let mut files: Vec<(String, String)> = dispatcher.store.all_contents().into_iter().collect();
    files.sort();
    for (uri, text) in files {
        let owner = Url::parse(&uri).ok().and_then(|u| ws.project_for_uri(&u)).map(|p| p.name.clone());
        let in_scope = match &owner {
            Some(n) => names.contains(n.as_str()),
            None => target.is_none(),
        };
        if !in_scope || (exclude_tests && super::code_lens::is_test_source(ws, &uri)) {
            continue;
        }
        let (t, m) = type_index::source_entries(&uri, &text);
        types.extend(t);
        methods.extend(m);
        texts.insert(uri, text);
    }
    let project = target
        .map(|p| p.name.clone())
        .or_else(|| ws.projects.first().map(|p| p.name.clone()))
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned());
    if !source_only && preferences::class_file_contents_supported() {
        let mut archives: Vec<String> = Vec::new();
        for p in &projects {
            for lib in &p.libraries {
                if exclude_tests && lib.is_test {
                    continue;
                }
                if target.is_some_and(|owner| owner.name != p.name)
                    && p.runtime.as_ref().is_some_and(|vm| vm.libraries.iter().any(|runtime| runtime.path == lib.path)) {
                    continue;
                }
                let s = lib.path.to_string_lossy().into_owned();
                if !archives.contains(&s) {
                    archives.push(s);
                }
            }
        }
        if target.is_none() {
            for c in dispatcher_classpath(dispatcher).await {
                if !archives.contains(&c) {
                    archives.push(c);
                }
            }
        }
        let missing: Vec<String> = archives.iter().filter(|a| type_index::cached_archive(a).is_none()).cloned().collect();
        // Configured VMs are already present as explicit library images.
        // Include the running VM only for projects that use that fallback.
        let include_jdk = if projects.is_empty() {
            dispatcher.context_for(None).await.options.get(crate::project::INCLUDE_RUNNING_VM)
                .map_or(true, |include| include == "true")
        } else {
            projects.iter().any(|p| p.runtime.is_none()
                && p.classpath.iter().any(|entry| entry.is_jre_container()))
        };
        let need_jdk = include_jdk && type_index::cached_jdk().is_none();
        if !missing.is_empty() || need_jdk {
            let sem = Semantic::new(dispatcher).await;
            if let Some(v) = sem.list_types(&missing, need_jdk).await {
                type_index::store_listing(&v);
            }
        }
        for a in &archives {
            types.extend(type_index::cached_archive(a).unwrap_or_default());
        }
        if include_jdk { types.extend(type_index::cached_jdk().unwrap_or_default()); }
    }
    SearchScope { workspace: ws.clone(), project, types, methods, texts }
}

async fn dispatcher_classpath(dispatcher: &Dispatcher) -> Vec<String> {
    dispatcher.context_for(None).await.classpath
}
