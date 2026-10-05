//! LocalCorrectionsBaseSubProcessor.getUnusedObjectAllocationProposalsBase.
use crate::{
    correction::{
        edit::Env, kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal,
    },
    rewrite::{ASTRewrite, RNode},
    semantic_ast::{BindingRef, NodeKind},
};

pub async fn proposals(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    out: &mut Vec<Proposal>,
) {
    let Some(selected) = problem.covering_node(ctx.ast()) else {
        return;
    };
    if let Some(statement) = selected
        .parent()
        .filter(|n| n.is(NodeKind::ExpressionStatement))
    {
        let Some(expression) = statement.child("expression") else {
            return;
        };
        let typ = expression.type_binding();
        if typ.is_some_and(throwable) {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            rw.add_tight_source_node(statement.id);
            let replacement = rw.new_node(NodeKind::ThrowStatement);
            let copied = rw.create_move_target(expression.id);
            rw.put_child(replacement, "expression", copied);
            rw.replace(RNode::Orig(statement.id), Some(replacement));
            out.push(Proposal::rewrite(
                messages::correction("LocalCorrectionsSubProcessor_throw_allocated_description"),
                kind::QUICK_FIX,
                relevance::THROW_ALLOCATED_OBJECT,
                rw,
            ));
        }
        if let Some(method) = std::iter::once(selected)
            .chain(selected.ancestors())
            .find(|n| {
                n.kind().is_body_declaration()
                    || n.is(NodeKind::AnonymousClassDeclaration)
                    || n.is(NodeKind::LambdaExpression)
            })
            .filter(|n| n.is(NodeKind::MethodDeclaration) && !n.flag("constructor"))
        {
            let target = method.child("returnType2").and_then(|t| t.binding());
            let rank = if target.zip(typ).is_some_and(|(target, typ)| {
                typ == target || typ.data().assignment_targets.contains(&target.id)
            }) {
                relevance::RETURN_ALLOCATED_OBJECT_MATCH
            } else if target.is_some_and(|t| t.name() == "void") {
                relevance::RETURN_ALLOCATED_OBJECT_VOID
            } else {
                relevance::RETURN_ALLOCATED_OBJECT
            };
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            rw.add_tight_source_node(statement.id);
            let copied = rw.create_move_target(expression.id);
            let replacement = rw.new_return_statement(Some(copied));
            rw.replace(RNode::Orig(statement.id), Some(replacement));
            out.push(Proposal::rewrite(
                messages::correction("LocalCorrectionsSubProcessor_return_allocated_description"),
                kind::QUICK_FIX,
                rank,
                rw,
            ));
        }
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        rw.remove(RNode::Orig(statement.id));
        out.push(Proposal::new(
            messages::correction("LocalCorrectionsSubProcessor_remove_allocated_description"),
            kind::QUICK_FIX,
            relevance::REMOVE_UNUSED_ALLOCATED_OBJECT,
            Change::Cu(vec![CuChange::rewrite(rw)]),
        ));
    }
    super::assignment::proposals(env, ctx, selected, out).await;
}

fn throwable(mut typ: BindingRef<'_>) -> bool {
    let mut seen = std::collections::HashSet::new();
    while seen.insert(typ.key().to_owned()) {
        if typ.qualified_name() == "java.lang.Throwable" {
            return true;
        }
        let Some(parent) = typ.superclass() else {
            break;
        };
        typ = parent;
    }
    false
}
