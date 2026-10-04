//! Port of jdt.ls `ParameterGuesser` (best guessed method arguments).

use super::proposal::{Context, VisibleElement};
use super::signature as sig;
use std::collections::HashSet;

const LOCAL: i32 = 0;
const INHERITED_FIELD: i32 = 2;
const INHERITED_METHOD: i32 = 4;
const LITERALS: i32 = 5;

#[derive(Debug, Clone)]
struct Variable {
    name: String,
    variable_type: i32,
    position_score: i32,
    is_autoboxing_match: bool,
    already_matched: bool,
}

pub struct ParameterGuesser<'a> {
    context: &'a Context,
    already_matched: HashSet<String>,
}

const PRIMITIVES: &[&str] = &["byte", "short", "char", "int", "long", "float", "double", "boolean", "void"];

fn is_primitive(t: &str) -> bool {
    PRIMITIVES.contains(&t)
}

impl<'a> ParameterGuesser<'a> {
    pub fn new(context: &'a Context) -> Self {
        ParameterGuesser { context, already_matched: HashSet::new() }
    }

    fn evaluate_visible_matches(&self, expected_type: &str, suggestions: &[VisibleElement]) -> Vec<Variable> {
        // jdt.ls passes the compilation unit as enclosing element, so there is
        // no current type: fields and methods always count as inherited and
        // 'this' is never proposed.
        let _ = self.context;
        let mut res = Vec::new();
        for (i, s) in suggestions.iter().enumerate() {
            if let Some(mut v) = self.create_variable(s, expected_type, i as i32) {
                if self.already_matched.contains(&v.name) {
                    v.already_matched = true;
                }
                res.push(v);
            }
        }
        match primitive_code(expected_type) {
            None => res.push(Variable { name: "null".into(), variable_type: LITERALS, position_score: res.len() as i32, is_autoboxing_match: false, already_matched: false }),
            Some(code) => {
                let autoboxing = code != expected_type;
                if code == "boolean" {
                    let n = res.len() as i32;
                    res.push(Variable { name: "true".into(), variable_type: LITERALS, position_score: n, is_autoboxing_match: autoboxing, already_matched: false });
                    let n = res.len() as i32;
                    res.push(Variable { name: "false".into(), variable_type: LITERALS, position_score: n, is_autoboxing_match: autoboxing, already_matched: false });
                } else {
                    let n = res.len() as i32;
                    res.push(Variable { name: "0".into(), variable_type: LITERALS, position_score: n, is_autoboxing_match: autoboxing, already_matched: false });
                }
            }
        }
        res
    }

    fn create_variable(&self, e: &VisibleElement, expected_type: &str, position: i32) -> Option<Variable> {
        let (variable_type, signature, name) = match e.kind {
            1 => (INHERITED_FIELD, e.type_signature.clone()?, e.name.clone()),
            0 => (LOCAL, e.type_signature.clone()?, e.name.clone()),
            2 => {
                let rt = e.return_type.clone()?;
                let suggest = e.parameter_count == 0 && rt != "V" && (e.name.starts_with("get") || e.name.starts_with("is"));
                if !suggest {
                    return None;
                }
                (INHERITED_METHOD, rt, format!("{}()", e.name))
            }
            _ => return None,
        };
        let t = sig::to_string(&signature).unwrap_or_default();
        let autobox = is_primitive(expected_type) != is_primitive(&t);
        Some(Variable { name, variable_type, position_score: position, is_autoboxing_match: autobox, already_matched: false })
    }

    /// `parameterProposals`.
    pub fn parameter_proposals(&mut self, expected_type: &str, param_name: &str, suggestions: &[VisibleElement]) -> Option<String> {
        let mut matches = self.evaluate_visible_matches(expected_type, suggestions);
        // Collections.sort is stable; comparator: score(two) - score(one)
        matches.sort_by(|a, b| score(b, param_name).cmp(&score(a, param_name)));
        let first = matches.first()?;
        self.already_matched.insert(first.name.clone());
        Some(first.name.clone())
    }
}

/// `ParameterGuesser.getPrimitiveTypeCode`: only primitive type names map
/// (boxed types never do, see the dead `code == ...` branches upstream).
fn primitive_code(t: &str) -> Option<&str> {
    if is_primitive(t) {
        Some(t)
    } else {
        None
    }
}

fn score(v: &Variable, param_name: &str) -> i64 {
    let variable_score = 100 - v.variable_type as i64;
    let mut substring_score = longest_common_substring(&v.name, param_name).chars().count() as i64;
    let shorter = v.name.chars().count().min(param_name.chars().count()) as f64;
    if (substring_score as f64) < 0.6 * shorter {
        substring_score = 0;
    }
    let position_score = v.position_score as i64;
    let matched_score = if v.already_matched { 0 } else { 1 };
    let autoboxing_score = if v.is_autoboxing_match { 0 } else { 1 };
    (autoboxing_score << 30) | (variable_score << 21) | (substring_score << 11) | (matched_score << 10) | position_score
}

/// Longest common substring, case-insensitive like `StringMatcher(*s*, ignoreCase=true)`.
fn longest_common_substring(first: &str, second: &str) -> String {
    let (shorter, longer) = if first.chars().count() <= second.chars().count() { (first, second) } else { (second, first) };
    let s: Vec<char> = shorter.chars().collect();
    let longer_l = longer.to_lowercase();
    let mut best = String::new();
    let min = s.len();
    for i in 0..min {
        for j in i + 1..=min {
            if j - i < best.chars().count() {
                continue;
            }
            let sub: String = s[i..j].iter().collect();
            if longer_l.contains(&sub.to_lowercase()) {
                best = sub;
            }
        }
    }
    best
}
