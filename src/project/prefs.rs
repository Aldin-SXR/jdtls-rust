//! Java `.properties` parsing (Eclipse `.settings/*.prefs` files use this format).

use std::collections::BTreeMap;
use std::path::Path;

/// Parse a Java properties file into key/value pairs.  Supports `#`/`!`
/// comments, `=`/`:`/whitespace separators, line continuations and the
/// standard backslash escapes.
pub fn parse_properties(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut logical = String::new();
    let mut lines = text.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = raw.trim_start();
        if logical.is_empty() && (line.is_empty() || line.starts_with('#') || line.starts_with('!')) {
            continue;
        }
        // An odd number of trailing backslashes continues the line.
        let trailing = line.chars().rev().take_while(|&c| c == '\\').count();
        if trailing % 2 == 1 {
            logical.push_str(&line[..line.len() - 1]);
            if lines.peek().is_some() {
                continue;
            }
        } else {
            logical.push_str(line);
        }
        let (k, v) = split_property(&logical);
        out.insert(unescape(&k), unescape(&v));
        logical.clear();
    }
    out
}

fn split_property(line: &str) -> (String, String) {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '=' | ':' | ' ' | '\t' => {
                let key = line[..i].to_owned();
                let mut rest = line[i..].trim_start();
                if !(c == '=' || c == ':') {
                    if let Some(r) = rest.strip_prefix(['=', ':']) {
                        rest = r;
                    }
                } else {
                    rest = &rest[1..];
                }
                return (key, rest.trim_start().to_owned());
            }
            _ => {}
        }
    }
    (line.to_owned(), String::new())
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

pub fn read_properties(path: &Path) -> Option<BTreeMap<String, String>> {
    std::fs::read_to_string(path).ok().map(|t| parse_properties(&t))
}

#[cfg(test)]
mod tests {
    use super::parse_properties;

    #[test]
    fn parses_prefs() {
        let p = parse_properties(
            "eclipse.preferences.version=1\n# c\norg.eclipse.jdt.core.compiler.compliance=1.8\nkey\\:x = a\\\n  b\n",
        );
        assert_eq!(p["org.eclipse.jdt.core.compiler.compliance"], "1.8");
        assert_eq!(p["key:x"], "ab");
    }
}
