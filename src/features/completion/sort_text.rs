//! Port of jdt.ls `SortTextHelper`.

use super::proposal::{kind, Proposal};

pub const CEILING: i64 = 999_999_999;
pub const MAX_RELEVANCE_VALUE: i64 = 99_999_999;

/// `convertRelevance`: higher relevance → lower sort text.
pub fn convert_relevance(relevance: i64) -> String {
    (CEILING - relevance.max(0)).to_string()
}

/// `computeSortText`.
pub fn compute(p: &Proposal) -> String {
    let base = p.relevance as i64 * 16;
    let r = match p.kind {
        kind::LABEL_REF => base + 1,
        kind::KEYWORD => base + 2,
        kind::TYPE_REF | kind::ANONYMOUS_CLASS_DECLARATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION => base + 3,
        kind::METHOD_REF
        | kind::CONSTRUCTOR_INVOCATION
        | kind::METHOD_NAME_REFERENCE
        | kind::METHOD_DECLARATION
        | kind::ANNOTATION_ATTRIBUTE_REF
        | kind::POTENTIAL_METHOD_DECLARATION => base + 4,
        kind::FIELD_REF => base + 5,
        kind::LOCAL_VARIABLE_REF | kind::VARIABLE_DECLARATION => base + 6,
        _ => base,
    };
    convert_relevance(r)
}
