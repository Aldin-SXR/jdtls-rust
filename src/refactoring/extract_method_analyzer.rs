//! Port of `ExtractMethodAnalyzer` (with its `CodeAnalyzer` /
//! `StatementAnalyzer` / `SelectionAnalyzer` bases), `LocalTypeAnalyzer`,
//! the extract method `ExceptionAnalyzer` and `CommentAnalyzer`.

use std::sync::Arc;

use crate::rewrite::scanner::{Tok, TokenScanner};
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::resolve::find_parent_type;
use crate::semantic_ast::{Ast, BindingId, BindingRef, Node, NodeId, NodeKind};

use super::flow::{self, ComputeMode, FlowAnalyzer, FlowContext, FlowRef};
use super::selection::{self, Selection};
use super::{msg, Status};

pub const ERROR: i32 = -2;
pub const UNDEFINED: i32 = -1;
pub const NO: i32 = 0;
pub const EXPRESSION: i32 = 1;
pub const ACCESS_TO_LOCAL: i32 = 2;
pub const RETURN_STATEMENT_VOID: i32 = 3;
pub const RETURN_STATEMENT_VALUE: i32 = 4;
pub const MULTIPLE: i32 = 5;

fn manipulation(key: &str) -> &'static str {
    crate::correction::messages::manipulation(key)
}

/// How `fReturnType` was created (materialized in the change's rewrite).
#[derive(Clone, Debug)]
pub enum ReturnType {
    /// `ast.newPrimitiveType(PrimitiveType.VOID)`.
    Void,
    /// `rewriter.addImport(binding, ast, context(node), RETURN_TYPE)`.
    Import { binding: BindingId, context: NodeId },
    /// `ASTNodeFactory.newReturnType(lambda, ast, rewriter, null)`.
    LambdaReturn { lambda: NodeId },
    /// `ASTNodeFactory.newNonVarType(ast, declaration, rewriter, context(declaration))`.
    NonVar { declaration: NodeId },
    /// The enclosing method's return type (copied).
    Copy { node: NodeId },
}

/// A return type binding (`ast.resolveWellKnownType("void")` or a binding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeRef {
    Void,
    Binding(BindingId),
}

/// `ExtractMethodAnalyzer`.
pub struct ExtractMethodAnalyzer {
    pub ast: Arc<Ast>,
    pub selection: Selection,
    pub status: Status,
    selected: Option<Vec<NodeId>>,
    last_covering: Option<NodeId>,
    pub enclosing_body_declaration: Option<NodeId>,
    enclosing_method_binding: Option<BindingId>,
    max_variable_id: i32,
    pub return_kind: i32,
    pub return_type: Option<ReturnType>,
    pub return_type_binding: Option<TypeRef>,
    input_flow_info: Option<FlowRef>,
    input_flow_context: Option<FlowContext>,
    pub arguments: Vec<BindingId>,
    pub method_locals: Vec<BindingId>,
    pub type_variables: Vec<BindingId>,
    pub return_value: Option<BindingId>,
    pub caller_locals: Vec<BindingId>,
    pub return_local: Option<BindingId>,
    pub all_exceptions: Vec<BindingId>,
    pub expression_binding: Option<BindingId>,
    pub force_static: bool,
    is_last_statement_selected: bool,
    enclosing_loop_label: Option<String>,
    pub selection_changed: bool,
}

impl ExtractMethodAnalyzer {
    pub fn new(ast: Arc<Ast>, selection: Selection) -> Self {
        ExtractMethodAnalyzer {
            ast,
            selection,
            status: Status::ok(),
            selected: None,
            last_covering: None,
            enclosing_body_declaration: None,
            enclosing_method_binding: None,
            max_variable_id: 0,
            return_kind: UNDEFINED,
            return_type: None,
            return_type_binding: None,
            input_flow_info: None,
            input_flow_context: None,
            arguments: Vec::new(),
            method_locals: Vec::new(),
            type_variables: Vec::new(),
            return_value: None,
            caller_locals: Vec::new(),
            return_local: None,
            all_exceptions: Vec::new(),
            expression_binding: None,
            force_static: false,
            is_last_statement_selected: false,
            enclosing_loop_label: None,
            selection_changed: false,
        }
    }

    // ── SelectionAnalyzer accessors ─────────────────────────────────────────

    pub fn selected_nodes(&self) -> Vec<Node<'_>> {
        self.selected.as_ref().map(|s| s.iter().map(|&i| self.ast.node(i)).collect()).unwrap_or_default()
    }

    pub fn selected_ids(&self) -> Vec<NodeId> {
        self.selected.clone().unwrap_or_default()
    }

    pub fn has_selected_nodes(&self) -> bool {
        self.selected.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn first_selected(&self) -> Option<Node<'_>> {
        self.selected.as_ref().and_then(|s| s.first()).map(|&i| self.ast.node(i))
    }

    pub fn last_selected(&self) -> Option<Node<'_>> {
        self.selected.as_ref().and_then(|s| s.last()).map(|&i| self.ast.node(i))
    }

    pub fn last_covering_node(&self) -> Option<Node<'_>> {
        self.last_covering.map(|i| self.ast.node(i))
    }

    pub fn is_expression_selected(&self) -> bool {
        self.first_selected().is_some_and(|n| n.kind().is_expression())
    }

    /// `getSelectedNodeRange()`: `(offset, length)`.
    pub fn selected_node_range(&self) -> Option<(usize, usize)> {
        let first = self.first_selected()?;
        let last = self.last_selected()?;
        Some((first.start(), last.end() - first.start()))
    }

    pub fn enclosing_body_declaration(&self) -> Option<Node<'_>> {
        self.enclosing_body_declaration.map(|i| self.ast.node(i))
    }

    fn reset(&mut self) {
        self.selected = None;
    }

    fn invalid_selection(&mut self, message: &str) {
        self.status.add_fatal(message);
        self.reset();
    }

    /// `root.accept(analyzer)`.
    pub fn run(&mut self) {
        let ast = self.ast.clone();
        self.accept(ast.root());
    }

    fn accept(&mut self, n: Node<'_>) {
        if self.visit(n) {
            for c in n.children() {
                self.accept(c);
            }
        }
        self.end_visit(n);
    }

    /// `SelectionAnalyzer.visitNode`.
    fn visit_node(&mut self, n: Node<'_>) -> bool {
        let sel = self.selection;
        if sel.lies_outside(n) {
            false
        } else if sel.covers(n) {
            if self.selected.is_none() {
                self.selected = Some(vec![n.id]);
            } else {
                // `handleNextSelectedNode`: super + `checkParent(node)`.
                let first_parent = self.first_selected().and_then(|f| f.parent()).map(|p| p.id);
                if let Some(list) = &mut self.selected {
                    if !list.is_empty() && first_parent == n.parent().map(|p| p.id) {
                        list.push(n.id);
                    }
                }
                let ok = n.ancestors().any(|a| Some(a.id) == first_parent);
                if !ok {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_parent_mismatch"));
                }
            }
            false
        } else if sel.covered_by(n) {
            self.last_covering = Some(n.id);
            true
        } else if sel.ends_in(n) {
            // `handleSelectionEndsIn`.
            self.invalid_selection(msg("StatementAnalyzer_doesNotCover"));
            false
        } else {
            true
        }
    }

    fn is_first_selected_node(&self, n: Node<'_>) -> bool {
        self.selection.visit_mode(n) == selection::SELECTED && self.first_selected().map(|f| f.id) == Some(n.id)
    }

    fn visit(&mut self, n: Node<'_>) -> bool {
        match n.kind() {
            NodeKind::AnonymousClassDeclaration => {
                let result = self.visit_node(n);
                if self.is_first_selected_node(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_anonymous_type"));
                    return false;
                }
                result
            }
            NodeKind::Assignment => {
                let result = self.visit_node(n);
                let sel = self.selection;
                let selected = NodeFinder::perform(n, sel.offset(), sel.length.max(0) as usize);
                let lhs = n.child("leftHandSide");
                let rhs = n.child("rightHandSide");
                if selected.is_some_and(is_left_hand_side_of_assignment) || (lhs.is_some_and(|l| sel.covers(l)) && !rhs.is_some_and(|r| sel.covers(r))) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_leftHandSideOfAssignment"));
                    return false;
                }
                result
            }
            NodeKind::DoStatement => {
                let result = self.visit_node(n);
                let ast = self.ast.clone();
                let mut scanner = TokenScanner::new(&ast.source);
                if let Ok(action_start) = scanner.token_end_offset(Tok::Kw("do"), n.start() as i32) {
                    if self.selection.offset() as i32 == action_start {
                        self.invalid_selection(msg("ExtractMethodAnalyzer_after_do_keyword"));
                        return false;
                    }
                }
                result
            }
            NodeKind::LambdaExpression => {
                let sel = self.selection;
                let (start, end) = (sel.start, sel.exclusive_end());
                let (lambda_start, lambda_end) = (n.start() as i64, n.end() as i64);
                let Some(body) = n.child("body") else { return false };
                let (body_start, body_end) = (body.start() as i64, body.end() as i64);
                let mut valid = false;
                if body.is(NodeKind::Block) && body_start <= start && end <= body_end {
                    valid = true;
                } else if body.kind().is_expression() {
                    let ast = self.ast.clone();
                    let mut scanner = TokenScanner::new(&ast.source);
                    if let Ok(arrow_end) = scanner.token_end_offset(Tok::Op("->"), lambda_start as i32) {
                        if start >= arrow_end as i64 {
                            valid = true;
                        }
                    }
                }
                if start <= lambda_start && end >= lambda_end {
                    valid = true;
                }
                if !valid {
                    return false;
                }
                self.visit_node(n)
            }
            NodeKind::MethodDeclaration => {
                let Some(body) = n.child("body") else { return false };
                let sel = self.selection;
                if body.start() as i64 >= sel.start || sel.exclusive_end() >= body.end() as i64 {
                    return false;
                }
                self.visit_node(n)
            }
            NodeKind::ConstructorInvocation | NodeKind::SuperConstructorInvocation => {
                let result = self.visit_node(n);
                if self.selection.visit_mode(n) == selection::SELECTED {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_super_or_this"));
                    return false;
                }
                result
            }
            NodeKind::VariableDeclarationFragment => {
                let result = self.visit_node(n);
                if self.is_first_selected_node(n) {
                    if n.parent().is_some_and(|p| p.is(NodeKind::FieldDeclaration)) {
                        self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_variable_declaration_fragment_from_field"));
                    } else {
                        self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_variable_declaration_fragment"));
                    }
                    return false;
                }
                result
            }
            _ => self.visit_node(n),
        }
    }

    fn contains(&self, n: Option<Node<'_>>) -> bool {
        n.is_some_and(|n| self.selected.as_ref().is_some_and(|s| s.contains(&n.id)))
    }

    fn contains_any(&self, list: &[Node<'_>]) -> bool {
        self.selected.as_ref().is_some_and(|s| s.iter().any(|id| list.iter().any(|l| l.id == *id)))
    }

    /// `StatementAnalyzer.doAfterValidation`.
    fn do_after_validation(&self, n: Node<'_>) -> bool {
        self.first_selected().is_some_and(|f| f.parent().map(|p| p.id) == Some(n.id)) && self.selection.end_visit_mode(n) == selection::AFTER
    }

    fn is_resource_in_try(&self, n: Node<'_>) -> bool {
        self.selection.end_visit_mode(n) == selection::SELECTED
            && self.first_selected().map(|f| f.id) == Some(n.id)
            && n.location_is("resources")
            && n.parent().is_some_and(|p| p.is(NodeKind::TryStatement))
    }

    fn check_type_in_declaration(&mut self, t: Option<Node<'_>>) {
        if let Some(t) = t {
            if self.selection.end_visit_mode(t) == selection::SELECTED && self.first_selected().map(|f| f.id) == Some(t.id) {
                self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_variable_declaration"));
            }
        }
    }

    fn end_visit(&mut self, n: Node<'_>) {
        match n.kind() {
            NodeKind::CompilationUnit => {
                self.end_visit_compilation_unit_em();
                self.end_visit_compilation_unit_statement(n);
            }
            NodeKind::FieldDeclaration => {
                if self.contains_any(&n.list("fragments")) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_variable_declaration_fragment_from_field"));
                }
            }
            NodeKind::ForStatement => {
                if self.selection.end_visit_mode(n) == selection::AFTER {
                    let first = self.first_selected().map(|f| f.id);
                    let last = self.last_selected().map(|f| f.id);
                    if n.list("initializers").iter().any(|i| Some(i.id) == first) {
                        self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_for_initializer"));
                    } else if n.list("updaters").iter().any(|i| Some(i.id) == last) {
                        self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_for_updater"));
                    }
                }
                if self.do_after_validation(n) {
                    let contains_expression = self.contains(n.child("expression"));
                    let contains_updaters = self.contains_any(&n.list("updaters"));
                    if self.contains_any(&n.list("initializers")) && contains_expression {
                        self.invalid_selection(manipulation("StatementAnalyzer_for_initializer_expression"));
                    } else if contains_expression && contains_updaters {
                        self.invalid_selection(manipulation("StatementAnalyzer_for_expression_updater"));
                    } else if contains_updaters && self.contains(n.child("body")) {
                        self.invalid_selection(manipulation("StatementAnalyzer_for_updater_body"));
                    }
                }
            }
            NodeKind::EnhancedForStatement => {
                if self.selection.end_visit_mode(n) == selection::AFTER && n.child("parameter").map(|p| p.id) == self.first_selected().map(|f| f.id) && self.has_selected_nodes() {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_for_initializer"));
                }
            }
            NodeKind::QualifiedName | NodeKind::SimpleName => {
                if self.is_resource_in_try(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_resource_used_in_try_with_resources"));
                }
            }
            NodeKind::BreakStatement => {
                if self.is_first_selected_node(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_break"));
                }
            }
            NodeKind::ContinueStatement => {
                if self.is_first_selected_node(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_continue"));
                }
            }
            NodeKind::YieldStatement => {
                if self.is_first_selected_node(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_cannot_extract_yield"));
                }
            }
            NodeKind::VariableDeclarationExpression => {
                if self.is_resource_in_try(n) {
                    self.invalid_selection(msg("ExtractMethodAnalyzer_resource_in_try_with_resources"));
                }
                self.check_type_in_declaration(n.child("type"));
            }
            NodeKind::VariableDeclarationStatement => self.check_type_in_declaration(n.child("type")),
            NodeKind::DoStatement => {
                if self.do_after_validation(n) && self.contains(n.child("body")) && self.contains(n.child("expression")) {
                    self.invalid_selection(manipulation("StatementAnalyzer_do_body_expression"));
                }
            }
            NodeKind::SwitchStatement | NodeKind::SwitchExpression => {
                if self.do_after_validation(n) {
                    let cases: Vec<Node<'_>> = n.list("statements").into_iter().filter(|s| s.is(NodeKind::SwitchCase)).collect();
                    if self.contains_any(&cases) {
                        self.invalid_selection(manipulation(if n.is(NodeKind::SwitchStatement) { "StatementAnalyzer_switch_statement" } else { "StatementAnalyzer_switch_expression" }));
                    }
                }
            }
            NodeKind::SynchronizedStatement => {
                if self.selection.end_visit_mode(n) == selection::SELECTED && self.first_selected().map(|f| f.id) == n.child("body").map(|b| b.id) && self.has_selected_nodes() {
                    self.invalid_selection(manipulation("StatementAnalyzer_synchronized_statement"));
                }
            }
            NodeKind::TryStatement => {
                if self.selection.end_visit_mode(n) == selection::AFTER {
                    let first = self.first_selected().map(|f| f.id);
                    if first.is_some() && (first == n.child("body").map(|b| b.id) || first == n.child("finally").map(|b| b.id)) {
                        self.invalid_selection(manipulation("StatementAnalyzer_try_statement"));
                    } else {
                        for c in n.list("catchClauses") {
                            let first = self.first_selected().map(|f| f.id);
                            if first.is_none() {
                                continue;
                            }
                            if first == Some(c.id) || first == c.child("body").map(|b| b.id) {
                                self.invalid_selection(manipulation("StatementAnalyzer_try_statement"));
                            } else if first == c.child("exception").map(|e| e.id) {
                                self.invalid_selection(manipulation("StatementAnalyzer_catch_argument"));
                            }
                        }
                    }
                }
            }
            NodeKind::WhileStatement => {
                if self.do_after_validation(n) && self.contains(n.child("expression")) && self.contains(n.child("body")) {
                    self.invalid_selection(manipulation("StatementAnalyzer_while_expression_body"));
                }
            }
            _ => {}
        }
    }

    /// `ExtractMethodAnalyzer.endVisit(CompilationUnit)` (before `super`).
    fn end_visit_compilation_unit_em(&mut self) {
        if self.status.has_fatal_error() {
            return;
        }
        if !self.has_selected_nodes() {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_invalid_selection"));
            return;
        }
        let ast = self.ast.clone();
        let first = self.first_selected().unwrap().id;
        let first = ast.node(first);
        let enclosing = first.ancestors().find(|a| a.kind().is_body_declaration());
        self.enclosing_body_declaration = enclosing.map(|e| e.id);
        let Some(enclosing) = enclosing.filter(|e| matches!(e.kind(), NodeKind::MethodDeclaration | NodeKind::FieldDeclaration | NodeKind::Initializer)) else {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_invalid_selection"));
            return;
        };
        if enclosing_type(enclosing).is_none() {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_compile_errors_no_parent_binding"));
            return;
        } else if enclosing.is(NodeKind::MethodDeclaration) {
            self.enclosing_method_binding = enclosing.binding().map(|b| b.id);
        }
        if first.kind().is_expression() && self.selected_ids().len() != 1 {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_single_expression_or_set"));
            return;
        }
        if self.is_expression_selected() {
            if first.kind().is_name() {
                let binding = first.binding();
                if binding.is_some_and(|b| b.is_type()) {
                    self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_type_reference"));
                    return;
                }
                if binding.is_some_and(|b| b.is_method()) {
                    self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_method_name_reference"));
                    return;
                }
                if binding.is_some_and(|b| b.is_variable()) {
                    let parent = first.parent();
                    let mut part_of_qualified_name = false;
                    let mut part_of_qualifier = false;
                    if first.location_is("name") && parent.is_some_and(|p| p.is(NodeKind::QualifiedName)) {
                        part_of_qualified_name = true;
                        let qualified = parent.unwrap();
                        let mut curr = qualified;
                        while let Some(p) = curr.parent().filter(|p| p.is(NodeKind::QualifiedName)) {
                            curr = p;
                        }
                        if curr.id != qualified.id {
                            part_of_qualifier = true;
                        }
                    }
                    let field_access_name = first.location_is("name")
                        && parent.is_some_and(|p| p.is(NodeKind::FieldAccess) && !p.child("expression").is_some_and(|e| e.is(NodeKind::ThisExpression)));
                    if (part_of_qualified_name && !part_of_qualifier) || field_access_name {
                        self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_part_of_qualified_name"));
                        return;
                    }
                }
                if first.is(NodeKind::SimpleName) && super::extract_temp::is_declaration(first) {
                    self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_name_in_declaration"));
                    return;
                }
            }
            self.force_static = first.ancestors().any(|a| a.is(NodeKind::SuperConstructorInvocation) || a.is(NodeKind::ConstructorInvocation));
        }
        let local_types = local_type_analyzer(enclosing, self.selection);
        self.status.merge(local_types);
        self.compute_last_statement_selected();
    }

    /// `StatementAnalyzer.endVisit(CompilationUnit)` + `CodeAnalyzer.checkSelectedNodes`.
    fn end_visit_compilation_unit_statement(&mut self, root: Node<'_>) {
        if !self.has_selected_nodes() {
            return;
        }
        let ast = self.ast.clone();
        let selected = ast.node(self.first_selected().unwrap().id);
        if root.id != selected.id {
            if let Some(parent) = selected.parent() {
                let status = comment_analyzer(&ast, self.selection, parent.start(), parent.length());
                self.status.merge(status);
            }
        }
        if !self.status.has_fatal_error() {
            self.check_selected_nodes();
            if !self.status.has_fatal_error() && self.first_selected().is_some_and(|n| n.is(NodeKind::ArrayInitializer)) {
                self.status.add_fatal(manipulation("CodeAnalyzer_array_initializer"));
            }
        }
    }

    /// `StatementAnalyzer.checkSelectedNodes`.
    fn check_selected_nodes(&mut self) {
        let nodes = self.selected_ids();
        if nodes.is_empty() {
            return;
        }
        let ast = self.ast.clone();
        let node = ast.node(nodes[0]);
        let selection_offset = self.selection.offset() as i32;
        let mut scanner = TokenScanner::new(&ast.source);
        let result = (|| -> Result<bool, crate::rewrite::scanner::ScanError> {
            let start = scanner.next_start_offset(selection_offset, true)?;
            if start == node.start() as i32 {
                let last_node_end = ast.node(*nodes.last().unwrap()).end() as i32;
                let pos = scanner.next_start_offset(last_node_end, true)?;
                let selection_end = self.selection.inclusive_end() as i32;
                if pos <= selection_end {
                    let is_separator = scanner.current_length() == 1 && matches!(ast.char_at(pos as usize), Some(c) if c == b';' as u16 || c == b',' as u16);
                    if start < last_node_end && is_separator {
                        self.selection = Selection::from_start_end(start as usize, (last_node_end - 1) as usize);
                    } else {
                        self.invalid_selection(manipulation("StatementAnalyzer_end_of_selection"));
                    }
                }
                return Ok(true);
            }
            Ok(false)
        })();
        if !matches!(result, Ok(true)) {
            self.invalid_selection(manipulation("StatementAnalyzer_beginning_of_selection"));
        }
    }

    fn compute_last_statement_selected(&mut self) {
        let ast = self.ast.clone();
        let nodes = self.selected_ids();
        if nodes.is_empty() {
            self.is_last_statement_selected = false;
            return;
        }
        let first = ast.node(nodes[0]);
        let mut body = None;
        if let Some(lambda) = find_enclosing_lambda_expression(first) {
            match lambda.child("body") {
                Some(b) if b.is(NodeKind::Block) => body = Some(b),
                _ => {
                    self.is_last_statement_selected = true;
                    return;
                }
            }
        } else if let Some(decl) = self.enclosing_body_declaration() {
            if matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer) {
                body = decl.child("body");
            }
        }
        if let Some(body) = body {
            let statements = body.list("statements");
            self.is_last_statement_selected = match statements.last() {
                Some(last) => *nodes.last().unwrap() == last.id,
                None => true,
            };
        }
    }

    // ── Activation checking ─────────────────────────────────────────────────

    /// `checkInitialConditions(rewriter)`.
    pub fn check_initial_conditions(&mut self) -> Status {
        self.check_expression();
        if self.status.has_fatal_error() {
            return self.status.clone();
        }
        let ast = self.ast.clone();
        let enclosing = self.enclosing_body_declaration().unwrap().id;
        let enclosing = ast.node(enclosing);
        let mut valid = false;
        let mut destination = enclosing.parent().and_then(find_parent_type);
        while let Some(d) = destination {
            if is_valid_destination(d) {
                valid = true;
            }
            destination = d.parent().and_then(find_parent_type);
        }
        if !valid {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_no_valid_destination_type"));
            return self.status.clone();
        }
        self.return_kind = UNDEFINED;
        self.max_variable_id = flow::local_variable_index(enclosing);
        self.analyze_selection();
        if self.status.has_fatal_error() {
            return self.status.clone();
        }
        let mut returns = if self.return_kind == NO { 0 } else { 1 };
        if self.return_value.is_some() {
            self.return_kind = ACCESS_TO_LOCAL;
            returns += 1;
        }
        if self.is_expression_selected() {
            let first = self.first_selected().unwrap();
            let in_statement = first.location_is("expression") && first.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement));
            if returns == 0 || !in_statement {
                self.return_kind = EXPRESSION;
                returns += 1;
            } else {
                let parent = first.parent().unwrap();
                let new_selection = Selection::from_start_end(self.selection.offset(), parent.start() + parent.length());
                self.selection = new_selection;
                self.reset();
                self.selection_changed = true;
                return self.status.clone();
            }
        }
        if returns > 1 {
            self.status.add_fatal(msg("ExtractMethodAnalyzer_ambiguous_return_value"));
            self.return_kind = MULTIPLE;
            return self.status.clone();
        }
        self.init_return_type();
        self.status.clone()
    }

    fn check_expression(&mut self) {
        let nodes = self.selected_nodes();
        if nodes.len() == 1 {
            let node = nodes[0];
            if node.kind().is_type() {
                self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_type_reference"));
            } else if node.parent().is_some_and(|p| p.is(NodeKind::SwitchCase)) && (node.location_is("expression") || node.location_is("expressions")) {
                self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_switch_case"));
            } else if node.kind().is_annotation() || node.ancestors().any(|a| a.kind().is_annotation()) {
                self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_from_annotation"));
            }
        }
    }

    fn init_return_type(&mut self) {
        let ast = self.ast.clone();
        self.return_type = None;
        self.return_type_binding = None;
        let enclosing = self.enclosing_body_declaration.unwrap();
        match self.return_kind {
            ACCESS_TO_LOCAL => {
                if let Some(declaration) = self.return_value.and_then(|v| find_variable_declaration(ast.binding(v), ast.node(enclosing))) {
                    self.return_type = Some(ReturnType::NonVar { declaration: declaration.id });
                    if let Some(b) = declaration.binding() {
                        self.return_type_binding = b.var_type().map(|t| type_ref(t));
                    }
                }
            }
            EXPRESSION => {
                let expression = ast.node(self.first_selected().unwrap().id);
                let binding = if expression.is(NodeKind::ClassInstanceCreation) {
                    expression.child("type").and_then(|t| t.binding().or_else(|| t.type_binding()))
                } else {
                    expression.type_binding()
                };
                self.expression_binding = binding.map(|b| b.id);
                match binding {
                    Some(b) if b.is_null_type() => self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_null_type")),
                    Some(b) => {
                        if let Some(normalized) = crate::correction::type_mismatch::bindings::normalize_for_declaration_use(b) {
                            self.return_type = Some(ReturnType::Import { binding: normalized.id, context: enclosing });
                            self.return_type_binding = Some(type_ref(normalized));
                        }
                    }
                    None => {
                        self.return_type = Some(ReturnType::Void);
                        self.return_type_binding = Some(TypeRef::Void);
                        self.status.add_error(msg("ExtractMethodAnalyzer_cannot_determine_return_type"));
                    }
                }
            }
            RETURN_STATEMENT_VALUE => {
                let first = ast.node(self.first_selected().unwrap().id);
                if let Some(lambda) = find_enclosing_lambda_expression(first) {
                    self.return_type = Some(ReturnType::LambdaReturn { lambda: lambda.id });
                    self.return_type_binding = lambda.method_binding().and_then(|m| m.return_type()).map(type_ref);
                } else if ast.node(enclosing).is(NodeKind::MethodDeclaration) {
                    let rt = ast.node(enclosing).child("returnType2");
                    self.return_type = rt.map(|t| ReturnType::Copy { node: t.id });
                    self.return_type_binding = rt.and_then(|t| t.binding().or_else(|| t.type_binding())).map(type_ref);
                }
            }
            _ => {
                self.return_type = Some(ReturnType::Void);
                self.return_type_binding = Some(TypeRef::Void);
            }
        }
        if self.return_type.is_none() {
            self.return_type = Some(ReturnType::Void);
            self.return_type_binding = Some(TypeRef::Void);
        }
    }

    /// `isLiteralNodeSelected()`.
    pub fn is_literal_node_selected(&self) -> bool {
        let nodes = self.selected_nodes();
        nodes.len() == 1 && matches!(nodes[0].kind(), NodeKind::BooleanLiteral | NodeKind::CharacterLiteral | NodeKind::NullLiteral | NodeKind::NumberLiteral)
    }

    fn analyze_selection(&mut self) {
        let ast = self.ast.clone();
        let mut context = FlowContext::new(0, self.max_variable_id + 1);
        context.set_consider_access_mode(true);
        context.set_compute_mode(ComputeMode::Arguments);
        let selected = self.selected_nodes();
        let info = FlowAnalyzer::in_out(&context, &selected);
        let selected_ids = self.selected_ids();
        if info.borrow().branches() {
            if let Some(problem) = self.can_handle_branches() {
                self.status.add_fatal(problem);
                self.return_kind = ERROR;
                return;
            }
        }
        let (value, void, partial, no_return, throw, undefined) = {
            let i = info.borrow();
            (i.is_value_return(), i.is_void_return(), i.is_partial_return(), i.is_no_return(), i.is_throw(), i.is_undefined())
        };
        if value {
            self.return_kind = RETURN_STATEMENT_VALUE;
        } else if void || (partial && self.is_void_method() && self.is_last_statement_selected) {
            if selected_ids.len() == 1 && ast.node(selected_ids[0]).is(NodeKind::ReturnStatement) {
                self.status.add_fatal(msg("ExtractMethodAnalyzer_cannot_extract_return"));
                self.return_kind = ERROR;
                return;
            }
            self.return_kind = RETURN_STATEMENT_VOID;
        } else if no_return || throw || undefined {
            self.return_kind = NO;
        }
        if self.return_kind == UNDEFINED {
            self.status.add_error(msg("FlowAnalyzer_execution_flow"));
            self.return_kind = NO;
        }
        if let Some(problem) = self.check_for_final_fields() {
            self.status.add_fatal(problem);
            self.return_kind = ERROR;
            return;
        }
        self.input_flow_info = Some(info);
        self.input_flow_context = Some(context);
        self.compute_input();
        self.all_exceptions = exception_analyzer(&self.selected_nodes());
        self.compute_output();
        if self.status.has_fatal_error() {
            return;
        }
        self.adjust_arguments_and_method_locals();
    }

    fn check_for_final_fields(&self) -> Option<&'static str> {
        for n in self.selected_nodes() {
            let mut found = false;
            super::walk(n, &mut |x| {
                if found || x.is(NodeKind::Javadoc) {
                    return false;
                }
                if x.is(NodeKind::Assignment) {
                    let binding = x.child("leftHandSide").and_then(|l| match l.kind() {
                        NodeKind::FieldAccess => l.child("name").and_then(|n| n.binding()),
                        NodeKind::SimpleName => l.binding(),
                        _ => None,
                    });
                    if binding.is_some_and(|b| b.is_variable() && b.is_field() && b.modifiers() & crate::semantic_ast::modifier::FINAL != 0) {
                        found = true;
                    }
                    return false;
                }
                true
            });
            if found {
                return Some(msg("ExtractMethodAnalyzer_cannot_extract_final_field_assignment"));
            }
        }
        None
    }

    /// `findFieldReferencesForType(type)`.
    pub fn find_field_references_for_type(&self, typ: Node<'_>) -> Vec<NodeId> {
        let mut result = Vec::new();
        let Some(type_binding) = typ.binding() else { return result };
        for n in self.selected_nodes() {
            super::walk(n, &mut |x| {
                if x.is(NodeKind::SimpleName) {
                    if let Some(b) = x.binding().filter(|b| b.is_variable() && b.is_field()) {
                        if b.declaring_class().is_some_and(|d| d.key() == type_binding.key()) {
                            result.push(x.id);
                        }
                    }
                }
                !x.is(NodeKind::Javadoc)
            });
        }
        result
    }

    fn can_handle_branches(&mut self) -> Option<String> {
        if self.return_value.is_some() {
            return Some(msg("ExtractMethodAnalyzer_branch_mismatch").to_owned());
        }
        let ast = self.ast.clone();
        let selected = self.selected_ids();
        let last = ast.node(*selected.last().unwrap());
        let body = last.parent().and_then(|p| self.parent_loop_body(p));
        let Some(body) = body.filter(|b| b.is(NodeKind::Block)) else {
            return Some(msg("ExtractMethodAnalyzer_branch_mismatch").to_owned());
        };
        if body.id != last.id {
            let statements = body.list("statements");
            if statements.last().map(|s| s.id) != Some(last.id) {
                return Some(msg("ExtractMethodAnalyzer_branch_mismatch").to_owned());
            }
        }
        let mut pending: Vec<(NodeId, String)> = Vec::new();
        let put = |pending: &mut Vec<(NodeId, String)>, key: NodeId, value: String| {
            if let Some(e) = pending.iter_mut().find(|(k, _)| *k == key) {
                e.1 = value;
            } else {
                pending.push((key, value));
            }
        };
        for id in &selected {
            let mut local_loop_labels: Vec<String> = Vec::new();
            let mut break_targets: Vec<NodeId> = Vec::new();
            let enclosing_label = self.enclosing_loop_label.clone();
            branch_visit(ast.node(*id), &mut |n, end| {
                if !end {
                    match n.kind() {
                        NodeKind::BreakStatement => {
                            let label = n.child("label");
                            if let Some(l) = label.filter(|l| !local_loop_labels.contains(&l.identifier())) {
                                put(&mut pending, l.id, crate::correction::messages::format(msg("ExtractMethodAnalyzer_branch_break_mismatch"), &[&format!("break {}", l.identifier())]));
                            } else if label.is_none() {
                                let parent = n.ancestors().find(|a| matches!(a.kind(), NodeKind::WhileStatement | NodeKind::ForStatement | NodeKind::DoStatement | NodeKind::SwitchStatement | NodeKind::EnhancedForStatement));
                                if let Some(p) = parent.filter(|p| !break_targets.contains(&p.id)) {
                                    put(&mut pending, p.id, msg("ExtractMethodAnalyzer_break_parent_missing").to_owned());
                                }
                            }
                            return false;
                        }
                        NodeKind::LabeledStatement => {
                            if let Some(l) = n.child("label") {
                                local_loop_labels.push(l.identifier());
                            }
                        }
                        _ => {}
                    }
                    true
                } else {
                    match n.kind() {
                        NodeKind::ForStatement | NodeKind::EnhancedForStatement | NodeKind::DoStatement | NodeKind::SwitchStatement | NodeKind::WhileStatement => {
                            pending.retain(|(k, _)| *k != n.id);
                            break_targets.push(n.id);
                        }
                        NodeKind::ContinueStatement => {
                            if let Some(l) = n.child("label").filter(|l| !local_loop_labels.contains(&l.identifier())) {
                                if enclosing_label.as_deref() != Some(l.identifier().as_str()) {
                                    put(&mut pending, n.id, crate::correction::messages::format(msg("ExtractMethodAnalyzer_branch_continue_mismatch"), &[&format!("continue {}", l.identifier())]));
                                }
                            }
                        }
                        _ => {}
                    }
                    true
                }
            });
        }
        pending.into_iter().next().map(|(_, v)| v)
    }

    fn parent_loop_body<'a>(&mut self, node: Node<'a>) -> Option<Node<'a>> {
        let start = std::iter::once(node).chain(node.ancestors()).find(|n| {
            matches!(n.kind(), NodeKind::ForStatement | NodeKind::DoStatement | NodeKind::WhileStatement | NodeKind::EnhancedForStatement | NodeKind::SwitchStatement)
        });
        let stmt = start.filter(|s| !s.is(NodeKind::SwitchStatement)).and_then(|s| s.child("body"));
        if let Some(s) = start {
            if let Some(labeled) = s.parent().filter(|p| p.is(NodeKind::LabeledStatement)) {
                self.enclosing_loop_label = labeled.child("label").map(|l| l.identifier());
            }
        }
        stmt
    }

    fn is_void_method(&self) -> bool {
        let first = self.first_selected().unwrap();
        let binding = if let Some(lambda) = find_enclosing_lambda_expression(first) {
            lambda.method_binding().and_then(|m| m.return_type())
        } else {
            let Some(m) = self.enclosing_method_binding else { return true };
            self.ast.binding(m).return_type()
        };
        super::checks::is_void(binding)
    }

    fn compute_input(&mut self) {
        let info = self.input_flow_info.clone().unwrap();
        let context = self.input_flow_context.as_ref().unwrap();
        let argument_mode = flow::READ | flow::READ_POTENTIAL | flow::WRITE_POTENTIAL | flow::UNKNOWN;
        let args = info.borrow().get(context, argument_mode);
        let locals = info.borrow().get(context, flow::WRITE | flow::WRITE_POTENTIAL);
        let type_vars = info.borrow().type_variables();
        self.arguments = self.remove_selected_declarations(&args);
        self.method_locals = self.remove_selected_declarations(&locals);
        self.type_variables = self.compute_type_variables(&type_vars);
    }

    fn remove_selected_declarations(&self, bindings: &[BindingId]) -> Vec<BindingId> {
        bindings.iter().copied().filter(|&b| !self.ast.binding(b).declaring_node().is_some_and(|d| self.selection.covers(d))).collect()
    }

    fn compute_type_variables(&self, bindings: &[BindingId]) -> Vec<BindingId> {
        let mut result: Vec<BindingId> = Vec::new();
        let ok = |b: BindingRef<'_>| match b.declaring_node() {
            None => true,
            Some(d) => !self.selection.covers(d) && d.parent().is_some_and(|p| p.is(NodeKind::MethodDeclaration)),
        };
        for &b in bindings {
            if ok(self.ast.binding(b)) && !result.contains(&b) {
                result.push(b);
            }
        }
        for &arg in &self.arguments {
            if let Some(t) = self.ast.binding(arg).var_type().filter(|t| t.is_type_variable()) {
                if ok(t) && !result.contains(&t.id) {
                    result.push(t.id);
                }
            }
        }
        result
    }

    fn compute_output(&mut self) {
        let ast = self.ast.clone();
        let mut context = FlowContext::new(0, self.max_variable_id + 1);
        context.set_consider_access_mode(true);
        context.set_compute_mode(ComputeMode::ReturnValues);
        let selected = self.selected_nodes();
        let return_info = FlowAnalyzer::in_out(&context, &selected);
        let mut return_values = return_info.borrow().get(&context, flow::WRITE | flow::WRITE_POTENTIAL | flow::UNKNOWN);
        let (min, len) = self.selected_node_range().unwrap();
        if let Some(covering) = self.last_covering_node() {
            return_values = local_write_visitor(covering, min, min + len, return_values);
        }
        let region_selection = Selection::from_start_length(min, len);
        let mut local_reads: Vec<BindingId> = Vec::new();
        context.set_compute_mode(ComputeMode::Arguments);
        let enclosing = ast.node(self.enclosing_body_declaration.unwrap());
        let reads = FlowAnalyzer::input(&context, region_selection, true, enclosing)
            .map(|i| i.borrow().get(&context, flow::READ | flow::READ_POTENTIAL | flow::UNKNOWN))
            .unwrap_or_default();
        for &binding in &return_values {
            if local_reads.len() >= return_values.len() {
                break;
            }
            if reads.contains(&binding) {
                local_reads.push(binding);
                self.return_value = Some(binding);
            }
        }
        match local_reads.len() {
            0 => self.return_value = None,
            1 => {}
            _ => {
                self.return_value = None;
                self.status.add_fatal(msg("ExtractMethodAnalyzer_assignments_to_local"));
                return;
            }
        }
        let mut caller_locals = Vec::new();
        if let Some(local_info) = FlowAnalyzer::input(&context, region_selection, false, enclosing) {
            for write in local_info.borrow().get(&context, flow::WRITE | flow::WRITE_POTENTIAL | flow::UNKNOWN) {
                if find_variable_declaration(ast.binding(write), enclosing).is_some_and(|d| self.selection.covers(d)) {
                    caller_locals.push(write);
                }
            }
        }
        self.caller_locals = caller_locals;
        if let Some(v) = self.return_value {
            if find_variable_declaration(ast.binding(v), enclosing).is_some_and(|d| self.selection.covers(d)) {
                self.return_local = Some(v);
            }
        }
    }

    fn adjust_arguments_and_method_locals(&mut self) {
        let info = self.input_flow_info.clone().unwrap();
        let context = self.input_flow_context.as_ref().unwrap();
        let mut arguments: Vec<Option<BindingId>> = self.arguments.iter().copied().map(Some).collect();
        let mut locals: Vec<Option<BindingId>> = self.method_locals.iter().copied().map(Some).collect();
        for slot in arguments.iter_mut() {
            let argument = slot.unwrap();
            if info.borrow().has_access_mode(context, argument, flow::WRITE_POTENTIAL) {
                if Some(argument) != self.return_value {
                    *slot = None;
                }
                if slot.is_some() {
                    for l in locals.iter_mut() {
                        if *l == Some(argument) {
                            *l = None;
                        }
                    }
                }
            }
        }
        self.arguments = arguments.into_iter().flatten().collect();
        self.method_locals = locals.into_iter().flatten().collect();
    }

    /// `getExceptions(includeRuntimeExceptions)`.
    pub fn exceptions(&self, include_runtime: bool) -> Vec<BindingId> {
        self.all_exceptions
            .iter()
            .copied()
            .filter(|&e| include_runtime || crate::correction::type_mismatch::bindings::find_type_in_hierarchy(self.ast.binding(e), "java.lang.RuntimeException").is_none())
            .collect()
    }

    pub fn is_valid_destination(&self, n: Node<'_>) -> bool {
        is_valid_destination(n)
    }
}

fn type_ref(b: BindingRef<'_>) -> TypeRef {
    if super::checks::is_void(Some(b)) {
        TypeRef::Void
    } else {
        TypeRef::Binding(b.id)
    }
}

/// `isValidDestination(node)`.
pub fn is_valid_destination(n: Node<'_>) -> bool {
    !n.is(NodeKind::AnnotationTypeDeclaration)
}

/// `ASTNodes.getEnclosingType(node)`.
pub fn enclosing_type(node: Node<'_>) -> Option<BindingRef<'_>> {
    find_parent_type(node).and_then(|t| t.binding())
}

/// `ASTResolving.findEnclosingLambdaExpression(node)`.
pub fn find_enclosing_lambda_expression(node: Node<'_>) -> Option<Node<'_>> {
    for a in node.ancestors() {
        if a.is(NodeKind::LambdaExpression) {
            return Some(a);
        }
        if a.kind().is_body_declaration() || a.is(NodeKind::AnonymousClassDeclaration) {
            return None;
        }
    }
    None
}

/// `ASTNodes.findVariableDeclaration(binding, root)`.
pub fn find_variable_declaration<'a>(binding: BindingRef<'_>, root: Node<'a>) -> Option<Node<'a>> {
    if binding.is_field() {
        return None;
    }
    let key = binding.key().to_owned();
    let mut found = None;
    super::walk(root, &mut |n| {
        if found.is_some() {
            return false;
        }
        if matches!(n.kind(), NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment) && n.binding().is_some_and(|b| b.key() == key) {
            found = Some(n);
            return false;
        }
        true
    });
    found
}

/// `SnippetFinder.isLeftHandSideOfAssignment(node)`.
pub fn is_left_hand_side_of_assignment(node: Node<'_>) -> bool {
    let Some(assignment) = node.ancestors().find(|a| a.is(NodeKind::Assignment)) else { return false };
    let Some(lhs) = assignment.child("leftHandSide") else { return false };
    if lhs.id == node.id {
        return true;
    }
    if node.ancestors().any(|a| a.id == lhs.id) {
        return match lhs.kind() {
            NodeKind::SimpleName => true,
            NodeKind::FieldAccess | NodeKind::QualifiedName | NodeKind::SuperFieldAccess => lhs.child("name").is_some_and(|n| n.id == node.id),
            _ => false,
        };
    }
    false
}

/// An `ASTVisitor` with `visit` (`end == false`) and `endVisit` (`end == true`)
/// callbacks (Javadoc not visited).
fn branch_visit<'a>(n: Node<'a>, f: &mut dyn FnMut(Node<'a>, bool) -> bool) {
    if n.is(NodeKind::Javadoc) {
        return;
    }
    if f(n, false) {
        for c in n.children() {
            branch_visit(c, f);
        }
    }
    f(n, true);
}

/// `LocalWriteVisitor`.
fn local_write_visitor(covering: Node<'_>, min: usize, max: usize, values: Vec<BindingId>) -> Vec<BindingId> {
    let original = values.clone();
    let mut ret = values;
    super::walk(covering, &mut |n| {
        if n.is(NodeKind::SimpleName) {
            let binding = n.binding().filter(|b| b.is_variable() && !b.is_field());
            if n.start() > min && n.start() < max {
                if n.parent().is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) {
                    if let Some(b) = binding {
                        if let Some(pos) = ret.iter().position(|&r| r == b.id) {
                            ret.remove(pos);
                        }
                    }
                }
            } else if let Some(b) = binding {
                if original.contains(&b.id) && !ret.contains(&b.id) {
                    ret.push(b.id);
                }
            }
        }
        !n.is(NodeKind::Javadoc)
    });
    ret
}

/// `LocalTypeAnalyzer.perform(declaration, selection)`.
fn local_type_analyzer(declaration: Node<'_>, selection: Selection) -> Status {
    let mut before: Vec<String> = Vec::new();
    let mut selected: Vec<String> = Vec::new();
    let mut before_referenced: Option<&'static str> = None;
    let mut selected_referenced: Option<&'static str> = None;
    super::walk(declaration, &mut |n| {
        if n.is(NodeKind::Javadoc) {
            return false;
        }
        if n.is(NodeKind::SimpleName) && !super::extract_temp::is_declaration(n) {
            if let Some(b) = n.binding().filter(|b| b.is_type()) {
                match selection.visit_mode(n) {
                    selection::SELECTED => {
                        if before_referenced.is_none() && before.iter().any(|k| k == b.key()) {
                            before_referenced = Some(msg("LocalTypeAnalyzer_local_type_from_outside"));
                        }
                    }
                    selection::AFTER => {
                        if selected_referenced.is_none() && selected.iter().any(|k| k == b.key()) {
                            selected_referenced = Some(msg("LocalTypeAnalyzer_local_type_referenced_outside"));
                        }
                    }
                    _ => {}
                }
            }
        }
        if matches!(n.kind(), NodeKind::TypeDeclaration | NodeKind::AnnotationTypeDeclaration | NodeKind::EnumDeclaration) {
            let key = n.binding().map(|b| b.key().to_owned()).unwrap_or_default();
            match selection.visit_mode(n) {
                selection::BEFORE => before.push(key),
                selection::SELECTED => selected.push(key),
                _ => {}
            }
        }
        true
    });
    let mut result = Status::ok();
    if let Some(m) = before_referenced {
        result.add_fatal(m);
    }
    if let Some(m) = selected_referenced {
        result.add_fatal(m);
    }
    result
}

/// The extract method `ExceptionAnalyzer.perform(statements)`.
pub fn exception_analyzer(statements: &[Node<'_>]) -> Vec<BindingId> {
    struct A<'a> {
        stack: Vec<Vec<BindingRef<'a>>>,
    }
    impl<'a> A<'a> {
        fn current(&mut self) -> &mut Vec<BindingRef<'a>> {
            self.stack.last_mut().unwrap()
        }
        fn add(&mut self, exception: BindingRef<'a>) {
            let e = crate::correction::type_mismatch::bindings::normalize_for_declaration_use(exception).unwrap_or(exception);
            if !self.current().iter().any(|c| c.id == e.id) {
                self.current().push(e);
            }
        }
        fn handle(&mut self, binding: Option<BindingRef<'a>>) {
            if let Some(b) = binding {
                for e in b.exception_types() {
                    self.add(e);
                }
            }
        }
        fn accept(&mut self, n: Node<'a>) {
            if self.visit(n) {
                for c in n.children() {
                    self.accept(c);
                }
            }
        }
        fn visit(&mut self, n: Node<'a>) -> bool {
            match n.kind() {
                NodeKind::Javadoc => false,
                NodeKind::ThrowStatement => {
                    if let Some(e) = n.child("expression").and_then(|e| e.type_binding()) {
                        self.add(e);
                    }
                    true
                }
                NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation => {
                    self.handle(n.child("name").and_then(|x| x.binding()).filter(|b| b.is_method()));
                    true
                }
                NodeKind::ClassInstanceCreation => {
                    self.handle(n.method_binding());
                    true
                }
                NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration => {
                    !n.parent().is_some_and(|p| p.is(NodeKind::TypeDeclarationStatement))
                }
                NodeKind::AnonymousClassDeclaration | NodeKind::LambdaExpression => false,
                NodeKind::TryStatement => {
                    self.stack.push(Vec::new());
                    if let Some(b) = n.child("body") {
                        self.accept(b);
                    }
                    for r in n.list("resources") {
                        self.accept(r);
                    }
                    let catches = n.list("catchClauses");
                    for c in &catches {
                        if let Some(t) = c.child("exception").and_then(|e| e.child("type")) {
                            let types = if t.is(NodeKind::UnionType) { t.list("types") } else { vec![t] };
                            for t in types {
                                if let Some(catch_type) = t.binding().or_else(|| t.type_binding()) {
                                    let current = self.current().clone();
                                    for throw in current {
                                        let mut cur = Some(throw);
                                        let mut caught = false;
                                        while let Some(x) = cur {
                                            if x.key() == catch_type.key() {
                                                caught = true;
                                                break;
                                            }
                                            cur = x.superclass();
                                        }
                                        if caught {
                                            self.current().retain(|c| c.id != throw.id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let current = self.stack.pop().unwrap();
                    for t in current {
                        self.add(t);
                    }
                    for c in catches {
                        self.accept(c);
                    }
                    if let Some(f) = n.child("finally") {
                        self.accept(f);
                    }
                    false
                }
                NodeKind::VariableDeclarationExpression => {
                    if n.location_is("resources") && n.parent().is_some_and(|p| p.is(NodeKind::TryStatement)) {
                        if let Some(t) = n.child("type").and_then(|t| t.binding().or_else(|| t.type_binding())) {
                            if let Some(m) = crate::correction::type_mismatch::bindings::find_method_in_hierarchy(t, "close") {
                                for e in m.exception_types() {
                                    self.add(e);
                                }
                            }
                        }
                    }
                    true
                }
                _ => true,
            }
        }
    }
    let mut a = A { stack: vec![Vec::new()] };
    for s in statements {
        a.accept(*s);
    }
    a.stack.pop().unwrap().into_iter().map(|b| b.id).collect()
}

/// `CommentAnalyzer.perform(selection, scanner, start, length)`.
fn comment_analyzer(ast: &Ast, selection: Selection, start: usize, length: usize) -> Status {
    let mut result = Status::ok();
    if length == 0 {
        return result;
    }
    let end = (start + length - 1) as i64;
    // adjustSelection
    let mut new_end = selection.inclusive_end();
    let mut i = selection.exclusive_end();
    while i <= end {
        match ast.char_at(i as usize) {
            Some(c) if c == b'\n' as u16 || c == b'\r' as u16 => new_end += 1,
            _ => break,
        }
        i += 1;
    }
    let sel = Selection { start: selection.start, length: new_end - selection.start + 1 };
    let mut scanner = TokenScanner::new(&ast.source);
    scanner.set_offset(start as i32);
    loop {
        let Ok(tok) = scanner.read_next(false) else { break };
        let token_start = scanner.current_start_offset() as i64;
        if token_start > end || tok == Tok::Eof {
            break;
        }
        if tok.is_comment() {
            let token_end = scanner.current_end_offset() as i64 - 1;
            let pos = sel.start;
            if token_start < pos && pos <= token_end {
                result.add_fatal(manipulation("CommentAnalyzer_starts_inside_comment"));
                break;
            }
            let pos = sel.inclusive_end();
            if token_start <= pos && pos < token_end {
                result.add_fatal(manipulation("CommentAnalyzer_ends_inside_comment"));
                break;
            }
        }
    }
    result
}
