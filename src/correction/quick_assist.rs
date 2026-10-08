//! Port of jdt.ls `QuickAssistProcessor` and `RefactorProcessor` (the parts
//! ported so far).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::edit::Env;
use super::handler::Request;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, Proposal};
use crate::refactoring::extract_constant::ExtractConstant;
use crate::refactoring::extract_field::{self, ExtractField};
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
        if let Some(p) = extract_field_proposal(env, req, problems_at_location).await {
            proposals.push(p);
        }
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

/// `RefactorProposalUtility.InitializeScope` names, in ordinal order
/// (`ExtractFieldRefactoring.INITIALIZE_IN_*`).
pub const INITIALIZE_SCOPES: [&str; 3] = ["Field declaration", "Current method", "Class constructors"];

/// `InitializeScope.fromName(name).ordinal()`.
pub fn initialize_scope_from_name(name: Option<&str>) -> Option<i32> {
    let name = name?;
    INITIALIZE_SCOPES.iter().position(|s| *s == name).map(|i| i as i32)
}

/// `RefactorProposalUtility.getInitializeScopes(refactoring)`.
fn initialize_scopes(refactoring: &mut ExtractField) -> Vec<&'static str> {
    let mut scopes = Vec::new();
    if refactoring.can_enable_setting_declare_in_method() {
        scopes.push(INITIALIZE_SCOPES[extract_field::INITIALIZE_IN_METHOD as usize]);
    }
    if refactoring.can_enable_setting_declare_in_field_declaration() {
        scopes.push(INITIALIZE_SCOPES[extract_field::INITIALIZE_IN_FIELD as usize]);
    }
    if refactoring.can_enable_setting_declare_in_constructors() {
        scopes.push(INITIALIZE_SCOPES[extract_field::INITIALIZE_IN_CONSTRUCTOR as usize]);
    }
    scopes
}

/// The `ExtractFieldRefactoring` behind an "Extract to field" proposal
/// (`RefactoringCorrectionProposalCore` whose `init` sets the guessed field
/// name).
pub struct ExtractFieldChange {
    pub ast: Arc<Ast>,
    pub offset: usize,
    pub length: usize,
    pub initialize_in: Option<i32>,
}

impl ExtractFieldChange {
    /// `createTextChange()`: the change and the `"name"` linked positions
    /// (no change when the final check is fatal).
    pub fn create(&self, options: BTreeMap<String, String>) -> (Vec<CuChange>, Vec<(crate::rewrite::RNode, i32)>) {
        let mut r = ExtractField::new(self.ast.clone(), options, self.offset, self.length);
        if !r.check_initial_conditions().is_ok() {
            return (Vec::new(), Vec::new());
        }
        if let Some(scope) = self.initialize_in {
            r.set_initialize_in(scope);
        }
        let name = r.guess_field_name();
        r.set_field_name(&name);
        let (status, cu) = r.check_final_conditions();
        match cu {
            Some(cu) if !status.has_fatal_error() => (vec![CuChange::rewrite(cu.rewrite).with_imports(cu.imports)], r.name_positions.clone()),
            _ => (Vec::new(), Vec::new()),
        }
    }
}

#[tower_lsp::async_trait]
impl LazyChange for ExtractFieldChange {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        Ok(self.create(options).0)
    }
}

/// `RefactorProcessor.getExtractFieldProposal` /
/// `RefactorProposalUtility.getGenericExtractFieldProposal`.
async fn extract_field_proposal(env: &Env<'_>, req: &Request<'_>, problems_at_location: bool) -> Option<Proposal> {
    // TODO: upstream tries `getConvertVariableToFieldProposal`
    // (PromoteTempToFieldRefactoring) first; it is not ported yet.
    let return_as_command = crate::features::preferences::extended_capability("advancedExtractRefactoringSupport");
    let infer_selection = crate::features::preferences::extended_capability_list_contains("inferSelectionSupport", "extractField");
    extract_field_proposal_for(env, &req.context, problems_at_location, None, return_as_command, infer_selection, Some(req.params)).await
}

/// `RefactorProposalUtility.getExtractFieldProposal(params, context,
/// problemsAtLocation, formatterOptions, initializeIn, returnAsCommand,
/// inferSelectionSupport)`.
pub async fn extract_field_proposal_for(
    env: &Env<'_>,
    ctx: &Context,
    problems_at_location: bool,
    initialize_in: Option<&str>,
    return_as_command: bool,
    infer_selection: bool,
    params: Option<&tower_lsp::lsp_types::CodeActionParams>,
) -> Option<Proposal> {
    if !supports_extract_variable(ctx) {
        return None;
    }
    let label = messages::ls_correction("QuickAssistProcessor_extract_to_field_description");
    let relevance = if ctx.selection_length == 0 {
        relevance::EXTRACT_LOCAL_ZERO_SELECTION
    } else if problems_at_location {
        relevance::EXTRACT_LOCAL_ERROR
    } else {
        relevance::EXTRACT_LOCAL
    };
    let options = env.options(&ctx.ast.uri).await;
    let params_json = || serde_json::to_value(params).expect("serializable code action parameters");
    let scope = initialize_scope_from_name(initialize_in);
    if ctx.selection_length == 0 && infer_selection {
        let mut parent = ctx.covering_node();
        while let Some(p) = parent.filter(|p| p.kind().is_expression()) {
            if !p.is(NodeKind::ParenthesizedExpression) {
                let mut r = ExtractField::new(ctx.ast.clone(), options.clone(), p.start(), p.length());
                if r.check_initial_conditions().is_ok() {
                    if let Some(scope) = scope {
                        r.set_initialize_in(scope);
                    }
                    if !initialize_scopes(&mut r).is_empty() {
                        return Some(Proposal::command(label, kind::REFACTOR_EXTRACT_FIELD, relevance, "java.action.applyRefactoringCommand", vec![serde_json::json!("extractField"), params_json()]));
                    }
                }
            }
            parent = p.parent();
        }
        return None;
    }
    let mut r = ExtractField::new(ctx.ast.clone(), options, ctx.selection_offset, ctx.selection_length);
    if !r.check_initial_conditions().is_ok() {
        return None;
    }
    if let Some(scope) = scope {
        r.set_initialize_in(scope);
    }
    if return_as_command {
        let scopes = initialize_scopes(&mut r);
        return Some(Proposal::command(
            label,
            kind::REFACTOR_EXTRACT_FIELD,
            relevance,
            "java.action.applyRefactoringCommand",
            vec![serde_json::json!("extractField"), params_json(), serde_json::json!({ "initializedScopes": scopes })],
        ));
    }
    let change = ExtractFieldChange { ast: ctx.ast.clone(), offset: ctx.selection_offset, length: ctx.selection_length, initialize_in: scope };
    Some(Proposal::new(label, kind::REFACTOR_EXTRACT_FIELD, relevance, Change::Lazy(Box::new(change))))
}
