//! Ports of `ConvertToStringBufferFixCore`, `ConvertToMessageFormatFixCore`
//! and `ConvertToStringFormatFixCore`.

use std::collections::BTreeMap;

use super::nls;
use super::text_block;
use super::util::compliance_at_least;
use crate::correction::local_corrections::used_names;
use crate::correction::type_mismatch::proposals::import_context;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::features::completion::naming;
use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_body_declaration, find_parent_statement};
use crate::semantic_ast::{Node, NodeKind};

type Options = BTreeMap<String, String>;

fn is_string(n: Node<'_>) -> bool {
    n.type_binding().is_some_and(|t| t.qualified_name() == "java.lang.String")
}

/// The outermost string `+` expression the conversion fixes work on.
fn old_infix_expression(mut node: Node<'_>) -> Option<Node<'_>> {
    let parent_decl = find_parent_body_declaration(node)?;
    if !matches!(parent_decl.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer) {
        return None;
    }
    if node.kind().is_expression() && !node.is(NodeKind::InfixExpression) {
        node = node.parent()?;
    }
    let mut current = Some(node);
    if node.is(NodeKind::VariableDeclarationFragment) {
        current = node.child("initializer");
    } else if node.is(NodeKind::Assignment) {
        current = node.child("rightHandSide");
    }
    let mut old = None;
    while let Some(curr) = current.filter(|c| c.is(NodeKind::InfixExpression)) {
        if is_string(curr) && curr.simple("operator") == Some("+") {
            old = Some(curr);
        } else {
            break;
        }
        current = curr.parent();
    }
    old
}

pub(super) fn collect_infix_plus_operands<'a>(expression: Node<'a>, collector: &mut Vec<Node<'a>>) {
    if expression.is(NodeKind::InfixExpression) && expression.simple("operator") == Some("+") {
        for operand in [expression.child("leftOperand"), expression.child("rightOperand")].into_iter().flatten().chain(expression.list("extendedOperands")) {
            collect_infix_plus_operands(operand, collector);
        }
    } else {
        collector.push(expression);
    }
}

/// `getEnclosingAppendBuffer`.
fn enclosing_append_buffer(infix: Node<'_>) -> Option<Node<'_>> {
    if !infix.location_is("arguments") {
        return None;
    }
    let invocation = infix.parent().filter(|p| p.is(NodeKind::MethodInvocation))?;
    if !invocation.parent().is_some_and(|p| p.kind().is_statement()) {
        return None;
    }
    if invocation.child("name")?.identifier() != "append" {
        return None;
    }
    let expression = invocation.child("expression").filter(|e| e.is(NodeKind::SimpleName))?;
    let binding = expression.binding().filter(|b| b.is_variable())?;
    let type_name = binding.var_type()?.qualified_name();
    (type_name == "java.lang.StringBuilder" || type_name == "java.lang.StringBuffer").then_some(expression)
}

/// `QuickAssistProcessor.getConvertToStringBufferProposal`.
pub fn convert_to_string_buffer(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    let Some(old_infix) = old_infix_expression(node) else { return };
    let existing = enclosing_append_buffer(old_infix);
    let builder_name = "StringBuilder";
    let mechanism = existing.map_or_else(|| builder_name.to_owned(), |e| e.identifier());
    let label = messages::format(messages::correction("QuickAssistProcessor_convert_to_string_buffer_description"), &[&mechanism]);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    if build_string_buffer(ctx, options, &mut rw, old_infix, existing).is_none() {
        return;
    }
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::CONVERT_TO_STRING_BUFFER, rw));
}

fn build_string_buffer(ctx: &Context, options: &Options, rw: &mut ASTRewrite, old_infix: Node<'_>, existing: Option<Node<'_>>) -> Option<()> {
    let builder_name = "StringBuilder";
    let enclosing_statement = find_parent_statement(old_infix)?;
    let insert_after: Option<RNode>;
    let buffer_name: String;
    let list_parent: RNode;
    let list_prop: &'static str;
    let control_body = crate::refactoring::checks::is_control_statement_body(enclosing_statement.location(), enclosing_statement.parent());

    if let Some(existing) = existing {
        if control_body {
            let block = rw.new_block(Vec::new());
            rw.replace(RNode::Orig(enclosing_statement.id), Some(block));
            list_parent = block;
            list_prop = "statements";
            insert_after = None;
        } else {
            list_parent = RNode::Orig(enclosing_statement.parent()?.id);
            list_prop = enclosing_statement.location()?;
            insert_after = Some(RNode::Orig(enclosing_statement.id));
        }
        buffer_name = existing.identifier();
    } else {
        let excluded = used_names(old_infix);
        let names = naming::suggest_names_with_affixes(builder_name, 0, &excluded, options, "local");
        buffer_name = names.first()?.clone();
        let fragment = rw.new_node(NodeKind::VariableDeclarationFragment);
        let name = rw.new_simple_name(&buffer_name);
        rw.put_child(fragment, "name", name);
        let creation = rw.new_node(NodeKind::ClassInstanceCreation);
        let creation_name = rw.new_name(builder_name);
        let creation_type = rw.new_simple_type(creation_name);
        rw.put_child(creation, "type", creation_type);
        rw.put_child(fragment, "initializer", creation);
        let declaration = rw.new_node(NodeKind::VariableDeclarationStatement);
        rw.put_list(declaration, "fragments", vec![fragment]);
        let declared_name = rw.new_name(builder_name);
        let declared_type = rw.new_simple_type(declared_name);
        rw.put_child(declaration, "type", declared_type);
        insert_after = Some(declaration);
        if control_body {
            let block = rw.new_block(Vec::new());
            let moved = rw.create_move_target(enclosing_statement.id);
            rw.list_insert_first(block, "statements", declaration);
            rw.list_insert_last(block, "statements", moved);
            rw.replace(RNode::Orig(enclosing_statement.id), Some(block));
            list_parent = block;
            list_prop = "statements";
        } else {
            list_parent = RNode::Orig(enclosing_statement.parent()?.id);
            list_prop = enclosing_statement.location()?;
            rw.list_insert_before(list_parent, list_prop, declaration, RNode::Orig(enclosing_statement.id));
        }
    }

    let mut operands = Vec::new();
    collect_infix_plus_operands(old_infix, &mut operands);
    let mut last_append = insert_after;
    let mut tags_count = 0;
    let ast = &ctx.ast;
    for operand in operands {
        let mut tagged = false;
        if let Some(line) = nls::scan_current_line(ast, operand.start()) {
            let column = nls::column(ast, operand.start());
            for element in &line.elements {
                if element.offset == column && element.has_tag {
                    tagged = true;
                    tags_count += 1;
                }
            }
        }
        let append_statement = if tagged {
            let call = format!("{buffer_name}.append({}); //$NON-NLS-1$", operand.source_text());
            rw.create_string_placeholder(&call, NodeKind::ExpressionStatement)
        } else {
            let reference = rw.new_simple_name(&buffer_name);
            let copy = rw.create_copy_target(operand.id);
            let invocation = rw.new_method_invocation(Some(reference), "append", vec![copy]);
            rw.new_expression_statement(invocation)
        };
        match last_append {
            None => rw.list_insert_first(list_parent, list_prop, append_statement),
            Some(last) => rw.list_insert_after(list_parent, list_prop, append_statement, last),
        }
        last_append = Some(append_statement);
    }

    if existing.is_some() {
        if insert_after.is_some() {
            rw.remove(RNode::Orig(enclosing_statement.id));
        }
    } else {
        let reference = rw.new_simple_name(&buffer_name);
        let to_string = rw.new_method_invocation(Some(reference), "toString", Vec::new());
        if tags_count > 0 {
            let call = format!("{buffer_name}.toString()");
            replace_and_remove_nls_by_count(ctx, rw, old_infix, &call, tags_count)?;
        } else {
            rw.replace(RNode::Orig(old_infix.id), Some(to_string));
        }
    }
    Some(())
}

/// `ASTNodes.replaceAndRemoveNLSByCount`.
pub(super) fn replace_and_remove_nls_by_count(ctx: &Context, rw: &mut ASTRewrite, visited: Node<'_>, replacement: &str, count: usize) -> Option<()> {
    let statement = visited.ancestors().find(|a| a.kind().is_statement() || a.is(NodeKind::FieldDeclaration))?;
    let source = &ctx.ast.source;
    let original_start = statement.extended_start();
    let original_end = original_start + statement.extended_length();
    let mut original = String::from_utf16_lossy(&source[original_start..original_end]);
    for _ in 0..count {
        original = text_block::remove_last_nls_comment(&original);
    }
    original = text_block::strip_leading_spaces(&original);
    let visited_string = ctx.ast.substring(visited.start(), visited.end());
    let modified = original.replace(&visited_string, replacement);
    let placeholder = rw.create_string_placeholder(&modified, statement.kind());
    rw.replace(RNode::Orig(statement.id), Some(placeholder));
    Some(())
}

/// Whether the string literals of the line either all carry an NLS tag or
/// none does (`foundNoneLiteralOperand` and tag consistency of the
/// `createConvertTo*Fix` methods).
fn eligible_operands(ctx: &Context, operands: &[Node<'_>]) -> bool {
    let mut found_none_literal = false;
    let mut seen_tag = false;
    let mut seen_no_tag = false;
    for operand in operands {
        if !operand.is(NodeKind::StringLiteral) {
            found_none_literal = true;
        } else if let Some(line) = nls::scan_current_line(&ctx.ast, operand.start()) {
            let column = nls::column(&ctx.ast, operand.start());
            if let Some(element) = line.elements.iter().find(|e| e.offset == column) {
                if element.has_tag {
                    if seen_no_tag {
                        return false;
                    }
                    seen_tag = true;
                } else {
                    if seen_tag {
                        return false;
                    }
                    seen_no_tag = true;
                }
            }
        }
    }
    found_none_literal
}

/// `indentOf(cu, exp)`: the leading `Character.isSpaceChar` characters of the line.
fn indent_of(ctx: &Context, exp: Node<'_>) -> String {
    let ast = &ctx.ast;
    let start = ast.line_start(ast.line_of(exp.start()));
    let mut indent = String::new();
    for &c in &ast.source[start..] {
        match char::from_u32(c as u32) {
            Some(ch) if matches!(ch, ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}') => indent.push(ch),
            _ => break,
        }
    }
    indent
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Message,
    String,
}

/// `QuickAssistProcessor.getConvertToMessageFormatProposal`.
pub fn convert_to_message_format(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    convert_to_format(ctx, options, node, Format::Message, out);
}

/// `QuickAssistProcessor.getConvertToStringFormatProposal`.
pub fn convert_to_string_format(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    convert_to_format(ctx, options, node, Format::String, out);
}

fn string_format_conversion(operand: Node<'_>) -> char {
    match operand.type_binding().map(|t| t.name()) {
        Some("byte" | "short" | "int" | "long") => 'd',
        Some("float" | "double") => 'f',
        Some("char") => 'c',
        _ => 's',
    }
}

fn convert_to_format(ctx: &Context, options: &Options, node: Node<'_>, format: Format, out: &mut Vec<Proposal>) {
    let Some(infix) = old_infix_expression(node) else { return };
    let mut operands = Vec::new();
    collect_infix_plus_operands(infix, &mut operands);
    if !eligible_operands(ctx, &operands) {
        return;
    }
    let is_15_or_higher = compliance_at_least(options, "15");
    let ast = &ctx.ast;
    let mut literals: Vec<String> = Vec::new();
    let mut format_arguments: Vec<String> = Vec::new();
    let mut format_string = String::new();
    let mut indent = String::new();
    let mut tags_count = 0usize;
    let mut is_first_literal = true;
    let mut is_first_argument = true;
    let mut first_literal = operands[0];
    let mut last_literal = first_literal;
    let mut first_argument = operands[0];
    let mut last_argument = first_argument;
    let mut total_size = 0usize;
    let mut message_index = 0;
    for operand in &operands {
        if operand.is(NodeKind::StringLiteral) {
            if is_first_literal {
                indent = indent_of(ctx, *operand);
                is_first_literal = false;
                first_literal = *operand;
            }
            last_literal = *operand;
            if let Some(line) = nls::scan_current_line(ast, operand.start()) {
                let column = nls::column(ast, operand.start());
                tags_count += line.elements.iter().filter(|e| e.offset == column && e.has_tag).count();
            }
            let mut value = operand.simple("escapedValue").unwrap_or_default().to_owned();
            total_size += value.encode_utf16().count();
            match format {
                Format::Message => {
                    value = value.replace('\'', "''").replace('{', "'{'").replace('}', "'}'").replace("'{''}'", "'{}'");
                }
                Format::String => value = value.replace('%', "%%"),
            }
            literals.push(value.clone());
            format_string.push_str(&value[1..value.len() - 1]);
        } else {
            if is_first_argument {
                first_argument = *operand;
                is_first_argument = false;
            }
            last_argument = *operand;
            match format {
                Format::Message => {
                    literals.push(format!("\"{{{message_index}}}\""));
                    format_string.push_str(&format!("{{{message_index}}}"));
                    message_index += 1;
                }
                Format::String => {
                    let conversion = string_format_conversion(*operand);
                    literals.push(format!("\"%{conversion}\""));
                    format_string.push_str(&format!("%{conversion}"));
                }
            }
            let start = operand.extended_start();
            format_arguments.push(ast.substring(start, start + operand.extended_length()));
        }
    }

    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), options);
    let mut buffer = String::new();
    match format {
        Format::Message => {
            let context = import_context(ast, infix, options);
            imports.add_import("java.text.MessageFormat", &context);
            buffer.push_str("MessageFormat.format(");
        }
        Format::String => buffer.push_str("String.format("),
    }
    let min_offset = first_literal.start().min(first_argument.start());
    let max_offset = if last_literal.start() > last_argument.start() { last_literal.end() } else { last_argument.end() };
    let less_than_three_lines = ast.line_number(max_offset) - ast.line_number(min_offset) < 2;
    if is_15_or_higher && !less_than_three_lines && total_size > 80 {
        buffer.push_str(&text_block::text_block_from_literals(&literals, &indent));
    } else {
        buffer.push_str(&format!("\"{}\"", format_string.replace('"', "\\\"")));
    }
    for argument in &format_arguments {
        buffer.push_str(", ");
        buffer.push_str(argument);
    }
    buffer.push(')');

    let mut rw = ASTRewrite::new(ast.clone());
    if tags_count > 1 {
        if is_15_or_higher {
            let last_operand = *operands.last().unwrap();
            let mut tags = 0;
            if let Some(line) = nls::scan_current_line(ast, last_operand.start()) {
                tags = line.elements.iter().filter(|e| e.has_tag).count();
            }
            if !last_operand.is(NodeKind::StringLiteral) || tags > 1 {
                let Some(statement) = infix.ancestors().find(|a| a.kind().is_statement()) else { return };
                let extended_start = statement.extended_start();
                let mut extended_length = statement.extended_length();
                let mut complete = ast.substring(extended_start, extended_start + extended_length);
                if tags > 1 {
                    for _ in 0..tags {
                        complete = text_block::remove_last_nls_comment(&complete);
                    }
                    extended_length = complete.encode_utf16().count();
                }
                let prefix_length = infix.start() - extended_start;
                let mut new_buffer = String::from_utf16_lossy(&complete.encode_utf16().take(prefix_length).collect::<Vec<_>>());
                new_buffer.push_str(&buffer);
                let infix_end = infix.end();
                new_buffer.push_str(&ast.substring(infix_end, extended_start + extended_length));
                new_buffer.push_str(" //$NON-NLS-1$");
                let placeholder = rw.create_string_placeholder(&new_buffer, statement.kind());
                rw.replace(RNode::Orig(statement.id), Some(placeholder));
            } else {
                let invocation = rw.create_string_placeholder(&buffer, NodeKind::MethodInvocation);
                rw.replace(RNode::Orig(infix.id), Some(invocation));
            }
        } else if replace_and_remove_nls_by_count(ctx, &mut rw, infix, &buffer, tags_count - 1).is_none() {
            return;
        }
    } else {
        let invocation = rw.create_string_placeholder(&buffer, NodeKind::MethodInvocation);
        rw.replace(RNode::Orig(infix.id), Some(invocation));
    }
    let key = match format {
        Format::Message => "QuickAssistProcessor_convert_to_message_format",
        Format::String => "QuickAssistProcessor_convert_to_string_format",
    };
    out.push(Proposal::new(
        messages::correction(key),
        kind::QUICK_ASSIST,
        relevance::CONVERT_TO_MESSAGE_FORMAT,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
}
