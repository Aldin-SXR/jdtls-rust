//! Port of `Java50FixCore.createRawTypeReferenceFix` (the "Add type
//! arguments" quick fix) and the type argument inference it runs
//! (`InferTypeArgumentsConstraintCreator`, `InferTypeArgumentsTCModel`,
//! `ParametricStructureComputer`, `InferTypeArgumentsConstraintsSolver` and
//! `InferTypeArgumentsRefactoring.inferArguments`).

pub mod creator;
pub mod model;
pub mod solver;

use std::collections::HashSet;

use self::model::{CvId, CvKind, Model, NOT_DECLARED, TT};
use self::solver::Solution;
use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{problem as p, Ast, Node, NodeId, NodeKind};

/// `Java50FixCore.isRawTypeReferenceProblem`.
pub fn is_raw_type_reference_problem(id: i32) -> bool {
    matches!(id, p::UnsafeTypeConversion | p::UnsafeElementTypeConversion | p::RawTypeReference | p::UnsafeRawMethodInvocation)
}

/// `Java50FixCore.hasFatalError`: an error of a build path, syntax, import,
/// type, member, internal or module category.
fn has_fatal_error(ast: &Ast) -> bool {
    ast.problems.iter().any(|p| p.is_error && matches!(p.category, 10 | 20 | 30 | 40 | 50 | 60 | 160))
}

fn is_var(node: Node<'_>) -> bool {
    node.is(NodeKind::SimpleType) && node.child("name").is_some_and(|n| n.is(NodeKind::SimpleName) && n.identifier() == "var")
}

/// `Java50FixCore.isRawTypeReference`.
fn is_raw_type_reference(node: Node<'_>) -> bool {
    if !node.is(NodeKind::SimpleType) {
        return false;
    }
    let Some(binding) = node.binding() else { return false };
    let declaration = binding.type_declaration().unwrap_or(binding);
    !declaration.type_parameters().is_empty()
}

/// `Java50FixCore.getRawReference(SimpleName)`: the raw declaration type of
/// the variable or method `name` refers to.
fn raw_reference_of_name<'a>(ast: &'a Ast, name: Node<'a>) -> Option<Node<'a>> {
    let binding = name.binding()?;
    for n in ast.all_nodes().filter(|n| n.is(NodeKind::SimpleName) && n.binding().is_some_and(|b| b == binding)) {
        let Some(parent) = n.parent() else { continue };
        let candidate = match parent.kind() {
            NodeKind::VariableDeclarationFragment => {
                let owner = parent.parent()?;
                if matches!(owner.kind(), NodeKind::VariableDeclarationStatement | NodeKind::FieldDeclaration) {
                    owner.child("type")
                } else {
                    None
                }
            }
            NodeKind::SingleVariableDeclaration => parent.child("type"),
            NodeKind::MethodDeclaration => parent.child("returnType2"),
            _ => None,
        };
        if let Some(c) = candidate.filter(|c| is_raw_type_reference(*c)) {
            return Some(c);
        }
    }
    None
}

/// `Java50FixCore.getRawReference(MethodInvocation)`.
fn raw_reference_of_invocation<'a>(ast: &'a Ast, invocation: Node<'a>) -> Option<Node<'a>> {
    if let Some(r) = invocation.child("name").filter(|n| n.is(NodeKind::SimpleName)).and_then(|n| raw_reference_of_name(ast, n)) {
        return Some(r);
    }
    let expression = invocation.child("expression")?;
    match expression.kind() {
        NodeKind::SimpleName => raw_reference_of_name(ast, expression),
        NodeKind::QualifiedName => {
            let mut name = expression;
            while name.is(NodeKind::QualifiedName) {
                if let Some(r) = name.child("name").and_then(|n| raw_reference_of_name(ast, n)) {
                    return Some(r);
                }
                name = name.child("qualifier")?;
            }
            raw_reference_of_name(ast, name)
        }
        NodeKind::MethodInvocation => raw_reference_of_invocation(ast, expression),
        _ => None,
    }
}

/// `Java50FixCore.createRawTypeReferenceOperations` for one problem.
fn raw_type_references<'a>(ast: &'a Ast, problem: &ProblemLocation) -> Vec<Node<'a>> {
    if has_fatal_error(ast) || !is_raw_type_reference_problem(problem.problem_id) {
        return Vec::new();
    }
    let mut result = Vec::new();
    let Some(mut node) = problem.covered_node(ast) else { return result };
    if node.is(NodeKind::SimpleType) {
        if let Some(name) = node.child("name").filter(|n| n.is(NodeKind::SimpleName)) {
            node = name;
        }
    }
    match node.kind() {
        NodeKind::ClassInstanceCreation => {
            if let Some(t) = node.child("type").filter(|t| !is_var(*t) && is_raw_type_reference(*t)) {
                result.push(t);
            }
        }
        NodeKind::SimpleName => {
            if let Some(raw) = node.parent().filter(|p| is_raw_type_reference(*p)) {
                let parent = raw.parent();
                if !parent.is_some_and(|p| matches!(p.kind(), NodeKind::ArrayType | NodeKind::ParameterizedType)) && !is_var(raw) {
                    result.push(raw);
                }
            }
        }
        NodeKind::MethodInvocation => {
            if let Some(raw) = raw_reference_of_invocation(ast, node).filter(|r| !is_var(*r)) {
                result.push(raw);
            }
        }
        _ => {}
    }
    result
}

/// `LocalCorrectionsSubProcessor.addTypeParametersToRawTypeReference`:
/// `Java50FixCore.createRawTypeReferenceFix` as a `FixCorrectionProposalCore`.
pub async fn raw_type_reference_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast.clone();
    let types = raw_type_references(&ast, problem);
    let Some(first) = types.first() else { return };
    let name = first.child("name").map(|n| n.identifier()).unwrap_or_default();
    let label = messages::format(messages::fix("Java50Fix_AddTypeArguments_description"), &[&name]);

    // AddTypeParametersOperation.rewriteAST
    let mut model = Model::default();
    if let Some(object) = ast.type_by_name("java.lang.Object") {
        model.register(TT::B(object));
    }
    creator::Creator::new(&mut model).walk(ast.root());
    let solution = solver::solve(&mut model);

    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let type_ids: HashSet<NodeId> = types.iter().map(|t| t.id).collect();
    let mut rewritten = HashSet::new();
    let mut changed = false;
    for &cv in &solution.declarations {
        let CvKind::Element(parent, ..) = model.cvs[cv].kind else { continue };
        let CvKind::Type(type_node) = model.cvs[parent].kind else { continue };
        if !type_ids.contains(&type_node) || !rewritten.insert(type_node) {
            continue;
        }
        let original = ast.node(type_node);
        if is_var(original) || !original.child("name").is_some_and(|n| n.kind().is_name()) {
            continue;
        }
        let argument_cvs = type_argument_cvs(&model, parent);
        let Some(arguments) = type_arguments(&model, &solution, original, &argument_cvs, &mut rw, &mut imports, 0) else { continue };
        let moving = rw.create_copy_target(original.id);
        let parameterized = rw.new_node(NodeKind::ParameterizedType);
        rw.put_child(parameterized, "type", moving);
        let name = original.child("name").unwrap_or(original);
        let ancestor = name.ancestors().find(|a| matches!(a.kind(), NodeKind::ClassInstanceCreation | NodeKind::CastExpression | NodeKind::VariableDeclarationFragment | NodeKind::Assignment));
        let arguments = if ancestor.is_none_or(|a| a.is(NodeKind::CastExpression)) { arguments } else { Vec::new() };
        rw.put_list(parameterized, "typeArguments", arguments);
        rw.replace(RNode::Orig(original.id), Some(parameterized));
        changed = true;
    }
    if !changed {
        return;
    }
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::CHANGE_VARIABLE, Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])));
}

/// `InferTypeArgumentsRefactoring.getTypeArgumentCvs`.
fn type_argument_cvs(model: &Model<'_>, base: CvId) -> Vec<Option<CvId>> {
    let mut out: Vec<Option<CvId>> = Vec::new();
    for (_, element) in model.element_variables(base) {
        if let CvKind::Element(_, _, index) = model.cvs[element].kind {
            if index != NOT_DECLARED {
                let index = index as usize;
                while index >= out.len() {
                    out.push(None);
                }
                out[index] = Some(element);
            }
        }
    }
    out
}

/// `InferTypeArgumentsRefactoring.unboundedWildcardAllowed`.
fn unbounded_wildcard_allowed(original: Option<Node<'_>>) -> bool {
    let Some(original) = original else { return true };
    let mut parent = original.parent();
    while let Some(p) = parent.filter(|p| p.kind().is_type()) {
        parent = p.parent();
    }
    !parent.is_some_and(|p| p.is(NodeKind::ClassInstanceCreation) || p.kind().is_abstract_type_declaration() || p.is(NodeKind::TypeLiteral))
}

/// `InferTypeArgumentsRefactoring.getTypeArguments`.
#[allow(clippy::too_many_arguments)]
fn type_arguments<'a>(
    model: &Model<'a>,
    solution: &Solution<'a>,
    base_type: Node<'_>,
    argument_cvs: &[Option<CvId>],
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    depth: usize,
) -> Option<Vec<RNode>> {
    if argument_cvs.is_empty() || depth > 8 {
        return None;
    }
    let mut out = Vec::new();
    for cv in argument_cvs {
        let cv = (*cv)?;
        let argument = match solution.chosen_type(model, cv) {
            Some(mut chosen) => {
                if chosen.is_wildcard() && !unbounded_wildcard_allowed(Some(base_type)) {
                    return None;
                }
                if chosen.is_parameterized() {
                    chosen = chosen.decl();
                }
                let typ = match chosen.binding() {
                    Some(b) if b.is_local() => {
                        let n = rw.new_simple_name(b.name());
                        rw.new_simple_type(n)
                    }
                    Some(b) => {
                        imports.add_import_type(b, rw, &DefaultContext, TypeLocation::TypeArgument)
                    }
                    None => {
                        let n = rw.new_simple_name("void");
                        rw.new_simple_type(n)
                    }
                };
                let nested = type_argument_cvs(model, cv);
                match type_arguments(model, solution, base_type, &nested, rw, imports, depth + 1) {
                    Some(nested) => {
                        let parameterized = rw.new_node(NodeKind::ParameterizedType);
                        rw.put_child(parameterized, "type", typ);
                        rw.put_list(parameterized, "typeArguments", nested)
                    }
                    None => typ,
                }
            }
            None => {
                if unbounded_wildcard_allowed(Some(base_type)) {
                    rw.new_node(NodeKind::WildcardType)
                } else {
                    let object = imports.add_import("java.lang.Object", &DefaultContext);
                    rw.create_string_placeholder(&object, NodeKind::SimpleType)
                }
            }
        };
        out.push(argument);
    }
    Some(out)
}
