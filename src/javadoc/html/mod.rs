//! A small jsoup-compatible HTML DOM and parser.
//!
//! The parser (`tokeniser` + `tree_builder`) is a port of jsoup 1.19.1's
//! `Tokeniser`/`TokeniserState` and `HtmlTreeBuilder`/`HtmlTreeBuilderState`,
//! so that `parse` produces the same tree as `Jsoup.parse(html)` — which the
//! Javadoc → Markdown conversion (a flexmark port) depends on. The DOM
//! accessors mirror the jsoup `Node`/`Element`/`TextNode` methods used by
//! flexmark, jdt.ls' `TableHelper` and `HtmlToPlainText`.

pub mod entities;
mod tokeniser;
mod tree_builder;

pub use tree_builder::parse;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Document,
    Element,
    Text,
    Comment,
    /// `DataNode` (script/style content)
    Data,
    /// `CDataNode` (a `TextNode` subclass)
    CData,
    Doctype,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ns {
    Html,
    MathMl,
    Svg,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    pub key: String,
    /// `None` for boolean attributes (`<input checked>`).
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    /// Element: tag name (`tagName()`); lowercase for HTML elements.
    pub name: String,
    /// Element: `normalName()` (lowercase).
    pub normal: String,
    pub ns: Ns,
    pub attrs: Vec<Attr>,
    /// Text / comment / data content.
    pub text: String,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

/// The document arena. `NodeId(0)` is the `#document` root.
#[derive(Clone, Debug)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub quirks: bool,
}

pub const ROOT: NodeId = NodeId(0);

// ─── Tag metadata (jsoup Tag) ────────────────────────────────────────────────

const BLOCK_TAGS: &[&str] = &[
    "html", "head", "body", "frameset", "script", "noscript", "style", "meta", "link", "title", "frame",
    "noframes", "section", "nav", "aside", "hgroup", "header", "footer", "p", "h1", "h2", "h3", "h4", "h5", "h6",
    "ul", "ol", "pre", "div", "blockquote", "hr", "address", "figure", "figcaption", "form", "fieldset", "ins",
    "del", "dl", "dt", "dd", "li", "table", "caption", "thead", "tfoot", "tbody", "colgroup", "col", "tr", "th",
    "td", "video", "audio", "canvas", "details", "menu", "plaintext", "template", "article", "main",
    "svg", "math", "center", "template", "dir", "applet", "marquee", "listing",
];
const INLINE_TAGS: &[&str] = &[
    "object", "base", "font", "tt", "i", "b", "u", "big", "small", "em", "strong", "dfn", "code", "samp", "kbd",
    "var", "cite", "abbr", "time", "acronym", "mark", "ruby", "rt", "rp", "rtc", "a", "img", "br", "wbr", "map", "q",
    "sub", "sup", "bdo", "iframe", "embed", "span", "input", "select", "textarea", "label", "optgroup",
    "option", "legend", "datalist", "keygen", "output", "progress", "meter", "area", "param", "source", "track",
    "summary", "command", "device", "area", "basefont", "bgsound", "menuitem", "param", "source", "track",
    "data", "bdi", "s", "strike", "nobr", "rb", "text", "mi", "mo", "msup", "mn", "mtext",
];
const EMPTY_TAGS: &[&str] = &[
    "meta", "link", "base", "frame", "img", "br", "wbr", "embed", "hr", "input", "keygen", "col", "command",
    "device", "area", "basefont", "bgsound", "menuitem", "param", "source", "track",
];
const FORMAT_AS_INLINE_TAGS: &[&str] = &[
    "title", "a", "p", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "address", "li", "th", "td", "script", "style",
    "ins", "del", "s", "button",
];
const PRESERVE_WHITESPACE_TAGS: &[&str] = &["pre", "plaintext", "title", "textarea"];
const MATHML_TAGS: &[&str] = &["math", "mi", "mo", "msup", "mn", "mtext"];
const SVG_TAGS: &[&str] = &["svg", "text"];

fn tag_ns(name: &str) -> Ns {
    if MATHML_TAGS.contains(&name) {
        Ns::MathMl
    } else if SVG_TAGS.contains(&name) {
        Ns::Svg
    } else {
        Ns::Html
    }
}

/// `Tag.isKnownTag`
pub fn is_known_tag(name: &str) -> bool {
    BLOCK_TAGS.contains(&name) || INLINE_TAGS.contains(&name) || EMPTY_TAGS.contains(&name)
        || FORMAT_AS_INLINE_TAGS.contains(&name) || PRESERVE_WHITESPACE_TAGS.contains(&name)
        || ["button", "fieldset", "input", "keygen", "object", "output", "select", "textarea"].contains(&name)
}

/// Is the registered (known) tag for `name` in namespace `ns`? Mirrors `Tag.valueOf`.
fn registered(name: &str, ns: Ns) -> bool {
    is_known_tag(name) && tag_ns(name) == ns
}

pub fn tag_is_block(name: &str, ns: Ns) -> bool {
    if !registered(name, ns) {
        return false;
    }
    if INLINE_TAGS.contains(&name) {
        return false;
    }
    if BLOCK_TAGS.contains(&name) {
        return true;
    }
    true
}

pub fn tag_format_as_block(name: &str, ns: Ns) -> bool {
    if !registered(name, ns) {
        return true;
    }
    if FORMAT_AS_INLINE_TAGS.contains(&name) {
        return false;
    }
    if INLINE_TAGS.contains(&name) {
        return false;
    }
    true
}

pub fn tag_is_empty(name: &str, ns: Ns) -> bool {
    registered(name, ns) && EMPTY_TAGS.contains(&name)
}

pub fn tag_preserve_whitespace(name: &str, ns: Ns) -> bool {
    registered(name, ns) && PRESERVE_WHITESPACE_TAGS.contains(&name)
}

// ─── jsoup StringUtil helpers ────────────────────────────────────────────────

/// `StringUtil.isActuallyWhitespace`
pub fn is_actually_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{000C}' | '\r' | '\u{00A0}')
}

/// `StringUtil.isWhitespace`
pub fn is_jsoup_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{000C}' | '\r')
}

/// `StringUtil.isBlank`
pub fn is_blank(s: &str) -> bool {
    s.chars().all(is_jsoup_whitespace)
}

fn is_invisible_char(c: char) -> bool {
    c == '\u{200B}' || c == '\u{00AD}'
}

/// `StringUtil.appendNormalisedWhitespace`
pub fn append_normalised_whitespace(accum: &mut String, s: &str, strip_leading: bool) {
    let mut last_was_white = false;
    let mut reached_non_white = false;
    for c in s.chars() {
        if is_actually_whitespace(c) {
            if (strip_leading && !reached_non_white) || last_was_white {
                continue;
            }
            accum.push(' ');
            last_was_white = true;
        } else if !is_invisible_char(c) {
            accum.push(c);
            last_was_white = false;
            reached_non_white = true;
        }
    }
}

/// `StringUtil.normaliseWhitespace`
pub fn normalise_whitespace(s: &str) -> String {
    let mut sb = String::new();
    append_normalised_whitespace(&mut sb, s, false);
    sb
}

/// Java `String.trim()`: strips chars `<= ' '`.
pub fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

fn last_char_is_whitespace(sb: &str) -> bool {
    sb.ends_with(' ')
}

// ─── DOM ─────────────────────────────────────────────────────────────────────

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Document {
            nodes: vec![Node {
                kind: Kind::Document,
                name: "#document".into(),
                normal: "#document".into(),
                ns: Ns::Html,
                attrs: Vec::new(),
                text: String::new(),
                parent: None,
                children: Vec::new(),
            }],
            quirks: false,
        }
    }

    fn push_node(&mut self, n: Node) -> NodeId {
        self.nodes.push(n);
        NodeId(self.nodes.len() - 1)
    }

    pub fn create_element(&mut self, name: &str) -> NodeId {
        self.create_element_ns(name, &name.to_lowercase(), Ns::Html, Vec::new())
    }

    pub fn create_element_ns(&mut self, name: &str, normal: &str, ns: Ns, attrs: Vec<Attr>) -> NodeId {
        self.push_node(Node {
            kind: Kind::Element,
            name: name.to_string(),
            normal: normal.to_string(),
            ns,
            attrs,
            text: String::new(),
            parent: None,
            children: Vec::new(),
        })
    }

    pub fn create_leaf(&mut self, kind: Kind, text: &str) -> NodeId {
        let name = match kind {
            Kind::Text => "#text",
            Kind::Comment => "#comment",
            Kind::Data => "#data",
            Kind::CData => "#cdata",
            Kind::Doctype => "#doctype",
            _ => "#node",
        };
        self.push_node(Node {
            kind,
            name: name.into(),
            normal: name.into(),
            ns: Ns::Html,
            attrs: Vec::new(),
            text: text.to_string(),
            parent: None,
            children: Vec::new(),
        })
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
    }

    pub fn kind(&self, id: NodeId) -> Kind {
        self.nodes[id.0].kind
    }

    pub fn is_element(&self, id: NodeId) -> bool {
        self.kind(id) == Kind::Element
    }

    /// `instanceof TextNode` (includes CDATA nodes).
    pub fn is_text(&self, id: NodeId) -> bool {
        matches!(self.kind(id), Kind::Text | Kind::CData)
    }

    /// `Node.nodeName()`
    pub fn node_name(&self, id: NodeId) -> &str {
        &self.nodes[id.0].name
    }

    /// `Element.tagName()`
    pub fn tag_name(&self, id: NodeId) -> &str {
        &self.nodes[id.0].name
    }

    /// `Element.normalName()`
    pub fn normal_name(&self, id: NodeId) -> &str {
        &self.nodes[id.0].normal
    }

    pub fn ns(&self, id: NodeId) -> Ns {
        self.nodes[id.0].ns
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.0].parent
    }

    /// Parent element (`Element.parent()`): `None` when the parent is the document.
    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        self.parent(id).filter(|p| self.is_element(*p))
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id.0].children
    }

    /// `Node.childNodes()` (a snapshot copy).
    pub fn child_nodes(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[id.0].children.clone()
    }

    pub fn child_node_size(&self, id: NodeId) -> usize {
        self.nodes[id.0].children.len()
    }

    /// `Element.children()` (element children).
    pub fn element_children(&self, id: NodeId) -> Vec<NodeId> {
        self.children(id).iter().copied().filter(|c| self.is_element(*c)).collect()
    }

    pub fn children_size(&self, id: NodeId) -> usize {
        self.children(id).iter().filter(|c| self.is_element(**c)).count()
    }

    pub fn sibling_index(&self, id: NodeId) -> Option<usize> {
        let p = self.parent(id)?;
        self.children(p).iter().position(|c| *c == id)
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.sibling_index(id)?;
        self.children(p).get(i + 1).copied()
    }

    pub fn previous_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.sibling_index(id)?;
        if i == 0 {
            None
        } else {
            self.children(p).get(i - 1).copied()
        }
    }

    pub fn next_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.sibling_index(id)?;
        self.children(p)[i + 1..].iter().copied().find(|c| self.is_element(*c))
    }

    pub fn previous_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.sibling_index(id)?;
        self.children(p)[..i].iter().rev().copied().find(|c| self.is_element(*c))
    }

    /// `Element.firstElementSibling()`
    pub fn first_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        match self.parent(id) {
            Some(p) => self.children(p).iter().copied().find(|c| self.is_element(*c)),
            None => Some(id),
        }
    }

    // ── attributes ──

    pub fn attrs(&self, id: NodeId) -> &[Attr] {
        &self.nodes[id.0].attrs
    }

    /// `Node.hasAttr` (case-insensitive key match).
    pub fn has_attr(&self, id: NodeId, key: &str) -> bool {
        self.attrs(id).iter().any(|a| a.key.eq_ignore_ascii_case(key))
    }

    /// `Node.attr` (case-insensitive; "" when missing).
    pub fn attr(&self, id: NodeId, key: &str) -> String {
        self.attrs(id)
            .iter()
            .find(|a| a.key.eq_ignore_ascii_case(key))
            .map(|a| a.value.clone().unwrap_or_default())
            .unwrap_or_default()
    }

    /// `Node.absUrl` with an empty base URI.
    pub fn abs_url(&self, id: NodeId, key: &str) -> String {
        if !self.has_attr(id, key) {
            return String::new();
        }
        let v: String = self.attr(id, key).chars().filter(|c| !c.is_control()).collect();
        let mut chars = v.chars();
        let ok = match chars.next() {
            Some(c) if c.is_ascii_alphabetic() => {
                let rest: String = chars.collect();
                match rest.find(':') {
                    Some(pos) => rest[..pos].chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'),
                    None => false,
                }
            }
            _ => false,
        };
        if ok {
            // java.net.URL lower-cases the scheme of known protocols
            if let Some(pos) = v.find(':') {
                let scheme = v[..pos].to_ascii_lowercase();
                if ["http", "https", "ftp", "file", "jar", "mailto"].contains(&scheme.as_str()) {
                    return format!("{}{}", scheme, &v[pos..]);
                }
            }
            v
        } else {
            String::new()
        }
    }

    pub fn class_name(&self, id: NodeId) -> String {
        java_trim(&self.attr(id, "class")).to_string()
    }

    /// `Element.classNames()` (ordered, deduped).
    pub fn class_names(&self, id: NodeId) -> Vec<String> {
        let cn = self.class_name(id);
        let mut out: Vec<String> = Vec::new();
        for n in cn.split(|c: char| c.is_whitespace()) {
            if !n.is_empty() && !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
        out
    }

    pub fn has_class(&self, id: NodeId, class: &str) -> bool {
        self.class_names(id).iter().any(|c| c.eq_ignore_ascii_case(class))
    }

    // ── tag info ──

    pub fn is_block(&self, id: NodeId) -> bool {
        self.is_element(id) && tag_is_block(self.normal_name(id), self.ns(id))
    }

    pub fn format_as_block(&self, id: NodeId) -> bool {
        tag_format_as_block(self.normal_name(id), self.ns(id))
    }

    fn preserve_whitespace(&self, node: Option<NodeId>) -> bool {
        let mut el = match node {
            Some(n) if self.is_element(n) => Some(n),
            _ => return false,
        };
        let mut i = 0;
        while let Some(e) = el {
            if tag_preserve_whitespace(self.normal_name(e), self.ns(e)) {
                return true;
            }
            el = self.parent_element(e);
            i += 1;
            if i >= 6 {
                break;
            }
        }
        false
    }

    // ── text ──

    /// `TextNode.getWholeText()`
    pub fn whole_text(&self, id: NodeId) -> &str {
        &self.nodes[id.0].text
    }

    /// `TextNode.text()`
    pub fn text_node_text(&self, id: NodeId) -> String {
        normalise_whitespace(self.whole_text(id))
    }

    fn append_normalised_text(&self, accum: &mut String, text_node: NodeId) {
        let text = self.whole_text(text_node);
        if self.preserve_whitespace(self.parent(text_node)) || self.kind(text_node) == Kind::CData {
            accum.push_str(text);
        } else {
            let strip = last_char_is_whitespace(accum);
            append_normalised_whitespace(accum, text, strip);
        }
    }

    /// `Element.text()`
    pub fn text(&self, id: NodeId) -> String {
        let mut accum = String::new();
        self.traverse(id, &mut |doc, node, enter| {
            if enter {
                if doc.is_text(node) {
                    doc.append_normalised_text(&mut accum, node);
                } else if doc.is_element(node)
                    && !accum.is_empty()
                    && (doc.is_block(node) || doc.normal_name(node) == "br")
                    && !last_char_is_whitespace(&accum)
                {
                    accum.push(' ');
                }
            } else if doc.is_element(node) {
                let next = doc.next_sibling(node);
                let next_ok = match next {
                    Some(n) if doc.is_text(n) => true,
                    Some(n) if doc.is_element(n) => !doc.format_as_block(n),
                    _ => false,
                };
                if doc.is_block(node) && next_ok && !last_char_is_whitespace(&accum) {
                    accum.push(' ');
                }
            }
        });
        java_trim(&accum).to_string()
    }

    /// `Element.ownText()`
    pub fn own_text(&self, id: NodeId) -> String {
        let mut accum = String::new();
        for &child in self.children(id) {
            if self.is_text(child) {
                self.append_normalised_text(&mut accum, child);
            } else if self.is_element(child) && self.normal_name(child) == "br" && !last_char_is_whitespace(&accum) {
                accum.push(' ');
            }
        }
        java_trim(&accum).to_string()
    }

    /// Depth-first traversal (`NodeTraversor`): `visit(doc, node, true)` on head and
    /// `visit(doc, node, false)` on tail.
    pub fn traverse(&self, root: NodeId, visit: &mut dyn FnMut(&Document, NodeId, bool)) {
        visit(self, root, true);
        for &c in self.children(root) {
            self.traverse(c, visit);
        }
        visit(self, root, false);
    }

    /// All elements under `root` (including `root`) in document order.
    pub fn descendants_and_self(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_elements(root, &mut out);
        out
    }

    fn collect_elements(&self, n: NodeId, out: &mut Vec<NodeId>) {
        if self.is_element(n) {
            out.push(n);
        }
        for &c in self.children(n) {
            self.collect_elements(c, out);
        }
    }

    /// `Element.getElementsByTag` / `select("tag")`
    pub fn select_tag(&self, root: NodeId, tag: &str) -> Vec<NodeId> {
        self.descendants_and_self(root).into_iter().filter(|e| self.normal_name(*e) == tag).collect()
    }

    /// `select("anc desc")`: `desc` elements with an `anc` ancestor (up to and including `root`).
    pub fn select_descendant(&self, root: NodeId, anc: &str, desc: &str) -> Vec<NodeId> {
        self.descendants_and_self(root)
            .into_iter()
            .filter(|e| self.normal_name(*e) == desc)
            .filter(|e| {
                if *e == root {
                    return false;
                }
                let mut p = self.parent(*e);
                while let Some(pp) = p {
                    if self.is_element(pp) && self.normal_name(pp) == anc {
                        return true;
                    }
                    if pp == root {
                        break;
                    }
                    p = self.parent(pp);
                }
                false
            })
            .collect()
    }

    pub fn body(&self) -> Option<NodeId> {
        let html = self.children(ROOT).iter().copied().find(|c| self.is_element(*c) && self.normal_name(*c) == "html")?;
        self.children(html)
            .iter()
            .copied()
            .find(|c| self.is_element(*c) && (self.normal_name(*c) == "body" || self.normal_name(*c) == "frameset"))
    }

    // ── mutation ──

    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.nodes[id.0].parent.take() {
            self.nodes[p.0].children.retain(|c| *c != id);
        }
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.nodes[child.0].parent = Some(parent);
        self.nodes[parent.0].children.push(child);
    }

    pub fn prepend_child(&mut self, parent: NodeId, child: NodeId) {
        self.insert_child(parent, 0, child);
    }

    pub fn insert_child(&mut self, parent: NodeId, index: usize, child: NodeId) {
        self.detach(child);
        self.nodes[child.0].parent = Some(parent);
        let len = self.nodes[parent.0].children.len();
        self.nodes[parent.0].children.insert(index.min(len), child);
    }

    /// `node.before(in)`
    pub fn insert_before(&mut self, reference: NodeId, new: NodeId) {
        let p = self.parent(reference).expect("before() on orphan");
        self.detach(new);
        let i = self.sibling_index(reference).unwrap();
        self.nodes[new.0].parent = Some(p);
        self.nodes[p.0].children.insert(i, new);
    }

    /// `node.after(in)`
    pub fn insert_after(&mut self, reference: NodeId, new: NodeId) {
        let p = self.parent(reference).expect("after() on orphan");
        self.detach(new);
        let i = self.sibling_index(reference).unwrap();
        self.nodes[new.0].parent = Some(p);
        self.nodes[p.0].children.insert(i + 1, new);
    }

    pub fn replace_with(&mut self, old: NodeId, new: NodeId) {
        let p = match self.parent(old) {
            Some(p) => p,
            None => return,
        };
        self.detach(new);
        let i = self.sibling_index(old).unwrap();
        self.nodes[p.0].children[i] = new;
        self.nodes[new.0].parent = Some(p);
        self.nodes[old.0].parent = None;
    }

    // ── serialisation (jsoup outerHtml, pretty print off for inline content) ──

    pub fn outer_html(&self, id: NodeId) -> String {
        let mut s = String::new();
        self.outer_html_into(id, &mut s);
        s
    }

    fn outer_html_into(&self, id: NodeId, s: &mut String) {
        let n = self.node(id);
        match n.kind {
            Kind::Text => {
                let t = if self.preserve_whitespace(n.parent) { n.text.clone() } else { normalise_whitespace(&n.text) };
                s.push_str(&escape_text(&t));
            }
            Kind::CData => {
                s.push_str("<![CDATA[");
                s.push_str(&n.text);
                s.push_str("]]>");
            }
            Kind::Data => s.push_str(&n.text),
            Kind::Comment => {
                s.push_str("<!--");
                s.push_str(&n.text);
                s.push_str("-->");
            }
            Kind::Doctype => s.push_str("<!doctype html>"),
            Kind::Document => {
                for &c in &n.children {
                    self.outer_html_into(c, s);
                }
            }
            Kind::Element => {
                s.push('<');
                s.push_str(&n.name);
                for a in &n.attrs {
                    s.push(' ');
                    s.push_str(&a.key);
                    if let Some(v) = &a.value {
                        s.push_str("=\"");
                        s.push_str(&escape_attr(v));
                        s.push('"');
                    }
                }
                if n.children.is_empty() && tag_is_empty(&n.normal, n.ns) {
                    s.push('>');
                    return;
                }
                s.push('>');
                for &c in &n.children {
                    self.outer_html_into(c, s);
                }
                s.push_str("</");
                s.push_str(&n.name);
                s.push('>');
            }
        }
    }
}

fn escape_text(t: &str) -> String {
    let mut o = String::new();
    for c in t.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '\u{00A0}' => o.push_str("&nbsp;"),
            _ => o.push(c),
        }
    }
    o
}

fn escape_attr(t: &str) -> String {
    let mut o = String::new();
    for c in t.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\u{00A0}' => o.push_str("&nbsp;"),
            _ => o.push(c),
        }
    }
    o
}
