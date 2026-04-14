//! Document symbols (outline) from the tree-sitter CST.

use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind, Position};
use tree_sitter::{Node, Tree};

pub fn document_symbols(tree: &Tree, source: &str) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    collect_symbols(tree.root_node(), source, &mut out);
    out
}

fn collect_symbols(node: Node, src: &str, out: &mut Vec<DocumentSymbol>) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "enum_declaration"
        | "annotation_type_declaration" | "record_declaration" => {
            if let Some(sym) = decl_symbol(node, src) {
                out.push(sym);
            }
            return; // children handled inside decl_symbol
        }
        _ => {}
    }
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        collect_symbols(child, src, out);
    }
}

fn decl_symbol(node: Node, src: &str) -> Option<DocumentSymbol> {
    let kind = match node.kind() {
        "class_declaration" | "record_declaration" => SymbolKind::CLASS,
        "interface_declaration" => SymbolKind::INTERFACE,
        "enum_declaration" => SymbolKind::ENUM,
        "annotation_type_declaration" => SymbolKind::INTERFACE,
        _ => return None,
    };

    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, src).to_owned();

    let range = node_range(node);
    let selection_range = node_range(name_node);

    let mut children = Vec::new();

    // Look inside the body for members
    let body_field = node.child_by_field_name("body");
    if let Some(body) = body_field {
        let mut cur = body.walk();
        for child in body.children(&mut cur) {
            match child.kind() {
                "class_declaration" | "interface_declaration" | "enum_declaration"
                | "annotation_type_declaration" | "record_declaration" => {
                    if let Some(s) = decl_symbol(child, src) {
                        children.push(s);
                    }
                }
                "method_declaration" | "constructor_declaration" => {
                    if let Some(s) = method_symbol(child, src) {
                        children.push(s);
                    }
                }
                "field_declaration" => {
                    for s in field_symbols(child, src) {
                        children.push(s);
                    }
                }
                "enum_constant" => {
                    if let Some(s) = enum_constant_symbol(child, src) {
                        children.push(s);
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() { None } else { Some(children) },
    })
}

fn method_symbol(node: Node, src: &str) -> Option<DocumentSymbol> {
    let kind = match node.kind() {
        "method_declaration" => SymbolKind::METHOD,
        "constructor_declaration" => SymbolKind::CONSTRUCTOR,
        _ => return None,
    };
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, src).to_owned();
    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: node_range(node),
        selection_range: node_range(name_node),
        children: None,
    })
}

fn field_symbols(node: Node, src: &str) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        if child.kind() == "variable_declarator" {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = node_text(name_node, src).to_owned();
                #[allow(deprecated)]
                out.push(DocumentSymbol {
                    name,
                    detail: None,
                    kind: SymbolKind::FIELD,
                    tags: None,
                    deprecated: None,
                    range: node_range(child),
                    selection_range: node_range(name_node),
                    children: None,
                });
            }
        }
    }
    out
}

fn enum_constant_symbol(node: Node, src: &str) -> Option<DocumentSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, src).to_owned();
    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: None,
        kind: SymbolKind::ENUM_MEMBER,
        tags: None,
        deprecated: None,
        range: node_range(node),
        selection_range: node_range(name_node),
        children: None,
    })
}

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn node_range(node: Node) -> Range {
    let s = node.start_position();
    let e = node.end_position();
    Range {
        start: Position { line: s.row as u32, character: s.column as u32 },
        end: Position { line: e.row as u32, character: e.column as u32 },
    }
}
