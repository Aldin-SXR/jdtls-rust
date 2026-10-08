//! Port of `StringConcatToTextBlockFixCore` (string concatenation part).

use std::collections::BTreeMap;

use regex::Regex;

use super::nls;
use crate::correction::{kind, messages, relevance, Context, Proposal};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Node, NodeKind};

type Options = BTreeMap<String, String>;

const NEWLINE: &str = "\n";

/// `([ ]*\/\/\$NON-NLS-[0-9]\$) *$` replaced once by the empty string.
pub(super) fn remove_last_nls_comment(s: &str) -> String {
    thread_local! {
        static COMMENT: Regex = Regex::new(r"([ ]*//\$NON-NLS-[0-9]\$) *$").unwrap();
    }
    COMMENT.with(|c| c.replace(s, "").into_owned())
}

/// `leadingspaces_start` and `leadingspaces` of `ASTNodes`.
pub(super) fn strip_leading_spaces(s: &str) -> String {
    let trimmed = s.trim_start_matches([' ', '\t']);
    Regex::new(r"\n[ \t]*").unwrap().replace_all(trimmed, "\n").into_owned()
}

/// `escapeTrailingWhitespace`.
fn escape_trailing_whitespace(unescaped: &str) -> String {
    let mut result = unescaped.to_owned();
    match result.chars().last() {
        Some(' ') => {
            result.pop();
            result.push_str("\\s");
        }
        Some('\t') => {
            result.pop();
            result.push_str("\\t");
        }
        _ => {}
    }
    result
}

/// `StringConcatToTextBlockFixCore.unescapeBlock`.
pub(super) fn unescape_block(escaped: &str) -> Vec<String> {
    let chars: Vec<char> = escaped.chars().collect();
    let starts_with = |at: usize, s: &str| chars.get(at..).is_some_and(|rest| rest.iter().take(s.chars().count()).copied().eq(s.chars()));
    let mut transformed = String::new();
    let mut read = 0usize;
    let mut parts = Vec::new();
    loop {
        let Some(bs) = chars[read.min(chars.len())..].iter().position(|c| *c == '\\').map(|p| p + read) else { break };
        let before: String = chars[read..bs].iter().collect();
        if starts_with(bs, "\\n") || starts_with(bs, "\\u005cn") {
            transformed.push_str(&before);
            parts.push(escape_trailing_whitespace(&transformed) + NEWLINE);
            transformed.clear();
            read = bs + if starts_with(bs, "\\n") { 2 } else { 7 };
        } else if starts_with(bs, "\\\"") || starts_with(bs, "\\u005c\"") {
            transformed.push_str(&before);
            let mut quote_count = 1;
            let mut index = if starts_with(bs, "\\\"") { 2 } else { 7 };
            while starts_with(bs + index, "\\\"") || starts_with(bs + index, "\\u005c\"") {
                quote_count += 1;
                index += if starts_with(bs + index, "\\\"") { 2 } else { 7 };
            }
            let mut i = 0;
            while i < quote_count / 3 {
                transformed.push_str("\\\"\"\"");
                i += 1;
            }
            if i > 0 && quote_count % 3 != 0 {
                transformed.push('\\');
            }
            for _ in 0..quote_count % 3 {
                transformed.push('"');
            }
            read = bs + index;
        } else if starts_with(bs, "\\t") || starts_with(bs, "\\u005ct") {
            transformed.push_str(&before);
            transformed.push('\t');
            read = bs + if starts_with(bs, "\\t") { 2 } else { 7 };
        } else if starts_with(bs, "\\'") || starts_with(bs, "\\u005c'") {
            transformed.push_str(&before);
            transformed.push('\'');
            read = bs + if starts_with(bs, "\\'") { 2 } else { 7 };
        } else {
            transformed.push_str(&before);
            transformed.push('\\');
            if let Some(c) = chars.get(bs + 1) {
                transformed.push(*c);
            }
            read = bs + 2;
        }
    }
    if read < chars.len() {
        transformed.extend(&chars[read..]);
    }
    if !transformed.is_empty() {
        parts.push(transformed);
    }
    parts
}

/// The text block assembled from the parts (shared tail of the fixes).
fn assemble(parts: &[String], indent: &str, escape_trailing_blank: bool) -> String {
    let mut buf: Vec<char> = "\"\"\"\n".chars().collect();
    let mut new_line = false;
    let mut all_whitespace_start = true;
    let mut all_empty = true;
    for part in parts {
        if buf.len() > 4 && !new_line {
            buf.push('\\');
            buf.extend(NEWLINE.chars());
        }
        new_line = part.ends_with(NEWLINE);
        all_whitespace_start = all_whitespace_start && (part.is_empty() || part.chars().next().is_some_and(char::is_whitespace));
        all_empty = all_empty && part.is_empty();
        buf.extend(indent.chars());
        buf.extend(part.chars());
    }
    if new_line || all_empty {
        buf.extend(indent.chars());
    } else if all_whitespace_start {
        buf.push('\\');
        buf.extend(NEWLINE.chars());
        buf.extend(indent.chars());
    } else {
        let mut read = buf.len() as isize - 1;
        let mut count = 0;
        while read >= 0 && buf[read as usize] == '"' && count <= 3 {
            read -= 1;
            count += 1;
        }
        if read >= 0 && buf[read as usize] == '\\' {
            count -= 1;
        }
        for _ in 0..count {
            buf.pop();
        }
        for _ in 0..count {
            buf.extend("\\\"".chars());
        }
        if escape_trailing_blank {
            match buf.last() {
                Some(' ') => {
                    buf.pop();
                    buf.extend("\\s".chars());
                }
                Some('\t') => {
                    buf.pop();
                    buf.extend("\\t".chars());
                }
                _ => {}
            }
        }
    }
    buf.extend("\"\"\"".chars());
    buf.into_iter().collect()
}

/// The text block of `literals` (escaped values with quotes), as the
/// `MessageFormat`/`String.format` conversions build it.
pub(super) fn text_block_from_literals(literals: &[String], indent: &str) -> String {
    let mut parts = Vec::new();
    for literal in literals {
        let inner: String = literal.chars().skip(1).take(literal.chars().count().saturating_sub(2)).collect();
        parts.extend(unescape_block(&inner));
    }
    assemble(&parts, indent, false)
}

fn has_nls(comments: &[Node<'_>]) -> bool {
    comments.iter().any(|c| c.is(NodeKind::LineComment) && c.source_text().contains("$NON-NLS"))
}

fn comments_for_region<'a>(ast: &'a crate::semantic_ast::Ast, start: usize, length: usize) -> Vec<Node<'a>> {
    ast.comments.iter().map(|&id| ast.node(id)).filter(|c| c.start() > start && c.start() < start + length).collect()
}

fn is_consistent(line: &nls::NlsLine, tagged: bool) -> bool {
    line.elements.iter().all(|e| e.has_tag == tagged)
}

struct Conversion<'a> {
    infix: Node<'a>,
    tagged: bool,
}

/// `StringConcatFinder.visit(InfixExpression)` (with `allConcats == true`).
fn visit_infix<'a>(visited: Node<'a>) -> Option<Conversion<'a>> {
    if visited.simple("operator") != Some("+") || visited.list("extendedOperands").is_empty() {
        return None;
    }
    if visited.location_is("leftOperand") || visited.location_is("rightOperand") {
        return None;
    }
    if !visited.type_binding().is_some_and(|t| t.qualified_name() == "java.lang.String") {
        return None;
    }
    let ast = visited.ast;
    let left = visited.child("leftOperand").filter(|l| l.is(NodeKind::StringLiteral))?;
    let right = visited.child("rightOperand").filter(|r| r.is(NodeKind::StringLiteral))?;
    let extended = visited.list("extendedOperands");
    let mut has_comments = has_nls(&super::util::trailing_comments(right));
    let line_region = |literal: Node<'_>| {
        let line = ast.line_of(literal.start());
        let end = if line + 1 < ast.line_count() { ast.line_start(line + 1) } else { ast.source.len() };
        comments_for_region(ast, literal.start(), end - literal.start())
    };
    has_comments = has_comments || has_nls(&line_region(left));
    has_comments = has_comments || has_nls(&line_region(right));
    for operand in &extended {
        if operand.is(NodeKind::StringLiteral) {
            has_comments = has_comments || has_nls(&line_region(*operand));
            continue;
        }
        return None;
    }
    let mut is_tagged = false;
    if !has_comments {
        let comments = comments_for_region(ast, visited.start(), visited.length());
        for comment in comments {
            if !comment.is(NodeKind::LineComment) {
                return None;
            }
            let text = ast.substring(comment.start() + 2, comment.start() + comment.length());
            if !text.trim().is_empty() {
                return None;
            }
        }
    } else if !visited.ancestors().any(|a| a.kind().is_annotation()) {
        let line = nls::scan_from(ast, left.start())?;
        is_tagged = line.elements.first()?.has_tag;
        if !is_consistent(&line, is_tagged) {
            return None;
        }
        let line = nls::scan_from(ast, right.start())?;
        if !is_consistent(&line, is_tagged) {
            return None;
        }
        for operand in &extended {
            let line = nls::scan_from(ast, operand.start())?;
            if !is_consistent(&line, is_tagged) {
                return None;
            }
        }
    }
    if !is_tagged || visited.ancestors().any(|a| a.kind().is_statement() || a.is(NodeKind::FieldDeclaration)) {
        return Some(Conversion { infix: visited, tagged: is_tagged });
    }
    None
}

fn find_conversion<'a>(exp: Node<'a>) -> Option<Conversion<'a>> {
    if exp.is(NodeKind::InfixExpression) {
        return visit_infix(exp);
    }
    exp.children().into_iter().find_map(find_conversion)
}

/// `ChangeStringConcatToTextBlock.rewriteAST`.
fn change_string_concat_to_text_block(ctx: &Context, options: &Options, conversion: &Conversion<'_>) -> Option<ASTRewrite> {
    let infix = conversion.infix;
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut indent = "\t".to_owned();
    if options.get("org.eclipse.jdt.core.formatter.tabulation.char").map(String::as_str) == Some("space") {
        indent = " ".repeat(crate::rewrite::indent::tab_width(options).max(0) as usize);
    }
    let mut parts = Vec::new();
    for operand in [infix.child("leftOperand")?, infix.child("rightOperand")?].into_iter().chain(infix.list("extendedOperands")) {
        let value = operand.simple("escapedValue")?;
        let inner: String = value.chars().skip(1).take(value.chars().count().saturating_sub(2)).collect();
        parts.extend(unescape_block(&inner));
    }
    let buf = assemble(&parts, &indent, true);
    if !conversion.tagged {
        let text_block = rw.create_string_placeholder(&buf, NodeKind::TextBlock);
        rw.replace(RNode::Orig(infix.id), Some(text_block));
    } else {
        let statement = infix.ancestors().find(|a| a.kind().is_statement() || a.is(NodeKind::FieldDeclaration))?;
        let ast = &ctx.ast;
        let mut buffer = ast.substring(statement.start(), infix.start());
        buffer.push_str(&buf);
        buffer.push_str(&ast.substring(infix.end(), statement.end()));
        let placeholder = rw.create_string_placeholder(&buffer, statement.kind());
        rw.set_source_range(statement.id, statement.start(), statement.length());
        rw.replace(RNode::Orig(statement.id), Some(placeholder));
    }
    Some(rw)
}

/// `QuickAssistProcessor.getStringConcatToTextBlockProposal`.
pub fn string_concat_to_text_block(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    let selects = |n: Node<'_>| matches!(n.kind(), NodeKind::Assignment | NodeKind::VariableDeclarationFragment | NodeKind::FieldDeclaration | NodeKind::InfixExpression);
    let exp = if selects(node) {
        node
    } else {
        match node.parent().filter(|p| selects(*p)) {
            Some(p) => p,
            None => return,
        }
    };
    if !super::util::compliance_at_least(options, "15") {
        return;
    }
    let Some(conversion) = find_conversion(exp) else { return };
    let Some(rw) = change_string_concat_to_text_block(ctx, options, &conversion) else { return };
    let label = messages::fix("StringConcatToTextBlockFix_convert_msg");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::CONVERT_TO_TEXT_BLOCK, rw));
}
