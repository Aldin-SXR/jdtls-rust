//! Rust port of the jdt.ls code action pipeline (`CodeActionHandler`,
//! `CodeActionResolveHandler`) and of the quick fix / quick assist /
//! refactor / source processors over the semantic AST and the Rust
//! `ASTRewrite`.
//!
//! The building blocks every processor uses:
//!
//! * [`Context`] — `IInvocationContext`: the unit's [`Ast`], the selection
//!   and its covering / covered node.
//! * [`ProblemLocation`] — `IProblemLocation` built from the request
//!   diagnostics (`CodeActionHandler.getProblemLocationCores`).
//! * [`Proposal`] — `ChangeCorrectionProposalCore` wrapped with its code
//!   action kind (`ProposalKindWrapper`): label, kind, relevance and a
//!   [`Change`] (an `ASTRewrite` + `ImportRewrite` per unit, raw text edits,
//!   or a lazily computed change) or a command.
//! * [`edit`] — converts a change into the jdt.ls `WorkspaceEdit` shape
//!   (`ChangeUtil.convertToWorkspaceEdit` + `TextEditConverter`).
//!
//! Processors live in their own modules (`quick_fix`, `local_corrections`,
//! `modifier_corrections`, ...), ported 1:1 from jdt.ls / jdt.core.manipulation.

pub mod edit;
pub mod handler;
pub mod messages;
pub mod relevance;
pub mod return_type;

pub mod assign_to_field;
pub mod getter_setter;
pub mod javadoc_tags;
pub mod local_corrections;
pub mod null_annotations;
pub mod modifier_corrections;
pub mod quick_assist;
pub mod refactor_edit;
pub mod quick_fix;
pub mod parentheses;
pub mod serial_hash;
pub mod serial_version;
pub mod source_assist;
pub mod infer_type_arguments;
pub mod type_mismatch;
pub mod unimplemented;
pub mod unresolved_elements;

use std::sync::Arc;

use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::text_edit::EditTree;
use crate::rewrite::ASTRewrite;
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::{Ast, Node};

/// `CodeActionKind` / `JavaCodeActionKind` constants.
pub mod kind {
    pub const QUICK_FIX: &str = "quickfix";
    pub const REFACTOR: &str = "refactor";
    pub const REFACTOR_EXTRACT: &str = "refactor.extract";
    pub const REFACTOR_INLINE: &str = "refactor.inline";
    pub const REFACTOR_REWRITE: &str = "refactor.rewrite";
    pub const SOURCE: &str = "source";
    pub const SOURCE_ORGANIZE_IMPORTS: &str = "source.organizeImports";
    pub const QUICK_ASSIST: &str = "quickassist";
    pub const SOURCE_GENERATE_ACCESSORS: &str = "source.generate.accessors";
    pub const SOURCE_GENERATE_HASHCODE_EQUALS: &str = "source.generate.hashCodeEquals";
    pub const SOURCE_GENERATE_TO_STRING: &str = "source.generate.toString";
    pub const SOURCE_GENERATE_CONSTRUCTORS: &str = "source.generate.constructors";
    pub const SOURCE_GENERATE_DELEGATE_METHODS: &str = "source.generate.delegateMethods";
    pub const SOURCE_OVERRIDE_METHODS: &str = "source.overrideMethods";
    pub const SOURCE_GENERATE_FINAL_MODIFIERS: &str = "source.generate.finalModifiers";
    pub const SOURCE_SORT_MEMBERS: &str = "source.sortMembers";
    pub const REFACTOR_EXTRACT_FUNCTION: &str = "refactor.extract.function";
    pub const REFACTOR_EXTRACT_CONSTANT: &str = "refactor.extract.constant";
    pub const REFACTOR_EXTRACT_VARIABLE: &str = "refactor.extract.variable";
    pub const REFACTOR_EXTRACT_FIELD: &str = "refactor.extract.field";
    pub const REFACTOR_ASSIGN_VARIABLE: &str = "refactor.assign.variable";
    pub const REFACTOR_ASSIGN_FIELD: &str = "refactor.assign.field";
    pub const REFACTOR_INTRODUCE_PARAMETER: &str = "refactor.introduce.parameter";
    pub const REFACTOR_MOVE: &str = "refactor.move";
}

/// `IInvocationContext` (`InnovationContext`).
pub struct Context {
    pub ast: Arc<Ast>,
    pub selection_offset: usize,
    pub selection_length: usize,
}

impl Context {
    pub fn new(ast: Arc<Ast>, selection_offset: usize, selection_length: usize) -> Self {
        Context { ast, selection_offset, selection_length }
    }

    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    pub fn root(&self) -> Node<'_> {
        self.ast.root()
    }

    fn finder(&self) -> NodeFinder<'_> {
        NodeFinder::new(self.ast.root(), self.selection_offset, self.selection_length)
    }

    /// `getCoveringNode()`.
    pub fn covering_node(&self) -> Option<Node<'_>> {
        self.finder().covering
    }

    /// `getCoveredNode()`.
    pub fn covered_node(&self) -> Option<Node<'_>> {
        self.finder().covered
    }
}

/// `IProblemLocation` (`ProblemLocation`).
#[derive(Clone, Debug)]
pub struct ProblemLocation {
    pub offset: usize,
    pub length: usize,
    pub problem_id: i32,
    pub arguments: Vec<String>,
    pub is_error: bool,
}

impl ProblemLocation {
    /// `getCoveringNode(astRoot)`.
    pub fn covering_node<'a>(&self, ast: &'a Ast) -> Option<Node<'a>> {
        NodeFinder::new(ast.root(), self.offset, self.length).covering
    }

    /// `getCoveredNode(astRoot)`.
    pub fn covered_node<'a>(&self, ast: &'a Ast) -> Option<Node<'a>> {
        NodeFinder::new(ast.root(), self.offset, self.length).covered
    }
}

/// The edits of one compilation unit: `ASTRewrite.rewriteAST()` plus
/// `ImportRewrite.rewriteImports()` plus extra raw edits, combined under one
/// root (`CompilationUnitChange`).
pub struct CuChange {
    pub ast: Arc<Ast>,
    pub rewrite: Option<ASTRewrite>,
    pub imports: Option<ImportRewrite>,
    pub edits: Option<EditTree>,
}

impl CuChange {
    pub fn rewrite(rewrite: ASTRewrite) -> Self {
        CuChange { ast: rewrite.ast.clone(), rewrite: Some(rewrite), imports: None, edits: None }
    }

    pub fn with_imports(mut self, imports: ImportRewrite) -> Self {
        self.imports = Some(imports);
        self
    }

    pub fn edits(ast: Arc<Ast>, edits: EditTree) -> Self {
        CuChange { ast, rewrite: None, imports: None, edits: Some(edits) }
    }
}

/// Computes a change on demand (proposals whose edit needs more bridge data,
/// e.g. the generated serial version id).
#[tower_lsp::async_trait]
pub trait LazyChange: Send + Sync {
    /// SourceAssistProcessor converts text edits to WorkspaceEdit.changes.
    fn changes_only(&self) -> bool { false }
    async fn compute(&self, env: &edit::Env<'_>) -> anyhow::Result<Vec<CuChange>>;
}

pub enum Change {
    Cu(Vec<CuChange>),
    Lazy(Box<dyn LazyChange>),
    /// A ready workspace edit (external files, settings, ...).
    WorkspaceEdit(tower_lsp::lsp_types::WorkspaceEdit),
    None,
}

/// Proposal flavours that change how `CodeActionHandler` presents them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalType {
    /// `ChangeCorrectionProposalCore` and subclasses.
    Change,
    /// `NewCUProposal`, `NewMethodCorrectionProposalCore`,
    /// `NewAnnotationMemberProposalCore`, `NewVariableCorrectionProposalCore`
    /// (get a `java.project.refreshDiagnostics` command when resolving lazily).
    NewElement,
    /// `ChangeToRequiredCompilerCompliance` (kept without edits).
    ChangeCompliance,
    /// `AddImportCorrectionProposalCore` (an import-only quick fix; it enables
    /// `QuickFixProcessor.addAddAllMissingImportsProposal`).
    AddImport,
}

/// A correction proposal with its code action kind (`ProposalKindWrapper`).
pub struct Proposal {
    pub name: String,
    pub kind: String,
    pub relevance: i32,
    pub change: Change,
    /// `CUCorrectionCommandProposal` / `RefactoringCorrectionCommandProposal`
    /// / `AssignToVariableAssistCommandProposal`: `(command id, arguments)`.
    pub command: Option<(String, Vec<serde_json::Value>)>,
    pub proposal_type: ProposalType,
}

impl Proposal {
    pub fn new(name: impl Into<String>, kind: &str, relevance: i32, change: Change) -> Self {
        Proposal { name: name.into(), kind: kind.to_owned(), relevance, change, command: None, proposal_type: ProposalType::Change }
    }

    pub fn rewrite(name: impl Into<String>, kind: &str, relevance: i32, rewrite: ASTRewrite) -> Self {
        Self::new(name, kind, relevance, Change::Cu(vec![CuChange::rewrite(rewrite)]))
    }

    pub fn command(name: impl Into<String>, kind: &str, relevance: i32, command: &str, args: Vec<serde_json::Value>) -> Self {
        Proposal {
            name: name.into(),
            kind: kind.to_owned(),
            relevance,
            change: Change::None,
            command: Some((command.to_owned(), args)),
            proposal_type: ProposalType::Change,
        }
    }
}

/// `String.compareToIgnoreCase`.
pub fn compare_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    let fold = |c: u16| -> u16 {
        let ch = char::from_u32(c as u32).unwrap_or('\0');
        let up: Vec<char> = ch.to_uppercase().collect();
        let up = if up.len() == 1 { up[0] } else { ch };
        let low: Vec<char> = up.to_lowercase().collect();
        let low = if low.len() == 1 { low[0] } else { up };
        if (low as u32) < 0x10000 {
            low as u32 as u16
        } else {
            c
        }
    };
    let av: Vec<u16> = a.encode_utf16().collect();
    let bv: Vec<u16> = b.encode_utf16().collect();
    for (x, y) in av.iter().zip(bv.iter()) {
        let (x, y) = (fold(*x), fold(*y));
        if x != y {
            return x.cmp(&y);
        }
    }
    av.len().cmp(&bv.len())
}

/// `String.compareTo` (UTF-16 code unit order).
pub fn compare_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `CodeActionHandler.ChangeCorrectionProposalComparator`.
pub fn compare_proposals(p1: &Proposal, p2: &Proposal) -> std::cmp::Ordering {
    let (k1, k2) = (&p1.kind, &p2.kind);
    if !k1.trim().is_empty() && !k2.trim().is_empty() && k1 != k2 {
        return compare_utf16(k1, k2);
    }
    let r = p2.relevance - p1.relevance;
    if r != 0 {
        return r.cmp(&0);
    }
    compare_ignore_case(&p1.name, &p2.name)
}

/// `java.util.List.sort` (TimSort) for the small lists code actions use:
/// binary insertion sort after `countRunAndMakeAscending`, exactly as
/// `TimSort.sort` does for fewer than 32 elements (comparators here are not
/// always consistent, so the algorithm matters).
pub fn java_sort<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> std::cmp::Ordering) {
    use std::cmp::Ordering::*;
    let n = v.len();
    if n < 2 {
        return;
    }
    if n >= 32 {
        // Stable merge sort for larger lists.
        v.sort_by(cmp);
        return;
    }
    // countRunAndMakeAscending
    let mut run_hi = 1;
    if cmp(&v[run_hi], &v[0]) == Less {
        run_hi += 1;
        while run_hi < n && cmp(&v[run_hi], &v[run_hi - 1]) == Less {
            run_hi += 1;
        }
        v[..run_hi].reverse();
    } else {
        run_hi += 1;
        while run_hi < n && cmp(&v[run_hi], &v[run_hi - 1]) != Less {
            run_hi += 1;
        }
    }
    // binarySort(a, lo, hi, start = runHi)
    for start in run_hi..n {
        let mut left = 0;
        let mut right = start;
        while left < right {
            let mid = (left + right) >> 1;
            if cmp(&v[start], &v[mid]) == Less {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        v[left..=start].rotate_right(1);
    }
}
