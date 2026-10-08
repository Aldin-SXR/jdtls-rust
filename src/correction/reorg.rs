//! Port of jdt.ls `ReorgCorrectionsSubProcessor` over
//! `ReorgCorrectionsBaseSubProcessor`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::{DocumentChangeOperation, DocumentChanges, RenameFile, ResourceOp, Url, WorkspaceEdit};

use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal, ProposalType};
use crate::rewrite::text_edit::{EditKind, EditTree};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::problem as p;
use crate::semantic_ast::resolve::parent_of_kind;
use crate::semantic_ast::{Ast, BindingRef, Node, NodeKind};

/// `JavaCore.latestSupportedJavaVersion()` of the bundled JDT core.
const LATEST_SUPPORTED_JAVA_VERSION: &str = "26";

/// `UnusedCodeFixCore.isUnusedImport`.
fn is_unused_import(id: i32) -> bool {
    matches!(id, p::UnusedImport | p::DuplicateImport | p::ConflictingImport | p::CannotImportPackage | p::ImportNotFound)
}

/// `UnusedCodeFixCore.getImportDeclaration`.
fn import_declaration<'a>(ast: &'a Ast, offset: usize, length: usize) -> Option<Node<'a>> {
    let selected = crate::semantic_ast::finder::NodeFinder::new(ast.root(), offset, length).covering?;
    parent_of_kind(selected, NodeKind::ImportDeclaration)
}

/// `addRemoveImportStatementProposals`.
pub fn remove_import_statement_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    if is_unused_import(problem.problem_id) {
        if let Some(import) = import_declaration(ctx.ast(), problem.offset, problem.length) {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            rw.remove(RNode::Orig(import.id));
            proposals.push(Proposal::rewrite(messages::fix("UnusedCodeFix_RemoveImport_description"), kind::QUICK_FIX, relevance::REMOVE_UNUSED_IMPORT, rw));
        }
    }
    if let Some(rw) = remove_all_unused_imports(&ctx.ast) {
        proposals.push(Proposal::rewrite(
            messages::ls_correction("ReorgCorrectionsSubProcessor_remove_all_unused_imports"),
            kind::QUICK_FIX,
            relevance::REMOVE_UNUSED_IMPORT,
            rw,
        ));
    }
}

/// `UnusedCodeFixCore.createCleanUp(.., removeUnusedImports)`.
fn remove_all_unused_imports(ast: &Arc<Ast>) -> Option<ASTRewrite> {
    let mut rw = ASTRewrite::new(ast.clone());
    let mut any = false;
    for problem in &ast.problems {
        let (offset, length) = (problem.source_start.max(0) as usize, (problem.source_end - problem.source_start + 1).max(0) as usize);
        let id = problem.id;
        if matches!(id, p::UnusedImport | p::DuplicateImport | p::ConflictingImport) {
            if let Some(import) = import_declaration(ast, offset, length) {
                rw.remove(RNode::Orig(import.id));
                any = true;
            }
        }
        if matches!(id, p::CannotImportPackage | p::ImportNotFound) {
            let Some(import) = import_declaration(ast, offset, length).filter(|i| !i.flag("onDemand")) else { continue };
            let full = import.child("name").map(|n| n.identifier()).unwrap_or_default();
            let name = full.rsplit('.').next().unwrap_or(&full).to_owned();
            let used = ast.all_nodes().any(|n| n.is(NodeKind::SimpleName) && n.identifier() == name && parent_of_kind(n, NodeKind::ImportDeclaration).is_none());
            if !used {
                rw.remove(RNode::Orig(import.id));
                any = true;
            }
        }
    }
    any.then_some(rw)
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_alphabetic() || first == '_' || first == '$') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !crate::features::scanner::is_keyword(name)
}

fn resource_operations() -> bool {
    crate::features::client_caps::resource_operations()
}

fn rename_file_edit(old: &Path, new: &Path) -> WorkspaceEdit {
    if !resource_operations() {
        return WorkspaceEdit::default();
    }
    let (Ok(old_uri), Ok(new_uri)) = (Url::from_file_path(old), Url::from_file_path(new)) else { return WorkspaceEdit::default() };
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(vec![DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
            old_uri,
            new_uri,
            options: None,
            annotation_id: None,
        }))])),
        ..Default::default()
    }
}

async fn file_exists(env: &Env<'_>, uri: &str, path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    let Ok(url) = Url::parse(uri) else { return false };
    let Ok(target) = Url::from_file_path(path) else { return false };
    env.dispatcher.context_for(Some(&url)).await.files.contains_key(target.as_str())
}

/// `addWrongTypeNameProposals`.
pub async fn wrong_type_name_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Some(path) = Url::parse(&ctx.ast.uri).ok().and_then(|u| crate::project::uri_to_path(&u)) else { return };
    let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
    let Some(covered) = problem.covered_node(ast).filter(|n| n.is(NodeKind::SimpleName)) else { return };
    let Some(parent_type) = covered.parent().filter(|p| p.kind().is_abstract_type_declaration()) else { return };

    let current_name = covered.identifier();
    let new_name = file_name.strip_suffix(".java").unwrap_or(&file_name).to_owned();

    let mut has_other_public_type_before = false;
    let mut found = false;
    for curr in ast.root().list("types") {
        if curr != parent_type {
            if curr.child("name").is_some_and(|n| n.identifier() == new_name) {
                return;
            }
            if !found && curr.modifiers() & crate::semantic_ast::modifier::PUBLIC != 0 {
                has_other_public_type_before = true;
            }
        } else {
            found = true;
        }
    }
    if is_valid_identifier(&new_name) {
        let label = messages::format(messages::ls_correction("ReorgCorrectionsSubProcessor_renametype_description"), &[&new_name]);
        let change = CorrectMainTypeName { ast: ctx.ast.clone(), old_name: current_name.clone(), new_name: new_name.clone() };
        proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::RENAME_TYPE, Change::Lazy(Box::new(change))));
    }
    if !has_other_public_type_before {
        let new_cu_name = format!("{current_name}.java");
        let new_path = path.with_file_name(&new_cu_name);
        if !file_exists(env, &ctx.ast.uri, &new_path).await && is_valid_identifier(&current_name) {
            let label = messages::format(messages::correction("ReorgCorrectionsSubProcessor_renamecu_description"), &[&new_cu_name]);
            let mut proposal = Proposal::new(label, kind::QUICK_FIX, relevance::RENAME_CU, Change::WorkspaceEdit(rename_file_edit(&path, &new_path)));
            proposal.proposal_type = ProposalType::Change;
            proposals.push(proposal);
        }
    }
}

/// `LinkedNodeFinder.findByBinding(root, typeName.resolveBinding())` for a
/// type: the declaration, constructors and references.
fn linked_type_names<'a>(root: Node<'a>, declaration: BindingRef<'_>) -> Vec<Node<'a>> {
    let key = declaration.type_declaration().unwrap_or(declaration).key().to_owned();
    let declared_key = |b: BindingRef<'_>| -> Option<String> {
        if b.is_type() {
            Some(b.type_declaration().unwrap_or(b).key().to_owned())
        } else if b.is_method() && b.is_constructor() {
            let class = b.declaring_class()?;
            Some(class.type_declaration().unwrap_or(class).key().to_owned())
        } else {
            None
        }
    };
    root.descendants().filter(|n| n.is(NodeKind::SimpleName) && n.binding().and_then(declared_key).is_some_and(|k| k == key)).collect()
}

/// `CorrectMainTypeNameProposalCore`.
struct CorrectMainTypeName {
    ast: Arc<Ast>,
    old_name: String,
    new_name: String,
}

#[tower_lsp::async_trait]
impl LazyChange for CorrectMainTypeName {
    async fn compute(&self, _env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let mut rw = ASTRewrite::new(self.ast.clone());
        let declaration = self.ast.root().list("types").into_iter().find(|t| t.child("name").is_some_and(|n| n.identifier() == self.old_name));
        if let Some(binding) = declaration.and_then(|d| d.child("name")).and_then(|n| n.binding()) {
            for same in linked_type_names(self.ast.root(), binding) {
                let name = rw.new_simple_name(&self.new_name);
                rw.replace(RNode::Orig(same.id), Some(name));
            }
        }
        Ok(vec![CuChange::rewrite(rw)])
    }
}

/// The source folder of a unit and the package of its folder (its package
/// fragment).
fn source_folder_package(env: &Env<'_>, path: &Path) -> Option<(PathBuf, String)> {
    let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
    let project = ws.project_for_path(path)?;
    let folder = project.source_folder_for(path)?;
    let dir = path.parent()?.strip_prefix(&folder.path).ok()?;
    let package: Vec<String> = dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Some((folder.path.clone(), package.join(".")))
}

/// `addWrongPackageDeclNameProposals`.
pub async fn wrong_package_decl_name_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let Ok(url) = Url::parse(&ctx.ast.uri) else { return };
    let Some(path) = crate::project::uri_to_path(&url) else { return };
    let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
    let package = ast.root().child("package");
    let declared = package.and_then(|pk| pk.child("name")).map(|n| n.identifier());
    let Some((source_root, expected)) = source_folder_package(env, &path) else { return };

    let relevance = if package.is_none() { relevance::MISSING_PACKAGE_DECLARATION } else { relevance::CORRECT_PACKAGE_DECLARATION };
    let options = env.options(&ctx.ast.uri).await;
    if let Some(proposal) = correct_package_declaration(env, ctx, problem, &expected, package, &options, relevance).await {
        proposals.push(proposal);
    }

    // move to package
    let new_pack = declared.unwrap_or_default();
    let mut new_dir: PathBuf = source_root;
    for segment in new_pack.split('.').filter(|s| !s.is_empty()) {
        new_dir.push(segment);
    }
    let new_path = new_dir.join(&file_name);
    if !file_exists(env, &ctx.ast.uri, &new_path).await {
        let label = if new_pack.is_empty() {
            messages::format(messages::ls_correction("ReorgCorrectionsSubProcessor_movecu_default_description"), &[&file_name])
        } else {
            messages::format(messages::ls_correction("ReorgCorrectionsSubProcessor_movecu_description"), &[&file_name, &new_pack])
        };
        proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::MOVE_CU_TO_PACKAGE, Change::WorkspaceEdit(rename_file_edit(&path, &new_path))));
    }
}

/// `CorrectPackageDeclarationFixCore`.
async fn correct_package_declaration(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    expected: &str,
    package: Option<Node<'_>>,
    options: &std::collections::BTreeMap<String, String>,
    relevance: i32,
) -> Option<Proposal> {
    if expected.is_empty() && package.is_some() {
        let compliance = options.get("org.eclipse.jdt.core.compiler.compliance").map(String::as_str).unwrap_or("1.8");
        if crate::project::compare_java_versions(compliance, "9") != std::cmp::Ordering::Less {
            let url = Url::parse(&ctx.ast.uri).ok()?;
            let files = env.dispatcher.context_for(Some(&url)).await.files;
            if files.keys().any(|f| f.ends_with("/module-info.java")) {
                return None;
            }
        }
    }
    let mut tree = EditTree::new();
    let edit = if expected.is_empty() && package.is_some() {
        let pk = package?;
        tree.new_edit(pk.start() as i32, pk.length() as i32, EditKind::Delete)
    } else if !expected.is_empty() && package.is_none() {
        let delimiter = if ctx.ast.text().contains("\r\n") { "\r\n" } else { "\n" };
        tree.new_edit(0, 0, EditKind::Insert(format!("package {expected};{delimiter}{delimiter}")))
    } else {
        tree.new_edit(problem.offset as i32, problem.length as i32, EditKind::Replace(expected.to_owned()))
    };
    tree.add_child(EditTree::ROOT, edit).ok()?;
    Some(Proposal::new(
        messages::correction("CorrectPackageDeclarationProposal_name"),
        kind::QUICK_FIX,
        relevance,
        Change::Cu(vec![CuChange::edits(ctx.ast.clone(), tree)]),
    ))
}

/// `getNeedHigherComplianceProposals`.
pub async fn need_higher_compliance_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, required_version: Option<&str>) {
    let (required, enable_previews) = match required_version {
        Some(v) => (v.to_owned(), false),
        None => {
            if problem.arguments.len() != 2 {
                return;
            }
            let required = problem.arguments[1].clone();
            let previews = required == LATEST_SUPPORTED_JAVA_VERSION;
            (required, previews)
        }
    };
    let jre = {
        let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
        ws.vm_version.clone().unwrap_or_else(|| "21".to_owned())
    };
    let jre = jre.split('.').next().unwrap_or("21").to_owned();
    let (Ok(required_number), Ok(jre_number)) = (required.parse::<f32>(), jre.parse::<f32>()) else { return };
    if required_number > jre_number {
        return;
    }
    let mut label = messages::format(messages::ls_correction("ReorgCorrectionsSubProcessor_change_project_compliance_description"), &[&required]);
    if enable_previews {
        label = messages::format(
            messages::ls_correction("ReorgCorrectionsSubProcessor_combine_two_quickfixes"),
            &[&label, messages::ls_correction("ReorgCorrectionsSubProcessor_enable_preview_features")],
        );
    }
    let change = ChangeToRequiredCompilerCompliance { uri: ctx.ast.uri.clone(), version: required, enable_previews };
    let mut proposal = Proposal::new(label, kind::QUICK_FIX, relevance::CHANGE_PROJECT_COMPLIANCE, Change::Lazy(Box::new(change)));
    proposal.proposal_type = ProposalType::ChangeCompliance;
    proposals.push(proposal);
}

/// `ChangeToRequiredCompilerCompliance.createChange`: updates the project
/// settings and yields no edit.
struct ChangeToRequiredCompilerCompliance {
    uri: String,
    version: String,
    enable_previews: bool,
}

#[tower_lsp::async_trait]
impl LazyChange for ChangeToRequiredCompilerCompliance {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let mut options = vec![
            ("org.eclipse.jdt.core.compiler.compliance", self.version.as_str()),
            ("org.eclipse.jdt.core.compiler.source", self.version.as_str()),
            ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", self.version.as_str()),
            ("org.eclipse.jdt.core.compiler.problem.assertIdentifier", "error"),
            ("org.eclipse.jdt.core.compiler.problem.enumIdentifier", "error"),
            ("org.eclipse.jdt.core.compiler.release", "enabled"),
        ];
        if self.enable_previews {
            options.push(("org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures", "enabled"));
            options.push(("org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures", "ignore"));
        } else {
            options.push(("org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures", "disabled"));
            options.push(("org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures", "warning"));
        }
        let url = Url::parse(&self.uri)?;
        let root = {
            let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
            ws.project_for_uri(&url).map(|p| p.location.clone())
        };
        if let Some(root) = root {
            let prefs = root.join(".settings").join("org.eclipse.jdt.core.prefs");
            let mut specific = crate::project::prefs::read_properties(&prefs).unwrap_or_default();
            for (k, v) in options {
                specific.insert(k.to_owned(), v.to_owned());
            }
            specific.insert("eclipse.preferences.version".into(), "1".into());
            let _ = crate::project::prefs::write_properties(&prefs, &specific);
        }
        Ok(Vec::new())
    }
}
