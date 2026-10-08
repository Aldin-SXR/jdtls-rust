//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.ExtractTempRefactoring`
//! (Extract Local Variable).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::find_parent_statement;
use crate::semantic_ast::{modifier, Ast, Node, NodeId, NodeKind};

use super::checkers::{has_side_effect, ChangedValueChecker, UnsafeCheckTester};
use super::checks::{self, RValue};
use super::fragments::{self, Fragment};
use super::naming::{self, VarKind};
use super::scope::{self, ScopeAnalyzer};
use super::{msg, status_code, Status};

/// The rewrite a refactoring produced (`CompilationUnitRewrite`).
pub struct CuRewrite {
    pub rewrite: ASTRewrite,
    pub imports: ImportRewrite,
}

impl CuRewrite {
    pub fn new(ast: &Arc<Ast>, options: &BTreeMap<String, String>) -> Self {
        CuRewrite { rewrite: ASTRewrite::new(ast.clone()), imports: ImportRewrite::create_for_corrections(ast.clone(), options) }
    }
}

/// `ContextSensitiveImportRewriteContext(node, importRewrite)`.
pub fn import_context(ast: &Arc<Ast>, node: Node<'_>, options: &BTreeMap<String, String>) -> ConstructorImportContext {
    ConstructorImportContext {
        ast: ast.clone(),
        declaration: crate::semantic_ast::resolve::find_parent_type(node).map(|n| n.id),
        nullness: crate::rewrite::import_rewrite::nullness::Filter::create(ast, Some(node.id), options),
    }
}

/// `ExtractTempRefactoring`.
pub struct ExtractTemp {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    selection_start: usize,
    selection_length: usize,
    replace_all: bool,
    declare_final: bool,
    temp_name: String,
    selected: Option<Fragment>,
    enclosing_key: Option<String>,
    excluded: Option<Vec<String>>,
    guessed: Option<Vec<String>>,
    start_point: i64,
    end_point: i64,
    seen: Vec<Fragment>,
}

/// How the temp's type is created (`createTempType`), materialized when the
/// declaration is inserted.
enum TempType {
    Node(RNode),
    CopyOf(NodeId),
}

impl ExtractTemp {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, selection_start: usize, selection_length: usize) -> Self {
        ExtractTemp {
            ast,
            options,
            selection_start,
            selection_length,
            replace_all: true,
            declare_final: false,
            temp_name: String::new(),
            selected: None,
            enclosing_key: None,
            excluded: None,
            guessed: None,
            start_point: -1,
            end_point: -1,
            seen: Vec::new(),
        }
    }

    pub fn set_replace_all_occurrences(&mut self, v: bool) {
        self.replace_all = v;
    }

    pub fn set_declare_final(&mut self, v: bool) {
        self.declare_final = v;
    }

    pub fn set_temp_name(&mut self, name: &str) {
        self.temp_name = name.to_owned();
    }

    pub fn temp_name(&self) -> &str {
        &self.temp_name
    }

    // ── Selection ───────────────────────────────────────────────────────────

    /// `getSelectedExpression()`.
    fn selected_expression(&mut self) -> Option<Fragment> {
        if self.selected.is_none() {
            self.selected = selected_expression_at(&self.ast, self.selection_start, self.selection_length);
        }
        self.selected.clone()
    }

    /// `getEnclosingBodyNode()`.
    fn enclosing_body_node(&mut self) -> Option<NodeId> {
        let ast = self.ast.clone();
        let sel = self.selected_expression()?;
        enclosing_body(sel.node(&ast)).map(|n| n.id)
    }

    // ── Initial conditions ──────────────────────────────────────────────────

    /// `checkInitialConditions(pm)`.
    pub fn check_initial_conditions(&mut self) -> Status {
        let result = self.check_selection();
        if !result.has_fatal_error() && self.is_literal_node_selected() {
            self.replace_all = false;
        }
        result
    }

    fn check_selection(&mut self) -> Status {
        let ast = self.ast.clone();
        let root = ast.root();
        let Some(selected) = self.selected_expression() else {
            return checks::check_method_syntax_errors(self.selection_start, self.selection_length, root, msg("ExtractTempRefactoring_select_expression"));
        };
        let node = selected.node(&ast);
        let expression = node;
        if expression.ancestors().any(|a| a.is(NodeKind::ConstructorInvocation) || a.is(NodeKind::SuperConstructorInvocation)) {
            return Status::fatal(msg("ExtractTempRefactoring_explicit_constructor"));
        }
        if self.enclosing_body_node().is_none() || node.ancestors().any(|a| a.kind().is_annotation()) {
            return Status::fatal(msg("ExtractTempRefactoring_expr_in_method_or_initializer"));
        }
        if node.kind().is_name() && node.location_is("type") && node.parent().is_some_and(|p| p.is(NodeKind::ClassInstanceCreation)) {
            return Status::fatal(msg("ExtractTempRefactoring_name_in_new"));
        }
        let mut result = Status::ok();
        result.merge(check_expression(expression));
        if result.has_fatal_error() {
            return result;
        }
        result.merge(check_rvalue(expression, "ExtractTempRefactoring_select_expression", "ExtractTempRefactoring_no_void"));
        if result.has_fatal_error() {
            return result;
        }
        if is_used_in_for_initializer_or_updater(expression) {
            return Status::fatal(msg("ExtractTempRefactoring_for_initializer_updater"));
        }
        if is_referring_to_local_variable_from_for(expression) {
            return Status::fatal(msg("ExtractTempRefactoring_refers_to_for_variable"));
        }
        result
    }

    fn is_literal_node_selected(&mut self) -> bool {
        let ast = self.ast.clone();
        self.selected_expression()
            .is_some_and(|f| matches!(f.node(&ast).kind(), NodeKind::BooleanLiteral | NodeKind::CharacterLiteral | NodeKind::NullLiteral | NodeKind::NumberLiteral))
    }

    // ── Names ───────────────────────────────────────────────────────────────

    /// `getExcludedVariableNames()`.
    fn excluded_variable_names(&mut self) -> Vec<String> {
        if let Some(e) = &self.excluded {
            return e.clone();
        }
        let ast = self.ast.clone();
        let root = ast.root();
        let mut names = Vec::new();
        if let (Some(sel), Some(body)) = (self.selected_expression(), self.enclosing_body_node()) {
            let body = ast.node(body);
            let bindings = ScopeAnalyzer::new(root).declarations_in_scope(sel.start(&ast), scope::VARIABLES | scope::CHECK_VISIBILITY);
            let enclosing = body.parent();
            let body_declaration = body.ancestors().find(|a| a.kind().is_body_declaration());
            for b in bindings {
                let mut modifiers = body_declaration.map(|d| d.modifiers()).unwrap_or(0);
                if body_declaration.is_some_and(|d| d.is(NodeKind::TypeDeclaration)) {
                    modifiers &= !modifier::STATIC;
                }
                if exclude_variable_name(enclosing, modifiers, b) {
                    names.push(b.name().to_owned());
                }
            }
        }
        self.excluded = Some(names.clone());
        names
    }

    /// `guessTempName()`.
    pub fn guess_temp_name(&mut self) -> String {
        let names = self.guess_temp_names();
        names.into_iter().next().unwrap_or_else(|| self.temp_name.clone())
    }

    /// `guessTempNames()`.
    pub fn guess_temp_names(&mut self) -> Vec<String> {
        if let Some(g) = &self.guessed {
            return g.clone();
        }
        let ast = self.ast.clone();
        let mut result = Vec::new();
        if let Some(sel) = self.selected_expression() {
            let expression = sel.node(&ast);
            let binding = expression.type_binding().or_else(|| checks::guess_binding_for_reference(expression));
            let excluded = self.excluded_variable_names();
            result = naming::variable_name_suggestions(VarKind::Local, binding, Some(expression), &excluded, &self.options);
        }
        self.guessed = Some(result.clone());
        result
    }

    // ── Matching fragments ──────────────────────────────────────────────────

    /// `getMatchingFragments()`.
    fn matching_fragments(&mut self) -> Vec<Fragment> {
        let ast = self.ast.clone();
        let Some(sel) = self.selected_expression() else { return Vec::new() };
        if self.replace_all {
            let Some(body) = self.enclosing_body_node() else { return Vec::new() };
            fragments::full_subtree(ast.node(body)).sub_fragments_matching(&sel, &ast)
        } else {
            vec![sel]
        }
    }

    /// `getCertainReplacedExpression(nodesToReplace, index)`.
    fn certain_replaced_expression(&mut self, nodes: &[Fragment], index: usize) -> Fragment {
        let sel = self.selected_expression().expect("selected expression");
        if !self.replace_all || nodes.is_empty() {
            return sel;
        }
        if index >= nodes.len() {
            return nodes[nodes.len() - 1].clone();
        }
        nodes[index].clone()
    }

    /// `retainOnlyReplacableMatches(allMatches)`.
    fn retain_only_replacable_matches(&mut self, all: Vec<Fragment>) -> Vec<Fragment> {
        let ast = self.ast.clone();
        let mut result: Vec<Fragment> = all.into_iter().filter(|f| can_replace(f.node(&ast))).collect();
        result.sort_by_key(|f| f.start(&ast));
        let Some(selected) = self.selected_expression() else { return result };
        let associated = selected.node(&ast);
        let first = self.certain_replaced_expression(&result, 0);
        let flag = if first.start(&ast) < selected.start(&ast) {
            false
        } else {
            associated.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement) || p.is(NodeKind::LambdaExpression))
                && selected.matches(&fragments::full_subtree(associated), &ast)
        };
        let mut upper = result.len();
        if flag {
            let mut parent = associated.parent();
            let mut location = parent.and_then(|p| p.location());
            let mut location_parent = parent.and_then(|p| p.parent());
            while let Some(p) = parent {
                if p.is(NodeKind::Block)
                    || p.kind().is_body_declaration()
                    || (p.is(NodeKind::LambdaExpression) && p.method_binding().is_some())
                    || matches!(p.kind(), NodeKind::EnhancedForStatement | NodeKind::WhileStatement | NodeKind::ForStatement | NodeKind::DoStatement)
                {
                    break;
                }
                location = p.location();
                location_parent = p.parent();
                let in_if = location_parent.is_some_and(|lp| lp.is(NodeKind::IfStatement)) && matches!(location, Some("elseStatement" | "thenStatement"));
                let in_switch = location == Some("statements") && location_parent.is_some_and(|lp| lp.is(NodeKind::SwitchStatement));
                if in_if || in_switch {
                    break;
                }
                parent = p.parent();
            }
            let Some(p) = parent else { return result };
            let mut offset = p.end();
            let in_switch = location == Some("statements") && location_parent.is_some_and(|lp| lp.is(NodeKind::SwitchStatement));
            if in_switch {
                if let Some(ss) = p.parent() {
                    let mut pre_offset: i64 = -1;
                    for n in ss.list("statements") {
                        if n.is(NodeKind::SwitchCase) && n.start() > offset {
                            break;
                        }
                        pre_offset = n.end() as i64;
                    }
                    if pre_offset > 0 {
                        offset = pre_offset as usize;
                    }
                }
            }
            for (i, f) in result.iter().enumerate() {
                if f.start(&ast) > offset {
                    upper = i;
                    break;
                }
            }
        }
        result.truncate(upper);
        result
    }

    fn match_nodes(&mut self) -> Vec<NodeId> {
        let ast = self.ast.clone();
        let all = self.matching_fragments();
        self.retain_only_replacable_matches(all).iter().map(|f| f.node(&ast).id).collect()
    }

    // ── Final conditions / change ───────────────────────────────────────────

    /// `checkFinalConditions` + `createChange`: the status and the rewrite.
    pub fn check_final_conditions(&mut self) -> (Status, Option<CuRewrite>) {
        let mut cu = CuRewrite::new(&self.ast, &self.options);
        cu.rewrite.set_no_comment_source_ranges();
        let mut result = Status::ok();
        self.start_point = -1;
        self.end_point = -1;
        self.seen.clear();
        let replace_all = self.replace_all;
        if self.replace_all {
            let side_effects = self.check_side_effects_in_selected_expression();
            if side_effects.has_info() {
                self.replace_all = false;
                result.merge(side_effects);
            }
        }
        self.process_selected_expression(&mut cu);
        if !cu.rewrite.has_changes() {
            result.add_entry(super::severity::FATAL, msg("ExtractTempRefactoring_side_effects_possible"), status_code::EXPRESSION_MAY_CAUSE_SIDE_EFFECTS);
            self.replace_all = replace_all;
            return (result, None);
        }
        if self.excluded_variable_names().contains(&self.temp_name) {
            result.add_warning(crate::correction::messages::format(msg("ExtractTempRefactoring_another_variable"), &[&self.temp_name]));
        }
        self.replace_all = replace_all;
        (result, Some(cu))
    }

    /// `checkSideEffectsInSelectedExpression()`.
    fn check_side_effects_in_selected_expression(&mut self) -> Status {
        let mut result = Status::ok();
        let all = self.matching_fragments();
        if self.replace_all && self.retain_only_replacable_matches(all).len() > 1 {
            let ast = self.ast.clone();
            let node = self.selected_expression().unwrap().node(&ast);
            if has_side_effect(node, self.enclosing_key.as_deref()) {
                result.add_info(msg("ExtractTempRefactoring_side_effcts_in_selected_expression"));
            }
        }
        result
    }

    /// `processSelectedExpression()`.
    fn process_selected_expression(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let mut cnt = 1;
        let Some(selected) = self.selected_expression() else { return };
        let selected_node = selected.node(&ast);
        if let Some(md) = selected_node.ancestors().find(|a| a.is(NodeKind::MethodDeclaration)) {
            if let Some(b) = md.binding() {
                self.enclosing_key = Some(b.method_declaration().unwrap_or(b).key().to_owned());
            }
        }
        self.selection_start = selected.start(&ast);
        self.selection_length = selected.length(&ast);
        let all = self.matching_fragments();
        let retained = self.retain_only_replacable_matches(all);
        let (tmp_start, tmp_length, tmp_selected) = (self.selection_start, self.selection_length, self.selected.clone());
        let used_names = used_local_names(selected_node);
        let new_name = self.temp_name.clone();
        let next_name = |this: &mut Self, cnt: &mut i32| {
            let old = this.temp_name.clone();
            let base = base_name(&this.temp_name);
            *cnt += 1;
            this.temp_name = format!("{base}{cnt}");
            while used_names.contains(&this.temp_name) || this.temp_name == old {
                *cnt += 1;
                this.temp_name = format!("{base}{cnt}");
            }
        };
        if !self.replace_all || self.should_replace_selected_expression_with_temp_declaration() || retained.is_empty() {
            self.create_temp_declaration(cu);
            self.add_replace_expression_with_temp(cu);
            next_name(self, &mut cnt);
        }
        let mut guard = 0;
        while self.replace_all && retained.len() > self.seen.len() && guard <= retained.len() * 2 {
            guard += 1;
            self.start_point = -1;
            self.end_point = -1;
            let next = retained.iter().find(|f| !self.seen.contains(f)).cloned();
            let Some(next) = next else { break };
            self.selection_start = next.start(&ast);
            self.selection_length = next.length(&ast);
            self.selected = None;
            if self.selected_expression().is_none() {
                break;
            }
            self.create_temp_declaration(cu);
            if self.start_point != -1 && self.end_point != -1 {
                self.add_replace_expression_with_temp(cu);
                next_name(self, &mut cnt);
            }
        }
        self.selection_start = tmp_start;
        self.selection_length = tmp_length;
        self.selected = tmp_selected;
        self.temp_name = new_name;
    }

    /// `shouldReplaceSelectedExpressionWithTempDeclaration()`.
    fn should_replace_selected_expression_with_temp_declaration(&mut self) -> bool {
        let ast = self.ast.clone();
        let Some(selected) = self.selected_expression() else { return false };
        let associated = selected.node(&ast);
        let all = self.matching_fragments();
        let retained = self.retain_only_replacable_matches(all);
        let first = self.certain_replaced_expression(&retained, 0);
        if first.start(&ast) < selected.start(&ast) {
            return false;
        }
        associated.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement) || p.is(NodeKind::LambdaExpression)) && selected.matches(&fragments::full_subtree(associated), &ast)
    }

    /// `createTempDeclaration()`.
    fn create_temp_declaration(&mut self, cu: &mut CuRewrite) {
        if self.should_replace_selected_expression_with_temp_declaration() {
            self.replace_selected_expression_with_temp_declaration(cu);
        } else {
            self.create_and_insert_temp_declaration(cu);
        }
    }

    /// `addReplaceExpressionWithTemp()`.
    fn add_replace_expression_with_temp(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let all = self.matching_fragments();
        let to_replace = self.retain_only_replacable_matches(all);
        if to_replace.is_empty() || self.end_point == -1 {
            return;
        }
        let mut i = self.end_point;
        while i >= self.start_point && i >= 0 {
            let Some(fragment) = to_replace.get(i as usize).cloned() else {
                i -= 1;
                continue;
            };
            i -= 1;
            if self.seen.contains(&fragment) {
                continue;
            }
            self.seen.push(fragment.clone());
            let node = fragment.node(&ast);
            let replacement = if node.location_is("leftHandSide") && node.parent().is_some_and(|p| p.is(NodeKind::Assignment)) {
                let lhs = cu.rewrite.new_simple_name(&self.temp_name);
                let rhs = cu.rewrite.create_copy_target(node.id);
                cu.rewrite.new_assignment(lhs, "=", rhs)
            } else {
                cu.rewrite.new_simple_name(&self.temp_name)
            };
            fragment.replace(&mut cu.rewrite, replacement);
        }
    }

    /// `createTempType()` (the import is added right away, like upstream).
    fn create_temp_type(&mut self, cu: &mut CuRewrite) -> TempType {
        let ast = self.ast.clone();
        let expression = self.selected_expression().unwrap().node(&ast);
        let mut binding = expression.type_binding();
        if expression.is(NodeKind::ClassInstanceCreation) && binding.is_none_or(|b| b.type_arguments().is_empty()) {
            if let Some(t) = expression.child("type") {
                return TempType::CopyOf(t.id);
            }
        }
        if expression.is(NodeKind::CastExpression) {
            if let Some(t) = expression.child("type") {
                return TempType::CopyOf(t.id);
            }
        }
        if binding.is_none() {
            binding = checks::guess_binding_for_reference(expression);
        }
        match binding.and_then(crate::correction::type_mismatch::bindings::normalize_for_declaration_use) {
            Some(b) => {
                let context = import_context(&self.ast, expression, &self.options);
                TempType::Node(cu.imports.add_import_type(b, &mut cu.rewrite, &context, TypeLocation::LocalVariable))
            }
            None => {
                let name = cu.rewrite.new_simple_name("Object");
                TempType::Node(cu.rewrite.new_simple_type(name))
            }
        }
    }

    /// `createTempDeclaration(initializer)`: a `VariableDeclarationExpression`.
    fn new_temp_declaration(&self, cu: &mut CuRewrite, initializer: RNode, typ: &TempType) -> RNode {
        let rw = &mut cu.rewrite;
        let fragment = rw.new_variable_declaration_fragment(&self.temp_name, Some(initializer));
        let vds = rw.new_node(NodeKind::VariableDeclarationExpression);
        if self.declare_final {
            let m = rw.new_modifiers(modifier::FINAL);
            rw.put_list(vds, "modifiers", m);
        }
        let t = match typ {
            TempType::Node(n) => *n,
            TempType::CopyOf(id) => rw.create_copy_target(*id),
        };
        rw.put_child(vds, "type", t);
        rw.put_list(vds, "fragments", vec![fragment]);
        vds
    }

    /// `createAndInsertTempDeclaration()`.
    fn create_and_insert_temp_declaration(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let selected = self.selected_expression().unwrap();
        let typ = self.create_temp_type(cu);
        let insert_at_selection = if !self.replace_all {
            true
        } else {
            let all = self.matching_fragments();
            let replacable = self.retain_only_replacable_matches(all);
            replacable.is_empty() || replacable.len() == 1 && replacable[0].node(&ast) == selected.node(&ast)
        };
        let mut add_final = false;
        let mut node = find_parent_statement(selected.node(&ast));
        if let Some(n) = node.filter(|n| n.is(NodeKind::SwitchCase)) {
            add_final = true;
            node = n.ancestors().find(|a| a.is(NodeKind::SwitchStatement));
            self.declare_final = true;
        }
        if let Some(try_statement) = node.filter(|n| n.is(NodeKind::TryStatement)) {
            let original = selected.node(&ast);
            if original.kind().is_expression() {
                if let Some(b) = original.type_binding() {
                    if crate::correction::type_mismatch::bindings::find_type_in_hierarchy(b, "java.lang.AutoCloseable").is_some() {
                        for resource in try_statement.list("resources") {
                            let offset = resource.start();
                            let mut found = false;
                            let mut parent = original.parent();
                            while let Some(p) = parent {
                                if p == resource {
                                    found = true;
                                    node = Some(resource);
                                    self.replace_all = false;
                                    break;
                                } else if p.start() < offset {
                                    break;
                                }
                                parent = p.parent();
                            }
                            if found {
                                break;
                            }
                        }
                    }
                }
            }
        }
        let all = self.matching_fragments();
        let retained = self.retain_only_replacable_matches(all);
        let select_number = retained.iter().position(|f| f.start(&ast) == self.selection_start).map_or(-1, |i| i as i64);
        let make_declaration = |this: &mut Self, cu: &mut CuRewrite| -> RNode {
            let initializer = selected.create_copy_target(&mut cu.rewrite, true);
            let vds = this.new_temp_declaration(cu, initializer, &typ);
            if add_final {
                let m = cu.rewrite.new_modifiers(modifier::FINAL);
                let existing = cu.rewrite.new_value(vds, "modifiers").list();
                if existing.is_empty() {
                    cu.rewrite.put_list(vds, "modifiers", m);
                }
            }
            vds
        };
        if let Some(n) = node.filter(|n| matches!(n.kind(), NodeKind::SwitchStatement | NodeKind::EnhancedForStatement | NodeKind::TryStatement)) {
            self.start_point = 0;
            self.end_point = retained.len() as i64 - 1;
            let vds = make_declaration(self, cu);
            let exs = cu.rewrite.new_expression_statement(vds);
            insert_at(cu, n, exs);
            return;
        }
        if let Some(n) = node.filter(|n| n.location_is("resources") && n.parent().is_some_and(|p| p.is(NodeKind::TryStatement))) {
            self.start_point = 0;
            self.end_point = retained.len() as i64 - 1;
            let vds = make_declaration(self, cu);
            if let Some(parent) = n.parent() {
                cu.rewrite.list_insert_before(RNode::Orig(parent.id), "resources", vds, RNode::Orig(n.id));
            }
            return;
        }
        let real = self.eval_start_and_end(&retained, select_number, None);
        if real.is_none() && select_number >= 0 {
            self.seen.push(retained[select_number as usize].clone());
        }
        if insert_at_selection {
            if real.is_some() || retained.is_empty() {
                let vds = make_declaration(self, cu);
                let exs = cu.rewrite.new_expression_statement(vds);
                insert_at(cu, selected.node(&ast), exs);
            }
            return;
        }
        if let Some(real) = real {
            let vds = make_declaration(self, cu);
            let exs = cu.rewrite.new_expression_statement(vds);
            insert_at(cu, ast.node(real), exs);
        }
    }

    /// `findDeepestCommonSuperNodePathForReplacedNodes(start, end)`.
    fn deepest_common_path_len(&mut self, start: usize, end: usize) -> usize {
        let ast = self.ast.clone();
        let match_nodes = self.match_nodes();
        let arrays: Vec<Vec<NodeId>> = (start..=end).filter(|&i| i < match_nodes.len()).map(|i| parents(ast.node(match_nodes[i]))).collect();
        if arrays.is_empty() {
            return 0;
        }
        let min = arrays.iter().map(Vec::len).min().unwrap_or(0);
        let mut length = 0;
        for i in 0..min {
            if arrays.iter().all(|a| a[i] == arrays[0][i]) {
                length += 1;
            } else {
                break;
            }
        }
        length
    }

    /// `evalStartAndEnd(retainOnlyReplacableMatches, selectNumber, fixedStartOffset)`.
    fn eval_start_and_end(&mut self, retained: &[Fragment], select_number: i64, fixed_start_offset: Option<usize>) -> Option<NodeId> {
        let ast = self.ast.clone();
        let root = ast.root();
        let mut real = None;
        if select_number < 0 || select_number as usize >= retained.len() {
            return real;
        }
        let mut start = select_number as usize;
        let mut end = select_number as usize;
        let mut expand = 2;
        let selected_node = self.selected_expression().unwrap().node(&ast);
        let mut cvc = ChangedValueChecker::new(&ast, selected_node, self.enclosing_key.as_deref(), true);
        while expand > 0 && start <= end {
            let fragment = retained[start].clone();
            let first_replace = self.certain_replaced_expression(retained, start).node(&ast);
            let first_parents = parents(first_replace);
            let common_len = if start == end { first_parents.len() } else { self.deepest_common_path_len(start, end) };
            if common_len == 0 || common_len > first_parents.len() {
                break;
            }
            let deepest = ast.node(first_parents[common_len - 1]);
            let expression = fragment.node(&ast);
            let end_offset = expression.start() as i64;
            let common = if deepest.is(NodeKind::Block) {
                match first_parents.get(common_len) {
                    Some(id) => ast.node(*id),
                    None => break,
                }
            } else {
                deepest
            };
            let common = convert_to_extract_node(common);
            let start_offset = fixed_start_offset.map_or(common.start() as i64 - 1, |o| o as i64);
            let last_expr_offset = retained[end].start(&ast) as i64;
            let uct = UnsafeCheckTester::new(root, common, expression, start_offset, end_offset);
            let candidates: Vec<Fragment> = retained[start..=end].to_vec();
            cvc.detect_conflict(start_offset, last_expr_offset, retained[end].node(&ast), deepest, &candidates);
            let ok = !uct.has_unsafe_check(&ast) && !cvc.has_conflict();
            if ok {
                self.start_point = start as i64;
                self.end_point = end as i64;
                real = Some(common.id);
                if expand == 2 && (end == retained.len() - 1 || self.seen.contains(&retained[end + 1])) {
                    expand = 1;
                }
                if expand == 1 && (start == 0 || self.seen.contains(&retained[start - 1])) {
                    expand = 0;
                }
                if expand == 1 {
                    start -= 1;
                } else if expand == 2 {
                    end += 1;
                }
            } else if expand == 2 {
                expand = 1;
                if end != select_number as usize {
                    end -= 1;
                }
                if start == 0 {
                    expand = 0;
                } else {
                    start -= 1;
                }
            } else {
                expand = 0;
            }
        }
        real
    }

    /// `replaceSelectedExpressionWithTempDeclaration()`.
    fn replace_selected_expression_with_temp_declaration(&mut self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let selected = self.selected_expression().unwrap();
        let expression = selected.node(&ast);
        let all = self.matching_fragments();
        let retained = self.retain_only_replacable_matches(all);
        self.eval_start_and_end(&retained, 0, Some(expression.end()));
        let typ = self.create_temp_type(cu);
        let initializer = cu.rewrite.create_move_target(expression.id);
        let declaration = self.new_temp_declaration(cu, initializer, &typ);
        let Some(parent) = expression.parent() else { return };
        let is_lambda = parent.is(NodeKind::LambdaExpression);
        let statement = cu.rewrite.new_expression_statement(declaration);
        let replacement = if is_lambda {
            let mut statements = vec![statement];
            let void = checks::is_void(parent.method_binding().and_then(|m| m.return_type()));
            if !void {
                let name = cu.rewrite.new_simple_name(&self.temp_name);
                statements.push(cu.rewrite.new_return_statement(Some(name)));
            }
            cu.rewrite.new_block(statements)
        } else if checks::is_control_statement_body(parent.location(), parent.parent()) {
            cu.rewrite.new_block(vec![statement])
        } else {
            statement
        };
        let replacee = if is_lambda || !checks::has_semicolon(parent) { expression } else { parent };
        cu.rewrite.replace(RNode::Orig(replacee.id), Some(replacement));
    }
}

/// `getSelectedExpression()` for a range (shared by the refactorings).
pub fn selected_expression_at(ast: &Arc<Ast>, start: usize, length: usize) -> Option<Fragment> {
    let root = ast.root();
    let fragment = fragments::for_source_range(root, start, length);
    let mut result = None;
    if let Some(f) = fragment {
        if f.is_expression() && !checks::is_inside_javadoc(f.node(ast)) {
            result = Some(f);
        } else {
            let n = f.node(ast);
            if n.is(NodeKind::ExpressionStatement) {
                result = n.child("expression").map(fragments::full_subtree);
            } else if n.is(NodeKind::Assignment) {
                result = Some(fragments::full_subtree(n));
            }
        }
    }
    if let Some(f) = &result {
        if checks::is_enum_case(f.node(ast).parent()) {
            return None;
        }
    }
    result
}

/// `getEnclosingBodyNode()` (without the enclosing key bookkeeping, which
/// upstream never reaches).
pub fn enclosing_body(n: Node<'_>) -> Option<Node<'_>> {
    let mut location = None;
    let mut node = Some(n);
    while let Some(x) = node {
        if x.kind().is_body_declaration() {
            break;
        }
        location = x.location();
        node = x.parent();
        if node.is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
            break;
        }
    }
    let node = node?;
    let ok = matches!(node.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer) && location == Some("body")
        || node.is(NodeKind::LambdaExpression) && location == Some("body") && node.method_binding().is_some();
    if ok {
        node.child("body")
    } else {
        None
    }
}

/// `checkExpression()` (ExtractTemp).
pub(crate) fn check_expression(e: Node<'_>) -> Option<Status> {
    let parent = e.parent();
    match e.kind() {
        NodeKind::NullLiteral => Some(Status::fatal(msg("ExtractTempRefactoring_null_literals"))),
        NodeKind::ArrayInitializer => Some(Status::fatal(msg("ExtractTempRefactoring_array_initializer"))),
        NodeKind::Assignment => {
            if parent.is_some_and(|p| p.kind().is_expression() && !p.is(NodeKind::ParenthesizedExpression)) {
                Some(Status::fatal(msg("ExtractTempRefactoring_assignment")))
            } else {
                None
            }
        }
        NodeKind::SimpleName => {
            if is_declaration(e) {
                return Some(Status::fatal(msg("ExtractTempRefactoring_names_in_declarations")));
            }
            if parent.is_some_and(|p| (p.is(NodeKind::QualifiedName) || p.is(NodeKind::FieldAccess)) && e.location_is("name")) {
                return Some(Status::fatal(msg("ExtractTempRefactoring_select_expression")));
            }
            None
        }
        NodeKind::VariableDeclarationExpression if parent.is_some_and(|p| p.is(NodeKind::TryStatement)) => {
            Some(Status::fatal(msg("ExtractTempRefactoring_resource_in_try_with_resources")))
        }
        _ => None,
    }
}

/// `SimpleName.isDeclaration()`.
pub fn is_declaration(name: Node<'_>) -> bool {
    let Some(parent) = name.parent() else { return false };
    name.location_is("name")
        && (parent.kind().is_abstract_type_declaration()
            || matches!(
                parent.kind(),
                NodeKind::MethodDeclaration
                    | NodeKind::SingleVariableDeclaration
                    | NodeKind::VariableDeclarationFragment
                    | NodeKind::EnumConstantDeclaration
                    | NodeKind::TypeParameter
                    | NodeKind::AnnotationTypeMemberDeclaration
                    | NodeKind::RecordDeclaration
            ))
}

/// `checkExpressionFragmentIsRValue()`.
pub fn check_rvalue(e: Node<'_>, select_key: &str, void_key: &str) -> Status {
    match checks::check_expression_is_rvalue(e) {
        RValue::NotRValueMisc => Status::with(super::severity::FATAL, msg(select_key), status_code::EXPRESSION_NOT_RVALUE),
        RValue::NotRValueVoid => Status::with(super::severity::FATAL, msg(void_key), status_code::EXPRESSION_NOT_RVALUE_VOID),
        _ => Status::ok(),
    }
}

/// `canReplace(fragment)`.
fn can_replace(node: Node<'_>) -> bool {
    let parent = node.parent();
    if let Some(p) = parent {
        if p.is(NodeKind::VariableDeclarationFragment) && node.location_is("name") {
            return false;
        }
    }
    if is_method_parameter(node) || is_throwable_in_catch_block(node) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::ExpressionStatement) || p.is(NodeKind::LambdaExpression)) {
        return false;
    }
    if is_left_value(node) {
        return false;
    }
    if node.kind().is_expression() && (is_referring_to_local_variable_from_for(node) || is_used_in_for_initializer_or_updater(node)) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::SuperConstructorInvocation) || p.is(NodeKind::ConstructorInvocation)) {
        return false;
    }
    if parent.is_some_and(|p| p.is(NodeKind::SwitchCase)) {
        return true;
    }
    if node.is(NodeKind::SimpleName) && node.location().is_some() {
        return !node.location_is("name");
    }
    true
}

pub(crate) fn is_method_parameter(node: Node<'_>) -> bool {
    node.is(NodeKind::SimpleName)
        && node.parent().is_some_and(|p| p.is(NodeKind::SingleVariableDeclaration) && p.parent().is_some_and(|pp| pp.is(NodeKind::MethodDeclaration)))
}

pub(crate) fn is_throwable_in_catch_block(node: Node<'_>) -> bool {
    node.is(NodeKind::SimpleName)
        && node.parent().is_some_and(|p| p.is(NodeKind::SingleVariableDeclaration) && p.parent().is_some_and(|pp| pp.is(NodeKind::CatchClause)))
}

/// `isLeftValue(node)`.
fn is_left_value(node: Node<'_>) -> bool {
    match node.parent() {
        Some(p) if p.is(NodeKind::PostfixExpression) => true,
        Some(p) if p.is(NodeKind::PrefixExpression) => matches!(p.simple("operator"), Some("++" | "--")),
        _ => false,
    }
}

/// `isReferringToLocalVariableFromFor(expression)`.
pub(crate) fn is_referring_to_local_variable_from_for(expression: Node<'_>) -> bool {
    let mut current = expression;
    let mut parent = current.parent();
    while let Some(p) = parent {
        if p.kind().is_body_declaration() {
            break;
        }
        if p.is(NodeKind::ForStatement) {
            let in_for = current.location_is("initializers") || current.location_is("updaters") || current.location_is("expression");
            if in_for {
                let initializers = p.list("initializers");
                if initializers.len() == 1 && initializers[0].is(NodeKind::VariableDeclarationExpression) {
                    let keys: Vec<String> = initializers[0].list("fragments").iter().filter_map(|f| f.binding()).map(|b| b.key().to_owned()).collect();
                    let mut referring = false;
                    let mut f = |n: Node<'_>| -> bool {
                        if n.is(NodeKind::SimpleName) {
                            if n.binding().is_some_and(|b| keys.iter().any(|k| k == b.key())) {
                                referring = true;
                            }
                            return false;
                        }
                        !n.is(NodeKind::Javadoc)
                    };
                    super::walk(expression, &mut f);
                    if referring {
                        return true;
                    }
                }
            }
        }
        current = p;
        parent = p.parent();
    }
    false
}

/// `isUsedInForInitializerOrUpdater(expression)`.
pub(crate) fn is_used_in_for_initializer_or_updater(expression: Node<'_>) -> bool {
    expression.parent().is_some_and(|p| p.is(NodeKind::ForStatement)) && (expression.location_is("initializers") || expression.location_is("updaters"))
}

/// `excludeVariableName(enclosingNode, modifiers, binding)`.
fn exclude_variable_name(enclosing: Option<Node<'_>>, modifiers: i32, binding: crate::semantic_ast::BindingRef<'_>) -> bool {
    if modifiers & modifier::STATIC != 0 && !binding.is_static() {
        if let Some(declaration) = binding.declaring_node().filter(|n| n.kind().is_variable_declaration()) {
            let method = declaration.ancestors().find(|a| a.is(NodeKind::MethodDeclaration));
            if enclosing.is_some() && method == enclosing {
                return true;
            }
        }
        return false;
    }
    true
}

/// `getUsedLocalNames(selected)`.
fn used_local_names(selected: Node<'_>) -> Vec<String> {
    let Some(block) = selected.ancestors().find(|a| a.is(NodeKind::Block) || a.is(NodeKind::MethodDeclaration)) else { return Vec::new() };
    ScopeAnalyzer::new(selected.root()).used_variable_names(block.start(), block.length()).into_iter().collect()
}

/// `getBaseName(tempName)`: strips trailing digits.
fn base_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut i = chars.len() as i64 - 1;
    while i > 0 && chars[i as usize].is_ascii_digit() {
        i -= 1;
    }
    chars[..(i + 1).max(0) as usize].iter().collect()
}

/// `getParents(node)`: the ancestors, root first.
fn parents(node: Node<'_>) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = node.ancestors().map(|a| a.id).collect();
    v.reverse();
    v
}

fn is_block_statements(n: Node<'_>) -> bool {
    n.location_is("statements") && n.parent().is_some_and(|p| p.is(NodeKind::Block) || p.is(NodeKind::SwitchStatement))
}

fn is_control_body(n: Node<'_>) -> bool {
    let Some(p) = n.parent() else { return false };
    match p.kind() {
        NodeKind::IfStatement => n.location_is("thenStatement") || n.location_is("elseStatement"),
        NodeKind::ForStatement | NodeKind::EnhancedForStatement | NodeKind::DoStatement | NodeKind::WhileStatement => n.location_is("body"),
        _ => false,
    }
}

fn is_lambda_expression_body(n: Node<'_>) -> bool {
    n.location_is("body") && n.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) && n.kind().is_expression()
}

/// `convertToExtractNode(target)`.
fn convert_to_extract_node(mut target: Node<'_>) -> Node<'_> {
    while !is_block_statements(target) {
        if is_control_body(target) || is_lambda_expression_body(target) {
            break;
        }
        match target.parent() {
            Some(p) => target = p,
            None => break,
        }
    }
    target
}

/// `insertAt(target, declaration)`.
fn insert_at(cu: &mut CuRewrite, mut target: Node<'_>, declaration: RNode) {
    let rw = &mut cu.rewrite;
    loop {
        let parent = target.parent();
        let is_resources = target.location_is("resources") && parent.is_some_and(|p| p.is(NodeKind::TryStatement));
        if is_block_statements(target) || is_resources {
            break;
        }
        if is_control_body(target) {
            let moved = rw.create_move_target(target.id);
            let block = rw.new_block(vec![declaration, moved]);
            rw.replace(RNode::Orig(target.id), Some(block));
            return;
        }
        if is_lambda_expression_body(target) {
            let lambda = parent.unwrap();
            let moved = rw.create_move_target(target.id);
            let last = if checks::is_void(lambda.method_binding().and_then(|m| m.return_type())) {
                rw.new_expression_statement(moved)
            } else {
                rw.new_return_statement(Some(moved))
            };
            let block = rw.new_block(vec![declaration, last]);
            rw.replace(RNode::Orig(target.id), Some(block));
            return;
        }
        match parent {
            Some(p) => target = p,
            None => return,
        }
    }
    let Some(parent) = target.parent() else { return };
    let prop = if target.location_is("resources") { "resources" } else { "statements" };
    rw.list_insert_before(RNode::Orig(parent.id), prop, declaration, RNode::Orig(target.id));
}
