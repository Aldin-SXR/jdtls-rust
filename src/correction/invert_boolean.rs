//! Port of jdt.ls `InvertBooleanUtility` (invert conditions, invert local
//! variable).

use std::collections::HashSet;

use serde_json::json;
use tower_lsp::lsp_types::CodeActionParams;

use super::parentheses::{expression_precedence_of, needs_parentheses, operator_precedence};
use super::{kind, messages, relevance, Context, Proposal};
use crate::refactoring::extract_temp::is_declaration;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{BindingRef, Node, NodeId, NodeKind};

pub const INVERT_VARIABLE_COMMAND: &str = "invertVariable";

/// `SimpleNameRenameProvider` of `getInvertVariableProposal`.
struct Renamer<'a> {
    binding: BindingRef<'a>,
    identifier: String,
    renamed: HashSet<NodeId>,
}

fn is_boolean(e: Node<'_>) -> bool {
    e.type_binding().is_some_and(|b| matches!(b.qualified_name(), "boolean" | "java.lang.Boolean"))
}

/// `getBooleanExpression(node)`.
fn boolean_expression(mut node: Node<'_>) -> Option<Node<'_>> {
    if !node.kind().is_expression() {
        return None;
    }
    let is_parent = |n: Node<'_>, k: NodeKind| n.parent().is_some_and(|p| p.is(k));
    if node.location_is("name") && is_parent(node, NodeKind::QualifiedName) {
        node = node.parent()?;
    }
    while node.location_is("expression") && is_parent(node, NodeKind::ParenthesizedExpression) {
        node = node.parent()?;
    }
    if !is_boolean(node) {
        return None;
    }
    let parent = node.parent()?;
    if parent.is(NodeKind::InfixExpression) {
        return Some(node);
    }
    let location = node.location()?;
    use NodeKind as K;
    let invertible = matches!(
        (parent.kind(), location),
        (K::Assignment, "rightHandSide")
            | (K::IfStatement | K::WhileStatement | K::DoStatement | K::ReturnStatement | K::ForStatement | K::AssertStatement | K::ConditionalExpression, "expression")
            | (K::MethodInvocation | K::ConstructorInvocation | K::SuperMethodInvocation | K::EnumConstantDeclaration | K::SuperConstructorInvocation | K::ClassInstanceCreation, "arguments")
            | (K::PrefixExpression, "operand")
    );
    invertible.then_some(node)
}

fn precedence(rw: &ASTRewrite, n: RNode) -> i32 {
    let value = rw.new_value(n, "operator");
    expression_precedence_of(rw.kind(n), value.simple())
}

fn parenthesize_if_required(rw: &mut ASTRewrite, operand: RNode, new_operator_precedence: i32) -> RNode {
    if new_operator_precedence > precedence(rw, operand) {
        rw.new_parenthesized_expression(operand)
    } else {
        operand
    }
}

fn new_not(rw: &mut ASTRewrite, operand: RNode) -> RNode {
    let prefix = rw.new_node(NodeKind::PrefixExpression);
    rw.put_simple(prefix, "operator", "!");
    rw.put_child(prefix, "operand", operand)
}

fn inversed_not_expression(rw: &mut ASTRewrite, expression: Node<'_>) -> RNode {
    let copy = rw.create_copy_target(expression.id);
    let parenthesized = rw.new_parenthesized_expression(copy);
    new_not(rw, parenthesized)
}

fn renamed_name_copy(provider: &mut Option<&mut Renamer<'_>>, rw: &mut ASTRewrite, expression: Node<'_>) -> RNode {
    if let Some(p) = provider {
        if expression.is(NodeKind::SimpleName) && expression.binding().is_some_and(|b| b == p.binding) {
            p.renamed.insert(expression.id);
            return rw.new_simple_name(&p.identifier);
        }
    }
    rw.create_copy_target(expression.id)
}

fn inversed_infix_expression(rw: &mut ASTRewrite, expression: Node<'_>, new_operator: &str, provider: &mut Option<&mut Renamer<'_>>) -> RNode {
    let left = renamed_name_copy(provider, rw, expression.child("leftOperand").expect("leftOperand"));
    let right = renamed_name_copy(provider, rw, expression.child("rightOperand").expect("rightOperand"));
    rw.new_infix_expression(left, new_operator, right)
}

fn inversed_and_or_expression(rw: &mut ASTRewrite, infix: Node<'_>, new_operator: &str, provider: &mut Option<&mut Renamer<'_>>) -> RNode {
    let precedence = operator_precedence(new_operator);
    let left = inversed_expression(rw, infix.child("leftOperand").expect("leftOperand"), provider);
    let left = parenthesize_if_required(rw, left, precedence);
    let right = inversed_expression(rw, infix.child("rightOperand").expect("rightOperand"), provider);
    let right = parenthesize_if_required(rw, right, precedence);
    let new = rw.new_infix_expression(left, new_operator, right);
    let mut extended = Vec::new();
    for operand in infix.list("extendedOperands") {
        let e = inversed_expression(rw, operand, provider);
        extended.push(parenthesize_if_required(rw, e, precedence));
    }
    rw.put_list(new, "extendedOperands", extended);
    new
}

/// `NecessaryParenthesesChecker.needsParentheses(copy, !, OPERAND_PROPERTY)`
/// for a copy placeholder or new node.
fn needs_parentheses_as_not_operand(rw: &ASTRewrite, n: RNode) -> bool {
    use NodeKind as K;
    match rw.kind(n) {
        K::SwitchExpression => true,
        K::PrefixExpression | K::ArrayCreation => false,
        k if super::parentheses::expression_type_needs_parentheses(k) => precedence(rw, n) <= 13,
        _ => false,
    }
}

/// `getInversedExpression(rewrite, expression, provider)`.
fn inversed_expression(rw: &mut ASTRewrite, expression: Node<'_>, provider: &mut Option<&mut Renamer<'_>>) -> RNode {
    use NodeKind as K;
    match expression.kind() {
        K::BooleanLiteral => {
            let value = expression.simple("booleanValue") == Some("true");
            let lit = rw.new_node(K::BooleanLiteral);
            return rw.put_simple(lit, "booleanValue", if value { "false" } else { "true" });
        }
        K::InfixExpression => {
            let inverse = match expression.simple("operator").unwrap_or("") {
                "<" => Some(">="),
                ">" => Some("<="),
                "<=" => Some(">"),
                ">=" => Some("<"),
                "==" => Some("!="),
                "!=" => Some("=="),
                _ => None,
            };
            if let Some(op) = inverse {
                return inversed_infix_expression(rw, expression, op, provider);
            }
            let and_or = match expression.simple("operator").unwrap_or("") {
                "&&" => Some("||"),
                "||" => Some("&&"),
                "&" => Some("|"),
                "|" => Some("&"),
                _ => None,
            };
            if let Some(op) = and_or {
                return inversed_and_or_expression(rw, expression, op, provider);
            }
            if expression.simple("operator") == Some("^") {
                return inversed_not_expression(rw, expression);
            }
        }
        K::PrefixExpression if expression.simple("operator") == Some("!") => {
            let mut operand = expression.child("operand").expect("operand");
            if operand.is(K::ParenthesizedExpression) {
                if let (Some(parent), Some(location)) = (expression.parent(), expression.location()) {
                    let inner = crate::semantic_ast::resolve::unparenthesed_expression(operand);
                    if !needs_parentheses(inner, parent, location) {
                        operand = operand.child("expression").expect("expression");
                    }
                }
            }
            let copy = renamed_name_copy(provider, rw, operand);
            if rw.kind(copy) == K::InfixExpression {
                let op = operand.simple("operator").unwrap_or("+").to_owned();
                rw.put_simple(copy, "operator", &op);
            }
            return copy;
        }
        K::InstanceofExpression => return inversed_not_expression(rw, expression),
        K::ParenthesizedExpression => {
            let mut inner = expression.child("expression").expect("expression");
            while inner.is(K::ParenthesizedExpression) {
                inner = inner.child("expression").expect("expression");
            }
            if inner.is(K::InstanceofExpression) {
                return inversed_expression(rw, inner, provider);
            }
            let inverse = inversed_expression(rw, inner, provider);
            return rw.new_parenthesized_expression(inverse);
        }
        K::ConditionalExpression => {
            let new = rw.new_node(K::ConditionalExpression);
            let condition = rw.create_copy_target(expression.child("expression").expect("expression").id);
            rw.put_child(new, "expression", condition);
            let then = inversed_expression(rw, expression.child("thenExpression").expect("then"), &mut None);
            rw.put_child(new, "thenExpression", then);
            let other = inversed_expression(rw, expression.child("elseExpression").expect("else"), &mut None);
            rw.put_child(new, "elseExpression", other);
            return new;
        }
        _ => {}
    }
    let mut operand = renamed_name_copy(provider, rw, expression);
    if needs_parentheses_as_not_operand(rw, operand) {
        operand = rw.new_parenthesized_expression(operand);
    }
    new_not(rw, operand)
}

/// `getInverseConditionProposals(params, context, covering, proposals)`.
pub fn inverse_condition_proposals(ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    if ctx.selection_length == 0 {
        let mut found = None;
        let mut current = Some(covering);
        while let Some(c) = current.filter(|c| c.kind().is_expression()) {
            if let Some(b) = boolean_expression(c) {
                found = Some(b);
            }
            current = c.parent();
        }
        let Some(found) = found else { return false };
        let inversed = inversed_expression(&mut rw, found, &mut None);
        rw.replace(RNode::Orig(found.id), Some(inversed));
    } else {
        let covered = crate::features::accessors::actions::fully_covered_context(ctx);
        if covered.is_empty() {
            return false;
        }
        let mut has_changes = false;
        for node in covered {
            if let Some(expression) = boolean_expression(node) {
                let inversed = inversed_expression(&mut rw, expression, &mut None);
                rw.replace(RNode::Orig(expression.id), Some(inversed));
                has_changes = true;
            }
        }
        if !has_changes {
            return false;
        }
    }
    let label = messages::ls_correction("AdvancedQuickAssistProcessor_inverseConditions_description");
    out.push(Proposal::rewrite(label, kind::REFACTOR, relevance::INVERSE_CONDITIONS, rw));
    true
}

/// `getInvertVariableProposal(params, context, covering, returnAsCommand)`.
pub fn invert_variable_proposal(ctx: &Context, covering: Node<'_>, params: Option<&CodeActionParams>, return_as_command: bool) -> Option<Proposal> {
    if !is_invertible_variable(covering) {
        return None;
    }
    let label = messages::ls_correction("AdvancedQuickAssistProcessor_inverseBooleanVariable");
    if return_as_command {
        return Some(Proposal::command(
            label,
            kind::REFACTOR,
            relevance::INVERSE_BOOLEAN_VARIABLE,
            "java.action.applyRefactoringCommand",
            vec![json!(INVERT_VARIABLE_COMMAND), serde_json::to_value(params).expect("serializable code action parameters")],
        ));
    }
    let (rw, _) = invert_variable_rewrite(ctx, covering)?;
    Some(Proposal::rewrite(label, kind::REFACTOR, relevance::INVERSE_BOOLEAN_VARIABLE, rw))
}

fn is_invertible_variable(covering: Node<'_>) -> bool {
    if !covering.is(NodeKind::SimpleName) || !is_declaration(covering) {
        return false;
    }
    let Some(binding) = covering.binding().filter(|b| b.is_variable()) else { return false };
    !binding.is_field() && binding.var_type().is_some_and(|t| matches!(t.qualified_name(), "boolean" | "java.lang.Boolean"))
}

/// The rewrite of the non-command `getInvertVariableProposal` and the new
/// name of the covering declaration (its first linked position).
pub fn invert_variable_rewrite(ctx: &Context, covering: Node<'_>) -> Option<(ASTRewrite, RNode)> {
    if !is_invertible_variable(covering) {
        return None;
    }
    let binding = covering.binding()?;
    let method = super::type_mismatch::bindings::find_parent_method_declaration(covering)?;
    let linked = find_by_binding(method, binding);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let old = covering.identifier();
    let not_string = messages::format(messages::ls_correction("AdvancedQuickAssistProcessor_negatedVariableName"), &[""]);
    let new_identifier = if let Some(rest) = old.strip_prefix(&not_string) {
        let mut chars = rest.chars();
        match chars.next() {
            Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
            None => old.clone(),
        }
    } else {
        let mut chars = old.chars();
        let first = chars.next()?;
        messages::format(messages::ls_correction("AdvancedQuickAssistProcessor_negatedVariableName"), &[&(first.to_uppercase().collect::<String>() + chars.as_str())])
    };
    let mut renamer = Renamer { binding, identifier: new_identifier.clone(), renamed: HashSet::new() };
    let mut tracked = None;
    for name in &linked {
        if renamer.renamed.contains(&name.id) {
            continue;
        }
        let new_name = rw.new_simple_name(&new_identifier);
        if name.id == covering.id {
            tracked = Some(new_name);
        }
        let parent = name.parent()?;
        if name.location_is("name") && parent.is(NodeKind::SingleVariableDeclaration) {
            rw.replace(RNode::Orig(name.id), Some(new_name));
        } else if name.location_is("leftHandSide") && parent.is(NodeKind::Assignment) {
            let expression = parent.child("rightHandSide")?;
            let (start, end) = (expression.start(), expression.start() + expression.length());
            let overlap: Vec<NodeId> = linked.iter().filter(|n| start <= n.start() && n.start() < end).map(|n| n.id).collect();
            let inversed = inversed_expression(&mut rw, expression, &mut Some(&mut renamer));
            if overlap.iter().any(|id| !renamer.renamed.contains(id)) {
                return None;
            }
            let replacement_operator = match parent.simple("operator") {
                Some("&=") => Some("|="),
                Some("|=") => Some("&="),
                _ => None,
            };
            if let Some(op) = replacement_operator {
                let assignment = rw.new_assignment(new_name, op, inversed);
                rw.replace(RNode::Orig(parent.id), Some(assignment));
            } else {
                rw.replace(RNode::Orig(expression.id), Some(inversed));
                rw.replace(RNode::Orig(name.id), Some(new_name));
            }
        } else if name.location_is("name") && parent.is(NodeKind::VariableDeclarationFragment) {
            if let Some(initializer) = parent.child("initializer") {
                let inversed = inversed_expression(&mut rw, initializer, &mut None);
                rw.replace(RNode::Orig(initializer.id), Some(inversed));
            }
            rw.replace(RNode::Orig(name.id), Some(new_name));
        } else if parent.is(NodeKind::PrefixExpression) && parent.simple("operator") == Some("!") {
            rw.replace(RNode::Orig(parent.id), Some(new_name));
        } else {
            let not = new_not(&mut rw, new_name);
            rw.replace(RNode::Orig(name.id), Some(not));
        }
    }
    Some((rw, tracked?))
}

/// `LinkedNodeFinder.findByBinding(root, binding)`.
fn find_by_binding<'a>(root: Node<'a>, binding: BindingRef<'_>) -> Vec<Node<'a>> {
    let key = binding.key().to_owned();
    let mut result = Vec::new();
    crate::refactoring::walk(root, &mut |n| {
        if n.is(NodeKind::SimpleName) && n.binding().is_some_and(|b| b.key() == key) {
            result.push(n);
        }
        true
    });
    result
}
