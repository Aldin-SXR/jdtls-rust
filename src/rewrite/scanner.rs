//! Port of the rewrite `TokenScanner` over a JDT-like Java scanner
//! (`Scanner(tokenizeComments = true, tokenizeWhiteSpace = false)`), on
//! UTF-16 source.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Ident,
    /// Keyword (incl. `true`/`false`/`null`, `non-sealed`).
    Kw(&'static str),
    /// Operator / separator (`{`, `->`, `>>`, ...).
    Op(&'static str),
    Literal,
    CommentLine,
    CommentBlock,
    CommentJavadoc,
    Eof,
}

impl Tok {
    /// `TokenScanner.isComment`.
    pub fn is_comment(self) -> bool {
        matches!(self, Tok::CommentLine | Tok::CommentBlock | Tok::CommentJavadoc)
    }
}

/// End of file / invalid input (`CoreException` from the scanner).
#[derive(Debug, Clone)]
pub struct ScanError(pub String);

const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue",
    "default", "do", "double", "else", "enum", "extends", "final", "finally", "float", "for", "goto", "if",
    "implements", "import", "instanceof", "int", "interface", "long", "native", "new", "package", "private",
    "protected", "public", "return", "short", "static", "strictfp", "super", "switch", "synchronized", "this",
    "throw", "throws", "transient", "try", "void", "volatile", "while", "true", "false", "null",
];

const OPS: &[&str] = &[
    ">>>=", "<<=", ">>=", ">>>", "...", "->", "::", "++", "--", "&&", "||", "==", "!=", "<=", ">=", "+=", "-=",
    "*=", "/=", "&=", "|=", "^=", "%=", "<<", ">>", "(", ")", "{", "}", "[", "]", ";", ",", ".", "@", "=", ">",
    "<", "!", "~", "?", ":", "+", "-", "*", "/", "&", "|", "^", "%",
];

pub struct TokenScanner<'a> {
    src: &'a [u16],
    pos: usize,
    end: usize,
    cur_start: usize,
    cur_end: usize,
}

fn is_ident_start(c: u16) -> bool {
    c == b'$' as u16
        || c == b'_' as u16
        || (c as u32) < 128 && (c as u8).is_ascii_alphabetic()
        || (c >= 128 && char::from_u32(c as u32).is_some_and(|ch| ch.is_alphabetic()))
        || (0xD800..0xDC00).contains(&c)
}

fn is_ident_part(c: u16) -> bool {
    is_ident_start(c) || (c as u32) < 128 && (c as u8).is_ascii_digit() || (0xDC00..0xE000).contains(&c)
        || (c >= 128 && char::from_u32(c as u32).is_some_and(|ch| ch.is_alphanumeric()))
}

impl<'a> TokenScanner<'a> {
    pub fn new(src: &'a [u16]) -> Self {
        TokenScanner { src, pos: 0, end: src.len(), cur_start: 0, cur_end: 0 }
    }

    pub fn set_offset(&mut self, offset: i32) {
        self.pos = (offset.max(0) as usize).min(self.end);
    }

    pub fn current_start_offset(&self) -> i32 {
        self.cur_start as i32
    }

    pub fn current_end_offset(&self) -> i32 {
        self.cur_end as i32
    }

    pub fn current_length(&self) -> i32 {
        (self.cur_end - self.cur_start) as i32
    }

    fn at(&self, i: usize) -> u16 {
        if i < self.end {
            self.src[i]
        } else {
            0
        }
    }

    fn starts_with(&self, i: usize, s: &str) -> bool {
        s.encode_utf16().enumerate().all(|(k, c)| self.at(i + k) == c) && i + s.len() <= self.end
    }

    /// `Scanner.getNextToken()` (comments returned, whitespace skipped).
    fn next_token(&mut self) -> Result<Tok, ScanError> {
        let mut i = self.pos;
        while i < self.end && super::indent::is_whitespace(self.src[i]) {
            i += 1;
        }
        self.cur_start = i;
        if i >= self.end {
            self.cur_end = i;
            self.pos = i;
            return Ok(Tok::Eof);
        }
        let c = self.src[i];
        let tok;
        if c == b'/' as u16 && self.at(i + 1) == b'/' as u16 {
            let mut j = i + 2;
            while j < self.end && !super::indent::is_line_delimiter_char(self.src[j]) {
                j += 1;
            }
            tok = Tok::CommentLine;
            i = j;
        } else if c == b'/' as u16 && self.at(i + 1) == b'*' as u16 {
            let javadoc = self.at(i + 2) == b'*' as u16 && self.at(i + 3) != b'/' as u16;
            let mut j = i + 2;
            loop {
                if j + 1 >= self.end + 1 || j >= self.end {
                    return Err(ScanError("Unterminated comment".into()));
                }
                if self.src[j] == b'*' as u16 && self.at(j + 1) == b'/' as u16 {
                    j += 2;
                    break;
                }
                j += 1;
            }
            tok = if javadoc { Tok::CommentJavadoc } else { Tok::CommentBlock };
            i = j;
        } else if self.starts_with(i, "\"\"\"") {
            let mut j = i + 3;
            loop {
                if j >= self.end {
                    return Err(ScanError("Unterminated text block".into()));
                }
                if self.src[j] == b'\\' as u16 {
                    j += 2;
                    continue;
                }
                if self.starts_with(j, "\"\"\"") {
                    j += 3;
                    break;
                }
                j += 1;
            }
            tok = Tok::Literal;
            i = j;
        } else if c == b'"' as u16 || c == b'\'' as u16 {
            let q = c;
            let mut j = i + 1;
            loop {
                if j >= self.end || super::indent::is_line_delimiter_char(self.src[j]) {
                    return Err(ScanError("Unterminated literal".into()));
                }
                if self.src[j] == b'\\' as u16 {
                    j += 2;
                    continue;
                }
                if self.src[j] == q {
                    j += 1;
                    break;
                }
                j += 1;
            }
            tok = Tok::Literal;
            i = j;
        } else if (c as u32) < 128 && (c as u8).is_ascii_digit() || (c == b'.' as u16 && (self.at(i + 1) as u32) < 128 && (self.at(i + 1) as u8).is_ascii_digit()) {
            let mut j = i;
            while j < self.end {
                let d = self.src[j];
                let ok = (d as u32) < 128 && ((d as u8).is_ascii_alphanumeric() || d == b'_' as u16 || d == b'.' as u16);
                let exp_sign = (d == b'+' as u16 || d == b'-' as u16)
                    && j > i
                    && matches!(self.src[j - 1], 0x65 | 0x45 | 0x70 | 0x50); // e E p P
                if ok || exp_sign {
                    j += 1;
                } else {
                    break;
                }
            }
            tok = Tok::Literal;
            i = j;
        } else if is_ident_start(c) {
            let mut j = i;
            while j < self.end && is_ident_part(self.src[j]) {
                j += 1;
            }
            let word = String::from_utf16_lossy(&self.src[i..j]);
            if word == "non" && self.starts_with(j, "-sealed") {
                j += 7;
                tok = Tok::Kw("non-sealed");
            } else if let Some(k) = KEYWORDS.iter().find(|k| **k == word) {
                tok = Tok::Kw(k);
            } else {
                tok = Tok::Ident;
            }
            i = j;
        } else {
            let op = OPS.iter().find(|o| self.starts_with(i, o));
            match op {
                Some(o) => {
                    tok = Tok::Op(o);
                    i += o.len();
                }
                None => {
                    // Invalid input: skip the character.
                    tok = Tok::Op("?invalid");
                    i += 1;
                }
            }
        }
        self.cur_end = i;
        self.pos = i;
        Ok(tok)
    }

    /// `TokenScanner.readNext(ignoreComments)`; EOF is an error.
    pub fn read_next(&mut self, ignore_comments: bool) -> Result<Tok, ScanError> {
        loop {
            let t = self.next_token()?;
            if t == Tok::Eof {
                return Err(ScanError("End Of File".into()));
            }
            if !(ignore_comments && t.is_comment()) {
                return Ok(t);
            }
        }
    }

    /// `readNext` returning `Eof` instead of failing (for loops that test it).
    pub fn read_next_or_eof(&mut self, ignore_comments: bool) -> Result<Tok, ScanError> {
        loop {
            let t = self.next_token()?;
            if !(ignore_comments && t.is_comment()) || t == Tok::Eof {
                return Ok(t);
            }
        }
    }

    pub fn read_next_at(&mut self, offset: i32, ignore_comments: bool) -> Result<Tok, ScanError> {
        self.set_offset(offset);
        self.read_next(ignore_comments)
    }

    pub fn next_start_offset(&mut self, offset: i32, ignore_comments: bool) -> Result<i32, ScanError> {
        self.read_next_at(offset, ignore_comments)?;
        Ok(self.current_start_offset())
    }

    pub fn next_end_offset(&mut self, offset: i32, ignore_comments: bool) -> Result<i32, ScanError> {
        self.read_next_at(offset, ignore_comments)?;
        Ok(self.current_end_offset())
    }

    pub fn read_to_token(&mut self, tok: Tok) -> Result<(), ScanError> {
        loop {
            if self.read_next(false)? == tok {
                return Ok(());
            }
        }
    }

    pub fn read_to_token_at(&mut self, tok: Tok, offset: i32) -> Result<(), ScanError> {
        self.set_offset(offset);
        self.read_to_token(tok)
    }

    pub fn token_start_offset(&mut self, tok: Tok, start: i32) -> Result<i32, ScanError> {
        self.read_to_token_at(tok, start)?;
        Ok(self.current_start_offset())
    }

    pub fn token_end_offset(&mut self, tok: Tok, start: i32) -> Result<i32, ScanError> {
        self.read_to_token_at(tok, start)?;
        Ok(self.current_end_offset())
    }

    pub fn previous_token_end_offset(&mut self, tok: Tok, start: i32) -> Result<i32, ScanError> {
        self.set_offset(start);
        let mut res = start;
        let mut cur = self.read_next(false)?;
        while cur != tok {
            res = self.current_end_offset();
            cur = self.read_next(false)?;
        }
        Ok(res)
    }
}
