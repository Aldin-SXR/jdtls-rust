//! Port of jdt.ls `SortTextHelper`.

use super::proposal::{kind, Proposal};

pub const CEILING: i64 = 999_999_999;
pub const MAX_RELEVANCE_VALUE: i64 = 99_999_999;

/// `convertRelevance`: converts the relevance to a 9-digit sort text, so that
/// higher relevance gets a lower sort text. `Err` is upstream's
/// `IllegalArgumentException` for a relevance above [`MAX_RELEVANCE_VALUE`].
pub fn try_convert_relevance(relevance: i64) -> Result<String, &'static str> {
    if relevance > MAX_RELEVANCE_VALUE {
        return Err("Relevance must be lower than 100,000,000");
    }
    Ok((CEILING - relevance.max(0)).to_string())
}

/// [`try_convert_relevance`] for the relevances jdt.ls computes, which stay
/// far below [`MAX_RELEVANCE_VALUE`] (JDT relevance * 16 + kind).
pub fn convert_relevance(relevance: i64) -> String {
    try_convert_relevance(relevance).expect("Relevance must be lower than 100,000,000")
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

/// Port of `org.eclipse.jdt.ls.core.internal.contentassist.SortTextHelperTest`.
#[cfg(test)]
mod sort_text_helper_test {
    use super::*;

    #[test]
    fn test_convert_relevance() {
        let mut result: Vec<String> = Vec::new();
        let mut i: i64 = 0;
        while i < 1000 {
            let relevance = convert_relevance(i);
            if i > 0 {
                let prev = (i - 1) as usize;
                let previous = &result[prev];
                assert!(
                    relevance.as_str() < previous.as_str(),
                    "relevance {i} should be sorted before {prev} : {relevance} vs {previous}"
                );
            }
            result.push(relevance);
            i += 1;
        }

        //Try some boundaries
        let min = convert_relevance(i32::MIN as i64);
        let zero = convert_relevance(0);
        assert_eq!(zero, min); //negative relevance is irrelevant

        i = MAX_RELEVANCE_VALUE;
        let max = convert_relevance(i);
        assert_eq!("900000000", max);

        assert!(
            try_convert_relevance(i32::MAX as i64).is_err(),
            "Values greater than {MAX_RELEVANCE_VALUE} are not supported"
        );
    }
}
