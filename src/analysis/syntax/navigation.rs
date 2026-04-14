//! Syntax-only same-file navigation, hover, and reference lookup.

use std::collections::HashSet;
use tower_lsp::lsp_types::{DocumentHighlight, DocumentHighlightKind, Position, Range};
use tree_sitter::{Node, Tree};

type ScopeKey = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeclKind {
    Local,
    Parameter,
    Field,
    Method,
    Constructor,
    Type,
    EnumConstant,
}

#[derive(Clone, Copy)]
struct Decl<'tree> {
    kind: DeclKind,
    decl_node: Node<'tree>,
    name_node: Node<'tree>,
    scope_node: Node<'tree>,
}

pub fn definition_range(tree: &Tree, source: &str, offset: usize) -> Option<Range> {
    let decl = resolve_declaration(tree.root_node(), source, offset)?;
    Some(node_range(decl.name_node))
}

pub fn hover_markdown(tree: &Tree, source: &str, offset: usize) -> Option<String> {
    let decl = resolve_declaration(tree.root_node(), source, offset)?;
    Some(render_decl(source, decl))
}

pub fn references(tree: &Tree, source: &str, offset: usize) -> Vec<Range> {
    let Some(decl) = resolve_declaration(tree.root_node(), source, offset) else {
        return Vec::new();
    };

    let name = node_text(decl.name_node, source);
    let mut ranges = Vec::new();
    collect_identifier_ranges(decl.scope_node, source, name, &mut ranges);
    ranges
}

pub fn document_highlights(tree: &Tree, source: &str, offset: usize) -> Vec<DocumentHighlight> {
    references(tree, source, offset)
        .into_iter()
        .map(|range| DocumentHighlight {
            range,
            kind: Some(DocumentHighlightKind::TEXT),
        })
        .collect()
}

fn resolve_declaration<'tree>(root: Node<'tree>, source: &str, offset: usize) -> Option<Decl<'tree>> {
    let identifier = identifier_at_offset(root, offset)?;

    if let Some(decl) = declaration_from_name_node(identifier) {
        return Some(with_scope(decl));
    }

    let name = node_text(identifier, source);

    if let Some(local_decl) = resolve_local_declaration(root, source, identifier, name) {
        return Some(local_decl);
    }

    if let Some(type_node) = enclosing_type(identifier) {
        if let Some(member_decl) = find_type_member_declaration(type_node, source, name) {
            return Some(with_scope(member_decl));
        }
    }

    find_type_declaration(root, source, name).map(with_scope)
}

fn resolve_local_declaration<'tree>(
    root: Node<'tree>,
    source: &str,
    identifier: Node<'tree>,
    name: &str,
) -> Option<Decl<'tree>> {
    let visible_scopes = visible_scope_chain(root, identifier.start_byte());
    let mut best: Option<Decl<'tree>> = None;

    collect_local_candidates(
        root,
        source,
        name,
        identifier.start_byte(),
        &visible_scopes,
        &mut best,
    );

    best
}

fn collect_local_candidates<'tree>(
    node: Node<'tree>,
    source: &str,
    target_name: &str,
    usage_offset: usize,
    visible_scopes: &HashSet<ScopeKey>,
    best: &mut Option<Decl<'tree>>,
) {
    if node.start_byte() > usage_offset {
        return;
    }

    match node.kind() {
        "formal_parameter" | "spread_parameter" | "catch_formal_parameter" => {
            if let Some(name_node) = declaration_name_node(node) {
                if node_text(name_node, source) == target_name && name_node.end_byte() <= usage_offset {
                    if let Some(scope_node) = local_scope_for_decl(node) {
                        if visible_scopes.contains(&scope_key(scope_node)) {
                            consider_candidate(
                                best,
                                Decl {
                                    kind: DeclKind::Parameter,
                                    decl_node: node,
                                    name_node,
                                    scope_node,
                                },
                            );
                        }
                    }
                }
            }
        }
        "variable_declarator" => {
            if let Some(parent) = node.parent() {
                if parent.kind() == "local_variable_declaration" {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        if node_text(name_node, source) == target_name && name_node.end_byte() <= usage_offset {
                            if let Some(scope_node) = local_scope_for_decl(parent) {
                                if visible_scopes.contains(&scope_key(scope_node)) {
                                    consider_candidate(
                                        best,
                                        Decl {
                                            kind: DeclKind::Local,
                                            decl_node: node,
                                            name_node,
                                            scope_node,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_local_candidates(child, source, target_name, usage_offset, visible_scopes, best);
    }
}

fn consider_candidate<'tree>(best: &mut Option<Decl<'tree>>, candidate: Decl<'tree>) {
    let replace = best
        .map(|current| candidate.name_node.end_byte() >= current.name_node.end_byte())
        .unwrap_or(true);
    if replace {
        *best = Some(candidate);
    }
}

fn with_scope<'tree>(decl: Decl<'tree>) -> Decl<'tree> {
    let scope_node = match decl.kind {
        DeclKind::Local | DeclKind::Parameter => decl.scope_node,
        _ => root_scope(decl.decl_node),
    };
    Decl { scope_node, ..decl }
}

fn declaration_from_name_node<'tree>(node: Node<'tree>) -> Option<Decl<'tree>> {
    let parent = node.parent()?;

    if matches!(
        parent.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    ) && parent.child_by_field_name("name") == Some(node)
    {
        return Some(Decl {
            kind: DeclKind::Type,
            decl_node: parent,
            name_node: node,
            scope_node: parent,
        });
    }

    if parent.kind() == "method_declaration" && parent.child_by_field_name("name") == Some(node) {
        return Some(Decl {
            kind: DeclKind::Method,
            decl_node: parent,
            name_node: node,
            scope_node: parent,
        });
    }

    if parent.kind() == "constructor_declaration" && parent.child_by_field_name("name") == Some(node) {
        return Some(Decl {
            kind: DeclKind::Constructor,
            decl_node: parent,
            name_node: node,
            scope_node: parent,
        });
    }

    if parent.kind() == "enum_constant" && parent.child_by_field_name("name") == Some(node) {
        return Some(Decl {
            kind: DeclKind::EnumConstant,
            decl_node: parent,
            name_node: node,
            scope_node: parent,
        });
    }

    if parent.kind() == "variable_declarator" && parent.child_by_field_name("name") == Some(node) {
        let owner = parent.parent()?;
        if owner.kind() == "field_declaration" {
            return Some(Decl {
                kind: DeclKind::Field,
                decl_node: parent,
                name_node: node,
                scope_node: owner,
            });
        }
        if owner.kind() == "local_variable_declaration" {
            return Some(Decl {
                kind: DeclKind::Local,
                decl_node: parent,
                name_node: node,
                scope_node: owner,
            });
        }
    }

    if matches!(parent.kind(), "formal_parameter" | "spread_parameter" | "catch_formal_parameter")
        && declaration_name_node(parent) == Some(node)
    {
        return Some(Decl {
            kind: DeclKind::Parameter,
            decl_node: parent,
            name_node: node,
            scope_node: parent,
        });
    }

    None
}

fn find_type_member_declaration<'tree>(type_node: Node<'tree>, source: &str, name: &str) -> Option<Decl<'tree>> {
    let body = type_node.child_by_field_name("body")?;
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "field_declaration" => {
                let mut vars = child.walk();
                for node in child.children(&mut vars) {
                    if node.kind() == "variable_declarator" {
                        if let Some(name_node) = node.child_by_field_name("name") {
                            if node_text(name_node, source) == name {
                                return Some(Decl {
                                    kind: DeclKind::Field,
                                    decl_node: node,
                                    name_node,
                                    scope_node: body,
                                });
                            }
                        }
                    }
                }
            }
            "method_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if node_text(name_node, source) == name {
                        return Some(Decl {
                            kind: DeclKind::Method,
                            decl_node: child,
                            name_node,
                            scope_node: body,
                        });
                    }
                }
            }
            "constructor_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if node_text(name_node, source) == name {
                        return Some(Decl {
                            kind: DeclKind::Constructor,
                            decl_node: child,
                            name_node,
                            scope_node: body,
                        });
                    }
                }
            }
            "enum_constant" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if node_text(name_node, source) == name {
                        return Some(Decl {
                            kind: DeclKind::EnumConstant,
                            decl_node: child,
                            name_node,
                            scope_node: body,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    None
}

fn find_type_declaration<'tree>(root: Node<'tree>, source: &str, name: &str) -> Option<Decl<'tree>> {
    if is_type_declaration(root) {
        if let Some(name_node) = root.child_by_field_name("name") {
            if node_text(name_node, source) == name {
                return Some(Decl {
                    kind: DeclKind::Type,
                    decl_node: root,
                    name_node,
                    scope_node: root,
                });
            }
        }
    }

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(found) = find_type_declaration(child, source, name) {
            return Some(found);
        }
    }

    None
}

fn collect_identifier_ranges(node: Node, source: &str, name: &str, out: &mut Vec<Range>) {
    if node.kind() == "identifier" && node_text(node, source) == name {
        out.push(node_range(node));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_identifier_ranges(child, source, name, out);
    }
}

fn render_decl(source: &str, decl: Decl<'_>) -> String {
    let signature = match decl.kind {
        DeclKind::Local | DeclKind::Field => {
            let type_node = decl
                .decl_node
                .parent()
                .and_then(|parent| parent.child_by_field_name("type"));
            match type_node {
                Some(type_node) => format!("{} {}", node_text(type_node, source), node_text(decl.name_node, source)),
                None => node_text(decl.name_node, source).to_owned(),
            }
        }
        DeclKind::Parameter => {
            let type_node = decl.decl_node.child_by_field_name("type");
            match type_node {
                Some(type_node) => format!("{} {}", node_text(type_node, source), node_text(decl.name_node, source)),
                None => node_text(decl.name_node, source).to_owned(),
            }
        }
        DeclKind::Method => format_method_signature(source, decl.decl_node, false),
        DeclKind::Constructor => format_method_signature(source, decl.decl_node, true),
        DeclKind::Type => format_type_signature(source, decl.decl_node),
        DeclKind::EnumConstant => node_text(decl.name_node, source).to_owned(),
    };

    format!("```java\n{signature}\n```")
}

fn format_method_signature(source: &str, node: Node, is_constructor: bool) -> String {
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(name, source))
        .unwrap_or("");
    let params = node
        .child_by_field_name("parameters")
        .map(|params| node_text(params, source))
        .unwrap_or("()");
    if is_constructor {
        format!("{name}{params}")
    } else {
        let ret = node
            .child_by_field_name("type")
            .map(|ret| node_text(ret, source))
            .unwrap_or("void");
        format!("{ret} {name}{params}")
    }
}

fn format_type_signature(source: &str, node: Node) -> String {
    let kind = match node.kind() {
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "enum_declaration" => "enum",
        "record_declaration" => "record",
        "annotation_type_declaration" => "@interface",
        _ => "type",
    };
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(name, source))
        .unwrap_or("");
    format!("{kind} {name}")
}

fn visible_scope_chain(root: Node, offset: usize) -> HashSet<ScopeKey> {
    let Some(mut node) = identifier_at_offset(root, offset) else {
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

fn local_scope_for_decl(node: Node) -> Option<Node> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "method_declaration" | "constructor_declaration" | "lambda_expression" | "block"
        ) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn root_scope(node: Node) -> Node {
    let mut current = node;
    while let Some(parent) = current.parent() {
        current = parent;
    }
    current
}

fn enclosing_type(node: Node) -> Option<Node> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_type_declaration(candidate) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn declaration_name_node(node: Node) -> Option<Node> {
    node.child_by_field_name("name").or_else(|| last_identifier(node))
}

fn identifier_at_offset(root: Node, offset: usize) -> Option<Node> {
    if root.end_byte() == 0 {
        return None;
    }

    let clamped = offset.min(root.end_byte().saturating_sub(1));
    let mut node = root
        .named_descendant_for_byte_range(clamped, clamped)
        .or_else(|| root.descendant_for_byte_range(clamped, clamped))?;

    if node.kind() == "identifier" {
        return Some(node);
    }

    loop {
        if node.kind() == "identifier" {
            return Some(node);
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }

    None
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

fn is_type_declaration(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

fn scope_key(node: Node) -> ScopeKey {
    (node.start_byte(), node.end_byte())
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn node_range(node: Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start: Position {
            line: start.row as u32,
            character: start.column as u32,
        },
        end: Position {
            line: end.row as u32,
            character: end.column as u32,
        },
    }
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
    fn resolves_local_definition() {
        let source = "class Test { void run(String arg) { int value = arg.length(); value++; } }";
        let tree = parse(source);
        let usage = source.rfind("value++").unwrap() + 1;
        let range = definition_range(&tree, source, usage).expect("definition");
        let decl_start = source.find("value =").unwrap() as u32;

        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, decl_start);
    }

    #[test]
    fn renders_method_hover() {
        let source = "class Test { String greet(String name) { return name; } void run() { greet(\"x\"); } }";
        let tree = parse(source);
        let usage = source.rfind("greet(\"x\")").unwrap() + 1;
        let hover = hover_markdown(&tree, source, usage).expect("hover");

        assert!(hover.contains("String greet(String name)"));
    }

    #[test]
    fn finds_same_file_references() {
        let source = "class Test { void run() { int value = 1; value++; System.out.println(value); } }";
        let tree = parse(source);
        let usage = source.rfind("value);").unwrap() + 1;
        let refs = references(&tree, source, usage);

        assert_eq!(refs.len(), 3);
    }
}
