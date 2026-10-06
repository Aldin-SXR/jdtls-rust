//! Convert bridge code-action response → lsp_types::CodeAction.

use super::protocol::{BridgeAction, BridgeFileEdit, BridgeTextEdit};
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Position, Range, TextEdit, Url, WorkspaceEdit,
};

/// Build an LSP `WorkspaceEdit` directly from a list of `BridgeFileEdit`s.
/// Used when the bridge returns a `WorkspaceEdit` response (e.g. organize-imports).
pub fn workspace_edit_from_bridge(file_edits: &[BridgeFileEdit]) -> WorkspaceEdit {
    workspace_edit(file_edits)
}

pub fn to_lsp(actions: &[BridgeAction]) -> Vec<CodeAction> {
    actions
        .iter()
        .map(|a| {
            let kind = a.kind.clone().map(|k| CodeActionKind::from(k));
            let edit = workspace_edit(&a.edits);
            // Legacy bridge import facts still carry the fully qualified type in
            // their title. Shape the public label with Eclipse's message in Rust.
            let title = a
                .title
                .strip_prefix("Import '")
                .and_then(|s| s.strip_suffix('\''))
                .and_then(|qualified| qualified.rsplit_once('.'))
                .map(|(container, name)| {
                    crate::correction::messages::format(
                        crate::correction::messages::ls_correction(
                            "UnresolvedElementsSubProcessor_importtype_description",
                        ),
                        &[name, container],
                    )
                })
                .unwrap_or_else(|| a.title.clone());
            CodeAction {
                title,
                kind,
                edit: Some(edit),
                is_preferred: if a.is_preferred { Some(true) } else { None },
                ..Default::default()
            }
        })
        .collect()
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
            start: Position {
                line: e.start_line,
                character: e.start_char,
            },
            end: Position {
                line: e.end_line,
                character: e.end_char,
            },
        },
        new_text: e.new_text.clone(),
    }
}
