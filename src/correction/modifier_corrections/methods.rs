//! Abstract/native/missing-body corrections from ModifierCorrectionSubProcessorCore.
use crate::{
    correction::{kind, messages, relevance, Context, ProblemLocation, Proposal},
    rewrite::{ASTRewrite, RNode},
    semantic_ast::{modifier, problem as p, resolve, Node, NodeKind},
};
fn method<'a>(ctx: &'a Context, problem: &ProblemLocation) -> Option<Node<'a>> {
    let mut node = problem.covering_node(ctx.ast())?;
    if node.is(NodeKind::SimpleName) {
        node = node.parent()?;
    }
    node.is(NodeKind::MethodDeclaration).then_some(node)
}
fn remove_modifier(rw: &mut ASTRewrite, decl: Node<'_>, flag: i32) -> bool {
    if let Some(n) = decl.list("modifiers").into_iter().find(|n| {
        n.is(NodeKind::Modifier) && modifier::flag_of(n.simple("keyword").unwrap_or("")) & flag != 0
    }) {
        rw.remove(RNode::Orig(n.id));
        true
    } else {
        false
    }
}
fn modifiers(rw: &mut ASTRewrite, decl: Node<'_>, included: i32, excluded: i32) {
    let mut missing = included;
    for n in decl.list("modifiers") {
        if n.is(NodeKind::Modifier) {
            let flag = modifier::flag_of(n.simple("keyword").unwrap_or(""));
            missing &= !flag;
            if excluded & flag != 0 {
                rw.remove(RNode::Orig(n.id));
            }
        }
    }
    for n in rw.new_modifiers(missing) {
        rw.list_insert_last(RNode::Orig(decl.id), "modifiers", n);
    }
}
fn default_body(rw: &mut ASTRewrite, decl: Node<'_>) -> RNode {
    let expression = decl.child("returnType2").and_then(|typ| {
        if decl.list("extraDimensions2").is_empty() && typ.is(NodeKind::PrimitiveType) {
            match typ.simple("primitiveTypeCode").unwrap_or("") {
                "void" => None,
                "boolean" => {
                    let n = rw.new_node(NodeKind::BooleanLiteral);
                    Some(rw.put_simple(n, "booleanValue", "false"))
                }
                _ => Some(rw.new_number_literal("0")),
            }
        } else if decl.list("extraDimensions2").is_empty()
            && typ.is(NodeKind::ParameterizedType)
            && typ
                .child("type")
                .and_then(|t| t.child("name"))
                .is_some_and(|n| n.source_text() == "java.util.Optional")
        {
            let receiver = rw.new_name("java.util.Optional");
            Some(rw.new_method_invocation(Some(receiver), "empty", Vec::new()))
        } else {
            Some(rw.new_node(NodeKind::NullLiteral))
        }
    });
    let statements = expression
        .map(|e| rw.new_return_statement(Some(e)))
        .into_iter()
        .collect();
    rw.new_block(statements)
}
fn proposal(proposals: &mut Vec<Proposal>, key: &str, relevance: i32, rw: ASTRewrite) {
    proposals.push(Proposal::rewrite(
        messages::correction(key),
        kind::QUICK_FIX,
        relevance,
        rw,
    ));
}
pub fn make_type_abstract(ctx: &Context, decl: Node<'_>, proposals: &mut Vec<Proposal>) {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let new = rw.new_modifier("abstract");
    rw.list_insert_last(RNode::Orig(decl.id), "modifiers", new);
    let name = decl
        .child("name")
        .map(|n| n.identifier())
        .unwrap_or_default();
    proposals.push(Proposal::rewrite(
        messages::format(
            messages::correction("ModifierCorrectionSubProcessor_addabstract_description"),
            &[&name],
        ),
        kind::QUICK_FIX,
        relevance::MAKE_TYPE_ABSTRACT_FIX,
        rw,
    ));
}
pub fn abstract_type(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(mut node) = problem.covering_node(ctx.ast()) else {
        return;
    };
    if node.is(NodeKind::SimpleName) {
        let Some(parent) = node.parent() else {
            return;
        };
        node = parent;
    }
    if node.is(NodeKind::TypeDeclaration) {
        make_type_abstract(ctx, node, proposals);
    }
}
pub fn abstract_method(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(decl) = method(ctx, problem) else {
        return;
    };
    let parent = resolve::find_parent_type(decl);
    let interface = parent
        .is_some_and(|p| p.is(NodeKind::TypeDeclaration) && p.simple("interface") == Some("true"));
    let abstract_class = parent.is_some_and(|p| {
        p.is(NodeKind::TypeDeclaration) && !interface && p.modifiers() & modifier::ABSTRACT != 0
    });
    let id = problem.problem_id;
    if matches!(
        id,
        p::AbstractMethodInAbstractClass
            | p::EnumAbstractMethodMustBeImplemented
            | p::AbstractMethodInEnum
    ) || abstract_class
    {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        remove_modifier(&mut rw, decl, modifier::ABSTRACT);
        if decl.child("body").is_none() {
            let body = default_body(&mut rw, decl);
            rw.set(RNode::Orig(decl.id), "body", Some(body));
        }
        proposal(
            proposals,
            "ModifierCorrectionSubProcessor_removeabstract_description",
            relevance::REMOVE_ABSTRACT_MODIFIER,
            rw,
        );
    }
    if decl.child("body").is_some() && id == p::BodyForAbstractMethod {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        rw.set(RNode::Orig(decl.id), "body", None);
        let keep =
            modifier::PUBLIC | modifier::ABSTRACT | if interface { 0 } else { modifier::PROTECTED };
        modifiers(&mut rw, decl, 0, !keep);
        proposal(
            proposals,
            "ModifierCorrectionSubProcessor_removebody_description",
            relevance::REMOVE_METHOD_BODY,
            rw,
        );
        if interface {
            for (included, excluded, key, rank) in [
                (
                    modifier::STATIC,
                    modifier::ABSTRACT | modifier::DEFAULT,
                    "ModifierCorrectionSubProcessor_changemodifiertostatic_description",
                    relevance::ADD_STATIC_MODIFIER,
                ),
                (
                    modifier::DEFAULT,
                    modifier::ABSTRACT | modifier::STATIC,
                    "ModifierCorrectionSubProcessor_changemodifiertodefault_description",
                    relevance::ADD_DEFAULT_MODIFIER,
                ),
            ] {
                let mut rw = ASTRewrite::new(ctx.ast.clone());
                modifiers(&mut rw, decl, included, excluded);
                let name = decl
                    .child("name")
                    .map(|n| n.identifier())
                    .unwrap_or_default();
                proposals.push(Proposal::rewrite(
                    messages::format(messages::correction(key), &[&name]),
                    kind::QUICK_FIX,
                    rank,
                    rw,
                ));
            }
        }
    }
    if id == p::AbstractMethodInAbstractClass {
        if let Some(parent) = parent.filter(|p| p.is(NodeKind::TypeDeclaration)) {
            make_type_abstract(ctx, parent, proposals);
        }
    }
}
pub fn native_method(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(decl) = method(ctx, problem) else {
        return;
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    remove_modifier(&mut rw, decl, modifier::NATIVE);
    let body = default_body(&mut rw, decl);
    rw.set(RNode::Orig(decl.id), "body", Some(body));
    proposal(
        proposals,
        "ModifierCorrectionSubProcessor_removenative_description",
        relevance::REMOVE_NATIVE,
        rw,
    );
    if decl.child("body").is_some() {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        rw.set(RNode::Orig(decl.id), "body", None);
        proposal(
            proposals,
            "ModifierCorrectionSubProcessor_removebody_description",
            relevance::REMOVE_METHOD_BODY,
            rw,
        );
    }
}
pub fn requires_body(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(decl) = problem
        .covering_node(ctx.ast())
        .filter(|n| n.is(NodeKind::MethodDeclaration))
    else {
        return;
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let removed = remove_modifier(&mut rw, decl, modifier::ABSTRACT);
    let body = default_body(&mut rw, decl);
    rw.set(RNode::Orig(decl.id), "body", Some(body));
    proposal(
        proposals,
        "ModifierCorrectionSubProcessor_addmissingbody_description",
        relevance::ADD_MISSING_BODY,
        rw,
    );
    if !removed {
        if let Some(binding) = decl.binding() {
            let owner = binding.declaring_class();
            let included = if owner.is_some_and(|t| t.is_interface()) {
                0
            } else {
                modifier::ABSTRACT
            };
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            modifiers(
                &mut rw,
                decl,
                included,
                modifier::STATIC | modifier::DEFAULT,
            );
            let name = format!(
                "{}.{}",
                owner.map(|t| t.name()).unwrap_or(""),
                binding.name()
            );
            let label = messages::format(
                messages::correction(
                    "ModifierCorrectionSubProcessor_changemodifiertoabstract_description",
                ),
                &[&name],
            );
            proposals.push(Proposal::rewrite(
                label,
                kind::QUICK_FIX,
                relevance::ADD_ABSTRACT_MODIFIER,
                rw,
            ));
        }
    }
}
