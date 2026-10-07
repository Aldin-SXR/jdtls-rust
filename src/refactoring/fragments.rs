//! Port of `org.eclipse.jdt.internal.corext.dom.fragments`
//! (`ASTFragmentFactory`, `SimpleFragment`, `SimpleExpressionFragment`,
//! `AssociativeInfixExpressionFragment`, `ASTMatchingFragmentFinder`) and of
//! `JdtASTMatcher`.

use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Ast, Node, NodeId, NodeKind, PropValue};

use super::selection::{Selection, SelectionAnalyzer};

/// An `IASTFragment`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Fragment {
    /// `SimpleFragment` (`expression == false`) or `SimpleExpressionFragment`.
    Simple { node: NodeId, expression: bool },
    /// `AssociativeInfixExpressionFragment`.
    Infix { root: NodeId, operands: Vec<NodeId> },
}

impl Fragment {
    /// `getAssociatedNode()` (the group root for infix fragments).
    pub fn node<'a>(&self, ast: &'a Ast) -> Node<'a> {
        match self {
            Fragment::Simple { node, .. } => ast.node(*node),
            Fragment::Infix { root, .. } => ast.node(*root),
        }
    }

    /// `instanceof IExpressionFragment`.
    pub fn is_expression(&self) -> bool {
        match self {
            Fragment::Simple { expression, .. } => *expression,
            Fragment::Infix { .. } => true,
        }
    }

    pub fn start(&self, ast: &Ast) -> usize {
        match self {
            Fragment::Simple { node, .. } => ast.node(*node).start(),
            Fragment::Infix { operands, .. } => ast.node(operands[0]).start(),
        }
    }

    pub fn length(&self, ast: &Ast) -> usize {
        match self {
            Fragment::Simple { node, .. } => ast.node(*node).length(),
            Fragment::Infix { operands, .. } => ast.node(*operands.last().unwrap()).end() - self.start(ast),
        }
    }

    /// `matches(other)`.
    pub fn matches(&self, other: &Fragment, ast: &Ast) -> bool {
        match (self, other) {
            (Fragment::Simple { node: a, expression: ea }, Fragment::Simple { node: b, expression: eb }) => {
                ea == eb && do_nodes_match(ast.node(*b), ast.node(*a))
            }
            (Fragment::Infix { root: ra, operands: oa }, Fragment::Infix { root: rb, operands: ob }) => {
                operator(ast.node(*ra)) == operator(ast.node(*rb))
                    && oa.len() == ob.len()
                    && oa.iter().zip(ob.iter()).all(|(x, y)| do_nodes_match(ast.node(*x), ast.node(*y)))
            }
            _ => false,
        }
    }

    /// `getMatchingFragmentsWithNode(node)`.
    fn matching_fragments_with_node(&self, n: Node<'_>) -> Vec<Fragment> {
        let ast = n.ast;
        match self {
            Fragment::Simple { node, .. } => {
                if !do_nodes_match(ast.node(*node), n) {
                    return Vec::new();
                }
                vec![full_subtree(n)]
            }
            Fragment::Infix { .. } => match full_subtree(n) {
                kin @ Fragment::Infix { .. } => kin.sub_fragments_with_my_node_matching(self, ast),
                _ => Vec::new(),
            },
        }
    }

    /// `getSubFragmentsMatching(toMatch)`.
    pub fn sub_fragments_matching(&self, to_match: &Fragment, ast: &Ast) -> Vec<Fragment> {
        match self {
            Fragment::Simple { node, .. } => find_matching_fragments(ast.node(*node), to_match),
            Fragment::Infix { operands, .. } => {
                let mut result = self.sub_fragments_with_my_node_matching(to_match, ast);
                for o in operands {
                    result.extend(find_matching_fragments(ast.node(*o), to_match));
                }
                result
            }
        }
    }

    fn sub_fragments_with_my_node_matching(&self, to_match: &Fragment, ast: &Ast) -> Vec<Fragment> {
        let (Fragment::Infix { root, operands }, Fragment::Infix { root: other_root, operands: other_operands }) = (self, to_match) else {
            return Vec::new();
        };
        if operator(ast.node(*other_root)) != operator(ast.node(*root)) {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut i = 0;
        while i < operands.len() {
            let matches_at = i + other_operands.len() <= operands.len()
                && other_operands.iter().enumerate().all(|(k, o)| do_nodes_match(ast.node(operands[i + k]), ast.node(*o)));
            if matches_at {
                result.push(Fragment::Infix { root: *root, operands: operands[i..i + other_operands.len()].to_vec() });
                i += other_operands.len();
            } else {
                i += 1;
            }
        }
        result
    }

    /// `IExpressionFragment.createCopyTarget(rewrite, removeSurroundingParenthesis)`.
    pub fn create_copy_target(&self, rw: &mut ASTRewrite, remove_surrounding_parenthesis: bool) -> RNode {
        let ast = rw.ast.clone();
        match self {
            Fragment::Simple { node, .. } => {
                let mut n = ast.node(*node);
                if remove_surrounding_parenthesis && n.is(NodeKind::ParenthesizedExpression) {
                    if let Some(e) = n.child("expression") {
                        n = e;
                    }
                }
                rw.create_copy_target(n.id)
            }
            Fragment::Infix { root, operands } => {
                let all = group_members(ast.node(*root));
                if all.len() == operands.len() {
                    return rw.create_copy_target(*root);
                }
                let source = ast.substring(self.start(&ast), self.start(&ast) + self.length(&ast));
                rw.create_string_placeholder(&source, NodeKind::InfixExpression)
            }
        }
    }

    /// `replace(rewrite, replacement, group)`.
    pub fn replace(&self, rw: &mut ASTRewrite, replacement: RNode) {
        let ast = rw.ast.clone();
        let replacement_is_name = rw.kind(replacement).is_name();
        match self {
            Fragment::Simple { node, .. } => {
                let n = ast.node(*node);
                match n.parent() {
                    Some(p) if replacement_is_name && p.is(NodeKind::ParenthesizedExpression) => {
                        rw.replace(RNode::Orig(p.id), Some(replacement));
                    }
                    _ => rw.replace(RNode::Orig(n.id), Some(replacement)),
                }
            }
            Fragment::Infix { root, operands } => {
                let group = ast.node(*root);
                let all = group_members(group);
                if all.len() == operands.len() {
                    match group.parent() {
                        Some(p) if replacement_is_name && p.is(NodeKind::ParenthesizedExpression) => {
                            rw.replace(RNode::Orig(p.id), Some(replacement));
                        }
                        _ => rw.replace(RNode::Orig(group.id), Some(replacement)),
                    }
                    return;
                }
                rw.replace(RNode::Orig(operands[0]), Some(replacement));
                let first = all.iter().position(|n| *n == operands[0]).unwrap_or(0);
                for o in all.iter().take(first + operands.len()).skip(first + 1) {
                    rw.remove(RNode::Orig(*o));
                }
            }
        }
    }
}

fn operator<'a>(n: Node<'a>) -> Option<&'a str> {
    n.simple("operator")
}

fn is_operator_associative(op: Option<&str>) -> bool {
    matches!(op, Some("+" | "*" | "^" | "|" | "&" | "||" | "&&"))
}

fn is_associative_infix(n: Node<'_>) -> bool {
    n.is(NodeKind::InfixExpression) && is_operator_associative(operator(n))
}

fn is_parent_infix_with_same_operator(n: Node<'_>) -> bool {
    n.parent().is_some_and(|p| p.is(NodeKind::InfixExpression) && operator(p) == operator(n))
}

fn find_group_root(mut n: Node<'_>) -> Node<'_> {
    while is_associative_infix(n) && is_parent_infix_with_same_operator(n) {
        n = n.parent().unwrap();
    }
    n
}

/// `GroupMemberFinder`: the operands of an associative infix group, in order.
fn group_members(root: Node<'_>) -> Vec<NodeId> {
    let op = operator(root);
    let mut out = Vec::new();
    let mut f = |n: Node<'_>| {
        if n.is(NodeKind::InfixExpression) && operator(n) == op {
            return true;
        }
        out.push(n.id);
        false
    };
    super::walk(root, &mut f);
    out
}

/// `ASTFragmentFactory.createFragmentForFullSubtree(node)`.
pub fn full_subtree(n: Node<'_>) -> Fragment {
    if is_associative_infix(n) {
        let root = find_group_root(n);
        return Fragment::Infix { root: root.id, operands: group_members(n) };
    }
    Fragment::Simple { node: n.id, expression: n.kind().is_expression() }
}

/// `ASTFragmentFactory.createFragmentForSourceRange(range, scope, cu)`.
pub fn for_source_range(scope: Node<'_>, offset: usize, length: usize) -> Option<Fragment> {
    let sa = SelectionAnalyzer::analyze(Selection::from_start_length(offset, length), false, scope);
    let selected = sa.selected_nodes(scope);
    if selected.len() == 1 && !range_includes_non_whitespace_outside(scope.ast, (offset, length), (selected[0].start(), selected[0].length())) {
        return Some(full_subtree(selected[0]));
    }
    let covering = sa.last_covering_node(scope);
    if length == 0 && selected.is_empty() {
        if let Some(c) = covering {
            return Some(full_subtree(c));
        }
    }
    let node = covering?;
    if node.is(NodeKind::InfixExpression) {
        return sub_part_by_source_range(node, (offset, length));
    }
    None
}

/// `AssociativeInfixExpressionFragment.createSubPartFragmentBySourceRange`.
fn sub_part_by_source_range(node: Node<'_>, range: (usize, usize)) -> Option<Fragment> {
    if covers(range, (node.start(), node.length())) || !covers((node.start(), node.length()), range) {
        return None;
    }
    if !is_associative_infix(node) {
        return None;
    }
    let root = find_group_root(node);
    let members = group_members(root);
    let ast = node.ast;
    let mut sub: Vec<NodeId> = Vec::new();
    let (mut entered, mut exited) = (false, false);
    let (pos, end) = (range.0, range.0 + range.1);
    if pos == ast.node(members[0]).start() {
        entered = true;
    }
    for i in 0..members.len() - 1 {
        let (m, next) = (ast.node(members[i]), ast.node(members[i + 1]));
        if entered {
            sub.push(m.id);
            if m.end() <= end && end <= next.start() {
                exited = true;
                break;
            }
        } else if m.end() <= pos && pos <= next.start() {
            entered = true;
        }
    }
    let last = ast.node(*members.last().unwrap());
    if end == last.end() {
        sub.push(last.id);
        exited = true;
    }
    if !exited || sub.is_empty() {
        return None;
    }
    let first = ast.node(sub[0]);
    let last = ast.node(*sub.last().unwrap());
    if range_includes_non_whitespace_outside(ast, range, (first.start(), last.end() - first.start())) {
        return None;
    }
    if sub.len() < 2 {
        return None;
    }
    Some(Fragment::Infix { root: root.id, operands: sub })
}

/// `Util.covers(thisRange, otherRange)`.
fn covers(this: (usize, usize), other: (usize, usize)) -> bool {
    this.0 <= other.0 && this.0 as i64 + this.1 as i64 - 1 >= other.0 as i64 + other.1 as i64 - 1
}

/// `Util.rangeIncludesNonWhitespaceOutsideRange(selection, nodes, buffer)`.
pub fn range_includes_non_whitespace_outside(ast: &Ast, selection: (usize, usize), nodes: (usize, usize)) -> bool {
    if !covers(selection, nodes) {
        return false;
    }
    if !is_just_whitespace(ast, selection.0, nodes.0) {
        return true;
    }
    !is_just_whitespace_or_comment(ast, nodes.0 + nodes.1, selection.0 + selection.1)
}

fn java_trim(s: &[u16]) -> &[u16] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && s[a] <= b' ' as u16 {
        a += 1;
    }
    while b > a && s[b - 1] <= b' ' as u16 {
        b -= 1;
    }
    &s[a..b]
}

fn is_just_whitespace(ast: &Ast, start: usize, end: usize) -> bool {
    if start >= end {
        return true;
    }
    java_trim(&ast.source[start..end.min(ast.source.len())]).is_empty()
}

fn is_just_whitespace_or_comment(ast: &Ast, start: usize, end: usize) -> bool {
    if start >= end {
        return true;
    }
    let trimmed = java_trim(&ast.source[start..end.min(ast.source.len())]);
    if trimmed.is_empty() {
        return true;
    }
    let mut scanner = crate::rewrite::scanner::TokenScanner::new(trimmed);
    matches!(scanner.read_next_or_eof(true), Ok(crate::rewrite::scanner::Tok::Eof))
}

/// `ASTMatchingFragmentFinder.findMatchingFragments(scope, toMatch)`
/// (`GenericVisitor(true)`, not descending into Javadoc).
pub fn find_matching_fragments(scope: Node<'_>, to_match: &Fragment) -> Vec<Fragment> {
    let mut matches: Vec<Fragment> = Vec::new();
    let mut f = |n: Node<'_>| {
        if n.is(NodeKind::Javadoc) {
            return false;
        }
        for m in to_match.matching_fragments_with_node(n) {
            if !matches.contains(&m) {
                matches.push(m);
            }
        }
        true
    };
    super::walk(scope, &mut f);
    matches
}

// ─── Matching ─────────────────────────────────────────────────────────────────

/// `ASTNode.subtreeMatch(new ASTMatcher(), other)`.
pub fn subtree_match(a: Node<'_>, b: Node<'_>) -> bool {
    matcher(a, b, false)
}

/// `JdtASTMatcher.doNodesMatch(one, other)`: structural equality where
/// simple names also have to resolve to the same bindings.
pub fn do_nodes_match(a: Node<'_>, b: Node<'_>) -> bool {
    matcher(a, b, true)
}

fn same_binding(a: Option<crate::semantic_ast::BindingRef<'_>>, b: Option<crate::semantic_ast::BindingRef<'_>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.key() == y.key(),
        _ => false,
    }
}

fn matcher(a: Node<'_>, b: Node<'_>, bindings: bool) -> bool {
    if a.kind() != b.kind() {
        return false;
    }
    if bindings && a.is(NodeKind::SimpleName) {
        if a.simple("identifier") != b.simple("identifier") {
            return false;
        }
        return same_binding(a.binding(), b.binding()) && same_binding(a.type_binding(), b.type_binding());
    }
    let (pa, pb) = (a.props(), b.props());
    if pa.len() != pb.len() {
        return false;
    }
    for ((na, va), (nb, vb)) in pa.iter().zip(pb.iter()) {
        if na != nb {
            return false;
        }
        let ok = match (va, vb) {
            (PropValue::Child(None), PropValue::Child(None)) => true,
            (PropValue::Child(Some(x)), PropValue::Child(Some(y))) => matcher(a.ast.node(*x), b.ast.node(*y), bindings),
            (PropValue::List(x), PropValue::List(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| matcher(a.ast.node(*p), b.ast.node(*q), bindings))
            }
            (PropValue::Simple(x), PropValue::Simple(y)) => x == y,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}
