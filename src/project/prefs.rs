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

/// `EclipsePreferences.decodePath(fullPath)`: `(path, key)`.
pub fn decode_path(full_path: &str) -> (Option<String>, String) {
    let (path, key) = match full_path.find("//") {
        Some(index) => (Some(&full_path[..index]), &full_path[index + 2..]),
        None => match full_path.rfind('/') {
            Some(last) => (Some(&full_path[..last]), &full_path[last + 1..]),
            None => (None, full_path),
        },
    };
    let path = path.filter(|p| !p.is_empty()).map(|p| p.strip_prefix('/').unwrap_or(p).to_owned());
    (path, key.to_owned())
}

/// The JavaCore options `java.settings.url` contributes
/// (`StandardProjectsManager.configureSettings`): every property of the
/// file except `file_export_version` and the `@`/`!` entries, keyed by the
/// preference key of its (possibly scoped) path. Empty when the URL is
/// unset or doesn't resolve to a readable file.
pub fn settings_url_options(url: Option<&str>, roots: &[std::path::PathBuf]) -> BTreeMap<String, String> {
    use std::sync::Mutex;
    type Cached = (std::path::PathBuf, Option<std::time::SystemTime>, BTreeMap<String, String>);
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

    let Some(path) = url.and_then(|u| crate::features::formatting::options::formatter_path(u, roots)) else {
        return BTreeMap::new();
    };
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, options)) = cache.as_ref() {
        if *p == path && *m == modified {
            return options.clone();
        }
    }
    let Some(properties) = read_properties(&path) else {
        tracing::error!("Cannot read {}", path.display());
        return BTreeMap::new();
    };
    let options: BTreeMap<String, String> = properties
        .into_iter()
        .filter(|(path, _)| path != "file_export_version" && !path.starts_with('@') && !path.starts_with('!'))
        .map(|(path, value)| (decode_path(&path).1, value))
        .collect();
    *cache = Some((path, modified, options.clone()));
    options
}

#[cfg(test)]
mod settings_url_tests {
    use super::*;

    #[test]
    fn decodes_scoped_preference_paths() {
        assert_eq!((None, "a.b".to_owned()), decode_path("a.b"));
        assert_eq!(
            (Some("instance/org.eclipse.jdt.core".to_owned()), "org.eclipse.jdt.core.x".to_owned()),
            decode_path("/instance/org.eclipse.jdt.core/org.eclipse.jdt.core.x")
        );
        assert_eq!((Some("a/b".to_owned()), "c/d".to_owned()), decode_path("/a/b//c/d"));
    }
}
