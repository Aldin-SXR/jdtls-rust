//! GenerateDelegateMethodsHandler and StubUtility2Core's delegate discovery.
pub(crate) mod actions;
mod operation;
use super::{
    accessors,
    constructors::{LspMethodBinding, LspVariableBinding},
    tostring,
};
use crate::{
    analysis::dispatcher::Dispatcher,
    correction::edit::Env,
    semantic_ast::{bflag, modifier, BindingRef},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tower_lsp::lsp_types::{CodeActionParams, WorkspaceEdit};
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDelegateMethodsResponse {
    pub delegate_fields: Vec<LspDelegateField>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDelegateField {
    pub field: LspVariableBinding,
    pub delegate_methods: Vec<LspMethodBinding>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDelegateEntry {
    pub field: LspVariableBinding,
    pub delegate_method: LspMethodBinding,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDelegateMethodsParams {
    pub context: CodeActionParams,
    #[serde(default)]
    pub delegate_entries: Option<Vec<LspDelegateEntry>>,
}
#[derive(Clone, Copy)]
struct Entry<'a> {
    field: BindingRef<'a>,
    method: BindingRef<'a>,
}
pub(crate) fn erasure(t: BindingRef<'_>) -> BindingRef<'_> {
    t.erasure().unwrap_or(t)
}
fn subtype(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    fn visit(a: BindingRef<'_>, b: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
        if a.key() == b.key() {
            return true;
        }
        // Interface bindings have no superclass edge, but every reference
        // type is subtype-compatible with Object in JDT.
        if b.qualified_name() == "java.lang.Object" && !a.is_primitive() {
            return true;
        }
        if !seen.insert(a.key().into()) {
            return false;
        }
        if a.is_array() {
            if [
                "java.lang.Object",
                "java.lang.Cloneable",
                "java.io.Serializable",
            ]
            .contains(&b.qualified_name())
            {
                return true;
            }
            if b.is_array() {
                return a
                    .component_type()
                    .zip(b.component_type())
                    .is_some_and(|(a, b)| visit(a, b, seen));
            }
        }
        a.superclass()
            .into_iter()
            .chain(a.interfaces())
            .chain(a.type_bounds())
            .any(|t| visit(t, b, seen))
    }
    visit(a, b, &mut HashSet::new())
}
/// Bindings.areOverriddenMethods is deliberately different from subsignature:
/// it checks erased parameters, covariant returns and declared exceptions.
pub(crate) fn overridden(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    a.name() == b.name()
        && a.parameter_types().len() == b.parameter_types().len()
        && a.return_type()
            .zip(b.return_type())
            .is_some_and(|(a, b)| subtype(erasure(a), erasure(b)))
        && a.parameter_types()
            .iter()
            .zip(b.parameter_types())
            .all(|(a, b)| erasure(*a).key() == erasure(b).key())
        && a.exception_types()
            .iter()
            .all(|a| b.exception_types().iter().any(|b| subtype(*a, *b)))
}
fn contains_variables(t: BindingRef<'_>) -> bool {
    t.is_type_variable()
        || t.element_type()
            .filter(|_| t.is_array())
            .is_some_and(contains_variables)
        || t.bound().is_some_and(contains_variables)
        || t.type_arguments().into_iter().any(contains_variables)
}
fn bounds(t: BindingRef<'_>) -> HashSet<String> {
    let bs = t.type_bounds();
    if bs
        .first()
        .is_some_and(|b| b.qualified_name() == "java.lang.Object")
    {
        return HashSet::new();
    }
    bs.into_iter()
        .map(|b| {
            if contains_variables(b) {
                erasure(b)
            } else if b.is_raw_type() {
                b.type_declaration().unwrap_or(b)
            } else {
                b
            }
        })
        .map(|b| b.key().into())
        .collect()
}
/// Port of Bindings.isSubsignature used for the final-method hierarchy check.
pub(crate) fn subsignature(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    let (ap, bp, at, bt) = (
        a.parameter_types(),
        b.parameter_types(),
        a.type_parameters(),
        b.type_parameters(),
    );
    if a.name() != b.name() || ap.len() != bp.len() || (!at.is_empty() && at.len() != bt.len()) {
        return false;
    }
    if !bt.is_empty()
        && !at
            .iter()
            .zip(bt.iter())
            .all(|(a, b)| bounds(*a) == bounds(*b))
    {
        return false;
    }
    ap.iter().zip(bp).all(|(a, b)| {
        let a = if !bt.is_empty() && (contains_variables(*a) || a.is_raw_type()) {
            erasure(*a)
        } else if a.is_raw_type() {
            a.type_declaration().unwrap_or(*a)
        } else {
            *a
        };
        a.key() == b.key() || a.key() == erasure(b).key()
    })
}
fn hierarchy_method<'a>(
    t: BindingRef<'a>,
    m: BindingRef<'a>,
    seen: &mut HashSet<String>,
) -> Option<BindingRef<'a>> {
    if !seen.insert(t.key().into()) {
        return None;
    }
    t.declared_methods()
        .unwrap_or_default()
        .into_iter()
        .find(|b| subsignature(m, *b))
        .or_else(|| {
            t.superclass()
                .into_iter()
                .chain(t.interfaces())
                .find_map(|t| hierarchy_method(t, m, seen))
        })
}
fn entries(binding: BindingRef<'_>) -> Vec<Entry<'_>> {
    fn collect<'a>(
        t: BindingRef<'a>,
        owner: BindingRef<'a>,
        field: BindingRef<'a>,
        methods: &mut Vec<BindingRef<'a>>,
        out: &mut Vec<Entry<'a>>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(t.key().into()) {
            return;
        }
        if t.is_type_variable() {
            let bs = t.type_bounds();
            if bs.is_empty() {
                let mut root = owner;
                while let Some(s) = root.superclass() {
                    root = s;
                }
                if root.qualified_name() == "java.lang.Object" {
                    collect(root, owner, field, methods, out, seen);
                }
            } else {
                for b in bs {
                    collect(b, owner, field, methods, out, seen);
                }
            }
            return;
        }
        for m in t.declared_methods().unwrap_or_default() {
            if m.is_constructor()
                || m.is_static()
                || (!t.is_interface() && m.modifiers() & modifier::PUBLIC == 0)
            {
                continue;
            }
            if hierarchy_method(owner, m, &mut HashSet::new()).is_some_and(|b| {
                b.modifiers() & modifier::FINAL != 0 && !b.has(bflag::SYNTHETIC_RECORD_METHOD)
            }) {
                continue;
            }
            if m.parameter_types()
                .iter()
                .any(|p| p.is_wildcard_type() && p.has(bflag::UPPERBOUND))
            {
                continue;
            }
            if !methods.iter().any(|existing| overridden(*existing, m)) {
                out.push(Entry { field, method: m });
                methods.push(m);
            }
        }
        for parent in t.superclass().into_iter().chain(t.interfaces()) {
            collect(parent, owner, field, methods, out, seen);
        }
    }
    let declared: Vec<_> = binding
        .declared_methods()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| !m.has(bflag::SYNTHETIC_RECORD_METHOD))
        .collect();
    let mut out = Vec::new();
    for f in binding.declared_fields().unwrap_or_default() {
        if f.is_field() && !f.is_enum_constant() && !f.has(bflag::SYNTHETIC) {
            if let Some(t) = f.var_type() {
                collect(
                    t,
                    binding,
                    f,
                    &mut declared.clone(),
                    &mut out,
                    &mut HashSet::new(),
                );
            }
        }
    }
    out
}
pub async fn check(d: &Dispatcher, params: CodeActionParams) -> CheckDelegateMethodsResponse {
    let Ok(ast) = crate::semantic_ast::fetch(d, &params.text_document.uri).await else {
        return CheckDelegateMethodsResponse::default();
    };
    let context = accessors::context(ast, &params);
    let Some(selected) = accessors::selection(&context, &params.text_document.uri) else {
        return CheckDelegateMethodsResponse::default();
    };
    let Some(binding) = context.ast.node(selected.declaration).binding() else {
        return CheckDelegateMethodsResponse::default();
    };
    let mut fields: Vec<LspDelegateField> = Vec::new();
    for e in entries(binding) {
        let i = fields
            .iter()
            .position(|f| f.field.binding_key == e.field.key())
            .unwrap_or_else(|| {
                fields.push(LspDelegateField {
                    field: tostring::dto(e.field, false),
                    delegate_methods: Vec::new(),
                });
                fields.len() - 1
            });
        fields[i].delegate_methods.push(LspMethodBinding {
            binding_key: e.method.key().into(),
            name: e.method.name().into(),
            parameters: e
                .method
                .parameter_types()
                .iter()
                .map(|p| p.name().into())
                .collect(),
        });
    }
    CheckDelegateMethodsResponse {
        delegate_fields: fields,
    }
}
pub async fn generate(
    env: &Env<'_>,
    params: GenerateDelegateMethodsParams,
) -> tower_lsp::jsonrpc::Result<Option<WorkspaceEdit>> {
    let Some(wanted) = params.delegate_entries else {
        return Ok(None);
    };
    if wanted.is_empty() {
        return Ok(None);
    }
    let uri = &params.context.text_document.uri;
    let Ok(ast) = crate::semantic_ast::fetch(env.dispatcher, uri).await else {
        return Ok(None);
    };
    let context = accessors::context(ast.clone(), &params.context);
    let Some(selected) = accessors::selection(&context, uri) else {
        return Ok(None);
    };
    let Some(binding) = ast.node(selected.declaration).binding() else {
        return Ok(None);
    };
    let available = entries(binding);
    let mut chosen: Vec<_> = wanted
        .iter()
        .filter_map(|w| {
            available
                .iter()
                .rev()
                .find(|e| {
                    e.field.key() == w.field.binding_key
                        && e.method.key() == w.delegate_method.binding_key
                })
                .copied()
        })
        .collect();
    // Upstream operation rejects a nonempty request whose keys all went stale.
    if chosen.is_empty() {
        return Err(tower_lsp::jsonrpc::Error::internal_error());
    }
    chosen.sort_by(|a, b| {
        if a.field == b.field {
            let (a, b) = (a.method, b.method);
            if a.data().source_offset >= 0 && b.data().source_offset >= 0 {
                a.data().source_offset.cmp(&b.data().source_offset)
            } else {
                a.name().cmp(b.name())
            }
        } else {
            a.field
                .data()
                .source_offset
                .cmp(&b.field.data().source_offset)
        }
    });
    let before = accessors::insert_before(&ast, &selected, Some(params.context.range));
    let Ok(mut change) = operation::create(env, ast.clone(), &selected, &chosen, before).await
    else {
        return Ok(None);
    };
    let Ok(tree) = crate::correction::edit::cu_tree(env, &mut change).await else {
        return Ok(None);
    };
    Ok(Some(WorkspaceEdit {
        changes: Some(
            [(
                uri.clone(),
                crate::correction::edit::tree_to_text_edits(&ast.source, &tree),
            )]
            .into(),
        ),
        ..Default::default()
    }))
}
