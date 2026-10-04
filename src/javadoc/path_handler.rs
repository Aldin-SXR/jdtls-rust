//! Port of jdt.ls `JavaDocHTMLPathHandler`: rewrites relative `<img src>`
//! paths in Javadoc text so the client can load them.
//!
//! Images extracted from javadoc/source jars need source attachments (the
//! jar locations of a binary package fragment), which source elements never
//! use; for those only the source-folder lookup applies.

use std::path::Path;

pub const TAGS: &[&str] = &["img"];

/// `containsHTMLTag`
pub fn contains_html_tag(text: &str) -> bool {
    TAGS.iter().any(|t| text.contains(&format!("<{t}")))
}

/// `extractSourcePathFromHTMLTag`: UTF-8 byte offsets of the `src` value
/// (between the quotes), or `None`.
pub fn extract_source_path_from_html_tag(text: &str) -> Option<(usize, usize)> {
    static RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r#"(src\s*=\s*['"])"#).unwrap());
    let m = RE.find(text)?;
    let start = m.end();
    let quote = text[..start].chars().last()?;
    let end = text[start..].find(quote).map(|i| start + i)?;
    Some((start, end))
}

/// `isPathAbsolute`: an absolute URI (has a scheme) or an absolute path.
pub fn is_path_absolute(path: &str) -> bool {
    if !is_valid_uri_reference(path) {
        return false;
    }
    if has_scheme(path) {
        return true;
    }
    Path::new(path).is_absolute() || path.starts_with('/')
}

fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else { return false };
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

/// Characters `java.net.URI` rejects (unescaped).
fn is_valid_uri_reference(s: &str) -> bool {
    !s.chars().any(|c| c == ' ' || c == '\\' || c == '"' || c == '<' || c == '>' || c == '^' || c == '`' || c == '{' || c == '|' || c == '}' || c.is_control())
}

/// `ResourceUtils.fixURI(file.toURI())`
pub fn file_uri(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(|u| u.to_string())
}

/// `getValidatedHTMLSrcAttribute(textElement, element)` for an element whose
/// package fragment is the folder `package_dir`.
pub fn validated_html_src_attribute(text: &str, package_dir: Option<&Path>) -> String {
    let Some((start, end)) = extract_source_path_from_html_tag(text) else {
        return text.to_owned();
    };
    let src_path = &text[start..end];
    if is_path_absolute(src_path) {
        return text.to_owned();
    }
    let Some(dir) = package_dir else {
        return text.to_owned();
    };
    let candidate = Path::new(&format!("{}/{}", dir.to_string_lossy(), src_path)).to_path_buf();
    if candidate.exists() {
        if let Some(uri) = file_uri(&candidate) {
            return format!("{}{}{}", &text[..start], uri, &text[end..]);
        }
    }
    text.to_owned()
}

#[cfg(test)]
mod java_doc_image_extraction_test {
    //! `JavaDocImageExtractionTest.testIsAbsolutePath` (the rest of the class
    //! is in `tests/javadoc_java_doc_image_extraction_test.rs`).
    use super::*;

    #[test]
    fn test_is_absolute_path() {
        assert!(is_path_absolute("/usr/nikolas/file.txt"));
        assert!(is_path_absolute("file:/usr/nikolas/file.txt"));
        assert!(is_path_absolute("file:///usr/nikolas/file.txt"));
        assert!(is_path_absolute("https://nikolas.com/file.txt"));

        assert!(!is_path_absolute("usr/nikolas/file.txt"));
        assert!(!is_path_absolute("usr/nikolas/folder/"));
    }
}
