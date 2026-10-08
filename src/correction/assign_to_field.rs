//! Port of the "assign parameter to field" quick assists
//! (`QuickAssistProcessor.getAssignParamToFieldProposals`,
//! `getAssignAllParamsToFieldsProposals` and the field part of
//! `AssignToVariableAssistProposalCore`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::edit::Env;
use super::local_corrections::{used_names, variable_name};
use super::type_mismatch::proposals::import_context;
use super::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::features::accessors;
use crate::refactoring::checks::is_assignment_compatible;
use crate::refactoring::scope::binding_of_parent_type;
use crate::refactoring::snippet_finder::is_in_static_context;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_body_declaration, find_parent_type, normalized_node};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

fn parameter_declaration<'a>(node: Node<'a>) -> Option<(Node<'a>, Node<'a>)> {
    let parent = node.parent().filter(|p| p.is(NodeKind::SingleVariableDeclaration))?;
    let method = parent.parent().filter(|m| m.is(NodeKind::MethodDeclaration))?;
    Some((parent, method))
}

struct Settings<'a> {
    options: &'a BTreeMap<String, String>,
    use_this: bool,
    add_final: bool,
}

/// `QuickAssistProcessor.getAssignParamToFieldProposals` and
/// `getAssignAllParamsToFieldsProposals`.
pub async fn assign_param_to_field_proposals(env: &Env<'_>, ctx: &Context, proposals: &mut Vec<Proposal>) {
    let Some(covering) = ctx.covering_node() else { return };
    let node = normalized_node(covering);
    if parameter_declaration(node).is_none() {
        return;
    }
    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ctx.ast.uri) else { return };
    let options = env.options(&ctx.ast.uri).await;
    let decls_to_final = crate::features::preferences::add_final_for_new_declaration();
    let profile = accessors::profile(env.dispatcher, &uri).await;
    let settings = Settings { options: &options, use_this: profile.use_this, add_final: decls_to_final == "all" || decls_to_final == "fields" };
    assign_param_to_field(ctx, node, &settings, proposals);
    assign_all_params_to_fields(ctx, node, &settings, proposals);
}

fn assign_param_to_field(ctx: &Context, node: Node<'_>, settings: &Settings<'_>, out: &mut Vec<Proposal>) {
    let Some((param, method)) = parameter_declaration(node) else { return };
    if method.child("body").is_none() {
        return;
    }
    let Some(type_binding) = param.binding().and_then(|b| b.var_type()) else { return };
    let parent_type = binding_of_parent_type(node);
    if let Some(parent_type) = parent_type {
        if parent_type.is_interface() {
            return;
        }
        let is_static_context = is_in_static_context(node);
        for field in parent_type.declared_fields().unwrap_or_default() {
            let Some(field_type) = field.var_type() else { continue };
            if is_static_context == (field.modifiers() & modifier::STATIC != 0) && is_assignment_compatible(type_binding, field_type) {
                let fragment = field.declaring_node().filter(|n| n.is(NodeKind::VariableDeclarationFragment));
                if let Some(fragment) = fragment.filter(|f| f.child("initializer").is_none()) {
                    let name = fragment.child("name").map(|n| n.identifier()).unwrap_or_default();
                    let label = messages::format(messages::correction("AssignToVariableAssistProposal_assigntoexistingfield_description"), &[&name]);
                    if let Some(p) = field_proposal(ctx, vec![param], Some(fragment), label, relevance::ASSIGN_PARAM_TO_EXISTING_FIELD, false, settings) {
                        out.push(p);
                    }
                }
            }
        }
    }
    let label = messages::correction("AssignToVariableAssistProposal_assignparamtofield_description").to_owned();
    if let Some(p) = field_proposal(ctx, vec![param], None, label, relevance::ASSIGN_PARAM_TO_NEW_FIELD, settings.add_final, settings) {
        out.push(p);
    }
}

fn assign_all_params_to_fields(ctx: &Context, node: Node<'_>, settings: &Settings<'_>, out: &mut Vec<Proposal>) {
    let Some((_, method)) = parameter_declaration(node) else { return };
    if method.child("body").is_none() {
        return;
    }
    let parameters = method.list("parameters");
    if parameters.len() <= 1 {
        return;
    }
    if binding_of_parent_type(node).is_none_or(|t| t.is_interface()) {
        return;
    }
    if parameters.iter().any(|p| p.binding().and_then(|b| b.var_type()).is_none()) {
        return;
    }
    let label = messages::correction("AssignToVariableAssistProposal_assignallparamstofields_description").to_owned();
    if let Some(p) = field_proposal(ctx, parameters, None, label, relevance::ASSIGN_ALL_PARAMS_TO_NEW_FIELDS, settings.add_final, settings) {
        out.push(p);
    }
}

/// `AssignToVariableAssistProposalCore.getRewrite` for fields.
fn field_proposal(
    ctx: &Context,
    nodes: Vec<Node<'_>>,
    existing: Option<Node<'_>>,
    label: String,
    rank: i32,
    add_final: bool,
    settings: &Settings<'_>,
) -> Option<Proposal> {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), settings.options);
    let param_names: Option<Vec<String>> = (nodes.len() > 1).then(|| nodes.iter().filter_map(|n| n.child("name")).map(|n| n.identifier()).collect());
    for (i, node) in nodes.iter().enumerate() {
        let type_binding = node.binding()?.var_type()?;
        add_field(&mut rw, &mut imports, &ctx.ast, *node, type_binding, i, existing, add_final, param_names.as_deref(), &nodes, settings)?;
    }
    Some(Proposal::new(label, kind::QUICK_ASSIST, rank, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])))
}

#[allow(clippy::too_many_arguments)]
fn add_field(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    ast: &Arc<Ast>,
    node_to_assign: Node<'_>,
    type_binding: BindingRef<'_>,
    index: usize,
    existing: Option<Node<'_>>,
    add_final: bool,
    param_names: Option<&[String]>,
    nodes: &[Node<'_>],
    settings: &Settings<'_>,
) -> Option<()> {
    let new_type_decl = find_parent_type(node_to_assign)?;
    let expression = node_to_assign.child("name")?;
    let body_decl = find_parent_body_declaration(node_to_assign)?;
    let body = match body_decl.kind() {
        NodeKind::MethodDeclaration | NodeKind::Initializer => body_decl.child("body")?,
        _ => return None,
    };
    let is_anonymous = new_type_decl.is(NodeKind::AnonymousClassDeclaration);
    let is_static = body_decl.modifiers() & modifier::STATIC != 0 && !is_anonymous;
    let mut modifiers = modifier::PRIVATE;
    if add_final {
        modifiers |= modifier::FINAL;
    }
    if is_static {
        modifiers |= modifier::STATIC;
    }

    let var_name = match existing {
        Some(fragment) => fragment.child("name")?.identifier(),
        None => {
            let preference = if modifiers & modifier::STATIC != 0 { "staticField" } else { "field" };
            let mut used = used_names(node_to_assign);
            if let Some(names) = param_names {
                let mut remaining = names.to_vec();
                if let Some(i) = nodes.iter().position(|n| *n == node_to_assign) {
                    if i < remaining.len() {
                        remaining.remove(i);
                    }
                }
                used.extend(remaining);
            }
            let name = variable_name(type_binding, expression, preference, settings.options, &used);
            let fragment = rw.new_variable_declaration_fragment(&name, None);
            let context = import_context(ast, node_to_assign, settings.options);
            let typ = imports.add_import_type(type_binding, rw, &context, TypeLocation::Field);
            let flags = rw.new_modifiers(modifiers);
            let declaration = rw.new_field_declaration(fragment, flags, typ);
            let decls = new_type_decl.list("bodyDeclarations");
            let position = node_to_assign.start();
            let insert = decls
                .iter()
                .rposition(|d| d.is(NodeKind::FieldDeclaration) && position > d.end())
                .map_or(0, |i| i + 1)
                + index;
            rw.set_insert_bound_to_previous(declaration);
            rw.list_insert_at(RNode::Orig(new_type_decl.id), "bodyDeclarations", declaration, insert as i32);
            name
        }
    };

    let rhs = rw.create_copy_target(expression.id);
    let mut needs_this = settings.use_this;
    needs_this |= var_name == expression.identifier();
    let access_name = rw.new_simple_name(&var_name);
    let left = if needs_this {
        let qualifier = if is_static {
            rw.new_simple_name(&new_type_decl.child("name")?.identifier())
        } else {
            rw.new_this_expression()
        };
        rw.new_field_access(qualifier, access_name)
    } else {
        access_name
    };
    let assignment = rw.new_assignment(left, "=", rhs);
    let statement = rw.new_expression_statement(assignment);
    let statements = body.list("statements");
    let insert = find_assignment_insert_index(&statements, node_to_assign) + index;
    rw.list_insert_at(RNode::Orig(body.id), "statements", statement, insert as i32);
    Some(())
}

/// `AssignToVariableAssistProposalCore.findAssignmentInsertIndex`.
fn find_assignment_insert_index(statements: &[Node<'_>], node_to_assign: Node<'_>) -> usize {
    let mut params_before = HashSet::new();
    if let Some(method) = node_to_assign.parent() {
        for p in method.list("parameters") {
            if p == node_to_assign {
                break;
            }
            if let Some(name) = p.child("name") {
                params_before.insert(name.identifier());
            }
        }
    }
    for (i, statement) in statements.iter().enumerate() {
        match statement.kind() {
            NodeKind::ConstructorInvocation | NodeKind::SuperConstructorInvocation => {}
            NodeKind::ExpressionStatement => {
                if let Some(assignment) = statement.child("expression").filter(|e| e.is(NodeKind::Assignment)) {
                    let right = assignment.child("rightHandSide");
                    if right.is_some_and(|r| r.is(NodeKind::SimpleName) && params_before.contains(&r.identifier())) {
                        let assigned = assignment.child("leftHandSide").and_then(|l| l.binding()).filter(|b| b.is_variable());
                        if assigned.is_none_or(|b| b.is_field()) {
                            continue;
                        }
                    }
                }
                return i;
            }
            _ => return i,
        }
    }
    statements.len()
}
