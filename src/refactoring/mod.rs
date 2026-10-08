//! Rust ports of the jdt.core.manipulation / jdt.ls refactorings behind the
//! `refactor.extract.*` code actions and `java/getRefactorEdit`
//! (`ExtractTempRefactoring`, `ExtractConstantRefactoring`,
//! `ExtractFieldRefactoring`, `ExtractMethodRefactoring`) over the semantic
//! AST and the Rust `ASTRewrite`.
//!
//! Shared infrastructure ported alongside them:
//!
//! * [`Status`] — `RefactoringStatus`.
//! * [`Visitor`] / [`accept`] — `ASTVisitor` / `GenericVisitor` traversal.
//! * [`selection`] — `Selection` and `SelectionAnalyzer`.
//! * [`fragments`] — `ASTFragmentFactory` and the expression fragments.
//! * [`scope`] — `ScopeAnalyzer` (variables in scope, used names).
//! * [`naming`] — `StubUtility.getVariableNameSuggestions` /
//!   `NamingConventions.suggestVariableNames`.
//! * [`checks`] — `Checks`, `ConstantChecks`, `ASTNodes` helpers.
//! * [`checkers`] — `SideEffectChecker`, `UnsafeCheckTester`,
//!   `ChangedValueChecker`.

pub mod checkers;
pub mod checks;
pub mod convert_to_record;
pub mod extract_constant;
pub mod extract_field;
pub mod extract_method;
pub mod extract_method_analyzer;
pub mod snippet_finder;
pub mod flow;
pub mod extract_temp;
pub mod check_source;
pub mod convert_for_loop;
pub mod inline_constant;
pub mod inline_temp;
pub mod fragments;
pub mod naming;
pub mod scope;
pub mod selection;

use crate::semantic_ast::{Node, NodeKind};

// ─── RefactoringStatus ────────────────────────────────────────────────────────

/// `RefactoringStatus` severities.
pub mod severity {
    pub const OK: i32 = 0;
    pub const INFO: i32 = 1;
    pub const WARNING: i32 = 2;
    pub const ERROR: i32 = 3;
    pub const FATAL: i32 = 4;
}

/// `RefactoringStatusCodes` used by the ported refactorings.
pub mod status_code {
    pub const NONE: i32 = 0;
    pub const EXPRESSION_NOT_RVALUE: i32 = 64;
    pub const EXPRESSION_NOT_RVALUE_VOID: i32 = 65;
    pub const EXPRESSION_MAY_CAUSE_SIDE_EFFECTS: i32 = 66;
}

#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub severity: i32,
    pub message: String,
    pub code: i32,
}

/// `org.eclipse.ltk.core.refactoring.RefactoringStatus`.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub entries: Vec<StatusEntry>,
    severity: i32,
}

impl Status {
    pub fn ok() -> Self {
        Status::default()
    }

    pub fn with(severity: i32, message: impl Into<String>, code: i32) -> Self {
        let mut s = Status::default();
        s.add_entry(severity, message, code);
        s
    }

    /// `createFatalErrorStatus(msg)`.
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::with(severity::FATAL, message, status_code::NONE)
    }

    /// `createErrorStatus(msg)`.
    pub fn error(message: impl Into<String>) -> Self {
        Self::with(severity::ERROR, message, status_code::NONE)
    }

    /// `createWarningStatus(msg)`.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::with(severity::WARNING, message, status_code::NONE)
    }

    pub fn add_entry(&mut self, severity: i32, message: impl Into<String>, code: i32) {
        self.entries.push(StatusEntry { severity, message: message.into(), code });
        self.severity = self.severity.max(severity);
    }

    pub fn add_fatal(&mut self, message: impl Into<String>) {
        self.add_entry(severity::FATAL, message, status_code::NONE);
    }

    pub fn add_error(&mut self, message: impl Into<String>) {
        self.add_entry(severity::ERROR, message, status_code::NONE);
    }

    pub fn add_warning(&mut self, message: impl Into<String>) {
        self.add_entry(severity::WARNING, message, status_code::NONE);
    }

    pub fn add_info(&mut self, message: impl Into<String>) {
        self.add_entry(severity::INFO, message, status_code::NONE);
    }

    /// `merge(other)` (`null` is a no-op).
    pub fn merge(&mut self, other: impl Into<Option<Status>>) {
        if let Some(other) = other.into() {
            for e in other.entries {
                self.add_entry(e.severity, e.message, e.code);
            }
        }
    }

    pub fn severity(&self) -> i32 {
        self.severity
    }

    pub fn is_ok(&self) -> bool {
        self.severity == severity::OK
    }

    pub fn has_info(&self) -> bool {
        self.severity >= severity::INFO
    }

    pub fn has_warning(&self) -> bool {
        self.severity >= severity::WARNING
    }

    pub fn has_error(&self) -> bool {
        self.severity >= severity::ERROR
    }

    pub fn has_fatal_error(&self) -> bool {
        self.severity == severity::FATAL
    }

    /// `getEntryWithHighestSeverity()`.
    pub fn entry_with_highest_severity(&self) -> Option<&StatusEntry> {
        let mut best: Option<&StatusEntry> = None;
        for e in &self.entries {
            if best.is_none_or(|b| e.severity > b.severity) {
                best = Some(e);
            }
        }
        best
    }
}

/// `RefactoringCoreMessages` text.
pub fn msg(key: &str) -> &'static str {
    crate::correction::messages::refactoring(key)
}

// ─── Visitors ─────────────────────────────────────────────────────────────────

/// `ASTVisitor`: `pre_visit` is `preVisit2` (a `false` skips the node
/// entirely), `visit` decides whether the children are visited and
/// `end_visit` runs afterwards.
pub trait Visitor<'a> {
    fn pre_visit(&mut self, _n: Node<'a>) -> bool {
        true
    }
    fn visit(&mut self, _n: Node<'a>) -> bool {
        true
    }
    fn end_visit(&mut self, _n: Node<'a>) {}
}

/// `ASTNode.accept(visitor)`.
pub fn accept<'a, V: Visitor<'a> + ?Sized>(n: Node<'a>, v: &mut V) {
    if !v.pre_visit(n) {
        return;
    }
    if v.visit(n) {
        for c in n.children() {
            accept(c, v);
        }
    }
    v.end_visit(n);
}

/// Preorder traversal where `f` decides whether to descend
/// (`ASTVisitor.preVisit2` only).
pub fn walk<'a>(n: Node<'a>, f: &mut dyn FnMut(Node<'a>) -> bool) {
    if f(n) {
        for c in n.children() {
            walk(c, f);
        }
    }
}

/// `new ASTVisitor()` does not visit Javadoc tags (`visit(Javadoc)` is
/// `false`).
pub fn is_doc(n: Node<'_>) -> bool {
    n.is(NodeKind::Javadoc)
}

/// `ASTNodes.getParent(node, kind)` including `node` itself
/// (`ASTNodes.getFirstAncestorOrNull` excludes the node).
pub fn ancestor_or_self<'a>(n: Node<'a>, pred: impl Fn(Node<'a>) -> bool) -> Option<Node<'a>> {
    std::iter::once(n).chain(n.ancestors()).find(|x| pred(*x))
}
