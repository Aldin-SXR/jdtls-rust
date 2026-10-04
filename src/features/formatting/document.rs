//! A minimal port of the jface `IDocument` operations the formatter handler
//! relies on.  Offsets are UTF-16 code units (Java `String` indices); lines
//! are delimited by `\r\n`, `\r` or `\n` (`DefaultLineTracker`).  Methods
//! return `None` where jface throws `BadLocationException`.

use super::Edit;
use tower_lsp::lsp_types::Position;

pub struct Document {
    units: Vec<u16>,
    /// Start offset of every line (`getNumberOfLines()` entries).
    line_starts: Vec<usize>,
    /// Delimiter ending each line (`None` for the last one).
    delimiters: Vec<Option<&'static str>>,
}

const LF: u16 = b'\n' as u16;
const CR: u16 = b'\r' as u16;

impl Document {
    pub fn new(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut line_starts = vec![0];
        let mut delimiters = Vec::new();
        let mut i = 0;
        while i < units.len() {
            match units[i] {
                CR if units.get(i + 1) == Some(&LF) => {
                    delimiters.push(Some("\r\n"));
                    i += 2;
                    line_starts.push(i);
                }
                CR => {
                    delimiters.push(Some("\r"));
                    i += 1;
                    line_starts.push(i);
                }
                LF => {
                    delimiters.push(Some("\n"));
                    i += 1;
                    line_starts.push(i);
                }
                _ => i += 1,
            }
        }
        delimiters.push(None);
        Self { units, line_starts, delimiters }
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    #[cfg(test)]
    pub fn number_of_lines(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_offset(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    /// Length of `line` including its delimiter.
    pub fn line_length(&self, line: usize) -> Option<usize> {
        let start = self.line_offset(line)?;
        let end = self.line_starts.get(line + 1).copied().unwrap_or(self.units.len());
        Some(end - start)
    }

    pub fn line_of_offset(&self, offset: usize) -> Option<usize> {
        if offset > self.units.len() {
            return None;
        }
        Some(self.line_starts.partition_point(|&s| s <= offset) - 1)
    }

    pub fn line_delimiter(&self, line: usize) -> Option<&'static str> {
        self.delimiters.get(line).copied().flatten()
    }

    pub fn char_at(&self, offset: usize) -> Option<u16> {
        self.units.get(offset).copied()
    }

    /// `IDocument.get(offset, length)`.
    pub fn get(&self, offset: usize, length: usize) -> Option<String> {
        let end = offset.checked_add(length)?;
        if end > self.units.len() {
            return None;
        }
        Some(String::from_utf16_lossy(&self.units[offset..end]))
    }

    /// `FormatterHandler.createPosition`.
    pub fn position(&self, offset: usize) -> Position {
        let line = self.line_of_offset(offset).unwrap_or(0);
        let start = self.line_starts[line];
        Position { line: line as u32, character: offset.saturating_sub(start) as u32 }
    }

    /// `TextUtilities.getDefaultLineDelimiter`: the first line's delimiter,
    /// else the platform line separator.
    pub fn default_line_delimiter(&self) -> &'static str {
        self.line_delimiter(0).unwrap_or(if cfg!(windows) { "\r\n" } else { "\n" })
    }

    /// Apply non-overlapping edits sorted by offset (`TextEdit.apply`).
    pub fn apply(&self, edits: &[Edit]) -> Option<String> {
        let mut out: Vec<u16> = Vec::with_capacity(self.units.len());
        let mut pos = 0;
        for e in edits {
            if e.offset < pos || e.offset + e.length > self.units.len() {
                return None;
            }
            out.extend_from_slice(&self.units[pos..e.offset]);
            out.extend(e.text.encode_utf16());
            pos = e.offset + e.length;
        }
        out.extend_from_slice(&self.units[pos..]);
        Some(String::from_utf16_lossy(&out))
    }
}

/// UTF-16 length of `s` (Java `String.length()`).
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Java `String.substring(begin, end)` on UTF-16 indices.
pub fn substring16(s: &str, begin: usize, end: usize) -> Option<String> {
    let units: Vec<u16> = s.encode_utf16().collect();
    if begin > end || end > units.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&units[begin..end]))
}

/// `Character.isWhitespace(char)`.
pub fn is_java_whitespace(c: u16) -> bool {
    match c {
        0x09..=0x0D | 0x1C..=0x1F => true,
        0x00A0 | 0x2007 | 0x202F => false,
        _ => char::from_u32(c as u32).is_some_and(|ch| {
            ch == ' ' || ch == '\u{1680}' || ('\u{2000}'..='\u{200A}').contains(&ch) || ch == '\u{2028}' || ch == '\u{2029}' || ch == '\u{205F}' || ch == '\u{3000}'
        }),
    }
}

/// Java `String.trim()`: strips code points `<= ' '` from both ends.
pub fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Java `String.isBlank()`.
pub fn java_is_blank(s: &str) -> bool {
    s.encode_utf16().all(is_java_whitespace)
}

/// `TextUtilities.determineLineDelimiter(text, hint)`: the first delimiter in
/// `text`, else `hint`.
pub fn determine_line_delimiter(text: &str, hint: &'static str) -> &'static str {
    Document::new(text).line_delimiter(0).unwrap_or(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let d = Document::new("ab\r\ncd\ref\n");
        assert_eq!(d.number_of_lines(), 4);
        assert_eq!(d.line_offset(1), Some(4));
        assert_eq!(d.line_length(0), Some(4));
        assert_eq!(d.line_offset(3), Some(10));
        assert_eq!(d.line_offset(4), None);
        assert_eq!(d.line_of_offset(10), Some(3));
        assert_eq!(d.default_line_delimiter(), "\r\n");
    }
}
