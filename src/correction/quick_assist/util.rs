//! Helpers shared by the quick assists (`JavaModelUtil`, `ASTNodeFactory`,
//! `StubUtility2Core`).

use std::collections::BTreeMap;

use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{BindingRef, Node, NodeKind};

/// `JavaModelUtil.isVersionLessThan(compliance, version)` negated.
pub fn compliance_at_least(options: &BTreeMap<String, String>, version: &str) -> bool {
    let compliance = options.get("org.eclipse.jdt.core.compiler.compliance").map(String::as_str).unwrap_or("1.8");
    crate::project::compare_java_versions(compliance, version) != std::cmp::Ordering::Less
}

pub fn is_11_or_higher(options: &BTreeMap<String, String>) -> bool {
    compliance_at_least(options, "11")
}

/// `Type.isVar()`.
pub fn is_var_type(t: Node<'_>) -> bool {
    t.is(NodeKind::SimpleType) && t.list("annotations").is_empty() && t.child("name").is_some_and(|n| n.is(NodeKind::SimpleName) && n.identifier() == "var")
}

/// `StubUtility2Core.replaceWildcardsAndCaptures`.
pub fn replace_wildcards_and_captures(mut t: BindingRef<'_>) -> BindingRef<'_> {
    while t.is_wildcard_type() || t.is_capture() || (t.is_array() && t.element_type().is_some_and(|e| e.is_capture())) {
        match t.bound().or_else(|| t.erasure()) {
            Some(next) if next != t => t = next,
            _ => break,
        }
    }
    t
}

/// `ASTNodeFactory.parenthesizeIfNeeded`.
pub fn parenthesize_if_needed(rw: &mut ASTRewrite, expression: RNode) -> RNode {
    use NodeKind::*;
    match rw.kind(expression) {
        AnnotationTypeDeclaration | AnnotationTypeMemberDeclaration | AnonymousClassDeclaration | ArrayAccess | ArrayCreation | ArrayInitializer
        | BooleanLiteral | CharacterLiteral | ClassInstanceCreation | CreationReference | ExpressionMethodReference | FieldAccess | MemberRef
        | MethodInvocation | MethodRef | NullLiteral | NumberLiteral | ParenthesizedExpression | PostfixExpression | PrefixExpression
        | QualifiedName | SimpleName | StringLiteral | SuperFieldAccess | SuperMethodInvocation | SuperMethodReference | ThisExpression
        | TypeLiteral | TypeMethodReference | VariableDeclarationExpression => expression,
        _ => rw.new_parenthesized_expression(expression),
    }
}

/// `ASTNodes.getLeadingComments`.
pub fn leading_comments<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let ast = node.ast;
    ast.comments
        .iter()
        .map(|&id| ast.node(id))
        .filter(|c| c.start() >= node.extended_start() && c.end() < node.start())
        .collect()
}

/// `ASTNodes.getTrailingComments`.
pub fn trailing_comments<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let ast = node.ast;
    let extended_end = node.extended_start() + node.extended_length();
    ast.comments
        .iter()
        .map(|&id| ast.node(id))
        .filter(|c| c.start() > node.start() && c.start() < extended_end)
        .collect()
}

/// `QuickAssistProcessorUtil.getIndex(offset, statements)`.
pub fn statement_index(offset: usize, statements: &[Node<'_>]) -> i64 {
    for (i, s) in statements.iter().enumerate() {
        if offset <= s.start() {
            return i as i64;
        }
        if offset < s.end() {
            return -1;
        }
    }
    statements.len() as i64
}
