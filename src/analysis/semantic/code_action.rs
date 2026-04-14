//! Convert bridge code-action response → lsp_types::CodeAction.

use super::protocol::{BridgeAction, BridgeFileEdit, BridgeTextEdit};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Position, Range, TextEdit, Url, WorkspaceEdit,
};
use std::collections::HashMap;

pub fn to_lsp(actions: &[BridgeAction]) -> Vec<CodeAction> {
    actions.iter().map(|a| {
        let kind = a.kind.clone().map(|k| CodeActionKind::from(k));
        let edit = workspace_edit(&a.edits);
        CodeAction {
            title: a.title.clone(),
            kind,
            edit: Some(edit),
            ..Default::default()
        }
    }).collect()
}

fn workspace_edit(file_edits: &[BridgeFileEdit]) -> WorkspaceEdit {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for fe in file_edits {
        if let Ok(uri) = Url::parse(&fe.uri) {
            let edits = fe.edits.iter().map(text_edit).collect::<Vec<_>>();
            changes.entry(uri).or_default().extend(edits);
        }
    }
    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

fn text_edit(e: &BridgeTextEdit) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position { line: e.start_line, character: e.start_char },
            end: Position { line: e.end_line, character: e.end_char },
        },
        new_text: e.new_text.clone(),
    }
}
