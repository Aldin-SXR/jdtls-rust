//! GenerateToStringHandler and JdtDomModels discovery/selection in Rust.
pub(crate) mod actions;
mod operation;
mod template;
use super::{
    accessors::{self, Selection},
    constructors::LspVariableBinding,
    java_model::Member,
};
use crate::{
    analysis::dispatcher::Dispatcher,
    correction::{edit::Env, Context, CuChange},
    semantic_ast::{modifier, Ast, BindingRef},
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet, sync::Arc};
use tower_lsp::lsp_types::{CodeActionParams, Range, WorkspaceEdit};
#[derive(Default, Serialize)]
pub struct CheckToStringResponse {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<LspVariableBinding>>,
    pub exists: bool,
}
#[derive(Deserialize)]
pub struct GenerateToStringParams {
    pub context: CodeActionParams,
    pub fields: Option<Vec<LspVariableBinding>>,
}
pub async fn check(d: &Dispatcher, params: CodeActionParams) -> CheckToStringResponse {
    let Ok(ast) = crate::semantic_ast::fetch(d, &params.text_document.uri).await else {
        return CheckToStringResponse::default();
    };
    let context = accessors::context(ast, &params);
    let Some(selected) = accessors::selection(&context, &params.text_document.uri) else {
        return CheckToStringResponse::default();
    };
    status(&context, &selected)
}
fn order(a: BindingRef<'_>, b: BindingRef<'_>) -> Ordering {
    let offset = |b: BindingRef<'_>| {
        if b.data().name_offset >= 0 {
            Some(b.data().name_offset as usize)
        } else {
            b.declaring_node()
                .and_then(|n| n.child("name"))
                .map(|n| n.start())
        }
    };
    match (offset(a), offset(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => Ordering::Equal,
    }
}
pub(crate) fn ordered(mut b: Vec<BindingRef<'_>>) -> Vec<BindingRef<'_>> {
    b.sort_by(|a, b| order(*a, *b));
    b
}
fn own_fields<'a>(binding: BindingRef<'a>, selected: &Selection) -> Vec<BindingRef<'a>> {
    let fields = binding.declared_fields().unwrap_or_default();
    selected
        .model
        .members
        .iter()
        .filter_map(|m| match m {
            Member::Field(f)
                if !f.enum_constant && (!binding.is_record() || f.record_component) =>
            {
                fields
                    .iter()
                    .find(|b| b.name() == f.name && !b.is_static())
                    .copied()
            }
            _ => None,
        })
        .collect()
}
fn methods(binding: BindingRef<'_>, inherited: bool) -> Vec<BindingRef<'_>> {
    binding
        .declared_methods()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            !m.is_static()
                && (!inherited || m.modifiers() & modifier::PRIVATE == 0)
                && m.parameter_types().is_empty()
                && m.return_type().is_some_and(|t| t.name() != "void")
                && m.name() != "clone"
                && (inherited || m.name() != "toString")
        })
        .collect()
}
fn parents(mut binding: BindingRef<'_>) -> Vec<BindingRef<'_>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    while let Some(parent) = binding.superclass() {
        if !seen.insert(parent.id) {
            break;
        }
        out.push(parent);
        binding = parent;
    }
    out
}
pub(crate) fn dto(b: BindingRef<'_>, selected: bool) -> LspVariableBinding {
    let method = b.return_type().is_some();
    LspVariableBinding {
        binding_key: b.key().into(),
        name: b.name().into(),
        type_name: if method {
            b.return_type()
        } else {
            b.var_type()
        }
        .map(|t| t.name().into())
        .unwrap_or_default(),
        is_field: b.is_field(),
        is_selected: selected,
        parameters: method.then(|| {
            b.parameter_types()
                .iter()
                .map(|t| t.name().into())
                .collect()
        }),
    }
}
pub(crate) fn exists(selected: &Selection) -> bool {
    selected
        .model
        .members
        .iter()
        .any(|m| matches!(m,Member::Method(m) if m.name=="toString" && m.params.is_empty()))
}
pub(crate) fn status(context: &Context, selected: &Selection) -> CheckToStringResponse {
    let Some(binding) = context.ast.node(selected.declaration).binding() else {
        return CheckToStringResponse::default();
    };
    let own = own_fields(binding, selected);
    let selected_fields = ordered(
        own.iter()
            .filter(|f| f.modifiers() & modifier::TRANSIENT == 0)
            .copied()
            .collect(),
    );
    let rest = ordered(
        own.iter()
            .filter(|f| f.modifiers() & modifier::TRANSIENT != 0)
            .copied()
            .collect(),
    );
    // The handler compares inherited fields only against the unselected fields.
    let inherited = ordered(
        parents(binding)
            .into_iter()
            .flat_map(|p| p.declared_fields().unwrap_or_default())
            .filter(|f| {
                !f.is_static()
                    && f.modifiers() & modifier::PRIVATE == 0
                    && !rest.iter().any(|r| r.name() == f.name())
            })
            .collect(),
    );
    let own_methods = ordered(methods(binding, false));
    let inherited_methods = ordered(
        parents(binding)
            .into_iter()
            .flat_map(|p| methods(p, true))
            .filter(|m| !own_methods.iter().any(|r| r.name() == m.name()))
            .collect(),
    );
    let fields = selected_fields
        .iter()
        .map(|b| dto(*b, true))
        .chain(
            rest.iter()
                .chain(inherited.iter())
                .chain(own_methods.iter())
                .chain(inherited_methods.iter())
                .map(|b| dto(*b, false)),
        )
        .collect();
    let type_name = binding
        .binary_name()
        .unwrap_or(binding.name())
        .strip_prefix(&format!("{}.", binding.package_name().unwrap_or("")))
        .unwrap_or(binding.binary_name().unwrap_or(binding.name()))
        .to_owned();
    CheckToStringResponse {
        type_name: Some(type_name),
        fields: Some(fields),
        exists: exists(selected),
    }
}
fn selected_members<'a>(
    ast: &'a Ast,
    selected: &Selection,
    fields: &[LspVariableBinding],
) -> Vec<BindingRef<'a>> {
    let Some(binding) = ast.node(selected.declaration).binding() else {
        return Vec::new();
    };
    let wanted: HashSet<_> = fields.iter().map(|f| f.binding_key.as_str()).collect();
    let mut variables: Vec<_> = binding
        .declared_fields()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| !f.is_static())
        .collect();
    for p in parents(binding) {
        for f in p.declared_fields().unwrap_or_default() {
            if !f.is_static()
                && f.modifiers() & modifier::PRIVATE == 0
                && !variables.iter().any(|r| r.name() == f.name())
            {
                variables.push(f)
            }
        }
    }
    let mut functions = methods(binding, false);
    for p in parents(binding) {
        for m in methods(p, true) {
            if !functions.iter().any(|r| r.name() == m.name()) {
                functions.push(m)
            }
        }
    }
    ordered(variables)
        .into_iter()
        .chain(ordered(functions))
        .filter(|b| wanted.contains(b.key()))
        .collect()
}
pub async fn generate(env: &Env<'_>, params: GenerateToStringParams) -> Option<WorkspaceEdit> {
    let fields = params.fields?;
    let uri = &params.context.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let context = accessors::context(ast.clone(), &params.context);
    let selected = accessors::selection(&context, uri)?;
    let mut change = create_change(
        env,
        ast.clone(),
        &selected,
        &fields,
        Some(params.context.range),
    )
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
pub(crate) async fn create_change(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    fields: &[LspVariableBinding],
    cursor: Option<Range>,
) -> anyhow::Result<CuChange> {
    let cursor = cursor.filter(|range| {
        let start = ast.offset_of(range.start).unwrap_or(0);
        let end = ast.offset_of(range.end).unwrap_or(start);
        accessors::declaration_node(crate::semantic_ast::finder::NodeFinder::perform(
            ast.root(),
            start,
            end.saturating_sub(start),
        ))
        .is_none()
    });
    let before = accessors::insert_before(&ast, selected, cursor);
    let bindings = selected_members(&ast, selected, fields);
    operation::create(env, ast.clone(), selected, &bindings, before).await
}
