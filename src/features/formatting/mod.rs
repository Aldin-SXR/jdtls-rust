//! Document, range and on-type formatting — Rust port of jdt.ls
//! `FormatterHandler`.
//!
//! Only the Eclipse code formatter itself runs in the bridge
//! (`BridgeRequest::Format`): this module resolves the options
//! (`options.rs`), computes the regions to format, post-processes the
//! formatter output and converts it into LSP edits.

pub mod defaults;
pub mod document;
pub mod options;
pub mod versioner;

use crate::analysis::dispatcher::Dispatcher;
use document::{is_java_whitespace, java_is_blank, java_trim, len16, substring16, Document};
pub use options::FormatSettings;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::{ExecuteCommandParams, FormattingOptions, Position, Range, TextEdit, Url};
use tracing::{error, warn};

// `org.eclipse.jdt.core.formatter.CodeFormatter` kinds and flags.
pub const K_UNKNOWN: i32 = 0x00;
pub const K_STATEMENTS: i32 = 0x02;
pub const K_COMPILATION_UNIT: i32 = 0x08;
pub const K_MODULE_INFO: i32 = 0x80;
pub const F_INCLUDE_COMMENTS: i32 = 0x1000;

const CLOSING_BRACE: u16 = b'}' as u16;
const NEW_LINE: u16 = b'\n' as u16;
const COMMA: u16 = b',' as u16;

const COMPILER_SOURCE: &str = "org.eclipse.jdt.core.compiler.source";
const COMPILER_COMPLIANCE: &str = "org.eclipse.jdt.core.compiler.compliance";

/// A formatter edit: replace `length` UTF-16 units at `offset` with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub offset: usize,
    pub length: usize,
    pub text: String,
}

/// `workspace/executeClientCommand` (jdt.ls `ExecuteCommandProposedClient`).
pub enum ExecuteClientCommand {}

impl tower_lsp::lsp_types::request::Request for ExecuteClientCommand {
    type Params = ExecuteCommandParams;
    type Result = Option<Value>;
    const METHOD: &'static str = "workspace/executeClientCommand";
}

/// What the handler needs from the server.
pub struct FormatEnv<'a> {
    pub dispatcher: &'a Dispatcher,
    pub client: &'a tower_lsp::Client,
    pub settings: FormatSettings,
    /// `Preferences.getRootPaths()` — used to resolve relative profile paths.
    pub roots: Vec<PathBuf>,
    /// `initializationOptions.extendedClientCapabilities`.
    pub extended_client_capabilities: Option<Value>,
}

impl FormatEnv<'_> {
    /// `cu.getOptions(true)` (or `JavaCore.getOptions()` without a CU): the
    /// workspace formatter options overlaid with the project's JDT options.
    pub async fn jdt_options(&self, uri: Option<&Url>) -> BTreeMap<String, String> {
        let mut options = options::workspace_formatter_options(&self.settings, &self.roots);
        let (project_options, source_level) = self.dispatcher.options_for(uri).await;
        options.extend(project_options);
        options.entry(COMPILER_SOURCE.to_owned()).or_insert_with(|| source_level.clone());
        options.entry(COMPILER_COMPLIANCE.to_owned()).or_insert(source_level);
        options
    }

    /// `FormatterHandler.getOptions(options, cu)`.
    async fn formatter_options(&self, uri: Option<&Url>, fo: &FormattingOptions) -> BTreeMap<String, String> {
        let mut options = self.jdt_options(uri).await;
        options::apply_formatting_options(&mut options, fo);
        options
    }

    async fn run(&self, source: &str, kind: i32, offset: usize, length: usize, line_separator: &str, options: BTreeMap<String, String>) -> Option<Vec<Edit>> {
        match self.dispatcher.format_source(source, kind, offset, length, line_separator, options).await {
            Ok(edits) => edits.map(|v| v.into_iter().map(|e| Edit { offset: e.offset, length: e.length, text: e.text }).collect()),
            Err(e) => {
                error!("formatter failed: {e}");
                None
            }
        }
    }

    /// The content callback when `uri` uses a scheme registered through the
    /// `nonStandardJavaFormatting` extended client capability.
    fn non_standard_callback(&self, uri: &Url) -> Option<String> {
        let nsjf = self.extended_client_capabilities.as_ref()?.get("nonStandardJavaFormatting")?;
        let schemes: Vec<&str> = nsjf.get("schemes")?.as_array()?.iter().filter_map(Value::as_str).collect();
        nsjf.get("extensions")?.as_array()?;
        let callback = nsjf.get("getContentCallback")?.as_str()?;
        (!schemes.is_empty() && !callback.trim().is_empty() && schemes.contains(&uri.scheme())).then(|| callback.to_owned())
    }
}

// ── textDocument/formatting & rangeFormatting ────────────────────────────────

/// `FormatterHandler.formatting` / `rangeFormatting`.
pub async fn format(env: &FormatEnv<'_>, uri: &Url, options: &FormattingOptions, range: Option<Range>) -> Vec<TextEdit> {
    if !env.settings.enabled {
        return Vec::new();
    }
    if let Some(callback) = env.non_standard_callback(uri) {
        let params = ExecuteCommandParams {
            command: callback,
            arguments: vec![Value::String(uri.to_string())],
            work_done_progress_params: Default::default(),
        };
        return match env.client.send_request::<ExecuteClientCommand>(params).await {
            Ok(Some(Value::String(text))) if !java_is_blank(&text) => format_java_code(env, &text, options, range).await,
            _ => Vec::new(),
        };
    }
    let Some(text) = env.dispatcher.store.get(uri).map(|s| s.content_string()) else {
        return Vec::new();
    };
    let document = Document::new(&text);
    let region = match range {
        None => Some((0, document.len())),
        Some(r) => get_region(&r, &document),
    };
    let Some(region) = region else { return Vec::new() };
    format_cu(env, uri, &text, &document, region, options, env.settings.comments_enabled).await
}

/// `FormatterHandler.format(cu, document, region, options, includeComments)`.
async fn format_cu(
    env: &FormatEnv<'_>,
    uri: &Url,
    text: &str,
    document: &Document,
    region: (usize, usize),
    fo: &FormattingOptions,
    include_comments: bool,
) -> Vec<TextEdit> {
    let options = env.formatter_options(Some(uri), fo).await;
    let line_delimiter = document.default_line_delimiter();
    let kind = formatting_kind(uri, include_comments);
    let comma_edit = compute_indentation_if_comma_present(document, region, fo, &options);
    let Some(mut edits) = env.run(text, kind, region.0, region.1, line_delimiter, options).await else {
        return Vec::new();
    };
    if let Some(comma) = comma_edit {
        if add_child(&mut edits, comma).is_err() {
            error!("Overlapping text edits while formatting {uri}");
            return Vec::new();
        }
    }
    edits.iter().map(|e| convert_edit(e, document)).collect()
}

fn formatting_kind(uri: &Url, include_comments: bool) -> i32 {
    let mut kind = if include_comments { F_INCLUDE_COMMENTS } else { 0 };
    let is_module_info = uri.path_segments().and_then(|mut s| s.next_back()).is_some_and(|n| n == "module-info.java");
    kind |= if is_module_info { K_MODULE_INFO } else { K_COMPILATION_UNIT };
    kind
}

/// `FormatterHandler.computeIndentationIfCommaPresent`.
fn compute_indentation_if_comma_present(
    document: &Document,
    region: (usize, usize),
    fo: &FormattingOptions,
    options: &BTreeMap<String, String>,
) -> Option<Edit> {
    let (offset, length) = region;
    if document.char_at(offset + length)? != NEW_LINE {
        return None;
    }
    let mut i = offset as i64 + length as i64 - 1;
    while i >= offset as i64 {
        let last = document.char_at(i as usize)?;
        if is_java_whitespace(last) {
            i -= 1;
            continue;
        }
        if last != COMMA {
            return None;
        }
        let num_tabs: usize = options
            .get(options::FORMATTER_CONTINUATION_INDENTATION)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let new_text = if fo.insert_spaces { " ".repeat(fo.tab_size as usize * num_tabs) } else { "\t".repeat(num_tabs) };
        let line = document.line_of_offset(offset)? + 1;
        return Some(Edit { offset: document.line_offset(line)?, length: 0, text: new_text });
    }
    None
}

/// `TextEdit.addChild` on the formatter's root `MultiTextEdit`: insertion
/// order of `TextEdit.INSERTION_COMPARATOR`, overlap is an error.
fn add_child(children: &mut Vec<Edit>, edit: Edit) -> Result<(), ()> {
    use std::cmp::Ordering;
    let compare = |a: &Edit, b: &Edit| -> Result<Ordering, ()> {
        if a.offset == b.offset && a.length == b.length {
            return Ok(Ordering::Equal);
        }
        if a.offset + a.length <= b.offset {
            return Ok(Ordering::Less);
        }
        if b.offset + b.length <= a.offset {
            return Ok(Ordering::Greater);
        }
        if a.offset == b.offset {
            if a.length == 0 && b.length > 0 {
                return Ok(Ordering::Less);
            }
            if a.length > 0 && b.length == 0 {
                return Ok(Ordering::Greater);
            }
        }
        Err(())
    };
    let mut index = 0;
    for (i, child) in children.iter().enumerate() {
        if compare(child, &edit)? != Ordering::Greater {
            index = i + 1;
        }
    }
    children.insert(index, edit);
    Ok(())
}

/// `FormatterHandler.getRegion(range, document)`.
fn get_region(range: &Range, document: &Document) -> Option<(usize, usize)> {
    let offset = document.line_offset(range.start.line as usize)? + range.start.character as usize;
    let end = document.line_offset(range.end.line as usize)? + range.end.character as usize;
    Some((offset, end.checked_sub(offset)?))
}

fn convert_edit(edit: &Edit, document: &Document) -> TextEdit {
    TextEdit {
        range: Range { start: document.position(edit.offset), end: document.position(edit.offset + edit.length) },
        new_text: edit.text.clone(),
    }
}

// ── textDocument/onTypeFormatting ────────────────────────────────────────────

/// `FormatterHandler.onTypeFormatting`.
pub async fn on_type_format(env: &FormatEnv<'_>, uri: &Url, options: &FormattingOptions, position: Position, trigger: &str) -> Vec<TextEdit> {
    if !env.settings.on_type_enabled {
        return Vec::new();
    }
    let Some(text) = env.dispatcher.store.get(uri).map(|s| s.content_string()) else {
        return Vec::new();
    };
    let document = Document::new(&text);
    let Some(region) = on_type_region(&text, &document, position, trigger) else {
        return Vec::new();
    };
    format_cu(env, uri, &text, &document, region, options, false).await
}

/// `FormatterHandler.getRegion(cu, document, position, trigger)`.
fn on_type_region(text: &str, document: &Document, position: Position, trigger: &str) -> Option<(usize, usize)> {
    let mut line = position.line as usize;
    let mut offset = document.line_offset(line)?;
    let mut length = position.character as usize;

    let mut trigger_char: u16;
    if let Some(first) = trigger.encode_utf16().next() {
        trigger_char = first;
        if trigger_char == NEW_LINE && (document.char_at(offset + length)? != trigger_char || length == 0) && line > 0 {
            let prev_line = line - 1;
            offset = document.line_offset(prev_line)?;
            length = document.line_length(prev_line)?;
            line = prev_line;
        }
    } else {
        trigger_char = document.char_at(offset + length)?;
    }

    let mut empty_line = false;
    if trigger_char == NEW_LINE {
        // Previous non-whitespace char (up to the previous line).
        let lines: Vec<usize> = if line > 0 { vec![line, line - 1] } else { vec![line] };
        'lines: for l in lines {
            let line_offset = document.line_offset(l)?;
            let max_position = document.line_length(l)? as i64 - 1;
            empty_line = false;
            let mut pos = max_position;
            while pos >= 0 {
                let ch = document.char_at(line_offset + pos as usize)?;
                if is_java_whitespace(ch) {
                    pos -= 1;
                    continue;
                }
                length = if ch == CLOSING_BRACE { (pos + 1) as usize } else { max_position.max(0) as usize };
                offset = line_offset;
                trigger_char = ch;
                break 'lines;
            }
            empty_line = true;
        }
    }

    if trigger_char == CLOSING_BRACE {
        // Format the whole block, from the beginning of its first line to the
        // end of its last line.
        let (block_start, block_length) = find_node(text, document, offset, length)?;
        let line_of_block = document.line_of_offset(block_start)?;
        let line_offset = document.line_offset(line_of_block)? as i64;
        let end_line = document.line_of_offset(block_start + block_length)?;
        let end_line_offset = document.line_offset(end_line)? as i64;
        let mut end_line_length = document.line_length(end_line)? as i64;
        let last_char = end_line_offset + end_line_length - 1;
        if last_char < 0 {
            return None;
        }
        if document.char_at(last_char as usize)? == NEW_LINE {
            end_line_length -= 1;
        }
        let last_position = end_line_offset + end_line_length;
        let total_length = last_position - line_offset;
        if total_length < 0 {
            return None;
        }
        return Some((line_offset as usize, total_length as usize));
    } else if !empty_line {
        // Format the current non-empty line.
        return Some((offset, length));
    }
    None
}

/// `NodeFinder(astRoot, offset, length)`: the covered node, else the covering
/// node, over a tree-sitter parse (named nodes, comments excluded, Javadoc
/// attached to the following declaration as in the JDT DOM).
fn find_node(text: &str, document: &Document, start: usize, length: usize) -> Option<(usize, usize)> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_java::language()).ok()?;
    let tree = parser.parse(text, None)?;
    // byte offset → UTF-16 offset
    let mut b2u = vec![0usize; text.len() + 1];
    let mut u = 0;
    for (b, ch) in text.char_indices() {
        for k in 0..ch.len_utf8() {
            b2u[b + k] = u;
        }
        u += ch.len_utf16();
    }
    b2u[text.len()] = u;

    struct Finder<'t> {
        text: &'t str,
        b2u: Vec<usize>,
        start: usize,
        end: usize,
        covering: Option<(usize, usize, usize)>,
        covered: Option<(usize, usize)>,
    }
    impl Finder<'_> {
        fn range(&self, node: tree_sitter::Node) -> (usize, usize) {
            let mut s = node.start_byte();
            if let Some(prev) = node.prev_named_sibling() {
                let between = &self.text[prev.end_byte()..s];
                if prev.kind() == "block_comment" && self.text[prev.start_byte()..].starts_with("/**") && between.trim().is_empty() && is_declaration(node.kind()) {
                    s = prev.start_byte();
                }
            }
            (self.b2u[s], self.b2u[node.end_byte()])
        }
        fn visit(&mut self, node: tree_sitter::Node, range: (usize, usize)) {
            let (ns, ne) = range;
            if ne < self.start || self.end < ns {
                return;
            }
            if ns <= self.start && self.end <= ne {
                self.covering = Some((ns, ne, node.id()));
            }
            if self.start <= ns && ne <= self.end {
                if self.covering.is_some_and(|c| c.2 == node.id()) {
                    self.covered = Some((ns, ne));
                } else {
                    if self.covered.is_none() {
                        self.covered = Some((ns, ne));
                    }
                    return;
                }
            }
            let mut cursor = node.walk();
            let children: Vec<tree_sitter::Node> = node
                .named_children(&mut cursor)
                .filter(|c| c.kind() != "line_comment" && c.kind() != "block_comment")
                .collect();
            for child in children {
                let r = self.range(child);
                self.visit(child, r);
            }
        }
    }
    let root = tree.root_node();
    let mut finder = Finder { text, b2u, start, end: start + length, covering: None, covered: None };
    finder.visit(root, (0, document.len()));
    let (s, e) = finder.covered.or(finder.covering.map(|(s, e, _)| (s, e)))?;
    Some((s, e - s))
}

fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
            | "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "field_declaration"
            | "constant_declaration"
            | "enum_constant"
            | "annotation_type_element_declaration"
            | "package_declaration"
            | "module_declaration"
    )
}

// ── Non-standard documents (notebooks): `formatJavaCode` ─────────────────────

/// `FormatterHandler.formatJavaCode`: format a snippet (statements, imports,
/// members or a whole class) and return a single edit replacing the text.
pub async fn format_java_code(env: &FormatEnv<'_>, text: &str, fo: &FormattingOptions, range: Option<Range>) -> Vec<TextEdit> {
    let mut start_text = String::new();
    let mut end_text = String::new();
    let original_text = text.to_owned();
    let mut text = text.to_owned();
    if let Some(range) = range {
        let document = Document::new(&text);
        let Some((offset, length)) = get_region(&range, &document) else { return Vec::new() };
        if offset > 0 || length < document.len() {
            if offset > 0 {
                let Some(s) = substring16(&text, 0, offset) else { return Vec::new() };
                start_text = s;
            }
            let Some(e) = substring16(&text, offset + length + 1, document.len()) else {
                warn!("formatJavaCode: range end out of bounds");
                return Vec::new();
            };
            end_text = e;
            let Some(t) = document.get(offset, length) else { return Vec::new() };
            text = t;
        }
    }
    let options = env.formatter_options(None, fo).await;
    let mut formatted = format_snippet(env, &text, "", "", K_STATEMENTS, &options).await;
    if formatted.is_none() {
        // check imports
        let import_pattern = regex::Regex::new(r"(?m)^(import\s+.*?);").unwrap();
        let mut builder = String::new();
        let mut import_end = 0;
        let line_delimiter = document::determine_line_delimiter(&text, "\n");
        for m in import_pattern.find_iter(&text) {
            builder.push_str(m.as_str());
            builder.push_str(line_delimiter);
            import_end = m.end();
        }
        let imports = builder;
        let mut builder = String::new();
        if !java_is_blank(&imports) {
            match format_snippet(env, &imports, "", "", K_UNKNOWN, &options).await {
                Some(res) => builder.push_str(&res),
                None => builder.push_str(&imports),
            }
            builder.push_str(line_delimiter);
        }
        let remaining = &text[import_end..];
        if !java_is_blank(java_trim(remaining)) {
            match format_string(env, java_trim(remaining), &options).await {
                Some(res) => builder.push_str(&res),
                None => builder.push_str(remaining),
            }
        }
        formatted = Some(builder);
    }
    let Some(formatted) = formatted else { return Vec::new() };
    let formatted = format!("{start_text}{formatted}{end_text}");
    let document = Document::new(&original_text);
    vec![TextEdit { range: Range { start: document.position(0), end: document.position(document.len()) }, new_text: formatted }]
}

/// `FormatterHandler.formatString`: statements, then wrapped in a method, an
/// initializer, and a class body.
async fn format_string(env: &FormatEnv<'_>, text: &str, options: &BTreeMap<String, String>) -> Option<String> {
    if let Some(f) = format_snippet(env, text, "", "", K_STATEMENTS, options).await {
        return Some(f);
    }
    let wrappers = [
        ("// PREFIX\npublic class Temporary { public void run() {\n", "// SUFFIX\n} }"),
        ("// PREFIX\npublic class Temporary { {\n", "// SUFFIX\n} }"),
        ("// PREFIX\npublic class Temporary {\n", "// SUFFIX\n}"),
    ];
    for (prefix, suffix) in wrappers {
        if let Some(f) = format_snippet(env, text, prefix, suffix, K_COMPILATION_UNIT, options).await {
            return Some(f);
        }
    }
    None
}

/// `FormatterHandler.format(text, prefix, suffix, type, options, monitor)`.
async fn format_snippet(env: &FormatEnv<'_>, text: &str, prefix: &str, suffix: &str, kind: i32, options: &BTreeMap<String, String>) -> Option<String> {
    let content = format!("{prefix}{text}{suffix}");
    let document = Document::new(&content);
    let mut kind = kind;
    if env.settings.comments_enabled {
        kind |= F_INCLUDE_COMMENTS;
    }
    let edits = env
        .run(&content, kind, len16(prefix), len16(text), document.default_line_delimiter(), options.clone())
        .await?;
    if edits.is_empty() {
        return None;
    }
    let Some(text) = document.apply(&edits) else {
        error!("formatter produced malformed edits");
        return None;
    };
    let text = text.replace(suffix, "");
    let text = if prefix.is_empty() { text } else { text.replace(prefix, "") };
    let indent = text.chars().take_while(|&c| c == ' ' || c == '\t').count(); // ASCII: chars == UTF-16 units
    let mut builder = String::new();
    for line in java_split_lines(&text) {
        if !line.is_empty() && len16(line) >= indent && (line.starts_with(' ') || line.starts_with('\t')) {
            builder.push_str(&substring16(line, indent, len16(line)).unwrap_or_default());
        } else {
            builder.push_str(line);
        }
        builder.push('\n');
    }
    Some(builder)
}

/// Java `text.split("\n")`: trailing empty strings are removed.
fn java_split_lines(text: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = text.split('\n').collect();
    if parts.len() > 1 {
        while parts.last().is_some_and(|p| p.is_empty()) {
            parts.pop();
        }
    }
    parts
}

// ── java.project.getSettings (JDT option keys) ───────────────────────────────

/// `ProjectCommand.getProjectSettings` for JDT option keys:
/// `javaProject.getOption(key, true)` of the project owning `uri`.  Keys
/// without a known value are left out.
pub async fn project_option_settings(env: &FormatEnv<'_>, uri: &Url, keys: &[String]) -> serde_json::Map<String, Value> {
    let options = env.jdt_options(Some(uri)).await;
    keys.iter()
        .filter_map(|k| options.get(k).map(|v| (k.clone(), Value::String(v.clone()))))
        .collect()
}

// ── java.edit.stringFormatting ───────────────────────────────────────────────

/// `FormatterHandler.stringFormatting(content, options, version)`.
pub async fn string_formatting(env: &FormatEnv<'_>, content: &str, options: Option<BTreeMap<String, String>>, version: i32) -> String {
    let document = Document::new(content);
    let format_options = match options {
        None => options::combined_default_formatter_settings(),
        Some(o) => versioner::update_and_complete(&o, version),
    };
    let mut kind = K_COMPILATION_UNIT;
    if env.settings.comments_enabled {
        kind |= F_INCLUDE_COMMENTS;
    }
    match env.run(content, kind, 0, document.len(), document.default_line_delimiter(), format_options).await {
        Some(edits) => document.apply(&edits).unwrap_or_else(|| content.to_owned()),
        None => content.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_like_java() {
        assert_eq!(java_split_lines("a\nb\n\n"), vec!["a", "b"]);
        assert_eq!(java_split_lines(""), vec![""]);
        assert_eq!(java_split_lines("\na"), vec!["", "a"]);
    }

    #[test]
    fn add_child_order() {
        let mut v = vec![Edit { offset: 0, length: 2, text: "x".into() }, Edit { offset: 5, length: 1, text: "y".into() }];
        add_child(&mut v, Edit { offset: 5, length: 0, text: "i".into() }).unwrap();
        assert_eq!(v[1].text, "i");
        assert!(add_child(&mut v, Edit { offset: 1, length: 0, text: "z".into() }).is_err());
    }

    #[test]
    fn on_type_regions() {
        let text = "package org.sample;\n\n    public      class     Baz {}  \n";
        let d = Document::new(text);
        let r = on_type_region(text, &d, Position { line: 2, character: 34 }, "\n").unwrap();
        assert_eq!(r, (21, 34));
        let text = "package org.sample;\n\n    public      class     Baz {  \nString          name       ;\n}  ";
        let d = Document::new(text);
        let r = on_type_region(text, &d, Position { line: 4, character: 0 }, "}").unwrap();
        assert_eq!(r, (21, text.len() - 21));
    }
}
