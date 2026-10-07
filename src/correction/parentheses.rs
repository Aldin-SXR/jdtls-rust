//! Port of `NecessaryParenthesesChecker` and `OperatorPrecedence`
//! (jdt.core.manipulation) over original semantic AST nodes.

use crate::semantic_ast::{BindingRef, Node, NodeKind};

/// `OperatorPrecedence.getOperatorPrecedence(operator)`.
pub fn operator_precedence(op: &str) -> i32 {
    match op {
        "||" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "!=" => 7,
        "<" | "<=" | ">" | ">=" => 8,
        "<<" | ">>" | ">>>" => 9,
        "+" | "-" => 10,
        "%" | "/" | "*" => 11,
        _ => i32::MAX,
    }
}

/// `OperatorPrecedence.getExpressionPrecedence(expression)` by node kind
/// and (for infix expressions) operator.
pub fn expression_precedence_of(kind: NodeKind, operator: Option<&str>) -> i32 {
    match kind {
        NodeKind::InfixExpression => operator_precedence(operator.unwrap_or("")),
        NodeKind::Assignment => 0,
        NodeKind::ConditionalExpression => 1,
        NodeKind::InstanceofExpression | NodeKind::PatternInstanceofExpression => 8,
        NodeKind::CastExpression => 12,
        NodeKind::PrefixExpression => 13,
        NodeKind::ClassInstanceCreation
        | NodeKind::FieldAccess
        | NodeKind::MethodInvocation
        | NodeKind::ArrayAccess
        | NodeKind::PostfixExpression => 14,
        _ => i32::MAX,
    }
}

pub fn expression_precedence(e: Node<'_>) -> i32 {
    expression_precedence_of(e.kind(), e.simple("operator"))
}

/// `expressionTypeNeedsParentheses`.
pub fn expression_type_needs_parentheses(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::InfixExpression
            | NodeKind::ConditionalExpression
            | NodeKind::PrefixExpression
            | NodeKind::PostfixExpression
            | NodeKind::CastExpression
            | NodeKind::InstanceofExpression
            | NodeKind::PatternInstanceofExpression
            | NodeKind::ArrayCreation
            | NodeKind::Assignment
            | NodeKind::SwitchExpression
    )
}

fn needs_parentheses_for_switch_expression(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::InfixExpression
            | NodeKind::ConditionalExpression
            | NodeKind::PrefixExpression
            | NodeKind::PostfixExpression
            | NodeKind::CastExpression
            | NodeKind::InstanceofExpression
            | NodeKind::PatternInstanceofExpression
            | NodeKind::ArrayCreation
            | NodeKind::Assignment
            | NodeKind::FieldAccess
            | NodeKind::MethodInvocation
    )
}

/// `locationNeedsParentheses(locationInParent)` for a location `(parent kind, property)`.
pub fn location_needs_parentheses(parent_node: Node<'_>, location: &str) -> bool {
    use NodeKind as K;
    let parent = parent_node.kind();
    if matches!(parent_node.prop(location), Some(crate::semantic_ast::PropValue::List(_))) && location != "extendedOperands" {
        return false;
    }
    !matches!(
        (parent, location),
        (K::VariableDeclarationFragment, "initializer")
            | (K::SingleVariableDeclaration, "initializer")
            | (K::ReturnStatement, "expression")
            | (K::EnhancedForStatement, "expression")
            | (K::ForStatement, "expression")
            | (K::WhileStatement, "expression")
            | (K::DoStatement, "expression")
            | (K::AssertStatement, "expression")
            | (K::AssertStatement, "message")
            | (K::IfStatement, "expression")
            | (K::SwitchStatement, "expression")
            | (K::SwitchCase, "expression")
            | (K::ArrayAccess, "index")
            | (K::ThrowStatement, "expression")
            | (K::SynchronizedStatement, "expression")
            | (K::ParenthesizedExpression, "expression")
    )
}

fn is_string_type(b: Option<BindingRef<'_>>) -> bool {
    b.is_some_and(|b| b.qualified_name() == "java.lang.String")
}

fn is_integer_type(b: Option<BindingRef<'_>>) -> bool {
    b.is_some_and(|b| b.is_primitive() && matches!(b.name(), "int" | "long" | "byte" | "char" | "short"))
}

fn is_associative(op: &str, t: Option<BindingRef<'_>>, same: bool) -> bool {
    match op {
        "+" => is_string_type(t) || is_integer_type(t) && same,
        "*" => is_integer_type(t) && same,
        _ => matches!(op, "&&" | "||" | "&" | "|" | "^"),
    }
}

fn all_operands_same_type<'a>(infix: Node<'a>, left: Option<BindingRef<'a>>, right: Option<BindingRef<'a>>) -> bool {
    let Some(left) = left else { return false };
    if Some(left) != right {
        return false;
    }
    infix.list("extendedOperands").iter().all(|o| o.type_binding() == Some(left))
}

fn infix_type<'a>(op: &str, left: Option<BindingRef<'a>>, right: Option<BindingRef<'a>>) -> Option<BindingRef<'a>> {
    if left == right {
        return left;
    }
    if op == "+" {
        if is_string_type(left) {
            return left;
        }
        if is_string_type(right) {
            return right;
        }
    }
    None
}

fn needs_parentheses_in_infix<'a>(
    expression: Node<'a>,
    parent: Node<'a>,
    location: &str,
    left_operand_type: Option<BindingRef<'a>>,
) -> bool {
    let op = parent.simple("operator").unwrap_or("");
    let (left, right, parent_type) = match left_operand_type {
        None => (
            parent.child("leftOperand").and_then(|n| n.type_binding()),
            parent.child("rightOperand").and_then(|n| n.type_binding()),
            parent.type_binding(),
        ),
        Some(l) => {
            let r = expression.type_binding();
            (Some(l), r, infix_type(op, Some(l), r))
        }
    };
    let same = all_operands_same_type(parent, left, right);
    if location == "leftOperand" {
        return false;
    }
    infix_operand_needs_parentheses(expression, op, left, parent_type, same)
}

/// The associativity part of `needsParenthesesInInfixExpression` for the
/// right operand (or an extended operand).
fn infix_operand_needs_parentheses<'a>(
    expression: Node<'a>,
    op: &str,
    left: Option<BindingRef<'a>>,
    parent_type: Option<BindingRef<'a>>,
    same: bool,
) -> bool {
    if !is_associative(op, parent_type, same) {
        return true;
    }
    if !expression.is(NodeKind::InfixExpression) {
        return false;
    }
    let inner = expression.simple("operator").unwrap_or("");
    if is_string_type(parent_type) {
        if op == "+" && inner == "+" && is_string_type(expression.type_binding()) {
            return !is_string_type(expression.child("leftOperand").and_then(|n| n.type_binding())) && !is_string_type(left);
        }
        return true;
    }
    if op != "*" {
        return false;
    }
    if inner == "*" {
        return false;
    }
    inner == "%" || inner == "/"
}

fn needs_parentheses_for_prefix(parent: Node<'_>, op: &str) -> bool {
    match parent.kind() {
        NodeKind::PrefixExpression => {
            let p = parent.simple("operator").unwrap_or("");
            (p == "+" && (op == "+" || op == "++")) || (p == "-" && (op == "-" || op == "--"))
        }
        NodeKind::InfixExpression => {
            let p = parent.simple("operator").unwrap_or("");
            (p == "+" && (op == "+" || op == "++")) || (p == "-" && (op == "-" || op == "--"))
        }
        NodeKind::CastExpression => !parent.child("type").is_some_and(|t| t.is(NodeKind::PrimitiveType)),
        _ => false,
    }
}

/// `NecessaryParenthesesChecker.needsParentheses(expression, parent, locationInParent)`.
pub fn needs_parentheses<'a>(expression: Node<'a>, parent: Node<'a>, location: &str) -> bool {
    needs_parentheses_impl(expression, parent, location, None)
}

/// `needsParenthesesForRightOperand(rightOperand, infixExpression, leftOperandType)`.
pub fn needs_parentheses_for_right_operand<'a>(right: Node<'a>, infix: Node<'a>, left_type: Option<BindingRef<'a>>) -> bool {
    needs_parentheses_impl(right, infix, "rightOperand", left_type)
}

/// `needsParenthesesForRightOperand(rightOperand, infixExpression, leftOperandType)`
/// where `infixExpression` is a new node (no bindings, no extended operands)
/// with operator `op`.
pub fn needs_parentheses_for_right_operand_of_new_infix<'a>(right: Node<'a>, op: &str, left_type: Option<BindingRef<'a>>) -> bool {
    if !expression_type_needs_parentheses(right.kind()) {
        return false;
    }
    if right.is(NodeKind::SwitchExpression) {
        return needs_parentheses_for_switch_expression(NodeKind::InfixExpression);
    }
    if right.is(NodeKind::PrefixExpression) {
        let inner = right.simple("operator").unwrap_or("");
        return (op == "+" && (inner == "+" || inner == "++")) || (op == "-" && (inner == "-" || inner == "--"));
    }
    if right.is(NodeKind::ArrayCreation) {
        return false;
    }
    let ep = expression_precedence(right);
    let pp = expression_precedence_of(NodeKind::InfixExpression, Some(op));
    if ep != pp {
        return ep < pp;
    }
    // The new infix has no bindings: without a left operand type, nothing is known.
    let (left, parent_type, same) = match left_type {
        None => (None, None, false),
        Some(l) => {
            let r = right.type_binding();
            (Some(l), infix_type(op, Some(l), r), Some(l) == r)
        }
    };
    infix_operand_needs_parentheses(right, op, left, parent_type, same)
}

fn needs_parentheses_impl<'a>(expression: Node<'a>, parent: Node<'a>, location: &str, left_type: Option<BindingRef<'a>>) -> bool {
    if !expression_type_needs_parentheses(expression.kind()) {
        return false;
    }
    if !location_needs_parentheses(parent, location) {
        return false;
    }
    if !parent.kind().is_expression() {
        return true;
    }
    if expression.is(NodeKind::SwitchExpression) {
        return needs_parentheses_for_switch_expression(parent.kind());
    }
    if matches!(expression.kind(), NodeKind::PrefixExpression | NodeKind::PostfixExpression)
        && parent.is(NodeKind::MethodInvocation)
        && location == "expression"
    {
        return true;
    }
    if expression.is(NodeKind::PrefixExpression) {
        return needs_parentheses_for_prefix(parent, expression.simple("operator").unwrap_or(""));
    }
    if expression.is(NodeKind::ArrayCreation) {
        return parent.is(NodeKind::ArrayAccess) && expression.child("initializer").is_none();
    }
    let ep = expression_precedence(expression);
    let pp = expression_precedence(parent);
    if ep > pp {
        return false;
    }
    if ep < pp {
        return true;
    }
    if parent.is(NodeKind::InfixExpression) {
        return needs_parentheses_in_infix(expression, parent, location, left_type);
    }
    parent.is(NodeKind::ConditionalExpression) && location == "expression"
}

/// `canRemoveParentheses(expression)`.
pub fn can_remove_parentheses(expression: Node<'_>) -> bool {
    let (Some(parent), Some(loc)) = (expression.parent(), expression.location()) else { return false };
    expression.is(NodeKind::ParenthesizedExpression)
        && !needs_parentheses(crate::semantic_ast::resolve::unparenthesed_expression(expression), parent, loc)
}

/// NecessaryParenthesesChecker for a newly constructed CastExpression.
pub fn needs_parentheses_for_cast(expression: Node<'_>, primitive: bool) -> bool {
    if !expression_type_needs_parentheses(expression.kind()) {return false;}
    if expression.is(NodeKind::SwitchExpression) {return true;}
    if expression.is(NodeKind::PrefixExpression) {return !primitive;}
    if expression.is(NodeKind::ArrayCreation) {return false;}
    expression_precedence(expression) < 12
}
