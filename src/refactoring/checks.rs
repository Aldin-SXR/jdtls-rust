//! Ports of the `Checks`, `ConstantChecks`, `CodeRefactoringUtil` and
//! `ASTNodes` helpers the extract refactorings use.

use crate::semantic_ast::{modifier, BindingRef, Node, NodeKind};

use super::fragments::Fragment;
use super::{msg, Status};

/// `Checks.checkExpressionIsRValue` results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RValue {
    IsRValue,
    NotRValueMisc,
    NotRValueVoid,
    IsRValueGuessed,
}

/// `ASTResolving.guessBindingForReference(node)`.
pub fn guess_binding_for_reference(n: Node<'_>) -> Option<BindingRef<'_>> {
    crate::correction::type_mismatch::bindings::guess_binding_for_reference(n)
}

/// `Checks.checkExpressionIsRValue(e)`.
pub fn check_expression_is_rvalue(e: Node<'_>) -> RValue {
    if e.kind().is_name() && !e.binding().is_some_and(|b| b.is_variable()) {
        return RValue::NotRValueMisc;
    }
    if e.kind().is_annotation() {
        return RValue::NotRValueMisc;
    }
    let mut guessing = false;
    let mut tb = e.type_binding();
    if tb.is_none() {
        guessing = true;
        tb = guess_binding_for_reference(e);
    }
    match tb {
        None => RValue::NotRValueMisc,
        Some(t) if t.name() == "void" => RValue::NotRValueVoid,
        Some(_) if guessing => RValue::IsRValueGuessed,
        Some(_) => RValue::IsRValue,
    }
}

/// `Checks.isEnumCase(node)`.
pub fn is_enum_case(n: Option<Node<'_>>) -> bool {
    let Some(n) = n.filter(|n| n.is(NodeKind::SwitchCase)) else { return false };
    let is_enum_const = |e: Node<'_>| e.kind().is_name() && e.binding().is_some_and(|b| b.is_variable() && b.is_enum_constant());
    let mut expressions = n.list("expressions");
    if !n.has_prop("expressions") {
        expressions = n.child("expression").into_iter().collect();
    }
    expressions.into_iter().all(is_enum_const)
}

/// `Checks.isInsideJavadoc(node)`.
pub fn is_inside_javadoc(n: Node<'_>) -> bool {
    std::iter::once(n).chain(n.ancestors()).any(|x| x.is(NodeKind::Javadoc))
}

/// `Checks.isExtractableExpression(node)`.
pub fn is_extractable_expression(n: Node<'_>) -> bool {
    if !n.kind().is_expression() {
        return false;
    }
    if n.kind().is_name() {
        return n.binding().is_none_or(|b| b.is_variable());
    }
    true
}

/// `CodeRefactoringUtil.checkMethodSyntaxErrors(start, length, cuNode, message)`.
pub fn check_method_syntax_errors(start: usize, length: usize, root: Node<'_>, invalid_selection_message: &str) -> Status {
    let sa = super::selection::SelectionAnalyzer::analyze(super::selection::Selection::from_start_length(start, length), true, root);
    let Some(covering) = sa.last_covering_node(root) else { return Status::fatal(invalid_selection_message) };
    if !covering.is(NodeKind::Block) || !covering.parent().is_some_and(|p| p.is(NodeKind::MethodDeclaration)) {
        return Status::fatal(invalid_selection_message);
    }
    let has_messages = root.ast.problems.iter().any(|p| p.source_start >= covering.start() as i32 && p.source_end < covering.end() as i32);
    if !has_messages {
        return Status::fatal(invalid_selection_message);
    }
    let name = covering.parent().and_then(|m| m.child("name")).map(|n| n.identifier()).unwrap_or_default();
    Status::fatal(crate::correction::messages::format(msg("CodeRefactoringUtil_error_message"), &[&name]))
}

/// `ITypeBinding.isAssignmentCompatible(target)` (compiler relation
/// exported by the bridge).
pub fn is_assignment_compatible(t: BindingRef<'_>, target: BindingRef<'_>) -> bool {
    t.key() == target.key() || t.data().assignment_targets.iter().any(|b| t.ast.binding(*b).key() == target.key())
}

/// `ASTNodes.hasSemicolon(statement, cu)`: the statement's source ends
/// with `;`.
pub fn has_semicolon(statement: Node<'_>) -> bool {
    let end = statement.end();
    end > 0 && statement.ast.char_at(end - 1) == Some(b';' as u16)
}

/// `ASTNodes.isControlStatementBody(locationInParent)`.
pub fn is_control_statement_body(location: Option<&str>, parent: Option<Node<'_>>) -> bool {
    let Some(parent) = parent else { return false };
    match parent.kind() {
        NodeKind::IfStatement => matches!(location, Some("thenStatement" | "elseStatement")),
        NodeKind::ForStatement | NodeKind::EnhancedForStatement | NodeKind::WhileStatement | NodeKind::DoStatement => location == Some("body"),
        _ => false,
    }
}

/// `Bindings.isVoidType(binding)`.
pub fn is_void(t: Option<BindingRef<'_>>) -> bool {
    t.is_some_and(|t| t.is_primitive() && t.name() == "void" || t.qualified_name() == "void")
}

// ─── ConstantChecks ───────────────────────────────────────────────────────────

/// `ConstantChecks.isLoadTimeConstant(fragment)`.
pub fn is_load_time_constant(f: &Fragment, root: Node<'_>) -> bool {
    load_time_constant(f.node(root.ast))
}

fn load_time_constant(n: Node<'_>) -> bool {
    let mut result = true;
    let mut f = |x: Node<'_>| -> bool {
        match x.kind() {
            NodeKind::Javadoc => false,
            NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation | NodeKind::ThisExpression => {
                result = false;
                false
            }
            NodeKind::FieldAccess => {
                if let Some(e) = x.child("expression") {
                    result &= load_time_constant(e);
                }
                false
            }
            NodeKind::MethodInvocation => {
                match x.child("expression") {
                    None => {
                        if let Some(name) = x.child("name") {
                            result &= check_name(name);
                        }
                    }
                    Some(e) => result &= load_time_constant(e),
                }
                false
            }
            NodeKind::QualifiedName | NodeKind::SimpleName => {
                result &= check_name(x);
                false
            }
            _ => true,
        }
    };
    super::walk(n, &mut f);
    result
}

fn check_name(name: Node<'_>) -> bool {
    let Some(b) = name.binding() else { return true };
    if b.is_variable() || b.is_method() {
        if name.is(NodeKind::SimpleName) {
            return b.modifiers() & modifier::STATIC != 0;
        }
        return name.child("qualifier").is_none_or(check_name);
    }
    if b.is_type() {
        return !b.is_type_variable();
    }
    true
}

/// `ConstantChecks.isStaticFinalConstant(fragment)`.
pub fn is_static_final_constant(f: &Fragment, root: Node<'_>) -> bool {
    let mut result = true;
    let mut v = |x: Node<'_>| -> bool {
        match x.kind() {
            NodeKind::Javadoc => false,
            NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation | NodeKind::ThisExpression => {
                result = false;
                false
            }
            NodeKind::QualifiedName | NodeKind::SimpleName => {
                let Some(b) = x.binding() else { return true };
                let m = b.modifiers();
                if b.is_variable() {
                    if m & modifier::STATIC == 0 || m & modifier::FINAL == 0 {
                        result = false;
                        return false;
                    }
                } else if b.is_method() {
                    if m & modifier::STATIC == 0 {
                        result = false;
                        return false;
                    }
                } else {
                    return false;
                }
                true
            }
            _ => true,
        }
    };
    super::walk(f.node(root.ast), &mut v);
    result
}
