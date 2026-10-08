//! `RefactorProcessor.getInlineProposal`: inline local variable, constant
//! and method.

use super::edit::Env;
use super::{kind, messages, relevance, Context, CuChange, Proposal};
use crate::refactoring::inline_constant::InlineConstant;
use crate::refactoring::inline_temp::InlineTemp;
use crate::semantic_ast::{Node, NodeKind};

pub async fn inline_proposals(env: &Env<'_>, ctx: &Context, node: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    if !node.is(NodeKind::SimpleName) {
        return false;
    }
    let Some(binding) = node.binding() else { return false };
    if binding.is_variable() {
        if binding.is_parameter() {
            return false;
        }
        if binding.is_field() {
            return inline_constant(ctx, binding, out);
        }
        let Some(decl) = binding.declaring_node() else { return false };
        if !decl.is(NodeKind::VariableDeclarationFragment) || !decl.parent().is_some_and(|p| p.is(NodeKind::VariableDeclarationStatement) && decl.location_is("fragments")) {
            return false;
        }
        return inline_local_variable(env, ctx, decl, out).await;
    }
    false
}

struct InlineConstantChange {
    ast: std::sync::Arc<crate::semantic_ast::Ast>,
    offset: usize,
    length: usize,
}

#[tower_lsp::async_trait]
impl super::LazyChange for InlineConstantChange {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let Ok(mut refactoring) = InlineConstant::create(env, &self.ast, options, self.offset, self.length).await else { return Ok(Vec::new()) };
        let references = refactoring.references(env).await;
        if references.is_empty() {
            return Ok(Vec::new());
        }
        refactoring.set_remove_declaration(refactoring.is_declaration_selected());
        refactoring.set_replace_all_references(refactoring.is_declaration_selected());
        Ok(refactoring.changes(env, &references).await)
    }
}

/// `RefactoringAvailabilityTesterCore.isInlineConstantAvailable`.
fn inline_constant(ctx: &Context, field: crate::semantic_ast::BindingRef<'_>, out: &mut Vec<Proposal>) -> bool {
    use crate::semantic_ast::modifier;
    let source = field.declaring_class().is_some_and(|c| c.is_from_source());
    if !source || field.modifiers() & modifier::STATIC == 0 || field.modifiers() & modifier::FINAL == 0 || field.is_enum_constant() {
        return false;
    }
    let label = messages::ls_action("InlineConstantRefactoringAction_label");
    let change = InlineConstantChange { ast: ctx.ast.clone(), offset: ctx.selection_offset, length: ctx.selection_length };
    out.push(Proposal::new(label, kind::REFACTOR_INLINE, relevance::INLINE_LOCAL, super::Change::Lazy(Box::new(change))));
    true
}

async fn inline_local_variable(env: &Env<'_>, ctx: &Context, decl: Node<'_>, out: &mut Vec<Proposal>) -> bool {
    let options = env.options(&ctx.ast.uri).await;
    let refactoring = InlineTemp::new(ctx.ast.clone(), options, decl);
    if !refactoring.check_initial_conditions().is_ok() {
        return false;
    }
    let cu = refactoring.create_rewrite();
    let mut change = CuChange::rewrite(cu.rewrite).with_imports(cu.imports);
    if !crate::refactoring::check_source::check_new_source(env, &mut change).await.is_ok() || refactoring.references().is_empty() {
        return false;
    }
    let cu = refactoring.create_rewrite();
    let label = messages::ls_correction("QuickAssistProcessor_inline_local_description");
    out.push(Proposal::new(label, kind::REFACTOR_INLINE, relevance::INLINE_LOCAL, super::Change::Cu(vec![CuChange::rewrite(cu.rewrite).with_imports(cu.imports)])));
    true
}
