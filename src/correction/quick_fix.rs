//! Port of jdt.ls `QuickFixProcessor` (the problem id → sub-processor
//! dispatch) and `NonProjectFixProcessor`.

use std::collections::HashSet;

use serde_json::json;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Command};

use super::edit::Env;
use super::handler::{Entry, Request};
use super::{kind, messages, ProblemLocation, Proposal};
use crate::semantic_ast::problem as p;

/// `DiagnosticsHandler.NON_PROJECT_JAVA_FILE` / `NOT_ON_CLASSPATH`.
pub const NON_PROJECT_JAVA_FILE: i32 = 0x10;
pub const NOT_ON_CLASSPATH: i32 = 0x20;

/// `NonProjectFixProcessor.getCorrections`.
pub fn non_project_fixes(env: &Env<'_>, req: &Request<'_>) -> Vec<Entry> {
    let mut out = Vec::new();
    let uri = req.uri.as_str();
    for loc in &req.locations {
        if loc.problem_id == NON_PROJECT_JAVA_FILE || loc.problem_id == NOT_ON_CLASSPATH {
            let syntax_only = !env.lifecycle.is_only_syntax_reported(&req.uri);
            let (file, session) = if syntax_only {
                ("ReportSyntaxErrorsForThisFile", "ReportSyntaxErrorsForAnyNonProjectFile")
            } else {
                ("ReportAllErrorsForThisFile", "ReportAllErrorsForAnyNonProjectFile")
            };
            out.push(diagnostics_fix(messages::ls_action(file), uri, "thisFile", syntax_only));
            out.push(diagnostics_fix(messages::ls_action(session), uri, "anyNonProjectFile", syntax_only));
        }
    }
    out
}

fn diagnostics_fix(message: &str, uri: &str, scope: &str, syntax_only: bool) -> Entry {
    let command = Command {
        title: message.to_owned(),
        command: "java.project.refreshDiagnostics".into(),
        arguments: Some(vec![json!(uri), json!(scope), json!(syntax_only)]),
    };
    if crate::features::client_caps::supported_code_action_kind(kind::QUICK_FIX) {
        let ca = CodeAction {
            title: message.to_owned(),
            kind: Some(kind::QUICK_FIX.into()),
            command: Some(command),
            diagnostics: Some(Vec::new()),
            ..Default::default()
        };
        Entry { action: CodeActionOrCommand::CodeAction(ca), data: None }
    } else {
        Entry::command(command)
    }
}

/// `QuickFixProcessor.handledProblems`: one location per problem id (and
/// `UndefinedName` is skipped when an `UndefinedType` has the same arguments).
fn handled(location: &ProblemLocation, locations: &[ProblemLocation], handled: &mut HashSet<i32>) -> bool {
    let id = location.problem_id;
    if handled.contains(&id) {
        return false;
    }
    if id == p::UndefinedName && locations.iter().any(|l| l.problem_id == p::UndefinedType && l.arguments == location.arguments) {
        handled.insert(id);
        return false;
    }
    handled.insert(id)
}

/// `QuickFixProcessor.getCorrections`.
pub async fn corrections(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    let mut seen = HashSet::new();
    for loc in &req.locations {
        if handled(loc, &req.locations, &mut seen) {
            process(env, req, loc, &mut proposals).await;
        }
    }
    proposals
}

/// `QuickFixProcessor.process`.
async fn process(env: &Env<'_>, req: &Request<'_>, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let id = problem.problem_id;
    if id == 0 {
        return;
    }
    let ctx = &req.context;
    match id {
        p::UnterminatedString => super::local_corrections::add_quote(ctx, problem, proposals),
        p::RedundantSuperinterface => super::local_corrections::redundant_super_interface(ctx, problem, proposals),
        p::NonStaticAccessToStaticField
        | p::NonStaticAccessToStaticMethod
        | p::NonStaticOrAlienTypeReceiver
        | p::IndirectAccessToStaticField
        | p::IndirectAccessToStaticMethod => super::local_corrections::correct_access_to_static(env, ctx, problem, proposals).await,
        p::StaticMethodRequested | p::NonStaticFieldFromStaticInvocation | p::InstanceMethodDuringConstructorInvocation | p::InstanceFieldDuringConstructorInvocation => {
            super::modifier_corrections::non_accessible_reference(env, ctx, problem, proposals, super::modifier_corrections::TO_STATIC, super::relevance::CHANGE_MODIFIER_TO_STATIC).await
        }
        p::NonBlankFinalLocalAssignment
        | p::DuplicateFinalLocalInitialization
        | p::FinalFieldAssignment
        | p::DuplicateBlankFinalFieldInitialization
        | p::AnonymousClassCannotExtendFinalClass
        | p::ClassExtendFinalClass => {
            super::modifier_corrections::non_accessible_reference(env, ctx, problem, proposals, super::modifier_corrections::TO_NON_FINAL, super::relevance::REMOVE_FINAL_MODIFIER).await
        }
        p::NotVisibleField => {
            super::getter_setter::add_getter_setter_proposal(env, ctx, problem, proposals, super::relevance::GETTER_SETTER_NOT_VISIBLE_FIELD).await;
            super::modifier_corrections::non_accessible_reference(env, ctx, problem, proposals, super::modifier_corrections::TO_VISIBLE, super::relevance::CHANGE_VISIBILITY).await
        }
        p::NotVisibleMethod | p::NotVisibleConstructor | p::NotVisibleType | p::JavadocNotVisibleType => {
            super::modifier_corrections::non_accessible_reference(env, ctx, problem, proposals, super::modifier_corrections::TO_VISIBLE, super::relevance::CHANGE_VISIBILITY).await
        }
        p::NeedToEmulateFieldReadAccess | p::NeedToEmulateFieldWriteAccess | p::NeedToEmulateMethodAccess | p::NeedToEmulateConstructorAccess => {
            super::modifier_corrections::non_accessible_reference(env, ctx, problem, proposals, super::modifier_corrections::TO_NON_PRIVATE, super::relevance::CHANGE_VISIBILITY_TO_NON_PRIVATE).await
        }
        p::SuperfluousSemicolon => super::local_corrections::superfluous_semicolon(ctx, problem, proposals),
        p::UnnecessaryCast => super::local_corrections::unnecessary_cast(ctx, problem, proposals),
        p::UnqualifiedFieldAccess => {
            super::getter_setter::add_getter_setter_proposal(env, ctx, problem, proposals, super::relevance::GETTER_SETTER_UNQUALIFIED_FIELD_ACCESS).await;
        }
        p::MissingSerialVersion => super::serial_version::serial_version_proposals(ctx, problem, proposals),
        p::BodyForAbstractMethod | p::AbstractMethodInAbstractClass | p::AbstractMethodInEnum | p::EnumAbstractMethodMustBeImplemented => super::modifier_corrections::abstract_method(ctx, problem, proposals),
        p::AbstractMethodsInConcreteClass => super::modifier_corrections::abstract_type(ctx, problem, proposals),
        p::BodyForNativeMethod => super::modifier_corrections::native_method(ctx, problem, proposals),
        p::MethodRequiresBody => super::modifier_corrections::requires_body(ctx, problem, proposals),
        p::AbstractMethodMustBeImplemented | p::EnumConstantMustImplementAbstractMethod => super::unimplemented::proposals(ctx, problem, proposals),
        _ => {}
    }
}

/// `QuickFixProcessor.addAddAllMissingImportsProposal`: only when an
/// `AddImportCorrectionProposal` is among the proposals.
pub async fn add_all_missing_imports_proposal(_env: &Env<'_>, _req: &Request<'_>, _proposals: &mut Vec<Proposal>) {
    // `AddImportCorrectionProposal`s (unresolved types) are not produced by
    // the Rust processors yet, so there is nothing to add.
}
