//! Port of `org.eclipse.jdt.core.dom.NodeFinder`.

use super::{Ast, Node, NodeId};

/// `new NodeFinder(root, start, length)`: the covering node (the innermost
/// node that includes the selection) and the covered node (the first node
/// the selection fully includes).
#[derive(Clone, Copy, Debug)]
pub struct NodeFinder<'a> {
    pub covering: Option<Node<'a>>,
    pub covered: Option<Node<'a>>,
}

impl<'a> NodeFinder<'a> {
    pub fn new(root: Node<'a>, start: usize, length: usize) -> Self {
        let ast: &'a Ast = root.ast;
        let f_start = start;
        let f_end = start + length;
        let mut covering: Option<Node<'a>> = None;
        let mut covered: Option<Node<'a>> = None;
        let end = ast.subtree_end(root.id).0;
        let mut i = root.id.0;
        // `NodeFinderVisitor.preVisit2` over the preorder numbering: a node
        // whose visit returns false has its subtree skipped.
        while i < end {
            let node = ast.node(NodeId(i));
            let node_start = node.start();
            let node_end = node_start + node.length();
            let descend = if node_end >= f_start && f_end >= node_start {
                if node_start <= f_start && f_end <= node_end {
                    covering = Some(node);
                }
                if f_start <= node_start && node_end <= f_end {
                    if covering == Some(node) {
                        covered = Some(node);
                        true
                    } else {
                        if covered.is_none() {
                            covered = Some(node);
                        }
                        false
                    }
                } else {
                    true
                }
            } else {
                false
            };
            i = if descend { i + 1 } else { ast.subtree_end(node.id).0 };
        }
        NodeFinder { covering, covered }
    }

    /// `NodeFinder.perform(root, start, length)`.
    pub fn perform(root: Node<'a>, start: usize, length: usize) -> Option<Node<'a>> {
        let f = NodeFinder::new(root, start, length);
        match f.covered {
            Some(c) if c.start() == start && c.length() == length => Some(c),
            _ => f.covering,
        }
    }
}
