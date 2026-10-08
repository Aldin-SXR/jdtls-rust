//! Port of the sort members actions of `SourceAssistProcessor`
//! (`getSortMembersAction`, `getSortMembersForSelectionProposal`),
//! `DefaultJavaElementComparator`, `SortElementsOperation` and
//! `PartialSortMembersOperation`.

use std::cmp::Ordering;
use std::sync::Arc;

use super::edit::Env;
use super::handler::{priority, ActionData, Entry, Request};
use super::{kind, messages, Change, CuChange, LazyChange, Proposal};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, nflag, Ast, Node, NodeId, NodeKind};
use tower_lsp::lsp_types::CodeActionOrCommand;

const TYPE_INDEX: usize = 0;
const CONSTRUCTORS_INDEX: usize = 1;
const METHOD_INDEX: usize = 2;
const FIELDS_INDEX: usize = 3;
const INIT_INDEX: usize = 4;
const STATIC_FIELDS_INDEX: usize = 5;
const STATIC_INIT_INDEX: usize = 6;
const STATIC_METHODS_INDEX: usize = 7;
const ENUM_CONSTANTS_INDEX: usize = 8;
const N_CATEGORIES: usize = 9;

/// Default `outlinesortoption`: `T,SF,SI,SM,F,I,C,M`; enum constants first.
fn category_offsets() -> [i32; N_CATEGORIES] {
    let mut offsets = [0; N_CATEGORIES];
    let mut i = 1;
    for token in ["T", "SF", "SI", "SM", "F", "I", "C", "M"] {
        let index = match token {
            "T" => TYPE_INDEX,
            "M" => METHOD_INDEX,
            "F" => FIELDS_INDEX,
            "I" => INIT_INDEX,
            "SF" => STATIC_FIELDS_INDEX,
            "SI" => STATIC_INIT_INDEX,
            "SM" => STATIC_METHODS_INDEX,
            _ => CONSTRUCTORS_INDEX,
        };
        offsets[index] = i;
        i += 1;
    }
    offsets[ENUM_CONSTANTS_INDEX] = 0;
    offsets
}

/// Default `org.eclipse.jdt.ui.visibility.order`: `B,V,R,D`.
fn visibility_index(flags: i32) -> i32 {
    if flags & modifier::PUBLIC != 0 {
        0
    } else if flags & modifier::PROTECTED != 0 {
        2
    } else if flags & modifier::PRIVATE != 0 {
        1
    } else {
        3
    }
}

fn is_interface_or_annotation(node: Option<Node<'_>>) -> bool {
    node.is_some_and(|n| (n.is(NodeKind::TypeDeclaration) && n.flag("interface")) || n.is(NodeKind::AnnotationTypeDeclaration))
}

fn is_static(decl: Node<'_>) -> bool {
    let nested_interface = decl.parent().is_some_and(|p| p.kind().is_abstract_type_declaration()) && is_interface_or_annotation(Some(decl));
    if nested_interface {
        return true;
    }
    if !matches!(decl.kind(), NodeKind::MethodDeclaration | NodeKind::AnnotationTypeMemberDeclaration) && is_interface_or_annotation(decl.parent()) {
        return true;
    }
    if decl.is(NodeKind::EnumConstantDeclaration) {
        return true;
    }
    if decl.is(NodeKind::EnumDeclaration) && decl.parent().is_some_and(|p| p.kind().is_abstract_type_declaration()) {
        return true;
    }
    decl.modifiers() & modifier::STATIC != 0
}

fn visibility_code(decl: Node<'_>) -> i32 {
    let flags = decl.modifiers();
    if is_interface_or_annotation(decl.parent()) || flags & modifier::PUBLIC != 0 {
        modifier::PUBLIC
    } else if flags & modifier::PROTECTED != 0 {
        modifier::PROTECTED
    } else if flags & modifier::PRIVATE != 0 {
        modifier::PRIVATE
    } else {
        0
    }
}

fn category(decl: Node<'_>) -> usize {
    match decl.kind() {
        NodeKind::MethodDeclaration => {
            if decl.flag("constructor") {
                CONSTRUCTORS_INDEX
            } else if decl.modifiers() & modifier::STATIC != 0 {
                STATIC_METHODS_INDEX
            } else {
                METHOD_INDEX
            }
        }
        NodeKind::FieldDeclaration => {
            if is_static(decl) {
                STATIC_FIELDS_INDEX
            } else {
                FIELDS_INDEX
            }
        }
        NodeKind::Initializer => {
            if decl.modifiers() & modifier::STATIC != 0 {
                STATIC_INIT_INDEX
            } else {
                INIT_INDEX
            }
        }
        NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration => TYPE_INDEX,
        NodeKind::EnumConstantDeclaration => ENUM_CONSTANTS_INDEX,
        NodeKind::AnnotationTypeMemberDeclaration => METHOD_INDEX,
        _ => 0,
    }
}

fn sort_preserved_category(category: usize) -> usize {
    match category {
        STATIC_FIELDS_INDEX | STATIC_INIT_INDEX => STATIC_FIELDS_INDEX,
        FIELDS_INDEX | INIT_INDEX => FIELDS_INDEX,
        other => other,
    }
}

fn is_sort_preserved(decl: Node<'_>) -> bool {
    matches!(decl.kind(), NodeKind::FieldDeclaration | NodeKind::EnumConstantDeclaration | NodeKind::Initializer)
}

/// `DefaultJavaElementComparator.compare`.
fn compare(do_not_sort_fields: bool, a: Node<'_>, b: Node<'_>) -> Ordering {
    let offsets = category_offsets();
    let preserved1 = do_not_sort_fields && is_sort_preserved(a);
    let preserved2 = do_not_sort_fields && is_sort_preserved(b);
    let mut cat1 = category(a);
    if preserved1 {
        cat1 = sort_preserved_category(cat1);
    }
    let mut cat2 = category(b);
    if preserved2 {
        cat2 = sort_preserved_category(cat2);
    }
    if cat1 != cat2 {
        return offsets[cat1].cmp(&offsets[cat2]);
    }
    if preserved1 {
        return a.start().cmp(&b.start());
    }
    visibility_index(visibility_code(a)).cmp(&visibility_index(visibility_code(b)))
}

fn malformed(node: Node<'_>) -> bool {
    node.flags() & nflag::MALFORMED != 0
}

fn contains_malformed(items: &[Node<'_>]) -> bool {
    items.iter().any(|n| malformed(*n))
}

/// `sortElements(elements, listRewrite)`.
fn sort_elements(rw: &mut ASTRewrite, parent: RNode, prop: &'static str, elements: &[Node<'_>], do_not_sort_fields: bool, changed: &mut bool) {
    if elements.is_empty() {
        return;
    }
    let mut sorted = elements.to_vec();
    sorted.sort_by(|a, b| compare(do_not_sort_fields, *a, *b));
    for (old, new) in elements.iter().zip(&sorted) {
        if old != new {
            let target = rw.create_move_target(new.id);
            rw.list_replace(parent, prop, RNode::Orig(old.id), target);
            *changed = true;
        }
    }
}

/// `SortElementsOperation.sortCompilationUnit` (all members) or
/// `PartialSortMembersOperation.sortCompilationUnit` (the selected nodes).
fn sort_compilation_unit(ast: &Arc<Ast>, selected: Option<&[NodeId]>, do_not_sort_fields: bool) -> Option<ASTRewrite> {
    let mut rw = ASTRewrite::new(ast.clone());
    let mut changed = false;
    let pick = |items: Vec<Node<'_>>| -> Vec<NodeId> {
        match selected {
            None => items.iter().map(|n| n.id).collect(),
            Some(selected) => selected.iter().copied().filter(|s| items.iter().any(|i| i.id == *s)).collect(),
        }
    };
    let root = ast.root();
    for node in std::iter::once(root).chain(root.descendants()) {
        let (prop, items): (&'static str, Vec<Node<'_>>) = match node.kind() {
            NodeKind::CompilationUnit => ("types", node.list("types")),
            NodeKind::AnnotationTypeDeclaration | NodeKind::AnonymousClassDeclaration | NodeKind::TypeDeclaration | NodeKind::EnumDeclaration => {
                ("bodyDeclarations", node.list("bodyDeclarations"))
            }
            _ => continue,
        };
        let mut malformed_nodes = contains_malformed(&items);
        if node.is(NodeKind::EnumDeclaration) {
            malformed_nodes |= contains_malformed(&node.list("enumConstants"));
        }
        if malformed_nodes {
            continue;
        }
        let elements: Vec<Node<'_>> = pick(items.clone()).into_iter().map(|id| ast.node(id)).collect();
        sort_elements(&mut rw, RNode::Orig(node.id), prop, &elements, do_not_sort_fields, &mut changed);
        if node.is(NodeKind::EnumDeclaration) {
            let constants = node.list("enumConstants");
            let elements: Vec<Node<'_>> = match selected {
                None => constants,
                Some(selected) => {
                    // `PartialSortMembersOperation` filters the enum constants with the body declarations.
                    selected.iter().copied().filter(|s| items.iter().any(|i| i.id == *s)).map(|id| ast.node(id)).collect()
                }
            };
            sort_elements(&mut rw, RNode::Orig(node.id), "enumConstants", &elements, do_not_sort_fields, &mut changed);
        }
    }
    changed.then_some(rw)
}

struct SortChange {
    rewrite: ASTRewrite,
}

#[tower_lsp::async_trait]
impl LazyChange for SortChange {
    fn changes_only(&self) -> bool {
        true
    }

    async fn compute(&self, _env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        Ok(vec![CuChange::rewrite(self.rewrite.clone())])
    }
}

/// `CodeActionUtility.findASTNode(coveredNodes, coveringNode, TypeDeclaration.class)`.
fn find_type_declaration<'a>(covered: &[Node<'a>], covering: Option<Node<'a>>) -> Option<Node<'a>> {
    let infer = |mut node: Node<'a>| -> Option<Node<'a>> {
        loop {
            if node.is(NodeKind::TypeDeclaration) {
                return Some(node);
            }
            if node.kind().is_statement() || node.kind().is_body_declaration() {
                return None;
            }
            node = node.parent()?;
        }
    };
    if !covered.is_empty() {
        return covered.iter().find_map(|n| infer(*n));
    }
    covering.and_then(infer)
}

/// The sort members entries of `SourceAssistProcessor.getSourceActionCommands`.
pub async fn actions(env: &Env<'_>, req: &Request<'_>, first: usize) -> Vec<(Entry, Option<Proposal>)> {
    let mut out: Vec<(Entry, Option<Proposal>)> = Vec::new();
    let ast = &req.context.ast;
    if ast.root().list("types").iter().any(|t| t.is(NodeKind::ImplicitTypeDeclaration)) {
        return out;
    }
    let covered = crate::features::accessors::actions::fully_covered(req);
    let covering = req.context.covering_node();
    let type_declaration = find_type_declaration(&covered, covering);
    let avoid_volatile = crate::features::preferences::get_bool("java.codeAction.sortMembers.avoidVolatileChanges").unwrap_or(true);
    let file_name = req.uri.path_segments().and_then(|mut s| s.next_back()).unwrap_or_default();
    let file_name = percent_decode(file_name);
    let template = messages::format(messages::ls_action("SortMembers_templateLabel"), &[&file_name]);

    let mut candidates: Vec<(&str, String, Option<Vec<NodeId>>)> = vec![(kind::SOURCE_SORT_MEMBERS, template.clone(), None)];
    if type_declaration.is_some_and(|t| t.parent().is_some_and(|p| p.is(NodeKind::CompilationUnit))) {
        candidates.push((kind::QUICK_ASSIST, template, None));
    }
    if !covered.is_empty() {
        candidates.push((kind::QUICK_ASSIST, messages::ls_action("SortMembers_selectionLabel").to_owned(), Some(covered.iter().map(|n| n.id).collect())));
    }
    let resolve = crate::features::client_caps::resolve_code_action();
    for (action_kind, label, selection) in candidates {
        if !crate::features::client_caps::supported_code_action_kind(action_kind) {
            continue;
        }
        if req.params.context.only.as_ref().is_some_and(|only| !only.is_empty() && !only.iter().any(|k| action_kind.starts_with(k.as_str()))) {
            continue;
        }
        let Some(rewrite) = sort_compilation_unit(ast, selection.as_deref(), avoid_volatile) else { continue };
        let mut proposal = Proposal::new(label, action_kind, 0, Change::Lazy(Box::new(SortChange { rewrite })));
        let index = first + out.iter().filter(|(_, p)| p.is_some()).count();
        if let Some(mut entry) = super::handler::code_action_from_proposal(env, &req.uri, &mut proposal, &req.params.context.diagnostics, resolve, index).await {
            if let CodeActionOrCommand::CodeAction(action) = &mut entry.action {
                action.diagnostics = Some(if resolve { Vec::new() } else { req.params.context.diagnostics.clone() });
            }
            entry.data = resolve.then_some(ActionData { proposal: Some(index), priority: priority::SORT_MEMBERS });
            out.push((entry, resolve.then_some(proposal)));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
