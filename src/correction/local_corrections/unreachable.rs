//! LocalCorrectionsSubProcessor.getUnreachableCodeProposals and InvertBooleanUtility splits.
use crate::{
    correction::{
        kind, messages, parentheses::needs_parentheses, relevance, Context, ProblemLocation,
        Proposal,
    },
    rewrite::{ASTRewrite, RNode},
    semantic_ast::{
        resolve::{find_parent_statement, unparenthesed_expression},
        Node, NodeKind, PropValue,
    },
};
fn add(ctx: &Context, rw: ASTRewrite, key: &str, rank: i32, proposals: &mut Vec<Proposal>) {
    let _ = ctx;
    proposals.push(Proposal::rewrite(
        messages::correction(key),
        kind::QUICK_FIX,
        rank,
        rw,
    ));
}
fn control_body(node: Node<'_>) -> bool {
    node.parent().is_some_and(|p| match p.kind() {
        NodeKind::IfStatement => {
            node.location_is("thenStatement") || node.location_is("elseStatement")
        }
        NodeKind::ForStatement
        | NodeKind::EnhancedForStatement
        | NodeKind::WhileStatement
        | NodeKind::DoStatement => node.location_is("body"),
        _ => false,
    })
}
async fn including_condition(
    env: &crate::correction::edit::Env<'_>,
    ctx: &Context,
    remove: Node<'_>,
    replacement: Option<Node<'_>>,
    proposals: &mut Vec<Proposal>,
) {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = None;
    let empty = replacement.is_none_or(|n| {
        n.is(NodeKind::EmptyStatement) || n.is(NodeKind::Block) && n.list("statements").is_empty()
    });
    if empty {
        if control_body(remove) {
            let block = rw.new_block(Vec::new());
            rw.replace(RNode::Orig(remove.id), Some(block));
        } else {
            rw.remove(RNode::Orig(remove.id));
        }
    } else {
        let replacement = replacement.unwrap();
        let target = if remove.kind().is_expression() && replacement.kind().is_expression() {
            let mut target = rw.create_move_target(replacement.id);
            if let Some(typ) = super::conversion::explicit_cast(replacement, remove) {
                let cast = rw.new_node(NodeKind::CastExpression);
                if crate::correction::parentheses::needs_parentheses_for_cast(
                    replacement,
                    typ.is_primitive(),
                ) {
                    target = rw.new_parenthesized_expression(target);
                }
                rw.put_child(cast, "expression", target);
                let type_node = if typ.is_primitive() {
                    rw.new_primitive_type(typ.name())
                } else {
                    let options = env.options(&ctx.ast.uri).await;
                    let mut ir =
                        crate::rewrite::import_rewrite::ImportRewrite::create_for_corrections(
                            ctx.ast.clone(),
                            &options,
                        );
                    let context = crate::features::constructors::ConstructorImportContext {
                        ast: ctx.ast.clone(),
                        declaration: crate::semantic_ast::resolve::find_parent_type(remove)
                            .map(|n| n.id),
                        nullness: crate::rewrite::import_rewrite::nullness::Filter::create(
                            &ctx.ast, Some(remove.id), &options,
                        ),
                    };
                    let name = ir.add_import_binding(typ, &context);
                    imports = Some(ir);
                    rw.create_string_placeholder(&name, NodeKind::SimpleType)
                };
                rw.put_child(cast, "type", type_node);
                target = cast;
            }
            target
        } else if remove
            .parent()
            .is_some_and(|p| matches!(p.kind(), NodeKind::Block | NodeKind::SwitchStatement))
            && replacement.is(NodeKind::Block)
        {
            rw.move_removed_block_contents(replacement.id)
        } else {
            rw.create_move_target(replacement.id)
        };
        rw.replace(RNode::Orig(remove.id), Some(target));
    }
    let mut change = crate::correction::CuChange::rewrite(rw);
    if let Some(imports) = imports {
        change = change.with_imports(imports);
    }
    proposals.push(Proposal::new(
        messages::correction(
            "LocalCorrectionsSubProcessor_removeunreachablecode_including_condition_description",
        ),
        kind::QUICK_FIX,
        relevance::REMOVE_UNREACHABLE_CODE_INCLUDING_CONDITION,
        crate::correction::Change::Cu(vec![change]),
    ));
}
pub async fn proposals(
    env: &crate::correction::edit::Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let Some(mut selected) = problem.covering_node(ctx.ast()) else {
        return;
    };
    while selected
        .parent()
        .is_some_and(|p| p.is(NodeKind::ExpressionStatement))
    {
        selected = selected.parent().unwrap();
    }
    let parent = selected.parent();
    if let Some(parent) = parent {
        if parent.is(NodeKind::WhileStatement) {
            including_condition(env, ctx, parent, None, proposals).await;
            return;
        }
        let replacement = match parent.kind() {
            NodeKind::IfStatement if selected.location_is("thenStatement") => {
                Some(parent.child("elseStatement"))
            }
            NodeKind::IfStatement if selected.location_is("elseStatement") => {
                Some(parent.child("thenStatement"))
            }
            NodeKind::ForStatement if selected.location_is("body") => Some(parent.child("body")),
            NodeKind::ConditionalExpression if selected.location_is("thenExpression") => {
                Some(parent.child("elseExpression"))
            }
            NodeKind::ConditionalExpression if selected.location_is("elseExpression") => {
                Some(parent.child("thenExpression"))
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            including_condition(env, ctx, parent, replacement, proposals).await;
            return;
        }
        if parent.is(NodeKind::InfixExpression) && selected.location_is("rightOperand") {
            let Some(left) = parent.child("leftOperand") else {
                return;
            };
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let mut replacement = unparenthesed_expression(left);
            let mut to_replace = parent;
            while to_replace
                .parent()
                .is_some_and(|n| n.is(NodeKind::ParenthesizedExpression))
            {
                to_replace = to_replace.parent().unwrap();
            }
            if to_replace
                .parent()
                .zip(to_replace.location())
                .is_some_and(|(p, loc)| needs_parentheses(replacement, p, loc))
            {
                if left.is(NodeKind::ParenthesizedExpression) {
                    replacement = replacement.parent().unwrap();
                } else if parent
                    .parent()
                    .is_some_and(|n| n.is(NodeKind::ParenthesizedExpression))
                {
                    to_replace = to_replace.child("expression").unwrap();
                }
            }
            let target = rw.create_move_target(replacement.id);
            rw.replace(RNode::Orig(to_replace.id), Some(target));
            add(
                ctx,
                rw,
                "LocalCorrectionsSubProcessor_removeunreachablecode_description",
                10,
                proposals,
            );
            splits(ctx, parent, proposals);
            return;
        }
        if selected.kind().is_statement()
            && selected
                .location()
                .is_some_and(|loc| matches!(parent.prop(loc), Some(PropValue::List(_))))
        {
            let statements = parent.list(selected.location().unwrap());
            let index = statements.iter().position(|n| n.id == selected.id).unwrap();
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let mut key = "LocalCorrectionsSubProcessor_removeunreachablecode_description";
            if index > 0 {
                let previous = statements[index - 1];
                if previous.is(NodeKind::IfStatement) && previous.child("elseStatement").is_none() {
                    if let Some(then) = previous.child("thenStatement") {
                        let target = rw.create_move_target(then.id);
                        rw.replace(RNode::Orig(previous.id), Some(target));
                        key = "LocalCorrectionsSubProcessor_removeunreachablecode_including_condition_description";
                    }
                }
            }
            for n in &statements[index..] {
                if n.is(NodeKind::SwitchCase) {
                    break;
                }
                rw.remove(RNode::Orig(n.id));
            }
            add(ctx, rw, key, 10, proposals);
            return;
        }
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.remove(RNode::Orig(selected.id));
    add(
        ctx,
        rw,
        "LocalCorrectionsSubProcessor_removeunreachablecode_description",
        10,
        proposals,
    );
}
fn operands(
    rw: &mut ASTRewrite,
    expr: Node<'_>,
    op: &str,
    offset: usize,
    sides: &mut [Option<RNode>; 2],
) {
    if expr.end() <= offset || expr.start() >= offset {
        let side = usize::from(expr.start() >= offset);
        let original = if sides[side].is_none() {
            unparenthesed_expression(expr)
        } else {
            expr
        };
        let target = rw.create_move_target(original.id);
        if original.is(NodeKind::InfixExpression) {
            rw.put_simple(
                target,
                "operator",
                original.simple("operator").unwrap_or(""),
            );
        }
        sides[side] = Some(if let Some(existing) = sides[side] {
            rw.new_infix_expression(existing, op, target)
        } else {
            target
        });
    } else if expr.is(NodeKind::InfixExpression) && expr.simple("operator") == Some(op) {
        for n in [expr.child("leftOperand"), expr.child("rightOperand")]
            .into_iter()
            .flatten()
            .chain(expr.list("extendedOperands"))
        {
            operands(rw, n, op, offset, sides);
        }
    }
}
fn splits(ctx: &Context, infix: Node<'_>, proposals: &mut Vec<Proposal>) {
    let op = infix.simple("operator").unwrap_or("");
    if !matches!(op, "&&" | "||") {
        return;
    }
    let Some(statement) = find_parent_statement(infix).filter(|n| n.is(NodeKind::IfStatement))
    else {
        return;
    };
    let mut top = infix;
    while top
        .parent()
        .is_some_and(|p| p.is(NodeKind::InfixExpression) && p.simple("operator") == Some(op))
    {
        top = top.parent().unwrap();
    }
    if statement.child("expression").is_none_or(|e| e.id != top.id) {
        return;
    }
    let (Some(left), Some(then)) = (infix.child("leftOperand"), statement.child("thenStatement"))
    else {
        return;
    };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut sides = [None, None];
    operands(&mut rw, top, op, left.end(), &mut sides);
    let [Some(left), Some(right)] = sides else {
        return;
    };
    rw.set(RNode::Orig(statement.id), "expression", Some(left));
    let inner = rw.new_node(NodeKind::IfStatement);
    rw.put_child(inner, "expression", right);
    if op == "&&" {
        let then_target = rw.create_move_target(then.id);
        rw.put_child(inner, "thenStatement", then_target);
        if let Some(e) = statement.child("elseStatement") {
            let target = rw.create_copy_target(e.id);
            rw.put_child(inner, "elseStatement", target);
        }
        let block = rw.new_block(vec![inner]);
        rw.replace(RNode::Orig(then.id), Some(block));
    } else {
        let target = rw.create_copy_target(then.id);
        rw.put_child(inner, "thenStatement", target);
        if let Some(e) = statement.child("elseStatement") {
            let target = rw.create_move_target(e.id);
            rw.put_child(inner, "elseStatement", target);
        }
        rw.set(RNode::Orig(statement.id), "elseStatement", Some(inner));
    }
    add(
        ctx,
        rw,
        if op == "&&" {
            "AdvancedQuickAssistProcessor_splitAndCondition_description"
        } else {
            "AdvancedQuickAssistProcessor_splitOrCondition_description"
        },
        if op == "&&" {
            relevance::SPLIT_AND_CONDITION
        } else {
            relevance::SPLIT_OR_CONDITION
        },
        proposals,
    );
}
