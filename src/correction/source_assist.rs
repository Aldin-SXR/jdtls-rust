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
            kinds.push((kind::QUICK_ASSIST, false));
            break;
        }
        node = current.parent();
    }
    kinds.push((kind::SOURCE_ORGANIZE_IMPORTS, false));
    if req.context.ast.problems.iter().any(|problem| {
        matches!(
            problem.id,
            crate::semantic_ast::problem::UndefinedType
                | crate::semantic_ast::problem::JavadocUndefinedType
        )
    }) {
        kinds.push((kind::SOURCE, true));
    }
    let resolve = crate::features::client_caps::resolve_code_action();
    let mut out = Vec::new();
    for (kind, restore) in kinds {
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let mut proposal = Proposal::new(
            messages::ls_correction(if restore {
                "UnresolvedElementsSubProcessor_add_allMissing_imports_description"
            } else {
                "ReorgCorrectionsSubProcessor_organizeimports_description"
            }),
            kind,
            0,
            Change::Lazy(Box::new(OrganizeImports {
                uri: req.uri.clone(),
                restore,
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
                data.priority = if restore {
                    super::handler::priority::ADD_ALL_MISSING_IMPORTS
                } else {
                    super::handler::priority::ORGANIZE_IMPORTS
                };
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
    restore: bool,
}

#[tower_lsp::async_trait]
impl LazyChange for OrganizeImports {
    fn changes_only(&self) -> bool {
        true
    }

    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(self.uri.as_str()).await;
        let interactive = env
            .format
            .extended_client_capabilities
            .as_ref()
            .and_then(|caps| caps.get("advancedOrganizeImportsSupport"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        // SourceAssist uses IResource.getLocationURI().toString(), whereas the
        // direct handler forwards the original client URI. Java's file URI
        // rendering has one slash before the absolute path.
        let chooser_uri = interactive.then(|| {
            crate::project::uri_to_path(&self.uri)
                .map(|path| crate::project::java_file_uri(&path, false))
                .unwrap_or_else(|| self.uri.to_string())
        });
        Ok(
            crate::features::organize_imports::operation::organize_action(
                env.format,
                &self.uri,
                &options,
                self.restore,
                chooser_uri,
            )
            .await?
            .into_iter()
            .collect(),
        )
    }
}
