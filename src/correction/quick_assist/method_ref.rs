//! Ports of `LambdaExpressionAndMethodRefFixCore`,
//! `ConvertLambdaToMethodReferenceFixCore` and the method reference to
//! lambda conversion of `QuickAssistProcessorUtil`.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::lambda::{block_body_for_lambda, single_expression_from_lambda_body};
use super::util::{parenthesize_if_needed, replace_wildcards_and_captures};
use crate::correction::type_mismatch::bindings::{is_super_type, relaxing_types};
use crate::correction::type_mismatch::proposals::import_context;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::refactoring::extract_method_analyzer::{enclosing_type, find_enclosing_lambda_expression};
use crate::refactoring::fragments::do_nodes_match;
use crate::refactoring::scope::binding_of_parent_type;
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_type, unparenthesed_expression};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

type Options = BTreeMap<String, String>;

fn is_static(b: BindingRef<'_>) -> bool {
    b.modifiers() & modifier::STATIC != 0
}

/// `ASTNodes.isStatic(MethodInvocation)`.
fn invocation_is_static(invocation: Node<'_>) -> Option<bool> {
    if let Some(m) = invocation.method_binding() {
        return Some(is_static(m));
    }
    let called = invocation.child("expression")?;
    if called.kind().is_name() && called.binding().is_some_and(|b| b.is_type()) {
        return Some(true);
    }
    None
}

/// `ITypeBinding.isSubTypeCompatible`.
fn is_sub_type_compatible(t: BindingRef<'_>, other: BindingRef<'_>) -> bool {
    if t.is_primitive() || other.is_primitive() {
        return false;
    }
    if t == other {
        return true;
    }
    let other = other.erasure().unwrap_or(other);
    is_super_type(other, t.erasure().unwrap_or(t), false)
}

/// `ASTNodes.usesGivenSignature(IMethodBinding, type, name, parameterTypes)`
/// for declared methods of the type being searched.
fn uses_given_signature(method: BindingRef<'_>, type_name: &str, name: &str, parameters: &[String]) -> bool {
    if method.name() != name || method.parameter_types().len() != parameters.len() {
        return false;
    }
    let declaring = method.declaring_class();
    let implemented = declaring.and_then(|d| find_implemented_type(d, type_name));
    if let Some(implemented) = implemented {
        if !implemented.is_raw_type() {
            let params = method.parameter_types();
            return params.iter().zip(parameters).all(|(p, n)| p.erasure().unwrap_or(*p).qualified_name() == n)
                || method_declaration_matches(method, type_name, name, parameters);
        }
    }
    method.parameter_types().iter().zip(parameters).all(|(p, n)| p.qualified_name() == n || p.erasure().is_some_and(|e| e.qualified_name() == n))
        || method_declaration_matches(method, type_name, name, parameters)
}

fn method_declaration_matches(method: BindingRef<'_>, type_name: &str, name: &str, parameters: &[String]) -> bool {
    method
        .method_declaration()
        .filter(|d| *d != method)
        .is_some_and(|d| uses_given_signature(d, type_name, name, parameters))
}

fn find_implemented_type<'a>(t: BindingRef<'a>, qualified: &str) -> Option<BindingRef<'a>> {
    if t.erasure().unwrap_or(t).qualified_name() == qualified || t.qualified_name() == qualified {
        return Some(t);
    }
    for i in t.interfaces() {
        if let Some(found) = find_implemented_type(i, qualified) {
            return Some(found);
        }
    }
    t.superclass().and_then(|s| find_implemented_type(s, qualified))
}

fn same_identifier(lambda: Node<'_>, i: usize, argument: Node<'_>) -> bool {
    let parameters = lambda.list("parameters");
    match parameters.get(i) {
        Some(p) if p.is(NodeKind::VariableDeclarationFragment) => p.child("name").is_some_and(|n| n.identifier() == argument.identifier()),
        _ => false,
    }
}

fn same_identifiers(lambda: Node<'_>, arguments: &[Node<'_>]) -> bool {
    for (i, a) in arguments.iter().enumerate().take(lambda.list("parameters").len()) {
        let expression = unparenthesed_expression(*a);
        if !expression.is(NodeKind::SimpleName) || !same_identifier(lambda, i, expression) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    DoNothing,
    RemoveReturn,
    ClassInstanceRef,
    TypeRef,
    SuperMethodRef,
    MethodRef,
    InstanceofRef,
}

struct LambdaOperation<'a> {
    lambda: Node<'a>,
    remove_parentheses: bool,
    action: Action,
    body: Option<Node<'a>>,
    class_binding: Option<BindingRef<'a>>,
}

/// `LambdaExpressionFinder.visit(LambdaExpression)`; `None` when the lambda
/// needs no change (children are then visited).
fn analyze<'a>(visited: Node<'a>) -> Option<LambdaOperation<'a>> {
    let mut action = Action::DoNothing;
    let mut class_binding = None;
    let parameters = visited.list("parameters");
    let remove_parentheses = visited.flag("parentheses") && parameters.len() == 1 && parameters[0].is(NodeKind::VariableDeclarationFragment);
    let mut body_expression: Option<Node<'a>> = None;
    let body = visited.child("body")?;
    if body.is(NodeKind::Block) {
        let statements = body.list("statements");
        if let [single] = statements.as_slice() {
            if single.is(NodeKind::ReturnStatement) {
                body_expression = single.child("expression");
                if body_expression.is_none() {
                    // an empty return carrying a comment stays
                    if single.extended_start() < single.start() || single.extended_length() > single.length() {
                        action = Action::DoNothing;
                    } else {
                        action = Action::RemoveReturn;
                    }
                } else {
                    action = Action::RemoveReturn;
                }
            }
        }
    } else if body.kind().is_expression() {
        body_expression = Some(body);
    }

    if let Some(expression) = body_expression {
        match expression.kind() {
            NodeKind::ClassInstanceCreation => {
                let arguments = expression.list("arguments");
                if parameters.len() == arguments.len() && same_identifiers(visited, &arguments) && expression.child("anonymousClassDeclaration").is_none() {
                    action = Action::ClassInstanceRef;
                }
            }
            NodeKind::SuperMethodInvocation => {
                let arguments = expression.list("arguments");
                if parameters.len() == arguments.len() && same_identifiers(visited, &arguments) {
                    action = Action::SuperMethodRef;
                }
            }
            NodeKind::MethodInvocation => {
                let called = expression.child("expression");
                let arguments = expression.list("arguments");
                if parameters.len() == arguments.len() {
                    if same_identifiers(visited, &arguments) {
                        let method = expression.method_binding();
                        let mut called_type = None;
                        if let Some(m) = method {
                            called_type = m.declaring_class();
                        } else if let Some(c) = called {
                            called_type = c.type_binding();
                        } else if let Some(enclosing) = find_parent_type(visited).and_then(|t| t.binding()) {
                            let type_arguments = expression.list("typeArguments");
                            let mut names = vec![String::new(); arguments.len()];
                            for (i, t) in type_arguments.iter().enumerate() {
                                match t.binding() {
                                    Some(b) if i < names.len() => names[i] = b.qualified_name().to_owned(),
                                    Some(_) => {}
                                    None => return None,
                                }
                            }
                            let name = expression.child("name").map(|n| n.identifier()).unwrap_or_default();
                            if let Some(declared) = enclosing.declared_methods() {
                                for d in declared {
                                    if uses_given_signature(d, enclosing.qualified_name(), &name, &names) {
                                        called_type = Some(enclosing);
                                        break;
                                    }
                                }
                            }
                        }
                        if invocation_is_static(expression) == Some(true) {
                            if let Some(called_type) = called_type {
                                let mut valid = true;
                                if let Some(first) = arguments.first().and_then(|a| a.type_binding()).filter(|t| is_sub_type_compatible(*t, called_type)) {
                                    let _ = first;
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
                                        let name = expression.child("name").map(|n| n.identifier()).unwrap_or_default();
                                        for d in called_type.declared_methods().unwrap_or_default() {
                                            if !is_static(d) && uses_given_signature(d, called_type.qualified_name(), &name, &remaining) {
                                                valid = false;
                                                break;
                                            }
                                        }
                                    }
                                }
                                if valid {
                                    action = Action::TypeRef;
                                    class_binding = Some(called_type);
                                }
                            }
                        }
                        if action != Action::TypeRef {
                            match called {
                                None => {
                                    if let (Some(called_type), Some(enclosing)) = (called_type, binding_of_parent_type(visited)) {
                                        if is_super_type(called_type, enclosing, false) {
                                            action = Action::MethodRef;
                                        }
                                    }
                                }
                                Some(c) => match c.kind() {
                                    NodeKind::StringLiteral | NodeKind::NumberLiteral | NodeKind::ThisExpression => action = Action::MethodRef,
                                    NodeKind::FieldAccess | NodeKind::SuperFieldAccess => {
                                        if c.binding().is_some_and(|b| b.has(crate::semantic_ast::bflag::EFFECTIVELY_FINAL)) {
                                            action = Action::MethodRef;
                                        }
                                    }
                                    _ => {}
                                },
                            }
                        }
                    }
                } else if called.is_some_and(|c| c.is(NodeKind::SimpleName)) && parameters.len() == arguments.len() + 1 {
                    let called = called.unwrap();
                    if same_identifier(visited, 0, called) {
                        let mut valid = true;
                        for (i, a) in arguments.iter().enumerate() {
                            let e = unparenthesed_expression(*a);
                            if !e.is(NodeKind::SimpleName) || !same_identifier(visited, i + 1, e) {
                                valid = false;
                                break;
                            }
                        }
                        if valid {
                            let class = called.type_binding().or_else(|| expression.method_binding().and_then(|m| m.declaring_class()));
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
                                    let name = expression.child("name").map(|n| n.identifier()).unwrap_or_default();
                                    for d in class.declared_methods().unwrap_or_default() {
                                        if is_static(d) && uses_given_signature(d, class.qualified_name(), &name, &cumulative) {
                                            valid = false;
                                            break;
                                        }
                                    }
                                }
                                if valid {
                                    action = Action::TypeRef;
                                    class_binding = Some(class);
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::InstanceofExpression => {
                if let Some(left) = expression.child("leftOperand") {
                    if parameters.len() == 1 && same_identifiers(visited, &[left]) {
                        if let Some(b) = expression.child("rightOperand").and_then(|r| r.binding()) {
                            action = Action::InstanceofRef;
                            class_binding = Some(b);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if remove_parentheses || action != Action::DoNothing {
        return Some(LambdaOperation { lambda: visited, remove_parentheses, action, body: body_expression, class_binding });
    }
    None
}

/// The first operation found by `LambdaExpressionFinder` in the subtree of `exp`.
fn first_operation<'a>(exp: Node<'a>) -> Option<LambdaOperation<'a>> {
    fn visit<'a>(n: Node<'a>) -> Option<LambdaOperation<'a>> {
        if n.is(NodeKind::LambdaExpression) {
            if let Some(op) = analyze(n) {
                return Some(op);
            }
        }
        n.children().into_iter().find_map(visit)
    }
    visit(exp)
}

/// `ASTNodeFactory.newCreationType`.
fn new_creation_type(rw: &mut ASTRewrite, imports: &mut ImportRewrite, t: BindingRef<'_>, context: &dyn crate::rewrite::import_rewrite::ImportRewriteContext) -> RNode {
    if t.is_parameterized_type() {
        let declaration = t.type_declaration().unwrap_or(t);
        let base = new_creation_type(rw, imports, declaration, context);
        let parameterized = rw.new_node(NodeKind::ParameterizedType);
        rw.put_child(parameterized, "type", base);
        let mut arguments = Vec::new();
        for a in t.type_arguments() {
            let a = replace_wildcards_and_captures(a);
            arguments.push(imports.add_import_type(a, rw, context, TypeLocation::TypeArgument));
        }
        rw.put_list(parameterized, "typeArguments", arguments);
        parameterized
    } else {
        imports.add_import_type(t, rw, context, TypeLocation::New)
    }
}

/// `ReplaceLambdaOperation.copyType`.
fn copy_type(rw: &mut ASTRewrite, imports: &mut ImportRewrite, ast: &Arc<Ast>, options: &Options, node: Node<'_>, t: BindingRef<'_>) -> RNode {
    let context = import_context(ast, node, options);
    let modified = if t.type_parameters().is_empty() {
        if t.is_capture() {
            t.type_bounds().first().copied().or_else(|| t.erasure()).unwrap_or(t)
        } else {
            t.erasure().unwrap_or(t)
        }
    } else {
        t
    };
    new_creation_type(rw, imports, modified, &context)
}

/// `castMethodRefIfNeeded` (both fix cores).
fn cast_method_ref_if_needed(rw: &mut ASTRewrite, imports: &mut ImportRewrite, lambda: Node<'_>, method_ref: RNode) -> RNode {
    let Some(parent) = lambda.parent().filter(|p| p.is(NodeKind::MethodInvocation) && lambda.location_is("arguments")) else { return method_ref };
    let args = parent.list("arguments");
    let Some(parent_binding) = parent.method_binding().and_then(|m| m.method_declaration().or(Some(m))) else { return method_ref };
    let mut need_cast = false;
    let mut type_binding = parent_binding.declaring_class().map(|d| d.erasure().unwrap_or(d));
    while let Some(t) = type_binding {
        for m in t.declared_methods().unwrap_or_default() {
            if m.name() == parent_binding.name() && m.parameter_types().len() == args.len() && m != parent_binding {
                need_cast = true;
                break;
            }
        }
        if need_cast {
            break;
        }
        type_binding = t.superclass();
    }
    if !need_cast {
        return method_ref;
    }
    for arg in &args {
        if *arg == lambda {
            let cast = rw.new_node(NodeKind::CastExpression);
            let Some(arg_type) = arg.type_binding() else { return method_ref };
            let t = imports.add_import_type(arg_type, rw, &DefaultContext, TypeLocation::Unknown);
            rw.put_child(cast, "expression", method_ref);
            rw.put_child(cast, "type", t);
            return cast;
        }
    }
    method_ref
}

fn copy_parameters(rw: &mut ASTRewrite, old: Node<'_>, new_lambda: RNode) {
    let mut moved = Vec::new();
    for p in old.list("parameters") {
        moved.push(rw.create_move_target(p.id));
    }
    rw.put_list(new_lambda, "parameters", moved);
    rw.put_simple(new_lambda, "parentheses", if old.flag("parentheses") { "true" } else { "false" });
}

fn execute(op: &LambdaOperation<'_>, ctx: &Context, options: &Options) -> Option<(ASTRewrite, ImportRewrite)> {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let visited = op.lambda;
    let replacement = match op.action {
        Action::RemoveReturn | Action::DoNothing => {
            let new_lambda = rw.new_node(NodeKind::LambdaExpression);
            copy_parameters(&mut rw, visited, new_lambda);
            if op.remove_parentheses {
                rw.put_simple(new_lambda, "parentheses", "false");
            }
            if op.action == Action::RemoveReturn {
                if let Some(body) = op.body {
                    let moved = rw.create_move_target(body.id);
                    let b = parenthesize_if_needed(&mut rw, moved);
                    rw.put_child(new_lambda, "body", b);
                }
            } else {
                let moved = rw.create_move_target(visited.child("body")?.id);
                rw.put_child(new_lambda, "body", moved);
            }
            new_lambda
        }
        Action::TypeRef => {
            let invocation = op.body?;
            let reference = rw.new_node(NodeKind::TypeMethodReference);
            let t = copy_type(&mut rw, &mut imports, &ctx.ast, options, invocation, op.class_binding?);
            rw.put_child(reference, "type", t);
            let name = invocation.child("name")?;
            let moved = rw.create_move_target(name.id);
            rw.put_child(reference, "name", moved);
            cast_method_ref_if_needed(&mut rw, &mut imports, visited, reference)
        }
        Action::MethodRef => {
            let invocation = op.body?;
            let reference = rw.new_node(NodeKind::ExpressionMethodReference);
            let expression = match invocation.child("expression") {
                Some(e) => rw.create_move_target(e.id),
                None => rw.new_this_expression(),
            };
            rw.put_child(reference, "expression", expression);
            let name = invocation.child("name")?;
            let moved = rw.create_move_target(name.id);
            rw.put_child(reference, "name", moved);
            cast_method_ref_if_needed(&mut rw, &mut imports, visited, reference)
        }
        Action::SuperMethodRef => {
            let invocation = op.body?;
            let reference = rw.new_node(NodeKind::SuperMethodReference);
            let name = invocation.child("name")?;
            let moved = rw.create_move_target(name.id);
            rw.put_child(reference, "name", moved);
            cast_method_ref_if_needed(&mut rw, &mut imports, visited, reference)
        }
        Action::ClassInstanceRef => {
            let creation = op.body?;
            let reference = rw.new_node(NodeKind::CreationReference);
            let t = copy_type(&mut rw, &mut imports, &ctx.ast, options, creation, creation.type_binding()?);
            rw.put_child(reference, "type", t);
            reference
        }
        Action::InstanceofRef => {
            let expression = op.body?;
            let reference = rw.new_node(NodeKind::ExpressionMethodReference);
            let literal = rw.new_node(NodeKind::TypeLiteral);
            let t = copy_type(&mut rw, &mut imports, &ctx.ast, options, expression, op.class_binding?);
            rw.put_child(literal, "type", t);
            rw.put_child(reference, "expression", literal);
            let name = rw.new_simple_name("isInstance");
            rw.put_child(reference, "name", name);
            cast_method_ref_if_needed(&mut rw, &mut imports, visited, reference)
        }
    };
    rw.replace(RNode::Orig(visited.id), Some(replacement));
    Some((rw, imports))
}

/// `QuickAssistProcessor.getConvertLambdaExpressionAndMethodRefCleanUpProposal`.
pub fn clean_up_lambda(ctx: &Context, options: &Options, covering: Node<'_>, out: &mut Vec<Proposal>) {
    if covering.is(NodeKind::Block) && covering.location_is("body") && covering.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
        return;
    }
    let node = covering.ancestors().find(|a| a.is(NodeKind::LambdaExpression) || a.kind().is_body_declaration());
    let Some(lambda) = node.filter(|n| n.is(NodeKind::LambdaExpression)) else { return };
    let Some(op) = first_operation(lambda) else { return };
    let Some((rw, imports)) = execute(&op, ctx, options) else { return };
    let label = messages::fix("LambdaExpressionAndMethodRefFix_clean_up_expression_msg");
    out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}

fn matches(expected: &[Node<'_>], to_match: &[Node<'_>]) -> bool {
    expected.len() == to_match.len() && expected.iter().zip(to_match).all(|(a, b)| do_nodes_match(*a, *b))
}

/// `ConvertLambdaToMethodReferenceFixCore.representsDefiningNode`.
fn represents_defining_node(inner: Node<'_>, defining: Node<'_>) -> bool {
    if inner == defining {
        return true;
    }
    match defining.kind() {
        NodeKind::ClassInstanceCreation => defining.child("type").is_some_and(|t| represents_defining_node(inner, t)),
        NodeKind::ArrayCreation => defining.child("type").is_some_and(|t| represents_defining_node(inner, t)),
        NodeKind::SuperMethodInvocation | NodeKind::MethodInvocation => defining.child("name").is_some_and(|n| n == inner),
        NodeKind::NameQualifiedType | NodeKind::QualifiedType | NodeKind::SimpleType => defining.child("name").is_some_and(|n| n == inner),
        NodeKind::ArrayType => defining.child("elementType").is_some_and(|t| represents_defining_node(inner, t)),
        NodeKind::ParameterizedType => defining.child("type").is_some_and(|t| represents_defining_node(inner, t)),
        _ => false,
    }
}

/// `ConvertLambdaToMethodReferenceFixCore.isValidLambdaReferenceToMethod`.
fn is_valid_lambda_reference_to_method(expression: Node<'_>) -> bool {
    match expression.kind() {
        NodeKind::ClassInstanceCreation | NodeKind::ArrayCreation | NodeKind::SuperMethodInvocation | NodeKind::InstanceofExpression => return true,
        NodeKind::MethodInvocation => {}
        _ => return false,
    }
    let Some(lambda) = expression.ancestors().find(|a| a.is(NodeKind::LambdaExpression)) else { return false };
    let Some(outer) = lambda.ancestors().find(|a| a.is(NodeKind::MethodInvocation)) else { return true };
    let Some(outer_binding) = outer.method_binding() else { return false };
    if !is_static(outer_binding) {
        return true;
    }
    if let Some(m) = expression.method_binding() {
        if let Some(t) = m.declaring_class() {
            return !t.is_interface() || is_static(m);
        }
    }
    false
}

/// `ASTNodes.getInvocationType`.
fn invocation_type<'a>(invocation: Node<'a>, method: BindingRef<'a>, qualifier: Option<Node<'a>>) -> Option<BindingRef<'a>> {
    if matches!(invocation.kind(), NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation) {
        let super_call = invocation.is(NodeKind::SuperMethodInvocation);
        if let Some(q) = qualifier {
            let t = q.type_binding();
            return if super_call { t.and_then(|t| t.superclass()) } else { t };
        }
        let mut enclosing = enclosing_type(invocation);
        if super_call {
            enclosing = enclosing.and_then(|e| e.superclass());
        }
        return match enclosing {
            Some(e) => {
                let params = method.parameter_types();
                if crate::correction::unresolved_elements::find_method_in_hierarchy(e, method.name(), Some(&params)).is_some() {
                    Some(e)
                } else {
                    method.declaring_class()
                }
            }
            None => method.declaring_class(),
        };
    }
    method.declaring_class()
}

fn is_super_class(method_declaration: BindingRef<'_>, lambda_declaration: BindingRef<'_>) -> bool {
    let mut parent = lambda_declaration.superclass();
    while let Some(p) = parent {
        if p == method_declaration {
            return true;
        }
        parent = p.superclass();
    }
    false
}

fn is_nested_class(method_declaration: BindingRef<'_>, lambda_declaration: BindingRef<'_>) -> bool {
    let mut parent = lambda_declaration;
    while parent.is_nested() {
        match parent.declaring_class() {
            Some(p) => parent = p,
            None => return false,
        }
        if parent == method_declaration {
            return true;
        }
    }
    false
}

fn nested_root_class(t: BindingRef<'_>) -> BindingRef<'_> {
    let mut parent = t;
    while parent.is_nested() {
        match parent.declaring_class() {
            Some(p) => parent = p,
            None => break,
        }
    }
    parent
}

fn is_nested_interface_class(method_declaring: BindingRef<'_>, invoking: BindingRef<'_>) -> bool {
    let method_narrowing = relaxing_types(method_declaring);
    let lambda_narrowing = relaxing_types(invoking);
    if method_narrowing.len() != 1 {
        return false;
    }
    lambda_narrowing.iter().any(|l| *l == method_narrowing[0])
}

/// `ConvertLambdaToMethodReferenceFixCore.createConvertLambdaToMethodReferenceFix`
/// and the quick assist wrapper.
pub fn convert_lambda_to_method_reference(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    let lambda = if node.is(NodeKind::LambdaExpression) {
        node
    } else if node.location_is("body") && node.parent().is_some_and(|p| p.is(NodeKind::LambdaExpression)) {
        node.parent().unwrap()
    } else {
        match find_enclosing_lambda_expression(node) {
            Some(l) => l,
            None => return,
        }
    };
    let Some(body) = lambda.child("body") else { return };
    let expr_body = if body.is(NodeKind::Block) { single_expression_from_lambda_body(body) } else { Some(body) };
    let Some(expr_body) = expr_body.map(unparenthesed_expression) else { return };
    if !is_valid_lambda_reference_to_method(expr_body) {
        return;
    }
    let is_parent = expr_body.ancestors().any(|a| a == node);
    if !is_parent && !represents_defining_node(node, expr_body) {
        return;
    }
    let parameters: Vec<Node<'_>> = lambda.list("parameters").iter().filter_map(|p| p.child("name")).collect();
    match expr_body.kind() {
        NodeKind::ClassInstanceCreation => {
            if expr_body.child("expression").is_some() || expr_body.child("anonymousClassDeclaration").is_some() {
                return;
            }
            if !matches(&parameters, &expr_body.list("arguments")) {
                return;
            }
        }
        NodeKind::ArrayCreation => {
            let dimensions = expr_body.list("dimensions");
            if dimensions.len() != 1 || !matches(&parameters, &dimensions) {
                return;
            }
        }
        NodeKind::SuperMethodInvocation => {
            let Some(method) = expr_body.method_binding() else { return };
            if is_static(method) && invocation_type(expr_body, method, expr_body.child("qualifier")).is_none() {
                return;
            }
            if !matches(&parameters, &expr_body.list("arguments")) {
                return;
            }
        }
        NodeKind::InstanceofExpression => {
            if expr_body.child("rightOperand").and_then(|r| r.binding()).is_none() {
                return;
            }
            let Some(left) = expr_body.child("leftOperand") else { return };
            if !matches(&parameters, &[left]) {
                return;
            }
        }
        _ => {
            let Some(method) = expr_body.method_binding() else { return };
            let qualifier = expr_body.child("expression");
            let arguments = expr_body.list("arguments");
            if is_static(method) {
                if invocation_type(expr_body, method, qualifier).is_none() || !matches(&parameters, &arguments) {
                    return;
                }
            } else if lambda.list("parameters").len() as isize - arguments.len() as isize == 1 {
                let Some(qualifier) = qualifier else { return };
                let Some(invocation_binding) = qualifier.type_binding() else { return };
                let Some(lambda_method) = lambda.method_binding() else { return };
                let Some(first) = lambda_method.parameter_types().first().copied() else { return };
                if (invocation_binding != first && !is_super_type(invocation_binding, first, false)) || !do_nodes_match(parameters[0], qualifier) || !matches(&parameters[1..], &arguments) {
                    return;
                }
            } else if !matches(&parameters, &arguments) {
                return;
            }
        }
    }
    if let Some((rw, imports)) = build_method_reference(ctx, options, lambda, expr_body) {
        let label = messages::correction("QuickAssistProcessor_convert_to_method_reference");
        out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::LAMBDA_EXPRESSION_AND_METHOD_REF_CLEANUP, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
    }
}

fn copied_type_arguments(rw: &mut ASTRewrite, node: Node<'_>) -> Vec<RNode> {
    node.list("typeArguments").iter().map(|t| rw.create_copy_target(t.id)).collect()
}

fn build_method_reference(ctx: &Context, options: &Options, lambda: Node<'_>, expr_body: Node<'_>) -> Option<(ASTRewrite, ImportRewrite)> {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let replacement = match expr_body.kind() {
        NodeKind::ClassInstanceCreation => {
            let reference = rw.new_node(NodeKind::CreationReference);
            let mut t = expr_body.child("type")?;
            if t.is(NodeKind::ParameterizedType) && t.list("typeArguments").is_empty() {
                t = t.child("type")?;
            }
            let copy = rw.create_copy_target(t.id);
            rw.put_child(reference, "type", copy);
            let arguments = copied_type_arguments(&mut rw, expr_body);
            rw.put_list(reference, "typeArguments", arguments);
            reference
        }
        NodeKind::ArrayCreation => {
            let reference = rw.new_node(NodeKind::CreationReference);
            let array_type = expr_body.child("type")?;
            let element = rw.create_copy_target(array_type.child("elementType")?.id);
            let dimensions = array_type.list("dimensions").len();
            let new_array = rw.new_node(NodeKind::ArrayType);
            rw.put_child(new_array, "elementType", element);
            let dims: Vec<RNode> = (0..dimensions).map(|_| rw.new_node(NodeKind::Dimension)).collect();
            rw.put_list(new_array, "dimensions", dims);
            rw.put_child(reference, "type", new_array);
            cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
        }
        NodeKind::SuperMethodInvocation => {
            let method = expr_body.method_binding()?;
            let qualifier = expr_body.child("qualifier");
            if is_static(method) {
                let reference = rw.new_node(NodeKind::TypeMethodReference);
                let name = rw.create_copy_target(expr_body.child("name")?.id);
                rw.put_child(reference, "name", name);
                let invocation = invocation_type(expr_body, method, qualifier)?;
                let t = invocation.type_declaration().unwrap_or(invocation);
                let t = t.erasure().unwrap_or(t);
                let ty = imports.add_import_type(t, &mut rw, &DefaultContext, TypeLocation::Unknown);
                rw.put_child(reference, "type", ty);
                let arguments = copied_type_arguments(&mut rw, expr_body);
                rw.put_list(reference, "typeArguments", arguments);
                cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
            } else {
                let reference = rw.new_node(NodeKind::SuperMethodReference);
                if let Some(q) = qualifier {
                    let copy = rw.create_copy_target(q.id);
                    rw.put_child(reference, "qualifier", copy);
                }
                let name = rw.create_copy_target(expr_body.child("name")?.id);
                rw.put_child(reference, "name", name);
                let arguments = copied_type_arguments(&mut rw, expr_body);
                rw.put_list(reference, "typeArguments", arguments);
                cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
            }
        }
        NodeKind::InstanceofExpression => {
            let reference = rw.new_node(NodeKind::ExpressionMethodReference);
            let literal = rw.new_node(NodeKind::TypeLiteral);
            let b = expr_body.child("rightOperand")?.binding()?;
            let t = b.type_declaration().unwrap_or(b);
            let t = t.erasure().unwrap_or(t);
            let ty = imports.add_import_type(t, &mut rw, &DefaultContext, TypeLocation::Unknown);
            rw.put_child(literal, "type", ty);
            let name = rw.new_simple_name("isInstance");
            rw.put_child(reference, "name", name);
            rw.put_child(reference, "expression", literal);
            cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
        }
        _ => {
            let method = expr_body.method_binding()?;
            let qualifier = expr_body.child("expression");
            let arguments = expr_body.list("arguments");
            let is_static_method = is_static(method);
            let type_ref_to_instance = arguments.len() != lambda.list("parameters").len();
            if is_static_method || type_ref_to_instance {
                let reference = rw.new_node(NodeKind::TypeMethodReference);
                let name = rw.create_copy_target(expr_body.child("name")?.id);
                rw.put_child(reference, "name", name);
                let invocation = replace_wildcards_and_captures(invocation_type(expr_body, method, qualifier)?);
                let context = import_context(&ctx.ast, lambda, options);
                let t = invocation.erasure().unwrap_or(invocation);
                let ty = imports.add_import_type(t, &mut rw, &context, TypeLocation::Other);
                rw.put_child(reference, "type", ty);
                let type_arguments = copied_type_arguments(&mut rw, expr_body);
                rw.put_list(reference, "typeArguments", type_arguments);
                cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
            } else {
                let reference = rw.new_node(NodeKind::ExpressionMethodReference);
                let name = rw.create_copy_target(expr_body.child("name")?.id);
                rw.put_child(reference, "name", name);
                let expression = if let Some(q) = qualifier {
                    rw.create_copy_target(q.id)
                } else {
                    let invoking_node = find_parent_type(lambda)?;
                    let invoking = invoking_node.binding()?;
                    let declaring = method.declaring_class()?;
                    let this = rw.new_this_expression();
                    let root = nested_root_class(invoking);
                    let super_class = is_super_class(declaring, invoking);
                    let nested = is_nested_class(declaring, invoking);
                    let mut qualify = false;
                    if declaring == invoking {
                    } else if method.modifiers() & modifier::DEFAULT != 0 {
                        let nested_interface = is_nested_interface_class(declaring, invoking);
                        if nested || (nested_interface && !super_class) {
                        } else if !nested_interface || root != invoking {
                            qualify = true;
                        }
                    } else if declaring.is_interface() {
                        if !super_class {
                            qualify = true;
                        }
                    } else if !super_class {
                        qualify = true;
                    }
                    if qualify {
                        let q = rw.new_name(root.name());
                        rw.put_child(this, "qualifier", q);
                    }
                    this
                };
                rw.put_child(reference, "expression", expression);
                let type_arguments = copied_type_arguments(&mut rw, expr_body);
                rw.put_list(reference, "typeArguments", type_arguments);
                cast_method_ref_if_needed(&mut rw, &mut imports, lambda, reference)
            }
        }
    };
    rw.replace(RNode::Orig(lambda.id), Some(replacement));
    Some((rw, imports))
}

// ─── Method reference to lambda ────────────────────────────────────────────

/// `QuickAssistProcessor.getFunctionalMethodForMethodReference`.
fn functional_method_for<'a>(reference: Node<'a>) -> Option<BindingRef<'a>> {
    let target = reference.type_binding()?;
    let functional = target.data().functional_method.map(|id| reference.ast.binding(id))?;
    Some(functional)
}

fn invocation_arguments(rw: &mut ASTRewrite, begin: usize, names: &[String]) -> Vec<RNode> {
    names.iter().skip(begin).map(|n| rw.new_simple_name(n)).collect()
}

fn is_type_reference_to_instance_method(reference: Node<'_>) -> bool {
    if reference.is(NodeKind::TypeMethodReference) {
        return true;
    }
    if reference.is(NodeKind::ExpressionMethodReference) {
        if let Some(e) = reference.child("expression").filter(|e| e.kind().is_name()) {
            return e.binding().is_some_and(|b| b.is_type());
        }
    }
    false
}

/// `ASTNodes.getVisibleLocalVariablesInScope`.
fn visible_local_variables(node: Node<'_>) -> Vec<String> {
    use crate::refactoring::scope::{ScopeAnalyzer, CHECK_VISIBILITY, NO_FIELDS, VARIABLES};
    ScopeAnalyzer::new(node.root())
        .declarations_in_scope(node.start(), VARIABLES | NO_FIELDS | CHECK_VISIBILITY)
        .into_iter()
        .map(|b| b.name().to_owned())
        .collect()
}

fn create_name(candidate: &str, excluded: &[String]) -> String {
    let mut i = 1;
    let mut result = candidate.to_owned();
    while excluded.contains(&result) {
        result = format!("{candidate}{i}");
        i += 1;
    }
    result
}

/// `QuickAssistProcessorUtil.getUniqueParameterNames`.
fn unique_parameter_names(reference: Node<'_>, functional: BindingRef<'_>) -> Vec<String> {
    let old: Vec<String> = functional.data().parameter_names.clone();
    let mut new = vec![String::new(); old.len()];
    let mut excluded = visible_local_variables(reference);
    for i in 0..old.len() {
        let mut all = excluded.clone();
        all.extend(old[..i].iter().cloned());
        all.extend(old[i + 1..].iter().cloned());
        if all.contains(&old[i]) {
            let n = create_name(&old[i], &all);
            excluded.push(n.clone());
            new[i] = n;
        } else {
            new[i] = old[i].clone();
        }
    }
    new
}

/// `QuickAssistProcessor.getConvertMethodReferenceToLambdaProposal`.
pub fn convert_method_reference_to_lambda(ctx: &Context, covering: Node<'_>, out: &mut Vec<Proposal>) {
    let reference = if covering.kind().is_method_reference() {
        covering
    } else if let Some(p) = covering.parent().filter(|p| p.kind().is_method_reference()) {
        p
    } else {
        return;
    };
    let Some(functional) = functional_method_for(reference) else { return };
    if functional.has(crate::semantic_ast::bflag::GENERIC_METHOD) {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let Some(lambda) = convert_method_reference_to_lambda_node(ctx, &mut rw, reference, functional, false) else { return };
    rw.replace(RNode::Orig(reference.id), Some(lambda));
    let label = messages::correction("QuickAssistProcessor_convert_to_lambda_expression");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::CONVERT_METHOD_REFERENCE_TO_LAMBDA, rw));
}

/// `QuickAssistProcessorUtil.convertMethodRefernceToLambda`.
pub fn convert_method_reference_to_lambda_node(ctx: &Context, rw: &mut ASTRewrite, reference: Node<'_>, functional: BindingRef<'_>, create_block_body: bool) -> Option<RNode> {
    let lambda = rw.new_node(NodeKind::LambdaExpression);
    let names = unique_parameter_names(reference, functional);
    let fragments: Vec<RNode> = names.iter().map(|n| rw.new_variable_declaration_fragment(n, None)).collect();
    rw.put_list(lambda, "parameters", fragments);
    rw.put_simple(lambda, "parentheses", if names.len() != 1 { "true" } else { "false" });
    let returns_void = functional.return_type().is_some_and(|r| r.is_primitive() && r.name() == "void");
    let referred = reference.method_binding();
    let wrap = |rw: &mut ASTRewrite, body: RNode| if create_block_body { block_body_for_lambda(rw, body, returns_void) } else { body };

    match reference.kind() {
        NodeKind::CreationReference => {
            let t = reference.child("type")?;
            if t.is(NodeKind::ArrayType) {
                let creation = rw.new_node(NodeKind::ArrayCreation);
                let element = rw.create_copy_target(t.child("elementType")?.id);
                let dims = t.list("dimensions").len();
                let array = rw.new_node(NodeKind::ArrayType);
                rw.put_child(array, "elementType", element);
                let dimensions: Vec<RNode> = (0..dims).map(|_| rw.new_node(NodeKind::Dimension)).collect();
                rw.put_list(array, "dimensions", dimensions);
                rw.put_child(creation, "type", array);
                let name = rw.new_simple_name(names.first()?);
                rw.put_list(creation, "dimensions", vec![name]);
                let body = wrap(rw, creation);
                rw.put_child(lambda, "body", body);
            } else {
                let cic = rw.new_node(NodeKind::ClassInstanceCreation);
                let generic = t.binding().is_some_and(|b| b.type_declaration().unwrap_or(b).is_generic_type());
                let copy = rw.create_copy_target(t.id);
                let new_type = if !t.is(NodeKind::ParameterizedType) && generic {
                    let p = rw.new_node(NodeKind::ParameterizedType);
                    rw.put_child(p, "type", copy);
                    p
                } else {
                    copy
                };
                rw.put_child(cic, "type", new_type);
                let args = invocation_arguments(rw, 0, &names);
                rw.put_list(cic, "arguments", args);
                let type_args = copied_type_arguments(rw, reference);
                rw.put_list(cic, "typeArguments", type_args);
                let body = wrap(rw, cic);
                rw.put_child(lambda, "body", body);
            }
        }
        _ if referred.is_some_and(is_static) => {
            let referred = referred?;
            let invocation = rw.new_node(NodeKind::MethodInvocation);
            let body = wrap(rw, invocation);
            rw.put_child(lambda, "body", body);
            let mut expression = None;
            let conflict = has_conflict(reference, referred);
            let enclosing = enclosing_type(reference);
            let declaring = referred.declaring_class();
            let super_of_enclosing = match (declaring, enclosing) {
                (Some(d), Some(e)) => is_super_type(d, e, false),
                _ => false,
            };
            if conflict || !super_of_enclosing || !reference.list("typeArguments").is_empty() {
                if reference.is(NodeKind::ExpressionMethodReference) {
                    expression = Some(rw.create_copy_target(reference.child("expression")?.id));
                } else if reference.is(NodeKind::TypeMethodReference) {
                    let t = reference.child("type")?;
                    if let Some(b) = t.binding() {
                        let mut imports = ImportRewrite::create(ctx.ast.clone(), true);
                        let name = imports.add_import_binding(b, &DefaultContext);
                        expression = Some(rw.new_name(&name));
                    }
                }
            }
            if let Some(e) = expression {
                rw.put_child(invocation, "expression", e);
            }
            let name = rw.create_copy_target(method_name(reference)?.id);
            rw.put_child(invocation, "name", name);
            let args = invocation_arguments(rw, 0, &names);
            rw.put_list(invocation, "arguments", args);
            let type_args = copied_type_arguments(rw, reference);
            rw.put_list(invocation, "typeArguments", type_args);
        }
        NodeKind::SuperMethodReference => {
            let invocation = rw.new_node(NodeKind::SuperMethodInvocation);
            let body = wrap(rw, invocation);
            rw.put_child(lambda, "body", body);
            if let Some(q) = reference.child("qualifier") {
                let copy = rw.create_copy_target(q.id);
                rw.put_child(invocation, "qualifier", copy);
            }
            let name = rw.create_copy_target(method_name(reference)?.id);
            rw.put_child(invocation, "name", name);
            let args = invocation_arguments(rw, 0, &names);
            rw.put_list(invocation, "arguments", args);
            let type_args = copied_type_arguments(rw, reference);
            rw.put_list(invocation, "typeArguments", type_args);
        }
        _ => {
            let invocation = rw.new_node(NodeKind::MethodInvocation);
            let body = wrap(rw, invocation);
            rw.put_child(lambda, "body", body);
            let type_reference = is_type_reference_to_instance_method(reference);
            if type_reference {
                let name = rw.new_simple_name(names.first()?);
                rw.put_child(invocation, "expression", name);
            } else {
                let expr = reference.child("expression")?;
                if !(expr.is(NodeKind::ThisExpression) && reference.list("typeArguments").is_empty()) {
                    let copy = rw.create_copy_target(expr.id);
                    rw.put_child(invocation, "expression", copy);
                }
            }
            let name = rw.create_copy_target(method_name(reference)?.id);
            rw.put_child(invocation, "name", name);
            let args = invocation_arguments(rw, usize::from(type_reference), &names);
            rw.put_list(invocation, "arguments", args);
            let type_args = copied_type_arguments(rw, reference);
            rw.put_list(invocation, "typeArguments", type_args);
        }
    }
    Some(lambda)
}

fn method_name<'a>(reference: Node<'a>) -> Option<Node<'a>> {
    reference.child("name")
}

/// `QuickAssistProcessorUtil.hasConflict`.
fn has_conflict(reference: Node<'_>, referred: BindingRef<'_>) -> bool {
    use crate::refactoring::scope::{ScopeAnalyzer, CHECK_VISIBILITY, METHODS};
    let declarations = ScopeAnalyzer::new(reference.root()).declarations_in_scope(reference.start(), METHODS | CHECK_VISIBILITY);
    let declaration = referred.method_declaration().unwrap_or(referred);
    declarations.iter().any(|d| d.name() == referred.name() && *d != declaration)
}
