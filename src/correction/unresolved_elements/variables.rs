//! The variable half of `UnresolvedElementsBaseSubProcessor`
//! (`collectVariableProposals`) and the similar type proposals it shares with
//! `collectTypeProposals`.

use std::collections::BTreeMap;

use super::new_variable::{NewVariable, VariableKind};
use super::types::{self, can_assign, declaration, type_label, well_known};
use super::{new_element, proposals, rename_proposal, scope, Units};
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_type, unparenthesed_expression};
use crate::semantic_ast::{BindingRef, Node, NodeKind};

/// `TypeKinds`.
pub mod type_kinds {
    pub const CLASSES: i32 = 1 << 1;
    pub const INTERFACES: i32 = 1 << 2;
    pub const ANNOTATIONS: i32 = 1 << 3;
    pub const ENUMS: i32 = 1 << 4;
    pub const VARIABLES: i32 = 1 << 5;
    pub const PRIMITIVETYPES: i32 = 1 << 6;
    pub const VOIDTYPE: i32 = 1 << 7;
    pub const REF_TYPES: i32 = CLASSES | INTERFACES | ENUMS | ANNOTATIONS;
    pub const REF_TYPES_AND_VAR: i32 = REF_TYPES | VARIABLES;
    pub const ALL_TYPES: i32 = PRIMITIVETYPES | REF_TYPES_AND_VAR;
}
use type_kinds as tk;

/// `Bindings.getBindingOfParentTypeContext(node)`.
fn parent_type_context_binding<'a>(node: Node<'a>) -> Option<BindingRef<'a>> {
    let mut last: Option<&'static str> = None;
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() {
            if matches!(last, Some("bodyDeclarations" | "javadoc")) || (x.is(NodeKind::EnumDeclaration) && last == Some("enumConstants")) {
                return x.binding();
            }
        } else if x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        last = x.location();
        n = x.parent();
    }
    None
}

/// `ASTResolving.findParentBodyDeclaration(node, true)`.
fn parent_body_declaration_treat_modifiers(node: Node<'_>) -> Option<Node<'_>> {
    let mut last: Option<&'static str> = None;
    let mut treat = true;
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_body_declaration() {
            if !treat || last != Some("modifiers") {
                return Some(x);
            }
            treat = false;
        }
        last = x.location();
        n = x.parent();
    }
    None
}

/// `ASTResolving.isInsideModifiers(node)`.
fn is_inside_modifiers(node: Node<'_>) -> bool {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_body_declaration() {
            return false;
        }
        if matches!(x.kind(), NodeKind::NormalAnnotation | NodeKind::MarkerAnnotation | NodeKind::SingleMemberAnnotation) {
            return true;
        }
        n = x.parent();
    }
    false
}

/// `ASTResolving.isWriteAccess(selectedNode)`.
fn is_write_access(node: Node<'_>) -> bool {
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        match parent.kind() {
            NodeKind::QualifiedName => {
                if curr.location_is("qualifier") {
                    return false;
                }
            }
            NodeKind::FieldAccess => {
                if curr.location_is("expression") {
                    return false;
                }
            }
            NodeKind::SuperFieldAccess => {}
            NodeKind::Assignment => return curr.location_is("leftHandSide"),
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => return curr.location_is("name"),
            NodeKind::PostfixExpression => return true,
            NodeKind::PrefixExpression => {
                let op = parent.simple("operator").unwrap_or("");
                return op == "--" || op == "++";
            }
            _ => return false,
        }
        curr = parent;
    }
    false
}

fn is_parent_switch_case(simple_name: Node<'_>) -> bool {
    simple_name.parent().is_some_and(|p| p.is(NodeKind::SwitchCase))
}

/// `StubUtility.hasPrefixOrSuffix` for the naming convention options.
fn has_prefix_or_suffix(options: &BTreeMap<String, String>, prefixes: &str, suffixes: &str, name: &str) -> bool {
    let list = |key: &str| -> Vec<String> {
        options
            .get(&format!("org.eclipse.jdt.core.codeComplete.{key}"))
            .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    };
    list(prefixes).iter().any(|p| name.starts_with(p.as_str())) || list(suffixes).iter().any(|s| name.ends_with(s.as_str()))
}

/// `NewVariableCorrectionProposalCore` wrapped by jdt.ls
/// (`newVariableCorrectionProposalToT` sets `addFinal`).
fn new_variable_proposal(ctx: &Context, label: String, kind: VariableKind, node: Node<'_>, sender: Option<BindingRef<'_>>, target: Option<String>, rel: i32) -> Proposal {
    let decls_to_final = crate::features::preferences::add_final_for_new_declaration();
    let add_final = decls_to_final == "all" || decls_to_final == "variables";
    new_element(
        label,
        rel,
        Box::new(NewVariable { source: ctx.ast.clone(), kind, node: node.id, sender: sender.map(|s| s.key().to_owned()), target_uri: target, add_final }),
    )
}

/// `UnresolvedElementsSubProcessor.getVariableProposals(context, problem, null, proposals)`.
pub async fn variable_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(selected) = problem.covered_node(ast) else { return };
    let Some(declaring_type) = parent_type_context_binding(selected) else { return };
    let mut suggest_variable_proposals = true;
    let mut type_kind = 0;
    let selected = unparenthesed_expression(selected);
    let mut binding: Option<BindingRef<'_>> = None;
    let mut node: Option<Node<'_>> = None;
    match selected.kind() {
        NodeKind::SimpleName => {
            node = Some(selected);
            let Some(parent) = selected.parent() else { return };
            if parent.is(NodeKind::ExpressionMethodReference) && selected.location_is("expression") {
                type_kind = tk::REF_TYPES;
            } else if parent.is(NodeKind::MethodInvocation) && selected.location_is("expression") {
                type_kind = tk::CLASSES | tk::INTERFACES | tk::ENUMS;
            } else if parent.is(NodeKind::FieldAccess) && selected.location_is("name") {
                if let Some(expression) = parent.child("expression") {
                    binding = expression.type_binding();
                    if binding.is_none() {
                        node = None;
                    }
                }
            } else if matches!(parent.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType) {
                suggest_variable_proposals = false;
                type_kind = tk::REF_TYPES_AND_VAR;
            } else if parent.is(NodeKind::QualifiedName) {
                if !selected.location_is("qualifier") {
                    binding = parent.child("qualifier").and_then(|q| q.type_binding());
                } else {
                    type_kind = tk::REF_TYPES;
                }
                let mut outer = parent.parent();
                while let Some(o) = outer.filter(|o| o.is(NodeKind::QualifiedName)) {
                    outer = o.parent();
                }
                if outer.is_some_and(|o| matches!(o.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType)) {
                    type_kind = tk::REF_TYPES;
                    suggest_variable_proposals = false;
                }
            } else if parent.is(NodeKind::SwitchCase) && selected.location_is("expression") {
                let switch_type = parent
                    .parent()
                    .filter(|s| matches!(s.kind(), NodeKind::SwitchStatement | NodeKind::SwitchExpression))
                    .and_then(|s| s.child("expression"))
                    .and_then(|e| e.type_binding());
                if let Some(t) = switch_type.filter(|t| t.is_enum()) {
                    binding = Some(t);
                }
            } else if parent.is(NodeKind::SuperFieldAccess) && selected.location_is("name") {
                binding = declaring_type.superclass();
            }
        }
        NodeKind::QualifiedName => {
            let qualifier = selected.child("qualifier");
            match qualifier.and_then(|q| q.type_binding()) {
                Some(q) => {
                    node = selected.child("name");
                    binding = Some(q);
                }
                None => {
                    node = qualifier;
                    type_kind = tk::REF_TYPES;
                    suggest_variable_proposals = qualifier.is_some_and(|q| q.is(NodeKind::SimpleName));
                }
            }
            if selected.parent().is_some_and(|p| matches!(p.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType)) {
                type_kind = tk::REF_TYPES;
                suggest_variable_proposals = false;
            }
        }
        NodeKind::FieldAccess => {
            if let Some(expression) = selected.child("expression") {
                binding = expression.type_binding();
                if binding.is_some() {
                    node = selected.child("name");
                }
            }
        }
        NodeKind::SuperFieldAccess => {
            binding = declaring_type.superclass();
            node = selected.child("name");
        }
        _ => {}
    }
    let Some(node) = node else { return };

    // add type proposals
    if type_kind != 0 {
        let identifier = simple_name_identifier(node);
        let rel = if identifier.chars().next().is_some_and(char::is_uppercase) { relevance::VARIABLE_TYPE_PROPOSAL_1 } else { relevance::VARIABLE_TYPE_PROPOSAL_2 };
        similar_type_proposals(env, ctx, type_kind, node, rel + 1, proposals).await;
        // collectNewTypeProposals (NewCUProposal) and the project setup fixes
        // are not ported.
    }

    if !suggest_variable_proposals {
        return;
    }

    let simple_name = if node.is(NodeKind::SimpleName) {
        node
    } else {
        match node.child("name") {
            Some(n) => n,
            None => return,
        }
    };
    let is_write = is_write_access(node);

    // similar variables
    similar_variable_proposals(ctx, binding, simple_name, is_write, proposals);

    if binding.is_none() {
        proposals::static_import_favorites(env, ctx, simple_name, false, proposals).await;
    }

    // resolvedField is always null here
    let options = env.options(&ast.uri).await;
    let units = Units::load(env, &ast.uri).await;
    new_field_proposals(ctx, &units, &options, binding, declaring_type, simple_name, is_write, proposals);
    if binding.is_none() && !is_parent_switch_case(simple_name) {
        new_variable_proposals(ctx, &options, node, simple_name, proposals);
    }
}

/// `ASTNodes.getSimpleNameIdentifier(name)`.
fn simple_name_identifier(node: Node<'_>) -> String {
    if node.is(NodeKind::QualifiedName) {
        node.child("name").map(|n| n.identifier()).unwrap_or_default()
    } else {
        node.identifier()
    }
}

/// `addNewVariableProposals`.
fn new_variable_proposals(ctx: &Context, options: &BTreeMap<String, String>, node: Node<'_>, simple_name: Node<'_>, proposals: &mut Vec<Proposal>) {
    let name = simple_name.identifier();
    let Some(body) = parent_body_declaration_treat_modifiers(node) else { return };
    let is_method = body.is(NodeKind::MethodDeclaration);
    if is_method {
        let rel = if has_prefix_or_suffix(options, "argumentPrefixes", "argumentSuffixes", &name) { relevance::CREATE_PARAMETER_PREFIX_OR_SUFFIX_MATCH } else { relevance::CREATE_PARAMETER };
        let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_createparameter_description"), &[&name]);
        proposals.push(new_variable_proposal(ctx, label, VariableKind::Param, simple_name, None, None, rel));
    }
    if body.is(NodeKind::Initializer) || (is_method && !scope::is_inside_constructor_invocation(body, node)) {
        let rel = if has_prefix_or_suffix(options, "localPrefixes", "localSuffixes", &name) { relevance::CREATE_LOCAL_PREFIX_OR_SUFFIX_MATCH } else { relevance::CREATE_LOCAL };
        let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_createlocal_description"), &[&name]);
        proposals.push(new_variable_proposal(ctx, label, VariableKind::Local, simple_name, None, None, rel));
    }

    if let Some(assignment) = node.parent().filter(|p| p.is(NodeKind::Assignment)) {
        if node.location_is("leftHandSide") {
            if let Some(statement) = assignment.parent().filter(|s| s.is(NodeKind::ExpressionStatement)) {
                let mut rw = ASTRewrite::new(ctx.ast.clone());
                if super::new_variable::is_control_statement_body(statement) {
                    let block = rw.new_block(Vec::new());
                    rw.replace(RNode::Orig(statement.id), Some(block));
                } else {
                    rw.remove(RNode::Orig(statement.id));
                }
                let label = messages::correction("UnresolvedElementsSubProcessor_removestatement_description").to_owned();
                proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::REMOVE_ASSIGNMENT, rw));
            }
        }
    }
}

/// `addNewFieldProposals`.
#[allow(clippy::too_many_arguments)]
fn new_field_proposals(
    ctx: &Context,
    units: &Units,
    options: &BTreeMap<String, String>,
    binding: Option<BindingRef<'_>>,
    declaring_type: BindingRef<'_>,
    simple_name: Node<'_>,
    is_write: bool,
    proposals: &mut Vec<Proposal>,
) {
    let ast = ctx.ast();
    let (sender_decl, target) = match binding {
        Some(b) => {
            let d = declaration(b);
            (d, units.find(ast, d))
        }
        None => (declaring_type, Some(None)),
    };
    let Some(target) = target else { return };
    if !sender_decl.is_from_source() {
        return;
    }
    let must_be_const = is_inside_modifiers(simple_name) || is_parent_switch_case(simple_name);
    new_field_for_type(ctx, options, &target, binding, sender_decl, simple_name, is_write, must_be_const, proposals);

    if binding.is_none() && sender_decl.is_nested() {
        if let Some(anonym) = sender_decl.declaring_node().filter(|n| std::ptr::eq(n.ast, ast)) {
            if let Some(bind) = anonym.parent().and_then(types::parent_type_binding) {
                if !bind.is_anonymous() {
                    new_field_for_type(ctx, options, &target, Some(bind), bind, simple_name, is_write, must_be_const, proposals);
                }
            }
        }
    }
}

/// `addNewFieldForType`.
#[allow(clippy::too_many_arguments)]
fn new_field_for_type(
    ctx: &Context,
    options: &BTreeMap<String, String>,
    target: &Option<String>,
    binding: Option<BindingRef<'_>>,
    sender: BindingRef<'_>,
    simple_name: Node<'_>,
    is_write: bool,
    must_be_const: bool,
    proposals: &mut Vec<Proposal>,
) {
    let name = simple_name.identifier();
    let mut create_enum = false;
    if sender.is_enum() && !is_write {
        create_enum = true;
        let parent = simple_name.parent();
        if let Some(ret) = parent.filter(|p| p.is(NodeKind::ReturnStatement)) {
            create_enum = false;
            if let Some(method) = ret.ancestors().find(|a| a.is(NodeKind::MethodDeclaration)) {
                if method.binding().and_then(|m| m.return_type()).is_some_and(|r| r == sender) {
                    create_enum = true;
                }
            }
        } else if let Some(infix) = parent.filter(|p| p.is(NodeKind::InfixExpression)) {
            create_enum = false;
            let op = infix.simple("operator").unwrap_or("");
            if op == "==" || op == "!=" {
                let other = if simple_name.location_is("leftOperand") { infix.child("rightOperand") } else { infix.child("leftOperand") };
                if let Some(other) = other.filter(|o| o.is(NodeKind::SimpleName)) {
                    if let Some(v) = other.binding().filter(|b| b.is_variable()) {
                        if !v.is_enum_constant() && v.var_type().is_some_and(|t| t == sender) {
                            create_enum = true;
                        }
                    }
                }
            }
        } else if simple_name.location_is("initializer") && parent.is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) {
            create_enum = false;
            if parent.and_then(|f| f.binding()).and_then(|b| b.var_type()).is_some_and(|t| t == sender) {
                create_enum = true;
            }
        } else if simple_name.location_is("rightHandSide") && parent.is_some_and(|p| p.is(NodeKind::Assignment)) {
            create_enum = false;
            if parent.and_then(|a| a.child("leftHandSide")).and_then(|l| l.type_binding()).is_some_and(|t| t == sender) {
                create_enum = true;
            }
        }
        if create_enum {
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_createenum_description"), &[&name, &type_label(sender)]);
            proposals.push(new_variable_proposal(ctx, label, VariableKind::EnumConst, simple_name, Some(sender), target.clone(), 10));
        }
    }
    if !create_enum && !must_be_const {
        let rel = if has_prefix_or_suffix(options, "fieldPrefixes", "fieldSuffixes", &name) { relevance::CREATE_FIELD_PREFIX_OR_SUFFIX_MATCH } else { relevance::CREATE_FIELD };
        let label = match binding {
            None => messages::format(messages::correction("UnresolvedElementsSubProcessor_createfield_description"), &[&name]),
            Some(_) => messages::format(messages::correction("UnresolvedElementsSubProcessor_createfield_other_description"), &[&name, &type_label(sender)]),
        };
        proposals.push(new_variable_proposal(ctx, label, VariableKind::Field, simple_name, Some(sender), target.clone(), rel));
    }
    if !create_enum && !is_write && !sender.is_anonymous() {
        let label = match binding {
            None => messages::format(messages::correction("UnresolvedElementsSubProcessor_createconst_description"), &[&name]),
            Some(_) => messages::format(messages::correction("UnresolvedElementsSubProcessor_createconst_other_description"), &[&name, &type_label(sender)]),
        };
        let rel = if has_prefix_or_suffix(options, "staticFinalFieldPrefixes", "staticFinalFieldSuffixes", &name) { relevance::CREATE_CONSTANT_PREFIX_OR_SUFFIX_MATCH } else { relevance::CREATE_CONSTANT };
        proposals.push(new_variable_proposal(ctx, label, VariableKind::ConstField, simple_name, Some(sender), target.clone(), rel));
    }
}

/// `hasMethodWithName` (upstream looks at the declared *fields*).
fn has_method_with_name(t: BindingRef<'_>, name: &str) -> bool {
    if t.declared_fields().unwrap_or_default().iter().any(|f| f.name() == name) {
        return true;
    }
    t.superclass().is_some_and(|s| has_method_with_name(s, name))
}

/// `hasFieldWithName` (upstream looks at the declared *methods*, then
/// recurses with `hasMethodWithName`).
fn has_field_with_name(t: BindingRef<'_>, name: &str) -> bool {
    if t.declared_methods().unwrap_or_default().iter().any(|m| m.name() == name) {
        return true;
    }
    t.superclass().is_some_and(|s| has_method_with_name(s, name))
}

/// `addSimilarVariableProposals` (with `resolvedField == null`).
fn similar_variable_proposals(ctx: &Context, binding: Option<BindingRef<'_>>, node: Node<'_>, is_write: bool, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let in_scope = scope::declarations_in_scope(node, !is_write);
    if !in_scope.is_empty() {
        let mut other_name_in_assign: Option<String> = None;
        let mut method_sender_name: Option<String> = None;
        let mut field_sender_name: Option<String> = None;
        if let Some(parent) = node.parent() {
            match parent.kind() {
                NodeKind::VariableDeclarationFragment => other_name_in_assign = parent.child("name").map(|n| n.identifier()),
                NodeKind::Assignment => {
                    if is_write {
                        if let Some(r) = parent.child("rightHandSide").filter(|r| r.is(NodeKind::SimpleName)) {
                            other_name_in_assign = Some(r.identifier());
                        }
                    } else if let Some(l) = parent.child("leftHandSide").filter(|l| l.is(NodeKind::SimpleName)) {
                        other_name_in_assign = Some(l.identifier());
                    }
                }
                NodeKind::MethodInvocation => {
                    if node.location_is("expression") {
                        method_sender_name = parent.child("name").map(|n| n.identifier());
                    }
                }
                NodeKind::QualifiedName => {
                    if node.location_is("qualifier") {
                        field_sender_name = parent.child("name").map(|n| n.identifier());
                    }
                }
                _ => {}
            }
        }
        let guessed = scope::guess_binding_for_reference(node);
        let object = well_known(ast, "java.lang.Object");
        let identifier = node.identifier();
        let is_static_context = scope::is_in_static_context(node);
        let mut new_proposals = Vec::new();
        for curr in in_scope {
            if new_proposals.len() > 50 {
                break;
            }
            if curr.is_variable() {
                let curr_name = curr.name();
                if other_name_in_assign.as_deref() == Some(curr_name) {
                    continue;
                }
                let is_final = curr.modifiers() & crate::semantic_ast::modifier::FINAL != 0;
                if is_final && curr.is_field() && is_write {
                    continue;
                }
                if is_static_context && curr.modifiers() & crate::semantic_ast::modifier::STATIC == 0 && curr.is_field() {
                    continue;
                }
                let mut rel = relevance::SIMILAR_VARIABLE_PROPOSAL;
                if scope::is_similar_name(curr_name, &identifier) {
                    rel += 3;
                }
                if curr_name.to_lowercase() == identifier.to_lowercase() {
                    rel += 5;
                }
                if let Some(var_type) = curr.var_type() {
                    if let Some(g) = guessed.filter(|g| Some(*g) != object) {
                        if (!is_write && can_assign(var_type, g)) || (is_write && can_assign(g, var_type)) {
                            rel += 2;
                        }
                    }
                    if method_sender_name.as_deref().is_some_and(|m| has_method_with_name(var_type, m)) {
                        rel += 2;
                    }
                    if field_sender_name.as_deref().is_some_and(|f| has_field_with_name(var_type, f)) {
                        rel += 2;
                    }
                }
                if rel > 0 {
                    let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_changevariable_description"), &[curr_name]);
                    new_proposals.push(rename_proposal(ctx, label, node.start(), node.length(), curr_name, rel));
                }
            } else if curr.is_method() {
                let Some(g) = guessed else { continue };
                if !curr.is_constructor() && curr.return_type().is_some_and(|r| can_assign(r, g)) && scope::is_similar_name(curr.name(), &identifier) {
                    let mut rw = ASTRewrite::new(ctx.ast.clone());
                    let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_changetomethod_description"), &[&types::method_label(curr)]);
                    let args = curr.parameter_types().into_iter().map(|t| proposals::default_expression(&mut rw, t)).collect();
                    let inv = rw.new_method_invocation(None, curr.name(), args);
                    rw.replace(RNode::Orig(node.id), Some(inv));
                    new_proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::CHANGE_TO_METHOD, rw));
                }
            }
        }
        if new_proposals.len() <= 50 {
            proposals.extend(new_proposals);
        }
    }
    if binding.is_some_and(|b| b.is_array()) {
        let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_changevariable_description"), &["length"]);
        proposals.push(rename_proposal(ctx, label, node.start(), node.length(), "length", relevance::CHANGE_VARIABLE));
    }
}

// ─── Similar types ──────────────────────────────────────────────────────────

/// `ASTResolving.guessBindingForTypeReference(node)`.
fn guess_binding_for_type_reference(node: Node<'_>) -> Option<BindingRef<'_>> {
    if node.location_is("qualifier") && node.parent().is_some_and(|p| p.is(NodeKind::QualifiedName)) {
        return None;
    }
    let mut node = node;
    if node.location_is("name") && node.parent().is_some_and(|p| matches!(p.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType)) {
        node = node.parent().unwrap();
    }
    let binding = types::normalize(possible_type_binding(node));
    if let Some(b) = binding.filter(|b| b.is_wildcard_type()) {
        return types::normalize_wildcard(b, true);
    }
    binding
}

/// `ASTResolving.guessVariableType(fragments)`.
fn guess_variable_type<'a>(fragments: Vec<Node<'a>>) -> Option<BindingRef<'a>> {
    for f in fragments {
        if let Some(i) = f.child("initializer") {
            return types::normalize(i.type_binding());
        }
    }
    None
}

/// `ASTResolving.getPossibleTypeBinding(node)`.
fn possible_type_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    let parent = node.parent()?;
    match parent.kind() {
        NodeKind::ArrayType => {
            let dim = parent.list("dimensions").len() as i32;
            let p = possible_type_binding(parent)?;
            if p.dimensions() == dim {
                return p.element_type();
            }
            None
        }
        NodeKind::ParameterizedType => {
            let p = possible_type_binding(parent).filter(|p| p.is_parameterized_type())?;
            if node.location_is("type") {
                return Some(p);
            }
            let args = p.type_arguments();
            let nodes = parent.list("typeArguments");
            let index = nodes.iter().position(|n| *n == node)?;
            if args.len() == nodes.len() {
                return args.get(index).copied();
            }
            None
        }
        NodeKind::WildcardType => {
            let p = possible_type_binding(parent).filter(|p| p.is_wildcard_type())?;
            if types::is_upperbound(p) == parent.flag("upperBound") {
                return p.bound();
            }
            None
        }
        NodeKind::QualifiedType | NodeKind::NameQualifiedType => {
            let p = possible_type_binding(parent).filter(|p| p.is_member())?;
            if node.location_is("qualifier") {
                return p.declaring_class();
            }
            Some(p)
        }
        NodeKind::VariableDeclarationStatement | NodeKind::FieldDeclaration | NodeKind::VariableDeclarationExpression => guess_variable_type(parent.list("fragments")),
        NodeKind::SingleVariableDeclaration => parent.child("initializer").and_then(|i| types::normalize(i.type_binding())),
        NodeKind::ArrayCreation => {
            if let Some(i) = parent.child("initializer") {
                return i.type_binding();
            }
            scope::guess_binding_for_reference(parent)
        }
        NodeKind::TypeLiteral => parent.child("type").and_then(|t| t.binding()),
        NodeKind::ClassInstanceCreation | NodeKind::CastExpression => scope::guess_binding_for_reference(parent),
        _ => None,
    }
}

/// `SimilarElementsRequestor.findSimilarElement(cu, name, kind)`: the type
/// names (with their kinds) completion proposes at the name.
async fn find_similar_types(env: &Env<'_>, ctx: &Context, node: Node<'_>, kind: i32) -> Vec<(i32, String)> {
    let ast = ctx.ast();
    let identifier = simple_name_identifier(node);
    let pos = if node.is(NodeKind::QualifiedName) {
        node.child("name").map(|n| n.start()).unwrap_or(node.start())
    } else {
        node.start() + 1
    };
    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ast.uri) else { return Vec::new() };
    let mut completion_ctx = env.dispatcher.context_for(Some(&uri)).await;
    completion_ctx.files.insert(uri.to_string(), ast.text().to_owned());
    let Ok(result) = env.dispatcher.code_assist(&completion_ctx, uri.as_str(), pos, serde_json::json!({ "op": "complete" })).await else {
        return Vec::new();
    };
    let Ok(raw) = serde_json::from_value::<crate::features::completion::proposal::EngineResult>(result) else { return Vec::new() };
    let filter = crate::features::completion::requestor::TypeFilter::new(&crate::features::completion::prefs::Prefs::load().filtered_types, &[]);
    use crate::features::completion::proposal::{flags, kind as ck};
    let mut result: Vec<(i32, String)> = Vec::new();
    for p in raw.proposals.iter().filter(|p| p.kind == ck::TYPE_REF) {
        let sig = p.signature();
        let k = if sig.starts_with('T') {
            tk::VARIABLES
        } else if flags::is(p.flags, flags::ANNOTATION) {
            tk::ANNOTATIONS
        } else if flags::is(p.flags, flags::INTERFACE) {
            tk::INTERFACES
        } else if flags::is(p.flags, flags::ENUM) {
            tk::ENUMS
        } else {
            tk::CLASSES
        };
        if kind & k == 0 {
            continue;
        }
        let Ok(full_name) = crate::features::completion::signature::to_string(&erasure_signature(sig)) else { continue };
        if filter.is_filtered(&full_name) {
            continue;
        }
        let simple = full_name.rsplit('.').next().unwrap_or(&full_name);
        if scope::is_similar_name(&identifier, simple) && !result.iter().any(|(rk, n)| *rk == k && *n == full_name) {
            result.push((k, full_name));
        }
    }
    // processKeywords
    if kind & tk::PRIMITIVETYPES != 0 {
        for t in ["boolean", "byte", "char", "short", "int", "long", "float", "double"] {
            if scope::is_similar_name(&identifier, t) {
                result.push((tk::PRIMITIVETYPES, t.to_owned()));
            }
        }
    }
    if kind & tk::VOIDTYPE != 0 && scope::is_similar_name(&identifier, "void") {
        result.push((tk::PRIMITIVETYPES, "void".to_owned()));
    }
    result
}

/// `Signature.getTypeErasure(signature)`.
fn erasure_signature(sig: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in sig.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `createTypeRefChangeProposal(cu, fullName, node, relevance, maxProposals)`.
fn type_ref_change_proposal(ctx: &Context, options: &BTreeMap<String, String>, full_name: &str, node: Node<'_>, relevance: i32) -> Option<Proposal> {
    let mut relevance = relevance;
    let (pack_name, mut simple_name) = match full_name.rsplit_once('.') {
        Some((q, s)) => (q.to_owned(), s.to_owned()),
        None => (String::new(), full_name.to_owned()),
    };
    let mut imports = None;
    if !pack_name.is_empty() {
        let mut ir = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
        let scope_node = crate::semantic_ast::resolve::find_parent_body_declaration(node).unwrap_or(node);
        let context = ConstructorImportContext { ast: ctx.ast.clone(), declaration: find_parent_type(scope_node).map(|n| n.id), nullness: None };
        simple_name = ir.add_import(full_name, &context);
        imports = Some(ir);
    }
    if !simple_name.chars().next().is_some_and(char::is_uppercase) {
        relevance -= 2;
    }
    if imports.is_some() && node.is(NodeKind::SimpleName) && simple_name == node.identifier() {
        // import only (AddImportCorrectionProposal / QualifyTypeProposal): not ported
        return None;
    }
    let label = if pack_name.is_empty() {
        messages::format(messages::correction("UnresolvedElementsSubProcessor_changetype_nopack_description"), &[&simple_name])
    } else {
        messages::format(messages::correction("UnresolvedElementsSubProcessor_changetype_description"), &[&simple_name, &pack_name])
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let placeholder = rw.create_string_placeholder(&simple_name, NodeKind::SimpleType);
    rw.replace(RNode::Orig(node.id), Some(placeholder));
    let mut cu = CuChange::rewrite(rw);
    if let Some(i) = imports {
        cu = cu.with_imports(i);
    }
    Some(Proposal::new(label, kind::QUICK_FIX, relevance, Change::Cu(vec![cu])))
}

/// `addSimilarTypeProposals(kind, cu, node, relevance, proposals)`.
pub async fn similar_type_proposals(env: &Env<'_>, ctx: &Context, kind: i32, node: Node<'_>, relevance: i32, proposals: &mut Vec<Proposal>) {
    let options = env.options(&ctx.ast.uri).await;
    let elements = find_similar_types(env, ctx, node, kind).await;
    let mut resolved_type_name: Option<String> = None;
    let mut simple_binding: Option<BindingRef<'_>> = None;
    if let Some(binding) = guess_binding_for_type_reference(node) {
        let mut sb = binding;
        if sb.is_array() {
            sb = sb.element_type().unwrap_or(sb);
        }
        let sb = declaration(sb);
        simple_binding = Some(sb);
        if !sb.is_recovered() {
            let name = sb.qualified_name().to_owned();
            if let Some(p) = type_ref_change_proposal(ctx, &options, &name, node, relevance + 2) {
                proposals.push(p);
            }
            resolved_type_name = Some(name);
            // createTypeRefChangeFullProposal for parameterized types: not ported
        }
    }
    for (k, full_name) in &elements {
        if k & tk::ALL_TYPES == 0 || resolved_type_name.as_deref() == Some(full_name.as_str()) {
            continue;
        }
        if let Some(sb) = simple_binding.filter(|b| !b.is_primitive() && !b.is_recovered()) {
            // The element must inherit from the expected type. Its binding is
            // only known when the unit's AST references it.
            let key = format!("L{};", full_name.replace('.', "/"));
            if let Some(q) = ctx.ast().binding_by_key(&key) {
                if q.name() != sb.name() && !q.is_generic_type() && !q.is_parameterized_type() && !q.is_wildcard_type() && !q.is_raw_type() && !q.is_record() && !q.is_recovered() && !is_inherited(Some(q), sb) {
                    continue;
                }
            }
        }
        if let Some(p) = type_ref_change_proposal(ctx, &options, full_name, node, relevance) {
            proposals.push(p);
        }
    }
}

/// `isInherited(binding, ancestorBinding)`.
fn is_inherited(binding: Option<BindingRef<'_>>, ancestor: BindingRef<'_>) -> bool {
    let Some(b) = binding else { return false };
    if b == ancestor {
        return true;
    }
    if b.interfaces().into_iter().any(|i| is_inherited(Some(i), ancestor)) {
        return true;
    }
    is_inherited(b.superclass(), ancestor)
}

/// `UnresolvedElementsSubProcessor.getTypeProposals` (the similar type
/// proposals only; new types, imports and module fixes are not ported).
pub async fn type_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(mut selected) = problem.covering_node(ast) else { return };
    while selected.is(NodeKind::ParenthesizedExpression) {
        match selected.child("expression") {
            Some(e) => selected = e,
            None => return,
        }
    }
    let kind = possible_type_kinds(selected);
    if selected.is(NodeKind::CastExpression) {
        if let Some(t) = selected.child("type") {
            selected = t;
        }
    }
    if selected.is(NodeKind::ParameterizedType) {
        if let Some(t) = selected.child("type") {
            selected = t;
        }
    }
    while selected.location_is("name") && selected.parent().is_some_and(|p| p.is(NodeKind::QualifiedName)) {
        selected = selected.parent().unwrap();
    }
    let node = match selected.kind() {
        NodeKind::SimpleType | NodeKind::NameQualifiedType => selected.child("name"),
        NodeKind::ArrayType => {
            let element = selected.child("elementType");
            match element {
                Some(e) if matches!(e.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType) => e.child("name"),
                _ => return,
            }
        }
        k if k.is_name() => Some(selected),
        _ => return,
    };
    let Some(node) = node else { return };
    // addEnhancedForWithoutTypeProposals and the `var` compliance fixes are not ported.
    similar_type_proposals(env, ctx, kind, node, relevance::SIMILAR_TYPE, proposals).await;
}

/// `ASTResolving.getPossibleTypeKinds(node)`.
fn possible_type_kinds(node: Node<'_>) -> i32 {
    let kind = tk::ALL_TYPES;
    let mut mask = tk::ALL_TYPES | tk::VOIDTYPE;
    let mut node = node;
    let mut parent = node.parent();
    while let Some(p) = parent.filter(|p| p.is(NodeKind::QualifiedName)) {
        if node.location_is("qualifier") {
            return tk::REF_TYPES;
        }
        node = p;
        parent = p.parent();
        mask = tk::REF_TYPES;
    }
    while let Some(p) = parent.filter(|p| p.kind().is_type()) {
        match p.kind() {
            NodeKind::QualifiedType | NodeKind::NameQualifiedType => {
                if node.location_is("qualifier") {
                    return mask & tk::REF_TYPES;
                }
                mask &= tk::REF_TYPES;
            }
            NodeKind::ParameterizedType => {
                if node.location_is("typeArguments") {
                    return mask & tk::REF_TYPES_AND_VAR;
                }
                mask &= tk::CLASSES | tk::INTERFACES;
            }
            NodeKind::WildcardType => {
                if node.location_is("bound") {
                    return mask & tk::REF_TYPES_AND_VAR;
                }
            }
            _ => {}
        }
        node = p;
        parent = p.parent();
    }
    let kind = match parent {
        Some(p) => match p.kind() {
            NodeKind::TypeDeclaration => {
                if node.location_is("superInterfaceTypes") {
                    tk::INTERFACES
                } else if node.location_is("superclassType") {
                    tk::CLASSES
                } else if node.location_is("permittedTypes") {
                    if p.flag("interface") { tk::CLASSES | tk::INTERFACES } else { tk::CLASSES }
                } else {
                    kind
                }
            }
            NodeKind::EnumDeclaration => tk::INTERFACES,
            NodeKind::MethodDeclaration => {
                if node.location_is("thrownExceptionTypes") {
                    tk::CLASSES
                } else if node.location_is("returnType2") {
                    tk::ALL_TYPES | tk::VOIDTYPE
                } else {
                    kind
                }
            }
            NodeKind::AnnotationTypeMemberDeclaration => tk::PRIMITIVETYPES | tk::ANNOTATIONS | tk::ENUMS,
            NodeKind::InstanceofExpression => tk::REF_TYPES,
            NodeKind::ThrowStatement => tk::CLASSES,
            NodeKind::ClassInstanceCreation => {
                if p.child("anonymousClassDeclaration").is_none() { tk::CLASSES } else { tk::CLASSES | tk::INTERFACES }
            }
            NodeKind::SingleVariableDeclaration => match p.parent().map(|g| g.kind()) {
                Some(NodeKind::CatchClause) => tk::CLASSES,
                Some(NodeKind::EnhancedForStatement) => tk::REF_TYPES,
                _ => kind,
            },
            NodeKind::TagElement => tk::REF_TYPES,
            NodeKind::MarkerAnnotation | NodeKind::SingleMemberAnnotation | NodeKind::NormalAnnotation => tk::ANNOTATIONS,
            NodeKind::TypeParameter => {
                if p.list("typeBounds").iter().position(|b| *b == node).is_some_and(|i| i > 0) { tk::INTERFACES } else { tk::REF_TYPES_AND_VAR }
            }
            NodeKind::TypeLiteral => tk::REF_TYPES,
            _ => kind,
        },
        None => kind,
    };
    kind & mask
}
