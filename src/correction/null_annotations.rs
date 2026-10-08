//! Port of jdt.ls `NullAnnotationsCorrectionProcessor` over
//! `NullAnnotationsCorrectionProcessorCore`, `NullAnnotationsFixCore` and the
//! null annotation proposals.

mod extract;
mod signature;

use std::collections::BTreeMap;
use std::sync::Arc;

use self::signature::Builder;
use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{problem as p, Ast, Node, NodeKind};

pub use self::extract::extract_checked_local_proposal;

const NULL_ANALYSIS: &str = "org.eclipse.jdt.core.compiler.annotation.nullanalysis";

/// `NullAnnotationsRewriteOperations.ChangeKind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Local,
    Inverse,
    Overridden,
    Target,
}

/// `NullAnnotationsFixCore.get{Nullable,NonNull,NonNullByDefault}AnnotationName(project, false)`.
pub(crate) fn annotation_name(options: &BTreeMap<String, String>, which: &str) -> Option<String> {
    options.get(&format!("org.eclipse.jdt.core.compiler.annotation.{which}")).cloned()
}

fn simple_name(qualified: &str) -> &str {
    qualified.rsplit('.').next().unwrap_or(qualified)
}

/// Whether the project has annotation based null analysis enabled.
pub async fn null_analysis_enabled(env: &Env<'_>, ctx: &Context) -> bool {
    env.options(&ctx.ast.uri).await.get(NULL_ANALYSIS).is_some_and(|v| v == "enabled")
}

/// `NullAnnotationsFixCore.isComplainingAboutArgument`.
fn is_complaining_about_argument(selected: Node<'_>) -> bool {
    if !selected.is(NodeKind::SimpleName) {
        return false;
    }
    let is_parameter = |b: Option<crate::semantic_ast::BindingRef<'_>>| b.is_some_and(|b| b.is_variable() && b.is_parameter());
    if is_parameter(selected.binding()) {
        return true;
    }
    match selected.ancestors().find(|a| a.kind().is_variable_declaration()) {
        Some(declaration) => is_parameter(declaration.binding()),
        None => false,
    }
}

/// `NullAnnotationsFixCore.isComplainingAboutReturn`.
fn is_complaining_about_return(selected: Node<'_>) -> bool {
    if selected.parent().is_some_and(|p| p.is(NodeKind::ReturnStatement)) {
        return true;
    }
    let mut node = Some(selected);
    while let Some(n) = node {
        if n.kind().is_type() {
            return n.location_is("returnType2");
        }
        node = n.parent();
    }
    false
}

/// `getReturnAndArgumentTypeProposal`.
pub async fn return_and_argument_type_proposal(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, change_kind: ChangeKind, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let is_argument = is_complaining_about_argument(selected);
    if is_argument || is_complaining_about_return(selected) {
        null_annotation_in_signature_proposal(env, ctx, problem, proposals, change_kind, is_argument).await;
    }
}

/// `getNullAnnotationInSignatureProposal` / `createNullAnnotationInSignatureFix`.
pub async fn null_annotation_in_signature_proposal(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, change_kind: ChangeKind, is_argument: bool) {
    let options = env.options(&ctx.ast.uri).await;
    let (Some(nullable), Some(nonnull)) = (annotation_name(&options, "nullable"), annotation_name(&options, "nonnull")) else { return };
    let mut builder = Builder::new(problem, ctx.ast.clone(), nullable, nonnull, true, is_argument, change_kind);
    let mut add_non_null = false;
    match problem.problem_id {
        p::IllegalDefinitionToNonNullParameter | p::IllegalRedefinitionToNonNullParameter => {
            if change_kind == ChangeKind::Overridden {
                add_non_null = true;
                builder.swap_annotations();
            }
        }
        p::ParameterLackingNonNullAnnotation | p::IllegalReturnNullityRedefinition => {
            if change_kind != ChangeKind::Overridden {
                add_non_null = true;
                builder.swap_annotations();
            }
        }
        p::NullityUncheckedTypeAnnotation
        | p::RequiredNonNullButProvidedNull
        | p::RequiredNonNullButProvidedPotentialNull
        | p::RequiredNonNullButProvidedUnknown
        | p::RequiredNonNullButProvidedSpecdNullable => {
            if is_argument == (change_kind != ChangeKind::Target) {
                add_non_null = true;
                builder.swap_annotations();
            }
        }
        p::ConflictingNullAnnotations | p::ConflictingInheritedNullAnnotations => {
            if problem.problem_id == p::ConflictingNullAnnotations && is_argument && change_kind == ChangeKind::Inverse {
                return;
            }
            if change_kind == ChangeKind::Inverse || change_kind == ChangeKind::Overridden {
                add_non_null = true;
                builder.swap_annotations();
            }
        }
        _ => {}
    }
    let Some(mut operation) = builder.create_add_annotation_operation(env).await else { return };
    if add_non_null {
        operation.remove_if_non_null_default = true;
        operation.non_null_default_names = non_null_by_default_names(&options);
    }
    let relevance = if change_kind == ChangeKind::Overridden { relevance::CHANGE_NULLNESS_ANNOTATION_IN_OVERRIDDEN_METHOD } else { relevance::CHANGE_NULLNESS_ANNOTATION };
    let message = operation.message.clone();
    proposals.push(Proposal::new(message, kind::QUICK_FIX, relevance, Change::Lazy(Box::new(operation))));
}

/// `RedundantNullnessTypeAnnotationsFilter.determineNonNullByDefaultNames`.
fn non_null_by_default_names(options: &BTreeMap<String, String>) -> Option<std::collections::HashSet<String>> {
    let primary = annotation_name(options, "nonnullbydefault")?;
    let mut names = std::collections::HashSet::from([primary]);
    if let Some(secondary) = annotation_name(options, "nonnullbydefault.secondary") {
        names.extend(secondary.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned));
    }
    Some(names)
}

/// `getRemoveRedundantAnnotationProposal`.
pub fn remove_redundant_annotation_proposal(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let change = RemoveRedundant { ast: ctx.ast.clone(), offset: problem.offset, length: problem.length, problem_id: problem.problem_id };
    proposals.push(Proposal::new(
        messages::fix("NullAnnotationsRewriteOperations_remove_redundant_nullness_annotation"),
        kind::QUICK_FIX,
        relevance::REMOVE_REDUNDANT_NULLNESS_ANNOTATION,
        Change::Lazy(Box::new(change)),
    ));
}

/// `RemoveRedundantAnnotationRewriteOperation`.
struct RemoveRedundant {
    ast: Arc<Ast>,
    offset: usize,
    length: usize,
    problem_id: i32,
}

#[tower_lsp::async_trait]
impl LazyChange for RemoveRedundant {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let mut rw = ASTRewrite::new(self.ast.clone());
        let selected = crate::semantic_ast::finder::NodeFinder::new(self.ast.root(), self.offset, self.length).covering;
        let annotation_name_of = |annotation: Node<'_>| self.ast.data(annotation.id).annotation.as_ref().map(|a| self.ast.binding(a.annotation_type).name().to_owned());
        if self.problem_id == p::RedundantNullAnnotation {
            let modifiers = match selected {
                Some(n) if matches!(n.kind(), NodeKind::SingleVariableDeclaration | NodeKind::FieldDeclaration | NodeKind::MethodDeclaration) => n.list("modifiers"),
                Some(n) if n.kind().is_annotatable_type() => n.list("annotations"),
                _ => return Ok(vec![CuChange::rewrite(rw)]),
            };
            let nonnull = annotation_name(&options, "nonnull").map(|n| simple_name(&n).to_owned());
            for modifier in modifiers {
                if modifier.is(NodeKind::MarkerAnnotation) && annotation_name_of(modifier) == nonnull {
                    rw.remove(RNode::Orig(modifier.id));
                }
            }
        } else if let Some(annotation) = selected.filter(|n| n.kind().is_annotation()) {
            let default = annotation_name(&options, "nonnullbydefault").map(|n| simple_name(&n).to_owned());
            if annotation_name_of(annotation) == default {
                rw.remove(RNode::Orig(annotation.id));
            }
        }
        Ok(vec![CuChange::rewrite(rw)])
    }
}

/// `getAddMissingDefaultNullnessProposal` (the package-info.java case).
pub async fn add_missing_default_nullness_proposal(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let _ = problem;
    let is_package_info = ctx.ast.uri.rsplit('/').next() == Some("package-info.java");
    if !is_package_info {
        return;
    }
    let options = env.options(&ctx.ast.uri).await;
    let Some(name) = annotation_name(&options, "nonnullbydefault") else { return };
    let label = messages::format(messages::fix("NullAnnotationsRewriteOperations_add_missing_default_nullness_annotation"), &[simple_name(&name)]);
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::ADD_MISSING_NULLNESS_ANNOTATION, Change::Lazy(Box::new(AddMissingDefault { ast: ctx.ast.clone() }))));
}

/// `AddMissingDefaultNullnessRewriteOperation`.
struct AddMissingDefault {
    ast: Arc<Ast>,
}

#[tower_lsp::async_trait]
impl LazyChange for AddMissingDefault {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let mut rw = ASTRewrite::new(self.ast.clone());
        if let (Some(package), Some(name)) = (self.ast.root().child("package"), annotation_name(&options, "nonnullbydefault")) {
            let annotation = rw.new_node(NodeKind::MarkerAnnotation);
            let type_name = rw.new_name(&name);
            rw.put_child(annotation, "typeName", type_name);
            rw.list_insert_last(RNode::Orig(package.id), "annotations", annotation);
        }
        Ok(vec![CuChange::rewrite(rw)])
    }
}

/// `getLocalVariableAnnotationProposal` (`MakeLocalVariableNonNullProposalCore`).
pub async fn local_variable_annotation_proposal(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let options = env.options(&ctx.ast.uri).await;
    let Some(non_null) = annotation_name(&options, "nonnull") else { return };
    let Some(selected) = problem.covered_node(ctx.ast()).filter(|n| n.kind().is_expression()) else { return };
    let Some(binding) = super::type_mismatch::bindings::resolve_expression_binding(selected, false).filter(|b| b.is_variable()) else { return };
    if binding.is_field() {
        return;
    }
    let Some(var_type) = binding.var_type() else { return };
    if var_type.is_array() {
        return;
    }
    let label = messages::format(messages::correction("NullAnnotationsCorrectionProcessor_change_local_variable_to_nonNull"), &[binding.name(), simple_name(&non_null)]);
    let change = MakeLocalVariableNonNull { ast: ctx.ast.clone(), binding: binding.key().to_owned(), annotation: non_null };
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::CHANGE_NULLNESS_ANNOTATION, Change::Lazy(Box::new(change))));
}

struct MakeLocalVariableNonNull {
    ast: Arc<Ast>,
    binding: String,
    annotation: String,
}

#[tower_lsp::async_trait]
impl LazyChange for MakeLocalVariableNonNull {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let mut rw = ASTRewrite::new(self.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(self.ast.clone(), &options);
        let Some(declaration) = super::modifier_corrections::find_declaring_node(&self.ast, &self.binding) else { return Ok(Vec::new()) };
        let context = super::type_mismatch::proposals::import_context(&self.ast, declaration, &options);
        let non_null = imports.add_import(&self.annotation, &context);
        let marker = |rw: &mut ASTRewrite| {
            let annotation = rw.new_node(NodeKind::MarkerAnnotation);
            let name = rw.new_name(&non_null);
            rw.put_child(annotation, "typeName", name)
        };
        match declaration.kind() {
            NodeKind::VariableDeclarationFragment => {
                let parent = declaration.parent();
                match parent {
                    Some(statement) if statement.is(NodeKind::VariableDeclarationStatement) => {
                        let fragments = statement.list("fragments");
                        if fragments.len() > 1 && statement.parent().is_some_and(|b| b.is(NodeKind::Block)) {
                            let placeholder = rw.create_move_target(declaration.id);
                            let new_statement = rw.new_node(NodeKind::VariableDeclarationStatement);
                            rw.put_list(new_statement, "fragments", vec![placeholder]);
                            let copied_type = rw.create_copy_target(statement.child("type").unwrap().id);
                            rw.put_child(new_statement, "type", copied_type);
                            let modifiers = rw.new_modifiers(statement.modifiers());
                            let annotation = marker(&mut rw);
                            let mut all = modifiers;
                            all.push(annotation);
                            rw.put_list(new_statement, "modifiers", all);
                            let block = RNode::Orig(statement.parent().unwrap().id);
                            if fragments.first().is_some_and(|f| *f == declaration) {
                                rw.list_insert_before(block, "statements", new_statement, RNode::Orig(statement.id));
                            } else {
                                rw.list_insert_after(block, "statements", new_statement, RNode::Orig(statement.id));
                            }
                        } else {
                            let annotation = marker(&mut rw);
                            rw.list_insert_last(RNode::Orig(statement.id), "modifiers", annotation);
                        }
                    }
                    Some(expression) if expression.is(NodeKind::VariableDeclarationExpression) => {
                        let annotation = marker(&mut rw);
                        rw.list_insert_last(RNode::Orig(expression.id), "modifiers", annotation);
                    }
                    _ => {}
                }
            }
            NodeKind::SingleVariableDeclaration => {
                let annotation = marker(&mut rw);
                rw.list_insert_last(RNode::Orig(declaration.id), "modifiers", annotation);
            }
            _ => return Ok(Vec::new()),
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}
