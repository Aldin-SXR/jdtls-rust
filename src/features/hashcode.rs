//! HashCodeEqualsHandler and JdtDomModels field selection.
pub(crate) mod actions;
mod operation;
use super::{
    accessors::{self, Selection},
    constructors::LspVariableBinding,
    tostring,
};
use crate::{
    analysis::dispatcher::Dispatcher,
    correction::{edit::Env, Context},
    semantic_ast::BindingRef,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};
use tower_lsp::lsp_types::{CodeActionParams, WorkspaceEdit};
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckHashCodeEqualsResponse {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<LspVariableBinding>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_methods: Option<Vec<String>>,
}
#[derive(Deserialize)]
pub struct GenerateHashCodeEqualsParams {
    pub context: CodeActionParams,
    pub fields: Option<Vec<LspVariableBinding>>,
    #[serde(default)]
    pub regenerate: bool,
}
pub async fn check(d: &Dispatcher, params: CodeActionParams) -> CheckHashCodeEqualsResponse {
    let Ok(ast) = crate::semantic_ast::fetch(d, &params.text_document.uri).await else {
        return CheckHashCodeEqualsResponse::default();
    };
    let context = accessors::context(ast, &params);
    let Some(selected) = accessors::selection(&context, &params.text_document.uri) else {
        return CheckHashCodeEqualsResponse::default();
    };
    status(&context, &selected)
}
pub(crate) fn signature(m: BindingRef<'_>, name: &str) -> bool {
    m.name() == name
        && match name {
            "equals" => {
                let p = m.parameter_types();
                p.len() == 1 && p[0].qualified_name() == "java.lang.Object"
            }
            _ => m.parameter_types().is_empty(),
        }
}
fn fields(binding: BindingRef<'_>) -> Vec<BindingRef<'_>> {
    tostring::ordered(
        binding
            .declared_fields()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| !f.is_static())
            .collect(),
    )
}
fn status(context: &Context, selected: &Selection) -> CheckHashCodeEqualsResponse {
    let Some(binding) = context.ast.node(selected.declaration).binding() else {
        return CheckHashCodeEqualsResponse::default();
    };
    let mut existing = Vec::new();
    for m in binding.declared_methods().unwrap_or_default() {
        if signature(m, "equals") || signature(m, "hashCode") {
            existing.push(m.name().into());
        }
        if existing.len() == 2 {
            break;
        }
    }
    let binary = binding.binary_name().unwrap_or(binding.name());
    CheckHashCodeEqualsResponse {
        type_name: Some(
            binary
                .strip_prefix(&format!("{}.", binding.package_name().unwrap_or("")))
                .unwrap_or(binary)
                .into(),
        ),
        fields: Some(
            fields(binding)
                .into_iter()
                .map(|f| tostring::dto(f, false))
                .collect(),
        ),
        existing_methods: Some(existing),
    }
}
pub async fn generate(
    env: &Env<'_>,
    params: GenerateHashCodeEqualsParams,
) -> Option<WorkspaceEdit> {
    let wanted = params.fields?;
    let uri = &params.context.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let context = accessors::context(ast.clone(), &params.context);
    let selected = accessors::selection(&context, uri)?;
    let binding = ast.node(selected.declaration).binding()?;
    let keys: HashSet<_> = wanted.iter().map(|f| f.binding_key.as_str()).collect();
    let fields: Vec<_> = fields(binding)
        .into_iter()
        .filter(|f| keys.contains(f.key()))
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
    let mut change = operation::create(
        env,
        Arc::clone(&ast),
        &selected,
        &fields,
        before,
        params.regenerate,
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
