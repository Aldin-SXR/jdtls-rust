//! Port of flexmark-util-sequence 0.64.8 `LineAppendableImpl` (+ `LineInfo`), the
//! line-oriented text accumulator behind flexmark's `MarkdownWriterBase`.
//!
//! All lengths/offsets are counted in chars; they are only ever compared with
//! each other, so this matches Java's UTF-16 accounting for the inputs we see.

pub const F_CONVERT_TABS: u32 = 1 << 0;
pub const F_COLLAPSE_WHITESPACE: u32 = 1 << 1;
pub const F_TRIM_TRAILING_WHITESPACE: u32 = 1 << 2;
pub const F_PASS_THROUGH: u32 = 1 << 3;
pub const F_TRIM_LEADING_WHITESPACE: u32 = 1 << 4;
pub const F_TRIM_LEADING_EOL: u32 = 1 << 5;
pub const F_PREFIX_PRE_FORMATTED: u32 = 1 << 6;
pub const F_WHITESPACE_REMOVAL: u32 = F_COLLAPSE_WHITESPACE | F_TRIM_TRAILING_WHITESPACE | F_TRIM_LEADING_WHITESPACE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pre {
    None,
    First,
    Body,
    Last,
}

#[derive(Clone, Debug)]
pub struct LineInfo {
    pub prefix: String,
    pub text: String,
    pub eol: String,
    pub prefix_len: usize,
    pub text_len: usize,
    pub len: usize,
    pub blank_prefix: bool,
    pub blank_text: bool,
    pub pre: Pre,
}

impl LineInfo {
    fn new(prefix: String, text: String, eol: String, blank_prefix: bool, blank_text: bool, pre: Pre) -> Self {
        let prefix_len = prefix.chars().count();
        let text_len = text.chars().count();
        let len = prefix_len + text_len + eol.chars().count();
        LineInfo {
            blank_prefix: blank_prefix || prefix_len == 0,
            blank_text: blank_text || text_len == 0,
            prefix,
            text,
            eol,
            prefix_len,
            text_len,
            len,
            pre,
        }
    }
    pub fn is_preformatted(&self) -> bool {
        self.pre != Pre::None
    }
    /// prefix + text + eol
    pub fn line(&self) -> String {
        format!("{}{}{}", self.prefix, self.text, self.eol)
    }
    pub fn line_no_eol(&self) -> String {
        format!("{}{}", self.prefix, self.text)
    }
    /// text + eol
    pub fn text_with_eol(&self) -> String {
        format!("{}{}", self.text, self.eol)
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn char_slice(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

fn is_blank(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}'))
}

pub fn trim_end_ws(s: &str) -> String {
    s.trim_end_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}')).to_string()
}

pub fn count_trailing_space_tab(s: &str) -> usize {
    s.chars().rev().take_while(|c| *c == ' ' || *c == '\t').count()
}

fn eol_end_length(s: &str) -> usize {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let pos = chars.len() - 1;
    match chars[pos] {
        '\r' => 1,
        '\n' => {
            if pos > 0 && chars[pos - 1] == '\r' {
                2
            } else {
                1
            }
        }
        _ => 0,
    }
}

/// Range with NULL support: `None` is `Range.NULL`.
type RangeOpt = Option<(usize, usize)>;

#[derive(Clone, Debug)]
pub struct LineAppendable {
    options: u32,
    pre_nesting: i32,
    pre_first_line: i64,
    pre_first_line_offset: usize,
    pre_last_line: i64,
    pre_last_line_offset: usize,
    appendable: String,
    app_len: usize,
    pub lines: Vec<LineInfo>,
    prefix: String,
    prefix_after_eol: String,
    prefix_stack: Vec<String>,
    indent_prefix_stack: Vec<bool>,
    all_whitespace: bool,
    last_was_whitespace: bool,
    option_stack: Vec<u32>,
}

impl LineAppendable {
    pub fn new(options: u32) -> Self {
        LineAppendable {
            options,
            pre_nesting: 0,
            pre_first_line: -1,
            pre_first_line_offset: 0,
            pre_last_line: -1,
            pre_last_line_offset: 0,
            appendable: String::new(),
            app_len: 0,
            lines: Vec::new(),
            prefix: String::new(),
            prefix_after_eol: String::new(),
            prefix_stack: Vec::new(),
            indent_prefix_stack: Vec::new(),
            all_whitespace: true,
            last_was_whitespace: false,
            option_stack: Vec::new(),
        }
    }

    pub fn get_options(&self) -> u32 {
        self.options
    }
    pub fn set_options(&mut self, o: u32) {
        self.options = o;
    }
    pub fn push_options(&mut self) {
        self.option_stack.push(self.options);
    }
    pub fn pop_options(&mut self) {
        self.options = self.option_stack.pop().expect("Option stack is empty");
    }
    pub fn remove_options(&mut self, flags: u32) {
        self.options &= !flags;
    }
    fn any(&self, f: u32) -> bool {
        self.options & f != 0
    }

    // ── prefixes ──

    /// `getPrefix()` (prefix after EOL)
    pub fn get_prefix(&self) -> String {
        self.prefix_after_eol.clone()
    }
    pub fn add_prefix_after(&mut self, p: &str, after_eol: bool) {
        if !self.any(F_PASS_THROUGH) && !p.is_empty() {
            if after_eol {
                self.prefix_after_eol = format!("{}{}", self.prefix_after_eol, p);
            } else {
                self.prefix = format!("{}{}", self.prefix_after_eol, p);
                self.prefix_after_eol = self.prefix.clone();
            }
        }
    }
    /// `addPrefix(prefix)`: takes effect immediately unless at the start of a line.
    pub fn add_prefix(&mut self, p: &str) {
        let after = self.get_pending_eol() == 0;
        self.add_prefix_after(p, after);
    }
    pub fn set_prefix(&mut self, p: &str, after_eol: bool) {
        if !self.any(F_PASS_THROUGH) {
            if after_eol {
                self.prefix_after_eol = p.to_string();
            } else {
                self.prefix = p.to_string();
                self.prefix_after_eol = self.prefix.clone();
            }
        }
    }
    pub fn push_prefix(&mut self) {
        if !self.any(F_PASS_THROUGH) {
            self.prefix_stack.push(self.prefix_after_eol.clone());
            self.indent_prefix_stack.push(false);
        }
    }
    pub fn pop_prefix(&mut self) {
        self.pop_prefix_after(false);
    }
    pub fn pop_prefix_after(&mut self, after_eol: bool) {
        if !self.any(F_PASS_THROUGH) {
            self.prefix_after_eol = self.prefix_stack.pop().expect("popPrefix with an empty stack");
            if !after_eol {
                self.prefix = self.prefix_after_eol.clone();
            }
            self.indent_prefix_stack.pop();
        }
    }

    // ── line infos ──

    fn last_line_info(&self) -> Option<&LineInfo> {
        self.lines.last()
    }

    fn sum_len(&self, upto: usize) -> usize {
        self.lines[..upto].iter().map(|l| l.len).sum()
    }

    fn is_trailing_blank_line(&self) -> bool {
        self.app_len == 0 && self.last_line_info().map(|l| l.blank_text).unwrap_or(true)
    }

    fn last_non_blank_line(&self, end_line: usize) -> isize {
        if end_line > self.lines.len() && self.app_len > 0 && !self.all_whitespace {
            return self.lines.len() as isize;
        }
        let mut i = end_line.min(self.lines.len()) as isize;
        loop {
            i -= 1;
            if i < 0 {
                break;
            }
            if !self.lines[i as usize].blank_text {
                break;
            }
        }
        i
    }

    pub fn get_trailing_blank_lines(&self, end_line: usize) -> usize {
        let end_line = end_line.min(self.lines.len());
        (end_line as isize - self.last_non_blank_line(end_line) - 1) as usize
    }

    pub fn ends_with_eol(&self) -> bool {
        self.app_len == 0 && !self.lines.is_empty()
    }

    fn get_line_range(&self, start: usize, end: usize, prefix: &str, null_range: bool) -> LineInfo {
        let sequence = &self.appendable;
        let eol_len = eol_end_length(sequence);
        let seq_len = self.app_len;
        let eol = if eol_len == 0 { "\n".to_string() } else { char_slice(sequence, seq_len - eol_len, seq_len) };
        let eol_chars = eol.chars().count();
        let text = if null_range {
            String::new()
        } else {
            let e = (end as isize - (eol_chars as isize - 1).max(0)).max(start as isize) as usize;
            char_slice(sequence, start, e)
        };
        let prefix = if null_range || start >= end { trim_end_ws(prefix) } else { prefix.to_string() };
        let cur = self.lines.len() as i64;
        let pre = if self.pre_nesting > 0 {
            if self.pre_first_line == cur { Pre::First } else { Pre::Body }
        } else if self.pre_first_line == cur {
            Pre::Last
        } else {
            Pre::None
        };
        let blank_prefix = is_blank(&prefix);
        let blank_text = self.all_whitespace || text.is_empty();
        LineInfo::new(prefix, text, eol, blank_prefix, blank_text, pre)
    }

    fn reset_builder(&mut self) {
        self.appendable.clear();
        self.app_len = 0;
        self.all_whitespace = true;
        self.last_was_whitespace = true;
    }

    fn add_line_range(&mut self, start: usize, end: usize, prefix: &str, null_range: bool) {
        let li = self.get_line_range(start, end, prefix, null_range);
        self.lines.push(li);
        self.reset_builder();
    }

    fn push_app(&mut self, c: char) {
        self.appendable.push(c);
        self.app_len += 1;
    }

    fn append_eol_raw(&mut self) {
        self.push_app('\n');
        let end_offset = self.app_len;
        let prefix = self.prefix.clone();
        self.add_line_range(0, end_offset - 1, &prefix, false);
        self.raw_indents_on_first_eol();
    }

    fn raw_indents_on_first_eol(&mut self) {
        self.prefix = self.prefix_after_eol.clone();
    }

    fn append_eol_count(&mut self, count: isize) {
        let mut c = count;
        while c > 0 {
            self.append_eol_raw();
            c -= 1;
        }
    }

    fn is_prefixed(&self, current_line: i64) -> bool {
        self.any(F_PREFIX_PRE_FORMATTED)
            || (self.pre_first_line == current_line || self.pre_nesting == 0 && self.pre_last_line != current_line)
    }

    fn range_prefix_after_eol(&self) -> (RangeOpt, String) {
        let mut start_offset: usize = 0;
        let mut end_offset: usize = self.app_len + 1;
        let current_line = self.lines.len() as i64;
        let need_prefix = self.is_prefixed(current_line);
        if self.any(F_PASS_THROUGH) {
            return (Some((start_offset, end_offset - 1)), if need_prefix { self.prefix.clone() } else { String::new() });
        }
        if self.all_whitespace
            && (self.pre_nesting == 0 && !(self.pre_first_line == current_line || self.pre_last_line == current_line))
        {
            if !self.any(F_TRIM_LEADING_EOL) || !self.lines.is_empty() {
                (Some((start_offset, end_offset - 1)), self.prefix.clone())
            } else {
                (None, String::new())
            }
        } else {
            if self.any(F_TRIM_TRAILING_WHITESPACE) && self.pre_nesting == 0 {
                if self.all_whitespace {
                    start_offset = end_offset - 1;
                } else {
                    end_offset -= count_trailing_space_tab(&self.appendable);
                }
            }
            if self.pre_first_line == current_line && start_offset > self.pre_first_line_offset {
                start_offset = self.pre_first_line_offset;
            }
            if self.pre_last_line == current_line && end_offset < self.pre_last_line_offset + 1 {
                end_offset = self.pre_last_line_offset + 1;
            }
            (Some((start_offset, end_offset - 1)), if need_prefix { self.prefix.clone() } else { String::new() })
        }
    }

    fn offset_after_eol(&self) -> usize {
        let (range, prefix) = self.range_prefix_after_eol();
        let last_sum = self.sum_len(self.lines.len());
        match range {
            None => last_sum,
            Some((s, e)) => {
                let empty = s >= e;
                let prefix = if empty && !prefix.is_empty() { trim_end_ws(&prefix) } else { prefix };
                let span = e as isize - s as isize;
                (last_sum as isize + span + char_len(&prefix) as isize) as usize
            }
        }
    }

    fn append_impl(&mut self, c: char) {
        if self.any(F_PASS_THROUGH) {
            if c == '\n' {
                self.append_eol_raw();
            } else {
                if c != '\t' && c != ' ' {
                    self.all_whitespace = false;
                }
                self.push_app(c);
            }
            return;
        }
        if c == '\n' {
            let (range, pfx) = self.range_prefix_after_eol();
            match range {
                None => self.reset_builder(),
                Some((s, e)) => {
                    self.push_app('\n');
                    self.add_line_range(s, e, &pfx, false);
                }
            }
            self.raw_indents_on_first_eol();
        } else if c == '\t' {
            if self.pre_nesting == 0 && self.any(F_COLLAPSE_WHITESPACE) {
                if !self.last_was_whitespace {
                    self.push_app(' ');
                    self.last_was_whitespace = true;
                }
            } else if self.any(F_CONVERT_TABS) {
                let column = self.app_len;
                let spaces = 4 - (column % 4);
                for _ in 0..spaces {
                    self.push_app(' ');
                }
            } else {
                self.push_app('\t');
            }
        } else if c == ' ' {
            if self.pre_nesting == 0 {
                if !self.any(F_TRIM_LEADING_WHITESPACE) || (self.app_len != 0 && !self.all_whitespace) {
                    if self.any(F_COLLAPSE_WHITESPACE) {
                        if !self.last_was_whitespace {
                            self.push_app(' ');
                        }
                    } else {
                        self.push_app(' ');
                    }
                }
            } else {
                self.push_app(' ');
            }
            self.last_was_whitespace = true;
        } else {
            self.all_whitespace = false;
            self.last_was_whitespace = false;
            self.push_app(c);
        }
    }

    pub fn append(&mut self, s: &str) -> &mut Self {
        for c in s.chars() {
            self.append_impl(c);
        }
        self
    }

    pub fn append_char(&mut self, c: char) -> &mut Self {
        self.append_impl(c);
        self
    }

    pub fn append_repeat(&mut self, c: char, count: usize) -> &mut Self {
        for _ in 0..count {
            self.append_impl(c);
        }
        self
    }

    pub fn line(&mut self) -> &mut Self {
        if self.pre_nesting > 0 || self.app_len != 0 {
            self.append_impl('\n');
        } else {
            let saved = self.prefix.clone();
            self.raw_indents_on_first_eol();
            if !saved.is_empty() && self.prefix.is_empty() {
                self.prefix = saved;
            }
        }
        self
    }

    pub fn line_if(&mut self, p: bool) -> &mut Self {
        if p {
            self.line();
        }
        self
    }

    pub fn line_with_trailing_spaces(&mut self, count: usize) -> &mut Self {
        if self.pre_nesting > 0 || self.app_len != 0 {
            let options = self.options;
            self.options &= !(F_TRIM_TRAILING_WHITESPACE | F_COLLAPSE_WHITESPACE);
            if count > 0 {
                self.append_repeat(' ', count);
            }
            self.append_impl('\n');
            self.options = options;
        }
        self
    }

    pub fn blank_line(&mut self) -> &mut Self {
        self.line();
        if (!self.lines.is_empty() && !self.is_trailing_blank_line()) || (self.lines.is_empty() && !self.any(F_TRIM_LEADING_EOL)) {
            self.append_eol_raw();
        }
        self
    }

    pub fn blank_line_if(&mut self, p: bool) -> &mut Self {
        if p {
            self.blank_line();
        }
        self
    }

    pub fn blank_line_count(&mut self, count: usize) -> &mut Self {
        self.line();
        if !self.any(F_TRIM_LEADING_EOL) || !self.lines.is_empty() {
            let add = count as isize - self.get_trailing_blank_lines(self.lines.len()) as isize;
            self.append_eol_count(add);
        }
        self
    }

    pub fn get_line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn get_line_count_with_pending(&self) -> usize {
        if self.app_len == 0 { self.lines.len() } else { self.lines.len() + 1 }
    }

    pub fn get_line_info(&self, idx: usize) -> Option<LineInfo> {
        if idx == self.lines.len() {
            if self.app_len == 0 {
                None
            } else {
                let (range, pfx) = self.range_prefix_after_eol();
                range.map(|(s, e)| self.get_line_range(s, e, &pfx, false))
            }
        } else {
            self.lines.get(idx).cloned()
        }
    }

    /// `getLineContent(i)`: line text without prefix/EOL.
    pub fn get_line_content(&self, idx: usize) -> String {
        self.get_line_info(idx).map(|l| l.text).unwrap_or_default()
    }

    pub fn offset_with_pending(&self) -> usize {
        self.offset_after_eol()
    }

    pub fn is_pending_space(&self) -> bool {
        self.app_len > 0 && self.last_was_whitespace
    }

    pub fn get_pending_space(&self) -> usize {
        if self.last_was_whitespace && self.app_len != 0 {
            count_trailing_space_tab(&self.appendable)
        } else {
            0
        }
    }

    pub fn get_pending_eol(&self) -> usize {
        if self.app_len == 0 {
            self.get_trailing_blank_lines(self.lines.len()) + 1
        } else {
            0
        }
    }

    pub fn is_pre_formatted(&self) -> bool {
        self.pre_nesting > 0
    }

    pub fn open_pre_formatted(&mut self) {
        if self.pre_nesting == 0 && self.pre_first_line != self.lines.len() as i64 {
            self.pre_first_line = self.lines.len() as i64;
            self.pre_first_line_offset = self.app_len;
        }
        self.pre_nesting += 1;
    }

    pub fn close_pre_formatted(&mut self) {
        assert!(self.pre_nesting > 0, "closePreFormatted called with nesting == 0");
        self.pre_nesting -= 1;
        if self.pre_nesting == 0 && !self.ends_with_eol() {
            self.pre_last_line = self.lines.len() as i64;
            self.pre_last_line_offset = self.app_len;
        }
    }

    pub fn remove_lines(&mut self, start: usize, end: usize) {
        let use_end = end.min(self.get_line_count_with_pending());
        if start < use_end {
            let e = use_end.min(self.lines.len());
            self.lines.drain(start..e);
            return;
        }
        if end >= self.get_line_count_with_pending() && self.app_len > 0 {
            self.reset_builder();
        }
    }

    /// `toString()` (no line(), all lines incl. dangling text).
    pub fn to_string_raw(&self) -> String {
        self.append_to_no_line(true, usize::MAX as i64, usize::MAX as i64, 0, usize::MAX)
    }

    /// `toString(maxBlankLines, maxTrailingBlankLines)` (calls `line()` first).
    pub fn to_string_with(&mut self, max_blank_lines: i64, max_trailing_blank_lines: i64) -> String {
        self.line();
        self.append_to_no_line(true, max_blank_lines, max_trailing_blank_lines, 0, usize::MAX)
    }

    pub fn append_to_no_line(&self, with_prefixes: bool, max_blank_lines: i64, max_trailing: i64, start_line: usize, end_line: usize) -> String {
        let mut out = String::new();
        let tail_eol = max_trailing >= 0;
        let max_blank_lines = max_blank_lines.max(0);
        let max_trailing = max_trailing.max(0);
        let end_line_pending = self.lines.len();
        let i_max = self.get_line_count_with_pending().min(end_line);
        let last_non_blank = self.last_non_blank_line(i_max);
        let mut consecutive: i64 = 0;
        for i in start_line..i_max {
            let info = match self.get_line_info(i) {
                Some(i) => i,
                None => LineInfo::new(String::new(), String::new(), String::new(), true, true, Pre::None),
            };
            let not_dangling = i < end_line_pending;
            if info.text_len == 0 && !info.is_preformatted() {
                let pfx = if self.any(F_TRIM_TRAILING_WHITESPACE) { trim_end_ws(&info.prefix) } else { info.prefix.clone() };
                if i as isize > last_non_blank {
                    if consecutive < max_trailing {
                        consecutive += 1;
                        if with_prefixes {
                            out.push_str(&pfx);
                        }
                        if not_dangling && (tail_eol || consecutive != max_trailing) {
                            out.push('\n');
                        }
                    }
                } else if consecutive < max_blank_lines {
                    consecutive += 1;
                    if with_prefixes {
                        out.push_str(&pfx);
                    }
                    if not_dangling {
                        out.push('\n');
                    }
                }
            } else {
                consecutive = 0;
                if not_dangling
                    && (tail_eol || (i as isize) < last_non_blank || info.is_preformatted() && info.pre != Pre::Last)
                {
                    if with_prefixes {
                        out.push_str(&info.line());
                    } else {
                        out.push_str(&info.text_with_eol());
                    }
                } else if with_prefixes {
                    out.push_str(&info.line_no_eol());
                } else {
                    out.push_str(&info.text_with_eol());
                }
            }
        }
        out
    }
}
