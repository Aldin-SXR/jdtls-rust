//! SourceAssistProcessor's toString prompt and direct edit proposals.
use crate::correction::{
    edit::Env,
    handler::{ActionData, Entry, Request},
    kind, Change, CuChange, LazyChange, Proposal,
};
use crate::features::constructors::LspVariableBinding;
use crate::features::{accessors, java_model::TypeKind};
use crate::semantic_ast::Ast;
use std::sync::Arc;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command, Range};

pub(crate) async fn actions(
    env: &Env<'_>,
    req: &Request<'_>,
    first: usize,
) -> Vec<(Entry, Option<Proposal>)> {
    let mut out = Vec::new();
    let Some(selected) = accessors::selection(&req.context, &req.uri) else {
        return out;
    };
    if selected.model.anonymous
        || matches!(
            selected.model.kind,
            TypeKind::Annotation | TypeKind::Interface | TypeKind::Enum
        )
    {
        return out;
    }
    let covered = accessors::actions::fully_covered(req);
    let nodes = if covered.is_empty() {
        req.context.covering_node().into_iter().collect()
    } else {
        covered
    };
    let quick =
        !super::exists(&selected) && nodes.iter().any(|n| accessors::actions::infer_type(*n));
    // SourceAssistProcessor.hasFields uses IType.getFields, omitting record components
    // and inherited fields. Empty types have a direct action without prompt support.
    let direct = !selected.model.members.iter().any(|m| matches!(m,crate::features::java_model::Member::Field(f) if !f.enum_constant && !f.record_component && f.flags & crate::features::java_model::flags::STATIC==0));
    if !direct
        && !crate::features::preferences::extended_capability("generateToStringPromptSupport")
    {
        return out;
    }
    let resolve = crate::features::client_caps::resolve_code_action();
    for action_kind in [kind::QUICK_ASSIST, kind::SOURCE_GENERATE_TO_STRING] {
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
                "Generate toString()",
                action_kind,
                0,
                Change::Lazy(Box::new(ToStringChange {
                    ast: req.context.ast.clone(),
                    selected: selected.clone(),
                    fields: Vec::new(),
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
                entry.data = Some(ActionData {
                    proposal: resolve.then_some(index),
                    priority: 40,
                });
                out.push((entry, resolve.then_some(proposal)));
            }
        } else {
            let title = "Generate toString()...".to_owned();
            let command = Command {
                title: title.clone(),
                command: "java.action.generateToStringPrompt".into(),
                arguments: Some(vec![
                    serde_json::to_value(req.params).expect("CodeActionParams")
                ]),
            };
            let entry = if crate::features::client_caps::supported_code_action_kind(
                kind::SOURCE_GENERATE_TO_STRING,
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
                        priority: 40,
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
struct ToStringChange {
    ast: Arc<Ast>,
    selected: accessors::Selection,
    fields: Vec<LspVariableBinding>,
    cursor: Range,
}
#[tower_lsp::async_trait]
impl LazyChange for ToStringChange {
    fn changes_only(&self) -> bool {
        true
    }
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        Ok(vec![
            super::create_change(
                env,
                self.ast.clone(),
                &self.selected,
                &self.fields,
                Some(self.cursor),
            )
            .await?,
        ])
    }
}
