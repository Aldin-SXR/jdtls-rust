//! ASTNodes.getExplicitCast and its target/overload helpers.
use crate::semantic_ast::{modifier, resolve::find_parent_type, BindingRef, Node, NodeKind};
use std::collections::HashSet;
fn assign(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    a == b || a.data().assignment_targets.contains(&b.id)
}
fn unboxed(t: BindingRef<'_>) -> BindingRef<'_> {
    let name = match t.qualified_name() {
        "java.lang.Boolean" => "boolean",
        "java.lang.Byte" => "byte",
        "java.lang.Character" => "char",
        "java.lang.Short" => "short",
        "java.lang.Integer" => "int",
        "java.lang.Long" => "long",
        "java.lang.Float" => "float",
        "java.lang.Double" => "double",
        _ => return t,
    };
    t.ast.type_by_name(name).unwrap_or(t)
}
fn contains_variables(t: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(t.key().into()) {
        return false;
    }
    t.is_type_variable()
        || t.type_arguments()
            .iter()
            .any(|t| contains_variables(*t, seen))
        || t.component_type()
            .is_some_and(|t| contains_variables(t, seen))
        || t.bound().is_some_and(|t| contains_variables(t, seen))
}
fn boxed_receiver(mut node: Node<'_>) -> bool {
    while node
        .parent()
        .is_some_and(|p| p.is(NodeKind::ParenthesizedExpression))
    {
        node = node.parent().unwrap();
    }
    node.location_is("expression")
        && node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                NodeKind::ClassInstanceCreation
                    | NodeKind::FieldAccess
                    | NodeKind::MethodInvocation
            )
        })
}
struct Parent<'a> {
    invocation: Node<'a>,
    method: Option<BindingRef<'a>>,
    index: usize,
    count: usize,
}
fn summary(mut node: Node<'_>) -> Option<Parent<'_>> {
    while node.parent().is_some_and(|p| {
        p.is(NodeKind::ParenthesizedExpression)
            || p.is(NodeKind::ConditionalExpression)
                && matches!(node.location(), Some("thenExpression" | "elseExpression"))
    }) {
        node = node.parent().unwrap();
    }
    let parent = node.parent()?;
    if !node.location_is("arguments")
        || !matches!(
            parent.kind(),
            NodeKind::MethodInvocation
                | NodeKind::SuperMethodInvocation
                | NodeKind::ConstructorInvocation
                | NodeKind::SuperConstructorInvocation
                | NodeKind::ClassInstanceCreation
                | NodeKind::EnumConstantDeclaration
        )
    {
        return None;
    }
    let args = parent.list("arguments");
    Some(Parent {
        invocation: parent,
        method: parent.method_binding(),
        index: args.iter().position(|a| a.id == node.id)?,
        count: args.len(),
    })
}
fn hierarchy<'a>(
    t: BindingRef<'a>,
    interfaces: bool,
    out: &mut Vec<BindingRef<'a>>,
    seen: &mut HashSet<String>,
) {
    if !seen.insert(t.key().into()) {
        return;
    }
    out.push(t);
    if let Some(s) = t.superclass() {
        hierarchy(s, interfaces, out, seen);
    }
    if interfaces {
        for i in t.interfaces() {
            hierarchy(i, true, out, seen);
        }
    }
}
fn invocation_type<'a>(p: &Parent<'a>, method: BindingRef<'a>) -> Option<BindingRef<'a>> {
    if !matches!(
        p.invocation.kind(),
        NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation
    ) {
        return method.declaring_class();
    }
    if let Some(q) = p
        .invocation
        .child(if p.invocation.is(NodeKind::SuperMethodInvocation) {
            "qualifier"
        } else {
            "expression"
        })
    {
        let t = q.type_binding()?;
        return if p.invocation.is(NodeKind::SuperMethodInvocation) {
            t.superclass()
        } else {
            Some(t)
        };
    }
    let mut enclosing = find_parent_type(p.invocation).and_then(|n| n.binding());
    if p.invocation.is(NodeKind::SuperMethodInvocation) {
        enclosing = enclosing.and_then(|t| t.superclass());
    }
    if let Some(enclosing) = enclosing {
        let mut types = Vec::new();
        hierarchy(enclosing, true, &mut types, &mut HashSet::new());
        if types
            .iter()
            .flat_map(|t| t.declared_methods().unwrap_or_default())
            .any(|m| m.name() == method.name() && m.parameter_types() == method.parameter_types())
        {
            return Some(enclosing);
        }
    }
    method.declaring_class()
}
fn parameter(method: BindingRef<'_>, index: usize) -> Option<BindingRef<'_>> {
    let params = method.parameter_types();
    if method.is_varargs() && index >= params.len().checked_sub(1)? {
        params.last().and_then(|p| p.component_type())
    } else {
        params.get(index).copied()
    }
}
/// `ASTNodes.isTargetAmbiguous(expression, expressionIsExplicitlyTyped)`.
pub(crate) fn target_ambiguous(reference: Node<'_>, explicit: bool) -> bool {
    ambiguous_impl(reference, None, None, true, Some(explicit))
}
fn ambiguous(
    reference: Node<'_>,
    initializer: Node<'_>,
    typ: BindingRef<'_>,
    functional: bool,
) -> bool {
    ambiguous_impl(reference, Some(initializer), Some(typ), functional, None)
}
fn ambiguous_impl(
    reference: Node<'_>,
    initializer: Option<Node<'_>>,
    typ: Option<BindingRef<'_>>,
    functional: bool,
    explicit_override: Option<bool>,
) -> bool {
    let Some(p) = summary(reference) else {
        return false;
    };
    let Some(method) = p.method else {
        return true;
    };
    let Some(invocation) = invocation_type(&p, method) else {
        return true;
    };
    let mut types = Vec::new();
    hierarchy(
        invocation,
        functional || invocation.is_interface() || invocation.modifiers() & modifier::ABSTRACT != 0,
        &mut types,
        &mut HashSet::new(),
    );
    let params = method.parameter_types();
    for candidate in types
        .iter()
        .flat_map(|t| t.declared_methods().unwrap_or_default())
    {
        if candidate.name() != method.name() {
            continue;
        }
        if functional {
            if candidate.method_declaration() == method.method_declaration() {
                continue;
            }
            let dc = candidate.declaring_class();
            if dc != method.declaring_class()
                && (candidate.modifiers() & modifier::PRIVATE != 0
                    || dc.is_some_and(|t| t.is_interface())
                        && candidate.modifiers() & modifier::STATIC != 0)
            {
                continue;
            }
            if method.data().method_overrides.contains(&candidate.id) {
                continue;
            }
            let cp = candidate.parameter_types();
            let possible = cp.len() == params.len()
                || (method.is_varargs() || candidate.is_varargs())
                    && p.count >= cp.len().saturating_sub(usize::from(candidate.is_varargs()));
            if !possible {
                continue;
            }
            let Some(cm) = parameter(candidate, p.index)
                .and_then(|t| t.data().functional_method.map(|id| t.ast.binding(id)))
            else {
                continue;
            };
            let explicit = explicit_override.unwrap_or_else(|| {
                initializer.is_some_and(|initializer| {
                    initializer.is(NodeKind::LambdaExpression)
                        && initializer
                            .list("parameters")
                            .first()
                            .is_none_or(|n| n.is(NodeKind::SingleVariableDeclaration))
                })
            });
            if !explicit {
                return true;
            }
            let original = parameter(method, p.index)
                .and_then(|t| t.data().functional_method.map(|id| t.ast.binding(id)));
            if original
                .and_then(|m| m.return_type())
                .map(|t| t.name() == "void")
                == cm.return_type().map(|t| t.name() == "void")
            {
                return true;
            }
        } else {
            if candidate == method {
                continue;
            }
            let cp = candidate.parameter_types();
            if cp.len() != params.len() {
                continue;
            }
            let index = if method.is_varargs() && p.index >= params.len().saturating_sub(1) {
                params.len().saturating_sub(1)
            } else {
                p.index
            };
            let compatible = params
                .iter()
                .enumerate()
                .zip(cp.iter())
                .all(|((i, old), target)| {
                    if i != index {
                        return assign(*old, *target);
                    }
                    if method.is_varargs() && p.index >= params.len().saturating_sub(1) {
                        target.component_type().is_some_and(|c| typ.is_some_and(|typ| assign(typ, c)))
                            || matches!(
                                target.qualified_name(),
                                "java.lang.Object" | "java.lang.Cloneable" | "java.io.Serializable"
                            )
                    } else {
                        typ.is_some_and(|typ| assign(typ, *target))
                    }
                });
            if compatible {
                return true;
            }
        }
    }
    false
}
/// `ASTNodes.getTargetType(expression)`.
pub(crate) fn target(node: Node<'_>) -> Option<BindingRef<'_>> {
    let parent = node.parent()?;
    let loc = node.location()?;
    match (parent.kind(), loc) {
        (
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration,
            "initializer",
        ) => parent.child("name")?.type_binding(),
        (NodeKind::Assignment, "rightHandSide") => parent.child("leftHandSide")?.type_binding(),
        (NodeKind::ReturnStatement, "expression") => {
            for n in parent.ancestors() {
                if n.is(NodeKind::LambdaExpression) {
                    return n.method_binding().and_then(|m| m.return_type());
                }
                if n.is(NodeKind::MethodDeclaration) {
                    return n.binding().and_then(|m| m.return_type());
                }
                if n.kind().is_body_declaration() || n.is(NodeKind::AnonymousClassDeclaration) {
                    return None;
                }
            }
            None
        }
        (
            NodeKind::ParenthesizedExpression | NodeKind::ConditionalExpression,
            "expression" | "thenExpression" | "elseExpression",
        ) if !parent.is(NodeKind::ConditionalExpression) || loc != "expression" => target(parent),
        (NodeKind::CastExpression, "expression") => parent.child("type")?.binding(),
        (NodeKind::ArrayAccess, "index") => node.ast.type_by_name("int"),
        (
            NodeKind::IfStatement
            | NodeKind::WhileStatement
            | NodeKind::DoStatement
            | NodeKind::ConditionalExpression,
            "expression",
        ) => node.ast.type_by_name("boolean"),
        (NodeKind::SwitchStatement, "expression") => node.type_binding().map(|t| {
            if t.is_primitive() || t.is_enum() || t.qualified_name() == "java.lang.String" {
                t
            } else {
                unboxed(t)
            }
        }),
        (NodeKind::LambdaExpression, "body") => {
            parent.method_binding().and_then(|m| m.return_type())
        }
        (_, "arguments") => {
            summary(node).and_then(|s| s.method.and_then(|m| parameter(m, s.index)))
        }
        (NodeKind::ArrayInitializer, "expressions") => {
            let mut p = parent;
            while p.parent().is_some_and(|n| n.is(NodeKind::ArrayInitializer)) {
                p = p.parent().unwrap();
            }
            let owner = p.parent()?;
            if owner.is(NodeKind::ArrayCreation) {
                owner.child("type")?.child("elementType")?.binding()
            } else {
                owner.child("name")?.type_binding()?.element_type()
            }
        }
        _ => None,
    }
}
pub fn explicit_cast<'a>(initializer: Node<'a>, reference: Node<'a>) -> Option<BindingRef<'a>> {
    let (a, b) = (initializer.type_binding()?, reference.type_binding()?);
    if a.is_primitive() && b.is_primitive() && a != b {
        return Some(b);
    }
    if a.is_primitive() && !b.is_primitive() {
        let unboxed = unboxed(b);
        if unboxed != a {
            return Some(unboxed);
        }
        if boxed_receiver(reference) {
            return Some(b);
        }
    } else if !a.is_primitive() && b.is_primitive() {
        if unboxed(a) != b {
            return Some(b);
        }
    } else if a.is_raw_type() && b.is_parameterized_type() {
        return Some(b);
    } else if initializer.is(NodeKind::LambdaExpression) || initializer.kind().is_method_reference()
    {
        if ambiguous(reference, initializer, a, true) || target(reference) != Some(b) {
            return Some(b);
        }
    } else if !assign(a, b) {
        if !contains_variables(b, &mut HashSet::new()) {
            return Some(b);
        }
    } else if a != b && ambiguous(reference, initializer, a, false) {
        return Some(b);
    }
    None
}
