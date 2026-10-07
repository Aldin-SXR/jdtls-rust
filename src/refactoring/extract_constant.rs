//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.ExtractConstantRefactoring`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::rewrite::import_rewrite::TypeLocation;
use crate::rewrite::RNode;
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::{modifier, Ast, Node, NodeId, NodeKind};

use super::checks;
use super::extract_temp::{check_rvalue, import_context, CuRewrite};
use super::fragments::{self, Fragment};
use super::naming::{self, VarKind};
use super::scope::ScopeAnalyzer;
use super::{msg, Status};

/// `JdtFlags.VISIBILITY_STRING_*`.
const PRIVATE: &str = "private";
const PUBLIC: &str = "public";
const PACKAGE: &str = "";

/// `ExtractConstantRefactoring`.
pub struct ExtractConstant {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    selection_start: usize,
    selection_length: usize,
    selected: Option<Fragment>,
    replace_all: bool,
    qualify_references: bool,
    visibility: &'static str,
    constant_name: String,
    excluded: Option<Vec<String>>,
    to_insert_after: Option<NodeId>,
    insert_first: bool,
}

impl ExtractConstant {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, selection_start: usize, selection_length: usize) -> Self {
        ExtractConstant {
            ast,
            options,
            selection_start,
            selection_length,
            selected: None,
            replace_all: true,
            qualify_references: false,
            visibility: PRIVATE,
            constant_name: String::new(),
            excluded: None,
            to_insert_after: None,
            insert_first: false,
        }
    }

    pub fn set_constant_name(&mut self, name: &str) {
        self.constant_name = name.to_owned();
    }

    pub fn constant_name(&self) -> &str {
        &self.constant_name
    }

    /// `getSelectedExpression()`.
    fn selected_expression(&mut self) -> Option<Fragment> {
        if self.selected.is_some() {
            return self.selected.clone();
        }
        let ast = self.ast.clone();
        let fragment = fragments::for_source_range(ast.root(), self.selection_start, self.selection_length);
        if let Some(f) = fragment {
            if f.is_expression() && !checks::is_inside_javadoc(f.node(&ast)) && !checks::is_enum_case(f.node(&ast).parent()) {
                self.selected = Some(f);
            }
        }
        self.selected.clone()
    }

    fn containing_type_declaration(&mut self) -> Option<NodeId> {
        let ast = self.ast.clone();
        let sel = self.selected_expression()?;
        let found = sel.node(&ast).ancestors().find(|a| a.kind().is_abstract_type_declaration()).map(|n| n.id);
        found
    }

    /// `checkInitialConditions(pm)`.
    pub fn check_initial_conditions(&mut self) -> Status {
        let ast = self.ast.clone();
        let mut result = self.check_selection();
        if result.has_fatal_error() {
            return result;
        }
        let selected = self.selected_expression().unwrap();
        let node = selected.node(&ast);
        if matches!(node.kind(), NodeKind::BooleanLiteral | NodeKind::CharacterLiteral | NodeKind::NullLiteral | NodeKind::NumberLiteral) {
            self.replace_all = false;
        }
        let Some(type_declaration) = self.containing_type_declaration() else {
            result.merge(Status::fatal(msg("ExtractConstantRefactoring_no_type")));
            return result;
        };
        if self.is_in_type_declaration_annotation(node) {
            self.visibility = PACKAGE;
        }
        if ast.node(type_declaration).binding().is_some_and(|b| b.is_interface()) {
            self.visibility = PUBLIC;
        }
        result
    }

    fn check_selection(&mut self) -> Status {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else {
            return checks::check_method_syntax_errors(self.selection_start, self.selection_length, ast.root(), msg("ExtractConstantRefactoring_select_expression"));
        };
        let e = selected.node(&ast);
        let mut result = check_rvalue(e, "ExtractConstantRefactoring_select_expression", "ExtractConstantRefactoring_no_void");
        if result.has_fatal_error() {
            return result;
        }
        if e.is(NodeKind::NullLiteral) {
            result.merge(Status::fatal(msg("ExtractConstantRefactoring_null_literals")));
        } else if !checks::is_load_time_constant(&selected, ast.root()) {
            result.merge(Status::fatal(msg("ExtractConstantRefactoring_not_load_time_constant")));
        } else if e.is(NodeKind::SimpleName)
            && e.location_is("name")
            && e.parent().is_some_and(|p| p.is(NodeKind::QualifiedName) || p.is(NodeKind::FieldAccess))
        {
            return Status::fatal(msg("ExtractConstantRefactoring_select_expression"));
        }
        result
    }

    fn is_in_type_declaration_annotation(&mut self, node: Node<'_>) -> bool {
        let Some(type_declaration) = self.containing_type_declaration() else { return false };
        node.ancestors().find(|a| a.kind().is_annotation()).is_some_and(|a| a.parent().is_some_and(|p| p.id == type_declaration))
    }

    /// `getExcludedVariableNames()`.
    fn excluded_variable_names(&mut self) -> Vec<String> {
        if let Some(e) = &self.excluded {
            return e.clone();
        }
        let ast = self.ast.clone();
        let names: Vec<String> = match self.selected_expression() {
            Some(sel) => ScopeAnalyzer::new(ast.root()).used_variable_names(sel.start(&ast), sel.length(&ast)).into_iter().collect(),
            None => Vec::new(),
        };
        self.excluded = Some(names.clone());
        names
    }

    /// `guessConstantName()`.
    pub fn guess_constant_name(&mut self) -> String {
        let ast = self.ast.clone();
        let Some(sel) = self.selected_expression() else { return self.constant_name.clone() };
        let expression = sel.node(&ast);
        let binding = expression.type_binding().or_else(|| checks::guess_binding_for_reference(expression));
        let excluded = self.excluded_variable_names();
        let names = naming::variable_name_suggestions(VarKind::StaticFinalField, binding, Some(expression), &excluded, &self.options);
        names.into_iter().next().unwrap_or_else(|| self.constant_name.clone())
    }

    /// `checkFinalConditions` + `createChange`.
    pub fn check_final_conditions(&mut self) -> (Status, Option<CuRewrite>) {
        let mut cu = CuRewrite::new(&self.ast, &self.options);
        self.create_constant_declaration(&mut cu);
        self.replace_expressions_with_constant(&mut cu);
        (Status::ok(), Some(cu))
    }

    fn create_constant_declaration(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let selected = self.selected_expression().unwrap();
        let expression = selected.node(&ast);
        // getConstantType()
        let binding = expression.type_binding().or_else(|| checks::guess_binding_for_reference(expression));
        let context_node = NodeFinder::new(ast.root(), self.selection_start, 0).covering.unwrap_or(expression);
        let typ = match binding.and_then(crate::correction::type_mismatch::bindings::normalize_for_declaration_use) {
            Some(b) => {
                let context = import_context(&ast, context_node, &self.options);
                cu.imports.add_import_type(b, &mut cu.rewrite, &context, TypeLocation::Field)
            }
            None => {
                let name = cu.rewrite.new_simple_name("Object");
                cu.rewrite.new_simple_type(name)
            }
        };
        let initializer = selected.create_copy_target(&mut cu.rewrite, true);
        let fragment = cu.rewrite.new_variable_declaration_fragment(&self.constant_name, Some(initializer));
        let mut flags = modifier::STATIC | modifier::FINAL;
        flags |= match self.visibility {
            PRIVATE => modifier::PRIVATE,
            PUBLIC => modifier::PUBLIC,
            "protected" => modifier::PROTECTED,
            _ => 0,
        };
        let modifiers = cu.rewrite.new_modifiers(flags);
        let field = cu.rewrite.new_field_declaration(fragment, modifiers, typ);
        let Some(parent) = self.containing_type_declaration() else { return };
        self.compute_declaration_location();
        if self.insert_first {
            cu.rewrite.list_insert_first(RNode::Orig(parent), "bodyDeclarations", field);
        } else if let Some(after) = self.to_insert_after {
            cu.rewrite.list_insert_after(RNode::Orig(parent), "bodyDeclarations", field, RNode::Orig(after));
        }
    }

    /// `computeConstantDeclarationLocation()`.
    fn compute_declaration_location(&mut self) {
        if self.insert_first || self.to_insert_after.is_some() {
            return;
        }
        let ast = self.ast.clone();
        let selected = self.selected_expression().unwrap();
        let mut last = None;
        if let Some(t) = self.containing_type_declaration() {
            for decl in ast.node(t).list("bodyDeclarations") {
                if !(decl.is(NodeKind::FieldDeclaration) || decl.is(NodeKind::Initializer)) {
                    continue;
                }
                if decl.modifiers() & modifier::STATIC != 0 && depends(&selected, decl) {
                    last = Some(decl.id);
                }
            }
        }
        match last {
            None => self.insert_first = true,
            Some(l) => self.to_insert_after = Some(l),
        }
    }

    /// `getReplacementScope()`.
    fn replacement_scope(&mut self) -> Vec<NodeId> {
        let ast = self.ast.clone();
        let mut scope = Vec::new();
        let Some(t) = self.containing_type_declaration() else { return scope };
        let containing = ast.node(t);
        if containing.is(NodeKind::EnumDeclaration) {
            scope.extend(containing.list("enumConstants").iter().map(|n| n.id));
        }
        for m in containing.list("modifiers") {
            if m.kind().is_annotation() {
                scope.push(m.id);
            }
        }
        self.compute_declaration_location();
        let mut reached = false;
        for bd in containing.list("bodyDeclarations") {
            if Some(bd.id) == self.to_insert_after {
                reached = true;
            }
            let static_field_or_initializer = (bd.is(NodeKind::FieldDeclaration) || bd.is(NodeKind::Initializer)) && bd.modifiers() & modifier::STATIC != 0;
            if self.insert_first || reached || !static_field_or_initializer {
                scope.push(bd.id);
            }
        }
        scope
    }

    /// `getFragmentsToReplace()`.
    fn fragments_to_replace(&mut self) -> Vec<Fragment> {
        let ast = self.ast.clone();
        let selected = self.selected_expression().unwrap();
        let mut result = Vec::new();
        if self.replace_all {
            for scope in self.replacement_scope() {
                let matches = fragments::full_subtree(ast.node(scope)).sub_fragments_matching(&selected, &ast);
                result.extend(matches.into_iter().filter(|f| can_replace(f.node(&ast))));
            }
        } else if can_replace(selected.node(&ast)) {
            result.push(selected);
        }
        result
    }

    /// `replaceExpressionsWithConstant()`.
    fn replace_expressions_with_constant(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let type_name = self.containing_type_declaration().and_then(|t| ast.node(t).binding()).map(|b| b.name().to_owned()).unwrap_or_default();
        for fragment in self.fragments_to_replace() {
            let node = fragment.node(&ast);
            let in_annotation = self.is_in_type_declaration_annotation(node);
            if in_annotation && self.visibility == PRIVATE {
                continue;
            }
            let reference = cu.rewrite.new_simple_name(&self.constant_name);
            let mut replacement = reference;
            if self.qualify_references || in_annotation {
                let qualifier = cu.rewrite.new_simple_name(&type_name);
                let q = cu.rewrite.new_node(NodeKind::QualifiedName);
                cu.rewrite.put_child(q, "qualifier", qualifier);
                cu.rewrite.put_child(q, "name", reference);
                replacement = q;
            }
            fragment.replace(&mut cu.rewrite, replacement);
        }
    }
}

/// `depends(selected, bd)`.
fn depends(selected: &Fragment, bd: Node<'_>) -> bool {
    if !bd.is(NodeKind::FieldDeclaration) {
        return false;
    }
    bd.list("fragments").iter().filter_map(|f| f.child("name")).any(|name| !selected.sub_fragments_matching(&fragments::full_subtree(name), bd.ast).is_empty())
}

/// `canReplace(fragment)` (ExtractConstant).
fn can_replace(node: Node<'_>) -> bool {
    let parent = node.parent();
    if parent.is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) && node.location_is("name") {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::ExpressionStatement)) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::SwitchCase)) && node.kind().is_name() {
        if let Some(t) = node.type_binding() {
            return !t.is_enum();
        }
    }
    true
}
