//! Port of jdt.ls `SelectionRangeHandler`: the chain of DOM nodes covering
//! each position (`NodeFinder`), innermost first, preceded by the line or
//! block comment containing the position.

use tower_lsp::lsp_types::{Position, SelectionRange};

use super::dom::{self, Dom};
use super::scanner::LineIndex;

pub fn selection_ranges(src: &str, positions: &[Position]) -> Vec<SelectionRange> {
    if positions.is_empty() {
        return Vec::new();
    }
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_java::language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else { return Vec::new() };
    let ast = dom::build(&tree, src);
    let comments = dom::comments(&tree, src);
    let li = LineIndex::new(src);

    let mut out = Vec::new();
    for pos in positions {
        let offset = li.offset(src, *pos);
        let Some(path) = dom::node_finder(&ast, offset, 0) else { continue };
        let mut selection: Option<SelectionRange> = None;
        for node in &path {
            selection = Some(SelectionRange { range: li.range(src, node.start, node.end), parent: selection.map(Box::new) });
        }
        if let Some(c) = containing_comment(&comments, offset) {
            selection = Some(SelectionRange { range: li.range(src, c.start, c.end), parent: selection.map(Box::new) });
        }
        if let Some(s) = selection {
            out.push(s);
        }
    }
    out
}

fn containing_comment(comments: &[Dom], offset: usize) -> Option<&Dom> {
    comments.iter().find(|c| c.start <= offset && offset <= c.end)
}
