//! `VariableDeclarationRewrite.rewriteModifiers` for a declaration with
//! several fragments, of which one changes its modifiers: the declaration is
//! split.

use std::collections::HashSet;

use super::change::set_modifiers;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Node, NodeId, NodeKind};

/// `ModifierRewrite.copyAllAnnotations(otherDecl)`.
fn copy_all_annotations(rw: &mut ASTRewrite, target: RNode, other: Node<'_>) {
    for m in other.list("modifiers") {
        if m.kind().is_annotation() {
            let copy = rw.create_copy_target(m.id);
            rw.list_insert_last(target, "modifiers", copy);
        }
    }
}

/// `ModifierRewrite.copyAllModifiers(otherDecl, group, copyIndividually)`.
fn copy_all_modifiers(rw: &mut ASTRewrite, target: RNode, other: Node<'_>) {
    for m in other.list("modifiers") {
        let copy = rw.create_copy_target(m.id);
        rw.list_insert_last(target, "modifiers", copy);
    }
}

/// `rewriteModifiers(FieldDeclaration, toChange, ..)`.
pub fn rewrite_field_modifiers(rw: &mut ASTRewrite, decl: Node<'_>, to_change: Node<'_>, included: i32, excluded: i32) {
    let Some(block) = decl.parent() else { return };
    let fragments = decl.list("fragments");
    let Some((first, rest)) = fragments.split_first() else { return };
    let mut last = *first;
    let mut last_statement = RNode::Orig(decl.id);
    if last == to_change {
        set_modifiers(rw, RNode::Orig(decl.id), included, excluded);
    }
    let mut fragments_rewrite: Option<RNode> = None;
    let mut moved: HashSet<NodeId> = HashSet::new();
    for &current in rest {
        let change_last = last == to_change;
        let change_current = current == to_change;
        if change_last != change_current || moved.contains(&last.id) {
            // need to split an existing field declaration
            let move_target = rw.create_move_target(current.id);
            let new_statement = rw.new_node(NodeKind::FieldDeclaration);
            rw.list_insert_last(new_statement, "fragments", move_target);
            moved.insert(current.id);
            if let Some(t) = decl.child("type") {
                let copy = rw.create_copy_target(t.id);
                rw.put_child(new_statement, "type", copy);
            }
            copy_all_annotations(rw, new_statement, decl);
            rw.list_insert_after(RNode::Orig(block.id), "bodyDeclarations", new_statement, last_statement);
            fragments_rewrite = Some(new_statement);
            last_statement = new_statement;
            if change_current {
                let new_modifiers = (decl.modifiers() & !excluded) | included;
                set_modifiers(rw, new_statement, new_modifiers, excluded);
            } else {
                set_modifiers(rw, new_statement, decl.modifiers(), 0);
            }
        } else if let Some(target) = fragments_rewrite {
            let fragment = rw.create_move_target(current.id);
            moved.insert(current.id);
            rw.list_insert_last(target, "fragments", fragment);
        }
        last = current;
    }
}

/// `rewriteModifiers(VariableDeclarationStatement, toChange, ..)`.
pub fn rewrite_statement_modifiers(rw: &mut ASTRewrite, decl: Node<'_>, to_change: Node<'_>, included: i32, excluded: i32) {
    let Some(block) = decl.parent() else { return };
    let fragments = decl.list("fragments");
    let Some((first, rest)) = fragments.split_first() else { return };
    let mut last = *first;
    let mut last_statement = RNode::Orig(decl.id);
    if last == to_change {
        set_modifiers(rw, RNode::Orig(decl.id), included, excluded);
    }
    let mut fragments_rewrite: Option<RNode> = None;
    for &current in rest {
        if (last == to_change) != (current == to_change) {
            let move_target = rw.create_move_target(current.id);
            let new_statement = rw.new_node(NodeKind::VariableDeclarationStatement);
            rw.list_insert_last(new_statement, "fragments", move_target);
            if let Some(t) = decl.child("type") {
                let copy = rw.create_copy_target(t.id);
                rw.put_child(new_statement, "type", copy);
            }
            if current == to_change {
                copy_all_annotations(rw, new_statement, decl);
                let new_modifiers = (decl.modifiers() & !excluded) | included;
                set_modifiers(rw, new_statement, new_modifiers, excluded);
            } else {
                copy_all_modifiers(rw, new_statement, decl);
            }
            rw.list_insert_after(RNode::Orig(block.id), "statements", new_statement, last_statement);
            fragments_rewrite = Some(new_statement);
            last_statement = new_statement;
        } else if let Some(target) = fragments_rewrite {
            let fragment = rw.create_move_target(current.id);
            rw.list_insert_last(target, "fragments", fragment);
        }
        last = current;
    }
}
