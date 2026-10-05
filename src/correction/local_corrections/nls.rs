//! StringFixCore.getReplace for an unnecessary NON-NLS tag.
use crate::{
    correction::{messages, relevance, Context, ProblemLocation, Proposal},
    rewrite::indent::{is_indent_char, is_line_delimiter_char},
};

pub fn unnecessary_tag(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let mut offset = problem.offset;
    let mut length = problem.length;
    let mut replacement = "";
    let mut more = false;
    let mut next = offset + length;
    while let Some(ch) = ctx.ast.char_at(next) {
        if is_indent_char(ch) {
            next += 1;
            if ctx.ast.substring(next, next + 11) == "//$NON-NLS-" {
                break;
            }
        } else if is_line_delimiter_char(ch) {
            length = next - offset;
            break;
        } else if ch == b'/' as u16 {
            next += 1;
            if ctx.ast.char_at(next) != Some(b'/' as u16) {
                replacement = "//";
            } else {
                length = next - offset - 1;
            }
            more = true;
            break;
        } else {
            replacement = "//";
            more = true;
            break;
        }
    }
    if !more {
        while offset > 0 && ctx.ast.char_at(offset - 1).is_some_and(is_indent_char) {
            offset -= 1;
            length += 1;
        }
    }
    if length > 0 {
        proposals.push(super::replace_proposal(
            ctx,
            messages::fix("StringFix_RemoveNonNls_description"),
            offset,
            length,
            replacement,
            relevance::UNNECESSARY_NLS_TAG,
        ));
    }
}
