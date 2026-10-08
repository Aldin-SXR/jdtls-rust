//! Port of jdt.ls `SuppressWarningsSubProcessor` over
//! `SuppressWarningsBaseSubProcessor` and `SuppressWarningsFixCore`.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::irritants::warning_token;
use crate::semantic_ast::{Ast, Node, NodeId, NodeKind};

const SUPPRESS_OPTIONAL_ERRORS: &str = "org.eclipse.jdt.core.compiler.problem.suppressOptionalErrors";
const SUPPRESS_WARNINGS: &str = "org.eclipse.jdt.core.compiler.problem.suppressWarnings";

/// `SuppressWarningsFixCore.getChildListPropertyDescriptor`: whether the node
/// carries a modifier list that can hold `@SuppressWarnings`.
fn has_modifier_list(node: Node<'_>, token: &str) -> bool {
    match node.kind() {
        NodeKind::SingleVariableDeclaration => !(node.parent().is_some_and(|p| p.is(NodeKind::PatternInstanceofExpression)) && token == "preview"),
        NodeKind::VariableDeclarationStatement
        | NodeKind::VariableDeclarationExpression
        | NodeKind::TypeDeclaration
        | NodeKind::RecordDeclaration
        | NodeKind::AnnotationTypeDeclaration
        | NodeKind::EnumDeclaration
        | NodeKind::FieldDeclaration
        | NodeKind::MethodDeclaration
        | NodeKind::AnnotationTypeMemberDeclaration
        | NodeKind::EnumConstantDeclaration => true,
        _ => false,
    }
}

fn first_fragment_name(node: Node<'_>) -> String {
    node.list("fragments").first().and_then(|f| f.child("name")).map(|n| n.identifier()).unwrap_or_default()
}

/// `getSuppressWarningsProposals`.
pub async fn suppress_warnings_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let options = env.options(&ctx.ast.uri).await;
    if problem.is_error && options.get(SUPPRESS_OPTIONAL_ERRORS).map(String::as_str) != Some("enabled") {
        return;
    }
    if options.get(SUPPRESS_WARNINGS).map(String::as_str) == Some("disabled") {
        return;
    }
    let Some(token) = warning_token(problem.problem_id) else { return };
    let ast = ctx.ast();
    let Some(node) = problem.covering_node(ast) else { return };

    let mut target = Some(node);
    let mut relevance = relevance::ADD_SUPPRESSWARNINGS;
    while let Some(current) = target {
        relevance = add_proposal_if_possible(ctx, current, token, relevance, proposals);
        if relevance == 0 {
            break;
        }
        target = current.parent();
    }
    if target.is_none() {
        let in_import = node.ancestors().any(|a| a.is(NodeKind::ImportDeclaration));
        if in_import {
            if let Some(first) = ast.root().list("types").first() {
                target = Some(*first);
                add_proposal_if_possible(ctx, *first, token, relevance::ADD_SUPPRESSWARNINGS, proposals);
            }
        }
    }
    if target.is_some() {
        add_all_proposal_if_possible(ctx, token, relevance::ADD_SUPPRESSWARNINGS - 1, proposals);
    }
}

/// `addSuppressWarningsProposalIfPossible`.
fn add_proposal_if_possible(ctx: &Context, node: Node<'_>, token: &'static str, relevance: i32, proposals: &mut Vec<Proposal>) -> i32 {
    let (name, is_local) = match node.kind() {
        NodeKind::SingleVariableDeclaration => {
            if !has_modifier_list(node, token) {
                return relevance;
            }
            (node.child("name").map(|n| n.identifier()).unwrap_or_default(), true)
        }
        NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression => (first_fragment_name(node), true),
        NodeKind::TypeDeclaration | NodeKind::RecordDeclaration | NodeKind::AnnotationTypeDeclaration | NodeKind::EnumDeclaration | NodeKind::EnumConstantDeclaration => {
            (node.child("name").map(|n| n.identifier()).unwrap_or_default(), false)
        }
        NodeKind::FieldDeclaration => (first_fragment_name(node), false),
        NodeKind::MethodDeclaration | NodeKind::AnnotationTypeMemberDeclaration => (format!("{}()", node.child("name").map(|n| n.identifier()).unwrap_or_default()), false),
        _ => return relevance,
    };
    let label = messages::format(messages::correction("SuppressWarningsSubProcessor_suppress_warnings_label"), &[token, &name]);
    let change = AddNeededSuppressWarnings { ast: ctx.ast.clone(), nodes: vec![node.id], token };
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::ADD_SUPPRESSWARNINGS - 1, Change::Lazy(Box::new(change))));
    if is_local {
        relevance - 1
    } else {
        0
    }
}

/// `addAllSuppressWarningsProposalIfPossible`.
fn add_all_proposal_if_possible(ctx: &Context, token: &'static str, relevance: i32, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let mut targets: BTreeSet<NodeId> = BTreeSet::new();
    for problem in &ast.problems {
        if warning_token(problem.id) != Some(token) {
            continue;
        }
        let length = (problem.source_end - problem.source_start + 1).max(0) as usize;
        let Some(covering) = NodeFinder::new(ast.root(), problem.source_start.max(0) as usize, length).covering else { continue };
        let mut target = Some(covering);
        while let Some(t) = target {
            if has_modifier_list(t, token) {
                targets.insert(t.id);
                break;
            }
            target = t.parent();
        }
    }
    if targets.len() > 1 {
        let label = messages::format(messages::correction("SuppressWarningsSubProcessor_suppress_all_warnings_label"), &[token]);
        let change = AddNeededSuppressWarnings { ast: ctx.ast.clone(), nodes: targets.into_iter().collect(), token };
        proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance - 1, Change::Lazy(Box::new(change))));
    }
}

/// `AddNeededSuppressWarningsOperation`.
struct AddNeededSuppressWarnings {
    ast: Arc<Ast>,
    nodes: Vec<NodeId>,
    token: &'static str,
}

/// `findExistingAnnotation`.
fn find_existing_annotation<'a>(ast: &'a Ast, modifiers: Vec<Node<'a>>) -> Option<Node<'a>> {
    modifiers.into_iter().find(|m| {
        if !matches!(m.kind(), NodeKind::NormalAnnotation | NodeKind::SingleMemberAnnotation) {
            return false;
        }
        match ast.data(m.id).annotation.as_ref() {
            Some(annotation) => ast.binding(annotation.annotation_type).qualified_name() == "java.lang.SuppressWarnings",
            None => matches!(m.child("typeName").map(|n| n.identifier()).as_deref(), Some("SuppressWarnings" | "java.lang.SuppressWarnings")),
        }
    })
}

fn new_string_literal(rw: &mut ASTRewrite, token: &str) -> RNode {
    let literal = rw.new_node(NodeKind::StringLiteral);
    rw.put_simple(literal, "escapedValue", &format!("\"{token}\""))
}

/// `addSuppressArgument`.
fn add_suppress_argument(rw: &mut ASTRewrite, value: Option<Node<'_>>, token: &str) -> bool {
    let Some(value) = value else { return false };
    match value.kind() {
        NodeKind::ArrayInitializer => {
            let literal = new_string_literal(rw, token);
            rw.list_insert_last(RNode::Orig(value.id), "expressions", literal);
        }
        NodeKind::StringLiteral => {
            let literal = new_string_literal(rw, token);
            let moved = rw.create_move_target(value.id);
            let array = rw.new_node(NodeKind::ArrayInitializer);
            rw.put_list(array, "expressions", vec![moved, literal]);
            rw.replace(RNode::Orig(value.id), Some(array));
        }
        _ => return false,
    }
    true
}

#[tower_lsp::async_trait]
impl LazyChange for AddNeededSuppressWarnings {
    async fn compute(&self, _env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let mut rw = ASTRewrite::new(self.ast.clone());
        for id in &self.nodes {
            let node = self.ast.node(*id);
            let existing = find_existing_annotation(&self.ast, node.list("modifiers"));
            match existing {
                None => {
                    let literal = new_string_literal(&mut rw, self.token);
                    let name = rw.new_name("SuppressWarnings");
                    let annotation = rw.new_node(NodeKind::SingleMemberAnnotation);
                    rw.put_child(annotation, "typeName", name);
                    rw.put_child(annotation, "value", literal);
                    rw.list_insert_first(RNode::Orig(node.id), "modifiers", annotation);
                }
                Some(existing) if existing.is(NodeKind::SingleMemberAnnotation) => {
                    if !add_suppress_argument(&mut rw, existing.child("value"), self.token) {
                        let literal = new_string_literal(&mut rw, self.token);
                        rw.set(RNode::Orig(existing.id), "value", Some(literal));
                    }
                }
                Some(existing) => {
                    let value = existing.list("values").into_iter().find(|p| p.child("name").is_some_and(|n| n.identifier() == "value")).and_then(|p| p.child("value"));
                    if !add_suppress_argument(&mut rw, value, self.token) {
                        let literal = new_string_literal(&mut rw, self.token);
                        let name = rw.new_simple_name("value");
                        let pair = rw.new_node(NodeKind::MemberValuePair);
                        rw.put_child(pair, "name", name);
                        rw.put_child(pair, "value", literal);
                        rw.list_insert_first(RNode::Orig(existing.id), "values", pair);
                    }
                }
            }
        }
        Ok(vec![CuChange::rewrite(rw)])
    }
}
