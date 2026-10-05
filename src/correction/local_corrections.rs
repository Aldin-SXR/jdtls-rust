//! Port of `LocalCorrectionsSubProcessor` / `LocalCorrectionsBaseSubProcessor`.

mod unreachable;
mod conversion;
pub use unreachable::proposals as unreachable_code;

use super::edit::Env;
use super::parentheses::needs_parentheses;
use super::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::text_edit::{EditKind, EditTree};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{normalized_node, subtree_match, unparenthesed_expression};
use crate::semantic_ast::NodeKind;

/// `ReplaceCorrectionProposalCore`.
pub fn replace_proposal(ctx: &Context, label: impl Into<String>, offset: usize, length: usize, text: &str, relevance: i32) -> Proposal {
    let mut tree = EditTree::new();
    let e = tree.new_edit(offset as i32, length as i32, EditKind::Replace(text.to_owned()));
    let _ = tree.add_child(EditTree::ROOT, e);
    Proposal::new(label, kind::QUICK_FIX, relevance, Change::Cu(vec![CuChange::edits(ctx.ast.clone(), tree)]))
}

/// `QuickFixProcessor.moveBack`.
fn move_back(ctx: &Context, mut offset: usize, start: usize, ignore: &str) -> usize {
    while offset >= start {
        let Some(c) = offset.checked_sub(1).and_then(|o| ctx.ast.char_at(o)) else { return start };
        if !ignore.encode_utf16().any(|i| i == c) {
            return offset;
        }
        if offset == 0 {
            break;
        }
        offset -= 1;
    }
    start
}

/// `QuickFixProcessor.process` case `UnterminatedString`.
pub fn add_quote(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let label = messages::ls_correction("JavaCorrectionProcessor_addquote_description");
    let pos = move_back(ctx, problem.offset + problem.length, problem.offset, "\n\r");
    proposals.push(replace_proposal(ctx, label, pos, 0, "\"", relevance::ADD_QUOTE));
}

/// `getRedundantSuperInterfaceProposal`.
pub fn redundant_super_interface(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    if !selected.kind().is_name() {
        return;
    }
    let node = normalized_node(selected);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.remove(RNode::Orig(node.id));
    let label = messages::correction("LocalCorrectionsSubProcessor_remove_redundant_superinterface");
    proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::REMOVE_REDUNDANT_SUPER_INTERFACE, rw));
}

/// `getSuperfluousSemicolonProposal`.
pub fn superfluous_semicolon(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let label = messages::correction("LocalCorrectionsSubProcessor_removesemicolon_description");
    proposals.push(replace_proposal(ctx, label, problem.offset, problem.length, "", relevance::REMOVE_SEMICOLON));
}

/// `getUnnecessaryCastProposal`: `UnusedCodeFixCore.createRemoveUnusedCastFix`
/// with `RemoveCastOperation`.
pub fn unnecessary_cast(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let cast = unparenthesed_expression(selected);
    if !cast.is(NodeKind::CastExpression) {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let Some(mut expression) = cast.child("expression") else { return };
    if expression.is(NodeKind::ParenthesizedExpression) {
        if let Some(child) = expression.child("expression") {
            if needs_parentheses(child, cast, "expression") {
                expression = child;
            }
        }
    }
    let mut exp = cast;
    while let Some(p) = exp.parent().filter(|p| p.is(NodeKind::ParenthesizedExpression)) {
        exp = p;
    }
    let mut removed = false;
    if exp.location_is("rightHandSide") {
        if let Some(assignment) = exp.parent() {
            if assignment.location_is("expression") && assignment.parent().is_some_and(|s| s.is(NodeKind::ExpressionStatement)) {
                let stmt = assignment.parent().unwrap();
                if let Some(lhs) = assignment.child("leftHandSide") {
                    if subtree_match(lhs, expression) {
                        rw.remove(RNode::Orig(stmt.id));
                        removed = true;
                    }
                }
            }
        }
    }
    if !removed {
        replace_cast(&mut rw, cast, expression);
    }
    proposals.push(Proposal::new(
        messages::fix("UnusedCodeFix_RemoveCast_description"),
        kind::QUICK_FIX,
        10,
        Change::Cu(vec![CuChange::rewrite(rw)]),
    ));
}

/// `UnusedCodeFixCore.replaceCast`.
fn replace_cast(rw: &mut ASTRewrite, cast: crate::semantic_ast::Node<'_>, replacement: crate::semantic_ast::Node<'_>) {
    let parent = cast.parent();
    let enclosed = parent.is_some_and(|p| {
        p.is(NodeKind::ParenthesizedExpression)
            && match (p.parent(), p.location()) {
                (Some(pp), Some(loc)) => needs_parentheses(cast, pp, loc),
                _ => false,
            }
    });
    let mut to_replace = if enclosed { parent.unwrap() } else { cast };
    let needs = match (to_replace.parent(), to_replace.location()) {
        (Some(pp), Some(loc)) => needs_parentheses(replacement, pp, loc),
        _ => false,
    };
    let mv = if needs {
        if let Some(rp) = replacement.parent().filter(|p| p.is(NodeKind::ParenthesizedExpression)) {
            rw.create_move_target(rp.id)
        } else if enclosed {
            to_replace = cast;
            rw.create_move_target(replacement.id)
        } else {
            let target = rw.create_move_target(replacement.id);
            rw.new_parenthesized_expression(target)
        }
    } else {
        rw.create_move_target(replacement.id)
    };
    rw.replace(RNode::Orig(to_replace.id), Some(mv));
}

/// `correctAccessToStatic` (`addCorrectAccessToStaticProposals`).
pub async fn correct_access_to_static(_env: &Env<'_>, _ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
