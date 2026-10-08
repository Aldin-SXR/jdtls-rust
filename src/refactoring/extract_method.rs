//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.ExtractMethodRefactoring`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::rewrite::import_rewrite::{DefaultContext, TypeLocation};
use crate::rewrite::scanner::{Tok, TokenScanner};
use crate::rewrite::RNode;
use crate::semantic_ast::resolve::{find_parent_type, unparenthesed_expression};
use crate::semantic_ast::{modifier, Ast, BindingId, BindingRef, Node, NodeId, NodeKind};

use super::extract_method_analyzer::{self as ema, find_variable_declaration, ExtractMethodAnalyzer, ReturnType, TypeRef};
use super::extract_temp::{import_context, CuRewrite};
use super::selection::Selection;
use super::snippet_finder::{self, Match};
use super::{msg, Status};

/// `ParameterInfo` (never renamed by the code actions).
#[derive(Clone, Debug)]
struct ParameterInfo {
    old_binding: BindingId,
    name: String,
    varargs: bool,
}

/// `ExtractMethodRefactoring`.
pub struct ExtractMethod {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    selection_start: i64,
    selection_length: i64,
    method_name: String,
    visibility: i32,
    analyzer: Option<ExtractMethodAnalyzer>,
    parameter_infos: Vec<ParameterInfo>,
    duplicates: Vec<Match>,
    replace_duplicates: bool,
    destinations: Vec<NodeId>,
    destination: Option<NodeId>,
    field_accesses: Vec<NodeId>,
    passed_type_name: String,
    /// The `"name"` linked position group (`fLinkedProposalModel`).
    pub name_positions: Vec<(RNode, i32)>,
}

impl ExtractMethod {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, selection_start: usize, selection_length: usize) -> Self {
        ExtractMethod {
            ast,
            options,
            selection_start: selection_start as i64,
            selection_length: selection_length as i64,
            method_name: "extracted".to_owned(),
            visibility: -1,
            analyzer: None,
            parameter_infos: Vec::new(),
            duplicates: Vec::new(),
            replace_duplicates: false,
            destinations: Vec::new(),
            destination: None,
            field_accesses: Vec::new(),
            passed_type_name: String::new(),
            name_positions: Vec::new(),
        }
    }

    pub fn set_method_name(&mut self, name: &str) {
        self.method_name = name.to_owned();
    }

    fn an(&self) -> &ExtractMethodAnalyzer {
        self.analyzer.as_ref().expect("checkInitialConditions ran")
    }

    /// `checkInitialConditions(pm)`.
    pub fn check_initial_conditions(&mut self) -> Status {
        let mut result = Status::ok();
        if self.selection_start < 0 || self.selection_length == 0 {
            result.add_fatal(msg("ExtractMethodRefactoring_no_set_of_statements"));
            return result;
        }
        let mut analyzer = ExtractMethodAnalyzer::new(self.ast.clone(), Selection { start: self.selection_start, length: self.selection_length });
        analyzer.run();
        self.selection_start = analyzer.selection.start;
        self.selection_length = analyzer.selection.length;
        result.merge(analyzer.check_initial_conditions());
        if analyzer.selection_changed {
            analyzer.run();
            self.selection_start = analyzer.selection.start;
            self.selection_length = analyzer.selection.length;
            result.merge(analyzer.check_initial_conditions());
        }
        self.analyzer = Some(analyzer);
        if result.has_fatal_error() {
            return result;
        }
        if self.visibility == -1 {
            self.visibility = modifier::PRIVATE;
        }
        self.initialize_parameter_infos();
        self.initialize_duplicates();
        self.initialize_destinations();
        result
    }

    /// `checkFinalConditions(pm)`: the name, parameter and override checks
    /// only report non-fatal problems.
    pub fn check_final_conditions(&mut self) -> Status {
        Status::ok()
    }

    fn initialize_parameter_infos(&mut self) {
        let ast = self.ast.clone();
        let an = self.an();
        let root = an.enclosing_body_declaration().unwrap();
        let mut infos = Vec::new();
        let mut vararg = None;
        for &argument in &an.arguments {
            let binding = ast.binding(argument);
            let declaration = find_variable_declaration(binding, root);
            let is_varargs = declaration.is_some_and(|d| d.is(NodeKind::SingleVariableDeclaration) && d.flag("varargs"));
            let info = ParameterInfo { old_binding: argument, name: binding.name().to_owned(), varargs: is_varargs };
            if is_varargs {
                vararg = Some(info);
            } else {
                infos.push(info);
            }
        }
        if let Some(v) = vararg {
            infos.push(v);
        }
        self.parameter_infos = infos;
    }

    fn initialize_duplicates(&mut self) {
        let ast = self.ast.clone();
        let mut start = self.an().enclosing_body_declaration().unwrap();
        while !start.kind().is_abstract_type_declaration() {
            match start.parent() {
                Some(p) => start = p,
                None => break,
            }
        }
        self.duplicates = self.find_valid_duplicates(&ast, start);
        self.replace_duplicates = !self.duplicates.is_empty() && !self.an().is_literal_node_selected();
    }

    fn find_valid_duplicates(&self, ast: &Arc<Ast>, start: Node<'_>) -> Vec<Match> {
        let an = self.an();
        let selected = an.selected_nodes();
        let mut valid = Vec::new();
        for duplicate in snippet_finder::perform(start, &selected) {
            if duplicate.is_invalid_node(ast) {
                continue;
            }
            let first = ast.node(duplicate.nodes[0]);
            let last = ast.node(*duplicate.nodes.last().unwrap());
            let mut analyzer = ExtractMethodAnalyzer::new(ast.clone(), Selection::from_start_length(first.start(), last.end() - first.start()));
            analyzer.run();
            let status = analyzer.check_initial_conditions();
            if status.has_fatal_error() {
                continue;
            }
            let (original, dup) = (an.return_type_binding, analyzer.return_type_binding);
            match (original, dup) {
                (None, None) => valid.push(duplicate),
                (Some(o), Some(d)) => {
                    if o != d {
                        if d == TypeRef::Void {
                            valid.push(duplicate);
                        }
                    } else {
                        match (an.return_value, analyzer.return_value) {
                            (None, None) => valid.push(duplicate),
                            (Some(ov), Some(dv)) => {
                                let oe = an.enclosing_body_declaration().unwrap();
                                let de = analyzer.enclosing_body_declaration().unwrap();
                                let on = find_variable_declaration(ast.binding(ov), oe);
                                let dn = find_variable_declaration(ast.binding(dv), de);
                                if let (Some(on), Some(dn)) = (on, dn) {
                                    let matches = if !an.selection.covers(on) && !analyzer.selection.covers(dn) {
                                        true
                                    } else {
                                        matches_location_in_enclosing_body_decl(oe, de, on, dn)
                                    };
                                    if matches {
                                        valid.push(duplicate);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        valid
    }

    fn initialize_destinations(&mut self) {
        let an = self.an();
        let decl = an.enclosing_body_declaration().unwrap();
        let mut result = Vec::new();
        let mut current = decl.parent().and_then(find_parent_type);
        if let Some(c) = current.filter(|c| ema::is_valid_destination(*c)) {
            result.push(c.id);
        }
        if current.is_some() && matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer | NodeKind::FieldDeclaration) {
            let mut binding = current.and_then(|c| c.binding());
            let mut next = current.and_then(|c| c.parent()).and_then(find_parent_type);
            while let (Some(n), Some(b)) = (next, binding) {
                if !b.is_nested() {
                    break;
                }
                if ema::is_valid_destination(n) {
                    result.push(n.id);
                }
                current = Some(n);
                binding = n.binding();
                next = n.parent().and_then(find_parent_type);
            }
        }
        self.destination = result.first().copied();
        self.destinations = result;
    }

    fn number_of_duplicates(&self) -> usize {
        self.duplicates.iter().filter(|d| !d.is_invalid_node(&self.ast)).count()
    }

    fn force_static(&self) -> bool {
        self.replace_duplicates && self.duplicates.iter().any(|d| !d.is_invalid_node(&self.ast) && d.is_node_in_static_context(&self.ast))
    }

    fn is_destination_interface(&self) -> bool {
        self.destination.map(|d| self.ast.node(d)).is_some_and(|d| d.is(NodeKind::TypeDeclaration) && d.flag("interface"))
    }

    // ── Change creation ─────────────────────────────────────────────────────

    /// `createChange(pm)`.
    pub fn create_change(&mut self) -> CuRewrite {
        let ast = self.ast.clone();
        let mut cu = CuRewrite::new(&ast, &self.options);
        self.name_positions.clear();
        let declaration = self.an().enclosing_body_declaration().unwrap().id;
        let declaration = ast.node(declaration);
        let selected: Vec<NodeId> = self.an().selected_ids();
        self.install_selection_aware_ranges(&mut cu, &selected);
        let method = self.create_new_method(&mut cu, &selected);
        let insert_name = cu.rewrite.new_value(method, "name").node();
        if let Some(n) = insert_name {
            self.name_positions.push((n, 1));
        }
        let parent_type = declaration.parent().and_then(find_parent_type);
        if self.destination.is_some() && self.destination == parent_type.map(|p| p.id) {
            if let (Some(parent), Some(location)) = (declaration.parent(), declaration.location()) {
                cu.rewrite.list_insert_after(RNode::Orig(parent.id), location, method, RNode::Orig(declaration.id));
            }
        } else if let Some(d) = self.destination {
            cu.rewrite.list_insert_last(RNode::Orig(d), "bodyDeclarations", method);
        }
        let modifiers = method_modifiers(&cu, method);
        self.replace_duplicates_in(&mut cu, modifiers);
        self.replace_branches(&mut cu);
        cu
    }

    /// `SelectionAwareSourceRangeComputer`.
    fn install_selection_aware_ranges(&self, cu: &mut CuRewrite, selected: &[NodeId]) {
        if selected.is_empty() {
            return;
        }
        let ast = self.ast.clone();
        let start = self.selection_start.max(0) as usize;
        let end = (self.selection_start + self.selection_length).max(0) as usize;
        let portion: Vec<u16> = ast.source[start.min(ast.source.len())..end.min(ast.source.len())].to_vec();
        let mut tokenizer = TokenScanner::new(&portion);
        let Ok(pos) = tokenizer.next_start_offset(0, false) else { return };
        let first = ast.node(selected[0]);
        let last_index = selected.len() - 1;
        let mut ranges: Vec<(NodeId, (usize, usize))> = Vec::new();
        let first_range = cu.rewrite.extended_range(first.id);
        let last = ast.node(selected[last_index]);
        let last_range = cu.rewrite.extended_range(last.id);
        let new_start = (start + pos as usize).min(first.start());
        let first_new = (new_start, first_range.1 + first_range.0 - new_start);
        ranges.push((first.id, first_new));
        if last_index != 0 {
            ranges.push((last.id, last_range));
        }
        let scanner_start = last.end() as i64 - start as i64;
        tokenizer.set_offset(scanner_start as i32);
        let mut pos = scanner_start;
        let mut token = None;
        while let Ok(t) = tokenizer.read_next(false) {
            token = Some(t);
            pos = tokenizer.current_end_offset() as i64;
        }
        if token == Some(Tok::CommentLine) {
            let mut index = pos - 1;
            while index >= 0 && portion.get(index as usize).is_some_and(|&c| c == b'\n' as u16 || c == b'\r' as u16) {
                pos -= 1;
                index -= 1;
            }
        }
        let new_end = ((start as i64 + pos) as usize).max(last.end());
        let current = ranges.iter().find(|(n, _)| *n == last.id).map(|(_, r)| *r).unwrap();
        let last_new = (current.0, new_end - current.0);
        if let Some(r) = ranges.iter_mut().find(|(n, _)| *n == last.id) {
            r.1 = last_new;
        }
        let mut current_node = last;
        loop {
            let children = current_node.children();
            if let Some(last_child) = children.last() {
                let ext = cu.rewrite.extended_range(last_child.id);
                if ext.0 + ext.1 > new_end {
                    ranges.push((last_child.id, (ext.0, new_end - ext.0)));
                    current_node = *last_child;
                    continue;
                }
            }
            break;
        }
        for (n, (s, l)) in ranges {
            cu.rewrite.set_source_range(n, s, l);
        }
    }

    /// `createNewMethod(selectedNodes, lineDelimiter, substitute)`.
    fn create_new_method(&mut self, cu: &mut CuRewrite, selected: &[NodeId]) -> RNode {
        let method = self.create_new_method_declaration(cu);
        let modifiers = method_modifiers(cu, method);
        let body = self.create_method_body(cu, selected, modifiers);
        cu.rewrite.put_child(method, "body", body);
        method
    }

    /// `createNewMethodDeclaration()`.
    fn create_new_method_declaration(&mut self, cu: &mut CuRewrite) -> RNode {
        let ast = self.ast.clone();
        let rw_method = cu.rewrite.new_node(NodeKind::MethodDeclaration);
        let mut modifiers = self.visibility;
        let enclosing = ast.node(self.an().enclosing_body_declaration().unwrap().id);
        let destination = self.destination;
        let is_interface = self.is_destination_interface();
        if is_interface && (!enclosing.is(NodeKind::MethodDeclaration) || enclosing.parent().map(|p| p.id) != destination || enclosing.modifiers() & modifier::PUBLIC == 0) {
            modifiers = 0;
        }
        let mut should_be_static = false;
        let mut current = Some(enclosing);
        loop {
            let Some(c) = current else { break };
            if c.kind().is_body_declaration() {
                should_be_static = should_be_static || jdt_is_static(c);
            }
            current = c.parent();
            if should_be_static || current.is_none() || current.map(|x| x.id) == destination {
                break;
            }
        }
        if should_be_static || self.an().force_static || self.force_static() {
            modifiers |= modifier::STATIC;
        } else if is_interface {
            modifiers |= modifier::DEFAULT;
        }
        let type_parameters = self.compute_local_type_variables(modifiers);
        let mut tps = Vec::new();
        for tv in type_parameters {
            let tv = ast.binding(tv);
            let p = cu.rewrite.new_node(NodeKind::TypeParameter);
            let name = cu.rewrite.new_simple_name(tv.name());
            cu.rewrite.put_child(p, "name", name);
            let mut bounds = Vec::new();
            for bound in tv.type_bounds() {
                if bound.qualified_name() != "java.lang.Object" {
                    bounds.push(cu.imports.add_import_type(bound, &mut cu.rewrite, &DefaultContext, TypeLocation::Unknown));
                }
            }
            cu.rewrite.put_list(p, "typeBounds", bounds);
            tps.push(p);
        }
        cu.rewrite.put_list(rw_method, "typeParameters", tps);
        let mods = cu.rewrite.new_modifiers(modifiers);
        cu.rewrite.put_list(rw_method, "modifiers", mods);
        let return_type = self.create_return_type(cu);
        cu.rewrite.put_child(rw_method, "returnType2", return_type);
        let name = cu.rewrite.new_simple_name(&self.method_name);
        cu.rewrite.put_child(rw_method, "name", name);
        let context = import_context(&ast, enclosing, &self.options);
        let enclosing_type = enclosing.ancestors().find(|a| a.kind().is_abstract_type_declaration() || a.is(NodeKind::AnonymousClassDeclaration));
        if let Some(et) = enclosing_type.filter(|et| Some(et.id) != destination && et.kind().is_abstract_type_declaration()) {
            self.field_accesses = self.an().find_field_references_for_type(et);
        }
        let mut parameters = Vec::new();
        if !self.field_accesses.is_empty() {
            let et = enclosing_type.unwrap();
            let p = cu.rewrite.new_node(NodeKind::SingleVariableDeclaration);
            if let Some(b) = et.binding() {
                let t = cu.imports.add_import_type(b, &mut cu.rewrite, &DefaultContext, TypeLocation::Unknown);
                cu.rewrite.put_child(p, "type", t);
                cu.imports.remove_import(b.qualified_name());
            }
            self.passed_type_name = format!("passed{}", et.child("name").map(|n| n.identifier()).unwrap_or_default());
            let n = cu.rewrite.new_simple_name(&self.passed_type_name);
            cu.rewrite.put_child(p, "name", n);
            parameters.push(p);
        }
        let root = ast.node(self.an().enclosing_body_declaration().unwrap().id);
        for info in self.parameter_infos.clone() {
            let Some(decl) = find_variable_declaration(ast.binding(info.old_binding), root) else { continue };
            let p = cu.rewrite.new_node(NodeKind::SingleVariableDeclaration);
            let flags = declaration_modifiers(decl).iter().filter(|m| m.is(NodeKind::Modifier)).map(|m| modifier::flag_of(m.simple("keyword").unwrap_or(""))).collect::<Vec<_>>();
            let mut mods = Vec::new();
            for f in flags {
                mods.extend(cu.rewrite.new_modifiers(f));
            }
            cu.rewrite.put_list(p, "modifiers", mods);
            let t = new_type(cu, &self.options, decl, &context, true);
            cu.rewrite.put_child(p, "type", t);
            let n = cu.rewrite.new_simple_name(&info.name);
            cu.rewrite.put_child(p, "name", n);
            if info.varargs {
                cu.rewrite.put_simple(p, "varargs", "true");
            }
            parameters.push(p);
        }
        cu.rewrite.put_list(rw_method, "parameters", parameters);
        let mut exceptions = Vec::new();
        for e in self.an().exceptions(false) {
            exceptions.push(cu.imports.add_import_type(ast.binding(e), &mut cu.rewrite, &context, TypeLocation::Exception));
        }
        cu.rewrite.put_list(rw_method, "thrownExceptionTypes", exceptions);
        rw_method
    }

    fn create_return_type(&self, cu: &mut CuRewrite) -> RNode {
        let ast = self.ast.clone();
        match self.an().return_type.clone().unwrap_or(ReturnType::Void) {
            ReturnType::Void => cu.rewrite.new_primitive_type("void"),
            ReturnType::Import { binding, context } => {
                let ctx = import_context(&ast, ast.node(context), &self.options);
                cu.imports.add_import_type(ast.binding(binding), &mut cu.rewrite, &ctx, TypeLocation::ReturnType)
            }
            ReturnType::LambdaReturn { lambda } => match ast.node(lambda).method_binding().and_then(|m| m.return_type()) {
                Some(rt) => cu.imports.add_import_type(rt, &mut cu.rewrite, &DefaultContext, TypeLocation::Unknown),
                None => {
                    let n = cu.rewrite.new_simple_name("Object");
                    cu.rewrite.new_simple_type(n)
                }
            },
            ReturnType::NonVar { declaration } => {
                let decl = ast.node(declaration);
                let ctx = import_context(&ast, decl, &self.options);
                new_type(cu, &self.options, decl, &ctx, true)
            }
            ReturnType::Copy { node } => cu.rewrite.copy_subtree(RNode::Orig(node)),
        }
    }

    /// `computeLocalTypeVariables(modifier)`.
    fn compute_local_type_variables(&self, modifiers: i32) -> Vec<BindingId> {
        let ast = self.ast.clone();
        let mut result: Vec<BindingId> = self.an().type_variables.clone();
        let mut process = |variable: BindingId, result: &mut Vec<BindingId>| {
            let Some(t) = ast.binding(variable).var_type().filter(|t| t.is_parameterized_type()) else { return };
            for arg in t.type_arguments() {
                let candidate = if arg.is_type_variable() {
                    Some(arg)
                } else if arg.is_wildcard_type() {
                    arg.bound()
                } else {
                    None
                };
                let Some(c) = candidate else { continue };
                if result.contains(&c.id) {
                    continue;
                }
                if let Some(decl) = c.declaring_node() {
                    if let Some(parent) = decl.parent() {
                        if parent.is(NodeKind::MethodDeclaration) || (parent.is(NodeKind::TypeDeclaration) && modifiers & modifier::STATIC != 0) {
                            result.push(c.id);
                        }
                    }
                }
            }
        };
        for info in &self.parameter_infos {
            process(info.old_binding, &mut result);
        }
        for &local in &self.an().method_locals {
            process(local, &mut result);
        }
        result
    }

    /// `createMethodBody(selectedNodes, substitute, modifiers)`.
    fn create_method_body(&mut self, cu: &mut CuRewrite, selected: &[NodeId], modifiers: i32) -> RNode {
        let ast = self.ast.clone();
        let mut statements: Vec<RNode> = Vec::new();
        let (range_start, range_len) = self.an().selected_node_range().unwrap();
        let end_of_selected = range_start + range_len;
        let enclosing = ast.node(self.an().enclosing_body_declaration().unwrap().id);
        for local in self.an().method_locals.clone() {
            statements.push(self.create_declaration(cu, local, None));
            let nodes = find_by_binding(enclosing, ast.binding(local));
            if let Some(first) = nodes.first() {
                if let Some(vdecl) = first.ancestors().find(|a| a.is(NodeKind::VariableDeclarationStatement)) {
                    let needed = nodes[1..].iter().any(|n| n.start() < range_start || n.start() > end_of_selected);
                    if !needed {
                        cu.rewrite.remove(RNode::Orig(vdecl.id));
                    }
                }
            }
        }
        for &fa in &self.field_accesses.clone() {
            let expression = cu.rewrite.new_simple_name(&self.passed_type_name);
            let name = cu.rewrite.create_copy_target(fa);
            let access = cu.rewrite.new_field_access(expression, name);
            cu.rewrite.replace(RNode::Orig(fa), Some(access));
        }
        let extracts_expression = self.an().is_expression_selected();
        let call_nodes = self.create_call_nodes(cu, None, modifiers);
        let replacement = if call_nodes.len() == 1 { call_nodes[0] } else { cu.rewrite.create_group_node(call_nodes) };
        let first = ast.node(selected[0]);
        if extracts_expression {
            let binding = self.an().expression_binding.map(|b| ast.binding(b));
            if binding.is_some_and(|b| !b.is_primitive() || b.name() != "void") {
                let moved = cu.rewrite.create_move_target(unparenthesed_expression(first).id);
                statements.push(cu.rewrite.new_return_statement(Some(moved)));
            } else {
                let moved = cu.rewrite.create_move_target(first.id);
                statements.push(cu.rewrite.new_expression_statement(moved));
            }
            let mut parenthesized = first;
            while let Some(p) = parenthesized.parent().filter(|p| p.is(NodeKind::ParenthesizedExpression)) {
                parenthesized = p;
            }
            cu.rewrite.replace(RNode::Orig(parenthesized.id), Some(replacement));
        } else {
            let last = ast.node(*selected.last().unwrap());
            let is_return_void = last.is(NodeKind::ReturnStatement) && self.an().return_type_binding == Some(TypeRef::Void);
            if selected.len() == 1 {
                if !is_return_void {
                    if first.is(NodeKind::Block) {
                        let block_statements = first.list("statements");
                        if let (Some(f), Some(l)) = (block_statements.first(), block_statements.last()) {
                            let to_move = cu.rewrite.list_create_range_target(RNode::Orig(first.id), "statements", f.id, l.id, None, true);
                            statements.push(to_move);
                        }
                    } else {
                        statements.push(cu.rewrite.create_move_target(first.id));
                    }
                }
                if first.location_is("body") && first.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
                    let kind = cu.rewrite.kind(replacement);
                    if matches!(kind, NodeKind::ExpressionStatement | NodeKind::ReturnStatement) {
                        if let Some(e) = cu.rewrite.new_value(replacement, "expression").node() {
                            cu.rewrite.replace(RNode::Orig(first.id), Some(e));
                        }
                    }
                } else {
                    cu.rewrite.replace(RNode::Orig(first.id), Some(replacement));
                }
            } else {
                if is_return_void {
                    cu.rewrite.remove(RNode::Orig(last.id));
                }
                let index = if is_return_void { selected.len() - 2 } else { selected.len() - 1 };
                if let (Some(parent), Some(location)) = (first.parent(), first.location()) {
                    let to_move = cu.rewrite.list_create_range_target(RNode::Orig(parent.id), location, first.id, selected[index], Some(replacement), true);
                    statements.push(to_move);
                }
            }
            if let Some(rv) = self.an().return_value {
                let name = self.name_of(rv);
                let n = cu.rewrite.new_simple_name(&name);
                statements.push(cu.rewrite.new_return_statement(Some(n)));
            }
        }
        cu.rewrite.new_block(statements)
    }

    /// `getName(binding)`.
    fn name_of(&self, binding: BindingId) -> String {
        self.parameter_infos.iter().find(|p| p.old_binding == binding).map(|p| p.name.clone()).unwrap_or_else(|| self.ast.binding(binding).name().to_owned())
    }

    /// `createCallNodes(duplicate, modifiers)`.
    fn create_call_nodes(&mut self, cu: &mut CuRewrite, duplicate: Option<&Match>, _modifiers: i32) -> Vec<RNode> {
        let ast = self.ast.clone();
        let mut result = Vec::new();
        for local in self.an().caller_locals.clone() {
            result.push(self.create_declaration(cu, local, None));
        }
        let mut arguments = Vec::new();
        if !self.field_accesses.is_empty() {
            arguments.push(cu.rewrite.new_this_expression());
        }
        for p in &self.parameter_infos {
            let name = match duplicate {
                None => p.name.clone(),
                Some(d) => d.mapped_name(p.old_binding).map(|n| ast.node(n).identifier()).unwrap_or_else(|| p.name.clone()),
            };
            arguments.push(cu.rewrite.new_name(&name));
        }
        let invocation = cu.rewrite.new_method_invocation(None, &self.method_name, arguments);
        if let Some(n) = cu.rewrite.new_value(invocation, "name").node() {
            self.name_positions.push((n, 1));
        }
        let mapped_binding = |b: BindingId| -> BindingId {
            match duplicate {
                None => b,
                Some(d) => d.mapped_name(b).and_then(|n| ast.node(n).binding()).map(|x| x.id).unwrap_or(b),
            }
        };
        let call = match self.an().return_kind {
            ema::ACCESS_TO_LOCAL => match self.an().return_local {
                Some(local) => {
                    let mapped = mapped_binding(local);
                    self.create_declaration(cu, mapped, Some(invocation))
                }
                None => {
                    let rv = self.an().return_value.unwrap();
                    let lhs = cu.rewrite.new_name(ast.binding(mapped_binding(rv)).name());
                    cu.rewrite.new_assignment(lhs, "=", invocation)
                }
            },
            ema::RETURN_STATEMENT_VALUE => cu.rewrite.new_return_statement(Some(invocation)),
            _ => invocation,
        };
        let call = if cu.rewrite.kind(call).is_expression() && !self.an().is_expression_selected() { cu.rewrite.new_expression_statement(call) } else { call };
        result.push(call);
        result
    }

    /// `createDeclaration(binding, initializer)`.
    fn create_declaration(&self, cu: &mut CuRewrite, binding: BindingId, initializer: Option<RNode>) -> RNode {
        let ast = self.ast.clone();
        let enclosing = ast.node(self.an().enclosing_body_declaration().unwrap().id);
        let original = find_variable_declaration(ast.binding(binding), enclosing).expect("declaration of a local");
        let name = original.child("name").map(|n| cu.rewrite.copy_subtree(RNode::Orig(n.id))).unwrap();
        let fragment = cu.rewrite.new_node(NodeKind::VariableDeclarationFragment);
        cu.rewrite.put_child(fragment, "name", name);
        if let Some(i) = initializer {
            cu.rewrite.put_child(fragment, "initializer", i);
        }
        let statement = cu.rewrite.new_node(NodeKind::VariableDeclarationStatement);
        let mods: Vec<RNode> = declaration_modifiers(original).into_iter().map(|m| cu.rewrite.copy_subtree(RNode::Orig(m.id))).collect();
        cu.rewrite.put_list(statement, "modifiers", mods);
        let context = import_context(&ast, original, &self.options);
        let t = new_type(cu, &self.options, original, &context, false);
        cu.rewrite.put_child(statement, "type", t);
        cu.rewrite.put_list(statement, "fragments", vec![fragment]);
        statement
    }

    /// `replaceDuplicates(result, modifiers)`.
    fn replace_duplicates_in(&mut self, cu: &mut CuRewrite, modifiers: i32) {
        if self.number_of_duplicates() == 0 || !self.replace_duplicates {
            return;
        }
        let ast = self.ast.clone();
        for duplicate in self.duplicates.clone() {
            if duplicate.is_invalid_node(&ast) {
                continue;
            }
            let reachable = duplicate.enclosing_method(&ast).is_some_and(|m| std::iter::once(m).chain(m.ancestors()).any(|a| Some(a.id) == self.destination));
            if !reachable {
                continue;
            }
            let call_nodes = self.create_call_nodes(cu, Some(&duplicate), modifiers);
            let nodes: Vec<NodeId> = duplicate
                .nodes
                .iter()
                .map(|&n| {
                    let mut n = ast.node(n);
                    while let Some(p) = n.parent().filter(|p| p.is(NodeKind::ParenthesizedExpression)) {
                        n = p;
                    }
                    n.id
                })
                .collect();
            statement_rewrite(cu, &ast, &nodes, &call_nodes);
        }
    }

    /// `replaceBranches(result)`.
    fn replace_branches(&self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let return_value = self.an().return_value;
        for id in self.an().selected_ids() {
            let mut open_loop_labels: Vec<Option<String>> = Vec::new();
            let mut replace: Vec<NodeId> = Vec::new();
            visit_with_end(ast.node(id), &mut |n, end| {
                let is_loop = matches!(n.kind(), NodeKind::ForStatement | NodeKind::WhileStatement | NodeKind::EnhancedForStatement | NodeKind::DoStatement);
                if !end {
                    if is_loop {
                        let label = n.parent().filter(|p| p.is(NodeKind::LabeledStatement)).and_then(|p| p.child("label")).map(|l| l.identifier());
                        open_loop_labels.push(label);
                    }
                } else if is_loop {
                    open_loop_labels.pop();
                } else if n.is(NodeKind::ContinueStatement) {
                    let label = n.child("label").map(|l| l.identifier());
                    if open_loop_labels.is_empty() || label.as_ref().is_some_and(|l| !open_loop_labels.iter().any(|o| o.as_ref() == Some(l))) {
                        replace.push(n.id);
                    }
                }
            });
            for n in replace {
                let expression = return_value.map(|rv| {
                    let name = self.name_of(rv);
                    cu.rewrite.new_simple_name(&name)
                });
                let rs = cu.rewrite.new_return_statement(expression);
                cu.rewrite.replace(RNode::Orig(n), Some(rs));
            }
        }
    }
}

fn method_modifiers(cu: &CuRewrite, method: RNode) -> i32 {
    cu.rewrite
        .new_value(method, "modifiers")
        .list()
        .iter()
        .map(|&m| modifier::flag_of(cu.rewrite.new_value(m, "keyword").simple().unwrap_or("")))
        .fold(0, |a, b| a | b)
}

/// `JdtFlags.isStatic(BodyDeclaration)`.
fn jdt_is_static(decl: Node<'_>) -> bool {
    let parent = decl.parent();
    let parent_is_interface = parent.is_some_and(|p| (p.is(NodeKind::TypeDeclaration) && p.flag("interface")) || p.is(NodeKind::AnnotationTypeDeclaration));
    // isNestedInterfaceOrAnnotation
    if ((decl.is(NodeKind::TypeDeclaration) && decl.flag("interface")) || decl.is(NodeKind::AnnotationTypeDeclaration)) && parent.is_some_and(|p| p.kind().is_abstract_type_declaration() || p.is(NodeKind::AnonymousClassDeclaration)) {
        return true;
    }
    if !matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::AnnotationTypeMemberDeclaration) && parent_is_interface {
        return true;
    }
    if decl.is(NodeKind::EnumConstantDeclaration) {
        return true;
    }
    if decl.is(NodeKind::EnumDeclaration) && parent.is_some_and(|p| p.kind().is_abstract_type_declaration()) {
        return true;
    }
    decl.modifiers() & modifier::STATIC != 0
}

/// `ASTNodes.getModifiers(VariableDeclaration)`.
fn declaration_modifiers(decl: Node<'_>) -> Vec<Node<'_>> {
    if decl.is(NodeKind::SingleVariableDeclaration) {
        return decl.list("modifiers");
    }
    match decl.parent() {
        Some(p) if matches!(p.kind(), NodeKind::VariableDeclarationExpression | NodeKind::VariableDeclarationStatement) => p.list("modifiers"),
        _ => Vec::new(),
    }
}

/// `ASTNodes.getType(declaration)`.
fn declaration_type(decl: Node<'_>) -> Option<Node<'_>> {
    if decl.is(NodeKind::SingleVariableDeclaration) {
        return decl.child("type");
    }
    let parent = decl.parent()?;
    match parent.kind() {
        NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression | NodeKind::FieldDeclaration => parent.child("type"),
        _ => None,
    }
}

fn is_var(t: Node<'_>) -> bool {
    t.is(NodeKind::SimpleType) && t.child("name").is_some_and(|n| n.is(NodeKind::SimpleName) && n.identifier() == "var")
}

/// `ASTNodeFactory.newType` / `newNonVarType(ast, declaration, importRewrite, context)`.
fn new_type(cu: &mut CuRewrite, _options: &BTreeMap<String, String>, decl: Node<'_>, context: &dyn crate::rewrite::import_rewrite::ImportRewriteContext, non_var: bool) -> RNode {
    if decl.is(NodeKind::VariableDeclarationFragment) {
        if let Some(lambda) = decl.parent().filter(|p| p.is(NodeKind::LambdaExpression)) {
            if let Some(m) = lambda.method_binding() {
                let index = lambda.list("parameters").iter().position(|p| p.id == decl.id).unwrap_or(0);
                if let Some(t) = m.parameter_types().get(index) {
                    return cu.imports.add_import_type(*t, &mut cu.rewrite, context, TypeLocation::Unknown);
                }
            }
            let n = cu.rewrite.new_simple_name("Object");
            return cu.rewrite.new_simple_type(n);
        }
    }
    let typ = declaration_type(decl);
    if non_var {
        if let Some(t) = typ.filter(|t| is_var(*t)) {
            if let Some(b) = t.binding().or_else(|| t.type_binding()) {
                return cu.imports.add_import_type(b, &mut cu.rewrite, context, TypeLocation::Unknown);
            }
            return cu.rewrite.copy_subtree(RNode::Orig(t.id));
        }
    }
    let Some(t) = typ else {
        let n = cu.rewrite.new_simple_name("Object");
        return cu.rewrite.new_simple_type(n);
    };
    if t.is(NodeKind::UnionType) {
        if let Some(b) = t.binding().or_else(|| t.type_binding()) {
            return cu.imports.add_import_type(b, &mut cu.rewrite, context, TypeLocation::Unknown);
        }
        if let Some(first) = t.list("types").first() {
            return RNode::Orig(first.id);
        }
    }
    let copy = cu.rewrite.copy_subtree(RNode::Orig(t.id));
    let dims = decl.list("extraDimensions");
    if dims.is_empty() {
        return copy;
    }
    let array = if cu.rewrite.kind(copy) == NodeKind::ArrayType {
        copy
    } else {
        let a = cu.rewrite.new_node(NodeKind::ArrayType);
        cu.rewrite.put_child(a, "elementType", copy);
        cu.rewrite.put_list(a, "dimensions", Vec::new());
        a
    };
    let mut list = cu.rewrite.new_value(array, "dimensions").list();
    for d in dims {
        list.push(cu.rewrite.copy_subtree(RNode::Orig(d.id)));
    }
    cu.rewrite.put_list(array, "dimensions", list);
    array
}

/// `LinkedNodeFinder.findByBinding(root, binding)`.
fn find_by_binding<'a>(root: Node<'a>, binding: BindingRef<'_>) -> Vec<Node<'a>> {
    let key = binding.key().to_owned();
    let mut result = Vec::new();
    super::walk(root, &mut |n| {
        if n.is(NodeKind::SimpleName) && n.binding().is_some_and(|b| b.key() == key) {
            result.push(n);
        }
        true
    });
    result
}

/// `matchesLocationInEnclosingBodyDecl(...)`.
fn matches_location_in_enclosing_body_decl(original_decl: Node<'_>, duplicate_decl: Node<'_>, original_node: Node<'_>, duplicate_node: Node<'_>) -> bool {
    let mut original = original_node;
    let mut duplicate = duplicate_node;
    loop {
        let (op, dp) = (original.parent(), duplicate.parent());
        let (ol, dl) = (original.location(), duplicate.location());
        match (op, dp) {
            (Some(op), Some(dp)) if op.kind() == dp.kind() && ol == dl => {
                let ol = ol.unwrap_or("");
                if matches!(op.prop(ol), Some(crate::semantic_ast::PropValue::List(_))) {
                    let oi = op.list(ol).iter().position(|n| n.id == original.id);
                    let di = dp.list(ol).iter().position(|n| n.id == duplicate.id);
                    if oi != di {
                        return false;
                    }
                }
                original = op;
                duplicate = dp;
            }
            _ => return false,
        }
        let oe = original.id == original_decl.id;
        let de = duplicate.id == duplicate_decl.id;
        if oe != de {
            return false;
        }
        if oe || de {
            return true;
        }
    }
}

/// `StatementRewrite.replace(replacements, description)`.
fn statement_rewrite(cu: &mut CuRewrite, ast: &Ast, to_replace: &[NodeId], replacements: &[RNode]) {
    let first = ast.node(to_replace[0]);
    let (Some(parent), Some(location)) = (first.parent(), first.location()) else { return };
    let rw = &mut cu.rewrite;
    if to_replace.len() == 1 {
        if replacements.len() == 1 {
            rw.replace(RNode::Orig(first.id), Some(replacements[0]));
            return;
        }
        if super::checks::is_control_statement_body(Some(location), Some(parent)) {
            let block = rw.new_block(replacements.to_vec());
            rw.replace(RNode::Orig(first.id), Some(block));
        } else {
            rw.list_replace(RNode::Orig(parent.id), location, RNode::Orig(first.id), replacements[0]);
            for i in 1..replacements.len() {
                rw.list_insert_after(RNode::Orig(parent.id), location, replacements[i], replacements[i - 1]);
            }
        }
        return;
    }
    let p = RNode::Orig(parent.id);
    if to_replace.len() == replacements.len() {
        for i in 0..to_replace.len() {
            rw.list_replace(p, location, RNode::Orig(to_replace[i]), replacements[i]);
        }
    } else if to_replace.len() < replacements.len() {
        for i in 0..to_replace.len() {
            rw.list_replace(p, location, RNode::Orig(to_replace[i]), replacements[i]);
        }
        for i in to_replace.len()..replacements.len() {
            rw.list_insert_after(p, location, replacements[i], replacements[i - 1]);
        }
    } else {
        let delta = to_replace.len() - replacements.len();
        for &r in &to_replace[..delta] {
            rw.list_remove(p, location, RNode::Orig(r));
        }
        for (i, r) in (delta..to_replace.len()).zip(0..) {
            rw.list_replace(p, location, RNode::Orig(to_replace[i]), replacements[r]);
        }
    }
}

/// An `ASTVisitor` with `visit` (`end == false`) and `endVisit` callbacks.
fn visit_with_end<'a>(n: Node<'a>, f: &mut dyn FnMut(Node<'a>, bool)) {
    if n.is(NodeKind::Javadoc) {
        return;
    }
    f(n, false);
    for c in n.children() {
        visit_with_end(c, f);
    }
    f(n, true);
}
