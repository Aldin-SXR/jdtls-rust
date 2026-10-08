//! Port of `LambdaExpressionsFixCore`: anonymous class creation to lambda
//! expression and lambda expression to anonymous class creation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::extract_temp::{import_context, CuRewrite};
use super::scope::{self, ScopeAnalyzer};
use crate::correction::local_corrections::conversion::{target, target_ambiguous};
use crate::correction::type_mismatch::bindings::{is_super_type, normalize_wildcard_type};
use crate::rewrite::import_rewrite::TypeLocation;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::unparenthesed_expression;
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

fn functional_method<'a>(t: BindingRef<'a>) -> Option<BindingRef<'a>> {
    t.data().functional_method.map(|id| t.ast.binding(id))
}

fn is_generic_method(m: BindingRef<'_>) -> bool {
    !m.type_parameters().is_empty()
}

fn find_field_declaration(node: Node<'_>, stop_at_methods: bool) -> Option<Node<'_>> {
    let original = node;
    let mut current = Some(node);
    while let Some(n) = current {
        if n.is(NodeKind::FieldDeclaration) {
            return Some(n);
        }
        if n.kind().is_abstract_type_declaration() {
            return None;
        }
        if stop_at_methods && n.is(NodeKind::MethodDeclaration) && n.id != original.id {
            return None;
        }
        current = n.parent();
    }
    None
}

/// `SuperThisReferenceFinder.hasReference`.
fn has_super_this_reference(method: Node<'_>) -> bool {
    let Some(cic) = method.parent().and_then(|p| p.parent()) else { return false };
    let functional_interface = cic.child("type").and_then(|t| t.binding());
    let mut found = false;
    super::walk(method, &mut |n| {
        if found {
            return false;
        }
        match n.kind() {
            NodeKind::AnonymousClassDeclaration => return false,
            NodeKind::MethodDeclaration => return n.id == method.id,
            k if k.is_body_declaration() => return false,
            NodeKind::ThisExpression if n.child("qualifier").is_none() => found = true,
            NodeKind::SuperMethodInvocation => match n.child("qualifier") {
                None => found = true,
                Some(q) => {
                    if q.binding().is_some_and(|b| b.is_type() && b.is_interface()) {
                        found = true;
                    }
                }
            },
            NodeKind::SuperFieldAccess if n.child("qualifier").is_none() => found = true,
            NodeKind::MethodInvocation => {
                if let (Some(binding), Some(fi)) = (n.method_binding(), functional_interface) {
                    if !binding.is_static() && n.child("expression").is_none() && binding.declaring_class().is_some_and(|d| is_super_type(d, fi, false)) {
                        found = true;
                    }
                }
            }
            _ => {}
        }
        !found
    });
    found
}

/// `FinalFieldAccessInFieldDeclarationFinder.hasReference`.
fn has_final_field_access_in_field_declaration(method: Node<'_>) -> bool {
    if find_field_declaration(method, false).is_none() {
        return false;
    }
    let check = |binding: Option<BindingRef<'_>>| -> bool {
        let Some(b) = binding.filter(|b| b.is_variable() && b.is_field() && b.modifiers() & modifier::FINAL != 0) else { return false };
        b.declaring_node().is_some_and(|d| matches!(d.kind(), NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration) && d.child("initializer").is_none())
    };
    let mut found = false;
    super::walk(method, &mut |n| {
        if found {
            return false;
        }
        match n.kind() {
            NodeKind::AnonymousClassDeclaration => return false,
            NodeKind::MethodDeclaration => return n.id == method.id,
            k if k.is_body_declaration() => return false,
            NodeKind::SuperFieldAccess | NodeKind::FieldAccess => {
                let binding = n.child("name").and_then(|x| x.binding()).and_then(|b| b.variable_declaration().or(Some(b)));
                found = check(binding);
            }
            NodeKind::SimpleName | NodeKind::QualifiedName => found = check(n.binding()),
            _ => {}
        }
        !found
    });
    found
}

/// `MethodRecursionFinder.isRecursiveLocal`.
fn is_recursive_local(method: Node<'_>) -> bool {
    if find_field_declaration(method, true).is_some() {
        return false;
    }
    let Some(binding) = method.binding() else { return false };
    let mut found = false;
    super::walk(method, &mut |n| {
        if n.is(NodeKind::MethodInvocation) && n.child("expression").is_none() && n.method_binding() == Some(binding) {
            found = true;
        }
        !found
    });
    found
}

/// `isFunctionalAnonymous(node)`: `Some(conversion removes annotations)`.
pub fn functional_anonymous(cic: Node<'_>) -> Option<bool> {
    let type_binding = cic.type_binding()?;
    let interfaces = type_binding.interfaces();
    if interfaces.len() != 1 {
        return None;
    }
    let fm = functional_method(interfaces[0])?;
    if is_generic_method(fm) {
        return None;
    }
    let anonymous = cic.child("anonymousClassDeclaration")?;
    anonymous.binding()?;
    let declarations = anonymous.list("bodyDeclarations");
    if declarations.len() != 1 || !declarations[0].is(NodeKind::MethodDeclaration) {
        return None;
    }
    let method = declarations[0];
    let method_binding = method.binding()?;
    if is_generic_method(method_binding) {
        return None;
    }
    let modifiers = method_binding.modifiers() | method.modifiers();
    if modifiers & modifier::SYNCHRONIZED != 0 || modifiers & modifier::STRICTFP != 0 {
        return None;
    }
    if has_super_this_reference(method) || has_final_field_access_in_field_declaration(method) {
        return None;
    }
    target(cic)?;
    if is_recursive_local(method) {
        return None;
    }
    let removes = method_binding.data().annotations.iter().any(|a| !matches!(cic.ast.binding(a.annotation_type).qualified_name(), "java.lang.Override" | "java.lang.Deprecated"));
    Some(removes)
}

fn is_same_identifier(method: Node<'_>, index: usize, argument: Node<'_>) -> bool {
    let parameters = method.list("parameters");
    parameters.get(index).and_then(|p| p.child("name")).map(|n| n.identifier()) == Some(argument.identifier())
}

fn are_same_identifiers(method: Node<'_>, arguments: &[Node<'_>]) -> bool {
    for (i, _) in method.list("parameters").iter().enumerate() {
        let Some(argument) = arguments.get(i) else { return false };
        let e = unparenthesed_expression(*argument);
        if !e.is(NodeKind::SimpleName) || !is_same_identifier(method, i, e) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MethodRefStatus {
    NoRef,
    TypeRef,
    MethodRef,
}

fn is_static_invocation(mi: Node<'_>) -> Option<bool> {
    if let Some(b) = mi.method_binding() {
        return Some(b.is_static());
    }
    mi.child("expression").filter(|e| matches!(e.kind(), NodeKind::SimpleName | NodeKind::QualifiedName)).and_then(|e| e.binding()).filter(|b| b.is_type()).map(|_| true)
}

/// `ASTNodes.usesGivenSignature(methodBinding, type, name, params)` for
/// concrete parameter types.
fn uses_given_signature(method: BindingRef<'_>, type_name: &str, name: &str, parameters: &[String]) -> bool {
    if method.name() != name || method.parameter_types().len() != parameters.len() {
        return false;
    }
    let declaring = method.declaring_class();
    let implemented = declaring.map(|d| {
        let mut seen = Vec::new();
        collect_hierarchy(d, &mut seen);
        seen
    });
    let implements = implemented.is_some_and(|h| h.iter().any(|t| t.erasure().unwrap_or(*t).qualified_name() == type_name));
    if implements && method.parameter_types().iter().zip(parameters).all(|(p, n)| p.qualified_name() == n) {
        return true;
    }
    if let Some(declaration) = method.method_declaration().filter(|d| *d != method) {
        return uses_given_signature(declaration, type_name, name, parameters);
    }
    false
}

fn collect_hierarchy<'a>(t: BindingRef<'a>, out: &mut Vec<BindingRef<'a>>) {
    if out.iter().any(|x| *x == t) {
        return;
    }
    out.push(t);
    if let Some(s) = t.superclass() {
        collect_hierarchy(s, out);
    }
    for i in t.interfaces() {
        collect_hierarchy(i, out);
    }
}

fn sub_type_compatible(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    a == b || a.data().assignment_targets.contains(&b.id) || is_super_type(b, a, true)
}

/// `checkMethodInvocation`: `None` when the status is `null`.
fn check_method_invocation<'a>(visited: Node<'a>, invocation: Node<'a>) -> Option<(MethodRefStatus, Option<BindingRef<'a>>)> {
    let called_expression = invocation.child("expression");
    let arguments = invocation.list("arguments");
    let parameters = visited.list("parameters");
    let mut action = MethodRefStatus::NoRef;
    let mut class_binding = None;
    if parameters.len() == arguments.len() {
        if are_same_identifiers(visited, &arguments) {
            let method_binding = invocation.method_binding();
            let mut called_type = None;
            if let Some(mb) = method_binding {
                called_type = mb.declaring_class();
            } else if let Some(e) = called_expression {
                called_type = e.type_binding();
            } else if let Some(enclosing) = visited.ancestors().find(|a| a.kind().is_abstract_type_declaration()).and_then(|t| t.binding()) {
                let type_arguments = invocation.list("typeArguments");
                let mut names = vec![String::new(); arguments.len()];
                for (i, t) in type_arguments.iter().enumerate() {
                    let b = t.binding()?;
                    if let Some(slot) = names.get_mut(i) {
                        *slot = b.qualified_name().to_owned();
                    }
                }
                let name = invocation.child("name").map(|n| n.identifier()).unwrap_or_default();
                if enclosing.declared_methods().unwrap_or_default().iter().any(|m| uses_given_signature(*m, enclosing.qualified_name(), &name, &names)) {
                    called_type = Some(enclosing);
                }
            }
            if is_static_invocation(invocation) == Some(true) {
                if let Some(called) = called_type {
                    let mut valid = true;
                    if let Some(first) = arguments.first().and_then(|a| a.type_binding()) {
                        if sub_type_compatible(first, called) {
                            let mut remaining = Vec::new();
                            for a in &arguments[1..] {
                                match a.type_binding() {
                                    Some(t) => remaining.push(t.qualified_name().to_owned()),
                                    None => {
                                        valid = false;
                                        break;
                                    }
                                }
                            }
                            if valid {
                                let name = invocation.child("name").map(|n| n.identifier()).unwrap_or_default();
                                if called.declared_methods().unwrap_or_default().iter().any(|m| !m.is_static() && uses_given_signature(*m, called.qualified_name(), &name, &remaining)) {
                                    valid = false;
                                }
                            }
                        }
                    }
                    if valid {
                        action = MethodRefStatus::TypeRef;
                        class_binding = Some(called);
                    }
                }
            }
            if action != MethodRefStatus::TypeRef {
                match called_expression {
                    None => {
                        if let Some(called) = called_type {
                            let enclosing = visited.parent().and_then(|p| p.parent()).and_then(scope::binding_of_parent_type);
                            if enclosing.is_some_and(|e| is_super_type(called, e, true)) {
                                action = MethodRefStatus::MethodRef;
                            }
                        }
                    }
                    Some(e) => match e.kind() {
                        NodeKind::StringLiteral | NodeKind::NumberLiteral | NodeKind::ThisExpression => action = MethodRefStatus::MethodRef,
                        NodeKind::FieldAccess | NodeKind::SuperFieldAccess => {
                            if e.child("name").and_then(|n| n.binding()).is_some_and(|b| b.has(crate::semantic_ast::bflag::EFFECTIVELY_FINAL)) {
                                action = MethodRefStatus::MethodRef;
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    } else if called_expression.is_some_and(|e| e.is(NodeKind::SimpleName)) && parameters.len() == arguments.len() + 1 {
        let called = called_expression.expect("expression");
        if is_same_identifier(visited, 0, called) {
            let mut valid = true;
            for (i, a) in arguments.iter().enumerate() {
                let e = unparenthesed_expression(*a);
                if !e.is(NodeKind::SimpleName) || !is_same_identifier(visited, i + 1, e) {
                    valid = false;
                    break;
                }
            }
            if valid {
                let class = called.type_binding().or_else(|| invocation.method_binding().and_then(|m| m.declaring_class()));
                if let Some(class) = class {
                    let mut cumulative = vec![class.qualified_name().to_owned()];
                    for a in &arguments {
                        match a.type_binding() {
                            Some(t) => cumulative.push(t.qualified_name().to_owned()),
                            None => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if valid {
                        let name = invocation.child("name").map(|n| n.identifier()).unwrap_or_default();
                        if class.declared_methods().unwrap_or_default().iter().any(|m| m.is_static() && uses_given_signature(*m, class.qualified_name(), &name, &cumulative)) {
                            valid = false;
                        }
                    }
                    if valid {
                        action = MethodRefStatus::TypeRef;
                        class_binding = Some(class);
                    }
                }
            }
        }
    }
    Some((action, class_binding))
}

/// `ASTNodeFactory.newCreationType`.
fn new_creation_type(cu: &mut CuRewrite, binding: BindingRef<'_>, context: &dyn crate::rewrite::import_rewrite::ImportRewriteContext) -> RNode {
    if binding.is_parameterized_type() {
        let declaration = binding.type_declaration().unwrap_or(binding);
        let base = new_creation_type(cu, declaration, context);
        let parameterized = cu.rewrite.new_node(NodeKind::ParameterizedType);
        cu.rewrite.put_child(parameterized, "type", base);
        let mut arguments = Vec::new();
        for argument in binding.type_arguments() {
            let argument = replace_wildcards_and_captures(argument);
            match argument {
                Some(a) => arguments.push(cu.imports.add_import_type(a, &mut cu.rewrite, context, TypeLocation::TypeArgument)),
                None => {
                    let n = cu.rewrite.new_simple_name("Object");
                    arguments.push(cu.rewrite.new_simple_type(n));
                }
            }
        }
        cu.rewrite.put_list(parameterized, "typeArguments", arguments);
        parameterized
    } else {
        cu.imports.add_import_type(binding, &mut cu.rewrite, context, TypeLocation::New)
    }
}

/// `StubUtility2Core.replaceWildcardsAndCaptures`.
fn replace_wildcards_and_captures(t: BindingRef<'_>) -> Option<BindingRef<'_>> {
    let mut current = t;
    while current.is_wildcard_type() || current.is_capture() {
        match normalize_wildcard_type(current, true) {
            Some(n) if n != current => current = n,
            Some(n) => return Some(n),
            None => return crate::correction::type_mismatch::bindings::well_known(current.ast, "java.lang.Object"),
        }
    }
    Some(current)
}

fn copy_type(cu: &mut CuRewrite, ast: &Arc<Ast>, options: &BTreeMap<String, String>, node: Node<'_>, binding: BindingRef<'_>) -> RNode {
    let context = import_context(ast, node, options);
    let modified = if binding.type_parameters().is_empty() {
        if binding.is_capture() {
            binding.type_bounds().first().copied().or_else(|| binding.erasure()).unwrap_or(binding)
        } else {
            binding.erasure().unwrap_or(binding)
        }
    } else {
        binding
    };
    new_creation_type(cu, modified, &context)
}

fn has_annotations(node: Node<'_>) -> bool {
    let mut found = false;
    super::walk(node, &mut |n| {
        if matches!(n.kind(), NodeKind::MarkerAnnotation | NodeKind::NormalAnnotation | NodeKind::SingleMemberAnnotation) {
            found = true;
        }
        !found
    });
    found
}

fn collect_inherited_types<'a>(t: Option<BindingRef<'a>>, out: &mut Vec<BindingRef<'a>>) {
    let Some(t) = t else { return };
    if let Some(mother) = t.superclass() {
        out.push(mother);
        collect_inherited_types(Some(mother), out);
    }
    for i in t.interfaces() {
        out.push(i);
        collect_inherited_types(Some(i), out);
    }
}

fn is_member_class(declaring: Option<BindingRef<'_>>, cic_binding: Option<BindingRef<'_>>) -> bool {
    let Some(cic) = cic_binding else { return false };
    let mut current = declaring;
    while let Some(c) = current {
        if c == cic {
            return true;
        }
        current = c.declaring_class();
    }
    false
}

struct LambdaOperation<'a> {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    simplify: bool,
    expressions: Vec<Node<'a>>,
}

/// `needCastForWildcardArgument`.
fn need_cast_for_wildcard_argument(cic: Node<'_>) -> bool {
    if !cic.location_is("arguments") {
        return false;
    }
    let Some(parent) = cic.parent().filter(|p| p.is(NodeKind::MethodInvocation)) else { return false };
    let arguments = parent.list("arguments");
    let Some(binding) = parent.method_binding() else { return false };
    let parameter_types = binding.parameter_types();
    let Some(index) = arguments.iter().position(|a| a.id == cic.id) else { return false };
    let parameter = if index < parameter_types.len() { Some(parameter_types[index]) } else { parameter_types.last().and_then(|p| p.component_type()) };
    let Some(parameter) = parameter else { return false };
    let Some(instance) = cic.child("type").and_then(|t| t.binding()) else { return false };
    let instance_arguments = instance.type_arguments();
    for (i, argument) in parameter.type_arguments().iter().enumerate() {
        if argument.is_wildcard_type() {
            let bound = argument.bound().or_else(|| argument.erasure());
            if let (Some(bound), Some(candidate)) = (bound, instance_arguments.get(i)) {
                if !sub_type_compatible(bound, *candidate) {
                    return true;
                }
            }
        }
    }
    false
}

impl<'a> LambdaOperation<'a> {
    /// `CreateLambdaOperation.rewriteAST`.
    fn rewrite(&self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let mut names_by_cic: HashMap<usize, HashSet<String>> = HashMap::new();
        for (i, cic) in self.expressions.iter().enumerate() {
            let Some(anonymous) = cic.child("anonymousClassDeclaration") else { continue };
            let declarations = anonymous.list("bodyDeclarations");
            let Some(method) = declarations.first().copied().filter(|d| d.is(NodeKind::MethodDeclaration)) else { continue };
            let mut excluded: HashSet<String> = HashSet::new();
            for (j, converted) in self.expressions[..i].iter().enumerate() {
                if converted.is_ancestor_or_self_of(*cic) && converted.id != cic.id {
                    excluded.extend(names_by_cic.get(&j).cloned().unwrap_or_default());
                }
            }
            let new_names = self.make_names_unique(cu, excluded, method);
            names_by_cic.insert(i, new_names);
            let parameters = method.list("parameters");
            let Some(body) = method.child("body") else { continue };
            let statements = body.list("statements");
            let mut lambda_body = body;
            if statements.len() == 1 {
                let statement = statements[0];
                if statement.extended_start() + statement.extended_length() <= statement.start() + statement.length() {
                    if statement.is(NodeKind::ExpressionStatement) {
                        lambda_body = statement.child("expression").unwrap_or(body);
                    } else if statement.is(NodeKind::ReturnStatement) {
                        if let Some(e) = statement.child("expression") {
                            lambda_body = e;
                        }
                    }
                }
            }
            let explicit_parameters = parameters.iter().any(|p| has_annotations(*p));
            let mut replacement = self.simplified_replacement(cu, *cic, method, lambda_body);
            let replacement = match replacement.take() {
                Some(r) => r,
                None => self.create_lambda(cu, *cic, anonymous, &parameters, lambda_body, explicit_parameters),
            };
            let explicit_lambda = cu.rewrite.kind(replacement) == NodeKind::LambdaExpression && {
                let params = cu.rewrite.new_value(replacement, "parameters").list();
                params.is_empty() || cu.rewrite.kind(params[0]) == NodeKind::SingleVariableDeclaration
            };
            let target_binding = target(*cic);
            let needs_cast = need_cast_for_wildcard_argument(*cic) || target_ambiguous(*cic, explicit_lambda) || target_binding.is_none_or(|t| functional_method(t).is_none());
            let mut result = replacement;
            if needs_cast {
                let cast = cu.rewrite.new_node(NodeKind::CastExpression);
                cu.rewrite.put_child(cast, "expression", result);
                let context = import_context(&ast, *cic, &self.options);
                if let Some(binding) = cic.child("type").and_then(|t| t.binding()) {
                    let cast_type = cu.imports.add_import_type(binding, &mut cu.rewrite, &context, TypeLocation::Cast);
                    cu.remover.register_added_imports(&cu.rewrite, cast_type);
                    cu.rewrite.put_child(cast, "type", cast_type);
                }
                result = cast;
            }
            cu.rewrite.replace(RNode::Orig(cic.id), Some(result));
            cu.remover.register_removed_node(cic.id);
            cu.remover.register_retained_node(lambda_body.id);
        }
    }

    fn create_lambda(&self, cu: &mut CuRewrite, cic: Node<'a>, anonymous: Node<'a>, parameters: &[Node<'a>], lambda_body: Node<'a>, explicit_parameters: bool) -> RNode {
        let ast = self.ast.clone();
        let lambda = cu.rewrite.new_node(NodeKind::LambdaExpression);
        cu.rewrite.put_simple(lambda, "parentheses", if explicit_parameters || parameters.len() != 1 { "true" } else { "false" });
        let mut lambda_parameters = Vec::new();
        for p in parameters {
            if explicit_parameters {
                lambda_parameters.push(cu.rewrite.create_copy_target(p.id));
                cu.remover.register_retained_node(p.id);
            } else {
                let fragment = cu.rewrite.new_node(NodeKind::VariableDeclarationFragment);
                let name = p.child("name").expect("parameter name");
                let copy = cu.rewrite.create_copy_target(name.id);
                cu.rewrite.put_child(fragment, "name", copy);
                lambda_parameters.push(fragment);
            }
        }
        cu.rewrite.put_list(lambda, "parameters", lambda_parameters);

        let mut inherited = Vec::new();
        collect_inherited_types(anonymous.binding(), &mut inherited);
        let cic_binding = cic.child("type").and_then(|t| t.binding());
        self.qualify_inherited_statics(cu, lambda_body, &inherited, cic_binding);
        self.qualify_field_forward_references(cu, cic, lambda_body);
        let body = copy_or_replacement(&mut cu.rewrite, lambda_body);
        cu.rewrite.put_child(lambda, "body", body);
        let _ = ast;
        lambda
    }

    /// The `inheritedFieldsVisitor`.
    fn qualify_inherited_statics(&self, cu: &mut CuRewrite, lambda_body: Node<'a>, inherited: &[BindingRef<'a>], cic_binding: Option<BindingRef<'a>>) {
        let cic_name = cic_binding.map(|b| b.name().to_owned());
        super::walk(lambda_body, &mut |n| {
            match n.kind() {
                NodeKind::SimpleName => {
                    let parent = n.parent();
                    let qualified_part = n.location_is("name") && parent.is_some_and(|p| matches!(p.kind(), NodeKind::QualifiedName | NodeKind::FieldAccess | NodeKind::SuperFieldAccess));
                    if qualified_part {
                        return true;
                    }
                    if let Some(binding) = n.binding().filter(|b| b.is_variable()) {
                        if binding.modifiers() & modifier::STATIC != 0 && binding.is_field() && binding.declaring_class().is_some_and(|d| inherited.contains(&d)) {
                            if let Some(name) = &cic_name {
                                let replacement = cu.rewrite.new_name(&format!("{}.{}", name, n.identifier()));
                                cu.rewrite.replace(RNode::Orig(n.id), Some(replacement));
                            }
                            return false;
                        }
                    }
                    true
                }
                NodeKind::QualifiedName => {
                    if let Some(binding) = n.binding().filter(|b| b.is_variable()) {
                        if binding.modifiers() & modifier::STATIC != 0
                            && (binding.is_field() || binding.is_enum_constant())
                            && binding.declaring_class().is_some_and(|d| inherited.contains(&d) || is_member_class(Some(d), cic_binding))
                        {
                            let full = n.source_text();
                            if let Some(name) = &cic_name {
                                if !full.starts_with(&format!("{name}.")) {
                                    let replacement = cu.rewrite.new_name(&format!("{name}.{full}"));
                                    cu.rewrite.replace(RNode::Orig(n.id), Some(replacement));
                                }
                            }
                        }
                    }
                    false
                }
                NodeKind::FieldAccess => {
                    if let Some(binding) = n.child("name").and_then(|x| x.binding()).filter(|b| b.is_variable()) {
                        if binding.modifiers() & modifier::STATIC != 0 && binding.is_field() {
                            if let Some(expression) = n.child("expression").filter(|e| matches!(e.kind(), NodeKind::SimpleName | NodeKind::QualifiedName)) {
                                if binding.declaring_class().is_some_and(|d| inherited.contains(&d) || is_member_class(Some(d), cic_binding)) {
                                    let full = expression.source_text();
                                    if let (Some(name), Some(field)) = (&cic_name, n.child("name")) {
                                        if !full.starts_with(&format!("{name}.")) {
                                            let replacement = cu.rewrite.new_name(&format!("{name}.{full}.{}", field.identifier()));
                                            cu.rewrite.replace(RNode::Orig(n.id), Some(replacement));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    false
                }
                _ => true,
            }
        });
    }

    /// The field declaration `visitor`: references to later fields and to
    /// the declaring method need qualification.
    fn qualify_field_forward_references(&self, cu: &mut CuRewrite, cic: Node<'a>, lambda_body: Node<'a>) {
        let fragment = cic.ancestors().find(|a| a.is(NodeKind::VariableDeclarationFragment) || a.kind().is_body_declaration());
        let Some(fragment) = fragment.filter(|f| f.is(NodeKind::VariableDeclarationFragment)) else { return };
        let Some(field) = fragment.parent().filter(|p| p.is(NodeKind::FieldDeclaration)) else { return };
        let Some(declaration_class) = field.ancestors().find(|a| a.is(NodeKind::TypeDeclaration)) else { return };
        let fields: Vec<Node<'_>> = declaration_class.list("bodyDeclarations").into_iter().filter(|d| d.is(NodeKind::FieldDeclaration)).collect();
        let next_fields: Vec<Node<'_>> = match fields.iter().position(|f| f.id == field.id) {
            Some(i) => fields[i..].to_vec(),
            None => Vec::new(),
        };
        let method_declaration = cic.child("anonymousClassDeclaration").and_then(|a| a.list("bodyDeclarations").first().copied());
        let static_field = field.modifiers() & modifier::STATIC != 0;
        let class_name = declaration_class.child("name");
        let fragment_name = fragment.child("name");
        let ast = self.ast.clone();
        let type_binding = declaration_class.binding();
        super::walk(lambda_body, &mut |n| {
            match n.kind() {
                NodeKind::MethodInvocation => {
                    let field_type = field.child("type").and_then(|t| t.binding());
                    let declaration = n.method_binding().and_then(|b| b.declaring_node()).filter(|d| d.ancestors().any(|a| a.id == declaration_class.id));
                    if n.child("expression").is_none() && field_type.is_some() && method_declaration.is_some_and(|m| declaration.is_some_and(|d| d.id == m.id)) {
                        let replacement = if static_field {
                            match (class_name, fragment_name) {
                                (Some(c), Some(f)) => {
                                    let cn = cu.rewrite.create_copy_target(c.id);
                                    let fnm = cu.rewrite.create_copy_target(f.id);
                                    let q = cu.rewrite.new_node(NodeKind::QualifiedName);
                                    cu.rewrite.put_child(q, "qualifier", cn);
                                    cu.rewrite.put_child(q, "name", fnm);
                                    Some(q)
                                }
                                _ => None,
                            }
                        } else if let Some(f) = fragment_name {
                            let this = cu.rewrite.new_this_expression();
                            let name = cu.rewrite.create_copy_target(f.id);
                            Some(cu.rewrite.new_field_access(this, name))
                        } else {
                            None
                        };
                        if let Some(r) = replacement {
                            cu.rewrite.set(RNode::Orig(n.id), "expression", Some(r));
                        }
                        return false;
                    }
                    true
                }
                NodeKind::SimpleName => {
                    let qualified_part = n.location_is("name") && n.parent().is_some_and(|p| matches!(p.kind(), NodeKind::QualifiedName | NodeKind::FieldAccess | NodeKind::SuperFieldAccess));
                    if !qualified_part {
                        let declaration = n.binding().and_then(|b| b.declaring_node()).filter(|d| d.ancestors().any(|a| a.id == declaration_class.id));
                        if let Some(declaration) = declaration.filter(|d| d.is(NodeKind::VariableDeclarationFragment)) {
                            if let Some(current) = declaration.parent().filter(|p| p.is(NodeKind::FieldDeclaration)) {
                                if next_fields.iter().any(|f| f.id == current.id) {
                                    if current.modifiers() & modifier::STATIC != 0 {
                                        if let Some(c) = class_name {
                                            let cn = cu.rewrite.create_copy_target(c.id);
                                            let moved = cu.rewrite.create_move_target(n.id);
                                            let q = cu.rewrite.new_node(NodeKind::QualifiedName);
                                            cu.rewrite.put_child(q, "qualifier", cn);
                                            cu.rewrite.put_child(q, "name", moved);
                                            cu.rewrite.replace(RNode::Orig(n.id), Some(q));
                                        }
                                    } else {
                                        let this = cu.rewrite.new_this_expression();
                                        let moved = cu.rewrite.create_move_target(n.id);
                                        let access = cu.rewrite.new_field_access(this, moved);
                                        cu.rewrite.replace(RNode::Orig(n.id), Some(access));
                                    }
                                    return false;
                                }
                            }
                        }
                    }
                    true
                }
                NodeKind::ThisExpression => {
                    if let Some(q) = n.child("qualifier") {
                        if q.binding().is_some_and(|b| b.is_type()) && q.binding() == type_binding {
                            cu.rewrite.remove(RNode::Orig(q.id));
                        }
                    }
                    true
                }
                _ => true,
            }
        });
        let _ = ast;
    }

    /// The method reference conversions (`fSimplifyLambda`).
    fn simplified_replacement(&self, cu: &mut CuRewrite, cic: Node<'a>, method: Node<'a>, lambda_body: Node<'a>) -> Option<RNode> {
        if !self.simplify {
            return None;
        }
        let ast = self.ast.clone();
        match lambda_body.kind() {
            NodeKind::MethodInvocation => {
                let (status, class_binding) = check_method_invocation(method, lambda_body)?;
                match status {
                    MethodRefStatus::MethodRef => {
                        let reference = cu.rewrite.new_node(NodeKind::ExpressionMethodReference);
                        let expression = match lambda_body.child("expression") {
                            Some(e) => cu.rewrite.create_move_target(e.id),
                            None => cu.rewrite.new_this_expression(),
                        };
                        cu.rewrite.put_child(reference, "expression", expression);
                        let name = cu.rewrite.create_move_target(lambda_body.child("name")?.id);
                        cu.rewrite.put_child(reference, "name", name);
                        Some(self.cast_method_ref_if_needed(cu, reference, cic))
                    }
                    MethodRefStatus::TypeRef => {
                        let reference = cu.rewrite.new_node(NodeKind::TypeMethodReference);
                        let typ = copy_type(cu, &ast, &self.options, lambda_body, class_binding?);
                        cu.rewrite.put_child(reference, "type", typ);
                        let name = cu.rewrite.create_move_target(lambda_body.child("name")?.id);
                        cu.rewrite.put_child(reference, "name", name);
                        Some(self.cast_method_ref_if_needed(cu, reference, cic))
                    }
                    MethodRefStatus::NoRef => None,
                }
            }
            NodeKind::ClassInstanceCreation => {
                let arguments = lambda_body.list("arguments");
                if method.list("parameters").len() == arguments.len() && are_same_identifiers(method, &arguments) && cic.child("anonymousClassDeclaration").is_none() {
                    let reference = cu.rewrite.new_node(NodeKind::CreationReference);
                    let binding = cic.type_binding()?;
                    let typ = copy_type(cu, &ast, &self.options, cic, binding);
                    cu.rewrite.put_child(reference, "type", typ);
                    Some(reference)
                } else {
                    None
                }
            }
            NodeKind::SuperMethodInvocation => {
                let arguments = lambda_body.list("arguments");
                if method.list("parameters").len() == arguments.len() && are_same_identifiers(method, &arguments) {
                    let reference = cu.rewrite.new_node(NodeKind::SuperMethodReference);
                    let name = cu.rewrite.create_move_target(lambda_body.child("name")?.id);
                    cu.rewrite.put_child(reference, "name", name);
                    Some(self.cast_method_ref_if_needed(cu, reference, cic))
                } else {
                    None
                }
            }
            NodeKind::InstanceofExpression => {
                let left = lambda_body.child("leftOperand")?;
                if method.list("parameters").len() == 1 && are_same_identifiers(method, &[left]) {
                    let reference = cu.rewrite.new_node(NodeKind::ExpressionMethodReference);
                    let literal = cu.rewrite.new_node(NodeKind::TypeLiteral);
                    let binding = lambda_body.child("rightOperand")?.binding()?;
                    let typ = copy_type(cu, &ast, &self.options, lambda_body, binding);
                    cu.rewrite.put_child(literal, "type", typ);
                    let name = cu.rewrite.new_simple_name("isInstance");
                    cu.rewrite.put_child(reference, "name", name);
                    cu.rewrite.put_child(reference, "expression", literal);
                    Some(self.cast_method_ref_if_needed(cu, reference, cic))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn cast_method_ref_if_needed(&self, cu: &mut CuRewrite, reference: RNode, visited: Node<'a>) -> RNode {
        if !(visited.location_is("arguments") && visited.parent().is_some_and(|p| p.is(NodeKind::MethodInvocation))) {
            return reference;
        }
        let parent = visited.parent().expect("parent");
        let arguments = parent.list("arguments");
        let Some(parent_binding) = parent.method_binding() else { return reference };
        let mut need_cast = false;
        let mut current = parent_binding.declaring_class();
        while let Some(t) = current {
            for m in t.declared_methods().unwrap_or_default() {
                if m.name() == parent_binding.name() && m.parameter_types().len() == arguments.len() && m != parent_binding {
                    need_cast = true;
                    break;
                }
            }
            if need_cast {
                break;
            }
            current = t.superclass();
        }
        if !need_cast {
            return reference;
        }
        let Some(arg_type) = visited.type_binding() else { return reference };
        let cast = cu.rewrite.new_node(NodeKind::CastExpression);
        cu.rewrite.put_child(cast, "expression", reference);
        let typ = cu.imports.add_import_type(arg_type, &mut cu.rewrite, &import_context(&self.ast, visited, &self.options), TypeLocation::Unknown);
        cu.rewrite.put_child(cast, "type", typ);
        cast
    }

    /// `makeNamesUnique`.
    fn make_names_unique(&self, cu: &mut CuRewrite, mut from_upper_scope: HashSet<String>, method: Node<'a>) -> HashSet<String> {
        let mut new_names = HashSet::new();
        let mut analyzer = ScopeAnalyzer::new(self.ast.root());
        for b in analyzer.declarations_in_scope(method.start(), scope::VARIABLES | scope::NO_FIELDS | scope::CHECK_VISIBILITY) {
            from_upper_scope.insert(b.name().to_owned());
        }
        let names = names_in_method(method);
        let identifiers: Vec<String> = names.iter().map(|n| n.identifier()).collect();
        for (i, name) in names.iter().enumerate() {
            let identifier = &identifiers[i];
            if from_upper_scope.contains(identifier) {
                let mut counter = 1;
                let mut result = identifier.clone();
                while from_upper_scope.contains(&result) || new_names.contains(&result) || identifiers.contains(&result) {
                    result = format!("{identifier}{counter}");
                    counter += 1;
                }
                from_upper_scope.insert(result.clone());
                new_names.insert(result.clone());
                let references = find_by_node(self.ast.root(), *name);
                for r in references {
                    cu.rewrite.set_simple(RNode::Orig(r.id), "identifier", Some(&result));
                }
            }
        }
        new_names
    }
}

/// `LinkedNodeFinder.findByNode(root, name)`.
fn find_by_node<'a>(root: Node<'a>, name: Node<'a>) -> Vec<Node<'a>> {
    let mut result = Vec::new();
    let key = name.binding().map(|b| b.key().to_owned());
    super::walk(root, &mut |n| {
        if n.is(NodeKind::SimpleName) {
            match &key {
                Some(k) => {
                    if n.binding().is_some_and(|b| b.key() == k) {
                        result.push(n);
                    }
                }
                None => {
                    if n.id == name.id {
                        result.push(n);
                    }
                }
            }
        }
        true
    });
    result
}

/// `getNamesInMethod`.
fn names_in_method(method: Node<'_>) -> Vec<Node<'_>> {
    let mut names = Vec::new();
    fn visit<'a>(n: Node<'a>, counter: &mut i32, names: &mut Vec<Node<'a>>) {
        let is_type = n.kind().is_abstract_type_declaration();
        let is_anonymous = n.is(NodeKind::AnonymousClassDeclaration);
        if is_type {
            if *counter == 0 {
                if let Some(name) = n.child("name") {
                    names.push(name);
                }
            }
            *counter += 1;
        } else if is_anonymous {
            *counter += 1;
        } else if matches!(n.kind(), NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment) && *counter == 0 {
            if let Some(name) = n.child("name") {
                names.push(name);
            }
        }
        for c in n.children() {
            visit(c, counter, names);
        }
        if is_type || is_anonymous {
            *counter -= 1;
        }
    }
    let mut counter = 0;
    visit(method, &mut counter, &mut names);
    names
}

/// `ASTNodes.getCopyOrReplacement(rewrite, node, group)`.
fn copy_or_replacement(rw: &mut ASTRewrite, node: Node<'_>) -> RNode {
    if let (Some(parent), Some(prop)) = (node.parent(), node.location()) {
        if let crate::rewrite::Value::Node(Some(rewritten)) = rw.new_value(RNode::Orig(parent.id), prop) {
            if rewritten != RNode::Orig(node.id) {
                rw.replace(rewritten, Some(RNode::Orig(node.id)));
                return rewritten;
            }
        }
    }
    rw.create_copy_target(node.id)
}

/// `CreateLambdaOperation` on the given anonymous class creations.
pub fn create_lambdas(cu: &mut CuRewrite, ast: &Arc<Ast>, options: &BTreeMap<String, String>, expressions: Vec<Node<'_>>, simplify: bool) {
    let operation = LambdaOperation { ast: ast.clone(), options: options.clone(), simplify, expressions };
    operation.rewrite(cu);
}
