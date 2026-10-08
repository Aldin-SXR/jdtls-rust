//! LocalCorrectionsSubProcessor.addUnusedMemberProposal and UnusedCodeFixCore.
use crate::{
    correction::{edit::Env, kind, messages, relevance, Context, ProblemLocation, Proposal},
    rewrite::{ASTRewrite, RNode},
    semantic_ast::{
        modifier, problem as p, resolve::unparenthesed_expression, BindingKind, BindingRef, Node,
        NodeKind,
    },
};
use std::collections::HashSet;

fn unused_name<'a>(ctx: &'a Context, problem: &ProblemLocation) -> Option<Node<'a>> {
    let selected = problem.covering_node(ctx.ast())?;
    match selected.kind() {
        NodeKind::SimpleName => Some(selected),
        NodeKind::MethodDeclaration => selected.child("name"),
        _ => None,
    }
}
fn canonical(binding: BindingRef<'_>) -> BindingRef<'_> {
    match binding.kind() {
        BindingKind::Type => binding.type_declaration().unwrap_or(binding),
        BindingKind::Variable => binding.variable_declaration().unwrap_or(binding),
        BindingKind::Method if binding.is_constructor() => binding
            .declaring_class()
            .and_then(|t| t.type_declaration())
            .unwrap_or(binding),
        BindingKind::Method => binding.method_declaration().unwrap_or(binding),
        _ => binding,
    }
}
fn linked(binding: BindingRef<'_>) -> Vec<Node<'_>> {
    let original = canonical(binding);
    binding
        .ast
        .all_nodes()
        .filter(|n| {
            n.is(NodeKind::SimpleName)
                && n.simple("var") != Some("true")
                && n.binding().is_some_and(|b| {
                    let b = canonical(b);
                    b == original
                        || b.kind() == BindingKind::Method
                            && original.kind() == BindingKind::Method
                            && (b.data().method_overrides.contains(&original.id)
                                || original.data().method_overrides.contains(&b.id))
                })
        })
        .collect()
}
fn control_body(node: Node<'_>) -> bool {
    node.parent().is_some_and(|p| match p.kind() {
        NodeKind::IfStatement => matches!(node.location(), Some("thenStatement" | "elseStatement")),
        NodeKind::WhileStatement
        | NodeKind::ForStatement
        | NodeKind::EnhancedForStatement
        | NodeKind::DoStatement => node.location_is("body"),
        _ => false,
    })
}
fn remove_statement(rw: &mut ASTRewrite, statement: Node<'_>) {
    if control_body(statement) {
        let block = rw.new_block(Vec::new());
        rw.replace(RNode::Orig(statement.id), Some(block));
    } else {
        rw.remove(RNode::Orig(statement.id));
    }
}
fn side_effects<'a>(node: Node<'a>, effects: &mut Vec<Node<'a>>) {
    if matches!(
        node.kind(),
        NodeKind::Assignment
            | NodeKind::PostfixExpression
            | NodeKind::MethodInvocation
            | NodeKind::ClassInstanceCreation
            | NodeKind::SuperMethodInvocation
    ) || node.is(NodeKind::PrefixExpression)
        && matches!(node.simple("operator"), Some("++" | "--"))
    {
        effects.push(node);
        return;
    }
    for child in node.children() {
        side_effects(child, effects);
    }
}
fn conditional_effect(node: Node<'_>) -> bool {
    matches!(
        unparenthesed_expression(node).kind(),
        NodeKind::MethodInvocation
            | NodeKind::PostfixExpression
            | NodeKind::PrefixExpression
            | NodeKind::Assignment
    )
}
fn replace_many(rw: &mut ASTRewrite, original: Node<'_>, replacements: Vec<RNode>) {
    if replacements.len() == 1 {
        rw.replace(RNode::Orig(original.id), Some(replacements[0]));
    } else if control_body(original) {
        let block = rw.new_block(replacements);
        rw.replace(RNode::Orig(original.id), Some(block));
    } else if let Some(parent) = original.parent() {
        let loc = original.location().unwrap();
        let mut previous = RNode::Orig(original.id);
        for replacement in replacements {
            rw.list_insert_after(RNode::Orig(parent.id), loc, replacement, previous);
            previous = replacement;
        }
        rw.remove(RNode::Orig(original.id));
    }
}
fn remove_with_initializer(
    rw: &mut ASTRewrite,
    initializer: Node<'_>,
    statement: Node<'_>,
    force: bool,
) {
    let mut effects = Vec::new();
    if !force {
        side_effects(initializer, &mut effects);
    }
    if effects.is_empty() {
        remove_statement(rw, statement);
    } else {
        let mut source = initializer;
        while source.is(NodeKind::FieldAccess) {
            let Some(receiver) = source.child("expression") else {
                break;
            };
            source = receiver;
        }
        let source = rw.create_move_target(source.id);
        let replacement = rw.new_expression_statement(source);
        rw.replace(RNode::Orig(statement.id), Some(replacement));
    }
}
fn split_declarations(
    rw: &mut ASTRewrite,
    fragment: Node<'_>,
    declaration: Node<'_>,
    effects: Vec<Node<'_>>,
) {
    if effects.is_empty() {
        return;
    }
    let Some(parent) = declaration.parent() else {
        return;
    };
    let loc = declaration.location().unwrap();
    let mut previous = RNode::Orig(declaration.id);
    for effect in effects {
        let moved = rw.create_move_target(effect.id);
        let statement = rw.new_expression_statement(moved);
        rw.list_insert_after(RNode::Orig(parent.id), loc, statement, previous);
        previous = statement;
    }
    let fragments = declaration.list("fragments");
    let Some(index) = fragments.iter().position(|n| n.id == fragment.id) else {
        return;
    };
    let following: Vec<_> = fragments[index + 1..]
        .iter()
        .map(|n| rw.create_move_target(n.id))
        .collect();
    if !following.is_empty() {
        let new = rw.new_node(NodeKind::VariableDeclarationStatement);
        if let Some(typ) = declaration.child("type") {
            let copy = rw.create_copy_target(typ.id);
            rw.put_child(new, "type", copy);
        }
        rw.put_list(new, "fragments", following);
        rw.list_insert_after(RNode::Orig(parent.id), loc, new, previous);
        if index == 0 {
            rw.remove(RNode::Orig(declaration.id));
        }
    }
}
fn remove_fragment(rw: &mut ASTRewrite, fragment: Node<'_>, force: bool) {
    let Some(declaration) = fragment.parent() else {
        return;
    };
    let fragments = declaration.list("fragments");
    let initializer = fragment.child("initializer");
    let mut effects = Vec::new();
    if let Some(initializer) = initializer {
        side_effects(initializer, &mut effects);
    }
    // Upstream special case: the entire declaration becomes an if, even for
    // multiple fragments. Retain its branch-evaluation and node-kind rules.
    if let Some(ce) = initializer
        .filter(|n| n.is(NodeKind::ConditionalExpression))
        .filter(|_| declaration.is(NodeKind::VariableDeclarationStatement))
    {
        let (Some(condition), Some(then), Some(other)) = (
            ce.child("expression"),
            ce.child("thenExpression"),
            ce.child("elseExpression"),
        ) else {
            return;
        };
        if force
            || !effects.iter().any(|n| conditional_effect(*n))
                && !conditional_effect(then)
                && !conditional_effect(other)
        {
            rw.remove(RNode::Orig(declaration.id));
            return;
        }
        let statement = rw.new_node(NodeKind::IfStatement);
        let condition = rw.create_copy_target(unparenthesed_expression(condition).id);
        rw.put_child(statement, "expression", condition);
        let mut branch = Vec::new();
        if conditional_effect(then) {
            let target = rw.create_copy_target(unparenthesed_expression(then).id);
            branch.push(rw.new_expression_statement(target));
        }
        let block = rw.new_block(branch);
        rw.put_child(statement, "thenStatement", block);
        if conditional_effect(other) {
            let target = rw.create_copy_target(unparenthesed_expression(other).id);
            let target = rw.new_expression_statement(target);
            let block = rw.new_block(vec![target]);
            rw.put_child(statement, "elseStatement", block);
        }
        rw.replace(RNode::Orig(declaration.id), Some(statement));
        return;
    }
    if fragments.len() == 1 {
        if force || declaration.is(NodeKind::FieldDeclaration) || effects.is_empty() {
            rw.remove(RNode::Orig(declaration.id));
        } else {
            let in_for = declaration
                .parent()
                .is_some_and(|n| n.is(NodeKind::ForStatement))
                && declaration.location_is("initializers");
            let replacements = effects
                .into_iter()
                .map(|n| {
                    let target = rw.create_move_target(n.id);
                    if in_for {
                        target
                    } else {
                        rw.new_expression_statement(target)
                    }
                })
                .collect();
            replace_many(rw, declaration, replacements);
        }
    } else if force || declaration.is(NodeKind::FieldDeclaration) {
        rw.remove(RNode::Orig(fragment.id));
    } else if declaration.is(NodeKind::VariableDeclarationStatement) {
        split_declarations(rw, fragment, declaration, effects);
        rw.remove(RNode::Orig(fragment.id));
    } else if declaration.is(NodeKind::VariableDeclarationExpression) && effects.is_empty() {
        rw.remove(RNode::Orig(fragment.id));
    }
}
fn remove_reference(rw: &mut ASTRewrite, reference: Node<'_>, force: bool) {
    let Some(mut parent) = reference.parent() else {
        return;
    };
    while parent.is(NodeKind::QualifiedName) {
        let Some(p) = parent.parent() else { return };
        parent = p;
    }
    if parent.is(NodeKind::FieldAccess) {
        let Some(p) = parent.parent() else { return };
        parent = p;
    }
    match parent.kind() {
        NodeKind::Assignment => {
            let (Some(rhs), Some(owner)) = (parent.child("rightHandSide"), parent.parent()) else {
                return;
            };
            if owner.is(NodeKind::ExpressionStatement) && !rhs.is(NodeKind::Assignment) {
                remove_with_initializer(rw, rhs, owner, force);
            } else {
                let copy = rw.create_copy_target(rhs.id);
                rw.replace(RNode::Orig(parent.id), Some(copy));
            }
        }
        NodeKind::SingleVariableDeclaration => rw.remove(RNode::Orig(parent.id)),
        NodeKind::VariableDeclarationFragment => remove_fragment(rw, parent, force),
        NodeKind::PostfixExpression | NodeKind::PrefixExpression => {
            if let Some(owner) = parent
                .parent()
                .filter(|n| n.is(NodeKind::ExpressionStatement))
            {
                remove_statement(rw, owner);
            } else {
                rw.remove(RNode::Orig(parent.id));
            }
        }
        _ => {}
    }
}
fn remove_param_tag(rw: &mut ASTRewrite, declaration: Node<'_>) {
    let Some(doc) = declaration
        .parent()
        .filter(|n| n.is(NodeKind::MethodDeclaration))
        .and_then(|n| n.child("javadoc"))
    else {
        return;
    };
    let Some(name) = declaration.child("name") else {
        return;
    };
    if let Some(tag) = doc.list("tags").into_iter().find(|n| {
        n.simple("tagName") == Some("@param")
            && n.list("fragments")
                .first()
                .is_some_and(|n| n.identifier() == name.identifier())
    }) {
        rw.remove(RNode::Orig(tag.id));
    }
}
fn remove_variable(rw: &mut ASTRewrite, binding: BindingRef<'_>, force: bool) {
    for reference in linked(binding) {
        remove_reference(rw, reference, force);
    }
    if let Some(declaration) = binding
        .variable_declaration()
        .unwrap_or(binding)
        .declaring_node()
        .filter(|n| n.is(NodeKind::SingleVariableDeclaration))
    {
        remove_param_tag(rw, declaration);
    }
}
fn remove_member(ctx: &Context, name: Node<'_>, force: bool) -> Option<Proposal> {
    let binding = name.binding()?;
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let key = match binding.kind() {
        BindingKind::Variable => {
            if name
                .parent()
                .filter(|n| n.is(NodeKind::SingleVariableDeclaration))
                .is_some_and(|n| {
                    n.location_is("parameter")
                        && n.parent()
                            .is_some_and(|p| p.is(NodeKind::EnhancedForStatement))
                })
            {
                return None;
            }
            remove_variable(&mut rw, binding, force);
            if force {
                "UnusedCodeFix_RemoveFieldOrLocalWithInitializer_description"
            } else {
                "UnusedCodeFix_RemoveFieldOrLocal_description"
            }
        }
        BindingKind::Method => {
            let declaration = binding
                .method_declaration()
                .unwrap_or(binding)
                .declaring_node()?;
            rw.remove(RNode::Orig(declaration.id));
            if binding.is_constructor() {
                "UnusedCodeFix_RemoveConstructor_description"
            } else {
                "UnusedCodeFix_RemoveMethod_description"
            }
        }
        BindingKind::Type => {
            let mut declaration = binding
                .type_declaration()
                .unwrap_or(binding)
                .declaring_node()?;
            if let Some(statement) = declaration
                .parent()
                .filter(|n| n.is(NodeKind::TypeDeclarationStatement))
            {
                declaration = statement;
            }
            rw.remove(RNode::Orig(declaration.id));
            "UnusedCodeFix_RemoveType_description"
        }
        _ => return None,
    };
    Some(Proposal::rewrite(
        messages::format(messages::fix(key), &[&name.identifier()]),
        kind::QUICK_FIX,
        relevance::UNUSED_MEMBER,
        rw,
    ))
}
fn visible_methods<'a>(
    typ: BindingRef<'a>,
    methods: &mut Vec<BindingRef<'a>>,
    seen: &mut HashSet<String>,
) {
    if !seen.insert(typ.key().into()) {
        return;
    }
    methods.extend(typ.declared_methods().unwrap_or_default());
    if let Some(parent) = typ.superclass() {
        visible_methods(parent, methods, seen);
    }
    for interface in typ.interfaces() {
        visible_methods(interface, methods, seen);
    }
}
fn remove_parameter(ctx: &Context, name: Node<'_>) -> Option<Proposal> {
    let parameter = name.ancestor_or_self(|k| k == NodeKind::SingleVariableDeclaration)?;
    let method = parameter.ancestor_or_self(|k| k == NodeKind::MethodDeclaration)?;
    let binding = method.binding()?;
    if binding.modifiers() & modifier::PRIVATE == 0 {
        return None;
    }
    // Upstream deliberately checks these three reference kinds.
    if ctx.ast.all_nodes().any(|n| {
        matches!(
            n.kind(),
            NodeKind::CreationReference
                | NodeKind::ExpressionMethodReference
                | NodeKind::TypeMethodReference
        ) && n.method_binding() == Some(binding)
    }) {
        return None;
    }
    let params = method.list("parameters");
    let index = params.iter().position(|n| n.id == parameter.id)?;
    let mut methods = Vec::new();
    visible_methods(
        binding.declaring_class()?,
        &mut methods,
        &mut HashSet::new(),
    );
    let conflicts: HashSet<_> = methods
        .into_iter()
        .filter(|m| {
            !(m.modifiers() & modifier::PRIVATE != 0
                && m.declaring_class() != binding.declaring_class())
                && m.name().starts_with(binding.name())
                && m.parameter_types().len() == params.len() - 1
        })
        .map(|m| m.name().to_owned())
        .collect();
    let mut renamed = binding.name().to_owned();
    let mut suffix = 1;
    while conflicts.contains(&renamed) {
        renamed = format!("{}{suffix}", binding.name());
        suffix += 1;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    for linked_name in linked(binding) {
        if let Some(invocation) = linked_name
            .parent()
            .filter(|n| n.is(NodeKind::MethodInvocation))
        {
            if let Some(argument) = invocation.list("arguments").get(index) {
                rw.remove(RNode::Orig(argument.id));
            }
        }
        if linked_name.identifier() != renamed {
            let new = rw.new_simple_name(&renamed);
            rw.replace(RNode::Orig(linked_name.id), Some(new));
        }
    }
    remove_variable(&mut rw, name.binding()?, false);
    Some(Proposal::rewrite(
        messages::format(
            messages::fix("UnusedCodeFix_RemoveParameter_description"),
            &[&name.identifier()],
        ),
        kind::QUICK_FIX,
        relevance::UNUSED_MEMBER,
        rw,
    ))
}
fn can_rename(name: Node<'_>) -> bool {
    let Some(parent) = name.parent() else {
        return false;
    };
    if parent.is(NodeKind::SingleVariableDeclaration) {
        parent
            .parent()
            .is_some_and(|n| n.kind().is_pattern() || n.is(NodeKind::EnhancedForStatement))
    } else if parent.is(NodeKind::VariableDeclarationFragment) {
        parent.parent().is_some_and(|n| {
            n.is(NodeKind::LambdaExpression)
                || n.is(NodeKind::VariableDeclarationExpression)
                    && n.parent().is_some_and(|p| {
                        matches!(p.kind(), NodeKind::TryStatement | NodeKind::ForStatement)
                    })
        })
    } else {
        false
    }
}
pub async fn proposals(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let Some(name) = unused_name(ctx, problem).filter(|n| n.binding().is_some()) else {
        return;
    };
    let id = problem.problem_id;
    if matches!(
        id,
        p::LocalVariableIsNeverUsed | p::LambdaParameterIsNeverUsed
    ) {
        let options = env.options(&ctx.ast.uri).await;
        let level = options
            .get("org.eclipse.jdt.core.compiler.source")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if level >= 22 && can_rename(name) {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let replacement = rw.new_simple_name("_");
            rw.replace(RNode::Orig(name.id), Some(replacement));
            proposals.push(Proposal::rewrite(
                messages::fix("UnusedCodeFix_RenameToUnnamedVariable_description"),
                kind::QUICK_FIX,
                relevance::UNUSED_MEMBER,
                rw,
            ));
        }
    }
    if id == p::LambdaParameterIsNeverUsed {
        return;
    }
    if id == p::ArgumentIsNeverUsed {
        crate::correction::javadoc_tags::unused_and_undocumented_parameter_or_exception_proposals(env, ctx, problem, proposals).await;
        if let Some(proposal) = remove_parameter(ctx, name) {
            proposals.push(proposal);
        }
    } else {
        if let Some(proposal) = remove_member(ctx, name, false) {
            proposals.push(proposal);
        }
        if id == p::LocalVariableIsNeverUsed {
            if let Some(proposal) = remove_member(ctx, name, true) {
                proposals.push(proposal);
            }
        }
    }
}
pub async fn type_parameter(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    if let Some(declaration) = unused_name(ctx, problem)
        .and_then(|n| n.binding())
        .and_then(|b| b.type_declaration().unwrap_or(b).declaring_node())
    {
        let declaration = declaration
            .parent()
            .filter(|n| n.is(NodeKind::TypeDeclarationStatement))
            .unwrap_or(declaration);
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        rw.remove(RNode::Orig(declaration.id));
        proposals.push(Proposal::rewrite(
            messages::fix("UnusedCodeFix_RemoveUnusedTypeParameter_description"),
            kind::QUICK_FIX,
            relevance::UNUSED_MEMBER,
            rw,
        ));
    }
    crate::correction::javadoc_tags::unused_and_undocumented_parameter_or_exception_proposals(env, ctx, problem, proposals).await;
}
