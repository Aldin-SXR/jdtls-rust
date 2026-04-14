//! Convert BridgeCompletion → lsp_types::CompletionItem.

use super::protocol::{BridgeCompletion, BridgeTextEdit};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent,
    MarkupKind, Position, Range, TextEdit,
};

pub fn to_lsp(c: &BridgeCompletion) -> CompletionItem {
    let kind = kind_from_u8(c.kind);
    let insert_text_format = c.insert_text_format.map(|f| match f {
        2 => InsertTextFormat::SNIPPET,
        _ => InsertTextFormat::PLAIN_TEXT,
    });

    let documentation = c.documentation.as_ref().map(|doc| {
        Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc.clone(),
        })
    });

    let additional_text_edits = c.additional_edits.as_ref().map(|edits| {
        edits.iter().map(bridge_text_edit_to_lsp).collect::<Vec<_>>()
    });

    CompletionItem {
        label: c.label.clone(),
        kind: Some(kind),
        detail: c.detail.clone(),
        documentation,
        insert_text: c.insert_text.clone(),
        insert_text_format,
        sort_text: c.sort_text.clone(),
        filter_text: c.filter_text.clone(),
        additional_text_edits,
        ..Default::default()
    }
}

fn bridge_text_edit_to_lsp(e: &BridgeTextEdit) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position { line: e.start_line, character: e.start_char },
            end: Position { line: e.end_line, character: e.end_char },
        },
        new_text: e.new_text.clone(),
    }
}

fn kind_from_u8(n: u8) -> CompletionItemKind {
    match n {
        1 => CompletionItemKind::TEXT,
        2 => CompletionItemKind::METHOD,
        3 => CompletionItemKind::FUNCTION,
        4 => CompletionItemKind::CONSTRUCTOR,
        5 => CompletionItemKind::FIELD,
        6 => CompletionItemKind::VARIABLE,
        7 => CompletionItemKind::CLASS,
        8 => CompletionItemKind::INTERFACE,
        9 => CompletionItemKind::MODULE,
        10 => CompletionItemKind::PROPERTY,
        11 => CompletionItemKind::UNIT,
        12 => CompletionItemKind::VALUE,
        13 => CompletionItemKind::ENUM,
        14 => CompletionItemKind::KEYWORD,
        15 => CompletionItemKind::SNIPPET,
        16 => CompletionItemKind::COLOR,
        17 => CompletionItemKind::FILE,
        18 => CompletionItemKind::REFERENCE,
        19 => CompletionItemKind::FOLDER,
        20 => CompletionItemKind::ENUM_MEMBER,
        21 => CompletionItemKind::CONSTANT,
        22 => CompletionItemKind::STRUCT,
        23 => CompletionItemKind::EVENT,
        24 => CompletionItemKind::OPERATOR,
        25 => CompletionItemKind::TYPE_PARAMETER,
        _ => CompletionItemKind::TEXT,
    }
}
