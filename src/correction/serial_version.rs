//! Port of `SerialVersionSubProcessor` (`SerialVersionBaseSubProcessor`,
//! `PotentialProgrammingProblemsFixCore.createMissingSerialVersionFixes`,
//! `AbstractSerialVersionOperationCore`, `SerialVersionDefaultOperationCore`
//! and `SerialVersionHashOperationCore`).

use std::sync::Arc;

use super::edit::Env;
use super::{kind, messages, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, problem as p, Ast, Node, NodeId, NodeKind};

/// `SerialVersionBaseSubProcessor.addSerialVersionProposals`.
pub fn serial_version_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    if problem.problem_id != p::MissingSerialVersion {
        return;
    }
    let ast = ctx.ast();
    let Some(name) = selected_name(ast, problem) else { return };
    let Some(decl) = declaration_node(name) else { return };
    let Some(target) = body_declarations_owner(decl) else { return };
    let decl_id = decl.id;
    proposals.push(Proposal::new(
        messages::fix("Java50Fix_SerialVersion_default_description"),
        kind::QUICK_FIX,
        9,
        Change::Cu(vec![CuChange::rewrite(add_field(ctx.ast.clone(), target, "1L"))]),
    ));
    proposals.push(Proposal::new(
        messages::fix("Java50Fix_SerialVersion_hash_description"),
        kind::QUICK_FIX,
        9,
        Change::Lazy(Box::new(GeneratedId { ast: ctx.ast.clone(), declaration: decl_id, target })),
    ));
}

/// `PotentialProgrammingProblemsFixCore.getSelectedName`.
fn selected_name<'a>(ast: &'a Ast, problem: &ProblemLocation) -> Option<Node<'a>> {
    let selection = problem.covered_node(ast)?;
    let name = match selection.kind() {
        NodeKind::SimpleType | NodeKind::NameQualifiedType | NodeKind::QualifiedType => selection.child("name"),
        NodeKind::ParameterizedType => {
            let raw = selection.child("type")?;
            match raw.kind() {
                NodeKind::SimpleType | NodeKind::NameQualifiedType | NodeKind::QualifiedType => raw.child("name"),
                _ => None,
            }
        }
        k if k.is_name() => Some(selection),
        _ => None,
    }?;
    if name.is(NodeKind::SimpleName) {
        Some(name)
    } else {
        name.child("name")
    }
}

/// `PotentialProgrammingProblemsFixCore.getDeclarationNode`.
fn declaration_node(name: Node<'_>) -> Option<Node<'_>> {
    let mut parent = name.parent()?;
    if !parent.kind().is_abstract_type_declaration() {
        parent = parent.parent()?;
        if parent.is(NodeKind::ParameterizedType) || parent.kind().is_type() {
            parent = parent.parent()?;
        }
        if parent.is(NodeKind::ClassInstanceCreation) {
            parent = parent.child("anonymousClassDeclaration")?;
        }
    }
    Some(parent)
}

/// The node whose `bodyDeclarations` receive the field.
fn body_declarations_owner(node: Node<'_>) -> Option<NodeId> {
    if node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration) {
        return Some(node.id);
    }
    if node.is(NodeKind::ParameterizedType) {
        let parent = node.parent()?;
        if parent.is(NodeKind::ClassInstanceCreation) {
            return parent.child("anonymousClassDeclaration").map(|a| a.id);
        }
    }
    None
}

/// `AbstractSerialVersionOperationCore.rewriteAST` with the given initializer.
fn add_field(ast: Arc<Ast>, target: NodeId, initializer: &str) -> ASTRewrite {
    let mut rw = ASTRewrite::new(ast);
    let literal = rw.new_number_literal(initializer);
    let fragment = rw.new_variable_declaration_fragment("serialVersionUID", Some(literal));
    let modifiers = rw.new_modifiers(modifier::PRIVATE | modifier::STATIC | modifier::FINAL);
    let typ = rw.new_primitive_type("long");
    let field = rw.new_field_declaration(fragment, modifiers, typ);
    rw.list_insert_at(RNode::Orig(target), "bodyDeclarations", field, 0);
    rw
}

/// `SerialVersionHashOperationCore`: compiles the unit and hashes the
/// declaring type's class file (`1L` when that fails).
struct GeneratedId {
    ast: Arc<Ast>,
    declaration: NodeId,
    target: NodeId,
}

#[tower_lsp::async_trait]
impl LazyChange for GeneratedId {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let mut id: i64 = 1;
        if let Some(name) = type_binary_name(&self.ast, self.declaration) {
            if let Some(hash) = compiled_hash(env, &self.ast.uri, &name).await {
                id = hash;
            }
        }
        Ok(vec![CuChange::rewrite(add_field(self.ast.clone(), self.target, &format!("{id}L")))])
    }
}

/// `typeBinding.getBinaryName()` of the declaration (`/` separated).
fn type_binary_name(ast: &Ast, decl: NodeId) -> Option<String> {
    let node = ast.node(decl);
    let binding = match node.kind() {
        NodeKind::ParameterizedType => node.binding(),
        _ => node.binding(),
    }?;
    Some(binding.binary_name()?.replace('.', "/"))
}

async fn compiled_hash(env: &Env<'_>, uri: &str, binary_name: &str) -> Option<i64> {
    use crate::analysis::semantic::{ecj_process::next_id, BridgeRequest, BridgeResponse};
    let url = tower_lsp::lsp_types::Url::parse(uri).ok()?;
    let ctx = env.dispatcher.context_for(Some(&url)).await;
    let resp = env
        .dispatcher
        .send_request(BridgeRequest::CompiledClasses {
            id: next_id(),
            files: ctx.files,
            classpath: ctx.classpath,
            source_level: ctx.source_level,
            options: ctx.options,
            names: vec![binary_name.to_owned()],
        })
        .await
        .ok()?;
    let BridgeResponse::CompiledClasses { classes, .. } = resp else { return None };
    let bytes = super::serial_hash::base64_decode(classes.get(binary_name)?)?;
    super::serial_hash::serial_version_id(&bytes)
}
