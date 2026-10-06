//! Port of jdt.ls `SourceAssistProcessor` (the parts ported so far).

use super::edit::Env;
use super::handler::{Entry, Request};
use super::{kind, messages, Change, CuChange, LazyChange, Proposal};
use crate::semantic_ast::NodeKind;
use tower_lsp::lsp_types::{CodeActionOrCommand, Url};

/// `SourceAssistProcessor.getSourceActionCommands`: entries plus the
/// proposals that resolve them (indices start at `next_proposal`).
pub async fn source_actions(
    env: &Env<'_>,
    req: &Request<'_>,
    next_proposal: usize,
) -> Vec<(Entry, Option<Proposal>)> {
    let mut kinds = Vec::new();
    let mut node = req.context.covering_node();
    while let Some(current) = node {
        if current.is(NodeKind::ImportDeclaration) {
            kinds.push(kind::QUICK_ASSIST);
            break;
        }
        node = current.parent();
    }
    kinds.push(kind::SOURCE_ORGANIZE_IMPORTS);
    let resolve = crate::features::client_caps::resolve_code_action();
    let mut out = Vec::new();
    for kind in kinds {
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let mut proposal = Proposal::new(
            messages::ls_correction("ReorgCorrectionsSubProcessor_organizeimports_description"),
            kind,
            0,
            Change::Lazy(Box::new(OrganizeImports {
                uri: req.uri.clone(),
            })),
        );
        if let Some(mut entry) = super::handler::code_action_from_proposal(
            env,
            &req.uri,
            &mut proposal,
            &req.params.context.diagnostics,
            resolve,
            next_proposal + out.len(),
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
            if let Some(data) = entry.data.as_mut() {
                data.priority = super::handler::priority::ORGANIZE_IMPORTS;
            }
            out.push((entry, resolve.then_some(proposal)));
        }
    }
    let first = next_proposal + out.iter().filter(|(_, p)| p.is_some()).count();
    out.extend(crate::features::accessors::actions::actions(env, req, first).await);
    let first = next_proposal + out.iter().filter(|(_, p)| p.is_some()).count();
    out.extend(crate::features::constructors::actions::actions(env, req, first).await);
    let first = next_proposal + out.iter().filter(|(_, p)| p.is_some()).count();
    out.extend(crate::features::tostring::actions::actions(env, req, first).await);
    out.extend(crate::features::hashcode::actions::actions(env, req).await);
    out.extend(crate::features::delegates::actions::actions(env, req).await);
    out.extend(crate::features::overrides::actions::actions(env, req).await);
    out
}

struct OrganizeImports {
    uri: Url,
}

#[tower_lsp::async_trait]
impl LazyChange for OrganizeImports {
    fn changes_only(&self) -> bool { true }

    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(self.uri.as_str()).await;
        Ok(crate::features::organize_imports::operation::organize(
            env.dispatcher, &self.uri, &options,
        ).await?.into_iter().collect())
    }
}
