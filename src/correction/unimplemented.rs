//! UnimplementedCodeFixCore, AddUnimplementedMethodsOperation and StubUtility2Core.getMethodsIn.
use crate::{
    correction::{
        edit::Env, kind, messages, relevance, Change, Context, CuChange, LazyChange,
        ProblemLocation, Proposal,
    },
    features::{
        delegates::{erasure, overridden, subsignature, subtype},
        overrides::operation,
    },
    semantic_ast::{modifier, Ast, BindingRef, Node, NodeId, NodeKind},
};
use std::{cmp::Ordering, collections::HashSet, sync::Arc};
fn sub(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    a.name() == b.name() && a.data().method_subsignatures.contains(&b.id)
}
fn eligible(m: BindingRef<'_>) -> bool {
    !m.is_constructor() && m.modifiers() & (modifier::STATIC | modifier::PRIVATE) == 0
}
fn abstract_method(m: BindingRef<'_>) -> bool {
    m.modifiers() & modifier::ABSTRACT != 0
}
fn default_method(m: BindingRef<'_>) -> bool {
    m.modifiers() & modifier::DEFAULT != 0
}
fn concrete(m: BindingRef<'_>) -> bool {
    !abstract_method(m) && !default_method(m)
}
fn visible(m: BindingRef<'_>, t: BindingRef<'_>) -> bool {
    m.modifiers() & (modifier::PUBLIC | modifier::PROTECTED) != 0
        || m.declaring_class()
            .is_some_and(|c| c.is_interface() || c.package_name() == t.package_name())
}
fn methods_in<'a>(typ: BindingRef<'a>, stack: &mut HashSet<String>) -> Vec<BindingRef<'a>> {
    if !stack.insert(typ.key().into()) {
        return Vec::new();
    }
    // AddUnimplementedMethodsOperation captures its initially null binding in
    // ignoreAbstractsOfInput, so own abstract methods remain in the input.
    let mut all: Vec<_> = typ
        .declared_methods()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| eligible(*m))
        .collect();
    let mut interfaces: Vec<_> = typ
        .interfaces()
        .into_iter()
        .map(|i| methods_in(i, stack))
        .collect();
    if !typ.is_interface() {
        let supers = typ
            .superclass()
            .map(|s| methods_in(s, stack))
            .unwrap_or_default();
        for m in &supers {
            if concrete(*m) && visible(*m, typ) && !all.iter().any(|c| sub(*c, *m)) {
                all.push(*m);
            }
        }
        interfaces.push(supers);
    }
    let mut inherited: Vec<BindingRef<'a>> = Vec::new();
    for group in interfaces {
        for m in group {
            if !abstract_method(m) && !default_method(m) {
                continue;
            }
            let previous = all.iter().copied().find(|c| sub(*c, m));
            let overriding = all
                .iter()
                .copied()
                .find(|c| overridden(*c, m) || subsignature(*c, m));
            if previous.is_none() && overriding.is_none_or(|c| !concrete(c)) {
                let sub_sig = inherited.iter().copied().find(|c| sub(*c, m));
                let super_sig = inherited.iter().copied().find(|c| sub(m, *c));
                if super_sig.is_some_and(|c| m.data().method_overrides.contains(&c.id)) {
                    if let Some(c) = sub_sig {
                        inherited.retain(|n| *n != c);
                    }
                    inherited.push(m);
                } else if let (Some(a), Some(_)) = (sub_sig, super_sig) {
                    if !a
                        .return_type()
                        .zip(m.return_type())
                        .is_some_and(|(a, b)| subtype(erasure(a), erasure(b)))
                    {
                        inherited.retain(|n| *n != a);
                        inherited.push(m);
                    }
                } else if let Some(c) = super_sig {
                    inherited.retain(|n| *n != c);
                    inherited.push(m);
                } else if sub_sig.is_none() {
                    inherited.push(m);
                }
            }
        }
    }
    all.extend(inherited);
    stack.remove(typ.key());
    all
}
fn compare(owner: BindingRef<'_>, a: BindingRef<'_>, b: BindingRef<'_>) -> Ordering {
    let (Some(ad), Some(bd)) = (a.declaring_class(), b.declaring_class()) else {
        return Ordering::Equal;
    };
    if ad == bd {
        return if a.data().source_offset >= 0 && b.data().source_offset >= 0 {
            a.data().source_offset.cmp(&b.data().source_offset)
        } else {
            a.name().cmp(b.name())
        };
    }
    if ad == owner {
        return Ordering::Greater;
    }
    if bd == owner {
        return Ordering::Less;
    }
    let (mut ac, mut bc) = (None, None);
    let mut current = owner.superclass();
    let mut count = 0;
    let mut seen = HashSet::new();
    while let Some(t) = current {
        if !seen.insert(t.key()) {
            break;
        }
        if ad == t {
            ac = Some(count);
        }
        if bd == t {
            bc = Some(count);
        }
        count += 1;
        current = t.superclass();
    }
    match (ac, bc) {
        (Some(a), Some(b)) => return a.cmp(&b),
        (Some(_), None) => return Ordering::Greater,
        (None, Some(_)) => return Ordering::Less,
        _ => {}
    }
    for i in owner.interfaces() {
        if ad == i {
            return Ordering::Greater;
        }
        if bd == i {
            return Ordering::Less;
        }
    }
    Ordering::Equal
}
fn selected_type<'a>(ctx: &'a Context, problem: &ProblemLocation) -> Option<Node<'a>> {
    let mut node = problem.covering_node(ctx.ast())?;
    if node.is(NodeKind::AnonymousClassDeclaration) {
        node = node.parent()?;
    }
    if node.location_is("name")
        && node
            .parent()
            .is_some_and(|n| n.is(NodeKind::EnumConstantDeclaration))
    {
        node = node.parent()?;
    }
    match node.kind() {
        NodeKind::SimpleName
            if node
                .parent()
                .is_some_and(|n| n.kind().is_abstract_type_declaration()) =>
        {
            node.parent()
        }
        NodeKind::ClassInstanceCreation => node.child("anonymousClassDeclaration"),
        NodeKind::EnumConstantDeclaration => {
            Some(node.child("anonymousClassDeclaration").unwrap_or(node))
        }
        _ => None,
    }
}
fn methods<'a>(ast: &'a Ast, declaration: NodeId) -> Vec<BindingRef<'a>> {
    let Some(owner) = ast.node(declaration).binding().and_then(|b| {
        if b.is_variable() {
            b.declaring_class()
        } else {
            Some(b)
        }
    }) else {
        return Vec::new();
    };
    let mut methods: Vec<_> = methods_in(owner, &mut HashSet::new())
        .into_iter()
        .filter(|m| abstract_method(*m))
        .collect();
    crate::correction::java_sort(&mut methods, |a, b| compare(owner, *a, *b));
    methods
}
pub fn proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(node) = selected_type(ctx, problem) else {
        return;
    };
    if methods(ctx.ast(), node.id).is_empty() {
        return;
    }
    proposals.push(Proposal::new(
        messages::correction("UnimplementedMethodsCorrectionProposal_description"),
        kind::QUICK_FIX,
        relevance::ADD_UNIMPLEMENTED_METHODS,
        Change::Lazy(Box::new(Implement {
            ast: ctx.ast.clone(),
            declaration: node.id,
        })),
    ));
}
struct Implement {
    ast: Arc<Ast>,
    declaration: NodeId,
}
#[tower_lsp::async_trait]
impl LazyChange for Implement {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        Ok(vec![
            operation::create_unimplemented(
                env,
                self.ast.clone(),
                self.declaration,
                &methods(&self.ast, self.declaration),
            )
            .await?,
        ])
    }
}
