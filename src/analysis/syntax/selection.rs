//! Selection range support derived from the tree-sitter AST.

use tower_lsp::lsp_types::{Position, Range, SelectionRange};
use tree_sitter::{Node, Tree};

pub fn selection_range(tree: &Tree, offset: usize) -> Option<SelectionRange> {
    let root = tree.root_node();
    let mut node = node_at_offset(root, offset)?;
    let mut ranges = Vec::new();

    loop {
        if node.is_named() || node == root {
            ranges.push(node_range(node));
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }

    let mut chain = None;
    for range in ranges.into_iter().rev() {
        chain = Some(SelectionRange {
            range,
            parent: chain.map(Box::new),
        });
    }
    chain
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
    fn builds_parent_chain() {
        let source = "class Test { void run() { call(arg); } }";
        let tree = parse(source);
        let offset = source.find("arg").unwrap() + 1;
        let range = selection_range(&tree, offset).expect("selection range");

        assert!(range.parent.is_some());
        assert!(range.parent.as_ref().and_then(|parent| parent.parent.as_ref()).is_some());
    }
}
