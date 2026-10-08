//! Port of jdt.ls `org.eclipse.jdt.ls.core.internal.corext.refactoring.code.ExtractFieldRefactoring`
//! (Extract to Field).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::rewrite::import_rewrite::TypeLocation;
use crate::rewrite::RNode;
use crate::semantic_ast::{modifier, Ast, Node, NodeId, NodeKind};

use super::checks;
use super::extract_temp::{self, check_rvalue, import_context, is_declaration, CuRewrite};
use super::fragments::{self, Fragment};
use super::naming::{self, VarKind};
use super::scope::{self, ScopeAnalyzer};
use super::{msg, Status};

pub const INITIALIZE_IN_FIELD: i32 = 0;
pub const INITIALIZE_IN_METHOD: i32 = 1;
pub const INITIALIZE_IN_CONSTRUCTOR: i32 = 2;

/// jdt.ls `RefactoringCoreMessages` (`corext/refactoring/refactoring.properties`).
fn ls_msg(key: &str) -> &'static str {
    crate::correction::messages::ls_refactoring(key)
}

/// `ExtractFieldRefactoring`.
pub struct ExtractField {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    selection_start: usize,
    selection_length: usize,
    selected: Option<Fragment>,
    guessed: Option<Vec<String>>,
    excluded_variable_names: Option<Vec<String>>,
    excluded_field_names: Option<Vec<String>>,
    declare_final: bool,
    declare_static: bool,
    field_name: String,
    visibility: i32,
    initialize_in: i32,
    initializer_uses_local_types: bool,
    /// The `"name"` linked position group: tracked nodes with their
    /// sequence rank (`addPosition(position, isFirst)`).
    pub name_positions: Vec<(RNode, i32)>,
}

impl ExtractField {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, selection_start: usize, selection_length: usize) -> Self {
        ExtractField {
            ast,
            options,
            selection_start,
            selection_length,
            selected: None,
            guessed: None,
            excluded_variable_names: None,
            excluded_field_names: None,
            declare_final: false,
            declare_static: false,
            field_name: String::new(),
            visibility: modifier::PRIVATE,
            initialize_in: INITIALIZE_IN_METHOD,
            initializer_uses_local_types: false,
            name_positions: Vec::new(),
        }
    }

    pub fn initialize_in(&self) -> i32 {
        self.initialize_in
    }

    /// `setInitializeIn(initializeIn)`.
    pub fn set_initialize_in(&mut self, initialize_in: i32) {
        assert!(matches!(initialize_in, INITIALIZE_IN_CONSTRUCTOR | INITIALIZE_IN_FIELD | INITIALIZE_IN_METHOD));
        self.initialize_in = initialize_in;
    }

    pub fn set_field_name(&mut self, name: &str) {
        self.field_name = name.to_owned();
    }

    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    /// `canEnableSettingDeclareInConstructors()`.
    pub fn can_enable_setting_declare_in_constructors(&mut self) -> bool {
        let ast = self.ast.clone();
        !self.declare_static
            && !self.initializer_uses_local_types
            && !self.method_declaration().is_some_and(|m| ast.node(m).flag("constructor"))
            && !self.is_declared_in_anonymous_class()
            && !self.is_declared_in_static_method()
    }

    /// `canEnableSettingDeclareInMethod()`.
    pub fn can_enable_setting_declare_in_method(&self) -> bool {
        !self.declare_final
    }

    /// `canEnableSettingDeclareInFieldDeclaration()`.
    pub fn can_enable_setting_declare_in_field_declaration(&self) -> bool {
        !self.initializer_uses_local_types
    }

    // ── Initial conditions ──────────────────────────────────────────────────

    /// `checkInitialConditions(pm)`.
    pub fn check_initial_conditions(&mut self) -> Status {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else {
            return checks::check_method_syntax_errors(self.selection_start, self.selection_length, ast.root(), msg("ExtractTempRefactoring_select_expression"));
        };
        if self.method_declaration().is_none() {
            return Status::fatal(ls_msg("ExtractFieldRefactoring_cannot_extract"));
        }
        let associated = selected.node(&ast);
        if associated.ancestors().any(|a| a.is(NodeKind::ConstructorInvocation) || a.is(NodeKind::SuperConstructorInvocation)) {
            return Status::fatal(msg("ExtractTempRefactoring_explicit_constructor"));
        }
        if extract_temp::enclosing_body(associated).is_none() || associated.ancestors().any(|a| a.kind().is_annotation()) {
            return Status::fatal(msg("ExtractTempRefactoring_expr_in_method_or_initializer"));
        }
        if associated.kind().is_name() && associated.location_is("type") && associated.parent().is_some_and(|p| p.is(NodeKind::ClassInstanceCreation)) {
            return Status::fatal(msg("ExtractTempRefactoring_name_in_new"));
        }
        let mut result = Status::ok();
        result.merge(extract_temp::check_expression(associated));
        if result.has_fatal_error() {
            return result;
        }
        result.merge(check_rvalue(associated, "ExtractTempRefactoring_select_expression", "ExtractTempRefactoring_no_void"));
        if result.has_fatal_error() {
            return result;
        }
        if extract_temp::is_used_in_for_initializer_or_updater(associated) {
            return Status::fatal(msg("ExtractTempRefactoring_for_initializer_updater"));
        }
        if extract_temp::is_referring_to_local_variable_from_for(associated) {
            return Status::fatal(msg("ExtractTempRefactoring_refers_to_for_variable"));
        }
        if let Some(t) = self.enclosing_type_declaration() {
            let t = ast.node(t);
            if t.is(NodeKind::TypeDeclaration) && t.flag("interface") {
                return Status::fatal(ls_msg("ExtractFieldRefactoring_interface_methods"));
            }
        }
        result.merge(self.check_temp_type_for_local_type_usage());
        if result.has_fatal_error() {
            return result;
        }
        self.check_temp_initializer_for_local_type_usage();
        self.initialize_defaults();
        result
    }

    /// `checkFinalConditions(pm)` (`checkMatchingFragments()`) followed by
    /// `createChange(pm)`.
    pub fn check_final_conditions(&mut self) -> (Status, Option<CuRewrite>) {
        let ast = self.ast.clone();
        let mut result = Status::ok();
        if let Some(selected) = self.selected_expression() {
            let node = selected.node(&ast);
            if is_left_value(node) && !extract_temp::is_referring_to_local_variable_from_for(node) {
                result.add_warning(msg("ExtractTempRefactoring_assigned_to"));
            }
        }
        (result, Some(self.create_change()))
    }

    /// `createChange(pm)`.
    pub fn create_change(&mut self) -> CuRewrite {
        let mut cu = CuRewrite::new(&self.ast, &self.options);
        cu.rewrite.set_no_comment_source_ranges();
        self.name_positions.clear();
        if self.initialize_in == INITIALIZE_IN_METHOD {
            self.add_initializer_to_method(&mut cu);
        } else if self.initialize_in == INITIALIZE_IN_CONSTRUCTOR {
            self.add_initializers_to_constructors(&mut cu);
        }
        self.add_field_declaration(&mut cu);
        self.add_replace_expression_with_field(&mut cu);
        cu
    }

    /// `initializeDefaults()`.
    fn initialize_defaults(&mut self) {
        self.visibility = modifier::PRIVATE;
        self.declare_static = self.is_declared_in_static_method();
        self.declare_final = false;
        if self.can_enable_setting_declare_in_method() {
            self.initialize_in = INITIALIZE_IN_METHOD;
        } else if self.can_enable_setting_declare_in_field_declaration() {
            self.initialize_in = INITIALIZE_IN_FIELD;
        } else if self.can_enable_setting_declare_in_constructors() {
            self.initialize_in = INITIALIZE_IN_CONSTRUCTOR;
        }
    }

    /// `getSelectedExpression()`.
    fn selected_expression(&mut self) -> Option<Fragment> {
        if self.selected.is_none() {
            self.selected = extract_temp::selected_expression_at(&self.ast, self.selection_start, self.selection_length);
        }
        self.selected.clone()
    }

    /// `checkTempTypeForLocalTypeUsage()`. A type created by the import
    /// rewrite is a new, unresolved node, so only the copied creation or cast
    /// types can refer to local types.
    fn check_temp_type_for_local_type_usage(&mut self) -> Option<Status> {
        let ast = self.ast.clone();
        let expression = self.selected_expression()?.node(&ast);
        let binding = expression.type_binding();
        let resulting = if expression.is(NodeKind::ClassInstanceCreation) && binding.is_none_or(|b| b.type_arguments().is_empty()) {
            expression.child("type")
        } else if expression.is(NodeKind::CastExpression) {
            expression.child("type")
        } else {
            None
        };
        let resulting = resulting?;
        let type_parameters = self.method_type_parameters();
        let mut analyzer = LocalTypeAndVariableUsageAnalyzer::new(type_parameters);
        analyzer.analyze(resulting);
        if !analyzer.references_to_enclosing.is_empty() {
            return Some(Status::fatal(ls_msg("ExtractFieldRefactoring_uses_type_declared_locally")));
        }
        None
    }

    /// `checkTempInitializerForLocalTypeUsage()`.
    fn check_temp_initializer_for_local_type_usage(&mut self) {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else { return };
        let type_parameters = self.method_type_parameters();
        let mut analyzer = LocalTypeAndVariableUsageAnalyzer::new(type_parameters);
        analyzer.analyze(selected.node(&ast));
        self.initializer_uses_local_types = !analyzer.references_to_enclosing.is_empty();
    }

    /// The keys of `getMethodDeclaration().resolveBinding().getTypeParameters()`.
    fn method_type_parameters(&mut self) -> Vec<String> {
        let ast = self.ast.clone();
        self.method_declaration()
            .and_then(|m| ast.node(m).binding())
            .map(|b| b.type_parameters().iter().map(|t| t.key().to_owned()).collect())
            .unwrap_or_default()
    }

    /// `guessFieldName()`.
    pub fn guess_field_name(&mut self) -> String {
        let names = self.guess_field_names();
        names.into_iter().next().unwrap_or_else(|| self.field_name.clone())
    }

    /// `guessFieldNames()`.
    pub fn guess_field_names(&mut self) -> Vec<String> {
        if let Some(g) = &self.guessed {
            return g.clone();
        }
        let ast = self.ast.clone();
        let mut result = Vec::new();
        if let Some(selected) = self.selected_expression() {
            let expression = selected.node(&ast);
            let binding = expression.type_binding().or_else(|| checks::guess_binding_for_reference(expression));
            let modifiers = self.modifiers();
            let kind = if modifiers & modifier::FINAL != 0 && modifiers & modifier::STATIC != 0 {
                VarKind::StaticFinalField
            } else if modifiers & modifier::STATIC != 0 {
                VarKind::StaticField
            } else {
                VarKind::InstanceField
            };
            let excluded = self.excluded_field_names();
            result = naming::variable_name_suggestions(kind, binding, Some(expression), &excluded, &self.options);
        }
        self.guessed = Some(result.clone());
        result
    }

    /// `getExcludedVariableNames()`.
    fn excluded_variable_names(&mut self) -> Vec<String> {
        if let Some(e) = &self.excluded_variable_names {
            return e.clone();
        }
        let ast = self.ast.clone();
        let names: Vec<String> = match self.selected_expression() {
            Some(selected) => ScopeAnalyzer::new(ast.root())
                .declarations_in_scope(selected.start(&ast), scope::VARIABLES | scope::CHECK_VISIBILITY)
                .iter()
                .map(|b| b.name().to_owned())
                .collect(),
            None => Vec::new(),
        };
        self.excluded_variable_names = Some(names.clone());
        names
    }

    /// `getExcludedFieldNames()`.
    fn excluded_field_names(&mut self) -> Vec<String> {
        if let Some(e) = &self.excluded_field_names {
            return e.clone();
        }
        let ast = self.ast.clone();
        let mut result = Vec::new();
        if let Some(t) = self.enclosing_type_declaration().map(|t| ast.node(t)).filter(|t| t.is(NodeKind::TypeDeclaration)) {
            for field in t.list("bodyDeclarations").into_iter().filter(|d| d.is(NodeKind::FieldDeclaration)) {
                for fragment in field.list("fragments") {
                    result.push(fragment.child("name").map(|n| n.identifier()).unwrap_or_default());
                }
            }
        }
        self.excluded_field_names = Some(result.clone());
        result
    }

    /// `isStandaloneExpression()`.
    fn is_standalone_expression(&mut self) -> bool {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else { return false };
        let target = selected.node(&ast);
        target.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement) || p.is(NodeKind::LambdaExpression))
            && selected.matches(&fragments::full_subtree(target), &ast)
    }

    /// `addInitializerToMethod()`.
    fn add_initializer_to_method(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let vds = self.create_new_assignment_statement(cu);
        let Some(selected) = self.selected_expression() else { return };
        let selected_expression = selected.node(&ast);
        let mut target = selected_expression;
        let Some(mut parent) = target.parent() else { return };
        if self.is_standalone_expression() {
            let replacement = if parent.is(NodeKind::LambdaExpression) {
                let mut statements = vec![vds];
                if !checks::is_void(parent.method_binding().and_then(|m| m.return_type())) {
                    let name = cu.rewrite.new_simple_name(&self.field_name);
                    statements.push(cu.rewrite.new_return_statement(Some(name)));
                }
                cu.rewrite.new_block(statements)
            } else if checks::is_control_statement_body(parent.location(), parent.parent()) {
                cu.rewrite.new_block(vec![vds])
            } else {
                vds
            };
            let replacee = if parent.is(NodeKind::LambdaExpression) || !checks::has_semicolon(parent) { selected_expression } else { parent };
            cu.rewrite.replace(RNode::Orig(replacee.id), Some(replacement));
            return;
        }
        while !is_statements_location(target) {
            if checks::is_control_statement_body(target.location(), Some(parent)) {
                // Create an intermediate block if the target was the body of a control statement.
                let moved = cu.rewrite.create_move_target(target.id);
                let replacement = cu.rewrite.new_block(vec![vds, moved]);
                cu.rewrite.replace(RNode::Orig(target.id), Some(replacement));
                return;
            } else if target.location_is("body") && parent.is(NodeKind::LambdaExpression) && parent.child("body").is_some_and(|b| b.kind().is_expression()) {
                let moved = cu.rewrite.create_move_target(target.id);
                let last = if checks::is_void(parent.method_binding().and_then(|m| m.return_type())) {
                    cu.rewrite.new_expression_statement(moved)
                } else {
                    cu.rewrite.new_return_statement(Some(moved))
                };
                let replacement = cu.rewrite.new_block(vec![vds, last]);
                cu.rewrite.replace(RNode::Orig(target.id), Some(replacement));
                return;
            }
            target = parent;
            match parent.parent() {
                Some(p) => parent = p,
                None => return,
            }
        }
        let Some(location) = target.location() else { return };
        cu.rewrite.list_insert_before(RNode::Orig(parent.id), location, vds, RNode::Orig(target.id));
    }

    /// `addInitializersToConstructors(rewrite)`.
    fn add_initializers_to_constructors(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let Some(declaration) = self.method_declaration().and_then(|m| ast.node(m).parent()).filter(|d| d.kind().is_abstract_type_declaration()) else {
            return;
        };
        let constructors: Vec<Node<'_>> = if declaration.is(NodeKind::TypeDeclaration) {
            declaration.list("bodyDeclarations").into_iter().filter(|d| d.is(NodeKind::MethodDeclaration) && d.flag("constructor")).collect()
        } else {
            Vec::new()
        };
        if constructors.is_empty() {
            let constructor = cu.rewrite.new_node(NodeKind::MethodDeclaration);
            cu.rewrite.put_simple(constructor, "constructor", "true");
            let modifiers = cu.rewrite.new_modifiers(declaration.modifiers() & (modifier::PUBLIC | modifier::PROTECTED | modifier::PRIVATE));
            cu.rewrite.put_list(constructor, "modifiers", modifiers);
            let name = cu.rewrite.new_simple_name(&declaration.child("name").map(|n| n.identifier()).unwrap_or_default());
            cu.rewrite.put_child(constructor, "name", name);
            cu.rewrite.put_list(constructor, "parameters", Vec::new());
            let statement = self.create_new_assignment_statement(cu);
            let body = cu.rewrite.new_block(vec![statement]);
            cu.rewrite.put_child(constructor, "body", body);
            let index = compute_insert_index_for_new_constructor(declaration);
            cu.rewrite.list_insert_at(RNode::Orig(declaration.id), "bodyDeclarations", constructor, index);
        } else {
            for constructor in constructors {
                if should_insert_temp_initialization(constructor) {
                    let Some(body) = constructor.child("body") else { continue };
                    let statement = self.create_new_assignment_statement(cu);
                    cu.rewrite.list_insert_last(RNode::Orig(body.id), "statements", statement);
                }
            }
        }
    }

    /// `addFieldDeclaration()`.
    fn add_field_declaration(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let Some(parent) = self.enclosing_type_declaration().map(|t| ast.node(t)) else { return };
        let declarations = parent.list("bodyDeclarations");
        let insert_index = match declarations.iter().rposition(|d| d.is(NodeKind::FieldDeclaration)) {
            None => 0,
            Some(last) => last as i32 + 1,
        };
        let declaration = self.create_new_field_declaration(cu);
        cu.rewrite.list_insert_at(RNode::Orig(parent.id), "bodyDeclarations", declaration, insert_index);
    }

    /// `createNewFieldDeclaration(rewrite)`.
    fn create_new_field_declaration(&mut self, cu: &mut CuRewrite) -> RNode {
        let fragment = cu.rewrite.new_node(NodeKind::VariableDeclarationFragment);
        let name = cu.rewrite.new_simple_name(&self.field_name);
        cu.rewrite.put_child(fragment, "name", name);
        self.name_positions.push((name, 1));
        if self.initialize_in == INITIALIZE_IN_FIELD {
            if let Some(selected) = self.selected_expression() {
                let initializer = selected.create_copy_target(&mut cu.rewrite, true);
                cu.rewrite.put_child(fragment, "initializer", initializer);
            }
        }
        let typ = self.create_field_type(cu);
        let modifiers = cu.rewrite.new_modifiers(self.modifiers());
        cu.rewrite.new_field_declaration(fragment, modifiers, typ)
    }

    /// `createFieldType()`.
    fn create_field_type(&mut self, cu: &mut CuRewrite) -> RNode {
        let ast = self.ast.clone();
        let expression = self.selected_expression().expect("selected expression").node(&ast);
        let mut binding = expression.type_binding();
        if expression.is(NodeKind::ClassInstanceCreation) && binding.is_none_or(|b| b.type_arguments().is_empty()) {
            if let Some(t) = expression.child("type") {
                return cu.rewrite.create_copy_target(t.id);
            }
        }
        if expression.is(NodeKind::CastExpression) {
            if let Some(t) = expression.child("type") {
                return cu.rewrite.create_copy_target(t.id);
            }
        }
        if binding.is_none() {
            binding = checks::guess_binding_for_reference(expression);
        }
        match binding.and_then(crate::correction::type_mismatch::bindings::normalize_for_declaration_use) {
            Some(b) => {
                let context = import_context(&self.ast, expression, &self.options);
                cu.imports.add_import_type(b, &mut cu.rewrite, &context, TypeLocation::LocalVariable)
            }
            None => {
                let name = cu.rewrite.new_simple_name("Object");
                cu.rewrite.new_simple_type(name)
            }
        }
    }

    /// `addReplaceExpressionWithField()`.
    fn add_replace_expression_with_field(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else { return };
        let selected_expression = selected.node(&ast);
        let target = selected_expression;
        let Some(parent) = target.parent() else { return };
        if self.is_standalone_expression() && (self.initialize_in == INITIALIZE_IN_FIELD || self.initialize_in == INITIALIZE_IN_CONSTRUCTOR) {
            let replacee = if parent.is(NodeKind::LambdaExpression) || !checks::has_semicolon(parent) { selected_expression } else { parent };
            if parent.is(NodeKind::LambdaExpression) || checks::is_control_statement_body(parent.location(), parent.parent()) {
                let field_name = cu.rewrite.new_simple_name(&self.field_name);
                let replacement = self.wrap_as_field_access_expression(cu, field_name);
                self.name_positions.push((replacement, 1));
                cu.rewrite.replace(RNode::Orig(replacee.id), Some(replacement));
            } else {
                cu.rewrite.remove(RNode::Orig(replacee.id));
            }
            return;
        }
        let fragments_to_replace: Vec<Fragment> = vec![selected].into_iter().filter(|f| can_replace(f.node(&ast))).collect();
        let mut seen: Vec<Fragment> = Vec::new();
        for fragment in fragments_to_replace {
            if seen.contains(&fragment) {
                continue;
            }
            seen.push(fragment.clone());
            let field_name = cu.rewrite.new_simple_name(&self.field_name);
            let replacer = self.wrap_as_field_access_expression(cu, field_name);
            fragment.replace(&mut cu.rewrite, replacer);
            self.name_positions.push((replacer, 1));
        }
    }

    /// `createNewAssignmentStatement()`.
    fn create_new_assignment_statement(&mut self, cu: &mut CuRewrite) -> RNode {
        let field_name = cu.rewrite.new_simple_name(&self.field_name);
        self.name_positions.push((field_name, 0));
        let lhs = self.wrap_as_field_access_expression(cu, field_name);
        let selected = self.selected_expression().expect("selected expression");
        let rhs = selected.create_copy_target(&mut cu.rewrite, true);
        let assignment = cu.rewrite.new_assignment(lhs, "=", rhs);
        cu.rewrite.new_expression_statement(assignment)
    }

    /// `wrapAsFieldAccessExpression(fieldName)`.
    fn wrap_as_field_access_expression(&mut self, cu: &mut CuRewrite, field_name: RNode) -> RNode {
        if !self.excluded_variable_names().contains(&self.field_name) {
            return field_name;
        }
        if self.modifiers() & modifier::STATIC != 0 {
            let type_name = cu.rewrite.new_simple_name(&self.enclosing_type_name());
            self.name_positions.push((type_name, 1));
            let qualified = cu.rewrite.new_node(NodeKind::QualifiedName);
            cu.rewrite.put_child(qualified, "qualifier", type_name);
            cu.rewrite.put_child(qualified, "name", field_name);
            return qualified;
        }
        // `wrapAsFieldAccess(fieldName, ast)`.
        let this = cu.rewrite.new_this_expression();
        if self.is_declared_in_lambda_expression() {
            let qualifier = cu.rewrite.new_simple_name(&self.enclosing_type_name());
            cu.rewrite.put_child(this, "qualifier", qualifier);
        }
        cu.rewrite.new_field_access(this, field_name)
    }

    /// `getModifiers()`.
    fn modifiers(&mut self) -> i32 {
        let mut flags = self.visibility;
        if self.is_declared_in_static_method() {
            flags |= modifier::STATIC;
        }
        flags
    }

    /// `getMethodDeclaration()`.
    fn method_declaration(&mut self) -> Option<NodeId> {
        let ast = self.ast.clone();
        let selected = self.selected_expression()?;
        let found = selected.node(&ast).ancestors().find(|a| a.is(NodeKind::MethodDeclaration)).map(|m| m.id);
        found
    }

    /// `getEnclosingTypeDeclaration()`.
    fn enclosing_type_declaration(&mut self) -> Option<NodeId> {
        let ast = self.ast.clone();
        if self.is_declared_in_lambda_expression() {
            let selected = self.selected_expression()?;
            return selected.node(&ast).ancestors().find(|a| a.kind().is_abstract_type_declaration()).map(|t| t.id);
        }
        self.method_declaration().and_then(|m| ast.node(m).parent()).map(|p| p.id)
    }

    /// `getEnclosingTypeName()`.
    fn enclosing_type_name(&mut self) -> String {
        let ast = self.ast.clone();
        let Some(t) = self.enclosing_type_declaration() else { return String::new() };
        enclosing_type(ast.node(t)).map(|b| b.name().to_owned()).unwrap_or_default()
    }

    /// `isDeclaredInLambdaExpression()`.
    fn is_declared_in_lambda_expression(&mut self) -> bool {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else { return false };
        for a in selected.node(&ast).ancestors() {
            if a.is(NodeKind::LambdaExpression) {
                return true;
            }
            if a.kind().is_body_declaration() {
                return false;
            }
        }
        false
    }

    /// `isDeclaredInAnonymousClass()`.
    fn is_declared_in_anonymous_class(&mut self) -> bool {
        let ast = self.ast.clone();
        self.selected_expression().is_some_and(|s| s.node(&ast).ancestors().any(|a| a.is(NodeKind::AnonymousClassDeclaration)))
    }

    /// `isDeclaredInStaticMethod()`.
    fn is_declared_in_static_method(&mut self) -> bool {
        let ast = self.ast.clone();
        self.method_declaration().is_some_and(|m| ast.node(m).modifiers() & modifier::STATIC != 0)
    }
}

/// `ASTNodes.getEnclosingType(node)`.
fn enclosing_type(node: Node<'_>) -> Option<crate::semantic_ast::BindingRef<'_>> {
    std::iter::once(node)
        .chain(node.ancestors())
        .find(|n| n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration))
        .and_then(|n| n.binding())
}

/// `locationInParent == Block.STATEMENTS_PROPERTY || locationInParent == SwitchStatement.STATEMENTS_PROPERTY`.
fn is_statements_location(n: Node<'_>) -> bool {
    n.location_is("statements") && n.parent().is_some_and(|p| p.is(NodeKind::Block) || p.is(NodeKind::SwitchStatement))
}

/// `computeInsertIndexForNewConstructor(declaration)`.
fn compute_insert_index_for_new_constructor(declaration: Node<'_>) -> i32 {
    let declarations = declaration.list("bodyDeclarations");
    if declarations.is_empty() {
        return 0;
    }
    match declarations.iter().position(|d| d.is(NodeKind::MethodDeclaration)) {
        Some(i) => i as i32,
        None => declarations.len() as i32,
    }
}

/// `shouldInsertTempInitialization(constructor)`.
fn should_insert_temp_initialization(constructor: Node<'_>) -> bool {
    let Some(body) = constructor.child("body") else { return false };
    let statements = body.list("statements");
    !statements.first().is_some_and(|s| s.is(NodeKind::ConstructorInvocation))
}

/// `canReplace(fragment)` (ExtractField).
fn can_replace(node: Node<'_>) -> bool {
    let parent = node.parent();
    if parent.is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) && node.location_is("name") {
        return false;
    }
    if extract_temp::is_method_parameter(node) || extract_temp::is_throwable_in_catch_block(node) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::ExpressionStatement) || p.is(NodeKind::LambdaExpression)) {
        return false;
    }
    if is_left_value(node) {
        return false;
    }
    if extract_temp::is_referring_to_local_variable_from_for(node) || extract_temp::is_used_in_for_initializer_or_updater(node) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::SwitchCase)) {
        return false;
    }
    true
}

/// `isLeftValue(node)` (ExtractField).
fn is_left_value(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else { return false };
    if parent.is(NodeKind::Assignment) && node.location_is("leftHandSide") {
        return true;
    }
    if parent.is(NodeKind::PostfixExpression) {
        return true;
    }
    if parent.is(NodeKind::PrefixExpression) {
        return matches!(parent.simple("operator"), Some("--" | "++"));
    }
    false
}

/// `ExtractFieldRefactoring.LocalTypeAndVariableUsageAnalyzer` (a
/// `HierarchicalASTVisitor` that only looks at simple names).
struct LocalTypeAndVariableUsageAnalyzer {
    local_definitions: Vec<String>,
    references_to_enclosing: Vec<NodeId>,
    method_type_variables: Vec<String>,
}

impl LocalTypeAndVariableUsageAnalyzer {
    fn new(method_type_variables: Vec<String>) -> Self {
        LocalTypeAndVariableUsageAnalyzer { local_definitions: Vec::new(), references_to_enclosing: Vec::new(), method_type_variables }
    }

    fn analyze(&mut self, node: Node<'_>) {
        super::walk(node, &mut |n| {
            if n.is(NodeKind::SimpleName) {
                self.visit_simple_name(n);
            }
            !super::is_doc(n)
        });
    }

    fn visit_simple_name(&mut self, node: Node<'_>) {
        let declaration = is_declaration(node);
        if let Some(t) = node.type_binding() {
            if t.is_local() {
                if declaration {
                    self.local_definitions.push(t.key().to_owned());
                } else if !self.local_definitions.iter().any(|k| k == t.key()) {
                    self.references_to_enclosing.push(node.id);
                }
            }
            if t.is_type_variable() {
                if declaration {
                    self.local_definitions.push(t.key().to_owned());
                } else if !self.local_definitions.iter().any(|k| k == t.key()) && self.method_type_variables.iter().any(|k| k == t.key()) {
                    self.references_to_enclosing.push(node.id);
                }
            }
        }
        if let Some(b) = node.binding() {
            if b.is_variable() && !b.is_field() {
                if declaration {
                    self.local_definitions.push(b.key().to_owned());
                } else if !self.local_definitions.iter().any(|k| k == b.key()) {
                    self.references_to_enclosing.push(node.id);
                }
            }
        }
    }
}
