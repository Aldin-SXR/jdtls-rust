//! NamingConventions / GetterSetterUtil names and JavaModelUtil's unresolved
//! signature matching. Prefix/suffix rules use the compilation unit options.
use super::{flags, FieldDecl, TypeDecl, TypeKind};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeMap;
type Options = BTreeMap<String, String>;

fn list<'a>(options: &'a Options, key: &str) -> Vec<&'a str> {
    options
        .get(&format!("org.eclipse.jdt.core.codeComplete.{key}"))
        .map(|s| s.split(',').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}
fn trim_affixes(name: &str, prefixes: &[&str], suffixes: &[&str]) -> String {
    let prefix = prefixes
        .iter()
        .filter(|p| name.starts_with(**p) && p.len() < name.len())
        .filter(|p| {
            !p.chars().last().unwrap().is_alphabetic()
                || name[p.len()..].chars().next().is_some_and(is_upper)
        })
        .max_by_key(|p| p.len())
        .copied()
        .unwrap_or("");
    let name = &name[prefix.len()..];
    let suffix = suffixes
        .iter()
        .filter(|s| name.ends_with(**s) && s.len() < name.len())
        .max_by_key(|s| s.len())
        .copied()
        .unwrap_or("");
    name[..name.len() - suffix.len()].into()
}
pub(crate) fn base(field: &FieldDecl, options: &Options, lower: bool) -> String {
    let kind = if field.flags & (flags::STATIC | flags::FINAL) == flags::STATIC | flags::FINAL {
        "staticFinalField"
    } else if field.flags & flags::STATIC != 0 {
        "staticField"
    } else {
        "field"
    };
    let name = trim_affixes(
        &field.name,
        &list(options, &format!("{kind}Prefixes")),
        &list(options, &format!("{kind}Suffixes")),
    );
    if kind == "staticFinalField" {
        let mut out = String::new();
        let mut upper = false;
        for c in name.chars() {
            if c == '_' {
                upper = true;
            } else {
                out.push(if upper { upper_char(c) } else { lower_char(c) });
                upper = false;
            }
        }
        out
    } else if lower {
        first_case(&name, false)
    } else {
        name
    }
}
fn upper_char(c: char) -> char {
    // Character.toUpperCase(char) cannot expand a code unit or case-map a
    // supplementary letter represented by a UTF-16 surrogate pair.
    if c as u32 > 0xffff {
        return c;
    }
    let mut upper = c.to_uppercase();
    let first = upper.next().unwrap_or(c);
    if upper.next().is_some() {
        c
    } else {
        first
    }
}
fn lower_char(c: char) -> char {
    if c as u32 > 0xffff {
        c
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}
fn is_upper(c: char) -> bool {
    c as u32 <= 0xffff && c.is_uppercase()
}
fn is_lower(c: char) -> bool {
    c as u32 <= 0xffff && c.is_lowercase()
}
fn first_case(name: &str, upper: bool) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!(
        "{}{}",
        if upper {
            upper_char(first)
        } else {
            lower_char(first)
        },
        chars.as_str()
    )
}
fn accessor(base: &str) -> String {
    let mut chars = base.chars();
    if chars.next().is_some_and(is_lower) && chars.next().is_none_or(|c| !c.is_uppercase()) {
        first_case(base, true)
    } else {
        base.into()
    }
}
fn is_boolean_name(base: &str) -> bool {
    base.starts_with("is") && base[2..].chars().next().is_some_and(is_upper)
}
pub(crate) fn getter(t: &TypeDecl, f: &FieldDecl, options: &Options, use_is: bool) -> String {
    if t.kind == TypeKind::Record && f.flags & flags::STATIC == 0 {
        return f.name.clone();
    }
    let base = base(f, options, false);
    if use_is && f.type_label.as_deref() == Some("boolean") {
        if is_boolean_name(&base) {
            base
        } else {
            format!("is{}", accessor(&base))
        }
    } else {
        format!("get{}", accessor(&base))
    }
}
pub(crate) fn setter(f: &FieldDecl, options: &Options, use_is: bool) -> String {
    let base_name = base(f, options, false);
    if use_is && f.type_label.as_deref() == Some("boolean") && is_boolean_name(&base_name) {
        let mut stripped = f.clone();
        stripped.name = base_name[2..].into();
        format!("set{}", accessor(&base(&stripped, options, false)))
    } else {
        format!("set{}", accessor(&base_name))
    }
}
pub(crate) fn argument(f: &FieldDecl, options: &Options) -> String {
    suggest_argument(&base(f, options, true), options)
}
pub(crate) fn method_argument(name: &str, options: &Options, excluded: &[String]) -> String {
    let base = trim_affixes(
        name,
        &list(options, "argumentPrefixes"),
        &list(options, "argumentSuffixes"),
    );
    // StubUtility keeps an existing parameter name when it already uses the
    // configured affixes, rather than normalizing it a second time.
    if base != name {
        return name.into();
    }
    argument_excluding(&first_case(&base, false), options, excluded)
}
pub(crate) fn constructor_argument(
    f: &FieldDecl,
    options: &Options,
    excluded: &[String],
) -> String {
    argument_excluding(&base(f, options, true), options, excluded)
}
pub(crate) fn argument_excluding(base: &str, options: &Options, excluded: &[String]) -> String {
    let name = suggest_argument(base, options);
    let suffixes = list(options, "argumentSuffixes");
    let suffix = suffixes.first().copied().unwrap_or("");
    let mut candidate = name.clone();
    let mut number = 2;
    while excluded.iter().any(|n| n.eq_ignore_ascii_case(&candidate)) {
        let stem = name.strip_suffix(suffix).unwrap_or(&name);
        candidate = format!("{stem}{number}{suffix}");
        number += 1;
    }
    candidate
}
fn suggest_argument(base: &str, options: &Options) -> String {
    // InternalNamingConventions.computeNonBaseTypeNames: find the first
    // camel-case/underscore word, lowercase it and preserve the remaining words.
    let chars: Vec<char> = base.chars().collect();
    let classify = |c| {
        if is_lower(c) {
            1
        } else if is_upper(c) {
            2
        } else if c == '_' {
            3
        } else {
            4
        }
    };
    let mut end = chars.len();
    let mut previous = chars.last().copied().map(classify).unwrap_or(4);
    for i in (0..chars.len()).rev() {
        match classify(chars[i]) {
            1 => {
                if previous == 2 {
                    end = i + 1;
                }
                previous = 1;
            }
            2 => {
                if previous == 1 {
                    end = i;
                    if i > 0 {
                        previous = classify(chars[i - 1]);
                    }
                } else {
                    previous = 2;
                }
            }
            3 => {
                if previous == 1 || previous == 2 {
                    end = i + 1;
                    if i > 0 {
                        previous = classify(chars[i - 1]);
                    }
                } else if previous != 3 {
                    previous = 3;
                }
            }
            _ => previous = 4,
        }
    }
    let base: String = chars
        .iter()
        .enumerate()
        .map(|(i, &c)| if i < end { lower_char(c) } else { c })
        .collect();
    let prefixes = list(options, "argumentPrefixes");
    let suffixes = list(options, "argumentSuffixes");
    let prefix = prefixes.first().copied().unwrap_or("");
    let suffix = suffixes.first().copied().unwrap_or("");
    let mut name = format!(
        "{prefix}{}{suffix}",
        if prefix.chars().last().is_some_and(char::is_alphabetic) {
            first_case(&base, true)
        } else {
            base
        }
    );
    if crate::features::scanner::is_keyword(&name) {
        name.push('1');
    }
    name
}

pub(super) fn signature_simple_type(name: &str) -> String {
    // Signature.getSignatureSimpleName drops dots before the first generic
    // start, retaining qualification within type arguments.
    let head = name.find(['<', '$']).unwrap_or(name.len());
    let start = name[..head].rfind('.').map_or(0, |i| i + 1);
    name[start..].to_owned()
}
pub(super) fn simple_type(name: &str) -> String {
    static QUALIFIED: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?:[\p{L}\p{N}_$]+\.)+([\p{L}\p{N}_$]+)").unwrap());
    QUALIFIED.replace_all(name, "$1").into_owned()
}
pub(super) fn method_type(name: &str) -> String {
    // JavaModelUtil compares Signature.toString types, including arguments;
    // varargs parameters have an array signature in the Java model.
    simple_type(&name.replace("...", "[]"))
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
