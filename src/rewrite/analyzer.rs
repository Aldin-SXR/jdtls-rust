//! Port of `org.eclipse.jdt.internal.core.dom.rewrite.ASTRewriteAnalyzer`.
//!
//! Walks the original AST; for every node with changed properties it emits
//! the text edits JDT emits (same offsets, separators, indentation and
//! formatting of inserted code).

use std::collections::{BTreeMap, HashMap};

use super::flattener::{MarkerData, NodeMarker};
use super::formatter::{self as fmt, BlockContext, CodeFormatter, Prefix, RewriteFormatter};
use super::indent;
use super::scanner::{Tok, TokenScanner};
use super::text_edit::{EditKind, EditTree, SourceModifier};
use super::{change, ASTRewrite, Event, ListEntry, RNode, RewriteError, Value};
use crate::semantic_ast::{NodeId, NodeKind};

type R<T> = Result<T, RewriteError>;

/// `TextUtilities.getDefaultLineDelimiter(document)`.
pub fn default_line_delimiter(content: &[u16]) -> String {
    for (i, &c) in content.iter().enumerate() {
        if c == b'\r' as u16 {
            return if content.get(i + 1) == Some(&(b'\n' as u16)) { "\r\n".into() } else { "\r".into() };
        }
        if c == b'\n' as u16 {
            return "\n".into();
        }
    }
    "\n".into()
}

pub fn rewrite(rw: &ASTRewrite, options: &BTreeMap<String, String>, formatter: &mut dyn CodeFormatter) -> R<EditTree> {
    let content: &[u16] = &rw.ast.source;
    let line_delim = default_line_delimiter(content);
    let mut line_starts = vec![0usize];
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'\r' as u16 {
            if content.get(i + 1) == Some(&(b'\n' as u16)) {
                i += 1;
            }
            line_starts.push(i + 1);
        } else if content[i] == b'\n' as u16 {
            line_starts.push(i + 1);
        }
        i += 1;
    }
    let mut line_comment_ends: Vec<i32> = rw
        .ast
        .comments
        .iter()
        .map(|&c| rw.ast.node(c))
        .filter(|c| c.kind() == NodeKind::LineComment)
        .map(|c| c.end() as i32)
        .collect();
    line_comment_ends.sort();
    let mut a = Analyzer {
        rw,
        content,
        line_starts,
        edits: EditTree::new(),
        current_edit: EditTree::ROOT,
        source_copy_info_to_edit: HashMap::new(),
        source_copy_end_nodes: Vec::new(),
        formatter: RewriteFormatter::new(options, &line_delim, formatter),
        scanner: TokenScanner::new(content),
        line_comment_ends,
        before_required_space_index: -1,
        options: options.clone(),
    };
    let root = rw.ast.root().id;
    a.accept(root)?;
    Ok(a.edits)
}

struct Analyzer<'a, 'f> {
    rw: &'a ASTRewrite,
    content: &'a [u16],
    line_starts: Vec<usize>,
    edits: EditTree,
    current_edit: usize,
    source_copy_info_to_edit: HashMap<usize, usize>,
    source_copy_end_nodes: Vec<NodeId>,
    formatter: RewriteFormatter<'f>,
    scanner: TokenScanner<'a>,
    line_comment_ends: Vec<i32>,
    before_required_space_index: i32,
    options: BTreeMap<String, String>,
}

fn o(n: NodeId) -> RNode {
    RNode::Orig(n)
}

const LBRACE: Tok = Tok::Op("{");
const LPAREN: Tok = Tok::Op("(");
const RPAREN: Tok = Tok::Op(")");
const LBRACKET: Tok = Tok::Op("[");
const RBRACKET: Tok = Tok::Op("]");
const LESS: Tok = Tok::Op("<");
const GREATER: Tok = Tok::Op(">");
const DOT: Tok = Tok::Op(".");
const COMMA: Tok = Tok::Op(",");
const SEMICOLON: Tok = Tok::Op(";");
const COLON: Tok = Tok::Op(":");
const COLON_COLON: Tok = Tok::Op("::");
const EQUAL: Tok = Tok::Op("=");
const ARROW: Tok = Tok::Op("->");

/// List rewriter flavours (`ListRewriter` and its subclasses).
#[derive(Clone, Copy)]
enum ListKind {
    Plain,
    Resources,
    Paragraph { initial_indent: i32, separator_lines: i32 },
    Modifier { annotation_separation: Prefix },
    Switch { initial_indent: i32, indent_compare: bool, labeled_rule: bool },
}

struct ListRewriter {
    kind: ListKind,
    constant_separator: String,
    start_pos: i32,
    node_indent_pos: i32,
    list: Vec<ListEntry>,
}

impl ListRewriter {
    fn new(kind: ListKind) -> Self {
        ListRewriter { kind, constant_separator: String::new(), start_pos: 0, node_indent_pos: 0, list: Vec::new() }
    }
    fn original(&self, i: usize) -> Option<RNode> {
        self.list[i].original
    }
    fn new_node(&self, i: usize) -> Option<RNode> {
        self.list[i].new
    }
    /// `ParagraphListRewriter.getNode`.
    fn node(&self, i: usize) -> Option<RNode> {
        self.list[i].original.or(self.list[i].new)
    }
}

fn orig_id(n: RNode) -> NodeId {
    match n {
        RNode::Orig(id) => id,
        RNode::New(_) => NodeId(u32::MAX),
    }
}

impl<'a, 'f> Analyzer<'a, 'f> {
    // ── Infrastructure ──────────────────────────────────────────────────────

    fn start(&self, n: NodeId) -> i32 {
        self.rw.ast.node(n).start() as i32
    }

    fn length(&self, n: NodeId) -> i32 {
        self.rw.ast.node(n).length() as i32
    }

    fn end(&self, n: NodeId) -> i32 {
        self.start(n) + self.length(n)
    }

    fn kind(&self, n: RNode) -> NodeKind {
        self.rw.kind(n)
    }

    fn extended_range(&self, n: NodeId) -> (i32, i32) {
        let (s, l) = self.rw.extended_range(n);
        (s as i32, l as i32)
    }

    fn extended_offset(&self, n: NodeId) -> i32 {
        self.extended_range(n).0
    }

    fn extended_end(&self, n: NodeId) -> i32 {
        let (s, l) = self.extended_range(n);
        s + l
    }

    fn event(&self, parent: NodeId, prop: &str) -> Option<&'a Event> {
        self.rw.event(o(parent), prop)
    }

    fn change_kind(&self, parent: NodeId, prop: &str) -> i32 {
        self.rw.change_kind(o(parent), prop)
    }

    fn is_changed(&self, parent: NodeId, prop: &str) -> bool {
        self.rw.is_changed(o(parent), prop)
    }

    fn has_children_changes(&self, n: NodeId) -> bool {
        self.rw.has_changed_properties(o(n))
    }

    fn original_value(&self, parent: NodeId, prop: &str) -> Value {
        self.rw.original_value(o(parent), prop)
    }

    fn new_value(&self, parent: NodeId, prop: &str) -> Value {
        self.rw.new_value(o(parent), prop)
    }

    fn line_delimiter(&self) -> String {
        self.formatter.line_delimiter.clone()
    }

    fn create_indent_string(&self, indent: i32) -> String {
        self.formatter.create_indent_string(indent)
    }

    fn create_indent_string_min(&self, indent: i32, min: i32) -> String {
        self.formatter.create_indent_string_min(indent, min)
    }

    /// `LineInformation.getLineOfOffset` (-1 outside the document).
    fn line_of_offset(&self, offset: i32) -> i32 {
        if offset < 0 || offset as usize > self.content.len() {
            return -1;
        }
        match self.line_starts.binary_search(&(offset as usize)) {
            Ok(i) => i as i32,
            Err(i) => i as i32 - 1,
        }
    }

    fn line_offset(&self, line: i32) -> i32 {
        if line < 0 {
            return -1;
        }
        self.line_starts.get(line as usize).map_or(-1, |&s| s as i32)
    }

    fn indent_of_line(&self, pos: i32) -> String {
        let line = self.line_of_offset(pos);
        if line < 0 {
            return String::new();
        }
        let start = self.line_offset(line) as usize;
        let mut i = start;
        while i < self.content.len() && indent::is_indent_char(self.content[i]) {
            i += 1;
        }
        indent::from_u16(&self.content[start..i])
    }

    fn indent_string(&self, line: &str, min: i32) -> String {
        let mut indent = self.formatter.get_indent_string(line);
        let spaces = self.formatter.compute_indent_in_spaces(&indent);
        if spaces < min {
            indent.push_str(&" ".repeat((min - spaces) as usize));
        }
        indent
    }

    fn indent_at_offset(&self, pos: i32) -> String {
        self.formatter.get_indent_string(&self.indent_of_line(pos))
    }

    fn get_indent(&self, offset: i32) -> i32 {
        self.formatter.compute_indent_units(&self.indent_of_line(offset))
    }

    fn get_indent_in_spaces(&self, offset: i32) -> i32 {
        self.formatter.compute_indent_in_spaces(&self.indent_of_line(offset))
    }

    fn is_end_of_line_comment(&self, offset: i32) -> bool {
        offset >= 0 && self.line_comment_ends.binary_search(&offset).is_ok()
    }

    fn is_end_of_line_comment_in_content(&self, offset: i32) -> bool {
        if offset >= 0 && (offset as usize >= self.content.len() || indent::is_line_delimiter_char(self.content[offset as usize])) {
            self.line_comment_ends.binary_search(&offset).is_ok()
        } else {
            false
        }
    }

    fn remove_line_comment_end(&mut self, offset: i32) {
        if let Ok(i) = self.line_comment_ends.binary_search(&offset) {
            self.line_comment_ends.remove(i);
            self.line_comment_ends.insert(0, -1);
        }
    }

    fn add_edit(&mut self, edit: usize) -> R<()> {
        self.edits.add_child(self.current_edit, edit)?;
        Ok(())
    }

    fn do_text_insert(&mut self, offset: i32, s: &str) -> R<()> {
        if s.is_empty() {
            return Ok(());
        }
        if self.is_end_of_line_comment_in_content(offset) {
            let delim = self.line_delimiter();
            if !s.starts_with(&delim) {
                let e = self.edits.new_edit(offset, 0, EditKind::Insert(delim));
                self.add_edit(e)?;
            }
            self.remove_line_comment_end(offset);
        }
        let e = self.edits.new_edit(offset, 0, EditKind::Insert(s.to_owned()));
        self.add_edit(e)
    }

    fn do_text_remove(&mut self, offset: i32, len: i32) -> R<Option<usize>> {
        if len == 0 {
            return Ok(None);
        }
        let e = self.edits.new_edit(offset, len, EditKind::Delete);
        self.add_edit(e)?;
        Ok(Some(e))
    }

    fn do_text_remove_and_visit(&mut self, offset: i32, len: i32, node: NodeId) -> R<()> {
        match self.do_text_remove(offset, len)? {
            Some(e) => {
                self.current_edit = e;
                self.accept(node)?;
                self.current_edit = self.edits.edits[e].parent.unwrap_or(EditTree::ROOT);
            }
            None => self.accept(node)?,
        }
        Ok(())
    }

    fn do_visit(&mut self, node: NodeId) -> R<i32> {
        self.accept(node)?;
        Ok(self.extended_end(node))
    }

    fn do_visit_prop(&mut self, parent: NodeId, prop: &str, offset: i32) -> R<i32> {
        match self.original_value(parent, prop) {
            Value::Node(Some(RNode::Orig(n))) => self.do_visit(n),
            Value::List(l) => {
                let mut end = offset;
                for c in l {
                    if let RNode::Orig(c) = c {
                        end = self.do_visit(c)?;
                    }
                }
                Ok(end)
            }
            _ => Ok(offset),
        }
    }

    fn void_visit_prop(&mut self, parent: NodeId, prop: &str) -> R<()> {
        match self.original_value(parent, prop) {
            Value::Node(Some(RNode::Orig(n))) => self.accept(n),
            Value::List(l) => {
                for c in l {
                    if let RNode::Orig(c) = c {
                        self.do_visit(c)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn do_visit_unchanged_children(&mut self, parent: NodeId) -> R<()> {
        for prop in self.rw.property_names(o(parent)) {
            self.void_visit_prop(parent, prop)?;
        }
        Ok(())
    }

    fn do_text_replace(&mut self, offset: i32, len: i32, s: &str) -> R<()> {
        if len > 0 || !s.is_empty() {
            let e = self.edits.new_edit(offset, len, EditKind::Replace(s.to_owned()));
            self.add_edit(e)?;
        }
        Ok(())
    }

    fn copy_source_edit(&mut self, info: usize) -> usize {
        if let Some(&e) = self.source_copy_info_to_edit.get(&info) {
            return e;
        }
        let node = self.rw.copy_sources[info].node;
        let (start, len) = if let Some((first, last)) = self.rw.copy_sources[info].range {
            let start = self.extended_offset(first);
            (start, self.extended_end(last) - start)
        } else { self.extended_range(node) };
        let kind = if self.rw.copy_sources[info].is_move { EditKind::MoveSource } else { EditKind::CopySource };
        let e = self.edits.new_edit(start, len, kind);
        self.source_copy_info_to_edit.insert(info, e);
        e
    }

    fn do_text_copy(&mut self, source_edit: usize, dest_offset: i32, source_indent_level: i32, dest_indent: &str) -> R<()> {
        self.edits.edits[source_edit].modifier = Some(SourceModifier {
            source_indent_level,
            destination_indent: dest_indent.to_owned(),
            tab_width: self.formatter.tab_width,
            indent_width: self.formatter.indent_width,
        });
        let kind = if matches!(self.edits.edits[source_edit].kind, EditKind::MoveSource) {
            EditKind::MoveTarget(source_edit)
        } else {
            EditKind::CopyTarget(source_edit)
        };
        let e = self.edits.new_edit(dest_offset, 0, kind);
        self.add_edit(e)
    }

    // ── Node rewriting ──────────────────────────────────────────────────────

    fn rewrite_required_node(&mut self, parent: NodeId, prop: &str) -> R<i32> {
        if let Some(ev) = self.event(parent, prop) {
            if ev.change_kind() == change::REPLACED {
                let node = orig_id(ev.original_value().node().unwrap());
                let (offset, length) = self.extended_range(node);
                self.do_text_remove_and_visit(offset, length, node)?;
                let new = ev.new_value().node().unwrap();
                let indent = self.get_indent(offset);
                self.do_text_insert_node(offset, new, indent, true, 0)?;
                return Ok(offset + length);
            }
        }
        self.do_visit_prop(parent, prop, 0)
    }

    fn rewrite_node(&mut self, parent: NodeId, prop: &str, offset: i32, prefix: Prefix) -> R<i32> {
        if let Some(ev) = self.event(parent, prop) {
            match ev.change_kind() {
                change::INSERTED => {
                    let node = ev.new_value().node().unwrap();
                    let indent = self.get_indent(offset);
                    let p = self.formatter.prefix(prefix, indent);
                    self.do_text_insert(offset, &p)?;
                    self.do_text_insert_node(offset, node, indent, true, 0)?;
                    return Ok(offset);
                }
                change::REMOVED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    let (start, len, node_end) = if offset == 0 {
                        let (s, l) = self.extended_range(node);
                        (s, l, s + l)
                    } else {
                        let e = self.extended_end(node);
                        (offset, e - offset, e)
                    };
                    self.do_text_remove_and_visit(start, len, node)?;
                    return Ok(node_end);
                }
                change::REPLACED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    let (node_offset, node_len) = self.extended_range(node);
                    self.do_text_remove_and_visit(node_offset, node_len, node)?;
                    let new = ev.new_value().node().unwrap();
                    let indent = self.get_indent(offset);
                    self.do_text_insert_node(node_offset, new, indent, true, 0)?;
                    return Ok(node_offset + node_len);
                }
                _ => {}
            }
        }
        self.do_visit_prop(parent, prop, offset)
    }

    fn rewrite_javadoc(&mut self, node: NodeId, prop: &str) -> R<i32> {
        let mut pos = self.rewrite_node(node, prop, self.start(node), fmt::NONE)?;
        let kind = self.change_kind(node, prop);
        if kind == change::INSERTED {
            let s = self.line_delimiter() + &self.indent_at_offset(pos);
            self.do_text_insert(pos, &s)?;
        } else if kind == change::REMOVED {
            self.scanner.read_next_at(pos, false)?;
            let next = self.scanner.current_start_offset();
            self.do_text_remove(pos, next - pos)?;
            pos = next;
        }
        Ok(pos)
    }

    fn rewrite_body_node(&mut self, parent: NodeId, prop: &str, offset: i32, mut end_pos: i32, mut indent: i32, context: BlockContext) -> R<i32> {
        if let Some(ev) = self.event(parent, prop) {
            match ev.change_kind() {
                change::INSERTED => {
                    let node = ev.new_value().node().unwrap();
                    let (p, s) = self.formatter.prefix_and_suffix(context, indent, self.rw, node);
                    self.do_text_insert(offset, &p)?;
                    self.do_text_insert_node(offset, node, indent, true, 0)?;
                    self.do_text_insert(offset, &s)?;
                    return Ok(offset);
                }
                change::REMOVED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    if end_pos == -1 {
                        end_pos = self.extended_end(node);
                    }
                    self.do_text_remove_and_visit(offset, end_pos - offset, node)?;
                    return Ok(end_pos);
                }
                change::REPLACED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    let mut insert_new_line = false;
                    if end_pos == -1 {
                        let previous_end = self.end(node);
                        end_pos = self.extended_end(node);
                        if end_pos != previous_end {
                            let tok = self.scanner.read_next_at(previous_end, false).unwrap_or(Tok::Eof);
                            if tok == Tok::CommentLine && self.kind(o(node)) == NodeKind::Block {
                                insert_new_line = true;
                            }
                        }
                    }
                    let replacing = ev.new_value().node().unwrap();
                    let (prefix, suffix) = self.formatter.prefix_and_suffix(context, indent, self.rw, replacing);
                    self.do_text_remove_and_visit(offset, end_pos - offset, node)?;
                    let mut inserted_prefix = prefix.clone();
                    if insert_new_line {
                        inserted_prefix = self.line_delimiter() + &self.formatter.create_indent_string(indent) + prefix.trim() + " ";
                    }
                    self.do_text_insert(offset, &inserted_prefix)?;
                    let line_start = current_line_start(&prefix, indent::len16(&prefix));
                    if line_start != 0 {
                        indent = self.formatter.compute_indent_units(&indent::sub16(&prefix, line_start, indent::len16(&prefix)));
                    }
                    let minimum = self.formatter.compute_indent_in_spaces(&self.indent_of_line(self.start(parent)));
                    self.do_text_insert_node(offset, replacing, indent, true, minimum)?;
                    self.do_text_insert(offset, &suffix)?;
                    return Ok(end_pos);
                }
                _ => {}
            }
        }
        let pos = self.do_visit_prop(parent, prop, offset)?;
        Ok(if end_pos != -1 { end_pos } else { pos })
    }

    fn rewrite_optional_qualifier(&mut self, parent: NodeId, prop: &str, start_pos: i32) -> R<i32> {
        if let Some(ev) = self.event(parent, prop) {
            match ev.change_kind() {
                change::INSERTED => {
                    let node = ev.new_value().node().unwrap();
                    let indent = self.get_indent(start_pos);
                    self.do_text_insert_node(start_pos, node, indent, true, 0)?;
                    self.do_text_insert(start_pos, ".")?;
                    return Ok(start_pos);
                }
                change::REMOVED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    let dot_end = self.scanner.token_end_offset(DOT, self.end(node))?;
                    self.do_text_remove_and_visit(start_pos, dot_end - start_pos, node)?;
                    return Ok(dot_end);
                }
                change::REPLACED => {
                    let node = orig_id(ev.original_value().node().unwrap());
                    let (offset, length) = self.extended_range(node);
                    self.do_text_remove_and_visit(offset, length, node)?;
                    let new = ev.new_value().node().unwrap();
                    let indent = self.get_indent(start_pos);
                    self.do_text_insert_node(offset, new, indent, true, 0)?;
                    return Ok(self.scanner.token_end_offset(DOT, offset + length)?);
                }
                _ => {}
            }
        }
        match self.original_value(parent, prop).node() {
            None => Ok(start_pos),
            Some(n) => {
                let pos = self.do_visit(orig_id(n))?;
                Ok(self.scanner.token_end_offset(DOT, pos)?)
            }
        }
    }

    fn rewrite_paragraph_list(&mut self, parent: NodeId, prop: &str, insert_pos: i32, insert_indent: i32, separator: i32, lead: i32) -> R<i32> {
        if let Some(ev) = self.event(parent, prop) {
            if ev.change_kind() != change::UNCHANGED {
                let events = ev.children();
                let mut lead_string = String::new();
                if is_all_of_kind(&events, change::INSERTED) {
                    for _ in 0..lead {
                        lead_string.push_str(&self.line_delimiter());
                    }
                    lead_string.push_str(&self.create_indent_string(insert_indent));
                }
                let mut lr = ListRewriter::new(ListKind::Paragraph { initial_indent: insert_indent, separator_lines: separator });
                return self.rewrite_list(&mut lr, parent, prop, &lead_string, None, insert_pos);
            }
        }
        self.do_visit_prop(parent, prop, insert_pos)
    }

    fn rewrite_optional_type_parameters(&mut self, parent: NodeId, prop: &str, offset: i32, keyword: &str, adjust_on_next: bool, mut needs_space_on_remove_all: bool) -> R<i32> {
        let mut pos = offset;
        let ev = self.event(parent, prop);
        if let Some(ev) = ev.filter(|e| e.change_kind() != change::UNCHANGED) {
            let children = ev.children();
            let is_all_inserted = is_all_of_kind(&children, change::INSERTED);
            if is_all_inserted && adjust_on_next {
                pos = self.scanner.next_start_offset(pos, false)?;
            }
            let is_all_removed = !is_all_inserted && is_all_of_kind(&children, change::REMOVED);
            if is_all_removed {
                let before = self.scanner.token_start_offset(LESS, pos)?;
                if before != pos {
                    needs_space_on_remove_all = false;
                }
                pos = before;
            }
            let mut lr = ListRewriter::new(ListKind::Plain);
            lr.constant_separator = ", ".into();
            pos = self.rewrite_list(&mut lr, parent, prop, "<", None, pos)?;
            if is_all_removed {
                let mut end_pos = self.scanner.token_end_offset(GREATER, pos)?;
                end_pos = self.scanner.next_start_offset(end_pos, false)?;
                let replacement = if needs_space_on_remove_all { " " } else { "" };
                self.do_text_replace(pos, end_pos - pos, replacement)?;
                return Ok(end_pos);
            }
            if is_all_inserted {
                self.do_text_insert(pos, &format!(">{keyword}"))?;
                return Ok(pos);
            }
        } else {
            pos = self.do_visit_prop(parent, prop, offset)?;
        }
        if pos != offset {
            return Ok(self.scanner.token_end_offset(GREATER, pos)?);
        }
        Ok(pos)
    }

    fn rewrite_node_list_end(&mut self, parent: NodeId, prop: &str, pos: i32, keyword: &str, end_keyword: &str, separator: &str) -> R<i32> {
        if self.change_kind(parent, prop) != change::UNCHANGED {
            let mut lr = ListRewriter::new(ListKind::Plain);
            lr.constant_separator = separator.to_owned();
            return self.rewrite_list(&mut lr, parent, prop, keyword, Some(end_keyword), pos);
        }
        self.do_visit_prop(parent, prop, pos)
    }

    fn rewrite_resources_node_list(&mut self, parent: NodeId, prop: &str, pos: i32, keyword: &str, end_keyword: &str, separator: &str) -> R<i32> {
        if self.change_kind(parent, prop) != change::UNCHANGED {
            let mut lr = ListRewriter::new(ListKind::Resources);
            lr.constant_separator = separator.to_owned();
            return self.rewrite_list(&mut lr, parent, prop, keyword, Some(end_keyword), pos);
        }
        self.do_visit_prop(parent, prop, pos)
    }

    fn rewrite_node_list(&mut self, parent: NodeId, prop: &str, pos: i32, keyword: &str, separator: &str) -> R<i32> {
        if self.change_kind(parent, prop) != change::UNCHANGED {
            let mut lr = ListRewriter::new(ListKind::Plain);
            lr.constant_separator = separator.to_owned();
            return self.rewrite_list(&mut lr, parent, prop, keyword, None, pos);
        }
        self.do_visit_prop(parent, prop, pos)
    }

    fn rewrite_method_body(&mut self, parent: NodeId, start_pos: i32) -> R<()> {
        if let Some(ev) = self.event(parent, "body") {
            match ev.change_kind() {
                change::INSERTED => {
                    let end_pos = self.end(parent);
                    let body = ev.new_value().node().unwrap();
                    self.do_text_remove(start_pos, end_pos - start_pos)?;
                    let indent = self.get_indent(self.start(parent));
                    let prefix = self.formatter.prefix(fmt::METHOD_BODY, indent);
                    self.do_text_insert(start_pos, &prefix)?;
                    self.do_text_insert_node(start_pos, body, indent, true, 0)?;
                    return Ok(());
                }
                change::REMOVED => {
                    let body = orig_id(ev.original_value().node().unwrap());
                    let end_pos = self.end(parent);
                    self.do_text_remove_and_visit(start_pos, end_pos - start_pos, body)?;
                    self.do_text_insert(start_pos, ";")?;
                    return Ok(());
                }
                change::REPLACED => {
                    let body = orig_id(ev.original_value().node().unwrap());
                    self.do_text_remove_and_visit(self.start(body), self.length(body), body)?;
                    let new = ev.new_value().node().unwrap();
                    let indent = self.get_indent(self.start(body));
                    self.do_text_insert_node(self.start(body), new, indent, true, 0)?;
                    return Ok(());
                }
                _ => {}
            }
        }
        self.void_visit_prop(parent, "body")
    }

    fn rewrite_extra_dimensions_info(&mut self, node: NodeId, pos: i32, prop: &str) -> R<i32> {
        self.rewrite_node_list(node, prop, pos, " ", "")
    }

    fn pos_after_token(&mut self, pos: i32, tok: Tok) -> R<i32> {
        let next = self.scanner.read_next_at(pos, true)?;
        if next == tok {
            return Ok(self.scanner.current_end_offset());
        }
        Ok(pos)
    }

    fn pos_after_left_brace(&mut self, pos: i32) -> R<i32> {
        self.pos_after_token(pos, LBRACE)
    }

    fn pos_after_right_parenthesis(&mut self, pos: i32) -> R<i32> {
        self.pos_after_token(pos, RPAREN)
    }

    fn pos_after_try(&mut self, pos: i32) -> R<i32> {
        self.pos_after_token(pos, Tok::Kw("try"))
    }

    /// `doTextInsert(insertOffset, node, initialIndentLevel, removeLeadingIndent, minimumIndentInSpaces)`.
    fn do_text_insert_node(&mut self, insert_offset: i32, node: RNode, initial_indent: i32, remove_leading_indent: bool, minimum_indent: i32) -> R<()> {
        let mut markers: Vec<NodeMarker> = Vec::new();
        let mut formatted = self.formatter.formatted_result(self.rw, node, initial_indent, minimum_indent, &mut markers);
        if initial_indent * self.formatter.indent_width < minimum_indent {
            let fv = indent::to_u16(&formatted);
            let lines: Vec<String> = split_lines(&fv);
            for curr in markers.iter_mut() {
                let offset = curr.offset;
                let mut total = 0i32;
                let mut line_count = 0usize;
                let mut total_padding = 0i32;
                while total <= offset && line_count < lines.len() {
                    total += indent::len16(&lines[line_count]) as i32 + 1;
                    let iol = self.formatter.get_indent_string_with_spaces(&lines[line_count]);
                    let sp = self.formatter.compute_indent_in_spaces(&iol);
                    if sp < minimum_indent {
                        total_padding += minimum_indent - sp;
                    }
                    line_count += 1;
                }
                curr.offset = offset + total_padding;
                total_padding = 0;
                let marker_end = offset + curr.length;
                while total <= marker_end && line_count < lines.len() {
                    total += indent::len16(&lines[line_count]) as i32 + 1;
                    let iol = self.formatter.get_indent_string_with_spaces(&lines[line_count]);
                    let sp = self.formatter.compute_indent_in_spaces(&iol);
                    if sp < minimum_indent {
                        total_padding += minimum_indent - sp;
                    }
                    line_count += 1;
                }
                curr.length += total_padding;
            }
            formatted = self.reindent(minimum_indent, &lines);
        }
        let fv = indent::to_u16(&formatted);
        let mut curr_pos = 0usize;
        if remove_leading_indent {
            while curr_pos < fv.len() && indent::is_whitespace(fv[curr_pos]) {
                curr_pos += 1;
            }
        }
        let mut i = 0;
        while i < markers.len() {
            let curr = markers[i].clone();
            let offset = curr.offset.max(0) as usize;
            if offset >= curr_pos {
                let insert_str = indent::from_u16(&fv[curr_pos..offset.min(fv.len())]);
                self.do_text_insert(insert_offset, &insert_str)?;
                let line_offset = current_line_start_u16(&fv, offset);
                let dest_indent = if line_offset == 0 {
                    self.create_indent_string_min(initial_indent, minimum_indent)
                } else {
                    self.indent_string(&indent::from_u16(&fv[line_offset..offset.min(fv.len())]), minimum_indent)
                };
                match &curr.data {
                    MarkerData::Copy(info) => {
                        let source_info = &self.rw.copy_sources[*info];
                        let src_node = source_info.range.map_or(source_info.node, |(_, last)| last);
                        let indent_node = source_info.range.map_or(source_info.node, |(first, _)| first);
                        let src_indent_level = self.get_indent(self.start(indent_node));
                        let source_edit = self.copy_source_edit(*info);
                        self.do_text_copy(source_edit, insert_offset, src_indent_level, &dest_indent)?;
                        curr_pos = offset + curr.length.max(0) as usize;
                        if self.needs_new_line_for_line_comment(src_node, &fv, curr_pos) {
                            let d = self.line_delimiter();
                            self.do_text_insert(insert_offset, &d)?;
                        }
                    }
                    MarkerData::Str(code) => {
                        let mut code = code.clone();
                        let dest_spaces = self.formatter.compute_indent_in_spaces(&dest_indent);
                        let lines: Vec<&str> = code.split('\n').collect();
                        let mut need_indent = false;
                        if minimum_indent == dest_spaces && minimum_indent % self.formatter.indent_width != 0 && lines.len() > 1 {
                            let mod_tab = self.formatter.compute_indent_in_spaces(&dest_indent) % self.formatter.indent_width;
                            let trimmed_dest = indent::sub16(&dest_indent, 0, indent::len16(&dest_indent) - mod_tab as usize);
                            let mut b = String::new();
                            b.push_str(lines[0]);
                            b.push('\n');
                            for line in &lines[1..lines.len() - 1] {
                                if !line.is_empty() {
                                    let iol = self.formatter.get_indent_string_with_spaces(line);
                                    let rest = indent::sub16(line, indent::len16(&iol), indent::len16(line));
                                    let l = if self.formatter.compute_indent_in_spaces(&iol) < minimum_indent {
                                        format!("{iol}{dest_indent}{rest}")
                                    } else {
                                        format!("{iol}{trimmed_dest}{rest}")
                                    };
                                    b.push_str(&l);
                                    b.push('\n');
                                }
                            }
                            let line = lines[lines.len() - 1];
                            let iol = self.formatter.get_indent_string_with_spaces(line);
                            let rest = indent::sub16(line, indent::len16(&iol), indent::len16(line));
                            let l = if self.formatter.compute_indent_in_spaces(&iol) < minimum_indent {
                                format!("{iol}{dest_indent}{rest}")
                            } else {
                                format!("{iol}{trimmed_dest}{rest}")
                            };
                            b.push_str(&l);
                            code = b;
                        } else {
                            need_indent = true;
                        }
                        let s = if need_indent { self.formatter.change_indent(&code, 0, &dest_indent) } else { code };
                        self.do_text_insert(insert_offset, &s)?;
                        curr_pos = offset + curr.length.max(0) as usize;
                    }
                }
            }
            i += 1;
        }
        if curr_pos < fv.len() {
            let s = indent::from_u16(&fv[curr_pos..]);
            self.do_text_insert(insert_offset, &s)?;
        }
        Ok(())
    }

    fn reindent(&self, minimum: i32, lines: &[String]) -> String {
        let mut s = String::new();
        let pad = |line: &str| -> String {
            if line.is_empty() {
                return line.to_owned();
            }
            let sp = self.formatter.compute_indent_in_spaces(line);
            if sp < minimum {
                let iol = self.formatter.get_indent_string_with_spaces(line);
                let rest = indent::sub16(line, indent::len16(&iol), indent::len16(line));
                format!("{iol}{}{rest}", " ".repeat((minimum - sp) as usize))
            } else {
                line.to_owned()
            }
        };
        for line in &lines[..lines.len().saturating_sub(1)] {
            s.push_str(&pad(line));
            s.push('\n');
        }
        if let Some(last) = lines.last() {
            s.push_str(&pad(last));
        }
        s
    }

    fn needs_new_line_for_line_comment(&self, node: NodeId, formatted: &[u16], offset: usize) -> bool {
        if !self.is_end_of_line_comment_in_content(self.extended_end(node)) {
            return false;
        }
        offset < formatted.len() && !indent::is_line_delimiter_char(formatted[offset])
    }

    fn rewrite_modifiers2(&mut self, node: NodeId, prop: &str, mut pos: i32) -> R<i32> {
        let Some(ev) = self.event(node, prop).filter(|e| e.change_kind() != change::UNCHANGED) else {
            return self.do_visit_prop(node, prop, pos);
        };
        let children = ev.children();
        let is_all_insert = is_all_of_kind(&children, change::INSERTED);
        let is_all_remove = is_all_of_kind(&children, change::REMOVED);
        let kind = self.kind(o(node));
        let is_varargs_annotations = kind == NodeKind::SingleVariableDeclaration && prop == "varargsAnnotations";
        let mut keyword = "";
        if is_varargs_annotations {
            keyword = " ";
        } else if is_all_insert || is_all_remove {
            pos = self.scanner.next_start_offset(pos, false)?;
        }
        let is_annotations_property = is_varargs_annotations || (kind.is_annotatable_type() && prop == "annotations");
        let local_modifiers = matches!(kind, NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationExpression | NodeKind::VariableDeclarationStatement)
            && prop == "modifiers"
            || kind == NodeKind::TypeParameter && prop == "modifiers";
        let formatter_prefix = if !local_modifiers && !is_annotations_property {
            fmt::ANNOTATION_SEPARATION
        } else {
            let parent_kind = self.rw.ast.node(node).parent().map(|p| p.kind());
            match parent_kind {
                Some(NodeKind::MethodDeclaration) => fmt::PARAM_ANNOTATION_SEPARATION,
                Some(NodeKind::Block) | Some(NodeKind::TryStatement) | Some(NodeKind::ForStatement) => fmt::LOCAL_ANNOTATION_SEPARATION,
                _ => fmt::TYPE_ANNOTATION_SEPARATION,
            }
        };
        let mut lr = ListRewriter::new(ListKind::Modifier { annotation_separation: formatter_prefix });
        lr.constant_separator = " ".into();
        let end_pos = self.rewrite_list(&mut lr, node, prop, keyword, None, pos)?;
        let next_pos = self.scanner.next_start_offset(end_pos, false)?;
        let last_child = children[children.len() - 1].clone();
        let last_unchanged = last_child.change_kind() != change::UNCHANGED;
        if is_all_remove {
            self.do_text_remove(end_pos, next_pos - end_pos)?;
            return Ok(next_pos);
        }
        if (is_all_insert || next_pos == end_pos && last_unchanged) && !is_varargs_annotations {
            let separator = if last_child.new.is_some_and(|n| self.kind(n).is_annotation()) {
                let mut sep = self.formatter.prefix(formatter_prefix, self.get_indent(pos));
                let extra = self.formatter.compute_indent_in_spaces(&self.indent_of_line(pos)) % self.formatter.tab_width.max(1);
                for _ in 0..extra {
                    sep.push(' ');
                }
                sep
            } else {
                " ".to_owned()
            };
            self.do_text_insert(end_pos, &separator)?;
        }
        Ok(end_pos)
    }

    fn replace_operation(&mut self, pos_before: i32, new_operation: &str) -> R<()> {
        self.scanner.read_next_at(pos_before, true)?;
        let (s, l) = (self.scanner.current_start_offset(), self.scanner.current_length());
        self.do_text_replace(s, l, new_operation)
    }

    fn rewrite_operation(&mut self, parent: NodeId, prop: &str, pos_before: i32) -> R<()> {
        if let Some(ev) = self.event(parent, prop).filter(|e| e.change_kind() != change::UNCHANGED) {
            let new_operation = ev.new_value().simple().unwrap_or("").to_owned();
            self.replace_operation(pos_before, &new_operation)?;
        }
        Ok(())
    }

    // ── Visiting ────────────────────────────────────────────────────────────

    fn accept(&mut self, node: NodeId) -> R<()> {
        self.pre_visit(node)?;
        self.visit(node)?;
        self.post_visit(node);
        Ok(())
    }

    fn pre_visit(&mut self, node: NodeId) -> R<()> {
        for info in self.rw.node_copy_sources(node) {
            let e = self.copy_source_edit(info);
            self.add_edit(e)?;
            self.current_edit = e;
            self.source_copy_end_nodes.push(node);
        }
        self.ensure_space_before_replace(node)
    }

    fn post_visit(&mut self, node: NodeId) {
        while self.source_copy_end_nodes.last() == Some(&node) {
            self.source_copy_end_nodes.pop();
            self.current_edit = self.edits.edits[self.current_edit].parent.unwrap_or(EditTree::ROOT);
        }
    }

    fn ensure_space_after_replace(&mut self, node: NodeId, prop: &str) -> R<()> {
        if self.change_kind(node, prop) == change::REPLACED {
            let orig = orig_id(self.original_value(node, prop).node().unwrap());
            let left_end = self.extended_end(orig);
            let offset = self.scanner.next_start_offset(left_end, true)?;
            if offset == left_end {
                self.do_text_insert(offset, " ")?;
            }
        }
        Ok(())
    }

    fn ensure_space_before_replace(&mut self, node: NodeId) -> R<()> {
        if self.before_required_space_index != -1 {
            let events: Vec<Event> = self.rw.changed_property_events(o(node)).into_iter().cloned().collect();
            for ev in events {
                if ev.change_kind() == change::REPLACED {
                    if let Some(RNode::Orig(orig)) = ev.original_value().node() {
                        if self.before_required_space_index == self.extended_offset(orig) {
                            let idx = self.before_required_space_index;
                            self.do_text_insert(idx, " ")?;
                            self.before_required_space_index = -1;
                            return Ok(());
                        }
                    }
                }
            }
            if self.before_required_space_index < self.extended_offset(node) {
                self.before_required_space_index = -1;
            }
        }
        Ok(())
    }

    fn visit(&mut self, node: NodeId) -> R<()> {
        use NodeKind::*;
        let kind = self.kind(o(node));
        // Statements that track the space required after their keyword.
        match kind {
            ReturnStatement => {
                self.before_required_space_index = self.scanner.token_end_offset(Tok::Kw("return"), self.start(node))?;
            }
            AssertStatement => {
                self.before_required_space_index = self.scanner.next_end_offset(self.start(node), true)?;
            }
            ThrowStatement => {
                self.before_required_space_index = self.scanner.token_end_offset(Tok::Kw("throw"), self.start(node))?;
            }
            _ => {}
        }
        if !self.has_children_changes(node) {
            return self.do_visit_unchanged_children(node);
        }
        match kind {
            CompilationUnit => {
                let mut start_pos = 0;
                let is_module_info = self.original_value(node, "module").node().is_some();
                if !is_module_info {
                    start_pos = self.rewrite_node(node, "package", 0, fmt::NONE)?;
                    if self.change_kind(node, "package") == change::INSERTED {
                        let d = self.line_delimiter();
                        self.do_text_insert(0, &d)?;
                    }
                }
                start_pos = self.rewrite_paragraph_list(node, "imports", start_pos, 0, 0, 2)?;
                if is_module_info {
                    self.rewrite_node(node, "module", start_pos, fmt::NONE)?;
                } else {
                    self.rewrite_paragraph_list(node, "types", start_pos, 0, -1, 2)?;
                }
            }
            ImplicitTypeDeclaration => {
                self.rewrite_javadoc(node, "javadoc")?;
                let indent = self.get_indent(self.start(node)) + 1;
                self.rewrite_paragraph_list(node, "bodyDeclarations", self.start(node), indent, -1, 2)?;
            }
            TypeDeclaration => self.visit_type_declaration(node)?,
            MethodDeclaration => self.visit_method_declaration(node)?,
            Dimension => {
                let mut keyword_space = true;
                let parent = self.rw.ast.node(node).parent();
                if parent.is_some_and(|p| p.kind() == ArrayType) {
                    let old = self.original_value(node, "annotations").list();
                    let new = self.new_value(node, "annotations").list();
                    if !old.is_empty() && new.is_empty() {
                        // Remove the annotations (and the space before them).
                        let first = orig_id(old[0]);
                        let last = orig_id(old[old.len() - 1]);
                        let prev = self.previous_dimension_node(node);
                        if let Some(prev) = prev {
                            let mut off = self.end(prev);
                            off = self.scanner.previous_token_end_offset(Tok::Op("@"), off).unwrap_or(off);
                            let del_end = self.start(first);
                            if off >= 0 && del_end > off {
                                self.do_text_remove(off, del_end - off)?;
                            }
                        }
                        let del_start = self.end(last);
                        if let Ok(del_end) = self.scanner.next_start_offset(del_start, false) {
                            self.do_text_remove(del_start, del_end - del_start)?;
                        }
                    } else if old.is_empty() && !new.is_empty() && self.start(node) > 0 && indent::is_whitespace(self.content[self.start(node) as usize - 1]) {
                        keyword_space = false;
                    }
                }
                self.rewrite_node_list_end(node, "annotations", self.start(node), if keyword_space { " " } else { "" }, " ", " ")?;
            }
            ModuleDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                let pos = self.rewrite_modifiers2(node, "annotations", pos)?;
                if let Some(ev) = self.event(node, "open").filter(|e| e.change_kind() != change::UNCHANGED) {
                    if ev.original_value().flag() {
                        let end = self.scanner.token_start_offset(Tok::Ident, pos)?;
                        self.do_text_remove(pos, end - pos)?;
                    } else {
                        self.do_text_insert(pos, "open ")?;
                    }
                }
                let pos = self.rewrite_required_node(node, "name")?;
                let start_pos = self.pos_after_left_brace(pos)?;
                let indent = self.get_indent(self.start(node)) + 1;
                self.rewrite_paragraph_list(node, "moduleDirectives", start_pos, indent, 0, 1)?;
            }
            Block => {
                let collapsed = false;
                let start_pos = if collapsed { self.start(node) } else { self.pos_after_left_brace(self.start(node))? };
                let mut need_parent_indent = false;
                let n = self.rw.ast.node(node);
                if n.location_is("body") && n.parent().is_some_and(|p| p.kind() == TryStatement) {
                    let parent = n.parent().unwrap();
                    let resources = parent.list("resources");
                    if let Some(last) = resources.last() {
                        if self.line_of_offset(last.start() as i32) == self.line_of_offset(self.start(node)) {
                            need_parent_indent = true;
                        }
                    }
                }
                let indent = if need_parent_indent {
                    self.get_indent(n.parent().unwrap().start() as i32) + 1
                } else {
                    self.get_indent(self.start(node)) + 1
                };
                self.rewrite_paragraph_list(node, "statements", start_pos, indent, 0, 1)?;
            }
            RecordDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "modifiers", pos)?;
                let pos = self.rewrite_required_node(node, "name")?;
                let mut pos = self.rewrite_optional_type_parameters(node, "typeParameters", pos, "", false, true)?;
                pos = self.scanner.token_end_offset(LPAREN, pos).unwrap_or(pos);
                pos = self.rewrite_node_list(node, "recordComponents", pos, "", ", ")?;
                pos = self.scanner.token_end_offset(RPAREN, pos).unwrap_or(pos);
                if self.change_kind(node, "superInterfaceTypes") != change::UNCHANGED {
                    pos = self.rewrite_node_list(node, "superInterfaceTypes", pos, " implements ", ", ")?;
                } else {
                    pos = self.do_visit_prop(node, "superInterfaceTypes", pos)?;
                }
                let indent = self.get_indent(self.start(node)) + 1;
                pos = self.pos_after_right_parenthesis(pos)?;
                pos = self.pos_after_left_brace(pos)?;
                self.rewrite_paragraph_list(node, "bodyDeclarations", pos, indent, -1, 2)?;
            }
            RecordPattern => {
                let pos = self.rewrite_required_node(node, "patternType")?;
                self.rewrite_node_list(node, "patterns", pos, "", ", ")?;
            }
            EitherOrMultiPattern => {
                self.rewrite_node_list(node, "patterns", self.start(node), "", ", ")?;
            }
            ReturnStatement => {
                self.ensure_space_before_replace(node)?;
                let idx = self.before_required_space_index;
                self.rewrite_node(node, "expression", idx, fmt::SPACE)?;
            }
            RequiresDirective => {
                let pos = self.pos_after_token(self.start(node), Tok::Ident)?;
                self.rewrite_node_list(node, "modifiers", pos, " ", " ")?;
                self.rewrite_required_node(node, "name")?;
            }
            AnonymousClassDeclaration => {
                let start_pos = self.pos_after_left_brace(self.start(node))?;
                let indent = self.get_indent(self.start(node)) + 1;
                self.rewrite_paragraph_list(node, "bodyDeclarations", start_pos, indent, -1, 2)?;
            }
            ArrayAccess => {
                self.rewrite_required_node(node, "array")?;
                self.rewrite_required_node(node, "index")?;
            }
            ArrayCreation => self.visit_array_creation(node)?,
            ArrayInitializer => {
                let start_pos = self.pos_after_left_brace(self.start(node))?;
                self.rewrite_node_list(node, "expressions", start_pos, "", ", ")?;
            }
            ArrayType => {
                let pos = self.rewrite_required_node(node, "elementType")?;
                self.rewrite_node_list(node, "dimensions", pos, "", "")?;
            }
            AssertStatement => {
                self.ensure_space_before_replace(node)?;
                let offset = self.rewrite_required_node(node, "expression")?;
                self.rewrite_node(node, "message", offset, fmt::ASSERT_COMMENT)?;
            }
            Assignment => {
                let pos = self.rewrite_required_node(node, "leftHandSide")?;
                self.rewrite_operation(node, "operator", pos)?;
                self.rewrite_required_node(node, "rightHandSide")?;
            }
            BooleanLiteral => {
                let v = self.new_value(node, "booleanValue").simple().unwrap_or("false").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            BreakStatement | ContinueStatement => {
                let kw = if kind == BreakStatement { "break" } else { "continue" };
                let offset = self.scanner.token_end_offset(Tok::Kw(kw), self.start(node))?;
                self.rewrite_node(node, "label", offset, fmt::SPACE)?;
            }
            CastExpression => {
                self.rewrite_required_node(node, "type")?;
                self.rewrite_required_node(node, "expression")?;
            }
            CatchClause => {
                self.rewrite_required_node(node, "exception")?;
                self.rewrite_required_node(node, "body")?;
            }
            CharacterLiteral | StringLiteral | TextBlock => {
                let v = self.new_value(node, "escapedValue").simple().unwrap_or("").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            ClassInstanceCreation => self.visit_class_instance_creation(node)?,
            ConditionalExpression => {
                self.rewrite_required_node(node, "expression")?;
                self.rewrite_required_node(node, "thenExpression")?;
                self.rewrite_required_node(node, "elseExpression")?;
            }
            ConstructorInvocation => {
                let pos = self.rewrite_optional_type_parameters(node, "typeArguments", self.start(node), "", false, false)?;
                let pos = self.scanner.token_end_offset(LPAREN, pos)?;
                self.rewrite_node_list(node, "arguments", pos, "", ", ")?;
            }
            CreationReference => {
                let pos = self.rewrite_required_node(node, "type")?;
                self.visit_reference_type_arguments(node, "typeArguments", pos)?;
            }
            DoStatement => {
                let pos = self.start(node);
                if let Some(ev) = self.event(node, "body").filter(|e| e.change_kind() == change::REPLACED) {
                    let start_offset = self.scanner.token_end_offset(Tok::Kw("do"), pos)?;
                    let body = orig_id(ev.original_value().node().unwrap());
                    let end_pos = self.scanner.token_start_offset(Tok::Kw("while"), self.end(body))?;
                    let indent = self.get_indent(self.start(node));
                    self.rewrite_body_node(node, "body", start_offset, end_pos, indent, fmt::DO_BLOCK)?;
                } else {
                    self.void_visit_prop(node, "body")?;
                }
                self.rewrite_required_node(node, "expression")?;
            }
            ExportsDirective | OpensDirective => {
                let pos = self.rewrite_required_node(node, "name")?;
                self.rewrite_node_list(node, "modules", pos, "to ", ", ")?;
            }
            ExpressionStatement => {
                self.rewrite_required_node(node, "expression")?;
            }
            FieldAccess => {
                self.rewrite_required_node(node, "expression")?;
                self.rewrite_required_node(node, "name")?;
            }
            FieldDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "modifiers", pos)?;
                let pos = self.rewrite_required_node(node, "type")?;
                self.ensure_space_after_replace(node, "type")?;
                self.rewrite_node_list(node, "fragments", pos, "", ", ")?;
            }
            ForStatement => {
                let mut pos = self.start(node);
                if self.is_changed(node, "initializers") {
                    let start_offset = self.scanner.token_end_offset(LPAREN, pos)?;
                    pos = self.rewrite_node_list(node, "initializers", start_offset, "", ", ")?;
                } else {
                    pos = self.do_visit_prop(node, "initializers", pos)?;
                }
                pos = self.scanner.token_end_offset(SEMICOLON, pos)?;
                pos = self.rewrite_node(node, "expression", pos, fmt::NONE)?;
                if self.is_changed(node, "updaters") {
                    let start_offset = self.scanner.token_end_offset(SEMICOLON, pos)?;
                    pos = self.rewrite_node_list(node, "updaters", start_offset, "", ", ")?;
                } else {
                    pos = self.do_visit_prop(node, "updaters", pos)?;
                }
                if self.change_kind(node, "body") == change::REPLACED {
                    let start_offset = self.scanner.token_end_offset(RPAREN, pos)?;
                    let indent = self.get_indent(self.start(node));
                    self.rewrite_body_node(node, "body", start_offset, -1, indent, fmt::FOR_BLOCK)?;
                } else {
                    self.void_visit_prop(node, "body")?;
                }
            }
            GuardedPattern => {
                self.rewrite_required_node(node, "pattern")?;
                self.rewrite_required_node(node, "expression")?;
            }
            IfStatement => self.visit_if_statement(node)?,
            ImportDeclaration => {
                if let Some(ev) = self.event(node, "static").filter(|e| e.change_kind() != change::UNCHANGED) {
                    let pos = self.scanner.token_end_offset(Tok::Kw("import"), self.start(node))?;
                    if ev.original_value().flag() {
                        let end = self.scanner.token_end_offset(Tok::Kw("static"), pos)?;
                        self.do_text_remove(pos, end - pos)?;
                    } else {
                        self.do_text_insert(pos, " static")?;
                    }
                }
                let pos = self.rewrite_required_node(node, "name")?;
                if let Some(ev) = self.event(node, "onDemand").filter(|e| e.change_kind() != change::UNCHANGED) {
                    if !ev.original_value().flag() {
                        self.do_text_insert(pos, ".*")?;
                    } else {
                        let end = self.scanner.token_start_offset(SEMICOLON, pos)?;
                        self.do_text_remove(pos, end - pos)?;
                    }
                }
            }
            InfixExpression => self.visit_infix_expression(node)?,
            Initializer => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "modifiers", pos)?;
                self.rewrite_required_node(node, "body")?;
            }
            InstanceofExpression => {
                self.rewrite_required_node(node, "leftOperand")?;
                self.ensure_space_after_replace(node, "leftOperand")?;
                self.rewrite_required_node(node, "rightOperand")?;
            }
            PatternInstanceofExpression => {
                self.rewrite_required_node(node, "leftOperand")?;
                self.ensure_space_after_replace(node, "leftOperand")?;
                if self.rw.ast.node(node).has_prop("pattern") {
                    self.rewrite_required_node(node, "pattern")?;
                } else {
                    self.rewrite_required_node(node, "rightOperand")?;
                }
            }
            IntersectionType => {
                self.rewrite_node_list(node, "types", self.start(node), "", " & ")?;
            }
            Javadoc => {
                let start_pos = self.start(node) + 3;
                let separator = self.line_delimiter() + &self.indent_at_offset(self.start(node)) + " * ";
                self.rewrite_node_list_end(node, "tags", start_pos, &separator, &separator, &separator)?;
            }
            JavaDocTextElement | TextElement => {
                let v = self.new_value(node, "text").simple().unwrap_or("").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            LabeledStatement => {
                self.rewrite_required_node(node, "label")?;
                self.rewrite_required_node(node, "body")?;
            }
            LambdaExpression => self.visit_lambda(node)?,
            MethodInvocation => {
                let pos = self.rewrite_optional_qualifier(node, "expression", self.start(node))?;
                self.rewrite_optional_type_parameters(node, "typeArguments", pos, "", false, false)?;
                let pos = self.rewrite_required_node(node, "name")?;
                if self.is_changed(node, "arguments") {
                    let start_offset = self.scanner.token_end_offset(LPAREN, pos)?;
                    self.rewrite_node_list(node, "arguments", start_offset, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "arguments")?;
                }
            }
            NumberLiteral => {
                let v = self.new_value(node, "token").simple().unwrap_or("").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            PackageDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "annotations", pos)?;
                self.rewrite_required_node(node, "name")?;
            }
            ParenthesizedExpression => {
                self.rewrite_required_node(node, "expression")?;
            }
            PostfixExpression => {
                let pos = self.rewrite_required_node(node, "operand")?;
                self.rewrite_operation(node, "operator", pos)?;
            }
            PrefixExpression => {
                self.rewrite_operation(node, "operator", self.start(node))?;
                self.rewrite_required_node(node, "operand")?;
            }
            PrimitiveType => {
                self.rewrite_modifiers2(node, "annotations", self.start(node))?;
                let v = self.new_value(node, "primitiveTypeCode").simple().unwrap_or("int").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            ProvidesDirective => {
                let pos = self.rewrite_required_node(node, "name")?;
                self.rewrite_node_list(node, "implementations", pos, " with ", ", ")?;
            }
            QualifiedName => {
                self.rewrite_required_node(node, "qualifier")?;
                self.rewrite_required_node(node, "name")?;
            }
            SimpleName => {
                let v = self.new_value(node, "identifier").simple().unwrap_or("").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            SimpleType => {
                self.rewrite_modifiers2(node, "annotations", self.start(node))?;
                self.rewrite_required_node(node, "name")?;
            }
            SingleVariableDeclaration => self.visit_single_variable_declaration(node)?,
            SuperConstructorInvocation => {
                let pos = self.rewrite_optional_qualifier(node, "expression", self.start(node))?;
                let mut pos = self.rewrite_optional_type_parameters(node, "typeArguments", pos, "", false, false)?;
                if self.is_changed(node, "arguments") {
                    pos = self.scanner.token_end_offset(LPAREN, pos)?;
                    self.rewrite_node_list(node, "arguments", pos, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "arguments")?;
                }
            }
            SuperFieldAccess => {
                self.rewrite_optional_qualifier(node, "qualifier", self.start(node))?;
                self.rewrite_required_node(node, "name")?;
            }
            SuperMethodInvocation => {
                let mut pos = self.rewrite_optional_qualifier(node, "qualifier", self.start(node))?;
                if self.is_changed(node, "typeArguments") {
                    pos = self.scanner.token_end_offset(DOT, pos)?;
                    self.rewrite_optional_type_parameters(node, "typeArguments", pos, "", false, false)?;
                }
                let pos = self.rewrite_required_node(node, "name")?;
                if self.is_changed(node, "arguments") {
                    let pos = self.scanner.token_end_offset(LPAREN, pos)?;
                    self.rewrite_node_list(node, "arguments", pos, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "arguments")?;
                }
            }
            SwitchCase => {
                let exprs = self.rw.ast.node(node).list("expression");
                let mut pos = if exprs.is_empty() {
                    self.start(node)
                } else {
                    self.rewrite_node_list(node, "expression", self.start(node), "", ", ")?
                };
                if self.is_changed(node, "switchLabeledRule") {
                    let (old_tok, new_val) = if self.new_value(node, "switchLabeledRule").flag() { (COLON, "->") } else { (ARROW, ":") };
                    pos = self.scanner.token_start_offset(old_tok, pos)?;
                    let end = self.scanner.token_end_offset(old_tok, pos)?;
                    self.do_text_remove(pos, end - pos)?;
                    self.do_text_insert(pos, new_val)?;
                }
            }
            SwitchExpression | SwitchStatement => {
                let pos = self.rewrite_required_node(node, "expression")?;
                if self.change_kind(node, "statements") != change::UNCHANGED {
                    let pos = self.scanner.token_end_offset(LBRACE, pos)?;
                    let mut insert_indent = self.get_indent(self.start(node));
                    if self.options.get("org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_switch").map(String::as_str) == Some("true") {
                        insert_indent += 1;
                    }
                    let indent_compare = self.options.get("org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_cases").map(String::as_str) == Some("true");
                    let mut lr = ListRewriter::new(ListKind::Switch { initial_indent: insert_indent, indent_compare, labeled_rule: true });
                    let lead = self.line_delimiter() + &self.create_indent_string(insert_indent);
                    self.rewrite_list(&mut lr, node, "statements", &lead, None, pos)?;
                } else {
                    self.void_visit_prop(node, "statements")?;
                }
            }
            SynchronizedStatement => {
                self.rewrite_required_node(node, "expression")?;
                self.rewrite_required_node(node, "body")?;
            }
            ThisExpression => {
                self.rewrite_optional_qualifier(node, "qualifier", self.start(node))?;
            }
            ThrowStatement => {
                self.ensure_space_before_replace(node)?;
                self.rewrite_required_node(node, "expression")?;
            }
            TryStatement => {
                let pos = self.start(node);
                if self.is_changed(node, "resources") {
                    let indent = self.get_indent(self.start(node));
                    let prefix = self.formatter.prefix(fmt::TRY_RESOURCES, indent);
                    let new_paren = self.formatter.prefix(fmt::TRY_RESOURCES_PAREN, indent) + "(";
                    let after = self.pos_after_try(pos)?;
                    self.rewrite_resources_node_list(node, "resources", after, &new_paren, ")", &format!(";{prefix}"))?;
                } else {
                    self.do_visit_prop(node, "resources", pos)?;
                }
                let mut pos = self.rewrite_required_node(node, "body")?;
                if self.is_changed(node, "catchClauses") {
                    let indent = self.get_indent(self.start(node));
                    let prefix = self.formatter.prefix(fmt::CATCH_BLOCK, indent);
                    pos = self.rewrite_node_list_end(node, "catchClauses", pos, &prefix, &prefix, &prefix)?;
                } else {
                    pos = self.do_visit_prop(node, "catchClauses", pos)?;
                }
                self.rewrite_node(node, "finally", pos, fmt::FINALLY_BLOCK)?;
            }
            TypeDeclarationStatement => {
                self.rewrite_required_node(node, "declaration")?;
            }
            TypeLiteral => {
                self.rewrite_required_node(node, "type")?;
            }
            UnionType => {
                self.rewrite_node_list(node, "types", self.start(node), "", " | ")?;
            }
            UsesDirective => {
                self.rewrite_required_node(node, "name")?;
            }
            VariableDeclarationExpression | VariableDeclarationStatement => {
                self.rewrite_modifiers2(node, "modifiers", self.start(node))?;
                let pos = self.rewrite_required_node(node, "type")?;
                self.rewrite_node_list(node, "fragments", pos, "", ", ")?;
            }
            VariableDeclarationFragment => {
                let pos = self.rewrite_required_node(node, "name")?;
                let pos = self.rewrite_extra_dimensions_info(node, pos, "extraDimensions2")?;
                self.rewrite_node(node, "initializer", pos, fmt::VAR_INITIALIZER)?;
            }
            WhileStatement => {
                let pos = self.rewrite_required_node(node, "expression")?;
                if self.is_changed(node, "body") {
                    let start_offset = self.scanner.token_end_offset(RPAREN, pos)?;
                    let indent = self.get_indent(self.start(node));
                    self.rewrite_body_node(node, "body", start_offset, -1, indent, fmt::WHILE_BLOCK)?;
                } else {
                    self.void_visit_prop(node, "body")?;
                }
            }
            MemberRef => {
                self.rewrite_node(node, "qualifier", self.start(node), fmt::NONE)?;
                self.rewrite_required_node(node, "name")?;
            }
            MethodRef => {
                self.rewrite_node(node, "qualifier", self.start(node), fmt::NONE)?;
                let pos = self.rewrite_required_node(node, "name")?;
                if self.is_changed(node, "parameters") {
                    let start_offset = self.scanner.token_end_offset(LPAREN, pos)?;
                    self.rewrite_node_list(node, "parameters", start_offset, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "parameters")?;
                }
            }
            MethodRefParameter => {
                let pos = self.rewrite_required_node(node, "type")?;
                if self.is_changed(node, "varargs") {
                    if self.new_value(node, "varargs").flag() {
                        self.do_text_insert(pos, "...")?;
                    } else {
                        let end = self.scanner.next_end_offset(pos, true)?;
                        self.do_text_remove(pos, end - pos)?;
                    }
                }
                self.rewrite_node(node, "name", pos, fmt::SPACE)?;
            }
            TagElement => {
                let ck = self.change_kind(node, "tagName");
                match ck {
                    change::INSERTED => {
                        let v = self.new_value(node, "tagName").simple().unwrap_or("").to_owned();
                        self.do_text_insert(self.start(node), &v)?;
                    }
                    change::REMOVED => {
                        let end = self.find_tag_name_end(node);
                        self.do_text_remove(self.start(node), end - self.start(node))?;
                    }
                    change::REPLACED => {
                        let v = self.new_value(node, "tagName").simple().unwrap_or("").to_owned();
                        let end = self.find_tag_name_end(node);
                        self.do_text_replace(self.start(node), end - self.start(node), &v)?;
                    }
                    _ => {}
                }
                if self.is_changed(node, "fragments") {
                    let end = self.find_tag_name_end(node);
                    self.rewrite_node_list(node, "fragments", end, " ", " ")?;
                } else {
                    self.void_visit_prop(node, "fragments")?;
                }
            }
            AnnotationTypeDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "modifiers", pos)?;
                let pos = self.rewrite_required_node(node, "name")?;
                let indent = self.get_indent(self.start(node)) + 1;
                let start_pos = self.pos_after_left_brace(pos)?;
                self.rewrite_paragraph_list(node, "bodyDeclarations", start_pos, indent, -1, 2)?;
            }
            AnnotationTypeMemberDeclaration => {
                let pos = self.rewrite_javadoc(node, "javadoc")?;
                self.rewrite_modifiers2(node, "modifiers", pos)?;
                self.rewrite_required_node(node, "type")?;
                let mut pos = self.rewrite_required_node(node, "name")?;
                let ck = self.change_kind(node, "default");
                if ck == change::INSERTED || ck == change::REMOVED {
                    pos = self.scanner.token_end_offset(RPAREN, pos)?;
                }
                self.rewrite_node(node, "default", pos, fmt::ANNOT_MEMBER_DEFAULT)?;
            }
            EnhancedForStatement => {
                self.rewrite_required_node(node, "parameter")?;
                let pos = self.rewrite_required_node(node, "expression")?;
                if self.change_kind(node, "body") == change::REPLACED {
                    let start_offset = self.scanner.token_end_offset(RPAREN, pos)?;
                    let indent = self.get_indent(self.start(node));
                    self.rewrite_body_node(node, "body", start_offset, -1, indent, fmt::FOR_BLOCK)?;
                } else {
                    self.void_visit_prop(node, "body")?;
                }
            }
            EnumConstantDeclaration => self.visit_enum_constant(node)?,
            EnumDeclaration => self.visit_enum_declaration(node)?,
            ExpressionMethodReference => {
                let pos = self.rewrite_required_node(node, "expression")?;
                self.visit_reference_type_arguments(node, "typeArguments", pos)?;
                self.rewrite_required_node(node, "name")?;
            }
            MarkerAnnotation => {
                self.rewrite_required_node(node, "typeName")?;
            }
            MemberValuePair => {
                self.rewrite_required_node(node, "name")?;
                self.rewrite_required_node(node, "value")?;
            }
            Modifier | ModuleModifier => {
                let v = self.new_value(node, "keyword").simple().unwrap_or("").to_owned();
                self.do_text_replace(self.start(node), self.length(node), &v)?;
            }
            NormalAnnotation => {
                let pos = self.rewrite_required_node(node, "typeName")?;
                if self.is_changed(node, "values") {
                    let start_offset = self.scanner.token_end_offset(LPAREN, pos)?;
                    self.rewrite_node_list(node, "values", start_offset, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "values")?;
                }
            }
            NameQualifiedType | QualifiedType => {
                let pos = self.rewrite_required_node(node, "qualifier")?;
                let pos = self.scanner.token_end_offset(DOT, pos)?;
                self.rewrite_modifiers2(node, "annotations", pos)?;
                self.rewrite_required_node(node, "name")?;
            }
            ParameterizedType => {
                let pos = self.rewrite_required_node(node, "type")?;
                if self.is_changed(node, "typeArguments") {
                    let start_offset = self.scanner.token_end_offset(LESS, pos)?;
                    self.rewrite_node_list(node, "typeArguments", start_offset, "", ", ")?;
                } else {
                    self.void_visit_prop(node, "typeArguments")?;
                }
            }
            SingleMemberAnnotation => {
                self.rewrite_required_node(node, "typeName")?;
                self.rewrite_required_node(node, "value")?;
            }
            SuperMethodReference => {
                let pos = self.rewrite_optional_qualifier(node, "qualifier", self.start(node))?;
                self.visit_reference_type_arguments(node, "typeArguments", pos)?;
                self.rewrite_required_node(node, "name")?;
            }
            TypeMethodReference => {
                let pos = self.rewrite_required_node(node, "type")?;
                self.visit_reference_type_arguments(node, "typeArguments", pos)?;
                self.rewrite_required_node(node, "name")?;
            }
            TypeParameter => {
                self.rewrite_modifiers2(node, "modifiers", self.start(node))?;
                let pos = self.rewrite_required_node(node, "name")?;
                self.rewrite_node_list(node, "typeBounds", pos, " extends ", " & ")?;
            }
            TypePattern => {
                if self.rw.ast.node(node).has_prop("patternVariable2") {
                    self.rewrite_required_node(node, "patternVariable2")?;
                } else {
                    self.rewrite_required_node(node, "patternVariable")?;
                }
            }
            WildcardType => {
                self.rewrite_modifiers2(node, "annotations", self.start(node))?;
                let pos = self.scanner.next_end_offset(self.start(node), true)?;
                let prefix = if self.new_value(node, "upperBound").flag() { fmt::WILDCARD_EXTENDS } else { fmt::WILDCARD_SUPER };
                if self.change_kind(node, "upperBound") != change::UNCHANGED {
                    let bound_change = self.change_kind(node, "bound");
                    if bound_change != change::INSERTED && bound_change != change::REMOVED {
                        if let Some(RNode::Orig(t)) = self.original_value(node, "bound").node() {
                            let s = self.formatter.prefix(prefix, 0);
                            self.do_text_replace(pos, self.start(t) - pos, &s)?;
                        }
                    }
                }
                self.rewrite_node(node, "bound", pos, prefix)?;
            }
            YieldStatement => {
                let implicit = self.rw.ast.node(node).flag("implicit");
                let offset = if implicit { self.start(node) } else { self.scanner.token_end_offset(SEMICOLON, self.start(node))? };
                self.rewrite_node(node, "expression", offset, fmt::SPACE)?;
            }
            _ => {
                // EmptyStatement, NullLiteral, ...: changes not supported.
                return Err(RewriteError(format!("Change not supported in {}", kind.name())));
            }
        }
        Ok(())
    }

    fn previous_dimension_node(&self, node: NodeId) -> Option<NodeId> {
        let n = self.rw.ast.node(node);
        let parent = n.parent()?;
        let mut prev = parent.child("elementType")?.id;
        for d in parent.list("dimensions") {
            if d.id == node {
                return Some(prev);
            }
            prev = d.id;
        }
        None
    }

    fn find_tag_name_end(&self, node: NodeId) -> i32 {
        if self.rw.ast.node(node).simple("tagName").is_none() {
            return self.start(node);
        }
        let mut i = self.start(node) as usize;
        while i < self.content.len() && !indent::is_indent_char(self.content[i]) {
            i += 1;
        }
        i as i32
    }

    fn visit_reference_type_arguments(&mut self, node: NodeId, prop: &str, pos: i32) -> R<()> {
        if self.is_changed(node, prop) {
            let pos = self.scanner.token_end_offset(COLON_COLON, pos)?;
            self.rewrite_optional_type_parameters(node, prop, pos, "", false, false)?;
        }
        Ok(())
    }

    fn visit_type_declaration(&mut self, node: NodeId) -> R<()> {
        let pos = self.rewrite_javadoc(node, "javadoc")?;
        self.rewrite_modifiers2(node, "modifiers", pos)?;
        let is_interface = self.original_value(node, "interface").flag();
        let invert_type = self.is_changed(node, "interface");
        if invert_type {
            let type_token = if is_interface { Tok::Kw("interface") } else { Tok::Kw("class") };
            let mut start_position = self.start(node);
            let modifiers = self.rw.ast.node(node).list("modifiers");
            if let Some(last) = modifiers.last() {
                start_position = last.end() as i32;
            }
            if self.scanner.read_to_token_at(type_token, start_position).is_ok() {
                let s = if is_interface { "class" } else { "interface" };
                let (start, end) = (self.scanner.current_start_offset(), self.scanner.current_end_offset());
                self.do_text_replace(start, end - start, s)?;
            }
        }
        let mut pos = self.rewrite_required_node(node, "name")?;
        pos = self.rewrite_optional_type_parameters(node, "typeParameters", pos, "", false, true)?;
        if !is_interface || invert_type {
            let ev = self.event(node, "superclassType");
            let ck = ev.map_or(change::UNCHANGED, Event::change_kind);
            match ck {
                change::UNCHANGED => pos = self.do_visit_prop(node, "superclassType", pos)?,
                change::INSERTED => {
                    let ev = ev.unwrap();
                    self.do_text_insert(pos, " extends ")?;
                    self.do_text_insert_node(pos, ev.new_value().node().unwrap(), 0, false, 0)?;
                }
                change::REMOVED => {
                    let sc = orig_id(ev.unwrap().original_value().node().unwrap());
                    let end_pos = self.extended_end(sc);
                    self.do_text_remove_and_visit(pos, end_pos - pos, sc)?;
                    pos = end_pos;
                }
                change::REPLACED => {
                    let ev = ev.unwrap();
                    let sc = orig_id(ev.original_value().node().unwrap());
                    let (offset, length) = self.extended_range(sc);
                    self.do_text_remove_and_visit(offset, length, sc)?;
                    self.do_text_insert_node(offset, ev.new_value().node().unwrap(), 0, false, 0)?;
                    pos = offset + length;
                }
                _ => {}
            }
        }
        if self.change_kind(node, "superInterfaceTypes") != change::UNCHANGED {
            let mut keyword = if is_interface == invert_type { " implements " } else { " extends " }.to_owned();
            if invert_type {
                let new_nodes = self.new_value(node, "superInterfaceTypes").list();
                if !new_nodes.is_empty() {
                    let orig = self.original_value(node, "superInterfaceTypes").list();
                    let first_start = orig.first().map_or(pos, |f| self.start(orig_id(*f)));
                    self.do_text_replace(pos, first_start - pos, &keyword)?;
                    keyword = String::new();
                    pos = first_start;
                }
            }
            pos = self.rewrite_node_list(node, "superInterfaceTypes", pos, &keyword, ", ")?;
        } else {
            if invert_type {
                let orig = self.original_value(node, "superInterfaceTypes").list();
                if let Some(first) = orig.first() {
                    let keyword = if is_interface { " implements " } else { " extends " };
                    let fs = self.start(orig_id(*first));
                    self.do_text_replace(pos, fs - pos, keyword)?;
                }
            }
            pos = self.do_visit_prop(node, "superInterfaceTypes", pos)?;
        }
        if self.rw.ast.node(node).has_prop("permitsTypes") {
            if self.change_kind(node, "permitsTypes") != change::UNCHANGED {
                pos = self.rewrite_node_list(node, "permitsTypes", pos, " permits ", ", ")?;
            } else {
                pos = self.do_visit_prop(node, "permitsTypes", pos)?;
            }
        }
        let start_indent = self.get_indent(self.start(node)) + 1;
        let start_pos = self.pos_after_left_brace(pos)?;
        self.rewrite_paragraph_list(node, "bodyDeclarations", start_pos, start_indent, -1, 2)?;
        Ok(())
    }

    fn rewrite_return_type(&mut self, node: NodeId, is_constructor: bool, is_constructor_change: bool) -> R<()> {
        let original = self.original_value(node, "returnType2").node();
        let return_type_exists = original.is_some_and(|r| matches!(r, RNode::Orig(_)));
        if !is_constructor_change && return_type_exists {
            self.rewrite_required_node(node, "returnType2")?;
            self.ensure_space_after_replace(node, "returnType2")?;
        } else {
            let new_return_type = self.new_value(node, "returnType2").node();
            if is_constructor_change || !return_type_exists && new_return_type != original {
                let name = orig_id(self.original_value(node, "name").node().unwrap());
                let next_start = self.start(name);
                if !is_constructor && return_type_exists {
                    let rt = orig_id(original.unwrap());
                    let offset = self.extended_offset(rt);
                    self.do_text_remove_and_visit(offset, next_start - offset, rt)?;
                } else if let Some(nrt) = new_return_type {
                    let indent = self.get_indent(next_start);
                    self.do_text_insert_node(next_start, nrt, indent, true, 0)?;
                    self.do_text_insert(next_start, " ")?;
                }
            }
        }
        Ok(())
    }

    fn rewrite_method_receiver(&mut self, method: NodeId, offset: i32) -> R<i32> {
        let mut offset = self.scanner.token_end_offset(LPAREN, offset)?;
        let new_param_count = self.new_value(method, "parameters").list().len();
        let old_param_count = self.rw.ast.node(method).list("parameters").len();
        let ev = self.event(method, "receiverType");
        let qual_ev = self.event(method, "receiverQualifier");
        let (new_qual, old_qual) = match qual_ev {
            Some(q) => (q.new_value().node(), q.original_value().node()),
            None => (None, None),
        };
        let mut rewrite_qualifier = false;
        if let Some(ev) = ev.filter(|e| e.change_kind() != change::UNCHANGED) {
            let ck = ev.change_kind();
            if ck == change::INSERTED {
                self.do_text_insert_node(offset, ev.new_value().node().unwrap(), 0, false, 0)?;
                self.do_text_insert(offset, " ")?;
                if let Some(q) = new_qual {
                    self.do_text_insert_node(offset, q, 0, false, 0)?;
                    self.do_text_insert(offset, ".")?;
                }
                self.do_text_insert(offset, "this")?;
                if new_param_count > 0 {
                    self.do_text_insert(offset, ", ")?;
                }
            } else {
                let elem = orig_id(ev.original_value().node().unwrap());
                let (elem_offset, elem_len) = self.extended_range(elem);
                let elem_end = elem_offset + elem_len;
                if ck == change::REMOVED {
                    let end_pos = if old_param_count == 0 {
                        self.scanner.token_start_offset(RPAREN, elem_end)?
                    } else {
                        self.scanner.token_end_offset(COMMA, elem_end)?
                    };
                    self.do_text_remove_and_visit(offset, end_pos - offset, elem)?;
                    return Ok(end_pos);
                }
                if ck == change::REPLACED {
                    self.do_text_remove_and_visit(elem_offset, elem_len, elem)?;
                    self.do_text_insert_node(elem_offset, ev.new_value().node().unwrap(), 0, false, 0)?;
                    rewrite_qualifier = true;
                }
            }
        } else {
            self.rewrite_required_node(method, "receiverType")?;
            if self.rw.ast.node(method).child("receiverType").is_some() {
                rewrite_qualifier = true;
            }
        }
        if rewrite_qualifier {
            if let Some(q) = qual_ev {
                match q.change_kind() {
                    change::INSERTED => {
                        let pos = self.scanner.token_start_offset(Tok::Kw("this"), offset)?;
                        self.do_text_insert_node(pos, new_qual.unwrap(), 0, false, 0)?;
                        self.do_text_insert(pos, ".")?;
                    }
                    change::REMOVED => {
                        let oq = orig_id(old_qual.unwrap());
                        let qual_offset = self.start(oq);
                        let end_pos = self.scanner.token_end_offset(DOT, qual_offset)?;
                        self.do_text_remove(qual_offset, end_pos - qual_offset)?;
                    }
                    change::REPLACED => {
                        let oq = orig_id(old_qual.unwrap());
                        let (eo, el) = self.extended_range(oq);
                        self.do_text_remove_and_visit(eo, el, oq)?;
                        self.do_text_insert_node(eo, new_qual.unwrap(), 0, false, 0)?;
                    }
                    _ => {}
                }
            }
            offset = self.scanner.token_end_offset(Tok::Kw("this"), offset)?;
            if new_param_count > 0 && old_param_count == 0 {
                self.do_text_insert(offset, ", ")?;
            }
        }
        Ok(offset)
    }

    fn visit_method_declaration(&mut self, node: NodeId) -> R<()> {
        let pos = self.rewrite_javadoc(node, "javadoc")?;
        let pos = self.rewrite_modifiers2(node, "modifiers", pos)?;
        let started = pos != self.start(node);
        self.rewrite_optional_type_parameters(node, "typeParameters", pos, " ", true, started)?;
        let is_constructor_change = self.is_changed(node, "constructor");
        let is_constructor = self.original_value(node, "constructor").flag();
        if !is_constructor || is_constructor_change {
            self.rewrite_return_type(node, is_constructor, is_constructor_change)?;
        }
        let pos = self.rewrite_required_node(node, "name")?;
        // `catch (CoreException var11) {}`: scanner failures end the method silently.
        let rest = (|| -> R<()> {
            let pos = self.rewrite_method_receiver(node, pos)?;
            let pos = self.rewrite_node_list(node, "parameters", pos, "", ", ")?;
            let pos = self.scanner.token_end_offset(RPAREN, pos)?;
            let pos = self.rewrite_extra_dimensions_info(node, pos, "extraDimensions2")?;
            let pos = self.rewrite_node_list(node, "thrownExceptionTypes", pos, " throws ", ", ")?;
            self.rewrite_method_body(node, pos)
        })();
        match rest {
            Err(e) if e.0.starts_with("Document does not match the AST") => Ok(()),
            other => other,
        }
    }

    fn visit_array_creation(&mut self, node: NodeId) -> R<()> {
        let array_type = orig_id(self.original_value(node, "type").node().unwrap());
        let mut replacing_type = RNode::Orig(array_type);
        let n_old_brackets = self.rw.ast.node(array_type).list("dimensions").len();
        let mut type_replaced = false;
        if let Some(ev) = self.event(node, "type").filter(|e| e.change_kind() == change::REPLACED) {
            type_replaced = true;
            replacing_type = ev.new_value().node().unwrap();
            let new_type = self.rw.new_value(replacing_type, "elementType").node();
            let old_type = self.original_value(array_type, "elementType").node();
            if new_type != old_type {
                let ot = orig_id(old_type.unwrap());
                let (offset, length) = self.extended_range(ot);
                self.do_text_remove(offset, length)?;
                if let Some(nt) = new_type {
                    self.do_text_insert_node(offset, nt, 0, false, 0)?;
                }
            }
        }
        let dim_ev = self.event(node, "dimensions");
        let has_dim_changes = dim_ev.is_some_and(|e| e.change_kind() != change::UNCHANGED);
        let events = if has_dim_changes { dim_ev.unwrap().children() } else { Vec::new() };
        let replacing_dims = self.rw.new_value(replacing_type, "dimensions").list();
        let replacing_type_dimensions = replacing_dims.len();
        let dim_size = events.len();
        let element_type = orig_id(self.rw.ast.node(array_type).child("elementType").map(|e| RNode::Orig(e.id)).unwrap());
        let mut offset = self.end(element_type);
        let mut i = 0usize;
        loop {
            if i < dim_size {
                self.rewrite_annotations_on_dimension(array_type, replacing_type, i, offset, type_replaced)?;
                offset = self.scanner.token_end_offset(LBRACKET, offset)?;
                let ev = &events[i];
                let ck = ev.change_kind();
                if ck == change::INSERTED {
                    let end_pos = self.scanner.token_start_offset(RBRACKET, offset)?;
                    self.do_text_remove(offset, end_pos - offset)?;
                    self.do_text_insert_node(offset, ev.new.unwrap(), 0, false, 0)?;
                } else {
                    let elem = orig_id(ev.original.unwrap());
                    let end_pos = self.scanner.token_start_offset(RBRACKET, self.end(elem))?;
                    if ck == change::REMOVED {
                        self.do_text_remove_and_visit(offset, end_pos - offset, elem)?;
                    } else if ck == change::REPLACED {
                        let (eo, el) = self.extended_range(elem);
                        self.do_text_remove_and_visit(eo, el, elem)?;
                        self.do_text_insert_node(eo, ev.new.unwrap(), 0, false, 0)?;
                    } else {
                        self.accept(elem)?;
                    }
                }
                offset = self.retrieve_right_bracket_end_position(offset, 1, true)?;
            } else if i < n_old_brackets {
                self.rewrite_annotations_on_dimension(array_type, replacing_type, i, offset, type_replaced)?;
                offset = self.retrieve_right_bracket_end_position(offset, 1, false)?;
            } else {
                self.insert_annotations_on_dimension(replacing_type, i, offset)?;
                self.do_text_insert(offset, "[]")?;
            }
            i += 1;
            if i >= replacing_type_dimensions {
                break;
            }
        }
        if i < n_old_brackets {
            let end_pos = self.retrieve_right_bracket_end_position(offset, (n_old_brackets - i) as i32, false)?;
            self.do_text_remove(offset, end_pos - offset)?;
        }
        let kind = self.change_kind(node, "initializer");
        let offset = if kind == change::REMOVED {
            self.scanner.previous_token_end_offset(LBRACE, offset)?
        } else {
            self.end(node)
        };
        self.rewrite_node(node, "initializer", offset, fmt::SPACE)?;
        Ok(())
    }

    fn insert_annotations_on_dimension(&mut self, replacing_type: RNode, index: usize, pos: i32) -> R<()> {
        let dims = self.rw.new_value(replacing_type, "dimensions").list();
        if let Some(&dim) = dims.get(index) {
            let annotations = self.rw.new_value(dim, "annotations").list();
            if !annotations.is_empty() {
                self.do_text_insert(pos, " ")?;
                for a in annotations {
                    let s = super::flattener::Flattener::as_string(self.rw, a) + " ";
                    self.do_text_insert(pos, &s)?;
                }
            }
        }
        Ok(())
    }

    fn rewrite_annotations_on_dimension(&mut self, old_array_type: NodeId, replacing_type: RNode, index: usize, pos: i32, type_replaced: bool) -> R<()> {
        if type_replaced {
            let dims = self.rw.ast.node(old_array_type).list("dimensions");
            if let Some(old_dim) = dims.get(index) {
                let old_ann = old_dim.list("annotations");
                if !old_ann.is_empty() {
                    let prev = self.previous_dimension_node(old_dim.id);
                    let start = prev.map_or(old_ann[0].start() as i32, |p| self.end(p));
                    let mut end = old_ann[old_ann.len() - 1].end() as i32;
                    end = self.scanner.token_end_offset(LBRACKET, end)? - 1;
                    self.do_text_remove(start, end - start)?;
                }
            }
            self.insert_annotations_on_dimension(replacing_type, index, pos)
        } else {
            let dims = self.rw.new_value(replacing_type, "dimensions").list();
            if let Some(RNode::Orig(dim)) = dims.get(index).copied() {
                self.rewrite_node_list_end(dim, "annotations", pos, " ", " ", " ")?;
            }
            Ok(())
        }
    }

    fn retrieve_right_bracket_end_position(&mut self, offset: i32, mut count: i32, is_left_read: bool) -> R<i32> {
        let mut balance = if is_left_read { 1 } else { 0 };
        self.scanner.set_offset(offset);
        loop {
            let tok = self.scanner.read_next_or_eof(true)?;
            match tok {
                Tok::Eof => return Ok(-1),
                t if t == LBRACKET => balance += 1,
                t if t == RBRACKET => {
                    balance -= 1;
                    if balance == 0 {
                        count -= 1;
                        if count == 0 {
                            return Ok(self.scanner.current_end_offset());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_class_instance_creation(&mut self, node: NodeId) -> R<()> {
        let mut pos = self.rewrite_optional_qualifier(node, "expression", self.start(node))?;
        if self.is_changed(node, "typeArguments") {
            pos = self.scanner.token_end_offset(Tok::Kw("new"), pos)?;
            self.rewrite_optional_type_parameters(node, "typeArguments", pos, " ", true, true)?;
        } else {
            self.void_visit_prop(node, "typeArguments")?;
        }
        pos = self.rewrite_required_node(node, "type")?;
        if self.is_changed(node, "arguments") {
            let start = self.scanner.token_end_offset(LPAREN, pos)?;
            self.rewrite_node_list(node, "arguments", start, "", ", ")?;
        } else {
            self.void_visit_prop(node, "arguments")?;
        }
        let kind = self.change_kind(node, "anonymousClassDeclaration");
        let pos = if kind == change::REMOVED {
            self.scanner.previous_token_end_offset(LBRACE, pos)?
        } else {
            self.end(node)
        };
        self.rewrite_node(node, "anonymousClassDeclaration", pos, fmt::SPACE)?;
        Ok(())
    }

    fn visit_if_statement(&mut self, node: NodeId) -> R<()> {
        let mut pos = self.rewrite_required_node(node, "expression")?;
        let then_ev = self.event(node, "thenStatement");
        let else_change = self.change_kind(node, "elseStatement");
        if let Some(then_ev) = then_ev.filter(|e| e.change_kind() != change::UNCHANGED) {
            let tok = self.scanner.read_next_at(pos, true)?;
            pos = if tok == RPAREN { self.scanner.current_end_offset() } else { self.scanner.current_start_offset() };
            let indent = self.get_indent(self.start(node));
            let mut end_pos = -1;
            let else_statement = self.original_value(node, "elseStatement").node();
            if else_statement.is_some() {
                let then_statement = orig_id(then_ev.original_value().node().unwrap());
                end_pos = self.scanner.token_start_offset(Tok::Kw("else"), self.end(then_statement))?;
            }
            if else_statement.is_some() && else_change == change::UNCHANGED {
                pos = self.rewrite_body_node(node, "thenStatement", pos, end_pos, indent, fmt::IF_BLOCK_WITH_ELSE)?;
            } else {
                pos = self.rewrite_body_node(node, "thenStatement", pos, end_pos, indent, fmt::IF_BLOCK_NO_ELSE)?;
            }
        } else {
            pos = self.do_visit_prop(node, "thenStatement", pos)?;
        }
        if else_change != change::UNCHANGED {
            let indent = self.get_indent(self.start(node));
            let new_then = self.new_value(node, "thenStatement").node();
            if new_then.is_some_and(|t| self.kind(t) == NodeKind::Block) {
                self.rewrite_body_node(node, "elseStatement", pos, -1, indent, fmt::ELSE_AFTER_BLOCK)?;
            } else {
                self.rewrite_body_node(node, "elseStatement", pos, -1, indent, fmt::ELSE_AFTER_STATEMENT)?;
            }
        } else {
            self.do_visit_prop(node, "elseStatement", pos)?;
        }
        Ok(())
    }

    fn visit_infix_expression(&mut self, node: NodeId) -> R<()> {
        let right = orig_id(self.rw.ast.node(node).child("rightOperand").map(|r| RNode::Orig(r.id)).unwrap());
        let left_ev = self.event(node, "leftOperand");
        let remove_left = left_ev.is_some_and(|e| e.change_kind() == change::REMOVED);
        let right_ev = self.event(node, "rightOperand");
        let remove_right = right_ev.is_some_and(|e| e.change_kind() == change::REMOVED);
        let mut pos;
        if remove_left {
            let left = self.rw.ast.node(node).child("leftOperand").unwrap().id;
            let left_start = self.extended_offset(left);
            pos = self.extended_offset(right);
            self.do_text_remove_and_visit(left_start, pos - left_start, left)?;
        } else {
            pos = self.rewrite_required_node(node, "leftOperand")?;
        }
        let needs_new_operation = self.is_changed(node, "operator");
        let operation = self.new_value(node, "operator").simple().unwrap_or("+").to_owned();
        if needs_new_operation && !remove_left && !remove_right {
            self.replace_operation(pos, &operation)?;
        }
        if remove_right {
            let ext = self.rw.ast.node(node).list("extendedOperands");
            let end = if remove_left && !ext.is_empty() { self.extended_offset(ext[0].id) } else { self.extended_end(right) };
            self.do_text_remove_and_visit(pos, end - pos, right)?;
            pos = end;
        } else {
            pos = self.rewrite_required_node(node, "rightOperand")?;
        }
        let ev = self.event(node, "extendedOperands");
        let prefix_string = format!(" {operation} ");
        if needs_new_operation {
            let mut start_pos = pos;
            if let Some(ev) = ev.filter(|e| e.change_kind() != change::UNCHANGED) {
                for curr in ev.children() {
                    if let Some(elem) = curr.original {
                        if curr.change_kind() != change::REPLACED {
                            self.replace_operation(start_pos, &operation)?;
                        }
                        start_pos = self.end(orig_id(elem));
                    }
                }
            } else {
                for elem in self.original_value(node, "extendedOperands").list() {
                    self.replace_operation(start_pos, &operation)?;
                    start_pos = self.end(orig_id(elem));
                }
            }
        }
        self.rewrite_node_list(node, "extendedOperands", pos, &prefix_string, &prefix_string)?;
        Ok(())
    }

    fn visit_lambda(&mut self, node: NodeId) -> R<()> {
        let new_value = self.new_value(node, "parentheses").flag();
        let mut has_parentheses = new_value;
        if !has_parentheses {
            let params = self.new_value(node, "parameters").list();
            has_parentheses = params.len() != 1 || self.kind(params[0]) != NodeKind::VariableDeclarationFragment;
        }
        let mut delete_parentheses = false;
        let mut insert_parentheses = false;
        let old_has_parentheses = self.original_value(node, "parentheses").flag();
        if let Some(ev) = self.event(node, "parentheses") {
            if ev.change_kind() == change::REPLACED {
                if new_value {
                    insert_parentheses = true;
                } else {
                    delete_parentheses = !has_parentheses;
                }
            }
        } else if !old_has_parentheses && has_parentheses && self.event(node, "parameters").is_some() {
            insert_parentheses = true;
        }
        let mut pos = self.start(node);
        if insert_parentheses {
            self.do_text_insert(pos, "(")?;
        } else if delete_parentheses {
            let lparen_end = self.scanner.token_end_offset(LPAREN, pos)?;
            self.do_text_remove(pos, lparen_end - pos)?;
            pos = lparen_end;
        }
        if self.is_changed(node, "parameters") {
            pos = if old_has_parentheses { self.scanner.token_end_offset(LPAREN, pos)? } else { pos };
            pos = self.rewrite_node_list(node, "parameters", pos, "", ", ")?;
        } else {
            pos = self.do_visit_prop(node, "parameters", pos)?;
        }
        if insert_parentheses {
            self.do_text_insert(pos, ")")?;
        } else if delete_parentheses {
            let end = self.scanner.token_end_offset(RPAREN, pos)?;
            self.do_text_remove(pos, end - pos)?;
        }
        self.rewrite_required_node(node, "body")?;
        Ok(())
    }

    fn visit_single_variable_declaration(&mut self, node: NodeId) -> R<()> {
        let pos = self.start(node);
        self.rewrite_modifiers2(node, "modifiers", pos)?;
        let mut pos = self.rewrite_required_node(node, "type")?;
        let is_varargs = self.rw.ast.node(node).flag("varargs");
        if self.is_changed(node, "varargs") {
            if self.new_value(node, "varargs").flag() {
                pos = self.rewrite_modifiers2(node, "varargsAnnotations", pos)?;
                let indent = self.get_indent(self.start(node));
                let prefix = self.formatter.prefix(fmt::VARARGS, indent);
                self.do_text_insert(pos, &prefix)?;
                self.do_text_insert(pos, "...")?;
            } else {
                let annotations = self.rw.ast.node(node).list("varargsAnnotations");
                let ellipsis_end = match annotations.last() {
                    Some(a) => self.scanner.next_end_offset(a.end() as i32, true)?,
                    None => self.scanner.next_end_offset(pos, true)?,
                };
                self.do_text_remove(pos, ellipsis_end - pos)?;
            }
        } else if is_varargs {
            self.rewrite_modifiers2(node, "varargsAnnotations", pos)?;
        }
        if !is_varargs {
            self.ensure_space_after_replace(node, "type")?;
        }
        let pos = self.rewrite_required_node(node, "name")?;
        let pos = self.rewrite_extra_dimensions_info(node, pos, "extraDimensions2")?;
        self.rewrite_node(node, "initializer", pos, fmt::VAR_INITIALIZER)?;
        Ok(())
    }

    fn visit_enum_constant(&mut self, node: NodeId) -> R<()> {
        let pos = self.rewrite_javadoc(node, "javadoc")?;
        self.rewrite_modifiers2(node, "modifiers", pos)?;
        let mut pos = self.rewrite_required_node(node, "name")?;
        if let Some(args_ev) = self.event(node, "arguments").filter(|e| e.change_kind() != change::UNCHANGED) {
            let children = args_ev.children();
            let next_tok = self.scanner.read_next_at(pos, true)?;
            let has_parents = next_tok == LPAREN;
            let is_all_removed = has_parents && is_all_of_kind(&children, change::REMOVED);
            let mut prefix = "";
            if !has_parents {
                prefix = "(";
            } else if !is_all_removed {
                pos = self.scanner.current_end_offset();
            }
            pos = self.rewrite_node_list(node, "arguments", pos, prefix, ", ")?;
            if !has_parents {
                self.do_text_insert(pos, ")")?;
            } else if is_all_removed {
                let after = self.scanner.next_end_offset(pos, true)?;
                self.do_text_remove(pos, after - pos)?;
                pos = after;
            }
        } else {
            pos = self.do_visit_prop(node, "arguments", pos)?;
        }
        let kind = self.change_kind(node, "anonymousClassDeclaration");
        let pos = if kind == change::REMOVED { self.scanner.previous_token_end_offset(LBRACE, pos)? } else { self.end(node) };
        self.rewrite_node(node, "anonymousClassDeclaration", pos, fmt::SPACE)?;
        Ok(())
    }

    fn visit_enum_declaration(&mut self, node: NodeId) -> R<()> {
        let pos = self.rewrite_javadoc(node, "javadoc")?;
        self.rewrite_modifiers2(node, "modifiers", pos)?;
        let pos = self.rewrite_required_node(node, "name")?;
        let pos = self.rewrite_node_list(node, "superInterfaceTypes", pos, " implements ", ", ")?;
        let mut pos = self.pos_after_left_brace(pos)?;
        let mut lead_string = String::new();
        if let Some(ev) = self.event(node, "enumConstants").filter(|e| e.change_kind() != change::UNCHANGED) {
            if is_all_of_kind(&ev.children(), change::INSERTED) {
                let indent = self.get_indent(self.start(node));
                lead_string = self.formatter.prefix(fmt::FIRST_ENUM_CONST, indent);
            }
        }
        pos = self.rewrite_node_list(node, "enumConstants", pos, &lead_string, ", ")?;
        let mut indent = 0;
        if let Some(body_ev) = self.event(node, "bodyDeclarations").filter(|e| e.change_kind() != change::UNCHANGED) {
            let has_constants = !self.new_value(node, "enumConstants").list().is_empty();
            let children = body_ev.children();
            indent = if has_constants { self.get_indent(pos) } else { self.get_indent(self.start(node)) + 1 };
            let token = self.scanner.read_next_at(pos, true)?;
            let has_semicolon = token == SEMICOLON;
            if !has_semicolon && is_all_of_kind(&children, change::INSERTED) {
                if !has_constants {
                    let s = self.formatter.prefix(fmt::FIRST_ENUM_CONST, indent - 1);
                    self.do_text_insert(pos, &s)?;
                }
                if token == COMMA {
                    let mut end_pos = self.scanner.current_end_offset();
                    let next = self.scanner.read_next_at(end_pos, true)?;
                    if next != SEMICOLON {
                        self.do_text_insert(end_pos, ";")?;
                    } else {
                        end_pos = self.scanner.current_end_offset();
                        if is_all_of_kind(&children, change::REMOVED) {
                            self.do_text_remove(pos, end_pos - pos)?;
                        }
                    }
                    pos = end_pos;
                } else {
                    self.do_text_insert(pos, ";")?;
                }
            } else if has_semicolon {
                let end_pos = self.scanner.current_end_offset();
                if is_all_of_kind(&children, change::REMOVED) {
                    self.do_text_remove(pos, end_pos - pos)?;
                }
                pos = end_pos;
            }
        }
        self.rewrite_paragraph_list(node, "bodyDeclarations", pos, indent, -1, 2)?;
        Ok(())
    }

    // ── ListRewriter ────────────────────────────────────────────────────────

    fn lr_separator(&mut self, lr: &ListRewriter, node_index: usize) -> String {
        match lr.kind {
            ListKind::Plain | ListKind::Resources => lr.constant_separator.clone(),
            ListKind::Modifier { annotation_separation } => {
                if lr.new_node(node_index).is_some_and(|n| self.kind(n).is_annotation()) {
                    let indent = self.lr_node_indent(lr, node_index + 1);
                    self.formatter.prefix(annotation_separation, indent)
                } else {
                    lr.constant_separator.clone()
                }
            }
            ListKind::Paragraph { .. } => self.lr_paragraph_separator(lr, node_index, node_index + 1),
            ListKind::Switch { .. } => {
                let total = lr.list.len();
                let mut next = node_index + 1;
                while next < total && lr.list[next].change_kind() == change::REMOVED {
                    next += 1;
                }
                if next == total {
                    self.lr_paragraph_separator(lr, node_index, node_index + 1)
                } else {
                    self.lr_paragraph_separator(lr, node_index, next)
                }
            }
        }
    }

    fn lr_paragraph_separator(&mut self, lr: &ListRewriter, node_index: usize, next_index: usize) -> String {
        if let ListKind::Switch { labeled_rule: true, .. } = lr.kind {
            let curr = lr.node(node_index);
            let next = lr.node(node_index + 1);
            let is_rule = curr.is_some_and(|c| self.kind(c) == NodeKind::SwitchCase && self.rw.new_value(c, "switchLabeledRule").flag())
                && next.is_some_and(|n| self.kind(n).is_statement());
            let space = if self.options.get("org.eclipse.jdt.core.formatter.insert_space_after_arrow_in_switch_case").map(String::as_str) == Some("insert") { " " } else { "" };
            let delim = if is_rule { space.to_owned() } else { self.line_delimiter() };
            let indent = self.lr_node_indent(lr, next_index);
            return delim + &self.create_indent_string(indent);
        }
        let separator_lines = match lr.kind {
            ListKind::Paragraph { separator_lines, .. } => separator_lines,
            _ => 0,
        };
        let new_lines = if separator_lines == -1 { self.lr_new_lines(lr, node_index) } else { separator_lines };
        let delim = self.line_delimiter();
        let mut buf = delim.clone();
        for _ in 0..new_lines {
            buf.push_str(&delim);
        }
        let indent = self.lr_node_indent(lr, next_index);
        let spaces = self.lr_node_indent_in_spaces(lr, next_index);
        buf.push_str(&self.create_indent_string_min(indent, spaces));
        buf
    }

    fn lr_new_lines(&self, lr: &ListRewriter, node_index: usize) -> i32 {
        let (Some(curr), Some(next)) = (lr.node(node_index), lr.node(node_index + 1)) else { return 1 };
        let curr_kind = self.kind(curr);
        let next_kind = self.kind(next);
        let mut last: Option<NodeId> = None;
        let mut second_last: Option<NodeId> = None;
        for ev in &lr.list {
            if let Some(RNode::Orig(elem)) = ev.original {
                if let Some(l) = last {
                    if self.kind(o(elem)) == next_kind && self.kind(o(l)) == curr_kind {
                        return self.count_empty_lines(l);
                    }
                    second_last = Some(l);
                }
                last = Some(elem);
            }
        }
        if curr_kind == NodeKind::FieldDeclaration && next_kind == NodeKind::FieldDeclaration {
            0
        } else if let Some(sl) = second_last {
            self.count_empty_lines(sl)
        } else {
            1
        }
    }

    fn count_empty_lines(&self, last: NodeId) -> i32 {
        let last_line = self.line_of_offset(self.extended_end(last));
        if last_line >= 0 {
            let start_line = last_line + 1;
            let start = self.line_offset(start_line);
            if start < 0 {
                return 0;
            }
            let mut i = start as usize;
            while i < self.content.len() && indent::is_whitespace(self.content[i]) {
                i += 1;
            }
            if i > start as usize {
                let ll = self.line_of_offset(i as i32);
                if ll > start_line {
                    return ll - start_line;
                }
            }
        }
        0
    }

    fn lr_initial_indent(&self, lr: &ListRewriter) -> i32 {
        match lr.kind {
            ListKind::Paragraph { initial_indent, .. } | ListKind::Switch { initial_indent, .. } => initial_indent,
            _ => self.get_indent(lr.node_indent_pos),
        }
    }

    fn lr_initial_indent_in_spaces(&self, lr: &ListRewriter) -> i32 {
        self.get_indent_in_spaces(lr.node_indent_pos)
    }

    fn lr_node_indent(&self, lr: &ListRewriter, index: usize) -> i32 {
        if let ListKind::Switch { indent_compare, labeled_rule, .. } = lr.kind {
            let mut indent = self.lr_initial_indent(lr);
            if indent_compare {
                let ev = &lr.list[index.min(lr.list.len() - 1)];
                let ck = ev.change_kind();
                let node = if ck != change::INSERTED && ck != change::REPLACED { ev.original } else { ev.new };
                if node.is_some_and(|n| self.kind(n) != NodeKind::SwitchCase) {
                    if labeled_rule && index > 0 {
                        let prev = lr.node(index - 1);
                        if prev.is_some_and(|p| self.kind(p) == NodeKind::SwitchCase && self.rw.new_value(p, "switchLabeledRule").flag()) {
                            return 0;
                        }
                    }
                    indent += 1;
                }
            }
            return indent;
        }
        if let Some(RNode::Orig(n)) = lr.original(index.min(lr.list.len().saturating_sub(1))).filter(|_| index < lr.list.len()) {
            return self.get_indent(self.start(n));
        }
        for i in (0..index.min(lr.list.len())).rev() {
            if let Some(RNode::Orig(c)) = lr.original(i) {
                return self.get_indent(self.start(c));
            }
        }
        self.lr_initial_indent(lr)
    }

    fn lr_node_indent_in_spaces(&self, lr: &ListRewriter, index: usize) -> i32 {
        if let ListKind::Switch { indent_compare, .. } = lr.kind {
            let mut indent = self.lr_initial_indent_in_spaces(lr);
            if indent_compare {
                let ev = &lr.list[index.min(lr.list.len() - 1)];
                let ck = ev.change_kind();
                let node = if ck != change::INSERTED && ck != change::REPLACED { ev.original } else { ev.new };
                if node.is_some_and(|n| self.kind(n) != NodeKind::SwitchCase) {
                    for e in &lr.list {
                        let k = e.change_kind();
                        if k == change::UNCHANGED || k == change::REPLACED {
                            if let Some(RNode::Orig(n)) = e.original {
                                if self.kind(o(n)) != NodeKind::SwitchCase {
                                    return self.get_indent_in_spaces(self.start(n));
                                }
                            }
                        }
                    }
                    indent += self.formatter.indent_width;
                }
            }
            return indent;
        }
        if index < lr.list.len() {
            if let Some(RNode::Orig(n)) = lr.original(index) {
                return self.get_indent_in_spaces(self.start(n));
            }
        }
        for i in (0..index.min(lr.list.len())).rev() {
            if let Some(RNode::Orig(c)) = lr.original(i) {
                return self.get_indent_in_spaces(self.start(c));
            }
        }
        self.lr_initial_indent_in_spaces(lr)
    }

    fn lr_start_of_next_node(&self, lr: &ListRewriter, next_index: usize, default_pos: i32) -> i32 {
        for i in next_index..lr.list.len() {
            let elem = &lr.list[i];
            if elem.change_kind() != change::INSERTED {
                if let Some(RNode::Orig(n)) = elem.original {
                    return self.extended_offset(n);
                }
            }
        }
        default_pos
    }

    /// The comment-skipping loop of `rewriteList` (removed / replaced
    /// entries); `None` when the scanner failed (`catch (CoreException)`).
    fn skip_comments_before(&mut self, from: i32, extended_offset: i32) -> Option<i32> {
        let mut new_offset = from;
        loop {
            let t = self.scanner.read_next_at(new_offset, false).ok()?;
            if !t.is_comment() {
                return Some(new_offset);
            }
            let temp = self.scanner.next_end_offset(new_offset, false).ok()?;
            if temp >= extended_offset {
                return Some(new_offset);
            }
            new_offset = temp;
        }
    }

    fn lr_end_of_node(&self, n: NodeId) -> i32 {
        self.extended_end(n)
    }

    fn lr_line_comment_swallows_actual_code(&mut self, lr: &ListRewriter, prev_end: i32) -> bool {
        if self.is_end_of_line_comment(prev_end) {
            if let Some(RNode::Orig(last)) = lr.list[lr.list.len() - 1].original {
                let last_end = self.lr_end_of_node(last);
                if let Ok(next) = self.scanner.next_start_offset(last_end, false) {
                    return self.line_of_offset(last_end) == self.line_of_offset(next);
                }
            }
        }
        false
    }

    fn lr_must_remove_separator(&self, lr: &ListRewriter, original_offset: i32, node_index: usize) -> bool {
        if !matches!(lr.kind, ListKind::Paragraph { .. } | ListKind::Switch { .. }) {
            return true;
        }
        let mut prev = node_index as i32 - 1;
        while prev >= 0 && lr.list[prev as usize].change_kind() == change::REMOVED {
            prev -= 1;
        }
        if prev > -1 {
            let prev_ev = &lr.list[prev as usize];
            let prev_kind = prev_ev.change_kind();
            if prev_kind == change::UNCHANGED || prev_kind == change::REPLACED {
                let prev_node = orig_id(prev_ev.original.unwrap());
                let prev_line = self.line_of_offset(self.end(prev_node));
                let line = self.line_of_offset(original_offset);
                if prev_line == line && node_index + 1 < lr.list.len() {
                    let next_ev = &lr.list[node_index + 1];
                    let next_kind = next_ev.change_kind();
                    if next_kind != change::UNCHANGED && prev_kind != change::REPLACED {
                        return false;
                    }
                    if let Some(RNode::Orig(next)) = next_ev.original {
                        return self.line_of_offset(self.start(next)) == line;
                    }
                    return false;
                }
            }
        }
        true
    }

    fn lr_update_indent(&mut self, lr: &ListRewriter, prev_mark: i32, original_offset: i32, mut node_index: usize) -> R<()> {
        if !matches!(lr.kind, ListKind::Switch { .. }) {
            return Ok(());
        }
        if prev_mark != change::UNCHANGED && prev_mark != change::REPLACED {
            return Ok(());
        }
        let mut prev = node_index as i32 - 1;
        while prev >= 0 && lr.list[prev as usize].change_kind() == change::REMOVED {
            prev -= 1;
        }
        if prev > -1 {
            let prev_ev = &lr.list[prev as usize];
            let pk = prev_ev.change_kind();
            if pk == change::UNCHANGED || pk == change::REPLACED {
                let prev_node = orig_id(prev_ev.original.unwrap());
                if self.line_of_offset(self.end(prev_node)) == self.line_of_offset(original_offset) {
                    return Ok(());
                }
            }
        }
        while node_index < lr.list.len() && lr.list[node_index].change_kind() == change::REMOVED {
            node_index += 1;
        }
        let original_indent = self.get_indent(original_offset);
        let new_indent = self.lr_node_indent(lr, node_index);
        if original_indent != new_indent {
            let line = self.line_of_offset(original_offset);
            if line >= 0 {
                let line_start = self.line_offset(line);
                self.do_text_remove(line_start, original_offset - line_start)?;
                let s = self.create_indent_string(new_indent);
                self.do_text_insert(line_start, &s)?;
            }
        }
        Ok(())
    }

    /// `ListRewriter.rewriteList(parent, property, keyword, endKeyword, offset)`.
    fn rewrite_list(&mut self, lr: &mut ListRewriter, parent: NodeId, prop: &str, keyword: &str, end_keyword: Option<&str>, offset: i32) -> R<i32> {
        lr.start_pos = offset;
        lr.node_indent_pos = offset;
        lr.list = self.event(parent, prop).map(Event::children).unwrap_or_default();
        let pnode = self.rw.ast.node(parent);
        if pnode.location_is("body") && pnode.parent().is_some_and(|p| p.kind() == NodeKind::TryStatement) {
            let try_parent = pnode.parent().unwrap();
            let resources = try_parent.list("resources");
            if let Some(last) = resources.last() {
                if self.line_of_offset(last.start() as i32) == self.line_of_offset(self.start(parent)) {
                    lr.node_indent_pos = try_parent.start() as i32;
                }
            }
        }
        let parent_kind = self.kind(o(parent));
        let maintain_minimum_indent = prop == "statements" && matches!(parent_kind, NodeKind::Block | NodeKind::SwitchStatement | NodeKind::SwitchExpression)
            || prop == "bodyDeclarations" && parent_kind == NodeKind::TypeDeclaration;
        let total = lr.list.len();
        if total == 0 {
            return Ok(lr.start_pos);
        }
        let mut curr_pos = -1;
        let mut last_non_insert: i32 = -1;
        let mut last_non_delete: i32 = -1;
        for i in 0..total {
            let mark = lr.list[i].change_kind();
            if mark != change::INSERTED {
                last_non_insert = i as i32;
                if curr_pos == -1 {
                    let elem = orig_id(lr.list[i].original.unwrap());
                    curr_pos = self.extended_offset(elem);
                }
            }
            if mark != change::REMOVED {
                last_non_delete = i as i32;
            }
        }
        let insert_new = curr_pos == -1;
        if insert_new {
            if !keyword.is_empty() {
                self.do_text_insert(offset, keyword)?;
            }
            curr_pos = offset;
        }
        if last_non_delete == -1 {
            curr_pos = offset;
        }
        let mut prev_end = curr_pos;
        let mut prev_mark = change::UNCHANGED;
        const NONE: i32 = 0;
        const NEW: i32 = 1;
        const EXISTING: i32 = 2;
        let mut separator_state = NEW;
        for i in 0..total {
            let curr_ev = lr.list[i].clone();
            let curr_mark = curr_ev.change_kind();
            let next_index = i + 1;
            if curr_mark == change::INSERTED {
                let node = curr_ev.new.unwrap();
                if separator_state == NONE {
                    let sep = self.lr_separator(lr, i - 1);
                    self.do_text_insert(curr_pos, &sep)?;
                    separator_state = NEW;
                }
                if separator_state != NEW && self.rw.is_insert_bound_to_previous(node) {
                    let sep = self.lr_separator(lr, i - 1);
                    self.do_text_insert(prev_end, &sep)?;
                    let indent = self.lr_node_indent(lr, i);
                    if maintain_minimum_indent {
                        let spaces = self.lr_node_indent_in_spaces(lr, i);
                        self.do_text_insert_node(prev_end, node, indent, true, spaces)?;
                    } else {
                        self.do_text_insert_node(prev_end, node, indent, true, 0)?;
                    }
                } else {
                    if separator_state == EXISTING {
                        self.lr_update_indent(lr, prev_mark, curr_pos, i)?;
                    }
                    let indent = self.lr_node_indent(lr, i);
                    if maintain_minimum_indent && separator_state != EXISTING {
                        let spaces = self.lr_node_indent_in_spaces(lr, i);
                        self.do_text_insert_node(curr_pos, node, indent, true, spaces)?;
                    } else {
                        self.do_text_insert_node(curr_pos, node, indent, true, 0)?;
                    }
                    separator_state = NEW;
                    if i as i32 != last_non_delete {
                        if lr.list[next_index].change_kind() != change::INSERTED {
                            let sep = self.lr_separator(lr, i);
                            self.do_text_insert(curr_pos, &sep)?;
                        } else {
                            separator_state = NONE;
                        }
                    }
                }
                if insert_new && i as i32 == last_non_delete {
                    if let Some(ek) = end_keyword.filter(|k| !k.is_empty()) {
                        self.do_text_insert(curr_pos, ek)?;
                    }
                }
            } else if curr_mark == change::REMOVED {
                let node = orig_id(curr_ev.original.unwrap());
                let curr_end = self.lr_end_of_node(node);
                let extended_offset = self.extended_offset(node);
                if let Some(new_offset) = self.skip_comments_before(prev_end, extended_offset) {
                    if curr_pos < new_offset {
                        curr_pos = extended_offset;
                    }
                    prev_end = new_offset;
                }
                if i as i32 > last_non_delete && separator_state == EXISTING {
                    self.do_text_remove(prev_end, curr_pos - prev_end)?;
                    self.do_text_remove_and_visit(curr_pos, curr_end - curr_pos, node)?;
                    if self.lr_line_comment_swallows_actual_code(lr, prev_end) {
                        let d = self.line_delimiter();
                        self.do_text_insert(curr_end, &d)?;
                    }
                    curr_pos = curr_end;
                    prev_end = curr_end;
                } else {
                    if (i as i32) < last_non_delete {
                        self.lr_update_indent(lr, prev_mark, curr_pos, i)?;
                    }
                    let mut end = self.lr_start_of_next_node(lr, next_index, curr_end);
                    if let Ok(next_tok) = self.scanner.read_next_at(curr_end, false) {
                        if next_tok.is_comment() {
                            if let Ok(ns) = self.scanner.next_start_offset(curr_end, false) {
                                if end != ns {
                                    end = curr_end;
                                }
                            }
                        }
                    }
                    self.do_text_remove_and_visit(curr_pos, curr_end - curr_pos, node)?;
                    if self.lr_must_remove_separator(lr, curr_pos, i) {
                        self.do_text_remove(curr_end, end - curr_end)?;
                    }
                    curr_pos = end;
                    prev_end = curr_end;
                    if matches!(lr.kind, ListKind::Resources) && next_index == total && last_non_delete == -1 {
                        if let Some(_ek) = end_keyword.filter(|k| !k.is_empty()) {
                            if let Ok(temp) = self.scanner.next_end_offset(curr_pos, true) {
                                self.do_text_remove(curr_pos, temp - curr_pos)?;
                                curr_pos = temp;
                            }
                        }
                    }
                    separator_state = NEW;
                }
            } else {
                if curr_mark == change::REPLACED {
                    let node = orig_id(curr_ev.original.unwrap());
                    let curr_end = self.lr_end_of_node(node);
                    let changed = curr_ev.new.unwrap();
                    self.lr_update_indent(lr, prev_mark, curr_pos, i)?;
                    let extended_offset = self.extended_offset(node);
                    if let Some(new_offset) = self.skip_comments_before(prev_end, extended_offset) {
                        if curr_pos < new_offset {
                            curr_pos = extended_offset;
                        }
                    }
                    self.do_text_remove_and_visit(curr_pos, curr_end - curr_pos, node)?;
                    let indent = self.lr_node_indent(lr, i);
                    if maintain_minimum_indent && separator_state != EXISTING {
                        let spaces = self.lr_node_indent_in_spaces(lr, i);
                        self.do_text_insert_node(curr_pos, changed, indent, true, spaces)?;
                    } else {
                        self.do_text_insert_node(curr_pos, changed, indent, true, 0)?;
                    }
                    prev_end = curr_end;
                } else {
                    let node = orig_id(curr_ev.original.unwrap());
                    self.accept(node)?;
                }
                if i as i32 == last_non_insert {
                    separator_state = NONE;
                    if curr_mark == change::UNCHANGED {
                        let node = orig_id(curr_ev.original.unwrap());
                        prev_end = self.lr_end_of_node(node);
                    }
                    curr_pos = prev_end;
                } else if lr.list[next_index].change_kind() != change::UNCHANGED {
                    if curr_mark == change::UNCHANGED {
                        let node = orig_id(curr_ev.original.unwrap());
                        prev_end = self.lr_end_of_node(node);
                    }
                    curr_pos = self.lr_start_of_next_node(lr, next_index, prev_end);
                    separator_state = EXISTING;
                }
            }
            prev_mark = curr_mark;
        }
        Ok(curr_pos)
    }
}

fn is_all_of_kind(children: &[ListEntry], kind: i32) -> bool {
    children.iter().all(|c| c.change_kind() == kind)
}

fn current_line_start(s: &str, pos: usize) -> usize {
    current_line_start_u16(&indent::to_u16(s), pos)
}

fn current_line_start_u16(v: &[u16], pos: usize) -> usize {
    let mut i = pos.min(v.len());
    while i > 0 {
        if indent::is_line_delimiter_char(v[i - 1]) {
            return i;
        }
        i -= 1;
    }
    0
}

/// `String.split("\n")` (trailing empty strings removed).
fn split_lines(v: &[u16]) -> Vec<String> {
    let s = indent::from_u16(v);
    let mut parts: Vec<String> = s.split('\n').map(str::to_owned).collect();
    while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}
