//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.InlineTempRefactoring`
//! (Inline Local Variable) for a refactoring created from a declaration.
//! Inlining generic invocations whose type arguments are inferred from the
//! expected type is not ported.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::extract_temp::{import_context, CuRewrite};
use super::scope::{self, ScopeAnalyzer};
use super::{msg, Status, Visitor};
use crate::correction::messages;
use crate::correction::parentheses::{needs_parentheses, needs_parentheses_for_cast, needs_parentheses_placeholder};
use crate::rewrite::import_rewrite::TypeLocation;
use crate::rewrite::{ASTRewrite, RNode, Value};
use crate::semantic_ast::{Ast, BindingRef, Node, NodeId, NodeKind, PropValue};

pub struct InlineTemp {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    declaration: NodeId,
}

/// A replacement for a clashing name: a (qualified) name or `this.name`.
enum Alternative {
    Name(String),
    ThisField(String),
}

impl Alternative {
    fn leftmost(&self) -> Option<&str> {
        match self {
            Alternative::Name(n) => n.split('.').next(),
            Alternative::ThisField(_) => None,
        }
    }
}

impl InlineTemp {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, declaration: Node<'_>) -> Self {
        InlineTemp { ast, options, declaration: declaration.id }
    }

    fn declaration(&self) -> Node<'_> {
        self.ast.node(self.declaration)
    }

    /// `getReferences()`.
    pub fn references(&self) -> Vec<Node<'_>> {
        let decl = self.declaration();
        let Some(binding) = decl.child("name").and_then(|n| n.binding()) else { return Vec::new() };
        let mut result = Vec::new();
        super::walk(self.ast.root(), &mut |n| {
            if n.is(NodeKind::Javadoc) {
                return false;
            }
            if n.is(NodeKind::SimpleName) {
                let is_declaration_name = n.parent().is_some_and(|p| matches!(p.kind(), NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration) && n.location_is("name"));
                if !is_declaration_name && n.binding() == Some(binding) {
                    result.push(n);
                }
            }
            true
        });
        result
    }

    /// `checkInitialConditions(pm)`.
    pub fn check_initial_conditions(&self) -> Status {
        let mut result = Status::ok();
        let decl = self.declaration();
        result.merge(self.check_selection(decl));
        result.merge(self.check_clashes(decl));
        result
    }

    fn check_clashes(&self, declaration: Node<'_>) -> Option<Status> {
        for reference in self.references() {
            let new_variables = self.new_variables(reference);
            let initializer_names = self.initializer_names(declaration.child("initializer"));
            for name in initializer_names {
                if clashes(&new_variables, leftmost_name(name)) {
                    let alternatives = self.alternative_qualifications(reference, name);
                    if !alternatives.is_empty() && !alternatives.iter().any(|a| !alternative_clashes(&new_variables, a)) {
                        return Some(Status::fatal(messages::format(msg("InlineTemRefactoring_error_message_inliningClashes"), &[&name.source_text()])));
                    }
                }
            }
        }
        None
    }

    fn check_selection(&self, decl: Node<'_>) -> Status {
        let Some(parent) = decl.parent() else { return Status::ok() };
        if parent.is(NodeKind::MethodDeclaration) {
            return Status::fatal(msg("InlineTempRefactoring_method_parameter"));
        }
        if parent.is(NodeKind::CatchClause) {
            return Status::fatal(msg("InlineTempRefactoring_exceptions_declared"));
        }
        if parent.is(NodeKind::VariableDeclarationExpression) {
            if parent.location_is("initializers") && parent.parent().is_some_and(|p| p.is(NodeKind::ForStatement)) {
                return Status::fatal(msg("InlineTempRefactoring_for_initializers"));
            }
            if parent.location_is("resources") && parent.parent().is_some_and(|p| p.is(NodeKind::TryStatement)) {
                return Status::fatal(msg("InlineTempRefactoring_resource_in_try_with_resources"));
            }
        }
        let name = decl.child("name").map(|n| n.identifier()).unwrap_or_default();
        if decl.child("initializer").is_none() {
            return Status::fatal(messages::format(msg("InlineTempRefactoring_not_initialized"), &[&name]));
        }
        for reference in self.references() {
            if reference.location_is("resources") && reference.parent().is_some_and(|p| p.is(NodeKind::TryStatement)) {
                return Status::fatal(msg("InlineTempRefactoring_resource_used_in_try_with_resources"));
            }
        }
        self.check_assignments(decl)
    }

    /// `TempAssignmentFinder`.
    fn check_assignments(&self, decl: Node<'_>) -> Status {
        let Some(binding) = decl.child("name").and_then(|n| n.binding()) else { return Status::ok() };
        let refers = |n: Node<'_>| matches!(n.kind(), NodeKind::SimpleName | NodeKind::QualifiedName) && n.binding() == Some(binding);
        let mut found = false;
        super::walk(self.ast.root(), &mut |n| {
            if found {
                return false;
            }
            let assignment = match n.kind() {
                NodeKind::Assignment => n.child("leftHandSide").is_some_and(|l| refers(l)),
                NodeKind::PostfixExpression => n.child("operand").is_some_and(|o| o.is(NodeKind::SimpleName) && refers(o)),
                NodeKind::PrefixExpression => {
                    matches!(n.simple("operator"), Some("++" | "--")) && n.child("operand").is_some_and(|o| o.is(NodeKind::SimpleName) && refers(o))
                }
                _ => false,
            };
            if assignment {
                found = true;
                return false;
            }
            true
        });
        if found {
            let name = decl.child("name").map(|n| n.identifier()).unwrap_or_default();
            Status::fatal(messages::format(msg("InlineTempRefactoring_assigned_more_once"), &[&name]))
        } else {
            Status::ok()
        }
    }

    /// The rewrite of `checkFinalConditions` (`inlineTemp`, `removeTemp`).
    pub fn create_rewrite(&self) -> CuRewrite {
        let mut cu = CuRewrite::new(&self.ast, &self.options);
        for reference in self.references() {
            let source = self.initializer_source(&mut cu, reference);
            cu.rewrite.replace(RNode::Orig(reference.id), Some(source));
        }
        self.remove_temp(&mut cu.rewrite);
        cu
    }

    fn remove_temp(&self, rw: &mut ASTRewrite) {
        let decl = self.declaration();
        match decl.parent() {
            Some(parent) if parent.is(NodeKind::VariableDeclarationStatement) && parent.list("fragments").len() == 1 => {
                rw.add_tight_source_node(parent.id);
                rw.remove(RNode::Orig(parent.id));
            }
            _ => {
                rw.add_tight_source_node(decl.id);
                rw.remove(RNode::Orig(decl.id));
            }
        }
    }

    fn initializer_source(&self, cu: &mut CuRewrite, reference: Node<'_>) -> RNode {
        let (copy, wrapped) = self.modified_initializer_source(cu, reference);
        let (Some(parent), Some(location)) = (reference.parent(), reference.location()) else { return copy };
        let needs = !wrapped && needs_parentheses_placeholder(cu.rewrite.kind(copy), parent, location);
        if needs {
            cu.rewrite.new_parenthesized_expression(copy)
        } else {
            copy
        }
    }

    /// `getModifiedInitializerSource`; the flag tells that the result needs
    /// no further parentheses check.
    fn modified_initializer_source(&self, cu: &mut CuRewrite, reference: Node<'_>) -> (RNode, bool) {
        let decl = self.declaration();
        let initializer = decl.child("initializer").expect("initializer");
        let initializer_names = self.initializer_names(Some(initializer));
        if !initializer_names.is_empty() {
            let mut replacements = self.clashing_replacements(reference, &initializer_names);
            if !replacements.is_empty() {
                return (self.replace_clashing_names(&mut cu.rewrite, initializer, &mut replacements), false);
            }
        }
        let rw = &mut cu.rewrite;
        let mut copy = rw.create_copy_target(initializer.id);
        let (parent, location) = (reference.parent(), reference.location());
        if let (Some(parent), Some(location)) = (parent, location) {
            if needs_parentheses(initializer, parent, location) {
                copy = rw.new_parenthesized_expression(copy);
            }
        }
        if let Some(cast) = crate::correction::local_corrections::conversion::explicit_cast(initializer, reference) {
            let mut operand = copy;
            if rw.kind(copy) != NodeKind::ParenthesizedExpression && needs_parentheses_for_cast(initializer, false) {
                operand = rw.new_parenthesized_expression(copy);
            }
            let node = rw.new_node(NodeKind::CastExpression);
            rw.put_child(node, "expression", operand);
            let context = import_context(&self.ast, reference, &self.options);
            let typ = cu.imports.add_import_type(cast, rw, &context, TypeLocation::Cast);
            rw.put_child(node, "type", typ);
            return (node, false);
        }
        if initializer.is(NodeKind::ArrayInitializer) && dimensions(decl) > 0 {
            let context = import_context(&self.ast, decl, &self.options);
            let typ = super::extract_method::new_type(cu, &self.options, decl, &context, false);
            let creation = cu.rewrite.new_node(NodeKind::ArrayCreation);
            cu.rewrite.put_child(creation, "type", typ);
            cu.rewrite.put_child(creation, "initializer", copy);
            return (creation, true);
        }
        (copy, false)
    }

    /// `getInitializerNames(initializer)`.
    fn initializer_names<'a>(&self, initializer: Option<Node<'a>>) -> Vec<Node<'a>> {
        let decl = self.declaration();
        let mut analyzer = ScopeAnalyzer::new(self.ast.root());
        let in_scope: HashSet<String> = analyzer.declarations_in_scope(decl.start(), scope::VARIABLES).iter().map(|b| b.key().to_owned()).collect();
        let mut names = Vec::new();
        if let Some(initializer) = initializer {
            collect_names(initializer, &in_scope, &mut names);
        }
        names
    }

    /// `getNewVariables(reference)`: names of variables in scope at the
    /// reference that are not in scope after the declaration.
    fn new_variables(&self, reference: Node<'_>) -> Vec<String> {
        let decl = self.declaration();
        let mut analyzer = ScopeAnalyzer::new(self.ast.root());
        let at_reference = analyzer.declarations_in_scope(reference.start(), scope::VARIABLES);
        let after = analyzer.declarations_in_scope(decl.start() + decl.length(), scope::VARIABLES);
        let after_keys: HashSet<&str> = after.iter().map(|b| b.key()).collect();
        let mut keys = HashSet::new();
        at_reference.iter().filter(|b| !after_keys.contains(b.key()) && keys.insert(b.key().to_owned())).map(|b| b.name().to_owned()).collect()
    }

    fn clashing_replacements<'a>(&self, reference: Node<'a>, names: &[Node<'a>]) -> Vec<(Node<'a>, Alternative)> {
        let new_variables = self.new_variables(reference);
        let mut replacements = Vec::new();
        for name in names {
            if clashes(&new_variables, leftmost_name(*name)) {
                let alternative = self.alternative_qualifications(reference, *name).into_iter().find(|a| !alternative_clashes(&new_variables, a));
                if let Some(alternative) = alternative {
                    replacements.push((*name, alternative));
                }
            }
        }
        replacements
    }

    fn alternative_qualifications(&self, reference: Node<'_>, name: Node<'_>) -> Vec<Alternative> {
        let mut result = Vec::new();
        let Some(binding) = name.binding().filter(|b| b.is_variable()) else { return result };
        if name.is(NodeKind::SimpleName) {
            if binding.is_static() {
                let declaring = binding.declaring_class();
                result.push(Alternative::Name(fully_qualified_name(name.identifier(), declaring, 0, false)));
                result.push(Alternative::Name(fully_qualified_name(name.identifier(), declaring, 0, true)));
            } else if binding.is_field() {
                result.push(Alternative::ThisField(name.identifier()));
            }
        } else if name.is(NodeKind::QualifiedName) && binding.is_static() {
            let simple = name.child("name").map(|n| n.identifier()).unwrap_or_default();
            let skip = self.common_declaring_class_index(reference, name);
            let qualifier_type = name.child("qualifier").and_then(|q| q.type_binding());
            result.push(Alternative::Name(fully_qualified_name(simple.clone(), qualifier_type, 0, false)));
            result.push(Alternative::Name(fully_qualified_name(simple.clone(), qualifier_type, skip, false)));
            result.push(Alternative::Name(fully_qualified_name(simple.clone(), qualifier_type, 0, true)));
            result.push(Alternative::Name(fully_qualified_name(simple, qualifier_type, skip, true)));
        }
        result
    }

    /// `findCommonDeclaringClassIndex(reference, initializerQualifiedName)`.
    fn common_declaring_class_index(&self, reference: Node<'_>, qualified: Node<'_>) -> usize {
        let mut names = Vec::new();
        let mut current = Some(qualified);
        while let Some(c) = current {
            if c.is(NodeKind::QualifiedName) {
                names.insert(0, c.child("name"));
                current = c.child("qualifier");
            } else {
                names.insert(0, Some(c));
                current = None;
            }
        }
        let enclosing = reference.ancestors().find(|a| a.is(NodeKind::TypeDeclaration)).and_then(|t| t.binding());
        let mut i = 1;
        while i < names.len() && enclosing != names[i].and_then(declaring_class_of) {
            i += 1;
        }
        i
    }

    /// `replaceClashingNames`.
    fn replace_clashing_names(&self, rw: &mut ASTRewrite, initializer: Node<'_>, replacements: &mut Vec<(Node<'_>, Alternative)>) -> RNode {
        let copy = copy_replacing(rw, initializer, initializer, replacements);
        if !replacements.is_empty() {
            let (_, alternative) = replacements.remove(0);
            return new_alternative(rw, alternative);
        }
        copy
    }
}

fn dimensions(decl: Node<'_>) -> usize {
    let mut dim = decl.list("extraDimensions").len();
    let typ = if decl.is(NodeKind::SingleVariableDeclaration) { decl.child("type") } else { decl.parent().and_then(|p| p.child("type")) };
    if let Some(t) = typ.filter(|t| t.is(NodeKind::ArrayType)) {
        dim += t.list("dimensions").len();
    }
    dim
}

fn declaring_class_of(name: Node<'_>) -> Option<BindingRef<'_>> {
    let binding = name.binding()?;
    if binding.is_type() {
        binding.declaring_class()
    } else if binding.is_variable() {
        binding.declaring_class()
    } else {
        None
    }
}

fn leftmost_name(name: Node<'_>) -> String {
    let mut n = name;
    while n.is(NodeKind::QualifiedName) {
        match n.child("qualifier") {
            Some(q) => n = q,
            None => break,
        }
    }
    n.identifier()
}

fn clashes(new_variables: &[String], leftmost: String) -> bool {
    new_variables.iter().any(|v| *v == leftmost)
}

fn alternative_clashes(new_variables: &[String], alternative: &Alternative) -> bool {
    alternative.leftmost().is_some_and(|l| new_variables.iter().any(|v| v == l))
}

fn collect_names<'a>(node: Node<'a>, in_scope: &HashSet<String>, out: &mut Vec<Node<'a>>) {
    struct Collector<'s, 'a> {
        in_scope: &'s HashSet<String>,
        out: &'s mut Vec<Node<'a>>,
    }
    impl<'a> Visitor<'a> for Collector<'_, 'a> {
        fn visit(&mut self, n: Node<'a>) -> bool {
            match n.kind() {
                NodeKind::QualifiedName => {
                    if n.binding().is_some_and(|b| b.is_variable()) {
                        self.out.push(n);
                    }
                    false
                }
                NodeKind::SimpleName => {
                    if n.binding().is_some_and(|b| b.is_variable() && self.in_scope.contains(b.key())) {
                        self.out.push(n);
                    }
                    false
                }
                _ => true,
            }
        }
    }
    super::accept(node, &mut Collector { in_scope, out });
}

/// `createFullyQualifiedName(simpleName, declaringClass, numberOfClassesToSkip, addPackage)`.
fn fully_qualified_name(simple: String, declaring_class: Option<BindingRef<'_>>, skip: usize, add_package: bool) -> String {
    let mut classes = Vec::new();
    let mut current = declaring_class;
    while let Some(c) = current {
        classes.push(c);
        current = c.declaring_class();
    }
    for _ in 0..skip {
        if classes.pop().is_none() {
            break;
        }
    }
    let mut qualified = String::new();
    if add_package {
        if let Some(outer) = classes.last() {
            qualified.push_str(outer.package_name().unwrap_or(""));
            qualified.push('.');
        }
    }
    for c in classes.iter().rev() {
        qualified.push_str(c.name());
        qualified.push('.');
    }
    qualified.push_str(&simple);
    qualified
}

fn new_alternative(rw: &mut ASTRewrite, alternative: Alternative) -> RNode {
    match alternative {
        Alternative::Name(n) => rw.new_name(&n),
        Alternative::ThisField(n) => {
            let this = rw.new_this_expression();
            let name = rw.new_simple_name(&n);
            rw.new_field_access(this, name)
        }
    }
}

/// `ASTNode.copySubtree` of `node` where the names matching a replacement
/// key are replaced (each replacement is used once).
fn copy_replacing(rw: &mut ASTRewrite, node: Node<'_>, root: Node<'_>, replacements: &mut Vec<(Node<'_>, Alternative)>) -> RNode {
    if matches!(node.kind(), NodeKind::QualifiedName | NodeKind::SimpleName) {
        let hit = replacements.iter().position(|(key, _)| match key.kind() {
            NodeKind::QualifiedName => crate::semantic_ast::resolve::subtree_match(*key, node),
            _ => node.is(NodeKind::SimpleName) && key.identifier() == node.identifier(),
        });
        if let Some(i) = hit {
            if node.id != root.id {
                let (_, alternative) = replacements.remove(i);
                return new_alternative(rw, alternative);
            }
        }
    }
    let copy = rw.new_node(node.kind());
    for (prop, value) in node.props() {
        let v = match value {
            PropValue::Child(Some(id)) => Value::Node(Some(copy_replacing(rw, node.ast.node(*id), root, replacements))),
            PropValue::Child(None) => Value::Node(None),
            PropValue::List(ids) => Value::List(ids.iter().map(|id| copy_replacing(rw, node.ast.node(*id), root, replacements)).collect()),
            PropValue::Simple(s) => Value::Simple(s.clone()),
        };
        rw.put(copy, prop, v);
    }
    copy
}
