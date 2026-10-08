//! Port of jdt.ls `QuickAssistProcessor` and `RefactorProcessor` (the parts
//! ported so far).

use std::collections::BTreeMap;
use std::sync::Arc;

mod lambda;
mod convert_var;
mod method_ref;
mod nls;
mod string_concat;
mod text_block;
mod util;
mod variable;

use super::edit::Env;
use super::handler::Request;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, Proposal};
use crate::refactoring::convert_to_record::ConvertToRecord;
use crate::refactoring::extract_constant::ExtractConstant;
use crate::refactoring::extract_field::{self, ExtractField};
use crate::refactoring::extract_method::ExtractMethod;
use crate::refactoring::extract_temp::ExtractTemp;
use crate::semantic_ast::{Ast, NodeKind};

/// `QuickAssistProcessor.getAssists`.
pub async fn assists(env: &Env<'_>, req: &Request<'_>) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    super::assign_to_field::assign_param_to_field_proposals(env, &req.context, &mut proposals)
        .await;
    if let Some(p) = extract_method_from_lambda_proposal(env, &req.context, !req.locations.is_empty()).await {
        proposals.push(p);
    }
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
    if let Some(covering) = req.context.covering_node() {
        convert_to_record_proposals(env, &req.context, covering, &mut proposals).await;
        let options = env.options(&req.context.ast.uri).await;
        lambda::add_inferred_lambda_parameter_types(&req.context, &options, covering, &mut proposals);
        lambda::add_var_lambda_parameter_types(&req.context, &options, covering, &mut proposals);
        lambda::remove_var_or_inferred_lambda_parameter_types(&req.context, &options, covering, &mut proposals);
        lambda::change_lambda_body_to_block(&req.context, covering, &mut proposals);
        lambda::change_lambda_body_to_expression(&req.context, covering, &mut proposals);
        method_ref::clean_up_lambda(&req.context, &options, covering, &mut proposals);
        method_ref::convert_method_reference_to_lambda(&req.context, covering, &mut proposals);
        method_ref::convert_lambda_to_method_reference(&req.context, &options, covering, &mut proposals);
        string_concat::convert_to_message_format(&req.context, &options, covering, &mut proposals);
        string_concat::convert_to_string_buffer(&req.context, &options, covering, &mut proposals);
        string_concat::convert_to_string_format(&req.context, &options, covering, &mut proposals);
        text_block::string_concat_to_text_block(&req.context, &options, covering, &mut proposals);
        variable::split_variable(&req.context, &options, covering, &mut proposals);
        variable::join_variable(&req.context, &options, covering, &mut proposals);
        variable::invert_equals(&req.context, covering, &mut proposals);
    }
    // jdt.ls offers "Add Javadoc comment" only for units backed by a file
    // (verified against 1.58.0 for a working copy of a nonexistent file);
    // documents that aren't `file:` URIs are virtual and keep it.
    let backed = req.uri.to_file_path().map_or(req.uri.scheme() != "file", |p| p.is_file());
    if backed && !req.locations.iter().any(|p| p.problem_id == crate::semantic_ast::problem::JavadocMissing) {
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
        let return_as_command = crate::features::preferences::extended_capability("advancedExtractRefactoringSupport");
        let infer_selection = crate::features::preferences::extended_capability_list_contains("inferSelectionSupport", "extractMethod");
        if let Some(p) = extract_method_proposal_for(env, &req.context, problems_at_location, return_as_command, infer_selection, Some(req.params)).await {
            proposals.push(p);
        }
        if let Some(p) = extract_field_proposal(env, req, problems_at_location).await {
            proposals.push(p);
        }
        let options = env.options(&req.context.ast.uri).await;
        convert_var::convert_var_type_to_resolved_type(env, &req.context, &options, covering, &mut proposals).await;
        convert_var::convert_resolved_type_to_var_type(&req.context, &options, covering, &mut proposals);
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

/// `QuickAssistProcessor.getConvertToRecordProposals` /
/// `ConvertRecordSubProcessor.getConvertToRecordProposals`.
async fn convert_to_record_proposals(env: &Env<'_>, ctx: &Context, node: crate::semantic_ast::Node<'_>, out: &mut Vec<Proposal>) {
    let options = env.options(&ctx.ast.uri).await;
    // `JavaModelUtil.is16OrHigher(project)`.
    let compliance = options.get("org.eclipse.jdt.core.compiler.compliance").map(String::as_str).unwrap_or("1.8");
    if crate::project::compare_java_versions(compliance, "16") == std::cmp::Ordering::Less {
        return;
    }
    let Ok(refactoring) = ConvertToRecord::check_all_conditions(env, &ctx.ast, node.start(), node.length()).await else { return };
    let label = messages::refactoring("ConvertToRecordRefactoring_name");
    out.push(Proposal::new(label, kind::QUICK_FIX, relevance::CONVERT_TO_RECORD, Change::Lazy(Box::new(refactoring))));
}

#[tower_lsp::async_trait]
impl LazyChange for ConvertToRecord {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        Ok(self.create_change(env).await)
    }
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

/// The `ExtractMethodRefactoring` behind an "Extract to method" proposal.
pub struct ExtractMethodChange {
    pub ast: Arc<Ast>,
    pub offset: usize,
    pub length: usize,
    pub method_name: String,
}

impl ExtractMethodChange {
    /// `createTextChange()`: the change and the `"name"` linked positions.
    pub fn create(&self, options: BTreeMap<String, String>) -> (Vec<CuChange>, Vec<(crate::rewrite::RNode, i32)>) {
        let mut r = ExtractMethod::new(self.ast.clone(), options, self.offset, self.length);
        r.set_method_name(&self.method_name);
        if r.check_initial_conditions().has_fatal_error() {
            return (Vec::new(), Vec::new());
        }
        if r.check_final_conditions().has_fatal_error() {
            return (Vec::new(), Vec::new());
        }
        let cu = r.create_change();
        (vec![CuChange::rewrite(cu.rewrite).with_imports(cu.imports)], r.name_positions.clone())
    }
}

#[tower_lsp::async_trait]
impl LazyChange for ExtractMethodChange {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        Ok(self.create(options).0)
    }
}

/// `RefactorProposalUtility.getIndex(offset, statements)`.
fn statement_index(offset: usize, statements: &[crate::semantic_ast::Node<'_>]) -> i64 {
    for (i, s) in statements.iter().enumerate() {
        if offset <= s.start() {
            return i as i64;
        }
        if offset < s.end() {
            return -1;
        }
    }
    statements.len() as i64
}

/// `StringUtils.capitalize`.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `RefactorProposalUtility.proposeMethodNameHeuristic(context, coveringNode)`.
fn propose_method_name_heuristic(ctx: &Context, covering: crate::semantic_ast::Node<'_>) -> String {
    let ast = ctx.ast.clone();
    let (sel_start, sel_end) = (ctx.selection_offset as i32, (ctx.selection_offset + ctx.selection_length) as i32);
    let mut unused: Option<&crate::semantic_ast::AstProblem> = None;
    for p in &ast.problems {
        if p.id == crate::semantic_ast::problem::LocalVariableIsNeverUsed && p.source_start >= sel_start && p.source_end <= sel_end {
            // `Stream.max`: the last of equal maxima wins.
            if unused.is_none_or(|b| p.source_start - b.source_start >= 0) {
                unused = Some(p);
            }
        }
    }
    if let Some(problem) = unused {
        let finder = crate::semantic_ast::finder::NodeFinder::new(ast.root(), problem.source_start.max(0) as usize, (problem.source_end - problem.source_start).max(0) as usize);
        if let Some(n) = finder.covering.filter(|n| n.is(NodeKind::SimpleName)) {
            return format!("get{}", capitalize(&n.identifier()));
        }
    }
    let selection = crate::refactoring::selection::Selection::from_start_length(ctx.selection_offset, ctx.selection_length);
    let analyzer = crate::refactoring::selection::SelectionAnalyzer::analyze(selection, true, ast.root());
    let mut var_decls: Vec<crate::semantic_ast::Node<'_>> = analyzer.selected_nodes(ast.root()).into_iter().filter(|n| n.is(NodeKind::VariableDeclarationStatement)).collect();
    if var_decls.is_empty() && covering.is(NodeKind::VariableDeclarationStatement) {
        var_decls.push(covering);
    } else if covering.is(NodeKind::ExpressionStatement) {
        if let Some(lhs) = covering.child("expression").filter(|e| e.is(NodeKind::Assignment)).and_then(|a| a.child("leftHandSide")) {
            if lhs.is(NodeKind::SimpleName) {
                return format!("get{}", capitalize(&lhs.identifier()));
            }
        }
    }
    if let Some(last) = var_decls.last() {
        if let Some(name) = last.list("fragments").first().and_then(|f| f.child("name")) {
            return format!("get{}", capitalize(&name.identifier()));
        }
    }
    "extracted".to_owned()
}

/// `RefactorProposalUtility.getUniqueMethodName(astNode, suggestedName)`.
fn unique_method_name(node: crate::semantic_ast::Node<'_>, suggested: &str) -> String {
    let typ = std::iter::once(node).chain(node.ancestors()).find(|n| n.is(NodeKind::TypeDeclaration) || n.is(NodeKind::AnonymousClassDeclaration));
    let Some(t) = typ.filter(|t| t.is(NodeKind::TypeDeclaration) && t.binding().is_some()) else { return suggested.to_owned() };
    let methods: Vec<String> = t.list("bodyDeclarations").iter().filter(|d| d.is(NodeKind::MethodDeclaration)).filter_map(|d| d.child("name")).map(|n| n.identifier()).collect();
    let mut postfix = 2;
    let mut result = suggested.to_owned();
    while postfix < 1000 {
        if !methods.contains(&result) {
            return result;
        }
        result = format!("{suggested}{postfix}");
        postfix += 1;
    }
    suggested.to_owned()
}

/// `RefactorProposalUtility.getExtractMethodProposal(params, context,
/// coveringNode, problemsAtLocation, formattingOptions, returnAsCommand,
/// inferSelectionSupport)`.
pub async fn extract_method_proposal_for(
    env: &Env<'_>,
    ctx: &Context,
    problems_at_location: bool,
    return_as_command: bool,
    infer_selection: bool,
    params: Option<&tower_lsp::lsp_types::CodeActionParams>,
) -> Option<Proposal> {
    let ast = ctx.ast.clone();
    let options = env.options(&ast.uri).await;
    extract_method_proposal_sync(ctx, options, problems_at_location, return_as_command, infer_selection, params)
}

fn extract_method_proposal_sync(
    ctx: &Context,
    options: BTreeMap<String, String>,
    problems_at_location: bool,
    return_as_command: bool,
    infer_selection: bool,
    params: Option<&tower_lsp::lsp_types::CodeActionParams>,
) -> Option<Proposal> {
    let ast = ctx.ast.clone();
    let covering = ctx.covering_node()?;
    if !(covering.kind().is_expression() || covering.kind().is_statement()) {
        return None;
    }
    if covering.is(NodeKind::Block) {
        let statements = covering.list("statements");
        let start = statement_index(ctx.selection_offset, &statements);
        if start == -1 {
            return None;
        }
        let end = statement_index(ctx.selection_offset + ctx.selection_length, &statements);
        if end == -1 || end <= start {
            return None;
        }
    }
    let suggested = propose_method_name_heuristic(ctx, covering);
    let method_name = unique_method_name(covering, &suggested);
    let label = messages::ls_correction("QuickAssistProcessor_extractmethod_description");
    let relevance = if problems_at_location { relevance::EXTRACT_METHOD_ERROR } else { relevance::EXTRACT_METHOD };
    let command = || {
        Proposal::command(
            label,
            kind::REFACTOR_EXTRACT_FUNCTION,
            relevance,
            "java.action.applyRefactoringCommand",
            vec![serde_json::json!("extractMethod"), serde_json::to_value(params).expect("serializable code action parameters")],
        )
    };
    if ctx.selection_length == 0 {
        if !infer_selection {
            return None;
        }
        let mut parent = Some(covering);
        while let Some(p) = parent.filter(|p| p.kind().is_expression()) {
            if !p.is(NodeKind::ParenthesizedExpression) {
                let mut r = ExtractMethod::new(ast.clone(), options.clone(), p.start(), p.length());
                if r.check_initial_conditions().is_ok() {
                    return Some(command());
                }
            }
            parent = p.parent();
        }
        return None;
    }
    let mut r = ExtractMethod::new(ast.clone(), options, ctx.selection_offset, ctx.selection_length);
    r.set_method_name(&method_name);
    if !r.check_initial_conditions().is_ok() {
        return None;
    }
    if return_as_command {
        return Some(command());
    }
    let change = ExtractMethodChange { ast: ast.clone(), offset: ctx.selection_offset, length: ctx.selection_length, method_name };
    Some(Proposal::new(label, kind::REFACTOR_EXTRACT_FUNCTION, relevance, Change::Lazy(Box::new(change))))
}

/// `QuickAssistProcessor.getExtractMethodFromLambdaProposal`.
async fn extract_method_from_lambda_proposal(env: &Env<'_>, ctx: &Context, problems_at_location: bool) -> Option<Proposal> {
    let options = env.options(&ctx.ast.uri).await;
    let covering = ctx.covering_node()?;
    if covering.is(NodeKind::Block) && covering.location_is("body") && covering.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
        return None;
    }
    let lambda = covering.ancestors().find(|a| a.is(NodeKind::LambdaExpression) || a.kind().is_body_declaration()).filter(|a| a.is(NodeKind::LambdaExpression))?;
    let body = lambda.child("body")?;
    let method_name = unique_method_name(covering, "extracted");
    let mut r = ExtractMethod::new(ctx.ast.clone(), options, body.start(), body.length());
    r.set_method_name(&method_name);
    if !r.check_initial_conditions().is_ok() {
        return None;
    }
    let label = messages::ls_correction("QuickAssistProcessor_extractmethod_from_lambda_description");
    let relevance = if problems_at_location { relevance::EXTRACT_METHOD_ERROR } else { relevance::EXTRACT_LAMBDA_BODY_TO_METHOD };
    let change = ExtractMethodChange { ast: ctx.ast.clone(), offset: body.start(), length: body.length(), method_name };
    Some(Proposal::new(label, kind::QUICK_ASSIST, relevance, Change::Lazy(Box::new(change))))
}

/// The refactoring of `getExtractMethodProposal(..., returnAsCommand=false)`
/// (`GetRefactorEditHandler`), when the proposal exists.
pub fn extract_method_change(ctx: &Context, options: BTreeMap<String, String>, problems_at_location: bool) -> Option<ExtractMethodChange> {
    extract_method_proposal_sync(ctx, options, problems_at_location, false, false, None)?;
    let covering = ctx.covering_node()?;
    let method_name = unique_method_name(covering, &propose_method_name_heuristic(ctx, covering));
    Some(ExtractMethodChange { ast: ctx.ast.clone(), offset: ctx.selection_offset, length: ctx.selection_length, method_name })
}
