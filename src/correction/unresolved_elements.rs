//! Port of `UnresolvedElementsBaseSubProcessor` (jdt.core.manipulation) as
//! wrapped by jdt.ls `UnresolvedElementsSubProcessor`: the method,
//! constructor, argument, annotation member and array access proposals.

mod names;
mod new_method;
mod proposals;
mod scope;
mod types;

use std::sync::Arc;

use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal, ProposalType};
use crate::rewrite::text_edit::{EditKind, EditTree};
use crate::rewrite::ASTRewrite;
use crate::semantic_ast::{modifier, problem as p, Ast, BindingRef, Node, NodeId, NodeKind};

use self::types::{can_assign, declaration, erasure, normalize, normalize_wildcard, type_label, type_names, well_known};

/// `ASTResolving.findCompilationUnitForBinding`: `Some(None)` is the
/// invocation's unit, `Some(Some(uri))` another unit of the project.
pub(super) struct Units {
    files: Vec<String>,
}

impl Units {
    async fn load(env: &Env<'_>, uri: &str) -> Units {
        let url = tower_lsp::lsp_types::Url::parse(uri).ok();
        let files = match url {
            Some(u) => env.dispatcher.context_for(Some(&u)).await.files.into_keys().collect(),
            None => Vec::new(),
        };
        Units { files }
    }

    fn find(&self, ast: &Ast, binding: BindingRef<'_>) -> Option<Option<String>> {
        if !binding.is_from_source() || binding.is_type_variable() || binding.is_wildcard_type() {
            return None;
        }
        let decl = declaration(binding);
        if let Some(node) = decl.declaring_node().filter(|n| std::ptr::eq(n.ast, ast)) {
            if node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration) {
                return Some(None);
            }
            return None;
        }
        let key = decl.key();
        let key = key.strip_prefix('L')?;
        let end = key.find([';', '<', '$', '~']).unwrap_or(key.len());
        let path = format!("/{}.java", &key[..end]);
        let mut matches: Vec<&String> = self.files.iter().filter(|f| f.ends_with(&path)).collect();
        matches.sort();
        matches.first().map(|f| Some((*f).clone()))
    }
}

fn rename_proposal(ctx: &Context, label: String, offset: usize, length: usize, new_name: &str, relevance: i32) -> Proposal {
    let ast = ctx.ast();
    let mut edits = EditTree::new();
    let mut ranges = Vec::new();
    // LinkedNodeFinder.findByProblems on the whole unit.
    if let Some(name) = crate::semantic_ast::finder::NodeFinder::new(ast.root(), offset, length).covered.filter(|n| n.is(NodeKind::SimpleName) && n.start() == offset && n.length() == length)
        .or_else(|| crate::semantic_ast::finder::NodeFinder::new(ast.root(), offset, length).covering.filter(|n| n.is(NodeKind::SimpleName)))
    {
        let kind_of = |id: i32| match id {
            p::UndefinedField => 1,
            p::UndefinedMethod => 2,
            p::UndefinedName | p::UnresolvedVariable => 4,
            p::UndefinedType => 8,
            _ => 0,
        };
        let name_kind = ast
            .problems
            .iter()
            .find(|pr| pr.source_start as usize == name.start() && (pr.source_end + 1) as usize == name.end() && kind_of(pr.id) != 0)
            .map(|pr| kind_of(pr.id))
            .unwrap_or(0);
        if name_kind != 0 {
            let identifier = name.identifier();
            let root = ast.root();
            for pr in &ast.problems {
                let (start, end) = (pr.source_start as usize, (pr.source_end + 1) as usize);
                if start > root.start() && end < root.end() && name_kind & kind_of(pr.id) != 0 {
                    if let Some(n) = crate::semantic_ast::finder::NodeFinder::new(root, start, end - start).covered.or(crate::semantic_ast::finder::NodeFinder::new(root, start, end - start).covering) {
                        if n.is(NodeKind::SimpleName) && n.identifier() == identifier {
                            ranges.push((n.start(), n.length()));
                        }
                    }
                }
            }
        }
    }
    if ranges.is_empty() {
        ranges.push((offset, length));
    }
    for (o, l) in ranges {
        let e = edits.new_edit(o as i32, l as i32, EditKind::Replace(new_name.to_owned()));
        let _ = edits.add_child(EditTree::ROOT, e);
    }
    Proposal::new(label, kind::QUICK_FIX, relevance, Change::Cu(vec![CuChange::edits(ctx.ast.clone(), edits)]))
}

fn replace_proposal(ctx: &Context, label: String, offset: usize, length: usize, text: &str, relevance: i32) -> Proposal {
    let mut edits = EditTree::new();
    let e = edits.new_edit(offset as i32, length as i32, if length == 0 { EditKind::Insert(text.to_owned()) } else { EditKind::Replace(text.to_owned()) });
    let _ = edits.add_child(EditTree::ROOT, e);
    Proposal::new(label, kind::QUICK_FIX, relevance, Change::Cu(vec![CuChange::edits(ctx.ast.clone(), edits)]))
}

fn new_element(label: String, relevance: i32, change: Box<dyn super::LazyChange>) -> Proposal {
    let mut p = Proposal::new(label, kind::QUICK_FIX, relevance, Change::Lazy(change));
    p.proposal_type = ProposalType::NewElement;
    p
}

/// `getParameterTypes(args)`: argument types, `Object` when unknown.
fn parameter_types<'a>(ast: &'a Ast, arguments: &[Node<'a>]) -> Vec<BindingRef<'a>> {
    arguments
        .iter()
        .filter_map(|a| {
            let mut t = normalize(a.type_binding());
            if let Some(w) = t.filter(|t| t.is_wildcard_type()) {
                t = normalize_wildcard(w, true);
            }
            t.or_else(|| well_known(ast, "java.lang.Object"))
        })
        .collect()
}

/// `getArgumentTypes(arguments)`.
fn argument_types<'a>(ast: &'a Ast, arguments: &[Node<'a>]) -> Option<Vec<BindingRef<'a>>> {
    let mut res = Vec::new();
    for a in arguments {
        let t = a.type_binding()?;
        if t.is_null_type() {
            res.push(t);
        } else {
            res.push(normalize(Some(t)).or_else(|| well_known(ast, "java.lang.Object"))?);
        }
    }
    Some(res)
}

/// `UnresolvedElementsSubProcessor.getMethodProposals`.
pub async fn method_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, only_parameter_mismatch: bool, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(name) = problem.covering_node(ast).filter(|n| n.is(NodeKind::SimpleName)) else { return };
    let Some(invocation) = name.parent() else { return };
    let (sender, is_super) = match invocation.kind() {
        NodeKind::MethodInvocation => (invocation.child("expression"), false),
        NodeKind::SuperMethodInvocation => (invocation.child("qualifier"), true),
        _ => return,
    };
    let arguments = invocation.list("arguments");
    let method_name = name.identifier();
    let units = Units::load(env, &ast.uri).await;

    let bindings = scope::methods_in_scope(name);
    let mut suggested = std::collections::HashSet::new();
    for b in &bindings {
        let curr = b.name();
        if curr != method_name && b.parameter_types().len() == arguments.len() && scope::is_similar_name(&method_name, curr) && suggested.insert(curr.to_owned()) {
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_changemethod_description"), &[curr]);
            proposals.push(rename_proposal(ctx, label, problem.offset, problem.length, curr, relevance::CHANGE_METHOD));
        }
    }

    if only_parameter_mismatch {
        let mismatches: Vec<_> = bindings.iter().copied().filter(|b| b.name() == method_name).collect();
        parameter_mismatch_proposals(env, ctx, &units, problem, &mismatches, invocation, &arguments, proposals).await;
    }

    if sender.is_none() {
        proposals::static_import_favorites(env, ctx, name, true, proposals).await;
    }

    add_new_method_proposals(ctx, &units, sender, &arguments, is_super, invocation, &method_name, proposals);

    if !only_parameter_mismatch && !is_super && sender.is_some() {
        proposals::missing_cast_parents(ctx, invocation, proposals);
    }

    if !is_super && sender.is_none() && invocation.parent().is_some_and(|p| p.is(NodeKind::ThrowStatement)) {
        let label = messages::correction("UnresolvedElementsSubProcessor_addnewkeyword_description").to_owned();
        let rel = if method_name.chars().next().is_some_and(char::is_uppercase) { relevance::ADD_NEW_KEYWORD_UPPERCASE } else { relevance::ADD_NEW_KEYWORD };
        proposals.push(replace_proposal(ctx, label, invocation.start(), 0, "new ", rel));
    }
}

/// `addNewMethodProposals`.
#[allow(clippy::too_many_arguments)]
fn add_new_method_proposals(
    ctx: &Context,
    units: &Units,
    sender: Option<Node<'_>>,
    arguments: &[Node<'_>],
    is_super: bool,
    invocation: Node<'_>,
    method_name: &str,
    proposals: &mut Vec<Proposal>,
) {
    let ast = ctx.ast();
    let node_parent_type = types::parent_type_binding(invocation);
    let binding = match sender {
        Some(s) => s.type_binding(),
        None => {
            let b = node_parent_type;
            if is_super { b.and_then(|b| b.superclass()) } else { b }
        }
    };
    let Some(binding) = binding.filter(|b| b.is_from_source()) else { return };
    let mut sender_decl = declaration(erasure(binding));
    let Some(target) = units.find(ast, sender_decl) else { return };
    let param_types = parameter_types(ast, arguments);
    let sig = types::method_signature(method_name, &param_types, false);
    let mut is_abstract_class = sender_decl.modifiers() & modifier::ABSTRACT != 0;
    let (label, label_abstract) = if node_parent_type.is_some_and(|p| p == sender_decl) {
        (
            messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_description"), &[&sig]),
            messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_abstract_description"), &[&sig]),
        )
    } else {
        let n = sender_decl.name();
        (
            messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_other_description"), &[&sig, n]),
            messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_abstract_other_description"), &[&sig, n]),
        )
    };
    let make = |sender: BindingRef<'_>, target: &Option<String>, is_abstract: bool| new_method::NewMethod {
        source: ctx.ast.clone(),
        invocation: invocation.id,
        arguments: arguments.iter().map(|a| a.id).collect(),
        sender: sender.key().to_owned(),
        target_uri: target.clone(),
        is_abstract,
    };
    proposals.push(new_element(label, relevance::CREATE_METHOD, Box::new(make(sender_decl, &target, false))));
    if is_abstract_class {
        proposals.push(new_element(label_abstract, relevance::CREATE_METHOD, Box::new(make(sender_decl, &target, true))));
    }
    if sender_decl.is_nested() && target.is_none() && sender.is_none() && types::find_method_in_hierarchy(sender_decl, method_name, None).is_none() {
        if let Some(anonym) = sender_decl.declaring_node().filter(|n| std::ptr::eq(n.ast, ast)) {
            let Some(outer) = anonym.parent().and_then(types::parent_type_binding) else { return };
            sender_decl = outer;
            is_abstract_class = sender_decl.modifiers() & modifier::ABSTRACT != 0;
            if !sender_decl.is_anonymous() {
                let type_sig = type_label(sender_decl);
                let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_other_description"), &[&sig, &type_sig]);
                let label_abstract = messages::format(messages::correction("UnresolvedElementsSubProcessor_createmethod_abstract_other_description"), &[&sig, &type_sig]);
                proposals.push(new_element(label, relevance::CREATE_METHOD, Box::new(make(sender_decl, &target, false))));
                if is_abstract_class {
                    proposals.push(new_element(label_abstract, relevance::CREATE_METHOD, Box::new(make(sender_decl, &target, true))));
                }
            }
        }
    }
}

/// `addParameterMissmatchProposals`.
#[allow(clippy::too_many_arguments)]
async fn parameter_mismatch_proposals(
    env: &Env<'_>,
    ctx: &Context,
    units: &Units,
    problem: &ProblemLocation,
    similar: &[BindingRef<'_>],
    invocation: Node<'_>,
    arguments: &[Node<'_>],
    proposals: &mut Vec<Proposal>,
) {
    let ast = ctx.ast();
    let Some(arg_types) = argument_types(ast, arguments) else { return };
    if similar.is_empty() {
        return;
    }
    for elem in similar {
        let diff = elem.parameter_types().len() as i64 - arg_types.len() as i64;
        if diff == 0 {
            let n = proposals.len();
            proposals::equal_number_of_parameters(env, ctx, units, problem, invocation, arguments, &arg_types, *elem, proposals).await;
            if n != proposals.len() {
                return;
            }
        } else if diff > 0 {
            proposals::more_parameters(ctx, units, invocation, &arg_types, *elem, proposals);
        } else {
            proposals::more_arguments(ctx, units, invocation, arguments, &arg_types, *elem, proposals);
        }
    }
}

/// `UnresolvedElementsSubProcessor.getConstructorProposals`.
pub async fn constructor_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(mut selected) = problem.covering_node(ast) else { return };
    let mut target: Option<BindingRef<'_>> = None;
    let mut arguments: Vec<Node<'_>> = Vec::new();
    let mut recursive: Option<BindingRef<'_>> = None;
    match selected.kind() {
        NodeKind::ClassInstanceCreation => {
            if let Some(b) = selected.child("type").and_then(|t| t.binding()) {
                target = Some(b);
                arguments = selected.list("arguments");
            }
        }
        NodeKind::SuperConstructorInvocation => {
            if let Some(t) = types::parent_type_binding(selected).filter(|t| !t.is_anonymous()) {
                target = t.superclass();
                arguments = selected.list("arguments");
            }
        }
        NodeKind::ConstructorInvocation => {
            if let Some(t) = types::parent_type_binding(selected).filter(|t| !t.is_anonymous()) {
                target = Some(t);
                arguments = selected.list("arguments");
                recursive = selected.ancestors().find(|a| a.is(NodeKind::MethodDeclaration)).and_then(|m| m.binding());
            }
        }
        _ => {}
    }
    if let Some(parent) = selected.parent().filter(|p| p.is(NodeKind::EnumConstantDeclaration)) {
        if let Some(t) = types::parent_type_binding(selected).filter(|t| !t.is_anonymous()) {
            target = Some(t);
            arguments = parent.list("arguments");
            selected = parent;
        }
    }
    let Some(target) = target else { return };
    let units = Units::load(env, &ast.uri).await;
    let similar: Vec<_> = target
        .declared_methods()
        .or_else(|| Some(target.constructors()))
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.is_constructor() && recursive.is_none_or(|r| r != *m))
        .collect();
    parameter_mismatch_proposals(env, ctx, &units, problem, &similar, selected, &arguments, proposals).await;

    if target.is_from_source() {
        let target_decl = declaration(target);
        if let Some(target_cu) = units.find(ast, target_decl) {
            let sig = types::method_signature(&type_label(target_decl), &parameter_types(ast, &arguments), false);
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_createconstructor_description"), &[&sig]);
            proposals.push(new_element(
                label,
                relevance::CREATE_CONSTRUCTOR,
                Box::new(new_method::NewMethod {
                    source: ctx.ast.clone(),
                    invocation: selected.id,
                    arguments: arguments.iter().map(|a| a.id).collect(),
                    sender: target_decl.key().to_owned(),
                    target_uri: target_cu,
                    is_abstract: false,
                }),
            ));
        }
    }
}

/// `UnresolvedElementsSubProcessor.getArrayAccessProposals`.
pub fn array_access_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(decl) = problem.covering_node(ast).filter(|n| n.is(NodeKind::MethodInvocation)) else { return };
    let Some(name) = decl.child("name") else { return };
    let method_name = name.identifier();
    for b in scope::methods_in_scope(name) {
        let curr = b.name();
        if scope::is_similar_name(&method_name, curr) {
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_arraychangetomethod_description"), &[curr]);
            proposals.push(rename_proposal(ctx, label, name.start(), name.length(), curr, relevance::ARRAY_CHANGE_TO_METHOD));
        }
    }
    let label = messages::correction("UnresolvedElementsSubProcessor_arraychangetolength_description").to_owned();
    let offset = name.start();
    let length = decl.start() + decl.length() - offset;
    proposals.push(rename_proposal(ctx, label, offset, length, "length", relevance::ARRAY_CHANGE_TO_LENGTH));
}

/// `UnresolvedElementsSubProcessor.getAnnotationMemberProposals`.
pub async fn annotation_member_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast) else { return };
    let (annotation, member_name) = if selected.location_is("name") && selected.parent().is_some_and(|p| p.is(NodeKind::MemberValuePair)) {
        let pair = selected.parent().unwrap();
        if !pair.location_is("values") {
            return;
        }
        (pair.parent().unwrap(), selected.identifier())
    } else if selected.location_is("value") && selected.parent().is_some_and(|p| p.is(NodeKind::SingleMemberAnnotation)) {
        (selected.parent().unwrap(), "value".to_owned())
    } else {
        return;
    };
    let Some(annot) = annotation.type_binding().or_else(|| annotation.child("typeName").and_then(|n| n.binding())) else { return };
    if annotation.is(NodeKind::NormalAnnotation) {
        for m in annot.declared_methods().unwrap_or_default() {
            let curr = m.name();
            let rel = if scope::is_similar_name(&member_name, curr) { relevance::CHANGE_TO_ATTRIBUTE_SIMILAR_NAME } else { relevance::CHANGE_TO_ATTRIBUTE };
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_UnresolvedElementsSubProcessor_changetoattribute_description"), &[curr]);
            proposals.push(rename_proposal(ctx, label, problem.offset, problem.length, curr, rel));
        }
    }
    if annot.is_from_source() {
        let units = Units::load(env, &ast.uri).await;
        if let Some(target) = units.find(ast, annot) {
            let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_UnresolvedElementsSubProcessor_createattribute_description"), &[&member_name]);
            proposals.push(new_element(
                label,
                relevance::CREATE_ATTRIBUTE,
                Box::new(proposals::NewAnnotationMember { source: ctx.ast.clone(), invocation: selected.id, sender: declaration(annot).key().to_owned(), target_uri: target }),
            ));
        }
    }
}

pub(super) fn _unused(_: Arc<Ast>, _: NodeId, _: &ASTRewrite, _: &[String]) {
    let _ = (can_assign, type_names);
}
