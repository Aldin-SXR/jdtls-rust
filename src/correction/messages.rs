//! The message bundles of jdt.ls and jdt.core.manipulation, embedded
//! verbatim (`*.properties` from the oracle jars / the jdt.ls checkout), and
//! `MessageFormat.format`.
//!
//! ```ignore
//! let label = messages::format(messages::fix("CodeStyleFix_ChangeAccessToStatic_description"), &["EnumA"]);
//! ```

use std::collections::HashMap;

use once_cell::sync::Lazy;

fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        // Logical line: join continuations (odd number of trailing backslashes).
        let mut logical = trimmed.to_owned();
        while ends_with_continuation(&logical) {
            logical.pop();
            match lines.next() {
                Some(next) => logical.push_str(next.trim_start()),
                None => break,
            }
        }
        let chars: Vec<char> = logical.chars().collect();
        let mut i = 0;
        let mut key = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                key.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '=' || c == ':' || c.is_whitespace() {
                break;
            }
            key.push(c);
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() && (chars[i] == '=' || chars[i] == ':') {
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                let n = chars[i + 1];
                match n {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    'f' => value.push('\u{c}'),
                    'u' => {
                        let hex: String = chars[i + 2..(i + 6).min(chars.len())].iter().collect();
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            value.push(ch);
                        }
                        i += 6;
                        continue;
                    }
                    other => value.push(other),
                }
                i += 2;
                continue;
            }
            value.push(c);
            i += 1;
        }
        map.insert(key, value);
    }
    map
}

fn ends_with_continuation(s: &str) -> bool {
    let n = s.chars().rev().take_while(|&c| c == '\\').count();
    n % 2 == 1
}

macro_rules! bundle {
    ($static:ident, $fn:ident, $file:literal) => {
        static $static: Lazy<HashMap<String, String>> = Lazy::new(|| parse(include_str!(concat!("messages/", $file))));

        /// Message of the bundle (the key itself when missing, like NLS).
        pub fn $fn(key: &str) -> &'static str {
            $static.get(key).map(String::as_str).unwrap_or_else(|| Box::leak(format!("!{key}!").into_boxed_str()))
        }
    };
}

// jdt.ls `org.eclipse.jdt.ls.core.internal.corrections.CorrectionMessages`.
bundle!(LS_CORRECTION, ls_correction, "ls_correction.properties");
// jdt.ls `org.eclipse.jdt.ls.core.internal.text.correction.ActionMessages`.
bundle!(LS_ACTION, ls_action, "ls_action.properties");
// jdt.ls `org.eclipse.jdt.ls.core.internal.corext.refactoring.RefactoringCoreMessages`.
bundle!(LS_REFACTORING, ls_refactoring, "ls_refactoring.properties");
// `org.eclipse.jdt.internal.core.manipulation.JavaManipulationMessages`.
bundle!(MANIPULATION, manipulation, "manipulation.properties");
// jdt.core.manipulation `org.eclipse.jdt.internal.ui.text.correction.CorrectionMessages`.
bundle!(CORRECTION, correction, "correction.properties");
// `org.eclipse.jdt.internal.corext.fix.FixMessages`.
bundle!(FIX, fix, "fix.properties");
// `org.eclipse.jdt.internal.ui.fix.MultiFixMessages`.
bundle!(MULTIFIX, multifix, "multifix.properties");
// `org.eclipse.jdt.internal.corext.refactoring.RefactoringCoreMessages`.
bundle!(REFACTORING, refactoring, "refactoring.properties");
// `org.eclipse.jdt.internal.corext.codemanipulation.CodeGenerationMessages`.
bundle!(CODEGEN, codegeneration, "codegeneration.properties");

/// `java.text.MessageFormat.format(pattern, args)` for `{n}` arguments.
pub fn format(pattern: &str, args: &[&str]) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_quote = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if i + 1 < chars.len() && chars[i + 1] == '\'' {
                out.push('\'');
                i += 2;
                continue;
            }
            in_quote = !in_quote;
            i += 1;
            continue;
        }
        if c == '{' && !in_quote {
            if let Some(end) = chars[i..].iter().position(|&x| x == '}') {
                let inner: String = chars[i + 1..i + end].iter().collect();
                let index = inner.split(',').next().unwrap_or("").trim();
                if let Ok(n) = index.parse::<usize>() {
                    match args.get(n) {
                        Some(a) => out.push_str(a),
                        None => {
                            out.push('{');
                            out.push_str(&inner);
                            out.push('}');
                        }
                    }
                    i += end + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_quotes_and_arguments() {
        assert_eq!(format("Change access to static using ''{0}'' (declaring type)", &["EnumA"]), "Change access to static using 'EnumA' (declaring type)");
        assert_eq!(fix("Java50Fix_SerialVersion_default_description"), "Add default serial version ID");
    }
}
