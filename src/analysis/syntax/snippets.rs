//! Static Java snippet completions.
//! Template content sourced directly from eclipse.jdt.ls:
//! org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/corext/template/java/CodeSnippetTemplate.java

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position, Range, TextEdit,
};

/// Keywords valid in expression position (after `=`, `return`, etc.).
/// Used instead of `java_snippets()` when the cursor is in an expression context.
pub fn expression_keywords() -> Vec<CompletionItem> {
    vec![
        keyword("new"), keyword("null"), keyword("true"), keyword("false"),
        keyword("this"), keyword("super"), keyword("instanceof"),
        snippet("new", "create new object", "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
    ]
}

/// Snippets valid only in a class body (not inside a method).
/// Corresponds to JavaContextType.ID_MEMBERS in jdtls.
pub fn class_body_snippets() -> Vec<CompletionItem> {
    vec![
        // Note: "ctor" is generated dynamically in CompletionService.java
        // with the actual enclosing class name substituted in.
        snippet("main", "public static main method",
            "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("psvm", "public static main method",
            "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("method", "method",
            "${1|public,protected,private|}${2| , static |}${3:void} ${4:name}(${5}) {\n\t${0}\n}"),
        snippet("staticmethod", "static method",
            "${1|public,private|} static ${2:void} ${3:name}(${4}) {\n\t${0}\n}"),
        snippet("field", "field",
            "${1|public,protected,private|} ${2:String} ${3:name};"),
        // "new" is ID_ALL in jdtls — valid in both members and statements contexts.
        snippet("new", "create new object",
            "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
    ]
}

/// Keywords valid in a class body while declaring fields, methods, or nested types.
pub fn class_body_keywords() -> Vec<CompletionItem> {
    vec![
        keyword("public"),
        keyword("private"),
        keyword("protected"),
        keyword("static"),
        keyword("final"),
        keyword("abstract"),
        keyword("synchronized"),
        keyword("transient"),
        keyword("volatile"),
        keyword("void"),
        keyword("int"),
        keyword("long"),
        keyword("double"),
        keyword("float"),
        keyword("boolean"),
        keyword("char"),
        keyword("byte"),
        keyword("short"),
        keyword("class"),
        keyword("interface"),
        keyword("enum"),
        keyword("record"),
    ]
}

/// Snippets valid inside a method body, matching jdtls template definitions.
pub fn method_body_snippets() -> Vec<CompletionItem> {
    vec![
        // ── Iteration ─────────────────────────────────────────────────────────
        snippet("foreach", "iterate over an array or Iterable",
            "for (${1:var} ${2:item} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("iter", "iterate over an array or Iterable",
            "for (${1:var} ${2:item} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("fori", "iterate over array",
            "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:array.length}; ${2:i}++) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        // ── Conditionals ──────────────────────────────────────────────────────
        snippet("if", "if statement",
            "if (${1:condition}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("ifelse", "if-else statement",
            "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}"),
        snippet("ife", "if-else statement",
            "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}"),
        snippet("ifnull", "if statement checking for null",
            "if (${1:name} == null) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("ifnotnull", "if statement checking for not null",
            "if (${1:name} != null) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        // ── Loops ─────────────────────────────────────────────────────────────
        snippet("while", "while statement",
            "while (${1:condition}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("dowhile", "do-while statement",
            "do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:condition});"),
        snippet("do", "do-while statement",
            "do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:condition});"),
        // ── Switch ────────────────────────────────────────────────────────────
        snippet("switch", "switch statement",
            "switch (${1:key}) {\n\tcase ${2:value}:\n\t\t${0}\n\t\tbreak;\n\n\tdefault:\n\t\tbreak;\n}"),
        snippet("sw", "switch statement",
            "switch (${1:key}) {\n\tcase ${2:value}:\n\t\t${0}\n\t\tbreak;\n\n\tdefault:\n\t\tbreak;\n}"),
        // ── Exception handling ────────────────────────────────────────────────
        snippet("try", "try/catch block",
            "try {\n\t$TM_SELECTED_TEXT${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}// TODO: handle exception\n}"),
        snippet("trycatch", "try/catch block",
            "try {\n\t$TM_SELECTED_TEXT${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}// TODO: handle exception\n}"),
        snippet("tryresources", "try/catch block with resources",
            "try (${1}) {\n\t$TM_SELECTED_TEXT${2}\n} catch (${3:Exception} ${4:e}) {\n\t${0}// TODO: handle exception\n}"),
        // ── Output ────────────────────────────────────────────────────────────
        snippet("sysout", "print to standard out",   "System.out.println(${0});"),
        snippet("sout",   "print to standard out",   "System.out.println(${0});"),
        snippet("syso",   "print to standard out",   "System.out.println(${0});"),
        snippet("syserr", "print to standard err",   "System.err.println(${0});"),
        snippet("serr",   "print to standard err",   "System.err.println(${0});"),
        snippet("soutm",  "print current method to standard out",
            "System.out.println(\"${0}\");"),
        // ── Misc ──────────────────────────────────────────────────────────────
        snippet("new", "create new object",
            "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
        // ── Keywords valid inside a method body ──────────────────────────────
        keyword("boolean"), keyword("byte"), keyword("char"), keyword("double"),
        keyword("float"), keyword("int"), keyword("long"), keyword("short"),
        keyword("void"), keyword("var"),
        keyword("null"), keyword("true"), keyword("false"),
        keyword("this"), keyword("super"), keyword("new"),
        keyword("return"), keyword("throw"), keyword("instanceof"),
        keyword("final"),
        keyword("yield"), keyword("break"), keyword("continue"), keyword("default"),
    ]
}

/// Postfix snippets matching the Eclipse JDT LS family used after `expr.`.
pub fn postfix_snippets(source: &str, offset: usize, pos: Position) -> Vec<CompletionItem> {
    let Some(access) = parse_postfix_access(source, offset) else {
        return vec![];
    };
    if is_likely_type_qualifier(&access.expression) {
        return vec![];
    }
    let replacement_range = replacement_range_for_postfix(source, access.expr_start, pos);
    let variable_name = suggested_variable_name(&access.expression);
    let mut items = vec![
        postfix_snippet(
            "cast",
            "Casts the expression to a new type",
            replacement_range,
            &access.member_prefix,
            format!("((${{1}}){})${{0}}", access.expression),
            None,
        ),
        postfix_snippet(
            "null",
            "Checks the expression for null",
            replacement_range,
            &access.member_prefix,
            format!("if ({} == null) {{\n\t${{0}}\n}}", access.expression),
            None,
        ),
        postfix_snippet(
            "opt",
            "Wraps the expression with Optional.ofNullable",
            replacement_range,
            &access.member_prefix,
            format!("Optional.ofNullable({})", access.expression),
            optional_import_edit(source),
        ),
        postfix_snippet(
            "par",
            "Wraps the expression in parentheses",
            replacement_range,
            &access.member_prefix,
            format!("({})", access.expression),
            None,
        ),
        postfix_snippet(
            "syserr",
            "Print the expression to standard err",
            replacement_range,
            &access.member_prefix,
            format!("System.err.println({});${{0}}", access.expression),
            None,
        ),
        postfix_snippet(
            "sysouf",
            "Print the expression with printf",
            replacement_range,
            &access.member_prefix,
            format!("System.out.printf(\"\", {});${{0}}", access.expression),
            None,
        ),
        postfix_snippet(
            "sysout",
            "Print the expression to standard out",
            replacement_range,
            &access.member_prefix,
            format!("System.out.println({});${{0}}", access.expression),
            None,
        ),
        postfix_snippet(
            "sysoutv",
            "Print the expression name and value to standard out",
            replacement_range,
            &access.member_prefix,
            format!(
                "System.out.println(\"{} = \" + {});${{0}}",
                variable_name, access.expression
            ),
            None,
        ),
        postfix_snippet(
            "var",
            "Assign the expression to a new variable",
            replacement_range,
            &access.member_prefix,
            format!("var ${{1:{variable_name}}} = {};${{0}}", access.expression),
            None,
        ),
    ];

    if access.member_prefix.is_empty() {
        items.extend([
            postfix_snippet(
                "format",
                "Pass the expression as the format string",
                replacement_range,
                &access.member_prefix,
                format!("String.format({}, ${{0}});", access.expression),
                None,
            ),
            postfix_snippet(
                "throw",
                "Throw the expression",
                replacement_range,
                &access.member_prefix,
                format!("throw {};", access.expression),
                None,
            ),
        ]);
    }

    items
        .into_iter()
        .filter(|item| item.label.starts_with(&access.member_prefix))
        .collect()
}

fn snippet(label: &str, detail: &str, insert_text: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_owned()),
        insert_text: Some(insert_text.to_owned()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("zz-{label}")),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```java\n{insert_text}\n```"),
        })),
        ..Default::default()
    }
}

fn postfix_snippet(
    label: &str,
    detail: &str,
    replacement_range: Range,
    member_prefix: &str,
    insert_text: String,
    additional_edit: Option<TextEdit>,
) -> CompletionItem {
    let prefix_units = member_prefix.encode_utf16().count() as u32;
    let main_start = replacement_range.end.character.saturating_sub(prefix_units);
    let main_range = Range {
        start: Position {
            line: replacement_range.end.line,
            character: main_start,
        },
        end: replacement_range.end,
    };
    let mut additional_text_edits = vec![TextEdit {
        range: Range {
            start: replacement_range.start,
            end: Position {
                line: replacement_range.end.line,
                character: main_start,
            },
        },
        new_text: String::new(),
    }];
    if let Some(edit) = additional_edit {
        additional_text_edits.push(edit);
    }

    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_owned()),
        insert_text: Some(insert_text.clone()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range: main_range,
            new_text: insert_text.clone(),
        })),
        additional_text_edits: Some(additional_text_edits),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```java\n{insert_text}\n```"),
        })),
        sort_text: Some(format!("zz-{label}")),
        ..Default::default()
    }
}

fn keyword(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    }
}

struct PostfixAccess {
    expression: String,
    expr_start: usize,
    member_prefix: String,
}

fn parse_postfix_access(source: &str, offset: usize) -> Option<PostfixAccess> {
    let end = offset.min(source.len());
    let mut suffix_start = end;
    while suffix_start > 0 {
        let ch = source[..suffix_start].chars().next_back()?;
        if !is_java_identifier(ch) {
            break;
        }
        suffix_start -= ch.len_utf8();
    }
    if suffix_start == 0 || source.as_bytes()[suffix_start - 1] != b'.' {
        return None;
    }

    let dot_pos = suffix_start - 1;
    let mut expr_start = expression_start_before_dot(source, dot_pos);
    expr_start = maybe_extend_new_keyword(source, expr_start);
    let expression = source[expr_start..dot_pos].trim().to_owned();
    if expression.is_empty() {
        return None;
    }

    Some(PostfixAccess {
        expression,
        expr_start,
        member_prefix: source[suffix_start..end].to_owned(),
    })
}

fn expression_start_before_dot(source: &str, dot_pos: usize) -> usize {
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;

    for (idx, ch) in source[..dot_pos].char_indices().rev() {
        match ch {
            ')' => {
                paren_depth += 1;
                start = idx;
            }
            '(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                    start = idx;
                } else {
                    start = idx + ch.len_utf8();
                    break;
                }
            }
            ']' => {
                bracket_depth += 1;
                start = idx;
            }
            '[' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                    start = idx;
                } else {
                    start = idx + ch.len_utf8();
                    break;
                }
            }
            _ if paren_depth > 0 || bracket_depth > 0 => {
                start = idx;
            }
            _ if is_expression_boundary(ch) => {
                start = idx + ch.len_utf8();
                break;
            }
            _ => {
                start = idx;
            }
        }
    }

    start
}

fn maybe_extend_new_keyword(source: &str, expr_start: usize) -> usize {
    let prefix = &source[..expr_start];
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    if !trimmed.ends_with("new") {
        return expr_start;
    }
    let new_start = trimmed.len().saturating_sub(3);
    if new_start > 0 {
        let prev = trimmed[..new_start].chars().next_back();
        if prev.is_some_and(|ch| is_java_identifier(ch)) {
            return expr_start;
        }
    }
    new_start
}

fn replacement_range_for_postfix(source: &str, expr_start: usize, end: Position) -> Range {
    let line_start = source[..expr_start].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let start_char = source[line_start..expr_start]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>() as u32;
    Range {
        start: Position {
            line: end.line,
            character: start_char,
        },
        end,
    }
}

fn suggested_variable_name(expression: &str) -> String {
    let trimmed = expression.trim();
    let candidate = trimmed
        .trim_end_matches(')')
        .rsplit(|c: char| !is_java_identifier(c))
        .find(|part| !part.is_empty())
        .unwrap_or("value");

    if candidate == "new" {
        return "value".to_owned();
    }

    let mut chars = candidate.chars();
    let Some(first) = chars.next() else {
        return "value".to_owned();
    };
    let mut name = first.to_ascii_lowercase().to_string();
    name.extend(chars);
    name
}

fn optional_import_edit(source: &str) -> Option<TextEdit> {
    if source.contains("import java.util.Optional;")
        || source.contains("import java.util.*;")
        || source.contains("java.util.Optional")
    {
        return None;
    }

    let mut insert_line = 0u32;
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("package ") || trimmed.starts_with("import ") {
            insert_line = idx as u32 + 1;
        }
    }

    Some(TextEdit {
        range: Range {
            start: Position {
                line: insert_line,
                character: 0,
            },
            end: Position {
                line: insert_line,
                character: 0,
            },
        },
        new_text: "import java.util.Optional;\n".to_owned(),
    })
}

fn is_java_identifier(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

fn is_likely_type_qualifier(expression: &str) -> bool {
    let expr = expression.trim();
    if expr.is_empty()
        || expr.contains(char::is_whitespace)
        || expr.ends_with(')')
        || expr.ends_with(']')
        || expr.ends_with('}')
    {
        return false;
    }

    let segments: Vec<&str> = expr.split('.').collect();
    if segments.is_empty() || !segments.iter().all(|seg| {
        !seg.is_empty() && seg.chars().all(is_java_identifier)
    }) {
        return false;
    }

    let last = segments.last().copied().unwrap_or_default();
    last.starts_with(|c: char| c.is_uppercase())
}

fn is_expression_boundary(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | ' ' | '\t' | ';' | ',' | '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|'
            | '!' | '?' | ':'
            | '{' | '}'
    )
}
