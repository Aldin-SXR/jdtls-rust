//! Port of jdt's `ContextSensitiveImportRewriteContext` (with the
//! `ScopeAnalyzer.getDeclarationsInScope(offset, METHODS | TYPES | VARIABLES)`
//! and `ImportReferencesCollector` data it consults), which jdt.ls uses for
//! the imports of `completionItem/resolve`.
//!
//! The data is collected once from the resolved AST of the unit; the
//! `findInContext` decisions run over plain names.

use super::imports::{ImportRewrite, RES_NAME_CONFLICT, RES_NAME_FOUND};
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::{modifier, BindingRef, Node, NodeKind};
use std::collections::HashSet;

const METHODS: i32 = 1;
const VARIABLES: i32 = 2;
const TYPES: i32 = 4;

/// A declaration visible at the offset (`IBinding`): its simple name and,
/// for a type, its qualified name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDeclaration {
    pub name: String,
    pub type_qualified_name: Option<String>,
}

/// A member type (at any depth) of a top-level type of the unit.
#[derive(Debug, Clone, Default)]
pub struct MemberType {
    pub qualified_name: String,
    /// A type on the declaring chain below the top-level type is private.
    pub private_on_chain: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TopLevelType {
    pub qualified_name: String,
    pub members: Vec<MemberType>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportContext {
    pub declarations: Vec<ScopeDeclaration>,
    /// Type declarations (`(simple name, qualified name)`) of the unit's type references.
    pub imported_types: Vec<(String, String)>,
    pub types: Vec<TopLevelType>,
    /// Types of the unit's package (for `java.lang` conflicts), when known.
    pub package_types: Option<HashSet<String>>,
}

impl ImportContext {
    pub fn collect(root: Node<'_>, offset: usize) -> Self {
        let mut ctx = ImportContext {
            declarations: declarations_in_scope(root, offset, METHODS | TYPES | VARIABLES),
            ..Default::default()
        };
        for name in crate::features::organize_imports::operation::type_import_references(root) {
            let Some(b) = name.binding() else { continue };
            if !b.is_type() || b.is_recovered() {
                continue;
            }
            let decl = b.type_declaration().unwrap_or(b);
            ctx.imported_types.push((decl.name().to_owned(), decl.qualified_name().to_owned()));
        }
        for t in root.list("types") {
            let Some(b) = t.binding() else { continue };
            let mut top = TopLevelType { qualified_name: b.qualified_name().to_owned(), members: Vec::new() };
            collect_members(b, b.key(), &mut top.members, &mut HashSet::new());
            ctx.types.push(top);
        }
        ctx
    }

    /// `ContextSensitiveImportRewriteContext.findInContext`.
    pub fn find_in_context(&self, rw: &ImportRewrite, qualifier: &str, name: &str, kind: i32) -> i32 {
        let qualified = if qualifier.is_empty() { name.to_owned() } else { format!("{qualifier}.{name}") };
        for d in &self.declarations {
            match &d.type_qualified_name {
                Some(q) if *q == qualified => return RES_NAME_FOUND,
                _ if d.name == name => return RES_NAME_CONFLICT,
                _ => {}
            }
        }
        for (simple, q) in &self.imported_types {
            if *q != qualified && simple == name {
                return RES_NAME_CONFLICT;
            }
        }
        for t in &self.types {
            if t.qualified_name == qualified {
                return RES_NAME_FOUND;
            }
            if let Some(m) = t.members.iter().find(|m| m.qualified_name == qualified) {
                if m.private_on_chain {
                    return RES_NAME_CONFLICT;
                }
            }
        }
        for added in rw.added_imports() {
            if added == qualified {
                return RES_NAME_FOUND;
            } else if added.rsplit('.').next() == Some(name) {
                return RES_NAME_CONFLICT;
            }
        }
        if qualifier == "java.lang" {
            if let Some(types) = &self.package_types {
                if types.contains(name) {
                    return RES_NAME_CONFLICT;
                }
            }
        }
        rw.find_in_imports(qualifier, name, kind)
    }
}

/// `containingDeclaration` + the private-modifier walk, precomputed for every
/// member type.
fn collect_members(binding: BindingRef<'_>, top_key: &str, out: &mut Vec<MemberType>, seen: &mut HashSet<String>) {
    if !seen.insert(binding.key().to_owned()) {
        return;
    }
    for child in binding.declared_types().unwrap_or_default() {
        let mut private_on_chain = false;
        let mut decl = Some(child);
        while let Some(d) = decl {
            if d.key() == top_key {
                break;
            }
            if d.modifiers() & modifier::PRIVATE != 0 {
                private_on_chain = true;
                break;
            }
            decl = d.declaring_class();
        }
        out.push(MemberType { qualified_name: child.qualified_name().to_owned(), private_on_chain });
        collect_members(child, top_key, out, seen);
    }
}

// ─── ScopeAnalyzer ───────────────────────────────────────────────────────────

/// `ScopeAnalyzer.DefaultBindingRequestor` (no visibility filtering).
#[derive(Default)]
struct Requestor {
    result: Vec<ScopeDeclaration>,
    names: HashSet<String>,
}

impl Requestor {
    fn accept(&mut self, binding: Option<BindingRef<'_>>) {
        let Some(b) = binding else { return };
        let signature = if b.is_method() {
            let params: Vec<String> = b
                .parameter_types()
                .iter()
                .map(|p| p.erasure().unwrap_or(*p).qualified_name().to_owned())
                .collect();
            format!("M{}({})", b.name(), params.join(","))
        } else if b.is_variable() {
            format!("V{}", b.name())
        } else if b.is_type() {
            format!("T{}", b.name())
        } else {
            return;
        };
        if self.names.insert(signature) {
            self.result.push(ScopeDeclaration {
                name: b.name().to_owned(),
                type_qualified_name: b.is_type().then(|| b.qualified_name().to_owned()),
            });
        }
    }
}

struct Analyzer {
    types_visited: HashSet<String>,
}

/// `Bindings.getBindingOfParentType(node)`.
fn binding_of_parent_type(node: Node<'_>) -> Option<BindingRef<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        n = x.parent();
    }
    None
}

fn has(flag: i32, flags: i32) -> bool {
    flags & flag != 0
}

impl Analyzer {
    fn add_inherited(&mut self, binding: BindingRef<'_>, is_super_interface: bool, flags: i32, r: &mut Requestor) {
        if !self.types_visited.insert(binding.key().to_owned()) {
            return;
        }
        if has(VARIABLES, flags) {
            for f in binding.declared_fields().unwrap_or_default() {
                r.accept(Some(f));
            }
        }
        if has(METHODS, flags) {
            for m in binding.declared_methods().unwrap_or_default() {
                if is_super_interface && m.is_static() {
                    continue;
                }
                if !m.is_constructor() {
                    r.accept(Some(m));
                }
            }
        }
        if has(TYPES, flags) {
            for t in binding.declared_types().unwrap_or_default() {
                r.accept(Some(t));
            }
        }
        if let Some(s) = binding.superclass() {
            self.add_inherited(s, false, flags, r);
        }
        for i in binding.interfaces() {
            self.add_inherited(i, true, flags, r);
        }
    }

    fn add_type_declarations(&mut self, root: Node<'_>, binding: BindingRef<'_>, flags: i32, r: &mut Requestor) {
        if has(TYPES, flags) && !binding.is_anonymous() {
            r.accept(Some(binding));
            for p in binding.type_parameters() {
                r.accept(Some(p));
            }
        }
        self.add_inherited(binding, false, flags, r);
        if binding.is_local() {
            // addOuterDeclarationsForLocalType
            if let Some(node) = binding.declaring_node() {
                if node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration) {
                    if let Some(parent) = node.parent() {
                        add_local_declarations(parent, parent.start(), flags, r);
                        if let Some(p) = binding_of_parent_type(parent) {
                            self.add_type_declarations(root, p, flags, r);
                        }
                    }
                }
            }
        } else if let Some(declaring) = binding.declaring_class() {
            self.add_type_declarations(root, declaring, flags, r);
        } else if has(TYPES, flags) && binding.declaring_node().is_some() {
            for t in root.list("types") {
                r.accept(t.binding());
            }
        }
    }
}

/// `ScopeAnalyzer.getDeclarationsInScope(offset, flags)`.
pub fn declarations_in_scope(root: Node<'_>, offset: usize, flags: i32) -> Vec<ScopeDeclaration> {
    let Some(node) = NodeFinder::new(root, offset, 0).covering else { return Vec::new() };
    let mut r = Requestor::default();
    let mut a = Analyzer { types_visited: HashSet::new() };
    if node.is(NodeKind::SimpleName) {
        if let Some(parent_type) = binding_of_parent_type(node) {
            match qualifier_type(node) {
                None => {
                    add_local_declarations(node, node.start(), flags, &mut r);
                    a.add_type_declarations(root, parent_type, flags, &mut r);
                }
                Some(q) => a.add_inherited(q, false, flags, &mut r),
            }
        }
        return r.result;
    }
    let binding = binding_of_parent_type(node);
    add_local_declarations(node, offset, flags, &mut r);
    if let Some(b) = binding {
        a.add_type_declarations(root, b, flags, &mut r);
    }
    r.result
}

/// `ScopeAnalyzer.getQualifier(selector)`.
fn qualifier_type(selector: Node<'_>) -> Option<BindingRef<'_>> {
    let parent = selector.parent()?;
    let is_name = |prop: &str| parent.child(prop).is_some_and(|n| n.id == selector.id);
    match parent.kind() {
        NodeKind::MethodInvocation if is_name("name") => parent.child("expression")?.type_binding(),
        NodeKind::QualifiedName if is_name("name") => parent.child("qualifier")?.type_binding(),
        NodeKind::FieldAccess if is_name("name") => parent.child("expression")?.type_binding(),
        NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation if is_name("name") => {
            binding_of_parent_type(parent)?.superclass()
        }
        _ => {
            if parent.kind().is_type() {
                let normalized = crate::semantic_ast::resolve::normalized_node(parent);
                if normalized.location_is("type") && normalized.parent().is_some_and(|p| p.is(NodeKind::ClassInstanceCreation)) {
                    return normalized.parent()?.child("expression")?.type_binding();
                }
            }
            None
        }
    }
}

/// `ScopeAnalyzer.addLocalDeclarations(node, offset, flags, requestor)`.
fn add_local_declarations(node: Node<'_>, offset: usize, flags: i32, r: &mut Requestor) {
    if !(has(VARIABLES, flags) || has(TYPES, flags)) {
        return;
    }
    let Some(declaration) = crate::semantic_ast::resolve::find_parent_body_declaration(node) else { return };
    if matches!(declaration.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer | NodeKind::FieldDeclaration) {
        let mut v = ScopeVisitor { position: offset, flags, brk: false };
        v.accept(declaration, r);
    }
}

/// `ScopeAnalyzer.ScopeAnalyzerVisitor`.
struct ScopeVisitor {
    position: usize,
    flags: i32,
    brk: bool,
}

impl ScopeVisitor {
    fn is_inside(&self, node: Node<'_>) -> bool {
        node.start() <= self.position && self.position < node.start() + node.length()
    }

    fn visit_backwards(&mut self, list: Vec<Node<'_>>, r: &mut Requestor) {
        if self.brk {
            return;
        }
        for n in list.into_iter().rev() {
            if n.start() < self.position {
                self.accept(n, r);
            }
        }
    }

    fn descend(&mut self, node: Node<'_>, r: &mut Requestor) {
        for c in node.children() {
            self.accept(c, r);
        }
    }

    fn accept(&mut self, node: Node<'_>, r: &mut Requestor) {
        let k = node.kind();
        match k {
            NodeKind::MethodDeclaration => {
                if self.is_inside(node) {
                    if let Some(body) = node.child("body") {
                        self.accept(body, r);
                    }
                    self.visit_backwards(node.list("parameters"), r);
                    self.visit_backwards(node.list("typeParameters"), r);
                }
            }
            NodeKind::TypeParameter => {
                if has(TYPES, self.flags) && node.start() < self.position {
                    r.accept(node.child("name").and_then(|n| n.binding()));
                }
                self.descend(node, r);
            }
            NodeKind::Initializer | NodeKind::FieldDeclaration => {
                if !self.brk && self.is_inside(node) {
                    self.descend(node, r);
                }
            }
            NodeKind::Block => {
                if self.is_inside(node) {
                    self.visit_backwards(node.list("statements"), r);
                }
            }
            NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression => {
                self.visit_backwards(node.list("fragments"), r);
            }
            NodeKind::CatchClause => {
                if self.is_inside(node) {
                    if let Some(b) = node.child("body") {
                        self.accept(b, r);
                    }
                    if let Some(e) = node.child("exception") {
                        self.accept(e, r);
                    }
                }
            }
            NodeKind::ForStatement => {
                if self.is_inside(node) {
                    if let Some(b) = node.child("body") {
                        self.accept(b, r);
                    }
                    self.visit_backwards(node.list("initializers"), r);
                }
            }
            NodeKind::TypeDeclarationStatement => {
                if has(TYPES, self.flags) && node.start() + node.length() < self.position {
                    r.accept(node.child("declaration").and_then(|d| d.binding()));
                } else if !self.brk && self.is_inside(node) {
                    self.descend(node, r);
                }
            }
            _ if k.is_variable_declaration() => {
                if has(VARIABLES, self.flags) && node.start() < self.position {
                    r.accept(node.binding());
                }
                self.descend(node, r);
            }
            _ if k.is_expression() || k.is_statement() => {
                if !self.brk && self.is_inside(node) {
                    self.descend(node, r);
                }
            }
            _ => {}
        }
    }
}
