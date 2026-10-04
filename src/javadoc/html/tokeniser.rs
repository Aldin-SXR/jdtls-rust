//! Port of jsoup 1.19.1 `CharacterReader`, `Token`, `Tokeniser` and `TokeniserState`.

use super::entities::{BASE, FULL};
use super::Attr;

pub const EOF: char = '\u{FFFF}';
const NULL_CHAR: char = '\u{0000}';
const REPLACEMENT: char = '\u{FFFD}';

// ─── CharacterReader ─────────────────────────────────────────────────────────

pub struct Reader {
    buf: Vec<char>,
    pos: usize,
    mark: Option<usize>,
}

impl Reader {
    pub fn new(s: &str) -> Self {
        Reader { buf: s.chars().collect(), pos: 0, mark: None }
    }
    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }
    pub fn current(&self) -> char {
        if self.is_empty() { EOF } else { self.buf[self.pos] }
    }
    pub fn consume(&mut self) -> char {
        let c = self.current();
        self.pos += 1;
        c
    }
    pub fn unconsume(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }
    pub fn advance(&mut self) {
        self.pos += 1;
    }
    fn mark(&mut self) {
        self.mark = Some(self.pos);
    }
    fn unmark(&mut self) {
        self.mark = None;
    }
    fn rewind_to_mark(&mut self) {
        if let Some(m) = self.mark {
            self.pos = m;
        }
        self.mark = None;
    }
    fn slice(&self, from: usize, to: usize) -> String {
        let len = self.buf.len();
        self.buf[from.min(len)..to.min(len).max(from.min(len))].iter().collect()
    }
    fn consume_while(&mut self, f: impl Fn(char) -> bool) -> String {
        if self.pos > self.buf.len() {
            self.pos = self.buf.len();
        }
        let start = self.pos;
        while self.pos < self.buf.len() && f(self.buf[self.pos]) {
            self.pos += 1;
        }
        self.slice(start, self.pos)
    }
    pub fn consume_to(&mut self, c: char) -> String {
        self.consume_while(|x| x != c)
    }
    fn consume_to_seq(&mut self, seq: &str) -> String {
        let s: Vec<char> = seq.chars().collect();
        let start = self.pos;
        let mut i = self.pos;
        while i + s.len() <= self.buf.len() {
            if self.buf[i..i + s.len()] == s[..] {
                self.pos = i;
                return self.slice(start, i);
            }
            i += 1;
        }
        self.pos = self.buf.len();
        self.slice(start, self.pos)
    }
    fn consume_to_any(&mut self, chars: &[char]) -> String {
        self.consume_while(|x| !chars.contains(&x))
    }
    fn consume_data(&mut self) -> String {
        self.consume_while(|x| x != '&' && x != '<' && x != NULL_CHAR)
    }
    fn consume_attribute_quoted(&mut self, single: bool) -> String {
        self.consume_while(|x| !(x == '&' || x == NULL_CHAR || (single && x == '\'') || (!single && x == '"')))
    }
    fn consume_raw_data(&mut self) -> String {
        self.consume_while(|x| x != '<' && x != NULL_CHAR)
    }
    fn consume_tag_name(&mut self) -> String {
        self.consume_while(|x| !matches!(x, '\t' | '\n' | '\r' | '\u{000C}' | ' ' | '/' | '>'))
    }
    fn consume_letter_sequence(&mut self) -> String {
        self.consume_while(is_letter)
    }
    fn consume_letter_then_digit_sequence(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.buf.len() && is_letter(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < self.buf.len() && self.buf[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        self.slice(start, self.pos)
    }
    fn consume_hex_sequence(&mut self) -> String {
        self.consume_while(|c| c.is_ascii_hexdigit())
    }
    fn consume_digit_sequence(&mut self) -> String {
        self.consume_while(|c| c.is_ascii_digit())
    }
    pub fn matches(&self, c: char) -> bool {
        !self.is_empty() && self.buf[self.pos] == c
    }
    fn matches_seq(&self, seq: &str) -> bool {
        let s: Vec<char> = seq.chars().collect();
        self.pos + s.len() <= self.buf.len() && self.buf[self.pos..self.pos + s.len()] == s[..]
    }
    fn matches_ignore_case(&self, seq: &str) -> bool {
        let s: Vec<char> = seq.chars().collect();
        if self.pos + s.len() > self.buf.len() {
            return false;
        }
        s.iter().zip(&self.buf[self.pos..]).all(|(a, b)| a.to_uppercase().eq(b.to_uppercase()))
    }
    fn matches_any(&self, chars: &[char]) -> bool {
        !self.is_empty() && chars.contains(&self.buf[self.pos])
    }
    fn matches_letter(&self) -> bool {
        !self.is_empty() && is_letter(self.buf[self.pos])
    }
    fn matches_ascii_alpha(&self) -> bool {
        !self.is_empty() && self.buf[self.pos].is_ascii_alphabetic()
    }
    fn matches_digit(&self) -> bool {
        !self.is_empty() && self.buf[self.pos].is_ascii_digit()
    }
    pub fn match_consume(&mut self, seq: &str) -> bool {
        if self.matches_seq(seq) {
            self.pos += seq.chars().count();
            true
        } else {
            false
        }
    }
    fn match_consume_ignore_case(&mut self, seq: &str) -> bool {
        if self.matches_ignore_case(seq) {
            self.pos += seq.chars().count();
            true
        } else {
            false
        }
    }
    fn contains_ignore_case(&self, seq: &str) -> bool {
        let rest: String = self.buf[self.pos..].iter().collect();
        rest.to_lowercase().contains(&seq.to_lowercase())
    }
}

fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || c.is_alphabetic()
}

// ─── Tokens ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct Tag {
    pub name: String,
    pub normal: String,
    pub self_closing: bool,
    pub attrs: Option<Vec<Attr>>,
    attr_name: String,
    has_attr_name: bool,
    attr_value: String,
    has_attr_value: bool,
    has_empty_attr_value: bool,
}

impl Tag {
    pub fn named(name: &str) -> Self {
        Tag { name: name.to_string(), normal: name.to_lowercase(), ..Default::default() }
    }
    fn append_tag_name(&mut self, s: &str) {
        let s = s.replace(NULL_CHAR, &REPLACEMENT.to_string());
        self.normal.push_str(&s.to_lowercase());
        self.name.push_str(&s);
    }
    fn new_attribute(&mut self) {
        if self.attrs.is_none() {
            self.attrs = Some(Vec::new());
        }
        if self.has_attr_name {
            let name = self.attr_name.trim().to_string();
            if !name.is_empty() {
                let value = if self.has_attr_value {
                    Some(self.attr_value.clone())
                } else if self.has_empty_attr_value {
                    Some(String::new())
                } else {
                    None
                };
                let attrs = self.attrs.as_mut().unwrap();
                if attrs.len() < 512 {
                    attrs.push(Attr { key: name, value });
                }
            }
        }
        self.attr_name.clear();
        self.has_attr_name = false;
        self.attr_value.clear();
        self.has_attr_value = false;
        self.has_empty_attr_value = false;
    }
    fn append_attribute_name(&mut self, s: &str) {
        self.has_attr_name = true;
        self.attr_name.push_str(&s.replace(NULL_CHAR, &REPLACEMENT.to_string()));
    }
    fn append_attribute_name_char(&mut self, c: char) {
        self.has_attr_name = true;
        self.attr_name.push(c);
    }
    fn append_attribute_value(&mut self, s: &str) {
        self.has_attr_value = true;
        self.attr_value.push_str(s);
    }
    fn append_attribute_value_char(&mut self, c: char) {
        self.has_attr_value = true;
        self.attr_value.push(c);
    }
    fn set_empty_attribute_value(&mut self) {
        self.has_empty_attr_value = true;
    }
    fn finalise(&mut self) {
        if self.has_attr_name {
            self.new_attribute();
        }
    }
    pub fn attr(&self, key: &str) -> Option<String> {
        self.attrs.as_ref()?.iter().find(|a| a.key == key).map(|a| a.value.clone().unwrap_or_default())
    }
    pub fn has_attr_ignore_case(&self, key: &str) -> bool {
        self.attrs.as_ref().map(|a| a.iter().any(|x| x.key.eq_ignore_ascii_case(key))).unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
pub enum Token {
    Doctype { name: String, public_id: String, force_quirks: bool },
    StartTag(Tag),
    EndTag(Tag),
    Comment(String),
    Character { data: String, cdata: bool },
    Eof,
}

impl Token {
    pub fn is_whitespace(&self) -> bool {
        match self {
            Token::Character { data, .. } => super::is_blank(data),
            _ => false,
        }
    }
    /// `Token.toString()`, used when jsoup inserts tokens as text.
    pub fn to_source(&self) -> String {
        match self {
            Token::StartTag(t) => format!("<{}>", t.name),
            Token::EndTag(t) => format!("</{}>", t.name),
            Token::Comment(d) => format!("<!--{}-->", d),
            Token::Character { data, .. } => data.clone(),
            Token::Doctype { name, .. } => format!("<!doctype {}>", name),
            Token::Eof => String::new(),
        }
    }
}

// ─── Tokeniser ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Data,
    CharacterReferenceInData,
    Rcdata,
    CharacterReferenceInRcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessthanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessthanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessthanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessthanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessthanSign,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    CdataSection,
}

pub struct Tokeniser {
    pub reader: Reader,
    state: State,
    emit_pending: Option<Token>,
    chars: Option<String>,
    tag_pending: Tag,
    tag_is_start: bool,
    comment_pending: String,
    data_buffer: String,
    last_start_tag: Option<String>,
}

const WIN1252: [u32; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
    0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x009D, 0x017E, 0x0178,
];

fn full_lookup(name: &str) -> Option<(u32, u32)> {
    FULL.binary_search_by(|(n, _, _)| n.cmp(&name)).ok().map(|i| (FULL[i].1, FULL[i].2))
}

fn is_base_named(name: &str) -> bool {
    BASE.binary_search(&name).is_ok()
}

fn find_prefix(input: &str) -> Option<&'static str> {
    let mut sorted: Vec<&'static str> = BASE.to_vec();
    sorted.sort_by(|a, b| b.len().cmp(&a.len())); // stable, like Java List.sort
    sorted.into_iter().find(|n| input.starts_with(n))
}

fn push_cp(s: &mut String, cp: u32) {
    s.push(char::from_u32(cp).unwrap_or(REPLACEMENT));
}

impl Tokeniser {
    pub fn new(input: &str) -> Self {
        Tokeniser {
            reader: Reader::new(input),
            state: State::Data,
            emit_pending: None,
            chars: None,
            tag_pending: Tag::default(),
            tag_is_start: true,
            comment_pending: String::new(),
            data_buffer: String::new(),
            last_start_tag: None,
        }
    }

    pub fn read(&mut self) -> Token {
        while self.emit_pending.is_none() {
            self.step();
        }
        if let Some(c) = self.chars.take() {
            return Token::Character { data: c, cdata: false };
        }
        self.emit_pending.take().unwrap()
    }

    /// Emits a token (used by the tree builder for self-closing tags).
    pub fn emit(&mut self, t: Token) {
        if let Token::StartTag(tag) = &t {
            self.last_start_tag = Some(tag.name.clone());
        }
        self.emit_pending = Some(t);
    }

    fn emit_str(&mut self, s: &str) {
        match &mut self.chars {
            Some(c) => c.push_str(s),
            None => self.chars = Some(s.to_string()),
        }
    }

    fn emit_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        let s = c.encode_utf8(&mut b);
        self.emit_str(s);
    }

    pub fn transition(&mut self, s: State) {
        self.state = s;
    }

    fn advance_transition(&mut self, s: State) {
        self.state = s;
        self.reader.advance();
    }

    fn create_tag_pending(&mut self, start: bool) {
        self.tag_pending = Tag::default();
        self.tag_is_start = start;
    }

    fn emit_tag_pending(&mut self) {
        self.tag_pending.finalise();
        let tag = std::mem::take(&mut self.tag_pending);
        let tok = if self.tag_is_start { Token::StartTag(tag) } else { Token::EndTag(tag) };
        self.emit(tok);
    }

    fn emit_comment_pending(&mut self) {
        let c = std::mem::take(&mut self.comment_pending);
        self.emit(Token::Comment(c));
    }

    fn is_appropriate_end_tag_token(&self) -> bool {
        match &self.last_start_tag {
            Some(l) => self.tag_pending.name.eq_ignore_ascii_case(l),
            None => false,
        }
    }

    fn consume_character_reference(&mut self, additional: Option<char>, in_attribute: bool) -> Option<String> {
        let r = &mut self.reader;
        if r.is_empty() {
            return None;
        }
        if let Some(a) = additional {
            if a == r.current() {
                return None;
            }
        }
        if r.matches_any(&['\t', '\n', '\r', '\u{000C}', ' ', '<', '&']) {
            return None;
        }
        r.mark();
        if r.match_consume("#") {
            let is_hex = r.match_consume_ignore_case("X");
            let num = if is_hex { r.consume_hex_sequence() } else { r.consume_digit_sequence() };
            if num.is_empty() {
                r.rewind_to_mark();
                return None;
            }
            r.unmark();
            r.match_consume(";");
            let charval = i64::from_str_radix(&num, if is_hex { 16 } else { 10 }).unwrap_or(-1);
            let mut out = String::new();
            if charval == -1 || charval > 0x10FFFF {
                out.push(REPLACEMENT);
            } else {
                let mut cv = charval as u32;
                if (0x80..0x80 + 32).contains(&cv) {
                    cv = WIN1252[(cv - 0x80) as usize];
                }
                push_cp(&mut out, cv);
            }
            Some(out)
        } else {
            let mut name = r.consume_letter_then_digit_sequence();
            let looks_legit = r.matches(';');
            let found = is_base_named(&name) || (full_lookup(&name).is_some() && looks_legit);
            if !found {
                r.rewind_to_mark();
                if in_attribute {
                    return None;
                }
                let prefix = find_prefix(&name)?;
                r.match_consume(prefix);
                name = prefix.to_string();
            }
            if in_attribute && (r.matches_letter() || r.matches_digit() || r.matches_any(&['=', '-', '_'])) {
                r.rewind_to_mark();
                return None;
            }
            r.unmark();
            r.match_consume(";");
            let (c1, c2) = full_lookup(&name)?;
            let mut out = String::new();
            push_cp(&mut out, c1);
            if c2 != 0 {
                push_cp(&mut out, c2);
            }
            Some(out)
        }
    }

    fn read_char_ref(&mut self, advance: State) {
        match self.consume_character_reference(None, false) {
            None => self.emit_char('&'),
            Some(s) => self.emit_str(&s),
        }
        self.transition(advance);
    }

    fn read_raw_data(&mut self, current: State, advance: State) {
        let _ = current;
        match self.reader.current() {
            '<' => self.advance_transition(advance),
            NULL_CHAR => {
                self.reader.advance();
                self.emit_char(REPLACEMENT);
            }
            EOF if self.reader.is_empty() => self.emit(Token::Eof),
            _ => {
                let d = self.reader.consume_raw_data();
                self.emit_str(&d);
            }
        }
    }

    fn read_end_tag(&mut self, a: State, b: State) {
        if self.reader.matches_ascii_alpha() {
            self.create_tag_pending(false);
            self.transition(a);
        } else {
            self.emit_str("</");
            self.transition(b);
        }
    }

    fn handle_data_end_tag(&mut self, else_transition: State) {
        if self.reader.matches_letter() {
            let name = self.reader.consume_letter_sequence();
            self.tag_pending.append_tag_name(&name);
            self.data_buffer.push_str(&name);
            return;
        }
        let mut needs_exit = false;
        if self.is_appropriate_end_tag_token() && !self.reader.is_empty() {
            let c = self.reader.consume();
            match c {
                '\t' | '\n' | '\r' | '\u{000C}' | ' ' => self.transition(State::BeforeAttributeName),
                '/' => self.transition(State::SelfClosingStartTag),
                '>' => {
                    self.emit_tag_pending();
                    self.transition(State::Data);
                }
                _ => {
                    self.data_buffer.push(c);
                    needs_exit = true;
                }
            }
        } else {
            needs_exit = true;
        }
        if needs_exit {
            self.emit_str("</");
            let db = self.data_buffer.clone();
            self.emit_str(&db);
            self.transition(else_transition);
        }
    }

    fn handle_data_double_escape_tag(&mut self, primary: State, fallback: State) {
        if self.reader.matches_letter() {
            let name = self.reader.consume_letter_sequence();
            self.data_buffer.push_str(&name);
            self.emit_str(&name);
            return;
        }
        let c = self.reader.consume();
        match c {
            '\t' | '\n' | '\r' | '\u{000C}' | ' ' | '/' | '>' => {
                if self.data_buffer == "script" {
                    self.transition(primary);
                } else {
                    self.transition(fallback);
                }
                self.emit_char(c);
            }
            _ => {
                self.reader.unconsume();
                self.transition(fallback);
            }
        }
    }

    /// consume() that maps "past the end" to EOF.
    fn consume(&mut self) -> char {
        if self.reader.is_empty() {
            self.reader.advance();
            EOF
        } else {
            self.reader.consume()
        }
    }

    fn cur(&self) -> char {
        self.reader.current()
    }

    fn at_eof(&self) -> bool {
        self.reader.is_empty()
    }

    fn step(&mut self) {
        use State::*;
        match self.state {
            Data => match self.cur() {
                _ if self.at_eof() => self.emit(Token::Eof),
                '&' => self.advance_transition(CharacterReferenceInData),
                '<' => self.advance_transition(TagOpen),
                NULL_CHAR => {
                    let c = self.reader.consume();
                    self.emit_char(c);
                }
                _ => {
                    let d = self.reader.consume_data();
                    self.emit_str(&d);
                }
            },
            CharacterReferenceInData => self.read_char_ref(Data),
            Rcdata => match self.cur() {
                _ if self.at_eof() => self.emit(Token::Eof),
                '&' => self.advance_transition(CharacterReferenceInRcdata),
                '<' => self.advance_transition(RcdataLessthanSign),
                NULL_CHAR => {
                    self.reader.advance();
                    self.emit_char(REPLACEMENT);
                }
                _ => {
                    let d = self.reader.consume_data();
                    self.emit_str(&d);
                }
            },
            CharacterReferenceInRcdata => self.read_char_ref(Rcdata),
            Rawtext => self.read_raw_data(Rawtext, RawtextLessthanSign),
            ScriptData => self.read_raw_data(ScriptData, ScriptDataLessthanSign),
            Plaintext => match self.cur() {
                _ if self.at_eof() => self.emit(Token::Eof),
                NULL_CHAR => {
                    self.reader.advance();
                    self.emit_char(REPLACEMENT);
                }
                _ => {
                    let d = self.reader.consume_to(NULL_CHAR);
                    self.emit_str(&d);
                }
            },
            TagOpen => match self.cur() {
                '!' if !self.at_eof() => self.advance_transition(MarkupDeclarationOpen),
                '/' if !self.at_eof() => self.advance_transition(EndTagOpen),
                '?' if !self.at_eof() => {
                    self.comment_pending.clear();
                    self.transition(BogusComment);
                }
                _ => {
                    if self.reader.matches_ascii_alpha() {
                        self.create_tag_pending(true);
                        self.transition(TagName);
                    } else {
                        self.emit_char('<');
                        self.transition(Data);
                    }
                }
            },
            EndTagOpen => {
                if self.at_eof() {
                    self.emit_str("</");
                    self.transition(Data);
                } else if self.reader.matches_ascii_alpha() {
                    self.create_tag_pending(false);
                    self.transition(TagName);
                } else if self.reader.matches('>') {
                    self.advance_transition(Data);
                } else {
                    self.comment_pending.clear();
                    self.comment_pending.push('/');
                    self.transition(BogusComment);
                }
            }
            TagName => {
                let name = self.reader.consume_tag_name();
                self.tag_pending.append_tag_name(&name);
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => self.transition(BeforeAttributeName),
                    '/' => self.transition(SelfClosingStartTag),
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => self.tag_pending.append_tag_name(&REPLACEMENT.to_string()),
                    _ => self.tag_pending.append_tag_name(&c.to_string()),
                }
            }
            RcdataLessthanSign => {
                if self.reader.matches('/') {
                    self.data_buffer.clear();
                    self.advance_transition(RcdataEndTagOpen);
                } else if self.reader.matches_ascii_alpha()
                    && self.last_start_tag.is_some()
                    && !self.reader.contains_ignore_case(&format!("</{}", self.last_start_tag.as_ref().unwrap()))
                {
                    let name = self.last_start_tag.clone().unwrap();
                    self.create_tag_pending(false);
                    self.tag_pending = Tag::named(&name);
                    self.emit_tag_pending();
                    self.transition(TagOpen);
                } else {
                    self.emit_str("<");
                    self.transition(Rcdata);
                }
            }
            RcdataEndTagOpen => {
                if self.reader.matches_ascii_alpha() {
                    self.create_tag_pending(false);
                    let c = self.cur();
                    self.tag_pending.append_tag_name(&c.to_string());
                    self.data_buffer.push(c);
                    self.advance_transition(RcdataEndTagName);
                } else {
                    self.emit_str("</");
                    self.transition(Rcdata);
                }
            }
            RcdataEndTagName => {
                if self.reader.matches_ascii_alpha() {
                    let name = self.reader.consume_letter_sequence();
                    self.tag_pending.append_tag_name(&name);
                    self.data_buffer.push_str(&name);
                    return;
                }
                let eof = self.at_eof();
                let c = self.consume();
                let appropriate = self.is_appropriate_end_tag_token();
                match c {
                    _ if eof => self.rcdata_anything_else(),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' if appropriate => self.transition(BeforeAttributeName),
                    '/' if appropriate => self.transition(SelfClosingStartTag),
                    '>' if appropriate => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    _ => self.rcdata_anything_else(),
                }
            }
            RawtextLessthanSign => {
                if self.reader.matches('/') {
                    self.data_buffer.clear();
                    self.advance_transition(RawtextEndTagOpen);
                } else {
                    self.emit_char('<');
                    self.transition(Rawtext);
                }
            }
            RawtextEndTagOpen => self.read_end_tag(RawtextEndTagName, Rawtext),
            RawtextEndTagName => self.handle_data_end_tag(Rawtext),
            ScriptDataLessthanSign => {
                let eof = self.at_eof();
                match self.consume() {
                    _ if eof => {
                        self.emit_str("<");
                        self.transition(Data);
                    }
                    '/' => {
                        self.data_buffer.clear();
                        self.transition(ScriptDataEndTagOpen);
                    }
                    '!' => {
                        self.emit_str("<!");
                        self.transition(ScriptDataEscapeStart);
                    }
                    _ => {
                        self.emit_str("<");
                        self.reader.unconsume();
                        self.transition(ScriptData);
                    }
                }
            }
            ScriptDataEndTagOpen => self.read_end_tag(ScriptDataEndTagName, ScriptData),
            ScriptDataEndTagName => self.handle_data_end_tag(ScriptData),
            ScriptDataEscapeStart => {
                if self.reader.matches('-') {
                    self.emit_char('-');
                    self.advance_transition(ScriptDataEscapeStartDash);
                } else {
                    self.transition(ScriptData);
                }
            }
            ScriptDataEscapeStartDash => {
                if self.reader.matches('-') {
                    self.emit_char('-');
                    self.advance_transition(ScriptDataEscapedDashDash);
                } else {
                    self.transition(ScriptData);
                }
            }
            ScriptDataEscaped => {
                if self.at_eof() {
                    self.transition(Data);
                    return;
                }
                match self.cur() {
                    '-' => {
                        self.emit_char('-');
                        self.advance_transition(ScriptDataEscapedDash);
                    }
                    '<' => self.advance_transition(ScriptDataEscapedLessthanSign),
                    NULL_CHAR => {
                        self.reader.advance();
                        self.emit_char(REPLACEMENT);
                    }
                    _ => {
                        let d = self.reader.consume_to_any(&['-', '<', NULL_CHAR]);
                        self.emit_str(&d);
                    }
                }
            }
            ScriptDataEscapedDash | ScriptDataEscapedDashDash => {
                if self.at_eof() {
                    self.transition(Data);
                    return;
                }
                let dashdash = self.state == ScriptDataEscapedDashDash;
                let c = self.consume();
                match c {
                    '-' => {
                        self.emit_char(c);
                        if !dashdash {
                            self.transition(ScriptDataEscapedDashDash);
                        }
                    }
                    '<' => self.transition(ScriptDataEscapedLessthanSign),
                    '>' if dashdash => {
                        self.emit_char(c);
                        self.transition(ScriptData);
                    }
                    NULL_CHAR => {
                        self.emit_char(REPLACEMENT);
                        self.transition(ScriptDataEscaped);
                    }
                    _ => {
                        self.emit_char(c);
                        self.transition(ScriptDataEscaped);
                    }
                }
            }
            ScriptDataEscapedLessthanSign => {
                if self.reader.matches_ascii_alpha() {
                    self.data_buffer.clear();
                    let c = self.cur();
                    self.data_buffer.push(c);
                    self.emit_str("<");
                    self.emit_char(c);
                    self.advance_transition(ScriptDataDoubleEscapeStart);
                } else if self.reader.matches('/') {
                    self.data_buffer.clear();
                    self.advance_transition(ScriptDataEscapedEndTagOpen);
                } else {
                    self.emit_char('<');
                    self.transition(ScriptDataEscaped);
                }
            }
            ScriptDataEscapedEndTagOpen => {
                if self.reader.matches_ascii_alpha() {
                    self.create_tag_pending(false);
                    let c = self.cur();
                    self.tag_pending.append_tag_name(&c.to_string());
                    self.data_buffer.push(c);
                    self.advance_transition(ScriptDataEscapedEndTagName);
                } else {
                    self.emit_str("</");
                    self.transition(ScriptDataEscaped);
                }
            }
            ScriptDataEscapedEndTagName => self.handle_data_end_tag(ScriptDataEscaped),
            ScriptDataDoubleEscapeStart => self.handle_data_double_escape_tag(ScriptDataDoubleEscaped, ScriptDataEscaped),
            ScriptDataDoubleEscaped => match self.cur() {
                _ if self.at_eof() => self.transition(Data),
                '-' => {
                    self.emit_char('-');
                    self.advance_transition(ScriptDataDoubleEscapedDash);
                }
                '<' => {
                    self.emit_char('<');
                    self.advance_transition(ScriptDataDoubleEscapedLessthanSign);
                }
                NULL_CHAR => {
                    self.reader.advance();
                    self.emit_char(REPLACEMENT);
                }
                _ => {
                    let d = self.reader.consume_to_any(&['-', '<', NULL_CHAR]);
                    self.emit_str(&d);
                }
            },
            ScriptDataDoubleEscapedDash | ScriptDataDoubleEscapedDashDash => {
                let dashdash = self.state == ScriptDataDoubleEscapedDashDash;
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '-' => {
                        self.emit_char(c);
                        if !dashdash {
                            self.transition(ScriptDataDoubleEscapedDashDash);
                        }
                    }
                    '<' => {
                        self.emit_char(c);
                        self.transition(ScriptDataDoubleEscapedLessthanSign);
                    }
                    '>' if dashdash => {
                        self.emit_char(c);
                        self.transition(ScriptData);
                    }
                    NULL_CHAR => {
                        self.emit_char(REPLACEMENT);
                        self.transition(ScriptDataDoubleEscaped);
                    }
                    _ => {
                        self.emit_char(c);
                        self.transition(ScriptDataDoubleEscaped);
                    }
                }
            }
            ScriptDataDoubleEscapedLessthanSign => {
                if self.reader.matches('/') {
                    self.emit_char('/');
                    self.data_buffer.clear();
                    self.advance_transition(ScriptDataDoubleEscapeEnd);
                } else {
                    self.transition(ScriptDataDoubleEscaped);
                }
            }
            ScriptDataDoubleEscapeEnd => self.handle_data_double_escape_tag(ScriptDataEscaped, ScriptDataDoubleEscaped),
            BeforeAttributeName => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => {}
                    '/' => self.transition(SelfClosingStartTag),
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => {
                        self.reader.unconsume();
                        self.tag_pending.new_attribute();
                        self.transition(AttributeName);
                    }
                    '"' | '\'' | '=' => {
                        self.tag_pending.new_attribute();
                        self.tag_pending.append_attribute_name_char(c);
                        self.transition(AttributeName);
                    }
                    _ => {
                        self.tag_pending.new_attribute();
                        self.reader.unconsume();
                        self.transition(AttributeName);
                    }
                }
            }
            AttributeName => {
                let name = self.reader.consume_to_any(&['\t', '\n', '\u{000C}', '\r', ' ', '"', '\'', '/', '<', '=', '>', '?']);
                self.tag_pending.append_attribute_name(&name);
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => self.transition(AfterAttributeName),
                    '/' => self.transition(SelfClosingStartTag),
                    '=' => self.transition(BeforeAttributeValue),
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    _ => self.tag_pending.append_attribute_name_char(c),
                }
            }
            AfterAttributeName => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => {}
                    '/' => self.transition(SelfClosingStartTag),
                    '=' => self.transition(BeforeAttributeValue),
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => {
                        self.tag_pending.append_attribute_name_char(REPLACEMENT);
                        self.transition(AttributeName);
                    }
                    '"' | '\'' | '<' => {
                        self.tag_pending.new_attribute();
                        self.tag_pending.append_attribute_name_char(c);
                        self.transition(AttributeName);
                    }
                    _ => {
                        self.tag_pending.new_attribute();
                        self.reader.unconsume();
                        self.transition(AttributeName);
                    }
                }
            }
            BeforeAttributeValue => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => {}
                    '"' => self.transition(AttributeValueDoubleQuoted),
                    '&' => {
                        self.reader.unconsume();
                        self.transition(AttributeValueUnquoted);
                    }
                    '\'' => self.transition(AttributeValueSingleQuoted),
                    NULL_CHAR => {
                        self.tag_pending.append_attribute_value_char(REPLACEMENT);
                        self.transition(AttributeValueUnquoted);
                    }
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    '<' | '=' | '`' => {
                        self.tag_pending.append_attribute_value_char(c);
                        self.transition(AttributeValueUnquoted);
                    }
                    _ => {
                        self.reader.unconsume();
                        self.transition(AttributeValueUnquoted);
                    }
                }
            }
            AttributeValueDoubleQuoted | AttributeValueSingleQuoted => {
                let single = self.state == AttributeValueSingleQuoted;
                let quote = if single { '\'' } else { '"' };
                let value = self.reader.consume_attribute_quoted(single);
                if !value.is_empty() {
                    self.tag_pending.append_attribute_value(&value);
                } else {
                    self.tag_pending.set_empty_attribute_value();
                }
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '&' => match self.consume_character_reference(Some(quote), true) {
                        Some(s) => self.tag_pending.append_attribute_value(&s),
                        None => self.tag_pending.append_attribute_value_char('&'),
                    },
                    NULL_CHAR => self.tag_pending.append_attribute_value_char(REPLACEMENT),
                    _ if c == quote => self.transition(AfterAttributeValueQuoted),
                    _ => self.tag_pending.append_attribute_value_char(c),
                }
            }
            AttributeValueUnquoted => {
                let value = self.reader.consume_to_any(&[NULL_CHAR, '\t', '\n', '\u{000C}', '\r', ' ', '"', '&', '\'', '<', '=', '>', '`']);
                if !value.is_empty() {
                    self.tag_pending.append_attribute_value(&value);
                }
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => self.transition(BeforeAttributeName),
                    '&' => match self.consume_character_reference(Some('>'), true) {
                        Some(s) => self.tag_pending.append_attribute_value(&s),
                        None => self.tag_pending.append_attribute_value_char('&'),
                    },
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => self.tag_pending.append_attribute_value_char(REPLACEMENT),
                    _ => self.tag_pending.append_attribute_value_char(c),
                }
            }
            AfterAttributeValueQuoted => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '\t' | '\n' | '\r' | '\u{000C}' | ' ' => self.transition(BeforeAttributeName),
                    '/' => self.transition(SelfClosingStartTag),
                    '>' => {
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    _ => {
                        self.reader.unconsume();
                        self.transition(BeforeAttributeName);
                    }
                }
            }
            SelfClosingStartTag => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => self.transition(Data),
                    '>' => {
                        self.tag_pending.self_closing = true;
                        self.emit_tag_pending();
                        self.transition(Data);
                    }
                    _ => {
                        self.reader.unconsume();
                        self.transition(BeforeAttributeName);
                    }
                }
            }
            BogusComment => {
                let s = self.reader.consume_to('>');
                self.comment_pending.push_str(&s);
                if self.cur() == '>' || self.at_eof() {
                    self.consume();
                    self.emit_comment_pending();
                    self.transition(Data);
                }
            }
            MarkupDeclarationOpen => {
                if self.reader.match_consume("--") {
                    self.comment_pending.clear();
                    self.transition(CommentStart);
                } else if self.reader.match_consume_ignore_case("DOCTYPE") {
                    self.transition(Doctype);
                } else if self.reader.match_consume("[CDATA[") {
                    self.data_buffer.clear();
                    self.transition(CdataSection);
                } else {
                    self.comment_pending.clear();
                    self.transition(BogusComment);
                }
            }
            CommentStart | CommentStartDash => {
                let dash = self.state == CommentStartDash;
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    '-' => self.transition(if dash { CommentEnd } else { CommentStartDash }),
                    NULL_CHAR => {
                        self.comment_pending.push(REPLACEMENT);
                        self.transition(Comment);
                    }
                    '>' => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    _ => {
                        if dash {
                            self.comment_pending.push(c);
                        } else {
                            self.reader.unconsume();
                        }
                        self.transition(Comment);
                    }
                }
            }
            Comment => match self.cur() {
                _ if self.at_eof() => {
                    self.emit_comment_pending();
                    self.transition(Data);
                }
                '-' => self.advance_transition(CommentEndDash),
                NULL_CHAR => {
                    self.reader.advance();
                    self.comment_pending.push(REPLACEMENT);
                }
                _ => {
                    let s = self.reader.consume_to_any(&['-', NULL_CHAR]);
                    self.comment_pending.push_str(&s);
                }
            },
            CommentEndDash => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    '-' => self.transition(CommentEnd),
                    NULL_CHAR => {
                        self.comment_pending.push('-');
                        self.comment_pending.push(REPLACEMENT);
                        self.transition(Comment);
                    }
                    _ => {
                        self.comment_pending.push('-');
                        self.comment_pending.push(c);
                        self.transition(Comment);
                    }
                }
            }
            CommentEnd => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    '>' => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => {
                        self.comment_pending.push_str("--");
                        self.comment_pending.push(REPLACEMENT);
                        self.transition(Comment);
                    }
                    '!' => self.transition(CommentEndBang),
                    '-' => self.comment_pending.push('-'),
                    _ => {
                        self.comment_pending.push_str("--");
                        self.comment_pending.push(c);
                        self.transition(Comment);
                    }
                }
            }
            CommentEndBang => {
                let eof = self.at_eof();
                let c = self.consume();
                match c {
                    _ if eof => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    '-' => {
                        self.comment_pending.push_str("--!");
                        self.transition(CommentEndDash);
                    }
                    '>' => {
                        self.emit_comment_pending();
                        self.transition(Data);
                    }
                    NULL_CHAR => {
                        self.comment_pending.push_str("--!");
                        self.comment_pending.push(REPLACEMENT);
                        self.transition(Comment);
                    }
                    _ => {
                        self.comment_pending.push_str("--!");
                        self.comment_pending.push(c);
                        self.transition(Comment);
                    }
                }
            }
            Doctype => {
                // Simplified doctype handling: read the name up to '>' (javadoc HTML has no doctypes).
                let raw = self.reader.consume_to('>');
                if !self.at_eof() {
                    self.reader.advance();
                }
                let trimmed = raw.trim();
                let name = trimmed.split_whitespace().next().unwrap_or("").to_lowercase();
                let rest = trimmed[trimmed.find(char::is_whitespace).unwrap_or(trimmed.len())..].trim().to_string();
                let force_quirks = name.is_empty();
                self.emit(Token::Doctype { name, public_id: rest, force_quirks });
                self.transition(Data);
            }
            CdataSection => {
                let data = self.reader.consume_to_seq("]]>");
                self.data_buffer.push_str(&data);
                if self.reader.match_consume("]]>") || self.at_eof() {
                    let d = std::mem::take(&mut self.data_buffer);
                    self.emit(Token::Character { data: d, cdata: true });
                    self.transition(Data);
                }
            }
        }
    }

    fn rcdata_anything_else(&mut self) {
        self.emit_str("</");
        let db = self.data_buffer.clone();
        self.emit_str(&db);
        self.reader.unconsume();
        self.transition(State::Rcdata);
    }
}
