//! A text buffer addressed like JDT/JFace documents: UTF-16 offsets, lines
//! split at `\n`, `\r\n` and `\r`.

use tower_lsp::lsp_types::{Position, Range};

#[derive(Debug, Clone)]
pub struct Doc {
    pub units: Vec<u16>,
    /// UTF-16 offset of each line start.
    line_starts: Vec<usize>,
}

impl Doc {
    pub fn new(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut line_starts = vec![0];
        let mut i = 0;
        while i < units.len() {
            let c = units[i];
            if c == b'\r' as u16 {
                if i + 1 < units.len() && units[i + 1] == b'\n' as u16 {
                    i += 1;
                }
                line_starts.push(i + 1);
            } else if c == b'\n' as u16 {
                line_starts.push(i + 1);
            }
            i += 1;
        }
        Doc { units, line_starts }
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn char_at(&self, offset: usize) -> char {
        self.units.get(offset).map(|&u| char::from_u32(u as u32).unwrap_or('\u{fffd}')).unwrap_or('\0')
    }

    pub fn get(&self, start: usize, len: usize) -> String {
        let s = start.min(self.units.len());
        let e = (start + len).min(self.units.len());
        String::from_utf16_lossy(&self.units[s..e])
    }

    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_offset(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(self.units.len())
    }

    /// Length of `line` without its delimiter.
    pub fn line_length(&self, line: usize) -> usize {
        let start = self.line_offset(line);
        let mut end = if line + 1 < self.line_starts.len() { self.line_starts[line + 1] } else { self.units.len() };
        while end > start && (self.units[end - 1] == b'\n' as u16 || self.units[end - 1] == b'\r' as u16) {
            end -= 1;
        }
        end - start
    }

    /// `IDocument.getLineInformationOfOffset`: (line start, length).
    pub fn line_info_of_offset(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset.min(self.units.len()));
        (self.line_offset(line), self.line_length(line))
    }

    pub fn delimiter_of_line(&self, line: usize) -> Option<String> {
        let start = self.line_offset(line) + self.line_length(line);
        let end = if line + 1 < self.line_starts.len() { self.line_starts[line + 1] } else { return None };
        Some(String::from_utf16_lossy(&self.units[start..end]))
    }

    /// `TextUtilities.getDefaultLineDelimiter`: the first line's delimiter,
    /// else the platform default.
    pub fn default_line_delimiter(&self) -> String {
        self.delimiter_of_line(0).unwrap_or_else(|| "\n".to_owned())
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.units.len());
        let line = self.line_of(offset);
        Position::new(line as u32, (offset - self.line_starts[line]) as u32)
    }

    /// `JDTUtils.toRange(cu, offset, length)`.
    pub fn range(&self, offset: usize, length: usize) -> Range {
        Range::new(self.position(offset), self.position(offset + length))
    }

    /// `JsonRpcHelpers.toOffset`.
    pub fn offset(&self, pos: Position) -> usize {
        let line = pos.line as usize;
        if line >= self.line_starts.len() {
            return self.units.len();
        }
        (self.line_starts[line] + pos.character as usize).min(self.units.len())
    }
}
