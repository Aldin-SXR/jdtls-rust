//! `RefactorProcessor` conversions: `for` loop to enhanced `for`, anonymous
//! class creation to lambda and back, anonymous class to nested class.

use super::edit::Env;
use super::{kind, messages, relevance, Context, CuChange, Proposal};
use crate::refactoring::convert_for_loop::ConvertForLoop;
use crate::refactoring::extract_temp::CuRewrite;
use crate::semantic_ast::{Node, NodeKind};

/// `getEnclosingHeader(node, ForStatement.class, initializers, expression, updaters)`.
fn enclosing_for_header(node: Node<'_>) -> Option<Node<'_>> {
    if node.is(NodeKind::ForStatement) {
        return Some(node);
    }
    let mut current = Some(node);
    while let Some(n) = current {
        let parent = n.parent();
        if let Some(p) = parent.filter(|p| p.is(NodeKind::ForStatement)) {
            return matches!(n.location(), Some("initializers" | "expression" | "updaters")).then_some(p);
        }
        current = parent;
    }
    None
}

/// `RefactorProcessor.getConvertForLoopProposal`.
pub async fn convert_for_loop_proposal(env: &Env<'_>, ctx: &Context, node: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    let Some(statement) = enclosing_for_header(node) else { return false };
    let options = env.options(&ctx.ast.uri).await;
    let mut operation = ConvertForLoop::new(ctx.ast.clone(), options.clone(), statement);
    if !operation.satisfies_preconditions() {
        return false;
    }
    let mut cu = CuRewrite::new(&ctx.ast, &options);
    operation.rewrite(&mut cu);
    let label = messages::fix("Java50Fix_ConvertToEnhancedForLoop_description");
    out.push(Proposal::new(label, kind::REFACTOR, relevance::CONVERT_FOR_LOOP_TO_ENHANCED, super::Change::Cu(vec![CuChange::rewrite(cu.rewrite).with_imports(cu.imports)])));
    true
}
