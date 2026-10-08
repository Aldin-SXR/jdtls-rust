//! Ports of `RefactorProcessor.getConvertVarTypeToResolvedTypeProposal` and
//! `getConvertResolvedTypeToVarTypeProposal`.

use std::collections::BTreeMap;

use super::util::compliance_at_least;
use crate::correction::edit::Env;
use crate::correction::type_mismatch::bindings::{find_type_in_hierarchy, normalize_for_declaration_use};
use crate::correction::type_mismatch::proposals::{import_context, type_change_proposal};
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::rewrite::import_remover::ImportRemover;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{BindingRef, Node, NodeKind};

use super::util::is_var_type;

/// `getSimpleNameForVariable`.
fn simple_name_for_variable(node: Node<'_>) -> Option<Node<'_>> {
    if !node.is(NodeKind::SimpleName) {
        return None;
    }
    let mut name = node;
    if !node.flag("declaration") {
        let mut n = Some(node);
        while let Some(x) = n.filter(|x| x.kind().is_name() || x.kind().is_type()) {
            n = x.parent();
        }
        if let Some(statement) = n.filter(|x| x.is(NodeKind::VariableDeclarationStatement)) {
            if let Some(first) = statement.list("fragments").first() {
                name = first.child("name")?;
            }
        }
    }
    Some(name)
}

struct Variable<'a> {
    binding: BindingRef<'a>,
    declaration: Node<'a>,
}

fn variable<'a>(options: &BTreeMap<String, String>, node: Node<'a>) -> Option<(Variable<'a>, Node<'a>)> {
    if !compliance_at_least(options, "10") {
        return None;
    }
    let name = simple_name_for_variable(node)?;
    let binding = name.binding().filter(|b| b.is_variable())?;
    if binding.is_field() || binding.is_parameter() {
        return None;
    }
    let declaration = binding.declaring_node()?;
    Some((Variable { binding, declaration }, name))
}

/// `RefactorProcessor.getConvertVarTypeToResolvedTypeProposal`.
pub async fn convert_var_type_to_resolved_type(env: &Env<'_>, ctx: &Context, options: &BTreeMap<String, String>, node: Node<'_>, out: &mut Vec<Proposal>) {
    let Some((v, _)) = variable(options, node) else { return };
    let Some(type_binding) = v.binding.var_type() else { return };
    if type_binding.is_anonymous() || type_binding.has(crate::semantic_ast::bflag::INTERSECTION) || type_binding.is_wildcard_type() {
        return;
    }
    let declared_type = match v.declaration.kind() {
        NodeKind::SingleVariableDeclaration => v.declaration.child("type"),
        NodeKind::VariableDeclarationFragment => v.declaration.parent().filter(|p| matches!(p.kind(), NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression)).and_then(|p| p.child("type")),
        _ => None,
    };
    if !declared_type.is_some_and(is_var_type) {
        return;
    }
    if let Some(mut p) = type_change_proposal(env, &ctx.ast, v.binding, type_binding, false, relevance::CHANGE_VARIABLE, None).await {
        p.kind = kind::REFACTOR.to_owned();
        out.push(p);
    }
}

/// `RefactorProcessor.getConvertResolvedTypeToVarTypeProposal`.
pub fn convert_resolved_type_to_var_type(ctx: &Context, options: &BTreeMap<String, String>, node: Node<'_>, out: &mut Vec<Proposal>) {
    let Some((v, _)) = variable(options, node) else { return };
    let Some(type_binding) = v.binding.var_type() else { return };
    let declaration = v.declaration;
    let mut declared_type = None;
    let mut expression = None;
    let mut expression_type = None;
    match declaration.kind() {
        NodeKind::SingleVariableDeclaration => {
            declared_type = declaration.child("type");
            expression = declaration.child("initializer");
            if let Some(e) = expression {
                expression_type = e.type_binding();
            } else if let Some(parent) = declaration.parent().filter(|p| p.is(NodeKind::EnhancedForStatement)) {
                expression = parent.child("expression");
                if let Some(binding) = expression.and_then(|e| e.type_binding()) {
                    if binding.is_array() {
                        expression_type = binding.element_type();
                    } else if let Some(iterable) = find_type_in_hierarchy(binding, "java.lang.Iterable") {
                        let arguments = iterable.type_arguments();
                        if let [argument] = arguments.as_slice() {
                            expression_type = normalize_for_declaration_use(*argument);
                        }
                    }
                }
            }
        }
        NodeKind::VariableDeclarationFragment => {
            expression = declaration.child("initializer");
            if let Some(e) = expression {
                expression_type = e.type_binding();
            }
            match declaration.parent() {
                Some(p) if p.is(NodeKind::VariableDeclarationStatement) => declared_type = p.child("type"),
                Some(p) if p.is(NodeKind::VariableDeclarationExpression) => {
                    if p.list("fragments").len() > 1 {
                        return;
                    }
                    declared_type = p.child("type");
                }
                _ => {}
            }
        }
        _ => {}
    }
    let Some(declared_type) = declared_type.filter(|t| !is_var_type(*t)) else { return };
    let Some(_) = expression.filter(|e| !matches!(e.kind(), NodeKind::ArrayInitializer | NodeKind::LambdaExpression) && !e.kind().is_method_reference()) else { return };
    if !expression_type.is_some_and(|t| t == type_binding) {
        return;
    }
    if let Some(p) = type_to_var_proposal(ctx, options, v.binding, declaration, declared_type) {
        out.push(p);
    }
}

/// `TypeChangeCorrectionProposalCore(cu, binding, astRoot, oldType, relevance)`.
fn type_to_var_proposal(ctx: &Context, options: &BTreeMap<String, String>, binding: BindingRef<'_>, decl: Node<'_>, old_type: Node<'_>) -> Option<Proposal> {
    let name = binding.name();
    let label = if decl.is(NodeKind::SingleVariableDeclaration) {
        messages::format(messages::correction("TypeChangeCompletionProposal_param_name"), &[name, "var"])
    } else {
        messages::format(messages::correction("TypeChangeCompletionProposal_variable_name"), &[name, "var"])
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let mut remover = ImportRemover::new();
    let context = import_context(&ctx.ast, decl, options);
    let var_name = rw.new_name("var");
    let var_type = rw.new_simple_type(var_name);
    let remove_dimensions = |rw: &mut ASTRewrite, node: Node<'_>| {
        for d in node.list("extraDimensions2") {
            rw.remove(RNode::Orig(d.id));
        }
    };
    match decl.kind() {
        NodeKind::VariableDeclarationFragment => {
            let parent = decl.parent()?;
            match parent.kind() {
                NodeKind::VariableDeclarationStatement => {
                    let fragments = parent.list("fragments");
                    let block = parent.parent();
                    if fragments.len() > 1 && block.is_some_and(|b| b.is(NodeKind::Block)) {
                        let block = block?;
                        let placeholder = rw.create_move_target(decl.id);
                        let statement = rw.new_node(NodeKind::VariableDeclarationStatement);
                        rw.put_child(statement, "type", var_type);
                        rw.put_list(statement, "fragments", vec![placeholder]);
                        if fragments[0] == decl {
                            rw.list_insert_before(RNode::Orig(block.id), "statements", statement, RNode::Orig(parent.id));
                        } else {
                            rw.list_insert_after(RNode::Orig(block.id), "statements", statement, RNode::Orig(parent.id));
                        }
                    } else {
                        rw.set(RNode::Orig(parent.id), "type", Some(var_type));
                        remove_dimensions(&mut rw, decl);
                        handled_inferred_parameterized_type(&mut rw, &mut imports, &context, parent, decl);
                        remover.register_removed_node(old_type.id);
                    }
                }
                NodeKind::VariableDeclarationExpression => {
                    rw.set(RNode::Orig(parent.id), "type", Some(var_type));
                    remove_dimensions(&mut rw, decl);
                    handled_inferred_parameterized_type(&mut rw, &mut imports, &context, parent, decl);
                    remover.register_removed_node(old_type.id);
                }
                _ => {}
            }
        }
        NodeKind::SingleVariableDeclaration => {
            rw.set(RNode::Orig(decl.id), "type", Some(var_type));
            remove_dimensions(&mut rw, decl);
            remover.register_removed_node(old_type.id);
        }
        _ => return None,
    }
    remover.apply_removes(&ctx.ast, &mut imports);
    Some(Proposal::new(label, kind::REFACTOR, relevance::CHANGE_VARIABLE, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])))
}

/// `TypeChangeCorrectionProposalCore.handledInferredParametrizedType`.
fn handled_inferred_parameterized_type(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    context: &crate::features::constructors::ConstructorImportContext,
    node: Node<'_>,
    declaring_node: Node<'_>,
) {
    let fragments = node.list("fragments");
    let [fragment] = fragments.as_slice() else { return };
    let mut process = fragment.child("initializer");
    if process.is_none() && declaring_node.is(NodeKind::VariableDeclarationFragment) {
        process = declaring_node.child("initializer");
    }
    let Some(created_type) = process.filter(|p| p.is(NodeKind::ClassInstanceCreation)).and_then(|c| c.child("type")).filter(|t| t.is(NodeKind::ParameterizedType)) else { return };
    let changed = std::iter::once(node).chain(node.descendants()).any(|n| {
        n.is(NodeKind::ParameterizedType) && n.list("typeArguments").is_empty() && n.binding().is_some_and(|b| !b.type_arguments().is_empty())
    });
    if !changed {
        return;
    }
    if let Some(binding) = created_type.binding() {
        for argument in binding.type_arguments() {
            let t = imports.add_import_type(argument, rw, context, TypeLocation::TypeArgument);
            rw.list_insert_last(RNode::Orig(created_type.id), "typeArguments", t);
        }
    }
}
