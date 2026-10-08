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

/// `RefactorProcessor.getClassInstanceCreation`.
pub fn class_instance_creation(node: Node<'_>) -> Option<Node<'_>> {
    let mut node = node;
    loop {
        let climb = matches!(node.kind(), NodeKind::SimpleName | NodeKind::QualifiedName | NodeKind::ModuleQualifiedName | NodeKind::Dimension)
            || node.kind().is_type()
            || node.parent().is_some_and(|p| p.is(NodeKind::MethodDeclaration))
            || (node.location_is("bodyDeclarations") && node.parent().is_some_and(|p| p.is(NodeKind::AnonymousClassDeclaration)));
        if !climb {
            break;
        }
        node = node.parent()?;
    }
    if node.is(NodeKind::ClassInstanceCreation) {
        Some(node)
    } else if node.location_is("anonymousClassDeclaration") {
        node.parent()
    } else {
        None
    }
}

/// `RefactorProcessor.getConvertLambdaToAnonymousClassCreationsProposals`.
pub async fn convert_lambda_to_anonymous_proposal(env: &Env<'_>, ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    let lambda = if covering.is(NodeKind::LambdaExpression) {
        covering
    } else if covering.location_is("body") && covering.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
        covering.parent().expect("lambda")
    } else {
        return false;
    };
    if crate::refactoring::lambda_anonymous::lambda_functional_method(lambda).is_none() {
        return false;
    }
    let options = env.options(&ctx.ast.uri).await;
    let mut cu = CuRewrite::new(&ctx.ast, &options);
    crate::refactoring::lambda_anonymous::create_anonymous_classes(&mut cu, &ctx.ast, &options, vec![lambda]);
    let label = messages::fix("LambdaExpressionsFix_convert_to_anonymous_class_creation");
    out.push(Proposal::new(label, kind::REFACTOR, relevance::CONVERT_TO_ANONYMOUS_CLASS_CREATION, super::Change::Cu(vec![cu.into_change()])));
    true
}

/// `RefactorProcessor.getConvertAnonymousClassCreationsToLambdaProposals`.
pub async fn convert_anonymous_to_lambda_proposal(env: &Env<'_>, ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    let Some(cic) = class_instance_creation(covering) else { return false };
    let Some(removes_annotations) = crate::refactoring::lambda_fix::functional_anonymous(cic) else { return false };
    let options = env.options(&ctx.ast.uri).await;
    let mut cu = CuRewrite::new(&ctx.ast, &options);
    crate::refactoring::lambda_fix::create_lambdas(&mut cu, &ctx.ast, &options, vec![cic], true);
    let label = if removes_annotations {
        messages::fix("LambdaExpressionsFix_convert_to_lambda_expression_removes_annotations")
    } else {
        messages::fix("LambdaExpressionsFix_convert_to_lambda_expression")
    };
    out.push(Proposal::new(label, kind::REFACTOR, relevance::CONVERT_TO_LAMBDA_EXPRESSION, super::Change::Cu(vec![cu.into_change()])));
    true
}
