//! Convert bridge location response → lsp_types::Location.

use super::protocol::BridgeLocation;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

pub fn to_lsp(locs: &[BridgeLocation]) -> Vec<Location> {
    locs.iter().filter_map(|l| {
        let uri = Url::parse(&l.uri).ok()?;
        Some(Location {
            uri,
            range: Range {
                start: Position { line: l.start_line, character: l.start_char },
                end: Position { line: l.end_line, character: l.end_char },
            },
        })
    }).collect()
}
