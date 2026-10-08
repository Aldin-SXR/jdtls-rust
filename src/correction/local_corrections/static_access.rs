//! `LocalCorrectionsSubProcessor.addCorrectAccessToStaticProposals` with
//! `CodeStyleFixCore.createIndirectAccessToStaticFix` /
//! `createNonStaticAccessFixes` and their `ToStaticAccessOperation`.

use crate::correction::edit::Env;
use crate::correction::modifier_corrections::{non_accessible_reference, TO_NON_STATIC};
use crate::correction::type_mismatch::bindings::normalize_type_binding;
use crate::correction::type_mismatch::proposals::import_context;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::find_parent_statement;
use crate::semantic_ast::{modifier as m, problem as p, BindingRef, Node, NodeKind};

/// `ToStaticAccessOperation`: replace `qualifier` with the type.
struct ToStaticAccess<'a> {
    type_binding: BindingRef<'a>,
    qualifier: Node<'a>,
}

impl ToStaticAccess<'_> {
    /// `getAccessorName()`.
    fn accessor_name(&self) -> &str {
        self.type_binding.name()
    }

    /// `rewriteAST(cuRewrite, model)` in a fresh `CompilationUnitRewrite`.
    fn change(&self, ctx: &Context, options: &std::collections::BTreeMap<String, String>) -> Change {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
        if matches!(self.qualifier.kind(), NodeKind::MethodInvocation | NodeKind::ClassInstanceCreation) {
            extract_qualifier(&mut rw, self.qualifier);
        }
        let context = import_context(&ctx.ast, self.qualifier, options);
        let typ = imports.add_import_type(self.type_binding, &mut rw, &context, TypeLocation::Unknown);
        rw.replace(RNode::Orig(self.qualifier.id), Some(typ));
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

/// `ToStaticAccessOperation.extractQualifier` (a new block per problem, as
/// each fix gets its own `createdBlocks` map).
fn extract_qualifier(rw: &mut ASTRewrite, qualifier: Node<'_>) {
    let Some(statement) = find_parent_statement(qualifier) else { return };
    let Some(parent) = statement.parent() else { return };
    let expression = rw.create_move_target(qualifier.id);
    let new_statement = rw.new_expression_statement(expression);
    if parent.is(NodeKind::Block) {
        rw.list_insert_before(RNode::Orig(parent.id), "statements", new_statement, RNode::Orig(statement.id));
    } else {
        let block = rw.new_block(Vec::new());
        let last = rw.create_move_target(statement.id);
        rw.list_insert_last(block, "statements", last);
        let Some(location) = statement.location() else { return };
        rw.set(RNode::Orig(parent.id), location, Some(block));
        rw.list_insert_before(block, "statements", new_statement, last);
    }
}

/// `CodeStyleFixCore.createToStaticAccessOperations(astRoot, createdBlocks, problem, false)`.
fn to_static_access_operations<'a>(ctx: &'a Context, problem: &ProblemLocation) -> Option<Vec<ToStaticAccess<'a>>> {
    let ast = ctx.ast();
    let mut selected = problem.covering_node(ast)?;
    if selected.is(NodeKind::SimpleName) {
        selected = selected.parent()?;
    }
    let (qualifier, access) = match selected.kind() {
        NodeKind::QualifiedName => (selected.child("qualifier"), selected.binding().or_else(|| selected.child("name").and_then(|n| n.binding()))),
        NodeKind::MethodInvocation => (selected.child("expression"), selected.child("name").and_then(|n| n.binding())),
        NodeKind::FieldAccess => (selected.child("expression"), selected.child("name").and_then(|n| n.binding())),
        _ => (None, None),
    };
    let (qualifier, access) = (qualifier?, access?);
    let mut declaring = None;
    let declaring_type = if access.is_method() || access.is_variable() { access.declaring_class() } else { None };
    let declaring_type = declaring_type.map(|t| t.type_declaration().unwrap_or(t));
    if let Some(declaring_type) = declaring_type {
        let modifiers = declaring_type.modifiers();
        if modifiers & (m::PUBLIC | m::PROTECTED | m::PRIVATE) == 0 {
            match ast.root().child("package") {
                None => return None,
                Some(package) => {
                    let name = package.child("name").map(|n| n.identifier()).unwrap_or_default();
                    if declaring_type.package_name().unwrap_or("") != name {
                        return None;
                    }
                }
            }
        }
        declaring = Some(ToStaticAccess { type_binding: declaring_type, qualifier });
    }
    let mut instance = None;
    if let Some(instance_type) = normalize_type_binding(qualifier.type_binding()) {
        let instance_type = instance_type.type_declaration().unwrap_or(instance_type);
        let declaration = instance_type.type_declaration().unwrap_or(instance_type);
        if declaring_type.is_none_or(|d| d.key() != declaration.key()) {
            instance = Some(ToStaticAccess { type_binding: instance_type, qualifier });
        }
    }
    // `new ToStaticAccessOperation[] {declaring}` may hold a null declaring
    // operation; such fixes are never created from it.
    let declaring = declaring?;
    let mut ops = vec![declaring];
    ops.extend(instance);
    Some(ops)
}

/// `LocalCorrectionsSubProcessor.addCorrectAccessToStaticProposals`.
pub async fn correct_access_to_static(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let id = problem.problem_id;
    let options = env.options(&ctx.ast.uri).await;
    if matches!(id, p::IndirectAccessToStaticField | p::IndirectAccessToStaticMethod) {
        if let Some(ops) = to_static_access_operations(ctx, problem) {
            let label = messages::format(messages::fix("CodeStyleFix_ChangeStaticAccess_description"), &[ops[0].accessor_name()]);
            proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::CREATE_INDIRECT_ACCESS_TO_STATIC, ops[0].change(ctx, &options)));
            return;
        }
    }
    if matches!(id, p::NonStaticAccessToStaticField | p::NonStaticAccessToStaticMethod | p::NonStaticOrAlienTypeReceiver) {
        if let Some(ops) = to_static_access_operations(ctx, problem) {
            let label = messages::format(messages::fix("CodeStyleFix_ChangeAccessToStatic_description"), &[ops[0].accessor_name()]);
            proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::CREATE_NON_STATIC_ACCESS_USING_DECLARING_TYPE, ops[0].change(ctx, &options)));
            if let Some(op) = ops.get(1) {
                let label = messages::format(messages::fix("CodeStyleFix_ChangeAccessToStaticUsingInstanceType_description"), &[op.accessor_name()]);
                proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::CREATE_NON_STATIC_ACCESS_USING_INSTANCE_TYPE, op.change(ctx, &options)));
            }
        }
    }
    non_accessible_reference(env, ctx, problem, proposals, TO_NON_STATIC, relevance::REMOVE_STATIC_MODIFIER).await;
}
