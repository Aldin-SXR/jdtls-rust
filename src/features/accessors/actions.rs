//! SourceAssistProcessor's getter/setter actions, including prompt capability,
//! selection-only quick assists and deferred changes.
use super::{AccessorField, AccessorKind, Selection};
use crate::correction::{
    edit::Env,
    handler::{ActionData, Entry, Request},
    kind, Change, CuChange, LazyChange, Proposal,
};
use crate::semantic_ast::{Ast, Node, NodeKind};
use serde_json::json;
use std::sync::Arc;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command, Range};

pub(crate) async fn actions(
    env: &Env<'_>,
    req: &Request<'_>,
    first: usize,
) -> Vec<(Entry, Option<Proposal>)> {
    let mut out = Vec::new();
    let Some(selected) = super::selection(&req.context, &req.uri) else {
        return out;
    };
    let (options, _) = env.dispatcher.options_for(Some(&req.uri)).await;
    let profile = super::profile(env.dispatcher, &req.uri).await;
    let both = super::unimplemented(&selected.model, AccessorKind::BOTH, &options, &profile);
    let getters = super::unimplemented(&selected.model, AccessorKind::GETTER, &options, &profile);
    let setters = super::unimplemented(&selected.model, AccessorKind::SETTER, &options, &profile);
    let covered = fully_covered(req);
    let covering = req.context.covering_node();
    let nodes = if covered.is_empty() {
        covering.into_iter().collect()
    } else {
        covered
    };
    let is_in_type = nodes.iter().any(|n| infer_type(*n));
    let names: Vec<String> = nodes.iter().flat_map(|n| field_names(*n)).collect();
    let mut offers = Vec::new();
    for (accessors, accessor_kind) in [
        (&both, AccessorKind::BOTH),
        (&getters, AccessorKind::GETTER),
        (&setters, AccessorKind::SETTER),
    ] {
        let selected_fields: Vec<_> = accessors
            .iter()
            .filter(|a| {
                names.contains(&a.field_name)
                    && (!matches!(accessor_kind, AccessorKind::BOTH)
                        || a.generate_getter && a.generate_setter)
            })
            .cloned()
            .collect();
        if !selected_fields.is_empty() {
            offers.push((kind::QUICK_ASSIST, accessor_kind, selected_fields));
        }
    }
    for (accessors, accessor_kind) in [
        (both, AccessorKind::BOTH),
        (getters.clone(), AccessorKind::GETTER),
        (setters.clone(), AccessorKind::SETTER),
    ] {
        if accessors.is_empty()
            || matches!(accessor_kind, AccessorKind::BOTH)
                && (getters.is_empty() || setters.is_empty())
        {
            continue;
        }
        if is_in_type {
            offers.push((kind::QUICK_ASSIST, accessor_kind, accessors.clone()));
        }
        offers.push((kind::SOURCE_GENERATE_ACCESSORS, accessor_kind, accessors));
    }
    let resolve = crate::features::client_caps::resolve_code_action();
    for (action_kind, accessor_kind, accessors) in offers {
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| action_kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let quick = action_kind == kind::QUICK_ASSIST;
        let prompt = !quick
            && accessors.len() > 1
            && crate::features::preferences::extended_capability(
                "advancedGenerateAccessorsSupport",
            );
        let plural = match accessor_kind {
            AccessorKind::BOTH => "Generate Getters and Setters",
            AccessorKind::GETTER => "Generate Getters",
            AccessorKind::SETTER => "Generate Setters",
        };
        let title = if prompt {
            format!("{plural}...")
        } else if quick && accessors.len() == 1 {
            let singular = match accessor_kind {
                AccessorKind::BOTH => "Getter and Setter",
                AccessorKind::GETTER => "Getter",
                AccessorKind::SETTER => "Setter",
            };
            format!("Generate {singular} for '{}'", accessors[0].field_name)
        } else {
            plural.to_owned()
        };
        if prompt {
            let mut argument = serde_json::to_value(req.params).expect("code action params");
            argument["kind"] = json!(accessor_kind);
            let command = Command {
                title: title.clone(),
                command: "java.action.generateAccessorsPrompt".into(),
                arguments: Some(vec![argument]),
            };
            let entry = if crate::features::client_caps::supported_code_action_kind(
                kind::SOURCE_GENERATE_ACCESSORS,
            ) {
                Entry {
                    action: CodeActionOrCommand::CodeAction(CodeAction {
                        title,
                        kind: Some(action_kind.into()),
                        command: Some(command),
                        diagnostics: Some(Vec::new()),
                        ..Default::default()
                    }),
                    data: Some(ActionData {
                        proposal: None,
                        priority: 10,
                    }),
                }
            } else {
                if req.params.context.only.as_ref().is_some_and(|only| {
                    !only.is_empty()
                        && !only.iter().any(|k| command.command.starts_with(k.as_str()))
                }) {
                    continue;
                }
                Entry::command(command)
            };
            out.push((entry, None));
        } else {
            let mut proposal = Proposal::new(
                title,
                action_kind,
                0,
                Change::Lazy(Box::new(AccessorsChange {
                    ast: req.context.ast.clone(),
                    selected: selected.clone(),
                    accessors,
                    cursor: (!is_in_type).then_some(req.params.range),
                })),
            );
            let index = first + out.iter().filter(|(_, p)| p.is_some()).count();
            if let Some(mut entry) = crate::correction::handler::code_action_from_proposal(
                env,
                &req.uri,
                &mut proposal,
                &req.params.context.diagnostics,
                resolve,
                index,
            )
            .await
            {
                if let CodeActionOrCommand::CodeAction(a) = &mut entry.action {
                    a.diagnostics = Some(if resolve {
                        Vec::new()
                    } else {
                        req.params.context.diagnostics.clone()
                    });
                }
                entry.data = Some(ActionData {
                    proposal: resolve.then_some(index),
                    priority: 10,
                });
                out.push((entry, resolve.then_some(proposal)));
            }
        }
    }
    out
}

struct AccessorsChange {
    ast: Arc<Ast>,
    selected: Selection,
    accessors: Vec<AccessorField>,
    cursor: Option<Range>,
}
#[tower_lsp::async_trait]
impl LazyChange for AccessorsChange {
    fn changes_only(&self) -> bool {
        true
    }
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let comments =
            crate::features::preferences::get_bool("java.codeGeneration.generateComments")
                .unwrap_or(false);
        Ok(vec![
            super::create_change(
                env,
                self.ast.clone(),
                &self.selected,
                &self.accessors,
                comments,
                self.cursor,
            )
            .await?,
        ])
    }
}
fn fully_covered<'a>(req: &'a Request<'_>) -> Vec<Node<'a>> {
    fn visit<'a>(node: Node<'a>, start: usize, end: usize, out: &mut Vec<Node<'a>>) {
        if node.end() < start || end < node.start() {
            return;
        }
        if start <= node.start() && node.end() <= end {
            let parent_covered = node
                .parent()
                .is_some_and(|p| start <= p.start() && p.end() <= end);
            if !parent_covered {
                out.push(node);
                return;
            }
        }
        for child in node.children() {
            visit(child, start, end, out);
        }
    }
    let mut out = Vec::new();
    if let Some(covering) = req.context.covering_node() {
        visit(
            covering,
            req.context.selection_offset,
            req.context.selection_offset + req.context.selection_length,
            &mut out,
        );
    }
    out
}
fn infer_type(mut node: Node<'_>) -> bool {
    loop {
        if node.is(NodeKind::TypeDeclaration) {
            return true;
        }
        if node.kind().is_body_declaration() || node.kind().is_statement() {
            return false;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}
fn field_names(node: Node<'_>) -> Vec<String> {
    match node.kind() {
        NodeKind::SimpleName
            if node
                .parent()
                .is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) =>
        {
            return field_names(node.parent().unwrap())
        }
        NodeKind::VariableDeclarationFragment => {
            return node
                .child("name")
                .into_iter()
                .map(|n| n.identifier())
                .collect()
        }
        NodeKind::FieldDeclaration => {
            return node
                .list("fragments")
                .into_iter()
                .flat_map(field_names)
                .collect()
        }
        _ => {}
    }
    if crate::features::preferences::quickfix_show_at() == "line" {
        let mut parent = node.parent();
        while let Some(p) = parent {
            if p.is(NodeKind::FieldDeclaration) {
                return field_names(p);
            }
            parent = p.parent();
        }
    }
    Vec::new()
}
