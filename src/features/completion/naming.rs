//! Port of JDT `InternalNamingConventions.suggestVariableNames` for local
//! variables (`VK_LOCAL`, `BK_TYPE_NAME`, no configured prefixes/suffixes).

use crate::features::scanner::is_keyword;
use once_cell::sync::Lazy;
use regex::Regex;

const BASE_TYPES: &[&str] = &["int", "byte", "short", "char", "long", "float", "double", "boolean"];

fn compute_base_type_names(first: char, excluded: &[String]) -> Option<String> {
    let mut c = first;
    let mut i = 0;
    while i < excluded.len() {
        if java_lower(&c.to_string()) == java_lower(&excluded[i]) {
            c = ((c as u8) + 1) as char;
            if c > 'z' {
                c = 'a';
            }
            if c == first {
                return None;
            }
            i = 0;
            continue;
        }
        i += 1;
    }
    Some(c.to_string())
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Lower,
    Upper,
    Underscore,
    Other,
}

/// ScannerHelper/Character operate on one UTF-16 char: no expanding uppercase
/// mappings and no supplementary-letter case changes.
fn java_case(c: char, upper: bool) -> char {
    if c as u32 > 0xffff {
        return c;
    }
    if upper {
        let mut mapped = c.to_uppercase();
        let first = mapped.next().unwrap_or(c);
        if mapped.next().is_some() {
            c
        } else {
            first
        }
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}
fn java_lower(s: &str) -> String {
    s.chars().map(|c| java_case(c, false)).collect()
}

fn kind(c: char) -> Kind {
    if c as u32 > 0xffff { return Kind::Other; }
    if c.is_lowercase() {
        Kind::Lower
    } else if c.is_uppercase() {
        Kind::Upper
    } else if c == '_' {
        Kind::Underscore
    } else {
        Kind::Other
    }
}

fn compute_non_base_type_names(source: &str, only_longest: bool) -> Vec<String> {
    let s: Vec<char> = source.chars().collect();
    let length = s.len();
    if length == 0 {
        return Vec::new();
    }
    if length == 1 {
        return generate_non_constant_name(&[java_lower(source)], only_longest);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut end = length;
    let mut previous = kind(s[length - 1]);
    let mut i = length as isize - 1;
    while i >= 0 {
        let iu = i as usize;
        let c = s[iu];
        match kind(c) {
            Kind::Lower => {
                if previous == Kind::Upper {
                    parts.push(s[iu + 1..end].iter().collect());
                    end = iu + 1;
                }
                previous = Kind::Lower;
            }
            Kind::Upper => {
                if previous == Kind::Lower {
                    parts.push(s[iu..end].iter().collect());
                    if iu > 0 {
                        previous = kind(s[iu - 1]);
                    }
                    end = iu;
                } else {
                    previous = Kind::Upper;
                }
            }
            Kind::Underscore => match previous {
                Kind::Underscore => {}
                Kind::Lower | Kind::Upper => {
                    parts.push(s[iu + 1..end].iter().collect());
                    if iu > 0 {
                        previous = kind(s[iu - 1]);
                    }
                    end = iu + 1;
                }
                _ => previous = Kind::Underscore,
            },
            Kind::Other => previous = Kind::Other,
        }
        i -= 1;
    }
    if end > 0 {
        parts.push(s[..end].iter().collect());
    }
    if parts.is_empty() {
        return vec![source.to_owned()];
    }
    generate_non_constant_name(&parts, only_longest)
}

fn generate_non_constant_name(parts: &[String], only_longest: bool) -> Vec<String> {
    let n = parts.len();
    let mut names = vec![String::new(); if only_longest { 1 } else { n }];
    let mut name = java_lower(&parts[0]);
    if !only_longest {
        names[n - 1] = name.clone();
    }
    let mut suffix = parts[0].clone();
    for i in 1..n {
        name = format!("{}{}", java_lower(&parts[i]), suffix);
        if !only_longest {
            names[n - 1 - i] = name.clone();
        }
        suffix = format!("{}{}", parts[i], suffix);
    }
    if only_longest {
        names[0] = name;
    }
    names
}

fn exclude_names(mut suffix_name: String, prefix_name: &str, excluded: &[String]) -> String {
    let mut count = 2;
    let mut m = 0;
    while m < excluded.len() {
        if java_lower(&suffix_name) == java_lower(&excluded[m]) {
            suffix_name = format!("{prefix_name}{count}");
            count += 1;
            m = 0;
        } else {
            m += 1;
        }
    }
    suffix_name
}

fn is_identifier(s: &str) -> bool {
    // Character.isJavaIdentifierStart/Part, including currency, connector,
    // combining and ignorable characters (Scanner consumes code points here).
    static IDENTIFIER: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^[\p{L}\p{Nl}\p{Sc}\p{Pc}][\p{L}\p{Nl}\p{Sc}\p{Pc}\p{Nd}\p{Mc}\p{Mn}\p{Cf}\x{0}-\x{8}\x{E}-\x{1B}\x{7F}-\x{9F}]*$").unwrap()
    });
    IDENTIFIER.is_match(s) && !is_keyword(s)
}

fn pluralize(name: &str) -> String {
    let c: Vec<char> = name.chars().collect();
    let l = c.len();
    if c[l - 1] == 's' {
        if l > 1 && c[l - 2] == 's' {
            return format!("{name}es");
        }
        name.to_owned()
    } else if c[l - 1] == 'y' {
        let vowel = l > 1 && matches!(c[l - 2], 'a' | 'e' | 'i' | 'o' | 'u');
        if vowel {
            format!("{name}s")
        } else {
            format!("{}ies", c[..l - 1].iter().collect::<String>())
        }
    } else {
        format!("{name}s")
    }
}

/// `StubUtility.getVariableNameSuggestions(VK_LOCAL, project, type, dim, excluded, evaluateDefault)`.
pub fn suggest_variable_names(base_name: &str, dim: usize, excluded: &[String], evaluate_default: bool) -> Vec<String> {
    let base_name = match base_name.find('<') {
        Some(i) => &base_name[..i],
        None => base_name,
    };
    if base_name.is_empty() {
        return Vec::new();
    }
    let first_word: String = base_name.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
    let temp_names = if BASE_TYPES.contains(&first_word.as_str()) {
        match compute_base_type_names(base_name.chars().next().unwrap(), excluded) {
            Some(n) => vec![n],
            None => compute_non_base_type_names(base_name, false),
        }
    } else {
        compute_non_base_type_names(base_name, false)
    };
    let mut out: Vec<String> = Vec::new();
    for temp in temp_names {
        if temp.is_empty() {
            continue;
        }
        let mut t = if dim > 0 { pluralize(&temp) } else { temp };
        // unprefixedName[0] upper, then matchingIndex 0 lowers it again
        let mut c: Vec<char> = t.chars().collect();
        c[0] = java_case(c[0], false);
        t = c.into_iter().collect();
        let prefix_name = t.clone();
        let suffix_name = exclude_names(t.clone(), &prefix_name, excluded);
        if is_identifier(&suffix_name) {
            if !out.contains(&suffix_name) {
                out.push(suffix_name);
            }
        } else {
            let s1 = exclude_names(format!("{prefix_name}1"), &prefix_name, excluded);
            if is_identifier(&s1) && !out.contains(&s1) {
                out.push(s1);
            }
        }
    }
    if evaluate_default && out.is_empty() {
        out.push(exclude_names("name".to_owned(), "name", excluded));
    }
    out
}

/// NamingConventions VK_PARAMETER/BK_TYPE_NAME, including affix preference
/// ranking. NewLocalVariableCorrectionProposalCore intentionally uses parameter
/// preferences and dimension zero rather than the local-variable preferences.
pub fn suggest_parameter_names(
    base_name: &str,
    excluded: &[String],
    options: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let base_name = base_name.split('<').next().unwrap_or(base_name);
    let first_word: String = base_name
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    let bases = if BASE_TYPES.contains(&first_word.as_str()) {
        compute_base_type_names(first_word.chars().next().unwrap(), excluded)
            .map(|s| vec![s])
            .unwrap_or_else(|| compute_non_base_type_names(base_name, false))
    } else {
        compute_non_base_type_names(base_name, false)
    };
    let affixes = |key: &str| {
        let mut list = options
            .get(&format!("org.eclipse.jdt.core.codeComplete.{key}"))
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        list.push(String::new());
        list
    };
    let prefixes = affixes("argumentPrefixes");
    let suffixes = affixes("argumentSuffixes");
    let mut ranked = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for base in bases.into_iter().filter(|s| !s.is_empty()) {
        for (i, prefix) in prefixes.iter().enumerate() {
            let mut chars = base.chars();
            let first = chars.next().unwrap();
            let first = java_case(
                first,
                prefix
                    .chars()
                    .last()
                    .is_some_and(|c| c as u32 <= 0xffff && c.is_alphanumeric()),
            );
            let stem = format!("{prefix}{first}{}", chars.as_str());
            for (j, suffix) in suffixes.iter().enumerate() {
                let exclude = |mut name: String| {
                    let mut count = 2;
                    while excluded.iter().any(|n| java_lower(n) == java_lower(&name)) {
                        name = format!("{stem}{count}{suffix}");
                        count += 1;
                    }
                    name
                };
                let mut name = exclude(format!("{stem}{suffix}"));
                if !is_identifier(&name) {
                    name = exclude(format!("{stem}1{suffix}"));
                }
                if !is_identifier(&name) || !seen.insert(name.clone()) {
                    continue;
                }
                let rank = match (prefix.is_empty(), suffix.is_empty(), i == 0, j == 0) {
                    (false, false, true, true) => 0,
                    (false, false, true, false) => 1,
                    (false, false, false, true) => 2,
                    (false, false, false, false) => 3,
                    (false, true, true, _) => 4,
                    (false, true, false, _) => 5,
                    (true, false, _, true) => 6,
                    (true, false, _, false) => 7,
                    _ => 8,
                };
                ranked.push((rank, name));
            }
        }
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    if ranked.is_empty() {
        vec![exclude_names("name".into(), "name", excluded)]
    } else {
        ranked.into_iter().map(|(_, n)| n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        assert_eq!(suggest_variable_names("int", 0, &[], true), vec!["i"]);
        assert_eq!(suggest_variable_names("String", 0, &["args".into()], true), vec!["string"]);
        assert_eq!(suggest_variable_names("ArrayList", 0, &[], true), vec!["arrayList", "list"]);
        assert_eq!(suggest_variable_names("int", 0, &["i".into()], true), vec!["j"]);
        assert_eq!(suggest_variable_names("String", 0, &["string".into()], true), vec!["string2"]);
    }
}
