//! `RefactorProposalUtility.getMoveRefactoringProposals`.

use serde_json::{json, Value};
use tower_lsp::lsp_types::CodeActionParams;

use super::edit::Env;
use super::{kind, messages, relevance, Context, Proposal};
use crate::semantic_ast::{modifier, BindingRef, Node, NodeKind};

const APPLY_REFACTORING_COMMAND_ID: &str = "java.action.applyRefactoringCommand";

/// `getDeclarationNode(node, alwaysShowMove)`.
fn declaration_node(node: Option<Node<'_>>, always_show_move: bool) -> Option<Node<'_>> {
    let mut node = node?;
    if always_show_move {
        loop {
            if node.kind().is_body_declaration() {
                return Some(node);
            }
            node = node.parent()?;
        }
    }
    if node.kind().is_body_declaration() {
        return None;
    }
    loop {
        if node.kind().is_body_declaration() || node.kind().is_statement() {
            return Some(node);
        }
        node = node.parent()?;
    }
}

fn display_name(declaration: Node<'_>) -> Option<String> {
    match declaration.kind() {
        NodeKind::MethodDeclaration => {
            let method = declaration.binding()?;
            let parameters: Vec<&str> = method.parameter_types().iter().map(|t| t.name()).collect();
            Some(format!("{}({})", method.name(), parameters.join(",")))
        }
        NodeKind::FieldDeclaration => {
            let names: Vec<String> = declaration.list("fragments").iter().filter_map(|f| f.binding().map(|b| b.name().to_owned())).collect();
            Some(names.join(","))
        }
        k if k.is_abstract_type_declaration() => Some(declaration.binding()?.name().to_owned()),
        _ => None,
    }
}

/// `ASTNodes.getEnclosingType(declaration.getParent())`'s qualified name.
fn enclosing_type_name(declaration: Node<'_>) -> Option<String> {
    let mut node = declaration.parent();
    while let Some(n) = node {
        if n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration) {
            return n.binding().map(|b| b.qualified_name().to_owned());
        }
        node = n.parent();
    }
    None
}

/// `JdtFlags.isStatic(member)` for a field, method or type binding.
fn is_static(member: BindingRef<'_>) -> bool {
    let declaring = member.declaring_class();
    if member.is_type() && (member.is_interface() || member.is_annotation()) && declaring.is_some() {
        return true;
    }
    if !member.is_method() && declaring.is_some_and(|d| d.is_interface() || d.is_annotation()) {
        return true;
    }
    if member.is_enum() && !member.is_method() {
        return true;
    }
    if member.is_record() && member.is_type() && declaring.is_some() {
        return true;
    }
    member.modifiers() & modifier::STATIC != 0
}

/// `RefactoringAvailabilityTesterCore.isMoveStaticAvailable(member)`.
fn move_static_available(member: BindingRef<'_>) -> bool {
    if member.is_variable() && member.is_enum_constant() {
        return false;
    }
    let Some(declaring) = member.declaring_class() else { return false };
    if !declaring.is_from_source() {
        return false;
    }
    if member.is_method() {
        if member.is_constructor() || !is_static(member) {
            return false;
        }
        return true;
    }
    if member.is_type() && !is_static(member) {
        return false;
    }
    declaring.is_interface() || is_static(member)
}

fn move_static_member_available(declaration: Node<'_>) -> bool {
    match declaration.kind() {
        NodeKind::MethodDeclaration => declaration.binding().is_some_and(move_static_available),
        NodeKind::FieldDeclaration => {
            let members: Vec<BindingRef<'_>> = declaration.list("fragments").iter().filter_map(|f| f.binding()).collect();
            if members.is_empty() || !members.iter().all(|m| move_static_available(*m)) {
                return false;
            }
            members.iter().all(|m| m.declaring_class() == members[0].declaring_class())
        }
        k if k.is_abstract_type_declaration() => declaration.binding().is_some_and(move_static_available),
        _ => false,
    }
}

/// `RefactoringAvailabilityTesterCore.isMoveMethodAvailable`.
fn move_method_available(declaration: Node<'_>) -> bool {
    let Some(method) = declaration.binding() else { return false };
    !method.is_constructor()
        && !is_static(method)
        && declaration.binding().and_then(|b| b.declaring_class()).is_some_and(|d| d.is_from_source())
        && (method.modifiers() & modifier::DEFAULT != 0 || !method.declaring_class().is_some_and(|d| d.is_interface()))
}

/// `RefactoringAvailabilityTesterCore.isMoveInnerAvailable` (`JavaElementUtil.isMainType`).
fn move_inner_available(declaration: Node<'_>) -> bool {
    let Some(binding) = declaration.binding() else { return false };
    if binding.is_anonymous() {
        return false;
    }
    let mut current = Some(binding);
    while let Some(t) = current {
        if t.is_local() {
            return false;
        }
        current = t.declaring_class();
    }
    if binding.declaring_class().is_none() {
        let root = declaration.root();
        let types = root.list("types");
        let file_name = declaration.ast.uri.rsplit('/').next().unwrap_or("").trim_end_matches(".java");
        let primary = types.iter().find(|t| t.child("name").is_some_and(|n| n.identifier() == file_name));
        let is_primary = primary.is_some_and(|p| p.id == declaration.id);
        if is_primary || types.len() == 1 {
            return false;
        }
    }
    true
}

pub async fn move_refactoring_proposals(env: &Env<'_>, ctx: &Context, params: &CodeActionParams, out: &mut Vec<Proposal>) {
    let always_show_move = params.context.only.as_ref().is_some_and(|only| only.iter().any(|k| k.as_str() == kind::REFACTOR));
    let node = ctx.covered_node().or_else(|| ctx.covering_node());
    let node = declaration_node(node, always_show_move);
    let uri = ctx.ast.uri.clone();
    let args = |command: &str, info: Value| vec![json!(command), serde_json::to_value(params).expect("serializable code action parameters"), info];
    let Some(node) = node else {
        if always_show_move {
            let label = messages::ls_action("MoveRefactoringAction_label");
            out.push(Proposal::command(label, kind::REFACTOR_MOVE, relevance::MOVE_REFACTORING, APPLY_REFACTORING_COMMAND_ID, args("moveFile", json!({ "uri": uri }))));
        }
        return;
    };
    if !(matches!(node.kind(), NodeKind::MethodDeclaration | NodeKind::FieldDeclaration) || node.kind().is_abstract_type_declaration()) {
        return;
    }
    let name = display_name(node);
    let label = if always_show_move {
        messages::ls_action("MoveRefactoringAction_label").to_owned()
    } else {
        messages::format(messages::ls_action("MoveRefactoringAction_templateLabel"), &[name.as_deref().unwrap_or("null")])
    };
    let member_type = node.kind().node_type();
    let enclosing = enclosing_type_name(node);
    let project_name = project_name(env, &uri);
    let member_info = |member_type: u32, kinds: Option<Vec<&str>>| {
        let mut info = json!({ "displayName": name, "memberType": member_type, "projectName": project_name });
        if let Some(e) = &enclosing {
            info["enclosingTypeName"] = json!(e);
        }
        if let Some(k) = kinds {
            info["supportedDestinationKinds"] = json!(k);
        }
        info
    };
    if node.kind().is_abstract_type_declaration() {
        let mut kinds = Vec::new();
        if move_inner_available(node) {
            kinds.push("newFile");
        }
        if move_static_member_available(node) {
            kinds.push("class");
        }
        if !kinds.is_empty() {
            let info = member_info(NodeKind::TypeDeclaration.node_type(), Some(kinds));
            out.push(Proposal::command(label, kind::REFACTOR_MOVE, relevance::MOVE_REFACTORING, APPLY_REFACTORING_COMMAND_ID, args("moveType", info)));
            return;
        }
        out.push(Proposal::command(label, kind::REFACTOR_MOVE, relevance::MOVE_REFACTORING, APPLY_REFACTORING_COMMAND_ID, args("moveFile", json!({ "uri": uri }))));
    } else if node.modifiers() & modifier::STATIC != 0 || is_static_declaration(node) {
        if move_static_member_available(node) {
            let info = member_info(member_type, None);
            out.push(Proposal::command(label, kind::REFACTOR_MOVE, relevance::MOVE_REFACTORING, APPLY_REFACTORING_COMMAND_ID, args("moveStaticMember", info)));
        }
    } else if node.is(NodeKind::MethodDeclaration) && move_method_available(node) {
        let info = json!({ "displayName": name, "memberType": 0 });
        out.push(Proposal::command(label, kind::REFACTOR_MOVE, relevance::MOVE_REFACTORING, APPLY_REFACTORING_COMMAND_ID, args("moveInstanceMethod", info)));
    }
}

/// `JdtFlags.isStatic((BodyDeclaration) node)`.
fn is_static_declaration(node: Node<'_>) -> bool {
    if node.is(NodeKind::FieldDeclaration) {
        return node.parent().and_then(|p| p.binding()).is_some_and(|d| d.is_interface() || d.is_annotation());
    }
    false
}

fn project_name(env: &Env<'_>, uri: &str) -> String {
    let workspace = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
    tower_lsp::lsp_types::Url::parse(uri)
        .ok()
        .and_then(|u| workspace.project_for_uri(&u).map(|p| p.name.clone()))
        .unwrap_or_else(|| crate::project::DEFAULT_PROJECT_NAME.to_owned())
}
