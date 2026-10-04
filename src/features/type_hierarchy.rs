//! Ports of jdt.ls `TypeHierarchyHandler` (LSP 3.17 `textDocument/
//! prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/
//! subtypes`) and of the legacy `TypeHierarchyCommand`
//! (`java.navigate.openTypeHierarchy`, `java.navigate.resolveTypeHierarchy`).

use super::java_element::{self, handle_identifier};
use super::semantic::{Elem, HierarchyEntry, Semantic};
use crate::analysis::dispatcher::Dispatcher;
use crate::project::Workspace;
use serde_json::{json, Value};
use tower_lsp::lsp_types::{Position, SymbolKind, TypeHierarchyItem, Url};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Children,
    Parents,
    Both,
}

impl Direction {
    pub fn from_value(v: i64) -> Option<Direction> {
        match v {
            0 => Some(Direction::Children),
            1 => Some(Direction::Parents),
            2 => Some(Direction::Both),
            _ => None,
        }
    }
}

fn workspace(d: &Dispatcher) -> Workspace {
    d.workspace.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `findTypeElement` + `getMember`: the type or method at the position, or
/// the unit's primary type.
async fn member_at(sem: &Semantic<'_>, uri: &Url, pos: Position) -> Option<Elem> {
    let (sel, _) = sem.select(uri, pos).await?;
    let element = match sel.select.into_iter().next() {
        Some(e) => Some(e),
        None => primary_type(uri, sel.primary_type),
    }?;
    (element.is_type() || element.kind == "method" || element.kind == "constructor").then_some(element)
}

/// `ICompilationUnit.findPrimaryType`: the top-level type named after the file.
fn primary_type(uri: &Url, first: Option<Elem>) -> Option<Elem> {
    let file = uri.path_segments()?.last()?.trim_end_matches(".java").to_owned();
    first.filter(|t| t.name == file)
}

/// One `TypeHierarchyItem` (`toTypeHierarchyItem(member, excludeMember, targetMethod)`).
struct Item {
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    deprecated: bool,
    uri: Url,
    range: tower_lsp::lsp_types::Range,
    selection_range: tower_lsp::lsp_types::Range,
    data: Value,
}

fn to_item(member: &Elem, ty: &Elem, exclude_member: bool, target: Option<&Elem>, ws: &Workspace, project: &str) -> Option<Item> {
    let location = java_element::full_location(member, project)?;
    let select = java_element::name_location(member, project)?;
    let fqn = ty.fqn.clone().unwrap_or_else(|| ty.name.clone());
    let (name, detail) = match fqn.rfind('.') {
        Some(i) if i >= 1 && i < fqn.len() - 1 && !ty.anonymous => (fqn[i + 1..].to_owned(), Some(fqn[..i].to_owned())),
        _ => (java_element::label(ty), ty.package_name.clone()),
    };
    let kind = if exclude_member { SymbolKind::NULL } else { java_element::type_symbol_kind(ty) };
    let mut data = serde_json::Map::new();
    data.insert("element".into(), json!(handle_identifier(member, ws, project)));
    if let Some(t) = target {
        data.insert("method".into(), json!(handle_identifier(t, ws, project)));
        data.insert("method_name".into(), json!(t.name));
        data.insert("methodKey".into(), json!(t.key));
    } else if member.is_method_like() {
        data.insert("method".into(), json!(handle_identifier(member, ws, project)));
        data.insert("method_name".into(), json!(member.name));
        data.insert("methodKey".into(), json!(member.key));
    }
    data.insert("key".into(), json!(member.key));
    data.insert("project".into(), json!(project));
    Some(Item {
        name,
        detail,
        kind,
        deprecated: member.deprecated,
        uri: location.uri,
        range: location.range,
        selection_range: select.range,
        data: Value::Object(data),
    })
}

impl Item {
    fn lsp(self) -> TypeHierarchyItem {
        TypeHierarchyItem {
            name: self.name,
            kind: self.kind,
            // lsp-types 0.94 types `tags` as a single tag; jdt.ls sends a list.
            tags: None,
            detail: self.detail,
            uri: self.uri,
            range: self.range,
            selection_range: self.selection_range,
            data: Some(self.data),
        }
    }

    fn legacy(self) -> Value {
        json!({
            "name": self.name,
            "detail": self.detail,
            "kind": self.kind,
            "deprecated": self.deprecated,
            "uri": self.uri,
            "range": self.range,
            "selectionRange": self.selection_range,
            "data": self.data,
        })
    }
}

async fn declaring_type(sem: &Semantic<'_>, member: &Elem, project: &str) -> Option<Elem> {
    if member.is_type() {
        return Some(member.clone());
    }
    sem.element(member.declaring_type_key.as_deref()?, Some(project).filter(|p| !p.is_empty())).await
}

/// The member and target method an item's `data` names.
async fn item_data(sem: &Semantic<'_>, data: Option<&Value>) -> Option<(Elem, Option<Elem>, String)> {
    let data = data?;
    let project = data.get("project").and_then(Value::as_str).unwrap_or("").to_owned();
    let key = data.get("key").and_then(Value::as_str)?;
    let member = sem.element(key, Some(project.as_str()).filter(|p| !p.is_empty())).await?;
    let target = match data.get("methodKey").and_then(Value::as_str) {
        Some(k) => sem.element(k, Some(project.as_str()).filter(|p| !p.is_empty())).await,
        None => None,
    };
    Some((member, target, project))
}

/// `TypeHierarchyHandler.prepareTypeHierarchy`.
pub async fn prepare(d: &Dispatcher, uri: &Url, pos: Position) -> Vec<TypeHierarchyItem> {
    let sem = Semantic::new(d).await;
    let ws = workspace(d);
    let Some(member) = member_at(&sem, uri, pos).await else { return Vec::new() };
    let project = java_element::project_of(&ws, Some(uri.as_str()));
    let target = member.is_method_like().then(|| member.clone());
    let Some(ty) = declaring_type(&sem, &member, &project).await else { return Vec::new() };
    to_item(&member, &ty, false, target.as_ref(), &ws, &project).map(|i| vec![i.lsp()]).unwrap_or_default()
}

/// `TypeHierarchyHandler.getSupertypeItems` / `getSubtypeItems`.
pub async fn resolve_items(d: &Dispatcher, item: &TypeHierarchyItem, direction: Direction) -> Vec<TypeHierarchyItem> {
    let sem = Semantic::new(d).await;
    let ws = workspace(d);
    let Some((member, target, project)) = item_data(&sem, item.data.as_ref()).await else { return Vec::new() };
    let Some(ty) = declaring_type(&sem, &member, &project).await else { return Vec::new() };
    let entries = if direction == Direction::Parents {
        sem.supertypes(&ty, target.as_ref(), Some(&project)).await
    } else {
        sem.subtypes(&ty, target.as_ref()).await
    };
    entries
        .into_iter()
        .filter_map(|e| entry_item(e, target.as_ref(), direction == Direction::Parents, &ws, &project))
        .map(Item::lsp)
        .collect()
}

fn entry_item(e: HierarchyEntry, target: Option<&Elem>, parents: bool, ws: &Workspace, project: &str) -> Option<Item> {
    match target {
        Some(t) => {
            let exclude = e.method.is_none();
            // Do not show java.lang.Object unless the target method is declared there
            if parents && exclude && e.ty.fqn.as_deref() == Some("java.lang.Object") {
                return None;
            }
            let member = e.method.clone().unwrap_or_else(|| e.ty.clone());
            to_item(&member, &e.ty, exclude, Some(t), ws, project)
        }
        None => to_item(&e.ty, &e.ty, false, None, ws, project),
    }
}

// ─── legacy commands ─────────────────────────────────────────────────────────

/// `java.navigate.openTypeHierarchy [TextDocumentPositionParams, direction, resolve]`.
/// jdt.ls reads command arguments with `JSONUtility.toModel`, which accepts
/// JSON values or JSON-encoded strings (vscode-java sends `JSON.stringify`
/// for each argument).
fn decode_args(args: &[Value]) -> Vec<Value> {
    args.iter()
        .map(|a| match a {
            Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| a.clone()),
            other => other.clone(),
        })
        .collect()
}

pub async fn open_type_hierarchy(d: &Dispatcher, args: &[Value]) -> Option<Value> {
    let args = &decode_args(args)[..];
    let params = args.first()?;
    let uri = Url::parse(params.pointer("/textDocument/uri")?.as_str()?).ok()?;
    let pos: Position = serde_json::from_value(params.get("position")?.clone()).ok()?;
    let direction = Direction::from_value(args.get(1)?.as_i64()?)?;
    let resolve = args.get(2)?.as_i64()?;
    let sem = Semantic::new(d).await;
    let ws = workspace(d);
    let member = member_at(&sem, &uri, pos).await?;
    let project = java_element::project_of(&ws, Some(uri.as_str()));
    let target = member.is_method_like().then(|| member.clone());
    legacy_hierarchy(&sem, &ws, &member, target.as_ref(), direction, resolve, &project).await
}

/// `java.navigate.resolveTypeHierarchy [TypeHierarchyItem, direction, resolve]`.
pub async fn resolve_type_hierarchy(d: &Dispatcher, args: &[Value]) -> Option<Value> {
    let args = &decode_args(args)[..];
    let item = args.first()?;
    item.get("range")?;
    item.get("uri")?.as_str()?;
    let direction = Direction::from_value(args.get(1)?.as_i64()?)?;
    let resolve = args.get(2)?.as_i64()?;
    let sem = Semantic::new(d).await;
    let ws = workspace(d);
    let (member, target, project) = item_data(&sem, item.get("data")).await?;
    legacy_hierarchy(&sem, &ws, &member, target.as_ref(), direction, resolve, &project).await
}

async fn legacy_hierarchy(
    sem: &Semantic<'_>,
    ws: &Workspace,
    member: &Elem,
    target: Option<&Elem>,
    direction: Direction,
    resolve: i64,
    project: &str,
) -> Option<Value> {
    let ty = declaring_type(sem, member, project).await?;
    // toTypeHierarchyItem(member): no target method on the root item
    let mut item = to_item(member, &ty, false, None, ws, project)?.legacy();
    legacy_resolve(sem, ws, &mut item, ty, target, direction, resolve, project).await;
    Some(item)
}

type BoxFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// `TypeHierarchyCommand.resolve`.
#[allow(clippy::too_many_arguments)]
fn legacy_resolve<'a>(
    sem: &'a Semantic<'a>,
    ws: &'a Workspace,
    item: &'a mut Value,
    ty: Elem,
    target: Option<&'a Elem>,
    direction: Direction,
    resolve: i64,
    project: &'a str,
) -> BoxFuture<'a> {
    Box::pin(async move {
        if resolve <= 0 {
            return;
        }
        if matches!(direction, Direction::Children | Direction::Both) {
            let mut children = Vec::new();
            for e in sem.subtypes(&ty, target).await {
                let child_ty = e.ty.clone();
                let Some(child) = entry_item(e, target, false, ws, project) else { continue };
                let mut child = child.legacy();
                legacy_resolve(sem, ws, &mut child, child_ty, target, direction, resolve - 1, project).await;
                children.push(child);
            }
            item["children"] = Value::Array(children);
        }
        if matches!(direction, Direction::Parents | Direction::Both) {
            let mut parents = Vec::new();
            for e in sem.supertypes(&ty, target, Some(project)).await {
                let parent_ty = e.ty.clone();
                let Some(parent) = entry_item(e, target, true, ws, project) else { continue };
                let mut parent = parent.legacy();
                legacy_resolve(sem, ws, &mut parent, parent_ty, target, direction, resolve - 1, project).await;
                parents.push(parent);
            }
            item["parents"] = Value::Array(parents);
        }
    })
}
