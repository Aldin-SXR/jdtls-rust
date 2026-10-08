//! Ports of `SplitVariableFixCore`, `JoinVariableFixCore` and
//! `InvertEqualsExpressionFixCore`.

use std::collections::BTreeMap;

use super::util::is_var_type;
use crate::correction::type_mismatch::proposals::import_context;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, Proposal};
use crate::refactoring::checks::is_control_statement_body;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{subtree_match, unparenthesed_expression};
use crate::semantic_ast::{modifier, nflag, BindingRef, Node, NodeKind};

type Options = BTreeMap<String, String>;

fn is_list_child(node: Node<'_>) -> bool {
    match (node.parent(), node.location()) {
        (Some(parent), Some(location)) => parent.has_prop(location) && matches!(parent.prop(location), Some(crate::semantic_ast::PropValue::List(_))),
        _ => false,
    }
}

/// `QuickAssistProcessor.getJoinVariableProposal` (which creates the split fix).
pub fn split_variable(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    let fragment = if node.is(NodeKind::VariableDeclarationFragment) {
        node
    } else if node.location_is("name") && node.parent().is_some_and(|p| p.is(NodeKind::VariableDeclarationFragment)) {
        node.parent().unwrap()
    } else {
        return;
    };
    if fragment.child("initializer").is_none() {
        return;
    }
    let Some(frag_parent) = fragment.parent() else { return };
    let statement;
    let is_var_type_decl;
    match frag_parent.kind() {
        NodeKind::VariableDeclarationStatement => {
            statement = frag_parent;
            is_var_type_decl = frag_parent.child("type").is_some_and(is_var_type);
        }
        NodeKind::VariableDeclarationExpression => {
            if frag_parent.location_is("resources") {
                return;
            }
            let Some(s) = frag_parent.parent() else { return };
            statement = s;
            is_var_type_decl = frag_parent.child("type").is_some_and(is_var_type);
        }
        _ => return,
    }
    if !matches!(statement.kind(), NodeKind::ForStatement | NodeKind::VariableDeclarationStatement) {
        return;
    }
    if !is_list_child(statement) {
        return;
    }
    let Some(statement_parent) = statement.parent() else { return };
    let property = statement.location().unwrap();
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let ast = &ctx.ast;

    if statement.is(NodeKind::ForStatement) {
        let old_declaration = frag_parent;
        let Some(old_type) = old_declaration.child("type") else { return };
        let old_fragments = old_declaration.list("fragments");
        for old_fragment in &old_fragments {
            let start = old_fragment.extended_start();
            let mut code = ast.substring(start, start + old_fragment.extended_length());
            if old_fragment.child("initializer").is_none() {
                let type_binding = old_type.binding();
                if type_binding.is_some_and(|b| b.binary_name() == Some("Z")) {
                    code.push_str(" = false");
                } else if old_type.is(NodeKind::PrimitiveType) {
                    code.push_str(" = 0");
                } else {
                    code.push_str(" = null");
                }
            }
            let assignment = rw.create_string_placeholder(&code, NodeKind::Assignment);
            rw.list_insert_last(RNode::Orig(statement.id), "initializers", assignment);
        }
        let new_type;
        if is_var_type_decl {
            let Some(binding) = old_type.binding() else { return };
            let context = import_context(ast, statement, options);
            let _ = imports.add_import_type(binding, &mut rw, &context, TypeLocation::LocalVariable);
            let mut declaration = binding.name().to_owned();
            let extended_start = old_declaration.extended_start();
            let mut comment = String::new();
            if old_declaration.start() > extended_start {
                comment = ast.substring(extended_start, old_declaration.start());
            }
            declaration.insert_str(0, &comment);
            new_type = rw.create_string_placeholder(declaration.trim(), old_type.kind());
        } else {
            let extended_start = old_declaration.extended_start();
            let first_fragment_start = old_fragments[0].start();
            let declaration = ast.substring(extended_start, first_fragment_start);
            new_type = rw.create_string_placeholder(declaration.trim(), old_type.kind());
        }
        let new_declaration = rw.new_node(NodeKind::VariableDeclarationStatement);
        let mut new_fragments = Vec::new();
        for old_fragment in &old_fragments {
            let fragment = rw.new_node(NodeKind::VariableDeclarationFragment);
            let name = rw.new_simple_name(&old_fragment.child("name").map(|n| n.identifier()).unwrap_or_default());
            rw.put_child(fragment, "name", name);
            let dimensions: Vec<RNode> = old_fragment.list("extraDimensions2").iter().map(|d| rw.create_copy_target(d.id)).collect();
            rw.put_list(fragment, "extraDimensions2", dimensions);
            new_fragments.push(fragment);
        }
        rw.put_list(new_declaration, "fragments", new_fragments);
        rw.put_child(new_declaration, "type", new_type);
        let modifiers = rw.new_modifiers(old_declaration.modifiers());
        rw.put_list(new_declaration, "modifiers", modifiers);
        rw.list_insert_before(RNode::Orig(statement_parent.id), property, new_declaration, RNode::Orig(statement.id));
        rw.remove(RNode::Orig(old_declaration.id));
    } else {
        let list = statement_parent.list(property);
        let mut insert_index = list.iter().position(|s| *s == statement).unwrap_or(0);
        let Some(initializer) = fragment.child("initializer") else { return };
        let binding = initializer.type_binding();
        let mut placeholder = rw.create_move_target(initializer.id);
        if initializer.is(NodeKind::ArrayInitializer) {
            if let Some(binding) = binding.filter(|b| b.is_array()) {
                let creation = rw.new_node(NodeKind::ArrayCreation);
                rw.put_child(creation, "initializer", placeholder);
                let Some(component) = binding.element_type() else { return };
                let element = if component.is_primitive() {
                    rw.new_primitive_type(component.name())
                } else {
                    let name = rw.new_simple_name(component.name());
                    rw.new_simple_type(name)
                };
                let array_type = rw.new_node(NodeKind::ArrayType);
                rw.put_child(array_type, "elementType", element);
                let dimensions: Vec<RNode> = (0..binding.dimensions()).map(|_| rw.new_node(NodeKind::Dimension)).collect();
                rw.put_list(array_type, "dimensions", dimensions);
                rw.put_child(creation, "type", array_type);
                placeholder = creation;
            }
        }
        let left = rw.new_simple_name(&fragment.child("name").map(|n| n.identifier()).unwrap_or_default());
        let assignment = rw.new_assignment(left, "=", placeholder);
        let new_statement = rw.new_expression_statement(assignment);
        insert_index += 1;
        if is_var_type_decl {
            let Some(type_node) = statement.child("type") else { return };
            let Some(binding) = type_node.binding() else { return };
            let context = import_context(ast, statement, options);
            let new_type = imports.add_import_type(binding, &mut rw, &context, TypeLocation::LocalVariable);
            rw.set(RNode::Orig(statement.id), "type", Some(new_type));
            for d in fragment.list("extraDimensions2") {
                rw.remove(RNode::Orig(d.id));
            }
        }
        rw.list_insert_at(RNode::Orig(statement_parent.id), property, new_statement, insert_index as i32);
    }
    let label = messages::correction("QuickAssistProcessor_splitdeclaration_description");
    out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::SPLIT_VARIABLE_DECLARATION, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}

fn single_statement(statement: Option<Node<'_>>) -> Option<Node<'_>> {
    let statement = statement?;
    if statement.is(NodeKind::Block) {
        let statements = statement.list("statements");
        return match statements.as_slice() {
            [single] => Some(*single),
            _ => None,
        };
    }
    Some(statement)
}

fn names_by_binding<'a>(root: Node<'a>, binding: BindingRef<'a>) -> Vec<Node<'a>> {
    let key = binding.variable_declaration().unwrap_or(binding).key().to_owned();
    std::iter::once(root)
        .chain(root.descendants())
        .filter(|n| n.is(NodeKind::SimpleName))
        .filter(|n| n.binding().is_some_and(|b| b.kind() == binding.kind() && b.variable_declaration().unwrap_or(b).key() == key))
        .collect()
}

/// `QuickAssistProcessor.getSplitVariableProposal` (which creates the join fix).
pub fn join_variable(ctx: &Context, options: &Options, node: Node<'_>, out: &mut Vec<Proposal>) {
    let parent = node.parent();
    let mut on_first_access = false;
    let fragment;
    if node.is(NodeKind::SimpleName) && node.location_is("leftHandSide") {
        on_first_access = true;
        let Some(binding) = node.binding().filter(|b| b.is_variable()) else { return };
        match binding.declaring_node() {
            Some(d) if d.is(NodeKind::VariableDeclarationFragment) => fragment = d,
            _ => return,
        }
    } else if let Some(p) = parent.filter(|p| p.is(NodeKind::VariableDeclarationFragment)) {
        fragment = p;
    } else {
        return;
    }
    let Some(binding) = fragment.binding().filter(|b| !b.is_field()) else { return };
    let initializer = fragment.child("initializer");
    if initializer.is_some_and(|i| !i.is(NodeKind::NullLiteral)) {
        return;
    }
    let Some(statement) = fragment.parent().filter(|p| p.is(NodeKind::VariableDeclarationStatement)) else { return };
    let Some(statement_parent) = statement.parent() else { return };
    let names = names_by_binding(statement_parent, binding);
    if names.len() <= 1 || Some(names[0]) != fragment.child("name") {
        return;
    }
    let first_access = names[1];
    if on_first_access {
        if first_access != node {
            return;
        }
    } else if !first_access.location_is("leftHandSide") {
        return;
    }
    let Some(assignment) = first_access.parent().filter(|p| p.is(NodeKind::Assignment)) else { return };
    if !assignment.location_is("expression") {
        return;
    }
    let Some(assign_parent) = assignment.parent().filter(|p| p.is(NodeKind::ExpressionStatement)) else { return };
    let mut if_statement = None;
    let mut then_expression = None;
    let mut else_expression = None;
    let Some(mut assign_parent_parent) = assign_parent.parent() else { return };
    let in_then = assign_parent_parent.location_is("thenStatement") && assign_parent_parent.parent().is_some_and(|p| p.is(NodeKind::IfStatement));
    if assign_parent_parent.is(NodeKind::IfStatement) || (in_then && !subtree_match(assign_parent_parent, statement_parent)) {
        if in_then {
            assign_parent_parent = assign_parent_parent.parent().unwrap();
        }
        if !assign_parent_parent.is(NodeKind::IfStatement) {
            return;
        }
        let if_stmt = assign_parent_parent;
        if_statement = Some(if_stmt);
        let then_statement = single_statement(if_stmt.child("thenStatement"));
        let else_statement = single_statement(if_stmt.child("elseStatement"));
        let (Some(then_statement), Some(else_statement)) = (then_statement, else_statement) else { return };
        if then_statement.is(NodeKind::ExpressionStatement) && else_statement.is(NodeKind::ExpressionStatement) {
            let inner1 = then_statement.child("expression");
            let inner2 = else_statement.child("expression");
            if let (Some(assign1), Some(assign2)) = (inner1.filter(|i| i.is(NodeKind::Assignment)), inner2.filter(|i| i.is(NodeKind::Assignment))) {
                let left1 = assign1.child("leftHandSide");
                let left2 = assign2.child("leftHandSide");
                if let (Some(left1), Some(left2)) = (left1.filter(|l| l.kind().is_name()), left2.filter(|l| l.kind().is_name())) {
                    if assign1.simple("operator") == assign2.simple("operator") {
                        let bind1 = left1.binding();
                        let bind2 = left2.binding();
                        if bind1.is_some() && bind1 == bind2 && bind1.is_some_and(|b| b.is_variable()) {
                            then_expression = assign1.child("rightHandSide");
                            else_expression = assign2.child("rightHandSide");
                        }
                    }
                }
            }
        }
        if then_expression.is_none() || else_expression.is_none() {
            return;
        }
    } else {
        let mut n = assign_parent.parent();
        loop {
            match n {
                None => break,
                Some(x) if x.kind() == statement_parent.kind() && subtree_match(x, statement_parent) => break,
                Some(x) if x.is(NodeKind::Block) => n = x.parent(),
                Some(_) => return,
            }
        }
    }

    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    rw.add_tight_source_node(if_statement.unwrap_or(assign_parent).id);
    if let Some(if_stmt) = if_statement {
        let (Some(then_expression), Some(else_expression), Some(condition)) = (then_expression, else_expression, if_stmt.child("expression")) else { return };
        let conditional = rw.new_node(NodeKind::ConditionalExpression);
        let condition_copy = rw.create_copy_target(condition.id);
        rw.put_child(conditional, "expression", condition_copy);
        let then_copy = rw.create_copy_target(then_expression.id);
        let else_copy = rw.create_copy_target(else_expression.id);
        add_explicit_type_arguments_if_necessary(&mut rw, &mut imports, ctx, options, then_expression);
        add_explicit_type_arguments_if_necessary(&mut rw, &mut imports, ctx, options, else_expression);
        rw.put_child(conditional, "thenExpression", then_copy);
        rw.put_child(conditional, "elseExpression", else_copy);
        rw.set(RNode::Orig(fragment.id), "initializer", Some(conditional));
        rw.remove(RNode::Orig(if_stmt.id));
    } else {
        let Some(right) = assignment.child("rightHandSide") else { return };
        let placeholder = rw.create_move_target(right.id);
        rw.set(RNode::Orig(fragment.id), "initializer", Some(placeholder));
        if on_first_access {
            let moved = rw.create_move_target(statement.id);
            rw.replace(RNode::Orig(assign_parent.id), Some(moved));
        } else if is_control_statement_body(assign_parent.location(), assign_parent.parent()) {
            let block = rw.new_block(Vec::new());
            rw.replace(RNode::Orig(assign_parent.id), Some(block));
        } else {
            rw.remove(RNode::Orig(assign_parent.id));
        }
    }
    let label = messages::correction("QuickAssistProcessor_joindeclaration_description");
    out.push(Proposal::new(label, kind::QUICK_ASSIST, relevance::JOIN_VARIABLE_DECLARATION, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}

/// `JoinVariableProposalOperation.addExplicitTypeArgumentsIfNecessary`.
fn add_explicit_type_arguments_if_necessary(rw: &mut ASTRewrite, imports: &mut ImportRewrite, ctx: &Context, options: &Options, invocation: Node<'_>) {
    if !matches!(invocation.kind(), NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation | NodeKind::ClassInstanceCreation) || invocation.flags() & nflag::INFERRED_FROM_EXPECTED == 0 {
        return;
    }
    let (type_arguments, target, prop): (Vec<BindingRef<'_>>, RNode, &'static str) = match invocation.kind() {
        NodeKind::ClassInstanceCreation => {
            let Some(t) = invocation.child("type") else { return };
            let Some(b) = t.binding() else { return };
            (b.type_arguments(), RNode::Orig(t.id), "typeArguments")
        }
        _ => {
            let Some(m) = invocation.method_binding() else { return };
            (m.type_arguments(), RNode::Orig(invocation.id), "typeArguments")
        }
    };
    let context = import_context(&ctx.ast, invocation, options);
    for argument in type_arguments {
        let node = imports.add_import_type(argument, rw, &context, TypeLocation::TypeArgument);
        rw.list_insert_last(target, prop, node);
    }
    if invocation.is(NodeKind::MethodInvocation) && invocation.child("expression").is_none() {
        let expression = match invocation.method_binding().filter(|m| m.modifiers() & modifier::STATIC != 0) {
            Some(m) => {
                let declaring = m.declaring_class().map(|d| d.type_declaration().unwrap_or(d));
                match declaring {
                    Some(d) => {
                        let name = imports.add_import_binding(d, &context);
                        rw.new_name(&name)
                    }
                    None => return,
                }
            }
            None => rw.new_this_expression(),
        };
        rw.set(RNode::Orig(invocation.id), "expression", Some(expression));
    }
}

/// `QuickAssistProcessor.getInvertEqualsProposal`.
pub fn invert_equals(ctx: &Context, node: Node<'_>, out: &mut Vec<Proposal>) {
    let method = if node.is(NodeKind::MethodInvocation) {
        node
    } else {
        match node.parent().filter(|p| p.is(NodeKind::MethodInvocation)) {
            Some(p) => p,
            None => return,
        }
    };
    let Some(identifier) = method.child("name").map(|n| n.identifier()) else { return };
    if identifier != "equals" && identifier != "equalsIgnoreCase" {
        return;
    }
    let arguments = method.list("arguments");
    let [right] = arguments.as_slice() else { return };
    let right = *right;
    if let Some(binding) = right.type_binding() {
        if !binding.is_class() && !binding.is_interface() && !binding.is_enum() {
            return;
        }
    }
    let left = method.child("expression");
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let Some(name) = method.child("name") else { return };
    match left {
        None => {
            let name_copy = rw.create_copy_target(name.id);
            let this = rw.new_this_expression();
            let expression = rw.create_copy_target(right.id);
            let replacement = rw.new_node(NodeKind::MethodInvocation);
            rw.put_child(replacement, "name", name_copy);
            rw.put_list(replacement, "arguments", vec![this]);
            rw.put_child(replacement, "expression", expression);
            rw.replace(RNode::Orig(method.id), Some(replacement));
        }
        Some(left) if right.is(NodeKind::ThisExpression) => {
            let name_copy = rw.create_copy_target(name.id);
            let argument = rw.create_copy_target(left.id);
            let replacement = rw.new_node(NodeKind::MethodInvocation);
            rw.put_child(replacement, "name", name_copy);
            rw.put_list(replacement, "arguments", vec![argument]);
            rw.replace(RNode::Orig(method.id), Some(replacement));
        }
        Some(left) => {
            let unparenthesed = unparenthesed_expression(left);
            let copy = rw.create_copy_target(unparenthesed.id);
            rw.replace(RNode::Orig(right.id), Some(copy));
            if matches!(right.kind(), NodeKind::CastExpression | NodeKind::Assignment | NodeKind::ConditionalExpression | NodeKind::InfixExpression) {
                let inner = rw.create_copy_target(right.id);
                let paren = rw.new_parenthesized_expression(inner);
                rw.replace(RNode::Orig(left.id), Some(paren));
            } else {
                let copy = rw.create_copy_target(right.id);
                rw.replace(RNode::Orig(left.id), Some(copy));
            }
        }
    }
    let label = messages::correction("QuickAssistProcessor_invertequals_description");
    out.push(Proposal::rewrite(label, kind::QUICK_ASSIST, relevance::INVERT_EQUALS, rw));
}
