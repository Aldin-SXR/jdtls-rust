//! Port of JDT's `org.eclipse.jdt.core.Signature` (the parts code assist
//! uses) and of `org.eclipse.jdt.internal.corext.template.java.SignatureUtil`.
//!
//! Signatures are handled as `Vec<char>` so indices behave like Java `char[]`.
//! Methods that throw `IllegalArgumentException` in JDT return `Err(())`.

pub type SigResult<T> = Result<T, ()>;

pub const C_ARRAY: char = '[';
pub const C_BOOLEAN: char = 'Z';
pub const C_BYTE: char = 'B';
pub const C_CAPTURE: char = '!';
pub const C_CHAR: char = 'C';
pub const C_COLON: char = ':';
pub const C_DOLLAR: char = '$';
pub const C_DOT: char = '.';
pub const C_DOUBLE: char = 'D';
pub const C_EXTENDS: char = '+';
pub const C_FLOAT: char = 'F';
pub const C_GENERIC_END: char = '>';
pub const C_GENERIC_START: char = '<';
pub const C_INT: char = 'I';
pub const C_INTERSECTION: char = '|';
pub const C_UNION: char = '&';
pub const C_LONG: char = 'J';
pub const C_NAME_END: char = ';';
pub const C_PARAM_END: char = ')';
pub const C_PARAM_START: char = '(';
pub const C_RESOLVED: char = 'L';
pub const C_SEMICOLON: char = ';';
pub const C_SHORT: char = 'S';
pub const C_STAR: char = '*';
pub const C_SUPER: char = '-';
pub const C_TYPE_VARIABLE: char = 'T';
pub const C_UNRESOLVED: char = 'Q';
pub const C_VOID: char = 'V';
pub const C_EXCEPTION_START: char = '^';

pub const SIG_VOID: &str = "V";

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn string(c: &[char]) -> String {
    c.iter().collect()
}

fn is_whitespace(c: char) -> bool {
    // ScannerHelper.isWhitespace
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}') || c.is_whitespace()
}

// ── Util.scan* ───────────────────────────────────────────────────────────────

fn scan_type_signature(s: &[char], start: usize) -> SigResult<usize> {
    let c = *s.get(start).ok_or(())?;
    match c {
        C_ARRAY => scan_array_type_signature(s, start),
        C_RESOLVED | C_UNRESOLVED => {
            let e = scan_class_type_signature(s, start)?;
            if e < 0 {
                Err(())
            } else {
                Ok(e as usize)
            }
        }
        C_TYPE_VARIABLE => scan_type_variable_signature(s, start),
        'B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'V' | 'Z' => Ok(start),
        C_CAPTURE => scan_capture_type_signature(s, start),
        C_EXTENDS | C_SUPER | C_STAR => scan_type_bound_signature(s, start),
        _ => Err(()),
    }
}

fn scan_array_type_signature(s: &[char], mut start: usize) -> SigResult<usize> {
    let len = s.len();
    if start + 1 >= len {
        return Err(());
    }
    if s[start] != C_ARRAY {
        return Err(());
    }
    start += 1;
    let mut c = s[start];
    while c == C_ARRAY {
        if start + 1 >= len {
            return Err(());
        }
        start += 1;
        c = s[start];
    }
    scan_type_signature(s, start)
}

fn scan_capture_type_signature(s: &[char], start: usize) -> SigResult<usize> {
    if start + 1 >= s.len() || s[start] != C_CAPTURE {
        return Err(());
    }
    scan_type_bound_signature(s, start + 1)
}

fn scan_type_variable_signature(s: &[char], start: usize) -> SigResult<usize> {
    if start + 2 >= s.len() || s[start] != C_TYPE_VARIABLE {
        return Err(());
    }
    let id = scan_identifier(s, start + 1)?;
    match s.get(id + 1) {
        Some(&C_SEMICOLON) => Ok(id + 1),
        _ => Err(()),
    }
}

fn scan_identifier(s: &[char], start: usize) -> SigResult<usize> {
    if start >= s.len() {
        return Err(());
    }
    let mut p = start;
    loop {
        let c = s[p];
        if matches!(c, '<' | '>' | ':' | ';' | '.' | '/') {
            return Ok(p.wrapping_sub(1));
        }
        p += 1;
        if p == s.len() {
            return Ok(p - 1);
        }
    }
}

/// Returns -1 (as `isize`) when `start` is not a class type signature.
fn scan_class_type_signature(s: &[char], start: usize) -> SigResult<isize> {
    if start + 2 >= s.len() {
        return Err(());
    }
    let c = s[start];
    if c != C_RESOLVED && c != C_UNRESOLVED {
        return Ok(-1);
    }
    let mut p = start + 1;
    loop {
        let c = *s.get(p).ok_or(())?;
        if c == C_SEMICOLON {
            return Ok(p as isize);
        } else if c == C_GENERIC_START {
            p = scan_type_argument_signatures(s, p)?;
        } else if c == C_DOT || c == '/' {
            p = scan_identifier(s, p + 1)?;
        }
        p += 1;
    }
}

fn scan_type_bound_signature(s: &[char], start: usize) -> SigResult<usize> {
    let c = *s.get(start).ok_or(())?;
    match c {
        C_STAR => return Ok(start),
        C_SUPER | C_EXTENDS => {}
        _ => return Err(()),
    }
    let start = start + 1;
    let c = *s.get(start).ok_or(())?;
    if c != C_STAR && start + 1 >= s.len() {
        return Err(());
    }
    match c {
        C_CAPTURE => scan_capture_type_signature(s, start),
        C_SUPER | C_EXTENDS => scan_type_bound_signature(s, start),
        C_RESOLVED | C_UNRESOLVED => {
            let e = scan_class_type_signature(s, start)?;
            if e < 0 {
                Err(())
            } else {
                Ok(e as usize)
            }
        }
        C_TYPE_VARIABLE => scan_type_variable_signature(s, start),
        C_ARRAY => scan_array_type_signature(s, start),
        C_STAR => Ok(start),
        _ => Err(()),
    }
}

fn scan_type_argument_signatures(s: &[char], start: usize) -> SigResult<usize> {
    if start + 1 >= s.len() || s[start] != C_GENERIC_START {
        return Err(());
    }
    let mut p = start + 1;
    loop {
        let c = *s.get(p).ok_or(())?;
        if c == C_GENERIC_END {
            return Ok(p);
        }
        let e = scan_type_argument_signature(s, p)?;
        p = e + 1;
    }
}

fn scan_type_argument_signature(s: &[char], start: usize) -> SigResult<usize> {
    let c = *s.get(start).ok_or(())?;
    match c {
        C_STAR => Ok(start),
        C_EXTENDS | C_SUPER => scan_type_bound_signature(s, start),
        _ => scan_type_signature(s, start),
    }
}

// ── appendXxx (toCharArray) ──────────────────────────────────────────────────

fn append_argument_simple_names(name: &[char], start: usize, end: usize, buf: &mut Vec<char>) {
    buf.push('<');
    let mut depth = 0;
    let mut argument_start: usize = 0;
    let mut count = 0;
    for i in start..=end {
        match name[i] {
            '<' => {
                depth += 1;
                if depth == 1 {
                    argument_start = i + 1;
                }
            }
            '>' => {
                if depth == 1 {
                    if count > 0 {
                        buf.push(',');
                    }
                    append_simple_name(name, argument_start, i - 1, buf);
                    count += 1;
                }
                depth -= 1;
            }
            ',' => {
                if depth == 1 {
                    if count > 0 {
                        buf.push(',');
                    }
                    append_simple_name(name, argument_start, i - 1, buf);
                    count += 1;
                    argument_start = i + 1;
                }
            }
            _ => {}
        }
    }
    buf.push('>');
}

fn append_array_type_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>, varargs: bool) -> SigResult<usize> {
    let len = s.len();
    if start + 1 >= len || s[start] != C_ARRAY {
        return Err(());
    }
    let mut index = start + 1;
    let mut c = s[index];
    while c == C_ARRAY {
        if index + 1 >= len {
            return Err(());
        }
        index += 1;
        c = s[index];
    }
    let e = append_type_signature(s, index, fq, buf, false)?;
    let dims = index - start;
    for _ in 1..dims {
        buf.push('[');
        buf.push(']');
    }
    if varargs {
        buf.extend(['.', '.', '.']);
    } else {
        buf.push('[');
        buf.push(']');
    }
    Ok(e)
}

fn append_capture_type_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>) -> SigResult<usize> {
    if start + 1 >= s.len() || s[start] != C_CAPTURE {
        return Err(());
    }
    buf.extend("capture-of ".chars());
    append_type_argument_signature(s, start + 1, fq, buf)
}

fn append_class_type_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>) -> SigResult<usize> {
    if start + 2 >= s.len() {
        return Err(());
    }
    let c = s[start];
    if c != C_RESOLVED && c != C_UNRESOLVED {
        return Err(());
    }
    let resolved = c == C_RESOLVED;
    let mut remove_package_qualifiers = !fq;
    if !resolved {
        remove_package_qualifiers = false;
    }
    let mut p = start + 1;
    let checkpoint = buf.len();
    let mut inner_type_start: isize = -1;
    let mut in_anonymous_type = false;
    loop {
        let c = *s.get(p).ok_or(())?;
        let prev_c = s[p - 1];
        let next_c = if p + 1 < s.len() { s[p + 1] } else { '\0' };
        match c {
            C_SEMICOLON => return Ok(p),
            C_GENERIC_START => {
                let e = append_type_argument_signatures(s, p, fq, buf)?;
                remove_package_qualifiers = false;
                p = e;
            }
            C_DOT => {
                if remove_package_qualifiers {
                    buf.truncate(checkpoint);
                } else {
                    buf.push('.');
                }
            }
            '/' => {
                if remove_package_qualifiers {
                    buf.truncate(checkpoint);
                } else {
                    buf.push('/');
                }
            }
            C_DOLLAR => {
                if next_c == C_DOT {
                    buf.push('$');
                } else {
                    let mut found_dot_after_dollar = false;
                    if prev_c == C_DOT {
                        let mut i = p + 1;
                        while i < s.len() {
                            let ch = s[i];
                            i += 1;
                            if ch == C_DOT {
                                found_dot_after_dollar = true;
                                break;
                            }
                        }
                    }
                    if found_dot_after_dollar {
                        buf.push('$');
                    } else {
                        inner_type_start = buf.len() as isize;
                        in_anonymous_type = false;
                        if resolved {
                            remove_package_qualifiers = false;
                            buf.push('.');
                        }
                    }
                }
            }
            _ => {
                if inner_type_start != -1 && !in_anonymous_type && c.is_ascii_digit() {
                    in_anonymous_type = true;
                    buf.truncate(inner_type_start as usize);
                    let tail: Vec<char> = buf.split_off(checkpoint);
                    buf.extend("new ".chars());
                    buf.extend(tail);
                    buf.extend("(){}".chars());
                }
                if !in_anonymous_type {
                    buf.push(c);
                }
                inner_type_start = -1;
            }
        }
        p += 1;
    }
}

fn append_intersection_type_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>) -> SigResult<usize> {
    if start + 1 >= s.len() || s[start] != C_INTERSECTION {
        return Err(());
    }
    let mut start = append_class_type_signature(s, start + 1, fq, buf)?;
    if start + 1 < s.len() {
        start += 1;
        if s[start] != C_COLON {
            return Err(());
        }
        while s[start] == C_COLON {
            buf.extend(" | ".chars());
            start = append_class_type_signature(s, start + 1, fq, buf)?;
            if start == s.len() - 1 {
                return Ok(start);
            } else if start > s.len() - 1 {
                return Err(());
            }
            start += 1;
        }
    }
    Ok(start)
}

fn check_name(name: &str, type_name: &[char], pos: usize, length: usize) -> isize {
    let n: Vec<char> = name.chars().collect();
    if pos + n.len() <= type_name.len() && type_name[pos..pos + n.len()] == n[..] {
        let pos = pos + n.len();
        if pos == length {
            return pos as isize;
        }
        let c = type_name[pos];
        match c {
            ' ' | '.' | '<' | '>' | '[' | ',' => return pos as isize,
            _ if is_whitespace(c) => return pos as isize,
            _ => {}
        }
    }
    -1
}

fn consume_whitespace(type_name: &[char], mut pos: usize, length: usize) -> usize {
    while pos < length {
        let c = type_name[pos];
        if c != ' ' && !is_whitespace(c) {
            break;
        }
        pos += 1;
    }
    pos
}

fn append_simple_name(name: &[char], start: usize, end: usize, buf: &mut Vec<char>) {
    let mut start = start;
    let mut last_dot: isize = -1;
    let mut last_generic_start: isize = -1;
    let mut last_generic_end: isize = -1;
    let mut depth = 0;
    if name[start] == '?' {
        buf.push('?');
        let mut index = consume_whitespace(name, start + 1, end + 1);
        match name.get(index) {
            Some('e') => {
                let check = check_name("extends", name, index, end);
                if check > 0 {
                    buf.extend(" extends ".chars());
                    index = consume_whitespace(name, check as usize, end + 1);
                }
            }
            Some('s') => {
                let check = check_name("super", name, index, end + 1);
                if check > 0 {
                    buf.extend(" super ".chars());
                    index = consume_whitespace(name, check as usize, end + 1);
                }
            }
            _ => {}
        }
        start = index;
    }
    let mut i = end as isize;
    while i >= start as isize {
        let c = name[i as usize];
        match c {
            '.' => {
                if depth == 0 {
                    last_dot = i;
                    let c0 = name[start];
                    if c0 == C_EXTENDS || c0 == C_SUPER {
                        buf.push(c0);
                    }
                    break;
                }
            }
            '<' => {
                depth -= 1;
                if depth == 0 {
                    last_generic_start = i;
                }
            }
            '>' => {
                if depth == 0 {
                    last_generic_end = i;
                }
                depth += 1;
            }
            _ => {}
        }
        i -= 1;
    }
    let name_start = if last_dot < 0 { start } else { (last_dot + 1) as usize };
    let name_end = if last_generic_start < 0 { end + 1 } else { last_generic_start as usize };
    buf.extend_from_slice(&name[name_start..name_end]);
    if last_generic_start >= 0 {
        append_argument_simple_names(name, last_generic_start as usize, last_generic_end as usize, buf);
        let from = (last_generic_end + 1) as usize;
        let count = end as isize - last_generic_end;
        if count > 0 {
            buf.extend_from_slice(&name[from..from + count as usize]);
        }
    }
}

fn append_type_argument_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>) -> SigResult<usize> {
    let c = *s.get(start).ok_or(())?;
    match c {
        C_STAR => {
            buf.push('?');
            Ok(start)
        }
        C_EXTENDS => {
            buf.extend("? extends ".chars());
            append_type_signature(s, start + 1, fq, buf, false)
        }
        C_SUPER => {
            buf.extend("? super ".chars());
            append_type_signature(s, start + 1, fq, buf, false)
        }
        _ => append_type_signature(s, start, fq, buf, false),
    }
}

fn append_type_argument_signatures(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>) -> SigResult<usize> {
    if start + 1 >= s.len() || s[start] != C_GENERIC_START {
        return Err(());
    }
    buf.push('<');
    let mut p = start + 1;
    let mut count = 0;
    loop {
        let c = *s.get(p).ok_or(())?;
        if c == C_GENERIC_END {
            buf.push('>');
            return Ok(p);
        }
        if count != 0 {
            buf.push(',');
        }
        let e = append_type_argument_signature(s, p, fq, buf)?;
        count += 1;
        p = e + 1;
    }
}

fn append_type_signature(s: &[char], start: usize, fq: bool, buf: &mut Vec<char>, varargs: bool) -> SigResult<usize> {
    let c = *s.get(start).ok_or(())?;
    if varargs {
        return match c {
            C_ARRAY => append_array_type_signature(s, start, fq, buf, true),
            _ => Err(()),
        };
    }
    match c {
        C_ARRAY => append_array_type_signature(s, start, fq, buf, false),
        C_RESOLVED | C_UNRESOLVED => append_class_type_signature(s, start, fq, buf),
        C_TYPE_VARIABLE => {
            let e = scan_type_variable_signature(s, start)?;
            buf.extend_from_slice(&s[start + 1..e]);
            Ok(e)
        }
        C_BOOLEAN => push(buf, "boolean", start),
        C_BYTE => push(buf, "byte", start),
        C_CHAR => push(buf, "char", start),
        C_DOUBLE => push(buf, "double", start),
        C_FLOAT => push(buf, "float", start),
        C_INT => push(buf, "int", start),
        C_LONG => push(buf, "long", start),
        C_SHORT => push(buf, "short", start),
        C_VOID => push(buf, "void", start),
        C_CAPTURE => append_capture_type_signature(s, start, fq, buf),
        C_INTERSECTION => append_intersection_type_signature(s, start, fq, buf),
        C_STAR | C_EXTENDS | C_SUPER => append_type_argument_signature(s, start, fq, buf),
        _ => Err(()),
    }
}

fn push(buf: &mut Vec<char>, s: &str, ret: usize) -> SigResult<usize> {
    buf.extend(s.chars());
    Ok(ret)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// `Signature.toCharArray(char[])` / `Signature.toString(String)`.
pub fn to_string(signature: &str) -> SigResult<String> {
    let s = chars(signature);
    if s.is_empty() {
        return Err(());
    }
    if s[0] == C_PARAM_START || s[0] == C_GENERIC_START {
        return to_method_string(signature, Some(""), None, true, true, false);
    }
    let mut buf = Vec::with_capacity(s.len() + 10);
    append_type_signature(&s, 0, true, &mut buf, false)?;
    Ok(string(&buf))
}

/// `Signature.toCharArray(methodSignature, methodName, parameterNames, fullyQualifyTypeNames, includeReturnType, isVarArgs)`.
pub fn to_method_string(
    method_signature: &str,
    method_name: Option<&str>,
    parameter_names: Option<&[String]>,
    fq: bool,
    include_return_type: bool,
    varargs: bool,
) -> SigResult<String> {
    let s = chars(method_signature);
    if !s.contains(&C_PARAM_START) {
        return Err(());
    }
    let mut buf = Vec::new();
    if include_return_type {
        let rts = chars(&get_return_type(method_signature)?);
        append_type_signature(&rts, 0, fq, &mut buf, false)?;
        buf.push(' ');
    }
    if let Some(n) = method_name {
        buf.extend(n.chars());
    }
    buf.push('(');
    let pts = get_parameter_types(method_signature)?;
    let max = pts.len();
    let mut index = max as isize - 1;
    for i in (0..max).rev() {
        if pts[i].starts_with(C_ARRAY) {
            break;
        }
        index -= 1;
    }
    for (i, p) in pts.iter().enumerate() {
        let pc = chars(p);
        if i as isize == index {
            append_type_signature(&pc, 0, fq, &mut buf, varargs)?;
        } else {
            append_type_signature(&pc, 0, fq, &mut buf, false)?;
        }
        if let Some(names) = parameter_names {
            buf.push(' ');
            buf.extend(names[i].chars());
        }
        if i != pts.len() - 1 {
            buf.push(',');
            buf.push(' ');
        }
    }
    buf.push(')');
    Ok(string(&buf))
}

pub fn get_parameter_count(method_signature: &str) -> SigResult<usize> {
    let s = chars(method_signature);
    let mut i = s.iter().position(|&c| c == C_PARAM_START).ok_or(())? + 1;
    let mut count = 0;
    loop {
        if *s.get(i).ok_or(())? == C_PARAM_END {
            return Ok(count);
        }
        let e = scan_type_signature(&s, i)?;
        i = e + 1;
        count += 1;
    }
}

pub fn get_parameter_types(method_signature: &str) -> SigResult<Vec<String>> {
    let s = chars(method_signature);
    let mut i = s.iter().position(|&c| c == C_PARAM_START).ok_or(())? + 1;
    let mut out = Vec::new();
    loop {
        if *s.get(i).ok_or(())? == C_PARAM_END {
            return Ok(out);
        }
        let e = scan_type_signature(&s, i)?;
        out.push(string(&s[i..=e]));
        i = e + 1;
    }
}

pub fn get_return_type(method_signature: &str) -> SigResult<String> {
    let s = chars(method_signature);
    let paren = s.iter().rposition(|&c| c == C_PARAM_END).ok_or(())?;
    let last = scan_type_signature(&s, paren + 1)?;
    Ok(string(&s[paren + 1..=last]))
}

pub fn get_qualifier(name: &str) -> String {
    let s = chars(name);
    let first_generic = s.iter().position(|&c| c == C_GENERIC_START);
    let end = match first_generic {
        None => s.len() as isize - 1,
        Some(g) => g as isize,
    };
    // CharOperation.lastIndexOf(char, array, start, end): search from end down to start (inclusive)
    let mut last_dot: isize = -1;
    let mut i = end.min(s.len() as isize - 1);
    while i >= 0 {
        if s[i as usize] == C_DOT {
            last_dot = i;
            break;
        }
        i -= 1;
    }
    if last_dot == -1 {
        return String::new();
    }
    string(&s[..last_dot as usize])
}

pub fn get_simple_name(name: &str) -> String {
    let s = chars(name);
    let mut last_dot: isize = -1;
    let mut last_generic_start: isize = -1;
    let mut last_generic_end: isize = -1;
    let mut depth = 0;
    let len = s.len();
    let mut i = len as isize - 1;
    while i >= 0 {
        match s[i as usize] {
            '.' => {
                if depth == 0 {
                    last_dot = i;
                    break;
                }
            }
            '<' => {
                depth -= 1;
                if depth == 0 {
                    last_generic_start = i;
                }
            }
            '>' => {
                if depth == 0 {
                    last_generic_end = i;
                }
                depth += 1;
            }
            _ => {}
        }
        i -= 1;
    }
    if last_generic_start < 0 {
        if last_dot < 0 {
            return name.to_owned();
        }
        return string(&s[(last_dot + 1) as usize..]);
    }
    let mut buf = Vec::new();
    let name_start = if last_dot < 0 { 0 } else { (last_dot + 1) as usize };
    buf.extend_from_slice(&s[name_start..last_generic_start as usize]);
    append_argument_simple_names(&s, last_generic_start as usize, last_generic_end as usize, &mut buf);
    buf.extend_from_slice(&s[(last_generic_end + 1) as usize..]);
    string(&buf)
}

pub fn get_signature_qualifier(type_signature: &str) -> String {
    let sig = chars(type_signature);
    let Ok(q) = to_string(type_signature) else { return String::new() };
    let qualified = chars(&q);
    let mut dot_count = 0;
    for &c in &sig {
        match c {
            C_DOT => dot_count += 1,
            C_GENERIC_START | C_DOLLAR => break,
            _ => {}
        }
    }
    if dot_count > 0 {
        for i in 0..qualified.len() {
            if qualified[i] == '.' {
                dot_count -= 1;
            }
            if dot_count <= 0 {
                return string(&qualified[..i]);
            }
        }
    }
    String::new()
}

pub fn get_signature_simple_name(type_signature: &str) -> String {
    let sig = chars(type_signature);
    let Ok(q) = to_string(type_signature) else { return String::new() };
    let qualified = chars(&q);
    let mut dot_count = 0;
    for &c in &sig {
        match c {
            C_DOT => dot_count += 1,
            C_GENERIC_START | C_DOLLAR => break,
            _ => {}
        }
    }
    if dot_count > 0 {
        let mut type_start = 0;
        for i in 0..qualified.len() {
            match qualified[i] {
                '.' => dot_count -= 1,
                ' ' => type_start = i + 1,
                _ => {}
            }
            if dot_count <= 0 {
                let simple = &qualified[i + 1..];
                if type_start > 0 && type_start < qualified.len() {
                    let mut r = qualified[..type_start].to_vec();
                    r.extend_from_slice(simple);
                    return string(&r);
                }
                return string(simple);
            }
        }
    }
    q
}

pub fn get_type_arguments(sig: &str) -> SigResult<Vec<String>> {
    let s = chars(sig);
    let len = s.len();
    if len < 2 || s[len - 2] != C_GENERIC_END {
        return Ok(Vec::new());
    }
    let mut count = 1;
    let mut start = len as isize - 2;
    while start >= 0 && count > 0 {
        start -= 1;
        if start < 0 {
            break;
        }
        match s[start as usize] {
            C_GENERIC_START => count -= 1,
            C_GENERIC_END => count += 1,
            _ => {}
        }
    }
    if start < 0 {
        return Err(());
    }
    let mut out = Vec::new();
    let mut p = (start + 1) as usize;
    loop {
        let c = *s.get(p).ok_or(())?;
        if c == C_GENERIC_END {
            return Ok(out);
        }
        let e = scan_type_argument_signature(&s, p)?;
        out.push(string(&s[p..=e]));
        p = e + 1;
    }
}

pub fn get_type_erasure(sig: &str) -> SigResult<String> {
    let s = chars(sig);
    let Some(mut end) = s.iter().position(|&c| c == C_GENERIC_START) else { return Ok(sig.to_owned()) };
    let _ = &mut end;
    let mut result = Vec::with_capacity(s.len());
    let mut start = 0;
    let mut deep: i32 = 0;
    for idx in end..s.len() {
        match s[idx] {
            C_GENERIC_START => {
                if deep == 0 {
                    result.extend_from_slice(&s[start..idx]);
                }
                deep += 1;
            }
            C_GENERIC_END => {
                deep -= 1;
                if deep < 0 {
                    return Err(());
                }
                if deep == 0 {
                    start = idx + 1;
                }
            }
            _ => {}
        }
    }
    if deep > 0 {
        return Err(());
    }
    result.extend_from_slice(&s[start..]);
    Ok(string(&result))
}

pub fn get_array_count(sig: &str) -> SigResult<usize> {
    let s = chars(sig);
    let mut count = 0;
    loop {
        match s.get(count) {
            Some(&C_ARRAY) => count += 1,
            Some(_) => return Ok(count),
            None => return Err(()),
        }
    }
}

pub fn get_element_type(sig: &str) -> SigResult<String> {
    let count = get_array_count(sig)?;
    Ok(sig.chars().skip(count).collect())
}

pub fn create_array_signature(sig: &str, count: usize) -> String {
    let mut s = "[".repeat(count);
    s.push_str(sig);
    s
}

pub fn remove_capture(sig: &str) -> String {
    sig.chars().filter(|&c| c != C_CAPTURE).collect()
}

pub fn to_qualified_name(segments: &[&str]) -> String {
    segments.join(".")
}

/// `Signature.createTypeSignature(typeName, isResolved)`.
pub fn create_type_signature(type_name: &str, is_resolved: bool) -> SigResult<String> {
    let t = chars(type_name);
    if t.is_empty() {
        return Err(());
    }
    let mut buf = Vec::new();
    let pos = encode_type_signature(&t, 0, is_resolved, t.len(), &mut buf)?;
    let pos = consume_whitespace(&t, pos, t.len());
    if pos < t.len() {
        return Err(());
    }
    Ok(string(&buf))
}

fn check_next_char(t: &[char], expected: char, pos: usize, length: usize, optional: bool) -> SigResult<isize> {
    let pos = consume_whitespace(t, pos, length);
    if pos < length && t[pos] == expected {
        return Ok((pos + 1) as isize);
    }
    if !optional {
        return Err(());
    }
    Ok(-1)
}

fn check_array_dimension(t: &[char], mut pos: usize, length: usize) -> isize {
    let mut balance = 0;
    while pos < length {
        match t[pos] {
            '<' => balance += 1,
            ',' => {
                if balance == 0 {
                    return -1;
                }
            }
            '>' => {
                if balance == 0 {
                    return -1;
                }
                balance -= 1;
            }
            '[' => {
                if balance == 0 {
                    return pos as isize;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    -1
}

fn encode_array_dimension(t: &[char], mut pos: usize, length: usize, buf: &mut Vec<char>) -> SigResult<usize> {
    while pos < length {
        let check = check_next_char(t, '[', pos, length, true)?;
        if check <= 0 {
            break;
        }
        pos = check_next_char(t, ']', check as usize, length, false)? as usize;
        buf.push(C_ARRAY);
    }
    Ok(pos)
}

fn encode_qualified_name(t: &[char], mut pos: usize, length: usize, buf: &mut Vec<char>) -> SigResult<usize> {
    let mut count = 0;
    let mut last: char = '\0';
    while pos < length {
        let c = t[pos];
        match c {
            '<' | '>' | '[' | ',' | '&' => break,
            '.' => {
                buf.push(C_DOT);
                last = C_DOT;
                count += 1;
            }
            _ => {
                if c == ' ' || is_whitespace(c) {
                    if last == C_DOT {
                        pos = consume_whitespace(t, pos, length) - 1;
                        pos += 1;
                        continue;
                    }
                    let check = check_next_char(t, '.', pos, length, true)?;
                    if check > 0 {
                        buf.push(C_DOT);
                        last = C_DOT;
                        count += 1;
                        pos = check as usize;
                        pos += 1;
                        continue;
                    }
                    break;
                }
                buf.push(c);
                last = c;
                count += 1;
            }
        }
        pos += 1;
    }
    if count == 0 {
        return Err(());
    }
    Ok(pos)
}

fn encode_type_signature(t: &[char], start: usize, resolved: bool, length: usize, buf: &mut Vec<char>) -> SigResult<usize> {
    let mut pos = consume_whitespace(t, start, length);
    if pos >= length {
        return Err(());
    }
    let c = t[pos];
    let base: &[(&str, char)] = &[
        ("boolean", C_BOOLEAN),
        ("byte", C_BYTE),
        ("double", C_DOUBLE),
        ("float", C_FLOAT),
        ("int", C_INT),
        ("long", C_LONG),
        ("short", C_SHORT),
        ("void", C_VOID),
        ("char", C_CHAR),
    ];
    for (name, code) in base {
        if name.starts_with(c) {
            let check = check_name(name, t, pos, length);
            if check > 0 {
                pos = encode_array_dimension(t, check as usize, length, buf)?;
                buf.push(*code);
                return Ok(pos);
            }
        }
    }
    let mut wildcard = c == '?';
    if c == 'c' {
        let check = check_name("capture-of", t, pos, length);
        if check > 0 {
            let p2 = consume_whitespace(t, check as usize, length);
            if t.get(p2) == Some(&'?') {
                buf.push(C_CAPTURE);
                pos = p2;
                wildcard = true;
            }
        }
    }
    if wildcard {
        pos = consume_whitespace(t, pos + 1, length);
        let check = check_name("extends", t, pos, length);
        if check > 0 {
            buf.push(C_EXTENDS);
            return encode_type_signature(t, check as usize, resolved, length, buf);
        }
        let check = check_name("super", t, pos, length);
        if check > 0 {
            buf.push(C_SUPER);
            return encode_type_signature(t, check as usize, resolved, length, buf);
        }
        buf.push(C_STAR);
        return Ok(pos);
    }
    let check = check_array_dimension(t, pos, length);
    let end: isize = if check > 0 { encode_array_dimension(t, check as usize, length, buf)? as isize } else { -1 };
    buf.push(if resolved { C_RESOLVED } else { C_UNRESOLVED });
    loop {
        pos = encode_qualified_name(t, pos, length, buf)?;
        let check = check_next_char(t, '<', pos, length, true)?;
        if check > 0 {
            buf.push(C_GENERIC_START);
            let close = check_next_char(t, '>', check as usize, length, true)?;
            if close > 0 {
                pos = close as usize;
                buf.push(C_GENERIC_END);
            } else {
                pos = encode_type_signature(t, check as usize, resolved, length, buf)?;
                loop {
                    let comma = check_next_char(t, ',', pos, length, true)?;
                    if comma <= 0 {
                        break;
                    }
                    pos = encode_type_signature(t, comma as usize, resolved, length, buf)?;
                }
                pos = check_next_char(t, '>', pos, length, false)? as usize;
                buf.push(C_GENERIC_END);
            }
        }
        let dot = check_next_char(t, '.', pos, length, true)?;
        if dot > 0 {
            buf.push(C_DOT);
            pos = dot as usize;
        } else {
            break;
        }
    }
    buf.push(C_NAME_END);
    loop {
        let amp = check_next_char(t, '&', pos, length, true)?;
        if amp <= 0 {
            break;
        }
        if buf.first() != Some(&C_UNION) {
            buf.insert(0, C_UNION);
        }
        buf.push(C_COLON);
        pos = encode_type_signature(t, amp as usize, resolved, length, buf)?;
        if pos == length {
            break;
        }
    }
    if end > 0 {
        pos = end as usize;
    }
    Ok(pos)
}

// ── SignatureUtil ────────────────────────────────────────────────────────────

const OBJECT_SIGNATURE: &str = "Ljava.lang.Object;";
const NULL_TYPE_SIGNATURE: &str = "Tnull;";

pub fn fix83600(sig: &str) -> String {
    if sig.chars().count() < 2 {
        return sig.to_owned();
    }
    remove_capture(sig)
}

pub fn get_upper_bound(sig: &str) -> String {
    let s = chars(sig);
    if s.is_empty() {
        return sig.to_owned();
    }
    if s[0] == C_STAR {
        return OBJECT_SIGNATURE.to_owned();
    }
    let super_index = s.iter().position(|&c| c == C_SUPER);
    if super_index == Some(0) {
        return OBJECT_SIGNATURE.to_owned();
    }
    if let Some(si) = super_index {
        let after = s.get(si + 1).copied().unwrap_or('\0');
        if after == C_STAR {
            let mut t = s[..si].to_vec();
            t.push(C_STAR);
            t.extend_from_slice(&s[si + 2..]);
            return get_upper_bound(&string(&t));
        }
        if after == C_EXTENDS {
            let te = type_end(&s, si + 1);
            let mut t = s[..si].to_vec();
            t.push(C_STAR);
            if te <= s.len() {
                t.extend_from_slice(&s[te..]);
            }
            return get_upper_bound(&string(&t));
        }
    }
    if s[0] == C_EXTENDS {
        return string(&s[1..]);
    }
    sig.to_owned()
}

pub fn get_lower_bound(sig: &str) -> String {
    let s = chars(sig);
    if s.is_empty() {
        return sig.to_owned();
    }
    if s.len() == 1 && s[0] == C_STAR {
        return sig.to_owned();
    }
    let ext = s.iter().position(|&c| c == C_EXTENDS);
    if ext == Some(0) {
        return NULL_TYPE_SIGNATURE.to_owned();
    }
    if let Some(i) = ext {
        let after = s.get(i + 1).copied().unwrap_or('\0');
        if after == C_STAR || after == C_EXTENDS {
            return NULL_TYPE_SIGNATURE.to_owned();
        }
    }
    if let Ok(args) = get_type_arguments(sig) {
        if args.iter().any(|a| a == NULL_TYPE_SIGNATURE) {
            return NULL_TYPE_SIGNATURE.to_owned();
        }
    }
    if s[0] == C_SUPER {
        return string(&s[1..]);
    }
    sig.to_owned()
}

fn type_end(s: &[char], mut pos: usize) -> usize {
    let mut depth = 0;
    while pos < s.len() {
        match s[pos] {
            C_GENERIC_START => depth += 1,
            C_GENERIC_END => {
                if depth == 0 {
                    return pos;
                }
                depth -= 1;
            }
            C_SEMICOLON => {
                if depth == 0 {
                    return pos + 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    pos + 1
}

/// `SignatureUtil.stripSignatureToFQN`.
pub fn strip_signature_to_fqn(sig: &str) -> SigResult<String> {
    let e = get_type_erasure(sig)?;
    let e = get_element_type(&e)?;
    to_string(&e)
}

/// `SignatureUtil.getQualifiedTypeName(proposal)`.
pub fn qualified_type_name(signature: &str) -> String {
    get_type_erasure(signature).and_then(|e| to_string(&e)).unwrap_or_default()
}

/// `SignatureUtil.getSimpleTypeName(proposal)`.
pub fn simple_type_name(signature: &str) -> String {
    get_simple_name(&qualified_type_name(signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_string_works() {
        assert_eq!(to_string("Ljava.util.List<Ljava.lang.String;>;").unwrap(), "java.util.List<java.lang.String>");
        assert_eq!(to_string("[I").unwrap(), "int[]");
        assert_eq!(to_string("TE;").unwrap(), "E");
        assert_eq!(to_string("+Ljava.lang.Number;").unwrap(), "? extends java.lang.Number");
        assert_eq!(to_string("Ljava.util.Map$Entry;").unwrap(), "java.util.Map.Entry");
        assert_eq!(get_simple_name("java.util.List<java.lang.String>"), "List<String>");
        assert_eq!(get_signature_simple_name("Ljava.util.List<Ljava.lang.String;>;"), "List<String>");
        assert_eq!(get_qualifier("java.util.List"), "java.util");
        assert_eq!(get_parameter_types("(ILjava.lang.String;)V").unwrap(), vec!["I", "Ljava.lang.String;"]);
        assert_eq!(get_return_type("(ILjava.lang.String;)V").unwrap(), "V");
        assert_eq!(get_type_erasure("Ljava.util.List<Ljava.lang.String;>;").unwrap(), "Ljava.util.List;");
        assert_eq!(create_type_signature("String", false).unwrap(), "QString;");
        assert_eq!(create_type_signature("java.util.List<String>", true).unwrap(), "Ljava.util.List<LString;>;");
        assert_eq!(create_type_signature("int[]", true).unwrap(), "[I");
        assert_eq!(get_signature_qualifier("Ljava.util.List;"), "java.util");
        assert_eq!(get_upper_bound("+Ljava.lang.Number;"), "Ljava.lang.Number;");
        assert_eq!(get_lower_bound("-Ljava.lang.Number;"), "Ljava.lang.Number;");
        assert_eq!(get_type_arguments("Ljava.util.Map<TK;TV;>;").unwrap(), vec!["TK;", "TV;"]);
    }
}
