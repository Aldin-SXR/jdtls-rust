//! `LinkedNodeFinder.findByBinding` / `findByNode` for names that have a
//! binding.

use crate::semantic_ast::{BindingKind, BindingRef, Node, NodeKind};

/// `BindingFinder.getDeclaration`.
fn declaration(binding: BindingRef<'_>) -> BindingRef<'_> {
    match binding.kind() {
        BindingKind::Type => binding.type_declaration().unwrap_or(binding),
        BindingKind::Method => {
            if binding.is_constructor() {
                binding.declaring_class().map(|c| c.type_declaration().unwrap_or(c)).unwrap_or(binding)
            } else {
                binding.method_declaration().unwrap_or(binding)
            }
        }
        BindingKind::Variable => binding.variable_declaration().unwrap_or(binding),
        _ => binding,
    }
}

/// `LinkedNodeFinder.findByBinding(root, binding)`.
pub fn find_by_binding<'a>(root: Node<'a>, binding: BindingRef<'_>) -> Vec<Node<'a>> {
    let wanted = declaration(binding);
    let mut result = Vec::new();
    for node in root.descendants() {
        if !node.is(NodeKind::SimpleName) || node.simple("var") == Some("true") {
            continue;
        }
        let Some(current) = node.binding().map(declaration) else { continue };
        if current.key() == wanted.key() {
            result.push(node);
        } else if current.kind() != wanted.kind() {
            continue;
        } else if current.kind() == BindingKind::Method
            && (wanted.data().method_overrides.contains(&current.id) || current.data().method_overrides.contains(&wanted.id))
        {
            result.push(node);
        }
    }
    result.sort_by_key(|n| n.start());
    result
}

/// `LinkedNodeFinder.findByNode(root, name)` for a name with a binding;
/// otherwise just the name itself.
pub fn find_by_node<'a>(root: Node<'a>, name: Node<'a>) -> Vec<Node<'a>> {
    match name.binding() {
        Some(binding) => find_by_binding(root, binding),
        None => vec![name],
    }
}
