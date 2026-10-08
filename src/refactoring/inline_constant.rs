//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.InlineConstantRefactoring`.
//! The import remover and explicit type arguments of inferred invocations
//! are not ported.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Url};

use super::extract_temp::{import_context, CuRewrite};
use super::{msg, Status, Visitor};
use crate::analysis::dispatcher::Dispatcher;
use crate::correction::edit::Env;
use crate::correction::parentheses::{needs_parentheses, needs_parentheses_for_cast};
use crate::correction::CuChange;
use crate::rewrite::import_rewrite::{ImportRewriteContext, TypeLocation};
use crate::rewrite::{indent, ASTRewrite, RNode};
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::{Ast, BindingRef, Node, NodeId, NodeKind};

pub struct InlineConstant {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    selected: NodeId,
    declaration_ast: Arc<Ast>,
    declaration: NodeId,
    declaration_selected: bool,
    remove_declaration: bool,
    replace_all_references: bool,
}

/// `findConstantNameNode()`.
fn find_constant_name(ast: &Ast, start: usize, length: usize) -> Option<NodeId> {
    let mut node = NodeFinder::perform(ast.root(), start, length)?;
    if node.is(NodeKind::FieldAccess) {
        node = node.child("name")?;
    }
    if !matches!(node.kind(), NodeKind::SimpleName | NodeKind::QualifiedName) {
        return None;
    }
    let binding = node.binding().filter(|b| b.is_variable())?;
    if !binding.is_field() || binding.is_enum_constant() {
        return None;
    }
    let modifiers = binding.modifiers();
    let (is_static, is_final) = (modifiers & crate::semantic_ast::modifier::STATIC != 0, modifiers & crate::semantic_ast::modifier::FINAL != 0);
    (is_static && is_final).then_some(node.id)
}

fn location_range(ast: &Ast, location: &Location) -> Option<(usize, usize)> {
    let start = ast.offset_of(location.range.start)?;
    let end = ast.offset_of(location.range.end)?;
    Some((start, end.saturating_sub(start)))
}

impl InlineConstant {
    /// Creates the refactoring and runs `checkInitialConditions`.
    pub async fn create(env: &Env<'_>, ast: &Arc<Ast>, options: BTreeMap<String, String>, start: usize, length: usize) -> Result<InlineConstant, Status> {
        let Some(selected) = find_constant_name(ast, start, length) else {
            return Err(Status::fatal(msg("InlineConstantRefactoring_static_final_field")));
        };
        let name = ast.node(selected);
        let binding = name.binding().expect("field binding");
        let mut declaring = binding.declaring_class();
        while let Some(c) = declaring {
            if c.is_local() || c.is_anonymous() {
                return Err(Status::fatal(msg("InlineConstantRefactoring_local_anonymous_unsupported")));
            }
            declaring = c.declaring_class();
        }
        let mut declaration_selected = false;
        let mut found: Option<(Arc<Ast>, NodeId)> = None;
        if let Some(parent) = name.parent().filter(|p| p.is(NodeKind::VariableDeclarationFragment)) {
            if parent.child("name").is_some_and(|n| n.id == name.id) {
                declaration_selected = true;
                found = Some((ast.clone(), parent.id));
            }
        }
        if found.is_none() {
            if let Some(decl) = binding.declaring_node().filter(|d| d.is(NodeKind::VariableDeclarationFragment)) {
                found = Some((ast.clone(), decl.id));
            }
        }
        if found.is_none() {
            found = definition_fragment(env, ast, name).await;
        }
        let Some((declaration_ast, declaration)) = found else {
            return Err(Status::fatal(msg("InlineConstantRefactoring_binary_file")));
        };
        if declaration_ast.node(declaration).child("initializer").is_none() {
            return Err(Status::fatal(msg("InlineConstantRefactoring_blank_finals")));
        }
        Ok(InlineConstant {
            ast: ast.clone(),
            options,
            selected,
            declaration_ast,
            declaration,
            declaration_selected,
            remove_declaration: false,
            replace_all_references: true,
        })
    }

    pub fn is_declaration_selected(&self) -> bool {
        self.declaration_selected
    }

    pub fn set_remove_declaration(&mut self, v: bool) {
        self.remove_declaration = v;
    }

    pub fn set_replace_all_references(&mut self, v: bool) {
        self.replace_all_references = v;
    }

    /// `getReferences(pm, status)`: the reference names per unit.
    pub async fn references(&self, env: &Env<'_>) -> Vec<(Arc<Ast>, Vec<NodeId>)> {
        let decl_ast = &self.declaration_ast;
        let Some(name) = decl_ast.node(self.declaration).child("name") else { return Vec::new() };
        let Ok(uri) = Url::parse(&decl_ast.uri) else { return Vec::new() };
        let position = decl_ast.position(name.start());
        let Some(locations) = crate::features::navigation::references(env.dispatcher, &uri, position, false).await else { return Vec::new() };
        let mut groups: Vec<(Arc<Ast>, Vec<NodeId>)> = Vec::new();
        for location in locations {
            let target = if location.uri.as_str() == self.ast.uri {
                self.ast.clone()
            } else if location.uri.as_str() == decl_ast.uri {
                decl_ast.clone()
            } else {
                match crate::semantic_ast::fetch(env.dispatcher, &location.uri).await {
                    Ok(a) => a,
                    Err(_) => continue,
                }
            };
            let Some((start, length)) = location_range(&target, &location) else { continue };
            let Some(node) = NodeFinder::perform(target.root(), start, length) else { continue };
            if !matches!(node.kind(), NodeKind::SimpleName | NodeKind::QualifiedName) || super::checks::is_inside_javadoc(node) {
                continue;
            }
            let id = node.id;
            match groups.iter_mut().find(|(a, _)| a.uri == target.uri) {
                Some((_, ids)) => ids.push(id),
                None => groups.push((target, vec![id])),
            }
        }
        groups
    }

    /// `checkFinalConditions`: the changes of every unit.
    pub async fn changes(&self, env: &Env<'_>, references: &[(Arc<Ast>, Vec<NodeId>)]) -> Vec<CuChange> {
        let initializer_ast = self.declaration_ast.clone();
        let declaration = initializer_ast.node(self.declaration);
        let initializer = declaration.child("initializer").expect("initializer");
        let static_imports = static_imports_in(initializer);
        let mut changes = Vec::new();
        let single = [(self.ast.clone(), vec![self.selected])];
        let groups: &[(Arc<Ast>, Vec<NodeId>)] = if self.replace_all_references { references } else { &single };
        for (ast, names) in groups {
            let options = env.options(&ast.uri).await;
            let names: Vec<Node<'_>> = names.iter().map(|id| ast.node(*id)).collect();
            if let Some(change) = self.target_change(env, ast, options, &names, &static_imports).await {
                changes.push(change);
            }
        }
        if self.remove_declaration && self.replace_all_references && !changes.iter().any(|c| c.ast.uri == initializer_ast.uri) {
            let options = env.options(&initializer_ast.uri).await;
            if let Some(change) = self.target_change(env, &initializer_ast, options, &[], &static_imports).await {
                changes.push(change);
            }
        }
        changes
    }

    async fn target_change(&self, env: &Env<'_>, ast: &Arc<Ast>, options: BTreeMap<String, String>, names: &[Node<'_>], static_imports: &HashSet<NodeId>) -> Option<CuChange> {
        let mut cu = CuRewrite::new(ast, &options);
        let initializer_ast = self.declaration_ast.clone();
        let declaration = initializer_ast.node(self.declaration);
        let initializer = declaration.child("initializer")?;
        let removes_here = self.remove_declaration && self.replace_all_references && ast.uri == initializer_ast.uri;
        for name in names {
            let reference = qualified_reference(*name);
            if reference.ancestors().any(|a| a.is(NodeKind::ImportDeclaration)) {
                continue;
            }
            let text = self.prepare_initializer_for_location(env, ast, &mut cu, &options, reference, initializer, static_imports).await;
            let rw = &mut cu.rewrite;
            let mut new_reference;
            let mut is_placeholder = false;
            if let Some(cast) = crate::correction::local_corrections::conversion::explicit_cast(initializer, reference) {
                let mut operand = rw.create_string_placeholder(&text, reference.kind());
                if needs_parentheses_for_cast(initializer, false) {
                    operand = rw.new_parenthesized_expression(operand);
                }
                let node = rw.new_node(NodeKind::CastExpression);
                rw.put_child(node, "expression", operand);
                let context = import_context(ast, reference, &options);
                let typ = cu.imports.add_import_type(cast, &mut cu.rewrite, &context, TypeLocation::Cast);
                cu.rewrite.put_child(node, "type", typ);
                new_reference = node;
            } else if initializer.is(NodeKind::ArrayInitializer) {
                let rw = &mut cu.rewrite;
                let creation = rw.new_node(NodeKind::ArrayCreation);
                let type_text = array_type_text(declaration);
                let array_type = rw.create_string_placeholder(&type_text, NodeKind::ArrayType);
                rw.put_child(creation, "type", array_type);
                let array_initializer = rw.create_string_placeholder(&text, NodeKind::ArrayInitializer);
                rw.put_child(creation, "initializer", array_initializer);
                new_reference = creation;
                if let Some(t) = declaration_type(declaration).and_then(|t| t.binding().or_else(|| t.type_binding())) {
                    let context = import_context(ast, reference, &options);
                    cu.imports.add_import_binding(t, &context);
                }
            } else {
                new_reference = cu.rewrite.create_string_placeholder(&text, reference.kind());
                is_placeholder = true;
            }
            let (parent, location) = (reference.parent(), reference.location());
            let needs = match (parent, location) {
                (Some(parent), Some(location)) if is_placeholder => needs_parentheses(initializer, parent, location),
                (Some(parent), Some(location)) => crate::correction::parentheses::needs_parentheses_placeholder(cu.rewrite.kind(new_reference), parent, location),
                _ => false,
            };
            if needs {
                new_reference = cu.rewrite.new_parenthesized_expression(new_reference);
            }
            cu.rewrite.replace(RNode::Orig(reference.id), Some(new_reference));
            cu.rewrite.add_tight_source_node(reference.id);
        }
        if removes_here {
            let declaration = ast.node(self.declaration);
            let field = declaration.parent()?;
            let to_remove = if field.list("fragments").len() == 1 { field } else { declaration };
            cu.rewrite.remove(RNode::Orig(to_remove.id));
        } else if names.is_empty() {
            return None;
        }
        Some(CuChange::rewrite(cu.rewrite).with_imports(cu.imports))
    }

    /// `prepareInitializerForLocation(location)`.
    async fn prepare_initializer_for_location(
        &self,
        env: &Env<'_>,
        ast: &Arc<Ast>,
        target: &mut CuRewrite,
        target_options: &BTreeMap<String, String>,
        location: Node<'_>,
        initializer: Node<'_>,
        static_imports_in_initializer: &HashSet<NodeId>,
    ) -> String {
        let initializer_ast = self.declaration_ast.clone();
        let static_import_in_reference = location.is(NodeKind::SimpleName) && static_import_needed(location);
        let rw = {
            let context = import_context(ast, location, target_options);
            let mut traversal = Traversal {
                initializer,
                rw: ASTRewrite::new(initializer_ast.clone()),
                static_imports_in_initializer,
                new_location: location,
                static_import_in_reference,
                target,
                context: &context,
                locally_declared: None,
            };
            traversal.perform();
            traversal.rw
        };
        let options = env.options(&initializer_ast.uri).await;
        let source = &initializer_ast.source;
        let (start, end) = (initializer.start(), initializer.start() + initializer.length());
        let rewritten = match crate::rewrite::formatter::rewrite_with_bridge(&rw, &options, env.dispatcher).await {
            Ok(tree) => {
                let new_source = tree.apply(source);
                let delta = new_source.len() as i64 - source.len() as i64;
                let new_end = ((end as i64 + delta).max(start as i64) as usize).min(new_source.len());
                String::from_utf16_lossy(&new_source[start..new_end])
            }
            Err(_) => String::from_utf16_lossy(&source[start..end]),
        };
        let (tab, width) = (indent::tab_width(&options), indent::indent_width(&options));
        let line_start = source[..start].iter().rposition(|c| *c == b'\n' as u16 || *c == b'\r' as u16).map_or(0, |i| i + 1);
        let line_end = source[start..].iter().position(|c| *c == b'\n' as u16 || *c == b'\r' as u16).map_or(source.len(), |i| start + i);
        let old_indent = indent::measure_indent_units(&source[line_start..line_end], tab, width);
        let delimiter = crate::rewrite::analyzer::default_line_delimiter(source);
        indent::change_indent(&rewritten, old_indent, tab, width, "", &delimiter)
    }
}

fn declaration_type(declaration: Node<'_>) -> Option<Node<'_>> {
    declaration.parent().and_then(|p| p.child("type"))
}

/// The source text of `ASTNodeFactory.newType(ast, declaration)` as array type.
fn array_type_text(declaration: Node<'_>) -> String {
    let mut text = declaration_type(declaration).map(|t| t.source_text()).unwrap_or_default();
    for _ in declaration.list("extraDimensions") {
        text.push_str("[]");
    }
    text
}

async fn definition_fragment(env: &Env<'_>, ast: &Arc<Ast>, name: Node<'_>) -> Option<(Arc<Ast>, NodeId)> {
    let uri = Url::parse(&ast.uri).ok()?;
    let locations = crate::features::navigation::definition(env.dispatcher, &uri, ast.position(name.start())).await?;
    let location = locations.into_iter().next()?;
    let target = crate::semantic_ast::fetch(env.dispatcher, &location.uri).await.ok()?;
    let (start, length) = location_range(&target, &location)?;
    let node = NodeFinder::perform(target.root(), start, length)?;
    let fragment = if node.is(NodeKind::VariableDeclarationFragment) { node } else { node.parent().filter(|p| p.is(NodeKind::VariableDeclarationFragment))? };
    let id = fragment.id;
    Some((target, id))
}

/// `getQualifiedReference(fieldName)`.
fn qualified_reference(name: Node<'_>) -> Node<'_> {
    if let Some(parent) = name.parent() {
        let qualifies = match parent.kind() {
            NodeKind::FieldAccess | NodeKind::QualifiedName | NodeKind::MethodInvocation => parent.child("name").is_some_and(|n| n.id == name.id),
            _ => false,
        };
        if qualifies {
            return parent;
        }
    }
    name
}

fn enclosing_types(node: Node<'_>) -> Vec<BindingRef<'_>> {
    node.ancestors().filter(|a| a.kind().is_abstract_type_declaration() || a.is(NodeKind::AnonymousClassDeclaration)).filter_map(|t| t.binding()).collect()
}

fn supertypes_closure<'a>(t: BindingRef<'a>, out: &mut Vec<String>) {
    if out.iter().any(|k| k == t.key()) {
        return;
    }
    out.push(t.key().to_owned());
    if let Some(s) = t.superclass() {
        supertypes_closure(s, out);
    }
    for i in t.interfaces() {
        supertypes_closure(i, out);
    }
}

/// Whether an unqualified static member reference needs a static import
/// (`ImportReferencesCollector`).
fn static_import_needed(name: Node<'_>) -> bool {
    let unqualified = match name.parent() {
        Some(p) => match p.kind() {
            NodeKind::QualifiedName | NodeKind::FieldAccess | NodeKind::MethodInvocation => !name.location_is("name") || (p.is(NodeKind::MethodInvocation) && p.child("expression").is_none()),
            _ => true,
        },
        None => true,
    };
    if !unqualified {
        return false;
    }
    let Some(binding) = name.binding() else { return false };
    if !(binding.is_variable() && binding.is_field() || binding.is_method()) || !binding.is_static() {
        return false;
    }
    let Some(declaring) = binding.declaring_class() else { return false };
    let mut known = Vec::new();
    for t in enclosing_types(name) {
        supertypes_closure(t, &mut known);
    }
    !known.iter().any(|k| k == declaring.key())
}

fn static_imports_in(initializer: Node<'_>) -> HashSet<NodeId> {
    let mut result = HashSet::new();
    super::walk(initializer, &mut |n| {
        if n.is(NodeKind::SimpleName) && static_import_needed(n) {
            result.insert(n.id);
        }
        true
    });
    result
}

/// `InitializerTraversal`.
struct Traversal<'a, 'c> {
    initializer: Node<'a>,
    rw: ASTRewrite,
    static_imports_in_initializer: &'c HashSet<NodeId>,
    new_location: Node<'c>,
    static_import_in_reference: bool,
    target: &'c mut CuRewrite,
    context: &'c dyn ImportRewriteContext,
    locally_declared: Option<HashSet<String>>,
}

impl<'a, 'c> Traversal<'a, 'c> {
    fn perform(&mut self) {
        let initializer = self.initializer;
        super::accept(initializer, &mut TraversalVisitor { t: self });
    }

    fn container_binding<'n>(node: Node<'n>) -> Option<BindingRef<'n>> {
        let container = std::iter::once(node).chain(node.ancestors()).find(|n| n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration))?;
        container.binding()
    }

    fn are_in_same_type(one: Node<'_>, other: Node<'_>) -> bool {
        let (Some(mut ones), Some(others)) = (Self::container_binding(one), Self::container_binding(other)) else { return false };
        if one.is(NodeKind::SimpleName) {
            if let Some(declaring) = one.binding().filter(|b| b.is_variable()).and_then(|b| b.declaring_class()) {
                ones = declaring;
            }
        } else if one.is(NodeKind::MethodInvocation) {
            if let Some(declaring) = one.method_binding().and_then(|b| b.declaring_class()) {
                ones = declaring;
            }
        }
        ones.key() == others.key()
    }

    fn may_be_shadowed(&mut self, member: Node<'_>) -> bool {
        if self.locally_declared.is_none() {
            let body = self.new_location.ancestors().find(|a| a.kind().is_body_declaration());
            let mut names = HashSet::new();
            if let Some(body) = body.filter(|b| !b.is(NodeKind::FieldDeclaration)) {
                collect_declared_names(body, &mut names);
            }
            self.locally_declared = Some(names);
        }
        self.locally_declared.as_ref().is_some_and(|n| n.contains(&member.identifier()))
    }

    fn should_unqualify(&mut self, member: Node<'_>) -> bool {
        Self::are_in_same_type(member, self.new_location) && !self.may_be_shadowed(member)
    }

    fn should_qualify(&mut self, member: Node<'_>) -> bool {
        if !Self::are_in_same_type(self.initializer, self.new_location) {
            return true;
        }
        self.may_be_shadowed(member)
    }

    fn qualify_if_necessary(&mut self, member: Node<'_>) {
        if self.should_qualify(member) {
            self.qualify_member_name(member);
        }
    }

    fn unqualify_member_name(&mut self, member: Node<'_>) {
        let Some(parent) = member.parent() else { return };
        let qualifies = match parent.kind() {
            NodeKind::FieldAccess | NodeKind::QualifiedName | NodeKind::MethodInvocation => parent.child("name").is_some_and(|n| n.id == member.id),
            _ => false,
        };
        if qualifies {
            let copy = self.rw.create_copy_target(member.id);
            self.rw.replace(RNode::Orig(parent.id), Some(copy));
        }
    }

    fn qualify_member_name(&mut self, member: Node<'_>) {
        let Some(binding) = member.binding() else { return };
        let is_static_access = if binding.is_type() {
            true
        } else if binding.is_variable() {
            binding.is_field()
        } else {
            binding.is_static()
        };
        if !is_static_access {
            return;
        }
        if (binding.is_variable() || binding.is_method()) && (self.static_import_in_reference || self.static_imports_in_initializer.contains(&member.id)) {
            self.import_statically(member, binding);
            return;
        }
        self.qualify_to_top_level_class(member, binding);
    }

    fn import_statically(&mut self, to_import: Node<'_>, binding: BindingRef<'_>) {
        let Some(declaring) = binding.declaring_class() else { return };
        let declaring_name = declaring.type_declaration().unwrap_or(declaring).qualified_name().to_owned();
        let name = self.target.imports.add_static_import(&declaring_name, binding.name(), binding.is_variable(), self.context);
        let new_name = self.rw.new_name(&name);
        self.rw.replace(RNode::Orig(to_import.id), Some(new_name));
    }

    fn qualify_to_top_level_class(&mut self, to_qualify: Node<'_>, binding: BindingRef<'_>) {
        let Some(declaring) = binding.declaring_class() else { return };
        let erasure = declaring.erasure().unwrap_or(declaring);
        let qualification = self.target.imports.add_import_type(erasure, &mut self.rw, self.context, TypeLocation::Unknown);
        let moved = self.rw.create_copy_target(to_qualify.id);
        let qualified = self.rw.new_node(NodeKind::QualifiedType);
        self.rw.put_child(qualified, "qualifier", qualification);
        self.rw.put_child(qualified, "name", moved);
        self.rw.replace(RNode::Orig(to_qualify.id), Some(qualified));
    }

    fn visit_name(&mut self, name: Node<'_>) {
        if matches!(name.parent().map(|p| p.kind()), Some(NodeKind::ExpressionMethodReference | NodeKind::TypeMethodReference | NodeKind::SuperMethodReference)) && name.location_is("name") {
            return;
        }
        let mut leftmost = name;
        while leftmost.is(NodeKind::QualifiedName) {
            match leftmost.child("qualifier") {
                Some(q) => leftmost = q,
                None => return,
            }
        }
        let Some(binding) = leftmost.binding() else { return };
        if binding.is_variable() || binding.is_method() || binding.is_type() {
            if self.should_unqualify(leftmost) {
                self.unqualify_member_name(leftmost);
            } else {
                self.qualify_if_necessary(leftmost);
            }
        }
        if binding.is_type() {
            self.target.imports.add_import_binding(binding, self.context);
        }
    }
}

fn collect_declared_names(scope: Node<'_>, out: &mut HashSet<String>) {
    super::walk(scope, &mut |n| {
        if n.id == scope.id {
            return true;
        }
        match n.kind() {
            k if k.is_abstract_type_declaration() => {
                if let Some(name) = n.child("name") {
                    out.insert(name.identifier());
                }
                false
            }
            NodeKind::AnonymousClassDeclaration => false,
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => {
                if let Some(name) = n.child("name") {
                    out.insert(name.identifier());
                }
                false
            }
            _ => true,
        }
    });
}

struct TraversalVisitor<'t, 'a, 'c> {
    t: &'t mut Traversal<'a, 'c>,
}

impl<'a> Visitor<'a> for TraversalVisitor<'_, '_, '_> {
    fn visit(&mut self, n: Node<'a>) -> bool {
        match n.kind() {
            NodeKind::FieldAccess => {
                if let Some(expression) = n.child("expression") {
                    super::accept(expression, self);
                }
                false
            }
            NodeKind::MethodInvocation => {
                match n.child("expression") {
                    None => {
                        if let Some(name) = n.child("name") {
                            self.t.qualify_if_necessary(name);
                        }
                    }
                    Some(expression) => super::accept(expression, self),
                }
                for argument in n.list("arguments") {
                    super::accept(argument, self);
                }
                false
            }
            NodeKind::SimpleName | NodeKind::QualifiedName => {
                self.t.visit_name(n);
                false
            }
            _ => true,
        }
    }
}
