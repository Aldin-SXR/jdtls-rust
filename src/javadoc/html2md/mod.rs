//! Port of flexmark-html2md-converter 0.64.8 (`FlexmarkHtmlConverter` +
//! `HtmlConverterCoreNodeRenderer`) as configured by jdt.ls'
//! `JavaDoc2MarkdownConverter`: default options plus `abbr` in the unwrapped
//! tags, `OUTPUT_ATTRIBUTES_ID=false`, `TYPOGRAPHIC_SMARTS=false`, and the jdt.ls
//! renderers for `a`, `tt`, `dfn` and `dl`.
//!
//! With these options no element attributes are ever collected for output, so
//! the attribute plumbing (`processAttributes`, `outputAttributes`,
//! `transferIdToParent`, ...) is a no-op and is omitted.

mod emoji;
pub mod line_appendable;
pub mod table;

use super::html::{java_trim, Document, NodeId};
use line_appendable::*;
use table::{Align, Cell, MarkdownTable};

/// `FlexmarkHtmlConverter.FORMAT_FLAGS` default (+ `F_PREFIX_PRE_FORMATTED` added by `MarkdownWriterBase`).
const FORMAT_FLAGS: u32 =
    F_TRIM_TRAILING_WHITESPACE | F_TRIM_LEADING_WHITESPACE | F_COLLAPSE_WHITESPACE | F_TRIM_LEADING_EOL | F_PREFIX_PRE_FORMATTED;
const MAX_BLANK_LINES: i64 = 2;
const DEFINITION_MARKER_SPACES: usize = 3;
const CODE_INDENT: &str = "    ";
const THEMATIC_BREAK: &str = "*** ** * ** ***";
const NBSP_TEXT: &str = " ";
const EOL_IN_TITLE_ATTRIBUTE: &str = " ";
const UNWRAPPED_TAGS: &[&str] = &["article", "address", "frameset", "section", "small", "iframe", "abbr"];
const WRAPPED_TAGS: &[&str] = &["kbd", "var"];
const HEADING_NODES: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

struct Ctx {
    out: LineAppendable,
    rendering_node: Option<NodeId>,
}

struct State {
    elements: Vec<NodeId>,
    index: usize,
}

pub struct Converter<'a> {
    doc: &'a Document,
    ctxs: Vec<Ctx>,
    states: Vec<State>,
    inline_code: bool,
    table: Option<MarkdownTable>,
    table_suppress_columns: bool,
}

/// `converter.convert(document, out, maxTrailingBlankLines)` for the jdt.ls-configured converter.
pub fn convert_document(doc: &Document, max_trailing_blank_lines: i64) -> String {
    let mut c = Converter {
        doc,
        ctxs: vec![Ctx { out: LineAppendable::new(FORMAT_FLAGS), rendering_node: None }],
        states: Vec::new(),
        inline_code: false,
        table: None,
        table_suppress_columns: false,
    };
    if let Some(body) = doc.body() {
        c.process_html_tree(0, body);
    }
    // flushTo(out, maxBlankLines, maxTrailingBlankLines): line() then appendTo (which also calls line())
    let out = &mut c.ctxs[0].out;
    out.line();
    out.to_string_with(MAX_BLANK_LINES, max_trailing_blank_lines)
}

fn java_whitespace(c: char) -> bool {
    // Character.isWhitespace
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{001C}' | '\u{001D}' | '\u{001E}' | '\u{001F}')
        || (c.is_whitespace() && c != '\u{00A0}' && c != '\u{2007}' && c != '\u{202F}' && c != '\u{0085}')
}

/// `HtmlConverterCoreNodeRenderer.getMaxRepeatedChars`
pub fn get_max_repeated_chars(text: &str, c: char, min_count: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut min_count = min_count;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == c {
            let mut count = 0;
            while i + count < chars.len() && chars[i + count] == c {
                count += 1;
            }
            if min_count <= count {
                min_count = count + 1;
            }
            i += count;
        } else {
            i += 1;
        }
    }
    min_count
}

impl<'a> Converter<'a> {
    fn w(&mut self, ctx: usize) -> &mut LineAppendable {
        &mut self.ctxs[ctx].out
    }

    // ── state handling ──

    fn push_state(&mut self, parent: NodeId) {
        self.states.push(State { elements: self.doc.child_nodes(parent), index: 0 });
    }

    fn pop_state(&mut self) {
        self.states.pop().expect("popState with an empty stack");
    }

    fn peek(&self) -> Option<NodeId> {
        let s = self.states.last()?;
        s.elements.get(s.index).copied()
    }

    fn next(&mut self) -> Option<NodeId> {
        let n = self.peek();
        if n.is_some() {
            self.states.last_mut().unwrap().index += 1;
        }
        n
    }

    fn new_sub_context(&mut self, parent_ctx: usize) -> usize {
        let opts = self.ctxs[parent_ctx].out.get_options();
        self.ctxs.push(Ctx { out: LineAppendable::new(opts), rendering_node: None });
        self.ctxs.len() - 1
    }

    fn drop_sub_context(&mut self) -> LineAppendable {
        self.ctxs.pop().unwrap().out
    }

    // ── rendering ──

    fn render(&mut self, node: NodeId, ctx: usize) {
        let old = self.ctxs[0].rendering_node;
        self.ctxs[ctx].rendering_node = Some(node);
        self.dispatch(node, ctx);
        self.ctxs[ctx].rendering_node = old;
    }

    fn dispatch(&mut self, node: NodeId, ctx: usize) {
        let name = self.doc.node_name(node).to_lowercase();
        match name.as_str() {
            "#comment" => {}
            "a" => self.process_a(node, ctx),
            "aside" => self.process_aside(node, ctx),
            "b" | "strong" => self.process_wrap_emphasis(node, ctx, "**"),
            "blockquote" => self.process_block_quote(node, ctx),
            "br" => self.process_br(ctx),
            "code" => self.process_code(node, ctx),
            "del" | "strike" => self.process_wrap_emphasis(node, ctx, "~~"),
            "div" => self.process_div(node, ctx),
            "dl" => self.process_dl(node, ctx),
            "em" | "i" => self.process_wrap_emphasis(node, ctx, "*"),
            "g-emoji" => self.process_emoji(node, ctx),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.process_heading(node, ctx),
            "hr" => {
                self.w(ctx).blank_line().append(THEMATIC_BREAK).blank_line();
            }
            "img" => self.process_img(node, ctx),
            "input" => self.process_input(node, ctx),
            "ins" | "u" => self.process_wrap_emphasis(node, ctx, "++"),
            "li" => self.handle_list(ctx, node, false, true, false),
            "math" => self.process_wrapped(node, ctx, None, true),
            "ol" => self.handle_list(ctx, node, true, false, false),
            "ul" => self.handle_list(ctx, node, false, false, false),
            "p" => self.process_p(node, ctx),
            "pre" => self.process_pre(node, ctx),
            "span" => self.process_span(node, ctx),
            "sub" => self.process_sub_sup(node, ctx, "~"),
            "sup" => self.process_sub_sup(node, ctx, "^"),
            "svg" => {
                if !self.doc.has_class(node, "octicon") {
                    self.process_html_tree(ctx, node);
                }
            }
            "table" => self.process_table(node, ctx),
            "#text" => self.process_text(node, ctx),
            "tt" => self.process_tt(node, ctx),
            "dfn" => self.process_dfn(node, ctx),
            _ if UNWRAPPED_TAGS.contains(&name.as_str()) => self.process_html_tree(ctx, node),
            _ if WRAPPED_TAGS.contains(&name.as_str()) => self.process_wrapped(node, ctx, Some(false), false),
            _ => self.process_html_tree(ctx, node),
        }
    }

    /// `processHtmlTree` / `renderChildren`
    fn process_html_tree(&mut self, ctx: usize, parent: NodeId) {
        self.push_state(parent);
        while let Some(node) = self.next() {
            self.render(node, ctx);
        }
        self.pop_state();
    }

    // ── text helpers ──

    /// `prepareText(text, inCode)`
    fn prepare_text_in(&self, text: &str, in_code: bool) -> String {
        // TYPOGRAPHIC_QUOTES (default true): map typographic quotes to ASCII
        let mut t = String::with_capacity(text.len());
        let mut rest = text;
        'outer: while !rest.is_empty() {
            for (pat, rep) in [
                ("\u{201C}", "\""),
                ("\u{201D}", "\""),
                ("\u{2018}", "'"),
                ("\u{2019}", "'"),
                ("\u{00AB}", "<<"),
                ("\u{00BB}", ">>"),
                ("&ldquo;", "\""),
                ("&rdquo;", "\""),
                ("&lsquo;", "'"),
                ("&rsquo;", "'"),
                ("&apos;", "'"),
                ("&laquo;", "<<"),
                ("&raquo;", ">>"),
            ] {
                if let Some(r) = rest.strip_prefix(pat) {
                    t.push_str(rep);
                    rest = r;
                    continue 'outer;
                }
            }
            let c = rest.chars().next().unwrap();
            t.push(c);
            rest = &rest[c.len_utf8()..];
        }
        if !in_code {
            escape_special_chars(&t)
        } else {
            t.replace('\u{00A0}', " ")
        }
    }

    fn prepare_text(&self, text: &str) -> String {
        self.prepare_text_in(text, self.inline_code)
    }

    /// `processTextNodes(node, stripIdAttribute, textPrefix, textSuffix)` (void version)
    fn process_text_nodes_into(&mut self, node: NodeId, ctx: usize, prefix: Option<&str>, suffix: Option<&str>) {
        self.push_state(node);
        while let Some(child) = self.next() {
            if self.doc.is_text(child) {
                if let Some(p) = prefix {
                    if !p.is_empty() {
                        self.w(ctx).append(p);
                    }
                }
                let text = self.doc.whole_text(child).to_string();
                let prepared = self.prepare_text(&text);
                self.w(ctx).append(&prepared);
                if let Some(s) = suffix {
                    if !s.is_empty() {
                        self.w(ctx).append(s);
                    }
                }
            } else if self.doc.is_element(child) {
                self.render(child, ctx);
            }
        }
        self.pop_state();
    }

    /// `processTextNodes(node)` (string version, always via the main context)
    fn process_text_nodes_string(&mut self, node: NodeId) -> String {
        self.push_state(node);
        let sub = self.new_sub_context(0);
        while let Some(child) = self.next() {
            if self.doc.is_text(child) {
                let text = self.doc.whole_text(child).to_string();
                let prepared = self.prepare_text(&text);
                self.w(sub).append(&prepared);
            } else if self.doc.is_element(child) {
                self.render(child, sub);
            }
        }
        self.pop_state();
        let mut out = self.drop_sub_context();
        out.to_string_with(-1, -1)
    }

    fn wrap_text_nodes(&mut self, node: NodeId, ctx: usize, wrap: &str, need_space_around: bool) {
        let mut text = self.process_text_nodes_string(node);
        let mut prefix_before: Option<String> = None;
        let mut append_after: Option<String> = None;
        let mut add_space_after = false;

        if !text.is_empty() && need_space_around {
            let first = text.chars().next().unwrap();
            if "\u{00A0} \t\n".contains(first) {
                prefix_before = Some(self.prepare_text(&first.to_string()));
                text = text[first.len_utf8()..].to_string();
            } else if text.starts_with("&nbsp;") {
                prefix_before = Some("&nbsp;".into());
                text = text["&nbsp;".len()..].to_string();
            }
            // (addSpaceBefore is always false in flexmark)

            let last = text.chars().last();
            if let Some(l) = last.filter(|l| "\u{00A0} \t\n".contains(*l)) {
                append_after = Some(self.prepare_text(&l.to_string()));
                text = text[..text.len() - l.len_utf8()].to_string();
            } else if text.ends_with("&nbsp;") {
                append_after = Some("&nbsp;".into());
                text = text[..text.len() - "&nbsp;".len()].to_string();
            } else {
                add_space_after = true;
                if let Some(next) = self.peek() {
                    if self.doc.is_text(next) {
                        let nt = self.doc.whole_text(next);
                        if let Some(c) = nt.chars().next() {
                            if java_whitespace(c) {
                                add_space_after = false;
                            }
                        }
                    }
                }
            }
        }

        if !text.is_empty() {
            let trimmed = text.trim_end_matches(java_whitespace);
            if !trimmed.is_empty() {
                let trimmed = trimmed.to_string();
                let out = self.w(ctx);
                if let Some(p) = &prefix_before {
                    out.append(p);
                }
                out.append(wrap);
                out.append(&trimmed);
                out.append(wrap);
                if let Some(a) = &append_after {
                    out.append(a);
                }
                if add_space_after {
                    out.append_char(' ');
                }
            }
        }
    }

    fn is_first_child(&self, element: NodeId) -> bool {
        let parent = match self.doc.parent(element) {
            Some(p) => p,
            None => return false,
        };
        for &node in self.doc.children(parent) {
            if self.doc.is_element(node) {
                return element == node;
            } else if self.doc.node_name(node) == "#text" && !self.doc.whole_text(node).chars().all(|c| c <= ' ') {
                break;
            }
        }
        false
    }

    fn is_last_child(&self, element: NodeId) -> bool {
        match self.doc.parent(element) {
            Some(p) => self.doc.element_children(p).last() == Some(&element),
            None => false,
        }
    }

    fn parent_tag_name(&self, element: NodeId) -> String {
        self.doc.parent(element).map(|p| self.doc.tag_name(p).to_string()).unwrap_or_default()
    }

    /// `MarkdownWriterBase.tailBlankLine()`
    fn tail_blank_line(&mut self, ctx: usize) {
        let prefix = self.ctxs[ctx].out.get_prefix();
        let replaced = self.last_block_quote_child_prefix(ctx, &prefix);
        let out = self.w(ctx);
        if replaced != prefix {
            out.set_prefix(&replaced, false);
            out.blank_line_count(1);
            out.set_prefix(&prefix, false);
        } else {
            out.blank_line_count(1);
        }
    }

    fn last_block_quote_child_prefix(&self, ctx: usize, prefix: &str) -> String {
        let mut prefix = prefix.to_string();
        if let Some(node) = self.ctxs[ctx].rendering_node {
            if self.doc.is_element(node) {
                let mut element = node;
                while self.doc.next_element_sibling(element).is_none() {
                    let parent = match self.doc.parent_element(element) {
                        Some(p) => p,
                        None => break,
                    };
                    if self.doc.node_name(parent).to_lowercase() == "blockquote" {
                        if let Some(pos) = prefix.rfind('>') {
                            prefix = format!("{} {}", &prefix[..pos], &prefix[pos + 1..]);
                        }
                    }
                    element = parent;
                }
            }
        }
        prefix
    }

    // ── node renderers ──

    fn process_text(&mut self, node: NodeId, ctx: usize) {
        if self.ctxs[ctx].out.is_pre_formatted() {
            let t = self.prepare_text_in(&self.doc.whole_text(node).to_string(), true);
            self.w(ctx).append(&t);
        } else {
            let text = self.prepare_text(&self.doc.text_node_text(node));
            if self.ctxs[ctx].out.offset_with_pending() != 0 || !java_trim(&text).is_empty() {
                self.w(ctx).append(&text);
            }
        }
    }

    /// jdt.ls `JavaDoc2MarkdownConverter.processA`
    fn process_a(&mut self, element: NodeId, ctx: usize) {
        let doc = self.doc;
        if doc.has_attr(element, "href") {
            let href = doc.attr(element, "href");
            if java_trim(&href).is_empty() || href.starts_with("eclipse-javadoc:") || href.starts_with('#') {
                self.process_html_tree(ctx, element);
                return;
            }
            let mut use_href = cleanup_url(&href);
            if self.ctxs[ctx].out.is_pre_formatted() {
                if let Some(slash) = use_href.rfind('/') {
                    if let Some(hash) = use_href[slash..].find('#').map(|h| h + slash) {
                        if slash + 1 == hash {
                            use_href = format!("{}{}", &use_href[..slash], &use_href[hash..]);
                        }
                    }
                }
                self.w(ctx).append(&use_href);
            } else {
                self.push_state(element);
                let text_nodes = self.process_text_nodes_string(element);
                let text = java_trim(&text_nodes).to_string();
                let title = if doc.has_attr(element, "title") { Some(doc.attr(element, "title")) } else { None };
                let parent_is_heading = doc
                    .parent_element(element)
                    .map(|p| HEADING_NODES.contains(&doc.tag_name(p).to_lowercase().as_str()))
                    .unwrap_or(false);
                if !text.is_empty() || !use_href.contains('#') || (!parent_is_heading && use_href != "#") {
                    let out = self.w(ctx);
                    if href == text && title.as_deref().map(|t| t.is_empty()).unwrap_or(true) {
                        out.append_char('<');
                        out.append(&use_href);
                        out.append_char('>');
                    } else if !use_href.starts_with("javascript:") {
                        out.append_char('[');
                        out.append(&text);
                        out.append_char(']');
                        out.append_char('(');
                        out.append(&use_href);
                        if let Some(t) = &title {
                            out.append(" \"");
                            out.append(&t.replace('\n', EOL_IN_TITLE_ATTRIBUTE).replace('"', "\\\""));
                            out.append_char('"');
                        }
                        out.append(")");
                    } else if href == text {
                        out.append(&use_href);
                    } else {
                        out.append(&text);
                    }
                }
                self.pop_state();
            }
        } else {
            self.process_text_nodes_into(element, ctx, None, None);
        }
    }

    fn process_aside(&mut self, element: NodeId, ctx: usize) {
        if self.is_first_child(element) {
            self.w(ctx).line();
        }
        self.w(ctx).push_prefix();
        self.w(ctx).add_prefix("| ");
        self.process_html_tree(ctx, element);
        self.w(ctx).line();
        self.w(ctx).pop_prefix();
    }

    fn process_block_quote(&mut self, element: NodeId, ctx: usize) {
        if self.is_first_child(element) {
            self.w(ctx).line();
        }
        self.w(ctx).push_prefix();
        self.w(ctx).add_prefix("> ");
        self.process_html_tree(ctx, element);
        self.w(ctx).line();
        self.w(ctx).pop_prefix();
    }

    fn process_br(&mut self, ctx: usize) {
        let out = self.w(ctx);
        if out.is_pre_formatted() {
            out.append_char('\n');
        } else {
            let options = out.get_options();
            out.set_options(options & !(F_TRIM_TRAILING_WHITESPACE | F_COLLAPSE_WHITESPACE));
            if out.get_pending_eol() == 0 {
                out.append_repeat(' ', 2).line();
            } else if out.get_pending_eol() == 1 {
                let s = out.to_string_raw();
                if !s.ends_with("<br />") {
                    out.blank_line();
                } else {
                    out.append("<br />").blank_line();
                }
            } else {
                out.append("<br />").blank_line();
            }
            out.set_options(options);
        }
    }

    fn process_code(&mut self, element: NodeId, ctx: usize) {
        let text = self.doc.own_text(element);
        let n = get_max_repeated_chars(&text, '`', 1);
        let ticks = "`".repeat(n);
        let old = self.inline_code;
        self.inline_code = true;
        self.process_text_nodes_into(element, ctx, Some(&ticks), Some(&ticks));
        self.inline_code = old;
    }

    /// jdt.ls `processTt`
    fn process_tt(&mut self, element: NodeId, ctx: usize) {
        let text = self.doc.own_text(element);
        let mut max_count = 0;
        let mut current = 0;
        for c in text.chars() {
            if c == '`' {
                current += 1;
                if current > max_count {
                    max_count = current;
                }
            } else {
                current = 0;
            }
        }
        let n = if max_count > 0 { max_count + 1 } else { 1 };
        let ticks = "`".repeat(n);
        let old = self.inline_code;
        self.inline_code = true;
        self.process_text_nodes_into(element, ctx, Some(&ticks), Some(&ticks));
        self.inline_code = old;
    }

    /// jdt.ls `processDfn`
    fn process_dfn(&mut self, element: NodeId, ctx: usize) {
        let text = self.doc.text(element);
        let out = self.w(ctx);
        if text.chars().all(java_whitespace) {
            out.append(&text);
            return;
        }
        out.append("_").append(&text).append("_");
    }

    fn process_wrap_emphasis(&mut self, element: NodeId, ctx: usize, wrap: &str) {
        if self.ctxs[ctx].out.is_pre_formatted() {
            self.wrap_text_nodes(element, ctx, "", false);
        } else {
            let need = self.doc.next_element_sibling(element).is_some();
            self.wrap_text_nodes(element, ctx, wrap, need);
        }
    }

    fn process_sub_sup(&mut self, element: NodeId, ctx: usize, wrap: &str) {
        if self.ctxs[ctx].out.is_pre_formatted() {
            self.wrap_text_nodes(element, ctx, "", false);
        } else {
            self.wrap_text_nodes(element, ctx, wrap, false);
        }
    }

    fn process_div(&mut self, element: NodeId, ctx: usize) {
        if !self.is_first_child(element) {
            let out = self.w(ctx);
            let pending_eol = out.get_pending_eol();
            if pending_eol == 0 {
                let pending_space = out.get_pending_space();
                out.line_with_trailing_spaces(2usize.saturating_sub(pending_space));
            } else if pending_eol == 1 {
                let line_count = out.get_line_count_with_pending();
                if line_count > 0 {
                    let content = out.get_line_content(line_count - 1);
                    let pending_space = count_trailing_space_tab(&content);
                    if pending_space < 2 {
                        out.remove_lines(line_count - 1, line_count);
                        out.append(&content);
                        out.line_with_trailing_spaces(2 - pending_space);
                    }
                }
            }
        }
        self.process_html_tree(ctx, element);
        if !self.is_last_child(element) {
            self.w(ctx).line();
        }
    }

    /// jdt.ls `processDl`
    fn process_dl(&mut self, element: NodeId, ctx: usize) {
        self.push_state(element);
        let mut last_was_definition = true;
        let mut first_item = true;
        while let Some(item) = self.next() {
            match self.doc.node_name(item).to_lowercase().as_str() {
                "dt" => {
                    self.w(ctx).blank_line_if(last_was_definition).line_if(!first_item);
                    self.process_text_nodes_into(item, ctx, None, None);
                    self.w(ctx).line_with_trailing_spaces(2);
                    last_was_definition = false;
                    first_item = false;
                }
                "dd" => {
                    self.handle_definition(item, ctx);
                    last_was_definition = true;
                    first_item = false;
                }
                _ => {}
            }
        }
        self.pop_state();
    }

    /// jdt.ls `handleDefinition`
    fn handle_definition(&mut self, item: NodeId, ctx: usize) {
        self.push_state(item);
        let options = self.w(ctx).get_options();
        let children = self.doc.element_children(item);
        let mut first_is_para = false;
        if let Some(&first) = children.first() {
            if self.doc.tag_name(first).eq_ignore_ascii_case("p") {
                self.w(ctx).blank_line();
                first_is_para = true;
            }
        }
        let child_prefix = " ".repeat(DEFINITION_MARKER_SPACES + 1);
        let out = self.w(ctx);
        out.append_repeat(' ', DEFINITION_MARKER_SPACES);
        out.push_prefix();
        out.add_prefix_after(&child_prefix, true);
        out.set_options(options);
        if first_is_para {
            self.process_html_tree(ctx, item);
        } else {
            self.process_text_nodes_into(item, ctx, None, None);
        }
        self.w(ctx).line_with_trailing_spaces(2);
        self.w(ctx).pop_prefix();
        self.pop_state();
    }

    fn process_emoji(&mut self, element: NodeId, ctx: usize) {
        if self.doc.has_attr(element, "alias") {
            let alias = self.doc.attr(element, "alias");
            self.w(ctx).append_char(':').append(&alias).append_char(':');
            return;
        }
        if self.doc.has_attr(element, "fallback-src") {
            if let Some(sc) = emoji_from_uri(&self.doc.attr(element, "fallback-src")) {
                let sc = sc.unwrap_or_else(|| "null".into());
                self.w(ctx).append_char(':').append(&sc).append_char(':');
                return;
            }
        }
        self.process_html_tree(ctx, element);
    }

    fn process_heading(&mut self, element: NodeId, ctx: usize) {
        let level = match self.doc.node_name(element).to_lowercase().as_str() {
            "h1" => 1,
            "h2" => 2,
            "h3" => 3,
            "h4" => 4,
            "h5" => 5,
            _ => 6,
        };
        let heading = java_trim(&self.process_text_nodes_string(element)).to_string();
        if !heading.is_empty() {
            let out = self.w(ctx);
            out.blank_line();
            if level <= 2 {
                out.append(&heading);
                let n = heading.encode_utf16().count().max(3);
                out.line().append_repeat(if level == 1 { '=' } else { '-' }, n);
            } else {
                out.append_repeat('#', level).append_char(' ');
                out.append(&heading);
            }
            out.blank_line();
        }
    }

    fn process_img(&mut self, element: NodeId, ctx: usize) {
        let doc = self.doc;
        if !doc.has_attr(element, "src") {
            return;
        }
        let src = doc.attr(element, "src");
        let mut emoji = emoji_from_uri(&src);
        if emoji.is_none() && doc.has_attr(element, "alt") {
            let emoji_alt = doc.attr(element, "alt");
            if let Some(rest) = emoji_alt.strip_prefix("emoji ") {
                if let Some(pos) = rest.find(':') {
                    if pos + 6 > 0 {
                        let shortcut = &rest[pos + 1..];
                        if emoji::EMOJI_SHORTCUTS.binary_search(&shortcut).is_ok() {
                            emoji = Some(Some(shortcut.to_string()));
                        }
                    }
                }
            }
        }
        if let Some(Some(shortcut)) = emoji {
            self.w(ctx).append_char(':').append(&shortcut).append_char(':');
            return;
        }
        let alt = if doc.has_attr(element, "alt") {
            Some(java_trim(&doc.attr(element, "alt")).replace('[', "\\[").replace(']', "\\]"))
        } else {
            None
        }
        .filter(|a| !a.is_empty());
        let title = if doc.has_attr(element, "title") {
            Some(doc.attr(element, "title").replace('\n', EOL_IN_TITLE_ATTRIBUTE).replace('"', "\\\""))
        } else {
            None
        }
        .filter(|t| !t.is_empty());
        let pos = src.find('?');
        let eol = pos.and_then(|p| src[p..].find("%0A").map(|e| e + p));
        let is_multi_line = matches!(pos, Some(p) if p > 0) && matches!(eol, Some(e) if e > 0);
        let out = self.w(ctx);
        out.append("![");
        if let Some(a) = &alt {
            out.append(a);
        }
        out.append_char(']').append_char('(');
        if is_multi_line {
            let p = pos.unwrap();
            out.append(&src[..p + 1]);
            let decoded = url_decode(&src[p + 1..].replace('+', "%2B"));
            out.line().append(&decoded);
        } else {
            out.append(&src);
        }
        if let Some(t) = &title {
            out.append(" \"").append(t).append_char('"');
        }
        out.append(")");
    }

    fn process_input(&mut self, element: NodeId, ctx: usize) {
        let mut is_item_paragraph = false;
        let first = self.doc.first_element_sibling(element);
        if first.is_none() || first == Some(element) {
            is_item_paragraph = self.parent_tag_name(element).eq_ignore_ascii_case("li");
        }
        if is_item_paragraph && self.doc.has_attr(element, "type") && self.doc.attr(element, "type").eq_ignore_ascii_case("checkbox") {
            if self.doc.has_attr(element, "checked") {
                self.w(ctx).append("[x] ");
            } else {
                self.w(ctx).append("[ ] ");
            }
            return;
        }
        self.process_html_tree(ctx, element);
    }

    fn process_wrapped(&mut self, node: NodeId, ctx: usize, is_block: Option<bool>, escape_markdown: bool) {
        let block = match is_block {
            None => self.doc.is_element(node) && self.doc.is_block(node),
            Some(b) => b,
        };
        if self.doc.is_element(node) && block {
            let s = self.doc.outer_html(node);
            let pos = s.find('>').map(|p| p + 1).unwrap_or(s.len());
            let lines = is_block.is_some();
            self.w(ctx).line_if(lines).append(&s[..pos]).line_if(lines);
            self.process_html_tree(ctx, node);
            let end_pos = s.rfind('<').unwrap_or(0);
            self.w(ctx).line_if(lines).append(&s[end_pos..]).line_if(lines);
        } else if escape_markdown {
            self.append_outer_html(node, ctx);
        } else {
            let s = self.doc.outer_html(node);
            self.w(ctx).append(&s);
        }
    }

    fn append_outer_html(&mut self, node: NodeId, ctx: usize) {
        let text = self.doc.outer_html(node);
        let head = text.find('>');
        let tail = text.rfind("</");
        match (head, tail) {
            (Some(h), Some(t)) => {
                self.w(ctx).append(&text[..h + 1]);
                let children = self.doc.child_nodes(node);
                if !children.is_empty() {
                    for c in children {
                        self.append_outer_html(c, ctx);
                    }
                } else {
                    let esc = escape_special_chars(&text[h + 1..t]);
                    self.w(ctx).append(&esc);
                }
                self.w(ctx).append(&text[t..]);
            }
            (None, _) => {
                let esc = escape_special_chars(&text);
                self.w(ctx).append(&esc);
            }
            _ => {
                self.w(ctx).append(&text);
            }
        }
    }

    // ── lists ──

    fn have_list_item_ancestor(&self, node: NodeId) -> bool {
        let mut p = self.doc.parent(node);
        while let Some(pp) = p {
            if self.doc.node_name(pp).to_lowercase() == "li" {
                return true;
            }
            p = self.doc.parent(pp);
        }
        false
    }

    fn handle_list_item(&mut self, ctx: usize, item: NodeId, numbered: bool, item_count: &mut i64) {
        self.push_state(item);
        *item_count += 1;
        let item_prefix = if numbered { format!("{}. ", item_count) } else { "* ".to_string() };
        let child_prefix = " ".repeat(item_prefix.chars().count());
        let out = self.w(ctx);
        out.line().append(&item_prefix);
        out.push_prefix();
        out.add_prefix_after(&child_prefix, true);
        let offset = out.offset_with_pending();
        self.process_html_tree(ctx, item);
        let out = self.w(ctx);
        if offset == out.offset_with_pending() {
            let options = out.get_options();
            out.set_options(options & !(F_TRIM_TRAILING_WHITESPACE | F_TRIM_LEADING_WHITESPACE));
            out.line();
            out.set_options(options);
        } else {
            out.line();
        }
        out.pop_prefix();
        self.pop_state();
    }

    fn handle_list(&mut self, ctx: usize, element: NodeId, is_numbered: bool, is_fake_list: bool, is_nested: bool) {
        if !is_fake_list {
            self.push_state(element);
            if !is_nested && !self.have_list_item_ancestor(element) && !self.is_first_child(element) {
                self.w(ctx).blank_line();
            }
        }
        if let Some(prev) = self.doc.previous_element_sibling(element) {
            let tag = self.doc.tag_name(prev).to_uppercase();
            if tag == self.doc.tag_name(element).to_uppercase() && (tag == "UL" || tag == "OL") {
                self.w(ctx).line().append("<!-- -->").blank_line();
            }
        }
        let mut item_count: i64 = 0;
        if is_numbered && self.doc.has_attr(element, "start") {
            if let Ok(i) = self.doc.attr(element, "start").parse::<i64>() {
                item_count = i - 1;
            }
        }
        let mut item = Some(element);
        let mut had_list_item = false;
        while let Some(it) = item {
            let name = self.doc.node_name(it).to_lowercase();
            match name.as_str() {
                "li" => {
                    self.handle_list_item(ctx, it, is_numbered, &mut item_count);
                    had_list_item = true;
                }
                "p" => {
                    if self.doc.child_node_size(it) > 0 {
                        self.handle_list_item(ctx, it, is_numbered, &mut item_count);
                    }
                }
                "ol" | "ul" => {
                    let is_numbered_list = name == "ol";
                    if it != element && self.doc.child_node_size(it) > 0 {
                        if had_list_item {
                            let item_prefix =
                                if is_numbered { format!("{}. ", item_count) } else { "* ".to_string() };
                            let child_prefix = " ".repeat(item_prefix.chars().count());
                            self.w(ctx).push_prefix();
                            self.w(ctx).add_prefix_after(&child_prefix, true);
                        }
                        self.handle_list(ctx, it, is_numbered_list, false, true);
                        if had_list_item {
                            self.w(ctx).pop_prefix();
                        }
                    }
                }
                _ => self.render(it, ctx),
            }
            item = self.next();
        }
        if !is_nested && self.doc.next_element_sibling(element).is_some() {
            self.w(ctx).blank_line();
        }
        if !is_fake_list {
            self.pop_state();
        }
    }

    fn process_p(&mut self, element: NodeId, ctx: usize) {
        let mut is_item = false;
        let mut is_def = false;
        let first = self.doc.first_element_sibling(element);
        if first.is_none() || first == Some(element) {
            let tag = self.parent_tag_name(element);
            is_item = tag.eq_ignore_ascii_case("li");
            is_def = tag.eq_ignore_ascii_case("dd");
        }
        let first_child = self.is_first_child(element);
        self.w(ctx).blank_line_if(!(is_item || is_def || first_child));
        if self.doc.child_node_size(element) == 0 {
            self.w(ctx).append("<br />").blank_line();
        } else {
            self.process_text_nodes_into(element, ctx, None, None);
        }
        self.w(ctx).line();
        if is_item || is_def {
            self.tail_blank_line(ctx);
        }
    }

    fn process_pre(&mut self, element: NodeId, ctx: usize) {
        self.push_state(element);
        let mut had_code = false;
        let mut class_name = String::new();
        let pre = self.new_sub_context(ctx);
        let opts = self.ctxs[ctx].out.get_options() & !(F_COLLAPSE_WHITESPACE | F_TRIM_TRAILING_WHITESPACE);
        self.w(pre).set_options(opts);
        self.w(pre).open_pre_formatted();
        while let Some(next) = self.next() {
            let name = self.doc.node_name(next).to_lowercase();
            if name == "code" || name == "tt" {
                had_code = true;
                let old = self.inline_code;
                self.inline_code = true;
                self.process_html_tree(pre, next);
                self.inline_code = old;
                if class_name.is_empty() {
                    let cn = self.doc.class_name(next);
                    class_name = cn.strip_prefix("language-").map(|s| s.to_string()).unwrap_or(cn);
                }
            } else if name == "br" {
                self.w(pre).append("\n");
            } else if name == "#text" {
                let t = self.doc.whole_text(next).to_string();
                self.w(pre).append(&t);
            } else {
                self.process_html_tree(pre, next);
            }
        }
        self.w(pre).close_pre_formatted();
        let mut pre_out = self.drop_sub_context();
        let text = pre_out.to_string_with(i32::MAX as i64, 2);
        let ticks = "`".repeat(get_max_repeated_chars(&text, '`', 3));
        let body = if text.is_empty() { "\n".to_string() } else { text.clone() };
        if !class_name.is_empty() || java_trim(&text).is_empty() || !had_code {
            let out = self.w(ctx);
            out.blank_line().append(&ticks);
            if !class_name.is_empty() {
                out.append(&class_name);
            }
            out.line();
            out.open_pre_formatted();
            out.append(&body);
            out.close_pre_formatted();
            out.line().append(&ticks).line();
            self.tail_blank_line(ctx);
        } else {
            let out = self.w(ctx);
            out.blank_line();
            out.push_prefix();
            out.add_prefix(CODE_INDENT);
            out.open_pre_formatted();
            out.append(&body);
            out.close_pre_formatted();
            out.line();
            self.tail_blank_line(ctx);
            self.w(ctx).pop_prefix();
        }
        self.pop_state();
    }

    // ── tables ──

    fn process_table(&mut self, table_el: NodeId, ctx: usize) {
        let old_table = self.table.take();
        self.push_state(table_el);
        self.table = Some(MarkdownTable::new());
        self.table_suppress_columns = false;
        while let Some(item) = self.next() {
            match self.doc.node_name(item).to_lowercase().as_str() {
                "caption" => {
                    let c = java_trim(&self.process_text_nodes_string(item)).to_string();
                    self.table.as_mut().unwrap().set_caption(&c);
                }
                "tbody" => {
                    self.table.as_mut().unwrap().set_header(false);
                    self.handle_table_section(item);
                }
                "thead" => {
                    self.table.as_mut().unwrap().set_header(true);
                    self.handle_table_section(item);
                }
                "tr" => {
                    let children = self.doc.element_children(item);
                    let h = !children.is_empty() && self.doc.tag_name(children[0]).eq_ignore_ascii_case("th");
                    self.table.as_mut().unwrap().set_header(h);
                    self.handle_table_row(item);
                }
                _ => {}
            }
        }
        let mut table = self.table.take().unwrap();
        table.finalize_table();
        if table.max_columns() > 0 {
            self.w(ctx).blank_line();
            table.append_table(&mut self.ctxs[ctx].out);
            self.tail_blank_line(ctx);
        }
        self.table = old_table;
        self.pop_state();
    }

    fn handle_table_section(&mut self, element: NodeId) {
        self.push_state(element);
        while let Some(node) = self.next() {
            if self.doc.node_name(node).eq_ignore_ascii_case("tr") {
                let children = self.doc.element_children(node);
                let was_heading = self.table.as_ref().unwrap().get_header();
                if !children.is_empty() && self.doc.tag_name(children[0]).eq_ignore_ascii_case("th") {
                    self.table.as_mut().unwrap().set_header(true);
                }
                let t = self.table.as_ref().unwrap();
                if t.get_header() && t.body_row_count() > 0 {
                    self.table_suppress_columns = true;
                }
                self.handle_table_row(node);
                self.table_suppress_columns = false;
                self.table.as_mut().unwrap().set_header(was_heading);
            }
        }
        self.pop_state();
    }

    fn handle_table_row(&mut self, element: NodeId) {
        self.push_state(element);
        while let Some(node) = self.next() {
            let n = self.doc.node_name(node).to_lowercase();
            if n == "th" || n == "td" {
                self.handle_table_cell(node);
            }
        }
        self.table.as_mut().unwrap().next_row();
        self.pop_state();
    }

    fn handle_table_cell(&mut self, element: NodeId) {
        let raw = self.process_text_nodes_string(element);
        let cell_text = collapse_newlines(java_trim(&raw));
        let doc = self.doc;
        let mut col_span = 1;
        let mut row_span = 1;
        let mut alignment: Option<Align> = None;
        if doc.has_attr(element, "colSpan") {
            if let Ok(v) = doc.attr(element, "colSpan").parse::<i32>() {
                col_span = v;
            }
        }
        if doc.has_attr(element, "rowSpan") {
            if let Ok(v) = doc.attr(element, "rowSpan").parse::<i32>() {
                row_span = v;
            }
        }
        if doc.has_attr(element, "align") {
            alignment = Some(Align::parse(&doc.attr(element, "align")));
        } else {
            let classes = doc.class_names(element);
            if !classes.is_empty() {
                for c in &classes {
                    let a = match c.as_str() {
                        "text-left" => Some(Align::Left),
                        "text-center" => Some(Align::Center),
                        "text-right" => Some(Align::Right),
                        _ => None,
                    };
                    if a.is_some() {
                        alignment = a;
                        break;
                    }
                }
                if alignment.is_none() {
                    'pat: for (word, a) in [("left", Align::Left), ("center", Align::Center), ("right", Align::Right)] {
                        for c in &classes {
                            if has_word(c, word) {
                                alignment = Some(a);
                                break 'pat;
                            }
                        }
                    }
                }
            }
        }
        if !self.table_suppress_columns {
            let text = cell_text.replace('\n', " ");
            let cell = Cell::new(&text, row_span.max(0) as usize, col_span.max(0) as usize, alignment);
            self.table.as_mut().unwrap().add_cell(cell);
        }
    }

    fn process_span(&mut self, element: NodeId, ctx: usize) {
        if self.doc.has_attr(element, "style") && self.doc.attr(element, "style") == "mso-list:Ignore" {
            let text = self.process_text_nodes_string(element);
            let t = text.as_str();
            let ws = r"[ \t\n\x0B\x0C\r]*";
            let out = self.w(ctx);
            if regex_match(&format!(r"^([0-9]+)\.{ws}$"), t).is_some() {
                out.append(t).append_char(' ');
            } else if let Some(n) = regex_match(&format!(r"^([0-9]+)\){ws}$"), t) {
                out.append(&n).append(". ");
            } else if let Some(n) = regex_match(
                &format!(r"^((?:(?:{ROMAN})|(?:{ROMAN_LC})|[a-z]+|[A-Z]+))\.{ws}$"),
                t,
            ) {
                out.append(&convert_numeric(&n)).append(". ");
            } else if let Some(n) = regex_match(&format!(r"^((?:[a-z]+|[A-Z]+))\){ws}$"), t) {
                out.append(&convert_numeric(&n)).append(". ");
            } else if regex_match(&format!(r"^([\u{{00B7}}]){ws}$"), t).is_some() {
                out.append("* ");
            } else {
                out.append("* ").append(t);
            }
            return;
        }
        self.process_html_tree(ctx, element);
    }
}

/// `EmojiShortcuts.getEmojiFromURI`: `Some(shortcut)` when an emoji matches (`shortcut` may be null).
fn emoji_from_uri(uri: &str) -> Option<Option<String>> {
    // new File(uri).getName(), then strip from ".png"
    let mut u = uri.trim_end_matches('/').to_string();
    while u.contains("//") {
        u = u.replace("//", "/");
    }
    let name = u.rsplit('/').next().unwrap_or("");
    let name = match name.find(".png") {
        Some(p) => &name[..p],
        None => name,
    };
    emoji::EMOJI_URIS
        .binary_search_by(|(k, _)| k.cmp(&name))
        .ok()
        .map(|i| emoji::EMOJI_URIS[i].1)
        .map(|s| if s.is_empty() { None } else { Some(s.to_string()) })
}

const ROMAN: &str = "M{0,3}(?:CM|DC{0,3}|CD|C{1,3})?(?:XC|LX{0,3}|XL|X{1,3})?(?:IX|VI{0,3}|IV|I{1,3})?";
const ROMAN_LC: &str = "m{0,3}(?:cm|dc{0,3}|cd|c{1,3})?(?:xc|lx{0,3}|xl|x{1,3})?(?:ix|vi{0,3}|iv|i{1,3})?";

/// `HtmlConverterCoreNodeRenderer.convertNumeric`
fn convert_numeric(text: &str) -> String {
    let text = java_trim(text);
    let full = |p: &str| regex::Regex::new(&format!("^(?:{p})$")).unwrap().is_match(text);
    if full("(?:X{1,3})?(?:IX|VI{0,3}|IV|I{1,3})?") || full("(?:x{1,3})?(?:ix|vi{0,3}|iv|i{1,3})?") {
        // RomanNumeral(text).toInt()
        let val = |c: char| match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => 0,
        };
        let r: Vec<char> = text.to_uppercase().chars().collect();
        let mut i = 0;
        let mut arabic = 0;
        while i < r.len() {
            let n = val(r[i]);
            i += 1;
            if i == r.len() {
                arabic += n;
            } else {
                let next = val(r[i]);
                if next > n {
                    arabic += next - n;
                    i += 1;
                } else {
                    arabic += n;
                }
            }
        }
        return arabic.max(1).to_string();
    } else if full("[a-z]+|[A-Z]+") {
        let mut value: i64 = 0;
        for c in text.to_uppercase().chars() {
            value *= 26;
            value += c as i64 - 'A' as i64 + 1;
        }
        return value.to_string();
    }
    "1".into()
}

fn regex_match(pat: &str, text: &str) -> Option<String> {
    let re = regex::Regex::new(pat).ok()?;
    re.captures(text).map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
}

fn has_word(s: &str, word: &str) -> bool {
    // Pattern "\bword\b" find
    let re = regex::Regex::new(&format!(r"\b{}\b", word)).unwrap();
    re.is_match(s)
}

/// `replaceAll("\\s*\n\\s*", " ")` (Java `\s` = [ \t\n\x0B\f\r])
fn collapse_newlines(s: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"[ \t\n\x0B\x0C\r]*\n[ \t\n\x0B\x0C\r]*").unwrap());
    if !s.contains('\n') {
        return s.to_string();
    }
    RE.replace_all(s, " ").into_owned()
}

/// `escapeSpecialChars`
pub fn escape_special_chars(text: &str) -> String {
    let mut o = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '*' => o.push_str("\\*"),
            '~' => o.push_str("\\~"),
            '^' => o.push_str("\\^"),
            '&' => o.push_str("\\&"),
            '<' => o.push_str("\\<"),
            '>' => o.push_str("\\>"),
            '[' => o.push_str("\\["),
            ']' => o.push_str("\\]"),
            '|' => o.push_str("\\|"),
            '`' => o.push_str("\\`"),
            '\u{00A0}' => o.push_str(NBSP_TEXT),
            _ => o.push(c),
        }
    }
    o
}

/// `JDTUtils.cleanupURL`
pub fn cleanup_url(url: &str) -> String {
    if url.contains('(') {
        return url.replace('(', "%28");
    }
    if url.contains(')') {
        return url.replace(')', "%29");
    }
    url.to_string()
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
