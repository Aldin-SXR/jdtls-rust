//! Port of jdt.ls `SourceAssistProcessor` (the parts ported so far).

use super::edit::Env;
use super::handler::{Entry, Request};
use super::{kind, messages, Change, CuChange, LazyChange, Proposal};
use crate::analysis::semantic::BridgeResponse;
use crate::rewrite::text_edit::EditTree;
use crate::semantic_ast::NodeKind;
use tower_lsp::lsp_types::{CodeActionOrCommand, Url};

/// `SourceAssistProcessor.getSourceActionCommands`: entries plus the
/// proposals that resolve them (indices start at `next_proposal`).
pub async fn source_actions(
    env: &Env<'_>,
    req: &Request<'_>,
    next_proposal: usize,
) -> Vec<(Entry, Option<Proposal>)> {
    let mut kinds = Vec::new();
    let mut node = req.context.covering_node();
    while let Some(current) = node {
        if current.is(NodeKind::ImportDeclaration) {
            kinds.push(kind::QUICK_ASSIST);
            break;
        }
        node = current.parent();
    }
    kinds.push(kind::SOURCE_ORGANIZE_IMPORTS);
    let resolve = crate::features::client_caps::resolve_code_action();
    let mut out = Vec::new();
    for kind in kinds {
        if req.params.context.only.as_ref().is_some_and(|only| {
            !only.is_empty() && !only.iter().any(|k| kind.starts_with(k.as_str()))
        }) {
            continue;
        }
        let mut proposal = Proposal::new(
            messages::ls_correction("ReorgCorrectionsSubProcessor_organizeimports_description"),
            kind,
            0,
            Change::Lazy(Box::new(OrganizeImports {
                uri: req.uri.clone(),
            })),
        );
        if let Some(mut entry) = super::handler::code_action_from_proposal(
            env,
            &req.uri,
            &mut proposal,
            &req.params.context.diagnostics,
            resolve,
            next_proposal + out.len(),
        )
        .await
        {
            if let CodeActionOrCommand::CodeAction(action) = &mut entry.action {
                action.diagnostics = Some(if resolve {
                    Vec::new()
                } else {
                    req.params.context.diagnostics.clone()
                });
            }
            if let Some(data) = entry.data.as_mut() {
                data.priority = super::handler::priority::ORGANIZE_IMPORTS;
            }
            out.push((entry, resolve.then_some(proposal)));
        }
    }
    out
}

struct OrganizeImports {
    uri: Url,
}

#[tower_lsp::async_trait]
impl LazyChange for OrganizeImports {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        // Unused imports must be removable even when their compiler warning
        // is disabled or absent from the request's diagnostic context.
        let mut context = env.dispatcher.context_for(Some(&self.uri)).await;
        context.options.insert(
            "org.eclipse.jdt.core.compiler.problem.unusedImport".into(),
            "warning".into(),
        );
        let ast =
            crate::semantic_ast::fetch_with(env.dispatcher, self.uri.as_str(), context).await?;
        let options = env.options(self.uri.as_str()).await;
        let mut imports = crate::rewrite::import_rewrite::ImportRewrite::create_for_corrections(
            ast.clone(),
            &options,
        );
        for import in ast.root().list("imports") {
            let name = import
                .child("name")
                .map(|n| n.identifier())
                .unwrap_or_default();
            let compiler_unused = ast.problems.iter().any(|p| {
                p.id == crate::semantic_ast::problem::UnusedImport
                    && p.source_start >= import.start() as i32
                    && p.source_end < import.end() as i32
            });
            // ECJ can suppress unused-import warnings when another type is
            // unresolved. For a resolved single-type import, inspect its
            // references directly so removals and missing imports still combine.
            let unreferenced_type = !import.flag("static")
                && !import.flag("onDemand")
                && import
                    .binding()
                    .is_some_and(|binding| binding.is_type() && !binding.is_recovered())
                && !ast.all_nodes().any(|node| {
                    if !node.is(NodeKind::SimpleName) {
                        return false;
                    }
                    let mut parent = node.parent();
                    while let Some(p) = parent {
                        if p.is(NodeKind::ImportDeclaration) || p.is(NodeKind::PackageDeclaration) {
                            return false;
                        }
                        parent = p.parent();
                    }
                    if node.parent().is_some_and(|p| {
                        p.is(NodeKind::QualifiedName)
                            && p.child("name").is_some_and(|n| n.id == node.id)
                    }) {
                        return false;
                    }
                    node.binding().is_some_and(|b| {
                        b.is_type()
                            && (b.erasure().unwrap_or(b).qualified_name() == name
                                || b.is_recovered()
                                    && node.identifier() == name.rsplit('.').next().unwrap_or(""))
                    })
                });
            if compiler_unused || unreferenced_type {
                let qualified = if import.flag("onDemand") {
                    format!("{name}.*")
                } else {
                    name
                };
                if import.flag("static") {
                    imports.remove_static_import(&qualified);
                } else {
                    imports.remove_import(&qualified);
                }
            }
        }
        // Type-search import selection still comes from the bridge. Rust
        // merges its candidates with removals in one ImportRewrite, preserving
        // the existing whitespace and avoiding overlapping import-block edits.
        if let BridgeResponse::TextEdits { edits, .. } =
            env.dispatcher.organize_imports(&self.uri).await?
        {
            for edit in edits {
                for line in edit.new_text.lines() {
                    if let Some(name) = line
                        .strip_prefix("import ")
                        .and_then(|s| s.strip_suffix(';'))
                    {
                        imports.add_import(name, &crate::rewrite::import_rewrite::DefaultContext);
                    }
                }
            }
        }
        Ok(vec![
            CuChange::edits(ast, EditTree::new()).with_imports(imports)
        ])
    }
}
