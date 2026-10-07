//! Port of jdt.ls `CompletionProposalReplacementProvider`: insert texts,
//! text edits (insert/replace ranges), argument placeholders and import
//! edits of completion items.

use super::doc::Doc;
use super::guesser::ParameterGuesser;
use super::imports::{ContainerTypes, CuStructure, ImportRewrite};
use super::item::{Item, ItemTextEdit};
use super::prefs::{Client, GuessMode, Prefs};
use super::proposal::{kind, Context, Proposal, VisibleElement};
use super::signature as sig;
use std::collections::BTreeMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{InsertReplaceEdit, InsertTextFormat, InsertTextMode, Range, TextEdit};

const CURSOR_POSITION: &str = "${0}";

/// An override stub computed in Rust from JDT binding data: text with '\n'
/// delimiters and the types whose imports it needs, in order.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct OverrideStub {
    pub text: String,
    pub imports: Vec<String>,
}

/// Precomputed replacement strings jdt.ls computes with JDT DOM rewrites.
#[derive(Debug, Clone, Default)]
pub struct Stubs {
    /// Override stubs keyed by proposal completion location + name + signature.
    pub overrides: BTreeMap<String, OverrideStub>,
    /// `AnonymousTypeCompletionProposal` new body (formatted `new A() {...}`).
    pub anonymous_body: Option<String>,
    /// Getter/setter stubs keyed by method name.
    pub accessors: BTreeMap<String, String>,
}

pub fn override_key(p: &Proposal) -> String {
    format!("{}|{}|{}", p.replace_start, p.name(), p.signature())
}

pub struct ReplacementProvider<'a> {
    pub doc: &'a Doc,
    pub cu: Arc<CuStructure>,
    pub context: &'a Context,
    pub offset: usize,
    pub prefs: &'a Prefs,
    pub client: &'a Client,
    pub resolving: bool,
    pub source_level: &'a str,
    pub container_types: &'a ContainerTypes,
    pub visible_elements: &'a BTreeMap<String, Vec<VisibleElement>>,
    pub stubs: &'a Stubs,
    pub line_delimiter: String,
    pub blank_lines_between_import_groups: usize,
    pub space_before_semicolon: bool,
    /// The unit is `package-info.java`.
    pub is_package_info: bool,
    /// Simple name of the unit's main type (file name without extension).
    pub main_type_name: String,
    /// `ContextSensitiveImportRewriteContext` data for the resolve request.
    pub context_types: Option<&'a super::import_context::ImportContext>,
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `CompletionUtils.sanitizeCompletion`.
pub fn sanitize(s: &str) -> String {
    s.replace('$', "\\$")
}

impl<'a> ReplacementProvider<'a> {
    fn range(&self, start: i32, end: i32) -> Range {
        let s = start.max(0) as usize;
        let e = end.max(start).max(0) as usize;
        self.doc.range(s, e - s)
    }

    fn new_rewrite(&self) -> ImportRewrite {
        ImportRewrite::create(
            self.cu.clone(),
            self.prefs.import_order.clone(),
            self.prefs.on_demand_threshold,
            self.prefs.static_on_demand_threshold,
        )
    }

    /// `updateReplacement`.
    pub fn update_replacement(&self, proposal: &Proposal, item: &mut Item, trigger: char) {
        let mut rewrite = self.new_rewrite();
        let mut additional: Vec<TextEdit> = Vec::new();
        let mut buffer = String::new();
        let mut insert: Option<Range> = None;
        let mut replace: Option<Range> = None;
        if is_supporting_required_proposals(proposal) {
            for required in proposal.required() {
                match required.kind {
                    kind::TYPE_IMPORT | kind::METHOD_IMPORT | kind::FIELD_IMPORT => {
                        self.append_import_proposal(&mut buffer, required, proposal.kind, &mut rewrite);
                    }
                    kind::TYPE_REF => {
                        let edit = self.to_required_type_edit(required, trigger, proposal.can_use_diamond, &mut rewrite);
                        if matches!(
                            proposal.kind,
                            kind::CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_DECLARATION
                        ) {
                            buffer.push_str(&edit.new_text);
                            self.set_insert_replace_range(required, &mut insert, &mut replace);
                        } else if !is_chain_completion(proposal) {
                            additional.push(edit);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.set_insert_replace_range(proposal, &mut insert, &mut replace);
        let text = self.text_edit_text(proposal, item, buffer, &mut insert, &mut replace, &mut rewrite);

        item.insert_text_format = Some(if self.client.snippets { InsertTextFormat::SNIPPET } else { InsertTextFormat::PLAIN_TEXT });
        if self.resolving
            || (!self.client.item_defaults_property("insertTextMode")
                && self.client.insert_text_mode_default != Some(InsertTextMode::ADJUST_INDENTATION))
        {
            item.insert_text_mode = Some(InsertTextMode::ADJUST_INDENTATION);
        }

        match (insert, replace) {
            (Some(ins), Some(rep)) => {
                if self.client.insert_replace {
                    item.text_edit = Some(ItemTextEdit::InsertReplace(InsertReplaceEdit { new_text: text, insert: ins, replace: rep }));
                } else if self.prefs.overwrite {
                    item.text_edit = Some(ItemTextEdit::Edit(TextEdit::new(rep, text)));
                } else {
                    item.text_edit = Some(ItemTextEdit::Edit(TextEdit::new(ins, text)));
                }
            }
            _ => {
                item.insert_text = Some(text.clone());
                if self.client.item_defaults_support() {
                    item.text_edit_text = Some(super::snippets::template_to_snippet(&text));
                }
            }
        }

        if !is_import_completion(proposal) && (!self.client.resolve_additional_text_edits() || self.resolving) {
            if let Some((off, len, new_text)) = rewrite.rewrite(
                self.container_types,
                &self.line_delimiter,
                self.blank_lines_between_import_groups,
                self.space_before_semicolon,
            ) {
                let edit = TextEdit::new(self.doc.range(off, len), new_text);
                if !(edit.range == Range::default() && edit.new_text.is_empty()) {
                    additional.push(edit);
                }
            }
            if !additional.is_empty() {
                item.additional_text_edits = Some(additional);
            }
        }
    }

    fn text_edit_text(
        &self,
        proposal: &Proposal,
        item: &Item,
        mut buffer: String,
        insert: &mut Option<Range>,
        replace: &mut Option<Range>,
        rewrite: &mut ImportRewrite,
    ) -> String {
        if self.prefs.lazy_resolve_text_edit && !self.resolving {
            if !buffer.is_empty() {
                return buffer;
            }
            let default_text = item
                .insert_text
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| Some(item.label.clone()).filter(|s| !s.trim().is_empty()))
                .unwrap_or_default();
            let start = proposal.replace_start.max(0) as usize;
            let end = proposal.replace_end.max(proposal.replace_start).max(0) as usize;
            let to_replace = self.doc.get(start, end - start);
            if default_text.starts_with(&to_replace) {
                return default_text;
            }
        }
        match proposal.kind {
            kind::METHOD_DECLARATION => {
                // OverrideCompletionProposal.updateReplacementString
                match self.stubs.overrides.get(&override_key(proposal)) {
                    Some(stub) => {
                        for t in &stub.imports {
                            rewrite.add_import(t);
                        }
                        buffer.push_str(&stub.text.replace('\n', &self.line_delimiter));
                    }
                    None => {
                        buffer.push_str(proposal.completion());
                        buffer.push_str(" {};");
                    }
                }
            }
            kind::POTENTIAL_METHOD_DECLARATION => {
                if let Some(gs) = &proposal.getter_setter {
                    let key = format!("{}{}", if gs.is_getter { "get:" } else { "set:" }, gs.field.name);
                    if let Some(s) = self.stubs.accessors.get(&key) {
                        buffer.push_str(s);
                    }
                } else {
                    self.append_replacement_string(&mut buffer, proposal, false, rewrite);
                }
            }
            kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_DECLARATION => {
                self.append_anonymous_class(&mut buffer, proposal, insert, replace);
            }
            kind::LAMBDA_EXPRESSION => self.append_lambda(&mut buffer, proposal),
            _ => {
                let overloaded = item.label_details.as_ref().and_then(|d| d.detail.as_deref()) == Some("(...)");
                self.append_replacement_string(&mut buffer, proposal, overloaded, rewrite);
            }
        }
        buffer
    }

    fn append_lambda(&self, buffer: &mut String, p: &Proposal) {
        let mut params = String::new();
        self.append_guessing_completion(&mut params, p);
        let needs_parens = params.contains(',') || params.is_empty();
        if needs_parens {
            buffer.push('(');
        }
        buffer.push_str(&params);
        if needs_parens {
            buffer.push(')');
        }
        buffer.push_str(" -> ");
        if self.client.snippets {
            buffer.push_str(CURSOR_POSITION);
        }
    }

    fn append_anonymous_class(&self, buffer: &mut String, p: &Proposal, insert: &mut Option<Range>, replace: &mut Option<Range>) {
        if p.declaration_key.is_none() {
            return;
        }
        let Some(body) = self.stubs.anonymous_body.as_deref() else { return };
        let doc = self.doc;
        let len = doc.len();
        let offset = p.replace_start.max(0) as usize;
        let mut replacement = anonymous_new_body(doc, body, offset);
        if len > offset {
            let ch = |i: usize| doc.char_at(i);
            if p.kind == kind::ANONYMOUS_CLASS_DECLARATION {
                let mut length = 0u32;
                let (line_off, line_len) = doc.line_info_of_offset(offset);
                let line_end = line_off + line_len;
                let mut pos = offset;
                let mut c = ch(pos);
                while pos < line_end && pos < len - 1 && !(c == ';' || c == ',') {
                    length += 1;
                    pos += 1;
                    c = ch(pos);
                }
                if let Some(r) = replace.as_mut() {
                    r.end.character += length;
                }
                if let Some(r) = insert.as_mut() {
                    r.end.character += length;
                }
                let mut length = 1u32;
                if offset >= 1 {
                    let mut pos = offset - 1;
                    if pos < len {
                        let line_start = line_off;
                        let mut c = ch(pos);
                        while pos > line_start && !(c == '(' || c == ';' || c == ',') {
                            length += 1;
                            pos -= 1;
                            c = ch(pos);
                        }
                    }
                }
                if let Some(r) = replace.as_mut() {
                    r.start.character = r.start.character.saturating_sub(length);
                }
                if let Some(r) = insert.as_mut() {
                    r.start.character = r.start.character.saturating_sub(length);
                }
                replacement = check_replacement_end(doc, replacement, offset);
            } else {
                let mut pos: isize = -1;
                if ch(offset) == '(' {
                    pos = offset as isize;
                } else if offset + 1 < len && ch(offset + 1) == '(' {
                    pos = offset as isize + 1;
                }
                if pos > 0 && (pos as usize) < len - 1 {
                    let (line_off, line_len) = doc.line_info_of_offset(offset);
                    let mut length = 1u32;
                    let line_end = line_off + line_len;
                    let mut p2 = pos as usize;
                    let mut c = ch(p2);
                    let mut closed = false;
                    while p2 < line_end && !(c == ')' || c == ';' || c == ',') {
                        length += 1;
                        p2 += 1;
                        c = ch(p2);
                        if c == ')' {
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        length = 0;
                        p2 = p2.saturating_sub(1);
                    }
                    pos = p2 as isize;
                    if length > 0 {
                        if let Some(r) = replace.as_mut() {
                            r.end.character += length;
                        }
                        if let Some(r) = insert.as_mut() {
                            r.end.character += length;
                        }
                    }
                }
                let next = if pos > 0 { pos as usize + 1 } else { offset };
                replacement = check_replacement_end(doc, replacement, next);
            }
        }
        buffer.push_str(&replacement);
    }

    fn set_insert_replace_range(&self, p: &Proposal, insert: &mut Option<Range>, replace: &mut Option<Range>) {
        let start = p.replace_start;
        let end = p.replace_end;
        if replace.is_none() {
            *replace = Some(self.range(start, end));
        }
        if insert.is_none() {
            let e = end.min(self.offset as i32);
            *insert = Some(self.range(start, e));
        }
    }

    fn has_argument_list(&self, p: &Proposal) -> bool {
        if p.kind == kind::METHOD_NAME_REFERENCE {
            return false;
        }
        if p.kind == kind::LAMBDA_EXPRESSION {
            return true;
        }
        let c = p.completion();
        !self.context.in_javadoc && c.ends_with(')')
    }

    fn append_replacement_string(&self, buffer: &mut String, p: &Proposal, overloaded: bool, rewrite: &mut ImportRewrite) {
        let snippets = self.client.snippets;
        if !self.has_argument_list(p) {
            let mut s;
            if p.kind == kind::TYPE_REF {
                s = self.java_type_replacement_string(p, rewrite);
                if snippets {
                    s = sanitize(&s);
                }
                if p.array_dimensions > 0 {
                    let mut a = s.clone();
                    for i in 0..p.array_dimensions {
                        a.push('[');
                        if snippets {
                            a.push('$');
                            a.push_str(&(i + 1).to_string());
                        }
                        a.push(']');
                    }
                    if snippets {
                        a.push_str("$0");
                    }
                    s = a;
                }
            } else {
                s = p.completion().to_owned();
                if snippets {
                    s = sanitize(&s);
                    if p.kind == kind::PACKAGE_REF && s.ends_with(".*;") {
                        s = s.replace(".*;", ".${0:*};");
                    }
                }
            }
            buffer.push_str(&s);
            return;
        }
        self.append_method_name_replacement(buffer, p);
        if snippets {
            buffer.push('(');
        }
        if self.has_parameters(p) || overloaded {
            self.append_guessing_completion(buffer, p);
        }
        if snippets {
            buffer.push(')');
            if self.can_append_semicolon(p) {
                buffer.push(';');
            }
        }
        if p.kind == kind::METHOD_DECLARATION {
            self.append_body(buffer);
        }
    }

    fn append_body(&self, buffer: &mut String) {
        if self.client.snippets {
            let s = sanitize(buffer);
            *buffer = s;
        }
        buffer.push_str(" {\n\t");
        if self.client.snippets {
            buffer.push_str(CURSOR_POSITION);
            buffer.push_str("\n}");
        }
    }

    fn has_parameters(&self, p: &Proposal) -> bool {
        self.has_argument_list(p) && sig::get_parameter_count(p.signature()).unwrap_or(0) > 0
    }

    fn can_append_semicolon(&self, p: &Proposal) -> bool {
        !p.constructor && sig::get_return_type(p.signature()).ok().as_deref() == Some("V")
    }

    fn append_method_name_replacement(&self, buffer: &mut String, p: &Proposal) {
        if p.kind == kind::METHOD_REF_WITH_CASTED_RECEIVER {
            let mut c = p.completion().to_owned();
            if self.client.snippets {
                c = sanitize(&c);
            }
            buffer.push_str(&c);
        }
        if p.kind != kind::CONSTRUCTOR_INVOCATION {
            let mut s = p.name().to_owned();
            if self.client.snippets {
                let core = p.completion().to_owned();
                if is_chain_completion(p) {
                    s = match core.rfind('(') {
                        Some(i) => core[..i].to_owned(),
                        None => core,
                    };
                } else {
                    s = sanitize(&s);
                }
            }
            buffer.push_str(&s);
        }
    }

    fn append_guessing_completion(&self, buffer: &mut String, p: &Proposal) {
        if !self.client.snippets {
            return;
        }
        if self.prefs.guess_mode == GuessMode::Off || self.prefs.collapse {
            buffer.push_str(CURSOR_POSITION);
            return;
        }
        let names = p.parameter_names();
        let count = names.len();
        let mut choices: Option<Vec<String>> = None;
        if self.prefs.guess_mode == GuessMode::InsertBestGuessedArguments
            && matches!(p.kind, kind::METHOD_REF | kind::CONSTRUCTOR_INVOCATION | kind::METHOD_REF_WITH_CASTED_RECEIVER)
        {
            choices = Some(self.guess_parameters(&names, p));
        }
        let placeholder_offset = buffer.matches("${").count() + 1;
        for i in 0..count {
            if i != 0 {
                buffer.push_str(", ");
            }
            let mut argument = match &choices {
                Some(c) => c[i].clone(),
                None => names[i].clone(),
            };
            if self.client.snippets {
                argument = sanitize(&argument);
            }
            buffer.push_str("${");
            buffer.push_str(&(i + placeholder_offset).to_string());
            buffer.push(':');
            buffer.push_str(&argument);
            buffer.push('}');
        }
    }

    fn guess_parameters(&self, names: &[String], p: &Proposal) -> Vec<String> {
        let s = sig::fix83600(p.signature());
        let types = sig::get_parameter_types(&s).unwrap_or_default();
        let mut guesser = ParameterGuesser::new(self.context);
        let mut result = vec![String::new(); names.len()];
        for i in (0..names.len()).rev() {
            let t = types.get(i).cloned().unwrap_or_default();
            let type_name = sig::to_string(&t).unwrap_or_default();
            let empty = Vec::new();
            let elements = self.visible_elements.get(&t).unwrap_or(&empty);
            result[i] = guesser.parameter_proposals(&type_name, &names[i], elements).unwrap_or_else(|| names[i].clone());
        }
        result
    }

    fn to_required_type_edit(&self, type_proposal: &Proposal, trigger: char, can_use_diamond: bool, rewrite: &mut ImportRewrite) -> TextEdit {
        let mut buffer = String::new();
        self.append_replacement_string(&mut buffer, type_proposal, false, rewrite);
        let range = self.range(type_proposal.replace_start, type_proposal.replace_end);
        if crate::features::completion::version_less_than(&normalize_version(self.source_level), "1.5") {
            return TextEdit::new(range, buffer);
        }
        let completion = type_proposal.completion();
        if completion.ends_with(';') || completion.ends_with('.') {
            return TextEdit::new(range, buffer);
        }
        let only_append_arguments = completion.is_empty() && self.offset > 0 && self.doc.char_at(self.offset - 1) == '<';
        if only_append_arguments || self.should_append_arguments(type_proposal, trigger) {
            let args = type_proposal.type_arguments.clone().unwrap_or_default();
            if !args.is_empty() {
                if can_use_diamond {
                    buffer.push_str("<>");
                } else {
                    if !only_append_arguments {
                        buffer.push('<');
                    }
                    buffer.push_str(&args.join(","));
                    if !only_append_arguments {
                        buffer.push('>');
                    }
                }
            }
        }
        TextEdit::new(range, buffer)
    }

    fn should_append_arguments(&self, p: &Proposal, trigger: char) -> bool {
        if trigger != '\0' && trigger != '<' && trigger != '(' {
            return false;
        }
        if p.completion().is_empty() {
            return false;
        }
        let end = p.replace_end.max(0) as usize;
        let (line_off, line_len) = self.doc.line_info_of_offset(end);
        let line: Vec<char> = self.doc.get(line_off, line_len).chars().collect();
        let mut index = end.saturating_sub(line_off);
        while index < line.len() && is_unicode_identifier_part(line[index]) {
            index += 1;
        }
        if index >= line.len() {
            return true;
        }
        line[index] != '<'
    }

    fn append_import_proposal(&self, buffer: &mut String, p: &Proposal, core_kind: i32, rewrite: &mut ImportRewrite) {
        let qualified = if p.kind == kind::TYPE_IMPORT {
            sig::to_string(p.signature()).unwrap_or_default()
        } else {
            let e = sig::get_type_erasure(p.declaration_signature.as_deref().unwrap_or("")).unwrap_or_default();
            sig::to_string(&e).unwrap_or_default()
        };
        if p.kind == kind::TYPE_IMPORT {
            let simple = rewrite.add_import(&qualified);
            if core_kind == kind::METHOD_REF {
                buffer.push_str(&simple);
                buffer.push(',');
            }
        } else {
            let res = rewrite.add_static_import(&qualified, p.name(), p.kind == kind::FIELD_IMPORT);
            if let Some(dot) = res.rfind('.') {
                buffer.push_str(&rewrite.add_import(&res[..dot]));
                buffer.push('.');
            }
        }
    }

    /// `computeJavaTypeReplacementString`.
    fn java_type_replacement_string(&self, p: &Proposal, rewrite: &mut ImportRewrite) -> String {
        let replacement = p.completion().to_owned();
        if is_import_completion(p) {
            return replacement;
        }
        if p.kind == kind::TYPE_REF && self.context.in_javadoc_text {
            return sig::simple_type_name(p.signature());
        }
        let qualified = sig::qualified_type_name(p.signature());
        if self.is_package_info {
            return qualified;
        }
        if !qualified.contains('.') && !replacement.is_empty() {
            return qualified;
        }
        let start = p.replace_start.max(0) as usize;
        let end = p.replace_end.max(p.replace_start).max(0) as usize;
        let prefix = self.doc.get(start, end - start);
        if let Some(dot) = prefix.rfind('.') {
            if qualified.to_lowercase().starts_with(&prefix[..=dot].to_lowercase()) {
                return qualified;
            }
        }
        if !replacement.contains('.') {
            if self.context.in_javadoc {
                return sig::simple_type_name(p.signature());
            }
            return replacement;
        }
        match self.context_types {
            Some(context) if self.resolving => {
                let ctx = move |rw: &ImportRewrite, qualifier: &str, name: &str, kind: i32| -> i32 {
                    context.find_in_context(rw, qualifier, name, kind)
                };
                rewrite.add_import_with(&qualified, Some(&ctx))
            }
            _ => rewrite.add_import(&qualified),
        }
    }
}

/// The rest of `AnonymousTypeCompletionProposal.updateReplacementString`
/// after formatting `new A() {...}` (`formatted`): append ';' unless the
/// line continues with ';', ',' or ')', then keep the text from '('.
fn anonymous_new_body(doc: &Doc, formatted: &str, offset: usize) -> String {
    let mut replacement = formatted.to_owned();
    let (line_off, line_len) = doc.line_info_of_offset(offset);
    let line_end = line_off + line_len;
    let mut p = offset;
    if p < doc.len() {
        let mut ch = doc.char_at(p);
        while p < line_end {
            if matches!(ch, '(' | ')' | ';' | ',') {
                break;
            }
            p += 1;
            ch = doc.char_at(p);
        }
        if ch != ';' && ch != ',' && ch != ')' {
            replacement.push(';');
        }
    }
    match replacement.find('(') {
        Some(i) => replacement[i..].to_owned(),
        None => replacement,
    }
}

fn check_replacement_end(doc: &Doc, replacement: String, pos: usize) -> String {
    let len = doc.len();
    let mut replacement = replacement;
    if pos > 0 && pos < len {
        let mut pos = pos;
        let mut next = doc.char_at(pos);
        while pos > 0
            && pos < len - 1
            && !(next == '(' || next == ')' || next == ';' || next == ',' || is_java_identifier_start(next))
        {
            pos += 1;
            next = doc.char_at(pos);
        }
        if (next == ',' || next == ';' || next == '(') && replacement.ends_with(';') {
            replacement.pop();
        }
    }
    replacement
}

pub fn is_java_identifier_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

pub fn is_unicode_identifier_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn normalize_version(v: &str) -> String {
    match v {
        "5" | "6" | "7" | "8" => format!("1.{v}"),
        _ => v.to_owned(),
    }
}

fn is_supporting_required_proposals(p: &Proposal) -> bool {
    matches!(
        p.kind,
        kind::METHOD_REF
            | kind::FIELD_REF
            | kind::TYPE_REF
            | kind::CONSTRUCTOR_INVOCATION
            | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION
            | kind::ANONYMOUS_CLASS_DECLARATION
    )
}

/// `CompletionProposalUtils.isImportCompletion`.
pub fn is_import_completion(p: &Proposal) -> bool {
    let c = p.completion();
    c.ends_with(';') || c.ends_with('.')
}

fn is_chain_completion(p: &Proposal) -> bool {
    (p.kind == kind::FIELD_REF || p.kind == kind::METHOD_REF) && p.completion().contains('.')
}

#[allow(dead_code)]
fn unused(_: usize) -> usize {
    utf16_len("")
}
