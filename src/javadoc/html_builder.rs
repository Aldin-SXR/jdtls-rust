//! Port of `org.eclipse.text.html.HTMLBuilder` escaping helpers.

/// `HTMLBuilder.convertToHTMLContent`
pub fn convert_to_html_content(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `HTMLBuilder.convertToHTMLContentWithWhitespace`
pub fn convert_to_html_content_with_whitespace(content: &str) -> String {
    format!("<span style='white-space:pre'>{}</span>", convert_to_html_content(content))
}
