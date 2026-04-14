//! Helpers for converting LSP position/offset for document handlers.

use tower_lsp::lsp_types::Position;
use ropey::Rope;

/// Convert an LSP Position to a byte offset into the document content.
pub fn pos_to_offset(rope: &Rope, pos: Position) -> Option<usize> {
    let line = pos.line as usize;
    let col = pos.character as usize;
    if line >= rope.len_lines() {
        return None;
    }
    let char_idx = rope.line_to_char(line) + col;
    Some(rope.char_to_byte(char_idx))
}
