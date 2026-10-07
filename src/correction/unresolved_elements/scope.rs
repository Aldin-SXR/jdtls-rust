//! `ScopeAnalyzer` (method declarations in scope), `NameMatcher` and the
//! `ASTResolving` guessing helpers used by the unresolved-element processors.

use std::collections::HashSet;

use super::types::{self, erasure, normalize, well_known};
use crate::semantic_ast::{modifier, BindingRef, Node, NodeKind};

/// `NameMatcher.getSimilarity`.
pub fn similarity(name1: &str, name2: &str) -> i32 {
    let (mut a, mut b): (Vec<u16>, Vec<u16>) = (name1.encode_utf16().collect(), name2.encode_utf16().collect());
    if a.len() > b.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let lower = |c: u16| -> u32 {
        char::from_u32(c as u32).map(|ch| {
            let l: Vec<char> = ch.to_lowercase().collect();
            if l.len() == 1 { l[0] as u32 } else { c as u32 }
        }).unwrap_or(c as u32)
    };
    let similar = |x: u16, y: u16| lower(x) == lower(y);
    let (len1, len2) = (a.len(), b.len());
    let mut matched = 0usize;
    let mut i = 0;
    while i < len1 && similar(a[i], b[i]) {
        i += 1;
        matched += 1;
    }
    let mut k = len1;
    let diff = len2 - len1;
    while k > i && similar(a[k - 1], b[k + diff - 1]) {
        k -= 1;
        matched += 1;
    }
    if matched == len2 {
        return 200;
    }
    if len2 - matched > matched {
        return -1;
    }
    let tolerance = (len2 / 4 + 1) as i32;
    (tolerance - (k - i) as i32) * 256 / tolerance
}

/// `NameMatcher.isSimilarName`.
pub fn is_similar_name(a: &str, b: &str) -> bool {
    similarity(a, b) >= 0
}

/// `ScopeAnalyzer.getSignature` of a method.
fn method_signature_key(m: BindingRef<'_>) -> String {
    let params: Vec<String> = m.parameter_types().iter().map(|p| erasure(*p).qualified_name().to_owned()).collect();
    format!("M{}({})", m.name(), params.join(","))
}

struct Requestor<'a> {
    result: Vec<BindingRef<'a>>,
    names: HashSet<String>,
    visited: HashSet<String>,
}

impl<'a> Requestor<'a> {
    fn accept(&mut self, m: BindingRef<'a>) {
        if self.names.insert(method_signature_key(m)) {
            self.result.push(m);
        }
    }

    /// `ScopeAnalyzer.addInherited(binding, isSuperInterfaceBinding, METHODS)`.
    fn add_inherited(&mut self, t: BindingRef<'a>, super_interface: bool) {
        if !self.visited.insert(t.key().to_owned()) {
            return;
        }
        for m in t.declared_methods().unwrap_or_default() {
            if super_interface && m.modifiers() & modifier::STATIC != 0 {
                continue;
            }
            if !m.has(crate::semantic_ast::bflag::SYNTHETIC) && !m.is_constructor() {
                self.accept(m);
            }
        }
        if let Some(s) = t.superclass() {
            self.add_inherited(s, false);
        } else if t.is_array() {
            if let Some(o) = well_known(t.ast, "java.lang.Object") {
                self.add_inherited(o, false);
            }
        }
        for i in t.interfaces() {
            self.add_inherited(i, true);
        }
    }

    /// `ScopeAnalyzer.addTypeDeclarations(binding, METHODS)`.
    fn add_type_declarations(&mut self, t: BindingRef<'a>) {
        self.add_inherited(t, false);
        if t.is_local() {
            if let Some(node) = t.declaring_node() {
                if let Some(parent) = node.parent() {
                    if let Some(p) = types::parent_type_binding(parent) {
                        self.add_type_declarations(p);
                    }
                }
            }
        } else if let Some(d) = t.declaring_class() {
            self.add_type_declarations(d);
        }
    }
}

/// `ScopeAnalyzer.getQualifier(selector)` for method names.
fn qualifier<'a>(selector: Node<'a>) -> Option<BindingRef<'a>> {
    let parent = selector.parent()?;
    match parent.kind() {
        NodeKind::MethodInvocation if selector.location_is("name") => parent.child("expression").and_then(|e| e.type_binding()),
        NodeKind::SuperMethodInvocation if selector.location_is("name") => types::parent_type_binding(parent).and_then(|t| t.superclass()),
        NodeKind::QualifiedName if selector.location_is("name") => parent.child("qualifier").and_then(|e| e.type_binding()),
        NodeKind::FieldAccess if selector.location_is("name") => parent.child("expression").and_then(|e| e.type_binding()),
        _ => None,
    }
}

/// `new ScopeAnalyzer(root).getDeclarationsInScope(selector, METHODS)`.
pub fn methods_in_scope<'a>(selector: Node<'a>) -> Vec<BindingRef<'a>> {
    let Some(parent_type) = types::parent_type_binding(selector) else { return Vec::new() };
    let mut r = Requestor { result: Vec::new(), names: HashSet::new(), visited: HashSet::new() };
    let qualifier = qualifier(selector);
    match qualifier {
        None => r.add_type_declarations(parent_type),
        Some(q) => r.add_inherited(q, false),
    }
    r.result
}

/// `ASTResolving.isInStaticContext`.
pub fn is_in_static_context(node: Node<'_>) -> bool {
    let Some(decl) = crate::semantic_ast::resolve::find_parent_body_declaration(node) else { return false };
    match decl.kind() {
        NodeKind::MethodDeclaration => {
            if is_inside_constructor_invocation(decl, node) {
                return true;
            }
            decl.modifiers() & modifier::STATIC != 0
        }
        NodeKind::Initializer => decl.modifiers() & modifier::STATIC != 0,
        NodeKind::FieldDeclaration => {
            // JdtFlags.isStatic: interface fields are implicitly static.
            decl.modifiers() & modifier::STATIC != 0
                || decl.parent().is_some_and(|p| p.is(NodeKind::TypeDeclaration) && p.flag("interface"))
        }
        _ => false,
    }
}

/// `ASTResolving.isInsideConstructorInvocation`.
fn is_inside_constructor_invocation(decl: Node<'_>, node: Node<'_>) -> bool {
    if decl.flag("constructor") {
        if let Some(statement) = crate::semantic_ast::resolve::find_parent_statement(node) {
            if statement.is(NodeKind::ConstructorInvocation) || statement.is(NodeKind::SuperConstructorInvocation) {
                return true;
            }
        }
    }
    false
}

/// `ASTResolving.getParameterTypeBinding(binding, index)`.
pub fn parameter_type_binding<'a>(m: BindingRef<'a>, index: usize) -> Option<BindingRef<'a>> {
    let params = m.parameter_types();
    if m.is_varargs() && index + 1 >= params.len() {
        return params.last().and_then(|p| p.component_type());
    }
    params.get(index).copied()
}

fn argument_parameter<'a>(node: Node<'a>, invocation: Node<'a>, m: Option<BindingRef<'a>>) -> Option<BindingRef<'a>> {
    let m = m?;
    let index = invocation.list("arguments").iter().position(|a| *a == node)?;
    parameter_type_binding(m, index)
}

fn find_enclosing_method<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut n = node.parent();
    while let Some(x) = n {
        match x.kind() {
            NodeKind::MethodDeclaration => return Some(x),
            NodeKind::LambdaExpression => return None,
            k if k.is_abstract_type_declaration() || k == NodeKind::AnonymousClassDeclaration => return None,
            _ => {}
        }
        n = x.parent();
    }
    None
}

/// `ASTResolving.guessBindingForReference`.
pub fn guess_binding_for_reference(node: Node<'_>) -> Option<BindingRef<'_>> {
    normalize(possible_reference_binding(node))
}

/// The array guesses of `getPossibleReferenceBinding` (`createArrayType(1)`)
/// whose array type binding the AST does not know: the element type and the
/// number of dimensions to add.
pub fn guess_array_reference(node: Node<'_>) -> Option<(BindingRef<'_>, usize)> {
    let parent = node.parent()?;
    match parent.kind() {
        NodeKind::ArrayAccess if !node.location_is("index") => {
            let (b, d) = match possible_reference_binding(parent) {
                Some(b) => (b, 0),
                None => guess_array_reference(parent).or_else(|| well_known(node.ast, "java.lang.Object").map(|o| (o, 0)))?,
            };
            Some((b, d + 1))
        }
        NodeKind::EnhancedForStatement if node.location_is("expression") => {
            let t = parent.child("parameter").and_then(|p| p.child("type")).and_then(|t| t.binding())?;
            Some((t, 1))
        }
        NodeKind::ParenthesizedExpression => guess_array_reference(parent),
        _ => None,
    }
}

fn possible_reference_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    let parent = node.parent()?;
    let ast = node.ast;
    let wk = |n: &str| well_known(ast, n);
    match parent.kind() {
        NodeKind::Assignment => {
            if node.location_is("leftHandSide") {
                parent.child("rightHandSide").and_then(|n| n.type_binding())
            } else {
                parent.child("leftHandSide").and_then(|n| n.type_binding())
            }
        }
        NodeKind::InfixExpression => {
            let op = parent.simple("operator").unwrap_or("");
            if op == "&&" || op == "||" {
                return wk("boolean");
            }
            if op == "<<" || op == ">>>" || op == ">>" {
                return wk("int");
            }
            if node.location_is("leftOperand") {
                if let Some(b) = parent.child("rightOperand").and_then(|n| n.type_binding()) {
                    return Some(b);
                }
            } else if let Some(b) = parent.child("leftOperand").and_then(|n| n.type_binding()) {
                return Some(b);
            }
            if op != "==" && op != "!=" {
                return wk("int");
            }
            None
        }
        NodeKind::InstanceofExpression => parent.child("rightOperand").and_then(|n| n.binding()),
        NodeKind::VariableDeclarationFragment => {
            if node.location_is("initializer") {
                parent.child("name").and_then(|n| n.type_binding())
            } else {
                None
            }
        }
        NodeKind::SuperMethodInvocation | NodeKind::MethodInvocation | NodeKind::SuperConstructorInvocation | NodeKind::ConstructorInvocation | NodeKind::ClassInstanceCreation => {
            if node.location_is("arguments") {
                argument_parameter(node, parent, parent.method_binding())
            } else {
                None
            }
        }
        NodeKind::ParenthesizedExpression => guess_binding_for_reference(parent),
        NodeKind::ArrayAccess => {
            if node.location_is("index") {
                wk("int")
            } else {
                let p = possible_reference_binding(parent).or_else(|| wk("java.lang.Object"))?;
                types::array_of(p)
            }
        }
        NodeKind::ArrayCreation => {
            if node.location_is("dimensions") {
                wk("int")
            } else {
                None
            }
        }
        NodeKind::ConditionalExpression => {
            if node.location_is("expression") {
                return wk("boolean");
            }
            if let Some(b) = possible_reference_binding(parent).filter(|b| !b.is_null_type()) {
                return Some(b);
            }
            if node.location_is("thenExpression") {
                if let Some(b) = parent.child("elseExpression").and_then(|n| n.type_binding()).filter(|b| !b.is_null_type()) {
                    return Some(b);
                }
            }
            if node.location_is("elseExpression") {
                if let Some(b) = parent.child("thenExpression").and_then(|n| n.type_binding()).filter(|b| !b.is_null_type()) {
                    return Some(b);
                }
            }
            possible_reference_binding(parent)
        }
        NodeKind::PostfixExpression => wk("int"),
        NodeKind::PrefixExpression => {
            if parent.simple("operator") == Some("!") {
                wk("boolean")
            } else {
                wk("int")
            }
        }
        NodeKind::EnhancedForStatement => {
            if node.location_is("expression") {
                let t = parent.child("parameter").and_then(|p| p.child("type")).and_then(|t| t.binding())?;
                types::array_of(t)
            } else {
                None
            }
        }
        NodeKind::IfStatement | NodeKind::WhileStatement | NodeKind::DoStatement => wk("boolean"),
        NodeKind::SwitchStatement => {
            if node.location_is("expression") {
                wk("int")
            } else {
                None
            }
        }
        NodeKind::ReturnStatement => {
            if let Some(decl) = find_enclosing_method(parent) {
                if !decl.flag("constructor") {
                    return decl.child("returnType2").and_then(|t| t.binding());
                }
                return None;
            }
            let mut n = parent.parent();
            while let Some(x) = n {
                if x.is(NodeKind::LambdaExpression) {
                    return x.method_binding().or_else(|| x.binding()).and_then(|m| m.return_type());
                }
                n = x.parent();
            }
            None
        }
        NodeKind::CastExpression => parent.child("type").and_then(|t| t.binding()),
        NodeKind::ThrowStatement | NodeKind::CatchClause => wk("java.lang.Exception"),
        NodeKind::FieldAccess => {
            if node.location_is("name") {
                possible_reference_binding(parent)
            } else {
                None
            }
        }
        NodeKind::SuperFieldAccess => possible_reference_binding(parent),
        NodeKind::QualifiedName => {
            if node.location_is("name") {
                possible_reference_binding(parent)
            } else {
                None
            }
        }
        NodeKind::SwitchCase => {
            let switch = parent.parent()?;
            switch.child("expression").and_then(|e| e.type_binding())
        }
        NodeKind::AssertStatement => {
            if node.location_is("expression") {
                wk("boolean")
            } else {
                wk("java.lang.String")
            }
        }
        NodeKind::SingleMemberAnnotation => annotation_member(parent, "value").and_then(|m| m.return_type()),
        NodeKind::MemberValuePair => {
            let name = parent.child("name").map(|n| n.identifier()).unwrap_or_default();
            annotation_member(parent.parent()?, &name).and_then(|m| m.return_type())
        }
        _ => None,
    }
}

/// `ASTResolving.findAnnotationMember`.
pub fn annotation_member<'a>(annotation: Node<'a>, name: &str) -> Option<BindingRef<'a>> {
    let t = annotation.child("typeName").and_then(|n| n.binding()).filter(|b| b.is_type())?;
    t.declared_methods().unwrap_or_default().into_iter().find(|m| m.name() == name && m.parameter_types().is_empty())
}

/// `ASTResolving.getParentMethodOrTypeBinding`.
pub fn parent_method_or_type_binding<'a>(node: Node<'a>) -> Option<BindingRef<'a>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.is(NodeKind::MethodDeclaration) || x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        n = x.parent();
    }
    None
}

/// `ASTResolving.getQualifierGuess(searchRoot, selector, arguments, context)`.
pub fn qualifier_guess<'a>(root: Node<'a>, selector: &str, n_args: usize, context: Option<BindingRef<'a>>) -> Vec<BindingRef<'a>> {
    let ast = root.ast;
    if let Some(object) = well_known(ast, "java.lang.Object") {
        if object.declared_methods().unwrap_or_default().iter().any(|m| m.name() == selector && m.parameter_types().len() == n_args) {
            return vec![object];
        }
    }
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut visit = |t: BindingRef<'a>, result: &mut Vec<BindingRef<'a>>| {
        let Some(t) = normalize(Some(t)) else { return };
        if !visited.insert(t.key().to_owned()) {
            return;
        }
        if t.is_generic_type() {
            return;
        }
        if let Some(c) = context {
            if !types::is_useable_in_context(t, c, false) {
                return;
            }
        }
        for m in t.declared_methods().unwrap_or_default() {
            if m.name() == selector && m.parameter_types().len() == n_args {
                result.push(t);
            }
        }
    };
    for n in root.descendants() {
        if !n.is(NodeKind::SimpleName) {
            continue;
        }
        let Some(t) = n.type_binding() else { continue };
        visit(t, &mut result);
        // Bindings.visitHierarchy
        let mut seen = HashSet::new();
        let mut stack = vec![t];
        fn supers<'a>(t: BindingRef<'a>, seen: &mut HashSet<String>, out: &mut Vec<BindingRef<'a>>) {
            if !seen.insert(t.key().to_owned()) {
                return;
            }
            out.push(t);
            if let Some(s) = t.superclass() {
                supers(s, seen, out);
            }
            for i in t.interfaces() {
                supers(i, seen, out);
            }
        }
        let mut all = Vec::new();
        supers(stack.pop().unwrap(), &mut seen, &mut all);
        for s in all.into_iter().skip(1) {
            visit(s, &mut result);
        }
    }
    result
}

/// `ScopeAnalyzerVisitor` (VARIABLES): local declarations visible at `position`.
struct LocalVisitor<'a> {
    position: usize,
    out: Vec<BindingRef<'a>>,
}

impl<'a> LocalVisitor<'a> {
    fn inside(&self, n: Node<'_>) -> bool {
        n.start() <= self.position && self.position < n.start() + n.length()
    }

    fn backwards(&mut self, list: Vec<Node<'a>>) {
        for n in list.into_iter().rev() {
            if n.start() < self.position {
                self.visit(n);
            }
        }
    }

    fn children(&mut self, n: Node<'a>) {
        for c in n.children() {
            self.visit(c);
        }
    }

    fn visit(&mut self, n: Node<'a>) {
        match n.kind() {
            NodeKind::MethodDeclaration => {
                if self.inside(n) {
                    if let Some(body) = n.child("body") {
                        self.visit(body);
                    }
                    self.backwards(n.list("parameters"));
                }
            }
            NodeKind::SwitchCase => {}
            NodeKind::Initializer | NodeKind::FieldDeclaration => {
                if self.inside(n) {
                    self.children(n);
                }
            }
            NodeKind::Block => {
                if self.inside(n) {
                    self.backwards(n.list("statements"));
                }
            }
            NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression => self.backwards(n.list("fragments")),
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => {
                if n.start() < self.position {
                    if let Some(b) = n.binding() {
                        self.out.push(b);
                    }
                }
                self.children(n);
            }
            NodeKind::CatchClause => {
                if self.inside(n) {
                    if let Some(b) = n.child("body") {
                        self.visit(b);
                    }
                    if let Some(e) = n.child("exception") {
                        self.visit(e);
                    }
                }
            }
            NodeKind::ForStatement => {
                if self.inside(n) {
                    if let Some(b) = n.child("body") {
                        self.visit(b);
                    }
                    self.backwards(n.list("initializers"));
                }
            }
            k if k.is_expression() || k.is_statement() => {
                if self.inside(n) {
                    self.children(n);
                }
            }
            _ => {}
        }
    }
}

/// `new ScopeAnalyzer(root).getDeclarationsInScope(node.getStartPosition(), VARIABLES)`.
pub fn variables_in_scope<'a>(node: Node<'a>) -> Vec<BindingRef<'a>> {
    let offset = node.start();
    let covering = crate::semantic_ast::finder::NodeFinder::new(node.ast.root(), offset, 0).covering.unwrap_or(node);
    let mut names = HashSet::new();
    let mut result: Vec<BindingRef<'a>> = Vec::new();
    let mut accept = |b: BindingRef<'a>| {
        if names.insert(format!("V{}", b.name())) {
            result.push(b);
        }
    };
    if covering.is(NodeKind::SimpleName) {
        if let Some(q) = qualifier(covering) {
            let mut visited = HashSet::new();
            add_inherited_fields(q, &mut visited, &mut accept);
            return result;
        }
    }
    if let Some(decl) = crate::semantic_ast::resolve::find_parent_body_declaration(covering) {
        if matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer | NodeKind::FieldDeclaration) {
            let mut v = LocalVisitor { position: offset, out: Vec::new() };
            v.visit(decl);
            for b in v.out {
                accept(b);
            }
        }
    }
    if let Some(t) = types::parent_type_binding(covering) {
        let mut visited = HashSet::new();
        let mut current = Some(t);
        while let Some(c) = current {
            add_inherited_fields(c, &mut visited, &mut accept);
            current = if c.is_local() {
                c.declaring_node().and_then(|n| n.parent()).and_then(types::parent_type_binding)
            } else {
                c.declaring_class()
            };
        }
    }
    result
}

fn add_inherited_fields<'a>(t: BindingRef<'a>, visited: &mut HashSet<String>, accept: &mut dyn FnMut(BindingRef<'a>)) {
    if !visited.insert(t.key().to_owned()) {
        return;
    }
    for f in t.declared_fields().unwrap_or_default() {
        accept(f);
    }
    if let Some(s) = t.superclass() {
        add_inherited_fields(s, visited, accept);
    }
    for i in t.interfaces() {
        add_inherited_fields(i, visited, accept);
    }
}
