//! `CoreJavaDocCommentReader` (a Javadoc comment without `/**`, `*/` and
//! the leading stars) and `JavadocContentAccess2.getPlainTextContent`.

use crate::javadoc::converter::javadoc_to_plain_text;
use crate::javadoc::text_reader::JavaDoc2HtmlTextReader;

fn is_line_delimiter(c: u16) -> bool {
    c == b'\n' as u16 || c == b'\r' as u16
}

fn is_java_whitespace(c: u16) -> bool {
    char::from_u32(c as u32).is_some_and(crate::javadoc::text_reader::java_is_whitespace)
}

/// `new CoreJavaDocCommentReader(source, 0, source.length() - 1)` read to
/// the end (`internalGetContentReader` passes `offset + length - 1`).
pub fn comment_content(comment: &str) -> String {
    let src: Vec<u16> = comment.encode_utf16().collect();
    let start = 3usize;
    let end = (src.len() as isize - 1 - 2).max(0) as usize;
    let mut pos = start;
    if pos < end && src[pos] == b'\r' as u16 {
        pos += 1;
    }
    if pos < end && src[pos] == b'\n' as u16 {
        pos += 1;
    }
    let mut was_new_line = true;
    let mut out: Vec<u16> = Vec::new();
    while pos < end {
        let mut ch = src[pos];
        pos += 1;
        if was_new_line && !is_line_delimiter(ch) {
            while pos < end && is_java_whitespace(ch) {
                ch = src[pos];
                pos += 1;
            }
            if ch == b'*' as u16 {
                if pos < end {
                    // `do { ch = getChar(fCurrPos++); } while (ch == '*');`
                    loop {
                        ch = src[pos];
                        pos += 1;
                        if ch != b'*' as u16 || pos >= src.len() {
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        }
        was_new_line = is_line_delimiter(ch);
        out.push(ch);
    }
    String::from_utf16_lossy(&out)
}

/// `JavadocContentAccess2.getPlainTextContent(member)` for a member whose
/// source Javadoc comment is `comment` (`None`: no comment or only
/// `{@inheritDoc}`).
pub fn plain_text_content(comment: &str) -> Option<String> {
    let content = comment_content(comment);
    if content.trim() == "{@inheritDoc}" {
        return None;
    }
    // CoreJavadocContentAccessUtility.getHTMLContentReader → CoreJavaDoc2HTMLTextReader,
    // which JavaDoc2PlainTextConverter wraps in a JdtLsJavaDoc2HTMLTextReader.
    let html = JavaDoc2HtmlTextReader::core(&content).get_string();
    javadoc_to_plain_text(Some(&html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comment_markers() {
        assert_eq!(comment_content("/**\n\t * Test\n\t */"), " Test\n\t");
        assert_eq!(comment_content("/** hi */"), "hi");
    }
}
