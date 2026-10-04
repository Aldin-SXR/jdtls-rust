//! Port of jdt.ls `CallHierarchyHandler` on top of JDT's
//! `CallHierarchyCore` semantics (`CallerMethodWrapper`,
//! `CalleeMethodWrapper`, `CalleeAnalyzerVisitor`), with the search and
//! binding resolution done by the semantic index.

use super::java_element::{self, handle_identifier, java_hash_map_order};
use super::semantic::{Elem, Rng, Semantic};
use crate::analysis::dispatcher::Dispatcher;
use crate::project::Workspace;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range, SymbolKind, SymbolTag, Url,
};

/// `CallHierarchyHandler.prepareCallHierarchy`.
pub async fn prepare(dispatcher: &Dispatcher, uri: &Url, pos: Position) -> Option<Vec<CallHierarchyItem>> {
    let sem = Semantic::new(dispatcher).await;
    let (candidate, project) = call_hierarchy_element(&sem, uri, pos, true).await?;
    let ws = workspace(dispatcher);
    let item = to_item(&candidate, &ws, &project)?;
    Some(vec![item])
}

/// `CallHierarchyHandler.callHierarchyIncomingCalls`.
pub async fn incoming(dispatcher: &Dispatcher, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyIncomingCall>> {
    let position = match item.kind {
        SymbolKind::CLASS | SymbolKind::ENUM | SymbolKind::INTERFACE => item.selection_range.start,
        _ => item.range.start,
    };
    let sem = Semantic::new(dispatcher).await;
    let (candidate, project) = call_hierarchy_element(&sem, &item.uri, position, false).await?;
    let ws = workspace(dispatcher);
    let root = call_root(&sem, &candidate).await?;
    // CallerMethodWrapper.canHaveChildren
    if !(root.0.is_method_like() || root.0.is_type() || root.0.kind == "field" || root.0.kind == "enumConstant") {
        return None;
    }
    let callers = find_callers(&sem, &root.0, root.1).await;
    let mut result = Vec::new();
    for (member, locations) in callers {
        let ranges: Vec<Range> = locations.iter().map(|r| r.to_lsp()).collect();
        for loc in &locations {
            if let Some(mut symbol) = to_item(&member, &ws, &project) {
                symbol.selection_range = loc.to_lsp();
                result.push(CallHierarchyIncomingCall { from: symbol, from_ranges: ranges.clone() });
            }
        }
    }
    Some(result)
}

/// `CallHierarchyHandler.callHierarchyOutgoingCalls`.
pub async fn outgoing(dispatcher: &Dispatcher, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyOutgoingCall>> {
    let sem = Semantic::new(dispatcher).await;
    let (candidate, project) = call_hierarchy_element(&sem, &item.uri, item.range.start, false).await?;
    let ws = workspace(dispatcher);
    let root = call_root(&sem, &candidate).await?;
    let calls = sem.callees(&root.0).await;
    // Group by callee handle, then sort by the first call location.
    let keys: Vec<String> = calls.iter().map(|(e, _)| handle_identifier(e, &ws, &project)).collect();
    let mut groups: Vec<(Elem, Vec<Rng>)> = Vec::new();
    let mut group_keys: Vec<String> = Vec::new();
    for ((callee, range), key) in calls.into_iter().zip(keys) {
        let Some(range) = range else {
            // implementations of an abstract root method carry no call location
            if !group_keys.contains(&key) {
                group_keys.push(key);
                groups.push((callee, Vec::new()));
            }
            continue;
        };
        match group_keys.iter().position(|k| *k == key) {
            Some(i) => groups[i].1.push(range),
            None => {
                group_keys.push(key);
                groups.push((callee, vec![range]));
            }
        }
    }
    let order = java_hash_map_order(&group_keys);
    let mut ordered: Vec<(Elem, Vec<Rng>)> = order.into_iter().map(|i| groups[i].clone()).collect();
    ordered.sort_by(|a, b| match (a.1.first(), b.1.first()) {
        (Some(x), Some(y)) => (x.sl, x.sc).cmp(&(y.sl, y.sc)).then((x.el, x.ec).cmp(&(y.el, y.ec))),
        _ => std::cmp::Ordering::Equal,
    });
    let mut result = Vec::new();
    for (callee, locations) in ordered {
        if locations.is_empty() {
            continue;
        }
        let ranges: Vec<Range> = locations.iter().map(|r| r.to_lsp()).collect();
        for _ in &locations {
            if let Some(symbol) = to_item(&callee, &ws, &project) {
                result.push(CallHierarchyOutgoingCall { to: symbol, from_ranges: ranges.clone() });
            }
        }
    }
    Some(result)
}

fn workspace(dispatcher: &Dispatcher) -> Workspace {
    dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `CallHierarchyCore.isPossibleInputElement`.
fn is_possible_input(e: &Elem) -> bool {
    match e.kind.as_str() {
        "type" => matches!(e.type_kind.as_deref(), Some("class") | Some("enum") | Some("record")),
        "method" | "constructor" | "field" | "enumConstant" | "initializer" => true,
        _ => false,
    }
}

/// `getEnclosingMember`: the method, initializer or field at the offset.
fn enclosing_member(e: Option<&Elem>) -> Option<Elem> {
    let e = e?;
    matches!(e.kind.as_str(), "method" | "constructor" | "initializer" | "field" | "enumConstant").then(|| e.clone())
}

/// `CallHierarchyHandler.getCallHierarchyElement`.
async fn call_hierarchy_element(sem: &Semantic<'_>, uri: &Url, pos: Position, prepare: bool) -> Option<(Elem, String)> {
    let (sel, _) = sem.select(uri, pos).await?;
    let ws = sem.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let project = java_element::project_of(&ws, Some(uri.as_str()));
    let enclosing = enclosing_member(sel.enclosing.as_ref());
    let first = sel.select.iter().find(|e| is_possible_input(e)).cloned();
    let candidate = match first {
        Some(element) if !prepare && element.is_type() => match &enclosing {
            Some(m) if m.key != element.key => Some(m.clone()),
            _ => Some(element),
        },
        Some(element) => Some(element),
        None => enclosing,
    };
    candidate.map(|c| (c, project))
}

/// `CallHierarchyCore.getRoots(...)[0]`: a type stands for its first
/// constructor (or for the default constructor when it declares none).
/// The flag tells whether a type root searches constructor references.
async fn call_root(sem: &Semantic<'_>, member: &Elem) -> Option<(Elem, bool)> {
    if member.is_type() && !member.anonymous {
        if member.constructor_keys.is_empty() || member.type_kind.as_deref() == Some("record") {
            return Some((member.clone(), true));
        }
        let key = &member.constructor_keys[0];
        let project = member.uri.as_deref().map(|_| ()).and(None::<&str>);
        let ctor = sem.element(key, project).await?;
        return Some((ctor, false));
    }
    Some((member.clone(), member.is_type()))
}

/// `CallerMethodWrapper.findChildren`: callers grouped by caller handle, in
/// `HashMap` order, each with its call locations.
async fn find_callers(sem: &Semantic<'_>, member: &Elem, type_root: bool) -> Vec<(Elem, Vec<Rng>)> {
    if member.kind == "initializer" {
        return Vec::new();
    }
    let ws = sem.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let project = java_element::project_of(&ws, member.uri.as_deref());
    let mode = if type_root && member.is_type() { "constructorsOf" } else { "references" };
    let (matches, elements) = sem.references(member, mode).await;
    let mut keys: Vec<String> = Vec::new();
    let mut groups: Vec<(Elem, Vec<Rng>)> = Vec::new();
    for m in matches {
        // MethodReferencesSearchRequestor: exact matches outside Javadoc, in members
        if !m.accurate || m.javadoc || m.enclosing_kind.as_deref() != Some("member") {
            continue;
        }
        let Some(key) = m.enclosing.as_ref() else { continue };
        let Some(caller) = elements.get(key) else { continue };
        let mut caller = caller.clone();
        sem.complete(&mut caller).await;
        let handle = handle_identifier(&caller, &ws, &project);
        match keys.iter().position(|k| *k == handle) {
            Some(i) => groups[i].1.push(m.range),
            None => {
                keys.push(handle);
                groups.push((caller, vec![m.range]));
            }
        }
    }
    java_hash_map_order(&keys).into_iter().map(|i| groups[i].clone()).collect()
}

/// `CallHierarchyHandler.toCallHierarchyItem`.
fn to_item(member: &Elem, ws: &Workspace, project: &str) -> Option<CallHierarchyItem> {
    let full = java_element::full_location(member, project)?;
    let selection = java_element::name_location(member, project).map(|l| l.range).unwrap_or(full.range);
    let detail = if member.is_type() {
        member.declaring_type_fqn.clone()
    } else {
        java_element::declaring_type_fqn(member)
    };
    let _ = ws;
    Some(CallHierarchyItem {
        name: java_element::label(member),
        kind: java_element::symbol_kind(member),
        tags: member.deprecated.then(|| vec![SymbolTag::DEPRECATED]),
        detail,
        uri: full.uri,
        range: full.range,
        selection_range: selection,
        data: None,
    })
}
