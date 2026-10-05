//! OverrideMethodsHandler, OverrideMethodsOperation and inherited-method discovery.
pub(crate) mod actions;
pub(crate) mod operation;
use super::{
    accessors,
    delegates::{overridden, subsignature},
    java_model::{self, Member, TypeDecl},
};
use crate::{
    analysis::dispatcher::Dispatcher,
    correction::edit::Env,
    semantic_ast::{bflag, modifier, Ast, BindingRef},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tower_lsp::lsp_types::{CodeActionParams, WorkspaceEdit};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverridableMethod {
    #[serde(default)]
    pub binding_key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parameters: Vec<String>,
    #[serde(default)]
    pub unimplemented: bool,
    #[serde(default)]
    pub declaring_class: String,
    #[serde(default)]
    pub declaring_class_type: String,
}
#[derive(Default, Serialize)]
pub struct OverridableMethodsResponse {
    #[serde(rename = "type")]
    pub type_name: String,
    pub methods: Vec<OverridableMethod>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOverridableMethodParams {
    pub context: CodeActionParams,
    pub overridable_methods: Option<Vec<OverridableMethod>>,
}
fn eligible(m: BindingRef<'_>) -> bool {
    !m.is_constructor() && m.modifiers() & (modifier::STATIC | modifier::PRIVATE) == 0
}
fn add<'a>(m: BindingRef<'a>, out: &mut Vec<BindingRef<'a>>) {
    if eligible(m)
        && !out
            .iter()
            .any(|other| overridden(*other, m) || subsignature(*other, m))
    {
        out.push(m);
    }
}
fn object(ast: &Ast) -> Option<BindingRef<'_>> {
    ast.binding_by_key("Ljava/lang/Object;")
}
fn methods<'a>(ast: &'a Ast, binding: BindingRef<'a>) -> Vec<BindingRef<'a>> {
    fn interfaces<'a>(
        t: BindingRef<'a>,
        out: &mut Vec<BindingRef<'a>>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(t.key().into()) {
            return;
        }
        for m in t.declared_methods().unwrap_or_default() {
            add(m, out);
        }
        for i in t.interfaces() {
            interfaces(i, out, seen);
        }
    }
    let declared = binding.declared_methods().unwrap_or_default();
    let mut out: Vec<_> = declared
        .iter()
        .copied()
        .filter(|m| eligible(*m) && !m.has(bflag::SYNTHETIC_RECORD_METHOD))
        .collect();
    let mut current = binding.superclass();
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(t) = current {
        if !seen.insert(t.key().into()) {
            break;
        }
        for m in t.declared_methods().unwrap_or_default() {
            add(m, &mut out);
        }
        current = t.superclass();
    }
    let mut current = Some(binding);
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(t) = current {
        if !seen.insert(t.key().into()) {
            break;
        }
        for i in t.interfaces() {
            interfaces(i, &mut out, &mut HashSet::new());
        }
        current = t.superclass();
    }
    if binding.is_interface() {
        if let Some(o) = object(ast) {
            interfaces(o, &mut out, &mut HashSet::new());
        }
    }
    out.retain(|m| !declared.contains(m) && m.modifiers() & modifier::FINAL == 0);
    out
}
fn has_supertype(t: BindingRef<'_>, name: &str, seen: &mut HashSet<String>) -> bool {
    if t.qualified_name() == name {
        return true;
    }
    if !seen.insert(t.key().into()) {
        return false;
    }
    t.superclass()
        .into_iter()
        .chain(t.interfaces())
        .any(|p| has_supertype(p, name, seen))
}
// SourceType.getTypeQualifiedName uses source names for local types and counts
// anonymous types within their JavaModel parent, rather than the binary name.
fn type_name(ast: &Ast, selected: &accessors::Selection) -> String {
    fn find(types: &[TypeDecl], prefix: &str, selected: &TypeDecl) -> Option<String> {
        let mut occurrence = 0;
        for t in types {
            let name = if t.anonymous {
                occurrence += 1;
                occurrence.to_string()
            } else {
                t.name.clone()
            };
            let name = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}${name}")
            };
            if t.source == selected.source && t.name_range == selected.name_range {
                return Some(name);
            }
            for member in &t.members {
                let children = match member {
                    Member::Type(t) => std::slice::from_ref(t),
                    Member::Method(m) => m.children.as_slice(),
                    Member::Field(f) => f.children.as_slice(),
                    Member::Initializer(i) => i.children.as_slice(),
                };
                if let Some(found) = find(children, &name, selected) {
                    return Some(found);
                }
            }
        }
        None
    }
    find(&java_model::parse(ast.text()).types, "", &selected.model)
        .unwrap_or_else(|| selected.model.name.clone())
}
pub async fn list(d: &Dispatcher, params: CodeActionParams) -> OverridableMethodsResponse {
    let Ok(ast) = crate::semantic_ast::fetch(d, &params.text_document.uri).await else {
        return OverridableMethodsResponse::default();
    };
    let context = accessors::context(ast, &params);
    let Some(selected) = accessors::selection(&context, &params.text_document.uri) else {
        return OverridableMethodsResponse::default();
    };
    let Some(binding) = context.ast.node(selected.declaration).binding() else {
        return OverridableMethodsResponse {
            type_name: selected.model.name,
            methods: Vec::new(),
        };
    };
    let cloneable = has_supertype(binding, "java.lang.Cloneable", &mut HashSet::new());
    let methods = methods(&context.ast, binding)
        .into_iter()
        .filter(|m| {
            m.modifiers() & (modifier::PUBLIC | modifier::PROTECTED) != 0
                || m.declaring_class()
                    .is_some_and(|t| t.is_interface() || t.package_name() == binding.package_name())
        })
        .map(|m| {
            let dc = m.declaring_class().unwrap();
            OverridableMethod {
                binding_key: m.key().into(),
                name: m.name().into(),
                parameters: m
                    .parameter_types()
                    .iter()
                    .map(|p| p.name().into())
                    .collect(),
                unimplemented: m.modifiers() & modifier::ABSTRACT != 0
                    || cloneable
                        && dc.qualified_name() == "java.lang.Object"
                        && m.name() == "clone"
                        && m.parameter_types().is_empty(),
                declaring_class: dc.qualified_name().into(),
                declaring_class_type: if dc.is_interface() {
                    "interface"
                } else {
                    "class"
                }
                .into(),
            }
        })
        .collect();
    OverridableMethodsResponse {
        type_name: type_name(&context.ast, &selected),
        methods,
    }
}
pub async fn generate(env: &Env<'_>, params: AddOverridableMethodParams) -> Option<WorkspaceEdit> {
    let wanted = params.overridable_methods?;
    if wanted.is_empty() {
        return None;
    }
    let uri = &params.context.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let context = accessors::context(ast.clone(), &params.context);
    let selected = accessors::selection(&context, uri)?;
    let binding = ast.node(selected.declaration).binding()?;
    let keys: HashSet<_> = wanted.iter().map(|m| m.binding_key.as_str()).collect();
    let chosen: Vec<_> = methods(&ast, binding)
        .into_iter()
        .filter(|m| keys.contains(m.key()))
        .collect();
    let range = params.context.range;
    let start = ast.offset_of(range.start).unwrap_or(0);
    let end = ast.offset_of(range.end).unwrap_or(start);
    let cursor = if accessors::declaration_node(crate::semantic_ast::finder::NodeFinder::perform(
        ast.root(),
        start,
        end.saturating_sub(start),
    ))
    .is_some()
    {
        None
    } else {
        Some(range)
    };
    let before = accessors::insert_before(&ast, &selected, cursor);
    let mut change = operation::create(env, ast.clone(), &selected, &chosen, before)
        .await
        .ok()?;
    let tree = crate::correction::edit::cu_tree(env, &mut change)
        .await
        .ok()?;
    Some(WorkspaceEdit {
        changes: Some(
            [(
                uri.clone(),
                crate::correction::edit::tree_to_text_edits(&ast.source, &tree),
            )]
            .into(),
        ),
        ..Default::default()
    })
}
