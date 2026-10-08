//! Port of `PatternInstanceofToSwitchFixCore` and
//! `QuickAssistProcessor.getConvertPatternInstanceofIfStmtToSwitchProposals`.

use std::collections::{BTreeMap, HashSet};

use super::switch_expression::{assignment_rhs, replace_with_leading_comments, statement_with_comments, yield_with_comments};
use super::util::compliance_at_least;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{subtree_match, unparenthesed_expression};
use crate::semantic_ast::{modifier, BindingRef, Node, NodeId, NodeKind};

const CASE_EXPRESSIONS: &str = "expression";

type Options = BTreeMap<String, String>;

/// `ASTNodes.as(expression, X.class)`: looks through parentheses.
fn as_kind<'a>(expression: Node<'a>, kind: NodeKind) -> Option<Node<'a>> {
    if expression.is(kind) {
        Some(expression)
    } else if expression.is(NodeKind::ParenthesizedExpression) {
        as_kind(unparenthesed_expression(expression), kind)
    } else {
        None
    }
}

/// `ASTNodes.asList(statement)`.
fn as_list<'a>(statement: Node<'a>) -> Vec<Node<'a>> {
    if statement.is(NodeKind::Block) {
        statement.list("statements")
    } else {
        vec![statement]
    }
}

/// `ASTNodes.isSameVariable(node1, node2)`.
fn is_same_variable(a: Node<'_>, b: Node<'_>) -> bool {
    let a = unparenthesed_expression(a);
    let b = unparenthesed_expression(b);
    if a.is(NodeKind::ThisExpression) {
        return b.is(NodeKind::ThisExpression);
    }
    let variable = |n: Node<'_>| -> Option<String> {
        let binding = match n.kind() {
            NodeKind::FieldAccess | NodeKind::SimpleName | NodeKind::QualifiedName | NodeKind::SuperFieldAccess => n.binding(),
            _ => n.type_binding(),
        };
        binding.map(|b| b.key().to_owned())
    };
    match (a.kind(), b.kind()) {
        (NodeKind::SimpleName, NodeKind::QualifiedName) => return false,
        (NodeKind::QualifiedName, NodeKind::SimpleName) => return false,
        (NodeKind::SimpleName, NodeKind::FieldAccess) => {
            return b.child("expression").and_then(|e| as_kind(e, NodeKind::ThisExpression)).is_some() && variable(a).is_some() && variable(a) == variable(b);
        }
        (NodeKind::FieldAccess, NodeKind::SimpleName) => {
            return a.child("expression").and_then(|e| as_kind(e, NodeKind::ThisExpression)).is_some() && variable(a).is_some() && variable(a) == variable(b);
        }
        (NodeKind::QualifiedName, NodeKind::QualifiedName) => {
            return variable(a).is_some() && variable(a) == variable(b) && matches!((a.child("qualifier"), b.child("qualifier")), (Some(x), Some(y)) if is_same_variable(x, y));
        }
        (NodeKind::FieldAccess, NodeKind::FieldAccess) => {
            return variable(a).is_some() && variable(a) == variable(b) && matches!((a.child("expression"), b.child("expression")), (Some(x), Some(y)) if is_same_variable(x, y));
        }
        _ => {}
    }
    variable(a).is_some() && variable(a) == variable(b)
}

/// `ASTNodes.fallsThrough(statement)`.
fn falls_through(statement: Option<Node<'_>>) -> bool {
    let Some(statement) = statement else { return false };
    let statements = as_list(statement);
    let Some(last) = statements.last() else { return false };
    match last.kind() {
        NodeKind::ReturnStatement | NodeKind::ThrowStatement | NodeKind::BreakStatement | NodeKind::ContinueStatement => true,
        NodeKind::Block => falls_through(Some(*last)),
        NodeKind::IfStatement => falls_through(last.child("thenStatement")) && falls_through(last.child("elseStatement")),
        NodeKind::TryStatement => {
            if !falls_through(last.child("body")) || last.child("finally").is_some_and(|f| falls_through(Some(f))) {
                return false;
            }
            last.list("catchClauses").iter().all(|c| falls_through(c.child("body")))
        }
        _ => false,
    }
}

/// `ASTNodes.getNextSibling(statement)`.
fn next_sibling<'a>(statement: Node<'a>) -> Option<Node<'a>> {
    let mut at_level = statement;
    while let Some(p) = at_level.parent().filter(|p| p.is(NodeKind::LabeledStatement)) {
        at_level = p;
    }
    let parent = at_level.parent()?;
    let statements = if parent.is(NodeKind::Block) || (parent.is(NodeKind::SwitchStatement) && at_level.location_is("statements")) {
        parent.list("statements")
    } else {
        return None;
    };
    let index = statements.iter().position(|s| *s == at_level)?;
    statements.get(index + 1).copied()
}

struct TypeVariable<'a> {
    name: Node<'a>,
    type_pattern: Option<Node<'a>>,
    pattern_name_used: bool,
}

#[derive(Clone)]
struct Section<'a> {
    type_pattern: Option<Node<'a>>,
    is_name_used: bool,
    statements: Vec<Node<'a>>,
    null_section: bool,
}

enum Operation<'a> {
    Statement { if_statements: Vec<Node<'a>>, switch_expression: Node<'a>, cases: Vec<Section<'a>>, remaining: Option<Node<'a>> },
    Expression {
        if_statements: Vec<Node<'a>>,
        switch_expression: Node<'a>,
        cases: Vec<Section<'a>>,
        added_sibling: Option<Node<'a>>,
        create_return_statement: bool,
        var_name: Option<String>,
        assignment_binding: Option<BindingRef<'a>>,
    },
}

struct Finder<'a, 'o> {
    options: &'o Options,
    ifs_processed: HashSet<NodeId>,
    result: Vec<Operation<'a>>,
}

fn is_invalid_statement(s: Node<'_>) -> bool {
    matches!(
        s.kind(),
        NodeKind::ContinueStatement
            | NodeKind::ForStatement
            | NodeKind::IfStatement
            | NodeKind::DoStatement
            | NodeKind::EnhancedForStatement
            | NodeKind::SwitchStatement
            | NodeKind::YieldStatement
            | NodeKind::TryStatement
            | NodeKind::WhileStatement
    )
}

impl<'a, 'o> Finder<'a, 'o> {
    fn extract_expression(&self, expression: Node<'a>) -> Option<TypeVariable<'a>> {
        if let Some(pattern) = as_kind(expression, NodeKind::PatternInstanceofExpression) {
            let type_pattern = pattern.child("pattern").filter(|p| p.is(NodeKind::TypePattern))?;
            return Some(TypeVariable { name: pattern.child("leftOperand")?, type_pattern: Some(type_pattern), pattern_name_used: true });
        }
        if expression.is(NodeKind::InfixExpression) && expression.simple("operator") == Some("==") {
            let left = expression.child("leftOperand")?;
            let right = expression.child("rightOperand")?;
            if left.is(NodeKind::NullLiteral) {
                return Some(TypeVariable { name: right, type_pattern: None, pattern_name_used: true });
            } else if right.is(NodeKind::NullLiteral) {
                return Some(TypeVariable { name: left, type_pattern: None, pattern_name_used: true });
            }
        }
        None
    }

    fn extract_statement(&self, statement: Node<'a>) -> Option<TypeVariable<'a>> {
        if !statement.is(NodeKind::IfStatement) {
            return None;
        }
        let mut result = self.extract_expression(statement.child("expression")?)?;
        if compliance_at_least(self.options, "22") {
            if let Some(pattern) = result.type_pattern {
                let binding = pattern.child("patternVariable2").and_then(|v| v.child("name")).and_then(|n| n.binding());
                if let (Some(binding), Some(then)) = (binding, statement.child("thenStatement")) {
                    let used = std::iter::once(then).chain(then.descendants()).any(|n| n.is(NodeKind::SimpleName) && n.binding().is_some_and(|b| b == binding));
                    result.pattern_name_used = used;
                }
            }
        }
        Some(result)
    }

    /// `SeveralIfVisitor.visit(IfStatement)`; whether children are visited.
    fn visit_if(&mut self, visited: Node<'a>) -> bool {
        if self.ifs_processed.contains(&visited.id) {
            return false;
        }
        let Some(mut variable) = self.extract_statement(visited) else { return true };
        let switch_expression = variable.name;
        let mut if_statements: Vec<Node<'a>> = Vec::new();
        let mut cases: Vec<Section<'a>> = Vec::new();
        let mut remaining: Option<Node<'a>> = None;
        let mut if_statement = Some(visited);
        let mut is_falling_through = true;
        while let Some(head) = if_statement {
            let mut current = head;
            while is_same_variable(switch_expression, variable.name) {
                cases.push(Section {
                    type_pattern: variable.type_pattern,
                    is_name_used: variable.pattern_name_used,
                    statements: current.child("thenStatement").map(as_list).unwrap_or_default(),
                    null_section: variable.type_pattern.is_none(),
                });
                if !falls_through(current.child("thenStatement")) {
                    is_falling_through = false;
                }
                remaining = current.child("elseStatement");
                let Some(rest) = remaining else { break };
                match self.extract_statement(rest) {
                    Some(v) => variable = v,
                    None => break,
                }
                current = rest;
            }
            if_statements.push(head);
            if_statement = next_sibling(head).filter(|s| s.is(NodeKind::IfStatement));
            let next_variable = if_statement.and_then(|s| self.extract_statement(s));
            match next_variable {
                Some(v) if is_falling_through && remaining.is_none() && is_same_variable(switch_expression, v.name) => variable = v,
                _ => break,
            }
        }
        self.maybe_replace(if_statements, switch_expression, cases, remaining)
    }

    fn maybe_replace(&mut self, if_statements: Vec<Node<'a>>, switch_expression: Node<'a>, cases: Vec<Section<'a>>, remaining: Option<Node<'a>>) -> bool {
        if cases.len() > 2 {
            for s in &if_statements {
                self.ifs_processed.insert(s.id);
            }
            match self.get_operation(&if_statements, switch_expression, &cases, remaining) {
                Some(op) => self.result.push(op),
                None => self.result.push(Operation::Statement { if_statements, switch_expression, cases, remaining }),
            }
            return false;
        }
        true
    }

    fn get_operation(&self, if_statements: &[Node<'a>], switch_expression: Node<'a>, cases: &[Section<'a>], mut remaining: Option<Node<'a>>) -> Option<Operation<'a>> {
        let mut throw_count = 0;
        let mut assignments = 0;
        let mut return_count = 0;
        let mut assignment_name: Option<String> = None;
        let mut assignment_binding: Option<BindingRef<'a>> = None;
        let mut added_sibling = None;
        if remaining.is_none() {
            let last = *if_statements.last()?;
            if let Some(sibling) = next_sibling(last).filter(|s| matches!(s.kind(), NodeKind::ReturnStatement | NodeKind::ThrowStatement)) {
                remaining = Some(sibling);
                added_sibling = Some(sibling);
            }
        }
        let remaining = remaining?;
        if as_list(remaining).is_empty() {
            return None;
        }
        let mut extended: Vec<Section<'a>> = cases.to_vec();
        extended.push(Section { type_pattern: None, is_name_used: false, statements: as_list(remaining), null_section: false });
        for section in &extended {
            let mut block_exit = false;
            let count = section.statements.len();
            for (i, statement) in section.statements.iter().enumerate() {
                let has_next = i + 1 < count;
                if is_invalid_statement(*statement) {
                    return None;
                } else if statement.is(NodeKind::ReturnStatement) {
                    if statement.child("expression").is_none() || has_next || block_exit {
                        return None;
                    }
                    block_exit = true;
                    return_count += 1;
                } else if statement.is(NodeKind::ThrowStatement) {
                    if has_next || block_exit {
                        return None;
                    }
                    block_exit = true;
                    throw_count += 1;
                } else if let Some(assignment) = statement.child("expression").filter(|e| statement.is(NodeKind::ExpressionStatement) && e.is(NodeKind::Assignment)) {
                    if block_exit {
                        return None;
                    }
                    if !has_next {
                        if let Some(name) = assignment.child("leftHandSide").filter(|l| l.kind().is_name()) {
                            if let Some(var) = name.binding().filter(|b| b.is_variable()) {
                                if assignment_name.is_none() || assignment_name.as_deref() == Some(var.name()) {
                                    assignment_name = Some(var.name().to_owned());
                                    assignment_binding = Some(var);
                                    assignments += 1;
                                }
                            }
                        }
                    }
                } else {
                    if block_exit {
                        return None;
                    }
                    if statement.is(NodeKind::Block) {
                        let inner = statement.list("statements");
                        for (j, block_statement) in inner.iter().enumerate() {
                            let inner_next = j + 1 < inner.len();
                            if is_invalid_statement(*block_statement) || block_statement.is(NodeKind::Block) {
                                return None;
                            }
                            if block_statement.is(NodeKind::ThrowStatement) {
                                if inner_next || block_exit {
                                    return None;
                                }
                                block_exit = true;
                                throw_count += 1;
                            }
                            if block_statement.is(NodeKind::ReturnStatement) {
                                if inner_next || block_exit {
                                    return None;
                                }
                                block_exit = true;
                                return_count += 1;
                            }
                        }
                    }
                }
            }
        }
        if return_count + throw_count == extended.len() {
            Some(Operation::Expression { if_statements: if_statements.to_vec(), switch_expression, cases: extended, added_sibling, create_return_statement: true, var_name: None, assignment_binding: None })
        } else if assignments == extended.len() {
            Some(Operation::Expression { if_statements: if_statements.to_vec(), switch_expression, cases: extended, added_sibling, create_return_statement: false, var_name: assignment_name, assignment_binding })
        } else {
            None
        }
    }

    /// `ASTVisitor` over the subtree of `node`.
    fn walk(&mut self, node: Node<'a>) {
        if node.is(NodeKind::IfStatement) && !self.visit_if(node) {
            return;
        }
        for child in node.children() {
            self.walk(child);
        }
    }
}

fn type_pattern_with_name_unused(rw: &mut ASTRewrite, imports: &mut ImportRewrite, pattern: Node<'_>) -> Option<RNode> {
    let variable = pattern.child("patternVariable2").filter(|v| v.is(NodeKind::SingleVariableDeclaration))?;
    let old_type = variable.child("type")?;
    let binding = old_type.binding()?;
    let new_pattern = rw.new_node(NodeKind::TypePattern);
    let declaration = rw.new_node(NodeKind::SingleVariableDeclaration);
    let name = rw.new_simple_name("_");
    rw.put_child(declaration, "name", name);
    let new_type = imports.add_import_type(binding, rw, &DefaultContext, TypeLocation::Unknown);
    rw.put_child(declaration, "type", new_type);
    rw.put_child(new_pattern, "patternVariable2", declaration);
    Some(new_pattern)
}

fn add_case_with_statements(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    switch_statements: &mut Vec<RNode>,
    case_value: Option<Node<'_>>,
    is_null_case: bool,
    have_null_case: bool,
    is_name_used: bool,
    inner: &[Node<'_>],
) {
    let need_block = inner.is_empty() || inner.len() > 1 || (inner.len() == 1 && !inner[0].is(NodeKind::ExpressionStatement) && !inner[0].is(NodeKind::ThrowStatement));
    let new_case = rw.new_node(NodeKind::SwitchCase);
    rw.put_simple(new_case, "switchLabeledRule", "true");
    match case_value {
        Some(value) => {
            let mut expression = None;
            if !is_name_used {
                expression = type_pattern_with_name_unused(rw, imports, value);
            }
            let expression = expression.unwrap_or_else(|| rw.create_move_target(value.id));
            rw.list_insert_last(new_case, CASE_EXPRESSIONS, expression);
        }
        None => {
            if !have_null_case {
                let null = rw.new_node(NodeKind::NullLiteral);
                rw.list_insert_last(new_case, CASE_EXPRESSIONS, null);
            }
            if !is_null_case && !have_null_case {
                let default = rw.new_node(NodeKind::CaseDefaultExpression);
                rw.list_insert_last(new_case, CASE_EXPRESSIONS, default);
            }
        }
    }
    switch_statements.push(new_case);
    let mut block_statements = Vec::new();
    for statement in inner {
        block_statements.push(rw.create_copy_target(statement.id));
    }
    if need_block {
        let block = rw.new_block(block_statements);
        switch_statements.push(block);
    } else {
        switch_statements.extend(block_statements);
    }
}

fn apply_statement_operation(rw: &mut ASTRewrite, imports: &mut ImportRewrite, op: &Operation<'_>) -> Option<()> {
    let Operation::Statement { if_statements, switch_expression, cases, remaining } = op else { return None };
    let switch = rw.new_node(NodeKind::SwitchStatement);
    let expression = rw.create_copy_target(switch_expression.id);
    rw.put_child(switch, "expression", expression);
    let mut statements = Vec::new();
    let mut have_null_case = false;
    for case in cases {
        if case.null_section {
            add_case_with_statements(rw, imports, &mut statements, case.type_pattern, true, false, case.is_name_used, &case.statements);
            have_null_case = true;
        } else {
            add_case_with_statements(rw, imports, &mut statements, case.type_pattern, false, have_null_case, case.is_name_used, &case.statements);
        }
    }
    match remaining {
        Some(remaining) => {
            rw.set_source_range(remaining.id, remaining.start(), remaining.length());
            add_case_with_statements(rw, imports, &mut statements, None, false, have_null_case, false, &as_list(*remaining));
        }
        None => add_case_with_statements(rw, imports, &mut statements, None, false, have_null_case, false, &[]),
    }
    rw.put_list(switch, "statements", statements);
    for s in &if_statements[..if_statements.len() - 1] {
        rw.set_source_range(s.id, s.start(), s.length());
        rw.remove(RNode::Orig(s.id));
    }
    let last = *if_statements.last()?;
    rw.set_source_range(last.id, last.start(), last.length());
    rw.replace(RNode::Orig(last.id), Some(switch));
    Some(())
}

fn apply_expression_operation(rw: &mut ASTRewrite, imports: &mut ImportRewrite, op: &Operation<'_>) -> Option<()> {
    let Operation::Expression { if_statements, switch_expression, cases, added_sibling, create_return_statement, var_name, assignment_binding } = op else { return None };
    let new_switch = rw.new_node(NodeKind::SwitchExpression);
    let expression = rw.create_copy_target(switch_expression.id);
    rw.put_child(new_switch, "expression", expression);
    let mut statements: Vec<RNode> = Vec::new();
    let mut have_null_case = false;
    for section in cases {
        let mut old_statements = section.statements.clone();
        let new_case = rw.new_node(NodeKind::SwitchCase);
        rw.put_simple(new_case, "switchLabeledRule", "true");
        statements.push(new_case);
        match section.type_pattern {
            None => {
                if !have_null_case {
                    let null = rw.new_node(NodeKind::NullLiteral);
                    rw.list_insert_last(new_case, CASE_EXPRESSIONS, null);
                }
                if !section.null_section && !have_null_case {
                    let default = rw.new_node(NodeKind::CaseDefaultExpression);
                    rw.list_insert_last(new_case, CASE_EXPRESSIONS, default);
                }
                have_null_case = true;
            }
            Some(pattern) => {
                let mut new_expression = None;
                if !section.is_name_used {
                    new_expression = type_pattern_with_name_unused(rw, imports, pattern);
                }
                let new_expression = new_expression.unwrap_or_else(|| rw.create_copy_target(pattern.id));
                rw.list_insert_last(new_case, CASE_EXPRESSIONS, new_expression);
            }
        }
        if old_statements.len() == 1 && old_statements[0].is(NodeKind::Block) {
            old_statements = old_statements[0].list("statements");
        }
        if old_statements.len() == 1 {
            let old = old_statements[0];
            let new_statement = if old.is(NodeKind::ThrowStatement) {
                rw.create_copy_target(old.id)
            } else if old.is(NodeKind::ReturnStatement) && *create_return_statement {
                rw.create_string_placeholder(&statement_with_comments(old, old.child("expression")?), NodeKind::ExpressionStatement)
            } else {
                rw.create_string_placeholder(&statement_with_comments(old, assignment_rhs(old)?), NodeKind::ExpressionStatement)
            };
            statements.push(new_statement);
        } else {
            let len = old_statements.len();
            let mut block_statements = Vec::new();
            for s in &old_statements[..len - 1] {
                block_statements.push(rw.create_copy_target(s.id));
            }
            let last = old_statements[len - 1];
            let new_statement = if last.is(NodeKind::ThrowStatement) {
                rw.create_copy_target(last.id)
            } else if last.is(NodeKind::ReturnStatement) {
                rw.create_string_placeholder(&yield_with_comments(last, last.child("expression")?), NodeKind::YieldStatement)
            } else {
                rw.create_string_placeholder(&yield_with_comments(last, assignment_rhs(last)?), NodeKind::YieldStatement)
            };
            block_statements.push(new_statement);
            let block = rw.new_block(block_statements);
            statements.push(block);
        }
    }
    rw.put_list(new_switch, "statements", statements);

    let new_expression_statement: RNode;
    let first_if = *if_statements.first()?;
    if *create_return_statement {
        new_expression_statement = rw.new_return_statement(Some(new_switch));
    } else {
        if let Some(binding) = assignment_binding {
            if !binding.is_field() && !binding.is_parameter() && !binding.has(crate::semantic_ast::bflag::SYNTHETIC) {
                if let Some(block) = first_if.parent().filter(|p| p.is(NodeKind::Block)) {
                    let block_statements = block.list("statements");
                    let mut declaration: Option<Node<'_>> = None;
                    let mut var_index: isize = -2;
                    for (i, statement) in block_statements.iter().enumerate() {
                        if statement.is(NodeKind::VariableDeclarationStatement) {
                            let fragments = statement.list("fragments");
                            if let [fragment] = fragments.as_slice() {
                                if fragment.child("initializer").is_none() && fragment.binding().is_some_and(|b| b == *binding) {
                                    declaration = Some(*statement);
                                    var_index = i as isize;
                                }
                            }
                        } else if statement.is(NodeKind::IfStatement) && subtree_match(*statement, first_if) {
                            if var_index == i as isize - 1 {
                                let new_fragment = rw.new_variable_declaration_fragment(var_name.as_ref()?, Some(new_switch));
                                let new_var = rw.new_node(NodeKind::VariableDeclarationStatement);
                                rw.put_list(new_var, "fragments", vec![new_fragment]);
                                let t = imports.add_import_type(binding.var_type()?, rw, &DefaultContext, TypeLocation::Unknown);
                                rw.put_child(new_var, "type", t);
                                if declaration.is_some_and(|d| d.modifiers() & modifier::FINAL != 0) {
                                    let modifiers = rw.new_modifiers(modifier::FINAL);
                                    rw.put_list(new_var, "modifiers", modifiers);
                                }
                                replace_with_leading_comments(rw, block, declaration?, new_var);
                                rw.list_remove(RNode::Orig(block.id), "statements", RNode::Orig(first_if.id));
                                return Some(());
                            }
                            break;
                        }
                    }
                }
            }
        }
        let left = rw.new_name(var_name.as_ref()?);
        let assignment = rw.new_assignment(left, "=", new_switch);
        new_expression_statement = rw.new_expression_statement(assignment);
    }

    match first_if.parent().filter(|p| p.is(NodeKind::Block)) {
        Some(block) => replace_with_leading_comments(rw, block, first_if, new_expression_statement),
        None => rw.replace(RNode::Orig(first_if.id), Some(new_expression_statement)),
    }
    for s in &if_statements[1..] {
        if !s.location_is("elseStatement") {
            rw.remove(RNode::Orig(s.id));
        }
    }
    if let Some(sibling) = added_sibling {
        rw.remove(RNode::Orig(sibling.id));
    }
    Some(())
}

/// `QuickAssistProcessor.getConvertPatternInstanceofIfStmtToSwitchProposals`.
pub fn convert_pattern_instanceof_if_stmt_to_switch(ctx: &Context, options: &Options, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let ancestor = covering.ancestors().find(|a| matches!(a.kind(), NodeKind::IfStatement | NodeKind::MethodDeclaration | NodeKind::TypeDeclaration));
    let Some(mut if_statement) = ancestor.filter(|a| a.is(NodeKind::IfStatement)) else { return };
    while if_statement.location_is("elseStatement") {
        match if_statement.parent() {
            Some(p) => if_statement = p,
            None => return,
        }
    }
    if !compliance_at_least(options, "21") {
        return;
    }
    let mut finder = Finder { options, ifs_processed: HashSet::new(), result: Vec::new() };
    finder.walk(if_statement);
    if finder.result.is_empty() {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    for op in &finder.result {
        let done = match op {
            Operation::Statement { .. } => apply_statement_operation(&mut rw, &mut imports, op),
            Operation::Expression { .. } => apply_expression_operation(&mut rw, &mut imports, op),
        };
        if done.is_none() {
            return;
        }
    }
    let label = messages::fix("PatternInstanceof_convert_if_to_switch");
    out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::CONVERT_PATTERN_INSTANCEOF_TO_SWITCH, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}
