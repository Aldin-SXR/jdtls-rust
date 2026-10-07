//! QuickAssistProcessor catch-to-throws and unnecessary thrown exceptions.
use crate::{
    correction::{
        edit::Env, kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal,
    },
    rewrite::{import_rewrite::ImportRewrite, ASTRewrite, RNode},
    semantic_ast::{
        resolve::{find_parent_statement, normalized_node},
        BindingRef, Node, NodeKind,
    },
};
use std::collections::HashSet;

/// ASTNodes.getTypeName / getQualifiedTypeName: exclude arguments and annotations.
pub(crate) fn type_name(node: Node<'_>, qualified: bool) -> String {
    match node.kind() {
        NodeKind::PrimitiveType => node.simple("primitiveTypeCode").unwrap_or("").into(),
        NodeKind::SimpleType => node
            .child("name")
            .map(|n| {
                if qualified {
                    n.identifier()
                } else {
                    n.child("name").unwrap_or(n).identifier()
                }
            })
            .unwrap_or_default(),
        NodeKind::QualifiedType | NodeKind::NameQualifiedType => {
            let name = node
                .child("name")
                .map(|n| n.identifier())
                .unwrap_or_default();
            if qualified {
                let qualifier = node
                    .child("qualifier")
                    .map(|n| {
                        if n.kind().is_name() {
                            n.identifier()
                        } else {
                            type_name(n, true)
                        }
                    })
                    .unwrap_or_default();
                format!("{qualifier}.{name}")
            } else {
                name
            }
        }
        NodeKind::ParameterizedType => node
            .child("type")
            .map(|n| type_name(n, qualified))
            .unwrap_or_default(),
        NodeKind::ArrayType => format!(
            "{}{}",
            node.child("elementType")
                .map(|n| type_name(n, qualified))
                .unwrap_or_default(),
            "[]".repeat(node.list("dimensions").len())
        ),
        _ => node
            .children()
            .into_iter()
            .map(|n| type_name(n, qualified))
            .collect(),
    }
}
fn control_body(node: Node<'_>) -> bool {
    node.parent().is_some_and(|p| match p.kind() {
        NodeKind::IfStatement => matches!(node.location(), Some("thenStatement" | "elseStatement")),
        NodeKind::ForStatement
        | NodeKind::EnhancedForStatement
        | NodeKind::WhileStatement
        | NodeKind::DoStatement => node.location_is("body"),
        _ => false,
    })
}
fn remove_catch(rw: &mut ASTRewrite, clause: Node<'_>) {
    let Some(statement) = clause.parent() else {
        return;
    };
    if statement.list("catchClauses").len() > 1
        || statement.child("finally").is_some()
        || !statement.list("resources").is_empty()
    {
        rw.remove(RNode::Orig(clause.id));
    } else if let Some(body) = statement.child("body") {
        let statements = body.list("statements");
        let replacement = match statements.len() {
            0 => {
                rw.remove(RNode::Orig(statement.id));
                return;
            }
            1 => rw.create_copy_target(statements[0].id),
            _ => {
                let copied = rw.copy_removed_block_contents(body.id);
                if control_body(statement) {
                    rw.new_block(vec![copied])
                } else {
                    copied
                }
            }
        };
        rw.replace(RNode::Orig(statement.id), Some(replacement));
    }
}
fn subtype(typ: BindingRef<'_>, target: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
    if typ == target {
        return true;
    }
    if !seen.insert(typ.key().into()) {
        return false;
    }
    typ.superclass().is_some_and(|s| subtype(s, target, seen))
        || typ
            .interfaces()
            .into_iter()
            .any(|i| subtype(i, target, seen))
}
fn add_throws(rw: &mut ASTRewrite, method: Node<'_>, typ: Node<'_>) {
    if typ.binding().is_none_or(|new| {
        !method.list("thrownExceptionTypes").iter().any(|t| {
            t.binding()
                .is_some_and(|existing| subtype(new, existing, &mut HashSet::new()))
        })
    }) {
        let copy = rw.copy_subtree(RNode::Orig(typ.id));
        rw.list_insert_last(RNode::Orig(method.id), "thrownExceptionTypes", copy);
    }
}
fn push(rw: ASTRewrite, key: &str, rank: i32, proposals: &mut Vec<Proposal>) {
    proposals.push(Proposal::rewrite(
        messages::correction(key),
        kind::QUICK_FIX,
        rank,
        rw,
    ));
}
pub fn unreachable_catch(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(node) = problem.covering_node(ctx.ast()) else {
        return;
    };
    let Some(clause) = node.ancestor_or_self(|k| k == NodeKind::CatchClause) else {
        return;
    };
    let Some(statement) = find_parent_statement(node) else {
        return;
    };
    if Some(statement.id) != clause.parent().map(|n| n.id)
        && Some(statement.id) != clause.child("body").map(|n| n.id)
    {
        return;
    }
    let Some(typ) = clause
        .child("exception")
        .and_then(|n| n.child("type"))
        .filter(|n| {
            matches!(
                n.kind(),
                NodeKind::SimpleType | NodeKind::UnionType | NodeKind::NameQualifiedType
            )
        })
    else {
        return;
    };
    let Some(body) = clause
        .ancestors()
        .find(|n| n.kind().is_body_declaration())
        .filter(|n| {
            matches!(
                n.kind(),
                NodeKind::MethodDeclaration | NodeKind::Initializer
            )
        })
    else {
        return;
    };
    let selected = if typ.is(NodeKind::UnionType) && node.kind().is_name() {
        let mut top = node;
        while let Some(parent) = top.parent().filter(|n| n.kind().is_name()) {
            top = parent;
        }
        top.parent()
            .filter(|n| matches!(n.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType))
    } else {
        None
    };
    if body.is(NodeKind::MethodDeclaration) {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        if let Some(selected) = selected {
            rw.remove(RNode::Orig(selected.id));
            add_throws(&mut rw, body, selected);
            push(
                rw,
                "QuickAssistProcessor_exceptiontothrows_description",
                relevance::REPLACE_EXCEPTION_WITH_THROWS,
                proposals,
            );
        } else {
            remove_catch(&mut rw, clause);
            let types = if typ.is(NodeKind::UnionType) {
                typ.list("types")
            } else {
                vec![typ]
            };
            if types
                .iter()
                .any(|n| !matches!(n.kind(), NodeKind::SimpleType | NodeKind::NameQualifiedType))
            {
                return;
            }
            for typ in types {
                add_throws(&mut rw, body, typ);
            }
            push(
                rw,
                "QuickAssistProcessor_catchclausetothrows_description",
                relevance::REPLACE_CATCH_CLAUSE_WITH_THROWS,
                proposals,
            );
        }
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    if let Some(selected) = selected {
        rw.remove(RNode::Orig(selected.id));
        push(
            rw,
            "QuickAssistProcessor_removeexception_description",
            relevance::REMOVE_EXCEPTION,
            proposals,
        );
    } else {
        remove_catch(&mut rw, clause);
        push(
            rw,
            "QuickAssistProcessor_removecatchclause_description",
            relevance::REMOVE_CATCH_CLAUSE,
            proposals,
        );
    }
}
/// ASTNodes.getNumberOfTypeReferences: declaration/expression types only,
/// ignoring imports and Javadoc, and descending into parameterized types.
pub(crate) fn type_references(binding: BindingRef<'_>) -> usize {
    let mut count = 0;
    for node in binding
        .ast
        .all_nodes()
        .filter(|n| !n.ancestors().any(|n| n.is(NodeKind::Javadoc)))
    {
        let mut types = match node.kind() {
            NodeKind::ArrayCreation
            | NodeKind::ClassInstanceCreation
            | NodeKind::SingleVariableDeclaration
            | NodeKind::CastExpression
            | NodeKind::VariableDeclarationExpression
            | NodeKind::VariableDeclarationStatement
            | NodeKind::FieldDeclaration => node.child("type").into_iter().collect::<Vec<_>>(),
            NodeKind::MethodDeclaration => node
                .child("returnType2")
                .into_iter()
                .chain(node.list("thrownExceptionTypes"))
                .collect(),
            NodeKind::InstanceofExpression => node.child("rightOperand").into_iter().collect(),
            NodeKind::ParameterizedType => node
                .child("type")
                .into_iter()
                .chain(node.list("typeArguments"))
                .collect(),
            NodeKind::TypeDeclaration | NodeKind::RecordDeclaration => node.list("typeParameters"),
            _ => Vec::new(),
        };
        count += types
            .drain(..)
            .filter(|t| !t.is(NodeKind::ParameterizedType))
            .filter_map(|t| t.binding())
            .map(|b| {
                if b.is_array() {
                    b.element_type().unwrap_or(b)
                } else {
                    b
                }
            })
            .filter(|b| *b == binding)
            .count();
    }
    count
}
pub async fn unnecessary_throws(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let Some(typ) = problem
        .covering_node(ctx.ast())
        .map(normalized_node)
        .filter(|n| {
            n.location_is("thrownExceptionTypes")
                && n.parent()
                    .is_some_and(|n| n.is(NodeKind::MethodDeclaration))
        })
    else {
        return;
    };
    let method = typ.parent().unwrap();
    if method.binding().is_some() {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let options = env.options(&ctx.ast.uri).await;
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        if let Some(binding) = typ.binding().filter(|b| type_references(*b) == 1) {
            imports.remove_import(binding.qualified_name());
        }
        rw.remove(RNode::Orig(typ.id));
        let name = type_name(typ, false);
        if let Some(tag) = method.child("javadoc").and_then(|doc| {
            doc.list("tags").into_iter().find(|tag| {
                matches!(tag.simple("tagName"), Some("@throws" | "@exception"))
                    && super::javadoc::argument(*tag).as_deref() == Some(name.as_str())
            })
        }) {
            rw.remove(RNode::Orig(tag.id));
        }
        proposals.push(Proposal::new(
            messages::correction("LocalCorrectionsSubProcessor_unnecessarythrow_description"),
            kind::QUICK_FIX,
            relevance::UNNECESSARY_THROW,
            Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
        ));
    }
    super::javadoc::document_unused(env, ctx, problem, proposals).await;
}
