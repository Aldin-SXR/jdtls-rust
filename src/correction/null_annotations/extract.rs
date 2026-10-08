//! `ExtractToNullCheckedLocalProposalCore`.

use std::sync::Arc;

use crate::correction::edit::Env;
use crate::correction::{kind, messages, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal};
use crate::refactoring::naming::{suggest_variable_names, VarKind};
use crate::refactoring::scope::ScopeAnalyzer;
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{problem as p, Ast, BindingRef, Node, NodeId, NodeKind};

/// `findProblemFieldName`.
fn find_problem_field_name<'a>(selected: Node<'a>, problem_id: i32) -> Option<Node<'a>> {
    let mut node = selected;
    if node.is(NodeKind::FieldAccess) || node.is(NodeKind::QualifiedName) {
        node = node.child("name")?;
    }
    if !node.is(NodeKind::SimpleName) {
        return None;
    }
    if problem_id == p::NullableFieldReference {
        return Some(node);
    }
    // not field dereference, but compatibility issue - is value a field reference?
    node.binding().filter(|b| b.is_variable() && b.is_field()).map(|_| node)
}

/// `getExtractCheckedLocalProposal`.
pub fn extract_checked_local_proposal(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let Some(name) = find_problem_field_name(selected, problem.problem_id) else { return };
    let method = selected
        .ancestors()
        .find(|a| a.is(NodeKind::MethodDeclaration))
        .or_else(|| selected.ancestors().find(|a| a.is(NodeKind::Initializer)));
    let Some(method) = method else { return };
    let change = ExtractToNullCheckedLocal { ast: ctx.ast.clone(), field_reference: name.id, enclosing_method: method.id };
    proposals.push(Proposal::new(
        messages::fix("ExtractToNullCheckedLocalProposal_extractToCheckedLocal_proposalName"),
        kind::QUICK_FIX,
        100,
        Change::Lazy(Box::new(change)),
    ));
}

struct ExtractToNullCheckedLocal {
    ast: Arc<Ast>,
    field_reference: NodeId,
    enclosing_method: NodeId,
}

/// `RearrangeStrategy`.
enum Strategy<'a> {
    ModifyBlock { block: Node<'a>, orig: Node<'a> },
    ModifyBlockWithLocalDecl { block: Node<'a>, orig: Node<'a> },
    ReplaceStatement { block: RNode, orig: Node<'a> },
}

impl<'a> Strategy<'a> {
    fn create(orig: Node<'a>, rw: &mut ASTRewrite) -> Strategy<'a> {
        match orig.parent() {
            Some(block) if block.is(NodeKind::Block) => {
                if orig.is(NodeKind::VariableDeclarationStatement) {
                    Strategy::ModifyBlockWithLocalDecl { block, orig }
                } else {
                    Strategy::ModifyBlock { block, orig }
                }
            }
            _ => Strategy::ReplaceStatement { block: rw.new_block(Vec::new()), orig },
        }
    }

    fn insert_local_decl(&self, rw: &mut ASTRewrite, local_decl: RNode) {
        match self {
            Strategy::ModifyBlock { block, orig } | Strategy::ModifyBlockWithLocalDecl { block, orig } => {
                rw.list_insert_before(RNode::Orig(block.id), "statements", local_decl, RNode::Orig(orig.id));
            }
            Strategy::ReplaceStatement { block, .. } => append_statement(rw, *block, local_decl),
        }
    }

    fn create_move_target_for_orig_stmt(&self, rw: &mut ASTRewrite) -> RNode {
        match self {
            Strategy::ModifyBlock { block, orig } | Strategy::ModifyBlockWithLocalDecl { block, orig } => {
                move_statements(rw, *block, orig.id, orig.id)
            }
            Strategy::ReplaceStatement { orig, .. } => rw.create_move_target(orig.id),
        }
    }

    fn insert_if_statement(&self, rw: &mut ASTRewrite, if_stmt: RNode, then_block: RNode) {
        match self {
            Strategy::ModifyBlock { block, orig } => {
                rw.list_replace(RNode::Orig(block.id), "statements", RNode::Orig(orig.id), if_stmt);
            }
            Strategy::ModifyBlockWithLocalDecl { block, orig } => {
                let statements = block.list("statements");
                let index = statements.iter().position(|s| s == orig);
                if let Some(index) = index.filter(|i| i + 1 < statements.len()) {
                    let target = move_statements(rw, *block, statements[index + 1].id, statements[statements.len() - 1].id);
                    append_statement(rw, then_block, target);
                }
                rw.list_replace(RNode::Orig(block.id), "statements", RNode::Orig(orig.id), if_stmt);
            }
            Strategy::ReplaceStatement { block, orig } => {
                append_statement(rw, *block, if_stmt);
                rw.replace(RNode::Orig(orig.id), Some(*block));
            }
        }
    }
}

/// `ListRewrite.createMoveTarget(first, last)` of block statements.
fn move_statements(rw: &mut ASTRewrite, block: Node<'_>, first: NodeId, last: NodeId) -> RNode {
    if first == last {
        rw.list_remove(RNode::Orig(block.id), "statements", RNode::Orig(first));
        return rw.create_move_target(first);
    }
    rw.list_create_range_target(RNode::Orig(block.id), "statements", first, last, None, true)
}

fn append_statement(rw: &mut ASTRewrite, block: RNode, statement: RNode) {
    let mut statements = rw.new_node_data(block).and_then(|n| n.props.iter().find(|(p, _)| *p == "statements").map(|(_, v)| v.list())).unwrap_or_default();
    statements.push(statement);
    rw.put_list(block, "statements", statements);
}

/// `ExtractToNullCheckedLocalProposalCore.newType`.
pub(super) fn new_type(rw: &mut ASTRewrite, imports: &mut ImportRewrite, binding: BindingRef<'_>) -> RNode {
    let dimensions = binding.dimensions().max(0) as usize;
    let mut binding = binding;
    if dimensions > 0 {
        binding = binding.element_type().unwrap_or(binding);
    }
    let type_arguments = binding.type_arguments();
    binding = binding.erasure().unwrap_or(binding);
    let mut element = if binding.is_primitive() {
        rw.new_primitive_type(binding.name())
    } else {
        let name = imports.add_import_binding(binding, &DefaultContext);
        let name = rw.new_name(&name);
        rw.new_simple_type(name)
    };
    if !type_arguments.is_empty() {
        let arguments: Vec<RNode> = type_arguments.into_iter().map(|a| new_type(rw, imports, a)).collect();
        let parameterized = rw.new_node(NodeKind::ParameterizedType);
        rw.put_child(parameterized, "type", element);
        rw.put_list(parameterized, "typeArguments", arguments);
        element = parameterized;
    }
    if dimensions > 0 {
        let array = rw.new_node(NodeKind::ArrayType);
        rw.put_child(array, "elementType", element);
        let dims = (0..dimensions).map(|_| rw.new_node(NodeKind::Dimension)).collect();
        rw.put_list(array, "dimensions", dims);
        return array;
    }
    element
}

#[tower_lsp::async_trait]
impl LazyChange for ExtractToNullCheckedLocal {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options = env.options(&self.ast.uri).await;
        let field_reference = self.ast.node(self.field_reference);
        let enclosing_method = self.ast.node(self.enclosing_method);
        let mut rw = ASTRewrite::new(self.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(self.ast.clone(), &options);

        let Some(orig_stmt) = field_reference.ancestors().find(|a| a.kind().is_statement()) else { return Ok(Vec::new()) };
        let strategy = Strategy::create(orig_stmt, &mut rw);

        let direct_parent = field_reference.parent();
        let to_replace = match direct_parent {
            Some(parent) if parent.is(NodeKind::FieldAccess) || (parent.is(NodeKind::QualifiedName) && field_reference.location_is("name")) => parent,
            _ => field_reference,
        };

        let identifier = field_reference.identifier();
        let mut excluded: Vec<String> = ScopeAnalyzer::new(self.ast.root()).used_variable_names(enclosing_method.start(), enclosing_method.length()).into_iter().collect();
        excluded.sort();
        excluded.push(identifier.clone());
        let local_name = suggest_variable_names(VarKind::Local, &identifier, 0, &excluded, &options, true).into_iter().next().unwrap_or(identifier);

        let Some(to_replace_type) = to_replace.type_binding() else { return Ok(Vec::new()) };
        let local_type = new_type(&mut rw, &mut imports, to_replace_type);
        let initializer = rw.copy_subtree(RNode::Orig(to_replace.id));
        let name = rw.new_simple_name(&local_name);
        let fragment = rw.new_node(NodeKind::VariableDeclarationFragment);
        rw.put_child(fragment, "name", name);
        rw.put_child(fragment, "initializer", initializer);
        let local_decl = rw.new_node(NodeKind::VariableDeclarationStatement);
        rw.put_list(local_decl, "fragments", vec![fragment]);
        rw.put_child(local_decl, "type", local_type);
        let final_modifier = rw.new_modifier("final");
        rw.put_list(local_decl, "modifiers", vec![final_modifier]);
        strategy.insert_local_decl(&mut rw, local_decl);

        let left = rw.new_simple_name(&local_name);
        let right = rw.new_node(NodeKind::NullLiteral);
        let null_check = rw.new_infix_expression(left, "!=", right);
        let if_stmt = rw.new_node(NodeKind::IfStatement);
        rw.put_child(if_stmt, "expression", null_check);

        let then_block = rw.new_block(Vec::new());
        let moved = strategy.create_move_target_for_orig_stmt(&mut rw);
        append_statement(&mut rw, then_block, moved);
        rw.put_child(if_stmt, "thenStatement", then_block);
        let dereferenced = rw.new_simple_name(&local_name);
        rw.replace(RNode::Orig(to_replace.id), Some(dereferenced));

        let else_block = rw.new_block(Vec::new());
        let mut todo = format!("// TODO {}", messages::fix("ExtractToNullCheckedLocalProposal_todoHandleNullDescription"));
        if orig_stmt.is(NodeKind::ReturnStatement) {
            if let Some(binding) = orig_stmt.child("expression").and_then(|e| e.type_binding()) {
                todo.push_str("\nreturn ");
                todo.push_str(default_expression(binding));
                todo.push(';');
            }
        }
        let placeholder = rw.create_string_placeholder(&todo, NodeKind::EmptyStatement);
        append_statement(&mut rw, else_block, placeholder);
        rw.put_child(if_stmt, "elseStatement", else_block);

        strategy.insert_if_statement(&mut rw, if_stmt, then_block);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

/// `ASTNodeFactory.newDefaultExpression` for a type without extra dimensions.
fn default_expression(binding: BindingRef<'_>) -> &'static str {
    if binding.is_primitive() && !binding.is_array() {
        match binding.name() {
            "boolean" => "false",
            _ => "0",
        }
    } else {
        "null"
    }
}
