//! Port of jdt.ls `QuickAssistProcessor` and `RefactorProcessor` (the parts
//! ported so far).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::edit::Env;
use super::handler::Request;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, Proposal};
use crate::refactoring::extract_constant::ExtractConstant;
use crate::refactoring::extract_temp::ExtractTemp;
use crate::semantic_ast::{Ast, NodeKind};

/// `QuickAssistProcessor.getAssists`.
pub async fn assists(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    super::assign_to_field::assign_param_to_field_proposals(env, &req.context, &mut proposals)
        .await;
    if !req.locations.iter().any(|p| {
        matches!(
            p.problem_id,
            crate::semantic_ast::problem::UnclosedCloseable
                | crate::semantic_ast::problem::PotentiallyUnclosedCloseable
                | crate::semantic_ast::problem::UnhandledException
        )
    }) {
        super::local_corrections::resource_assist(env, &req.context, &mut proposals).await;
    }
    if !req.locations.iter().any(|p| p.problem_id == crate::semantic_ast::problem::JavadocMissing) {
        if let Some(covering) = req.context.covering_node() {
            super::javadoc_tags::missing_javadoc_comment_proposals(env, &req.context, covering, kind::QUICK_ASSIST, &mut proposals).await;
        }
    }
    proposals
}

/// `RefactorProcessor.getProposals`.
pub async fn refactor_proposals(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    let Some(covering) = req.context.covering_node() else {
        return proposals;
    };
    if no_errors_at_location(req, covering) {
        let problems_at_location = !req.locations.is_empty();
        extract_variable_proposals(env, req, problems_at_location, &mut proposals).await;
    }
    proposals.extend(super::local_corrections::assignment_refactors(env, req).await);
    proposals
}

/// `RefactorProcessor.noErrorsAtLocation`.
fn no_errors_at_location(req: &Request<'_>, covering: crate::semantic_ast::Node<'_>) -> bool {
    let (start, end) = (covering.start(), covering.end());
    !req.locations.iter().any(|p| {
        !(p.offset > end || p.offset + p.length < start)
            && p.is_error
            && crate::semantic_ast::irritants::option_key_for_problem(p.problem_id).is_none()
    })
}

/// Which extract refactoring a proposal runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Extract {
    /// `ExtractTempRefactoring` replacing all occurrences.
    AllOccurrences,
    /// `ExtractTempRefactoring` replacing the selected expression only.
    Variable,
    /// `ExtractConstantRefactoring`.
    Constant,
}

impl Extract {
    fn label(self) -> &'static str {
        messages::ls_correction(match self {
            Extract::AllOccurrences => "QuickAssistProcessor_extract_to_local_all_description",
            Extract::Variable => "QuickAssistProcessor_extract_to_local_description",
            Extract::Constant => "QuickAssistProcessor_extract_to_constant_description",
        })
    }

    /// `RefactorProposalUtility.EXTRACT_*_COMMAND`.
    fn command(self) -> &'static str {
        match self {
            Extract::AllOccurrences => "extractVariableAllOccurrence",
            Extract::Variable => "extractVariable",
            Extract::Constant => "extractConstant",
        }
    }

    fn kind(self) -> &'static str {
        if self == Extract::Constant {
            kind::REFACTOR_EXTRACT_CONSTANT
        } else {
            kind::REFACTOR_EXTRACT_VARIABLE
        }
    }

    fn relevance(self, ctx: &Context, problems_at_location: bool) -> i32 {
        let zero = ctx.selection_length == 0;
        match self {
            Extract::AllOccurrences if zero => relevance::EXTRACT_LOCAL_ALL_ZERO_SELECTION,
            Extract::AllOccurrences if problems_at_location => relevance::EXTRACT_LOCAL_ALL_ERROR,
            Extract::AllOccurrences => relevance::EXTRACT_LOCAL_ALL,
            Extract::Variable if zero => relevance::EXTRACT_LOCAL_ZERO_SELECTION,
            Extract::Variable if problems_at_location => relevance::EXTRACT_LOCAL_ERROR,
            Extract::Variable => relevance::EXTRACT_LOCAL,
            Extract::Constant if zero => relevance::EXTRACT_CONSTANT_ZERO_SELECTION,
            Extract::Constant if problems_at_location => relevance::EXTRACT_CONSTANT_ERROR,
            Extract::Constant => relevance::EXTRACT_CONSTANT,
        }
    }
}

/// The refactoring behind an extract proposal (`RefactoringCorrectionProposalCore`).
enum ExtractRefactoring {
    Temp(ExtractTemp),
    Constant(ExtractConstant),
}

impl ExtractRefactoring {
    /// Creates the refactoring configured as `RefactorProposalUtility` does
    /// and reports whether `checkInitialConditions` is OK.
    fn create(what: Extract, ast: &Arc<Ast>, options: BTreeMap<String, String>, offset: usize, length: usize, set_final: bool) -> (Self, bool) {
        match what {
            Extract::Constant => {
                let mut r = ExtractConstant::new(ast.clone(), options, offset, length);
                let ok = r.check_initial_conditions().is_ok();
                (ExtractRefactoring::Constant(r), ok)
            }
            _ => {
                let mut r = ExtractTemp::new(ast.clone(), options, offset, length);
                if what == Extract::Variable {
                    r.set_replace_all_occurrences(false);
                }
                r.set_declare_final(set_final);
                let ok = r.check_initial_conditions().is_ok();
                if what == Extract::AllOccurrences {
                    r.set_replace_all_occurrences(true);
                }
                (ExtractRefactoring::Temp(r), ok)
            }
        }
    }

    /// `RefactoringCorrectionProposalCore.createTextChange` (a fatal final
    /// check yields an empty change).
    fn create_change(self) -> Vec<CuChange> {
        let (status, cu) = match self {
            ExtractRefactoring::Temp(mut r) => {
                let name = r.guess_temp_name();
                r.set_temp_name(&name);
                r.check_final_conditions()
            }
            ExtractRefactoring::Constant(mut r) => {
                let name = r.guess_constant_name();
                r.set_constant_name(&name);
                r.check_final_conditions()
            }
        };
        match cu {
            Some(cu) if !status.has_fatal_error() => vec![CuChange::rewrite(cu.rewrite).with_imports(cu.imports)],
            _ => Vec::new(),
        }
    }
}

struct ExtractChange {
    what: Extract,
    ast: Arc<Ast>,
    offset: usize,
    length: usize,
    set_final: bool,
}

#[tower_lsp::async_trait]
impl LazyChange for ExtractChange {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let (refactoring, _) = ExtractRefactoring::create(self.what, &self.ast, options, self.offset, self.length, self.set_final);
        Ok(refactoring.create_change())
    }
}

/// `RefactorProcessor.getExtractVariableProposal` /
/// `RefactorProposalUtility.getExtractVariableProposals`.
async fn extract_variable_proposals(env: &Env<'_>, req: &Request<'_>, problems_at_location: bool, out: &mut Vec<Proposal>) {
    let ctx = &req.context;
    if !supports_extract_variable(ctx) {
        return;
    }
    let return_as_command = crate::features::preferences::extended_capability("advancedExtractRefactoringSupport");
    let infer_selection = crate::features::preferences::extended_capability_list_contains("inferSelectionSupport", "extractVariable");
    let decls_to_final = crate::features::preferences::add_final_for_new_declaration();
    let declare_final = decls_to_final == "all" || decls_to_final == "variables";
    let options = env.options(&ctx.ast.uri).await;
    for what in [Extract::AllOccurrences, Extract::Variable, Extract::Constant] {
        let set_final = declare_final && what != Extract::Constant;
        let label = what.label();
        let relevance = what.relevance(ctx, problems_at_location);
        // `CUCorrectionCommandProposal`.
        let command = || {
            Proposal::command(
                label,
                what.kind(),
                relevance,
                "java.action.applyRefactoringCommand",
                vec![
                    serde_json::json!(what.command()),
                    serde_json::to_value(req.params).expect("serializable code action parameters"),
                ],
            )
        };
        if infer_selection && ctx.selection_length == 0 {
            let mut parent = ctx.covering_node();
            while let Some(p) = parent.filter(|p| p.kind().is_expression()) {
                if !p.is(NodeKind::ParenthesizedExpression) {
                    let (_, ok) = ExtractRefactoring::create(what, &ctx.ast, options.clone(), p.start(), p.length(), set_final);
                    if ok {
                        out.push(command());
                        break;
                    }
                }
                parent = p.parent();
            }
            continue;
        }
        let (offset, length) = (ctx.selection_offset, ctx.selection_length);
        let (_, ok) = ExtractRefactoring::create(what, &ctx.ast, options.clone(), offset, length, set_final);
        if !ok {
            continue;
        }
        if return_as_command {
            out.push(command());
        } else {
            let change = ExtractChange { what, ast: ctx.ast.clone(), offset, length, set_final };
            out.push(Proposal::new(label, what.kind(), relevance, Change::Lazy(Box::new(change))));
        }
    }
}

/// `RefactorProposalUtility.supportsExtractVariable`.
fn supports_extract_variable(ctx: &Context) -> bool {
    let mut node = ctx.covered_node();
    if !node.is_some_and(|n| n.kind().is_expression()) {
        if ctx.selection_length != 0 {
            return false;
        }
        node = ctx.covering_node();
        if !node.is_some_and(|n| n.kind().is_expression()) {
            return false;
        }
    }
    let Some(binding) = node.and_then(|n| n.type_binding()) else { return false };
    if binding.name() == "void" {
        return false;
    }
    // `JDTUtils.isUnnamedClass` on the unit's types.
    !ctx.root().children().iter().any(|t| t.is(NodeKind::ImplicitTypeDeclaration))
}
