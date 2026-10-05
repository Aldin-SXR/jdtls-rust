//! SourceAssistProcessor's override/implement prompt actions.
use crate::correction::{
    edit::Env,
    handler::{priority, ActionData, Entry, Request},
    kind, Proposal,
};
use crate::features::accessors;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command};
pub(crate) async fn actions(_env: &Env<'_>, req: &Request<'_>) -> Vec<(Entry, Option<Proposal>)> {
    if !crate::features::preferences::extended_capability("overrideMethodsPromptSupport") {
        return Vec::new();
    }
    if ["module-info.java", "package-info.java"]
        .iter()
        .any(|name| req.uri.path().ends_with(name))
    {
        return Vec::new();
    }
    let covered = accessors::actions::fully_covered(req);
    let nodes = if covered.is_empty() {
        req.context.covering_node().into_iter().collect()
    } else {
        covered
    };
    let quick = nodes.iter().any(|n| accessors::actions::infer_type(*n));
    let mut out = Vec::new();
    for action_kind in [kind::QUICK_ASSIST, kind::SOURCE_OVERRIDE_METHODS] {
        if action_kind == kind::QUICK_ASSIST && !quick {
            continue;
        }
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| action_kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let title = "Override/Implement Methods...".to_owned();
        let command = Command {
            title: title.clone(),
            command: "java.action.overrideMethodsPrompt".into(),
            arguments: Some(vec![
                serde_json::to_value(req.params).expect("CodeActionParams")
            ]),
        };
        let entry = if crate::features::client_caps::supported_code_action_kind(
            kind::SOURCE_OVERRIDE_METHODS,
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
                    priority: priority::GENERATE_OVERRIDE_IMPLEMENT,
                }),
            }
        } else {
            if req.params.context.only.as_ref().is_some_and(|only| {
                !only.is_empty() && !only.iter().any(|k| command.command.starts_with(k.as_str()))
            }) {
                continue;
            }
            Entry::command(command)
        };
        out.push((entry, None));
    }
    out
}
