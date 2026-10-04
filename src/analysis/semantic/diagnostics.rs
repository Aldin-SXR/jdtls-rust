//! Rust port of jdt.ls `BaseDiagnosticsHandler.toDiagnosticsArray`: turns
//! the bridge's raw `IProblem` data into LSP diagnostics.

use super::protocol::BridgeDiagnostic;
use crate::semantic_ast::problem as p;
use serde_json::json;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Url};

/// `JavaLanguageServerPlugin.SERVER_SOURCE_ID`.
pub const SERVER_SOURCE_ID: &str = "Java";

/// `BaseDiagnosticsHandler.DIAG_ARGUMENTS`.
pub const DIAG_ARGUMENTS: &str = "arguments";

/// A raw problem (`IProblem`).
#[derive(Clone, Debug)]
pub struct RawProblem {
    pub id: i32,
    pub source_start: i32,
    pub source_end: i32,
    pub line: i32,
    pub is_error: bool,
    pub is_warning: bool,
    pub message: String,
    pub arguments: Vec<String>,
}

impl RawProblem {
    pub fn from_bridge(d: &BridgeDiagnostic) -> Option<Self> {
        Some(RawProblem {
            id: d.problem_id?,
            source_start: d.source_start.unwrap_or(-1),
            source_end: d.source_end.unwrap_or(-1),
            line: d.source_line.unwrap_or(d.start_line as i32 + 1),
            is_error: d.severity == 1,
            is_warning: d.severity == 2,
            message: d.message.clone(),
            arguments: d.arguments.clone().unwrap_or_default(),
        })
    }

    pub fn from_ast(pr: &crate::semantic_ast::AstProblem) -> Self {
        RawProblem {
            id: pr.id,
            source_start: pr.source_start,
            source_end: pr.source_end,
            line: pr.line,
            is_error: pr.is_error,
            is_warning: pr.is_warning,
            message: pr.message.clone(),
            arguments: pr.arguments.clone(),
        }
    }
}

/// UTF-16 view of a document for jface `Document` offset ↔ line conversions.
pub struct Doc16 {
    text: Vec<u16>,
    line_starts: Vec<usize>,
}

impl Doc16 {
    pub fn new(text: &str) -> Self {
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut line_starts = vec![0];
        let mut i = 0;
        while i < text.len() {
            if text[i] == b'\r' as u16 {
                if text.get(i + 1) == Some(&(b'\n' as u16)) {
                    i += 1;
                }
                line_starts.push(i + 1);
            } else if text[i] == b'\n' as u16 {
                line_starts.push(i + 1);
            }
            i += 1;
        }
        Doc16 { text, line_starts }
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn char_at(&self, offset: usize) -> Option<u16> {
        self.text.get(offset).copied()
    }

    /// `JsonRpcHelpers.toLine(document, offset)`.
    pub fn to_line(&self, offset: i64) -> Option<(u32, u32)> {
        if offset < 0 || offset as usize > self.text.len() {
            return None;
        }
        let offset = offset as usize;
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        Some((line as u32, (offset - self.line_starts[line]) as u32))
    }

    /// `JDTUtils.toRange(openable, offset, length)`.
    pub fn to_range(&self, offset: i64, length: i64) -> Range {
        let mut range = Range::default();
        if offset > 0 || length > 0 {
            let (sl, sc) = self.to_line(offset).unwrap_or((0, 0));
            let (el, ec) = self.to_line(offset + length).unwrap_or((0, 0));
            range.start = Position { line: sl, character: sc };
            range.end = Position { line: el, character: ec };
        }
        range
    }

    /// `JsonRpcHelpers.toOffset(document, line, column)` (-1 when invalid).
    pub fn to_offset(&self, line: u32, column: u32) -> i64 {
        match self.line_starts.get(line as usize) {
            Some(&s) => s as i64 + column as i64,
            None => -1,
        }
    }
}

/// `BaseDiagnosticsHandler.toDiagnosticsArray(openable, problems, isDiagnosticTagSupported)`.
pub fn to_diagnostics_array(doc: &Doc16, problems: &[RawProblem], tag_support: bool) -> Vec<Diagnostic> {
    problems.iter().map(|pr| to_diagnostic(doc, pr, tag_support)).collect()
}

pub fn to_diagnostic(doc: &Doc16, pr: &RawProblem, tag_support: bool) -> Diagnostic {
    let mut data = serde_json::Map::new();
    if matches!(
        pr.id,
        p::UndefinedName
            | p::UndefinedType
            | p::UninitializedBlankFinalField
            | p::DuplicateInheritedDefaultMethods
            | p::FeatureNotSupported
            | p::MultiConstantCaseLabelsNotSupported
            | p::InvalidUsageOfTypeAnnotations
            | p::InheritedDefaultMethodConflictsWithOtherInherited
    ) {
        data.insert(DIAG_ARGUMENTS.to_owned(), json!(pr.arguments));
    }
    Diagnostic {
        range: convert_range(doc, pr),
        severity: Some(convert_severity(pr)),
        code: Some(NumberOrString::String(pr.id.to_string())),
        code_description: None,
        source: Some(SERVER_SOURCE_ID.to_owned()),
        message: pr.message.clone(),
        related_information: None,
        tags: if tag_support { diagnostic_tag(pr.id) } else { None },
        data: if data.is_empty() { None } else { Some(serde_json::Value::Object(data)) },
    }
}

/// `BaseDiagnosticsHandler.getDiagnosticTag(id)`.
pub fn diagnostic_tag(id: i32) -> Option<Vec<DiagnosticTag>> {
    match id {
        p::UsingDeprecatedType
        | p::UsingDeprecatedField
        | p::UsingDeprecatedMethod
        | p::UsingDeprecatedConstructor
        | p::OverridingDeprecatedMethod
        | p::JavadocUsingDeprecatedField
        | p::JavadocUsingDeprecatedConstructor
        | p::JavadocUsingDeprecatedMethod
        | p::JavadocUsingDeprecatedType
        | p::UsingTerminallyDeprecatedType
        | p::UsingTerminallyDeprecatedMethod
        | p::UsingTerminallyDeprecatedConstructor
        | p::UsingTerminallyDeprecatedField
        | p::OverridingTerminallyDeprecatedMethod
        | p::UsingDeprecatedSinceVersionType
        | p::UsingDeprecatedSinceVersionMethod
        | p::UsingDeprecatedSinceVersionConstructor
        | p::UsingDeprecatedSinceVersionField
        | p::OverridingDeprecatedSinceVersionMethod
        | p::UsingTerminallyDeprecatedSinceVersionType
        | p::UsingTerminallyDeprecatedSinceVersionMethod
        | p::UsingTerminallyDeprecatedSinceVersionConstructor
        | p::UsingTerminallyDeprecatedSinceVersionField
        | p::OverridingTerminallyDeprecatedSinceVersionMethod
        | p::UsingDeprecatedPackage
        | p::UsingDeprecatedSinceVersionPackage
        | p::UsingTerminallyDeprecatedPackage
        | p::UsingTerminallyDeprecatedSinceVersionPackage
        | p::UsingDeprecatedModule
        | p::UsingDeprecatedSinceVersionModule
        | p::UsingTerminallyDeprecatedModule
        | p::UsingTerminallyDeprecatedSinceVersionModule => Some(vec![DiagnosticTag::DEPRECATED]),
        p::UnnecessaryCast
        | p::UnnecessaryInstanceof
        | p::UnnecessaryElse
        | p::UnnecessaryNLSTag
        | p::UnusedPrivateType
        | p::UnusedPrivateField
        | p::UnusedPrivateMethod
        | p::UnusedPrivateConstructor
        | p::UnusedObjectAllocation
        | p::UnusedMethodDeclaredThrownException
        | p::UnusedConstructorDeclaredThrownException
        | p::UnusedLabel
        | p::UnusedImport
        | p::UnusedTypeArgumentsForMethodInvocation
        | p::UnusedWarningToken
        | p::UnusedTypeArgumentsForConstructorInvocation
        | p::UnusedTypeParameter
        | p::LocalVariableIsNeverUsed
        | p::ArgumentIsNeverUsed
        | p::ExceptionParameterIsNeverUsed => Some(vec![DiagnosticTag::UNNECESSARY]),
        _ => None,
    }
}

/// `BaseDiagnosticsHandler.convertSeverity`.
fn convert_severity(pr: &RawProblem) -> DiagnosticSeverity {
    if pr.is_error {
        DiagnosticSeverity::ERROR
    } else if pr.is_warning && pr.id != p::Task {
        DiagnosticSeverity::WARNING
    } else {
        DiagnosticSeverity::INFORMATION
    }
}

/// `BaseDiagnosticsHandler.convertRange`.
fn convert_range(doc: &Doc16, pr: &RawProblem) -> Range {
    let start = pr.source_start as i64;
    let end = pr.source_end as i64;
    if pr.id == p::UndefinedType {
        if let Some(s) = annotation_source_start(doc, pr) {
            return doc.to_range(s, end - s + 1);
        }
    }
    doc.to_range(start, end - start + 1)
}

/// `BaseDiagnosticsHandler.getSourceStart`: the `@` of an annotation whose
/// type is undefined.
fn annotation_source_start(doc: &Doc16, pr: &RawProblem) -> Option<i64> {
    let mut start = pr.source_start as i64;
    if start <= 0 {
        return None;
    }
    start -= 1;
    let mut ch = doc.char_at(start as usize)?;
    while char::from_u32(ch as u32).is_some_and(char::is_whitespace) {
        start -= 1;
        if start < 0 {
            return None;
        }
        ch = doc.char_at(start as usize)?;
    }
    (ch == b'@' as u16).then_some(start)
}

/// Converts a bridge `compile` diagnostic.  Diagnostics without raw problem
/// data keep their precomputed range.
pub fn to_lsp(d: &BridgeDiagnostic, doc: Option<&Doc16>, tag_support: bool) -> Option<(Url, Diagnostic)> {
    let uri = Url::parse(&d.uri).ok()?;
    if let (Some(pr), Some(doc)) = (RawProblem::from_bridge(d), doc) {
        return Some((uri, to_diagnostic(doc, &pr, tag_support)));
    }
    let severity = match d.severity {
        1 => DiagnosticSeverity::ERROR,
        2 => DiagnosticSeverity::WARNING,
        3 => DiagnosticSeverity::INFORMATION,
        4 => DiagnosticSeverity::HINT,
        _ => DiagnosticSeverity::ERROR,
    };
    let diag = Diagnostic {
        range: Range {
            start: Position { line: d.start_line, character: d.start_char },
            end: Position { line: d.end_line, character: d.end_char },
        },
        severity: Some(severity),
        code: d.code.as_ref().map(|c| NumberOrString::String(c.clone())),
        source: Some(SERVER_SOURCE_ID.to_owned()),
        message: d.message.clone(),
        ..Default::default()
    };
    Some((uri, diag))
}
