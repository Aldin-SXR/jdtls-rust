//! Port of jdt.ls `FoldingRangeHandler`.
//!
//! The algorithm is the upstream one: a scanner pass over the whole unit for
//! comments, comment groups and `// region` markers, followed by the type
//! ranges of the Java model (imports, types, methods, initializers) and the
//! brace / `case` ranges inside every method body.

use std::collections::BTreeMap;

use regex::Regex;
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

use super::java_model::{self, CompilationUnit, Member, TypeDecl};
use super::scanner::{scan, scan_range, LineIndex, TokKind, Token};

fn range(start: u32, end: u32, kind: Option<FoldingRangeKind>) -> FoldingRange {
    FoldingRange { start_line: start, start_character: None, end_line: end, end_character: None, kind, collapsed_text: None }
}

pub fn folding_ranges(src: &str) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    if src.trim().is_empty() {
        return out;
    }
    let li = LineIndex::new(src);
    // 1-based line numbers, as `IScanner.getLineNumber`.
    let line1 = |off: usize| li.line(off) as i64 + 1;

    static REGION_START: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"^//\s*#?region|^//\s+<editor-fold.*>").unwrap());
    static REGION_END: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"^//\s*#?endregion|^//\s+</editor-fold>").unwrap());

    let tokens = scan(src);
    let mut single_start: i64 = -1;
    let mut single_end: i64 = -1;
    // The scanner has not recorded any line end before the first token.
    let mut prev_line: i64 = 1;
    let mut region_starts: Vec<usize> = Vec::new();
    for tok in &tokens {
        let start = tok.start;
        let start_line = line1(start);
        match tok.kind {
            TokKind::Javadoc | TokKind::BlockComment => {
                let end = tok.end - 1;
                out.push(range((start_line - 1) as u32, (line1(end) - 1) as u32, Some(FoldingRangeKind::Comment)));
            }
            TokKind::LineComment => {
                let text = tok.text(src);
                if REGION_START.is_match(text) {
                    region_starts.push(start);
                } else if REGION_END.is_match(text) {
                    if let Some(rs) = region_starts.pop() {
                        out.push(range((line1(rs) - 1) as u32, (line1(start) - 1) as u32, Some(FoldingRangeKind::Region)));
                    }
                } else if prev_line == start_line {
                    add_comment_range(&mut out, single_start, single_end);
                } else if start_line == single_end + 1 {
                    single_end = start_line;
                } else {
                    add_comment_range(&mut out, single_start, single_end);
                    single_start = start_line;
                    single_end = start_line;
                }
            }
            _ => {}
        }
        prev_line = start_line;
    }
    add_comment_range(&mut out, single_start, single_end);

    let cu: CompilationUnit = java_model::parse(src);
    if let Some((s, e)) = cu.import_container() {
        out.push(range(li.line(s), li.line(e), Some(FoldingRangeKind::Imports)));
    }
    for t in &cu.types {
        type_ranges(src, &li, t, &mut out);
    }
    out
}

fn add_comment_range(out: &mut Vec<FoldingRange>, start: i64, end: i64) {
    if end > start {
        out.push(range((start - 1) as u32, (end - 1) as u32, Some(FoldingRangeKind::Comment)));
    }
}

fn type_ranges(src: &str, li: &LineIndex, t: &TypeDecl, out: &mut Vec<FoldingRange>) {
    out.push(range(li.line(t.name_range.0), li.line(t.source.1), None));
    for m in &t.members {
        match m {
            Member::Method(m) => method_ranges(src, li, m.source, Some(m.name_range.0), out),
            Member::Initializer(i) => method_ranges(src, li, i.source, None, out),
            Member::Type(t) => type_ranges(src, li, t, out),
            Member::Field(_) => {}
        }
    }
}

fn method_ranges(src: &str, li: &LineIndex, source: (usize, usize), name_start: Option<usize>, out: &mut Vec<FoldingRange>) {
    let (shift, end) = source;
    let name_start = name_start.unwrap_or(shift);
    out.push(range(li.line(name_start), li.line(end), None));

    // `resetTo(shift, shift + length)`: the end position is inclusive.
    let tokens: Vec<Token> = scan_range(src, shift, end + 1);
    let mut prev_token_line: i64 = li.line(shift) as i64;
    let mut left_parens: Option<Vec<i64>> = None;
    let mut prev_case_lines: Vec<i64> = Vec::new();
    let mut candidates: BTreeMap<i64, i64> = BTreeMap::new();
    for tok in &tokens {
        let current_line = li.line(tok.start) as i64;
        let text = if tok.kind == TokKind::Op || tok.kind == TokKind::Keyword { tok.text(src) } else { "" };
        match text {
            "{" => match left_parens.as_mut() {
                None => left_parens = Some(Vec::new()),
                Some(stack) => {
                    let keys: Vec<i64> = candidates.iter().filter(|(_, v)| **v == current_line).map(|(k, _)| *k).collect();
                    for key in keys {
                        candidates.remove(&key);
                        if key < current_line - 1 {
                            candidates.insert(key, current_line - 1);
                        }
                    }
                    if prev_token_line != current_line && prev_case_lines.last() == Some(&prev_token_line) {
                        stack.push(prev_token_line);
                    } else {
                        stack.push(current_line);
                    }
                }
            },
            "}" => {
                if let Some(stack) = left_parens.as_mut() {
                    if let Some(start_line) = stack.pop() {
                        let end_line = li.line(tok.end - 1) as i64;
                        if start_line < end_line {
                            candidates.insert(start_line, end_line);
                        }
                        if let Some(&prev_case_line) = prev_case_lines.last() {
                            if start_line < prev_case_line && end_line - 1 > prev_case_line {
                                candidates.insert(prev_case_line, end_line - 1);
                                prev_case_lines.pop();
                            }
                        }
                    }
                }
            }
            "switch" => prev_case_lines.push(-1),
            "case" | "default" => {
                if let Some(prev_case_line) = prev_case_lines.pop() {
                    if prev_case_line != -1 && current_line - 1 >= prev_case_line {
                        candidates.insert(prev_case_line, current_line - 1);
                    }
                    prev_case_lines.push(current_line);
                }
            }
            _ => {}
        }
        prev_token_line = current_line;
    }
    for (k, v) in candidates {
        out.push(range(k as u32, v as u32, None));
    }
}
