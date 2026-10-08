//! Port of `RefactorProcessor.getAddStaticImportProposals`.

use std::collections::BTreeMap;

use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::rewrite::import_remover::ImportRemover;
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, BindingRef, Node, NodeKind};

/// `isDirectlyAccessible`.
fn is_directly_accessible(name: Node<'_>, declaring_class: BindingRef<'_>) -> bool {
    name.ancestors().any(|n| {
        (n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration))
            && n.binding().is_some_and(|b| {
                let erased = declaring_class.erasure().unwrap_or(declaring_class);
                b == declaring_class || crate::correction::type_mismatch::bindings::is_super_type(erased, b.erasure().unwrap_or(b), false)
            })
    })
}

fn full_name(name: Node<'_>) -> String {
    name.source_text().chars().filter(|c| !c.is_whitespace()).collect()
}

pub fn add_static_import_proposals(ctx: &Context, options: &BTreeMap<String, String>, node: Node<'_>, out: &mut Vec<Proposal>) {
    if !node.is(NodeKind::SimpleName) {
        return;
    }
    let name = node;
    let Some(parent) = name.parent() else { return };
    let binding: BindingRef<'_>;
    let declaring_class: Option<BindingRef<'_>>;
    let is_field;
    if parent.is(NodeKind::MethodInvocation) {
        let Some(expression) = parent.child("expression") else { return };
        if expression == name {
            return;
        }
        let Some(method) = parent.method_binding() else { return };
        binding = method;
        declaring_class = method.declaring_class();
        is_field = false;
    } else if parent.is(NodeKind::QualifiedName) {
        if parent.child("qualifier") == Some(name) || parent.parent().is_some_and(|p| p.is(NodeKind::ImportDeclaration)) {
            return;
        }
        let Some(variable) = parent.binding().filter(|b| b.is_variable()) else { return };
        binding = variable;
        declaring_class = variable.declaring_class();
        is_field = true;
    } else {
        return;
    }
    if binding.modifiers() & modifier::STATIC == 0 {
        return;
    }
    let Some(declaring_class) = declaring_class else { return };
    let mut need_import = false;
    if !is_directly_accessible(name, declaring_class) {
        if declaring_class.modifiers() & modifier::PRIVATE != 0 {
            return;
        }
        need_import = true;
    }

    let ast = &ctx.ast;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut rw_all = ASTRewrite::new(ast.clone());
    let mut remover = ImportRemover::new();
    let mut remover_all = ImportRemover::new();
    let invocation = parent.is(NodeKind::MethodInvocation).then_some(parent);
    let qualified = parent.is(NodeKind::QualifiedName).then_some(parent);
    if let Some(mi) = invocation {
        if let Some(expression) = mi.child("expression") {
            rw.remove(RNode::Orig(expression.id));
            remover.register_removed_node(expression.id);
            remover_all.register_removed_node(expression.id);
        }
        for t in mi.list("typeArguments") {
            rw.remove(RNode::Orig(t.id));
            remover.register_removed_node(t.id);
            remover_all.register_removed_node(t.id);
        }
    } else if let Some(qn) = qualified {
        let replacement = rw.new_name(&name.identifier());
        rw.replace(RNode::Orig(qn.id), Some(replacement));
        remover.register_removed_node(qn.id);
        remover_all.register_removed_node(qn.id);
    }

    if let Some(mi_final) = invocation {
        let final_expression = mi_final.child("expression").filter(|e| e.kind().is_name());
        for candidate in std::iter::once(ast.root()).chain(ast.root().descendants()).filter(|n| n.is(NodeKind::MethodInvocation)) {
            let Some(expression) = candidate.child("expression").filter(|e| e.kind().is_name()) else { continue };
            if let Some(final_expression) = final_expression {
                if full_name(final_expression) == full_name(expression)
                    && mi_final.child("name").map(|n| n.identifier()) == candidate.child("name").map(|n| n.identifier())
                {
                    for t in candidate.list("typeArguments") {
                        rw_all.remove(RNode::Orig(t.id));
                        remover_all.register_removed_node(t.id);
                    }
                    rw_all.remove(RNode::Orig(expression.id));
                    remover_all.register_removed_node(expression.id);
                }
            }
        }
    }
    if let Some(qn_final) = qualified {
        let target = full_name(qn_final);
        for candidate in std::iter::once(ast.root()).chain(ast.root().descendants()).filter(|n| n.is(NodeKind::QualifiedName)) {
            if full_name(candidate) == target {
                let replacement = rw_all.new_name(&name.identifier());
                rw_all.replace(RNode::Orig(candidate.id), Some(replacement));
                remover_all.register_removed_node(candidate.id);
            }
        }
    }

    let make_imports = || {
        let mut imports = ImportRewrite::create_for_corrections(ast.clone(), options);
        if need_import {
            let declaring = declaring_class.type_declaration().unwrap_or(declaring_class);
            imports.add_static_import(declaring.qualified_name(), binding.name(), is_field, &DefaultContext);
        }
        imports
    };
    let mut imports = make_imports();
    remover.apply_removes(ast, &mut imports);
    let mut imports_all = make_imports();
    remover_all.apply_removes(ast, &mut imports_all);
    out.push(Proposal::new(
        messages::correction("QuickAssistProcessor_convert_to_static_import"),
        kind::REFACTOR,
        relevance::ADD_STATIC_IMPORT,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
    out.push(Proposal::new(
        messages::correction("QuickAssistProcessor_convert_to_static_import_replace_all"),
        kind::REFACTOR,
        relevance::ADD_STATIC_IMPORT,
        Change::Cu(vec![CuChange::rewrite(rw_all).with_imports(imports_all)]),
    ));
}
