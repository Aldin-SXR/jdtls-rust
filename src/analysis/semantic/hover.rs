//! Convert bridge hover response → lsp_types::Hover.

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

pub fn to_lsp(markdown: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown.to_owned(),
        }),
        range: None,
    }
}
