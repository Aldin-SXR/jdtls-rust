//! Port of jdt.ls `JavadocCompletionProposal` (the "Javadoc comment"
//! snippet after `/**`) and the type definition snippets of
//! `SnippetCompletionProposal`.

use super::doc::Doc;
use super::handler::UnitInfo;
use super::item::{item_kind, EditRange, Item, ItemDefaults, ItemTextEdit};
use super::prefs::Client;
use super::proposal::Context;
use super::snippets::{set_insert_text_format, set_insert_text_mode, type_definition_snippets as build_type_snippets, TypeSnippetEnv};
use super::sort_text::convert_relevance;
use super::Env;
use crate::analysis::dispatcher::RequestContext;
use serde::Deserialize;
use serde_json::json;
use tower_lsp::lsp_types::{Documentation, TextEdit};

pub const JAVA_DOC_COMMENT: &str = "Javadoc comment";

/// The member following a `/**` (bridge `javadocTarget`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JavadocTarget {
    /// "type" or "method"; `None` when no type/method contains the offset.
    pub kind: Option<String>,
    pub name_offset: i32,
    /// Methods: overrides or implements a method of a supertype.
    pub inherited: bool,
    pub constructor: bool,
    pub return_void: bool,
    pub type_params: Vec<String>,
    pub params: Vec<String>,
    pub exceptions: Vec<String>,
    /// Types: qualified (dotted) name within the unit; record components.
    pub type_qualified_name: Option<String>,
    pub record: bool,
    pub record_components: Vec<String>,
}

fn find_end_of_whitespace(doc: &Doc, mut offset: usize, end: usize) -> usize {
    while offset < end {
        let c = doc.char_at(offset);
        if c != ' ' && c != '\t' {
            return offset;
        }
        offset += 1;
    }
    end
}

fn has_end_javadoc(doc: &Doc, mut offset: usize) -> bool {
    let mut pos: isize = -1;
    while offset < doc.len() {
        let c = doc.char_at(offset);
        if !c.is_whitespace() && c != '*' {
            pos = offset as isize;
            break;
        }
        offset += 1;
    }
    if doc.len() as isize >= pos + 2 && pos >= 1 && doc.get(pos as usize - 1, 2) == "*/" {
        return true;
    }
    false
}

fn strip_start_ws(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

fn strip_end_ws(s: &str) -> &str {
    s.trim_end_matches([' ', '\t'])
}

/// `prepareTemplate`.
fn prepare_template(text: &str, delim: &str, mut add_gap: bool) -> String {
    let ends = text.ends_with(delim);
    let mut lines: Vec<&str> = text.split(delim).collect();
    // String.split drops trailing empty strings
    while lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    let mut buf = String::new();
    for (i, line) in lines.iter().enumerate() {
        if add_gap {
            let stripped = strip_start_ws(line);
            if stripped.starts_with('*') {
                if stripped != "*" {
                    let index = line.find('*').unwrap();
                    buf.push_str(&line[..=index]);
                    buf.push_str(" ${0}");
                    buf.push_str(delim);
                }
                add_gap = false;
            }
        }
        buf.push_str(strip_end_ws(line));
        if i < lines.len() - 1 || ends {
            buf.push_str(delim);
        }
    }
    buf
}

/// `CodeGeneration.getMethodComment` / `getTypeComment` with jdt.ls'
/// default templates, then `prepareTemplateComment`.
fn comment_body(target: &JavadocTarget, delim: &str) -> Option<String> {
    let mut tags: Vec<String> = Vec::new();
    let comment = match target.kind.as_deref()? {
        "method" => {
            if target.inherited {
                return None; // override comment template is empty
            }
            for t in &target.type_params {
                tags.push(format!("@param <{t}>"));
            }
            for p in &target.params {
                tags.push(format!("@param {p}"));
            }
            if !target.constructor && !target.return_void {
                tags.push("@return".to_owned());
            }
            for e in &target.exceptions {
                tags.push(format!("@throws {e}"));
            }
            // "/**\n * ${tags}\n */\n": the tags line stays (empty) without tags
            let mut c = String::from("/**");
            c.push_str(delim);
            c.push_str(" * ");
            c.push_str(&tags.join(&format!("{delim} * ")));
            c.push_str(delim);
            c.push_str(" */");
            c.push_str(delim);
            c
        }
        "type" => {
            if target.record {
                for p in &target.record_components {
                    tags.push(format!("@param {p}"));
                }
            } else {
                for t in &target.type_params {
                    tags.push(format!("@param <{t}>"));
                }
            }
            let mut c = String::from("/**");
            c.push_str(delim);
            c.push_str(" * ");
            c.push_str(target.type_qualified_name.as_deref().unwrap_or(""));
            c.push_str(delim);
            c.push_str(" * ");
            c.push_str(&tags.join(&format!("{delim} * ")));
            c.push_str(delim);
            c.push_str(" */");
            c
        }
        _ => return None,
    };
    // prepareTemplateComment
    let mut comment = comment.trim().to_owned();
    if comment.ends_with("*/") {
        comment.truncate(comment.len() - 2);
    }
    let mut comment = comment.trim().to_owned();
    if comment.starts_with("/*") {
        if comment.len() > 2 && comment.as_bytes()[2] == b'*' {
            comment = comment[3..].to_owned();
        } else {
            comment = comment[2..].to_owned();
        }
    }
    let comment = comment.trim_start_matches(' ').to_owned();
    Some(comment)
}

/// `JavadocCompletionProposal.getProposals`.
pub async fn javadoc_proposals(
    env: &Env,
    ctx: &RequestContext,
    unit: &UnitInfo,
    offset: usize,
    context: &Context,
    client: &Client,
    defaults: &ItemDefaults,
) -> Vec<Item> {
    let d = &unit.doc;
    if d.len() == 0 {
        return Vec::new();
    }
    let p = if offset == d.len() { offset - 1 } else { offset };
    let (line_off, line_len) = d.line_info_of_offset(p);
    let line_str = d.get(line_off, line_len);
    if !line_str.trim().starts_with("/**") {
        return Vec::new();
    }
    if !has_end_javadoc(d, offset) {
        return Vec::new();
    }
    let text = context.token.clone().unwrap_or_default();
    let mut buf = text;
    // findPrefixRange
    let line_end = line_off + line_len;
    let mut indent_end = find_end_of_whitespace(d, line_off, line_end);
    if indent_end < line_end && d.char_at(indent_end) == '*' {
        indent_end += 1;
        while indent_end < line_end && d.char_at(indent_end) == ' ' {
            indent_end += 1;
        }
    }
    let indentation = d.get(line_off, indent_end - line_off);
    let length_to_add = (offset.saturating_sub(line_off)).min(indent_end - line_off);
    buf.push_str(&indentation.chars().take(length_to_add).collect::<String>());
    let delim = d.default_line_delimiter();
    let q = json!({ "op": "javadocTarget" });
    let target: JavadocTarget = match env.dispatcher.code_assist(ctx, unit.uri.as_str(), offset, q).await {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    if target.kind.is_none() || target.name_offset as usize <= offset {
        return Vec::new();
    }
    let Some(s) = comment_body(&target, &delim) else { return Vec::new() };
    if s.trim() == "*" {
        return Vec::new();
    }
    buf.push_str(&s);
    let next_non_ws = find_end_of_whitespace(d, offset, d.len());
    if !d.char_at(next_non_ws).is_whitespace() {
        buf.push_str(&delim);
    }
    let mut ci = Item::default();
    let range = d.range(offset, 0);
    let replacement = prepare_template(&buf, &delim, client.snippets);
    if client.item_defaults_property("editRange") && defaults.edit_range == Some(EditRange::Range(range)) && false {
        ci.text_edit_text = Some(replacement);
    } else {
        ci.text_edit = Some(ItemTextEdit::Edit(TextEdit::new(range, replacement)));
    }
    ci.filter_text = Some(JAVA_DOC_COMMENT.into());
    ci.label = JAVA_DOC_COMMENT.into();
    ci.sort_text = Some(convert_relevance(0));
    ci.kind = Some(item_kind::SNIPPET);
    set_insert_text_format(&mut ci, client, defaults);
    set_insert_text_mode(&mut ci, client, defaults);
    let mut documentation = prepare_template(&buf, &delim, false);
    if documentation.starts_with(&delim) {
        documentation = documentation.replacen(&delim, "", 1);
    }
    ci.documentation = Some(Documentation::String(documentation));
    vec![ci]
}

fn is_modifier(w: &str) -> bool {
    matches!(
        w,
        "public" | "protected" | "private" | "static" | "abstract" | "final" | "native" | "synchronized" | "transient" | "volatile" | "strictfp" | "default"
    )
}

/// `SnippetCompletionProposal.getTypeDefinitionSnippets`.
pub fn type_definition_snippets(unit: &UnitInfo, context: &Context, client: &Client, defaults: &ItemDefaults) -> Vec<Item> {
    let node = context.completion_node.as_deref().unwrap_or("");
    let ok_context = context.extended && !context.in_javadoc;
    let in_method = context.enclosing_kind.as_deref() == Some("method");
    let accept = |accept_class: bool| -> bool {
        if !ok_context {
            return false;
        }
        match node {
            "CompletionOnKeyword2" | "CompletionOnFieldType" => true,
            "CompletionOnSingleNameReference" if accept_class => {
                if in_method {
                    // astNode == null || astNode.getParent() instanceof ExpressionStatement
                    context.completion_node_parent.is_none()
                        || context.token_location & super::proposal::tl::STATEMENT_START != 0
                } else {
                    true
                }
            }
            _ => false,
        }
    };
    let needs_public = {
        let mut r = false;
        if ok_context && matches!(node, "CompletionOnKeyword2" | "CompletionOnFieldType" | "CompletionOnSingleNameReference") && !in_method {
            // the previous token must not be a modifier
            let toks = crate::features::scanner::scan(&unit.text);
            let token_start_byte = utf16_to_byte(&unit.text, context.token_start.max(0) as usize);
            let prev = toks.iter().filter(|t| !t.is_comment() && t.end <= token_start_byte).last();
            let prev_is_modifier = prev.is_some_and(|t| is_modifier(t.text(&unit.text)));
            r = !prev_is_modifier && !(node == "CompletionOnSingleNameReference" && context.enclosing_kind.as_deref() == Some("initializer"));
        }
        r
    };
    let compliance = unit.options.get("org.eclipse.jdt.core.compiler.compliance").cloned().unwrap_or_default();
    let env = TypeSnippetEnv {
        context,
        doc: &unit.doc,
        client,
        defaults,
        compliance: &compliance,
        unit_name: &unit.unit_name,
        package_name: &unit.folder_package,
        has_package_declaration: unit.has_package_declaration,
        all_types: &unit.all_type_names,
        has_types: !unit.cu.type_names.is_empty(),
        line_delimiter: &unit.line_delimiter(),
        needs_public,
        accept_class: accept(true),
        accept_other: accept(false),
        markdown: client.documentation_markdown,
    };
    build_type_snippets(&env)
}

fn utf16_to_byte(text: &str, off: usize) -> usize {
    let mut u = 0;
    for (b, c) in text.char_indices() {
        if u >= off {
            return b;
        }
        u += c.len_utf16();
    }
    text.len()
}
