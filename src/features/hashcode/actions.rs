//! SourceAssistProcessor's hashCode/equals prompt actions.
use crate::correction::{
    edit::Env,
    handler::{priority, ActionData, Entry, Request},
    kind, Proposal,
};
use crate::features::{
    accessors,
    java_model::{Member, TypeKind},
};
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command};
pub(crate) async fn actions(_env: &Env<'_>, req: &Request<'_>) -> Vec<(Entry, Option<Proposal>)> {
    let Some(selected) = accessors::selection(&req.context, &req.uri) else {
        return Vec::new();
    };
    if matches!(
        selected.model.kind,
        TypeKind::Annotation | TypeKind::Interface | TypeKind::Enum
    ) || !crate::features::preferences::extended_capability("hashCodeEqualsPromptSupport")
    {
        return Vec::new();
    }
    let Some(binding) = req.context.ast.node(selected.declaration).binding() else {
        return Vec::new();
    };
    if !binding
        .declared_fields()
        .unwrap_or_default()
        .iter()
        .any(|f| !f.is_static())
    {
        return Vec::new();
    }
    // The action checks Java-model declarations, so implicit record methods do
    // not suppress the prompt, while status reports the DOM's implicit methods.
    let has_hash = selected
        .model
        .members
        .iter()
        .any(|m| matches!(m,Member::Method(m) if m.name=="hashCode" && m.params.is_empty()));
    // CodeActionUtility compares Class.getName() with raw JDT signatures;
    // the inverted check consequently accepts ordinary one-argument overloads.
    let has_equals = selected
        .model
        .members
        .iter()
        .any(|m| matches!(m,Member::Method(m) if m.name=="equals" && m.params.len()==1));
    let covered = accessors::actions::fully_covered(req);
    let nodes = if covered.is_empty() {
        req.context.covering_node().into_iter().collect()
    } else {
        covered
    };
    let quick =
        !(has_hash && has_equals) && nodes.iter().any(|n| accessors::actions::infer_type(*n));
    let mut out = Vec::new();
    for action_kind in [kind::QUICK_ASSIST, kind::SOURCE_GENERATE_HASHCODE_EQUALS] {
        if action_kind == kind::QUICK_ASSIST && !quick {
            continue;
        }
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| action_kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let title = "Generate hashCode() and equals()...".to_owned();
        let command = Command {
            title: title.clone(),
            command: "java.action.hashCodeEqualsPrompt".into(),
            arguments: Some(vec![
                serde_json::to_value(req.params).expect("CodeActionParams")
            ]),
        };
        let entry = if crate::features::client_caps::supported_code_action_kind(
            kind::SOURCE_GENERATE_HASHCODE_EQUALS,
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
                    priority: priority::GENERATE_HASHCODE_EQUALS,
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
