//! SourceAssistProcessor's delegate prompt (source action only).
use crate::{
    correction::{
        edit::Env,
        handler::{priority, ActionData, Entry, Request},
        kind, Proposal,
    },
    features::{
        accessors,
        java_model::{Member, TypeKind},
        preferences,
    },
};
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command};
pub(crate) async fn actions(_env: &Env<'_>, req: &Request<'_>) -> Vec<(Entry, Option<Proposal>)> {
    if !preferences::extended_capability("generateDelegateMethodsPromptSupport") {
        return Vec::new();
    }
    let Some(selected) = accessors::selection(&req.context, &req.uri) else {
        return Vec::new();
    };
    if matches!(
        selected.model.kind,
        TypeKind::Interface | TypeKind::Annotation
    ) {
        return Vec::new();
    }
    let Some(binding) = req.context.ast.node(selected.declaration).binding() else {
        return Vec::new();
    };
    // Java-model IType.getFields does not include record components. Source
    // type-variable references carry unresolved Q signatures and qualify.
    if !binding
        .declared_fields()
        .unwrap_or_default()
        .iter()
        .any(|f| {
            !f.is_enum_constant()
                && !selected.model.members.iter().any(|m| matches!(m, Member::Field(model) if model.name == f.name() && model.record_component))
                && f.var_type()
                    .is_some_and(|t| !t.is_primitive() && !t.is_array())
        })
    {
        return Vec::new();
    }
    let title = "Generate Delegate Methods...".to_owned();
    let command = Command {
        title: title.clone(),
        command: "java.action.generateDelegateMethodsPrompt".into(),
        arguments: Some(vec![
            serde_json::to_value(req.params).expect("CodeActionParams")
        ]),
    };
    let action_kind = kind::SOURCE_GENERATE_DELEGATE_METHODS;
    let literal = crate::features::client_caps::supported_code_action_kind(action_kind);
    let filter = if literal {
        action_kind
    } else {
        &command.command
    };
    if req.params.context.only.as_ref().is_some_and(|only| {
        !only.is_empty() && !only.iter().any(|k| filter.starts_with(k.as_str()))
    }) {
        return Vec::new();
    }
    let entry = if literal {
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
                priority: priority::GENERATE_DELEGATE_METHOD,
            }),
        }
    } else {
        Entry::command(command)
    };
    vec![(entry, None)]
}
