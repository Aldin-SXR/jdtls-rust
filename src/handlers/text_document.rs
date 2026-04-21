//! Helpers for converting LSP position/offset for document handlers.

use ropey::Rope;
use tower_lsp::lsp_types::Position;

/// Convert an LSP Position to a char offset into the document content.
pub fn pos_to_char(rope: &Rope, pos: Position) -> Option<usize> {
    let line = pos.line as usize;
    if line >= rope.len_lines() {
        return None;
    }

    let line_start = rope.line_to_char(line);
    let line_slice = rope.line(line);
    let mut utf16_units = 0usize;
    let mut char_col = 0usize;
    let target = pos.character as usize;

    for ch in line_slice.chars() {
        if utf16_units >= target {
            break;
        }

        let ch_utf16 = ch.len_utf16();
        if utf16_units + ch_utf16 > target {
            break;
        }

        utf16_units += ch_utf16;
        char_col += 1;
    }

    Some(line_start + char_col)
}

/// Convert an LSP Position to a byte offset into the document content.
pub fn pos_to_offset(rope: &Rope, pos: Position) -> Option<usize> {
    let char_idx = pos_to_char(rope, pos)?;
    Some(rope.char_to_byte(char_idx))
}
