//! Port of jdt.ls `InlayHintFilter` / `InlayHintFilterManager`: matching of
//! `java.inlayHints.parameterNames.exclusions` patterns against methods.
//!
//! A pattern is parsed the way JDT's `SearchPattern.createPattern(pattern,
//! METHOD, DECLARATIONS, R_PATTERN_MATCH | R_CASE_SENSITIVE)` parses a method
//! pattern (`[declaringType.]selector[(paramType, ...)][ returnType]`), and the
//! resulting `MethodPattern` is matched like `InlayHintFilter.match(IMethod)`:
//! the "parameter types" of the pattern are matched against parameter *names*.

/// The parts of a JDT `IMethod` the filter looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// `IMethod.getElementName()` (the simple type name for constructors).
    pub name: String,
    /// `getDeclaringType().getPackageFragment().getElementName()`.
    pub package: String,
    /// `getDeclaringType().getTypeQualifiedName()` (`Outer$Inner`).
    pub type_qualified_name: String,
    /// `IMethod.getParameterNames()`.
    pub parameter_names: Vec<String>,
}

// SearchPattern match modes.
const R_EXACT_MATCH: u32 = 0;
const R_PATTERN_MATCH: u32 = 2;
const R_CASE_SENSITIVE: u32 = 8;

/// `InlayHintFilter`: one valid exclusion pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHintFilter {
    pattern: MethodPattern,
    match_mode: u32,
}

/// The fields of JDT's `MethodPattern` the filter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodPattern {
    selector: Option<Vec<char>>,
    declaring_qualification: Option<Vec<char>>,
    declaring_simple_name: Option<Vec<char>>,
    /// `None` when the pattern has no parameter list.
    parameter_simple_names: Option<Vec<Option<Vec<char>>>>,
}

impl InlayHintFilter {
    /// `new InlayHintFilter(pattern)`; `None` when `!isValid()`.
    pub fn new(method_pattern: &str) -> Option<Self> {
        let method_pattern = if method_pattern.starts_with('(') {
            format!("*{method_pattern}")
        } else {
            method_pattern.to_owned()
        };
        let match_rule = validate_match_rule(&method_pattern, R_PATTERN_MATCH | R_CASE_SENSITIVE);
        let pattern = create_method_pattern(&method_pattern)?;
        Some(Self { pattern, match_mode: match_rule & MATCH_MODE_MASK })
    }

    /// `InlayHintFilter.match(IMethod)`.
    pub fn matches(&self, method: &MethodInfo) -> bool {
        let args_length = method.parameter_names.len();
        if let Some(q) = &self.pattern.declaring_qualification {
            if !self.matches_name(Some(q), &method.package) {
                return false;
            }
        }
        if let Some(n) = &self.pattern.declaring_simple_name {
            if !self.matches_name(Some(n), &method.type_qualified_name) {
                return false;
            }
        }
        // Verify method name and parameter count.
        if !self.matches_name(self.pattern.selector.as_ref(), &method.name) {
            return false;
        }
        if let Some(params) = &self.pattern.parameter_simple_names {
            if params.len() != args_length {
                return false;
            }
            // Check parameter names.
            for parameter_name in &method.parameter_names {
                let any = (0..args_length).any(|i| self.matches_name(params[i].as_ref(), parameter_name));
                if !any {
                    return false;
                }
            }
        }
        true
    }

    /// `matchesName` / `matchNameValue` (case sensitive).
    fn matches_name(&self, pattern: Option<&Vec<char>>, name: &str) -> bool {
        let Some(pattern) = pattern else { return true };
        let name: Vec<char> = name.chars().collect();
        if name.is_empty() {
            return pattern.is_empty();
        } else if pattern.is_empty() {
            return false;
        }
        match self.match_mode {
            R_EXACT_MATCH => pattern == &name,
            R_PATTERN_MATCH => char_operation_match(pattern, &name),
            _ => false,
        }
    }
}

/// `InlayHintFilterManager`: the exclusion filters built from preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlayHintFilterManager {
    exclusions: Vec<InlayHintFilter>,
}

impl InlayHintFilterManager {
    pub fn from_exclusions(exclusions: Option<&[String]>) -> Self {
        let exclusions = exclusions.unwrap_or_default().iter().filter_map(|p| InlayHintFilter::new(p)).collect();
        Self { exclusions }
    }

    /// `InlayHintFilterManager.match(IMethod)`; `None` stands for a `null` method.
    pub fn matches(&self, method: Option<&MethodInfo>) -> bool {
        let Some(method) = method else { return false };
        self.exclusions.iter().any(|f| f.matches(method))
    }
}

// ─── SearchPattern ────────────────────────────────────────────────────────────

const R_PREFIX_MATCH: u32 = 1;
const R_REGEXP_MATCH: u32 = 4;
const R_CAMELCASE_MATCH: u32 = 128;
const R_CAMELCASE_SAME_PART_COUNT_MATCH: u32 = 256;
const MATCH_MODE_MASK: u32 =
    R_EXACT_MATCH | R_PREFIX_MATCH | R_PATTERN_MATCH | R_REGEXP_MATCH | R_CAMELCASE_MATCH | R_CAMELCASE_SAME_PART_COUNT_MATCH;

/// `SearchPattern.validateMatchRule` for the rules the filter uses.
fn validate_match_rule(pattern: &str, mut rule: u32) -> u32 {
    if pattern.contains('*') || pattern.contains('?') {
        rule |= R_PATTERN_MATCH;
    } else {
        rule &= !R_PATTERN_MATCH;
    }
    if rule & R_PATTERN_MATCH != 0 {
        rule &= !(R_CAMELCASE_MATCH | R_CAMELCASE_SAME_PART_COUNT_MATCH | R_PREFIX_MATCH);
    }
    rule
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Whitespace,
    Dot,
    LParen,
    RParen,
    Comma,
    Less,
    Greater,
    RightShift,
    UnsignedRightShift,
    Other,
}

/// A minimal JDT `Scanner` (whitespace tokens on): only the token kinds the
/// method pattern parser distinguishes; everything else is `Other`.
/// `None` stands for an `InvalidInputException`.
fn tokenize(s: &str) -> Option<Vec<(Tok, String)>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let tok = if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            Tok::Whitespace
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            Tok::Other
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            Tok::Other
        } else if c == '"' || c == '\'' {
            i += 1;
            loop {
                match chars.get(i) {
                    None | Some('\n') | Some('\r') => return None,
                    Some('\\') => i += 2,
                    Some(&q) if q == c => {
                        i += 1;
                        break;
                    }
                    Some(_) => i += 1,
                }
            }
            Tok::Other
        } else {
            let rest: String = chars[i..].iter().take(4).collect();
            let (tok, len) = if rest.starts_with("...") {
                (Tok::Other, 3)
            } else if rest.starts_with(">>>=") {
                (Tok::Other, 4)
            } else if rest.starts_with(">>>") {
                (Tok::UnsignedRightShift, 3)
            } else if rest.starts_with(">>=") {
                (Tok::Other, 3)
            } else if rest.starts_with(">>") {
                (Tok::RightShift, 2)
            } else if rest.starts_with(">=") || rest.starts_with("<=") || rest.starts_with("<<") {
                (Tok::Other, 2)
            } else {
                let t = match c {
                    '.' => Tok::Dot,
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    ',' => Tok::Comma,
                    '<' => Tok::Less,
                    '>' => Tok::Greater,
                    _ => Tok::Other,
                };
                (t, 1)
            };
            i += len;
            tok
        };
        out.push((tok, chars[start..i].iter().collect()));
    }
    Some(out)
}

/// `SearchPattern.createMethodOrConstructorPattern(pattern, limitTo, rule, false)`.
fn create_method_pattern(pattern: &str) -> Option<MethodPattern> {
    if pattern.is_empty() {
        return None;
    }
    #[derive(PartialEq)]
    enum Mode {
        InsideSelector,
        InsideTypeArguments,
        InsideParameter,
        InsideReturnType,
    }
    let tokens = tokenize(pattern)?;
    let mut last_token: Option<Tok> = None;
    let mut declaring_type: Option<String> = None;
    let mut selector: Option<String> = None;
    let mut parameter_type: Option<String> = None;
    let mut parameter_types: Option<Vec<String>> = None;
    let mut type_arguments_string: Option<String> = None;
    let mut return_type: Option<String> = None;
    let mut found_closing_parenthesis = false;
    let mut mode = Mode::InsideSelector;
    let mut arg_count: i32 = 0;
    let closes = |t: Tok| match t {
        Tok::Greater => 1,
        Tok::RightShift => 2,
        Tok::UnsignedRightShift => 3,
        _ => 0,
    };

    for (token, text) in tokens {
        match mode {
            Mode::InsideSelector => {
                if arg_count == 0 {
                    match token {
                        Tok::Less => {
                            arg_count += 1;
                            if selector.is_none() || last_token == Some(Tok::Dot) {
                                type_arguments_string = Some(text.clone());
                                mode = Mode::InsideTypeArguments;
                            } else {
                                let mut dt = match declaring_type.take() {
                                    None => selector.clone().unwrap_or_default(),
                                    Some(dt) => format!("{dt}.{}", selector.clone().unwrap_or_default()),
                                };
                                dt.push_str(&text);
                                declaring_type = Some(dt);
                                selector = None;
                            }
                        }
                        Tok::Dot => {
                            if type_arguments_string.is_some() {
                                return None; // invalid syntax
                            }
                            match &mut declaring_type {
                                None => {
                                    declaring_type = Some(selector.clone()?);
                                }
                                Some(dt) => {
                                    if let Some(sel) = &selector {
                                        dt.push_str(&text);
                                        dt.push_str(sel);
                                    }
                                }
                            }
                            selector = None;
                        }
                        Tok::LParen => {
                            parameter_types = Some(Vec::new());
                            mode = Mode::InsideParameter;
                        }
                        Tok::Whitespace => match last_token {
                            Some(Tok::Whitespace | Tok::Dot | Tok::Greater | Tok::RightShift | Tok::UnsignedRightShift) => {}
                            _ => mode = Mode::InsideReturnType,
                        },
                        _ => selector.get_or_insert_with(String::new).push_str(&text),
                    }
                } else {
                    match token {
                        Tok::Less => arg_count += 1,
                        t if closes(t) > 0 => arg_count -= closes(t),
                        _ => {}
                    }
                    declaring_type.as_mut()?.push_str(&text);
                }
            }
            Mode::InsideTypeArguments => {
                type_arguments_string.as_mut()?.push_str(&text);
                match token {
                    Tok::Less => arg_count += 1,
                    t if closes(t) > 0 => {
                        arg_count -= closes(t);
                        if arg_count == 0 {
                            mode = Mode::InsideSelector;
                        }
                    }
                    _ => {}
                }
            }
            Mode::InsideParameter => {
                if arg_count == 0 {
                    match token {
                        Tok::Whitespace => {}
                        Tok::Comma => {
                            let p = parameter_type.take()?;
                            if let Some(types) = &mut parameter_types {
                                types.push(p);
                            }
                        }
                        Tok::RParen => {
                            found_closing_parenthesis = true;
                            if let (Some(p), Some(types)) = (parameter_type.take(), &mut parameter_types) {
                                types.push(p);
                            }
                            mode = Mode::InsideReturnType;
                        }
                        _ => {
                            if token == Tok::Less {
                                arg_count += 1;
                                parameter_type.as_ref()?; // invalid syntax
                            }
                            parameter_type.get_or_insert_with(String::new).push_str(&text);
                        }
                    }
                } else {
                    parameter_type.as_mut()?.push_str(&text);
                    match token {
                        Tok::Less => arg_count += 1,
                        t if closes(t) > 0 => arg_count -= closes(t),
                        _ => {}
                    }
                }
            }
            Mode::InsideReturnType => {
                if arg_count == 0 {
                    match token {
                        Tok::Whitespace => {}
                        Tok::LParen => {
                            parameter_types = Some(Vec::new());
                            mode = Mode::InsideParameter;
                        }
                        _ => {
                            if token == Tok::Less {
                                arg_count += 1;
                                return_type.as_ref()?; // invalid syntax
                            }
                            return_type.get_or_insert_with(String::new).push_str(&text);
                        }
                    }
                } else {
                    return_type.as_mut()?.push_str(&text);
                    match token {
                        Tok::Less => arg_count += 1,
                        t if closes(t) > 0 => arg_count -= closes(t),
                        _ => {}
                    }
                }
            }
        }
        last_token = Some(token);
    }
    let parameter_count = parameter_types.as_ref().map(Vec::len);
    // Parenthesis mismatch.
    if parameter_count.is_some_and(|n| n > 0) && !found_closing_parenthesis {
        return None;
    }
    // Type arguments mismatch.
    if arg_count > 0 {
        return None;
    }

    // Selector.
    let selector: Vec<char> = selector?.chars().collect();
    let selector = if selector == ['*'] { None } else { Some(selector) };

    // Declaring type.
    let (mut declaring_qualification, mut declaring_simple_name) = (None, None);
    if let Some(dt) = declaring_type {
        let part: Vec<char> = type_erasure(&dt)?.chars().collect();
        match part.iter().rposition(|&c| c == '.') {
            Some(dot) => {
                let q = part[..dot].to_vec();
                declaring_qualification = if q == ['*'] { None } else { Some(q) };
                declaring_simple_name = Some(part[dot + 1..].to_vec());
            }
            None => declaring_simple_name = Some(part),
        }
        if declaring_simple_name.as_deref() == Some(&['*'][..]) {
            declaring_simple_name = None;
        }
    }

    // Parameter types: only the simple names are used by the filter.
    let parameter_simple_names = match parameter_types {
        Some(types) => {
            let mut names = Vec::with_capacity(types.len());
            for t in types {
                let part: Vec<char> = type_erasure(&t)?.chars().collect();
                let simple = match part.iter().rposition(|&c| c == '.') {
                    Some(dot) => part[dot + 1..].to_vec(),
                    None => part,
                };
                names.push(if simple == ['*'] { None } else { Some(simple) });
            }
            Some(names)
        }
        None => None,
    };

    Some(MethodPattern { selector, declaring_qualification, declaring_simple_name, parameter_simple_names })
}

/// `Signature.getTypeErasure` of `Signature.createTypeSignature(t)`, rendered
/// back as a type name: the type with its type arguments removed.  `None` for
/// unbalanced type arguments (`IllegalArgumentException`).
fn type_erasure(t: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in t.chars() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    (depth == 0).then_some(out)
}

/// `CharOperation.match(pattern, name, true)`: `*` and `?` wildcards.
fn char_operation_match(pattern: &[char], name: &[char]) -> bool {
    let pattern_end = pattern.len();
    let name_end = name.len();
    let mut i_pattern = 0;
    let mut i_name = 0;
    let mut pattern_char;
    // Check the first segment.
    loop {
        if i_pattern == pattern_end {
            return i_name == name_end;
        }
        pattern_char = pattern[i_pattern];
        if pattern_char == '*' {
            break;
        }
        if i_name == name_end {
            return false;
        }
        if pattern_char != name[i_name] && pattern_char != '?' {
            return false;
        }
        i_name += 1;
        i_pattern += 1;
    }
    // Check the sequence of star+segment.
    let mut segment_start = if pattern_char == '*' {
        i_pattern += 1;
        i_pattern
    } else {
        0
    };
    let mut prefix_start = i_name;
    while i_name < name_end {
        if i_pattern == pattern_end {
            i_pattern = segment_start;
            prefix_start += 1;
            i_name = prefix_start;
            continue;
        }
        pattern_char = pattern[i_pattern];
        if pattern_char == '*' {
            i_pattern += 1;
            segment_start = i_pattern;
            if segment_start == pattern_end {
                return true;
            }
            prefix_start = i_name;
            continue;
        }
        if name[i_name] != pattern_char && pattern_char != '?' {
            i_pattern = segment_start;
            prefix_start += 1;
            i_name = prefix_start;
            continue;
        }
        i_name += 1;
        i_pattern += 1;
    }
    segment_start == pattern_end
        || (i_name == name_end && i_pattern == pattern_end)
        || (i_pattern + 1 == pattern_end && pattern[i_pattern] == '*')
}

// ─── Tests: port of `InlayHintFilterManagerTest` ─────────────────────────────

#[cfg(test)]
mod tests {
    //! The upstream tests build `IMethod`s from working copies in the `hello`
    //! project (source folder `src`).  Here the methods of the primary type
    //! are read from the same sources with tree-sitter and turned into the
    //! `MethodInfo` JDT reports for them (package from the source-folder
    //! location, element name, parameter names).  Methods reached through a
    //! binding (`Arrays.asList`, `new Foo(...)`) are described by what JDT
    //! reports for that binding.
    use super::*;

    fn manager(exclusions: &[&str]) -> InlayHintFilterManager {
        let list: Vec<String> = exclusions.iter().map(|s| s.to_string()).collect();
        InlayHintFilterManager::from_exclusions(Some(&list))
    }

    /// `getWorkingCopy(path, source).findPrimaryType().getMethods()`.
    fn primary_type_methods(path: &str, source: &str) -> Vec<(MethodInfo, bool)> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_java::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let file_stem = path.rsplit('/').next().unwrap().trim_end_matches(".java");
        let package = path
            .strip_prefix("src/")
            .unwrap()
            .rsplit_once('/')
            .map(|(dir, _)| dir.replace('/', "."))
            .unwrap_or_default();
        let root = tree.root_node();
        let mut cursor = root.walk();
        let class = root
            .children(&mut cursor)
            .find(|n| {
                n.kind() == "class_declaration"
                    && n.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok()) == Some(file_stem)
            })
            .expect("primary type");
        let body = class.child_by_field_name("body").unwrap();
        let mut out = Vec::new();
        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            let is_constructor = member.kind() == "constructor_declaration";
            if member.kind() != "method_declaration" && !is_constructor {
                continue;
            }
            let name = member.child_by_field_name("name").unwrap().utf8_text(source.as_bytes()).unwrap();
            let params = member.child_by_field_name("parameters").unwrap();
            let mut pc = params.walk();
            let parameter_names = params
                .named_children(&mut pc)
                .filter(|p| p.kind() == "formal_parameter" || p.kind() == "spread_parameter")
                .filter_map(|p| p.child_by_field_name("name").or_else(|| p.named_child(p.named_child_count() - 1)))
                .map(|n| n.utf8_text(source.as_bytes()).unwrap().to_owned())
                .collect();
            out.push((
                MethodInfo {
                    name: name.to_owned(),
                    package: package.clone(),
                    type_qualified_name: file_stem.to_owned(),
                    parameter_names,
                },
                is_constructor,
            ));
        }
        out
    }

    #[test]
    fn test_method_simple_match() {
        let m = manager(&["*.print(*)"]);
        let methods = primary_type_methods(
            "src/Foo.java",
            "public class Foo {\n\
             \tvoid print(int i) {}\n\
             \tvoid output() {}\n\
             }\n",
        );
        assert_eq!(methods.len(), 2);
        for (method, _) in &methods {
            if method.name == "print" {
                assert!(m.matches(Some(method)));
            } else if method.name == "output" {
                assert!(!m.matches(Some(method)));
            }
        }
    }

    #[test]
    fn test_method_simple_match2() {
        let m = manager(&["*.Arrays.asList"]);
        // import java.util.Arrays; ... Arrays.asList(1, 2);
        // resolveMethodBinding().getJavaElement(): java.util.Arrays.asList(T... a)
        let method = MethodInfo {
            name: "asList".into(),
            package: "java.util".into(),
            type_qualified_name: "Arrays".into(),
            parameter_names: vec!["a".into()],
        };
        assert!(m.matches(Some(&method)));
    }

    #[test]
    fn test_method_start_match() {
        let m = manager(&["*.*print*(*)"]);
        let methods = primary_type_methods(
            "src/Foo.java",
            "public class Foo {\n\
             \tvoid print(int i) {}\n\
             \tvoid println(String s) {}\n\
             \tvoid sprint(String s) {}\n\
             }\n",
        );
        assert_eq!(methods.len(), 3);
        for (method, _) in &methods {
            if method.name.contains("print") {
                assert!(m.matches(Some(method)));
            }
        }
    }

    #[test]
    fn test_method_parameter_match() {
        let m = manager(&["*.foo(*,*)"]);
        let methods = primary_type_methods(
            "src/Foo.java",
            "public class Foo {\n\
             \tvoid foo(String s1) {}\n\
             \tvoid foo(String s1, String s2) {}\n\
             \tvoid foo(String s1, String s2, String s3) {}\n\
             }\n",
        );
        assert_eq!(methods.len(), 3);
        for (method, _) in &methods {
            if method.parameter_names.len() == 2 {
                assert!(m.matches(Some(method)));
            } else {
                assert!(!m.matches(Some(method)));
            }
        }
    }

    #[test]
    fn test_method_parameter_match2() {
        let m = manager(&["(from*, to*)"]);
        let methods = primary_type_methods(
            "src/Foo.java",
            "public class Foo {\n\
             \tvoid foo(String from) {}\n\
             \tvoid foo(String from, String to) {}\n\
             \tvoid foo(String fromStart, String toEnd) {}\n\
             }\n",
        );
        assert_eq!(methods.len(), 3);
        for (method, _) in &methods {
            if method.parameter_names.len() == 2 {
                assert!(m.matches(Some(method)));
            } else {
                assert!(!m.matches(Some(method)));
            }
        }
    }

    #[test]
    fn test_constructor_match() {
        let m = manager(&["*Foo"]);
        let methods = primary_type_methods(
            "src/Foo.java",
            "public class Foo {\n\
             \tpublic Foo(int foo) {}\n\
             \tpublic Foo(int foo, int bar) {}\n\
             }\n",
        );
        assert_eq!(methods.len(), 2);
        for (method, is_constructor) in &methods {
            if *is_constructor {
                assert!(m.matches(Some(method)));
            }
        }
    }

    #[test]
    fn test_constructor_match2() {
        let m = manager(&["java.foo.Foo.*"]);
        let path = "src/java/foo/Foo.java";
        let source = "public class Foo {\n\
             \tpublic Foo(int foo) {}\n\
             \tpublic Foo(int foo, int bar) {}\n\
             \tpublic bar() {\n\
             \t\tnew Foo(1);\n\
             \t\tnew Foo(1, 2);\n\
             \t}\n\
             }\n";
        // `new Foo(1)` / `new Foo(1, 2)` resolve to the two constructors of
        // the primary type, in package fragment `java.foo`.
        let constructors: Vec<MethodInfo> = primary_type_methods(path, source)
            .into_iter()
            .filter(|(m, c)| *c && m.name == "Foo")
            .map(|(m, _)| m)
            .collect();
        assert_eq!(constructors.len(), 2);
        for method in &constructors {
            assert_eq!(method.package, "java.foo");
            assert!(m.matches(Some(method)));
        }
    }

    #[test]
    fn pattern_parsing() {
        let p = create_method_pattern("*.Arrays.asList").unwrap();
        assert_eq!(p.selector, Some("asList".chars().collect()));
        assert_eq!(p.declaring_qualification, None);
        assert_eq!(p.declaring_simple_name, Some("Arrays".chars().collect()));
        assert_eq!(p.parameter_simple_names, None);
        let p = create_method_pattern("java.util.List<String>.add(java.lang.String, *)").unwrap();
        assert_eq!(p.declaring_qualification, Some("java.util".chars().collect()));
        assert_eq!(p.declaring_simple_name, Some("List".chars().collect()));
        assert_eq!(p.parameter_simple_names, Some(vec![Some("String".chars().collect()), None]));
        assert!(create_method_pattern("foo(a, b").is_none());
        assert!(create_method_pattern("").is_none());
        assert!(char_operation_match(&"*print*".chars().collect::<Vec<_>>(), &"sprint".chars().collect::<Vec<_>>()));
        assert!(!char_operation_match(&"from*".chars().collect::<Vec<_>>(), &"to".chars().collect::<Vec<_>>()));
    }
}
