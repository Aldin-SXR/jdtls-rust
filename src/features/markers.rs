//! Resource markers (`org.eclipse.core.resources.IMarker`) and their
//! conversion to diagnostics, a port of jdt.ls
//! `WorkspaceDiagnosticsHandler.toDiagnosticsArray(IDocument, IMarker[], ..)`.
//!
//! Saved (not open) files are published from the markers of the last build,
//! which jdt.ls converts differently from the problems of a reconciled
//! working copy: the range is on the marker's `lineNumber` line (columns
//! are `charStart`/`charEnd` minus that line's offset), and no `data` is
//! attached.

use std::collections::HashMap;

use serde_json::Value;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::analysis::semantic::diagnostics::{diagnostic_tag, Doc16, RawProblem, SERVER_SOURCE_ID};

/// `IMarker` attribute names.
pub mod attr {
    pub const MESSAGE: &str = "message";
    pub const SEVERITY: &str = "severity";
    pub const LINE_NUMBER: &str = "lineNumber";
    pub const CHAR_START: &str = "charStart";
    pub const CHAR_END: &str = "charEnd";
    /// `IJavaModelMarker.ID`.
    pub const ID: &str = "id";
    /// `IMavenConstants.MARKER_COLUMN_START`.
    pub const MAVEN_COLUMN_START: &str = "columnStart";
    /// `IMavenConstants.MARKER_COLUMN_END`.
    pub const MAVEN_COLUMN_END: &str = "columnEnd";
    /// `GradleProjectImporter.GRADLE_MARKER_COLUMN_START`.
    pub const GRADLE_COLUMN_START: &str = "gradleColumnStart";
    /// `GradleProjectImporter.GRADLE_MARKER_COLUMN_END`.
    pub const GRADLE_COLUMN_END: &str = "gradleColumnEnd";
    /// `BaseDiagnosticsHandler.DIAG_JAVAC_CODE`.
    pub const JAVAC_CODE: &str = "javacCode";
}

/// `IMarker.SEVERITY_*`.
pub mod severity {
    pub const INFO: i64 = 0;
    pub const WARNING: i64 = 1;
    pub const ERROR: i64 = 2;
}

/// Marker types and their declared supertypes (`plugin.xml`
/// `org.eclipse.core.resources.markers` extensions).
pub mod types {
    pub const MARKER: &str = "org.eclipse.core.resources.marker";
    pub const PROBLEM: &str = "org.eclipse.core.resources.problemmarker";
    pub const TASK: &str = "org.eclipse.core.resources.taskmarker";
    pub const TEXT: &str = "org.eclipse.core.resources.textmarker";
    /// `IJavaModelMarker.JAVA_MODEL_PROBLEM_MARKER`.
    pub const JAVA_MODEL_PROBLEM: &str = "org.eclipse.jdt.core.problem";
    pub const NO_EXPLICIT_ENCODING: &str = "org.eclipse.core.resources.noExplicitEncoding";
    /// `IJavaModelMarker.TASK_MARKER`.
    pub const JAVA_TASK: &str = "org.eclipse.jdt.core.task";
    /// `IJavaModelMarker.BUILDPATH_PROBLEM_MARKER`.
    pub const BUILDPATH_PROBLEM: &str = "org.eclipse.jdt.core.buildpath_problem";
    /// `IMavenConstants.MARKER_ID`.
    pub const MAVEN: &str = "org.eclipse.m2e.core.maven2Problem";
    /// `IMavenConstants.MARKER_CONFIGURATION_ID`.
    pub const MAVEN_CONFIGURATION: &str = "org.eclipse.m2e.core.maven2Problem.configuration";
    /// `GradleProjectImporter.GRADLE_UPGRADE_WRAPPER_MARKER_ID`.
    pub const GRADLE_UPGRADE_WRAPPER: &str = "org.eclipse.jdt.ls.gradle.upgradeWrapper";

    /// The declared supertypes of `marker_type`.
    pub fn supertypes(marker_type: &str) -> &'static [&'static str] {
        match marker_type {
            PROBLEM | TASK | TEXT => &[MARKER],
            NO_EXPLICIT_ENCODING => &[PROBLEM],
            JAVA_MODEL_PROBLEM => &[PROBLEM, TEXT],
            JAVA_TASK => &[TASK, TEXT],
            BUILDPATH_PROBLEM => &[PROBLEM],
            MAVEN => &[PROBLEM],
            t if t.starts_with("org.eclipse.m2e.core.maven2Problem.") => &[MAVEN],
            GRADLE_UPGRADE_WRAPPER => &[PROBLEM],
            _ => &[MARKER],
        }
    }
}

/// An `IMarker`: a type and its attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub marker_type: String,
    pub attributes: HashMap<String, Value>,
    /// `IMarker.exists()`.
    pub exists: bool,
}

impl Marker {
    pub fn new(marker_type: impl Into<String>) -> Self {
        Marker { marker_type: marker_type.into(), attributes: HashMap::new(), exists: true }
    }

    pub fn with(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.attributes.insert(name.to_owned(), value.into());
        self
    }

    /// `getAttribute(name, int defaultValue)`.
    pub fn get_int(&self, name: &str, default: i64) -> i64 {
        self.attributes.get(name).and_then(Value::as_i64).unwrap_or(default)
    }

    /// `getAttribute(name, String defaultValue)`.
    pub fn get_string(&self, name: &str, default: &str) -> String {
        self.attributes.get(name).and_then(Value::as_str).unwrap_or(default).to_owned()
    }

    /// `getAttribute(name)`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    /// `isSubtypeOf(superType)`.
    pub fn is_subtype_of(&self, super_type: &str) -> bool {
        fn walk(t: &str, target: &str) -> bool {
            t == target || types::supertypes(t).iter().any(|s| *s != t && walk(s, target))
        }
        walk(&self.marker_type, super_type)
    }

    /// The marker the Java builder creates for a compiler problem
    /// (`AbstractImageBuilder.storeProblemsFor`): a task marker for
    /// `IProblem.Task`, a Java model problem marker otherwise.
    pub fn from_problem(problem: &RawProblem) -> Self {
        const TASK: i32 = 536871362; // IProblem.Task
        let is_task = problem.id == TASK;
        let mut m = Marker::new(if is_task { types::JAVA_TASK } else { types::JAVA_MODEL_PROBLEM })
            .with(attr::MESSAGE, problem.message.clone())
            .with(attr::ID, problem.id)
            .with(attr::CHAR_START, problem.source_start)
            .with(attr::CHAR_END, problem.source_end + 1)
            .with(attr::LINE_NUMBER, problem.line);
        if !is_task {
            let severity = if problem.is_error {
                severity::ERROR
            } else if problem.is_warning {
                severity::WARNING
            } else {
                severity::INFO
            };
            m = m.with(attr::SEVERITY, severity);
        }
        m
    }
}

/// The `IDocument` calls the marker conversion makes.
pub trait Document {
    /// `getLineOffset(line)`; `None` is a `BadLocationException`.
    fn get_line_offset(&self, line: i64) -> Option<i64>;
    /// `getChar(offset)`; `None` is a `BadLocationException`.
    fn get_char(&self, offset: i64) -> Option<u16>;
    /// `JsonRpcHelpers.toLine(document, offset)`.
    fn to_line(&self, offset: i64) -> Option<(u32, u32)>;
}

impl Document for Doc16 {
    fn get_line_offset(&self, line: i64) -> Option<i64> {
        if line < 0 {
            return None;
        }
        // jface `Document.getLineOffset(numberOfLines)` is the document length.
        match self.to_offset(line as u32, 0) {
            -1 => None,
            o => Some(o),
        }
    }

    fn get_char(&self, offset: i64) -> Option<u16> {
        if offset < 0 {
            return None;
        }
        self.char_at(offset as usize)
    }

    fn to_line(&self, offset: i64) -> Option<(u32, u32)> {
        Doc16::to_line(self, offset)
    }
}

/// `WorkspaceDiagnosticsHandler.isInteresting(marker)`: not of a type the
/// client excluded (`ClientPreferences.excludedMarkerTypes`).
fn is_interesting(marker: &Marker) -> bool {
    crate::features::preferences::excluded_marker_types().iter().all(|t| !marker.is_subtype_of(t))
}

/// `WorkspaceDiagnosticsHandler.toDiagnosticsArray(document, markers, isDiagnosticTagSupported)`;
/// `None` entries are `null` markers.
pub fn to_diagnostics_array(document: &dyn Document, markers: &[Option<&Marker>], tag_support: bool) -> Vec<Diagnostic> {
    markers
        .iter()
        .filter(|m| m.map_or(true, is_interesting))
        .filter_map(|m| to_diagnostic(document, (*m)?, tag_support))
        .collect()
}

fn is_ignored(marker: &Marker, ignore_project_encoding: bool) -> bool {
    !marker.exists || (ignore_project_encoding && marker.marker_type == types::NO_EXPLICIT_ENCODING)
}

pub fn to_diagnostic_array(range: Range, markers: &[&Marker], tag_support: bool, ignore_project_encoding: bool) -> Vec<Diagnostic> {
    markers
        .iter()
        .filter(|m| is_interesting(m))
        .filter(|m| !is_ignored(m, ignore_project_encoding))
        .map(|m| {
            let mut d = to_diagnostic(&FixedRange(range), m, tag_support).expect("existing marker");
            d.range = range;
            d
        })
        .collect()
}

struct FixedRange(Range);

impl Document for FixedRange {
    fn get_line_offset(&self, _line: i64) -> Option<i64> {
        Some(0)
    }
    fn get_char(&self, _offset: i64) -> Option<u16> {
        None
    }
    fn to_line(&self, _offset: i64) -> Option<(u32, u32)> {
        None
    }
}

fn to_diagnostic(document: &dyn Document, marker: &Marker, tag_support: bool) -> Option<Diagnostic> {
    if !marker.exists {
        return None;
    }
    let problem_id = marker.get_int(attr::ID, 0);
    let mut d = Diagnostic {
        source: Some(SERVER_SOURCE_ID.to_owned()),
        message: marker.get_string(attr::MESSAGE, ""),
        code: Some(NumberOrString::String(problem_id.to_string())),
        severity: Some(convert_severity(marker.get_int(attr::SEVERITY, -1))),
        range: convert_range(document, marker),
        ..Default::default()
    };
    if tag_support {
        d.tags = diagnostic_tag(problem_id as i32);
    }
    if let Some(Value::String(javac_code)) = marker.get(attr::JAVAC_CODE) {
        d.code = Some(NumberOrString::String(javac_code.clone()));
        d.data = Some(serde_json::json!({ "ecjProblemId": problem_id.to_string() }));
    }
    Some(d)
}

/// `WorkspaceDiagnosticsHandler.convertSeverity`.
pub fn convert_severity(severity: i64) -> DiagnosticSeverity {
    match severity {
        severity::ERROR => DiagnosticSeverity::ERROR,
        severity::WARNING => DiagnosticSeverity::WARNING,
        _ => DiagnosticSeverity::INFORMATION,
    }
}

fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range { start: Position { line: sl, character: sc }, end: Position { line: el, character: ec } }
}

/// `WorkspaceDiagnosticsHandler.convertRange(document, marker)`.
fn convert_range(document: &dyn Document, marker: &Marker) -> Range {
    let line = marker.get_int(attr::LINE_NUMBER, -1) - 1;
    if line < 0 {
        let end = marker.get_int(attr::CHAR_END, -1);
        let start = marker.get_int(attr::CHAR_START, -1);
        if start >= 0 && end >= start {
            if let Some(r) = get_annotation_range(document, marker) {
                return r;
            }
            if let (Some((sl, sc)), Some((el, ec))) = (document.to_line(start), document.to_line(end)) {
                return range(sl, sc, el, ec);
            }
        }
        return range(0, 0, 0, 0);
    }
    let line_u = line as u32;
    let (c_start, c_end);
    if marker.is_subtype_of(types::MAVEN) {
        c_start = marker.get_int(attr::MAVEN_COLUMN_START, -1);
        c_end = marker.get_int(attr::MAVEN_COLUMN_END, -1);
    } else if marker.is_subtype_of(types::GRADLE_UPGRADE_WRAPPER) {
        c_start = marker.get_int(attr::GRADLE_COLUMN_START, -1);
        c_end = marker.get_int(attr::GRADLE_COLUMN_END, -1);
    } else {
        if marker.get_int(attr::ID, -1) == UNDEFINED_TYPE {
            if let Some(r) = get_annotation_range(document, marker) {
                return r;
            }
        }
        let Some(line_offset) = document.get_line_offset(line) else {
            return range(line_u, 0, line_u, 0);
        };
        c_end = marker.get_int(attr::CHAR_END, -1) - line_offset;
        c_start = marker.get_int(attr::CHAR_START, -1) - line_offset;
    }
    range(line_u, c_start.max(0) as u32, line_u, c_end.max(0) as u32)
}

/// `IProblem.UndefinedType`.
const UNDEFINED_TYPE: i64 = 16777218;

/// `WorkspaceDiagnosticsHandler.getAnnotationRange`: an unresolved
/// annotation type is reported from its `@`.
fn get_annotation_range(document: &dyn Document, marker: &Marker) -> Option<Range> {
    if marker.get_int(attr::ID, -1) != UNDEFINED_TYPE {
        return None;
    }
    let end = marker.get_int(attr::CHAR_END, -1);
    let mut start = marker.get_int(attr::CHAR_START, -1);
    if start <= 0 {
        return None;
    }
    start -= 1;
    // A BadLocationException is logged and the default range used.
    let mut ch = document.get_char(start)?;
    while char::from_u32(ch as u32).is_some_and(is_java_whitespace) {
        start -= 1;
        ch = document.get_char(start)?;
    }
    if ch != b'@' as u16 {
        return None;
    }
    // `JDTUtils.toRange(document, start, end - start)`.
    let length = end - start;
    if start > 0 || length > 0 {
        let (sl, sc) = document.to_line(start).unwrap_or((0, 0));
        let (el, ec) = document.to_line(start + length).unwrap_or((0, 0));
        return Some(range(sl, sc, el, ec));
    }
    Some(Range::default())
}

/// `Character.isWhitespace`.
fn is_java_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}')
        || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
}

#[cfg(test)]
mod workspace_diagnostics_handler_test {
    //! The marker conversion cases of
    //! `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceDiagnosticsHandlerTest`
    //! (the LSP cases are in `tests/handlers_workspace_diagnostics_handler_test.rs`).
    //! The Mockito `IDocument` answers 0 for unstubbed `getLineOffset` and
    //! `getChar` calls.

    use super::*;

    #[derive(Default)]
    struct MockDocument {
        line_offsets: HashMap<i64, i64>,
    }

    impl Document for MockDocument {
        fn get_line_offset(&self, line: i64) -> Option<i64> {
            Some(self.line_offsets.get(&line).copied().unwrap_or(0))
        }
        fn get_char(&self, _offset: i64) -> Option<u16> {
            Some(0)
        }
        fn to_line(&self, _offset: i64) -> Option<(u32, u32)> {
            None
        }
    }

    fn create_marker(severity: i64, msg: &str, line: i64, start: i64, end: i64) -> Marker {
        // A mock of no particular type: `isSubtypeOf` answers false.
        Marker::new("mock")
            .with(attr::SEVERITY, severity)
            .with(attr::MESSAGE, msg)
            .with(attr::LINE_NUMBER, line)
            .with(attr::CHAR_START, start)
            .with(attr::CHAR_END, end)
    }

    fn create_undefined_type_marker(severity: i64, msg: &str, line: i64, start: i64, end: i64) -> Marker {
        create_marker(severity, msg, line, start, end).with(attr::ID, UNDEFINED_TYPE)
    }

    fn create_maven_marker(severity: i64, msg: &str, line: i64, start: i64, end: i64) -> Marker {
        let mut m = create_marker(severity, msg, line, start, end)
            .with(attr::MAVEN_COLUMN_START, start)
            .with(attr::MAVEN_COLUMN_END, end);
        m.marker_type = types::MAVEN.to_owned();
        m
    }

    #[test]
    fn test_to_diagnostics_array() {
        let msg1 = "Something's wrong Jim";
        let m1 = create_marker(severity::WARNING, msg1, 2, 95, 100);

        let msg2 = "He's dead";
        let m2 = create_marker(severity::ERROR, msg2, 10, 1015, 1025);

        let msg3 = "It's probably time to panic";
        let m3 = create_marker(42, msg3, 100, 10000, 10005);

        let msg4 = "ArrayList cannot be resolved to a type";
        let m4 = create_undefined_type_marker(severity::ERROR, msg4, 14, 215, 228);

        let mut d = MockDocument::default();
        d.line_offsets.insert(1, 90);
        d.line_offsets.insert(9, 1000);
        d.line_offsets.insert(14, 210);
        d.line_offsets.insert(99, 10000);

        let diags = to_diagnostics_array(&d, &[Some(&m1), Some(&m2), Some(&m3), Some(&m4)], true);
        assert_eq!(4, diags.len());

        let d1 = &diags[0];
        assert_eq!(msg1, d1.message);
        assert_eq!(Some(DiagnosticSeverity::WARNING), d1.severity);
        let r = d1.range;
        assert_eq!(1, r.start.line);
        assert_eq!(5, r.start.character);
        assert_eq!(1, r.end.line);
        assert_eq!(10, r.end.character);

        let d2 = &diags[1];
        assert_eq!(msg2, d2.message);
        assert_eq!(Some(DiagnosticSeverity::ERROR), d2.severity);
        let r = d2.range;
        assert_eq!(9, r.start.line);
        assert_eq!(15, r.start.character);
        assert_eq!(9, r.end.line);
        assert_eq!(25, r.end.character);

        let d3 = &diags[2];
        assert_eq!(msg3, d3.message);
        assert_eq!(Some(DiagnosticSeverity::INFORMATION), d3.severity);
        let r = d3.range;
        assert_eq!(99, r.start.line);
        assert_eq!(0, r.start.character);
        assert_eq!(99, r.end.line);
        assert_eq!(5, r.end.character);

        let d4 = &diags[3];
        assert_eq!(msg4, d4.message);
        assert_eq!(Some(DiagnosticSeverity::ERROR), d4.severity);
        let r = d4.range;
        assert_eq!(13, r.start.line);
        assert_eq!(215, r.start.character);
        assert_eq!(13, r.end.line);
        assert_eq!(228, r.end.character);
    }

    #[test]
    fn test_maven_markers() {
        let msg1 = "Some dependency is missing";
        let m1 = create_maven_marker(severity::ERROR, msg1, 2, 95, 100);

        let d = MockDocument::default();

        let diags = to_diagnostics_array(&d, &[Some(&m1), None], true);
        assert_eq!(1, diags.len());

        let d1 = &diags[0];
        assert_eq!(msg1, d1.message);
        assert_eq!(Some(DiagnosticSeverity::ERROR), d1.severity);
        let r = d1.range;
        assert_eq!(1, r.start.line);
        assert_eq!(95, r.start.character);
        assert_eq!(1, r.end.line);
        assert_eq!(100, r.end.character);
    }

    #[test]
    fn test_encoding() {
        let range = Range::default();
        let marker = Marker::new(types::NO_EXPLICIT_ENCODING)
            .with(attr::MESSAGE, "Project 'hello' has no explicit encoding set")
            .with(attr::SEVERITY, severity::WARNING);
        let mut preferences = crate::features::preferences::model::Preferences::default();

        preferences.set_project_encoding(crate::features::preferences::model::ProjectEncodingMode::Ignore);
        let ignore = preferences.get_project_encoding() == crate::features::preferences::model::ProjectEncodingMode::Ignore;
        assert_eq!(0, to_diagnostic_array(range, &[&marker], false, ignore).len());

        preferences.set_project_encoding(crate::features::preferences::model::ProjectEncodingMode::Warning);
        let ignore = preferences.get_project_encoding() == crate::features::preferences::model::ProjectEncodingMode::Ignore;
        assert_eq!(1, to_diagnostic_array(range, &[&marker], false, ignore).len());
    }
}
