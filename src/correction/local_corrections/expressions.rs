//! Invalid operators and NewLocalVariableCorrectionProposalCore.
use crate::{
    correction::{
        edit::Env, kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal,
    },
    rewrite::{
        import_rewrite::{DefaultContext, ImportRewrite, TypeLocation},
        text_edit::{EditKind, EditTree},
        ASTRewrite, RNode,
    },
    semantic_ast::{resolve::unparenthesed_expression, Node, NodeKind},
};

fn bitwise(node: Node<'_>) -> bool {
    node.is(NodeKind::InfixExpression) && matches!(node.simple("operator"), Some("&" | "|" | "^"))
}

/// CompareInBitWiseOpFinder: visit bitwise operands, keeping the last equality.
fn comparison<'a>(node: Node<'a>, found: &mut Option<Node<'a>>) {
    if node.is(NodeKind::InfixExpression) {
        if matches!(node.simple("operator"), Some("==" | "!=")) {
            *found = Some(node);
            return;
        }
        if !bitwise(node) {
            return;
        }
    }
    for child in node.children() {
        comparison(child, found);
    }
}

pub fn invalid_operator(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem
        .covering_node(ctx.ast())
        .map(unparenthesed_expression)
    else {
        return;
    };
    if selected.is(NodeKind::PrefixExpression) && selected.simple("operator") == Some("!") {
        let Some(parent) = selected.parent() else {
            return;
        };
        let label = match parent.kind() {
            NodeKind::InstanceofExpression => messages::correction(
                "LocalCorrectionsSubProcessor_setparenteses_instanceof_description",
            )
            .to_owned(),
            NodeKind::InfixExpression => messages::format(
                messages::correction("LocalCorrectionsSubProcessor_setparenteses_description"),
                &[parent.simple("operator").unwrap_or("")],
            ),
            _ => return,
        };
        let Some(operand) = selected.child("operand") else {
            return;
        };
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let operand = rw.create_move_target(operand.id);
        rw.replace(RNode::Orig(selected.id), Some(operand));
        let target = rw.create_move_target(parent.id);
        let enclosed = rw.new_parenthesized_expression(target);
        let prefix = rw.new_node(NodeKind::PrefixExpression);
        rw.put_simple(prefix, "operator", "!");
        rw.put_child(prefix, "operand", enclosed);
        rw.replace(RNode::Orig(parent.id), Some(prefix));
        proposals.push(Proposal::rewrite(
            label,
            kind::QUICK_FIX,
            relevance::INVALID_OPERATOR,
            rw,
        ));
    } else if bitwise(selected) {
        let mut found = None;
        comparison(selected, &mut found);
        let Some(compare) = found else { return };
        let mut outer = selected;
        while let Some(parent) = outer.parent().filter(|n| bitwise(*n)) {
            outer = parent;
        }
        let (Some(left), Some(right)) =
            (compare.child("leftOperand"), compare.child("rightOperand"))
        else {
            return;
        };
        let mut edits = EditTree::new();
        let mut ranges = Vec::new();
        if outer.start() < left.start() {
            ranges.push((outer.start(), left.end()));
        }
        if outer.end() > right.end() {
            ranges.push((right.start(), outer.end()));
        }
        for (start, end) in ranges {
            for (offset, text) in [(start, "("), (end, ")")] {
                let edit = edits.new_edit(offset as i32, 0, EditKind::Insert(text.into()));
                let _ = edits.add_child(EditTree::ROOT, edit);
            }
        }
        proposals.push(Proposal::new(
            messages::correction("LocalCorrectionsSubProcessor_setparenteses_bitop_description"),
            kind::QUICK_FIX,
            relevance::INVALID_OPERATOR,
            Change::Cu(vec![CuChange::edits(ctx.ast.clone(), edits)]),
        ));
    }
}

/// ScopeAnalyzer.DeclarationsAfterVisitor(VARIABLES), excluding nested types
/// and variable initializers exactly as the hierarchical visitor does.
fn later_declarations(node: Node<'_>, offset: usize, names: &mut Vec<String>) {
    if matches!(
        node.kind(),
        NodeKind::AnonymousClassDeclaration | NodeKind::TypeDeclarationStatement
    ) {
        return;
    }
    if node.kind().is_variable_declaration() {
        if node.start() > offset {
            if let Some(binding) = node.binding() {
                names.push(binding.name().into());
            }
        }
        return;
    }
    for child in node.children() {
        later_declarations(child, offset, names);
    }
}

pub async fn expression_variable(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let Some(selected) = problem.covering_node(ctx.ast()) else {
        return;
    };
    let statement = if selected.is(NodeKind::ExpressionStatement) {
        selected
    } else if selected.kind().is_expression() && selected.location_is("expression") {
        let Some(parent) = selected
            .parent()
            .filter(|n| n.is(NodeKind::ExpressionStatement))
        else {
            return;
        };
        parent
    } else {
        return;
    };
    let Some(expression) = statement.child("expression").map(unparenthesed_expression) else {
        return;
    };
    let binding = expression.type_binding();
    if binding.is_none() && !expression.is(NodeKind::CastExpression) {
        return;
    }
    let mut names = Vec::new();
    if let Some(method) = statement.ancestor_or_self(|k| k == NodeKind::MethodDeclaration) {
        names.extend(
            method
                .list("parameters")
                .into_iter()
                .filter_map(|p| p.child("name"))
                .map(|n| n.identifier()),
        );
        if let Some(body) = method.child("body") {
            later_declarations(body, body.start(), &mut names);
        }
    }
    let options = env.options(&ctx.ast.uri).await;
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
    let (typ, type_name) = if let Some(binding) = binding {
        (
            imports.add_import_type(binding, &mut rw, &DefaultContext, TypeLocation::Unknown),
            binding.name().to_owned(),
        )
    } else if let Some(typ) = expression.child("type") {
        (rw.create_copy_target(typ.id), typ.source_text())
    } else {
        return;
    };
    let name =
        crate::features::completion::naming::suggest_parameter_names(&type_name, &names, &options)
            .into_iter()
            .next()
            .unwrap_or_else(|| "name".into());
    let initializer = rw.create_copy_target(expression.id);
    let fragment = rw.new_variable_declaration_fragment(&name, Some(initializer));
    let declaration = rw.new_node(NodeKind::VariableDeclarationStatement);
    rw.put_child(declaration, "type", typ);
    rw.put_list(declaration, "fragments", vec![fragment]);
    rw.replace(RNode::Orig(statement.id), Some(declaration));
    proposals.push(Proposal::new(
        messages::correction("LocalCorrectionsSubProcessor_createLocalVariable_description"),
        kind::QUICK_FIX,
        relevance::CREATE_LOCAL,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
}
