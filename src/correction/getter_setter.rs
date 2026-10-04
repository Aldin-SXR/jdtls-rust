//! Port of `GetterSetterCorrectionSubProcessor`.

use super::edit::Env;
use super::{Context, ProblemLocation, Proposal};

pub async fn add_getter_setter_proposal(_env: &Env<'_>, _ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>, _relevance: i32) {}
