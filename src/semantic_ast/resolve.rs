//! `ASTNodes` / `ASTResolving` helpers (jdt.core.manipulation) over the
//! semantic AST.

use super::{Node, NodeKind};

/// `ASTNodes.getUnparenthesedExpression(node)`.
pub fn unparenthesed_expression(node: Node<'_>) -> Node<'_> {
    let mut n = node;
    while n.is(NodeKind::ParenthesizedExpression) {
        match n.child("expression") {
            Some(e) => n = e,
            None => break,
        }
    }
    n
}

/// `ASTNodes.getNormalizedNode(node)`.
pub fn normalized_node(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    if node.location_is("name") && node.parent().is_some_and(|p| p.is(NodeKind::QualifiedName)) {
        current = node.parent().unwrap();
    }
    if current.location_is("name")
        && current.parent().is_some_and(|p| matches!(p.kind(), NodeKind::QualifiedType | NodeKind::SimpleType | NodeKind::NameQualifiedType))
    {
        current = current.parent().unwrap();
    }
    if current.location_is("type") && current.parent().is_some_and(|p| p.is(NodeKind::ParameterizedType)) {
        current = current.parent().unwrap();
    }
    current
}

/// `ASTResolving.findParentStatement(node)`.
pub fn find_parent_statement(node: Node<'_>) -> Option<Node<'_>> {
    let mut n = node;
    while !n.kind().is_statement() {
        n = n.parent()?;
        if n.kind().is_body_declaration() {
            return None;
        }
    }
    Some(n)
}

/// `ASTResolving.findParentBodyDeclaration(node)`.
pub fn find_parent_body_declaration(node: Node<'_>) -> Option<Node<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_body_declaration() {
            return Some(x);
        }
        n = x.parent();
    }
    None
}

/// `ASTResolving.findParentType(node)`: the enclosing type declaration or
/// anonymous class (excluding `node` itself unless it is one).
pub fn find_parent_type(node: Node<'_>) -> Option<Node<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return Some(x);
        }
        n = x.parent();
    }
    None
}

/// `ASTNodes.getParent(node, kind)` (the node itself excluded).
pub fn parent_of_kind(node: Node<'_>, kind: NodeKind) -> Option<Node<'_>> {
    node.ancestors().find(|a| a.is(kind))
}

/// `AbstractTypeDeclaration.getBodyDeclarationsProperty()` /
/// `AnonymousClassDeclaration.BODY_DECLARATIONS_PROPERTY`.
pub const BODY_DECLARATIONS: &str = "bodyDeclarations";

/// `ASTNode.subtreeMatch(new ASTMatcher(), other)`: structural equality.
pub fn subtree_match(a: Node<'_>, b: Node<'_>) -> bool {
    if a.kind() != b.kind() {
        return false;
    }
    let (pa, pb) = (a.props(), b.props());
    if pa.len() != pb.len() {
        return false;
    }
    for ((na, va), (nb, vb)) in pa.iter().zip(pb.iter()) {
        if na != nb {
            return false;
        }
        use super::PropValue::*;
        let ok = match (va, vb) {
            (Child(None), Child(None)) => true,
            (Child(Some(x)), Child(Some(y))) => subtree_match(a.ast.node(*x), b.ast.node(*y)),
            (List(x), List(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| subtree_match(a.ast.node(*p), b.ast.node(*q))),
            (Simple(x), Simple(y)) => x == y,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}
