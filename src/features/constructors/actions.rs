//! SourceAssistProcessor's constructor prompt and direct edit proposals.
use super::{LspMethodBinding, LspVariableBinding};
use crate::correction::{
    edit::Env,
    handler::{ActionData, Entry, Request},
    kind, Change, CuChange, LazyChange, Proposal,
};
use crate::features::{accessors, java_model::TypeKind};
use crate::semantic_ast::{Ast, Node, NodeKind};
use std::sync::Arc;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command, Range};

pub(crate) async fn actions(
    env: &Env<'_>,
    req: &Request<'_>,
    first: usize,
) -> Vec<(Entry, Option<Proposal>)> {
    let mut out = Vec::new();
    if !crate::features::preferences::extended_capability("generateConstructorsPromptSupport") {
        return out;
    }
    let Some(selected) = accessors::selection(&req.context, &req.uri) else {
        return out;
    };
    if selected.model.anonymous
        || matches!(
            selected.model.kind,
            TypeKind::Annotation | TypeKind::Interface
        )
    {
        return out;
    }
    let status = super::status(&req.context, &selected);
    if status.constructors.is_empty() {
        return out;
    }
    let covered = accessors::actions::fully_covered(req);
    let nodes = if covered.is_empty() {
        req.context.covering_node().into_iter().collect()
    } else {
        covered
    };
    let quick = nodes.iter().any(|n| {
        accessors::actions::infer_type(*n)
            || field(*n).is_some_and(|n| {
                n.list("modifiers")
                    .iter()
                    .all(|m| m.simple("keyword") != Some("static"))
            })
    });
    let resolve = crate::features::client_caps::resolve_code_action();
    let direct = status.constructors.len() == 1 && status.fields.is_empty();
    for action_kind in [kind::QUICK_ASSIST, kind::SOURCE_GENERATE_CONSTRUCTORS] {
        if action_kind == kind::QUICK_ASSIST && !quick {
            continue;
        }
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| action_kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        if direct {
            let mut proposal = Proposal::new(
                "Generate Constructors",
                action_kind,
                0,
                Change::Lazy(Box::new(ConstructorsChange {
                    ast: req.context.ast.clone(),
                    selected: selected.clone(),
                    constructors: status.constructors.clone(),
                    fields: status.fields.clone(),
                    cursor: req.params.range,
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
                if let CodeActionOrCommand::CodeAction(action) = &mut entry.action {
                    action.diagnostics = Some(if resolve {
                        Vec::new()
                    } else {
                        req.params.context.diagnostics.clone()
                    });
                }
                // `getCodeActionFromProposal`: data only for clients that resolve.
                entry.data = resolve.then_some(ActionData {
                    proposal: resolve.then_some(index),
                    priority: 20,
                });
                out.push((entry, resolve.then_some(proposal)));
            }
        } else {
            let title = "Generate Constructors...".to_owned();
            let command = Command {
                title: title.clone(),
                command: "java.action.generateConstructorsPrompt".into(),
                arguments: Some(vec![
                    serde_json::to_value(req.params).expect("CodeActionParams")
                ]),
            };
            let entry = if crate::features::client_caps::supported_code_action_kind(
                kind::SOURCE_GENERATE_CONSTRUCTORS,
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
                        priority: 20,
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
        }
    }
    out
}
fn field(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        if node.is(NodeKind::FieldDeclaration) {
            return Some(node);
        }
        if node.kind().is_body_declaration() || node.kind().is_statement() {
            return None;
        }
        node = node.parent()?;
    }
}
struct ConstructorsChange {
    ast: Arc<Ast>,
    selected: accessors::Selection,
    constructors: Vec<LspMethodBinding>,
    fields: Vec<LspVariableBinding>,
    cursor: Range,
}
#[tower_lsp::async_trait]
impl LazyChange for ConstructorsChange {
    fn changes_only(&self) -> bool {
        true
    }
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        Ok(vec![
            super::create_change(
                env,
                self.ast.clone(),
                &self.selected,
                &self.constructors,
                &self.fields,
                Some(self.cursor),
            )
            .await?,
        ])
    }
}
