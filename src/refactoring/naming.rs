//! Ports of `NamingConventions.suggestVariableNames` /
//! `InternalNamingConventions` (BK_TYPE_NAME) and of the
//! `StubUtility.getVariableNameSuggestions` family.

use std::collections::BTreeMap;

use crate::semantic_ast::{BindingRef, Node, NodeKind};

/// `NamingConventions.VK_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    InstanceField,
    StaticField,
    StaticFinalField,
    Local,
    Parameter,
}

impl VarKind {
    fn option_prefix(self) -> &'static str {
        match self {
            VarKind::InstanceField => "field",
            VarKind::StaticField => "staticField",
            VarKind::StaticFinalField => "staticFinalField",
            VarKind::Local => "local",
            VarKind::Parameter => "argument",
        }
    }
}

fn affixes(options: &BTreeMap<String, String>, key: &str) -> Vec<Vec<char>> {
    let mut list: Vec<Vec<char>> = options
        .get(&format!("org.eclipse.jdt.core.codeComplete.{key}"))
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().collect()).collect())
        .unwrap_or_default();
    // `new char[length + 1]` with the empty affix last (or only).
    list.push(Vec::new());
    list
}

fn is_lower(c: char) -> bool {
    (c as u32) <= 0xffff && c.is_lowercase()
}

fn is_upper(c: char) -> bool {
    (c as u32) <= 0xffff && c.is_uppercase()
}

fn to_lower(c: char) -> char {
    if (c as u32) > 0xffff {
        return c;
    }
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn to_upper(c: char) -> char {
    if (c as u32) > 0xffff {
        return c;
    }
    let mut l = c.to_uppercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn lower_all(s: &[char]) -> Vec<char> {
    s.iter().map(|c| to_lower(*c)).collect()
}

fn upper_all(s: &[char]) -> Vec<char> {
    s.iter().map(|c| to_upper(*c)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Lower,
    Upper,
    Underscore,
    Other,
}

fn char_kind(c: char) -> CharKind {
    if is_lower(c) {
        CharKind::Lower
    } else if is_upper(c) {
        CharKind::Upper
    } else if c == '_' {
        CharKind::Underscore
    } else {
        CharKind::Other
    }
}

fn compute_base_type_name(first: char, excluded: &[String]) -> Option<Vec<char>> {
    let mut name = first;
    let mut i = 0;
    while i < excluded.len() {
        if eq_ignore_case(&[name], &excluded[i]) {
            name = char::from_u32(name as u32 + 1).unwrap_or(name);
            if name > 'z' {
                name = 'a';
            }
            if name == first {
                return None;
            }
            i = 0;
            continue;
        }
        i += 1;
    }
    Some(vec![name])
}

fn eq_ignore_case(a: &[char], b: &str) -> bool {
    let b: Vec<char> = b.chars().collect();
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y || to_lower(*x) == to_lower(*y))
}

fn compute_non_base_type_names(source: &[char], constant: bool, only_longest: bool) -> Vec<Vec<char>> {
    let length = source.len();
    if length == 0 {
        return Vec::new();
    }
    if length == 1 {
        let parts = vec![lower_all(source)];
        return if constant { generate_constant_name(&parts, only_longest) } else { generate_non_constant_name(&parts, only_longest) };
    }
    let mut parts: Vec<Vec<char>> = Vec::new();
    let mut end = length;
    let mut previous = char_kind(source[length - 1]);
    for i in (0..length).rev() {
        let c = source[i];
        match char_kind(c) {
            CharKind::Lower => {
                if previous == CharKind::Upper {
                    parts.push(source[i + 1..end].to_vec());
                    end = i + 1;
                }
                previous = CharKind::Lower;
            }
            CharKind::Upper => {
                if previous == CharKind::Lower {
                    parts.push(source[i..end].to_vec());
                    if i > 0 {
                        previous = char_kind(source[i - 1]);
                    }
                    end = i;
                } else {
                    previous = CharKind::Upper;
                }
            }
            CharKind::Underscore => match previous {
                CharKind::Underscore => {
                    if constant {
                        if i > 0 {
                            previous = char_kind(source[i - 1]);
                        }
                        end = i;
                    }
                }
                CharKind::Lower | CharKind::Upper => {
                    parts.push(source[i + 1..end].to_vec());
                    if i > 0 {
                        previous = char_kind(source[i - 1]);
                    }
                    end = i + 1;
                }
                CharKind::Other => previous = CharKind::Underscore,
            },
            CharKind::Other => previous = CharKind::Other,
        }
    }
    if end > 0 {
        parts.push(source[..end].to_vec());
    }
    if parts.is_empty() {
        return vec![source.to_vec()];
    }
    if constant {
        generate_constant_name(&parts, only_longest)
    } else {
        generate_non_constant_name(&parts, only_longest)
    }
}

fn generate_non_constant_name(parts: &[Vec<char>], only_longest: bool) -> Vec<Vec<char>> {
    let ptr = parts.len() - 1;
    let mut names = vec![Vec::new(); if only_longest { 1 } else { ptr + 1 }];
    let mut name = lower_all(&parts[0]);
    if !only_longest {
        names[ptr] = name.clone();
    }
    let mut suffix = parts[0].clone();
    for i in 1..=ptr {
        let part = &parts[i];
        name = lower_all(part);
        name.extend(suffix.iter());
        if !only_longest {
            names[ptr - i] = name.clone();
        }
        let mut s = part.clone();
        s.extend(suffix.iter());
        suffix = s;
    }
    if only_longest {
        names[0] = name;
    }
    names
}

fn generate_constant_name(parts: &[Vec<char>], only_longest: bool) -> Vec<Vec<char>> {
    let ptr = parts.len() - 1;
    let mut names = vec![Vec::new(); if only_longest { 1 } else { ptr + 1 }];
    let mut name = upper_all(&parts[0]);
    if !only_longest {
        names[ptr] = name.clone();
    }
    for i in 1..=ptr {
        let part = upper_all(&parts[i]);
        let mut n = part.clone();
        if part.last() != Some(&'_') {
            n.push('_');
        }
        n.extend(name.iter());
        name = n;
        if !only_longest {
            names[ptr - i] = name.clone();
        }
    }
    if only_longest {
        names[0] = name;
    }
    names
}

fn exclude_names(mut suffix_name: Vec<char>, prefix_name: &[char], suffix: &[char], excluded: &[String]) -> Vec<char> {
    let mut count = 2;
    let mut m = 0;
    while m < excluded.len() {
        if eq_ignore_case(&suffix_name, &excluded[m]) {
            let mut n = prefix_name.to_vec();
            n.extend(count.to_string().chars());
            n.extend(suffix.iter());
            suffix_name = n;
            count += 1;
            m = 0;
        } else {
            m += 1;
        }
    }
    suffix_name
}

/// `Character.isJavaIdentifierStart`.
pub fn is_java_identifier_start(c: char) -> bool {
    c == '$' || c == '_' || c.is_alphabetic() || matches!(unicode_category(c), Cat::Sc | Cat::Pc | Cat::Nl)
}

/// `Character.isJavaIdentifierPart`.
pub fn is_java_identifier_part(c: char) -> bool {
    is_java_identifier_start(c) || c.is_numeric() || matches!(unicode_category(c), Cat::Mn | Cat::Mc | Cat::Cf) || (c as u32) <= 8 || (0xe..=0x1b).contains(&(c as u32)) || (0x7f..=0x9f).contains(&(c as u32))
}

#[derive(PartialEq)]
enum Cat {
    Sc,
    Pc,
    Nl,
    Mn,
    Mc,
    Cf,
    Other,
}

fn unicode_category(c: char) -> Cat {
    // Only the categories relevant to identifiers beyond alphanumerics.
    static SC: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Sc}$").unwrap());
    static PC: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Pc}$").unwrap());
    static NL: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Nl}$").unwrap());
    static MN: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Mn}$").unwrap());
    static MC: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Mc}$").unwrap());
    static CF: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\p{Cf}$").unwrap());
    if c.is_ascii() && c != '$' && c != '_' {
        return Cat::Other;
    }
    let s = c.to_string();
    if SC.is_match(&s) {
        Cat::Sc
    } else if PC.is_match(&s) {
        Cat::Pc
    } else if NL.is_match(&s) {
        Cat::Nl
    } else if MN.is_match(&s) {
        Cat::Mn
    } else if MC.is_match(&s) {
        Cat::Mc
    } else if CF.is_match(&s) {
        Cat::Cf
    } else {
        Cat::Other
    }
}

/// Whether the name scanner reads `name` as one identifier token.
fn is_identifier(name: &[char]) -> bool {
    let Some((&first, rest)) = name.split_first() else { return false };
    if !is_java_identifier_start(first) || !rest.iter().all(|c| is_java_identifier_part(*c)) {
        return false;
    }
    let s: String = name.iter().collect();
    s != "_" && !crate::features::scanner::is_keyword(&s) && !matches!(s.as_str(), "true" | "false" | "null" | "goto" | "const" | "assert" | "enum")
}

const BASE_TYPES: &[&str] = &["int", "byte", "short", "char", "long", "float", "double", "boolean"];

/// `NamingConventions.suggestVariableNames(kind, BK_TYPE_NAME, baseName, project, dim, excluded, evaluateDefault)`.
pub fn suggest_variable_names(kind: VarKind, base_name: &str, dim: usize, excluded: &[String], options: &BTreeMap<String, String>, evaluate_default: bool) -> Vec<String> {
    let constant = kind == VarKind::StaticFinalField;
    let prefixes = affixes(options, &format!("{}Prefixes", kind.option_prefix()));
    let suffixes = affixes(options, &format!("{}Suffixes", kind.option_prefix()));
    let base: Vec<char> = base_name.chars().collect();
    // First scanner token of the base name.
    let first_token: String = base.iter().take_while(|c| is_java_identifier_part(**c)).collect();
    let temp_names = if BASE_TYPES.contains(&first_token.as_str()) && !base.is_empty() && is_java_identifier_start(base[0]) {
        if constant {
            compute_non_base_type_names(&base, true, false)
        } else {
            match compute_base_type_name(base[0], excluded) {
                Some(n) => vec![n],
                None => compute_non_base_type_names(&base, false, false),
            }
        }
    } else {
        compute_non_base_type_names(&base, constant, false)
    };

    // NamingRequestor buckets.
    let mut buckets: [Vec<String>; 9] = Default::default();
    let mut found: Vec<Vec<char>> = Vec::new();
    let mut accept_default = true;
    for temp in temp_names {
        if temp.is_empty() {
            continue;
        }
        let mut temp = temp;
        if dim > 0 {
            pluralize(&mut temp, constant);
        }
        let mut unprefixed = temp.clone();
        if !constant {
            unprefixed[0] = to_upper(unprefixed[0]);
        }
        // internalPrefix is empty: matchingIndex is 0.
        let mut temp_name = unprefixed;
        if !constant {
            temp_name[0] = to_lower(temp_name[0]);
        }
        for (k, prefix) in prefixes.iter().enumerate() {
            if !constant {
                if prefix.last().is_some_and(|c| (*c as u32) <= 0xffff && c.is_alphanumeric()) {
                    temp_name[0] = to_upper(temp_name[0]);
                } else {
                    temp_name[0] = to_lower(temp_name[0]);
                }
            }
            let mut prefix_name = prefix.clone();
            prefix_name.extend(temp_name.iter());
            for (l, suffix) in suffixes.iter().enumerate() {
                let mut suffix_name = prefix_name.clone();
                suffix_name.extend(suffix.iter());
                suffix_name = exclude_names(suffix_name, &prefix_name, suffix, excluded);
                let mut candidate = None;
                if is_identifier(&suffix_name) {
                    candidate = Some(suffix_name);
                } else if !starts_with_identifier(&suffix_name) {
                    let mut n = prefix_name.clone();
                    n.push('1');
                    n.extend(suffix.iter());
                    let n = exclude_names(n, &prefix_name, suffix, excluded);
                    if is_identifier(&n) {
                        candidate = Some(n);
                    }
                }
                if let Some(name) = candidate {
                    if !found.contains(&name) {
                        let bucket = match (!prefix.is_empty(), !suffix.is_empty()) {
                            (true, true) => match (k == 0, l == 0) {
                                (true, true) => 0,
                                (true, false) => 1,
                                (false, true) => 2,
                                (false, false) => 3,
                            },
                            (true, false) => {
                                if k == 0 {
                                    4
                                } else {
                                    5
                                }
                            }
                            (false, true) => {
                                if l == 0 {
                                    6
                                } else {
                                    7
                                }
                            }
                            (false, false) => 8,
                        };
                        buckets[bucket].push(name.iter().collect());
                        found.push(name);
                        accept_default = false;
                    }
                }
            }
        }
    }
    if evaluate_default && accept_default {
        let name: Vec<char> = "name".chars().collect();
        let n = exclude_names(name.clone(), &name, &[], excluded);
        buckets[8].push(n.iter().collect());
    }
    buckets.into_iter().flatten().collect()
}

/// The scanner's first token is an identifier (the `TokenNameIdentifier`
/// branch) even though more tokens follow.
fn starts_with_identifier(name: &[char]) -> bool {
    let Some(&first) = name.first() else { return false };
    if !is_java_identifier_start(first) {
        return false;
    }
    let word: String = name.iter().take_while(|c| is_java_identifier_part(**c)).collect();
    word != "_" && !crate::features::scanner::is_keyword(&word) && !matches!(word.as_str(), "true" | "false" | "null")
}

fn pluralize(name: &mut Vec<char>, constant: bool) {
    let (s, y, vowels, ies, es) = if constant { ('S', 'Y', "AEIOU", "IES", "ES") } else { ('s', 'y', "aeiou", "ies", "es") };
    let len = name.len();
    let last = name[len - 1];
    if last == s {
        if len > 1 && name[len - 2] == s {
            name.extend(es.chars());
        }
    } else if last == y {
        if len > 1 && vowels.contains(name[len - 2]) {
            name.push(s);
        } else {
            name.pop();
            name.extend(ies.chars());
        }
    } else {
        name.push(s);
    }
}

/// `NamingConventions.getBaseName(kind, name, project)`.
pub fn base_name(kind: VarKind, name: &str, options: &BTreeMap<String, String>) -> String {
    let prefixes: Vec<Vec<char>> = affixes(options, &format!("{}Prefixes", kind.option_prefix())).into_iter().filter(|p| !p.is_empty()).collect();
    let suffixes: Vec<Vec<char>> = affixes(options, &format!("{}Suffixes", kind.option_prefix())).into_iter().filter(|p| !p.is_empty()).collect();
    let name: Vec<char> = name.chars().collect();
    let mut without_prefix = name.clone();
    let mut best = 0;
    for prefix in &prefixes {
        if name.starts_with(prefix) {
            let len = prefix.len();
            let last_is_letter = prefix[len - 1].is_alphabetic();
            if (!last_is_letter || (name.len() > len && is_upper(name[len]))) && best < len && name.len() != len {
                without_prefix = name[len..].to_vec();
                best = len;
            }
        }
    }
    let mut without_suffix = without_prefix.clone();
    let mut best = 0;
    for suffix in &suffixes {
        if without_prefix.ends_with(suffix) {
            let len = suffix.len();
            if best < len && without_prefix.len() != len {
                without_suffix = without_prefix[..without_prefix.len() - len].to_vec();
                best = len;
            }
        }
    }
    if let Some(c) = without_suffix.first_mut() {
        *c = to_lower(*c);
    }
    if kind == VarKind::StaticFinalField {
        let mut out = Vec::new();
        let mut previous_underscore = false;
        for c in without_suffix {
            if c != '_' {
                out.push(if previous_underscore { to_upper(c) } else { to_lower(c) });
                previous_underscore = false;
            } else {
                previous_underscore = true;
            }
        }
        return out.into_iter().collect();
    }
    without_suffix.into_iter().collect()
}

const KNOWN_METHOD_NAME_PREFIXES: [&str; 17] = [
    "get", "is", "to", "create", "load", "find", "build", "generate", "prepare", "parse", "current", "read", "resolve", "retrieve", "make", "add", "extract",
];

/// `StubUtility.getBaseName(IVariableBinding, project)`.
pub fn variable_binding_base_name(b: BindingRef<'_>, options: &BTreeMap<String, String>) -> String {
    let kind = if b.is_field() {
        let m = b.modifiers();
        let st = m & crate::semantic_ast::modifier::STATIC != 0;
        if st && m & crate::semantic_ast::modifier::FINAL != 0 {
            VarKind::StaticFinalField
        } else if st {
            VarKind::StaticField
        } else {
            VarKind::InstanceField
        }
    } else if b.is_parameter() {
        VarKind::Parameter
    } else {
        VarKind::Local
    };
    base_name(kind, b.name(), options)
}

/// `ConvertLoopOperation.modifyBaseName`.
pub fn modify_base_name(suggested: &str) -> String {
    const ELEMENT: &str = "element";
    let mut name = suggested.to_owned();
    if suggested.len() > 3 {
        let after = suggested[3..].chars().next().unwrap_or(' ');
        if (after.is_uppercase() || after == '_') && suggested.to_lowercase().starts_with("all") {
            let without = &suggested[3..];
            name = if without.starts_with('_') && without.len() > 1 { without[1..].to_owned() } else { without.to_owned() };
            if name.chars().count() == 1 {
                return name;
            }
        }
    }
    const IRREGULAR: [(&str, &str); 18] = [
        ("Children", "Child"),
        ("Entries", "Entry"),
        ("Proxies", "Proxy"),
        ("Indices", "Index"),
        ("People", "Person"),
        ("Properties", "Property"),
        ("Factories", "Factory"),
        ("Archives", "archive"),
        ("Aliases", "Alias"),
        ("Alternatives", "Alternative"),
        ("Capabilities", "Capability"),
        ("Hashes", "Hash"),
        ("Directories", "Directory"),
        ("Statuses", "Status"),
        ("Instances", "Instance"),
        ("Classes", "Class"),
        ("Deliveries", "Delivery"),
        ("Vertices", "Vertex"),
    ];
    for (suffix, replacement) in IRREGULAR {
        if name.to_lowercase().ends_with(&suffix.to_lowercase()) {
            return format!("{}{replacement}", &name[..name.len() - suffix.len()]);
        }
    }
    if ["ints", "floats", "doubles", "booleans", "bytes", "chars", "shorts", "longs"].iter().any(|s| name.eq_ignore_ascii_case(s)) {
        return ELEMENT.to_owned();
    }
    if ["xes", "ies", "oes", "ses", "hes", "zes", "ves", "ces", "ss", "is", "us", "os", "as"].iter().any(|s| name.to_lowercase().ends_with(s)) {
        return ELEMENT.to_owned();
    }
    if name.len() > 2 && name.ends_with('s') {
        return name[..name.len() - 1].to_owned();
    }
    ELEMENT.to_owned()
}

/// `StubUtility.getBaseNameFromExpression(project, expression, variableKind)`.
pub fn base_name_from_expression(expression: Node<'_>, kind: VarKind, options: &BTreeMap<String, String>) -> Option<String> {
    let mut e = expression;
    if e.is(NodeKind::CastExpression) {
        e = e.child("expression")?;
    }
    let name: Option<String>;
    if e.kind().is_name() {
        if let Some(b) = e.binding().filter(|b| b.is_variable()) {
            return Some(variable_binding_base_name(b, options));
        }
        let simple = if e.is(NodeKind::QualifiedName) { e.child("name")? } else { e };
        return Some(simple.identifier());
    } else if e.is(NodeKind::MethodInvocation) {
        let n = e.child("name")?.identifier();
        if n == "next" {
            let receiver = e.child("expression");
            let modified = match receiver {
                Some(r) if r.is(NodeKind::SimpleName) => modify_base_name(&r.identifier()),
                _ => "element".to_owned(),
            };
            if modified != "element" {
                return Some(modified);
            }
        }
        name = Some(n);
    } else if e.is(NodeKind::SuperMethodInvocation) {
        name = Some(e.child("name")?.identifier());
    } else if e.is(NodeKind::FieldAccess) {
        return Some(e.child("name")?.identifier());
    } else if kind == VarKind::StaticFinalField && (e.is(NodeKind::StringLiteral) || e.is(NodeKind::NumberLiteral)) {
        let string = if e.is(NodeKind::StringLiteral) { string_literal_value(e.simple("escapedValue").unwrap_or("")) } else { e.simple("token").unwrap_or("").to_owned() };
        let mut res = String::new();
        let mut needs_underscore = false;
        for ch in string.chars() {
            if is_java_identifier_part(ch) {
                if (res.is_empty() && !is_java_identifier_start(ch)) || needs_underscore {
                    res.push('_');
                }
                res.push(ch);
                needs_underscore = false;
            } else {
                needs_underscore = !res.is_empty();
            }
        }
        if !res.is_empty() {
            return Some(res);
        }
        name = None;
    } else {
        name = None;
    }
    let name = name?;
    for prefix in KNOWN_METHOD_NAME_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            if rest.chars().next().is_some_and(char::is_uppercase) {
                return Some(rest.to_owned());
            }
        }
    }
    Some(name)
}

/// `StringLiteral.getLiteralValue()` of an escaped literal.
pub fn string_literal_value(escaped: &str) -> String {
    let inner = escaped.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(escaped);
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{8}'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('s') => out.push(' '),
            Some('u') => {
                while chars.peek() == Some(&'u') {
                    chars.next();
                }
                let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(d @ '0'..='7') => {
                let mut v = d.to_digit(8).unwrap();
                for _ in 0..2 {
                    match chars.peek() {
                        Some(n @ '0'..='7') if v * 8 + n.to_digit(8).unwrap() <= 0o377 => {
                            v = v * 8 + n.to_digit(8).unwrap();
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if let Some(ch) = char::from_u32(v) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `StubUtility.getBaseNameFromLocationInParent(expression)`.
fn base_name_from_location_in_parent(expression: Node<'_>, options: &BTreeMap<String, String>) -> Option<String> {
    if !expression.location_is("arguments") {
        return None;
    }
    let parent = expression.parent()?;
    if !matches!(
        parent.kind(),
        NodeKind::MethodInvocation | NodeKind::ClassInstanceCreation | NodeKind::SuperMethodInvocation | NodeKind::ConstructorInvocation | NodeKind::SuperConstructorInvocation
    ) {
        return None;
    }
    let binding = parent.method_binding()?;
    let arguments = parent.list("arguments");
    let params = binding.parameter_types();
    if params.len() != arguments.len() {
        return None;
    }
    let index = arguments.iter().position(|a| *a == expression)?;
    if let Some(t) = expression.type_binding() {
        if !super::checks::is_assignment_compatible(t, params[index]) {
            return None;
        }
    }
    let declaration = binding.method_declaration().unwrap_or(binding);
    // `method.getOpenable().getBuffer() != null`: source or attached source.
    if !declaration.is_from_source() && declaration.data().source_offset < 0 {
        return None;
    }
    let name = declaration.data().parameter_names.get(index)?;
    Some(base_name(VarKind::Parameter, name, options))
}

fn add(names: Vec<String>, res: &mut Vec<String>) {
    for n in names {
        if !res.contains(&n) {
            res.push(n);
        }
    }
}

fn default_suggestions(kind: VarKind, excluded: &[String]) -> Vec<String> {
    let prop = if kind == VarKind::StaticFinalField { "X" } else { "x" };
    let mut name = prop.to_owned();
    let mut i = 1;
    while excluded.contains(&name) {
        name = format!("{prop}{i}");
        i += 1;
    }
    vec![name]
}

/// The type name and dimensions `getVariableNameSuggestions` derives from
/// the expected type.
fn expected_type_name(expected: Option<BindingRef<'_>>) -> Option<(String, usize)> {
    let t = crate::correction::type_mismatch::bindings::normalize_type_binding(expected)?;
    let mut t = t;
    let mut dim = 0;
    if t.is_array() {
        dim = t.dimensions().max(0) as usize;
        t = t.element_type()?;
    }
    if t.is_parameterized_type() {
        t = t.type_declaration().unwrap_or(t);
    }
    Some((t.name().to_owned(), dim))
}

/// `StubUtility.getVariableNameSuggestions(kind, project, ITypeBinding expectedType, Expression assignedExpression, excluded)`.
pub fn variable_name_suggestions(kind: VarKind, expected: Option<BindingRef<'_>>, expression: Option<Node<'_>>, excluded: &[String], options: &BTreeMap<String, String>) -> Vec<String> {
    let mut res: Vec<String> = Vec::new();
    if let Some(e) = expression {
        if let Some(name) = base_name_from_expression(e, kind, options) {
            add(suggest_variable_names(kind, &name, 0, excluded, options, false), &mut res);
        }
        if let Some(name) = base_name_from_location_in_parent(e, options) {
            add(suggest_variable_names(kind, &name, 0, excluded, options, false), &mut res);
        }
    }
    if let Some((type_name, dim)) = expected_type_name(expected) {
        if !type_name.is_empty() {
            add(suggest_variable_names(kind, &type_name, dim, excluded, options, false), &mut res);
        }
    }
    if res.is_empty() {
        return default_suggestions(kind, excluded);
    }
    res
}

/// `StubUtility.getVariableNameSuggestions(kind, project, expectedType, assignedExpression, excluded, usedNameForIdenticalExpressionInCu, usedNamesForIdenticalExpressionInMethod)`.
pub fn variable_name_suggestions_with_context(
    kind: VarKind,
    expected: Option<BindingRef<'_>>,
    expression: Option<Node<'_>>,
    excluded: &[String],
    used_in_cu: Option<&str>,
    used_in_method: &[String],
    options: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut res: Vec<String> = Vec::new();
    let typed = expected_type_name(expected);
    let type_available = typed.as_ref().is_some_and(|(n, _)| !n.is_empty());
    if let Some(e) = expression {
        if type_available && e.is(NodeKind::MethodInvocation) {
            let type_name = &typed.as_ref().unwrap().0;
            let name = e.child("name").map(|n| n.identifier()).unwrap_or_default();
            if !name.to_lowercase().contains(&type_name.to_lowercase()) {
                let arguments = e.list("arguments");
                if arguments.len() > 1 || arguments.iter().any(|a| a.is(NodeKind::MethodInvocation)) {
                    if let Some(used) = used_in_cu {
                        if !excluded.iter().any(|x| x == used) && !used_in_method.iter().any(|x| x == used) {
                            add(suggest_variable_names(kind, used, 0, excluded, options, false), &mut res);
                        }
                    }
                }
            }
        }
        if let Some(name) = base_name_from_expression(e, kind, options) {
            add(suggest_variable_names(kind, &name, 0, excluded, options, false), &mut res);
        }
        if let Some(name) = base_name_from_location_in_parent(e, options) {
            add(suggest_variable_names(kind, &name, 0, excluded, options, false), &mut res);
        }
    }
    if let Some((type_name, dim)) = typed.filter(|(n, _)| !n.is_empty()) {
        add(suggest_variable_names(kind, &type_name, dim, excluded, options, false), &mut res);
    }
    if res.is_empty() {
        return default_suggestions(kind, excluded);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        let o = BTreeMap::new();
        assert_eq!(suggest_variable_names(VarKind::Local, "int", 0, &[], &o, false), vec!["i"]);
        assert_eq!(suggest_variable_names(VarKind::Local, "int", 0, &["i".into()], &o, false), vec!["j"]);
        assert_eq!(suggest_variable_names(VarKind::Local, "ArrayList", 0, &[], &o, false), vec!["arrayList", "list"]);
        assert_eq!(suggest_variable_names(VarKind::StaticFinalField, "_0", 0, &[], &o, false), vec!["_0"]);
        assert_eq!(suggest_variable_names(VarKind::StaticFinalField, "int", 0, &[], &o, false), vec!["INT"]);
        assert_eq!(suggest_variable_names(VarKind::StaticFinalField, "fooBar", 0, &[], &o, false), vec!["FOO_BAR", "BAR"]);
    }
}
