//! Port of jdt.ls `GetRefactorEditHandler.getEditsForRefactor`
//! (`java/getRefactorEdit`).

use std::sync::Arc;

use serde_json::{json, Value};
use tower_lsp::lsp_types::CodeActionParams;

use super::edit::Env;
use super::{Change, Context, CuChange};
use crate::analysis::semantic::diagnostics::Doc16;
use crate::rewrite::text_edit::EditKind;
use crate::rewrite::{Placeholder, RNode};
use crate::semantic_ast::{Ast, NodeKind};

/// `GetRefactorEditHandler.RENAME_COMMAND`.
pub const RENAME_COMMAND: &str = "java.action.rename";

/// `JSONUtility.toModel(object, String.class)`.
fn to_string_model(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(serde_json::from_str::<String>(s).unwrap_or_else(|_| s.clone())),
        _ => None,
    }
}

/// `JSONUtility.toModel(object, SelectionInfo.class)`: `(offset, length)`.
fn to_selection_info(v: Option<&Value>) -> Option<(usize, usize)> {
    let v = match v? {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        Value::Null => return None,
        other => other.clone(),
    };
    let offset = v.get("offset").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    let length = v.get("length").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    Some((offset, length))
}

/// `GetRefactorEditHandler.getEditsForRefactor(params)`.
pub async fn get_refactor_edit(env: &Env<'_>, params: Value) -> Option<Value> {
    let command = params["command"].as_str()?.to_owned();
    if command == "assignField" || command == "assignVariable" {
        return super::local_corrections::get_refactor_edit(env, params).await;
    }
    let action: CodeActionParams = serde_json::from_value(params["context"].clone()).ok()?;
    let uri = &action.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let doc = Doc16::new(ast.text());
    let start = doc.to_offset(action.range.start.line, action.range.start.character).max(0) as usize;
    let end = doc.to_offset(action.range.end.line, action.range.end.character).max(0) as usize;
    let mut ctx = Context::new(ast.clone(), start, end.saturating_sub(start));
    let locations = super::handler::problem_locations(&doc, &action.context.diagnostics);
    let problems_at_location = !locations.is_empty();
    let arguments: Vec<Value> = params["commandArguments"].as_array().cloned().unwrap_or_default();
    match command.as_str() {
        "extractField" => {
            let initialize_in = to_string_model(arguments.first());
            if let Some((offset, length)) = to_selection_info(arguments.get(1)) {
                ctx = Context::new(ast.clone(), offset, length);
            }
            super::quick_assist::extract_field_proposal_for(env, &ctx, problems_at_location, initialize_in.as_deref(), false, false, Some(&action)).await?;
            let change = super::quick_assist::ExtractFieldChange {
                ast: ast.clone(),
                offset: ctx.selection_offset,
                length: ctx.selection_length,
                initialize_in: super::quick_assist::initialize_scope_from_name(initialize_in.as_deref()),
            };
            let options = env.options(&ast.uri).await;
            let (cus, positions) = change.create(options);
            refactor_workspace_edit(env, &ast, cus, first_by_sequence_rank(&positions)).await
        }
        "extractMethod" => {
            if let Some((offset, length)) = to_selection_info(arguments.first()) {
                ctx = Context::new(ast.clone(), offset, length);
            }
            let options = env.options(&ast.uri).await;
            let change = super::quick_assist::extract_method_change(&ctx, options.clone(), problems_at_location)?;
            let (cus, positions) = change.create(options);
            refactor_workspace_edit(env, &ast, cus, first_by_sequence_rank(&positions)).await
        }
        "extractVariable" | "extractVariableAllOccurrence" | "extractConstant" => {
            if let Some((offset, length)) = to_selection_info(arguments.first()) {
                ctx = Context::new(ast.clone(), offset, length);
            }
            let options = env.options(&ast.uri).await;
            let cus = super::quick_assist::extract_variable_change(&command, &ctx, options)?;
            let tracked = cus.first().and_then(|cu| cu.rewrite.as_ref()).and_then(new_declaration_name);
            refactor_workspace_edit(env, &ast, cus, tracked).await
        }
        super::invert_boolean::INVERT_VARIABLE_COMMAND => {
            let covering = ctx.covering_node()?;
            let (rewrite, tracked) = super::invert_boolean::invert_variable_rewrite(&ctx, covering)?;
            refactor_workspace_edit(env, &ast, vec![CuChange::rewrite(rewrite)], Some(tracked)).await
        }
        _ => None,
    }
}

/// The name of the first new variable declaration fragment of a rewrite
/// (the declaration position of the extract refactorings).
fn new_declaration_name(rw: &crate::rewrite::ASTRewrite) -> Option<RNode> {
    (0..rw.new_nodes.len() as u32).find_map(|i| {
        let node = RNode::New(i);
        (rw.kind(node) == NodeKind::VariableDeclarationFragment).then(|| rw.new_value(node, "name").node()).flatten()
    })
}

/// `getFirstTrackedNodePositionBySequenceRank(positionGroup)`.
fn first_by_sequence_rank(positions: &[(RNode, i32)]) -> Option<RNode> {
    let mut target = positions.first()?;
    for p in &positions[1..] {
        if p.1 < target.1 {
            target = p;
        }
    }
    Some(target.0)
}

/// An identifier of the same length as `name` that differs from it.
fn stand_in(name: &str) -> String {
    let a: String = name.chars().map(|_| 'a').collect();
    if a != name {
        a
    } else {
        name.chars().map(|_| 'b').collect()
    }
}

/// `new RefactorWorkspaceEdit(ChangeUtil.convertToWorkspaceEdit(change),
/// renameCommand)`: the edit plus a `java.action.rename` command at the
/// tracked name in the changed document.
async fn refactor_workspace_edit(env: &Env<'_>, ast: &Arc<Ast>, mut cus: Vec<CuChange>, tracked: Option<RNode>) -> Option<Value> {
    let uri = ast.uri.clone();
    let mut rename = None;
    if let (Some(RNode::New(id)), Some(cu)) = (tracked, cus.first_mut()) {
        if let Some(rw) = cu.rewrite.as_mut().filter(|rw| rw.kind(RNode::New(id)) == NodeKind::SimpleName) {
            let name = rw.new_value(RNode::New(id), "identifier").simple().unwrap_or("").to_owned();
            // A string placeholder gives the tracked name its own insert
            // edit; a run with a same-length stand-in tells it apart from
            // other insertions of the same text.
            let mut other = rw.clone();
            other.new_nodes[id as usize].placeholder = Some(Placeholder::Str(stand_in(&name)));
            rw.new_nodes[id as usize].placeholder = Some(Placeholder::Str(name.clone()));
            let options = env.options(&uri).await;
            let other_tree = crate::rewrite::formatter::rewrite_with_bridge(&other, &options, env.dispatcher).await.ok()?;
            let tree = super::edit::cu_tree(env, cu).await.ok()?;
            let alt = stand_in(&name);
            let index = (0..other_tree.edits.len()).find(|&i| {
                matches!(&other_tree.edits[i].kind, EditKind::Insert(s) if *s == alt) && matches!(tree.edits.get(i + 1).map(|e| &e.kind), Some(EditKind::Insert(s)) if *s == name)
            })? + 1;
            let mut marker = String::from("\0jdtls-rename\0");
            let result = String::from_utf16_lossy(&tree.apply(&ast.source));
            while result.contains(&marker) {
                marker.push('\0');
            }
            let mut tracked_tree = tree.clone();
            tracked_tree.edits[index].kind = EditKind::Insert(marker.clone());
            let tracked_result = String::from_utf16_lossy(&tracked_tree.apply(&ast.source));
            let offset = tracked_result[..tracked_result.find(&marker)?].encode_utf16().count();
            rename = Some(json!({ "title": "Rename", "command": RENAME_COMMAND, "arguments": [{ "uri": uri, "offset": offset, "length": name.encode_utf16().count() }] }));
            *cu = CuChange::edits(ast.clone(), tree);
        }
    }
    let mut change = Change::Cu(cus);
    let edit = super::edit::to_workspace_edit(env, &mut change).await.ok()?;
    let mut result = json!({ "edit": edit });
    if let Some(command) = rename {
        result["command"] = command;
    }
    Some(result)
}
