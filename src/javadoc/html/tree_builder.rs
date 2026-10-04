//! Port of jsoup 1.19.1 `HtmlTreeBuilder` / `HtmlTreeBuilderState`.

use super::tokeniser::{State as TState, Tag, Token, Tokeniser};
use super::{is_known_tag, Attr, Document, Kind, NodeId, Ns, ROOT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    InSelectInTable,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

const TAGS_SEARCH_IN_SCOPE: &[&str] = &["applet", "caption", "html", "marquee", "object", "table", "td", "template", "th"];
const TAG_SEARCH_IN_SCOPE_MATH: &[&str] = &["annotation-xml", "mi", "mn", "mo", "ms", "mtext"];
const TAG_SEARCH_IN_SCOPE_SVG: &[&str] = &["desc", "foreignObject", "title"];
const TAG_SEARCH_LIST: &[&str] = &["ol", "ul"];
const TAG_SEARCH_BUTTON: &[&str] = &["button"];
const TAG_SEARCH_TABLE_SCOPE: &[&str] = &["html", "table"];
const TAG_SEARCH_SELECT_SCOPE: &[&str] = &["optgroup", "option"];
const TAG_SEARCH_END_TAGS: &[&str] = &["dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc"];
const TAG_THOROUGH_SEARCH_END_TAGS: &[&str] = &[
    "caption", "colgroup", "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc", "tbody", "td", "tfoot",
    "th", "thead", "tr",
];
const TAG_SEARCH_SPECIAL: &[&str] = &[
    "address", "applet", "area", "article", "aside", "base", "basefont", "bgsound", "blockquote", "body", "br",
    "button", "caption", "center", "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "embed", "fieldset",
    "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head",
    "header", "hgroup", "hr", "html", "iframe", "img", "input", "keygen", "li", "link", "listing", "main", "marquee",
    "menu", "meta", "nav", "noembed", "noframes", "noscript", "object", "ol", "p", "param", "plaintext", "pre",
    "script", "search", "section", "select", "source", "style", "summary", "table", "tbody", "td", "template",
    "textarea", "tfoot", "th", "thead", "title", "tr", "track", "ul", "wbr", "xmp",
];
const TAG_SEARCH_SPECIAL_MATH: &[&str] = &["annotation-xml", "mi", "mn", "mo", "ms", "mtext"];
const TAG_MATHML_TEXT_INTEGRATION: &[&str] = &["mi", "mn", "mo", "ms", "mtext"];
const TAG_SVG_HTML_INTEGRATION: &[&str] = &["desc", "foreignObject", "title"];
const MAX_SCOPE_SEARCH_DEPTH: usize = 100;
const MAX_QUEUE_DEPTH: usize = 256;
const MAX_USED_FORMATTING_ELEMENTS: usize = 12;

// Constants
const IN_HEAD_EMPTY: &[&str] = &["base", "basefont", "bgsound", "command", "link"];
const IN_HEAD_RAW: &[&str] = &["noframes", "style"];
const IN_HEAD_END: &[&str] = &["body", "br", "html"];
const AFTER_HEAD_BODY: &[&str] = &["body", "br", "html"];
const BEFORE_HTML_TO_HEAD: &[&str] = &["body", "br", "head", "html"];
const IN_HEAD_NOSCRIPT_HEAD: &[&str] = &["basefont", "bgsound", "link", "meta", "noframes", "style"];
const IN_BODY_START_TO_HEAD: &[&str] =
    &["base", "basefont", "bgsound", "command", "link", "meta", "noframes", "script", "style", "template", "title"];
const IN_BODY_START_P_CLOSERS: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dir", "div", "dl", "fieldset", "figcaption",
    "figure", "footer", "header", "hgroup", "menu", "nav", "ol", "p", "section", "summary", "ul",
];
const HEADINGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];
const IN_BODY_START_LI_BREAKERS: &[&str] = &["address", "div", "p"];
const DD_DT: &[&str] = &["dd", "dt"];
const IN_BODY_START_APPLETS: &[&str] = &["applet", "marquee", "object"];
const IN_BODY_START_MEDIA: &[&str] = &["param", "source", "track"];
const IN_BODY_START_DROP: &[&str] =
    &["caption", "col", "colgroup", "frame", "head", "tbody", "td", "tfoot", "th", "thead", "tr"];
const IN_BODY_END_CLOSERS: &[&str] = &[
    "address", "article", "aside", "blockquote", "button", "center", "details", "dir", "div", "dl", "fieldset",
    "figcaption", "figure", "footer", "header", "hgroup", "listing", "menu", "nav", "ol", "pre", "section", "summary",
    "ul",
];
const IN_BODY_END_ADOPTION_FORMATTERS: &[&str] =
    &["a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u"];
const IN_TABLE_TO_BODY: &[&str] = &["tbody", "tfoot", "thead"];
const IN_TABLE_ADD_BODY: &[&str] = &["td", "th", "tr"];
const IN_TABLE_TO_HEAD: &[&str] = &["script", "style", "template"];
const IN_CELL_NAMES: &[&str] = &["td", "th"];
const IN_CELL_BODY: &[&str] = &["body", "caption", "col", "colgroup", "html"];
const IN_CELL_TABLE: &[&str] = &["table", "tbody", "tfoot", "thead", "tr"];
const IN_CELL_COL: &[&str] = &["caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr"];
const IN_TABLE_END_ERR: &[&str] =
    &["body", "caption", "col", "colgroup", "html", "tbody", "td", "tfoot", "th", "thead", "tr"];
const IN_TABLE_FOSTER: &[&str] = &["table", "tbody", "tfoot", "thead", "tr"];
const IN_TABLE_BODY_EXIT: &[&str] = &["caption", "col", "colgroup", "tbody", "tfoot", "thead"];
const IN_TABLE_BODY_END_IGNORE: &[&str] = &["body", "caption", "col", "colgroup", "html", "td", "th", "tr"];
const IN_ROW_MISSING: &[&str] = &["caption", "col", "colgroup", "tbody", "tfoot", "thead", "tr"];
const IN_ROW_IGNORE: &[&str] = &["body", "caption", "col", "colgroup", "html", "td", "th"];
const IN_SELECT_END: &[&str] = &["input", "keygen", "textarea"];
const IN_SELECT_TABLE_END: &[&str] = &["caption", "table", "tbody", "td", "tfoot", "th", "thead", "tr"];
const IN_TABLE_END_IGNORE: &[&str] = &["tbody", "tfoot", "thead"];
const IN_HEAD_NOSCRIPT_IGNORE: &[&str] = &["head", "noscript"];
const IN_CAPTION_IGNORE: &[&str] = &["body", "col", "colgroup", "html", "tbody", "td", "tfoot", "th", "thead", "tr"];
const IN_TEMPLATE_TO_HEAD: &[&str] =
    &["base", "basefont", "bgsound", "link", "meta", "noframes", "script", "style", "template", "title"];
const IN_TEMPLATE_TO_TABLE: &[&str] = &["caption", "colgroup", "tbody", "tfoot", "thead"];
const IN_FOREIGN_TO_HTML: &[&str] = &[
    "b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt", "em", "embed", "h1", "h2", "h3",
    "h4", "h5", "h6", "head", "hr", "i", "img", "li", "listing", "menu", "meta", "nobr", "ol", "p", "pre", "ruby", "s",
    "small", "span", "strike", "strong", "sub", "sup", "table", "tt", "u", "ul", "var",
];

fn in_list(name: &str, list: &[&str]) -> bool {
    list.contains(&name)
}

/// `Jsoup.parse(html)`
pub fn parse(html: &str) -> Document {
    let mut tb = TreeBuilder::new(html);
    tb.run();
    tb.doc
}

struct TreeBuilder {
    doc: Document,
    tokeniser: Tokeniser,
    stack: Vec<NodeId>,
    state: Mode,
    original_state: Mode,
    head_element: Option<NodeId>,
    form_element: Option<NodeId>,
    formatting: Vec<Option<NodeId>>,
    tmpl_insert_mode: Vec<Mode>,
    pending_table_chars: Vec<Token>,
    frameset_ok: bool,
    foster_inserts: bool,
}

impl TreeBuilder {
    fn new(html: &str) -> Self {
        TreeBuilder {
            doc: Document::new(),
            tokeniser: Tokeniser::new(html),
            stack: Vec::new(),
            state: Mode::Initial,
            original_state: Mode::Initial,
            head_element: None,
            form_element: None,
            formatting: Vec::new(),
            tmpl_insert_mode: Vec::new(),
            pending_table_chars: Vec::new(),
            frameset_ok: true,
            foster_inserts: false,
        }
    }

    fn run(&mut self) {
        loop {
            let t = self.tokeniser.read();
            let eof = matches!(t, Token::Eof);
            self.process(&t);
            if eof {
                break;
            }
        }
    }

    // ── element helpers ──

    fn name(&self, el: NodeId) -> &str {
        self.doc.normal_name(el)
    }

    fn ns(&self, el: NodeId) -> Ns {
        self.doc.ns(el)
    }

    fn is_html(&self, el: NodeId) -> bool {
        self.doc.kind(el) == Kind::Element && self.ns(el) == Ns::Html
    }

    fn element_is(&self, el: NodeId, name: &str) -> bool {
        self.name(el) == name && self.ns(el) == Ns::Html
    }

    fn current(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(ROOT)
    }

    fn current_is(&self, name: &str) -> bool {
        !self.stack.is_empty() && self.element_is(self.current(), name)
    }

    fn pop(&mut self) -> Option<NodeId> {
        self.stack.pop()
    }

    fn push(&mut self, el: NodeId) {
        self.stack.push(el);
    }

    // ── processing ──

    fn process(&mut self, t: &Token) -> bool {
        if self.use_current_or_foreign_insert(t) {
            self.process_in(self.state, t)
        } else {
            self.foreign_content(t)
        }
    }

    fn use_current_or_foreign_insert(&self, t: &Token) -> bool {
        if self.stack.is_empty() {
            return true;
        }
        let el = self.current();
        let ns = self.ns(el);
        if ns == Ns::Html {
            return true;
        }
        if ns == Ns::MathMl && in_list(self.name(el), TAG_MATHML_TEXT_INTEGRATION) {
            if let Token::StartTag(s) = t {
                if s.normal != "mglyph" && s.normal != "malignmark" {
                    return true;
                }
            }
            if matches!(t, Token::Character { .. }) {
                return true;
            }
        }
        if ns == Ns::MathMl && self.name(el) == "annotation-xml" {
            if let Token::StartTag(s) = t {
                if s.normal == "svg" {
                    return true;
                }
            }
        }
        if self.is_html_integration(el) && matches!(t, Token::StartTag(_) | Token::Character { .. }) {
            return true;
        }
        matches!(t, Token::Eof)
    }

    fn is_html_integration(&self, el: NodeId) -> bool {
        if self.ns(el) == Ns::MathMl && self.name(el) == "annotation-xml" {
            let enc = self.doc.attr(el, "encoding").trim().to_lowercase();
            if enc == "text/html" || enc == "application/xhtml+xml" {
                return true;
            }
        }
        self.ns(el) == Ns::Svg && in_list(self.doc.tag_name(el), TAG_SVG_HTML_INTEGRATION)
    }

    fn process_start_tag(&mut self, name: &str) -> bool {
        self.process(&Token::StartTag(Tag::named(name)))
    }

    fn process_end_tag(&mut self, name: &str) -> bool {
        self.process(&Token::EndTag(Tag::named(name)))
    }

    fn transition(&mut self, m: Mode) {
        self.state = m;
    }

    fn mark_insertion_mode(&mut self) {
        self.original_state = self.state;
    }

    // ── insertion ──

    fn create_element_for(&mut self, tag: &Tag, ns: Ns, preserve_case: bool) -> NodeId {
        let mut attrs: Vec<Attr> = Vec::new();
        if let Some(a) = &tag.attrs {
            for at in a {
                let key = if preserve_case { at.key.clone() } else { at.key.to_lowercase() };
                if !attrs.iter().any(|x| x.key == key) {
                    attrs.push(Attr { key, value: at.value.clone() });
                }
            }
        }
        let name = if preserve_case { tag.name.trim().to_string() } else { tag.normal.trim().to_string() };
        let element_ns = if ns == Ns::Html {
            Ns::Html
        } else {
            ns
        };
        self.doc.create_element_ns(&name, &tag.normal, element_ns, attrs)
    }

    fn insert_element_for(&mut self, tag: &Tag) -> NodeId {
        let el = self.create_element_for(tag, Ns::Html, false);
        self.do_insert_element(el);
        if tag.self_closing {
            // jsoup: unknown tags remember self-closing; known non-void tags are an error. Either way
            // the tokeniser emits a synthetic end tag.
            self.tokeniser.transition(TState::Data);
            let name = self.doc.tag_name(el).to_string();
            self.tokeniser.emit(Token::EndTag(Tag::named(&name)));
        }
        el
    }

    fn insert_foreign_element_for(&mut self, tag: &Tag, ns: Ns) -> NodeId {
        let el = self.create_element_for(tag, ns, true);
        self.do_insert_element(el);
        if tag.self_closing {
            self.pop();
        }
        el
    }

    fn insert_empty_element_for(&mut self, tag: &Tag) -> NodeId {
        let el = self.create_element_for(tag, Ns::Html, false);
        self.do_insert_element(el);
        self.pop();
        el
    }

    fn insert_form_element(&mut self, tag: &Tag, on_stack: bool, check_template: bool) -> NodeId {
        let el = self.create_element_for(tag, Ns::Html, false);
        if check_template {
            if !self.on_stack_name("template") {
                self.form_element = Some(el);
            }
        } else {
            self.form_element = Some(el);
        }
        self.do_insert_element(el);
        if !on_stack {
            self.pop();
        }
        el
    }

    fn do_insert_element(&mut self, el: NodeId) {
        let cur = self.current();
        if self.foster_inserts && in_list(self.name(cur), IN_TABLE_FOSTER) {
            self.insert_in_foster_parent(el);
        } else {
            self.doc.append_child(cur, el);
        }
        self.push(el);
    }

    fn insert_comment(&mut self, data: &str) {
        let node = self.doc.create_leaf(Kind::Comment, data);
        let cur = self.current();
        self.doc.append_child(cur, node);
    }

    fn insert_character(&mut self, t: &Token) {
        let cur = self.current();
        self.insert_character_to(t, cur);
    }

    fn insert_character_to(&mut self, t: &Token, el: NodeId) {
        if let Token::Character { data, cdata } = t {
            let name = self.name(el).to_string();
            let kind = if *cdata {
                Kind::CData
            } else if name == "script" || name == "style" {
                Kind::Data
            } else {
                Kind::Text
            };
            let node = self.doc.create_leaf(kind, data);
            self.doc.append_child(el, node);
        }
    }

    fn insert_in_foster_parent(&mut self, node: NodeId) {
        let last_table = self.get_from_stack("table");
        let mut is_last_table_parent = false;
        let foster_parent;
        if let Some(lt) = last_table {
            if let Some(p) = self.doc.parent(lt) {
                foster_parent = p;
                is_last_table_parent = true;
            } else {
                foster_parent = self.above_on_stack(lt).unwrap_or(ROOT);
            }
        } else {
            foster_parent = self.stack[0];
        }
        if is_last_table_parent {
            self.doc.insert_before(last_table.unwrap(), node);
        } else {
            self.doc.append_child(foster_parent, node);
        }
    }

    // ── stack queries ──

    fn on_stack(&self, el: NodeId) -> bool {
        let bottom = self.stack.len() as isize - 1;
        let upper = if bottom >= MAX_QUEUE_DEPTH as isize { bottom - MAX_QUEUE_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= upper {
            if self.stack[pos as usize] == el {
                return true;
            }
            pos -= 1;
        }
        false
    }

    fn get_from_stack(&self, name: &str) -> Option<NodeId> {
        let bottom = self.stack.len() as isize - 1;
        let upper = if bottom >= MAX_QUEUE_DEPTH as isize { bottom - MAX_QUEUE_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= upper {
            let el = self.stack[pos as usize];
            if self.element_is(el, name) {
                return Some(el);
            }
            pos -= 1;
        }
        None
    }

    fn on_stack_name(&self, name: &str) -> bool {
        self.get_from_stack(name).is_some()
    }

    fn remove_from_stack(&mut self, el: NodeId) -> bool {
        if let Some(pos) = self.stack.iter().rposition(|e| *e == el) {
            self.stack.remove(pos);
            true
        } else {
            false
        }
    }

    fn pop_stack_to_close(&mut self, name: &str) -> Option<NodeId> {
        while let Some(el) = self.pop() {
            if self.element_is(el, name) {
                return Some(el);
            }
        }
        None
    }

    fn pop_stack_to_close_any_namespace(&mut self, name: &str) -> Option<NodeId> {
        while let Some(el) = self.pop() {
            if self.name(el) == name {
                return Some(el);
            }
        }
        None
    }

    fn pop_stack_to_close_any(&mut self, names: &[&str]) {
        while let Some(el) = self.pop() {
            if in_list(self.name(el), names) && self.ns(el) == Ns::Html {
                break;
            }
        }
    }

    fn clear_stack_to_context(&mut self, names: &[&str]) {
        while let Some(&next) = self.stack.last() {
            if self.ns(next) == Ns::Html && (in_list(self.name(next), names) || self.name(next) == "html") {
                break;
            }
            self.pop();
        }
    }

    fn clear_stack_to_table_context(&mut self) {
        self.clear_stack_to_context(&["table", "template"]);
    }

    fn clear_stack_to_table_body_context(&mut self) {
        self.clear_stack_to_context(&["tbody", "tfoot", "thead", "template"]);
    }

    fn clear_stack_to_table_row_context(&mut self) {
        self.clear_stack_to_context(&["tr", "template"]);
    }

    fn above_on_stack(&self, el: NodeId) -> Option<NodeId> {
        let pos = self.stack.iter().rposition(|e| *e == el)?;
        if pos == 0 {
            None
        } else {
            Some(self.stack[pos - 1])
        }
    }

    fn insert_on_stack_after(&mut self, after: NodeId, el: NodeId) {
        let i = self.stack.iter().rposition(|e| *e == after).expect("insertOnStackAfter");
        self.stack.insert(i + 1, el);
    }

    fn replace_on_stack(&mut self, out: NodeId, inn: NodeId) {
        if let Some(i) = self.stack.iter().rposition(|e| *e == out) {
            self.stack[i] = inn;
        }
    }

    fn reset_insertion_mode(&mut self) -> bool {
        let orig = self.state;
        if self.stack.is_empty() {
            self.transition(Mode::InBody);
        }
        let bottom = self.stack.len() as isize - 1;
        let upper = if bottom >= MAX_QUEUE_DEPTH as isize { bottom - MAX_QUEUE_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= upper {
            let node = self.stack[pos as usize];
            let last = pos == upper;
            pos -= 1;
            if self.ns(node) != Ns::Html {
                continue;
            }
            let name = self.name(node).to_string();
            match name.as_str() {
                "select" => {
                    self.transition(Mode::InSelect);
                    break;
                }
                "td" | "th" if !last => {
                    self.transition(Mode::InCell);
                    break;
                }
                "tr" => {
                    self.transition(Mode::InRow);
                    break;
                }
                "tbody" | "thead" | "tfoot" => {
                    self.transition(Mode::InTableBody);
                    break;
                }
                "caption" => {
                    self.transition(Mode::InCaption);
                    break;
                }
                "colgroup" => {
                    self.transition(Mode::InColumnGroup);
                    break;
                }
                "table" => {
                    self.transition(Mode::InTable);
                    break;
                }
                "template" => {
                    let m = self.tmpl_insert_mode.last().copied().unwrap_or(Mode::InBody);
                    self.transition(m);
                    break;
                }
                "head" if !last => {
                    self.transition(Mode::InHead);
                    break;
                }
                "body" => {
                    self.transition(Mode::InBody);
                    break;
                }
                "frameset" => {
                    self.transition(Mode::InFrameset);
                    break;
                }
                "html" => {
                    self.transition(if self.head_element.is_none() { Mode::BeforeHead } else { Mode::AfterHead });
                    break;
                }
                _ => {}
            }
            if last {
                self.transition(Mode::InBody);
                break;
            }
        }
        self.state != orig
    }

    fn reset_body(&mut self) {
        if !self.on_stack_name("body") {
            if let Some(b) = self.doc.body() {
                self.stack.push(b);
            }
        }
        self.transition(Mode::InBody);
    }

    fn in_specific_scope(&self, targets: &[&str], base: &[&str], extra: Option<&[&str]>) -> bool {
        let bottom = self.stack.len() as isize - 1;
        let top = if bottom > MAX_SCOPE_SEARCH_DEPTH as isize { bottom - MAX_SCOPE_SEARCH_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= top {
            let el = self.stack[pos as usize];
            let name = self.name(el);
            let ns = self.ns(el);
            if ns == Ns::Html {
                if in_list(name, targets) {
                    return true;
                }
                if in_list(name, base) {
                    return false;
                }
                if let Some(e) = extra {
                    if in_list(name, e) {
                        return false;
                    }
                }
            } else if base == TAGS_SEARCH_IN_SCOPE {
                if ns == Ns::MathMl && in_list(name, TAG_SEARCH_IN_SCOPE_MATH) {
                    return false;
                }
                if ns == Ns::Svg && in_list(name, TAG_SEARCH_IN_SCOPE_SVG) {
                    return false;
                }
            }
            pos -= 1;
        }
        false
    }

    fn in_scope(&self, name: &str) -> bool {
        self.in_specific_scope(&[name], TAGS_SEARCH_IN_SCOPE, None)
    }

    fn in_scope_any(&self, names: &[&str]) -> bool {
        self.in_specific_scope(names, TAGS_SEARCH_IN_SCOPE, None)
    }

    fn in_list_item_scope(&self, name: &str) -> bool {
        self.in_specific_scope(&[name], TAGS_SEARCH_IN_SCOPE, Some(TAG_SEARCH_LIST))
    }

    fn in_button_scope(&self, name: &str) -> bool {
        self.in_specific_scope(&[name], TAGS_SEARCH_IN_SCOPE, Some(TAG_SEARCH_BUTTON))
    }

    fn in_table_scope(&self, name: &str) -> bool {
        self.in_specific_scope(&[name], TAG_SEARCH_TABLE_SCOPE, None)
    }

    fn in_select_scope(&self, name: &str) -> bool {
        for &el in self.stack.iter().rev() {
            let n = self.name(el);
            if n == name {
                return true;
            }
            if !in_list(n, TAG_SEARCH_SELECT_SCOPE) {
                return false;
            }
        }
        false
    }

    fn on_stack_not(&self, allowed: &[&str]) -> bool {
        let bottom = self.stack.len() as isize - 1;
        let top = if bottom > MAX_SCOPE_SEARCH_DEPTH as isize { bottom - MAX_SCOPE_SEARCH_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= top {
            if !in_list(self.name(self.stack[pos as usize]), allowed) {
                return true;
            }
            pos -= 1;
        }
        false
    }

    fn generate_implied_end_tags_except(&mut self, exclude: Option<&str>) {
        while in_list(self.name(self.current()), TAG_SEARCH_END_TAGS) {
            if let Some(ex) = exclude {
                if self.current_is(ex) {
                    break;
                }
            }
            self.pop();
        }
    }

    fn generate_implied_end_tags(&mut self, thorough: bool) {
        let search = if thorough { TAG_THOROUGH_SEARCH_END_TAGS } else { TAG_SEARCH_END_TAGS };
        while !self.stack.is_empty() && self.ns(self.current()) == Ns::Html && in_list(self.name(self.current()), search) {
            self.pop();
        }
    }

    fn close_element(&mut self, name: &str) {
        self.generate_implied_end_tags_except(Some(name));
        self.pop_stack_to_close(name);
    }

    fn is_special(&self, el: NodeId) -> bool {
        let name = self.name(el);
        match self.ns(el) {
            Ns::Html => in_list(name, TAG_SEARCH_SPECIAL),
            Ns::MathMl => in_list(name, TAG_SEARCH_SPECIAL_MATH),
            Ns::Svg => in_list(name, TAG_SVG_HTML_INTEGRATION),
        }
    }

    // ── active formatting elements ──

    fn last_formatting_element(&self) -> Option<NodeId> {
        self.formatting.last().copied().flatten()
    }

    fn position_of_element(&self, el: NodeId) -> Option<usize> {
        self.formatting.iter().position(|e| *e == Some(el))
    }

    fn push_active_formatting(&mut self, el: NodeId) {
        self.check_active_formatting(el);
        self.formatting.push(Some(el));
    }

    fn push_with_bookmark(&mut self, el: NodeId, bookmark: usize) {
        self.check_active_formatting(el);
        if bookmark <= self.formatting.len() {
            self.formatting.insert(bookmark, Some(el));
        } else {
            self.formatting.push(Some(el));
        }
    }

    fn check_active_formatting(&mut self, el: NodeId) {
        let mut num_seen = 0;
        let size = self.formatting.len() as isize - 1;
        let mut ceil = size - MAX_USED_FORMATTING_ELEMENTS as isize;
        if ceil < 0 {
            ceil = 0;
        }
        let mut pos = size;
        while pos >= ceil {
            let e = match self.formatting[pos as usize] {
                None => break,
                Some(e) => e,
            };
            if self.is_same_formatting_element(el, e) {
                num_seen += 1;
            }
            if num_seen == 3 {
                self.formatting.remove(pos as usize);
                break;
            }
            pos -= 1;
        }
    }

    fn is_same_formatting_element(&self, a: NodeId, b: NodeId) -> bool {
        self.name(a) == self.name(b) && attrs_equal(self.doc.attrs(a), self.doc.attrs(b))
    }

    fn reconstruct_formatting_elements(&mut self) {
        if self.stack.len() > MAX_QUEUE_DEPTH {
            return;
        }
        let last = match self.last_formatting_element() {
            None => return,
            Some(l) => l,
        };
        if self.on_stack(last) {
            return;
        }
        let size = self.formatting.len();
        let mut ceil = size as isize - MAX_USED_FORMATTING_ELEMENTS as isize;
        if ceil < 0 {
            ceil = 0;
        }
        let mut pos = size as isize - 1;
        let mut skip = false;
        loop {
            if pos == ceil {
                skip = true;
                break;
            }
            pos -= 1;
            let entry = self.formatting[pos as usize];
            match entry {
                None => break,
                Some(e) if self.on_stack(e) => break,
                _ => {}
            }
        }
        loop {
            if !skip {
                pos += 1;
            }
            skip = false;
            let entry = match self.formatting[pos as usize] {
                Some(e) => e,
                None => break,
            };
            let name = self.doc.tag_name(entry).to_string();
            let normal = self.name(entry).to_string();
            let attrs = self.doc.attrs(entry).to_vec();
            let new_el = self.doc.create_element_ns(&name, &normal, Ns::Html, attrs);
            self.do_insert_element(new_el);
            self.formatting[pos as usize] = Some(new_el);
            if pos as usize == size - 1 {
                break;
            }
        }
    }

    fn clear_formatting_elements_to_last_marker(&mut self) {
        while let Some(e) = self.formatting.pop() {
            if e.is_none() {
                break;
            }
        }
    }

    fn remove_from_active_formatting(&mut self, el: NodeId) {
        if let Some(pos) = self.formatting.iter().rposition(|e| *e == Some(el)) {
            self.formatting.remove(pos);
        }
    }

    fn is_in_active_formatting(&self, el: NodeId) -> bool {
        let bottom = self.formatting.len() as isize - 1;
        let upper = if bottom >= MAX_QUEUE_DEPTH as isize { bottom - MAX_QUEUE_DEPTH as isize } else { 0 };
        let mut pos = bottom;
        while pos >= upper {
            if self.formatting[pos as usize] == Some(el) {
                return true;
            }
            pos -= 1;
        }
        false
    }

    fn get_active_formatting_element(&self, name: &str) -> Option<NodeId> {
        for e in self.formatting.iter().rev() {
            match e {
                None => break,
                Some(e) if self.name(*e) == name => return Some(*e),
                _ => {}
            }
        }
        None
    }

    fn replace_active_formatting(&mut self, out: NodeId, inn: NodeId) {
        if let Some(i) = self.formatting.iter().rposition(|e| *e == Some(out)) {
            self.formatting[i] = Some(inn);
        }
    }

    fn insert_marker(&mut self) {
        self.formatting.push(None);
    }

    // ── states ──

    fn process_in(&mut self, mode: Mode, t: &Token) -> bool {
        match mode {
            Mode::Initial => self.initial(t),
            Mode::BeforeHtml => self.before_html(t),
            Mode::BeforeHead => self.before_head(t),
            Mode::InHead => self.in_head(t),
            Mode::InHeadNoscript => self.in_head_noscript(t),
            Mode::AfterHead => self.after_head(t),
            Mode::InBody => self.in_body(t),
            Mode::Text => self.text(t),
            Mode::InTable => self.in_table(t),
            Mode::InTableText => self.in_table_text(t),
            Mode::InCaption => self.in_caption(t),
            Mode::InColumnGroup => self.in_column_group(t),
            Mode::InTableBody => self.in_table_body(t),
            Mode::InRow => self.in_row(t),
            Mode::InCell => self.in_cell(t),
            Mode::InSelect => self.in_select(t),
            Mode::InSelectInTable => self.in_select_in_table(t),
            Mode::InTemplate => self.in_template(t),
            Mode::AfterBody => self.after_body(t),
            Mode::InFrameset => self.in_frameset(t),
            Mode::AfterFrameset => self.after_frameset(t),
            Mode::AfterAfterBody => self.after_after_body(t),
            Mode::AfterAfterFrameset => self.after_after_frameset(t),
        }
    }

    fn initial(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { name, public_id, force_quirks } => {
                let dt = self.doc.create_leaf(Kind::Doctype, name);
                self.doc.append_child(ROOT, dt);
                if *force_quirks || name != "html" || public_id.eq_ignore_ascii_case("HTML") {
                    self.doc.quirks = true;
                }
                self.transition(Mode::BeforeHtml);
            }
            _ => {
                self.doc.quirks = true;
                self.transition(Mode::BeforeHtml);
                return self.process(t);
            }
        }
        true
    }

    fn before_html(&mut self, t: &Token) -> bool {
        match t {
            Token::Doctype { .. } => false,
            Token::Comment(d) => {
                self.insert_comment(d);
                true
            }
            _ if t.is_whitespace() => {
                self.insert_character(t);
                true
            }
            Token::StartTag(s) if s.normal == "html" => {
                self.insert_element_for(s);
                self.transition(Mode::BeforeHead);
                true
            }
            Token::EndTag(e) if in_list(&e.normal, BEFORE_HTML_TO_HEAD) => self.before_html_anything_else(t),
            Token::EndTag(_) => false,
            _ => self.before_html_anything_else(t),
        }
    }

    fn before_html_anything_else(&mut self, t: &Token) -> bool {
        self.process_start_tag("html");
        self.transition(Mode::BeforeHead);
        self.process(t)
    }

    fn before_head(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => {
                self.insert_comment(d);
                true
            }
            Token::Doctype { .. } => false,
            Token::StartTag(s) if s.normal == "html" => self.in_body(t),
            Token::StartTag(s) if s.normal == "head" => {
                let head = self.insert_element_for(s);
                self.head_element = Some(head);
                self.transition(Mode::InHead);
                true
            }
            Token::EndTag(e) if in_list(&e.normal, BEFORE_HTML_TO_HEAD) => {
                self.process_start_tag("head");
                self.process(t)
            }
            Token::EndTag(_) => false,
            _ => {
                self.process_start_tag("head");
                self.process(t)
            }
        }
    }

    fn in_head(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return false,
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if name == "html" {
                    return self.in_body(t);
                } else if in_list(name, IN_HEAD_EMPTY) || name == "meta" {
                    self.insert_empty_element_for(s);
                } else if name == "title" {
                    self.handle_rcdata(s);
                } else if in_list(name, IN_HEAD_RAW) {
                    self.handle_rawtext(s);
                } else if name == "noscript" {
                    self.insert_element_for(s);
                    self.transition(Mode::InHeadNoscript);
                } else if name == "script" {
                    self.tokeniser.transition(TState::ScriptData);
                    self.mark_insertion_mode();
                    self.transition(Mode::Text);
                    self.insert_element_for(s);
                } else if name == "head" {
                    return false;
                } else if name == "template" {
                    self.insert_element_for(s);
                    self.insert_marker();
                    self.frameset_ok = false;
                    self.transition(Mode::InTemplate);
                    self.tmpl_insert_mode.push(Mode::InTemplate);
                } else {
                    return self.in_head_anything_else(t);
                }
            }
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if name == "head" {
                    self.pop();
                    self.transition(Mode::AfterHead);
                } else if in_list(name, IN_HEAD_END) {
                    return self.in_head_anything_else(t);
                } else if name == "template" {
                    if self.on_stack_name(name) {
                        self.generate_implied_end_tags(true);
                        self.pop_stack_to_close(name);
                        self.clear_formatting_elements_to_last_marker();
                        self.tmpl_insert_mode.pop();
                        self.reset_insertion_mode();
                    }
                } else {
                    return false;
                }
            }
            _ => return self.in_head_anything_else(t),
        }
        true
    }

    fn in_head_anything_else(&mut self, t: &Token) -> bool {
        self.process_end_tag("head");
        self.process(t)
    }

    fn in_head_noscript(&mut self, t: &Token) -> bool {
        match t {
            Token::Doctype { .. } => {}
            Token::StartTag(s) if s.normal == "html" => return self.process_in(Mode::InBody, t),
            Token::EndTag(e) if e.normal == "noscript" => {
                self.pop();
                self.transition(Mode::InHead);
            }
            _ if t.is_whitespace()
                || matches!(t, Token::Comment(_))
                || matches!(t, Token::StartTag(s) if in_list(&s.normal, IN_HEAD_NOSCRIPT_HEAD)) =>
            {
                return self.process_in(Mode::InHead, t);
            }
            Token::EndTag(e) if e.normal == "br" => return self.in_head_noscript_anything_else(t),
            Token::StartTag(s) if in_list(&s.normal, IN_HEAD_NOSCRIPT_IGNORE) => return false,
            Token::EndTag(_) => return false,
            _ => return self.in_head_noscript_anything_else(t),
        }
        true
    }

    fn in_head_noscript_anything_else(&mut self, t: &Token) -> bool {
        let data = t.to_source();
        self.insert_character(&Token::Character { data, cdata: false });
        true
    }

    fn after_head(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => {}
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if name == "html" {
                    return self.process_in(Mode::InBody, t);
                } else if name == "body" {
                    self.insert_element_for(s);
                    self.frameset_ok = false;
                    self.transition(Mode::InBody);
                } else if name == "frameset" {
                    self.insert_element_for(s);
                    self.transition(Mode::InFrameset);
                } else if in_list(name, IN_BODY_START_TO_HEAD) {
                    if let Some(head) = self.head_element {
                        self.push(head);
                        self.process_in(Mode::InHead, t);
                        self.remove_from_stack(head);
                    }
                } else if name == "head" {
                    return false;
                } else {
                    self.after_head_anything_else(t);
                }
            }
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if in_list(name, AFTER_HEAD_BODY) {
                    self.after_head_anything_else(t);
                } else if name == "template" {
                    self.process_in(Mode::InHead, t);
                } else {
                    return false;
                }
            }
            _ => {
                self.after_head_anything_else(t);
            }
        }
        true
    }

    fn after_head_anything_else(&mut self, t: &Token) -> bool {
        self.process_start_tag("body");
        self.frameset_ok = true;
        self.process(t)
    }

    fn in_body(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { data, .. } => {
                if data == "\u{0000}" {
                    return false;
                } else if self.frameset_ok && t.is_whitespace() {
                    self.reconstruct_formatting_elements();
                    self.insert_character(t);
                } else {
                    self.reconstruct_formatting_elements();
                    self.insert_character(t);
                    self.frameset_ok = false;
                }
                true
            }
            Token::Comment(d) => {
                self.insert_comment(d);
                true
            }
            Token::Doctype { .. } => false,
            Token::StartTag(s) => self.in_body_start_tag(t, s),
            Token::EndTag(e) => self.in_body_end_tag(t, e),
            Token::Eof => {
                if !self.tmpl_insert_mode.is_empty() {
                    return self.process_in(Mode::InTemplate, t);
                }
                true
            }
        }
    }

    fn in_body_start_tag(&mut self, t: &Token, s: &Tag) -> bool {
        let name = s.normal.as_str();
        match name {
            "a" => {
                if self.get_active_formatting_element("a").is_some() {
                    self.process_end_tag("a");
                    if let Some(remaining) = self.get_from_stack("a") {
                        self.remove_from_active_formatting(remaining);
                        self.remove_from_stack(remaining);
                    }
                }
                self.reconstruct_formatting_elements();
                let el = self.insert_element_for(s);
                self.push_active_formatting(el);
            }
            "span" => {
                self.reconstruct_formatting_elements();
                self.insert_element_for(s);
            }
            "li" => {
                self.frameset_ok = false;
                let mut i = self.stack.len() as isize - 1;
                while i > 0 {
                    let el = self.stack[i as usize];
                    if self.name(el) == "li" {
                        self.process_end_tag("li");
                        break;
                    }
                    if self.is_special(el) && !in_list(self.name(el), IN_BODY_START_LI_BREAKERS) {
                        break;
                    }
                    i -= 1;
                }
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_element_for(s);
            }
            "html" => {
                if self.on_stack_name("template") {
                    return false;
                }
                if let Some(&html) = self.stack.first() {
                    self.merge_attributes(s, html);
                }
            }
            "body" => {
                let len = self.stack.len();
                if len == 1 || (len > 2 && self.name(self.stack[1]) != "body") || self.on_stack_name("template") {
                    return false;
                }
                self.frameset_ok = false;
                if let Some(body) = self.get_from_stack("body") {
                    self.merge_attributes(s, body);
                }
            }
            "frameset" => {
                let len = self.stack.len();
                if len == 1 || (len > 2 && self.name(self.stack[1]) != "body") {
                    return false;
                } else if !self.frameset_ok {
                    return false;
                } else {
                    let second = self.stack[1];
                    if self.doc.parent(second).is_some() {
                        self.doc.detach(second);
                    }
                    while self.stack.len() > 1 {
                        self.stack.pop();
                    }
                    self.insert_element_for(s);
                    self.transition(Mode::InFrameset);
                }
            }
            "form" => {
                if self.form_element.is_some() && !self.on_stack_name("template") {
                    return false;
                }
                if self.in_button_scope("p") {
                    self.close_element("p");
                }
                self.insert_form_element(s, true, true);
            }
            "plaintext" => {
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_element_for(s);
                self.tokeniser.transition(TState::Plaintext);
            }
            "button" => {
                if self.in_button_scope("button") {
                    self.process_end_tag("button");
                    self.process(t);
                } else {
                    self.reconstruct_formatting_elements();
                    self.insert_element_for(s);
                    self.frameset_ok = false;
                }
            }
            "nobr" => {
                self.reconstruct_formatting_elements();
                if self.in_scope("nobr") {
                    self.process_end_tag("nobr");
                    self.reconstruct_formatting_elements();
                }
                let el = self.insert_element_for(s);
                self.push_active_formatting(el);
            }
            "table" => {
                if !self.doc.quirks && self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_element_for(s);
                self.frameset_ok = false;
                self.transition(Mode::InTable);
            }
            "input" => {
                self.reconstruct_formatting_elements();
                let el = self.insert_empty_element_for(s);
                if !self.doc.attr(el, "type").eq_ignore_ascii_case("hidden") {
                    self.frameset_ok = false;
                }
            }
            "hr" => {
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_empty_element_for(s);
                self.frameset_ok = false;
            }
            "image" => {
                if self.get_from_stack("svg").is_none() {
                    let mut s2 = s.clone();
                    s2.name = "img".into();
                    s2.normal = "img".into();
                    return self.process(&Token::StartTag(s2));
                } else {
                    self.insert_element_for(s);
                }
            }
            "textarea" => {
                self.insert_element_for(s);
                if !s.self_closing {
                    self.tokeniser.transition(TState::Rcdata);
                    self.mark_insertion_mode();
                    self.frameset_ok = false;
                    self.transition(Mode::Text);
                }
            }
            "xmp" => {
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.reconstruct_formatting_elements();
                self.frameset_ok = false;
                self.handle_rawtext(s);
            }
            "iframe" => {
                self.frameset_ok = false;
                self.handle_rawtext(s);
            }
            "noembed" => self.handle_rawtext(s),
            "select" => {
                self.reconstruct_formatting_elements();
                self.insert_element_for(s);
                self.frameset_ok = false;
                if s.self_closing {
                    return true;
                }
                let st = self.state;
                if matches!(st, Mode::InTable | Mode::InCaption | Mode::InTableBody | Mode::InRow | Mode::InCell) {
                    self.transition(Mode::InSelectInTable);
                } else {
                    self.transition(Mode::InSelect);
                }
            }
            "math" => {
                self.reconstruct_formatting_elements();
                self.insert_foreign_element_for(s, Ns::MathMl);
            }
            "svg" => {
                self.reconstruct_formatting_elements();
                self.insert_foreign_element_for(s, Ns::Svg);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                if in_list(self.name(self.current()), HEADINGS) {
                    self.pop();
                }
                self.insert_element_for(s);
            }
            "pre" | "listing" => {
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_element_for(s);
                self.tokeniser.reader.match_consume("\n");
                self.frameset_ok = false;
            }
            "dd" | "dt" => {
                self.frameset_ok = false;
                let bottom = self.stack.len() as isize - 1;
                let upper = if bottom >= 24 { bottom - 24 } else { 0 };
                let mut i = bottom;
                while i >= upper {
                    let el = self.stack[i as usize];
                    if in_list(self.name(el), DD_DT) {
                        let n = self.name(el).to_string();
                        self.process_end_tag(&n);
                        break;
                    }
                    if self.is_special(el) && !in_list(self.name(el), IN_BODY_START_LI_BREAKERS) {
                        break;
                    }
                    i -= 1;
                }
                if self.in_button_scope("p") {
                    self.process_end_tag("p");
                }
                self.insert_element_for(s);
            }
            "optgroup" | "option" => {
                if self.current_is("option") {
                    self.process_end_tag("option");
                }
                self.reconstruct_formatting_elements();
                self.insert_element_for(s);
            }
            "rb" | "rtc" => {
                if self.in_scope("ruby") {
                    self.generate_implied_end_tags(false);
                }
                self.insert_element_for(s);
            }
            "rp" | "rt" => {
                if self.in_scope("ruby") {
                    self.generate_implied_end_tags_except(Some("rtc"));
                }
                self.insert_element_for(s);
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_formatting_elements();
                self.insert_empty_element_for(s);
                self.frameset_ok = false;
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong" | "tt" | "u" => {
                self.reconstruct_formatting_elements();
                let el = self.insert_element_for(s);
                self.push_active_formatting(el);
            }
            _ => {
                if !is_known_tag(name) {
                    self.insert_element_for(s);
                } else if in_list(name, IN_BODY_START_P_CLOSERS) {
                    if self.in_button_scope("p") {
                        self.process_end_tag("p");
                    }
                    self.insert_element_for(s);
                } else if in_list(name, IN_BODY_START_TO_HEAD) {
                    return self.process_in(Mode::InHead, t);
                } else if in_list(name, IN_BODY_START_APPLETS) {
                    self.reconstruct_formatting_elements();
                    self.insert_element_for(s);
                    self.insert_marker();
                    self.frameset_ok = false;
                } else if in_list(name, IN_BODY_START_MEDIA) {
                    self.insert_empty_element_for(s);
                } else if in_list(name, IN_BODY_START_DROP) {
                    return false;
                } else {
                    self.reconstruct_formatting_elements();
                    self.insert_element_for(s);
                }
            }
        }
        true
    }

    fn merge_attributes(&mut self, s: &Tag, dest: NodeId) {
        if let Some(attrs) = &s.attrs {
            for a in attrs {
                let key = a.key.to_lowercase();
                if !self.doc.attrs(dest).iter().any(|x| x.key == key) {
                    self.doc.node_mut(dest).attrs.push(Attr { key, value: a.value.clone() });
                }
            }
        }
    }

    fn in_body_end_tag(&mut self, t: &Token, e: &Tag) -> bool {
        let name = e.normal.as_str();
        match name {
            "template" => {
                self.process_in(Mode::InHead, t);
            }
            "sarcasm" | "span" => return self.any_other_end_tag(e),
            "li" => {
                if !self.in_list_item_scope(name) {
                    return false;
                }
                self.generate_implied_end_tags_except(Some(name));
                self.pop_stack_to_close(name);
            }
            "body" => {
                if !self.in_scope("body") {
                    return false;
                }
                self.transition(Mode::AfterBody);
            }
            "html" => {
                if !self.on_stack_name("body") {
                    return false;
                }
                self.transition(Mode::AfterBody);
                return self.process(t);
            }
            "form" => {
                if !self.on_stack_name("template") {
                    let current_form = self.form_element.take();
                    match current_form {
                        Some(cf) if self.in_scope(name) => {
                            self.generate_implied_end_tags(false);
                            self.remove_from_stack(cf);
                        }
                        _ => return false,
                    }
                } else {
                    if !self.in_scope(name) {
                        return false;
                    }
                    self.generate_implied_end_tags(false);
                    self.pop_stack_to_close(name);
                }
            }
            "p" => {
                if !self.in_button_scope(name) {
                    self.process_start_tag(name);
                    return self.process(t);
                } else {
                    self.generate_implied_end_tags_except(Some(name));
                    self.pop_stack_to_close(name);
                }
            }
            "dd" | "dt" => {
                if !self.in_scope(name) {
                    return false;
                }
                self.generate_implied_end_tags_except(Some(name));
                self.pop_stack_to_close(name);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if !self.in_scope_any(HEADINGS) {
                    return false;
                }
                self.generate_implied_end_tags_except(Some(name));
                self.pop_stack_to_close_any(HEADINGS);
            }
            "br" => {
                self.process_start_tag("br");
                return false;
            }
            _ => {
                if in_list(name, IN_BODY_END_ADOPTION_FORMATTERS) {
                    return self.in_body_end_tag_adoption(e);
                } else if in_list(name, IN_BODY_END_CLOSERS) {
                    if !self.in_scope(name) {
                        return false;
                    }
                    self.generate_implied_end_tags(false);
                    self.pop_stack_to_close(name);
                } else if in_list(name, IN_BODY_START_APPLETS) {
                    if !self.in_scope("name") {
                        if !self.in_scope(name) {
                            return false;
                        }
                        self.generate_implied_end_tags(false);
                        self.pop_stack_to_close(name);
                        self.clear_formatting_elements_to_last_marker();
                    }
                } else {
                    return self.any_other_end_tag(e);
                }
            }
        }
        true
    }

    fn any_other_end_tag(&mut self, e: &Tag) -> bool {
        let name = e.normal.as_str();
        if self.get_from_stack(name).is_none() {
            return false;
        }
        let mut pos = self.stack.len() as isize - 1;
        while pos >= 0 {
            let node = self.stack[pos as usize];
            if self.name(node) == name {
                self.generate_implied_end_tags_except(Some(name));
                self.pop_stack_to_close(name);
                break;
            } else if self.is_special(node) {
                return false;
            }
            pos -= 1;
        }
        true
    }

    fn in_body_end_tag_adoption(&mut self, e: &Tag) -> bool {
        let subject = e.normal.clone();
        let cur = self.current();
        if self.name(cur) == subject && !self.is_in_active_formatting(cur) {
            self.pop();
            return true;
        }
        let mut outer = 0;
        loop {
            if outer >= 8 {
                return true;
            }
            outer += 1;
            let mut format_el = None;
            for i in (0..self.formatting.len()).rev() {
                match self.formatting[i] {
                    None => break,
                    Some(n) if self.name(n) == subject => {
                        format_el = Some(n);
                        break;
                    }
                    _ => {}
                }
            }
            let format_el = match format_el {
                None => return self.any_other_end_tag(e),
                Some(f) => f,
            };
            if !self.on_stack(format_el) {
                self.remove_from_active_formatting(format_el);
                return true;
            }
            if !self.in_scope(&self.name(format_el).to_string()) {
                return false;
            }
            let mut furthest_block = None;
            if let Some(fei) = self.stack.iter().rposition(|x| *x == format_el) {
                for i in fei + 1..self.stack.len() {
                    let el = self.stack[i];
                    if self.is_special(el) {
                        furthest_block = Some(el);
                        break;
                    }
                }
            }
            let furthest_block = match furthest_block {
                None => {
                    while self.current() != format_el {
                        self.pop();
                    }
                    self.pop();
                    self.remove_from_active_formatting(format_el);
                    return true;
                }
                Some(f) => f,
            };
            let common_ancestor = match self.above_on_stack(format_el) {
                None => return true,
                Some(c) => c,
            };
            let mut bookmark = self.position_of_element(format_el).map(|x| x as isize).unwrap_or(-1);
            let mut el = furthest_block;
            let mut last_el = furthest_block;
            let mut inner = 0;
            loop {
                inner += 1;
                let next = if !self.on_stack(el) { self.doc.parent_element(el) } else { self.above_on_stack(el) };
                el = match next {
                    None => break,
                    Some(n) => n,
                };
                if el == format_el {
                    break;
                }
                if inner > 3 && self.is_in_active_formatting(el) {
                    self.remove_from_active_formatting(el);
                    break;
                }
                if !self.is_in_active_formatting(el) {
                    self.remove_from_stack(el);
                    continue;
                }
                let tag_name = self.doc.tag_name(el).to_string();
                let normal = self.name(el).to_string();
                let replacement = self.doc.create_element_ns(&tag_name, &normal, Ns::Html, Vec::new());
                self.replace_active_formatting(el, replacement);
                self.replace_on_stack(el, replacement);
                el = replacement;
                if last_el == furthest_block {
                    bookmark = self.position_of_element(el).map(|x| x as isize).unwrap_or(-1) + 1;
                }
                self.doc.append_child(el, last_el);
                last_el = el;
            }
            self.doc.append_child(common_ancestor, last_el);
            let tag_name = self.doc.tag_name(format_el).to_string();
            let normal = self.name(format_el).to_string();
            let fns = self.ns(format_el);
            let attrs = self.doc.attrs(format_el).to_vec();
            let adoptor = self.doc.create_element_ns(&tag_name, &normal, fns, attrs);
            for child in self.doc.child_nodes(furthest_block) {
                self.doc.append_child(adoptor, child);
            }
            self.doc.append_child(furthest_block, adoptor);
            self.remove_from_active_formatting(format_el);
            let bm = if bookmark < 0 { usize::MAX } else { bookmark as usize };
            self.push_with_bookmark(adoptor, bm);
            self.remove_from_stack(format_el);
            self.insert_on_stack_after(furthest_block, adoptor);
        }
    }

    fn text(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { .. } => self.insert_character(t),
            Token::Eof => {
                self.pop();
                let o = self.original_state;
                self.transition(o);
                return self.process(t);
            }
            Token::EndTag(_) => {
                self.pop();
                let o = self.original_state;
                self.transition(o);
            }
            _ => {}
        }
        true
    }

    fn in_table(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { .. } if in_list(self.name(self.current()), IN_TABLE_FOSTER) => {
                self.pending_table_chars.clear();
                self.mark_insertion_mode();
                self.transition(Mode::InTableText);
                self.process(t)
            }
            Token::Comment(d) => {
                self.insert_comment(d);
                true
            }
            Token::Doctype { .. } => false,
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if name == "caption" {
                    self.clear_stack_to_table_context();
                    self.insert_marker();
                    self.insert_element_for(s);
                    self.transition(Mode::InCaption);
                } else if name == "colgroup" {
                    self.clear_stack_to_table_context();
                    self.insert_element_for(s);
                    self.transition(Mode::InColumnGroup);
                } else if name == "col" {
                    self.clear_stack_to_table_context();
                    self.process_start_tag("colgroup");
                    return self.process(t);
                } else if in_list(name, IN_TABLE_TO_BODY) {
                    self.clear_stack_to_table_context();
                    self.insert_element_for(s);
                    self.transition(Mode::InTableBody);
                } else if in_list(name, IN_TABLE_ADD_BODY) {
                    self.clear_stack_to_table_context();
                    self.process_start_tag("tbody");
                    return self.process(t);
                } else if name == "table" {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    self.pop_stack_to_close(name);
                    if !self.reset_insertion_mode() {
                        self.insert_element_for(s);
                        return true;
                    }
                    return self.process(t);
                } else if in_list(name, IN_TABLE_TO_HEAD) {
                    return self.process_in(Mode::InHead, t);
                } else if name == "input" {
                    let hidden = s.attrs.is_some() && s.attr("type").unwrap_or_default().eq_ignore_ascii_case("hidden");
                    if !hidden {
                        return self.in_table_anything_else(t);
                    }
                    self.insert_empty_element_for(s);
                } else if name == "form" {
                    if self.form_element.is_some() || self.on_stack_name("template") {
                        return false;
                    }
                    self.insert_form_element(s, false, false);
                } else {
                    return self.in_table_anything_else(t);
                }
                true
            }
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if name == "table" {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    self.pop_stack_to_close("table");
                    self.reset_insertion_mode();
                } else if in_list(name, IN_TABLE_END_ERR) {
                    return false;
                } else if name == "template" {
                    self.process_in(Mode::InHead, t);
                } else {
                    return self.in_table_anything_else(t);
                }
                true
            }
            Token::Eof => true,
            _ => self.in_table_anything_else(t),
        }
    }

    fn in_table_anything_else(&mut self, t: &Token) -> bool {
        self.foster_inserts = true;
        self.process_in(Mode::InBody, t);
        self.foster_inserts = false;
        true
    }

    fn in_table_text(&mut self, t: &Token) -> bool {
        if let Token::Character { data, .. } = t {
            if data == "\u{0000}" {
                return false;
            }
            self.pending_table_chars.push(t.clone());
            return true;
        }
        let pending = std::mem::take(&mut self.pending_table_chars);
        for c in &pending {
            if !c.is_whitespace() {
                if in_list(self.name(self.current()), IN_TABLE_FOSTER) {
                    self.foster_inserts = true;
                    self.process_in(Mode::InBody, c);
                    self.foster_inserts = false;
                } else {
                    self.process_in(Mode::InBody, c);
                }
            } else {
                self.insert_character(c);
            }
        }
        let o = self.original_state;
        self.transition(o);
        self.process(t)
    }

    fn in_caption(&mut self, t: &Token) -> bool {
        match t {
            Token::EndTag(e) if e.normal == "caption" => {
                if !self.in_table_scope("caption") {
                    return false;
                }
                self.generate_implied_end_tags(false);
                self.pop_stack_to_close("caption");
                self.clear_formatting_elements_to_last_marker();
                self.transition(Mode::InTable);
            }
            _ if matches!(t, Token::StartTag(s) if in_list(&s.normal, IN_CELL_COL))
                || matches!(t, Token::EndTag(e) if e.normal == "table") =>
            {
                if !self.in_table_scope("caption") {
                    return false;
                }
                self.generate_implied_end_tags(false);
                self.pop_stack_to_close("caption");
                self.clear_formatting_elements_to_last_marker();
                self.transition(Mode::InTable);
                self.in_table(t);
            }
            Token::EndTag(e) if in_list(&e.normal, IN_CAPTION_IGNORE) => return false,
            _ => return self.process_in(Mode::InBody, t),
        }
        true
    }

    fn in_column_group(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => {}
            Token::StartTag(s) => match s.normal.as_str() {
                "html" => return self.process_in(Mode::InBody, t),
                "col" => {
                    self.insert_empty_element_for(s);
                }
                "template" => {
                    self.process_in(Mode::InHead, t);
                }
                _ => return self.in_column_group_anything_else(t),
            },
            Token::EndTag(e) => match e.normal.as_str() {
                "colgroup" => {
                    if !self.current_is("colgroup") {
                        return false;
                    }
                    self.pop();
                    self.transition(Mode::InTable);
                }
                "template" => {
                    self.process_in(Mode::InHead, t);
                }
                _ => return self.in_column_group_anything_else(t),
            },
            Token::Eof => {
                if self.current_is("html") {
                    return true;
                }
                return self.in_column_group_anything_else(t);
            }
            _ => return self.in_column_group_anything_else(t),
        }
        true
    }

    fn in_column_group_anything_else(&mut self, t: &Token) -> bool {
        if !self.current_is("colgroup") {
            return false;
        }
        self.pop();
        self.transition(Mode::InTable);
        self.process(t);
        true
    }

    fn in_table_body(&mut self, t: &Token) -> bool {
        match t {
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if name == "tr" {
                    self.clear_stack_to_table_body_context();
                    self.insert_element_for(s);
                    self.transition(Mode::InRow);
                } else if in_list(name, IN_CELL_NAMES) {
                    self.process_start_tag("tr");
                    return self.process(t);
                } else if in_list(name, IN_TABLE_BODY_EXIT) {
                    return self.exit_table_body(t);
                } else {
                    return self.process_in(Mode::InTable, t);
                }
                true
            }
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if in_list(name, IN_TABLE_END_IGNORE) {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    self.clear_stack_to_table_body_context();
                    self.pop();
                    self.transition(Mode::InTable);
                } else if name == "table" {
                    return self.exit_table_body(t);
                } else if in_list(name, IN_TABLE_BODY_END_IGNORE) {
                    return false;
                } else {
                    return self.process_in(Mode::InTable, t);
                }
                true
            }
            _ => self.process_in(Mode::InTable, t),
        }
    }

    fn exit_table_body(&mut self, t: &Token) -> bool {
        if !(self.in_table_scope("tbody") || self.in_table_scope("thead") || self.in_scope("tfoot")) {
            return false;
        }
        self.clear_stack_to_table_body_context();
        let n = self.name(self.current()).to_string();
        self.process_end_tag(&n);
        self.process(t)
    }

    fn in_row(&mut self, t: &Token) -> bool {
        match t {
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if in_list(name, IN_CELL_NAMES) {
                    self.clear_stack_to_table_row_context();
                    self.insert_element_for(s);
                    self.transition(Mode::InCell);
                    self.insert_marker();
                } else if in_list(name, IN_ROW_MISSING) {
                    if !self.in_table_scope("tr") {
                        return false;
                    }
                    self.clear_stack_to_table_row_context();
                    self.pop();
                    self.transition(Mode::InTableBody);
                    return self.process(t);
                } else {
                    return self.process_in(Mode::InTable, t);
                }
                true
            }
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if name == "tr" {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    self.clear_stack_to_table_row_context();
                    self.pop();
                    self.transition(Mode::InTableBody);
                } else if name == "table" {
                    if !self.in_table_scope("tr") {
                        return false;
                    }
                    self.clear_stack_to_table_row_context();
                    self.pop();
                    self.transition(Mode::InTableBody);
                    return self.process(t);
                } else if in_list(name, IN_TABLE_TO_BODY) {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    if !self.in_table_scope("tr") {
                        return false;
                    }
                    self.clear_stack_to_table_row_context();
                    self.pop();
                    self.transition(Mode::InTableBody);
                    return self.process(t);
                } else if in_list(name, IN_ROW_IGNORE) {
                    return false;
                } else {
                    return self.process_in(Mode::InTable, t);
                }
                true
            }
            _ => self.process_in(Mode::InTable, t),
        }
    }

    fn in_cell(&mut self, t: &Token) -> bool {
        match t {
            Token::EndTag(e) => {
                let name = e.normal.as_str();
                if in_list(name, IN_CELL_NAMES) {
                    if !self.in_table_scope(name) {
                        self.transition(Mode::InRow);
                        return false;
                    }
                    self.generate_implied_end_tags(false);
                    self.pop_stack_to_close(name);
                    self.clear_formatting_elements_to_last_marker();
                    self.transition(Mode::InRow);
                } else if in_list(name, IN_CELL_BODY) {
                    return false;
                } else if in_list(name, IN_CELL_TABLE) {
                    if !self.in_table_scope(name) {
                        return false;
                    }
                    self.close_cell();
                    return self.process(t);
                } else {
                    return self.process_in(Mode::InBody, t);
                }
                true
            }
            Token::StartTag(s) if in_list(&s.normal, IN_CELL_COL) => {
                if !(self.in_table_scope("td") || self.in_table_scope("th")) {
                    return false;
                }
                self.close_cell();
                self.process(t)
            }
            _ => self.process_in(Mode::InBody, t),
        }
    }

    fn close_cell(&mut self) {
        if self.in_table_scope("td") {
            self.process_end_tag("td");
        } else {
            self.process_end_tag("th");
        }
    }

    fn in_select(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { data, .. } => {
                if data == "\u{0000}" {
                    return false;
                }
                self.insert_character(t);
            }
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return false,
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if name == "html" {
                    return self.process_in(Mode::InBody, t);
                } else if name == "option" {
                    if self.current_is("option") {
                        self.process_end_tag("option");
                    }
                    self.insert_element_for(s);
                } else if name == "optgroup" {
                    if self.current_is("option") {
                        self.process_end_tag("option");
                    }
                    if self.current_is("optgroup") {
                        self.process_end_tag("optgroup");
                    }
                    self.insert_element_for(s);
                } else if name == "select" {
                    return self.process_end_tag("select");
                } else if in_list(name, IN_SELECT_END) {
                    if !self.in_select_scope("select") {
                        return false;
                    }
                    self.process_end_tag("select");
                    return self.process(t);
                } else if name == "script" || name == "template" {
                    return self.process_in(Mode::InHead, t);
                } else {
                    return false;
                }
            }
            Token::EndTag(e) => match e.normal.as_str() {
                "optgroup" => {
                    if self.current_is("option") {
                        if let Some(above) = self.above_on_stack(self.current()) {
                            if self.name(above) == "optgroup" {
                                self.process_end_tag("option");
                            }
                        }
                    }
                    if self.current_is("optgroup") {
                        self.pop();
                    }
                }
                "option" => {
                    if self.current_is("option") {
                        self.pop();
                    }
                }
                "select" => {
                    if !self.in_select_scope("select") {
                        return false;
                    }
                    self.pop_stack_to_close("select");
                    self.reset_insertion_mode();
                }
                "template" => return self.process_in(Mode::InHead, t),
                _ => return false,
            },
            Token::Eof => {}
        }
        true
    }

    fn in_select_in_table(&mut self, t: &Token) -> bool {
        match t {
            Token::StartTag(s) if in_list(&s.normal, IN_SELECT_TABLE_END) => {
                self.pop_stack_to_close("select");
                self.reset_insertion_mode();
                self.process(t)
            }
            Token::EndTag(e) if in_list(&e.normal, IN_SELECT_TABLE_END) => {
                if self.in_table_scope(&e.normal) {
                    self.pop_stack_to_close("select");
                    self.reset_insertion_mode();
                    self.process(t)
                } else {
                    false
                }
            }
            _ => self.process_in(Mode::InSelect, t),
        }
    }

    fn in_template(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { .. } | Token::Comment(_) | Token::Doctype { .. } => {
                self.process_in(Mode::InBody, t);
            }
            Token::StartTag(s) => {
                let name = s.normal.as_str();
                if in_list(name, IN_TEMPLATE_TO_HEAD) {
                    self.process_in(Mode::InHead, t);
                } else {
                    let m = if in_list(name, IN_TEMPLATE_TO_TABLE) {
                        Mode::InTable
                    } else if name == "col" {
                        Mode::InColumnGroup
                    } else if name == "tr" {
                        Mode::InTableBody
                    } else if name == "td" || name == "th" {
                        Mode::InRow
                    } else {
                        Mode::InBody
                    };
                    self.tmpl_insert_mode.pop();
                    self.tmpl_insert_mode.push(m);
                    self.transition(m);
                    return self.process(t);
                }
            }
            Token::EndTag(e) => {
                if e.normal == "template" {
                    self.process_in(Mode::InHead, t);
                } else {
                    return false;
                }
            }
            Token::Eof => {
                if !self.on_stack_name("template") {
                    return true;
                }
                self.pop_stack_to_close("template");
                self.clear_formatting_elements_to_last_marker();
                self.tmpl_insert_mode.pop();
                self.reset_insertion_mode();
                if self.state != Mode::InTemplate && self.tmpl_insert_mode.len() < 12 {
                    return self.process(t);
                }
                return true;
            }
        }
        true
    }

    fn after_body(&mut self, t: &Token) -> bool {
        let html = self.get_from_stack("html");
        if t.is_whitespace() {
            match html {
                Some(h) => self.insert_character_to(t, h),
                None => {
                    self.process_in(Mode::InBody, t);
                }
            }
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return false,
            Token::StartTag(s) if s.normal == "html" => return self.process_in(Mode::InBody, t),
            Token::EndTag(e) if e.normal == "html" => self.transition(Mode::AfterAfterBody),
            Token::Eof => {}
            _ => {
                self.reset_body();
                return self.process(t);
            }
        }
        true
    }

    fn in_frameset(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return false,
            Token::StartTag(s) => match s.normal.as_str() {
                "html" => return self.process_in(Mode::InBody, t),
                "frameset" => {
                    self.insert_element_for(s);
                }
                "frame" => {
                    self.insert_empty_element_for(s);
                }
                "noframes" => return self.process_in(Mode::InHead, t),
                _ => return false,
            },
            Token::EndTag(e) if e.normal == "frameset" => {
                if self.current_is("html") {
                    return false;
                }
                self.pop();
                if !self.current_is("frameset") {
                    self.transition(Mode::AfterFrameset);
                }
            }
            Token::Eof => {}
            _ => return false,
        }
        true
    }

    fn after_frameset(&mut self, t: &Token) -> bool {
        if t.is_whitespace() {
            self.insert_character(t);
            return true;
        }
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return false,
            Token::StartTag(s) if s.normal == "html" => return self.process_in(Mode::InBody, t),
            Token::EndTag(e) if e.normal == "html" => self.transition(Mode::AfterAfterFrameset),
            Token::StartTag(s) if s.normal == "noframes" => return self.process_in(Mode::InHead, t),
            Token::Eof => {}
            _ => return false,
        }
        true
    }

    fn after_after_body(&mut self, t: &Token) -> bool {
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return self.process_in(Mode::InBody, t),
            Token::StartTag(s) if s.normal == "html" => return self.process_in(Mode::InBody, t),
            _ if t.is_whitespace() => self.insert_character_to(t, ROOT),
            Token::Eof => {}
            _ => {
                self.reset_body();
                return self.process(t);
            }
        }
        true
    }

    fn after_after_frameset(&mut self, t: &Token) -> bool {
        match t {
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => return self.process_in(Mode::InBody, t),
            _ if t.is_whitespace() => return self.process_in(Mode::InBody, t),
            Token::StartTag(s) if s.normal == "html" => return self.process_in(Mode::InBody, t),
            Token::Eof => {}
            Token::StartTag(s) if s.normal == "noframes" => return self.process_in(Mode::InHead, t),
            _ => return false,
        }
        true
    }

    fn foreign_content(&mut self, t: &Token) -> bool {
        match t {
            Token::Character { data, .. } => {
                if data == "\u{0000}" {
                } else if t.is_whitespace() {
                    self.insert_character(t);
                } else {
                    self.insert_character(t);
                    self.frameset_ok = false;
                }
            }
            Token::Comment(d) => self.insert_comment(d),
            Token::Doctype { .. } => {}
            Token::StartTag(s) => {
                if in_list(&s.normal, IN_FOREIGN_TO_HTML) {
                    return self.process_in(self.state, t);
                }
                if s.normal == "font"
                    && (s.has_attr_ignore_case("color") || s.has_attr_ignore_case("face") || s.has_attr_ignore_case("size"))
                {
                    return self.process_in(self.state, t);
                }
                let ns = self.ns(self.current());
                self.insert_foreign_element_for(s, ns);
            }
            Token::EndTag(e) => {
                if e.normal == "br" || e.normal == "p" {
                    return self.process_in(self.state, t);
                }
                if e.normal == "script" && self.name(self.current()) == "script" && self.ns(self.current()) == Ns::Svg {
                    self.pop();
                    return true;
                }
                let mut i = self.stack.len() - 1;
                let mut el = self.stack[i];
                while i != 0 {
                    if self.name(el) == e.normal {
                        let n = self.name(el).to_string();
                        self.pop_stack_to_close_any_namespace(&n);
                        return true;
                    }
                    i -= 1;
                    el = self.stack[i];
                    if self.ns(el) == Ns::Html {
                        return self.process_in(self.state, t);
                    }
                }
            }
            Token::Eof => {}
        }
        true
    }

    fn handle_rcdata(&mut self, s: &Tag) {
        self.tokeniser.transition(TState::Rcdata);
        self.mark_insertion_mode();
        self.transition(Mode::Text);
        self.insert_element_for(s);
    }

    fn handle_rawtext(&mut self, s: &Tag) {
        self.tokeniser.transition(TState::Rawtext);
        self.mark_insertion_mode();
        self.transition(Mode::Text);
        self.insert_element_for(s);
    }
}

fn attrs_equal(a: &[Attr], b: &[Attr]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| y.key == x.key && y.value.clone().unwrap_or_default() == x.value.clone().unwrap_or_default()))
}
