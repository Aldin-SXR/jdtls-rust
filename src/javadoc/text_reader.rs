//! Port of jdt.core.manipulation `CoreJavaDoc2HTMLTextReader` (on top of
//! org.eclipse.text `SubstitutionReader`) together with the jdt.ls subclass
//! `AbstractJavaDocConverter.JdtLsJavaDoc2HTMLTextReader`.
//!
//! It turns raw Javadoc text (block tags `@param ...`, inline tags `{@code ...}`)
//! into HTML.

use super::html_builder::convert_to_html_content;

const EOF: i32 = -1;

/// Java `Character.isWhitespace`
pub fn java_is_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{001C}' | '\u{001D}' | '\u{001E}' | '\u{001F}')
        || (c.is_whitespace() && c != '\u{00A0}' && c != '\u{2007}' && c != '\u{202F}' && c != '\u{0085}')
}

fn java_is_letter(c: char) -> bool {
    c.is_alphabetic()
}

fn java_is_identifier_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || (c as u32) < 0x09 || ('\u{000E}'..='\u{001B}').contains(&c)
}

fn to_char(c: i32) -> char {
    if c < 0 {
        '\u{FFFF}'
    } else {
        char::from_u32(c as u32).unwrap_or('\u{FFFD}')
    }
}

fn java_trim(s: &str) -> String {
    s.trim_matches(|c: char| c <= ' ').to_string()
}

struct Pair {
    tag: Option<String>,
    content: Option<String>,
}

pub struct JavaDoc2HtmlTextReader {
    input: Vec<char>,
    pos: usize,
    // SubstitutionReader
    buffer: Vec<char>,
    index: usize,
    read_from_buffer: bool,
    was_white_space: bool,
    // CoreJavaDoc2HTMLTextReader
    parameters: Vec<String>,
    return_: Option<String>,
    exceptions: Vec<String>,
    authors: Vec<String>,
    sees: Vec<String>,
    since: Vec<String>,
    rest: Vec<Pair>,
    // JdtLsJavaDoc2HTMLTextReader
    pre_tag_depth: i32,
    code_tag_depth: i32,
    tag_buffer: String,
    in_tag: bool,
    check_next_char: bool,
    quote_char: char,
    in_comment: bool,
    comment_buffer: String,
}

impl JavaDoc2HtmlTextReader {
    pub fn new(input: &str) -> Self {
        JavaDoc2HtmlTextReader {
            input: input.chars().collect(),
            pos: 0,
            buffer: Vec::new(),
            index: 0,
            read_from_buffer: false,
            was_white_space: true,
            parameters: Vec::new(),
            return_: None,
            exceptions: Vec::new(),
            authors: Vec::new(),
            sees: Vec::new(),
            since: Vec::new(),
            rest: Vec::new(),
            pre_tag_depth: 0,
            code_tag_depth: 0,
            tag_buffer: String::new(),
            in_tag: false,
            check_next_char: false,
            quote_char: '\0',
            in_comment: false,
            comment_buffer: String::new(),
        }
    }

    /// `SingleCharacterReader.getString()`
    pub fn get_string(&mut self) -> String {
        let mut s = String::new();
        loop {
            let c = self.read();
            if c == EOF {
                break;
            }
            s.push(to_char(c));
        }
        s
    }

    fn reader_read(&mut self) -> i32 {
        if self.pos < self.input.len() {
            let c = self.input[self.pos];
            self.pos += 1;
            c as i32
        } else {
            EOF
        }
    }

    /// `SubstitutionReader.nextChar()` (skipWhitespace = false)
    fn next_char(&mut self) -> i32 {
        self.read_from_buffer = !self.buffer.is_empty();
        if self.read_from_buffer {
            let ch = self.buffer[self.index];
            self.index += 1;
            if self.index >= self.buffer.len() {
                self.buffer.clear();
                self.index = 0;
            }
            return ch as i32;
        }
        self.reader_read()
    }

    /// `SubstitutionReader.read()`
    fn read(&mut self) -> i32 {
        let mut c = self.next_char();
        while !self.read_from_buffer && c != EOF {
            match self.compute_substitution(c) {
                None => break,
                Some(s) => {
                    if !s.is_empty() {
                        let mut nb: Vec<char> = s.chars().collect();
                        nb.extend_from_slice(&self.buffer[self.index..]);
                        self.buffer = nb;
                        self.index = 0;
                    }
                }
            }
            c = self.next_char();
        }
        self.was_white_space = c == ' ' as i32 || c == '\r' as i32 || c == '\n' as i32;
        c
    }

    /// `JdtLsJavaDoc2HTMLTextReader.computeSubstitution`
    fn compute_substitution(&mut self, c: i32) -> Option<String> {
        let ch = to_char(c);
        if self.in_comment {
            self.comment_buffer.push(ch);
            if self.comment_buffer.chars().count() >= 3 && self.comment_buffer.ends_with("-->") {
                self.in_comment = false;
                self.comment_buffer.clear();
            }
            return self.super_compute_substitution(c);
        }
        if self.check_next_char {
            self.check_next_char = false;
            if ch == '!' {
                self.comment_buffer.clear();
                self.comment_buffer.push_str("<!");
            } else if java_is_letter(ch) || ch == '/' {
                self.in_tag = true;
                self.quote_char = '\0';
                self.tag_buffer.clear();
                self.tag_buffer.push('<');
                self.tag_buffer.push(ch);
            }
        } else if !self.comment_buffer.is_empty() {
            self.comment_buffer.push(ch);
            if self.comment_buffer == "<!--" {
                self.in_comment = true;
            } else if self.comment_buffer.chars().count() >= 4 || ch == '>' {
                self.comment_buffer.clear();
            }
        } else if self.in_tag {
            self.tag_buffer.push(ch);
            if self.quote_char == '\0' && (ch == '"' || ch == '\'') {
                self.quote_char = ch;
            } else if ch == self.quote_char {
                self.quote_char = '\0';
            } else if ch == '>' && self.quote_char == '\0' {
                self.in_tag = false;
                let tag = self.tag_buffer.to_lowercase();
                self.update_tag_depth(&tag);
            }
        } else if ch == '<' {
            self.check_next_char = true;
        }
        if (self.pre_tag_depth > 0 || self.code_tag_depth > 0) && (ch == '@' || ch == '{') {
            return None;
        }
        self.super_compute_substitution(c)
    }

    fn update_tag_depth(&mut self, tag: &str) {
        let chars: Vec<char> = tag.chars().collect();
        if tag.starts_with("<pre") && (tag == "<pre>" || chars.get(4) == Some(&' ')) {
            self.pre_tag_depth += 1;
        } else if tag == "</pre>" {
            self.pre_tag_depth = (self.pre_tag_depth - 1).max(0);
        } else if tag.starts_with("<code") && (tag == "<code>" || chars.get(5) == Some(&' ')) {
            self.code_tag_depth += 1;
        } else if tag == "</code>" {
            self.code_tag_depth = (self.code_tag_depth - 1).max(0);
        }
    }

    /// `CoreJavaDoc2HTMLTextReader.computeSubstitution`
    fn super_compute_substitution(&mut self, c: i32) -> Option<String> {
        if c == '@' as i32 && self.was_white_space {
            return Some(self.process_simple_tag());
        }
        if c == '{' as i32 {
            return self.process_block_tag();
        }
        None
    }

    fn get_tag(&mut self, buffer: &mut String) -> i32 {
        let mut c = self.next_char();
        while c == '.' as i32 || c != EOF && java_is_letter(to_char(c)) {
            buffer.push(to_char(c));
            c = self.next_char();
        }
        c
    }

    fn get_content(&mut self, buffer: &mut String, stop: char) -> i32 {
        let mut c = self.next_char();
        while c != EOF && c != stop as i32 {
            buffer.push(to_char(c));
            c = self.next_char();
        }
        c
    }

    fn get_content_until_next_tag(&mut self, buffer: &mut Vec<char>) -> i32 {
        let mut c = self.next_char();
        let mut block_start_read = false;
        while c != EOF {
            if c == '@' as i32 {
                let mut index = buffer.len() as isize;
                loop {
                    index -= 1;
                    if !(index >= 0 && java_is_whitespace(buffer[index as usize])) {
                        break;
                    }
                    match buffer[index as usize] {
                        '\n' | '\r' => return c,
                        _ => {}
                    }
                    if index <= 0 {
                        return c;
                    }
                }
            }
            if block_start_read {
                let s = self.process_block_tag();
                // StringBuilder.append((String) null) appends "null"
                buffer.extend(s.unwrap_or_else(|| "null".into()).chars());
            } else {
                buffer.push(to_char(c));
            }
            c = self.next_char();
            block_start_read = c == '{' as i32;
        }
        c
    }

    fn substitute_qualification(qualification: &str) -> String {
        let mut result: Vec<char> = qualification.chars().collect();
        if !qualification.contains("<a") {
            result = result.into_iter().map(|c| if c == '#' { '.' } else { c }).collect();
        } else {
            let length = result.len();
            let mut inside_tag = false;
            for i in 0..length {
                let ch = result[i];
                if ch == '<' && result.get(i + 1) == Some(&'a') {
                    inside_tag = true;
                }
                if ch == '>' {
                    inside_tag = false;
                }
                if ch == '#' && !inside_tag {
                    result[i] = '.';
                }
            }
        }
        let mut s: String = result.into_iter().collect();
        if s.starts_with('.') {
            s = s[1..].to_string();
        }
        s
    }

    fn get_param_end_offset(s: &[char]) -> usize {
        let mut i = 0;
        let length = s.len();
        while i < length && java_is_whitespace(s[i]) {
            i += 1;
        }
        if i < length && s[i] == '<' {
            while i < length && java_is_whitespace(s[i]) {
                i += 1;
            }
            while i < length && java_is_identifier_part(s[i]) {
                i += 1;
            }
            while i < length && s[i] != '>' {
                i += 1;
            }
        } else {
            while i < length && java_is_identifier_part(s[i]) {
                i += 1;
            }
        }
        i
    }

    fn print_definitions(buffer: &mut String, list: &[String], firstword: bool) {
        for s in list {
            buffer.push_str("<li>");
            if !firstword {
                buffer.push_str(s);
            } else {
                buffer.push_str("<b>");
                let chars: Vec<char> = s.chars().collect();
                let i = Self::get_param_end_offset(&chars);
                if i <= chars.len() {
                    let head: String = chars[..i].iter().collect();
                    let tail: String = chars[i..].iter().collect();
                    buffer.push_str(&convert_to_html_content(&head));
                    buffer.push_str("</b>");
                    buffer.push_str(&tail);
                } else {
                    buffer.push_str("</b>");
                }
            }
            buffer.push_str("</li>");
        }
    }

    fn print_list(buffer: &mut String, tag: &str, elements: &[String], firstword: bool) {
        if !elements.is_empty() {
            buffer.push_str("<li><b>");
            buffer.push_str(tag);
            buffer.push_str("</b><ul>");
            Self::print_definitions(buffer, elements, firstword);
            buffer.push_str("</ul></li>");
        }
    }

    fn print_content(buffer: &mut String, tag: &str, content: &Option<String>) {
        if let Some(c) = content {
            buffer.push_str("<li><b>");
            buffer.push_str(tag);
            buffer.push_str("</b><ul><li>");
            buffer.push_str(c);
            buffer.push_str("</li></ul></li>");
        }
    }

    fn print_rest(&self, buffer: &mut String) {
        for p in &self.rest {
            buffer.push_str("<li>");
            if let Some(t) = &p.tag {
                buffer.push_str(t);
            }
            if let Some(c) = &p.content {
                buffer.push_str("<ul><li>");
                buffer.push_str(c);
                buffer.push_str("</li></ul>");
            }
            buffer.push_str("</li>");
        }
    }

    fn print_simple_tag(&self) -> String {
        let mut buffer = String::from("<ul>");
        Self::print_list(&mut buffer, "See Also:", &self.sees, false);
        Self::print_list(&mut buffer, "Parameters:", &self.parameters, true);
        Self::print_content(&mut buffer, "Returns:", &self.return_);
        Self::print_list(&mut buffer, "Throws:", &self.exceptions, false);
        Self::print_list(&mut buffer, "Author:", &self.authors, false);
        Self::print_list(&mut buffer, "Since:", &self.since, false);
        self.print_rest(&mut buffer);
        buffer.push_str("</ul>");
        buffer
    }

    fn handle_tag(&mut self, tag: &str, content: &str) {
        let content = java_trim(content);
        match tag {
            "@param" => self.parameters.push(content),
            "@return" => self.return_ = Some(content),
            "@exception" | "@throws" => self.exceptions.push(content),
            "@author" => self.authors.push(Self::substitute_qualification(&content)),
            "@see" => self.sees.push(Self::substitute_qualification(&content)),
            "@since" => self.since.push(Self::substitute_qualification(&content)),
            _ => self.rest.push(Pair { tag: Some(tag.to_string()), content: Some(content) }),
        }
    }

    fn process_simple_tag(&mut self) -> String {
        self.parameters.clear();
        self.exceptions.clear();
        self.authors.clear();
        self.sees.clear();
        self.since.clear();
        self.rest.clear();
        self.return_ = None;

        let mut c: i32 = '@' as i32;
        while c != EOF {
            let mut tagbuf = String::new();
            tagbuf.push(to_char(c));
            c = self.get_tag(&mut tagbuf);
            let tag = tagbuf;
            let mut content: Vec<char> = Vec::new();
            if c != EOF {
                c = self.get_content_until_next_tag(&mut content);
            }
            let content: String = content.into_iter().collect();
            self.handle_tag(&tag, &content);
        }
        self.print_simple_tag()
    }

    fn print_block_tag(tag: &str, content: &str) -> String {
        match tag {
            "@link" | "@linkplain" => {
                let chars: Vec<char> = content.chars().collect();
                let mut in_paren = false;
                let mut label_start = 0;
                for (i, &nc) in chars.iter().enumerate() {
                    if i == 0 && java_is_whitespace(nc) {
                        label_start = 1;
                        continue;
                    }
                    if nc == '(' {
                        in_paren = true;
                        continue;
                    }
                    if nc == ')' {
                        in_paren = false;
                        continue;
                    }
                    if !in_paren && java_is_whitespace(nc) {
                        label_start = i + 1;
                        break;
                    }
                }
                let label: String = chars[label_start.min(chars.len())..].iter().collect();
                if tag == "@link" {
                    format!("<code>{}</code>", Self::substitute_qualification(&label))
                } else {
                    Self::substitute_qualification(&label)
                }
            }
            "@literal" => Self::print_literal(content),
            "@code" => format!("<code>{}</code>", Self::print_literal(content)),
            _ => Self::substitute_qualification(content),
        }
    }

    fn print_literal(content: &str) -> String {
        let chars: Vec<char> = content.chars().collect();
        let mut start = 0;
        for (i, c) in chars.iter().enumerate() {
            if !java_is_whitespace(*c) {
                start = i;
                break;
            }
        }
        let s: String = chars[start..].iter().collect();
        convert_to_html_content(&s)
    }

    fn process_block_tag(&mut self) -> Option<String> {
        let mut c = self.next_char();
        if c != '@' as i32 {
            let mut s = String::from("{");
            s.push(to_char(c));
            return Some(s);
        }
        let mut buffer = String::new();
        buffer.push(to_char(c));
        c = self.get_tag(&mut buffer);
        let tag = buffer;
        let mut content = String::new();
        if c != EOF && c != '}' as i32 {
            content.push(to_char(c));
            self.get_content(&mut content, '}');
        }
        Some(Self::print_block_tag(&tag, &content))
    }
}

/// `new JdtLsJavaDoc2HTMLTextReader(new StringReader(javadoc)).getString()`
pub fn javadoc_to_html(javadoc: &str) -> String {
    JavaDoc2HtmlTextReader::new(javadoc).get_string()
}
