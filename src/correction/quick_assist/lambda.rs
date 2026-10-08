//! Ports of the lambda quick assists: `ChangeLambdaBodyToBlockFixCore`,
//! `ChangeLambdaBodyToExpressionFixCore`, `AddInferredLambdaParameterTypesFixCore`,
//! `AddVarLambdaParameterTypesFixCore`, `RemoveVarOrInferredLambdaParameterTypesFixCore`.

use std::collections::BTreeMap;

use super::util::{is_11_or_higher, is_var_type, replace_wildcards_and_captures};
use crate::correction::type_mismatch::proposals::import_context;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Node, NodeKind};

/// The lambda a name/body node belongs to, as the `create*Fix` methods derive it.
fn lambda_of_body(node: Node<'_>) -> Option<Node<'_>> {
    if node.is(NodeKind::LambdaExpression) {
        return Some(node);
    }
    node.parent().filter(|p| p.is(NodeKind::LambdaExpression) && node.location_is("body"))
}

fn lambda_of_parameter_name(node: Node<'_>) -> Option<Node<'_>> {
    let parent = node.parent()?;
    let name_of_declaration = node.location_is("name") && matches!(parent.kind(), NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration);
    if node.is(NodeKind::LambdaExpression) {
        return Some(node);
    }
    if name_of_declaration && parent.location_is("parameters") {
        return parent.parent().filter(|l| l.is(NodeKind::LambdaExpression));
    }
    None
}

/// `QuickAssistProcessorUtil.getBlockBodyForLambda`.
pub(super) fn block_body_for_lambda(rw: &mut ASTRewrite, body: RNode, returns_void: bool) -> RNode {
    let statement = if returns_void { rw.new_expression_statement(body) } else { rw.new_return_statement(Some(body)) };
    rw.new_block(vec![statement])
}

/// `QuickAssistProcessor.getChangeLambdaBodyToBlockProposal`.
pub fn change_lambda_body_to_block(ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let Some(lambda) = lambda_of_body(covering) else { return };
    let Some(body) = lambda.child("body") else { return };
    if !body.kind().is_expression() {
        return;
    }
    let Some(method) = lambda.method_binding() else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let moved = rw.create_move_target(body.id);
    let returns_void = method.return_type().is_some_and(|r| r.is_primitive() && r.name() == "void");
    let block = block_body_for_lambda(&mut rw, moved, returns_void);
    rw.set(RNode::Orig(lambda.id), "body", Some(block));
    let label = messages::correction("QuickAssistProcessor_change_lambda_body_to_block");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, rw));
}

/// `LambdaQueries.getSingleExpressionFromLambdaBody`.
pub(super) fn single_expression_from_lambda_body(body: Node<'_>) -> Option<Node<'_>> {
    let statements = body.list("statements");
    let [single] = statements.as_slice() else { return None };
    match single.kind() {
        NodeKind::ReturnStatement => single.child("expression"),
        NodeKind::ExpressionStatement => {
            let expression = single.child("expression")?;
            let valid = match expression.kind() {
                NodeKind::Assignment | NodeKind::ClassInstanceCreation | NodeKind::MethodInvocation | NodeKind::PostfixExpression | NodeKind::SuperMethodInvocation => true,
                NodeKind::PrefixExpression => matches!(expression.simple("operator"), Some("++" | "--")),
                _ => false,
            };
            valid.then_some(expression)
        }
        _ => None,
    }
}

/// `QuickAssistProcessor.getChangeLambdaBodyToExpressionProposal`.
pub fn change_lambda_body_to_expression(ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let Some(lambda) = lambda_of_body(covering) else { return };
    let Some(body) = lambda.child("body").filter(|b| b.is(NodeKind::Block)) else { return };
    let Some(expression) = single_expression_from_lambda_body(body) else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let moved = rw.create_move_target(expression.id);
    rw.set(RNode::Orig(lambda.id), "body", Some(moved));
    let label = messages::correction("QuickAssistProcessor_change_lambda_body_to_expression");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, rw));
}

/// `QuickAssistProcessor.getAddInferredLambdaParameterTypesProposal`.
pub fn add_inferred_lambda_parameter_types(ctx: &Context, options: &BTreeMap<String, String>, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let Some(lambda) = lambda_of_parameter_name(covering) else { return };
    let parameters = lambda.list("parameters");
    let Some(first) = parameters.first() else { return };
    let mut var_type = false;
    if first.is(NodeKind::SingleVariableDeclaration) {
        if is_11_or_higher(options) && first.child("type").is_some_and(is_var_type) {
            var_type = true;
        }
        if !var_type {
            return;
        }
    }
    let Some(method) = lambda.method_binding() else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    rw.set_simple(RNode::Orig(lambda.id), "parentheses", Some("true"));
    let context = import_context(&ctx.ast, lambda, options);
    let parameter_types = method.parameter_types();
    for (i, param) in parameters.iter().enumerate() {
        let Some(name) = param.child("name") else { return };
        let new_param = rw.new_node(NodeKind::SingleVariableDeclaration);
        let new_name = rw.new_simple_name(&name.identifier());
        rw.put_child(new_param, "name", new_name);
        let Some(t) = parameter_types.get(i) else { return };
        let t = replace_wildcards_and_captures(*t);
        let typ = imports.add_import_type(t, &mut rw, &context, TypeLocation::Parameter);
        rw.put_child(new_param, "type", typ);
        rw.replace(RNode::Orig(param.id), Some(new_param));
    }
    let key = if var_type {
        "QuickAssistProcessor_replace_var_with_inferred_lambda_parameter_types"
    } else {
        "QuickAssistProcessor_add_inferred_lambda_parameter_types"
    };
    let label = messages::correction(key);
    out.push(Proposal::new(
        label,
        kind::QUICK_ASSIST,
        relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
}

/// `QuickAssistProcessor.getAddVarLambdaParameterTypesProposal`.
pub fn add_var_lambda_parameter_types(ctx: &Context, options: &BTreeMap<String, String>, covering: Node<'_>, out: &mut Vec<Proposal>) {
    if covering.parent().is_none() || !is_11_or_higher(options) {
        return;
    }
    let Some(lambda) = lambda_of_parameter_name(covering) else { return };
    let parameters = lambda.list("parameters");
    let Some(first) = parameters.first() else { return };
    let mut explicit_type = false;
    if first.is(NodeKind::SingleVariableDeclaration) {
        if first.child("type").is_some_and(is_var_type) {
            return;
        }
        explicit_type = true;
    }
    if lambda.method_binding().is_none() {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.set_simple(RNode::Orig(lambda.id), "parentheses", Some("true"));
    for param in &parameters {
        let old_type = if param.is(NodeKind::SingleVariableDeclaration) { param.child("type") } else { None };
        if let Some(old) = old_type {
            let name = rw.new_name("var");
            let var = rw.new_simple_type(name);
            rw.replace(RNode::Orig(old.id), Some(var));
        } else {
            let new_param = rw.new_node(NodeKind::SingleVariableDeclaration);
            let Some(name) = param.child("name") else { return };
            let new_name = rw.new_simple_name(&name.identifier());
            rw.put_child(new_param, "name", new_name);
            let type_name = rw.new_name("var");
            let var = rw.new_simple_type(type_name);
            rw.put_child(new_param, "type", var);
            rw.replace(RNode::Orig(param.id), Some(new_param));
        }
    }
    let key = if explicit_type { "QuickAssistProcessor_replace_lambda_parameter_types_with_var" } else { "QuickAssistProcessor_add_var_lambda_parameter_types" };
    out.push(Proposal::rewrite(messages::correction(key), kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, rw));
}

/// `QuickAssistProcessor.getRemoveVarOrInferredLambdaParameterTypesProposal`.
pub fn remove_var_or_inferred_lambda_parameter_types(ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let Some(parent) = covering.parent() else { return };
    let lambda = if covering.is(NodeKind::LambdaExpression) {
        covering
    } else if covering.location_is("name") && parent.is(NodeKind::SingleVariableDeclaration) && parent.location_is("parameters") {
        match parent.parent().filter(|l| l.is(NodeKind::LambdaExpression)) {
            Some(l) => l,
            None => return,
        }
    } else {
        return;
    };
    let parameters = lambda.list("parameters");
    if !parameters.first().is_some_and(|p| p.is(NodeKind::SingleVariableDeclaration)) {
        return;
    }
    if lambda.method_binding().is_none() {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.set_simple(RNode::Orig(lambda.id), "parentheses", Some("true"));
    for param in &parameters {
        if param.is(NodeKind::SingleVariableDeclaration) {
            let Some(name) = param.child("name") else { return };
            let fragment = rw.new_variable_declaration_fragment(&name.identifier(), None);
            rw.replace(RNode::Orig(param.id), Some(fragment));
        }
    }
    let label = messages::correction("QuickAssistProcessor_remove_lambda_parameter_types");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, rw));
}
