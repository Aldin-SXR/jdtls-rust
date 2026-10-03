//! `textDocument/inlayHint`: port of jdt.ls `InlayHintsHandler` and
//! `InlayHintVisitor`.
//!
//! The bridge (`InlayHintService.java`) reports, in AST pre-order, the nodes
//! the jdt.ls visitor visits together with their bindings.  Everything else —
//! range checks, parameter-name modes, exclusion filters, the numbered
//! parameter filter, lambda parameter types, `var` types and format specifier
//! hints — is computed here.

use crate::analysis::dispatcher::{Dispatcher, InlayHintData};
use crate::analysis::semantic::protocol::{BridgeInlayExpr, BridgeInlayMethod, BridgeInlayNode};
use crate::analysis::semantic::BridgeResponse;
use crate::features::inlay_hint_filter::{InlayHintFilterManager, MethodInfo};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use tower_lsp::lsp_types::{InlayHint, InlayHintLabel, InlayHintParams, Position, Range};
use tracing::warn;

// ─── Preferences ──────────────────────────────────────────────────────────────

pub const JAVA_INLAYHINTS_PARAMETERNAMES_ENABLED: &str = "java.inlayHints.parameterNames.enabled";
pub const JAVA_INLAYHINTS_PARAMETERNAMES_SUPPRESS_WHEN_SAME_NAME_NUMBERED: &str =
    "java.inlayHints.parameterNames.suppressWhenSameNameNumbered";
pub const JAVA_INLAYHINTS_PARAMETERNAMES_EXCLUSIONS: &str = "java.inlayHints.parameterNames.exclusions";
pub const JAVA_INLAYHINTS_VARIABLETYPES_ENABLED: &str = "java.inlayHints.variableTypes.enabled";
pub const JAVA_INLAYHINTS_PARAMETERTYPES_ENABLED: &str = "java.inlayHints.parameterTypes.enabled";
pub const JAVA_INLAYHINTS_FORMATPARAMETERS_ENABLED: &str = "java.inlayHints.formatParameters.enabled";

/// jdt.ls `InlayHintsParameterMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InlayHintsParameterMode {
    /// Do not show parameter name hints.
    None,
    /// Only for literal arguments.
    #[default]
    Literals,
    /// For all arguments.
    All,
}

impl InlayHintsParameterMode {
    /// `InlayHintsParameterMode.fromString(value, default)`.
    pub fn from_str_or(value: Option<&str>, default: Self) -> Self {
        match value.map(str::to_uppercase).as_deref() {
            Some("NONE") => Self::None,
            Some("LITERALS") => Self::Literals,
            Some("ALL") => Self::All,
            _ => default,
        }
    }
}

/// The inlay-hint part of jdt.ls `Preferences` (same defaults).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlayHintPreferences {
    pub parameter_mode: InlayHintsParameterMode,
    pub suppress_when_same_name_numbered: bool,
    pub exclusions: Option<Vec<String>>,
    pub variable_types_enabled: bool,
    pub parameter_types_enabled: bool,
    pub format_parameters_enabled: bool,
}

impl InlayHintPreferences {
    /// `Preferences.updateFrom(existing, configuration)` for the inlay-hint
    /// keys: keys absent from `settings` keep their current value.
    pub fn update_from(&mut self, settings: &Value) {
        if contains_key(settings, JAVA_INLAYHINTS_PARAMETERNAMES_ENABLED) {
            let mode = get_string(settings, JAVA_INLAYHINTS_PARAMETERNAMES_ENABLED);
            self.parameter_mode = InlayHintsParameterMode::from_str_or(mode, self.parameter_mode);
        }
        if contains_key(settings, JAVA_INLAYHINTS_PARAMETERNAMES_SUPPRESS_WHEN_SAME_NAME_NUMBERED) {
            self.suppress_when_same_name_numbered = get_boolean(
                settings,
                JAVA_INLAYHINTS_PARAMETERNAMES_SUPPRESS_WHEN_SAME_NAME_NUMBERED,
                self.suppress_when_same_name_numbered,
            );
        }
        if contains_key(settings, JAVA_INLAYHINTS_PARAMETERNAMES_EXCLUSIONS) {
            self.exclusions = get_list(settings, JAVA_INLAYHINTS_PARAMETERNAMES_EXCLUSIONS, self.exclusions.clone());
        }
        if contains_key(settings, JAVA_INLAYHINTS_VARIABLETYPES_ENABLED) {
            self.variable_types_enabled =
                get_boolean(settings, JAVA_INLAYHINTS_VARIABLETYPES_ENABLED, self.variable_types_enabled);
        }
        if contains_key(settings, JAVA_INLAYHINTS_PARAMETERTYPES_ENABLED) {
            self.parameter_types_enabled =
                get_boolean(settings, JAVA_INLAYHINTS_PARAMETERTYPES_ENABLED, self.parameter_types_enabled);
        }
        if contains_key(settings, JAVA_INLAYHINTS_FORMATPARAMETERS_ENABLED) {
            self.format_parameters_enabled =
                get_boolean(settings, JAVA_INLAYHINTS_FORMATPARAMETERS_ENABLED, self.format_parameters_enabled);
        }
    }

    /// `InlayHintsPreferenceChangeListener`: whether a change from `self` to
    /// `new` asks the client to refresh its inlay hints.
    pub fn needs_refresh(&self, new: &Self) -> bool {
        self != new
    }
}

// `MapFlattener`: chained keys (`java.inlayHints.x`) over nested maps, or a
// flat top-level key.

fn get_value<'a>(configuration: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = configuration.get(key).filter(|v| !v.is_null()) {
        return Some(v);
    }
    let mut current = configuration;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        let v = current.get(*part);
        if i == parts.len() - 1 {
            return v.filter(|v| !v.is_null());
        }
        match v {
            Some(m) if m.is_object() => current = m,
            _ => return None,
        }
    }
    None
}

fn contains_key(configuration: &Value, key: &str) -> bool {
    if configuration.get(key).is_some() {
        return true;
    }
    let mut current = configuration;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            return current.get(*part).is_some();
        }
        match current.get(*part) {
            Some(m) if m.is_object() => current = m,
            _ => return false,
        }
    }
    false
}

fn get_string<'a>(configuration: &'a Value, key: &str) -> Option<&'a str> {
    get_value(configuration, key).and_then(Value::as_str)
}

fn get_boolean(configuration: &Value, key: &str, default: bool) -> bool {
    match get_value(configuration, key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => default,
    }
}

fn get_list(configuration: &Value, key: &str, default: Option<Vec<String>>) -> Option<Vec<String>> {
    match get_value(configuration, key) {
        Some(Value::String(s)) => {
            let mut s = s.clone();
            if !s.trim().starts_with('[') {
                if s.contains(',') {
                    s = format!("[{s}]");
                } else {
                    return Some(s.split(' ').filter(|e| !e.is_empty()).map(str::to_owned).collect());
                }
            }
            match serde_json::from_str::<Vec<String>>(&s) {
                Ok(list) => Some(list),
                // Gson parses leniently: unquoted elements are accepted.
                Err(_) => lenient_string_list(&s).or(default),
            }
        }
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
                .collect(),
        ),
        _ => default,
    }
}

/// Gson's lenient reading of a `[a, "b", c]` string list.
fn lenient_string_list(s: &str) -> Option<Vec<String>> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    inner
        .split(',')
        .map(|e| {
            let e = e.trim();
            if e.is_empty() {
                return None;
            }
            let unquoted = e
                .strip_prefix('"')
                .and_then(|e| e.strip_suffix('"'))
                .or_else(|| e.strip_prefix('\'').and_then(|e| e.strip_suffix('\'')))
                .unwrap_or(e);
            Some(unquoted.to_owned())
        })
        .collect()
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// `InlayHintsHandler.inlayHint`.
pub async fn inlay_hint(dispatcher: &Dispatcher, prefs: &InlayHintPreferences, params: &InlayHintParams) -> Vec<InlayHint> {
    if prefs.parameter_mode == InlayHintsParameterMode::None
        && !prefs.variable_types_enabled
        && !prefs.format_parameters_enabled
    {
        return Vec::new();
    }
    if !dispatcher.is_ecj_ready().await {
        return Vec::new();
    }
    let uri = &params.text_document.uri;
    match dispatcher.inlay_hint_data(uri, prefs.format_parameters_enabled).await {
        Ok(InlayHintData { response: BridgeResponse::InlayHints { nodes, .. }, source, folder_package }) => {
            compute(&nodes, &source, params.range, prefs, folder_package.as_deref())
        }
        Ok(InlayHintData { response: BridgeResponse::Error { message, .. }, .. }) => {
            warn!("inlay hints ECJ error: {message}");
            Vec::new()
        }
        Ok(_) => Vec::new(),
        Err(e) => {
            warn!("inlay hints error: {e}");
            Vec::new()
        }
    }
}

/// Run the `InlayHintVisitor` over the bridge's nodes of `source`.
pub fn compute(
    nodes: &[BridgeInlayNode],
    source: &str,
    range: Range,
    prefs: &InlayHintPreferences,
    folder_package: Option<&str>,
) -> Vec<InlayHint> {
    let doc = Document::new(source);
    let mut visitor = InlayHintVisitor {
        doc: &doc,
        start_offset: doc.to_offset(range.start.line, range.start.character),
        end_offset: doc.to_offset(range.end.line, range.end.character),
        prefs,
        filters: InlayHintFilterManager::from_exclusions(prefs.exclusions.as_deref()),
        folder_package,
        hints: Vec::new(),
    };
    for node in nodes {
        visitor.visit(node);
    }
    visitor.hints
}

/// An Eclipse `IDocument` over the source (UTF-16 offsets, `\n`, `\r\n` and
/// `\r` line delimiters), as used by jdt.ls `JsonRpcHelpers`.
struct Document {
    len: usize,
    /// Offset of every line start, including the position after a trailing
    /// delimiter.
    starts: Vec<usize>,
    /// `ListLineTracker.fLines.size()`.
    tracked_lines: usize,
}

impl Document {
    fn new(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut starts = vec![0];
        let mut i = 0;
        while i < units.len() {
            match units[i] {
                0x0D if units.get(i + 1) == Some(&0x0A) => {
                    i += 2;
                    starts.push(i);
                }
                0x0D | 0x0A => {
                    i += 1;
                    starts.push(i);
                }
                _ => i += 1,
            }
        }
        let delimiters = starts.len() - 1;
        let trailing = *starts.last().unwrap() < units.len();
        Self { len: units.len(), starts, tracked_lines: delimiters + usize::from(trailing) }
    }

    /// `JsonRpcHelpers.toOffset(document, line, column)`; -1 on a bad line.
    fn to_offset(&self, line: u32, column: u32) -> i64 {
        let line = line as usize;
        let lines = self.tracked_lines;
        let line_offset = if line > lines {
            return -1;
        } else if lines == 0 {
            0
        } else if line == lines {
            self.len
        } else {
            self.starts[line]
        };
        line_offset as i64 + i64::from(column)
    }

    /// `JsonRpcHelpers.toLine(document, offset)`.
    fn to_position(&self, offset: usize) -> Option<Position> {
        if offset > self.len {
            return None;
        }
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        Some(Position { line: line as u32, character: (offset - self.starts[line]) as u32 })
    }
}

/// Classes whose `format` / `printf` methods use `java.util.Formatter` syntax.
const FORMAT_CLASSES: &[&str] =
    &["java.lang.String", "java.io.PrintStream", "java.io.PrintWriter", "java.util.Formatter", "java.io.Console"];

/// Java format specifiers (`java.util.Formatter` syntax).  Group 1: explicit
/// argument index (`2$`), group 2: conversion.
static FORMAT_SPECIFIER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"%([0-9]+\$)?[-#+ 0,(]*[0-9]*(?:\.[0-9]+)?([bBhHsScCdoxXeEfgGaAtTnN%])").unwrap()
});

/// First parameter of a numbered sequence: prefix (no digits) followed by `1`.
static FIRST_PARAM_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^([^0-9]+)1$").unwrap());

struct InlayHintVisitor<'a> {
    doc: &'a Document,
    start_offset: i64,
    end_offset: i64,
    prefs: &'a InlayHintPreferences,
    filters: InlayHintFilterManager,
    folder_package: Option<&'a str>,
    hints: Vec<InlayHint>,
}

fn hint(position: Position, label: String) -> InlayHint {
    InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: None,
        data: None,
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl InlayHintVisitor<'_> {
    fn visit(&mut self, node: &BridgeInlayNode) {
        let args: &[BridgeInlayExpr] = node.arguments.as_deref().unwrap_or_default();
        match node.kind.as_str() {
            "EnumConstantDeclaration"
            | "ClassInstanceCreation"
            | "SuperMethodInvocation"
            | "ConstructorInvocation"
            | "SuperConstructorInvocation" => {
                if self.is_out_of_range(node) {
                    return;
                }
                self.resolve_parameter_inlay_hints(node.method.as_ref(), args);
            }
            "MethodInvocation" => {
                if self.is_out_of_range(node) {
                    return;
                }
                self.resolve_parameter_inlay_hints(node.method.as_ref(), args);
                if self.prefs.format_parameters_enabled {
                    self.resolve_format_parameter_inlay_hints(node);
                }
            }
            "LambdaExpression" => self.visit_lambda(node),
            "VariableDeclarationStatement" => self.visit_variable_declaration_statement(node),
            _ => {}
        }
    }

    /// Whether the node is outside the requested range.
    fn is_out_of_range(&self, node: &BridgeInlayNode) -> bool {
        let start = node.start as i64;
        start > self.end_offset || start + (node.length as i64) < self.start_offset
    }

    fn add(&mut self, offset: usize, label: String, padding_left: Option<bool>, padding_right: Option<bool>) {
        if let Some(position) = self.doc.to_position(offset) {
            let mut h = hint(position, label);
            h.padding_left = padding_left;
            h.padding_right = padding_right;
            self.hints.push(h);
        }
    }

    fn visit_lambda(&mut self, node: &BridgeInlayNode) {
        if self.is_out_of_range(node) || !self.prefs.parameter_types_enabled {
            return;
        }
        let Some(types) = &node.lambda_parameter_types else { return };
        let parameters = node.lambda_parameters.as_deref().unwrap_or_default();
        if parameters.len() != types.len() {
            return;
        }
        for (param, type_name) in parameters.iter().zip(types) {
            // Explicitly typed parameters show their type already, and mixing
            // implicit and explicit parameter types is forbidden.
            if param.node == "SingleVariableDeclaration" {
                return;
            }
            if param.name_start >= 0 {
                self.add(param.name_start as usize, type_name.clone(), None, Some(true));
            }
        }
    }

    fn visit_variable_declaration_statement(&mut self, node: &BridgeInlayNode) {
        if !self.prefs.variable_types_enabled || self.is_out_of_range(node) || !node.is_var {
            return;
        }
        for fragment in node.fragments.as_deref().unwrap_or_default() {
            if !fragment.resolved {
                continue;
            }
            // Skip direct assignment of primitives, String, new object, array, or any cast.
            if is_uninteresting_expression(fragment.initializer.as_deref()) {
                continue;
            }
            let Some(inferred) = fragment.type_name.as_deref().filter(|t| !t.is_empty()) else { continue };
            self.add(fragment.name_start + fragment.name_length, format!(": {inferred}"), Some(true), None);
        }
    }

    fn resolve_parameter_inlay_hints(&mut self, method: Option<&BridgeInlayMethod>, arguments: &[BridgeInlayExpr]) {
        let Some(method) = method else { return };
        if self.prefs.parameter_mode == InlayHintsParameterMode::None || arguments.is_empty() {
            return;
        }
        let Some(parameter_names) = parameter_names(method) else { return };
        // Not showing hints while arguments are incomplete, to avoid flickering.
        if !method.varargs && arguments.len() != parameter_names.len() {
            return;
        }
        if self.filters.matches(self.java_element(method, parameter_names).as_ref()) {
            return;
        }
        if self.prefs.suppress_when_same_name_numbered && has_numbered_parameter_pattern(parameter_names) {
            return;
        }
        let count = parameter_names.len().min(arguments.len());
        for i in 0..count {
            let arg = &arguments[i];
            if !self.accept_argument(arg, &parameter_names[i]) {
                continue;
            }
            let mut label = format!("{}:", parameter_names[i]);
            if i == parameter_names.len() - 1 && method.varargs {
                label = format!("...{label}");
            }
            self.add(arg.start, label, None, Some(true));
        }
    }

    /// `(IMethod) methodBinding.getJavaElement()`, `None` for methods without
    /// a Java element (synthetic ones such as implicit record constructors).
    fn java_element(&self, method: &BridgeInlayMethod, parameter_names: &[String]) -> Option<MethodInfo> {
        if method.synthetic {
            return None;
        }
        let declared = method.declaring_package.clone().unwrap_or_default();
        let package = match (method.in_target_unit, self.folder_package) {
            (true, Some(p)) => p.to_owned(),
            _ => declared,
        };
        Some(MethodInfo {
            name: method.name.clone(),
            package,
            type_qualified_name: method.declaring_type_qualified_name.clone().unwrap_or_default(),
            parameter_names: parameter_names.to_vec(),
        })
    }

    /// Hidden: non-literals in literal mode, lambdas, and arguments named
    /// like the parameter.
    fn accept_argument(&self, argument: &BridgeInlayExpr, param_name: &str) -> bool {
        if self.prefs.parameter_mode == InlayHintsParameterMode::Literals && !is_literal(argument) {
            return false;
        }
        if argument.node == "LambdaExpression" {
            return false;
        }
        !is_same_name(argument, param_name)
    }

    fn resolve_format_parameter_inlay_hints(&mut self, node: &BridgeInlayNode) {
        let Some(method) = &node.method else { return };
        let Some(declaring) = method.declaring_type.as_deref().filter(|d| FORMAT_CLASSES.contains(d)) else {
            return;
        };
        let all_args: &[BridgeInlayExpr] = node.arguments.as_deref().unwrap_or_default();
        let (format_expr, format_string, format_args) = if method.name == "formatted" && declaring == "java.lang.String" {
            let Some(expr) = &node.expression else { return };
            let Some(format_string) = extract_string_value(expr) else { return };
            (expr, format_string, all_args)
        } else if method.name == "format" || method.name == "printf" {
            if all_args.is_empty() {
                return;
            }
            let types = method.parameter_types.as_deref().unwrap_or_default();
            let Some(first) = types.first() else { return };
            let pattern_index = if first == "java.util.Locale" { 1 } else { 0 };
            if all_args.len() <= pattern_index {
                return;
            }
            let Some(format_string) = extract_string_value(&all_args[pattern_index]) else { return };
            (&all_args[pattern_index], format_string, &all_args[pattern_index + 1..])
        } else {
            return;
        };
        self.create_format_specifier_hints(format_expr, format_string, format_args);
    }

    fn create_format_specifier_hints(&mut self, format_expr: &BridgeInlayExpr, format_string: &str, format_args: &[BridgeInlayExpr]) {
        if format_args.is_empty() {
            return;
        }
        let Some(source) = format_expr.text.as_deref() else { return };
        let is_text_block = format_expr.node == "TextBlock";
        let mut implicit_arg_index = 0usize;
        let mut specifier_index_in_order = 0usize;
        for caps in FORMAT_SPECIFIER_PATTERN.captures_iter(format_string) {
            let conversion = &caps[2];
            // %% (literal percent) and %n (newline) don't consume arguments.
            if matches!(conversion, "%" | "n" | "N") {
                continue;
            }
            let arg_index: i64 = match caps.get(1) {
                // Explicit 1-based argument index like %2$s.
                Some(m) => match m.as_str().trim_end_matches('$').parse::<i32>() {
                    Ok(n) => i64::from(n) - 1,
                    Err(_) => return,
                },
                None => {
                    let i = implicit_arg_index as i64;
                    implicit_arg_index += 1;
                    i
                }
            };
            if arg_index < 0 || arg_index as usize >= format_args.len() {
                continue;
            }
            let arg_text = format_args[arg_index as usize].text.clone().unwrap_or_default();
            let end = utf16_len(&format_string[..caps.get(0).unwrap().end()]);
            let position = if is_text_block {
                // The literal value strips indentation, so find the n-th
                // argument-consuming specifier in the raw source content.
                let content_start = source.find('\n').map_or(0, |i| i + 1);
                let content = &source[content_start..];
                format_expr.start + utf16_len(&source[..content_start]) + nth_argument_consuming_specifier_end(content, specifier_index_in_order)
            } else {
                // Skip the opening quote.
                let units: Vec<u16> = source.encode_utf16().collect();
                format_expr.start + map_format_offset_to_source(units.get(1..).unwrap_or_default(), end) + 1
            };
            specifier_index_in_order += 1;
            self.add(position, format!(":{arg_text}"), Some(false), None);
        }
    }
}

/// The parameter names jdt.ls shows (`InlayHintVisitor.getParameterNames`):
/// only for methods whose declaring type has source.
fn parameter_names(method: &BridgeInlayMethod) -> Option<&[String]> {
    if !method.from_source {
        return None;
    }
    if method.synthetic && !method.record {
        return None;
    }
    method.parameter_names.as_deref()
}

fn is_uninteresting_expression(initializer: Option<&str>) -> bool {
    matches!(
        initializer,
        None | Some(
            "NumberLiteral"
                | "BooleanLiteral"
                | "CharacterLiteral"
                | "StringLiteral"
                | "ClassInstanceCreation"
                | "ArrayCreation"
                | "ArrayInitializer"
                | "CastExpression"
        )
    )
}

fn is_literal(argument: &BridgeInlayExpr) -> bool {
    matches!(
        argument.node.as_str(),
        "BooleanLiteral" | "CharacterLiteral" | "NullLiteral" | "NumberLiteral" | "StringLiteral" | "TypeLiteral"
    )
}

/// Whether the argument (or the casted expression of a cast) is a simple
/// name equal to the parameter name.
fn is_same_name(argument: &BridgeInlayExpr, param_name: &str) -> bool {
    let argument = if argument.node == "CastExpression" {
        match &argument.inner {
            Some(inner) => inner.as_ref(),
            None => return false,
        }
    } else {
        argument
    };
    argument.node == "SimpleName" && argument.identifier.as_deref() == Some(param_name)
}

/// All parameters follow `prefix1, prefix2, ...` (at least two of them).
fn has_numbered_parameter_pattern(parameter_names: &[String]) -> bool {
    if parameter_names.len() < 2 {
        return false;
    }
    let Some(caps) = FIRST_PARAM_PATTERN.captures(&parameter_names[0]) else { return false };
    let prefix = &caps[1];
    parameter_names.iter().enumerate().skip(1).all(|(i, name)| *name == format!("{prefix}{}", i + 1))
}

fn extract_string_value(expr: &BridgeInlayExpr) -> Option<&str> {
    match expr.node.as_str() {
        "StringLiteral" | "TextBlock" => expr.literal_value.as_deref(),
        _ => None,
    }
}

/// End (exclusive, UTF-16) of the n-th argument-consuming specifier in `content`, or 0.
fn nth_argument_consuming_specifier_end(content: &str, n: usize) -> usize {
    let mut count = 0;
    for caps in FORMAT_SPECIFIER_PATTERN.captures_iter(content) {
        if matches!(&caps[2], "%" | "n" | "N") {
            continue;
        }
        if count == n {
            return utf16_len(&content[..caps.get(0).unwrap().end()]);
        }
        count += 1;
    }
    0
}

/// Map an offset in the (unescaped) literal value to an offset in the
/// (escaped) source of a string literal, accounting for escape sequences.
fn map_format_offset_to_source(source: &[u16], target_offset: usize) -> usize {
    let is_octal = |c: u16| (u16::from(b'0')..=u16::from(b'7')).contains(&c);
    let mut source_idx = 0;
    let mut literal_idx = 0;
    while literal_idx < target_offset && source_idx < source.len() {
        let c = source[source_idx];
        if c == u16::from(b'\\') && source_idx + 1 < source.len() {
            let next = source[source_idx + 1];
            if next == u16::from(b'u') {
                // Unicode escape: \uXXXX
                source_idx += 6;
            } else if is_octal(next) {
                // Octal escape: \0 to \377
                source_idx += 2;
                if source_idx < source.len() && is_octal(source[source_idx]) {
                    source_idx += 1;
                    if next <= u16::from(b'3') && source_idx < source.len() && is_octal(source[source_idx]) {
                        source_idx += 1;
                    }
                }
            } else {
                // Simple escape: \n, \t, \\, \", ...
                source_idx += 2;
            }
        } else {
            source_idx += 1;
        }
        literal_idx += 1;
    }
    source_idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preferences_update_from_settings() {
        let mut p = InlayHintPreferences::default();
        assert_eq!(p.parameter_mode, InlayHintsParameterMode::Literals);
        p.update_from(&json!({ "java": { "inlayHints": {
            "parameterNames": { "enabled": "all", "exclusions": ["*.foo"], "suppressWhenSameNameNumbered": true },
            "variableTypes": { "enabled": true },
            "parameterTypes": { "enabled": "true" },
            "formatParameters": { "enabled": true }
        } } }));
        assert_eq!(p.parameter_mode, InlayHintsParameterMode::All);
        assert_eq!(p.exclusions, Some(vec!["*.foo".to_owned()]));
        assert!(p.suppress_when_same_name_numbered && p.variable_types_enabled);
        assert!(p.parameter_types_enabled && p.format_parameters_enabled);
        // Absent keys keep their values; invalid modes keep the current mode.
        p.update_from(&json!({ "java.inlayHints.parameterNames.enabled": "bogus" }));
        assert_eq!(p.parameter_mode, InlayHintsParameterMode::All);
        p.update_from(&json!({ "java.inlayHints.parameterNames.enabled": "NONE" }));
        assert_eq!(p.parameter_mode, InlayHintsParameterMode::None);
        assert!(p.variable_types_enabled);
        p.update_from(&json!({ "java": { "inlayHints": { "parameterNames": { "exclusions": "a, b" } } } }));
        assert_eq!(p.exclusions, Some(vec!["a".to_owned(), "b".to_owned()]));
    }

    #[test]
    fn document_offsets() {
        let d = Document::new("ab\ncd\n");
        assert_eq!(d.to_offset(0, 0), 0);
        assert_eq!(d.to_offset(1, 1), 4);
        assert_eq!(d.to_offset(2, 0), 6);
        assert_eq!(d.to_offset(3, 0), -1);
        assert_eq!(d.to_position(4), Some(Position { line: 1, character: 1 }));
        assert_eq!(d.to_position(6), Some(Position { line: 2, character: 0 }));
        let d = Document::new("ab\ncd");
        assert_eq!(d.to_offset(2, 0), 5);
        assert_eq!(d.to_offset(3, 0), -1);
    }

    #[test]
    fn format_offset_mapping() {
        let src: Vec<u16> = "Hello %s\\n\"".encode_utf16().collect();
        assert_eq!(map_format_offset_to_source(&src, 8), 8);
        let src: Vec<u16> = "a\\tb %s\"".encode_utf16().collect();
        assert_eq!(map_format_offset_to_source(&src, 6), 7);
    }
}
