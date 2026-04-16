//! Syntax diagnostics derived from tree-sitter parse errors.

use std::collections::HashSet;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use tree_sitter::{Node, Tree};

pub fn collect(tree: &Tree) -> Vec<Diagnostic> {
    let mut raw = Vec::new();
    collect_node(tree.root_node(), &mut raw);

    let mut seen = HashSet::new();
    raw.into_iter()
        .filter(|diag| {
            seen.insert((
                diag.range.start.line,
                diag.range.start.character,
                diag.range.end.line,
                diag.range.end.character,
                diag.message.clone(),
            ))
        })
        .collect()
}

fn collect_node(node: Node, out: &mut Vec<Diagnostic>) {
    if node.is_error() || node.kind() == "ERROR" {
        // Narrow the range to the first leaf token inside the ERROR node so
        // that tree-sitter's error-recovery region (which can span many lines)
        // does not cause the underline to spread past the offending token.
        let range = first_leaf_range(node).unwrap_or_else(|| node_range(node));
        out.push(diagnostic(range, "Syntax error"));
    }

    if node.is_missing() {
        out.push(diagnostic(
            node_range(node),
            format!("Missing {}", node.kind().replace('_', " ")),
        ));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, out);
    }
}

/// Walk down to the first leaf (childless) node and return its range.
/// This pinpoints the offending token rather than the full error-recovery span.
fn first_leaf_range(node: Node) -> Option<Range> {
    if node.child_count() == 0 {
        let r = node_range(node);
        // Ignore zero-width leaves (MISSING nodes produce these).
        if r.start == r.end {
            return None;
        }
        return Some(r);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(r) = first_leaf_range(child) {
            return Some(r);
        }
    }
    None
}

fn diagnostic(range: Range, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("tree-sitter".to_owned()),
        message: message.into(),
        ..Default::default()
    }
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
    fn reports_parse_errors() {
        let tree = parse("class Test { void broken( { }");
        let diagnostics = collect(&tree);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|diag| diag.severity == Some(DiagnosticSeverity::ERROR)));
    }
}
