//! Workspace type index: the names jdt.ls finds through
//! `SearchEngine.searchAllTypeNames` / `searchAllMethodNames`.
//!
//! * Source types and methods come from the Rust Java model
//!   ([`crate::features::java_model`]) of every workspace and open document.
//! * Library types come from the class files of the classpath archives, and
//!   JDK types from the `jrt:/` image, both listed once by the bridge
//!   (`SemanticIndexService.listTypes`, which reads class headers with
//!   JDT's `ClassFileReader`) and cached here per archive.
//!
//! [`TypeIndex::search_types`] and [`TypeIndex::search_methods`] implement
//! the matching rules of JDT's search engine (`SearchPattern.R_PATTERN_MATCH`
//! and `R_CAMELCASE_MATCH`).

use crate::features::java_model::{self, flags, Member, TypeDecl, TypeKind};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const ACC_INTERFACE: u32 = 0x0200;
pub const ACC_ANNOTATION: u32 = 0x2000;
pub const ACC_ENUM: u32 = 0x4000;
pub const ACC_DEPRECATED: u32 = 0x0010_0000;

/// Where a type is declared.
#[derive(Debug, Clone)]
pub enum TypeOrigin {
    /// A source file: URI and the byte span of the type name.
    Source { uri: String, name: (usize, usize) },
    /// A class file in an archive (jar/folder) or a JDK module.
    Binary { archive: String, module: Option<String>, class_file: String, source_file_name: Option<String> },
}

#[derive(Debug, Clone)]
pub struct TypeEntry {
    pub package: String,
    /// Enclosing type names (outermost first) for member types.
    pub enclosing: Vec<String>,
    pub name: String,
    /// JDT modifier flags (`AccInterface`, `AccEnum`, `AccAnnotation`, `AccDeprecated`, ...).
    pub modifiers: u32,
    pub origin: TypeOrigin,
    /// Fully qualified direct supertypes from class-file headers.
    pub super_types: Vec<String>,
}

impl TypeEntry {
    /// `TypeNameMatch.getTypeContainerName`: package, or the enclosing type
    /// qualified with dots.
    pub fn container(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if !self.package.is_empty() {
            parts.push(&self.package);
        }
        parts.extend(self.enclosing.iter().map(String::as_str));
        parts.join(".")
    }

    pub fn is_binary(&self) -> bool {
        matches!(self.origin, TypeOrigin::Binary { .. })
    }
}

#[derive(Debug, Clone)]
pub struct MethodEntry {
    pub name: String,
    /// `getDeclaringType().getFullyQualifiedName()` (`$` for member types).
    pub declaring_type: String,
    pub modifiers: u32,
    pub uri: String,
    pub name_span: (usize, usize),
}

/// Source types and methods of one compilation unit.
pub fn source_entries(uri: &str, text: &str) -> (Vec<TypeEntry>, Vec<MethodEntry>) {
    let cu = java_model::parse(text);
    let package = cu.package.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let mut types = Vec::new();
    let mut methods = Vec::new();
    for t in &cu.types {
        collect(uri, &package, &[], t, &mut types, &mut methods);
    }
    (types, methods)
}

fn collect(uri: &str, package: &str, enclosing: &[String], t: &TypeDecl, types: &mut Vec<TypeEntry>, methods: &mut Vec<MethodEntry>) {
    if t.anonymous || t.enum_body {
        return;
    }
    let mut modifiers = t.flags;
    match t.kind {
        TypeKind::Interface => modifiers |= ACC_INTERFACE,
        TypeKind::Annotation => modifiers |= ACC_INTERFACE | ACC_ANNOTATION,
        TypeKind::Enum => modifiers |= ACC_ENUM,
        _ => {}
    }
    types.push(TypeEntry {
        package: package.to_owned(),
        enclosing: enclosing.to_vec(),
        name: t.name.clone(),
        modifiers,
        origin: TypeOrigin::Source { uri: uri.to_owned(), name: t.name_range },
        super_types: Vec::new(),
    });
    let mut chain = enclosing.to_vec();
    chain.push(t.name.clone());
    let fqn = {
        let mut s = String::new();
        if !package.is_empty() {
            s.push_str(package);
            s.push('.');
        }
        s.push_str(&chain.join("$"));
        s
    };
    for m in &t.members {
        match m {
            Member::Type(inner) => collect(uri, package, &chain, inner, types, methods),
            Member::Method(md) if !md.constructor => methods.push(MethodEntry {
                name: md.name.clone(),
                declaring_type: fqn.clone(),
                modifiers: md.flags,
                uri: uri.to_owned(),
                name_span: md.name_range,
            }),
            _ => {}
        }
    }
}

// ─── binary listings ─────────────────────────────────────────────────────────

struct CachedArchive {
    stamp: (u64, u64),
    types: Vec<TypeEntry>,
}

static ARCHIVES: Mutex<Option<HashMap<String, CachedArchive>>> = Mutex::new(None);
static JDK: Mutex<Option<(String, Vec<TypeEntry>)>> = Mutex::new(None);

fn stamp(path: &str) -> (u64, u64) {
    std::fs::metadata(path)
        .map(|m| {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
            (t, m.len())
        })
        .unwrap_or((0, 0))
}

/// Cached library types for `archives`; `None` for archives not listed yet.
pub fn cached_archive(path: &str) -> Option<Vec<TypeEntry>> {
    let guard = ARCHIVES.lock().unwrap_or_else(|e| e.into_inner());
    let c = guard.as_ref()?.get(path)?;
    (c.stamp == stamp(path)).then(|| c.types.clone())
}

pub fn cached_jdk() -> Option<Vec<TypeEntry>> {
    JDK.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(_, t)| t.clone())
}

/// Store a `listTypes` answer.
pub fn store_listing(v: &Value) {
    if let Some(archives) = v.get("archives").and_then(Value::as_array) {
        let mut guard = ARCHIVES.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        for a in archives {
            let Some(path) = a.get("path").and_then(Value::as_str) else { continue };
            let types = parse_types(a.get("types"), path, None);
            map.insert(path.to_owned(), CachedArchive { stamp: stamp(path), types });
        }
    }
    if let Some(jdk) = v.get("jdk") {
        let jrt = jdk.get("jrtFs").and_then(Value::as_str).unwrap_or("").to_owned();
        let mut types = Vec::new();
        for m in jdk.get("modules").and_then(Value::as_array).into_iter().flatten() {
            let module = m.get("module").and_then(Value::as_str).unwrap_or("").to_owned();
            types.extend(parse_types(m.get("types"), &jrt, Some(module)));
        }
        let home = jdk.get("home").and_then(Value::as_str).unwrap_or("").to_owned();
        *JDK.lock().unwrap_or_else(|e| e.into_inner()) = Some((home, types));
    }
}

fn parse_types(v: Option<&Value>, archive: &str, module: Option<String>) -> Vec<TypeEntry> {
    let mut out = Vec::new();
    for t in v.and_then(Value::as_array).into_iter().flatten() {
        let Some(t) = t.as_array() else { continue };
        let package = t.first().and_then(Value::as_str).unwrap_or("").to_owned();
        let simple = t.get(1).and_then(Value::as_str).unwrap_or("").to_owned();
        let modifiers = t.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
        let mut parts: Vec<String> = simple.split('$').map(str::to_owned).collect();
        let name = parts.pop().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(TypeEntry {
            package,
            enclosing: parts,
            name,
            modifiers,
            super_types: t.get(3).and_then(Value::as_str).into_iter().chain(
                t.get(4).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)
            ).map(str::to_owned).collect(),
            origin: TypeOrigin::Binary {
                archive: archive.to_owned(),
                module: module.clone().or_else(|| t.get(5).and_then(Value::as_str).map(str::to_owned)),
                class_file: format!("{simple}.class"),
                source_file_name: t.get(6).and_then(Value::as_str).map(str::to_owned),
            },
        });
    }
    out
}

// ─── matching ────────────────────────────────────────────────────────────────

/// `SearchPattern` match rules used by the workspace symbol search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchRule {
    Pattern,
    CamelCase,
}

/// `CharOperation.match(pattern, name, false)`: `*` and `?` wildcards,
/// case-insensitive.
pub fn pattern_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn is_upper_or_digit(c: char) -> bool {
    c.is_uppercase() || c.is_ascii_digit()
}

/// `CharOperation.camelCaseMatch(pattern, name, false)`.
pub fn camel_case_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    if p.is_empty() {
        return true;
    }
    if n.is_empty() || p[0] != n[0] {
        return false;
    }
    let (mut ip, mut iname) = (0usize, 0usize);
    loop {
        ip += 1;
        iname += 1;
        if ip == p.len() {
            return true;
        }
        if iname == n.len() {
            return false;
        }
        let pc = p[ip];
        if pc == n[iname] {
            continue;
        }
        if !is_upper_or_digit(pc) {
            return false;
        }
        loop {
            if iname == n.len() {
                return false;
            }
            let nc = n[iname];
            if nc.is_lowercase() || nc == '$' || nc == '_' || (!nc.is_alphanumeric() && !nc.is_uppercase()) {
                iname += 1;
            } else if nc.is_ascii_digit() {
                if pc == nc {
                    break;
                }
                iname += 1;
            } else if pc != nc {
                return false;
            } else {
                break;
            }
        }
    }
}

/// `BasicSearchEngine.match(pattern, rule, name)` (case-insensitive rules).
pub fn name_matches(pattern: &str, rule: MatchRule, name: &str) -> bool {
    match rule {
        MatchRule::Pattern => pattern_match(pattern, name),
        MatchRule::CamelCase => {
            camel_case_match(pattern, name) || name.to_lowercase().starts_with(&pattern.to_lowercase())
        }
    }
}

/// `SearchPattern.validateMatchRule`: wildcards turn camel case into a
/// pattern match.
pub fn validate_rule(pattern: &str, rule: MatchRule) -> MatchRule {
    if rule == MatchRule::CamelCase && (pattern.contains('*') || pattern.contains('?')) {
        MatchRule::Pattern
    } else {
        rule
    }
}

/// Library roots of a search scope.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub sources: Vec<(String, String)>,
    pub archives: Vec<PathBuf>,
    pub jdk: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case() {
        assert!(camel_case_match("NPE", "NullPointerException"));
        assert!(camel_case_match("HaMa", "HashMap"));
        assert!(camel_case_match("Array", "ArrayList"));
        assert!(!camel_case_match("inkSet", "LinkedHashSet"));
        assert!(camel_case_match("main", "main"));
    }

    #[test]
    fn patterns() {
        assert!(pattern_match("*buff*stream*", "BufferedInputStream"));
        assert!(pattern_match("*ink*Set*", "LinkedHashSet"));
        assert!(pattern_match("*util*", "java.util.regex"));
        assert!(!pattern_match("java.io", "java.iox"));
        assert!(pattern_match("B*", "bar"));
    }
}
