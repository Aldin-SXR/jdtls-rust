//! Sealed type corrections: `getSealedMissingModifierProposal`
//! (ModifierCorrectionSubProcessorCore) and the `SealedClassFixCore`
//! proposals of `LocalCorrectionsSubProcessor`
//! (`addSealedAsDirectSuperTypeProposal`, `addTypeAsPermittedSubTypeProposal`).

use std::sync::Arc;

use super::change::ModifierChange;
use super::visibility::Units;
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

/// `getSealedMissingModifierProposal(context, problem, proposals)`.
pub fn sealed_missing_modifier(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::SimpleName)) else { return };
    let Some(type_decl) = selected.parent().filter(|p| p.is(NodeKind::TypeDeclaration)) else { return };
    let is_interface = type_decl.simple("interface") == Some("true");
    let Some(binding) = type_decl.binding() else { return };
    let name = type_decl.child("name").map(|n| n.identifier()).unwrap_or_default();
    let rel = relevance::CHANGE_MODIFIER_TO_FINAL;
    let change = |included: i32| ModifierChange { source: ctx.ast.clone(), target_uri: None, binding: binding.key().to_owned(), included, excluded: 0 };
    if !is_interface {
        // Add final modifier
        let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifierto_final_description"), &[&name]);
        proposals.push(ModifierChange::proposal(label, rel, change(modifier::FINAL)));
    }
    // Add sealed modifier
    let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifierto_sealed_description"), &[&name]);
    proposals.push(ModifierChange::proposal(label, rel, change(modifier::SEALED)));
    // Add non-sealed modifier
    let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifierto_nonsealed_description"), &[&name]);
    proposals.push(ModifierChange::proposal(label, rel, change(modifier::NON_SEALED)));
}

/// Climbs `while (selectedNode.getParent() instanceof Type)`.
fn outermost_type(node: Node<'_>) -> Node<'_> {
    let mut n = node;
    while let Some(p) = n.parent().filter(|p| p.kind().is_type()) {
        n = p;
    }
    n
}

/// The type binding of a `SimpleType` that resolves to a Java element.
fn simple_type_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    node.is(NodeKind::SimpleType).then(|| node.binding()).flatten().filter(|b| !b.is_recovered())
}

/// Adds `type` to a type declaration of another (or the same) unit:
/// `AddSealedAsDirectSuperTypeProposalOperation` and
/// `AddTypeAsPermittedSubTypeProposalOperation`.
struct AddSuperType {
    source: Arc<Ast>,
    target_uri: Option<String>,
    /// Key of the type declaration to change.
    declaration: String,
    /// Simple name of the type to add.
    name: String,
    /// Key of the type to import (a binding of `source`).
    import: String,
    /// `TypeDeclaration` property the new type goes to.
    property: &'static str,
}

#[tower_lsp::async_trait]
impl LazyChange for AddSuperType {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let target = match &self.target_uri {
            None => self.source.clone(),
            Some(uri) => crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(uri)?).await?,
        };
        let decl = target
            .binding_by_key(&self.declaration)
            .and_then(|b| b.declaring_node())
            .ok_or_else(|| anyhow::anyhow!("no type declaration"))?;
        let options = env.options(&target.uri).await;
        let mut rw = ASTRewrite::new(target.clone());
        let name = rw.new_simple_name(&self.name);
        let typ = rw.new_simple_type(name);
        if self.property == "superclassType" {
            rw.set(RNode::Orig(decl.id), "superclassType", Some(typ));
        } else {
            rw.list_insert_last(RNode::Orig(decl.id), self.property, typ);
        }
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        if let Some(binding) = self.source.binding_by_key(&self.import) {
            let context = ConstructorImportContext { ast: target.clone(), declaration: Some(decl.id), nullness: None };
            imports.add_import_binding(binding, &context);
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

/// `LocalCorrectionsSubProcessor.addSealedAsDirectSuperTypeProposal`.
pub async fn sealed_as_direct_super_type(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast) else { return };
    let selected = outermost_type(selected);
    if !selected.location_is("permitsTypes") {
        return;
    }
    let Some(sealed_type) = selected.parent() else { return };
    let Some(permitted) = simple_type_binding(selected) else { return };
    let units = Units::load(env, &ast.uri).await;
    let Some(target_uri) = units.find(ast, permitted) else { return };
    let Some(sealed_binding) = sealed_type.binding() else { return };
    let is_interface = sealed_type.simple("interface") == Some("true");
    let permitted_name = permitted.type_declaration().unwrap_or(permitted).name().to_owned();
    let sealed_name = sealed_type.child("name").map(|n| n.identifier()).unwrap_or_default();
    let key = if is_interface {
        "LocalCorrectionsSubProcessor_declareSealedAsDirectSuperInterface_description"
    } else {
        "LocalCorrectionsSubProcessor_declareSealedAsDirectSuperClass_description"
    };
    let label = messages::format(messages::correction(key), &[&sealed_name, &permitted_name]);
    let change = AddSuperType {
        source: ctx.ast.clone(),
        target_uri,
        declaration: permitted.type_declaration().unwrap_or(permitted).key().to_owned(),
        name: sealed_name,
        import: sealed_binding.key().to_owned(),
        property: if is_interface { "superInterfaceTypes" } else { "superclassType" },
    };
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::DECLARE_SEALED_AS_DIRECT_SUPER_TYPE, Change::Lazy(Box::new(change))));
}

/// `LocalCorrectionsSubProcessor.addTypeAsPermittedSubTypeProposal`.
pub async fn type_as_permitted_sub_type(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast) else { return };
    // SealedClassFixCore.getSealedType
    let selected = outermost_type(selected);
    if !selected.location_is("superclassType") && !selected.location_is("superInterfaceTypes") {
        return;
    }
    let Some(sealed) = simple_type_binding(selected) else { return };
    let sealed = sealed.type_declaration().unwrap_or(sealed);
    // getCompilationUnitForSealedType: a sealed source type.
    if !sealed.is_from_source() || sealed.modifiers() & modifier::SEALED == 0 {
        return;
    }
    let Some(sub_type) = selected.parent().filter(|p| p.is(NodeKind::TypeDeclaration)) else { return };
    let Some(sub_binding) = sub_type.binding() else { return };
    let units = Units::load(env, &ast.uri).await;
    let Some(target_uri) = units.find(ast, sealed) else { return };
    let sub_name = sub_type.child("name").map(|n| n.identifier()).unwrap_or_default();
    let label = messages::format(
        messages::correction("LocalCorrectionsSubProcessor_declareSubClassAsPermitsSealedClass_description"),
        &[&sub_name, sealed.name()],
    );
    let change = AddSuperType {
        source: ctx.ast.clone(),
        target_uri,
        declaration: sealed.key().to_owned(),
        name: sub_name,
        import: sub_binding.key().to_owned(),
        property: "permitsTypes",
    };
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::DECLARE_SEALED_AS_DIRECT_SUPER_TYPE, Change::Lazy(Box::new(change))));
}
