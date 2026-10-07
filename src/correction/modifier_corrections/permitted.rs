//! `LocalCorrectionsBaseSubProcessor.getPermittedTypesProposal` /
//! `createPermittedTypeCasesProposal`: cases for the permitted subtypes of a
//! sealed type in an empty switch.

use std::sync::Arc;

use super::visibility::Units;
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Ast, BindingRef, Node, NodeKind};

/// The type qualified name (`IType.getTypeQualifiedName('.')`) and package.
fn type_qualified_name(b: BindingRef<'_>) -> (String, String) {
    let decl = b.type_declaration().unwrap_or(b);
    let qualified = decl.qualified_name();
    let package = decl.package_name().unwrap_or("").to_owned();
    let tqn = if package.is_empty() { qualified.to_owned() } else { qualified.strip_prefix(&format!("{package}.")).unwrap_or(qualified).to_owned() };
    (package, tqn)
}

/// The declaration of a source type, in this unit or in its own unit.
async fn declaration_ast(env: &Env<'_>, units: &Units, ast: &Arc<Ast>, binding: BindingRef<'_>) -> Option<Arc<Ast>> {
    let decl = binding.type_declaration().unwrap_or(binding);
    match units.find(ast, decl)? {
        None => Some(ast.clone()),
        Some(uri) => crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&uri).ok()?).await.ok(),
    }
}

/// `"(" + componentType + " " + componentName + ", ..." + ")"` of a record
/// declaration (`IType.getRecordComponents()`).
fn record_components(decl: Node<'_>) -> String {
    let mut s = String::from("(");
    let mut separator = "";
    for c in decl.list("recordComponents") {
        s.push_str(separator);
        s.push_str(&c.child("type").map(|t| t.source_text()).unwrap_or_default());
        separator = ", ";
        s.push(' ');
        s.push_str(&c.child("name").map(|n| n.identifier()).unwrap_or_default());
    }
    s.push(')');
    s
}

/// `computeReservedIdentifiers(node, cu)`: the parameters of the enclosing
/// method and the variables declared in its body.
fn reserved_identifiers(node: Node<'_>) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(decl) = node.ancestors().find(|a| a.is(NodeKind::MethodDeclaration)) {
        for p in decl.list("parameters") {
            names.push(p.child("name").map(|n| n.identifier()).unwrap_or_default());
        }
        if let Some(body) = decl.child("body") {
            for d in body.descendants() {
                if matches!(d.kind(), NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration) {
                    names.push(d.child("name").map(|n| n.identifier()).unwrap_or_default());
                }
            }
        }
    }
    names
}

fn pattern_variable(permitted: &str, excluded: &mut Vec<String>) -> String {
    let pattern: String = permitted.chars().next().map(|c| c.to_lowercase().collect()).unwrap_or_default();
    let mut name = pattern.clone();
    let mut count = 1;
    while excluded.contains(&name) {
        count += 1;
        name = format!("{pattern}{count}");
    }
    excluded.push(name.clone());
    name
}

/// `getPermittedTypesProposal(context, problem, proposals)`.
pub async fn permitted_types(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(selected) = problem.covering_node(ast).filter(|n| n.kind().is_expression()) else { return };
    let Some(parent) = selected.parent() else { return };
    if !selected.location_is("expression") || !matches!(parent.kind(), NodeKind::SwitchStatement | NodeKind::SwitchExpression) {
        return;
    }
    if !parent.list("statements").is_empty() {
        return;
    }
    let Some(type_binding) = selected.type_binding() else { return };
    let units = Units::load(env, &ast.uri).await;
    let Some(sealed_ast) = declaration_ast(env, &units, &ctx.ast, type_binding).await else { return };
    let sealed_key = type_binding.type_declaration().unwrap_or(type_binding).key().to_owned();
    let Some(sealed_decl) = sealed_ast.binding_by_key(&sealed_key).and_then(|b| b.declaring_node()) else { return };
    if sealed_decl.list("permitsTypes").is_empty() {
        return;
    }

    // createPermittedTypeCasesProposal
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut case_code = "{}";
    if parent.is(NodeKind::SwitchExpression) {
        let sw_type = match parent.parent() {
            Some(p) if p.is(NodeKind::VariableDeclarationFragment) => p.binding().and_then(|b| b.var_type()),
            Some(p) if p.is(NodeKind::ReturnStatement) => p
                .ancestors()
                .find(|a| a.is(NodeKind::MethodDeclaration))
                .and_then(|m| m.child("returnType2"))
                .and_then(|t| t.binding()),
            _ => None,
        };
        let Some(sw_type) = sw_type else { return };
        case_code = if sw_type.is_primitive() {
            if sw_type.name() == "boolean" {
                "false;"
            } else {
                "0;"
            }
        } else {
            "null"
        };
    }
    let (_, sealed_tqn) = type_qualified_name(type_binding);
    let sealed_simple = type_binding.type_declaration().unwrap_or(type_binding).name().to_owned();
    let pkg_name = ast.root().child("package").and_then(|p| p.child("name")).map(|n| n.source_text()).unwrap_or_default();
    let mut excluded = reserved_identifiers(parent);
    let options = env.options(&ast.uri).await;
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);

    for permitted in sealed_decl.list("permitsTypes") {
        let mut permitted_name = permitted.source_text();
        let mut need_import = false;
        let mut import_name = String::new();
        if let Some(resolved) = permitted.binding().filter(|b| !b.is_recovered()) {
            let (package, tqn) = type_qualified_name(resolved);
            if !package.is_empty() {
                import_name = format!("{package}.{tqn}");
                if package != pkg_name {
                    need_import = true;
                }
            } else {
                import_name = tqn.clone();
            }
            if permitted_name.starts_with(&sealed_tqn) && permitted_name.len() > sealed_tqn.len() {
                need_import = false;
                let name = permitted_name[sealed_tqn.len() + 1..].to_owned();
                let inner = sealed_decl
                    .list("bodyDeclarations")
                    .into_iter()
                    .find(|d| d.kind().is_abstract_type_declaration() && d.child("name").is_some_and(|n| n.identifier() == name));
                if let Some(inner) = inner {
                    permitted_name = format!("{sealed_simple}.{name}");
                    if inner.is(NodeKind::RecordDeclaration) {
                        permitted_name.push_str(&record_components(inner));
                    } else {
                        let var = pattern_variable(&permitted_name, &mut excluded);
                        permitted_name = format!("{permitted_name} {var}");
                    }
                }
            } else {
                // SearchEngine: type declarations named importName in the project.
                permitted_name = tqn.clone();
                let record = if resolved.is_record() {
                    declaration_ast(env, &units, &ctx.ast, resolved).await.and_then(|a| {
                        a.binding_by_key(resolved.type_declaration().unwrap_or(resolved).key())
                            .and_then(|b| b.declaring_node())
                            .map(record_components)
                    })
                } else {
                    None
                };
                match record {
                    Some(components) => permitted_name.push_str(&components),
                    None => {
                        let var = pattern_variable(&permitted_name, &mut excluded);
                        permitted_name = format!("{permitted_name} {var}");
                    }
                }
            }
        }
        let case = rw.create_string_placeholder(&format!("case {permitted_name} -> {case_code}"), NodeKind::SwitchCase);
        rw.list_insert_last(RNode::Orig(parent.id), "statements", case);
        if need_import {
            imports.add_import(&import_name, &DefaultContext);
        }
    }
    let null_case = rw.create_string_placeholder(&format!("case null -> {case_code}"), NodeKind::SwitchCase);
    rw.list_insert_last(RNode::Orig(parent.id), "statements", null_case);
    let default_case = rw.create_string_placeholder(&format!("default -> {case_code}"), NodeKind::SwitchCase);
    rw.list_insert_last(RNode::Orig(parent.id), "statements", default_case);
    let label = messages::correction("LocalCorrectionsSubProcessor_add_permitted_types_description").to_owned();
    let change = Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]);
    proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::ADD_PERMITTED_TYPES, change));
}
