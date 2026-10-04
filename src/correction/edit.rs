//! Change → `WorkspaceEdit` (jdt.ls `ChangeUtil.convertToWorkspaceEdit` with
//! `TextEditConverter`): each unit's edit tree becomes one LSP text edit
//! spanning the root `MultiTextEdit`'s region with the new content of that
//! region.

use std::collections::{BTreeMap, HashMap};

use tower_lsp::lsp_types::{
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Range, TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

use super::{Change, CuChange};
use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::diagnostics::Doc16;
use crate::rewrite::import_rewrite::NoTypes;
use crate::rewrite::text_edit::EditTree;

/// What converting changes needs from the server.
pub struct Env<'a> {
    pub dispatcher: &'a Dispatcher,
    pub format: &'a crate::features::formatting::FormatEnv<'a>,
    pub lifecycle: &'a crate::features::lifecycle::Lifecycle,
}

impl Env<'_> {
    /// `cu.getOptions(true)`: the rewrite / formatter options of `uri`.
    pub async fn options(&self, uri: &str) -> BTreeMap<String, String> {
        let url = Url::parse(uri).ok();
        self.format.jdt_options(url.as_ref()).await
    }
}

/// The text edits of one unit's edit tree (`TextEditConverter.visit(MultiTextEdit)`
/// plus `ChangeUtil.filterTextEdits`).
pub fn tree_to_text_edits(text: &[u16], tree: &EditTree) -> Vec<TextEdit> {
    let Some((start, end)) = tree.covered_region() else { return Vec::new() };
    let new_text = tree.apply(text);
    let delta = new_text.len() as i64 - text.len() as i64;
    let new_end = ((end as i64 + delta).max(start as i64) as usize).min(new_text.len());
    let content = String::from_utf16_lossy(&new_text[start.min(new_end)..new_end]);
    let doc = Doc16::new(&String::from_utf16_lossy(text));
    let range: Range = doc.to_range(start as i64, (end - start) as i64);
    if content.is_empty() && range.start == range.end {
        return Vec::new();
    }
    vec![TextEdit { range, new_text: content }]
}

/// Builds the edit tree of a unit change (`CompilationUnitChange` root).
pub async fn cu_tree(env: &Env<'_>, cu: &mut CuChange) -> anyhow::Result<EditTree> {
    let mut root = EditTree::new();
    if let Some(rw) = &cu.rewrite {
        let options = env.options(&cu.ast.uri).await;
        let tree = crate::rewrite::formatter::rewrite_with_bridge(rw, &options, env.dispatcher)
            .await
            .map_err(|e| anyhow::anyhow!("rewrite failed: {e}"))?;
        root.add_tree(&tree).map_err(|e| anyhow::anyhow!("{}", e.0))?;
    }
    if let Some(imports) = cu.imports.as_mut() {
        let tree = imports.rewrite_imports(&NoTypes).map_err(|e| anyhow::anyhow!("{}", e.0))?;
        root.add_tree(&tree).map_err(|e| anyhow::anyhow!("{}", e.0))?;
    }
    if let Some(extra) = &cu.edits {
        root.add_tree(extra).map_err(|e| anyhow::anyhow!("{}", e.0))?;
    }
    Ok(root)
}

/// `ChangeUtil.convertToWorkspaceEdit(proposal.getChange())`.
pub async fn to_workspace_edit(env: &Env<'_>, change: &mut Change) -> anyhow::Result<WorkspaceEdit> {
    let changes_only = matches!(change, Change::Lazy(l) if l.changes_only());
    let mut cus: Vec<CuChange> = match change {
        Change::Cu(cus) => std::mem::take(cus),
        Change::Lazy(l) => l.compute(env).await?,
        Change::WorkspaceEdit(we) => return Ok(we.clone()),
        Change::None => return Ok(WorkspaceEdit::default()),
    };
    let resource_ops = !changes_only && crate::features::client_caps::resource_operations();
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    let mut document_changes: Vec<TextDocumentEdit> = Vec::new();
    let mut order: Vec<Url> = Vec::new();
    for cu in cus.iter_mut() {
        let tree = cu_tree(env, cu).await?;
        let edits = tree_to_text_edits(&cu.ast.source, &tree);
        if edits.is_empty() {
            continue;
        }
        let Ok(uri) = Url::parse(&cu.ast.uri) else { continue };
        if resource_ops {
            document_changes.push(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            });
        } else {
            if !order.contains(&uri) {
                order.push(uri.clone());
            }
            changes.entry(uri).or_default().extend(edits);
        }
    }
    // Put the edits back (the change may be converted again on resolve).
    if let Change::Cu(slot) = change {
        *slot = cus;
    }
    let mut we = WorkspaceEdit::default();
    if resource_ops {
        if !document_changes.is_empty() {
            we.document_changes = Some(DocumentChanges::Edits(document_changes));
        }
    } else {
        we.changes = Some(changes);
    }
    Ok(we)
}

/// `ChangeUtil.hasChanges(edit)`.
pub fn has_changes(we: &WorkspaceEdit) -> bool {
    if let Some(dc) = &we.document_changes {
        let empty = match dc {
            DocumentChanges::Edits(e) => e.is_empty(),
            DocumentChanges::Operations(o) => o.is_empty(),
        };
        if !empty {
            return true;
        }
    }
    let zero = Range::default();
    we.changes
        .as_ref()
        .is_some_and(|c| c.values().any(|edits| edits.iter().any(|e| e.range != zero || !e.new_text.is_empty())))
}
