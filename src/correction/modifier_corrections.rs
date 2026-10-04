//! Port of `ModifierCorrectionSubProcessor(Core)`.

use super::edit::Env;
use super::{Context, ProblemLocation, Proposal};

pub const TO_STATIC: i32 = 1;
pub const TO_VISIBLE: i32 = 2;
pub const TO_NON_PRIVATE: i32 = 3;
pub const TO_NON_STATIC: i32 = 4;
pub const TO_NON_FINAL: i32 = 5;

pub async fn non_accessible_reference(_env: &Env<'_>, _ctx: &Context, _problem: &ProblemLocation, _proposals: &mut Vec<Proposal>, _kind: i32, _relevance: i32) {}
