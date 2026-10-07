//! Port of `org.eclipse.jdt.internal.corext.dom.ScopeAnalyzer`.

use std::collections::HashSet;

use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::resolve::{find_parent_body_declaration, find_parent_statement};
use crate::semantic_ast::{modifier, BindingKind, BindingRef, Node, NodeKind};

pub const METHODS: i32 = 1;
pub const VARIABLES: i32 = 2;
pub const TYPES: i32 = 4;
pub const NO_FIELDS: i32 = 8;
pub const CHECK_VISIBILITY: i32 = 16;

fn has(flag: i32, flags: i32) -> bool {
    flags & flag != 0
}

/// `Bindings.getBindingOfParentType(node)`.
pub fn binding_of_parent_type(node: Node<'_>) -> Option<BindingRef<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        n = x.parent();
    }
    None
}

/// `ScopeAnalyzer.getSignature(binding, flags)`.
fn signature(b: BindingRef<'_>, flags: i32) -> Option<String> {
    match b.kind() {
        BindingKind::Method => {
            let params: Vec<String> = b
                .parameter_types()
                .iter()
                .map(|p| p.erasure().unwrap_or(*p).qualified_name().to_owned())
                .collect();
            Some(format!("M{}({})", b.name(), params.join(",")))
        }
        BindingKind::Variable => {
            if has(NO_FIELDS, flags) && b.is_field() {
                Some(format!("F{}", b.name()))
            } else {
                Some(format!("V{}", b.name()))
            }
        }
        BindingKind::Type => Some(format!("T{}", b.name())),
        _ => None,
    }
}

/// `DefaultBindingRequestor`.
struct Requestor<'a> {
    parent: Option<BindingRef<'a>>,
    flags: i32,
    result: Vec<BindingRef<'a>>,
    names: HashSet<String>,
}

impl<'a> Requestor<'a> {
    fn new(parent: Option<BindingRef<'a>>, flags: i32) -> Self {
        Requestor { parent, flags, result: Vec::new(), names: HashSet::new() }
    }

    fn accept(&mut self, b: BindingRef<'a>) -> bool {
        if let Some(sig) = signature(b, self.flags) {
            if self.names.insert(sig) {
                self.result.push(b);
            }
        }
        false
    }

    fn result(mut self) -> Vec<BindingRef<'a>> {
        if has(CHECK_VISIBILITY, self.flags) {
            if let Some(parent) = self.parent {
                self.result.retain(|b| is_visible(*b, parent));
            }
        }
        if has(NO_FIELDS, self.flags) {
            self.result.retain(|b| !(b.is_variable() && b.is_field()));
        }
        self.result
    }
}

/// `ScopeAnalyzer.isVisible(binding, context)`.
pub fn is_visible(binding: BindingRef<'_>, context: BindingRef<'_>) -> bool {
    if binding.is_variable() && !binding.is_field() {
        return true;
    }
    let declaring = match binding.kind() {
        BindingKind::Variable | BindingKind::Method => binding.declaring_class(),
        BindingKind::Type => Some(binding),
        _ => None,
    };
    let Some(declaring) = declaring else { return false };
    let declaring = declaring.type_declaration().unwrap_or(declaring);
    let modifiers = binding.modifiers();
    let context_modifiers = context.modifiers();
    if context.is_class() && context_modifiers & modifier::STATIC != 0 && binding.is_variable() && modifiers & modifier::STATIC == 0 {
        return context.key() == declaring.key();
    }
    if modifiers & modifier::PUBLIC != 0 || declaring.is_interface() {
        true
    } else if modifiers & modifier::PROTECTED != 0 || modifiers & modifier::PRIVATE == 0 {
        if declaring.package_name() == context.package_name() {
            return true;
        }
        is_type_in_scope(declaring, context, modifiers & modifier::PROTECTED != 0)
    } else {
        is_type_in_scope(declaring, context, false)
    }
}

fn is_type_in_scope(declaring: BindingRef<'_>, context: BindingRef<'_>, include_hierarchy: bool) -> bool {
    let mut curr = Some(context.type_declaration().unwrap_or(context));
    let mut guard = 0;
    while let Some(c) = curr {
        if c.key() == declaring.key() {
            return true;
        }
        if include_hierarchy && is_in_super_type_hierarchy(declaring, c, &mut HashSet::new()) {
            return true;
        }
        curr = c.declaring_class();
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    false
}

fn is_in_super_type_hierarchy(possible: BindingRef<'_>, typ: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
    if typ.key() == possible.key() {
        return true;
    }
    if !seen.insert(typ.key().to_owned()) {
        return false;
    }
    if let Some(s) = typ.superclass() {
        if is_in_super_type_hierarchy(possible, s.type_declaration().unwrap_or(s), seen) {
            return true;
        }
    }
    if possible.is_interface() {
        for i in typ.interfaces() {
            if is_in_super_type_hierarchy(possible, i.type_declaration().unwrap_or(i), seen) {
                return true;
            }
        }
    }
    false
}

/// `ScopeAnalyzer`.
pub struct ScopeAnalyzer<'a> {
    root: Node<'a>,
    visited: HashSet<String>,
}

impl<'a> ScopeAnalyzer<'a> {
    pub fn new(root: Node<'a>) -> Self {
        ScopeAnalyzer { root, visited: HashSet::new() }
    }

    fn add_inherited(&mut self, b: BindingRef<'a>, is_super_interface: bool, flags: i32, req: &mut Requestor<'a>) -> bool {
        if !self.visited.insert(b.key().to_owned()) {
            return false;
        }
        if has(VARIABLES, flags) {
            for f in b.declared_fields().unwrap_or_default() {
                if req.accept(f) {
                    return true;
                }
            }
        }
        if has(METHODS, flags) {
            for m in b.declared_methods().unwrap_or_default() {
                if is_super_interface && m.is_static() {
                    continue;
                }
                if !m.has(crate::semantic_ast::bflag::SYNTHETIC) && !m.is_constructor() && req.accept(m) {
                    return true;
                }
            }
        }
        if has(TYPES, flags) {
            for t in b.declared_types().unwrap_or_default() {
                if req.accept(t) {
                    return true;
                }
            }
        }
        if let Some(s) = b.superclass() {
            if self.add_inherited(s, false, flags, req) {
                return true;
            }
        } else if b.is_array() {
            if let Some(object) = self.root.ast.binding_by_key("Ljava/lang/Object;") {
                if self.add_inherited(object, false, flags, req) {
                    return true;
                }
            }
        }
        for i in b.interfaces() {
            if self.add_inherited(i, true, flags, req) {
                return true;
            }
        }
        false
    }

    fn add_type_declarations(&mut self, b: BindingRef<'a>, flags: i32, req: &mut Requestor<'a>) -> bool {
        if has(TYPES, flags) && !b.is_anonymous() {
            if req.accept(b) {
                return true;
            }
            for tp in b.type_parameters() {
                if req.accept(tp) {
                    return true;
                }
            }
        }
        self.add_inherited(b, false, flags, req);
        if b.is_local() {
            self.add_outer_declarations_for_local_type(b, flags, req);
        } else if let Some(declaring) = b.declaring_class() {
            if self.add_type_declarations(declaring, flags, req) {
                return true;
            }
        } else if has(TYPES, flags) && b.declaring_node().is_some() {
            for t in self.root.list("types") {
                if let Some(tb) = t.binding() {
                    if req.accept(tb) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn add_outer_declarations_for_local_type(&mut self, b: BindingRef<'a>, flags: i32, req: &mut Requestor<'a>) -> bool {
        let Some(node) = b.declaring_node() else { return false };
        if node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration) {
            let Some(parent) = node.parent() else { return false };
            if self.add_local_declarations(parent, parent.start(), flags, req) {
                return true;
            }
            if let Some(pt) = binding_of_parent_type(parent) {
                if self.add_type_declarations(pt, flags, req) {
                    return true;
                }
            }
        }
        false
    }

    fn add_local_declarations(&mut self, node: Node<'a>, offset: usize, flags: i32, req: &mut Requestor<'a>) -> bool {
        if has(VARIABLES, flags) || has(TYPES, flags) {
            if let Some(decl) = find_parent_body_declaration(node) {
                if matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer | NodeKind::FieldDeclaration) {
                    let mut v = ScopeVisitor { position: offset, flags, brk: false, req };
                    v.accept(decl);
                    return v.brk;
                }
            }
        }
        false
    }

    /// `getQualifier(selector)`.
    fn qualifier(selector: Node<'a>) -> Option<BindingRef<'a>> {
        let parent = selector.parent()?;
        let binding = |e: Option<Node<'a>>| e.and_then(|e| e.type_binding());
        match parent.kind() {
            NodeKind::MethodInvocation | NodeKind::FieldAccess if selector.location_is("name") => binding(parent.child("expression")),
            NodeKind::QualifiedName if selector.location_is("name") => binding(parent.child("qualifier")),
            NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation if selector.location_is("name") => {
                binding_of_parent_type(parent).and_then(|c| c.superclass())
            }
            k if k.is_type() => {
                let normalized = crate::semantic_ast::resolve::normalized_node(parent);
                if normalized.location_is("type") && normalized.parent().is_some_and(|p| p.is(NodeKind::ClassInstanceCreation)) {
                    return binding(normalized.parent().unwrap().child("expression"));
                }
                None
            }
            _ => None,
        }
    }

    /// `getDeclarationsInScope(SimpleName selector, flags)`.
    pub fn declarations_in_scope_of_name(&mut self, selector: Node<'a>, flags: i32) -> Vec<BindingRef<'a>> {
        let result = (|| {
            if selector.location_is("expression") || selector.location_is("expressions") {
                if let Some(case) = selector.parent().filter(|p| p.is(NodeKind::SwitchCase)) {
                    let switch = case.parent();
                    let b = switch.and_then(|s| s.child("expression")).and_then(|e| e.type_binding());
                    if let Some(b) = b.filter(|b| b.is_enum()) {
                        return b.declared_fields().unwrap_or_default().into_iter().filter(|f| f.is_enum_constant()).collect();
                    }
                }
            }
            let Some(parent_type) = binding_of_parent_type(selector) else { return Vec::new() };
            let mut req = Requestor::new(Some(parent_type), flags);
            match Self::qualifier(selector) {
                None => {
                    self.add_local_declarations(selector, selector.start(), flags, &mut req);
                    self.add_type_declarations(parent_type, flags, &mut req);
                }
                Some(q) => {
                    self.add_inherited(q, false, flags, &mut req);
                }
            }
            req.result()
        })();
        self.visited.clear();
        result
    }

    /// `getDeclarationsInScope(int offset, flags)`.
    pub fn declarations_in_scope(&mut self, offset: usize, flags: i32) -> Vec<BindingRef<'a>> {
        let Some(node) = NodeFinder::new(self.root, offset, 0).covering else { return Vec::new() };
        if node.is(NodeKind::SimpleName) {
            return self.declarations_in_scope_of_name(node, flags);
        }
        let binding = binding_of_parent_type(node);
        let mut req = Requestor::new(binding, flags);
        self.add_local_declarations(node, offset, flags, &mut req);
        if let Some(b) = binding {
            self.add_type_declarations(b, flags, &mut req);
        }
        self.visited.clear();
        req.result()
    }

    /// `getDeclarationsAfter(offset, flags)`.
    pub fn declarations_after(&mut self, offset: usize, flags: i32) -> Option<Vec<BindingRef<'a>>> {
        let node = NodeFinder::new(self.root, offset, 0).covering?;
        let mut declaration = find_parent_statement(node);
        while let Some(d) = declaration {
            if d.kind().is_statement() && !d.is(NodeKind::Block) {
                declaration = d.parent();
            } else {
                break;
            }
        }
        let Some(block) = declaration.filter(|d| d.is(NodeKind::Block)) else { return Some(Vec::new()) };
        let mut req = Requestor::new(None, flags);
        let position = node.start();
        let mut brk = false;
        let mut f = |n: Node<'a>| -> bool {
            if brk {
                return false;
            }
            match n.kind() {
                NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => {
                    if has(VARIABLES, flags) && position < n.start() {
                        if let Some(b) = n.binding() {
                            brk = req.accept(b);
                        }
                    }
                    false
                }
                NodeKind::AnonymousClassDeclaration => false,
                NodeKind::TypeDeclarationStatement => {
                    if has(TYPES, flags) && position < n.start() {
                        if let Some(b) = n.child("declaration").and_then(|d| d.binding()) {
                            brk = req.accept(b);
                        }
                    }
                    false
                }
                _ => true,
            }
        };
        super::walk(block, &mut f);
        self.visited.clear();
        Some(req.result())
    }

    /// `getUsedVariableNames(offset, length)`.
    pub fn used_variable_names(&mut self, offset: usize, length: usize) -> HashSet<String> {
        let mut result = HashSet::new();
        for b in self.declarations_in_scope(offset, VARIABLES | CHECK_VISIBILITY) {
            result.insert(b.name().to_owned());
        }
        for b in self.declarations_after(offset + length, VARIABLES | CHECK_VISIBILITY).unwrap_or_default() {
            result.insert(b.name().to_owned());
        }
        for import in self.root.list("imports") {
            if import.flag("static") && !import.flag("onDemand") {
                if let Some(name) = import.child("name") {
                    let id = name.identifier();
                    result.insert(id.rsplit('.').next().unwrap_or("").to_owned());
                }
            }
        }
        result
    }
}

/// `ScopeAnalyzerVisitor` (a `HierarchicalASTVisitor`).
struct ScopeVisitor<'r, 'a> {
    position: usize,
    flags: i32,
    brk: bool,
    req: &'r mut Requestor<'a>,
}

impl<'a> ScopeVisitor<'_, 'a> {
    fn inside(&self, n: Node<'_>) -> bool {
        n.start() <= self.position && self.position < n.end()
    }

    fn backwards(&mut self, list: Vec<Node<'a>>) {
        if self.brk {
            return;
        }
        for n in list.into_iter().rev() {
            if n.start() < self.position {
                self.accept(n);
            }
        }
    }

    fn children(&mut self, n: Node<'a>) {
        for c in n.children() {
            self.accept(c);
        }
    }

    fn accept(&mut self, n: Node<'a>) {
        match n.kind() {
            NodeKind::MethodDeclaration => {
                if self.inside(n) {
                    if let Some(body) = n.child("body") {
                        self.accept(body);
                    }
                    self.backwards(n.list("parameters"));
                    self.backwards(n.list("typeParameters"));
                }
            }
            NodeKind::TypeParameter => {
                if has(TYPES, self.flags) && n.start() < self.position {
                    if let Some(b) = n.child("name").and_then(|x| x.binding()) {
                        self.brk = self.req.accept(b);
                    }
                }
                if !self.brk {
                    self.children(n);
                }
            }
            NodeKind::SwitchCase => {
                if has(VARIABLES, self.flags) && !n.flag("default") {
                    let mut exprs = n.list("expressions");
                    if let Some(e) = n.child("expression") {
                        exprs.push(e);
                    }
                    for e in exprs {
                        if self.inside(e) {
                            let b = n.parent().and_then(|s| s.child("expression")).and_then(|e| e.type_binding());
                            if let Some(b) = b.filter(|b| b.is_enum()) {
                                for f in b.declared_fields().unwrap_or_default() {
                                    if f.is_enum_constant() {
                                        self.brk = self.req.accept(f);
                                        if self.brk {
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::Initializer | NodeKind::FieldDeclaration => {
                if !self.brk && self.inside(n) {
                    self.children(n);
                }
            }
            NodeKind::Block => {
                if self.inside(n) {
                    self.backwards(n.list("statements"));
                }
            }
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => {
                if has(VARIABLES, self.flags) && n.start() < self.position {
                    if let Some(b) = n.binding() {
                        self.brk = self.req.accept(b);
                    }
                }
                if !self.brk {
                    self.children(n);
                }
            }
            NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression => {
                self.backwards(n.list("fragments"));
            }
            NodeKind::CatchClause => {
                if self.inside(n) {
                    if let Some(b) = n.child("body") {
                        self.accept(b);
                    }
                    if let Some(e) = n.child("exception") {
                        self.accept(e);
                    }
                }
            }
            NodeKind::ForStatement => {
                if self.inside(n) {
                    if let Some(b) = n.child("body") {
                        self.accept(b);
                    }
                    self.backwards(n.list("initializers"));
                }
            }
            NodeKind::TypeDeclarationStatement => {
                if has(TYPES, self.flags) && n.end() < self.position {
                    if let Some(b) = n.child("declaration").and_then(|d| d.binding()) {
                        self.brk = self.req.accept(b);
                    }
                    return;
                }
                if !self.brk && self.inside(n) {
                    self.children(n);
                }
            }
            k if k.is_expression() || k.is_statement() => {
                if !self.brk && self.inside(n) {
                    self.children(n);
                }
            }
            _ => {}
        }
    }
}
