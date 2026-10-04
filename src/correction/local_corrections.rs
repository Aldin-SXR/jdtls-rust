//! Port of `LocalCorrectionsSubProcessor` / `LocalCorrectionsBaseSubProcessor`.

use super::edit::Env;
use super::{Context, ProblemLocation, Proposal};

pub fn add_quote(_ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
pub fn redundant_super_interface(_ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
pub fn superfluous_semicolon(_ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
pub fn unnecessary_cast(_ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
pub async fn correct_access_to_static(_env: &Env<'_>, _ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>) {}
