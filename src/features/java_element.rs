//! Java-model conventions shared by the navigation features: element labels
//! (`JavaElementLabelsCore`), symbol kinds (`SymbolUtils.mapKind`), element
//! locations (`JDTUtils.toLocation`), JDT handle identifiers, `jdt://`
//! class-file URIs, and the iteration order of the `java.util.HashMap`s that
//! JDT's call hierarchy returns its results in.

use super::semantic::{Elem, ACC_FINAL, ACC_STATIC};
use crate::project::{Workspace, DEFAULT_PROJECT_NAME};
use tower_lsp::lsp_types::{Location, Position, Range, SymbolKind, Url};

pub const DECL_STRING: &str = " : ";

/// `JDTUtils.getName`: `JavaElementLabelsCore.getElementLabel(element,
/// ALL_DEFAULT | M_APP_RETURNTYPE | ROOT_VARIABLE)`.
pub fn label(e: &Elem) -> String {
    match e.kind.as_str() {
        "method" | "constructor" => {
            let mut s = e.name.clone();
            s.push('(');
            let n = e.params.len();
            for (i, p) in e.params.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if e.varargs && i + 1 == n && p.ends_with("[]") {
                    s.push_str(&p[..p.len() - 2]);
                    s.push_str("...");
                } else {
                    s.push_str(p);
                }
            }
            s.push(')');
            if !e.type_params.is_empty() {
                s.push_str(" <");
                s.push_str(&e.type_params.join(", "));
                s.push('>');
            }
            if e.kind == "method" {
                if let Some(r) = &e.return_type {
                    s.push_str(DECL_STRING);
                    s.push_str(r);
                }
            }
            s
        }
        "type" => type_label(e),
        "initializer" => "{...}".to_owned(),
        _ => e.name.clone(),
    }
}

/// Type label with `T_TYPE_PARAMETERS`.
pub fn type_label(e: &Elem) -> String {
    if e.anonymous {
        return "new Anonymous".to_owned();
    }
    let mut s = e.name.clone();
    if !e.type_params.is_empty() {
        s.push('<');
        s.push_str(&e.type_params.join(", "));
        s.push('>');
    }
    s
}

/// `SymbolUtils.mapKind`.
pub fn symbol_kind(e: &Elem) -> SymbolKind {
    match e.kind.as_str() {
        "type" => type_symbol_kind(e),
        "enumConstant" => SymbolKind::ENUM_MEMBER,
        "field" => {
            if e.has_flag(ACC_STATIC) && e.has_flag(ACC_FINAL) {
                SymbolKind::CONSTANT
            } else {
                SymbolKind::FIELD
            }
        }
        "initializer" | "constructor" => SymbolKind::CONSTRUCTOR,
        "method" => SymbolKind::METHOD,
        _ => SymbolKind::STRING,
    }
}

pub fn type_symbol_kind(e: &Elem) -> SymbolKind {
    match e.type_kind.as_deref() {
        Some("interface") | Some("annotation") => SymbolKind::INTERFACE,
        Some("enum") => SymbolKind::ENUM,
        _ => SymbolKind::CLASS,
    }
}

/// `IType.getFullyQualifiedName()` of the element's declaring type (`$` for
/// member types).
pub fn declaring_type_fqn(e: &Elem) -> Option<String> {
    e.declaring_type_fqn.clone()
}

/// `JDTUtils.toLocation(element, FULL_RANGE)` / `NAME_RANGE` (falling back
/// to the class file location for binary members).
pub fn full_location(e: &Elem, project: &str) -> Option<Location> {
    location(e, project, false)
}

pub fn name_location(e: &Elem, project: &str) -> Option<Location> {
    location(e, project, true)
}

fn location(e: &Elem, project: &str, name: bool) -> Option<Location> {
    if e.has_source_location() {
        let uri = Url::parse(e.uri.as_deref()?).ok()?;
        let r = if name { e.name_range.or(e.range) } else { e.range }?;
        return Some(Location { uri, range: r.to_lsp() });
    }
    let uri = Url::parse(&class_file_uri(e, project)?).ok()?;
    Some(Location { uri, range: zero_range() })
}

pub fn zero_range() -> Range {
    Range { start: Position::new(0, 0), end: Position::new(0, 0) }
}

// ─── Handle identifiers ──────────────────────────────────────────────────────

/// Characters JDT escapes in handle mementos (`JavaElement.JEM_*`).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '=' | '/' | '<' | '^' | '~' | '|' | '{' | '(' | '\'' | '[' | '%' | '#' | '!' | '@' | ']' | '}' | ')' | '&' | '"' | '`' | '*'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Project name and package fragment root path (relative to the project, or
/// absolute for an archive) of a source element.
fn source_root(ws: &Workspace, uri: &str) -> (String, String) {
    let path = Url::parse(uri)
        .ok()
        .and_then(|u| crate::project::uri_to_path(&u));
    if let Some(path) = path {
        if let Some(p) = ws.project_for_path(&path) {
            let root = p
                .source_folder_for(&path)
                .map(|sf| sf.path.strip_prefix(&p.root).unwrap_or(&sf.path).to_string_lossy().into_owned())
                .unwrap_or_default();
            return (p.name.clone(), root);
        }
    }
    (DEFAULT_PROJECT_NAME.to_owned(), "src".to_owned())
}

/// The project a source element belongs to (for class-file URIs of the
/// binaries it references).
pub fn project_of(ws: &Workspace, uri: Option<&str>) -> String {
    if let Some(reference) = uri.and_then(crate::classfile::ClassFileRef::parse) {
        return reference.project;
    }
    uri.map(|u| source_root(ws, u).0).unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned())
}

/// `IJavaElement.getHandleIdentifier()`.
pub fn handle_identifier(e: &Elem, ws: &Workspace, project: &str) -> String {
    let mut s = String::new();
    if e.from_source {
        let uri = e.uri.clone().unwrap_or_default();
        let (proj, root) = source_root(ws, &uri);
        s.push('=');
        s.push_str(&escape(&proj));
        s.push('/');
        s.push_str(&escape(&root));
        s.push('<');
        s.push_str(&escape(e.package_name.as_deref().unwrap_or("")));
        s.push('{');
        let file = uri.rsplit('/').next().unwrap_or("").to_owned();
        s.push_str(&escape(&percent_decode(&file)));
    } else {
        s.push_str(&class_file_handle(e, project));
    }
    let chain: &[String] = &e.type_chain;
    let skip_first = !e.from_source; // a class file handle already names its type
    for (i, t) in chain.iter().enumerate() {
        if i == 0 && skip_first {
            s.push('[');
            s.push_str(&escape(t));
            continue;
        }
        s.push('[');
        s.push_str(&escape(t));
    }
    match e.kind.as_str() {
        "method" | "constructor" => {
            s.push('~');
            s.push_str(&escape(&e.name));
            for sig in &e.param_sigs {
                s.push('~');
                s.push_str(&escape(sig));
            }
        }
        "field" | "enumConstant" => {
            s.push('^');
            s.push_str(&escape(&e.name));
        }
        "initializer" => {
            s.push('|');
            s.push_str(&e.occurrence.max(1).to_string());
            return s;
        }
        _ => {}
    }
    if e.occurrence > 1 {
        s.push('!');
        s.push_str(&e.occurrence.to_string());
    }
    s
}

fn class_file_handle(e: &Elem, project: &str) -> String {
    let mut s = String::from("=");
    s.push_str(&escape(project));
    s.push('/');
    s.push_str(&escape(e.archive.as_deref().unwrap_or("")));
    if let Some(m) = &e.module {
        s.push('`');
        s.push_str(&escape(m));
    }
    s.push('<');
    s.push_str(&escape(e.package_name.as_deref().unwrap_or("")));
    s.push('(');
    s.push_str(&escape(e.class_file.as_deref().unwrap_or("")));
    s
}

/// `JDTUtils.toUri(IClassFile)`: `jdt://contents/<root>/<package>/<Class>.class?<handle>`.
pub fn class_file_uri(e: &Elem, project: &str) -> Option<String> {
    let class_file = e.class_file.as_deref()?;
    let root = match &e.module {
        Some(m) => m.clone(),
        None => std::path::Path::new(e.archive.as_deref()?).file_name()?.to_string_lossy().into_owned(),
    };
    let pkg = e.package_name.as_deref().unwrap_or("");
    let handle = class_file_handle(e, project);
    Some(format!(
        "jdt://contents/{}/{}/{}?{}",
        encode(&root, false),
        encode(pkg, false),
        encode(class_file, false),
        encode(&handle, true)
    ))
}

fn encode(s: &str, query: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        let keep = c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '$') || (query && matches!(c, '=' | '/'));
        if keep {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ─── java.util.HashMap iteration order ───────────────────────────────────────

/// `String.hashCode()`.
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// Indexes of `keys` (first occurrence of each distinct key, in insertion
/// order) in the order a `new HashMap<>()` filled with them iterates.
pub fn java_hash_map_order(keys: &[String]) -> Vec<usize> {
    let mut distinct: Vec<usize> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if !distinct.iter().any(|&j| keys[j] == *k) {
            distinct.push(i);
        }
    }
    let mut cap: usize = 16;
    while distinct.len() as f64 > cap as f64 * 0.75 {
        cap *= 2;
    }
    let mut order: Vec<(usize, usize, usize)> = distinct
        .iter()
        .enumerate()
        .map(|(ins, &i)| {
            let h = java_string_hash(&keys[i]);
            let spread = (h ^ ((h as u32) >> 16) as i32) as u32 as usize;
            (spread & (cap - 1), ins, i)
        })
        .collect();
    order.sort();
    order.into_iter().map(|(_, _, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_hash_matches_java() {
        assert_eq!(java_string_hash("hello"), 99162322);
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("polygenelubricants"), i32::MIN);
    }

    #[test]
    fn hash_map_order_by_bucket() {
        // "a" (97) and "b" (98) land in buckets 1 and 2.
        let keys = vec!["b".to_owned(), "a".to_owned(), "b".to_owned()];
        assert_eq!(java_hash_map_order(&keys), vec![1, 0]);
    }
}
