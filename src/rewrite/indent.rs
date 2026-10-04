//! Port of `org.eclipse.jdt.core.formatter.IndentManipulation` plus the
//! UTF-16 string helpers the rewrite engine needs (JDT positions count
//! UTF-16 code units).

pub fn to_u16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn from_u16(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

/// Length of `s` in UTF-16 code units.
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.substring(start, end)` in UTF-16 units.
pub fn sub16(s: &str, start: usize, end: usize) -> String {
    let v = to_u16(s);
    let end = end.min(v.len());
    let start = start.min(end);
    from_u16(&v[start..end])
}

/// `ScannerHelper.isWhitespace` (Java whitespace).
pub fn is_whitespace(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x1C | 0x1D | 0x1E | 0x1F)
        || char::from_u32(c as u32).is_some_and(|ch| ch != '\u{00A0}' && ch != '\u{2007}' && ch != '\u{202F}' && ch.is_whitespace())
}

pub fn is_line_delimiter_char(c: u16) -> bool {
    c == b'\n' as u16 || c == b'\r' as u16
}

pub fn is_indent_char(c: u16) -> bool {
    is_whitespace(c) && !is_line_delimiter_char(c)
}

fn space_equivalents(tab_width: i32, n: i32) -> i32 {
    if tab_width == 0 {
        n
    } else {
        n + tab_width - n % tab_width
    }
}

pub fn measure_indent_in_spaces(line: &[u16], tab_width: i32) -> i32 {
    let mut length = 0;
    for &ch in line {
        if ch == b'\t' as u16 {
            length = space_equivalents(tab_width, length);
        } else if is_indent_char(ch) {
            length += 1;
        } else {
            return length;
        }
    }
    length
}

pub fn measure_indent_units(line: &[u16], tab_width: i32, indent_width: i32) -> i32 {
    if indent_width == 0 {
        return 0;
    }
    measure_indent_in_spaces(line, tab_width) / indent_width
}

pub fn extract_indent_string(line: &str, tab_width: i32, indent_width: i32) -> String {
    let v = to_u16(line);
    let size = v.len();
    let mut end = 0usize;
    let mut spaces = 0;
    let mut characters = 0usize;
    for &c in &v {
        if c == b'\t' as u16 {
            spaces = space_equivalents(tab_width, spaces);
            characters += 1;
        } else if is_indent_char(c) {
            spaces += 1;
            characters += 1;
        } else {
            break;
        }
        if spaces >= indent_width {
            end += characters;
            characters = 0;
            spaces = if indent_width == 0 { 0 } else { spaces % indent_width };
        }
    }
    if end == 0 {
        String::new()
    } else if end == size {
        line.to_owned()
    } else {
        from_u16(&v[..end])
    }
}

pub fn trim_indent(line: &str, units: i32, tab_width: i32, indent_width: i32) -> String {
    if units <= 0 || indent_width == 0 {
        return line.to_owned();
    }
    let v = to_u16(line);
    let to_remove = units * indent_width;
    let mut start = 0usize;
    let mut spaces = 0;
    let size = v.len();
    let mut prefix: Option<String> = None;
    for (i, &c) in v.iter().enumerate() {
        if c == b'\t' as u16 {
            spaces = space_equivalents(tab_width, spaces);
        } else if is_indent_char(c) {
            spaces += 1;
        } else {
            start = i;
            break;
        }
        if spaces == to_remove {
            start = i + 1;
            break;
        }
        if spaces > to_remove {
            start = i + 1;
            prefix = Some(" ".repeat((spaces - to_remove) as usize));
            break;
        }
    }
    let trimmed = if start == size { String::new() } else { from_u16(&v[start..]) };
    match prefix {
        Some(p) => p + &trimmed,
        None => trimmed,
    }
}

/// `DefaultLineTracker` line regions `(offset, length)` (delimiters excluded).
pub fn line_regions(v: &[u16]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < v.len() {
        let c = v[i];
        if c == b'\r' as u16 {
            out.push((start, i - start));
            if v.get(i + 1) == Some(&(b'\n' as u16)) {
                i += 1;
            }
            start = i + 1;
        } else if c == b'\n' as u16 {
            out.push((start, i - start));
            start = i + 1;
        }
        i += 1;
    }
    out.push((start, v.len() - start));
    out
}

pub fn change_indent(code: &str, units: i32, tab_width: i32, indent_width: i32, new_indent: &str, line_delim: &str) -> String {
    let v = to_u16(code);
    let lines = line_regions(&v);
    if lines.len() == 1 {
        return code.to_owned();
    }
    let mut buf = String::new();
    for (i, &(s, l)) in lines.iter().enumerate() {
        let line = from_u16(&v[s..s + l]);
        if i == 0 {
            buf.push_str(&line);
        } else {
            buf.push_str(line_delim);
            buf.push_str(new_indent);
            if indent_width != 0 {
                buf.push_str(&trim_indent(&line, units, tab_width, indent_width));
            } else {
                buf.push_str(&line);
            }
        }
    }
    buf
}

fn index_of_indent(line: &[u16], units: i32, tab_width: i32, indent_width: i32) -> i32 {
    let spaces_needed = units * indent_width;
    let mut result: i32 = -1;
    let mut blanks = 0;
    let mut i = 0usize;
    while i < line.len() && blanks < spaces_needed {
        let c = line[i];
        if c == b'\t' as u16 {
            blanks = space_equivalents(tab_width, blanks);
        } else if is_indent_char(c) {
            blanks += 1;
        } else {
            break;
        }
        result = i as i32;
        i += 1;
    }
    if blanks < spaces_needed {
        -1
    } else {
        result + 1
    }
}

/// `IndentManipulation.getChangeIndentEdits`.
pub fn get_change_indent_edits(source: &[u16], units: i32, tab_width: i32, indent_width: i32, new_indent: &str) -> Vec<(usize, usize, String)> {
    let mut result = Vec::new();
    let lines = line_regions(source);
    if lines.len() == 1 {
        return result;
    }
    for &(offset, len) in lines.iter().skip(1) {
        let line = &source[offset..offset + len];
        let length = index_of_indent(line, units, tab_width, indent_width);
        if length >= 0 {
            result.push((offset, length as usize, new_indent.to_owned()));
        } else {
            let l = measure_indent_units(line, tab_width, indent_width);
            result.push((offset, l.max(0) as usize, String::new()));
        }
    }
    result
}

/// `IndentManipulation.getTabWidth(options)`.
pub fn tab_width(options: &std::collections::BTreeMap<String, String>) -> i32 {
    options.get("org.eclipse.jdt.core.formatter.tabulation.size").and_then(|v| v.parse().ok()).unwrap_or(4)
}

/// `IndentManipulation.getIndentWidth(options)`.
pub fn indent_width(options: &std::collections::BTreeMap<String, String>) -> i32 {
    let tab = tab_width(options);
    if options.get("org.eclipse.jdt.core.formatter.tabulation.char").map(String::as_str) == Some("mixed") {
        options.get("org.eclipse.jdt.core.formatter.indentation.size").and_then(|v| v.parse().ok()).unwrap_or(tab)
    } else {
        tab
    }
}

/// `CodeFormatter.createIndentationString(level)` (`DefaultCodeFormatter`).
pub fn create_indentation_string(options: &std::collections::BTreeMap<String, String>, level: i32) -> String {
    if level <= 0 {
        return String::new();
    }
    let tab_size = tab_width(options);
    let indentation_size = options
        .get("org.eclipse.jdt.core.formatter.indentation.size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let (tabs, spaces) = match options.get("org.eclipse.jdt.core.formatter.tabulation.char").map(String::as_str) {
        Some("space") => (0, level * tab_size),
        Some("mixed") => {
            if tab_size != 0 {
                let eq = level * indentation_size;
                (eq / tab_size, eq % tab_size)
            } else {
                (0, 0)
            }
        }
        _ => (level, 0),
    };
    "\t".repeat(tabs.max(0) as usize) + &" ".repeat(spaces.max(0) as usize)
}
