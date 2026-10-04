//! Port of JDT `InternalNamingConventions.suggestVariableNames` for local
//! variables (`VK_LOCAL`, `BK_TYPE_NAME`, no configured prefixes/suffixes).

use crate::features::scanner::is_keyword;

const BASE_TYPES: &[&str] = &["int", "byte", "short", "char", "long", "float", "double", "boolean"];

fn compute_base_type_names(first: char, excluded: &[String]) -> Option<String> {
    let mut c = first;
    let mut i = 0;
    while i < excluded.len() {
        if c.to_string().eq_ignore_ascii_case(&excluded[i]) {
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

fn kind(c: char) -> Kind {
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
        return generate_non_constant_name(&[source.to_lowercase()], only_longest);
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
    let mut name = parts[0].to_lowercase();
    if !only_longest {
        names[n - 1] = name.clone();
    }
    let mut suffix = parts[0].clone();
    for i in 1..n {
        name = format!("{}{}", parts[i].to_lowercase(), suffix);
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
        if suffix_name.eq_ignore_ascii_case(&excluded[m]) || suffix_name.to_lowercase() == excluded[m].to_lowercase() {
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
    let mut chars = s.chars();
    let Some(f) = chars.next() else { return false };
    (f.is_alphabetic() || f == '_' || f == '$') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !is_keyword(s)
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
        c[0] = c[0].to_lowercase().next().unwrap_or(c[0]);
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
