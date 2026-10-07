//! `StubUtility` variable name suggestions for new parameters
//! (`VK_PARAMETER`).

use std::collections::BTreeMap;

use crate::features::completion::naming;
use crate::semantic_ast::{BindingRef, Node, NodeKind};

const KNOWN_METHOD_NAME_PREFIXES: [&str; 17] = [
    "get", "is", "to", "create", "load", "find", "build", "generate", "prepare", "parse", "current", "read", "resolve", "retrieve", "make",
    "add", "extract",
];

/// `ASTNodes.getTypeName` of the type created for `binding` (element type
/// name without type arguments, and the array dimensions).
pub fn type_base_name(binding: BindingRef<'_>) -> Option<(String, usize)> {
    let mut t = binding;
    let mut dims = 0;
    if t.is_array() {
        dims = t.dimensions().max(0) as usize;
        t = t.element_type()?;
    }
    let t = super::types::erasure(t);
    let name = super::types::declaration(t).name();
    if name.is_empty() {
        return None;
    }
    Some((name.to_owned(), dims))
}

/// `StubUtility.getBaseNameFromExpression(project, expression, VK_PARAMETER)`.
pub fn base_name_from_expression(expression: Node<'_>) -> Option<String> {
    let mut e = expression;
    if e.is(NodeKind::CastExpression) {
        e = e.child("expression")?;
    }
    let name = match e.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName => {
            if let Some(b) = e.binding().filter(|b| b.is_variable()) {
                return Some(b.name().to_owned());
            }
            let simple = if e.is(NodeKind::QualifiedName) { e.child("name")? } else { e };
            return Some(simple.identifier());
        }
        NodeKind::MethodInvocation => {
            let name = e.child("name")?.identifier();
            if name == "next" {
                let receiver = e.child("expression");
                let modified = match receiver {
                    Some(r) if r.is(NodeKind::SimpleName) => modify_base_name(&r.identifier()),
                    _ => "element".to_owned(),
                };
                if modified != "element" {
                    return Some(modified);
                }
            }
            name
        }
        NodeKind::SuperMethodInvocation => e.child("name")?.identifier(),
        NodeKind::FieldAccess => return Some(e.child("name")?.identifier()),
        _ => return None,
    };
    for prefix in KNOWN_METHOD_NAME_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            if rest.chars().next().is_some_and(char::is_uppercase) {
                return Some(rest.to_owned());
            }
        }
    }
    Some(name)
}

/// `ConvertLoopOperation.modifyBaseName`.
fn modify_base_name(name: &str) -> String {
    let lower = name.to_lowercase();
    for (plural, singular) in [("ies", "y"), ("es", ""), ("s", "")] {
        if lower.ends_with(plural) && name.len() > plural.len() {
            let _ = singular;
        }
    }
    // Iterator receivers (`it.next()`) give no useful name.
    if name.len() > 1 && name.ends_with('s') {
        return name[..name.len() - 1].to_owned();
    }
    "element".to_owned()
}

/// `StubUtility.getBaseNameFromLocationInParent(expression)`.
fn base_name_from_location_in_parent(expression: Node<'_>) -> Option<String> {
    if !expression.location_is("arguments") {
        return None;
    }
    let parent = expression.parent()?;
    if !matches!(
        parent.kind(),
        NodeKind::MethodInvocation
            | NodeKind::ClassInstanceCreation
            | NodeKind::SuperMethodInvocation
            | NodeKind::ConstructorInvocation
            | NodeKind::SuperConstructorInvocation
    ) {
        return None;
    }
    let binding = parent.method_binding()?;
    let arguments = parent.list("arguments");
    let params = binding.parameter_types();
    if params.len() != arguments.len() {
        return None;
    }
    let index = arguments.iter().position(|a| *a == expression)?;
    if let Some(t) = expression.type_binding() {
        if !super::types::can_assign(t, params[index]) {
            return None;
        }
    }
    let declaration = super::types::method_declaration(binding);
    if !declaration.is_from_source() && declaration.data().source_offset < 0 {
        return None;
    }
    declaration.data().parameter_names.get(index).cloned()
}

/// `NamingConventions.suggestVariableNames(VK_PARAMETER, BK_TYPE_NAME, base, dims, excluded, evaluateDefault)`.
pub fn suggestions(base: &str, dims: usize, excluded: &[String], options: &BTreeMap<String, String>, evaluate_default: bool) -> Vec<String> {
    let mut names = naming::suggest_names_with_affixes(base, dims, excluded, options, "argument");
    if names.is_empty() && evaluate_default {
        names = naming::suggest_names_with_affixes("name", 0, excluded, options, "argument");
    }
    names
}

/// `StubUtility.getVariableNameSuggestions(VK_PARAMETER, project, Type, argNode, takenNames)[0]`.
pub fn parameter_name(argument: Node<'_>, type_name: Option<(String, usize)>, taken: &[String], options: &BTreeMap<String, String>) -> String {
    let mut res: Vec<String> = Vec::new();
    let mut add = |names: Vec<String>, res: &mut Vec<String>| {
        for n in names {
            if !res.contains(&n) {
                res.push(n);
            }
        }
    };
    if let Some(base) = base_name_from_expression(argument) {
        add(suggestions(&base, 0, taken, options, false), &mut res);
    }
    if let Some(base) = base_name_from_location_in_parent(argument) {
        add(suggestions(&base, 0, taken, options, false), &mut res);
    }
    if let Some((base, dims)) = type_name {
        add(suggestions(&base, dims, taken, options, false), &mut res);
    }
    if let Some(first) = res.into_iter().next() {
        return first;
    }
    // getDefaultVariableNameSuggestions
    let mut name = "x".to_owned();
    let mut i = 1;
    while taken.contains(&name) {
        name = format!("x{i}");
        i += 1;
    }
    name
}

/// `StubUtility.suggestArgumentName(project, baseName, excluded)`.
pub fn suggest_argument_name(base: &str, excluded: &[String], options: &BTreeMap<String, String>) -> String {
    suggestions(base, 0, excluded, options, true).into_iter().next().unwrap_or_else(|| base.to_owned())
}

/// `StubUtility.getArgumentNameSuggestions(project, type, excluded)`.
pub fn argument_name_suggestions(binding: BindingRef<'_>, excluded: &[String], options: &BTreeMap<String, String>) -> Vec<String> {
    match type_base_name(binding) {
        Some((base, dims)) => suggestions(&base, dims, excluded, options, true),
        None => suggestions("name", 0, excluded, options, true),
    }
}

/// `UnresolvedElementsBaseSubProcessor.getExpressionBaseName`.
pub fn expression_base_name(expression: Node<'_>) -> Option<String> {
    let binding = resolve_expression_binding(expression);
    if let Some(b) = binding.filter(|b| b.is_variable()) {
        return Some(b.name().to_owned());
    }
    if expression.is(NodeKind::SimpleName) {
        return Some(expression.identifier());
    }
    None
}

/// `Bindings.resolveExpressionBinding(expression, allowTypeBinding)`.
pub fn resolve_expression_binding(expression: Node<'_>) -> Option<BindingRef<'_>> {
    match expression.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName => expression.binding(),
        NodeKind::FieldAccess | NodeKind::SuperFieldAccess => expression.child("name").and_then(|n| n.binding()),
        NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation => expression.method_binding(),
        NodeKind::ParenthesizedExpression => expression.child("expression").and_then(resolve_expression_binding),
        _ => None,
    }
}
