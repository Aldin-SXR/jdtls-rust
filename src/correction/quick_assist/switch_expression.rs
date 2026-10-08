//! Port of `SwitchExpressionsFixCore` (convert a switch statement to a
//! switch expression) and `QuickAssistProcessor.getConvertToSwitchExpressionProposals`.

use std::collections::BTreeMap;

use super::util::{compliance_at_least, leading_comments, trailing_comments};
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::subtree_match;
use crate::semantic_ast::{modifier, BindingRef, Node, NodeId, NodeKind};

const CASE_EXPRESSIONS: &str = "expression";

type Options = BTreeMap<String, String>;

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

struct Operation<'a> {
    switch_statement: Node<'a>,
    case_map: Vec<(Node<'a>, Vec<Node<'a>>)>,
    create_return_statement: bool,
    var_name: Option<String>,
    assignment_binding: Option<BindingRef<'a>>,
    force_old_style: bool,
    use_switch_statement: bool,
}

fn put_case<'a>(map: &mut Vec<(Node<'a>, Vec<Node<'a>>)>, case: Node<'a>, block: Vec<Node<'a>>) {
    match map.iter_mut().find(|(c, _)| *c == case) {
        Some(entry) => entry.1 = block,
        None => map.push((case, block)),
    }
}

fn is_default_case(case: Node<'_>) -> bool {
    case.list(CASE_EXPRESSIONS).is_empty()
}

/// `SwitchStatementsFinder.getOperation`.
fn get_operation<'a>(switch_statement: Node<'a>) -> Option<Operation<'a>> {
    let mut throw_list: Vec<Option<NodeId>> = Vec::new();
    let mut return_list: Vec<Option<NodeId>> = Vec::new();
    let mut default_found = false;
    let mut use_switch_statement = false;
    let mut is_switch_labeled_rule = false;
    let mut current_block: Option<Vec<Node<'a>>> = None;
    let mut current_case: Option<Node<'a>> = None;
    let mut case_map: Vec<(Node<'a>, Vec<Node<'a>>)> = Vec::new();
    for statement in switch_statement.list("statements") {
        match statement.kind() {
            NodeKind::SwitchCase => {
                if statement.flag("switchLabeledRule") {
                    is_switch_labeled_rule = true;
                }
                if is_default_case(statement) {
                    default_found = true;
                }
                if current_block.as_ref().is_some_and(|b| !b.is_empty()) {
                    return None;
                }
                if let (Some(case), Some(block)) = (current_case, current_block.take()) {
                    put_case(&mut case_map, case, block);
                }
                current_block = Some(Vec::new());
                current_case = Some(statement);
            }
            NodeKind::ReturnStatement => {
                statement.child("expression")?;
                return_list.push(current_case.map(|c| c.id));
                let mut block = current_block.take()?;
                block.push(statement);
                put_case(&mut case_map, current_case?, block);
                current_case = None;
            }
            _ if is_invalid_statement(statement) => return None,
            NodeKind::BreakStatement => {
                if current_block.as_ref().is_some_and(|b| b.is_empty()) {
                    return None;
                }
                if let (Some(case), Some(block)) = (current_case, current_block.take()) {
                    put_case(&mut case_map, case, block);
                }
                current_block = None;
                current_case = None;
            }
            NodeKind::ThrowStatement => {
                throw_list.push(current_case.map(|c| c.id));
                let mut block = current_block.take()?;
                block.push(statement);
                put_case(&mut case_map, current_case?, block);
                current_case = None;
            }
            _ => {
                let block = current_block.as_mut()?;
                let mut block_complete = false;
                if statement.is(NodeKind::Block) {
                    let inner = statement.list("statements");
                    for (i, block_statement) in inner.iter().enumerate() {
                        if is_invalid_statement(*block_statement) || block_statement.is(NodeKind::Block) {
                            return None;
                        }
                        if block_statement.is(NodeKind::ThrowStatement) {
                            if i + 1 < inner.len() {
                                return None;
                            }
                            block_complete = true;
                            throw_list.push(current_case.map(|c| c.id));
                        }
                        if block_statement.is(NodeKind::ReturnStatement) {
                            if i + 1 < inner.len() {
                                return None;
                            }
                            block_complete = true;
                            return_list.push(current_case.map(|c| c.id));
                        }
                    }
                }
                block.push(statement);
                if block_complete {
                    let block = current_block.take()?;
                    put_case(&mut case_map, current_case?, block);
                    current_case = None;
                }
            }
        }
    }

    if let Some(case) = current_case {
        if current_block.as_ref().is_some_and(|b| b.is_empty()) {
            return None;
        }
        put_case(&mut case_map, case, current_block.take().unwrap_or_default());
    }
    let mut common_assignment_name: Option<String> = None;
    let mut assignment_binding: Option<BindingRef<'a>> = None;
    let mut create_return_statement = false;

    if !return_list.is_empty() {
        create_return_statement = true;
        let case_count = case_map.iter().filter(|(_, b)| !b.is_empty()).count();
        if return_list.len() + throw_list.len() < case_count {
            create_return_statement = false;
        }
    }
    if !create_return_statement {
        for (entry_case, entry_statements) in &case_map {
            if throw_list.contains(&Some(entry_case.id)) || entry_statements.is_empty() {
                continue;
            }
            let mut last = *entry_statements.last().unwrap();
            if last.is(NodeKind::Block) {
                let inner = last.list("statements");
                match inner.last() {
                    Some(l) => last = *l,
                    None => continue,
                }
            }
            let assignment = last.child("expression").filter(|e| last.is(NodeKind::ExpressionStatement) && e.is(NodeKind::Assignment));
            let Some(assignment) = assignment else {
                assignment_binding = None;
                common_assignment_name = None;
                use_switch_statement = true;
                break;
            };
            if assignment.simple("operator") != Some("=") {
                assignment_binding = None;
                common_assignment_name = None;
                use_switch_statement = true;
                break;
            }
            let lhs = assignment.child("leftHandSide");
            match &common_assignment_name {
                None => match lhs.filter(|e| e.kind().is_name()) {
                    Some(name) => {
                        common_assignment_name = Some(name.source_text().replace(char::is_whitespace, ""));
                        assignment_binding = name.binding();
                    }
                    None => break,
                },
                Some(common) => match lhs.filter(|e| e.kind().is_name()) {
                    Some(name) => {
                        if name.source_text().replace(char::is_whitespace, "") != *common {
                            common_assignment_name = None;
                            assignment_binding = None;
                            use_switch_statement = true;
                            break;
                        }
                    }
                    None => {
                        common_assignment_name = None;
                        assignment_binding = None;
                        use_switch_statement = true;
                        break;
                    }
                },
            }
        }
    }

    let binding = switch_statement.child("expression").and_then(|e| e.type_binding());
    match binding.filter(|b| b.is_enum()) {
        Some(binding) => {
            let enum_count = binding.declared_fields().unwrap_or_default().iter().filter(|f| f.is_enum_constant()).count();
            if enum_count != case_map.len() && !default_found {
                return None;
            }
        }
        None => {
            if !default_found {
                return None;
            }
        }
    }
    let force_old_style = case_map.iter().any(|(c, _)| !trailing_comments(*c).is_empty());
    if (force_old_style || is_switch_labeled_rule) && !create_return_statement && assignment_binding.is_none() {
        return None;
    }
    Some(Operation {
        switch_statement,
        case_map,
        create_return_statement,
        var_name: common_assignment_name,
        assignment_binding,
        force_old_style,
        use_switch_statement,
    })
}

/// `SwitchStatementsFinder`: the first operation in the subtree.
fn first_operation<'a>(node: Node<'a>) -> Option<Operation<'a>> {
    if node.is(NodeKind::SwitchStatement) {
        if let Some(op) = get_operation(node) {
            return Some(op);
        }
    }
    node.children().into_iter().find_map(first_operation)
}

fn comment_text(c: Node<'_>) -> String {
    c.source_text()
}

fn is_line_comment(c: Node<'_>) -> bool {
    c.is(NodeKind::LineComment)
}

/// `getNewStatementFromReturn` / `getNewStatementForCase`: the statement
/// text with leading and trailing comments.
fn statement_with_comments(statement: Node<'_>, expression: Node<'_>) -> String {
    let ast = statement.ast;
    let mut b = String::new();
    for comment in leading_comments(statement) {
        if is_line_comment(comment) {
            let text = comment_text(comment);
            b.push_str(&format!("/*{} */ ", &text[2..]));
        } else {
            b.push_str(&comment_text(comment));
            b.push(' ');
        }
    }
    b.push_str(&ast.substring(expression.start(), expression.end()));
    b.push(';');
    for comment in trailing_comments(statement) {
        b.push(' ');
        b.push_str(&comment_text(comment));
    }
    b
}

/// `getNewYieldStatement` / `getNewYieldStatementFromReturn`.
fn yield_with_comments(statement: Node<'_>, expression: Node<'_>) -> String {
    let ast = statement.ast;
    let mut b = String::new();
    for comment in leading_comments(statement) {
        b.push_str(&comment_text(comment));
        b.push('\n');
    }
    b.push_str("yield ");
    b.push_str(&ast.substring(expression.start(), expression.end()));
    b.push(';');
    for comment in trailing_comments(statement) {
        b.push(' ');
        b.push_str(&comment_text(comment));
    }
    b
}

fn assignment_rhs<'a>(statement: Node<'a>) -> Option<Node<'a>> {
    statement.child("expression")?.child("rightHandSide")
}

fn execute(op: &Operation<'_>, ctx: &Context, options: &Options) -> Option<(ASTRewrite, ImportRewrite)> {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let switch_statement = op.switch_statement;

    let mut statements: Vec<RNode> = Vec::new();
    let new_switch = rw.new_node(if op.use_switch_statement { NodeKind::SwitchStatement } else { NodeKind::SwitchExpression });
    let expression_copy = rw.create_copy_target(switch_statement.child("expression")?.id);
    rw.put_child(new_switch, "expression", expression_copy);

    let mut last_switch_case: Option<RNode> = None;
    let mut default_fall_through = false;
    let new_case = |rw: &mut ASTRewrite| {
        let c = rw.new_node(NodeKind::SwitchCase);
        rw.put_simple(c, "switchLabeledRule", "true");
        c
    };
    for (old_case, old_statements) in &op.case_map {
        let mut old_statements = old_statements.clone();
        if old_statements.is_empty() {
            if op.force_old_style {
                statements.push(rw.create_copy_target(old_case.id));
            } else if old_case.list(CASE_EXPRESSIONS).is_empty() {
                default_fall_through = true;
            } else {
                let last = *last_switch_case.get_or_insert_with(|| {
                    let c = new_case(&mut rw);
                    statements.push(c);
                    c
                });
                for e in old_case.list(CASE_EXPRESSIONS) {
                    let copy = rw.create_copy_target(e.id);
                    rw.list_insert_last(last, CASE_EXPRESSIONS, copy);
                }
            }
            continue;
        }
        let mut need_duplicate_default = false;
        if op.force_old_style {
            statements.push(rw.create_copy_target(old_case.id));
        } else {
            let case = match last_switch_case {
                None => {
                    let c = new_case(&mut rw);
                    statements.push(c);
                    c
                }
                Some(c) => c,
            };
            if last_switch_case.is_some() && old_case.list(CASE_EXPRESSIONS).is_empty() {
                need_duplicate_default = true;
            }
            last_switch_case = None;
            for e in old_case.list(CASE_EXPRESSIONS) {
                let copy = rw.create_copy_target(e.id);
                rw.list_insert_last(case, CASE_EXPRESSIONS, copy);
            }
        }

        loop {
            if old_statements.len() == 1 && old_statements[0].is(NodeKind::Block) {
                old_statements = old_statements[0].list("statements");
            }
            if old_statements.len() == 1 {
                let old_statement = old_statements[0];
                let new_statement: RNode;
                let mut new_statement2: Option<RNode> = None;
                if old_statement.is(NodeKind::ThrowStatement) {
                    new_statement = rw.create_copy_target(old_statement.id);
                    if default_fall_through {
                        new_statement2 = Some(rw.create_copy_target(old_statement.id));
                    }
                } else if old_statement.is(NodeKind::ReturnStatement) && op.create_return_statement {
                    let expression = old_statement.child("expression")?;
                    if op.force_old_style {
                        new_statement = rw.create_string_placeholder(&yield_with_comments(old_statement, expression), NodeKind::YieldStatement);
                    } else {
                        new_statement = rw.create_string_placeholder(&statement_with_comments(old_statement, expression), NodeKind::ExpressionStatement);
                        if default_fall_through {
                            new_statement2 = Some(rw.create_string_placeholder(&statement_with_comments(old_statement, expression), NodeKind::ExpressionStatement));
                        }
                    }
                } else if op.force_old_style {
                    let rhs = assignment_rhs(old_statement)?;
                    new_statement = rw.create_string_placeholder(&yield_with_comments(old_statement, rhs), NodeKind::YieldStatement);
                } else if op.assignment_binding.is_some() {
                    let rhs = assignment_rhs(old_statement)?;
                    new_statement = rw.create_string_placeholder(&statement_with_comments(old_statement, rhs), NodeKind::ExpressionStatement);
                    if default_fall_through {
                        new_statement2 = Some(rw.create_string_placeholder(&statement_with_comments(old_statement, rhs), NodeKind::ExpressionStatement));
                    }
                } else if old_statement.is(NodeKind::ReturnStatement) {
                    let copy = rw.create_copy_target(old_statement.id);
                    new_statement = rw.new_block(vec![copy]);
                    if default_fall_through {
                        let copy2 = rw.create_copy_target(old_statement.id);
                        new_statement2 = Some(rw.new_block(vec![copy2]));
                    }
                } else {
                    new_statement = rw.create_copy_target(old_statement.id);
                    if default_fall_through {
                        new_statement2 = Some(rw.create_copy_target(old_statement.id));
                    }
                }
                statements.push(new_statement);
                if default_fall_through {
                    let c = new_case(&mut rw);
                    statements.push(c);
                    statements.push(new_statement2?);
                    default_fall_through = false;
                }
            } else {
                let mut block_statements = Vec::new();
                let len = old_statements.len();
                for s in &old_statements[..len - 1] {
                    block_statements.push(rw.create_copy_target(s.id));
                }
                let last = old_statements[len - 1];
                let new_statement = if last.is(NodeKind::ThrowStatement) {
                    rw.create_copy_target(last.id)
                } else if (op.assignment_binding.is_some() || op.create_return_statement) && last.is(NodeKind::ReturnStatement) {
                    let expression = last.child("expression")?;
                    rw.create_string_placeholder(&yield_with_comments(last, expression), NodeKind::YieldStatement)
                } else if op.assignment_binding.is_some() {
                    let rhs = assignment_rhs(last)?;
                    rw.create_string_placeholder(&yield_with_comments(last, rhs), NodeKind::YieldStatement)
                } else {
                    rw.create_copy_target(last.id)
                };
                block_statements.push(new_statement);
                let new_block = rw.new_block(block_statements);
                statements.push(new_block);
                if default_fall_through {
                    let c = new_case(&mut rw);
                    let default_expression = rw.new_node(NodeKind::CaseDefaultExpression);
                    rw.list_insert_last(c, CASE_EXPRESSIONS, default_expression);
                    statements.push(c);
                    statements.push(new_block);
                }
            }
            if need_duplicate_default {
                need_duplicate_default = false;
                let c = new_case(&mut rw);
                statements.push(c);
            } else {
                break;
            }
        }
    }
    rw.put_list(new_switch, "statements", statements);

    let new_expression_statement: RNode;
    if op.create_return_statement {
        new_expression_statement = rw.new_return_statement(Some(new_switch));
    } else {
        if let Some(binding) = op.assignment_binding.filter(|b| b.is_variable()) {
            if !binding.is_field() && !binding.is_parameter() && !binding.has(crate::semantic_ast::bflag::SYNTHETIC) {
                if let Some(block) = switch_statement.parent().filter(|p| p.is(NodeKind::Block)) {
                    let block_statements = block.list("statements");
                    let mut declaration: Option<Node<'_>> = None;
                    let mut var_index: isize = -2;
                    for (i, statement) in block_statements.iter().enumerate() {
                        if statement.is(NodeKind::VariableDeclarationStatement) {
                            let fragments = statement.list("fragments");
                            if let [fragment] = fragments.as_slice() {
                                if fragment.child("initializer").is_none() && fragment.binding().is_some_and(|b| b == binding) {
                                    declaration = Some(*statement);
                                    var_index = i as isize;
                                }
                            }
                        } else if statement.is(NodeKind::SwitchStatement) && subtree_match(*statement, switch_statement) {
                            if var_index == i as isize - 1 {
                                let var_name = op.var_name.as_ref()?;
                                let new_fragment = rw.new_variable_declaration_fragment(var_name, Some(new_switch));
                                let new_var = rw.new_node(NodeKind::VariableDeclarationStatement);
                                rw.put_list(new_var, "fragments", vec![new_fragment]);
                                let var_type = binding.var_type()?;
                                let t = imports.add_import_type(var_type, &mut rw, &DefaultContext, TypeLocation::Unknown);
                                rw.put_child(new_var, "type", t);
                                if declaration.is_some_and(|d| d.modifiers() & modifier::FINAL != 0) {
                                    let modifiers = rw.new_modifiers(modifier::FINAL);
                                    rw.put_list(new_var, "modifiers", modifiers);
                                }
                                replace_with_leading_comments(&mut rw, block, declaration?, new_var);
                                rw.list_remove(RNode::Orig(block.id), "statements", RNode::Orig(switch_statement.id));
                                return Some((rw, imports));
                            }
                            break;
                        }
                    }
                }
            }
        }
        if let Some(var_name) = &op.var_name {
            let left = rw.new_name(var_name);
            let assignment = rw.new_assignment(left, "=", new_switch);
            new_expression_statement = rw.new_expression_statement(assignment);
        } else if op.use_switch_statement {
            new_expression_statement = new_switch;
        } else {
            new_expression_statement = rw.new_expression_statement(new_switch);
        }
    }

    match switch_statement.parent().filter(|p| p.is(NodeKind::Block)) {
        Some(block) => replace_with_leading_comments(&mut rw, block, switch_statement, new_expression_statement),
        None => rw.replace(RNode::Orig(switch_statement.id), Some(new_expression_statement)),
    }
    Some((rw, imports))
}

/// `replaceWithLeadingComments`.
fn replace_with_leading_comments(rw: &mut ASTRewrite, block: Node<'_>, old_node: Node<'_>, new_node: RNode) {
    let comments = leading_comments(old_node);
    let parent = RNode::Orig(block.id);
    if let Some(first) = comments.first() {
        let placeholder = |rw: &mut ASTRewrite, c: Node<'_>| {
            let kind = if c.is(NodeKind::BlockComment) || c.is(NodeKind::Javadoc) { NodeKind::BlockComment } else { NodeKind::LineComment };
            rw.create_string_placeholder(&c.source_text(), kind)
        };
        let mut last = placeholder(rw, *first);
        rw.list_replace(parent, "statements", RNode::Orig(old_node.id), last);
        for comment in &comments[1..] {
            let next = placeholder(rw, *comment);
            rw.list_insert_after(parent, "statements", next, last);
            last = next;
        }
        rw.list_insert_after(parent, "statements", new_node, last);
    } else {
        rw.list_replace(parent, "statements", RNode::Orig(old_node.id), new_node);
    }
}

/// `QuickAssistProcessor.getConvertToSwitchExpressionProposals`.
pub fn convert_to_switch_expression(ctx: &Context, options: &Options, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let mut node = covering;
    if covering.is(NodeKind::Block) {
        let statements = covering.list("statements");
        let start = super::util::statement_index(ctx.selection_offset, &statements);
        if start < 0 || start as usize >= statements.len() {
            return;
        }
        node = statements[start as usize];
    } else {
        while node.is(NodeKind::SwitchCase) || node.is(NodeKind::SwitchExpression) {
            match node.parent() {
                Some(p) => node = p,
                None => return,
            }
        }
    }
    if !node.is(NodeKind::SwitchStatement) {
        return;
    }
    if !compliance_at_least(options, "14") {
        return;
    }
    let Some(op) = first_operation(node) else { return };
    let Some((rw, imports)) = execute(&op, ctx, options) else { return };
    let label = messages::fix("SwitchExpressionsFix_convert_to_switch_expression");
    out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::CONVERT_TO_SWITCH_EXPRESSION, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}
