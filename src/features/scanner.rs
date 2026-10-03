//! A Java token scanner modelled on JDT's `IScanner` (as created by
//! `JDTUtils.createScanner(project, tokenizeComments, ...)`).
//!
//! It produces the token stream jdt.ls handlers walk (folding ranges, the
//! type-declaration keyword gap of semantic tokens, ...).  Like JDT, invalid
//! input (empty character literals, unterminated strings, stray characters)
//! is skipped and scanning resumes after it.  Offsets are byte offsets into
//! the source string.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    Keyword,
    LineComment,
    BlockComment,
    Javadoc,
    StringLit,
    TextBlock,
    CharLit,
    Number,
    /// Punctuation / operators (`{`, `}`, `(`, `.`, `@`, `->`, ...).  `>` is
    /// always a single-character token so generic type arguments nest.
    Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokKind,
    pub start: usize,
    /// Exclusive end offset.
    pub end: usize,
}

impl Token {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, TokKind::LineComment | TokKind::BlockComment | TokKind::Javadoc)
    }

}

pub const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const",
    "continue", "default", "do", "double", "else", "enum", "extends", "final", "finally", "float",
    "for", "goto", "if", "implements", "import", "instanceof", "int", "interface", "long", "native",
    "new", "package", "private", "protected", "public", "return", "short", "static", "strictfp",
    "super", "switch", "synchronized", "this", "throw", "throws", "transient", "try", "void",
    "volatile", "while", "true", "false", "null",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

const OPS: &[&str] = &[
    "<<=", "...", "->", "::", "++", "--", "&&", "||", "==", "!=", "<=", "+=", "-=", "*=", "/=", "&=",
    "|=", "^=", "%=", "<<",
];

/// Scan `src[start..end]` into tokens (comments included).
pub fn scan_range(src: &str, start: usize, end: usize) -> Vec<Token> {
    let bytes = src.as_bytes();
    let end = end.min(bytes.len());
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let c = bytes[i];
        // Whitespace
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0c {
            i += 1;
            continue;
        }
        let s = i;
        // Comments
        if c == b'/' && i + 1 < end && bytes[i + 1] == b'/' {
            while i < end && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            out.push(Token { kind: TokKind::LineComment, start: s, end: i });
            continue;
        }
        if c == b'/' && i + 1 < end && bytes[i + 1] == b'*' {
            let javadoc = i + 2 < end && bytes[i + 2] == b'*' && !(i + 3 < end && bytes[i + 3] == b'/');
            let mut j = i + 2;
            let mut closed = false;
            while j + 1 < end {
                if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                // Unterminated comment: JDT reports invalid input and stops.
                break;
            }
            i = j + 2;
            out.push(Token { kind: if javadoc { TokKind::Javadoc } else { TokKind::BlockComment }, start: s, end: i });
            continue;
        }
        // Text blocks
        if c == b'"' && i + 2 < end && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
            let mut j = i + 3;
            let mut closed = false;
            while j < end {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' && j + 2 < end && bytes[j + 1] == b'"' && bytes[j + 2] == b'"' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                break;
            }
            i = j + 3;
            out.push(Token { kind: TokKind::TextBlock, start: s, end: i });
            continue;
        }
        // Strings
        if c == b'"' {
            let mut j = i + 1;
            let mut closed = false;
            while j < end && bytes[j] != b'\n' && bytes[j] != b'\r' {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                i = j + 1;
                out.push(Token { kind: TokKind::StringLit, start: s, end: i });
            } else {
                i = j.min(end); // invalid: resume at line end
            }
            continue;
        }
        // Character literals
        if c == b'\'' {
            let mut j = i + 1;
            if j < end && bytes[j] == b'\'' {
                // Empty character literal: invalid; relocate past a close quote.
                i = j + 1;
                continue;
            }
            let mut closed = false;
            while j < end && bytes[j] != b'\n' && bytes[j] != b'\r' {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'\'' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                i = j + 1;
                out.push(Token { kind: TokKind::CharLit, start: s, end: i });
            } else {
                i = (i + 2).min(end);
            }
            continue;
        }
        // Numbers
        if c.is_ascii_digit() || (c == b'.' && i + 1 < end && bytes[i + 1].is_ascii_digit()) {
            let mut j = i + 1;
            while j < end {
                let d = bytes[j];
                if d.is_ascii_alphanumeric() || d == b'_' || d == b'.' {
                    j += 1;
                } else if (d == b'+' || d == b'-') && matches!(bytes[j - 1], b'e' | b'E' | b'p' | b'P') && !src[s..j].starts_with("0x") && !src[s..j].starts_with("0X")
                    || (d == b'+' || d == b'-') && matches!(bytes[j - 1], b'p' | b'P')
                {
                    j += 1;
                } else {
                    break;
                }
            }
            i = j;
            out.push(Token { kind: TokKind::Number, start: s, end: i });
            continue;
        }
        // Identifiers / keywords
        let ch = src[i..].chars().next().unwrap();
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut j = i;
            for (k, ch2) in src[i..end].char_indices() {
                if ch2.is_alphanumeric() || ch2 == '_' || ch2 == '$' {
                    j = i + k + ch2.len_utf8();
                } else {
                    break;
                }
            }
            i = j;
            let kind = if is_keyword(&src[s..i]) { TokKind::Keyword } else { TokKind::Ident };
            out.push(Token { kind, start: s, end: i });
            continue;
        }
        // Operators
        if c.is_ascii_punctuation() {
            let mut len = 1;
            for op in OPS {
                if src[i..end].starts_with(op) {
                    len = op.len();
                    break;
                }
            }
            i += len;
            out.push(Token { kind: TokKind::Op, start: s, end: i });
            continue;
        }
        // Anything else is invalid input.
        i += ch.len_utf8();
    }
    out
}

pub fn scan(src: &str) -> Vec<Token> {
    scan_range(src, 0, src.len())
}

/// Line starts for 0-based line numbers (handles `\n`, `\r\n`, `\r`).
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let b = src.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\r' {
                if i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                starts.push(i + 1);
            } else if b[i] == b'\n' {
                starts.push(i + 1);
            }
            i += 1;
        }
        Self { starts }
    }

    /// 0-based line of a byte offset.  A line separator belongs to the line
    /// it terminates.
    pub fn line(&self, offset: usize) -> u32 {
        match self.starts.binary_search(&offset) {
            Ok(i) => i as u32,
            Err(i) => (i - 1) as u32,
        }
    }


    /// LSP position (UTF-16 columns) of a byte offset.
    pub fn position(&self, src: &str, offset: usize) -> tower_lsp::lsp_types::Position {
        let offset = offset.min(src.len());
        let line = self.line(offset);
        let start = self.starts[line as usize];
        let col: usize = src[start..offset].chars().map(|c| c.len_utf16()).sum();
        tower_lsp::lsp_types::Position { line, character: col as u32 }
    }

    pub fn range(&self, src: &str, start: usize, end: usize) -> tower_lsp::lsp_types::Range {
        tower_lsp::lsp_types::Range { start: self.position(src, start), end: self.position(src, end) }
    }

    /// Byte offset of an LSP position (clamped to the line / document).
    pub fn offset(&self, src: &str, pos: tower_lsp::lsp_types::Position) -> usize {
        if pos.line as usize >= self.starts.len() {
            return src.len();
        }
        let start = self.starts[pos.line as usize];
        let line_end = self.starts.get(pos.line as usize + 1).copied().unwrap_or(src.len());
        let mut units = 0usize;
        for (k, c) in src[start..line_end].char_indices() {
            if units >= pos.character as usize || c == '\n' || c == '\r' {
                return start + k;
            }
            units += c.len_utf16();
        }
        line_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_basic_tokens() {
        let src = "class A { /** doc */ int x = 'a'; // c\n String s = \"}\"; char e = ''; }";
        let toks = scan(src);
        let texts: Vec<&str> = toks.iter().map(|t| t.text(src)).collect();
        assert_eq!(
            texts,
            vec!["class", "A", "{", "/** doc */", "int", "x", "=", "'a'", ";", "// c", "String", "s", "=", "\"}\"", ";", "char", "e", "=", ";", "}"]
        );
        assert_eq!(toks[3].kind, TokKind::Javadoc);
    }

    #[test]
    fn line_index() {
        let src = "a\r\nb\nc\rd";
        let li = LineIndex::new(src);
        assert_eq!(li.line(0), 0);
        assert_eq!(li.line(1), 0);
        assert_eq!(li.line(2), 0);
        assert_eq!(li.line(3), 1);
        assert_eq!(li.line(5), 2);
        assert_eq!(li.line(7), 3);
    }
}
