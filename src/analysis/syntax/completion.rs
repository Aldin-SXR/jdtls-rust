//! Syntax-only completion items sourced from the current tree-sitter tree.

use std::collections::{BTreeMap, HashSet};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};
use tree_sitter::{Node, Tree};

type ScopeKey = (usize, usize);

pub fn local_completions(tree: &Tree, source: &str, offset: usize) -> Vec<CompletionItem> {
    let prefix = identifier_prefix(source, offset);
    let visible_scopes = visible_scope_chain(tree, offset);
    let mut items = BTreeMap::new();

    collect_visible_declarations(tree.root_node(), source, offset, &visible_scopes, &mut items);

    items
        .into_iter()
        .filter(|(label, _)| matches_prefix(label, &prefix))
        .map(|(_, item)| item)
        .collect()
}

fn collect_visible_declarations(
    node: Node,
    source: &str,
    offset: usize,
    visible_scopes: &HashSet<ScopeKey>,
    out: &mut BTreeMap<String, CompletionItem>,
) {
    if node.start_byte() > offset {
        return;
    }

    match node.kind() {
        "formal_parameter" | "spread_parameter" | "catch_formal_parameter" => {
            if let Some(scope) = enclosing_scope(node) {
                if visible_scopes.contains(&scope) {
                    maybe_add_name(
                        declaration_name_node(node),
                        source,
                        offset,
                        CompletionItemKind::VARIABLE,
                        "Parameter",
                        out,
                    );
                }
            }
        }
        "local_variable_declaration" => {
            if let Some(scope) = enclosing_scope(node) {
                if visible_scopes.contains(&scope) {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "variable_declarator" {
                            maybe_add_name(
                                child.child_by_field_name("name"),
                                source,
                                offset,
                                CompletionItemKind::VARIABLE,
                                "Local",
                                out,
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_visible_declarations(child, source, offset, visible_scopes, out);
    }
}

fn maybe_add_name(
    name_node: Option<Node>,
    source: &str,
    offset: usize,
    kind: CompletionItemKind,
    detail: &str,
    out: &mut BTreeMap<String, CompletionItem>,
) {
    let Some(name_node) = name_node else {
        return;
    };

    if name_node.end_byte() > offset {
        return;
    }

    let label = name_node
        .utf8_text(source.as_bytes())
        .ok()
        .map(str::to_owned);
    let Some(label) = label else {
        return;
    };

    out.entry(label.clone()).or_insert_with(|| CompletionItem {
        label: label.clone(),
        kind: Some(kind),
        detail: Some(detail.to_owned()),
        sort_text: Some(format!("0-{label}")),
        ..Default::default()
    });
}

fn declaration_name_node(node: Node) -> Option<Node> {
    node.child_by_field_name("name")
        .or_else(|| last_identifier(node))
}

fn visible_scope_chain(tree: &Tree, offset: usize) -> HashSet<ScopeKey> {
    let root = tree.root_node();
    let Some(mut node) = node_at_offset(root, offset) else {
        return HashSet::new();
    };

    let mut scopes = HashSet::new();
    loop {
        if is_scope(node) {
            scopes.insert(scope_key(node));
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }
    scopes
}

fn node_at_offset(root: Node, offset: usize) -> Option<Node> {
    if root.end_byte() == 0 {
        return Some(root);
    }

    let clamped = offset.min(root.end_byte().saturating_sub(1));
    root.named_descendant_for_byte_range(clamped, clamped)
        .or_else(|| root.descendant_for_byte_range(clamped, clamped))
        .or(Some(root))
}

fn enclosing_scope(node: Node) -> Option<ScopeKey> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_scope(candidate) {
            return Some(scope_key(candidate));
        }
        current = candidate.parent();
    }
    None
}

fn scope_key(node: Node) -> ScopeKey {
    (node.start_byte(), node.end_byte())
}

fn is_scope(node: Node) -> bool {
    matches!(
        node.kind(),
        "program"
            | "compilation_unit"
            | "class_body"
            | "interface_body"
            | "enum_body"
            | "annotation_type_body"
            | "block"
            | "switch_block"
            | "method_declaration"
            | "constructor_declaration"
            | "lambda_expression"
            | "catch_clause"
            | "for_statement"
            | "enhanced_for_statement"
    )
}

fn identifier_prefix(source: &str, offset: usize) -> String {
    let Some(prefix_end) = source.get(..offset) else {
        return String::new();
    };

    let start = prefix_end
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!is_java_identifier_part(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);

    prefix_end[start..].to_owned()
}

fn matches_prefix(label: &str, prefix: &str) -> bool {
    prefix.is_empty() || label.starts_with(prefix)
}

fn is_java_identifier_part(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

fn last_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    for child in children.into_iter().rev() {
        if child.kind() == "identifier" {
            return Some(child);
        }
        if let Some(identifier) = last_identifier(child) {
            return Some(identifier);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::language())
            .expect("tree-sitter-java init");
        parser.parse(source, None).expect("tree available")
    }

    #[test]
    fn completes_local_variables_in_scope() {
        let source = "class Test { void run(String arg) { int value = 1;  } }";
        let tree = parse(source);
        let offset = source.find("  }").unwrap() + 1;

        let labels: Vec<String> = local_completions(&tree, source, offset)
            .into_iter()
            .map(|item| item.label)
            .collect();

        assert!(labels.iter().any(|label| label == "value"));
        assert!(labels.iter().any(|label| label == "arg"));
    }

    #[test]
    fn filters_local_completions_by_prefix() {
        let source = "class Test { void run(String arg) { int value = 1; val } }";
        let tree = parse(source);
        let offset = source.find("val }").unwrap() + 3;

        let labels: Vec<String> = local_completions(&tree, source, offset)
            .into_iter()
            .map(|item| item.label)
            .collect();

        assert!(labels.iter().any(|label| label == "value"));
        assert!(!labels.iter().any(|label| label == "arg"));
    }
}
