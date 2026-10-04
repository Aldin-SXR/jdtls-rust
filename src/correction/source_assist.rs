//! Port of jdt.ls `SourceAssistProcessor` (the parts ported so far).

use super::edit::Env;
use super::handler::{Entry, Request};
use super::Proposal;

/// `SourceAssistProcessor.getSourceActionCommands`: entries plus the
/// proposals that resolve them (indices start at `next_proposal`).
pub async fn source_actions(_env: &Env<'_>, _req: &Request<'_>, _next_proposal: usize) -> Vec<(Entry, Option<Proposal>)> {
    Vec::new()
}
