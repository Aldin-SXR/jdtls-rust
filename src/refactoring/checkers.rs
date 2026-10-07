//! Ports of `SideEffectChecker`, `UnsafeCheckTester` and
//! `ChangedValueChecker` (`org.eclipse.jdt.internal.corext.refactoring.util`).
//!
//! `findFunctionDefinition` parses the declaring unit of an invoked method;
//! here the method's declaration is looked up in the unit's own AST (methods
//! of binary types — the JRE and libraries — have no declaration, exactly
//! like upstream).

use std::collections::HashSet;
use std::sync::Arc;

use crate::rewrite::{flattener::Flattener, ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

use super::fragments::{self, Fragment};

const THRESHOLD: usize = 2500;

const MARKED_METHODS: [&str; 5] = [
    "java.lang.System.currentTimeMillis",
    "java.lang.System.nanoTime",
    "java.io.PrintStream.print",
    "java.io.PrintStream.printf",
    "java.io.PrintStream.println",
];

fn qualified_method_name(m: Option<BindingRef<'_>>) -> Option<String> {
    let m = m?;
    let declaring = m.declaring_class()?;
    Some(format!("{}.{}", declaring.qualified_name(), m.name()))
}

fn is_marked(m: Option<BindingRef<'_>>) -> bool {
    qualified_method_name(m).is_some_and(|n| MARKED_METHODS.contains(&n.as_str()))
}

/// `getOriginalExpression(expr)`: strips parentheses and casts.
pub fn original_expression(mut e: Node<'_>) -> Node<'_> {
    while e.is(NodeKind::ParenthesizedExpression) || e.is(NodeKind::CastExpression) {
        match e.child("expression") {
            Some(x) => e = x,
            None => break,
        }
    }
    e
}

/// The source declaration of `method` in this unit (the part of
/// `findFunctionDefinition` that does not need other units).
fn method_declaration_in_unit<'a>(method: BindingRef<'a>) -> Option<Node<'a>> {
    let declaration = method.method_declaration().unwrap_or(method);
    let node = declaration.declaring_node()?;
    node.is(NodeKind::MethodDeclaration).then_some(node)
}

fn method_key(m: BindingRef<'_>) -> String {
    m.method_declaration().unwrap_or(m).key().to_owned()
}

// ─── SideEffectChecker ────────────────────────────────────────────────────────

/// `SideEffectChecker`.
pub fn has_side_effect(expression: Node<'_>, enclosing_method_signature: Option<&str>) -> bool {
    let mut side_effect = false;
    let mut f = |node: Node<'_>| -> bool {
        if side_effect {
            return false;
        }
        if node.is(NodeKind::Javadoc) {
            return false;
        }
        if self_modified(node) {
            side_effect = true;
            return false;
        }
        if node.is(NodeKind::MethodInvocation) {
            let binding = node.method_binding();
            if binding.is_none() || is_marked(binding) {
                side_effect = true;
                return false;
            }
            let binding = binding.unwrap();
            if enclosing_method_signature.is_some_and(|s| s == method_key(binding)) {
                return true;
            }
            match find_function_definition_for_side_effects(binding) {
                Ok(Some(md)) => {
                    if md.length() < THRESHOLD && method_updates_no_temp(md) {
                        side_effect = true;
                    }
                }
                Ok(None) => {}
                Err(()) => side_effect = true,
            }
        }
        true
    };
    super::walk(expression, &mut f);
    side_effect
}

/// `SideEffectChecker.findFunctionDefinition`: `Err` where upstream marks a
/// side effect.
fn find_function_definition_for_side_effects(method: BindingRef<'_>) -> Result<Option<Node<'_>>, ()> {
    let declaration = method.method_declaration().unwrap_or(method);
    if declaration.declaring_class().is_none() {
        return Err(());
    }
    if !declaration.is_from_source() {
        // Binary types: no compilation unit (JRE container: not searched).
        return Ok(None);
    }
    match method_declaration_in_unit(method) {
        Some(md) => {
            if md.modifiers() & modifier::ABSTRACT != 0 || md.child("body").is_none() {
                // Look for an implementation in a subtype of this unit.
                let key = declaration.key();
                let implementation = md.ast.all_nodes().find(|n| {
                    n.is(NodeKind::MethodDeclaration)
                        && n.child("body").is_some()
                        && n.binding().is_some_and(|b| b.data().method_overrides.iter().any(|o| md.ast.binding(*o).key() == key))
                });
                return implementation.map(Some).ok_or(());
            }
            Ok(Some(md))
        }
        // Declared in another unit of the project.
        None => Ok(None),
    }
}

/// `SideEffectChecker.selfModied(node)`.
fn self_modified(node: Node<'_>) -> bool {
    match node.kind() {
        NodeKind::Assignment => {
            let (Some(lhs), Some(rhs)) = (node.child("leftHandSide"), node.child("rightHandSide")) else { return false };
            if !fragments::subtree_match(original_expression(rhs), original_expression(lhs)) {
                return depends_on(rhs, lhs);
            }
            false
        }
        NodeKind::PrefixExpression => matches!(node.simple("operator"), Some("++" | "--")),
        NodeKind::PostfixExpression => true,
        _ => false,
    }
}

/// `AssignmentVisitor`: some node of `n` matches `lvalue`.
fn depends_on(n: Node<'_>, lvalue: Node<'_>) -> bool {
    let mut depend = false;
    let mut f = |x: Node<'_>| -> bool {
        if depend || x.is(NodeKind::Javadoc) {
            return false;
        }
        if fragments::subtree_match(x, lvalue) {
            depend = true;
        }
        true
    };
    super::walk(n, &mut f);
    depend
}

/// `SideEffectChecker.MethodVisitor.hasUpdateNoTemp()`.
fn method_updates_no_temp(md: Node<'_>) -> bool {
    let mut update = false;
    let mut f = |node: Node<'_>| -> bool {
        if update || node.is(NodeKind::Javadoc) {
            return false;
        }
        if node.is(NodeKind::MethodInvocation) && is_marked(node.method_binding()) {
            update = true;
            return false;
        }
        let mut operand = None;
        if node.is(NodeKind::Assignment) {
            let (Some(lhs), Some(rhs)) = (node.child("leftHandSide"), node.child("rightHandSide")) else { return true };
            let op = original_expression(lhs);
            if !is_no_temp(op) {
                return true;
            }
            if (!fragments::subtree_match(original_expression(rhs), op) || node.simple("operator") != Some("=")) && depends_on(rhs, lhs) {
                update = true;
                return true;
            }
            return true;
        }
        if node.is(NodeKind::PrefixExpression) {
            if matches!(node.simple("operator"), Some("++" | "--")) {
                operand = node.child("operand");
            }
        } else if node.is(NodeKind::PostfixExpression) {
            operand = node.child("operand");
        }
        if operand.is_some_and(is_no_temp) {
            update = true;
        }
        true
    };
    super::walk(md, &mut f);
    update
}

fn is_no_temp(e: Node<'_>) -> bool {
    let expr = original_expression(e);
    let binding = match expr.kind() {
        NodeKind::SimpleName => expr.binding().filter(|b| b.is_variable()),
        NodeKind::FieldAccess => expr.child("name").and_then(|n| n.binding()),
        NodeKind::QualifiedName => {
            let b = expr.binding().filter(|b| b.is_variable());
            return b.is_some_and(|b| b.modifiers() & modifier::STATIC != 0) || expr.child("qualifier").is_some_and(is_no_temp);
        }
        NodeKind::ArrayAccess => return expr.child("array").is_some_and(is_no_temp),
        _ => None,
    };
    binding.is_some_and(|b| b.is_field() || b.modifiers() & modifier::STATIC != 0)
}

// ─── UnsafeCheckTester ────────────────────────────────────────────────────────

/// `UnsafeCheckTester`.
pub struct UnsafeCheckTester {
    start: i64,
    end: i64,
    common: crate::semantic_ast::NodeId,
    match_positions: HashSet<(usize, usize)>,
    invocations: HashSet<String>,
    invocation_casts: Vec<(String, Option<String>)>,
    match_casts: Vec<((usize, usize), Option<String>)>,
}

/// `UnsafeCheckTester.getEnclosingBodyNode(node)`.
fn enclosing_body_node(node: Node<'_>) -> Option<Node<'_>> {
    let mut location = None;
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_body_declaration() {
            break;
        }
        location = x.location();
        n = x.parent();
        if n.is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
            break;
        }
    }
    let n = n?;
    let ok = (n.is(NodeKind::MethodDeclaration) || n.is(NodeKind::Initializer)) && location == Some("body")
        || (n.is(NodeKind::LambdaExpression) && location == Some("body") && n.method_binding().is_some());
    if ok {
        return n.child("body");
    }
    None
}

impl UnsafeCheckTester {
    pub fn new(root: Node<'_>, common: Node<'_>, expression: Node<'_>, start: i64, end: i64) -> Self {
        let mut t = UnsafeCheckTester {
            start,
            end,
            common: common.id,
            match_positions: HashSet::new(),
            invocations: HashSet::new(),
            invocation_casts: Vec::new(),
            match_casts: Vec::new(),
        };
        let mut f = |node: Node<'_>| -> bool {
            if node.is(NodeKind::Javadoc) {
                return false;
            }
            let mut temp = None;
            match node.kind() {
                NodeKind::CastExpression => {
                    let target = node.child("type").and_then(|t| t.binding()).map(|b| b.key().to_owned());
                    if let Some(e) = node.child("expression") {
                        let e = original_expression(e);
                        match e.binding().filter(|_| e.kind().is_name()) {
                            Some(b) => t.invocation_casts.push((b.key().to_owned(), target)),
                            None => t.match_casts.push(((e.start(), e.length()), target)),
                        }
                    }
                }
                NodeKind::MethodInvocation | NodeKind::FieldAccess => temp = node.child("expression"),
                NodeKind::QualifiedName => temp = node.child("qualifier"),
                NodeKind::ArrayAccess => temp = node.child("array"),
                _ => {}
            }
            if let Some(temp) = temp {
                let temp = original_expression(temp);
                if let (Some(body), Some(frag)) = (enclosing_body_node(temp), fragments::for_source_range(root, temp.start(), temp.length())) {
                    for m in fragments::full_subtree(body).sub_fragments_matching(&frag, root.ast) {
                        let n = m.node(root.ast);
                        t.match_positions.insert((n.start(), n.length()));
                    }
                }
                if temp.kind().is_name() {
                    if let Some(b) = temp.binding() {
                        t.invocations.insert(b.key().to_owned());
                    }
                }
            }
            true
        };
        super::walk(expression, &mut f);
        t
    }

    fn inheritance(itb1: Option<&str>, itb2: Option<BindingRef<'_>>, depth: usize) -> bool {
        let (Some(a), Some(b)) = (itb1, itb2) else { return false };
        if a == b.key() {
            return true;
        }
        if depth > 32 {
            return false;
        }
        b.interfaces().iter().any(|i| Self::inheritance(itb1, Some(*i), depth + 1)) || b.superclass().is_some_and(|s| Self::inheritance(itb1, Some(s), depth + 1))
    }

    /// `hasUnsafeCheck()`.
    pub fn has_unsafe_check(&self, ast: &Ast) -> bool {
        let mut null_flag = false;
        let mut cast_flag = false;
        let common = ast.node(self.common);
        let mut f = |node: Node<'_>| -> bool {
            if node.is(NodeKind::Javadoc) {
                return false;
            }
            let (sl, el) = (node.start() as i64, node.end() as i64);
            if el < self.start || sl > self.end || null_flag || cast_flag {
                return false;
            }
            if !(sl >= self.start && el <= self.end) {
                return true;
            }
            if node.is(NodeKind::InstanceofExpression) {
                let Some(left) = node.child("leftOperand").map(original_expression) else { return true };
                let right = node.child("rightOperand").and_then(|r| r.binding());
                if let Some(b) = left.binding().filter(|_| left.kind().is_name()) {
                    let cast = self.invocation_casts.iter().rev().find(|(k, _)| k == b.key()).and_then(|(_, t)| t.as_deref());
                    if Self::inheritance(cast, right, 0) {
                        cast_flag = true;
                        return false;
                    }
                }
                let pos = (left.start(), left.length());
                let cast = self.match_casts.iter().rev().find(|(p, _)| *p == pos).and_then(|(_, t)| t.as_deref());
                if Self::inheritance(cast, right, 0) {
                    cast_flag = true;
                    return false;
                }
                return true;
            }
            let mut target = None;
            if node.is(NodeKind::InfixExpression) && matches!(node.simple("operator"), Some("==" | "!=")) {
                let (l, r) = (node.child("leftOperand"), node.child("rightOperand"));
                if r.is_some_and(|r| r.is(NodeKind::NullLiteral)) {
                    target = l;
                } else if l.is_some_and(|l| l.is(NodeKind::NullLiteral)) {
                    target = r;
                }
            }
            if let Some(target) = target.map(original_expression) {
                if target.kind().is_name() && target.binding().is_some_and(|b| self.invocations.contains(b.key())) {
                    null_flag = true;
                    return false;
                }
                if self.match_positions.contains(&(target.start(), target.length())) {
                    null_flag = true;
                    return false;
                }
            }
            true
        };
        super::walk(common, &mut f);
        null_flag || cast_flag
    }
}

// ─── ChangedValueChecker ──────────────────────────────────────────────────────

/// `ChangedValueChecker`.
pub struct ChangedValueChecker {
    ast: Arc<Ast>,
    enclosing: Option<String>,
    ignore_assignment_to: Option<crate::semantic_ast::NodeId>,
    depend: HashSet<String>,
    middle: Vec<crate::semantic_ast::NodeId>,
}

impl ChangedValueChecker {
    pub fn new(ast: &Arc<Ast>, selected: Node<'_>, enclosing: Option<&str>, ignore_assignment_updates: bool) -> Self {
        let mut c = ChangedValueChecker {
            ast: ast.clone(),
            enclosing: enclosing.map(str::to_owned),
            ignore_assignment_to: ignore_assignment_updates.then_some(selected.id),
            depend: HashSet::new(),
            middle: Vec::new(),
        };
        c.depend = c.read_set(selected, true);
        c
    }

    fn to_string(&self, n: Node<'_>) -> String {
        let rw = ASTRewrite::new(self.ast.clone());
        Flattener::as_string(&rw, RNode::Orig(n.id))
    }

    /// `new Elem(node, flag)`: `(string, memberKey != null)`.
    fn elem(&self, node: Option<Node<'_>>, flag: bool) -> (String, bool) {
        let Some(node) = node else { return (String::new(), false) };
        match node.kind() {
            NodeKind::SimpleName => {
                let key = node.binding().filter(|b| !b.is_variable() || b.is_field() || flag).map(|b| b.key().to_owned());
                match key {
                    Some(k) => (k, true),
                    None => (String::new(), false),
                }
            }
            NodeKind::QualifiedName => {
                let key = node.child("name").and_then(|n| n.binding()).map(|b| b.key().to_owned());
                let mut e = String::new();
                if node.binding().is_some_and(|b| b.modifiers() != modifier::STATIC) {
                    e = self.elem(node.child("qualifier"), flag).0;
                }
                (format!("{e}{}", key.clone().unwrap_or_default()), key.is_some())
            }
            NodeKind::FieldAccess => {
                let mut key = node.child("name").and_then(|n| n.binding()).map(|b| b.key().to_owned());
                let expr = node.child("expression").map(original_expression);
                let mut e = String::new();
                if let Some(x) = expr.filter(|x| x.is(NodeKind::MethodInvocation)) {
                    if flag {
                        e = self.to_string(x);
                    } else {
                        key = None;
                    }
                } else {
                    e = self.elem(expr, flag).0;
                }
                (format!("{e}{}", key.clone().unwrap_or_default()), key.is_some())
            }
            NodeKind::MethodInvocation => {
                // memberKey stays null.
                let e = if flag { self.to_string(node) } else { String::new() };
                (e, false)
            }
            _ => (String::new(), false),
        }
    }

    /// `ReadVisitor(visitMethodCall)` over `n`: the read set.
    fn read_set(&self, n: Node<'_>, visit_method_call: bool) -> HashSet<String> {
        let mut set = HashSet::new();
        let mut f = |x: Node<'_>| -> bool {
            match x.kind() {
                NodeKind::Javadoc => false,
                NodeKind::FieldAccess | NodeKind::QualifiedName => {
                    let (s, valid) = self.elem(Some(x), visit_method_call);
                    if valid {
                        set.insert(s);
                    }
                    false
                }
                NodeKind::SimpleName => {
                    if x.binding().is_some_and(|b| b.is_variable()) {
                        let (s, valid) = self.elem(Some(x), visit_method_call);
                        if valid {
                            set.insert(s);
                        }
                    }
                    false
                }
                NodeKind::MethodInvocation => {
                    let Some(b) = x.method_binding() else { return true };
                    if !visit_method_call || self.enclosing.as_deref().is_some_and(|s| s == method_key(b)) {
                        return true;
                    }
                    if let Some(md) = self.find_function_definition(b) {
                        if md.length() < THRESHOLD {
                            let receiver = self.elem(x.child("expression"), visit_method_call).0;
                            for e in self.read_set(md, false) {
                                set.insert(format!("{receiver}{e}"));
                            }
                        }
                    }
                    true
                }
                _ => true,
            }
        };
        super::walk(n, &mut f);
        set
    }

    /// `ChangedValueChecker.findFunctionDefinition`.
    fn find_function_definition<'a>(&self, method: BindingRef<'a>) -> Option<Node<'a>> {
        let declaration = method.method_declaration().unwrap_or(method);
        let declaring = declaration.declaring_class()?;
        if !declaration.is_from_source() || declaring.is_interface() {
            return None;
        }
        let md = method_declaration_in_unit(method)?;
        (md.modifiers() & modifier::ABSTRACT == 0).then_some(md)
    }

    /// `UpdateVisitor(dependSet, visitMethodCall, ignore)` over `n`.
    fn update_set(&self, n: Node<'_>, visit_method_call: bool) -> HashSet<String> {
        let mut set = HashSet::new();
        let ignore = self.ignore_assignment_to.map(|i| self.ast.node(i));
        let mut f = |x: Node<'_>| -> bool {
            match x.kind() {
                NodeKind::Javadoc => false,
                NodeKind::Assignment => {
                    if let (Some(ignore), Some(lhs)) = (ignore, x.child("leftHandSide")) {
                        if fragments::subtree_match(lhs, ignore) {
                            return true;
                        }
                    }
                    if let Some(lhs) = x.child("leftHandSide") {
                        set.extend(self.read_set(lhs, visit_method_call));
                    }
                    true
                }
                NodeKind::PrefixExpression | NodeKind::PostfixExpression => {
                    if matches!(x.simple("operator"), Some("++" | "--")) {
                        if let Some(o) = x.child("operand") {
                            set.extend(self.read_set(o, visit_method_call));
                        }
                    }
                    true
                }
                NodeKind::MethodInvocation => {
                    let Some(b) = x.method_binding() else { return false };
                    if !visit_method_call || self.enclosing.as_deref().is_some_and(|s| s == method_key(b)) {
                        return true;
                    }
                    if let Some(md) = self.find_function_definition(b) {
                        if md.length() < THRESHOLD {
                            let receiver = self.elem(x.child("expression"), visit_method_call).0;
                            for e in self.update_set(md, false) {
                                set.insert(format!("{receiver}{e}"));
                            }
                        }
                    }
                    true
                }
                _ => true,
            }
        };
        super::walk(n, &mut f);
        set
    }

    /// `detectConflict(startOffset, endOffset, node, bodyNode, candidateList)`.
    pub fn detect_conflict(&mut self, start: i64, end: i64, node: Node<'_>, body: Node<'_>, candidates: &[Fragment]) {
        let ast = node.ast;
        let mut pv = PathVisitor::new(start, end, node, candidates.to_vec());
        let mut b = Some(body);
        while let Some(x) = b {
            if (x.end() as i64) < pv.end || (x.start() as i64) > pv.start {
                b = x.parent();
            } else {
                break;
            }
        }
        if let Some(b) = b {
            pv.run(b, ast);
        }
        self.middle = pv.nodes;
    }

    /// `hasConflict()`.
    pub fn has_conflict(&self) -> bool {
        let mut seen = HashSet::new();
        for id in &self.middle {
            let n = self.ast.node(*id);
            if !seen.insert((n.start(), n.length())) {
                continue;
            }
            let updates = self.update_set(n, true);
            if self.depend.iter().any(|d| updates.contains(d)) {
                return true;
            }
        }
        false
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Traversal {
    NotYet,
    Between,
    Exited,
}

/// `ChangedValueChecker.PathVisitor`.
struct PathVisitor {
    nodes: Vec<crate::semantic_ast::NodeId>,
    positions: HashSet<(usize, usize)>,
    candidates: Vec<Fragment>,
    start: i64,
    end: i64,
    state: Traversal,
    selected: crate::semantic_ast::NodeId,
}

impl PathVisitor {
    fn new(start: i64, end: i64, node: Node<'_>, candidates: Vec<Fragment>) -> Self {
        let mut pv = PathVisitor { nodes: Vec::new(), positions: HashSet::new(), candidates, start, end, state: Traversal::NotYet, selected: node.id };
        // extend2EndOfLoop
        let mut temp = Some(node);
        while let Some(t) = temp {
            if t.is(NodeKind::MethodDeclaration) {
                break;
            }
            if matches!(t.kind(), NodeKind::EnhancedForStatement | NodeKind::WhileStatement | NodeKind::ForStatement | NodeKind::DoStatement) {
                let offset = t.start() as i64;
                let new_end = t.end() as i64;
                let cond1 = offset < pv.start && new_end > pv.start;
                let cond2 = offset <= pv.end && new_end >= pv.end;
                if !cond1 && cond2 {
                    pv.end = new_end;
                }
            }
            temp = t.parent();
        }
        pv
    }

    fn run(&mut self, root: Node<'_>, ast: &Ast) {
        let mut stack = vec![root];
        // Preorder traversal with the visitor's descend decision.
        fn go(pv: &mut PathVisitor, n: Node<'_>, ast: &Ast) {
            if pv.pre_visit(n, ast) {
                for c in n.children() {
                    go(pv, c, ast);
                }
            }
        }
        if let Some(r) = stack.pop() {
            go(self, r, ast);
        }
    }

    fn pre_visit(&mut self, node: Node<'_>, ast: &Ast) -> bool {
        if node.is(NodeKind::Javadoc) {
            return false;
        }
        let (s, e) = (node.start() as i64, node.end() as i64);
        if self.state == Traversal::NotYet && s >= self.start && e <= self.end {
            self.state = Traversal::Between;
        } else if self.state == Traversal::Between && s > self.end {
            self.state = Traversal::Exited;
        }
        if self.state != Traversal::Between {
            return true;
        }
        if node.kind().is_statement() && node.location_is("thenStatement") {
            if let Some(is) = node.parent().filter(|p| p.is(NodeKind::IfStatement)) {
                let selected = ast.node(self.selected);
                if let Some(else_stmt) = is.child("elseStatement") {
                    if else_stmt.is_ancestor_or_self_of(selected) && else_stmt.id != selected.id || is_parent(selected, else_stmt) {
                        let then = is.child("thenStatement").unwrap();
                        let (offset, length) = (then.start(), then.length());
                        let n = self.candidates.len();
                        let mut i = 0;
                        while i < n {
                            let cs = self.candidates[i].start(ast);
                            if cs >= offset && cs <= offset + length {
                                while i < n - 1 && self.candidates[i + 1].start(ast) >= offset && self.candidates[i + 1].start(ast) <= offset + length {
                                    i += 1;
                                }
                                if i < n {
                                    let c = &self.candidates[i];
                                    let mut pv = PathVisitor::new(offset as i64, c.start(ast) as i64, c.node(ast), self.candidates.clone());
                                    pv.run(then, ast);
                                    self.nodes.extend(pv.nodes);
                                    self.positions.extend(pv.positions);
                                }
                                break;
                            }
                            i += 1;
                        }
                        return false;
                    }
                }
            }
        }
        if s >= self.start && e <= self.end {
            if node.kind().is_type() || matches!(node.kind(), NodeKind::NumberLiteral | NodeKind::StringLiteral | NodeKind::NullLiteral) {
                return false;
            }
            if self.positions.insert((node.start(), node.length())) {
                self.nodes.push(node.id);
            }
            return false;
        }
        true
    }
}

/// `ASTNodes.isParent(node, parent)`: `parent` is a proper ancestor of `node`.
fn is_parent(node: Node<'_>, parent: Node<'_>) -> bool {
    node.ancestors().any(|a| a.id == parent.id)
}
