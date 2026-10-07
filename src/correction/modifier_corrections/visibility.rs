//! `getNonAccessibleReferenceProposal`, `getChangeOverriddenModifierProposal`
//! and their `Bindings` / `JdtFlags` helpers.

use std::collections::HashSet;

use super::change::ModifierChange;
use super::{TO_NON_FINAL, TO_NON_PRIVATE, TO_NON_STATIC, TO_STATIC, TO_VISIBLE};
use crate::correction::edit::Env;
use crate::correction::{messages, relevance, Context, ProblemLocation, Proposal};
use crate::semantic_ast::{modifier as m, problem as p, Ast, BindingKind, BindingRef, Node, NodeKind};

/// `ASTResolving.findCompilationUnitForBinding`: `Some(None)` is the
/// invocation's unit, `Some(Some(uri))` another unit of the project.
pub(crate) struct Units {
    files: Vec<String>,
}

impl Units {
    pub(crate) async fn load(env: &Env<'_>, uri: &str) -> Units {
        let files = match tower_lsp::lsp_types::Url::parse(uri) {
            Ok(u) => env.dispatcher.context_for(Some(&u)).await.files.into_keys().collect(),
            Err(_) => Vec::new(),
        };
        Units { files }
    }

    pub(crate) fn find(&self, ast: &Ast, binding: BindingRef<'_>) -> Option<Option<String>> {
        if !binding.is_from_source() || binding.is_type_variable() || binding.is_wildcard_type() {
            return None;
        }
        let decl = binding.type_declaration().unwrap_or(binding);
        if let Some(node) = decl.declaring_node().filter(|n| std::ptr::eq(n.ast, ast)) {
            if node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration) {
                return Some(None);
            }
            return None;
        }
        // Bindings.findCompilationUnit: the unit of the type's Java element.
        let key = decl.key().strip_prefix('L')?;
        let end = key.find([';', '<', '$', '~']).unwrap_or(key.len());
        let path = format!("/{}.java", &key[..end]);
        let mut matches: Vec<&String> = self.files.iter().filter(|f| f.ends_with(&path)).collect();
        matches.sort();
        matches.first().map(|f| Some((*f).clone()))
    }
}

/// `Bindings.getDeclaration(binding)` for types.
fn type_decl(b: BindingRef<'_>) -> BindingRef<'_> {
    b.type_declaration().unwrap_or(b)
}

/// `Bindings.getBindingOfParentType(node)`.
fn parent_type_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    node.ancestor_or_self(|k| k.is_abstract_type_declaration() || k == NodeKind::AnonymousClassDeclaration)
        .and_then(|n| n.binding())
}

/// `Bindings.isSuperType(possibleSuperType, type)`.
fn is_super_type(possible: BindingRef<'_>, t: BindingRef<'_>) -> bool {
    fn walk(possible: BindingRef<'_>, t: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(t.key().to_owned()) {
            return false;
        }
        if type_decl(t).key() == type_decl(possible).key() {
            return true;
        }
        if let Some(s) = t.superclass() {
            if walk(possible, s, seen) {
                return true;
            }
        }
        if possible.is_interface() {
            for i in t.interfaces() {
                if walk(possible, i, seen) {
                    return true;
                }
            }
        }
        false
    }
    walk(possible, t, &mut HashSet::new())
}

/// `JdtFlags.isInterfaceOrAnnotationMember(binding)`.
fn is_interface_or_annotation_member(b: BindingRef<'_>) -> bool {
    b.declaring_class().is_some_and(|d| d.is_interface() || d.is_annotation())
}

/// `JdtFlags.getVisibilityCode(binding)`.
pub(super) fn visibility_code(b: BindingRef<'_>) -> i32 {
    let mods = b.modifiers();
    if is_interface_or_annotation_member(b) || mods & m::PUBLIC != 0 {
        m::PUBLIC
    } else if mods & m::PROTECTED != 0 {
        m::PROTECTED
    } else if mods & m::PRIVATE != 0 {
        m::PRIVATE
    } else {
        0
    }
}

fn visibility_rank(v: i32) -> i32 {
    match v {
        m::PRIVATE => 0,
        m::PROTECTED => 2,
        m::PUBLIC => 3,
        _ => 1,
    }
}

/// `JdtFlags.getHigherVisibility(v1, v2)`.
fn higher_visibility(v1: i32, v2: i32) -> i32 {
    if visibility_rank(v1) > visibility_rank(v2) {
        v1
    } else {
        v2
    }
}

/// `ModifierCorrectionSubProcessorCore.getVisibilityString(code)`.
fn visibility_string(code: i32) -> &'static str {
    if code & m::PUBLIC != 0 {
        "public"
    } else if code & m::PROTECTED != 0 {
        "protected"
    } else if code & m::PRIVATE != 0 {
        "private"
    } else {
        messages::correction("ModifierCorrectionSubProcessor_default")
    }
}

/// `getNeededVisibility(currNode, targetType, binding)`.
fn needed_visibility(curr: Node<'_>, target: BindingRef<'_>, binding: BindingRef<'_>) -> i32 {
    let Some(curr_binding) = parent_type_binding(curr) else {
        // import
        return m::PUBLIC;
    };
    if is_super_type(target, curr_binding) {
        if binding.modifiers() & m::PROTECTED != 0 || binding.kind() == BindingKind::Type {
            return m::PUBLIC;
        }
        return m::PROTECTED;
    }
    if curr_binding.package_name() == target.package_name() {
        return 0;
    }
    m::PUBLIC
}

/// `ModifierCorrectionSubProcessorCore.getMethodLabel`.
fn method_label(b: BindingRef<'_>) -> String {
    format!("{}.{}", b.declaring_class().map(|c| c.name()).unwrap_or(""), b.name())
}

/// `getNonAccessibleReferenceProposal(context, problem, proposals, kind, relevance)`.
pub async fn non_accessible_reference(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, kind: i32, relevance: i32) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast) else { return };
    let binding = match selected.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName | NodeKind::SimpleType | NodeKind::NameQualifiedType => selected.binding(),
        NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation | NodeKind::FieldAccess | NodeKind::SuperFieldAccess => {
            selected.child("name").and_then(|n| n.binding())
        }
        NodeKind::ClassInstanceCreation | NodeKind::SuperConstructorInvocation => selected.method_binding(),
        _ => return,
    };
    let Some(mut binding) = binding else { return };
    if binding.is_variable() && problem.problem_id == p::NotVisibleType {
        let Some(t) = binding.var_type() else { return };
        binding = t;
    }
    if binding.is_method() && problem.problem_id == p::NotVisibleType {
        let Some(t) = binding.return_type() else { return };
        binding = t;
    }
    let type_binding: Option<BindingRef<'_>>;
    let name: String;
    let binding_decl: BindingRef<'_>;
    let mut is_local_var = false;
    if binding.is_method() {
        if binding.has(crate::semantic_ast::bflag::DEFAULT_CONSTRUCTOR) {
            crate::correction::unresolved_elements::constructor_proposals(env, ctx, problem, proposals).await;
            return;
        }
        binding_decl = binding.method_declaration().unwrap_or(binding);
        type_binding = binding.declaring_class();
        name = format!("{}()", binding.name());
    } else if binding.is_variable() {
        type_binding = binding.declaring_class();
        name = binding.name().to_owned();
        is_local_var = !binding.is_field();
        binding_decl = binding.variable_declaration().unwrap_or(binding);
    } else if binding.is_type() {
        type_binding = Some(binding);
        binding_decl = type_decl(binding);
        name = binding.name().to_owned();
    } else {
        return;
    }
    if type_binding.is_some_and(|t| t.is_from_source()) || is_local_var {
        let mut included = 0;
        let mut excluded = 0;
        let label = match kind {
            TO_VISIBLE => {
                excluded = m::PRIVATE | m::PROTECTED | m::PUBLIC;
                let Some(t) = type_binding else { return };
                included = needed_visibility(selected, t, binding);
                messages::format(messages::correction("ModifierCorrectionSubProcessor_changevisibility_description"), &[&name, visibility_string(included)])
            }
            TO_STATIC => {
                included = m::STATIC;
                if binding_decl.kind() == BindingKind::Method {
                    excluded = m::DEFAULT | m::ABSTRACT;
                }
                messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifiertostatic_description"), &[&name])
            }
            TO_NON_STATIC => {
                if type_binding.is_some_and(|t| t.is_interface()) {
                    return;
                }
                excluded = m::STATIC;
                messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifiertononstatic_description"), &[&name])
            }
            TO_NON_PRIVATE => {
                let unit_package = ast.root().child("package").and_then(|pk| pk.child("name")).map(|n| n.source_text()).unwrap_or_default();
                let visibility;
                if type_binding.is_some_and(|t| t.package_name().unwrap_or("") == unit_package) {
                    visibility = 0;
                    excluded = m::PRIVATE;
                } else {
                    visibility = m::PUBLIC;
                    included = m::PUBLIC;
                    excluded = m::PRIVATE | m::PROTECTED | m::PUBLIC;
                }
                messages::format(messages::correction("ModifierCorrectionSubProcessor_changevisibility_description"), &[&name, visibility_string(visibility)])
            }
            TO_NON_FINAL => {
                if type_binding.is_some_and(|t| t.is_interface()) {
                    return;
                }
                excluded = m::FINAL;
                messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifiertononfinal_description"), &[&name])
            }
            _ => return,
        };
        let target = match type_binding {
            Some(t) if !is_local_var => {
                let units = Units::load(env, &ast.uri).await;
                units.find(ast, type_decl(t))
            }
            _ => Some(None),
        };
        if let Some(target_uri) = target {
            let change = ModifierChange { source: ctx.ast.clone(), target_uri, binding: binding_decl.key().to_owned(), included, excluded };
            proposals.push(ModifierChange::proposal(label, relevance, change));
        }
    }
    // getVariableProposals (UnresolvedElementsSubProcessor.collectVariableProposals)
    if binding_decl.is_variable() {
        let super_ctor_arg = kind == TO_STATIC
            && problem.problem_id == p::InstanceFieldDuringConstructorInvocation
            && selected.is(NodeKind::SimpleName)
            && selected.location_is("arguments")
            && selected.parent().is_some_and(|p| p.is(NodeKind::SuperConstructorInvocation));
        if kind == TO_VISIBLE || super_ctor_arg {
            let key = binding_decl.key().to_owned();
            crate::correction::unresolved_elements::variable_proposals_for(env, ctx, problem, Some(&key), proposals).await;
        }
    }
}

/// `Bindings.isSubsignature(overriding, overridden)`.
fn is_subsignature(overriding: BindingRef<'_>, overridden: BindingRef<'_>) -> bool {
    overriding.name() == overridden.name() && overriding.data().method_subsignatures.contains(&overridden.id)
}

/// `Bindings.findOverriddenMethodInType(type, method)`.
pub(super) fn find_overridden_method_in_type<'a>(t: BindingRef<'a>, method: BindingRef<'a>) -> Option<BindingRef<'a>> {
    t.declared_methods().unwrap_or_default().into_iter().find(|curr| is_subsignature(method, *curr))
}

/// `Bindings.findOverriddenMethodInHierarchy(type, binding)`.
fn find_overridden_method_in_hierarchy<'a>(t: BindingRef<'a>, method: BindingRef<'a>) -> Option<BindingRef<'a>> {
    if let Some(r) = find_overridden_method_in_type(t, method) {
        return Some(r);
    }
    if let Some(s) = t.superclass() {
        if let Some(r) = find_overridden_method_in_hierarchy(s, method) {
            return Some(r);
        }
    }
    t.interfaces().into_iter().find_map(|i| find_overridden_method_in_hierarchy(i, method))
}

/// `Bindings.findOverriddenMethods(overriding, false, false)`.
fn find_overridden_methods<'a>(overriding: BindingRef<'a>) -> Vec<BindingRef<'a>> {
    let mut list = Vec::new();
    let mods = overriding.modifiers();
    if mods & (m::PRIVATE | m::STATIC) != 0 || overriding.is_constructor() {
        return list;
    }
    let Some(t) = overriding.declaring_class() else { return list };
    if let Some(s) = t.superclass() {
        if let Some(r) = find_overridden_method_in_hierarchy(s, overriding) {
            if r.modifiers() & m::PRIVATE == 0 {
                list.push(r);
            }
        }
    }
    for i in t.interfaces() {
        if let Some(r) = find_overridden_method_in_hierarchy(i, overriding) {
            list.push(r);
        }
    }
    list
}

/// `ModifierCorrectionSubProcessorCore.findOverriddenMethodInType(type, method, set)`
/// (the interfaces of `type`, recursively).
fn find_overridden_in_interfaces<'a>(t: BindingRef<'a>, method: BindingRef<'a>, out: &mut Vec<BindingRef<'a>>) {
    for i in t.interfaces() {
        if let Some(r) = find_overridden_method_in_type(i, method) {
            if !out.iter().any(|o| o.key() == r.key()) {
                out.push(r);
            }
        }
        find_overridden_in_interfaces(i, method, out);
    }
}

/// `getChangeOverriddenModifierProposal(context, problem, proposals, kind)`.
pub async fn change_overridden_modifier(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, kind: i32) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    let Some(method) = selected.binding() else { return };
    let Some(mut curr) = method.declaring_class() else { return };

    if kind == TO_VISIBLE && problem.problem_id != p::OverridingNonVisibleMethod {
        // e.g. IProblem.InheritedMethodReducesVisibility, IProblem.MethodReducesVisibility
        let methods = find_overridden_methods(method);
        if !methods.is_empty() {
            let mut included = 0;
            for b in &methods {
                included = higher_visibility(visibility_code(*b), included);
            }
            let excluded = m::PRIVATE | m::PROTECTED | m::PUBLIC;
            let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemethodvisibility_description"), &[visibility_string(included)]);
            let change = ModifierChange { source: ctx.ast.clone(), target_uri: None, binding: method.key().to_owned(), included, excluded };
            proposals.push(ModifierChange::proposal(label, relevance::CHANGE_OVERRIDDEN_MODIFIER_1, change));
        }
    }

    let mut overridden_in_class = None;
    let mut mother_classes = Vec::new();
    while overridden_in_class.is_none() {
        let Some(s) = curr.superclass() else { break };
        curr = s;
        mother_classes.push(curr);
        overridden_in_class = find_overridden_method_in_type(curr, method);
    }
    if overridden_in_class.is_none() {
        if let Some(c) = method.declaring_class() {
            mother_classes.insert(0, c);
        }
        let mut bindings = Vec::new();
        for mother in &mother_classes {
            find_overridden_in_interfaces(*mother, method, &mut bindings);
        }
        if let Some(first) = bindings.first().copied() {
            let decl = |b: BindingRef<'_>| b.method_declaration().unwrap_or(b).key().to_owned();
            overridden_in_class = bindings.iter().all(|b| decl(*b) == decl(first)).then_some(first);
        }
    }
    let Some(overridden) = overridden_in_class else { return };
    let overridden_decl = overridden.method_declaration().unwrap_or(overridden);
    let Some(declaring) = overridden_decl.declaring_class() else { return };
    let units = Units::load(env, &ast.uri).await;
    let Some(overridden_cu) = units.find(ast, declaring) else { return };

    let mut target_method = overridden_decl;
    let mut target_cu = overridden_cu;
    let (label, excluded, included) = match kind {
        TO_VISIBLE => {
            let (excluded, included);
            if method.modifiers() & m::PRIVATE != 0 {
                // Propose to increase the visibility of this method, because decreasing to private is not possible.
                target_method = method;
                target_cu = None;
                excluded = m::PRIVATE | m::PROTECTED | m::PUBLIC;
                included = visibility_code(overridden_decl);
            } else if visibility_code(method) == 0
                && declaring.package_name() != method.declaring_class().and_then(|c| c.package_name())
            {
                // method is package visible but not in the same package as overridden method
                excluded = m::PRIVATE;
                included = m::PROTECTED;
                if overridden_decl.modifiers() & m::PROTECTED != 0 {
                    return;
                }
            } else {
                excluded = m::PRIVATE | m::PROTECTED | m::PUBLIC;
                included = visibility_code(method);
                if visibility_code(overridden_decl) == visibility_code(method) {
                    // don't propose the same visibility it already has
                    return;
                }
            }
            let label = messages::format(
                messages::correction("ModifierCorrectionSubProcessor_changeoverriddenvisibility_description"),
                &[&method_label(target_method), visibility_string(included)],
            );
            (label, excluded, included)
        }
        TO_NON_FINAL => {
            let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemethodtononfinal_description"), &[&method_label(target_method)]);
            (label, m::FINAL, 0)
        }
        TO_NON_STATIC => {
            let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemethodtononstatic_description"), &[&method_label(target_method)]);
            (label, m::STATIC, 0)
        }
        _ => return,
    };
    let change = ModifierChange { source: ctx.ast.clone(), target_uri: target_cu, binding: target_method.key().to_owned(), included, excluded };
    proposals.push(ModifierChange::proposal(label, relevance::CHANGE_OVERRIDDEN_MODIFIER_2, change));
}

/// `getNonFinalLocalProposal` / `getNeedToEmulateProposal`.
pub fn make_final(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, relevance: i32) {
    let Some(selected) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::SimpleName)) else { return };
    let Some(binding) = selected.binding().filter(|b| b.is_variable()) else { return };
    let binding = binding.variable_declaration().unwrap_or(binding);
    let label = messages::format(messages::correction("ModifierCorrectionSubProcessor_changemodifiertofinal_description"), &[binding.name()]);
    let change = ModifierChange { source: ctx.ast.clone(), target_uri: None, binding: binding.key().to_owned(), included: m::FINAL, excluded: 0 };
    proposals.push(ModifierChange::proposal(label, relevance, change));
}

/// `getAddMethodModifierProposal(context, problem, proposals, modifier, label)`.
pub fn add_method_modifier(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, modifier: i32, label_key: &str) {
    let Some(selected) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    if modifier == m::STATIC {
        for annotation in selected.list("modifiers").into_iter().filter(|n| n.kind().is_annotation()) {
            let Some(type_name) = annotation.child("typeName") else { continue };
            if matches!(
                type_name.source_text().as_str(),
                "BeforeEach" | "AfterEach" | "BeforeAll" | "AfterAll" | "Before" | "After" | "Test" | "TestTemplate" | "TestFactory" | "ParameterizedTest" | "RepeatedTest"
            ) {
                let package = type_name.binding().and_then(|b| b.package_name()).unwrap_or("");
                if matches!(package, "org.junit" | "org.junit.jupiter.api" | "org.junit.jupiter.params") {
                    return;
                }
            }
        }
    }
    let Some(binding) = selected.binding() else { return };
    let binding = binding.method_declaration().unwrap_or(binding);
    let label = messages::correction(label_key).to_owned();
    let change = ModifierChange { source: ctx.ast.clone(), target_uri: None, binding: binding.key().to_owned(), included: modifier, excluded: 0 };
    proposals.push(ModifierChange::proposal(label, relevance::ADD_METHOD_MODIFIER, change));
}
