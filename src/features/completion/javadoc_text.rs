//! Javadoc comment → completion documentation (`JavadocContentAccess2`
//! plain text / Markdown for source members).

/// The description lines of a Javadoc comment as JDT's DOM sees them: the
/// text after the leading `*` of each line, without the `/**` and `*/`
/// lines' surrounding whitespace.
fn lines(raw: &str) -> Vec<String> {
    let body = raw.strip_prefix("/**").unwrap_or(raw);
    let body = body.strip_suffix("*/").unwrap_or(body);
    let parts: Vec<&str> = body.split('\n').collect();
    let n = parts.len();
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let part = part.strip_suffix('\r').unwrap_or(part);
        let mut text = if i == 0 {
            part.trim_start().to_owned()
        } else {
            let t = part.trim_start_matches([' ', '\t']);
            let t = t.trim_start_matches('*');
            t.to_owned()
        };
        if i == n - 1 {
            text = text.trim_end().to_owned();
        }
        out.push(text);
    }
    out
}

/// Description text before the first block tag, lines joined with `\n`.
fn description(raw: &str) -> (String, Vec<(String, String)>) {
    let mut desc: Vec<String> = Vec::new();
    let mut tags: Vec<(String, String)> = Vec::new();
    let all = lines(raw);
    let n = all.len();
    for (i, l) in all.into_iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with('@') {
            let (tag, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
            tags.push((tag.to_owned(), rest.trim().to_owned()));
            continue;
        }
        if let Some(last) = tags.last_mut() {
            if !t.is_empty() {
                last.1.push(' ');
                last.1.push_str(t);
            }
            continue;
        }
        if l.is_empty() && (i == 0 || i == n - 1) {
            continue;
        }
        desc.push(l);
    }
    let mut text = String::new();
    for (i, l) in desc.iter().enumerate() {
        text.push_str(l);
        if i + 1 < desc.len() || n > 1 {
            text.push('\n');
        }
    }
    (text, tags)
}

fn inline_tags(s: &str, markdown: bool) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{@") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let (tag, arg) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        let arg = arg.trim();
        let rendered = match tag {
            "code" | "literal" if markdown && tag == "code" => format!("`{arg}`"),
            "link" | "linkplain" => {
                let (target, label) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
                if label.trim().is_empty() {
                    target.trim_start_matches('#').replace('#', ".")
                } else {
                    label.trim().to_owned()
                }
            }
            _ => arg.to_owned(),
        };
        out.push_str(&rendered);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `JavadocContentAccess2.getPlainTextContent`.
pub fn plain_text(raw: &str) -> Option<String> {
    let (desc, tags) = description(raw);
    let mut out = inline_tags(&desc, false).replace('\n', " ");
    let section = |title: &str, items: Vec<&String>, out: &mut String| {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("\n{title}\n"));
        for i in items {
            out.push_str(&format!("  {}\n", inline_tags(i, false)));
        }
    };
    let pick = |names: &[&str]| tags.iter().filter(|(t, _)| names.contains(&t.as_str())).map(|(_, v)| v).collect::<Vec<_>>();
    section("Parameters:", pick(&["@param"]), &mut out);
    section("Returns:", pick(&["@return"]), &mut out);
    section("Throws:", pick(&["@throws", "@exception"]), &mut out);
    section("See Also:", pick(&["@see"]), &mut out);
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// `JavadocContentAccess2.getMarkdownContent`.
pub fn markdown(raw: &str) -> Option<String> {
    let (desc, tags) = description(raw);
    let mut out = inline_tags(&desc, true).replace('\n', " ").trim().to_owned();
    let mut section = |title: &str, items: Vec<String>| {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("\n\n * **{title}**"));
        for i in items {
            out.push_str(&format!("\n    * {i}"));
        }
    };
    let pick = |names: &[&str]| tags.iter().filter(|(t, _)| names.contains(&t.as_str())).map(|(_, v)| inline_tags(v, true)).collect::<Vec<_>>();
    section("Parameters:", pick(&["@param"]));
    section("Returns:", pick(&["@return"]));
    section("Throws:", pick(&["@throws", "@exception"]));
    section("See Also:", pick(&["@see"]));
    if out.is_empty() {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain() {
        assert_eq!(plain_text("/** Test */").unwrap(), "Test");
        assert_eq!(plain_text("/**\n\t* This method has Javadoc\n\t*/").unwrap(), " This method has Javadoc ");
    }
}
