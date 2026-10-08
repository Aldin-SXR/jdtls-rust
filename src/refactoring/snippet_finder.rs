//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.SnippetFinder`.

use crate::semantic_ast::{BindingId, Node, NodeId, NodeKind, PropValue};

use super::extract_method_analyzer::is_left_hand_side_of_assignment;
use super::extract_temp::is_declaration;

/// `SnippetFinder.Match`.
#[derive(Clone, Debug, Default)]
pub struct Match {
    pub nodes: Vec<NodeId>,
    /// `fLocalMappings` (snippet local → candidate name).
    pub locals: Vec<(BindingId, NodeId)>,
}

impl Match {
    fn has_correct_nesting(&self, node: Node<'_>) -> bool {
        let Some(&first) = self.nodes.first() else { return true };
        let parent = node.parent();
        if node.ast.node(first).parent().map(|p| p.id) != parent.map(|p| p.id) {
            return false;
        }
        parent.is_some_and(|p| p.is(NodeKind::Block) || p.is(NodeKind::SwitchStatement))
    }

    /// `getMappedName(org)`.
    pub fn mapped_name(&self, org: BindingId) -> Option<NodeId> {
        self.locals.iter().find(|(b, _)| *b == org).map(|(_, n)| *n)
    }

    fn add_local(&mut self, org: BindingId, local: NodeId) {
        if let Some(e) = self.locals.iter_mut().find(|(b, _)| *b == org) {
            e.1 = local;
        } else {
            self.locals.push((org, local));
        }
    }

    fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.locals.is_empty()
    }

    /// `isInvalidNode()`.
    pub fn is_invalid_node(&self, ast: &crate::semantic_ast::Ast) -> bool {
        ast.node(self.nodes[0]).parent().is_some_and(|p| p.is(NodeKind::MethodDeclaration))
    }

    /// `getEnclosingMethod()`.
    pub fn enclosing_method<'a>(&self, ast: &'a crate::semantic_ast::Ast) -> Option<Node<'a>> {
        ast.node(self.nodes[0]).ancestors().find(|a| a.is(NodeKind::MethodDeclaration))
    }

    /// `isNodeInStaticContext()`.
    pub fn is_node_in_static_context(&self, ast: &crate::semantic_ast::Ast) -> bool {
        is_in_static_context(ast.node(self.nodes[0]))
    }
}

/// `ASTResolving.isInStaticContext(node)`.
pub fn is_in_static_context(node: Node<'_>) -> bool {
    let Some(decl) = std::iter::once(node).chain(node.ancestors()).find(|n| n.kind().is_body_declaration()) else { return false };
    match decl.kind() {
        NodeKind::MethodDeclaration => {
            if decl.flag("constructor") && node.ancestors().take_while(|a| a.id != decl.id).any(|a| a.is(NodeKind::ConstructorInvocation) || a.is(NodeKind::SuperConstructorInvocation)) {
                return true;
            }
            decl.modifiers() & crate::semantic_ast::modifier::STATIC != 0
        }
        NodeKind::Initializer | NodeKind::FieldDeclaration => decl.modifiers() & crate::semantic_ast::modifier::STATIC != 0,
        _ => false,
    }
}

struct Finder<'a> {
    snippet: Vec<Node<'a>>,
    result: Vec<Match>,
    current: Match,
    index: usize,
    types: i32,
}

impl<'a> Finder<'a> {
    fn reset(&mut self) {
        self.index = 0;
        self.current = Match::default();
    }

    fn accept(&mut self, n: Node<'a>) {
        let is_type = matches!(n.kind(), NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration);
        let descend = if is_type {
            self.types += 1;
            self.types <= 1 && self.visit_node(n)
        } else {
            self.visit_node(n)
        };
        if descend {
            for c in n.children() {
                self.accept(c);
            }
        }
        if is_type {
            self.types -= 1;
        }
    }

    fn visit_node(&mut self, n: Node<'a>) -> bool {
        if self.matches(n) {
            return false;
        }
        if !(self.index == 0 && self.current.is_empty()) {
            self.reset();
            if self.matches(n) {
                return false;
            }
        }
        true
    }

    fn matches(&mut self, n: Node<'a>) -> bool {
        if self.snippet.iter().any(|s| s.id == n.id) {
            return false;
        }
        let snippet = self.snippet[self.index];
        if subtree_match(n, snippet, &mut self.current) && self.current.has_correct_nesting(n) {
            self.current.nodes.push(n.id);
            self.index += 1;
            if self.index == self.snippet.len() {
                let m = std::mem::take(&mut self.current);
                self.result.push(m);
                self.reset();
            }
            return true;
        }
        false
    }
}

/// `node.subtreeMatch(new Matcher(), snippet)`.
fn subtree_match(candidate: Node<'_>, snippet: Node<'_>, m: &mut Match) -> bool {
    if candidate.kind() != snippet.kind() {
        return false;
    }
    if candidate.is(NodeKind::SimpleName) {
        return match_simple_name(candidate, snippet, m);
    }
    let (pa, pb) = (candidate.props(), snippet.props());
    if pa.len() != pb.len() {
        return false;
    }
    for ((na, va), (nb, vb)) in pa.iter().zip(pb.iter()) {
        if na != nb {
            return false;
        }
        let ok = match (va, vb) {
            (PropValue::Child(None), PropValue::Child(None)) => true,
            (PropValue::Child(Some(x)), PropValue::Child(Some(y))) => subtree_match(candidate.ast.node(*x), snippet.ast.node(*y), m),
            (PropValue::List(x), PropValue::List(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| subtree_match(candidate.ast.node(*p), snippet.ast.node(*q), m)),
            (PropValue::Simple(x), PropValue::Simple(y)) => x == y,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// `Matcher.match(SimpleName candidate, Object s)`.
fn match_simple_name(candidate: Node<'_>, snippet: Node<'_>, m: &mut Match) -> bool {
    if is_declaration(candidate) != is_declaration(snippet) {
        return false;
    }
    let (Some(cb), Some(sb)) = (candidate.binding(), snippet.binding()) else { return false };
    let (vcb, vsb) = (Some(cb).filter(|b| b.is_variable()), Some(sb).filter(|b| b.is_variable()));
    let (Some(vcb), Some(vsb)) = (vcb, vsb) else { return cb.key() == sb.key() };
    let same_type = match (vcb.var_type(), vsb.var_type()) {
        (Some(a), Some(b)) => a.key() == b.key(),
        (None, None) => true,
        _ => false,
    };
    if !vcb.is_field() && !vsb.is_field() && same_type {
        if let Some(mapped) = m.mapped_name(vsb.id) {
            let mapped_binding = candidate.ast.node(mapped).binding().filter(|b| b.is_variable());
            if mapped_binding.map(|b| b.key()) != Some(vcb.key()) {
                return false;
            }
        }
        m.add_local(vsb.id, candidate.id);
        return true;
    }
    cb.key() == sb.key()
}

/// `SnippetFinder.perform(start, snippet)`.
pub fn perform<'a>(start: Node<'a>, snippet: &[Node<'a>]) -> Vec<Match> {
    let mut finder = Finder { snippet: snippet.to_vec(), result: Vec::new(), current: Match::default(), index: 0, types: 0 };
    finder.accept(start);
    let ast = start.ast;
    finder.result.retain(|m| !(m.nodes.len() == 1 && is_left_hand_side_of_assignment(ast.node(m.nodes[0]))));
    finder.result
}
