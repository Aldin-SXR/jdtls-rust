//! Port of `LambdaExpressionsFixCore.CreateAnonymousClassCreationOperation`
//! (lambda expression to anonymous class creation).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::extract_temp::{import_context, CuRewrite};
use super::lambda_fix::{copy_or_replacement, functional_method, new_creation_type, replace_wildcards_and_captures};
use crate::rewrite::import_rewrite::{DefaultContext, TypeLocation};
use crate::rewrite::RNode;
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

/// The functional interface method of the lambda's type.
pub fn lambda_functional_method(lambda: Node<'_>) -> Option<BindingRef<'_>> {
    functional_method(lambda.type_binding()?)
}

/// `SuperThisQualifier.perform`.
fn qualify_super_this(cu: &mut CuRewrite, lambda: Node<'_>, parent_type: BindingRef<'_>) {
    let mut targets = Vec::new();
    super::walk(lambda, &mut |n| {
        if n.is(NodeKind::AnonymousClassDeclaration) || n.kind().is_body_declaration() {
            return false;
        }
        if matches!(n.kind(), NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation | NodeKind::ThisExpression) && n.child("qualifier").is_none() {
            targets.push(n);
        }
        true
    });
    for n in targets {
        let declaration = parent_type.type_declaration().unwrap_or(parent_type);
        let name = cu.imports.add_import_binding(declaration, &DefaultContext);
        let qualifier = cu.rewrite.new_name(&name);
        cu.rewrite.set(RNode::Orig(n.id), "qualifier", Some(qualifier));
    }
}

/// `CreateAnonymousClassCreationOperation.rewriteAST`.
pub fn create_anonymous_classes(cu: &mut CuRewrite, ast: &Arc<Ast>, options: &BTreeMap<String, String>, lambdas: Vec<Node<'_>>) {
    for lambda in lambdas {
        let Some(lambda_type) = lambda.type_binding() else { continue };
        let Some(method_binding) = functional_method(lambda_type) else { continue };
        let parameters = lambda.list("parameters");
        let parameter_names: Vec<String> = parameters.iter().map(|p| p.child("name").map(|n| n.identifier()).unwrap_or_default()).collect();
        let context = import_context(ast, lambda, options);

        let declaration = cu.rewrite.new_node(NodeKind::MethodDeclaration);
        let mut modifiers = Vec::new();
        let declaring = method_binding.declaring_class();
        let override_enabled = declaring.is_none_or(|d| !d.is_interface())
            || options.get("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation").is_none_or(|s| s != "disabled");
        if override_enabled {
            let name = cu.imports.add_import("java.lang.Override", &DefaultContext);
            let marker = cu.rewrite.new_node(NodeKind::MarkerAnnotation);
            let type_name = cu.rewrite.new_name(&name);
            cu.rewrite.put_child(marker, "typeName", type_name);
            modifiers.push(marker);
        }
        let flags = method_binding.modifiers() & !(modifier::ABSTRACT | modifier::NATIVE | modifier::PRIVATE | modifier::DEFAULT);
        modifiers.extend(cu.rewrite.new_modifiers(flags));
        cu.rewrite.put_list(declaration, "modifiers", modifiers);
        let name = cu.rewrite.new_simple_name(method_binding.name());
        cu.rewrite.put_child(declaration, "name", name);
        cu.rewrite.put_simple(declaration, "constructor", "false");
        if let Some(r) = method_binding.return_type().and_then(replace_wildcards_and_captures) {
            let typ = cu.imports.add_import_type(r, &mut cu.rewrite, &context, TypeLocation::ReturnType);
            cu.rewrite.put_child(declaration, "returnType2", typ);
        }
        let types = method_binding.parameter_types();
        let mut declared_parameters = Vec::new();
        for (i, t) in types.iter().enumerate() {
            let parameter = cu.rewrite.new_node(NodeKind::SingleVariableDeclaration);
            let t = replace_wildcards_and_captures(*t).unwrap_or(*t);
            let varargs = method_binding.is_varargs() && i == types.len() - 1 && t.is_array();
            let (typ, _) = cu.imports.add_import_parameter_type(t, &mut cu.rewrite, &context, varargs);
            cu.rewrite.put_child(parameter, "type", typ);
            if varargs {
                cu.rewrite.put_simple(parameter, "varargs", "true");
            }
            let param_name = cu.rewrite.new_simple_name(parameter_names.get(i).map(String::as_str).unwrap_or("arg"));
            cu.rewrite.put_child(parameter, "name", param_name);
            declared_parameters.push(parameter);
        }
        cu.rewrite.put_list(declaration, "parameters", declared_parameters);
        let mut thrown = Vec::new();
        for e in method_binding.exception_types() {
            thrown.push(cu.imports.add_import_type(e, &mut cu.rewrite, &context, TypeLocation::Exception));
        }
        cu.rewrite.put_list(declaration, "thrownExceptionTypes", thrown);

        if let Some(parent_type) = crate::semantic_ast::resolve::find_parent_type(lambda).and_then(|t| t.binding()) {
            if let Some(normalized) = crate::correction::type_mismatch::bindings::normalize_type_binding(Some(parent_type)) {
                qualify_super_this(cu, lambda, normalized);
            }
        }

        let lambda_body = lambda.child("body").expect("lambda body");
        let block = if lambda_body.is(NodeKind::Block) {
            copy_or_replacement(&mut cu.rewrite, lambda_body)
        } else {
            let copy = copy_or_replacement(&mut cu.rewrite, lambda_body);
            let statement = if method_binding.return_type().is_some_and(|r| r.name() == "void") {
                cu.rewrite.new_expression_statement(copy)
            } else {
                cu.rewrite.new_return_statement(Some(copy))
            };
            cu.rewrite.new_block(vec![statement])
        };
        cu.rewrite.put_child(declaration, "body", block);

        let anonymous = cu.rewrite.new_node(NodeKind::AnonymousClassDeclaration);
        cu.rewrite.put_list(anonymous, "bodyDeclarations", vec![declaration]);
        let creation_type = new_creation_type(cu, lambda_type, &context);
        let creation = cu.rewrite.new_node(NodeKind::ClassInstanceCreation);
        cu.rewrite.put_child(creation, "type", creation_type);
        cu.rewrite.put_child(creation, "anonymousClassDeclaration", anonymous);

        let mut to_replace = lambda;
        if lambda.location_is("expression") {
            if let Some(cast) = lambda.parent().filter(|p| p.is(NodeKind::CastExpression)) {
                if cast.type_binding() == Some(lambda_type) {
                    to_replace = cast;
                }
            }
        }
        cu.rewrite.replace(RNode::Orig(to_replace.id), Some(creation));
    }
}
