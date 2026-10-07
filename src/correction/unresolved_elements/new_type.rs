//! The type half of `UnresolvedElementsBaseSubProcessor`:
//! `collectNewTypeProposals` (jdt.ls `NewCUProposal` and
//! `AddTypeParameterProposalCore`) and
//! `collectAmbiguosTypeReferenceProposals`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    CreateFile, CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp,
    TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

use super::variables::type_kinds as tk;
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal, ProposalType};
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::find_parent_body_declaration;
use crate::semantic_ast::{modifier, BindingKind, Node, NodeKind};

/// `NewCUProposal.K_*`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    Class,
    Interface,
    Enum,
    Annotation,
    Record,
}

/// `ASTNodes.getSimpleNameIdentifier(name)`.
fn simple_name_identifier(node: Node<'_>) -> String {
    if node.is(NodeKind::QualifiedName) {
        node.child("name").map(|n| n.identifier()).unwrap_or_default()
    } else {
        node.identifier()
    }
}

/// `Name.getFullyQualifiedName()` / `ASTResolving.getFullName(name)`.
fn full_name(node: Node<'_>) -> String {
    if node.is(NodeKind::QualifiedName) {
        let q = node.child("qualifier").map(full_name).unwrap_or_default();
        format!("{q}.{}", simple_name_identifier(node))
    } else {
        node.identifier()
    }
}

/// `isLikelyTypeName(name)`.
pub(super) fn is_likely_type_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// `isLikelyPackageName(name)`.
fn is_likely_package_name(name: &str) -> bool {
    name.split('.').all(|segment| !segment.chars().next().is_some_and(char::is_uppercase))
}

/// `isLikelyTypeParameterName(name)`.
fn is_likely_type_parameter_name(name: &str) -> bool {
    let mut c = name.chars();
    matches!((c.next(), c.next()), (Some(ch), None) if ch.is_uppercase())
}

/// `isLikelyMethodTypeParameterName(name)`.
fn is_likely_method_type_parameter_name(name: &str) -> bool {
    matches!(name, "S" | "T" | "U")
}

/// The source folder holding the unit (`JavaModelUtil.getPackageFragmentRoot(cu)`)
/// and the unit's package (`cu.getParent()`), when it is a source package.
fn source_location(env: &Env<'_>, uri: &str) -> Option<(PathBuf, String)> {
    let url = Url::parse(uri).ok()?;
    let path = crate::project::uri_to_path(&url)?;
    let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let folder = ws.project_for_path(&path).and_then(|p| p.source_folder_for(&path))?.path.clone();
    let dir = path.parent()?;
    let rel = dir.strip_prefix(&folder).ok()?;
    let package = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join(".");
    Some((folder, package))
}

fn package_dir(folder: &Path, package: &str) -> PathBuf {
    let mut dir = folder.to_path_buf();
    for segment in package.split('.').filter(|s| !s.is_empty()) {
        dir.push(segment);
    }
    dir
}

/// `canUseRecord(project, refNode)`.
fn can_use_record(options: &std::collections::BTreeMap<String, String>, ref_node: Node<'_>) -> bool {
    let compliance = options.get("org.eclipse.jdt.core.compiler.compliance").map(String::as_str).unwrap_or("1.8");
    if crate::features::completion::version_less_than(compliance, "16") {
        return false;
    }
    let typ = ref_node.ancestors().find(|n| n.kind().is_type());
    let type_decl = ref_node.ancestors().find(|n| n.is(NodeKind::TypeDeclaration));
    if let (Some(t), Some(d)) = (typ, type_decl) {
        if t.location_is("superclassType") || (t.location_is("permittedTypes") && !d.flag("interface")) {
            return false;
        }
    }
    true
}

/// `collectNewTypeProposals(cu, refNode, kind, relevance, proposals)`.
pub async fn new_type_proposals(env: &Env<'_>, ctx: &Context, ref_node: Node<'_>, kind: i32, relevance: i32, proposals: &mut Vec<Proposal>) {
    new_type_proposals_interactive(env, ctx, ref_node, kind, relevance, proposals).await;
    new_type_proposals_params(ctx, ref_node, kind, relevance, proposals);
}

/// `addNewTypeProposalsInteractive`.
async fn new_type_proposals_interactive(env: &Env<'_>, ctx: &Context, ref_node: Node<'_>, kind: i32, relevance: i32, proposals: &mut Vec<Proposal>) {
    let Some((folder, cu_package)) = source_location(env, &ctx.ast.uri) else { return };
    let options = env.options(&ctx.ast.uri).await;
    let mut node = Some(ref_node);
    while let Some(n) = node {
        let type_name = simple_name_identifier(n);
        let mut qualifier = None;
        if is_likely_type_name(&type_name) || n == ref_node {
            let mut enclosing_package: Option<String> = None;
            if n.is(NodeKind::SimpleName) {
                enclosing_package = Some(cu_package.clone());
            } else if let Some(qualifier_name) = n.child("qualifier") {
                let binding = qualifier_name.binding().filter(|b| !b.is_recovered());
                match binding {
                    // A new member type (`enclosingType`): not ported.
                    Some(b) if b.kind() == BindingKind::Type => {}
                    Some(b) if b.kind() == BindingKind::Package => {
                        qualifier = Some(qualifier_name);
                        enclosing_package = Some(b.name().to_owned());
                    }
                    _ => {
                        qualifier = Some(qualifier_name);
                        enclosing_package = Some(full_name(qualifier_name));
                    }
                }
            }
            if let Some(pack) = enclosing_package {
                let rel = if is_likely_package_name(&pack) { relevance + 3 } else { relevance };
                let dir = package_dir(&folder, &pack);
                if !dir.join(format!("{type_name}.java")).exists() {
                    inner_loop(ctx, &options, n, &dir, &pack, rel, kind, ref_node, proposals);
                }
            }
        }
        node = qualifier;
    }
}

/// jdt.ls `UnresolvedElementsSubProcessor.addNewTypeProposalsInteractiveInnerLoop`.
#[allow(clippy::too_many_arguments)]
fn inner_loop(
    ctx: &Context,
    options: &std::collections::BTreeMap<String, String>,
    node: Node<'_>,
    dir: &Path,
    pack: &str,
    rel: i32,
    kind: i32,
    ref_node: Node<'_>,
    proposals: &mut Vec<Proposal>,
) {
    let mut kinds = Vec::new();
    if kind & tk::CLASSES != 0 {
        kinds.push(TypeKind::Class);
        if can_use_record(options, ref_node) {
            kinds.push(TypeKind::Record);
        }
    }
    if kind & tk::INTERFACES != 0 {
        kinds.push(TypeKind::Interface);
    }
    if kind & tk::ENUMS != 0 {
        kinds.push(TypeKind::Enum);
    }
    if kind & tk::ANNOTATIONS != 0 {
        kinds.push(TypeKind::Annotation);
    }
    for k in kinds {
        if let Some(p) = new_cu_proposal(ctx, node, k, dir, pack, rel + 3) {
            proposals.push(p);
        }
    }
}

/// `NewCUProposal.isParameterizedType(typeKind, node)`.
fn is_parameterized_type(kind: TypeKind, node: Node<'_>) -> bool {
    matches!(kind, TypeKind::Class | TypeKind::Interface) && node.parent().is_some_and(|p| p.location_is("type") && p.parent().is_some_and(|g| g.is(NodeKind::ParameterizedType)))
}

/// `NewCUProposal.getTypeName(typeKind, node)`.
fn type_name_with_parameters(kind: TypeKind, node: Node<'_>) -> String {
    let name = simple_name_identifier(node);
    if !is_parameterized_type(kind, node) {
        return name;
    }
    let base = if name.starts_with('T') { "S" } else { "T" };
    let n = node.parent().and_then(|p| p.parent()).map(|p| p.list("typeArguments").len()).unwrap_or(0);
    let args: Vec<String> = if n == 1 { vec![base.to_owned()] } else { (1..=n).map(|i| format!("{base}{i}")).collect() };
    format!("{name}<{}>", args.join(", "))
}

/// `NewCUProposal` for a type in a source package (`createChange` with a
/// `CreateFileChange` and the new unit's content).
fn new_cu_proposal(ctx: &Context, node: Node<'_>, kind: TypeKind, dir: &Path, pack: &str, relevance: i32) -> Option<Proposal> {
    let type_name = type_name_with_parameters(kind, node);
    // setDisplayName: the qualifier written in the reference.
    let container = if node.is(NodeKind::QualifiedName) { node.child("qualifier").map(full_name).unwrap_or_default() } else { String::new() };
    let key = match kind {
        TypeKind::Class => "createclass",
        TypeKind::Interface => "createinterface",
        TypeKind::Enum => "createenum",
        TypeKind::Annotation => "createannotation",
        TypeKind::Record => "createrecord",
    };
    let label = if container.is_empty() {
        messages::format(messages::ls_correction(&format!("NewCUCompletionUsingWizardProposal_{key}_description")), &[&type_name])
    } else {
        messages::format(messages::ls_correction(&format!("NewCUCompletionUsingWizardProposal_{key}_inpackage_description")), &[&type_name, &container])
    };
    let simple = simple_name_identifier(node);
    let path = dir.join(format!("{simple}.java"));
    let uri = Url::from_file_path(&path).ok()?;
    let d = "\n";
    let stub = type_stub(ctx, kind, &type_name, d);
    let content = cu_content(pack, &simple, &stub, d);
    let insert = TextEdit { range: Range::new(Position::new(0, 0), Position::new(0, 0)), new_text: content };
    let mut we = WorkspaceEdit::default();
    if crate::features::client_caps::resource_operations() {
        we.document_changes = Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: uri.clone(),
                options: Some(CreateFileOptions { overwrite: Some(false), ignore_if_exists: Some(true) }),
                annotation_id: None,
            })),
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                edits: vec![OneOf::Left(insert)],
            }),
        ]));
    } else {
        we.changes = Some([(uri, vec![insert])].into_iter().collect());
    }
    let mut p = Proposal::new(label, kind::QUICK_FIX, relevance, Change::WorkspaceEdit(we));
    p.proposal_type = ProposalType::NewElement;
    Some(p)
}

/// `NewCUProposal.constructTypeStub(parentCU, name, AccPublic, lineDelimiter)`.
fn type_stub(ctx: &Context, kind: TypeKind, name: &str, d: &str) -> String {
    let mut buf = String::from("public ");
    // fCompilationUnit.findPrimaryType()
    let unit_name = ctx.ast.uri.rsplit('/').next().unwrap_or("").trim_end_matches(".java").to_owned();
    let primary = ctx.root().list("types").into_iter().find(|t| t.child("name").is_some_and(|n| n.identifier() == unit_name));
    let permitted: Vec<String> = primary.map(|t| t.list("permittedTypes").iter().map(|p| p.source_text()).collect()).unwrap_or_default();
    let is_interface = primary.is_some_and(|t| t.is(NodeKind::TypeDeclaration) && t.flag("interface"));
    let is_permitted = permitted.iter().any(|p| p == name);
    if is_permitted {
        buf.push_str(if kind == TypeKind::Interface { "non-sealed " } else { "final " });
    }
    let (typ, super_type) = match kind {
        TypeKind::Class => ("class ", if is_interface { "implements " } else { "extends " }),
        TypeKind::Interface => ("interface ", "extends "),
        TypeKind::Enum => ("enum ", ""),
        TypeKind::Annotation => ("@interface ", ""),
        TypeKind::Record => ("record ", if is_interface { "implements " } else { "extends " }),
    };
    buf.push_str(typ);
    buf.push_str(name);
    if kind == TypeKind::Record {
        buf.push_str("()");
    }
    if is_permitted {
        buf.push(' ');
        buf.push_str(super_type);
        buf.push_str(&unit_name);
    }
    buf.push_str(" {");
    buf.push_str(d);
    // CodeGeneration.getTypeBody: the body templates are empty.
    buf.push_str(d);
    buf.push('}');
    buf.push_str(d);
    buf
}

/// A jdt.ls code template from the `java.templates.*` preference (`fileHeader`
/// / `typeComment`), evaluated for the new unit; empty `${tags}` lines are
/// dropped (`StubUtility.getTypeComment`).
fn code_template(key: &str, pack: &str, type_name: &str) -> String {
    let Some(serde_json::Value::Array(lines)) = crate::features::preferences::get(key) else { return String::new() };
    let file_name = format!("{type_name}.java");
    let mut evaluated = Vec::new();
    for line in lines.iter().filter_map(serde_json::Value::as_str) {
        if line.contains("${tags}") && line.replace("${tags}", "").trim().trim_matches('*').trim().is_empty() {
            continue;
        }
        evaluated.push(line.replace("${tags}", "").replace("${type_name}", type_name).replace("${file_name}", &file_name).replace("${package_name}", pack));
    }
    let content = evaluated.join("\n");
    if content.trim().is_empty() {
        String::new()
    } else {
        content
    }
}

/// `NewCUProposal.constructCUContent` with `CodeGeneration.getCompilationUnitContent`
/// (the `newtype` template `${filecomment}${package_declaration}\n\n${typecomment}\n${type_declaration}`,
/// whose empty full-line variables remove their line).
fn cu_content(pack: &str, type_name: &str, type_content: &str, d: &str) -> String {
    let mut file_comment = code_template("java.templates.fileHeader", pack, type_name);
    if !file_comment.is_empty() {
        file_comment.push_str(d);
    }
    let type_comment = code_template("java.templates.typeComment", pack, type_name);
    let pack_decl = if pack.is_empty() { String::new() } else { format!("package {pack};") };
    let mut lines: Vec<String> = Vec::new();
    let first = format!("{file_comment}{pack_decl}");
    if !first.trim().is_empty() {
        lines.push(first);
    }
    lines.push(String::new());
    if !type_comment.is_empty() {
        lines.push(type_comment);
    }
    let mut content = lines.join(d);
    content.push_str(d);
    content.push_str(type_content);
    while let Some(rest) = content.strip_prefix(d) {
        content = rest.to_owned();
    }
    content
}

/// `addNewTypeProposalsParams(cu, refNode, kind, relevance, proposals)`.
fn new_type_proposals_params(ctx: &Context, ref_node: Node<'_>, kind: i32, relevance: i32, proposals: &mut Vec<Proposal>) {
    if !ref_node.is(NodeKind::SimpleName) || kind & tk::VARIABLES == 0 {
        return;
    }
    let name = ref_node.identifier();
    let mut declaration = find_parent_body_declaration(ref_node);
    let base_rel = if is_likely_type_parameter_name(&name) { relevance + 8 } else { relevance };
    while let Some(decl) = declaration {
        let mut rel = base_rel;
        let target = match decl.kind() {
            NodeKind::MethodDeclaration => {
                if is_likely_method_type_parameter_name(&name) {
                    rel += 2;
                }
                decl.binding().map(|b| super::types::method_label(b))
            }
            NodeKind::TypeDeclaration => {
                rel += 1;
                decl.binding().map(|b| super::types::type_label(b))
            }
            _ => None,
        };
        if let Some(target) = target {
            proposals.push(add_type_parameter_proposal(ctx, decl, &name, &target, rel));
        }
        declaration = if decl.modifiers() & modifier::STATIC == 0 { decl.parent().and_then(find_parent_body_declaration) } else { None };
    }
}

/// `AddTypeParameterProposalCore` (the declaration is in this unit).
fn add_type_parameter_proposal(ctx: &Context, decl: Node<'_>, name: &str, target: &str, relevance: i32) -> Proposal {
    let key = if decl.is(NodeKind::MethodDeclaration) { "AddTypeParameterProposal_method_label" } else { "AddTypeParameterProposal_type_label" };
    let label = messages::format(messages::correction(key), &[name, target]);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let param = rw.new_node(NodeKind::TypeParameter);
    let n = rw.new_simple_name(name);
    rw.put_child(param, "name", n);
    rw.list_insert_last(RNode::Orig(decl.id), "typeParameters", param);
    if let Some(doc) = decl.child("javadoc") {
        // JavadocTagsSubProcessorCore.getPreviousTypeParamNames(otherTypeParams, null)
        let previous: HashSet<String> = decl.list("typeParameters").iter().filter_map(|t| t.child("name")).map(|n| format!("<{}>", n.identifier())).collect();
        let tag = rw.new_node(NodeKind::TagElement);
        rw.put_simple(tag, "tagName", "@param");
        let text = rw.new_node(NodeKind::TextElement);
        rw.put_simple(text, "text", &format!("<{name}>"));
        rw.put_list(tag, "fragments", vec![text]);
        super::proposals::insert_tag(&mut rw, doc, tag, "@param", &previous);
    }
    Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw)
}

/// `collectAmbiguosTypeReferenceProposals(context, problem, proposals)`:
/// `cu.codeSelect` of an ambiguous type answers the candidate types of the
/// on-demand imports.
pub async fn ambiguous_type_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let ast = ctx.ast();
    let selected = ast.substring(problem.offset, problem.offset + problem.length);
    let simple = selected.rsplit('.').next().unwrap_or("").trim().to_owned();
    if simple.is_empty() {
        return;
    }
    let Ok(uri) = Url::parse(&ast.uri) else { return };
    let candidates = crate::features::organize_imports::operation::types_named(env.dispatcher, &uri, &simple).await;
    let filter = crate::features::completion::requestor::TypeFilter::new(&crate::features::completion::prefs::Prefs::load().filtered_types, &[]);
    let mut seen = HashSet::new();
    for import in ctx.root().list("imports") {
        if !import.flag("onDemand") || import.flag("static") {
            continue;
        }
        let Some(container) = import.child("name").map(full_name) else { continue };
        let qualified = format!("{container}.{simple}");
        if !candidates.contains(&qualified) || filter.is_filtered(&qualified) || !seen.insert(qualified.clone()) {
            continue;
        }
        let label = messages::format(messages::correction("UnresolvedElementsSubProcessor_importexplicit_description"), &[&qualified]);
        let options = env.options(&ast.uri).await;
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        imports.add_import(&qualified, &DefaultContext);
        let change = CuChange { ast: ctx.ast.clone(), rewrite: None, imports: Some(imports), edits: None };
        proposals.push(Proposal::new(label, kind::QUICK_FIX, relevance::IMPORT_EXPLICIT, Change::Cu(vec![change])));
    }
}
