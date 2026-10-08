//! Port of `NLSScanner` / `NLSLine` / `NLSElement` as far as the string
//! quick assists need them: the string literals of a line and whether each
//! has a `//$NON-NLS-n$` tag.

use crate::rewrite::scanner::{Tok, TokenScanner};
use crate::semantic_ast::Ast;

pub struct NlsElement {
    /// Offset of the literal in the scanned text.
    pub offset: usize,
    pub has_tag: bool,
}

pub struct NlsLine {
    pub elements: Vec<NlsElement>,
}

const TAG_PREFIX: &str = "//$NON-NLS-";

fn line_number(source: &[u16], offset: usize) -> usize {
    let mut line = 0;
    let mut i = 0;
    while i < offset.min(source.len()) {
        match source[i] {
            0x0A => line += 1,
            0x0D => {
                if source.get(i + 1) != Some(&0x0A) {
                    line += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    line
}

/// `NLSScanner.scan(String)`: the lines carrying string literals.
pub fn scan(source: &[u16]) -> Option<Vec<NlsLine>> {
    let mut lines: Vec<(usize, NlsLine)> = Vec::new();
    let mut scanner = TokenScanner::new(source);
    let mut current_line_nr: Option<usize> = None;
    let mut previous_line_nr: Option<usize> = None;
    let mut inside_annotation: Vec<i32> = Vec::new();
    let mut default_counter = 0;
    loop {
        let token = scanner.read_next_or_eof(false).ok()?;
        match token {
            Tok::Eof => break,
            Tok::Op("@") => inside_annotation.push(-1),
            Tok::Kw("interface") => inside_annotation.clear(),
            Tok::Ident => {
                if let Some(last) = inside_annotation.last_mut() {
                    if *last == -1 {
                        *last = 0;
                    } else if *last == 0 {
                        inside_annotation.pop();
                    }
                }
            }
            Tok::Op(".") => {
                if let Some(last) = inside_annotation.last_mut() {
                    if *last == 0 {
                        *last = -1;
                    } else if *last == -1 {
                        inside_annotation.pop();
                    }
                }
            }
            Tok::Op("(") => {
                if let Some(last) = inside_annotation.last_mut() {
                    *last += 1;
                }
            }
            Tok::Op(")") => {
                if let Some(last) = inside_annotation.last_mut() {
                    *last -= 1;
                    if *last <= 0 {
                        inside_annotation.pop();
                    }
                }
            }
            Tok::Kw("default") => default_counter = 1,
            Tok::Op(":") => {
                if default_counter == 1 {
                    default_counter = 0;
                } else if default_counter > 0 {
                    default_counter += 1;
                }
            }
            Tok::Op(";") => default_counter = 0,
            Tok::Op("{") => {
                if default_counter > 1 {
                    default_counter = 0;
                }
            }
            Tok::Literal if source.get(scanner.current_start_offset() as usize) == Some(&(b'"' as u16)) => {
                if inside_annotation.is_empty() && default_counter == 0 {
                    let start = scanner.current_start_offset() as usize;
                    let end = scanner.current_end_offset() as usize;
                    let is_text_block = end - start >= 6 && source[start..start + 3] == [0x22, 0x22, 0x22];
                    let nr = line_number(source, if is_text_block { end.saturating_sub(1) } else { start });
                    if lines.is_empty() || previous_line_nr != Some(nr) {
                        lines.push((nr, NlsLine { elements: Vec::new() }));
                        previous_line_nr = Some(nr);
                    }
                    current_line_nr = Some(nr);
                    lines.last_mut().unwrap().1.elements.push(NlsElement { offset: start, has_tag: false });
                }
            }
            Tok::CommentLine => {
                default_counter = 0;
                let start = scanner.current_start_offset() as usize;
                if current_line_nr != Some(line_number(source, start)) {
                    continue;
                }
                let end = scanner.current_end_offset() as usize;
                parse_tags(lines.last_mut().map(|l| &mut l.1), &String::from_utf16_lossy(&source[start..end]));
            }
            Tok::CommentBlock | Tok::CommentJavadoc => {}
            _ => {
                if default_counter > 0 {
                    default_counter += 1;
                }
            }
        }
    }
    Some(lines.into_iter().map(|(_, l)| l).collect())
}

fn parse_tags(line: Option<&mut NlsLine>, comment: &str) {
    let Some(line) = line else { return };
    let mut pos = comment.find(TAG_PREFIX);
    while let Some(p) = pos {
        let start = p + TAG_PREFIX.len();
        let Some(end) = comment[start..].find('$').map(|e| e + start) else { return };
        let Ok(index) = comment[start..end].parse::<i32>() else { return };
        let i = index - 1;
        if i >= 0 && (i as usize) < line.elements.len() {
            line.elements[i as usize].has_tag = true;
        } else {
            return;
        }
        pos = comment[start..].find(TAG_PREFIX).map(|e| e + start);
    }
}

/// The first NLS line of `text` (`scanCurrentLine`), `None` when the text has
/// no literal or cannot be scanned.
fn first_line(text: &[u16]) -> Option<NlsLine> {
    scan(text)?.into_iter().next()
}

/// `scanCurrentLine(cu, exp)` of the conversion fixes: the whole line of
/// `offset`, up to the start of the next line.
pub fn scan_current_line(ast: &Ast, offset: usize) -> Option<NlsLine> {
    scan_range(ast, ast.line_start(ast.line_of(offset)), next_line_start(ast, offset)?)
}

/// The variant of `StringConcatToTextBlockFixCore`: from `offset` to the
/// start of the next line.
pub fn scan_from(ast: &Ast, offset: usize) -> Option<NlsLine> {
    scan_range(ast, offset, next_line_start(ast, offset)?)
}

fn next_line_start(ast: &Ast, offset: usize) -> Option<usize> {
    let line = ast.line_of(offset);
    (line + 1 < ast.line_count()).then(|| ast.line_start(line + 1))
}

fn scan_range(ast: &Ast, start: usize, end: usize) -> Option<NlsLine> {
    first_line(ast.source.get(start..end)?)
}

/// `CompilationUnit.getColumnNumber(offset)`.
pub fn column(ast: &Ast, offset: usize) -> usize {
    offset - ast.line_start(ast.line_of(offset))
}
