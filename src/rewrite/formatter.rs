//! Port of `ASTRewriteFormatter`: formats flattened new nodes with the
//! Eclipse code formatter (run in the bridge) and tracks placeholder markers.
//!
//! The analyzer is synchronous; [`FormatterCache`] answers format requests
//! from results fetched beforehand and records the misses, so a rewrite runs
//! the analyzer, fetches the missing results in one `formatBatch` bridge
//! request and runs it again (see [`rewrite_with_bridge`]).

use std::collections::{BTreeMap, HashMap};

use super::flattener::{Flattener, NodeMarker};
use super::indent;
use super::text_edit::{evaluate_formatter_edits, Position};
use super::{ASTRewrite, RNode};
use crate::semantic_ast::NodeKind;

/// `CodeFormatter` kinds.
pub const K_EXPRESSION: i32 = 0x01;
pub const K_STATEMENTS: i32 = 0x02;
pub const K_CLASS_BODY_DECLARATIONS: i32 = 0x04;
pub const K_COMPILATION_UNIT: i32 = 0x08;
pub const K_MODULE_INFO: i32 = 0x80;

/// One `CodeFormatter.format(kind, source, offset, length, indent, lineDelim)` call.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FormatRequest {
    pub kind: i32,
    pub source: String,
    pub offset: usize,
    pub length: usize,
    pub indent: i32,
}

/// Flat formatter edits `(offset, length, text)` (UTF-16 offsets).
pub type FormatEdits = Vec<(usize, usize, String)>;

pub trait CodeFormatter {
    /// `None` when the formatter returned `null`.
    fn format(&mut self, req: &FormatRequest) -> Option<FormatEdits>;
}

/// Results of earlier `formatBatch` requests, recording misses.
#[derive(Default)]
pub struct FormatterCache {
    pub results: HashMap<FormatRequest, Option<FormatEdits>>,
    pub missing: Vec<FormatRequest>,
}

impl CodeFormatter for FormatterCache {
    fn format(&mut self, req: &FormatRequest) -> Option<FormatEdits> {
        match self.results.get(req) {
            Some(r) => r.clone(),
            None => {
                if !self.missing.contains(req) {
                    self.missing.push(req.clone());
                }
                None
            }
        }
    }
}

pub struct RewriteFormatter<'a> {
    pub options: BTreeMap<String, String>,
    pub line_delimiter: String,
    pub tab_width: i32,
    pub indent_width: i32,
    pub formatter: &'a mut dyn CodeFormatter,
}

/// A `Prefix` of `ASTRewriteFormatter`.
#[derive(Clone, Copy, Debug)]
pub enum Prefix {
    Const(&'static str),
    /// `FormattingPrefix(string, sub, kind)`.
    Formatting(&'static str, &'static str, i32),
}

pub const NONE: Prefix = Prefix::Const("");
pub const SPACE: Prefix = Prefix::Const(" ");
pub const ASSERT_COMMENT: Prefix = Prefix::Const(" : ");
pub const VAR_INITIALIZER: Prefix = Prefix::Formatting("A a={};", "a={", K_STATEMENTS);
pub const METHOD_BODY: Prefix = Prefix::Formatting("void a() {}", ") {", K_CLASS_BODY_DECLARATIONS);
pub const FINALLY_BLOCK: Prefix = Prefix::Formatting("try {} finally {}", "} finally {", K_STATEMENTS);
pub const CATCH_BLOCK: Prefix = Prefix::Formatting("try {} catch(Exception e) {}", "} c", K_STATEMENTS);
pub const ANNOT_MEMBER_DEFAULT: Prefix = Prefix::Formatting("String value() default 1;", ") default 1", K_CLASS_BODY_DECLARATIONS);
pub const ENUM_BODY_START: Prefix = Prefix::Formatting("enum E { A(){void foo(){}} }", "){v", K_COMPILATION_UNIT);
pub const ENUM_BODY_END: Prefix = Prefix::Formatting("enum E { A(){void foo(){ }}, B}", "}},", K_COMPILATION_UNIT);
pub const WILDCARD_EXTENDS: Prefix = Prefix::Formatting("A<? extends B> a;", "? extends B", K_CLASS_BODY_DECLARATIONS);
pub const WILDCARD_SUPER: Prefix = Prefix::Formatting("A<? super B> a;", "? super B", K_CLASS_BODY_DECLARATIONS);
pub const FIRST_ENUM_CONST: Prefix = Prefix::Formatting("enum E { X;}", "{ X", K_COMPILATION_UNIT);
pub const ANNOTATION_SEPARATION: Prefix = Prefix::Formatting("@A @B class C {}", "A @", K_COMPILATION_UNIT);
pub const PARAM_ANNOTATION_SEPARATION: Prefix = Prefix::Formatting("void foo(@A @B C p) { }", "A @", K_CLASS_BODY_DECLARATIONS);
pub const LOCAL_ANNOTATION_SEPARATION: Prefix = Prefix::Formatting("@A @B C p;", "A @", K_STATEMENTS);
pub const TYPE_ANNOTATION_SEPARATION: Prefix = Prefix::Formatting("C<@A @B D> l;", "A @", K_STATEMENTS);
pub const VARARGS: Prefix = Prefix::Formatting("void foo(A ... a) { }", "A .", K_CLASS_BODY_DECLARATIONS);
pub const TRY_RESOURCES: Prefix = Prefix::Formatting("try (A a = new A(); B b = new B()) {}", "; B", K_STATEMENTS);
pub const TRY_RESOURCES_PAREN: Prefix = Prefix::Formatting("try (A a = new A(); B b = new B()) {}", "y (", K_STATEMENTS);

/// A `BlockContext`.
#[derive(Clone, Copy, Debug)]
pub enum BlockContext {
    /// `BlockFormattingPrefix(prefix, start)`.
    Prefix(&'static str, usize),
    /// `BlockFormattingPrefixSuffix(prefix, suffix, start)`.
    PrefixSuffix(&'static str, &'static str, usize),
}

pub const IF_BLOCK_WITH_ELSE: BlockContext = BlockContext::PrefixSuffix("if (true)", "else{}", 8);
pub const IF_BLOCK_NO_ELSE: BlockContext = BlockContext::Prefix("if (true)", 8);
pub const ELSE_AFTER_STATEMENT: BlockContext = BlockContext::Prefix("if (true) foo();else ", 15);
pub const ELSE_AFTER_BLOCK: BlockContext = BlockContext::Prefix("if (true) {}else ", 11);
pub const FOR_BLOCK: BlockContext = BlockContext::Prefix("for (;;) ", 7);
pub const WHILE_BLOCK: BlockContext = BlockContext::Prefix("while (true)", 11);
pub const DO_BLOCK: BlockContext = BlockContext::PrefixSuffix("do ", "while (true);", 1);

impl<'a> RewriteFormatter<'a> {
    pub fn new(options: &BTreeMap<String, String>, line_delimiter: &str, formatter: &'a mut dyn CodeFormatter) -> Self {
        let mut options = options.clone();
        // `alignment_for_resources_in_try = createAlignmentValue(true, WRAP_NEXT_PER_LINE, INDENT_DEFAULT)`.
        options.insert("org.eclipse.jdt.core.formatter.alignment_for_resources_in_try".into(), "81".into());
        RewriteFormatter {
            tab_width: indent::tab_width(&options),
            indent_width: indent::indent_width(&options),
            options,
            line_delimiter: line_delimiter.to_owned(),
            formatter,
        }
    }

    pub fn create_indent_string(&self, units: i32) -> String {
        indent::create_indentation_string(&self.options, units)
    }

    pub fn create_indent_string_min(&self, units: i32, minimum_spaces: i32) -> String {
        let mut s = self.create_indent_string(units);
        let spaces = self.compute_indent_in_spaces(&s);
        if spaces < minimum_spaces {
            s.push_str(&" ".repeat((minimum_spaces - spaces) as usize));
        }
        s
    }

    pub fn get_indent_string(&self, line: &str) -> String {
        indent::extract_indent_string(line, self.tab_width, self.indent_width)
    }

    pub fn get_indent_string_with_spaces(&self, line: &str) -> String {
        let v = indent::to_u16(line);
        let n = v.iter().take_while(|&&c| indent::is_indent_char(c)).count();
        indent::from_u16(&v[..n])
    }

    pub fn change_indent(&self, code: &str, code_indent_level: i32, new_indent: &str) -> String {
        indent::change_indent(code, code_indent_level, self.tab_width, self.indent_width, new_indent, &self.line_delimiter)
    }

    pub fn compute_indent_units(&self, line: &str) -> i32 {
        indent::measure_indent_units(&indent::to_u16(line), self.tab_width, self.indent_width)
    }

    pub fn compute_indent_in_spaces(&self, line: &str) -> i32 {
        indent::measure_indent_in_spaces(&indent::to_u16(line), self.tab_width)
    }

    /// `formatString(kind, string, offset, length, indentationLevel)`.
    pub fn format_string(&mut self, kind: i32, string: &str, offset: usize, length: usize, indent: i32) -> Option<FormatEdits> {
        self.formatter.format(&FormatRequest { kind, source: string.to_owned(), offset, length, indent })
    }

    /// `Prefix.getPrefix(indent)`.
    pub fn prefix(&mut self, prefix: Prefix, indent: i32) -> String {
        match prefix {
            Prefix::Const(s) => s.to_owned(),
            Prefix::Formatting(string, sub, kind) => {
                let start = indent::len16(&string[..string.find(sub).unwrap_or(0)]);
                let mut pos = [Position { offset: start as i32, length: indent::len16(sub) as i32, deleted: false }];
                let len = indent::len16(string);
                let res = self.format_string(kind, string, 0, len, indent);
                let s = match res {
                    Some(edits) => evaluate_formatter_edits(string, &edits, &mut pos),
                    None => string.to_owned(),
                };
                indent::sub16(&s, (pos[0].offset + 1) as usize, (pos[0].offset + pos[0].length - 1).max(pos[0].offset + 1) as usize)
            }
        }
    }

    /// `BlockContext.getPrefixAndSuffix(indent, node, events)`.
    pub fn prefix_and_suffix(&mut self, ctx: BlockContext, indent: i32, rw: &ASTRewrite, node: RNode) -> (String, String) {
        let node_string = Flattener::as_string(rw, node);
        match ctx {
            BlockContext::Prefix(prefix, start) => {
                let s = format!("{prefix}{node_string}");
                let plen = indent::len16(prefix);
                let mut pos = [Position { offset: start as i32, length: (plen + 1 - start) as i32, deleted: false }];
                let len = indent::len16(&s);
                let res = self.format_string(K_STATEMENTS, &s, 0, len, indent);
                let s = match res {
                    Some(edits) => evaluate_formatter_edits(&s, &edits, &mut pos),
                    None => s,
                };
                (indent::sub16(&s, (pos[0].offset + 1) as usize, (pos[0].offset + pos[0].length - 1) as usize), String::new())
            }
            BlockContext::PrefixSuffix(prefix, suffix, start) => {
                let node_start = indent::len16(prefix);
                let node_end = node_start + indent::len16(&node_string) - 1;
                let s = format!("{prefix}{node_string}{suffix}");
                let mut pos = [
                    Position { offset: start as i32, length: (node_start + 1 - start) as i32, deleted: false },
                    Position { offset: node_end as i32, length: 2, deleted: false },
                ];
                let len = indent::len16(&s);
                let res = self.format_string(K_STATEMENTS, &s, 0, len, indent);
                let s = match res {
                    Some(edits) => evaluate_formatter_edits(&s, &edits, &mut pos),
                    None => s,
                };
                (
                    indent::sub16(&s, (pos[0].offset + 1) as usize, (pos[0].offset + pos[0].length - 1) as usize),
                    indent::sub16(&s, (pos[1].offset + 1) as usize, (pos[1].offset + pos[1].length - 1) as usize),
                )
            }
        }
    }

    /// `getFormattedResult(node, initialIndentationLevel, minimumIndentInSpaces, resultingMarkers)`.
    pub fn formatted_result(&mut self, rw: &ASTRewrite, node: RNode, initial_indent: i32, minimum_indent: i32, markers: &mut Vec<NodeMarker>) -> String {
        let mut flattener = Flattener::new(rw, true);
        flattener.accept(node);
        let unformatted = flattener.result();
        let node_markers = std::mem::take(&mut flattener.markers);
        let edits = self.format_node(rw, node, &unformatted, initial_indent);
        let mut positions: Vec<Position> = node_markers.iter().map(|m| Position { offset: m.offset, length: m.length, deleted: false }).collect();
        let result = match edits {
            Some(edits) => evaluate_formatter_edits(&unformatted, &edits, &mut positions),
            None => {
                if initial_indent <= 0 {
                    unformatted.clone()
                } else {
                    let mut indent_string = self.create_indent_string(initial_indent);
                    let spaces = self.compute_indent_in_spaces(&indent_string);
                    if spaces < minimum_indent {
                        indent_string = " ".repeat((minimum_indent - spaces) as usize) + &indent_string;
                    }
                    let mut edits = vec![(0usize, 0usize, indent_string.clone())];
                    edits.extend(indent::get_change_indent_edits(&indent::to_u16(&unformatted), 0, self.tab_width, self.indent_width, &indent_string));
                    evaluate_formatter_edits(&unformatted, &edits, &mut positions)
                }
            }
        };
        for (m, p) in node_markers.into_iter().zip(positions) {
            markers.push(NodeMarker { offset: p.offset, length: p.length, data: m.data });
        }
        result
    }

    /// `formatNode(node, str, indentationLevel, minimumIndentInSpaces)`.
    fn format_node(&mut self, rw: &ASTRewrite, node: RNode, s: &str, indent: i32) -> Option<FormatEdits> {
        use NodeKind::*;
        let kind = rw.kind(node);
        let (code, prefix, suffix): (i32, &str, &str) = if kind.is_statement() {
            if kind == SwitchCase {
                (K_STATEMENTS, "switch(1) {", "}")
            } else {
                (K_STATEMENTS, "", "")
            }
        } else if kind.is_expression() && kind != VariableDeclarationExpression {
            if kind.is_annotation() {
                (K_COMPILATION_UNIT, "", "\nclass A {}")
            } else {
                (K_EXPRESSION, "", "")
            }
        } else if kind.is_body_declaration() {
            (K_CLASS_BODY_DECLARATIONS, "", "")
        } else {
            match kind {
                AnonymousClassDeclaration => (K_STATEMENTS, "new A()", ";"),
                ArrayType | PrimitiveType | SimpleType | ParameterizedType | QualifiedType => {
                    (K_CLASS_BODY_DECLARATIONS, "void m(final ", " x);")
                }
                CatchClause => (K_STATEMENTS, "try {}", ""),
                CompilationUnit => (K_COMPILATION_UNIT, "", ""),
                ImportDeclaration | PackageDeclaration => (K_COMPILATION_UNIT, "", "\nclass A {}"),
                Javadoc => (K_COMPILATION_UNIT, "", "\nclass A {}"),
                SingleVariableDeclaration => (K_CLASS_BODY_DECLARATIONS, "void m(", ");"),
                VariableDeclarationExpression => (K_STATEMENTS, "", ";"),
                VariableDeclarationFragment => (K_STATEMENTS, "A ", ";"),
                TagElement | TextElement | MemberRef | MethodRef | MethodRefParameter | JavaDocTextElement => return None,
                TypeParameter => (K_COMPILATION_UNIT, "class X<", "> {}"),
                WildcardType => (K_CLASS_BODY_DECLARATIONS, "A<", "> x;"),
                MemberValuePair => (K_COMPILATION_UNIT, "@Author(", ") class x {}"),
                Modifier => (K_COMPILATION_UNIT, "", " class x {}"),
                ModuleDeclaration | ModuleModifier => (K_MODULE_INFO, "", ""),
                _ => return None,
            }
        };
        let concat = format!("{prefix}{s}{suffix}");
        let plen = indent::len16(prefix);
        let edits = self.format_string(code, &concat, plen, indent::len16(s), indent)?;
        if plen == 0 {
            return Some(edits);
        }
        // `shifEdit`: drop the prefix offset.
        Some(edits.into_iter().filter(|(o, _, _)| *o >= plen).map(|(o, l, t)| (o - plen, l, t)).collect())
    }
}

/// Runs `rewrite.rewrite_ast` with the Eclipse formatter in the bridge:
/// the analyzer is re-run until every formatter call it makes is answered.
pub async fn rewrite_with_bridge(
    rw: &ASTRewrite,
    options: &BTreeMap<String, String>,
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
) -> Result<super::text_edit::EditTree, super::RewriteError> {
    let mut cache = FormatterCache::default();
    let line_delim = super::analyzer::default_line_delimiter(&rw.ast.source);
    for _ in 0..6 {
        cache.missing.clear();
        let result = rw.rewrite_ast(options, &mut cache);
        if cache.missing.is_empty() {
            return result;
        }
        let jobs: Vec<crate::analysis::semantic::protocol::FormatJob> = cache
            .missing
            .iter()
            .map(|r| crate::analysis::semantic::protocol::FormatJob {
                source: r.source.clone(),
                kind: r.kind,
                offset: r.offset,
                length: r.length,
                indentation_level: r.indent,
            })
            .collect();
        let mut format_options = options.clone();
        format_options.insert("org.eclipse.jdt.core.formatter.alignment_for_resources_in_try".into(), "81".into());
        let resp = dispatcher
            .send_request(crate::analysis::semantic::BridgeRequest::FormatBatch {
                id: crate::analysis::semantic::ecj_process::next_id(),
                jobs,
                line_separator: line_delim.clone(),
                options: format_options,
            })
            .await
            .map_err(|e| super::RewriteError(format!("formatBatch failed: {e}")))?;
        let results = match resp {
            crate::analysis::semantic::BridgeResponse::FormatBatch { results, .. } => results,
            _ => return Err(super::RewriteError("unexpected formatBatch response".into())),
        };
        let missing = std::mem::take(&mut cache.missing);
        for (req, res) in missing.into_iter().zip(results) {
            cache.results.insert(req, res.map(|edits| edits.into_iter().map(|e| (e.offset, e.length, e.text)).collect()));
        }
    }
    rw.rewrite_ast(options, &mut cache)
}
