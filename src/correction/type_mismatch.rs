//! Port of jdt.ls `TypeMismatchSubProcessor` / `TypeMismatchBaseSubProcessor`
//! (jdt.core.manipulation): type mismatch, incompatible return type,
//! incompatible throws clause and enhanced for type proposals.

pub mod bindings;
pub mod proposals;

use std::sync::Arc;

use self::bindings::{
    box_or_unbox, find_method_in_hierarchy, find_overridden_method, find_parent_method_declaration, guess_binding_for_reference,
    is_cast_compatible, is_super_type, is_useable_type_in_context, is_void, normalize_for_declaration_use, normalize_type_binding,
    normalize_wildcard_type, resolve_expression_binding, type_label, well_known, Ty,
};
use self::proposals::{
    cast_proposal, change_exceptions_proposal, compilation_unit_for, constructor_type_proposal, implement_interface_proposal,
    loop_variable_proposal, type_change_proposal, ExceptionChange,
};
use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::find_parent_body_declaration;
use crate::semantic_ast::{Ast, BindingKind, BindingRef, Node, NodeKind};

fn correction(key: &str, args: &[&str]) -> String {
    messages::format(messages::correction(key), args)
}

/// `TypeMismatchBaseSubProcessor.collectTypeMismatchProposals` (jdt.ls pads
/// the problem arguments to two, so the argument check always passes).
pub async fn type_mismatch(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast.clone();
    let Some(selected) = problem.covered_node(&ast) else { return };
    if !selected.kind().is_expression() {
        return;
    }
    let Some(parent) = selected.parent() else { return };
    let mut node_to_cast = Some(selected);
    let mut receiver: Option<Node<'_>> = None;
    let cast_type: Option<BindingRef<'_>>;
    match parent.kind() {
        NodeKind::Assignment => {
            let lhs = parent.child("leftHandSide");
            if lhs == Some(selected) {
                node_to_cast = parent.child("rightHandSide");
            }
            cast_type = lhs.and_then(|l| l.type_binding());
            receiver = lhs.and_then(|l| {
                if l.kind().is_name() {
                    Some(l)
                } else if l.is(NodeKind::FieldAccess) {
                    l.child("name")
                } else {
                    None
                }
            });
        }
        NodeKind::VariableDeclarationFragment => {
            if parent.child("name") == Some(selected) || parent.child("initializer") == Some(selected) {
                node_to_cast = parent.child("initializer");
                cast_type = parent.parent().and_then(|d| d.child("type")).and_then(|t| t.binding());
                receiver = parent.child("name");
            } else {
                cast_type = None;
            }
        }
        NodeKind::MemberValuePair => {
            receiver = parent.child("name");
            cast_type = guess_binding_for_reference(selected);
        }
        NodeKind::SingleMemberAnnotation => {
            receiver = parent.child("typeName");
            cast_type = guess_binding_for_reference(selected);
        }
        _ => cast_type = guess_binding_for_reference(selected),
    }
    let (Some(cast_type), Some(node_to_cast)) = (cast_type, node_to_cast) else { return };

    let mut curr = node_to_cast.type_binding();
    if curr.is_none() && node_to_cast.is(NodeKind::MethodInvocation) {
        curr = node_to_cast.method_binding().and_then(|m| m.return_type());
    }

    if !node_to_cast.is(NodeKind::ArrayInitializer) {
        let cast_fix = match curr {
            None => Some(Ty::Binding(cast_type)),
            Some(c) if is_cast_compatible(Ty::Binding(cast_type), c) || node_to_cast.is(NodeKind::CastExpression) => Some(Ty::Binding(cast_type)),
            Some(c) => match box_or_unbox(cast_type, c) {
                Ty::Binding(b) if b == cast_type => None,
                other => is_cast_compatible(other, c).then_some(other),
            },
        };
        if let Some(cast_fix) = cast_fix {
            proposals.push(create_cast_proposal(env, &ast, cast_fix, node_to_cast, relevance::CREATE_CAST).await);
        }
    }

    let null_or_void = curr.is_none_or(is_void);

    // Change the method return type to the actual type.
    if !null_or_void && is_type_returned(node_to_cast) {
        if let Some(decl) = find_parent_body_declaration(selected).filter(|d| d.is(NodeKind::MethodDeclaration)) {
            let mut c = normalize_type_binding(curr).or_else(|| well_known(&ast, "java.lang.Object"));
            if let Some(w) = c.filter(|c| c.is_wildcard_type()) {
                c = normalize_wildcard_type(w, true);
            }
            if let Some(c) = c {
                proposals.push(change_return_type_proposal(env, &ast, c, decl).await);
            }
        }
    }

    let mut curr = curr;
    if !null_or_void {
        if let Some(receiver) = receiver {
            let mut c = normalize_type_binding(curr).or_else(|| well_known(&ast, "java.lang.Object"));
            if let Some(w) = c.filter(|c| c.is_wildcard_type()) {
                c = normalize_wildcard_type(w, true);
            }
            curr = c;
            if let Some(c) = c {
                change_sender_type_proposals(env, ctx, receiver, c, true, relevance::CHANGE_TYPE_OF_RECEIVER_NODE, proposals).await;
            }
        }
    }

    change_sender_type_proposals(env, ctx, node_to_cast, cast_type, false, relevance::CHANGE_TYPE_OF_NODE_TO_CAST, proposals).await;

    if cast_type.is_primitive() && cast_type.name() == "boolean" && curr.is_some_and(|c| !c.is_primitive() && !is_void(c)) {
        let label = messages::correction("TypeMismatchSubProcessor_insertnullcheck_description").to_owned();
        let mut rw = ASTRewrite::new(ast.clone());
        let left = rw.create_move_target(node_to_cast.id);
        let right = rw.new_node(NodeKind::NullLiteral);
        let infix = rw.new_infix_expression(left, "!=", right);
        rw.replace(RNode::Orig(node_to_cast.id), Some(infix));
        proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::INSERT_NULL_CHECK, rw));
    }
}

/// `TypeMismatchBaseSubProcessor.isTypeReturned`.
fn is_type_returned(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        NodeKind::ReturnStatement => true,
        NodeKind::ParenthesizedExpression => is_type_returned(parent),
        NodeKind::ConditionalExpression if node.location_is("thenExpression") || node.location_is("elseExpression") => is_type_returned(parent),
        _ => false,
    }
}

/// `collectCastProposals(context, castTypeBinding, nodeToCast, relevance)`.
pub async fn create_cast_proposal(env: &Env<'_>, ast: &Arc<Ast>, cast_type: Ty<'_>, node_to_cast: Node<'_>, relevance: i32) -> Proposal {
    let label_type = match cast_type {
        Ty::Binding(b) => type_label(b),
        Ty::Named(n) => n.rsplit('.').next().unwrap_or(n).to_owned(),
    };
    let key = if node_to_cast.is(NodeKind::CastExpression) {
        "TypeMismatchSubProcessor_changecast_description"
    } else {
        "TypeMismatchSubProcessor_addcast_description"
    };
    cast_proposal(env, ast, correction(key, &[&label_type]), node_to_cast, cast_type, relevance).await
}

/// `TypeMismatchSubProcessor.createChangeReturnTypeProposal`.
async fn change_return_type_proposal(env: &Env<'_>, ast: &Arc<Ast>, curr: BindingRef<'_>, decl: Node<'_>) -> Proposal {
    let label = correction("TypeMismatchSubProcessor_changereturntype_description", &[curr.name()]);
    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = proposals::import_context(ast, decl, &options);
    let typ = imports.add_import_type(curr, &mut rw, &context, TypeLocation::ReturnType);
    if let Some(ret) = decl.child("returnType2") {
        rw.replace(RNode::Orig(ret.id), Some(typ));
    }
    Proposal::new(label, kind::QUICK_FIX, relevance::CHANGE_METHOD_RETURN_TYPE, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]))
}

/// `TypeMismatchBaseSubProcessor.collectChangeSenderTypeProposals`.
pub async fn change_sender_type_proposals(
    env: &Env<'_>,
    ctx: &Context,
    node_to_cast: Node<'_>,
    cast_type: BindingRef<'_>,
    is_assigned_node: bool,
    relevance: i32,
    proposals: &mut Vec<Proposal>,
) {
    let ast = &ctx.ast;
    let caller = resolve_expression_binding(node_to_cast, false);
    let mut target: Option<Arc<Ast>> = None;
    let mut declaring_type: Option<BindingRef<'_>> = None;
    let mut caller_decl = caller;
    match caller {
        Some(var) if var.kind() == BindingKind::Variable => {
            if var.is_enum_constant() {
                return;
            }
            if !var.is_field() {
                target = Some(ast.clone());
            } else {
                caller_decl = var.variable_declaration().or(Some(var));
                let Some(declaring_class) = var.declaring_class() else { return };
                declaring_type = Some(declaring_class.type_declaration().unwrap_or(declaring_class));
            }
        }
        Some(method) if method.kind() == BindingKind::Method => {
            if !method.is_constructor() {
                declaring_type = method.declaring_class().map(|c| c.type_declaration().unwrap_or(c));
                caller_decl = method.method_declaration().or(Some(method));
            }
        }
        Some(typ) if typ.kind() == BindingKind::Type && node_to_cast.location_is("typeName") && node_to_cast.parent().is_some_and(|p| p.is(NodeKind::SingleMemberAnnotation)) => {
            declaring_type = Some(typ);
            caller_decl = bindings::find_method_in_type(typ, "value");
            if caller_decl.is_none() {
                return;
            }
        }
        _ => {}
    }
    if let Some(declaring_type) = declaring_type.filter(|t| t.is_from_source()) {
        target = compilation_unit_for(env, ast, declaring_type).await;
    }
    if let (Some(target), Some(caller_decl)) = (&target, caller_decl) {
        if is_useable_type_in_context(cast_type, Some(caller_decl), false) {
            if let Some(p) = type_change_proposal(env, target, caller_decl, cast_type, is_assigned_node, relevance, None).await {
                proposals.push(p);
            }
        }
    }

    if !is_assigned_node {
        if let Some(node_type) = node_to_cast.type_binding() {
            if cast_type.is_interface() && node_type.is_class() && !node_type.is_anonymous() && node_type.is_from_source() {
                let type_decl = node_type.type_declaration().unwrap_or(node_type);
                if let Some(node_cu) = compilation_unit_for(env, ast, type_decl).await {
                    if is_useable_type_in_context(cast_type, Some(type_decl), true) {
                        if let Some(p) = implement_interface_proposal(env, &node_cu, type_decl, cast_type, relevance - 1).await {
                            proposals.push(p);
                        }
                    }
                }
            }
        }
        if node_to_cast.is(NodeKind::ClassInstanceCreation) {
            let covering = ctx.covering_node();
            let constructor = covering.and_then(|c| c.ancestor_or_self(|k| k == NodeKind::ClassInstanceCreation));
            if let Some(constructor) = constructor {
                if let Some(p) = constructor_type_proposal(env, ast, constructor, cast_type, relevance).await {
                    proposals.push(p);
                }
            }
        }
    }
}

/// `TypeMismatchBaseSubProcessor.collectIncompatibleReturnTypeProposals`.
pub async fn incompatible_return_type(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = &ctx.ast;
    let Some(selected) = problem.covering_node(ast) else { return };
    let Some(decl) = find_parent_method_declaration(selected) else { return };
    let Some(method) = decl.binding() else { return };
    let Some(return_type) = method.return_type() else { return };
    let Some(overridden) = find_overridden_method(method) else { return };
    let Some(overridden_return_type) = overridden.return_type() else { return };
    if overridden_return_type == return_type {
        return;
    }
    let method_decl = method.method_declaration().unwrap_or(method);
    let bounds = overridden_return_type.type_bounds();
    if decl.list("typeParameters").is_empty() || bounds.is_empty() || bounds.iter().all(|b| b.type_arguments().is_empty()) {
        let erasure = overridden_return_type.erasure().unwrap_or(overridden_return_type);
        if let Some(p) = type_change_proposal(env, ast, method_decl, erasure, false, relevance::CHANGE_RETURN_TYPE, None).await {
            proposals.push(p);
        }
    }
    if overridden_return_type.is_type_variable() {
        if let Some(p) = type_change_proposal(env, ast, method_decl, overridden_return_type, false, relevance::CHANGE_RETURN_TYPE, None).await {
            proposals.push(p);
        }
    }
    let overridden_decl = overridden.method_declaration().unwrap_or(overridden);
    let Some(overridden_decl_type) = overridden_decl.declaring_class() else { return };
    if overridden_decl_type.is_from_source() {
        if let Some(target) = compilation_unit_for(env, ast, overridden_decl_type).await {
            if is_useable_type_in_context(return_type, Some(overridden_decl), false) {
                let key = if overridden_decl_type.is_interface() {
                    "TypeMismatchSubProcessor_changereturnofimplemented_description"
                } else {
                    "TypeMismatchSubProcessor_changereturnofoverridden_description"
                };
                let name = correction(key, &[overridden_decl.name()]);
                if let Some(p) = type_change_proposal(env, &target, overridden_decl, return_type, false, relevance::CHANGE_RETURN_TYPE_OF_OVERRIDDEN, Some(name)).await {
                    proposals.push(p);
                }
            }
        }
    }
}

/// `TypeMismatchBaseSubProcessor.isDeclaredException`.
fn is_declared_exception(curr: BindingRef<'_>, declared: &[BindingRef<'_>]) -> bool {
    declared.iter().any(|d| is_super_type(*d, curr, true))
}

/// `TypeMismatchBaseSubProcessor.collectIncompatibleThrowsProposals`.
pub async fn incompatible_throws(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = &ctx.ast;
    let Some(decl) = problem.covering_node(ast).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    let Some(method) = decl.binding() else { return };
    let Some(overridden) = find_overridden_method(method) else { return };
    let method_exceptions = method.exception_types();
    let defined = overridden.exception_types();
    let mut undeclared = Vec::new();
    let mut changes = Vec::new();
    for e in &method_exceptions {
        if !is_declared_exception(*e, &defined) {
            changes.push(ExceptionChange::Remove);
            undeclared.push(*e);
        } else {
            changes.push(ExceptionChange::Keep);
        }
    }
    if undeclared.is_empty() {
        return;
    }
    let label = correction("TypeMismatchSubProcessor_removeexceptions_description", &[method.name()]);
    if let Some(p) = change_exceptions_proposal(env, ast, label, method, &changes, relevance::REMOVE_EXCEPTIONS).await {
        proposals.push(p);
    }

    let Some(declaring_type) = overridden.declaring_class() else { return };
    if !declaring_type.is_from_source() {
        return;
    }
    let Some(target) = compilation_unit_for(env, ast, declaring_type).await else { return };
    let mut changes: Vec<ExceptionChange<'_>> = defined.iter().map(|_| ExceptionChange::Keep).collect();
    changes.extend(undeclared.iter().map(|e| ExceptionChange::Insert(*e)));
    let overridden_decl = overridden.method_declaration().unwrap_or(overridden);
    let label = correction("TypeMismatchSubProcessor_addexceptions_description", &[declaring_type.name(), overridden.name()]);
    if let Some(p) = change_exceptions_proposal(env, &target, label, overridden_decl, &changes, relevance::ADD_EXCEPTIONS).await {
        proposals.push(p);
    }
}

/// `TypeMismatchBaseSubProcessor.collectTypeMismatchInForEachProposals`.
pub async fn type_mismatch_in_for_each(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = &ctx.ast;
    let Some(selected) = problem.covering_node(ast) else { return };
    if !selected.location_is("expression") {
        return;
    }
    let Some(statement) = selected.parent().filter(|p| p.is(NodeKind::EnhancedForStatement)) else { return };
    let Some(expression_binding) = statement.child("expression").and_then(|e| e.type_binding()) else { return };
    let expected = if expression_binding.is_array() {
        expression_binding.component_type()
    } else {
        let iterator = find_method_in_hierarchy(expression_binding, "iterator");
        match iterator {
            Some(m) => {
                let args = m.return_type().map(|r| r.type_arguments()).unwrap_or_default();
                if args.len() != 1 {
                    return;
                }
                Some(args[0])
            }
            // The graph has no members of this library type: its `Iterable`
            // supertype carries the same element type.
            None => {
                let Some(iterable) = bindings::find_type_in_hierarchy(expression_binding, "java.lang.Iterable") else { return };
                let args = iterable.type_arguments();
                if args.len() != 1 {
                    return;
                }
                Some(args[0])
            }
        }
    };
    let Some(expected) = expected.and_then(normalize_for_declaration_use) else { return };
    let Some(parameter) = statement.child("parameter") else { return };
    let Some(name) = parameter.child("name") else { return };

    if name.length() == 0 {
        let simple_name = parameter.child("type").and_then(|t| match t.kind() {
            NodeKind::SimpleType => t.child("name").filter(|n| n.is(NodeKind::SimpleName)),
            NodeKind::NameQualifiedType => t.child("name"),
            _ => None,
        });
        if let Some(simple_name) = simple_name {
            let identifier = simple_name.identifier();
            let options = env.options(&ast.uri).await;
            let has_local_name = |key: &str, suffix: bool| {
                options.get(key).is_some_and(|v| v.split(',').filter(|s| !s.is_empty()).any(|s| if suffix { identifier.ends_with(s) } else { identifier.starts_with(s) }))
            };
            let rank = if has_local_name("org.eclipse.jdt.core.codeComplete.localPrefixes", false) || has_local_name("org.eclipse.jdt.core.codeComplete.localSuffixes", true) { 10 } else { 7 };
            let label = correction("TypeMismatchSubProcessor_create_loop_variable_description", &[&identifier]);
            if let Some(p) = loop_variable_proposal(env, ast, label, simple_name, rank).await {
                proposals.push(p);
            }
            return;
        }
    }

    let label = correction("TypeMismatchSubProcessor_incompatible_for_each_type_description", &[&name.identifier(), &type_label(expected)]);
    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let scope = find_parent_body_declaration(selected).unwrap_or(selected);
    let context = proposals::import_context(ast, scope, &options);
    let typ = imports.add_import_type(expected, &mut rw, &context, TypeLocation::LocalVariable);
    if let Some(t) = parameter.child("type") {
        rw.replace(RNode::Orig(t.id), Some(typ));
    }
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::INCOMPATIBLE_FOREACH_TYPE, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}
