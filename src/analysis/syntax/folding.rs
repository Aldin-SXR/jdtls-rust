//! Folding ranges derived from the tree-sitter CST.

use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};
use tree_sitter::{Node, Tree};

/// Return all folding ranges for a Java file.
pub fn folding_ranges(tree: &Tree, source: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    collect_folds(tree.root_node(), source.as_bytes(), &mut ranges);
    ranges
}

fn collect_folds(node: Node, src: &[u8], out: &mut Vec<FoldingRange>) {
    let kind = node.kind();
    match kind {
        // Block-like constructs
        "class_body"
        | "interface_body"
        | "enum_body"
        | "block"
        | "switch_block"
        | "annotation_type_body"
        | "record_declaration" => {
            push_range(node, None, out);
        }
        // Import group folding
        "import_declaration" => {
            // handled below: we fold consecutive imports as a group
        }
        // Comments
        "block_comment" => {
            push_range(node, Some(FoldingRangeKind::Comment), out);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    // Fold consecutive import_declaration siblings as one region
    let mut i = 0;
    while i < children.len() {
        if children[i].kind() == "import_declaration" {
            let start_line = children[i].start_position().row as u32;
            let mut j = i;
            while j < children.len() && children[j].kind() == "import_declaration" {
                j += 1;
            }
            let end_line = children[j - 1].end_position().row as u32;
            if end_line > start_line {
                out.push(FoldingRange {
                    start_line,
                    start_character: None,
                    end_line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Imports),
                    collapsed_text: None,
                });
            }
            i = j;
        } else {
            collect_folds(children[i], src, out);
            i += 1;
        }
    }
}

fn push_range(node: Node, kind: Option<FoldingRangeKind>, out: &mut Vec<FoldingRange>) {
    let start = node.start_position().row as u32;
    let end = node.end_position().row as u32;
    if end > start {
        out.push(FoldingRange {
            start_line: start,
            start_character: None,
            end_line: end,
            end_character: None,
            kind,
            collapsed_text: None,
        });
    }
}
