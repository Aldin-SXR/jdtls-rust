//! Unused-item documentation and AddMissingJavadocTagProposalCore.
use crate::{
    correction::{edit::Env, kind, messages, relevance, Context, ProblemLocation, Proposal},
    rewrite::{ASTRewrite, RNode},
    semantic_ast::{problem as p, resolve::normalized_node, Node, NodeKind},
};
use std::collections::HashSet;
fn text(rw: &mut ASTRewrite, value: &str) -> RNode {
    let node = rw.new_node(NodeKind::TextElement);
    rw.put_simple(node, "text", value);
    node
}
fn argument(tag: Node<'_>) -> Option<String> {
    let fragments = tag.list("fragments");
    let first = fragments.first()?;
    if first.kind().is_name() {
        return Some(first.child("name").unwrap_or(*first).identifier());
    }
    if first.is(NodeKind::TextElement) && tag.simple("tagName") == Some("@param") {
        let value = first.simple("text")?;
        if value == "<"
            && fragments.len() >= 3
            && fragments[1].kind().is_name()
            && fragments[2].is(NodeKind::TextElement)
            && fragments[2].simple("text") == Some(">")
        {
            return Some(format!(
                "<{}>",
                fragments[1]
                    .child("name")
                    .unwrap_or(fragments[1])
                    .identifier()
            ));
        }
        if value.starts_with('<') && value.ends_with('>') && value.len() > 2 {
            return Some(value[1..value.len() - 1].into());
        }
    }
    None
}
fn rank(tag: &str) -> usize {
    let tag = if tag == "@exception" { "@throws" } else { tag };
    [
        "@author",
        "@version",
        "@param",
        "@return",
        "@throws",
        "@see",
        "@since",
        "@serial",
        "@deprecated",
    ]
    .iter()
    .position(|t| *t == tag)
    .unwrap_or(9)
}
fn add_tag(rw: &mut ASTRewrite, declaration: Node<'_>, missing: Node<'_>) -> bool {
    let original_doc = declaration.child("javadoc");
    let doc = if let Some(doc) = original_doc {
        RNode::Orig(doc.id)
    } else {
        let doc = rw.new_node(NodeKind::Javadoc);
        rw.set(RNode::Orig(declaration.id), "javadoc", Some(doc));
        doc
    };
    let Some(owner) = missing.parent() else {
        return false;
    };
    let mut leading = HashSet::new();
    let (tag_name, mut fragments) = match (owner.kind(), missing.location()) {
        (NodeKind::SingleVariableDeclaration, Some("name")) => {
            let parameters = declaration.list(if declaration.is(NodeKind::RecordDeclaration) {
                "recordComponents"
            } else {
                "parameters"
            });
            for param in parameters.into_iter().take_while(|n| n.id != owner.id) {
                if let Some(name) = param.child("name") {
                    leading.insert(name.identifier());
                }
            }
            for param in declaration.list("typeParameters") {
                if let Some(name) = param.child("name") {
                    leading.insert(format!("<{}>", name.identifier()));
                }
            }
            ("@param", vec![rw.new_simple_name(&missing.identifier())])
        }
        (NodeKind::TypeParameter, Some("name")) => {
            for param in declaration
                .list("typeParameters")
                .into_iter()
                .take_while(|n| n.id != owner.id)
            {
                if let Some(name) = param.child("name") {
                    leading.insert(format!("<{}>", name.identifier()));
                }
            }
            (
                "@param",
                vec![text(rw, &format!("<{}>", missing.identifier()))],
            )
        }
        (NodeKind::MethodDeclaration, Some("returnType2")) => ("@return", Vec::new()),
        (NodeKind::MethodDeclaration, Some("thrownExceptionTypes")) => {
            for typ in declaration
                .list("thrownExceptionTypes")
                .into_iter()
                .take_while(|n| n.id != missing.id)
            {
                leading.insert(typ.ast.substring(typ.start(), typ.end()));
            }
            (
                "@throws",
                vec![text(
                    rw,
                    &missing.ast.substring(missing.start(), missing.end()),
                )],
            )
        }
        _ => return false,
    };
    fragments.push(text(rw, ""));
    if original_doc.is_none() {
        fragments.push(text(rw, ""));
    }
    let tag = rw.new_node(NodeKind::TagElement);
    rw.put_simple(tag, "tagName", tag_name);
    rw.put_list(tag, "fragments", fragments);
    let after = original_doc.and_then(|doc| {
        doc.list("tags").into_iter().rev().find(|t| {
            t.simple("tagName").is_none_or(|name| {
                rank(tag_name) > rank(name)
                    || (name == tag_name || tag_name == "@throws" && name == "@exception")
                        && argument(*t).is_some_and(|a| leading.contains(&a))
            })
        })
    });
    if let Some(after) = after {
        rw.list_insert_after(doc, "tags", tag, RNode::Orig(after.id));
    } else {
        rw.list_insert_first(doc, "tags", tag);
    }
    true
}
pub async fn document_unused(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let options = env.options(&ctx.ast.uri).await;
    if options
        .get("org.eclipse.jdt.core.compiler.doc.comment.support")
        .map(String::as_str)
        != Some("enabled")
    {
        return;
    }
    let type_param = problem.problem_id == p::UnusedTypeParameter;
    let parameter = type_param || problem.problem_id == p::ArgumentIsNeverUsed;
    let key = if parameter {
        "org.eclipse.jdt.core.compiler.problem.unusedParameterIncludeDocCommentReference"
    } else {
        "org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionIncludeDocCommentReference"
    };
    if options.get(key).map(String::as_str) != Some("enabled") {
        return;
    }
    let Some(mut node) = problem.covering_node(ctx.ast()) else {
        return;
    };
    let Some(declaration) = node
        .ancestor_or_self(|k| k.is_body_declaration())
        .filter(|n| n.binding().is_some())
    else {
        return;
    };
    let key = if type_param {
        "JavadocTagsSubProcessor_document_type_parameter_description"
    } else if parameter {
        "JavadocTagsSubProcessor_document_parameter_description"
    } else {
        node = normalized_node(node);
        "JavadocTagsSubProcessor_document_exception_description"
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    if add_tag(&mut rw, declaration, node) {
        proposals.push(Proposal::rewrite(
            messages::correction(key),
            kind::QUICK_FIX,
            relevance::DOCUMENT_UNUSED_ITEM,
            rw,
        ));
    }
}
