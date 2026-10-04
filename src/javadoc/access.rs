//! Javadoc DOM → HTML: port of jdt.core.manipulation `CoreJavadocAccessImpl`
//! with the overrides of jdt.ls `JavadocContentAccess2.JdtLsJavadocAccessImpl`
//! (block tags as `<ul>/<li>` lists, links to `uri#line` locations, snippet
//! handling), plus `JavadocLookup` / `InheritDocVisitor` for `{@inheritDoc}`
//! and `CoreJavadocAccess.createSuperMethodReferencesHTML`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::javadoc::doc_ast::{DocContext, DocNode, DocSource, InheritData, Location, Utf16};
use crate::javadoc::html_builder::convert_to_html_content_with_whitespace;
use crate::javadoc::labels::{method_short_label, CONCAT_STRING, COMMA_STRING};
use crate::javadoc::path_handler;
use crate::javadoc::snippet::SnippetEvaluator;

// JavaDocMessages
const PARAMETERS_SECTION: &str = "Parameters:";
const RETURNS_SECTION: &str = "Returns:";
const THROWS_SECTION: &str = "Throws:";
const TYPE_PARAMETERS_SECTION: &str = "Type Parameters:";
const AUTHOR_SECTION: &str = "Author:";
const DEPRECATED_SECTION: &str = "Deprecated.";
const OVERRIDES_SECTION: &str = "Overrides:";
const SEE_SECTION: &str = "See Also:";
const SINCE_SECTION: &str = "Since:";
const SPECIFIED_BY_SECTION: &str = "Specified by:";
const VERSION_SECTION: &str = "Version:";
const API_NOTE: &str = "API Note:";
const IMPL_SPEC: &str = "Impl Spec:";
const IMPL_NOTE: &str = "Impl Note:";
const USES: &str = "Uses:";
const PROVIDES: &str = "Provides:";
const RETURNS_PRE: &str = "Returns ";
const RETURNS_POST: &str = ".";

// TagElement constants
const TAG_PARAM: &str = "@param";
const TAG_RETURN: &str = "@return";
const TAG_EXCEPTION: &str = "@exception";
const TAG_THROWS: &str = "@throws";
const TAG_PROVIDES: &str = "@provides";
const TAG_USES: &str = "@uses";
const TAG_SINCE: &str = "@since";
const TAG_VERSION: &str = "@version";
const TAG_AUTHOR: &str = "@author";
const TAG_SEE: &str = "@see";
const TAG_DEPRECATED: &str = "@deprecated";
const TAG_API_NOTE: &str = "@apiNote";
const TAG_IMPL_SPEC: &str = "@implSpec";
const TAG_IMPL_NOTE: &str = "@implNote";
const TAG_HIDDEN: &str = "@hidden";
const TAG_VALUE: &str = "@value";
const TAG_LINK: &str = "@link";
const TAG_LINKPLAIN: &str = "@linkplain";
const TAG_CODE: &str = "@code";
const TAG_LITERAL: &str = "@literal";
const TAG_SUMMARY: &str = "@summary";
const TAG_INDEX: &str = "@index";
const TAG_SNIPPET: &str = "@snippet";
const TAG_INHERITDOC: &str = "@inheritDoc";
const TAG_DOCROOT: &str = "@docRoot";

const BLOCK_TAG_TITLE_START: &str = "<dt>";
const BLOCK_TAG_TITLE_END: &str = "</dt>";
const PARAM_NAME_START: &str = "<b>";
const PARAM_NAME_END: &str = "</b> ";

/// Shared, per-hover environment.
pub struct Env<'a> {
    pub inherit: Option<&'a InheritData>,
    /// `{@docRoot}` replacement (source folder URI without trailing `/`).
    pub doc_root: Option<String>,
    /// Formats a resolved location as a link target (`uri#line`, or `""`).
    pub link: &'a dyn Fn(&Location) -> String,
    /// Image `src` values extracted from jars (original `src` → file URI).
    pub images: &'a HashMap<String, String>,
}

/// The element a Javadoc belongs to.
#[derive(Clone, Copy)]
pub struct DocElement<'a> {
    pub doc: &'a DocSource,
    pub ctx: &'a DocContext,
    /// Key of the declaring type (for `{@inheritDoc}` lookups).
    pub type_key: Option<&'a str>,
}

/// `JavadocContentAccess2.getMarkdownContent`'s HTML step for a member:
/// `CoreJavadocAccess.getHTMLContent(element, true)`.
pub fn html_content(env: &Env, element: DocElement, can_inherit: bool) -> Option<String> {
    let html = Access::new(env, element, can_inherit).to_html();
    Some(html)
}

/// What a `{@inheritDoc}` lookup asks an overridden method for.
#[derive(Clone)]
enum Getter {
    Main,
    TypeParam(usize),
    Param(usize),
    Return,
    Exception(String),
}

pub struct Access<'a> {
    env: &'a Env<'a>,
    el: DocElement<'a>,
    src: Utf16,
    /// `fMethod != null`
    has_method: bool,
    buf: String,
    literal_content: i32,
    pre_counter: i32,
    in_pre_code_counter: i32,
    /// Guards `getInheritedTypeParamDescription` re-entrancy (upstream caches
    /// an empty buffer before computing it).
    type_param_in_progress: Vec<usize>,
}

fn is_java_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}')
        || (c.is_whitespace() && c != '\u{A0}' && c != '\u{2007}' && c != '\u{202F}')
}

/// `removeDocLineIntros`: drops `[ \t\x0B\f]*\*` after every line break.
pub fn remove_doc_line_intros(text: &str) -> String {
    strip_after_line_breaks(text, |c| matches!(c, ' ' | '\t' | '\u{0B}' | '\u{0C}'))
}

/// `text.replaceAll("(\r\n?|\n)([ \t]*\\*)", "$1")`
fn remove_star_prefixes(text: &str) -> String {
    strip_after_line_breaks(text, |c| matches!(c, ' ' | '\t'))
}

fn strip_after_line_breaks(text: &str, blank: impl Fn(char) -> bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        let is_break = c == '\n' || c == '\r';
        if !is_break {
            continue;
        }
        if c == '\r' && i < chars.len() && chars[i] == '\n' {
            out.push('\n');
            i += 1;
        }
        let mut j = i;
        while j < chars.len() && blank(chars[j]) {
            j += 1;
        }
        if j < chars.len() && chars[j] == '*' {
            i = j + 1;
        }
    }
    out
}

impl<'a> Access<'a> {
    pub fn new(env: &'a Env<'a>, el: DocElement<'a>, has_method: bool) -> Self {
        Self {
            env,
            el,
            src: Utf16::new(&el.doc.raw),
            has_method,
            buf: String::new(),
            literal_content: 0,
            pre_counter: 0,
            in_pre_code_counter: -1,
            type_param_in_progress: Vec::new(),
        }
    }

    fn node(&self, id: usize) -> &'a DocNode {
        &self.el.doc.nodes[id]
    }

    fn tags(&self) -> &'a [usize] {
        &self.el.doc.tags
    }

    // ─── names of the method (initTypeParameterNames & co) ──────────────────

    fn type_parameter_names(&self) -> Vec<Option<String>> {
        if !self.has_method {
            return Vec::new();
        }
        self.el.ctx.type_parameter_names.iter().cloned().map(Some).collect()
    }

    fn parameter_names(&self) -> Vec<Option<String>> {
        if !self.has_method {
            return Vec::new();
        }
        self.el.ctx.parameter_names.iter().cloned().map(Some).collect()
    }

    fn exception_names(&self) -> Vec<Option<String>> {
        if !self.has_method {
            return Vec::new();
        }
        self.el.ctx.exception_names.iter().cloned().map(Some).collect()
    }

    fn needs_return_tag(&self) -> bool {
        self.has_method && !self.el.ctx.returns_void
    }

    // ─── toHTML ──────────────────────────────────────────────────────────────

    pub fn to_html(mut self) -> String {
        self.buf.clear();
        self.literal_content = 0;
        self.element_to_html();
        std::mem::take(&mut self.buf)
    }

    fn element_to_html(&mut self) {
        let mut type_parameter_names = self.type_parameter_names();
        let mut parameter_names = self.parameter_names();
        let mut exception_names = self.exception_names();

        let mut deprecated: Option<usize> = None;
        let mut start: Option<usize> = None;
        let mut type_parameters = Vec::new();
        let mut parameters = Vec::new();
        let mut exceptions = Vec::new();
        let mut provides = Vec::new();
        let mut uses = Vec::new();
        let mut versions = Vec::new();
        let mut authors = Vec::new();
        let mut sees = Vec::new();
        let mut since = Vec::new();
        let mut rest = Vec::new();
        let mut apinote = Vec::new();
        let mut implspec = Vec::new();
        let mut implnote = Vec::new();
        let mut hidden = Vec::new();

        for &tag in self.tags() {
            let t = self.node(tag);
            match t.tag_name() {
                None => start = Some(tag),
                Some(TAG_PARAM) => {
                    let fr = &t.fragments;
                    if let Some(&first) = fr.first() {
                        let first = self.node(first);
                        if first.is_simple_name() {
                            let name = first.identifier.clone();
                            if let Some(i) = parameter_names.iter().position(|n| n.as_deref() == name.as_deref()) {
                                parameter_names[i] = None;
                            }
                            parameters.push(tag);
                        } else if fr.len() > 2 && first.is_text() && first.text() == "<" {
                            let second = self.node(fr[1]);
                            let third = self.node(fr[2]);
                            if second.is_simple_name() && third.is_text() && third.text() == ">" {
                                let name = second.identifier.clone();
                                if let Some(i) =
                                    type_parameter_names.iter().position(|n| n.as_deref() == name.as_deref())
                                {
                                    type_parameter_names[i] = None;
                                }
                                type_parameters.push(tag);
                            }
                        }
                    }
                }
                Some(TAG_RETURN) => {}
                Some(TAG_EXCEPTION) | Some(TAG_THROWS) => {
                    exceptions.push(tag);
                    if let Some(&first) = t.fragments.first() {
                        let first = self.node(first);
                        if first.is_name() {
                            let name = first.identifier.clone();
                            if let Some(i) = exception_names.iter().position(|n| n.as_deref() == name.as_deref()) {
                                exception_names[i] = None;
                            }
                        }
                    }
                }
                Some(TAG_PROVIDES) => provides.push(tag),
                Some(TAG_USES) => uses.push(tag),
                Some(TAG_SINCE) => since.push(tag),
                Some(TAG_VERSION) => versions.push(tag),
                Some(TAG_AUTHOR) => authors.push(tag),
                Some(TAG_SEE) => sees.push(tag),
                Some(TAG_DEPRECATED) => {
                    if deprecated.is_none() {
                        deprecated = Some(tag);
                    }
                }
                Some(TAG_API_NOTE) => apinote.push(tag),
                Some(TAG_IMPL_SPEC) => implspec.push(tag),
                Some(TAG_IMPL_NOTE) => implnote.push(tag),
                Some(TAG_HIDDEN) => hidden.push(tag),
                Some(_) => rest.push(tag),
            }
        }

        let mut return_tags = Vec::new();
        self.find_tags(TAG_RETURN, &mut return_tags, self.tags());
        let return_tag = return_tags.first().copied();

        if let Some(d) = deprecated {
            self.handle_deprecated_tag(d);
        }
        if let Some(s) = start {
            let fr = self.node(s).fragments.clone();
            self.handle_content_elements(&fr, false, None);
        } else if self.has_method {
            let inherited = self.inherited(Getter::Main);
            self.handle_inherited(inherited);
        }

        let mut type_parameter_descriptions: Vec<Option<String>> = vec![None; type_parameter_names.len()];
        let has_inherited_type_parameters =
            self.inherit_descriptions(&type_parameter_names, &mut type_parameter_descriptions, true);
        let has_type_parameters = !type_parameters.is_empty() || has_inherited_type_parameters;

        let mut parameter_descriptions: Vec<Option<String>> = vec![None; parameter_names.len()];
        let has_inherited_parameters = self.inherit_descriptions(&parameter_names, &mut parameter_descriptions, false);
        let has_parameters = !parameters.is_empty() || has_inherited_parameters;

        let mut return_description = None;
        if return_tag.is_none() && self.needs_return_tag() {
            return_description = self.inherited(Getter::Return);
        }
        let has_return_tag = return_tag.is_some() || return_description.is_some();

        let mut exception_descriptions: Vec<Option<String>> = vec![None; exception_names.len()];
        let mut has_inherited_exceptions = false;
        for (i, name) in exception_names.iter().enumerate() {
            if let Some(name) = name {
                exception_descriptions[i] = self.inherited(Getter::Exception(name.clone()));
                if exception_descriptions[i].is_some() {
                    has_inherited_exceptions = true;
                }
            }
        }
        let has_exceptions = !exceptions.is_empty() || has_inherited_exceptions;

        if has_parameters
            || has_type_parameters
            || has_return_tag
            || has_exceptions
            || !versions.is_empty()
            || !authors.is_empty()
            || !since.is_empty()
            || !sees.is_empty()
            || !apinote.is_empty()
            || !implnote.is_empty()
            || !implspec.is_empty()
            || !uses.is_empty()
            || !provides.is_empty()
            || !hidden.is_empty()
            || !rest.is_empty()
            || (!self.buf.is_empty() && (!parameter_descriptions.is_empty() || !exception_descriptions.is_empty()))
        {
            self.handle_super_method_references();
            self.buf.push_str("<ul>");
            self.handle_parameter_tags(&type_parameters, &type_parameter_names, &type_parameter_descriptions, true);
            self.handle_parameter_tags(&parameters, &parameter_names, &parameter_descriptions, false);
            self.handle_return_tag(return_tag, return_description.as_deref());
            self.handle_exception_tags(&exceptions, &exception_names, &exception_descriptions);
            self.handle_block_tags_titled(SINCE_SECTION, &since);
            self.handle_block_tags_titled(VERSION_SECTION, &versions);
            self.handle_block_tags_titled(AUTHOR_SECTION, &authors);
            self.handle_block_tags_titled(SEE_SECTION, &sees);
            self.handle_block_tags_titled(API_NOTE, &apinote);
            self.handle_block_tags_titled(IMPL_SPEC, &implspec);
            self.handle_block_tags_titled(IMPL_NOTE, &implnote);
            self.handle_block_tags_titled(USES, &uses);
            self.handle_block_tags_titled(PROVIDES, &provides);
            if !hidden.is_empty() {
                self.handle_block_tags_hidden();
            }
            self.handle_block_tags_rest(&rest);
            self.buf.push_str("</ul>");
        } else if !self.buf.is_empty() {
            self.handle_super_method_references();
        }
    }

    fn inherit_descriptions(&mut self, names: &[Option<String>], out: &mut [Option<String>], type_params: bool) -> bool {
        let mut has = false;
        for (i, name) in names.iter().enumerate() {
            if name.is_some() {
                out[i] = self.inherited(if type_params { Getter::TypeParam(i) } else { Getter::Param(i) });
                if out[i].is_some() {
                    has = true;
                }
            }
        }
        has
    }

    fn find_tags(&self, name: &str, found: &mut Vec<usize>, tags: &[usize]) {
        for &id in tags {
            let n = self.node(id);
            if n.is_tag() {
                if n.tag_name() == Some(name) {
                    found.push(id);
                }
                self.find_tags(name, found, &n.fragments);
            }
        }
    }

    fn handle_block_tags_hidden(&mut self) {
        let s = self.buf.replace("<ul>", "<dl hidden>");
        let s = s.replace(BLOCK_TAG_TITLE_START, "<dt hidden>");
        let s = s.replace("<li>", "<dd hidden>");
        let s = s.replace(PARAM_NAME_START, "<b hidden>");
        self.buf = s;
    }

    fn handle_deprecated_tag(&mut self, tag: usize) {
        self.buf.push_str("<p><b>");
        self.buf.push_str(DEPRECATED_SECTION);
        self.buf.push_str("</b> <i>");
        let fr = self.node(tag).fragments.clone();
        self.handle_content_elements(&fr, false, None);
        self.buf.push_str("</i><p>");
    }

    /// `handleSuperMethodReferences` → `createSuperMethodReferencesHTMLStaticImpl`.
    fn handle_super_method_references(&mut self) {
        if !self.has_method {
            return;
        }
        let Some(inherit) = self.env.inherit else { return };
        let Some(start) = self.el.type_key else { return };
        let types: HashMap<&str, &crate::javadoc::doc_ast::HierarchyType> =
            inherit.types.iter().map(|t| (t.key.as_str(), t)).collect();
        let mut interface_methods: Vec<(String, String)> = Vec::new();
        let mut class_method: Option<(String, String)> = None;
        visit_inherit_doc(inherit, &types, start, &mut |key| {
            let t = types[key];
            let Some(o) = &t.overridden else { return Visit::Continue };
            let entry = (method_short_label(&o.name, o.has_parameters), o.declaring_type_name.clone());
            if t.is_interface {
                interface_methods.push(entry);
            } else {
                class_method = Some(entry);
            }
            Visit::StopBranch
        });
        if interface_methods.is_empty() && class_method.is_none() {
            return;
        }
        let link = |label: &str| format!("<a href='eclipse-javadoc:%E2%98%82{label}'>{label}</a>");
        let method_in_type = |(m, t): &(String, String)| format!("{} in {}", link(m), link(t));
        self.buf.push_str("<div>");
        if !interface_methods.is_empty() {
            self.buf.push_str("<b>");
            self.buf.push_str(SPECIFIED_BY_SECTION);
            self.buf.push_str("</b> ");
            let parts: Vec<String> = interface_methods.iter().map(method_in_type).collect();
            self.buf.push_str(&parts.join(COMMA_STRING));
        }
        if let Some(cm) = &class_method {
            if !interface_methods.is_empty() {
                self.buf.push_str(COMMA_STRING);
            }
            self.buf.push_str("<b>");
            self.buf.push_str(OVERRIDES_SECTION);
            self.buf.push_str("</b> ");
            self.buf.push_str(&method_in_type(cm));
        }
        self.buf.push_str("</div>");
    }

    // ─── Descriptions used by JavadocLookup ─────────────────────────────────

    fn with_buffer(&mut self, f: impl FnOnce(&mut Self)) -> Option<String> {
        let saved = std::mem::take(&mut self.buf);
        self.literal_content = 0;
        f(self);
        let out = std::mem::replace(&mut self.buf, saved);
        (!out.is_empty()).then_some(out)
    }

    fn describe(&mut self, getter: &Getter) -> Option<String> {
        match getter {
            Getter::Main => self.main_description(),
            Getter::TypeParam(i) => self.inherited_type_param_description(*i),
            Getter::Param(i) => self.inherited_param_description(*i),
            Getter::Return => self.return_description(),
            Getter::Exception(name) => self.exception_description(name),
        }
    }

    fn main_description(&mut self) -> Option<String> {
        let tags = self.tags();
        self.with_buffer(|s| {
            for &tag in tags {
                if s.node(tag).tag_name().is_none() {
                    let fr = s.node(tag).fragments.clone();
                    s.handle_content_elements(&fr, false, None);
                    break;
                }
            }
        })
    }

    fn return_description(&mut self) -> Option<String> {
        let mut return_tags = Vec::new();
        self.find_tags(TAG_RETURN, &mut return_tags, self.tags());
        self.with_buffer(|s| {
            if let Some(&t) = return_tags.first() {
                let fr = s.node(t).fragments.clone();
                s.handle_content_elements(&fr, false, None);
            }
        })
    }

    fn inherited_type_param_description(&mut self, index: usize) -> Option<String> {
        if !self.has_method || self.type_param_in_progress.contains(&index) {
            return None;
        }
        let names = self.type_parameter_names();
        let Some(Some(name)) = names.get(index).cloned() else { return None };
        self.type_param_in_progress.push(index);
        let tags = self.tags();
        let out = self.with_buffer(|s| {
            for &tag in tags {
                let t = s.node(tag);
                if t.tag_name() == Some(TAG_PARAM) && t.fragments.len() > 2 {
                    let (a, b, c) = (s.node(t.fragments[0]), s.node(t.fragments[1]), s.node(t.fragments[2]));
                    if a.is_text() && b.is_simple_name() && c.is_text() && a.text() == "<" && c.text() == ">"
                        && b.identifier.as_deref() == Some(name.as_str())
                    {
                        let fr = t.fragments[3..].to_vec();
                        s.handle_content_elements(&fr, false, None);
                        break;
                    }
                }
            }
        });
        out
    }

    fn inherited_param_description(&mut self, index: usize) -> Option<String> {
        if !self.has_method {
            return None;
        }
        let names = self.el.ctx.parameter_names.clone();
        let name = names.get(index)?.clone();
        let tags = self.tags();
        self.with_buffer(|s| {
            for &tag in tags {
                let t = s.node(tag);
                if t.tag_name() == Some(TAG_PARAM) {
                    if let Some(&first) = t.fragments.first() {
                        let f = s.node(first);
                        if f.is_simple_name() && f.identifier.as_deref() == Some(name.as_str()) {
                            let fr = t.fragments[1..].to_vec();
                            s.handle_content_elements(&fr, false, None);
                            break;
                        }
                    }
                }
            }
        })
    }

    fn exception_description(&mut self, simple_name: &str) -> Option<String> {
        if !self.has_method {
            return None;
        }
        let tags = self.tags();
        self.with_buffer(|s| {
            for &tag in tags {
                let t = s.node(tag);
                if matches!(t.tag_name(), Some(TAG_THROWS) | Some(TAG_EXCEPTION)) {
                    if let Some(&first) = t.fragments.first() {
                        let f = s.node(first);
                        if f.is_name() && f.identifier.as_deref() == Some(simple_name) {
                            if t.fragments.len() > 1 {
                                let fr = t.fragments[1..].to_vec();
                                s.handle_content_elements(&fr, false, None);
                            }
                            break;
                        }
                    }
                }
            }
        })
    }

    /// `JavadocLookup.getInheritedDescription(fMethod, getter)`
    fn inherited(&self, getter: Getter) -> Option<String> {
        if !self.has_method {
            return None;
        }
        let inherit = self.env.inherit?;
        let start = self.el.type_key?;
        let types: HashMap<&str, &crate::javadoc::doc_ast::HierarchyType> =
            inherit.types.iter().map(|t| (t.key.as_str(), t)).collect();
        let mut result = None;
        visit_inherit_doc(inherit, &types, start, &mut |key| {
            let t = types[key];
            let Some(o) = &t.overridden else { return Visit::Continue };
            let Some(doc) = &o.javadoc else {
                return if o.has_source { Visit::Continue } else { Visit::StopBranch };
            };
            let el = DocElement { doc, ctx: &o.doc_context, type_key: Some(t.key.as_str()) };
            let mut access = Access::new(self.env, el, true);
            match access.describe(&getter) {
                Some(d) => {
                    result = Some(d);
                    Visit::Found
                }
                None => Visit::Continue,
            }
        });
        result
    }

    // ─── content ─────────────────────────────────────────────────────────────

    fn handle_content_elements(&mut self, nodes: &[usize], skip_leading_whitespace: bool, tag_element: Option<usize>) {
        let mut previous: Option<usize> = None;
        for &child in nodes {
            let c = self.node(child);
            if let Some(prev) = previous {
                let p = self.node(prev);
                let previous_end = p.end();
                let child_start = c.s;
                if previous_end > child_start {
                    // should never happen (logged upstream)
                } else if previous_end != child_start {
                    let text = remove_doc_line_intros(&self.src.substring(previous_end, child_start));
                    self.buf.push_str(&text);
                }
            } else if let Some(te) = tag_element {
                if self.pre_counter >= 1 {
                    let t = self.node(te);
                    let child_start = c.s;
                    let previous_end = t.s + t.tag_name().map(|n| n.encode_utf16().count()).unwrap_or(0) as i64 + 1;
                    let text = remove_doc_line_intros(&self.src.substring(previous_end, child_start));
                    self.buf.push_str(&text);
                }
            }
            previous = Some(child);
            if c.is_text() {
                self.handle_in_line_text_element(child, skip_leading_whitespace, tag_element);
            } else if c.is_tag() {
                self.handle_inline_tag_element(child);
            } else {
                let text = self.src.substring(c.s, c.end());
                self.buf.push_str(&remove_doc_line_intros(&text));
            }
        }
    }

    /// jdt.ls `handleInLineTextElement`
    fn handle_in_line_text_element(&mut self, te: usize, skip_leading_whitespace: bool, tag_element: Option<usize>) {
        let mut text = self.node(te).text().to_owned();
        if path_handler::contains_html_tag(&text) {
            if self.el.doc.class_file.is_some() {
                if let Some((start, end)) = path_handler::extract_source_path_from_html_tag(&text) {
                    if let Some(uri) = self.env.images.get(&text[start..end]) {
                        text = path_handler::replace_src(&text, uri);
                    }
                }
            } else {
                let dir = self.package_dir();
                text = path_handler::validated_html_src_attribute(&text, dir.as_deref());
            }
        }
        if skip_leading_whitespace {
            if let Some(c) = text.chars().next() {
                if is_java_whitespace(c) {
                    text = text[c.len_utf8()..].to_owned();
                }
            }
        }
        let text = remove_star_prefixes(&text);
        let text = self.handle_pre_counter(tag_element, text);
        // handleInLineText(text, previousNode): previousNode is the text
        // element itself, so it is never inside a snippet.
        self.handle_text(&text);
    }

    fn package_dir(&self) -> Option<PathBuf> {
        let uri = self.el.doc.uri.as_deref()?;
        let path = url::Url::parse(uri).ok()?.to_file_path().ok()?;
        path.parent().map(|p| p.to_path_buf())
    }

    fn handle_pre_counter(&mut self, tag_element: Option<usize>, text: String) -> String {
        if tag_element.is_none() && text == "<pre>" {
            self.pre_counter += 1;
        } else if tag_element.is_none() && text == "</pre>" {
            self.pre_counter -= 1;
            if self.pre_counter == self.in_pre_code_counter {
                self.in_pre_code_counter = -1;
            }
        } else if tag_element.is_none() && self.pre_counter > 0 && matches_brace_end_pre(&text) {
            self.pre_counter -= 1;
            if self.pre_counter == self.in_pre_code_counter {
                if let Some(last) = self.buf.rfind("</code>") {
                    self.buf.replace_range(last..last + 7, "");
                }
                self.in_pre_code_counter = -1;
                return "</code></pre>".to_owned();
            }
        }
        text
    }

    fn handle_text(&mut self, text: &str) {
        if self.literal_content == 0 {
            handle_unicode(&mut self.buf, text);
        } else {
            let escaped = append_escaped(text);
            handle_unicode(&mut self.buf, &escaped);
        }
    }

    /// jdt.ls `handleInlineTagElement`
    fn handle_inline_tag_element(&mut self, id: usize) {
        let node = self.node(id);
        let name = node.tag_name().unwrap_or("");
        if name == TAG_VALUE && self.handle_value_tag(id) {
            return;
        }
        let is_link = name == TAG_LINK;
        let is_linkplain = name == TAG_LINKPLAIN;
        let is_code = name == TAG_CODE;
        let is_literal = name == TAG_LITERAL;
        let is_summary = name == TAG_SUMMARY;
        let is_index = name == TAG_INDEX;
        let is_snippet = name == TAG_SNIPPET;
        let is_return = name == TAG_RETURN;

        if is_literal || is_code || is_summary || is_index {
            self.literal_content += 1;
        }
        if is_code {
            if self.pre_counter > 0 && self.buf.rfind("<pre>").map(|i| i as i64) == Some(self.buf.len() as i64 - 5) {
                self.in_pre_code_counter = self.pre_counter - 1;
            }
            self.buf.push_str("<code>");
        }
        if is_return {
            self.buf.push_str(RETURNS_PRE);
        }
        let fragments = node.fragments.clone();
        if is_link || is_linkplain {
            self.handle_link(&fragments);
        } else if is_summary {
            self.handle_summary(&fragments);
        } else if is_index {
            self.handle_index(&fragments);
        } else if is_code || is_literal {
            self.handle_content_elements(&fragments, true, Some(id));
        } else if is_return {
            self.handle_content_elements(&fragments, false, Some(id));
        } else if is_snippet {
            self.handle_snippet(id);
        } else if self.handle_inherit_doc(id) || self.handle_doc_root(id) {
            // handled
        } else {
            let text = self.src.substring(node.s, node.end());
            self.buf.push_str(&remove_doc_line_intros(&text));
        }
        if is_return {
            self.buf.push_str(RETURNS_POST);
        }
        if is_code {
            self.buf.push_str("</code>");
        }
        if is_snippet {
            self.buf.push_str("</code></pre>");
        }
        if is_literal || is_code || is_summary || is_index {
            self.literal_content -= 1;
        }
    }

    fn handle_value_tag(&mut self, id: usize) -> bool {
        let node = self.node(id);
        if !matches!(self.el.ctx.kind.as_str(), "method" | "field" | "type") {
            return false;
        }
        if node.fragments.is_empty() {
            if self.el.ctx.kind == "field" && self.el.ctx.static_final {
                if let Some(c) = &self.el.ctx.constant {
                    let text = constant_text(c);
                    let text = convert_to_html_content_with_whitespace(&text);
                    self.handle_text(&text);
                    return true;
                }
            }
        } else if node.fragments.len() == 1 {
            let first = self.node(node.fragments[0]);
            if first.t == "memberRef" {
                if let Some(v) = &first.value {
                    if let Some(c) = &v.constant {
                        let text = convert_to_html_content_with_whitespace(&constant_text(c));
                        let uri = v.location.as_ref().map(|l| (self.env.link)(l)).unwrap_or_default();
                        self.buf.push_str(&format!("<a href='{uri}'>{text}</a>"));
                        return true;
                    }
                }
            }
        }
        false
    }

    fn handle_doc_root(&mut self, id: usize) -> bool {
        if self.node(id).tag_name() != Some(TAG_DOCROOT) {
            return false;
        }
        match &self.env.doc_root {
            Some(url) => {
                let url = url.strip_suffix('/').unwrap_or(url);
                self.buf.push_str(url);
                true
            }
            None => false,
        }
    }

    fn handle_inherit_doc(&mut self, id: usize) -> bool {
        let node = self.node(id);
        if node.tag_name() != Some(TAG_INHERITDOC) {
            return false;
        }
        if !self.has_method {
            return false;
        }
        let Some(parent) = node.p.map(|p| self.node(p)) else { return false };
        match parent.tag_name() {
            None => {
                let inherited = self.inherited(Getter::Main);
                self.handle_inherited(inherited)
            }
            Some(TAG_PARAM) => {
                let fr = &parent.fragments;
                if let Some(&first) = fr.first() {
                    let first = self.node(first);
                    if first.is_simple_name() {
                        let name = first.identifier.clone().unwrap_or_default();
                        if let Some(i) = self.el.ctx.parameter_names.iter().position(|n| *n == name) {
                            let inherited = self.inherited(Getter::Param(i));
                            return self.handle_inherited(inherited);
                        }
                    } else if fr.len() > 2 && first.is_text() && first.text() == "<" {
                        let second = self.node(fr[1]);
                        let third = self.node(fr[2]);
                        if second.is_simple_name() && third.is_text() && third.text() == ">" {
                            let name = second.identifier.clone().unwrap_or_default();
                            if let Some(i) = self.el.ctx.type_parameter_names.iter().position(|n| *n == name) {
                                // upstream asks itself, not the lookup
                                let inherited = self.inherited_type_param_description(i);
                                return self.handle_inherited(inherited);
                            }
                        }
                    }
                }
                false
            }
            Some(TAG_RETURN) => {
                let inherited = self.inherited(Getter::Return);
                self.handle_inherited(inherited)
            }
            Some(TAG_THROWS) | Some(TAG_EXCEPTION) => {
                if let Some(&first) = parent.fragments.first() {
                    let first = self.node(first);
                    if first.is_name() {
                        let name = first.identifier.clone().unwrap_or_default();
                        let inherited = self.inherited(Getter::Exception(name));
                        return self.handle_inherited(inherited);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn handle_inherited(&mut self, inherited: Option<String>) -> bool {
        match inherited {
            Some(s) => {
                self.buf.push_str(&s);
                true
            }
            None => false,
        }
    }

    // ─── block tags (jdt.ls overrides) ───────────────────────────────────────

    fn handle_block_tags_titled(&mut self, title: &str, tags: &[usize]) {
        if tags.is_empty() {
            return;
        }
        self.handle_block_tag_title(title);
        self.buf.push_str("<ul>");
        for &tag in tags {
            self.handle_single_tag(tag);
        }
        self.buf.push_str("</ul>");
        self.buf.push_str("</li>");
    }

    fn handle_single_tag(&mut self, tag: usize) {
        self.buf.push_str("<li>");
        let fr = self.node(tag).fragments.clone();
        if self.node(tag).tag_name() == Some(TAG_SEE) {
            self.handle_link(&fr);
        } else {
            self.handle_content_elements(&fr, false, None);
        }
        self.buf.push_str("</li>");
    }

    fn handle_return_tag(&mut self, tag: Option<usize>, description: Option<&str>) {
        if tag.is_none() && description.is_none() {
            return;
        }
        self.handle_block_tag_title(RETURNS_SECTION);
        // jdt.ls handleReturnTagBody
        let Some(tag) = tag else { return };
        self.buf.push_str("<ul>");
        self.buf.push_str("<li>");
        let fr = self.node(tag).fragments.clone();
        self.handle_content_elements(&fr, false, None);
        self.buf.push_str("</li>");
        self.buf.push_str("</ul>");
        self.buf.push_str("</li>");
    }

    fn handle_block_tags_rest(&mut self, tags: &[usize]) {
        for &tag in tags {
            let name = self.node(tag).tag_name().unwrap_or("").to_owned();
            self.handle_block_tag_title(&name);
            // jdt.ls handleBlockTagBody
            let fr = self.node(tag).fragments.clone();
            if !fr.is_empty() {
                self.buf.push_str("<ul>");
                self.buf.push_str("<li>");
                self.handle_content_elements(&fr, false, None);
                self.buf.push_str("</li>");
                self.buf.push_str("</ul>");
            }
        }
    }

    fn handle_block_tag_title(&mut self, title: &str) {
        self.buf.push_str("<li><b>");
        self.buf.push_str(title);
        self.buf.push_str("</b>");
    }

    fn handle_exception_tags(&mut self, tags: &[usize], names: &[Option<String>], descriptions: &[Option<String>]) {
        if tags.is_empty() && names.iter().all(|n| n.is_none()) {
            return;
        }
        self.handle_block_tag_title(THROWS_SECTION);
        // jdt.ls handleExceptionTagsBody
        if tags.is_empty() && names.is_empty() {
            return;
        }
        self.buf.push_str("<ul>");
        for &tag in tags {
            self.buf.push_str("<li>");
            self.handle_throws_tag(tag);
            self.buf.push_str("</li>");
        }
        for (i, d) in descriptions.iter().enumerate() {
            if let Some(name) = &names[i] {
                self.handle_single_exception(name, d.as_deref());
            }
        }
        self.buf.push_str("</ul>");
        self.buf.push_str("</li>");
    }

    fn handle_single_exception(&mut self, name: &str, description: Option<&str>) {
        self.buf.push_str("<li>");
        // handleLink(newSimpleName(name))
        let uri = self.el.ctx.exception_links.get(name).map(|l| (self.env.link)(l)).unwrap_or_default();
        self.buf.push_str(&format!("<a href='{uri}'>{name}</a>"));
        if let Some(d) = description {
            self.buf.push_str(CONCAT_STRING);
            self.buf.push_str(d);
        }
        self.buf.push_str("</li>");
    }

    fn handle_throws_tag(&mut self, tag: usize) {
        let fr = self.node(tag).fragments.clone();
        if !fr.is_empty() {
            self.handle_link(&fr[..1]);
            if fr.len() > 1 {
                self.buf.push_str(CONCAT_STRING);
                self.handle_content_elements(&fr[1..], false, None);
            }
        }
    }

    fn handle_parameter_tags(&mut self, tags: &[usize], names: &[Option<String>], descriptions: &[Option<String>], type_params: bool) {
        if tags.is_empty() && names.iter().all(|n| n.is_none()) {
            return;
        }
        self.handle_block_tag_title(if type_params { TYPE_PARAMETERS_SECTION } else { PARAMETERS_SECTION });
        if !tags.is_empty() {
            self.buf.push_str("<ul>");
            for &tag in tags {
                self.buf.push_str("<li>");
                self.handle_param_tag(tag);
                self.buf.push_str("</li>");
            }
            self.buf.push_str("</ul>");
        }
        for (i, d) in descriptions.iter().enumerate() {
            if let Some(name) = &names[i] {
                self.buf.push_str("<ul>");
                self.buf.push_str("<li>");
                self.buf.push_str(PARAM_NAME_START);
                if type_params {
                    self.buf.push_str("&lt;");
                }
                self.buf.push_str(name);
                if type_params {
                    self.buf.push_str("&gt;");
                }
                self.buf.push_str(PARAM_NAME_END);
                if let Some(d) = d {
                    self.buf.push_str(d);
                }
                self.buf.push_str("</li>");
                self.buf.push_str("</ul>");
            }
        }
    }

    fn handle_param_tag(&mut self, tag: usize) {
        let fr = self.node(tag).fragments.clone();
        let mut i = 0;
        if fr.is_empty() {
            return;
        }
        let first = self.node(fr[0]);
        self.buf.push_str(PARAM_NAME_START);
        if first.is_simple_name() {
            self.buf.push_str(first.identifier.as_deref().unwrap_or(""));
            i += 1;
        } else if first.is_text() && first.text() == "<" {
            self.buf.push_str("&lt;");
            i += 1;
            if fr.len() > 1 {
                let second = self.node(fr[1]);
                if second.is_simple_name() {
                    self.buf.push_str(second.identifier.as_deref().unwrap_or(""));
                    i += 1;
                    if fr.len() > 2 {
                        let third = self.node(fr[2]);
                        if third.text() == ">" {
                            self.buf.push_str("&gt;");
                            i += 1;
                        }
                    }
                }
            }
        }
        self.buf.push_str(PARAM_NAME_END);
        self.handle_content_elements(&fr[i..], false, None);
    }

    fn handle_summary(&mut self, fragments: &[usize]) {
        if let Some(&first) = fragments.first() {
            let f = self.node(first);
            if f.is_text() {
                self.buf.push_str(&format!("{BLOCK_TAG_TITLE_START}Summary: {}{BLOCK_TAG_TITLE_END}", f.text()));
            }
        }
    }

    /// jdt.ls `handleSnippet`
    fn handle_snippet(&mut self, id: usize) {
        let node = self.node(id);
        let props = node.props.clone().unwrap_or_default();
        if props.valid == Some(true) && props.error.is_none() {
            if !node.fragments.is_empty() {
                self.buf.push_str("<pre>");
                match props.id.as_deref() {
                    Some(sid) if !sid.trim().is_empty() => self.buf.push_str(&format!("<code id={sid}>")),
                    _ => self.buf.push_str("<code>"),
                }
                SnippetEvaluator::new(self.el.doc).add_tag_element_string(id, &mut self.buf);
            }
        } else {
            self.buf.push_str("<pre><code>\n");
            self.buf.push_str("<mark>invalid @Snippet</mark>");
            if let Some(err) = &props.error {
                self.buf.push_str(&format!("<br><p>{err}</p>"));
            }
        }
    }

    fn handle_index(&mut self, fragments: &[usize]) {
        if let Some(&first) = fragments.first() {
            let f = self.node(first);
            if f.is_text() {
                self.buf.push_str(f.text());
            }
        }
    }

    /// jdt.ls `handleLink` (strips `##anchor` references) + upstream `handleLink`.
    fn handle_link(&mut self, fragments: &[usize]) {
        if fragments.is_empty() {
            return;
        }
        let first = self.node(fragments[0]);
        if first.is_text() && first.text().contains("##") {
            let interesting = replace_first_anchor(first.text());
            self.handle_text(&interesting);
            return;
        }
        let mut ref_type_name: Option<String> = None;
        let mut ref_member_name: Option<String> = None;
        let mut params: Option<Vec<(String, Option<String>)>> = None;
        match first.t.as_str() {
            "name" => ref_type_name = first.fqn.clone(),
            "memberRef" => {
                ref_type_name = Some(first.qualifier.clone().unwrap_or_default());
                ref_member_name = first.name.clone();
            }
            "methodRef" => {
                ref_type_name = Some(first.qualifier.clone().unwrap_or_default());
                ref_member_name = first.name.clone();
                params = Some(first.params.iter().map(|p| (p.ty.clone(), p.name.clone())).collect());
            }
            "text" => {
                let parent = first.p.map(|p| self.node(p));
                if parent.is_some_and(|p| matches!(p.tag_name(), Some(TAG_LINK) | Some(TAG_LINKPLAIN))) {
                    ref_type_name = Some(first.text().to_owned());
                }
            }
            _ => {}
        }
        let Some(ref_type_name) = ref_type_name else {
            self.handle_content_elements(fragments, false, None);
            return;
        };
        let uri = first.link.as_ref().map(|l| (self.env.link)(l)).unwrap_or_default();
        self.buf.push_str("<a href='");
        self.buf.push_str(&uri);
        self.buf.push_str("'>");
        let fs = fragments.len();
        let second_is_ws = fs == 2 && {
            let s = self.node(fragments[1]);
            s.is_text() && s.text().trim().is_empty()
        };
        if fs > 1 && !second_is_ws {
            self.handle_content_elements(&fragments[1..], true, None);
        } else {
            self.buf.push_str(&ref_type_name);
            if let Some(member) = &ref_member_name {
                if !ref_type_name.is_empty() {
                    self.buf.push('.');
                }
                self.buf.push_str(member);
                if let Some(ps) = &params {
                    self.buf.push('(');
                    for (i, (ty, name)) in ps.iter().enumerate() {
                        self.buf.push_str(ty);
                        if let Some(n) = name {
                            self.buf.push(' ');
                            self.buf.push_str(n);
                        }
                        if i + 1 < ps.len() {
                            self.buf.push_str(", ");
                        }
                    }
                    self.buf.push(')');
                }
            }
        }
        self.buf.push_str("</a>");
    }
}

/// `text.replaceFirst("##\\w+\\s*", "")`
fn replace_first_anchor(text: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"##[A-Za-z0-9_]+\s*").unwrap());
    RE.replacen(text, 1, "").into_owned()
}

/// `text.matches("}\\s*</pre>")`
fn matches_brace_end_pre(text: &str) -> bool {
    text.strip_prefix('}')
        .and_then(|r| r.strip_suffix("</pre>"))
        .is_some_and(|mid| mid.chars().all(is_java_whitespace))
}

/// `handleUnicode`: `\uXXXX` → `&#xXXXX;`
fn handle_unicode(buf: &mut String, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut next_to_copy = 0usize;
    let mut seen_backslash = false;
    let mut i = 0usize;
    while i < len {
        let ch = chars[i];
        let mut rep: Option<String> = None;
        match ch {
            '\\' => seen_backslash = true,
            'u' => {
                if seen_backslash {
                    seen_backslash = false;
                    if i + 4 < len && chars[i + 1..=i + 4].iter().all(|c| c.to_digit(16).is_some()) {
                        rep = Some(format!("&#x{}{}{}{};", chars[i + 1], chars[i + 2], chars[i + 3], chars[i + 4]));
                    }
                }
            }
            _ => seen_backslash = false,
        }
        if let Some(rep) = rep {
            if next_to_copy < i {
                buf.extend(&chars[next_to_copy..i - 1]);
            }
            buf.push_str(&rep);
            i += 4;
            next_to_copy = i + 1;
        }
        i += 1;
    }
    if next_to_copy < len {
        buf.extend(&chars[next_to_copy..]);
    }
}

fn append_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// `handleConstantValue` text: escaped string literal for strings,
/// `toString()` otherwise.
pub fn constant_text(c: &crate::javadoc::doc_ast::Constant) -> String {
    if c.kind == "string" {
        escaped_string_literal(&c.value)
    } else {
        c.value.clone()
    }
}

/// `org.eclipse.jdt.internal.compiler.util.Util.appendEscapedChar`
fn append_escaped_char(buf: &mut String, c: char, string_literal: bool) {
    match c {
        '\u{8}' => buf.push_str("\\b"),
        '\t' => buf.push_str("\\t"),
        '\n' => buf.push_str("\\n"),
        '\u{c}' => buf.push_str("\\f"),
        '\r' => buf.push_str("\\r"),
        '"' => {
            if string_literal {
                buf.push_str("\\\"");
            } else {
                buf.push(c);
            }
        }
        '\'' => {
            if string_literal {
                buf.push(c);
            } else {
                buf.push_str("\\'");
            }
        }
        '\\' => buf.push_str("\\\\"),
        _ => {
            let v = c as u32;
            if v >= 0x20 {
                buf.push(c);
            } else if v >= 0x10 {
                buf.push_str(&format!("\\u00{:x}", v));
            } else if v >= 0x01 {
                buf.push_str(&format!("\\u000{:x}", v));
            } else {
                buf.push(c);
            }
        }
    }
}

/// `ASTNodes.getEscapedStringLiteral`
pub fn escaped_string_literal(s: &str) -> String {
    let mut b = String::from("\"");
    for c in s.chars() {
        append_escaped_char(&mut b, c, true);
    }
    b.push('"');
    b
}

/// `ASTNodes.getEscapedCharacterLiteral`
pub fn escaped_character_literal(c: char) -> String {
    let mut b = String::from("'");
    append_escaped_char(&mut b, c, false);
    b.push('\'');
    b
}

// ─── InheritDocVisitor ───────────────────────────────────────────────────────

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Visit {
    Continue,
    StopBranch,
    /// A non-CONTINUE, non-STOP_BRANCH result: stop the whole visit.
    Found,
}

/// `InheritDocVisitor.visitInheritDoc(currentType, hierarchy)`.
pub fn visit_inherit_doc(
    data: &InheritData,
    types: &HashMap<&str, &crate::javadoc::doc_ast::HierarchyType>,
    current: &str,
    visit: &mut dyn FnMut(&str) -> Visit,
) {
    let mut visited: Vec<String> = vec![current.to_owned()];
    if visit_interfaces(types, &mut visited, current, visit) != Visit::Continue {
        return;
    }
    let Some(cur) = types.get(current) else { return };
    let mut super_class = if cur.is_interface { data.object.clone() } else { cur.superclass.clone() };
    while let Some(sc) = super_class.clone() {
        if visited.contains(&sc) || !types.contains_key(sc.as_str()) {
            break;
        }
        match visit(&sc) {
            Visit::StopBranch => return,
            Visit::Continue => {
                visited.push(sc.clone());
                if visit_interfaces(types, &mut visited, &sc, visit) != Visit::Continue {
                    return;
                }
                super_class = types[sc.as_str()].superclass.clone();
            }
            Visit::Found => return,
        }
    }
}

fn visit_interfaces(
    types: &HashMap<&str, &crate::javadoc::doc_ast::HierarchyType>,
    visited: &mut Vec<String>,
    current: &str,
    visit: &mut dyn FnMut(&str) -> Visit,
) -> Visit {
    let Some(cur) = types.get(current) else { return Visit::Continue };
    let mut to_visit_children = Vec::new();
    for itf in &cur.interfaces {
        if visited.contains(itf) || !types.contains_key(itf.as_str()) {
            continue;
        }
        visited.push(itf.clone());
        match visit(itf) {
            Visit::StopBranch => {}
            Visit::Continue => to_visit_children.push(itf.clone()),
            Visit::Found => return Visit::Found,
        }
    }
    for child in to_visit_children {
        let r = visit_interfaces(types, visited, &child, visit);
        if r != Visit::Continue {
            return r;
        }
    }
    Visit::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_line_intros() {
        assert_eq!(remove_doc_line_intros("\n\t * a\n * b"), "\n a\n b");
        assert_eq!(remove_doc_line_intros("\r\n  *x"), "\r\nx");
    }

    #[test]
    fn unicode_escapes() {
        let mut b = String::new();
        handle_unicode(&mut b, "a\\u0041b");
        assert_eq!(b, "a&#x0041;b");
    }

    #[test]
    fn literal_escaping() {
        assert_eq!(escaped_string_literal("a\"b\n"), "\"a\\\"b\\n\"");
    }
}
