//! The argument, signature, cast and type-change proposals of
//! `UnresolvedElementsBaseSubProcessor` and the proposal classes they use
//! (`AddArgumentCorrectionProposalCore`, `ChangeMethodSignatureProposalCore`,
//! `CastCorrectionProposalCore`, `TypeChangeCorrectionProposalCore`,
//! `NewAnnotationMemberProposalCore`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::names;
use super::new_method::target_ast;
use super::scope;
use super::types::{self, can_assign, declaration, is_cast_compatible, method_declaration, normalize, normalize_wildcard, type_label, type_names, well_known};
use super::Units;
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::flattener::Flattener;
use crate::rewrite::import_rewrite::{ImportRewrite, ImportRewriteContext, TypeLocation, KIND_STATIC_FIELD, KIND_STATIC_METHOD, RES_NAME_CONFLICT};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeId, NodeKind};

fn context_for(ast: &Arc<Ast>, node: Node<'_>) -> ConstructorImportContext {
    let decl = crate::semantic_ast::resolve::find_parent_type(node).map(|n| n.id);
    ConstructorImportContext { ast: ast.clone(), declaration: decl, nullness: None }
}

/// `ASTNodes.asString(node)`.
fn as_string(ast: &Arc<Ast>, node: Node<'_>) -> String {
    let rw = ASTRewrite::new(ast.clone());
    Flattener::as_string(&rw, RNode::Orig(node.id))
}

fn rewrite_proposal(label: String, relevance: i32, rw: ASTRewrite, imports: Option<ImportRewrite>) -> Proposal {
    let mut cu = CuChange::rewrite(rw);
    if let Some(i) = imports {
        cu = cu.with_imports(i);
    }
    Proposal::new(label, kind::QUICK_FIX, relevance, Change::Cu(vec![cu]))
}

// ─── Static import favorites ────────────────────────────────────────────────

/// `ContextSensitiveImportRewriteContext` for static members: a member of
/// the same name declared in scope conflicts.
struct StaticContext<'a> {
    types: ConstructorImportContext,
    in_scope: Vec<String>,
    _node: Node<'a>,
}

impl ImportRewriteContext for StaticContext<'_> {
    fn find_in_context(&self, imports: &ImportRewrite, qualifier: &str, name: &str, kind: i32) -> i32 {
        if (kind == KIND_STATIC_METHOD || kind == KIND_STATIC_FIELD) && self.in_scope.iter().any(|n| n == name) {
            return RES_NAME_CONFLICT;
        }
        self.types.find_in_context(imports, qualifier, name, kind)
    }
}

/// `addStaticImportFavoriteProposals`.
pub async fn static_import_favorites(env: &Env<'_>, ctx: &Context, node: Node<'_>, is_method: bool, proposals: &mut Vec<Proposal>) {
    let favorites = crate::features::preferences::organize_import_favorites();
    if favorites.is_empty() {
        return;
    }
    let ast = ctx.ast();
    let name = node.identifier();
    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ast.uri) else { return };
    let filename = crate::classfile::percent_decode(uri.path().rsplit('/').next().unwrap_or(""));
    let Some(primary) = ast
        .root()
        .list("types")
        .into_iter()
        .find(|n| n.child("name").is_some_and(|n| Some(n.identifier().as_str()) == filename.strip_suffix(".java")))
        .and_then(|n| n.child("name"))
    else {
        return;
    };
    let package = ast.root().child("package").and_then(|p| p.child("name")).map(|n| n.identifier()).unwrap_or_default();
    let package_decl = if package.is_empty() { String::new() } else { format!("package {package};") };
    let dummy = format!("{package_decl}public class {}{{\n static {{\n{name}", primary.identifier());
    let offset = dummy.encode_utf16().count();
    let mut completion_ctx = env.dispatcher.context_for(Some(&uri)).await;
    completion_ctx.files.insert(uri.to_string(), format!("{dummy}\n}}\n }}"));
    let Ok(result) = env.dispatcher.code_assist(&completion_ctx, uri.as_str(), offset, serde_json::json!({ "op": "complete", "favorites": favorites })).await else {
        return;
    };
    let Ok(raw) = serde_json::from_value::<crate::features::completion::proposal::EngineResult>(result) else { return };
    use crate::features::completion::proposal::kind as ck;
    let required_kind = if is_method { ck::METHOD_IMPORT } else { ck::FIELD_IMPORT };
    let mut found: Vec<String> = Vec::new();
    for p in raw.proposals.iter().filter(|p| p.name() == name) {
        for r in p.required() {
            if r.kind == required_kind {
                if let Some(owner) = r.declaration_signature.as_deref().and_then(|s| crate::features::completion::signature::to_string(s).ok()) {
                    let q = format!("{owner}.{name}");
                    if !found.contains(&q) {
                        found.push(q);
                    }
                }
            }
        }
    }
    let options = env.options(&ast.uri).await;
    let in_scope: Vec<String> = if is_method {
        scope::methods_in_scope(node).iter().map(|m| m.name().to_owned()).collect()
    } else {
        Vec::new()
    };
    for curr in found {
        let qualified_type = curr.rsplit_once('.').map(|(q, _)| q.to_owned()).unwrap_or_default();
        let simple_type = qualified_type.rsplit('.').next().unwrap_or("").to_owned();
        let element_label = format!("{simple_type}.{name}");
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let context = StaticContext { types: context_for(&ctx.ast, node), in_scope: in_scope.clone(), _node: node };
        let res = imports.add_static_import(&qualified_type, &name, !is_method, &context);
        let label = if res.contains('.') {
            let used = imports.add_import(&qualified_type, &context.types);
            let q = rw.new_name(&format!("{used}.{name}"));
            rw.replace(RNode::Orig(node.id), Some(q));
            messages::format(messages::correction("UnresolvedElementsSubProcessor_change_to_static_import_description"), &[&element_label])
        } else {
            messages::format(messages::correction("UnresolvedElementsSubProcessor_add_static_import_description"), &[&element_label])
        };
        proposals.push(rewrite_proposal(label, relevance::ADD_STATIC_IMPORT, rw, Some(imports)));
    }
}

// ─── Casts ──────────────────────────────────────────────────────────────────

/// `CastCorrectionProposalCore` (computed with the default import options).
pub struct CastProposal {
    pub ast: Arc<Ast>,
    pub node: NodeId,
    /// Key of the cast type (`None`: guessed).
    pub cast_type: Option<String>,
}

fn needs_inner_parentheses(node: Node<'_>) -> bool {
    matches!(node.kind(), NodeKind::InfixExpression | NodeKind::ConditionalExpression | NodeKind::Assignment | NodeKind::InstanceofExpression)
}

fn needs_outer_parentheses(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        NodeKind::MethodInvocation | NodeKind::FieldAccess => node.location_is("expression"),
        NodeKind::QualifiedName => node.location_is("qualifier"),
        _ => false,
    }
}

impl CastProposal {
    fn cast_type_node(&self, rw: &mut ASTRewrite, imports: &mut ImportRewrite) -> RNode {
        let ast = &*self.ast;
        let node_to_cast = ast.node(self.node);
        let context = context_for(&self.ast, node_to_cast);
        if let Some(key) = &self.cast_type {
            if let Some(b) = ast.binding_by_key(key) {
                return imports.add_import_type(b, rw, &context, TypeLocation::Cast);
            }
        }
        let mut node = node_to_cast;
        let mut parent = node.parent();
        if parent.is_some_and(|p| p.is(NodeKind::CastExpression)) {
            node = parent.unwrap();
            parent = node.parent();
        }
        while parent.is_some_and(|p| p.is(NodeKind::ParenthesizedExpression)) {
            node = parent.unwrap();
            parent = node.parent();
        }
        if let Some(invocation) = parent.filter(|p| p.is(NodeKind::MethodInvocation)) {
            if node.location_is("expression") {
                let target_context = scope::parent_method_or_type_binding(node);
                let selector = invocation.child("name").map(|n| n.identifier()).unwrap_or_default();
                let bindings = scope::qualifier_guess(node.root(), &selector, invocation.list("arguments").len(), target_context);
                if !bindings.is_empty() {
                    let first = cast_favorite(&bindings, node_to_cast.type_binding());
                    return imports.add_import_type(first, rw, &context, TypeLocation::Cast);
                }
            }
        }
        let n = rw.new_simple_name("Object");
        rw.new_simple_type(n)
    }
}

fn cast_favorite<'a>(suggested: &[BindingRef<'a>], node_type: Option<BindingRef<'a>>) -> BindingRef<'a> {
    let Some(node_type) = node_type else { return suggested[0] };
    let mut favourite = suggested[0];
    for curr in suggested {
        if is_cast_compatible(node_type, *curr) {
            return *curr;
        }
        if curr.is_interface() {
            favourite = *curr;
        }
    }
    favourite
}

#[tower_lsp::async_trait]
impl LazyChange for CastProposal {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let mut imports = ImportRewrite::create_for_corrections(self.ast.clone(), &options);
        let mut rw = ASTRewrite::new(self.ast.clone());
        let typ = self.cast_type_node(&mut rw, &mut imports);
        let node = self.ast.node(self.node);
        if node.is(NodeKind::CastExpression) {
            if let Some(t) = node.child("type") {
                rw.replace(RNode::Orig(t.id), Some(typ));
            }
        } else {
            let mut copy = rw.create_copy_target(node.id);
            if needs_inner_parentheses(node) {
                copy = rw.new_parenthesized_expression(copy);
            }
            let cast = rw.new_node(NodeKind::CastExpression);
            rw.put_child(cast, "type", typ);
            rw.put_child(cast, "expression", copy);
            let replacing = if needs_outer_parentheses(node) { rw.new_parenthesized_expression(cast) } else { cast };
            rw.replace(RNode::Orig(node.id), Some(replacing));
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

fn cast_proposal(ctx: &Context, label: String, node: Node<'_>, cast_type: Option<BindingRef<'_>>, relevance: i32) -> Proposal {
    Proposal::new(
        label,
        kind::QUICK_FIX,
        relevance,
        Change::Lazy(Box::new(CastProposal { ast: ctx.ast.clone(), node: node.id, cast_type: cast_type.map(|t| t.key().to_owned()) })),
    )
}

/// `addMissingCastParentsProposal`.
pub fn missing_cast_parents(ctx: &Context, invocation: Node<'_>, proposals: &mut Vec<Proposal>) {
    let Some(sender) = invocation.child("expression") else { return };
    if sender.is(NodeKind::ThisExpression) {
        return;
    }
    let Some(sender_binding) = sender.type_binding() else { return };
    if sender_binding.modifiers() & modifier::FINAL != 0 {
        return;
    }
    if sender.kind().is_name() && sender.binding().is_some_and(|b| b.is_type()) {
        return;
    }
    let mut parent = invocation.parent();
    while let Some(p) = parent.filter(|p| p.kind().is_expression() && !p.is(NodeKind::CastExpression)) {
        parent = p.parent();
    }
    let mut has_cast = false;
    if let Some(cast) = parent.filter(|p| p.is(NodeKind::CastExpression)) {
        let arg_types = super::argument_types(ctx.ast(), &invocation.list("arguments"));
        has_cast = use_existing_parent_cast(ctx, cast, sender, invocation.child("name").unwrap(), arg_types.as_deref(), proposals);
    }
    if !has_cast {
        let target = crate::semantic_ast::resolve::unparenthesed_expression(sender);
        let label = if !target.is(NodeKind::CastExpression) {
            if target.length() <= 18 {
                let name = as_string(&ctx.ast, target);
                messages::format(messages::correction("UnresolvedElementsSubProcessor_methodtargetcast2_description"), &[&name])
            } else {
                messages::correction("UnresolvedElementsSubProcessor_methodtargetcast_description").to_owned()
            }
        } else if target.length() <= 18 {
            let name = as_string(&ctx.ast, target.child("expression").unwrap_or(target));
            messages::format(messages::correction("UnresolvedElementsSubProcessor_changemethodtargetcast2_description"), &[&name])
        } else {
            messages::correction("UnresolvedElementsSubProcessor_changemethodtargetcast_description").to_owned()
        };
        proposals.push(cast_proposal(ctx, label, target, None, relevance::CHANGE_CAST));
    }
}

/// `useExistingParentCastProposal`.
fn use_existing_parent_cast(ctx: &Context, expression: Node<'_>, access: Node<'_>, selector: Node<'_>, param_types: Option<&[BindingRef<'_>]>, proposals: &mut Vec<Proposal>) -> bool {
    let Some(cast_type) = expression.child("type").and_then(|t| t.binding()) else { return false };
    let name = selector.identifier();
    match param_types {
        Some(p) => {
            if types::find_method_in_hierarchy(cast_type, &name, Some(p)).is_none() {
                return false;
            }
        }
        None => {
            if types::find_field_in_hierarchy(cast_type, &name).is_none() {
                return false;
            }
        }
    }
    if let Some(b) = access.type_binding() {
        if !is_cast_compatible(b, cast_type) {
            return false;
        }
    }
    if types::find_method_in_hierarchy(cast_type, &name, param_types).is_none() {
        return false;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let new_cast = rw.new_node(NodeKind::CastExpression);
    let typ = rw.copy_subtree(RNode::Orig(expression.child("type").unwrap().id));
    rw.put_child(new_cast, "type", typ);
    let target = rw.create_copy_target(access.id);
    rw.put_child(new_cast, "expression", target);
    let parents = rw.new_parenthesized_expression(new_cast);
    let inner = rw.create_copy_target(expression.child("expression").unwrap().id);
    rw.replace(RNode::Orig(expression.id), Some(inner));
    rw.replace(RNode::Orig(access.id), Some(parents));
    let label = messages::correction("UnresolvedElementsSubProcessor_missingcastbrackets_description").to_owned();
    proposals.push(rewrite_proposal(label, relevance::ADD_PARENTHESES_AROUND_CAST, rw, None));
    true
}

// ─── Parameter mismatch ─────────────────────────────────────────────────────

/// `getArgumentName(arguments, index)`.
fn argument_name(ctx: &Context, arguments: &[Node<'_>], index: usize) -> String {
    let def = (index + 1).to_string();
    let expr = arguments[index];
    if expr.length() > 18 {
        return def;
    }
    for (i, a) in arguments.iter().enumerate() {
        if i != index && crate::semantic_ast::resolve::subtree_match(expr, *a) {
            return def;
        }
    }
    format!("'{}'", as_string(&ctx.ast, expr))
}

/// `doEqualNumberOfParameters`.
#[allow(clippy::too_many_arguments)]
pub async fn equal_number_of_parameters(
    env: &Env<'_>,
    ctx: &Context,
    units: &Units,
    problem: &ProblemLocation,
    invocation: Node<'_>,
    arguments: &[Node<'_>],
    arg_types: &[BindingRef<'_>],
    method: BindingRef<'_>,
    proposals: &mut Vec<Proposal>,
) {
    let ast = ctx.ast();
    let param_types = method.parameter_types();
    let mut diffs = Vec::new();
    for n in 0..arg_types.len() {
        if !can_assign(arg_types[n], param_types[n]) {
            diffs.push(n);
        }
    }
    let Some(declaring) = method.declaring_class() else { return };
    let declaring_decl = declaration(declaring);
    let Some(name_node) = problem.covering_node(ast) else { return };
    if method.is_constructor() && declaring_decl.is_record() {
        return;
    }
    if diffs.is_empty() {
        if let Some(inv) = name_node.parent().filter(|p| p.is(NodeKind::MethodInvocation)) {
            if inv.child("expression").is_none() {
                qualifier_to_outer(ctx, inv, method, proposals);
            }
        }
        return;
    }
    if diffs.len() == 1 {
        let idx = diffs[0];
        let node_to_cast = arguments[idx];
        let mut cast_type = normalize(Some(param_types[idx]));
        if let Some(w) = cast_type.filter(|t| t.is_wildcard_type()) {
            cast_type = normalize_wildcard(w, false);
        }
        if let Some(cast_type) = cast_type {
            let binding = node_to_cast.type_binding();
            let mut cast_fix = None;
            match binding {
                None => cast_fix = Some(cast_type),
                Some(b) if is_cast_compatible(cast_type, b) => cast_fix = Some(cast_type),
                Some(b) => {
                    let boxed = types::box_or_unbox(cast_type, b);
                    if boxed != cast_type && is_cast_compatible(boxed, b) {
                        cast_fix = Some(boxed);
                    }
                }
            }
            if let Some(fix) = cast_fix {
                let name = type_label(fix);
                let label = messages::format(
                    messages::correction("UnresolvedElementsSubProcessor_addargumentcast_description"),
                    &[&argument_name(ctx, arguments, idx), &name],
                );
                proposals.push(cast_proposal(ctx, label, node_to_cast, Some(fix), relevance::CAST_ARGUMENT_1));
            }
            crate::correction::type_mismatch::change_sender_type_proposals(env, ctx, node_to_cast, cast_type, false, relevance::CAST_ARGUMENT_2, proposals).await;
        }
    }
    if diffs.len() == 2 {
        let (idx1, idx2) = (diffs[0], diffs[1]);
        if can_assign(arg_types[idx1], param_types[idx2]) && can_assign(arg_types[idx2], param_types[idx1]) {
            let (arg1, arg2) = (arguments[idx1], arguments[idx2]);
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let c2 = rw.create_copy_target(arg2.id);
            rw.replace(RNode::Orig(arg1.id), Some(c2));
            let c1 = rw.create_copy_target(arg1.id);
            rw.replace(RNode::Orig(arg2.id), Some(c1));
            let label = messages::format(
                messages::correction("UnresolvedElementsSubProcessor_swaparguments_description"),
                &[&argument_name(ctx, arguments, idx1), &argument_name(ctx, arguments, idx2)],
            );
            proposals.push(rewrite_proposal(label, relevance::SWAP_ARGUMENTS, rw, None));
            if declaring_decl.is_from_source() {
                if let Some(target) = units.find(ast, declaring_decl) {
                    let decl = method_declaration(method);
                    let mut changes = vec![ChangeDesc::Keep; param_types.len()];
                    changes[idx1] = ChangeDesc::Swap(idx2);
                    let decl_params = decl.parameter_types();
                    let swapped = [decl_params[idx1], decl_params[idx2]];
                    let args = [types::method_label(decl), type_names(&swapped)];
                    let key = if decl.is_constructor() { "UnresolvedElementsSubProcessor_swapparams_constr_description" } else { "UnresolvedElementsSubProcessor_swapparams_description" };
                    let label = messages::format(messages::correction(key), &[&args[0], &args[1]]);
                    proposals.push(change_signature(ctx, label, target, decl, changes, relevance::CHANGE_METHOD_SWAP_PARAMETERS));
                }
            }
            return;
        }
    }
    if declaring_decl.is_from_source() {
        if let Some(target) = units.find(ast, declaring_decl) {
            let mut changes = vec![ChangeDesc::Keep; param_types.len()];
            for &d in &diffs {
                let arg = arguments[d];
                let name = names::expression_base_name(arg);
                let mut arg_type = arg_types[d];
                if arg_type.is_wildcard_type() {
                    match normalize_wildcard(arg_type, true) {
                        Some(t) => arg_type = t,
                        None => return,
                    }
                }
                changes[d] = ChangeDesc::Edit { typ: arg_type.key().to_owned(), name };
            }
            let decl = method_declaration(method);
            let decl_params = decl.parameter_types();
            let new_types: Vec<BindingRef<'_>> = changes
                .iter()
                .enumerate()
                .map(|(i, c)| match c {
                    ChangeDesc::Edit { typ, .. } => ast.binding_by_key(typ).unwrap_or(decl_params[i]),
                    _ => decl_params[i],
                })
                .collect();
            if decl.is_varargs() && new_types.last().is_some_and(|t| !t.is_array()) {
                let mut new_args: Vec<BindingRef<'_>> = arg_types.to_vec();
                new_args.push(*param_types.last().unwrap());
                more_arguments(ctx, units, invocation, arguments, &new_args, method, proposals);
                return;
            }
            let is_varargs = decl.is_varargs() && new_types.last().is_some_and(|t| t.is_array());
            let args = [types::method_label(decl), types::method_signature(decl.name(), &new_types, is_varargs)];
            let key = if decl.is_constructor() { "UnresolvedElementsSubProcessor_changeparamsignature_constr_description" } else { "UnresolvedElementsSubProcessor_changeparamsignature_description" };
            let label = messages::format(messages::correction(key), &[&args[0], &args[1]]);
            proposals.push(change_signature(ctx, label, target, decl, changes, relevance::CHANGE_METHOD_SIGNATURE));
        }
    }
}

/// `doMoreParameters`.
pub fn more_parameters(ctx: &Context, units: &Units, invocation: Node<'_>, arg_types: &[BindingRef<'_>], method: BindingRef<'_>, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let param_types = method.parameter_types();
    let diff = param_types.len() - arg_types.len();
    let mut k = 0;
    let mut skipped = Vec::new();
    for (i, p) in param_types.iter().enumerate() {
        if k < arg_types.len() && can_assign(arg_types[k], *p) {
            k += 1;
        } else {
            if skipped.len() >= diff {
                return;
            }
            skipped.push(i);
        }
    }
    let Some(declaring) = method.declaring_class() else { return };
    {
        let sig = types::method_label(method);
        let key = if diff == 1 { "UnresolvedElementsSubProcessor_addargument_description" } else { "UnresolvedElementsSubProcessor_addarguments_description" };
        let label = messages::format(messages::correction(key), &[&sig]);
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        for (i, &idx) in skipped.iter().enumerate() {
            let _ = i;
            let arg = argument_expression(&mut rw, invocation, param_types[idx]);
            rw.list_insert_at(RNode::Orig(invocation.id), "arguments", arg, idx as i32);
        }
        proposals.push(rewrite_proposal(label, relevance::ADD_ARGUMENTS, rw, None));
    }
    if !declaring.is_from_source() || (method.is_constructor() && declaring.is_record()) {
        return;
    }
    if let Some(target) = units.find(ast, declaring) {
        let decl = method_declaration(method);
        let decl_params = decl.parameter_types();
        let mut changes = vec![ChangeDesc::Keep; decl_params.len()];
        let mut changed = Vec::new();
        for &idx in &skipped {
            changes[idx] = ChangeDesc::Remove;
            changed.push(decl_params[idx]);
        }
        let args = [types::method_label(decl), type_names(&changed)];
        let key = match (decl.is_constructor(), diff == 1) {
            (true, true) => "UnresolvedElementsSubProcessor_removeparam_constr_description",
            (true, false) => "UnresolvedElementsSubProcessor_removeparams_constr_description",
            (false, true) => "UnresolvedElementsSubProcessor_removeparam_description",
            (false, false) => "UnresolvedElementsSubProcessor_removeparams_description",
        };
        let label = messages::format(messages::correction(key), &[&args[0], &args[1]]);
        proposals.push(change_signature(ctx, label, target, decl, changes, relevance::CHANGE_METHOD_REMOVE_PARAMETER));
    }
}

/// `doMoreArguments`.
pub fn more_arguments(ctx: &Context, units: &Units, invocation: Node<'_>, arguments: &[Node<'_>], arg_types: &[BindingRef<'_>], method: BindingRef<'_>, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let param_types = method.parameter_types();
    let diff = arg_types.len() - param_types.len();
    let mut k = 0;
    let mut skipped = Vec::new();
    for (i, a) in arg_types.iter().enumerate() {
        if k < param_types.len() && can_assign(*a, param_types[k]) {
            k += 1;
        } else {
            if skipped.len() >= diff {
                return;
            }
            skipped.push(i);
        }
    }
    {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        for &i in skipped.iter().rev() {
            rw.remove(RNode::Orig(arguments[i].id));
        }
        let sig = types::method_label(method);
        let key = if diff == 1 { "UnresolvedElementsSubProcessor_removeargument_description" } else { "UnresolvedElementsSubProcessor_removearguments_description" };
        let label = messages::format(messages::correction(key), &[&sig]);
        proposals.push(rewrite_proposal(label, relevance::REMOVE_ARGUMENTS, rw, None));
    }
    let decl = method_declaration(method);
    let Some(declaring) = decl.declaring_class() else { return };
    if !declaring.is_from_source() || (decl.is_constructor() && declaring.is_record()) {
        return;
    }
    let Some(target) = units.find(ast, declaring) else { return };
    if decl.has(crate::semantic_ast::bflag::DEFAULT_CONSTRUCTOR) {
        return;
    }
    let mut changes = vec![ChangeDesc::Keep; arg_types.len()];
    let mut change_types = Vec::new();
    for &idx in skipped.iter().rev() {
        let arg = arguments.get(idx).copied();
        let name = arg.and_then(names::expression_base_name);
        let mut new_type = normalize(Some(arg_types[idx])).or_else(|| well_known(ast, "java.lang.Object"));
        if let Some(w) = new_type.filter(|t| t.is_wildcard_type()) {
            new_type = normalize_wildcard(w, true);
        }
        let Some(new_type) = new_type else { return };
        if !types::is_useable_in_context(new_type, decl, false) {
            return;
        }
        changes[idx] = ChangeDesc::Insert { typ: new_type.key().to_owned(), name };
        change_types.insert(0, new_type);
    }
    let args = [types::method_label(decl), type_names(&change_types)];
    let key = match (decl.is_constructor(), diff == 1) {
        (true, true) => "UnresolvedElementsSubProcessor_addparam_constr_description",
        (true, false) => "UnresolvedElementsSubProcessor_addparams_constr_description",
        (false, true) => "UnresolvedElementsSubProcessor_addparam_description",
        (false, false) => "UnresolvedElementsSubProcessor_addparams_description",
    };
    let label = messages::format(messages::correction(key), &[&args[0], &args[1]]);
    proposals.push(change_signature(ctx, label, target, decl, changes, relevance::CHANGE_METHOD_ADD_PARAMETER));
}

/// `addQualifierToOuterProposal`.
fn qualifier_to_outer(ctx: &Context, invocation: Node<'_>, method: BindingRef<'_>, proposals: &mut Vec<Proposal>) {
    let Some(declaring) = method.declaring_class() else { return };
    let parent_type = types::parent_type_binding(invocation);
    let mut curr = parent_type;
    let is_instance = method.modifiers() & modifier::STATIC == 0;
    while let Some(c) = curr {
        if types::is_super_type(declaring, c) {
            break;
        }
        if is_instance && c.modifiers() & modifier::STATIC != 0 {
            return;
        }
        curr = c.declaring_class();
    }
    let Some(curr) = curr else { return };
    if parent_type.is_some_and(|p| p == curr) {
        return;
    }
    let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_changetoouter_description"), &[&type_label(curr)]);
    let ast = ctx.ast.clone();
    proposals.push(Proposal::new(
        label,
        kind::QUICK_FIX,
        relevance::QUALIFY_WITH_ENCLOSING_TYPE,
        Change::Lazy(Box::new(QualifyOuter { ast, invocation: invocation.id, outer: curr.key().to_owned(), is_instance })),
    ));
}

struct QualifyOuter {
    ast: Arc<Ast>,
    invocation: NodeId,
    outer: String,
    is_instance: bool,
}

#[tower_lsp::async_trait]
impl LazyChange for QualifyOuter {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let mut imports = ImportRewrite::create_for_corrections(self.ast.clone(), &options);
        let mut rw = ASTRewrite::new(self.ast.clone());
        let invocation = self.ast.node(self.invocation);
        let outer = self.ast.binding_by_key(&self.outer).ok_or_else(|| anyhow::anyhow!("no outer type"))?;
        let context = context_for(&self.ast, invocation);
        let qualifier = imports.add_import_binding(outer, &context);
        let name = rw.new_name(&qualifier);
        let expression = if self.is_instance {
            let this = rw.new_this_expression();
            rw.put_child(this, "qualifier", name)
        } else {
            name
        };
        rw.set(RNode::Orig(invocation.id), "expression", Some(expression));
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

// ─── AddArgumentCorrectionProposalCore ──────────────────────────────────────

/// `ASTNodeFactory.newDefaultExpression(ast, type)`.
pub(super) fn default_expression(rw: &mut ASTRewrite, t: BindingRef<'_>) -> RNode {
    if t.is_primitive() {
        match t.name() {
            "boolean" => {
                let n = rw.new_node(NodeKind::BooleanLiteral);
                rw.put_simple(n, "booleanValue", "false")
            }
            "char" => {
                let n = rw.new_node(NodeKind::CharacterLiteral);
                rw.put_simple(n, "escapedValue", "'\\0'")
            }
            _ => rw.new_number_literal("0"),
        }
    } else {
        rw.new_node(NodeKind::NullLiteral)
    }
}

fn argument_expression(rw: &mut ASTRewrite, caller: Node<'_>, required: BindingRef<'_>) -> RNode {
    let mut best: Option<(String, BindingRef<'_>)> = None;
    for v in scope::variables_in_scope(caller) {
        let Some(t) = v.var_type() else { continue };
        if can_assign(t, required) && test_modifier(v, caller) {
            let more_specific = |best: BindingRef<'_>, curr: BindingRef<'_>| can_assign(best, curr) && !can_assign(curr, best);
            if best.as_ref().is_none_or(|(_, bt)| more_specific(*bt, t)) {
                best = Some((v.name().to_owned(), t));
            }
        }
    }
    match best {
        Some((name, _)) => rw.new_simple_name(&name),
        None => default_expression(rw, required),
    }
}

fn test_modifier(v: BindingRef<'_>, caller: Node<'_>) -> bool {
    let mods = v.modifiers();
    let static_final = modifier::STATIC | modifier::FINAL;
    if mods & static_final == static_final {
        return false;
    }
    if mods & modifier::STATIC != 0 && !scope::is_in_static_context(caller) {
        return false;
    }
    true
}

// ─── ChangeMethodSignatureProposalCore ──────────────────────────────────────

#[derive(Clone, Debug)]
pub enum ChangeDesc {
    Keep,
    Swap(usize),
    Remove,
    Edit { typ: String, name: Option<String> },
    Insert { typ: String, name: Option<String> },
}

pub struct ChangeSignature {
    pub source: Arc<Ast>,
    pub method: String,
    pub target_uri: Option<String>,
    pub changes: Vec<ChangeDesc>,
}

fn change_signature(ctx: &Context, label: String, target: Option<String>, decl: BindingRef<'_>, changes: Vec<ChangeDesc>, relevance: i32) -> Proposal {
    Proposal::new(
        label,
        kind::QUICK_FIX,
        relevance,
        Change::Lazy(Box::new(ChangeSignature { source: ctx.ast.clone(), method: decl.key().to_owned(), target_uri: target, changes })),
    )
}

fn tag_rank(tag: &str) -> usize {
    let tag = if tag == "@exception" { "@throws" } else { tag };
    ["@author", "@version", "@param", "@return", "@throws", "@see", "@since", "@serial", "@deprecated"].iter().position(|t| *t == tag).unwrap_or(9)
}

fn tag_argument(tag: Node<'_>) -> Option<String> {
    let first = *tag.list("fragments").first()?;
    if first.kind().is_name() {
        return Some(first.child("name").unwrap_or(first).identifier());
    }
    None
}

fn find_param_tag<'a>(decl: Node<'a>, name: &str) -> Option<Node<'a>> {
    let doc = decl.child("javadoc")?;
    doc.list("tags").into_iter().find(|t| t.simple("tagName") == Some("@param") && tag_argument(*t).as_deref() == Some(name))
}

impl ChangeSignature {
    fn modify(&self, rw: &mut ASTRewrite, imports: &mut ImportRewrite, decl: Node<'_>, options: &BTreeMap<String, String>, target: &Arc<Ast>) {
        let source = &*self.source;
        let method = source.binding_by_key(&self.method);
        let mut used: Vec<String> = Vec::new();
        if let Some(c) = method.and_then(|m| m.declaring_class()) {
            used.extend(c.declared_fields().unwrap_or_default().iter().map(|f| f.name().to_owned()));
        }
        let context = ConstructorImportContext { ast: target.clone(), declaration: crate::semantic_ast::resolve::find_parent_type(decl).map(|n| n.id), nullness: None };
        let parameters = decl.list("parameters");
        let mut k = 0;
        // (desc index, names to set, type binding, original name, tag arg)
        struct Created<'b> {
            index: usize,
            names: Vec<RNode>,
            typ: BindingRef<'b>,
            suggested: Option<String>,
            tag_arg: Option<RNode>,
        }
        let mut created: Vec<Created<'_>> = Vec::new();
        for (i, change) in self.changes.iter().enumerate() {
            match change {
                ChangeDesc::Keep => {
                    if let Some(p) = parameters.get(k) {
                        used.push(p.child("name").map(|n| n.identifier()).unwrap_or_default());
                    }
                    k += 1;
                }
                ChangeDesc::Insert { typ, name } => {
                    let Some(t) = source.binding_by_key(typ) else { continue };
                    let param = rw.new_node(NodeKind::SingleVariableDeclaration);
                    let tnode = imports.add_import_type(t, rw, &context, TypeLocation::Parameter);
                    rw.put_child(param, "type", tnode);
                    let n = rw.new_simple_name("x");
                    rw.put_child(param, "name", n);
                    rw.put_simple(param, "varargs", "false");
                    rw.list_insert_at(RNode::Orig(decl.id), "parameters", param, i as i32);
                    let mut tag_arg = None;
                    if let Some(doc) = decl.child("javadoc") {
                        let tag = rw.new_node(NodeKind::TagElement);
                        rw.put_simple(tag, "tagName", "@param");
                        let arg = rw.new_simple_name("x");
                        let text = rw.new_node(NodeKind::TextElement);
                        rw.put_simple(text, "text", "");
                        rw.put_list(tag, "fragments", vec![arg, text]);
                        let previous: HashSet<String> = parameters.iter().take(k).map(|p| p.child("name").map(|n| n.identifier()).unwrap_or_default()).collect();
                        insert_tag(rw, doc, tag, "@param", &previous);
                        tag_arg = Some(arg);
                    }
                    created.push(Created { index: i, names: vec![n], typ: t, suggested: name.clone(), tag_arg });
                }
                ChangeDesc::Remove => {
                    if let Some(p) = parameters.get(k) {
                        rw.list_remove(RNode::Orig(decl.id), "parameters", RNode::Orig(p.id));
                        let pname = p.child("name").map(|n| n.identifier()).unwrap_or_default();
                        if let Some(tag) = find_param_tag(decl, &pname) {
                            rw.remove(RNode::Orig(tag.id));
                        }
                    }
                    k += 1;
                }
                ChangeDesc::Edit { typ, name } => {
                    let Some(p) = parameters.get(k).copied() else { continue };
                    let Some(mut t) = source.binding_by_key(typ) else { continue };
                    if k == parameters.len() - 1 && i == self.changes.len() - 1 && p.flag("varargs") && t.is_array() {
                        t = t.element_type().unwrap_or(t);
                    } else {
                        rw.set_simple(RNode::Orig(p.id), "varargs", Some("false"));
                    }
                    let tnode = imports.add_import_type(t, rw, &context, TypeLocation::Parameter);
                    if let Some(old) = p.child("type") {
                        rw.replace(RNode::Orig(old.id), Some(tnode));
                    }
                    for d in p.list("extraDimensions2") {
                        rw.list_remove(RNode::Orig(p.id), "extraDimensions2", RNode::Orig(d.id));
                    }
                    let mut new_names = Vec::new();
                    let pname = p.child("name").map(|n| n.identifier()).unwrap_or_default();
                    let pbinding = p.child("name").and_then(|n| n.binding()).map(|b| b.key().to_owned());
                    match pbinding {
                        Some(key) => {
                            for n in target.all_nodes().filter(|n| n.is(NodeKind::SimpleName) && n.binding().is_some_and(|b| b.key() == key)) {
                                let new = rw.new_simple_name("x");
                                rw.replace(RNode::Orig(n.id), Some(new));
                                new_names.push(new);
                            }
                        }
                        None => {
                            if let Some(n) = p.child("name") {
                                let new = rw.new_simple_name("x");
                                rw.replace(RNode::Orig(n.id), Some(new));
                                new_names.push(new);
                            }
                        }
                    }
                    k += 1;
                    let mut tag_arg = None;
                    if let Some(tag) = find_param_tag(decl, &pname) {
                        if let Some(first) = tag.list("fragments").first() {
                            let arg = rw.new_simple_name("x");
                            rw.replace(RNode::Orig(first.id), Some(arg));
                            tag_arg = Some(arg);
                        }
                    }
                    created.push(Created { index: i, names: new_names, typ: t, suggested: name.clone(), tag_arg });
                }
                ChangeDesc::Swap(index) => {
                    let (Some(d1), Some(d2)) = (parameters.get(k).copied(), parameters.get(*index).copied()) else { continue };
                    let c2 = rw.create_copy_target(d2.id);
                    rw.replace(RNode::Orig(d1.id), Some(c2));
                    let c1 = rw.create_copy_target(d1.id);
                    rw.replace(RNode::Orig(d2.id), Some(c1));
                    used.push(d1.child("name").map(|n| n.identifier()).unwrap_or_default());
                    k += 1;
                    let n1 = d1.child("name").map(|n| n.identifier()).unwrap_or_default();
                    let n2 = d2.child("name").map(|n| n.identifier()).unwrap_or_default();
                    if let (Some(t1), Some(t2)) = (find_param_tag(decl, &n1), find_param_tag(decl, &n2)) {
                        let c2 = rw.create_copy_target(t2.id);
                        rw.replace(RNode::Orig(t1.id), Some(c2));
                        let c1 = rw.create_copy_target(t1.id);
                        rw.replace(RNode::Orig(t2.id), Some(c1));
                    }
                }
            }
        }
        if created.is_empty() {
            return;
        }
        if let Some(body) = decl.child("body") {
            for n in body.descendants() {
                if n.kind().is_variable_declaration() {
                    if let Some(name) = n.child("name") {
                        used.push(name.identifier());
                    }
                }
            }
        }
        // fixupNames
        created.sort_by_key(|c| c.index);
        for c in created {
            let mut favourite = c.suggested.as_deref().map(|s| names::suggest_argument_name(s, &used, options));
            if favourite.is_none() {
                favourite = names::argument_name_suggestions(c.typ, &used, options).into_iter().next();
            }
            let favourite = favourite.unwrap_or_else(|| "x".to_owned());
            used.push(favourite.clone());
            for n in &c.names {
                rw.put_simple(*n, "identifier", &favourite);
            }
            if let Some(a) = c.tag_arg {
                rw.put_simple(a, "identifier", &favourite);
            }
        }
    }
}

/// `JavadocTagsSubProcessorCore.insertTag`.
pub(super) fn insert_tag(rw: &mut ASTRewrite, doc: Node<'_>, tag: RNode, tag_name: &str, leading: &HashSet<String>) {
    let tags = rw.list_rewritten(RNode::Orig(doc.id), "tags");
    let rank = tag_rank(tag_name);
    let mut after = None;
    for t in tags.iter().rev() {
        let RNode::Orig(id) = t else { continue };
        let curr = doc.ast.node(*id);
        let name = curr.simple("tagName");
        if name.is_none_or(|n| rank > tag_rank(n)) {
            after = Some(*t);
            break;
        }
        if name == Some(tag_name) && tag_argument(curr).is_some_and(|a| leading.contains(&a)) {
            after = Some(*t);
            break;
        }
    }
    match after {
        Some(a) => rw.list_insert_after(RNode::Orig(doc.id), "tags", tag, a),
        None => rw.list_insert_first(RNode::Orig(doc.id), "tags", tag),
    }
}

#[tower_lsp::async_trait]
impl LazyChange for ChangeSignature {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let target = target_ast(env, &self.source, &self.target_uri).await?;
        let options = env.options(&target.uri).await;
        let decl = target
            .binding_by_key(&self.method)
            .and_then(|b| b.declaring_node())
            .filter(|n| n.is(NodeKind::MethodDeclaration))
            .ok_or_else(|| anyhow::anyhow!("no method declaration"))?;
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        let mut rw = ASTRewrite::new(target.clone());
        self.modify(&mut rw, &mut imports, decl, &options, &target);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

// ─── NewAnnotationMemberProposalCore ────────────────────────────────────────

pub struct NewAnnotationMember {
    pub source: Arc<Ast>,
    pub invocation: NodeId,
    pub sender: String,
    pub target_uri: Option<String>,
}

#[tower_lsp::async_trait]
impl LazyChange for NewAnnotationMember {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let target = target_ast(env, &self.source, &self.target_uri).await?;
        let options = env.options(&target.uri).await;
        let decl = target
            .binding_by_key(&self.sender)
            .and_then(|b| b.declaring_node())
            .filter(|n| n.is(NodeKind::AnnotationTypeDeclaration))
            .ok_or_else(|| anyhow::anyhow!("no annotation declaration"))?;
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        let mut rw = ASTRewrite::new(target.clone());
        let invocation = self.source.node(self.invocation);
        let member = rw.new_node(NodeKind::AnnotationTypeMemberDeclaration);
        let mods = decl.list("bodyDeclarations").into_iter().find(|b| b.is(NodeKind::AnnotationTypeMemberDeclaration)).map(|b| b.modifiers()).unwrap_or(0);
        let modifiers = rw.new_modifiers(mods);
        rw.put_list(member, "modifiers", modifiers);
        let is_pair_name = invocation.location_is("name") && invocation.parent().is_some_and(|p| p.is(NodeKind::MemberValuePair));
        let name = if is_pair_name { invocation.identifier() } else { "value".to_owned() };
        let n = rw.new_simple_name(&name);
        rw.put_child(member, "name", n);
        let binding = if is_pair_name {
            invocation.parent().and_then(|p| p.child("value")).and_then(|v| v.type_binding())
        } else if invocation.kind().is_expression() {
            invocation.type_binding()
        } else {
            None
        };
        let context = ConstructorImportContext { ast: target.clone(), declaration: Some(decl.id), nullness: None };
        let typ = match binding {
            Some(b) => imports.add_import_type(b, &mut rw, &context, TypeLocation::ReturnType),
            None => {
                let s = rw.new_simple_name("String");
                rw.new_simple_type(s)
            }
        };
        rw.put_child(member, "type", typ);
        let index = decl.list("bodyDeclarations").len();
        rw.list_insert_at(RNode::Orig(decl.id), "bodyDeclarations", member, index as i32);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}
