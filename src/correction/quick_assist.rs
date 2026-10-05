//! Port of jdt.ls `QuickAssistProcessor` and `RefactorProcessor` (the parts
//! ported so far).

use super::edit::Env;
use super::handler::Request;
use super::Proposal;

/// `QuickAssistProcessor.getAssists`.
pub async fn assists(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    super::assign_to_field::assign_param_to_field_proposals(env, &req.context, &mut proposals)
        .await;
    if !req.locations.iter().any(|p| {
        matches!(
            p.problem_id,
            crate::semantic_ast::problem::UnclosedCloseable
                | crate::semantic_ast::problem::PotentiallyUnclosedCloseable
                | crate::semantic_ast::problem::UnhandledException
        )
    }) {
        super::local_corrections::resource_assist(env, &req.context, &mut proposals).await;
    }
    proposals
}

/// `RefactorProcessor.getProposals`.
pub async fn refactor_proposals(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    super::local_corrections::assignment_refactors(env, req).await
}
