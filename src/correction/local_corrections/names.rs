//! `LocalCorrectionsBaseSubProcessor.getInvalidVariableNameProposals`
//! (`LinkedNamesAssistProposalCore`).

use crate::correction::linked_nodes::find_by_node;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::text_edit::{EditKind, EditTree};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{problem as p, NodeKind};

pub fn invalid_variable_names(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(mut selected) = problem.covering_node(ctx.ast()) else { return };
    if selected.is(NodeKind::MethodDeclaration) {
        if selected.flag("constructor") {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            rw.remove(RNode::Orig(selected.id));
            proposals.push(Proposal::rewrite(messages::correction("LocalCorrectionsSubProcessor_removeunreachablecode_description"), kind::QUICK_FIX, 10, rw));
            return;
        }
        let Some(name) = selected.child("name") else { return };
        selected = name;
    }
    if !selected.is(NodeKind::SimpleName) {
        return;
    }
    let identifier = selected.identifier();
    let key = match problem.problem_id {
        p::LocalVariableHidingLocalVariable | p::LocalVariableHidingField => "LocalCorrectionsSubProcessor_hiding_local_label",
        p::FieldHidingLocalVariable | p::FieldHidingField | p::DuplicateField => "LocalCorrectionsSubProcessor_hiding_field_label",
        p::ArgumentHidingLocalVariable | p::ArgumentHidingField => "LocalCorrectionsSubProcessor_hiding_argument_label",
        p::DuplicateMethod => "LocalCorrectionsSubProcessor_renaming_duplicate_method",
        _ => "LocalCorrectionsSubProcessor_rename_var_label",
    };
    let label = messages::format(messages::correction(key), &[&identifier]);
    let suggestion = if problem.problem_id == p::UseEnumAsAnIdentifier { "enumeration".to_owned() } else { format!("{identifier}1") };

    let mut tree = EditTree::new();
    for node in find_by_node(ctx.root(), selected) {
        let edit = tree.new_edit(node.start() as i32, node.length() as i32, EditKind::Replace(suggestion.clone()));
        let _ = tree.add_child(EditTree::ROOT, edit);
    }
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::LINKED_NAMES_ASSIST, Change::Cu(vec![CuChange::edits(ctx.ast.clone(), tree)])));
}
