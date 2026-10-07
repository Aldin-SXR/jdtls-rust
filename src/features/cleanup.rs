//! CleanUpRegistry: select and compose Rust cleanup operations over resolved
//! DOM facts. Reparse each changed working copy without modifying editor state.

use super::{formatting::FormatEnv, organize_imports::operation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{self, resolve, Ast, BindingRef, Node, NodeKind};
use std::collections::HashSet;
use std::sync::Arc;
use tower_lsp::lsp_types::{TextEdit, Url};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Cleanup {
    InvertEquals,
    OrganizeImports,
}

fn cleanup(id: &str) -> Option<Cleanup> {
    match id {
        "invertEquals" | "cleanup.invert_equals" => Some(Cleanup::InvertEquals),
        "organizeImports" => Some(Cleanup::OrganizeImports),
        _ => None,
    }
}

pub async fn edits(env: &FormatEnv<'_>, uri: &Url, ids: &[String]) -> Vec<TextEdit> {
    match compute(env, uri, ids).await {
        Ok(edits) => edits,
        Err(error) => {
            tracing::warn!(%uri, %error, "Cleanup failed");
            Vec::new()
        }
    }
}

async fn compute(env: &FormatEnv<'_>, uri: &Url, ids: &[String]) -> anyhow::Result<Vec<TextEdit>> {
    let mut seen = HashSet::new();
    let cleanups: Vec<_> = ids
        .iter()
        .filter_map(|id| {
            let action = cleanup(id);
            if action.is_none() {
                tracing::warn!(%id, "Not found cleanup id");
            }
            action
        })
        .filter(|action| seen.insert(*action))
        .collect();
    if cleanups.is_empty() {
        return Ok(Vec::new());
    }
    let mut context = env.dispatcher.context_for(Some(uri)).await;
    let Some(original) = context.files.get(uri.as_str()).cloned() else {
        return Ok(Vec::new());
    };
    let mut text = original.clone();
    let options = env.jdt_options(Some(uri)).await;
    for cleanup in cleanups {
        context.files.insert(uri.to_string(), text.clone());
        match cleanup {
            Cleanup::OrganizeImports => {
                if let Some(change) =
                    operation::organize_with_context(env.dispatcher, uri, context.clone(), &options)
                        .await?
                {
                    text = String::from_utf16_lossy(
                        &change
                            .edits
                            .as_ref()
                            .expect("import edits")
                            .apply(&change.ast.source),
                    );
                }
            }
            Cleanup::InvertEquals => {
                let ast =
                    semantic_ast::fetch_with(env.dispatcher, uri.as_str(), context.clone()).await?;
                let rewrite = invert_equals(ast.clone());
                let tree = crate::rewrite::formatter::rewrite_with_bridge(
                    &rewrite,
                    &options,
                    env.dispatcher,
                )
                .await
                .map_err(|error| anyhow::anyhow!("rewrite failed: {error}"))?;
                text = String::from_utf16_lossy(&tree.apply(&ast.source));
            }
        }
    }
    if text == original {
        return Ok(Vec::new());
    }
    let document = crate::analysis::semantic::diagnostics::Doc16::new(&original);
    Ok(vec![TextEdit {
        range: document.to_range(0, original.encode_utf16().count() as i64),
        new_text: text,
    }])
}

fn constant(node: Node<'_>) -> bool {
    node.is_constant_expression()
        || matches!(node.kind(), NodeKind::SimpleName | NodeKind::QualifiedName)
            && node
                .binding()
                .is_some_and(|binding| binding.is_variable() && binding.is_enum_constant())
}

fn string_concat(node: Node<'_>) -> bool {
    let node = resolve::unparenthesed_expression(node);
    node.is(NodeKind::InfixExpression)
        && node.simple("operator") == Some("+")
        && node
            .type_binding()
            .is_some_and(|ty| ty.qualified_name() == "java.lang.String")
}

fn equality(method: BindingRef<'_>) -> bool {
    if method.is_static() || method.is_recovered() {
        return false;
    }
    let parameters = method.parameter_types();
    if parameters.len() != 1 {
        return false;
    }
    let owner = method
        .declaring_class()
        .map(|ty| ty.qualified_name())
        .unwrap_or("");
    (method.name() == "equals"
        && parameters[0].qualified_name() == "java.lang.Object"
        && method
            .return_type()
            .is_some_and(|ty| ty.qualified_name() == "boolean"))
        || (owner == "java.lang.String"
            && method.name() == "equalsIgnoreCase"
            && parameters[0].qualified_name() == "java.lang.String")
}

fn invert_equals(ast: Arc<Ast>) -> ASTRewrite {
    let mut rewrite = ASTRewrite::new(ast.clone());
    // The Eclipse finder does not visit the children of a selected invocation.
    let mut selected = Vec::new();
    for invocation in ast
        .root()
        .descendants()
        .filter(|node| node.is(NodeKind::MethodInvocation))
    {
        if selected
            .iter()
            .any(|node: &Node<'_>| node.is_ancestor_or_self_of(invocation))
        {
            continue;
        }
        let Some(expression) = invocation.child("expression") else {
            continue;
        };
        let receiver = resolve::unparenthesed_expression(expression);
        if receiver.is(NodeKind::ThisExpression)
            || constant(expression)
            || string_concat(expression)
        {
            continue;
        }
        if !invocation.method_binding().is_some_and(equality) {
            continue;
        }
        let arguments = invocation.list("arguments");
        if arguments.len() != 1 {
            continue;
        }
        let argument = arguments[0];
        let target = resolve::unparenthesed_expression(argument);
        if !(constant(argument) && argument.type_binding().is_some_and(|ty| !ty.is_primitive())
            || target.is(NodeKind::ThisExpression)
            || string_concat(argument))
        {
            continue;
        }
        let mut moved = rewrite.create_move_target(argument.id);
        if matches!(
            argument.kind(),
            NodeKind::InfixExpression
                | NodeKind::ConditionalExpression
                | NodeKind::Assignment
                | NodeKind::CastExpression
                | NodeKind::InstanceofExpression
        ) {
            moved = rewrite.new_parenthesized_expression(moved);
        }
        rewrite.replace(RNode::Orig(expression.id), Some(moved));
        let moved = rewrite.create_move_target(receiver.id);
        rewrite.replace(RNode::Orig(argument.id), Some(moved));
        selected.push(invocation);
    }
    rewrite
}
