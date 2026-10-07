//! Ports of `org.eclipse.jdt.internal.corext.dom.Selection` and
//! `SelectionAnalyzer`.

use crate::semantic_ast::{Node, NodeId, NodeKind};

/// `Selection` visit modes.
pub const INTERSECTS: i32 = 0;
pub const BEFORE: i32 = 1;
pub const SELECTED: i32 = 2;
pub const AFTER: i32 = 3;

/// `Selection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: i64,
    pub length: i64,
}

impl Selection {
    /// `createFromStartLength(s, l)`.
    pub fn from_start_length(start: usize, length: usize) -> Self {
        Selection { start: start as i64, length: length as i64 }
    }

    /// `createFromStartEnd(s, e)` (`e` inclusive).
    pub fn from_start_end(start: usize, end: usize) -> Self {
        Selection { start: start as i64, length: end as i64 - start as i64 + 1 }
    }

    pub fn offset(&self) -> usize {
        self.start as usize
    }

    pub fn exclusive_end(&self) -> i64 {
        self.start + self.length
    }

    pub fn inclusive_end(&self) -> i64 {
        self.exclusive_end() - 1
    }

    fn range(n: Node<'_>) -> (i64, i64) {
        (n.start() as i64, n.end() as i64)
    }

    /// `getVisitSelectionMode(node)`.
    pub fn visit_mode(&self, n: Node<'_>) -> i32 {
        let (s, e) = Self::range(n);
        if e <= self.start {
            BEFORE
        } else if self.covers(n) {
            SELECTED
        } else if self.exclusive_end() <= s {
            AFTER
        } else {
            INTERSECTS
        }
    }

    /// `getEndVisitSelectionMode(node)`.
    pub fn end_visit_mode(&self, n: Node<'_>) -> i32 {
        let (_, e) = Self::range(n);
        if e <= self.start {
            BEFORE
        } else if self.covers(n) {
            SELECTED
        } else if e >= self.exclusive_end() {
            AFTER
        } else {
            INTERSECTS
        }
    }

    /// `covers(int position)`.
    pub fn covers_position(&self, position: i64) -> bool {
        self.start <= position && position < self.start + self.length
    }

    /// `covers(ASTNode)`.
    pub fn covers(&self, n: Node<'_>) -> bool {
        let (s, e) = Self::range(n);
        self.start <= s && e <= self.exclusive_end()
    }

    /// `coveredBy(ASTNode)`.
    pub fn covered_by(&self, n: Node<'_>) -> bool {
        let (s, e) = Self::range(n);
        s <= self.start && self.exclusive_end() <= e
    }

    /// `coveredBy(IRegion)`.
    pub fn covered_by_region(&self, offset: i64, length: i64) -> bool {
        offset <= self.start && self.exclusive_end() <= offset + length
    }

    /// `endsIn(ASTNode)`.
    pub fn ends_in(&self, n: Node<'_>) -> bool {
        let (s, e) = Self::range(n);
        s < self.exclusive_end() && self.exclusive_end() < e
    }

    /// `liesOutside(ASTNode)`.
    pub fn lies_outside(&self, n: Node<'_>) -> bool {
        let (s, e) = Self::range(n);
        e < self.start || self.exclusive_end() < s
    }
}

/// `SelectionAnalyzer` state (`GenericVisitor(true)` over the whole tree).
#[derive(Clone, Debug)]
pub struct SelectionAnalyzer {
    pub selection: Selection,
    pub traverse_selected_node: bool,
    pub last_covering: Option<NodeId>,
    pub selected: Option<Vec<NodeId>>,
}

impl SelectionAnalyzer {
    pub fn new(selection: Selection, traverse_selected_node: bool) -> Self {
        SelectionAnalyzer { selection, traverse_selected_node, last_covering: None, selected: None }
    }

    /// Runs the analyzer over `root` (`root.accept(analyzer)`).
    pub fn analyze(selection: Selection, traverse_selected_node: bool, root: Node<'_>) -> Self {
        let mut sa = SelectionAnalyzer::new(selection, traverse_selected_node);
        sa.run(root);
        sa
    }

    pub fn run(&mut self, root: Node<'_>) {
        let mut f = |n: Node<'_>| self.visit_node(n);
        super::walk(root, &mut f);
    }

    /// `visitNode(node)` (with `handleSelectionEndsIn` returning `false`).
    pub fn visit_node(&mut self, n: Node<'_>) -> bool {
        if self.selection.lies_outside(n) {
            false
        } else if self.selection.covers(n) {
            if self.selected.is_none() {
                self.handle_first_selected_node(n);
            } else {
                self.handle_next_selected_node(n);
            }
            self.traverse_selected_node
        } else if self.selection.covered_by(n) {
            self.last_covering = Some(n.id);
            true
        } else if self.selection.ends_in(n) {
            false
        } else {
            true
        }
    }

    pub fn handle_first_selected_node(&mut self, n: Node<'_>) {
        self.selected = Some(vec![n.id]);
    }

    pub fn handle_next_selected_node(&mut self, n: Node<'_>) {
        let first = self.selected.as_ref().and_then(|s| s.first().copied());
        if let Some(first) = first {
            if n.ast.node(first).parent() == n.parent() {
                self.selected.as_mut().unwrap().push(n.id);
            }
        }
    }

    pub fn has_selected_nodes(&self) -> bool {
        self.selected.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn selected_nodes<'a>(&self, root: Node<'a>) -> Vec<Node<'a>> {
        self.selected.as_ref().map(|s| s.iter().map(|&i| root.ast.node(i)).collect()).unwrap_or_default()
    }

    pub fn first_selected<'a>(&self, root: Node<'a>) -> Option<Node<'a>> {
        self.selected.as_ref().and_then(|s| s.first()).map(|&i| root.ast.node(i))
    }

    pub fn last_selected<'a>(&self, root: Node<'a>) -> Option<Node<'a>> {
        self.selected.as_ref().and_then(|s| s.last()).map(|&i| root.ast.node(i))
    }

    pub fn last_covering_node<'a>(&self, root: Node<'a>) -> Option<Node<'a>> {
        self.last_covering.map(|i| root.ast.node(i))
    }

    /// `isExpressionSelected()`.
    pub fn is_expression_selected(&self, root: Node<'_>) -> bool {
        self.first_selected(root).is_some_and(|n| n.kind().is_expression())
    }

    /// `getSelectedNodeRange()`: `(offset, length)`.
    pub fn selected_node_range(&self, root: Node<'_>) -> Option<(usize, usize)> {
        let first = self.first_selected(root)?;
        let last = self.last_selected(root)?;
        Some((first.start(), last.end() - first.start()))
    }
}

/// Whether `n` is a `Block` (helper for analyzers).
pub fn is_block(n: Node<'_>) -> bool {
    n.is(NodeKind::Block)
}
