//! Port of jdt.ls `ReturnTypeSubProcessor` over jdt.core.manipulation's
//! `ReturnTypeBaseSubProcessor`, with `MissingReturnTypeCorrectionProposalCore`
//! and `MissingReturnTypeInLambdaCorrectionProposalCore`.

use super::edit::Env;
use super::javadoc_tags::{find_tag, insert_tag, new_tag, text};
use super::type_mismatch::bindings::{normalize_type_binding, type_label, well_known, Ty};
use super::type_mismatch::proposals::{add_import, import_context};
use super::unresolved_elements::normalize_wildcard;
use super::{kind, local_corrections, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::refactoring::checks::is_assignment_compatible;
use crate::refactoring::scope::{ScopeAnalyzer, CHECK_VISIBILITY, VARIABLES};
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::finder::NodeFinder;
use crate::semantic_ast::resolve::{find_parent_body_declaration, find_parent_type};
use crate::semantic_ast::{modifier as m, BindingRef, Node, NodeKind};

fn rewrite_proposal(label: impl Into<String>, relevance: i32, rw: ASTRewrite) -> Proposal {
    Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw)
}

/// `ReturnStatementCollector.getTypeBinding(ast)`.
fn collected_return_type<'a>(decl: Node<'a>) -> Ty<'a> {
    fn collect<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
        match n.kind() {
            NodeKind::ReturnStatement => out.push(n),
            NodeKind::AnonymousClassDeclaration | NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration => {}
            _ => n.children().into_iter().for_each(|c| collect(c, out)),
        }
    }
    let mut returns = Vec::new();
    for c in decl.children() {
        collect(c, &mut returns);
    }
    let mut could_be_object = false;
    for node in returns {
        match node.child("expression") {
            Some(expr) => match normalize_type_binding(expr.type_binding()) {
                Some(b) => return Ty::Binding(b),
                None => could_be_object = true,
            },
            None => return Ty::Named("void"),
        }
    }
    if could_be_object {
        Ty::Named("java.lang.Object")
    } else {
        Ty::Named("void")
    }
}

/// A well-known type as a binding when the bridge exported it.
fn resolve_named<'a>(ctx: &'a Context, ty: Ty<'a>) -> Ty<'a> {
    match ty {
        Ty::Named(name) => well_known(ctx.ast(), name).map_or(ty, Ty::Binding),
        b => b,
    }
}

/// `BindingLabelProviderCore.getBindingLabel(type, DEFAULT_TEXTFLAGS)`.
fn label_of(ty: Ty<'_>) -> String {
    match ty {
        Ty::Binding(b) => type_label(b),
        Ty::Named(name) => name.rsplit('.').next().unwrap_or(name).to_owned(),
    }
}

fn is_wildcard(ty: Ty<'_>) -> bool {
    matches!(ty, Ty::Binding(b) if b.is_wildcard_type())
}

/// `rewrite.getListRewrite(javadoc, TAGS).insert(@return)` with a comment start.
fn insert_return_tag(rw: &mut ASTRewrite, method: Node<'_>) {
    if let Some(javadoc) = method.child("javadoc") {
        let comment_start = text(rw, "");
        let tag = new_tag(rw, "@return", vec![comment_start]);
        insert_tag(rw, RNode::Orig(javadoc.id), tag, None);
    }
}

/// `collectMethodWithConstrNameProposals`.
pub fn method_with_constructor_name(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(declaration) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.set_simple(RNode::Orig(declaration.id), "constructor", Some("true"));
    proposals.push(rewrite_proposal(messages::correction("ReturnTypeSubProcessor_constrnamemethod_description"), relevance::CHANGE_TO_CONSTRUCTOR, rw));
}

/// `collectVoidMethodReturnsProposals`.
pub async fn void_method_returns(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let Some(method) = find_parent_body_declaration(selected).filter(|d| d.is(NodeKind::MethodDeclaration)) else { return };
    if !selected.is(NodeKind::ReturnStatement) {
        return;
    }
    // The problem implies an expression (`remove(null)` would throw).
    let Some(expr) = selected.child("expression") else { return };
    let options = env.options(&ctx.ast.uri).await;
    let mut binding = match normalize_type_binding(expr.type_binding()) {
        Some(b) => Ty::Binding(b),
        None => resolve_named(ctx, Ty::Named("java.lang.Object")),
    };
    if let Ty::Binding(b) = binding {
        if b.is_wildcard_type() {
            binding = normalize_wildcard(b, true).map_or(Ty::Named("java.lang.Object"), Ty::Binding);
        }
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let label = messages::format(messages::correction("ReturnTypeSubProcessor_voidmethodreturns_description"), &[&label_of(binding)]);
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
    let context = import_context(&ctx.ast, method, &options);
    let new_return_type = add_import(&mut imports, &mut rw, &context, binding, TypeLocation::ReturnType);
    if method.flag("constructor") {
        rw.set_simple(RNode::Orig(method.id), "constructor", Some("false"));
        rw.set(RNode::Orig(method.id), "returnType2", Some(new_return_type));
    } else if let Some(return_type) = method.child("returnType2") {
        rw.replace(RNode::Orig(return_type.id), Some(new_return_type));
    }
    insert_return_tag(&mut rw, method);
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::VOID_METHOD_RETURNS, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));

    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.remove(RNode::Orig(expr.id));
    proposals.push(rewrite_proposal(messages::correction("ReturnTypeSubProcessor_removereturn_description"), relevance::CHANGE_TO_RETURN, rw));
}

/// `collectMissingReturnTypeProposals`.
pub async fn missing_return_type(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let Some(method) = find_parent_body_declaration(selected).filter(|d| d.is(NodeKind::MethodDeclaration)) else { return };
    let options = env.options(&ctx.ast.uri).await;
    let mut type_binding = resolve_named(ctx, collected_return_type(method));
    if let Ty::Binding(b) = type_binding {
        // `Bindings.normalizeTypeBinding`, `void` when it normalizes to null.
        type_binding = normalize_type_binding(Some(b)).map_or_else(|| resolve_named(ctx, Ty::Named("void")), Ty::Binding);
    }
    if is_wildcard(type_binding) {
        if let Ty::Binding(b) = type_binding {
            type_binding = normalize_wildcard(b, true).map_or(Ty::Named("java.lang.Object"), Ty::Binding);
        }
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let label = messages::format(messages::correction("ReturnTypeSubProcessor_missingreturntype_description"), &[&label_of(type_binding)]);
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
    let context = import_context(&ctx.ast, method, &options);
    let typ = add_import(&mut imports, &mut rw, &context, type_binding, TypeLocation::ReturnType);
    rw.set(RNode::Orig(method.id), "returnType2", Some(typ));
    rw.set_simple(RNode::Orig(method.id), "constructor", Some("false"));
    insert_return_tag(&mut rw, method);
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::MISSING_RETURN_TYPE, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));

    // change to constructor
    if let Some(parent_type) = find_parent_type(method).filter(|t| t.kind().is_abstract_type_declaration()) {
        let is_interface = parent_type.is(NodeKind::TypeDeclaration) && parent_type.flag("interface");
        if !is_interface {
            let constructor_name = parent_type.child("name").map(|n| n.identifier()).unwrap_or_default();
            if let Some(name) = method.child("name") {
                let label = messages::format(messages::correction("ReturnTypeSubProcessor_wrongconstructorname_description"), &[&constructor_name]);
                proposals.push(local_corrections::replace_proposal(ctx, label, name.start(), name.length(), &constructor_name, relevance::CHANGE_TO_CONSTRUCTOR));
            }
        }
    }
}

/// `MissingReturnTypeCorrectionProposalCore` (method or lambda).
struct MissingReturn<'a> {
    /// The method declaration, or `None` for a lambda.
    method: Option<Node<'a>>,
    lambda: Option<Node<'a>>,
    existing: Option<Node<'a>>,
}

impl<'a> MissingReturn<'a> {
    fn body(&self) -> Option<Node<'a>> {
        self.method.or(self.lambda).and_then(|n| n.child("body"))
    }

    /// `getReturnTypeBinding()`.
    fn return_type_binding(&self) -> Option<BindingRef<'a>> {
        let binding = match (self.method, self.lambda) {
            (Some(m), _) => m.binding(),
            (None, Some(l)) => l.method_binding().or_else(|| l.binding()),
            _ => None,
        }?;
        binding.return_type()
    }

    /// `getModifiers()`.
    fn modifiers(&self) -> i32 {
        self.method.map_or(0, |m| m.modifiers())
    }

    /// `testModifier(curr)`.
    fn test_modifier(&self, curr: BindingRef<'_>) -> bool {
        let modifiers = curr.modifiers();
        let static_final = m::STATIC | m::FINAL;
        if modifiers & static_final == static_final {
            return false;
        }
        !(modifiers & m::STATIC != 0 && self.modifiers() & m::STATIC == 0)
    }

    /// `computeProposals(...)`: the first variable in scope of the return type.
    fn compute_proposal(&self, ctx: &'a Context, return_binding: BindingRef<'_>, offset: usize) -> Option<String> {
        let root = ctx.root();
        let excluded = if self.lambda.is_some() {
            NodeFinder::new(root, offset, 0)
                .covering
                .and_then(|n| n.ancestor_or_self(|k| k == NodeKind::VariableDeclarationFragment))
                .and_then(|f| f.binding())
                .map(|b| b.key().to_owned())
        } else {
            None
        };
        let mut analyzer = ScopeAnalyzer::new(root);
        for curr in analyzer.declarations_in_scope(offset, VARIABLES | CHECK_VISIBILITY) {
            let Some(t) = curr.var_type() else { continue };
            if is_assignment_compatible(t, return_binding) && self.test_modifier(curr) && excluded.as_deref() != Some(curr.key()) {
                return Some(curr.name().to_owned());
            }
        }
        None
    }

    /// `createDefaultExpression(ast)`.
    fn default_expression(&self, rw: &mut ASTRewrite) -> Option<RNode> {
        if let Some(method) = self.method {
            let typ = method.child("returnType2")?;
            let extra = method.list("extraDimensions2").len();
            if extra == 0 && typ.is(NodeKind::PrimitiveType) {
                return match typ.simple("primitiveTypeCode") {
                    Some("boolean") => Some(boolean_false(rw)),
                    Some("void") => None,
                    _ => Some(rw.new_number_literal("0")),
                };
            }
            if extra == 0 && typ.is(NodeKind::ParameterizedType) {
                if let Some(outer) = typ.child("type").filter(|t| t.is(NodeKind::SimpleType)) {
                    let name = outer.child("name").map(|n| n.identifier()).unwrap_or_default();
                    if name == "java.util.Optional" {
                        let qualifier = rw.new_name(&name);
                        return Some(rw.new_method_invocation(Some(qualifier), "empty", Vec::new()));
                    }
                }
            }
            return Some(rw.new_node(NodeKind::NullLiteral));
        }
        let t = self.return_type_binding()?;
        if t.is_primitive() {
            return match t.name() {
                "boolean" => Some(boolean_false(rw)),
                "void" => None,
                _ => Some(rw.new_number_literal("0")),
            };
        }
        let erasure = t.erasure().unwrap_or(t).qualified_name().to_owned();
        if erasure == "java.util.Optional" {
            let qualifier = rw.new_name(&erasure);
            return Some(rw.new_method_invocation(Some(qualifier), "empty", Vec::new()));
        }
        Some(rw.new_node(NodeKind::NullLiteral))
    }

    /// `evaluateReturnExpressions(ast, returnBinding, offset)`.
    fn return_expression(&self, ctx: &'a Context, rw: &mut ASTRewrite, offset: usize) -> Option<RNode> {
        let found = self.return_type_binding().and_then(|b| self.compute_proposal(ctx, b, offset));
        let default = self.default_expression(rw);
        match found {
            Some(name) => Some(rw.new_simple_name(&name)),
            None => default,
        }
    }

    /// `getRewrite()`.
    fn rewrite(&self, ctx: &'a Context) -> ASTRewrite {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let return_binding = self.return_type_binding();
        if let Some(existing) = self.existing {
            if let Some(expression) = self.return_expression(ctx, &mut rw, existing.start()) {
                rw.set(RNode::Orig(existing.id), "expression", Some(expression));
            }
            return rw;
        }
        let Some(block) = self.body().filter(|b| b.is(NodeKind::Block)) else { return rw };
        let statements = block.list("statements");
        let last = statements.last().copied();
        if let (Some(return_binding), Some(last)) = (return_binding, last) {
            if last.is(NodeKind::ExpressionStatement) {
                if let Some(expression) = last.child("expression") {
                    if expression.type_binding().is_some_and(|b| is_assignment_compatible(b, return_binding)) {
                        let placeholder = rw.create_move_target(expression.id);
                        let return_statement = rw.new_return_statement(Some(placeholder));
                        rw.replace(RNode::Orig(last.id), Some(return_statement));
                        return rw;
                    }
                }
            }
        }
        let offset = match last {
            None => block.start() + 1,
            Some(l) => l.end(),
        };
        let expression = self.return_expression(ctx, &mut rw, offset);
        let return_statement = rw.new_return_statement(expression);
        rw.list_insert_last(RNode::Orig(block.id), "statements", return_statement);
        rw
    }

    fn label(&self) -> &'static str {
        messages::correction(if self.existing.is_some() {
            "MissingReturnTypeCorrectionProposal_changereturnstatement_description"
        } else {
            "MissingReturnTypeCorrectionProposal_addreturnstatement_description"
        })
    }
}

fn boolean_false(rw: &mut ASTRewrite) -> RNode {
    let n = rw.new_node(NodeKind::BooleanLiteral);
    rw.put_simple(n, "booleanValue", "false")
}

/// `collectMissingReturnStatementProposals`.
pub fn missing_return_statement(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(selected) = problem.covering_node(ctx.ast()) else { return };
    let existing = selected.is(NodeKind::ReturnStatement).then_some(selected);
    if selected.is(NodeKind::LambdaExpression) {
        let proposal = MissingReturn { method: None, lambda: Some(selected), existing };
        proposals.push(rewrite_proposal(proposal.label(), relevance::MISSING_RETURN_TYPE, proposal.rewrite(ctx)));
        return;
    }
    let Some(method) = find_parent_body_declaration(selected).filter(|d| d.is(NodeKind::MethodDeclaration)) else { return };
    if method.child("body").is_none() {
        return;
    }
    let proposal = MissingReturn { method: Some(method), lambda: None, existing };
    proposals.push(rewrite_proposal(proposal.label(), relevance::MISSING_RETURN_TYPE, proposal.rewrite(ctx)));
    if let Some(return_type) = method.child("returnType2") {
        let rw0 = ASTRewrite::new(ctx.ast.clone());
        let as_string = crate::rewrite::flattener::Flattener::as_string(&rw0, RNode::Orig(return_type.id));
        if as_string != "void" {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let void = rw.new_primitive_type("void");
            rw.replace(RNode::Orig(return_type.id), Some(void));
            if let Some(javadoc) = method.child("javadoc") {
                if let Some(tag) = find_tag(javadoc, "@return", None) {
                    rw.remove(RNode::Orig(tag.id));
                }
            }
            proposals.push(rewrite_proposal(messages::correction("ReturnTypeSubProcessor_changetovoid_description"), relevance::CHANGE_RETURN_TYPE_TO_VOID, rw));
        }
    }
}

/// `collectReplaceReturnWithYieldStatementProposals`.
pub fn replace_return_with_yield(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(return_statement) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::ReturnStatement)) else { return };
    let Some(expression) = return_statement.child("expression") else { return };
    let Some(parent) = return_statement.parent() else { return };
    if !matches!(parent.kind(), NodeKind::Block | NodeKind::SwitchExpression) {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let moved = rw.create_move_target(expression.id);
    let yield_statement = rw.new_node(NodeKind::YieldStatement);
    rw.put_child(yield_statement, "expression", moved);
    rw.replace(RNode::Orig(return_statement.id), Some(yield_statement));
    proposals.push(rewrite_proposal(messages::correction("ReturnTypeSubProcessor_changeReturnToYield_description"), relevance::REMOVE_ABSTRACT_MODIFIER, rw));
}

/// `collectMethodReturnsVoidProposals`.
pub async fn method_returns_void(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(return_statement) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::ReturnStatement)) else { return };
    let Some(expression) = return_statement.child("expression") else { return };
    let Some(method) = find_parent_body_declaration(return_statement).filter(|d| d.is(NodeKind::MethodDeclaration)) else { return };
    let Some(return_type) = method.child("returnType2").and_then(|t| t.binding().or_else(|| t.type_binding())) else { return };
    super::type_mismatch::change_sender_type_proposals(env, ctx, expression, return_type, false, relevance::METHOD_RETURNS_VOID, proposals).await;
}
