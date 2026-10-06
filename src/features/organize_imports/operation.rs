//! Rust port of JDT's OrganizeImportsOperation reference and import selection.
//! JDT supplies the resolved DOM and binary type names; reference collection,
//! search scope, ambiguity selection and ImportRewrite run in Rust.

use crate::analysis::dispatcher::{Dispatcher, RequestContext};
use crate::features::{
    completion::{
        proposal::{kind, EngineResult},
        requestor::TypeFilter,
    },
    preferences,
};
use crate::index::type_index::{self, TypeEntry, ACC_ANNOTATION, ACC_ENUM, ACC_INTERFACE};
use crate::rewrite::import_rewrite::{
    DefaultContext, ImportRewrite, ImportRewriteContext, TypeLookup, RES_NAME_UNKNOWN,
};
use crate::semantic_ast::{self, modifier, BindingRef, Node, NodeKind};
use std::collections::{BTreeMap, HashSet};
use tower_lsp::lsp_types::{Url, WorkspaceEdit};

pub(crate) fn import_names(text: &str) -> HashSet<String> {
    crate::features::java_model::parse(text)
        .imports
        .iter()
        .map(|&(s, e)| {
            let declaration = &text[s..e];
            crate::features::scanner::scan(declaration)
                .into_iter()
                .filter(|t| {
                    !t.is_comment() && !matches!(t.text(declaration), "import" | "static" | ";")
                })
                .map(|t| t.text(declaration))
                .collect::<String>()
        })
        .collect()
}

struct SearchTypes(Vec<TypeEntry>);
impl TypeLookup for SearchTypes {
    fn type_exists(&self, container: &str, name: &str) -> bool {
        self.0
            .iter()
            .any(|t| t.name == name && t.container() == container)
    }
}

/// ImportNotFound declarations are retained only when an unresolved reference
/// needs them. An exact simple-name import takes precedence over on-demand ones.
struct UnresolvableImports {
    types: BTreeMap<String, Vec<String>>,
    statics: BTreeMap<String, Vec<String>>,
}
impl UnresolvableImports {
    fn new(ast: &semantic_ast::Ast) -> Self {
        let mut imports = Self {
            types: BTreeMap::new(),
            statics: BTreeMap::new(),
        };
        for import in ast.root().list("imports") {
            if !ast.problems.iter().any(|problem| {
                problem.id == semantic_ast::problem::ImportNotFound
                    && problem.source_start >= import.start() as i32
                    && problem.source_end < import.end() as i32
            }) {
                continue;
            }
            let Some(name) = import.child("name") else {
                continue;
            };
            let qualified = if import.flag("onDemand") {
                format!("{}.*", name.identifier())
            } else {
                name.identifier()
            };
            let simple = qualified.rsplit('.').next().unwrap_or("").to_owned();
            let map = if import.flag("static") {
                &mut imports.statics
            } else {
                &mut imports.types
            };
            map.entry(simple).or_default().push(qualified);
        }
        imports
    }
    fn matching(&self, name: &str, is_static: bool) -> &[String] {
        let map = if is_static {
            &self.statics
        } else {
            &self.types
        };
        map.get(name)
            .or_else(|| map.get("*"))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

struct UnresolvableContext;
impl ImportRewriteContext for UnresolvableContext {
    fn find_in_context(&self, _: &ImportRewrite, _: &str, _: &str, _: i32) -> i32 {
        RES_NAME_UNKNOWN
    }
}

pub(crate) async fn missing_imports(
    d: &Dispatcher,
    uri: &Url,
    ctx: RequestContext,
    copied: &HashSet<String>,
) -> anyhow::Result<Option<WorkspaceEdit>> {
    let (options, _) = d.options_for(Some(uri)).await;
    let Some(change) = rewrite_imports(d, uri, ctx, copied, true, &options).await? else {
        return Ok(None);
    };
    let edits = crate::correction::edit::tree_to_text_edits(
        &change.ast.source,
        change.edits.as_ref().expect("import edits"),
    );
    Ok(Some(WorkspaceEdit {
        changes: Some([(uri.clone(), edits)].into()),
        ..Default::default()
    }))
}

/// Organize without restoring imports (the command and source action). Paste
/// uses the same reference collection with restoreExistingImports=true.
pub(crate) async fn organize(
    d: &Dispatcher,
    uri: &Url,
    options: &BTreeMap<String, String>,
) -> anyhow::Result<Option<crate::correction::CuChange>> {
    let ctx = d.context_for(Some(uri)).await;
    rewrite_imports(d, uri, ctx, &HashSet::new(), false, options).await
}

async fn rewrite_imports(
    d: &Dispatcher,
    uri: &Url,
    ctx: RequestContext,
    copied: &HashSet<String>,
    restore: bool,
    options: &BTreeMap<String, String>,
) -> anyhow::Result<Option<crate::correction::CuChange>> {
    let ast = semantic_ast::fetch_with(d, uri.as_str(), ctx.clone()).await?;
    let (threshold, static_threshold) = preferences::import_thresholds();
    let mut imports = ImportRewrite::create(ast.clone(), restore).configure(
        &preferences::import_order(),
        threshold,
        static_threshold,
        options,
    );
    let package = imports.package_name().to_owned();
    let unresolvable = UnresolvableImports::new(&ast);
    let mut old_single = HashSet::new();
    let mut old_demand = HashSet::new();
    for imp in ast.root().list("imports") {
        let name = imp
            .child("name")
            .map(|n| n.identifier())
            .unwrap_or_default();
        if imp.flag("onDemand") {
            old_demand.insert(name);
        } else {
            old_single.insert(name);
        }
    }
    let mut refs = References::default();
    refs.visit(ast.root());
    let mut seen = HashSet::new();
    let mut unresolved = BTreeMap::new();
    for name in refs.types {
        let identifier = name.identifier();
        if seen.contains(&identifier) {
            continue;
        }
        if let Some(binding) = name.binding() {
            if !binding.is_type() {
                continue;
            }
            let binding = if binding.is_array() {
                binding.element_type().unwrap_or(binding)
            } else {
                binding
            };
            let binding = binding.type_declaration().unwrap_or(binding);
            if !binding.is_recovered() {
                let qualified = binding.qualified_name();
                let container = qualified.rsplit_once('.').map(|(q, _)| q).unwrap_or("");
                if container == "java.lang" || container == package || container.is_empty() {
                    continue;
                }
                if (binding.is_top_level() || binding.is_member())
                    && binding.modifiers() & modifier::PRIVATE == 0
                    && (binding.modifiers() & modifier::PUBLIC != 0
                        || binding.package_name() == Some(&package))
                    && !declared_in_scope(name, binding)
                {
                    imports.add_import(qualified, &DefaultContext);
                    seen.insert(identifier);
                }
                continue;
            }
        } else if identifier
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() && c.is_lowercase())
        {
            continue;
        }
        seen.insert(identifier.clone());
        unresolved.insert(identifier, name);
    }
    let types = search_types(d, uri, &ctx).await;
    // TypeNameMatchCollector filters configured type patterns; an explicitly
    // imported type does not remove those filters for organize-imports.
    let patterns: Vec<String> = preferences::get("java.completion.filteredTypes")
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default();
    let filter = TypeFilter::new(&patterns, &[]);
    for (name, node) in unresolved {
        let kinds = possible_type_kinds(node);
        let mut containers = HashSet::new();
        let candidates: Vec<&TypeEntry> = types
            .0
            .iter()
            .filter(|t| {
                t.name == name
                    && !t.package.is_empty()
                    && visible(t, &package)
                    && of_kind(t, kinds)
                    && !filter.is_filtered(&full_name(t))
                    && containers.insert(t.container())
            })
            .collect();
        let selected = if candidates.len() == 1 {
            candidates.first().copied()
        } else {
            candidates
                .iter()
                .find(|t| old_single.contains(&full_name(t)))
                .copied()
                .or_else(|| {
                    let preferred: Vec<_> = candidates
                        .iter()
                        .filter(|t| {
                            old_demand.contains(&t.container())
                                || t.container() == "java.lang"
                                || t.container() == package
                        })
                        .copied()
                        .collect();
                    if preferred.len() == 1 {
                        preferred.first().copied()
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .find(|t| copied.contains(&full_name(t)))
                        .copied()
                })
        };
        if let Some(t) = selected {
            imports.add_import(&full_name(t), &DefaultContext);
        } else {
            for name in unresolvable.matching(&name, false) {
                imports.add_import(name, &UnresolvableContext);
            }
        }
    }
    let mut statics_seen = HashSet::new();
    for name in refs.statics {
        if let Some(binding) = name.binding().filter(|b| !b.is_recovered()) {
            let declaration = if binding.is_method() {
                binding.method_declaration().unwrap_or(binding)
            } else {
                binding.variable_declaration().unwrap_or(binding)
            };
            if let Some(owner) = declaration.declaring_class() {
                imports.add_static_import(
                    owner.qualified_name(),
                    declaration.name(),
                    !declaration.is_method(),
                    &DefaultContext,
                );
            }
        } else if statics_seen.insert(name.identifier()) {
            let existing = unresolvable.matching(&name.identifier(), true);
            if !existing.is_empty() {
                for qualified in existing {
                    if let Some((owner, member)) = qualified.rsplit_once('.') {
                        imports.add_static_import(owner, member, false, &UnresolvableContext);
                    }
                }
                continue;
            }
            // JDT searches completion favorites for an unresolved static
            // selector. Use raw ECJ proposals, not LSP completion items.
            let favorites = preferences::organize_import_favorites();
            if favorites.is_empty() {
                continue;
            }
            // StaticImportFavoritesCompletionInvoker completes a fresh CU
            // containing only the package, primary type and a static initializer.
            let filename =
                crate::classfile::percent_decode(uri.path().rsplit('/').next().unwrap_or(""));
            let primary = ast
                .root()
                .list("types")
                .into_iter()
                .find(|n| {
                    n.child("name").is_some_and(|n| {
                        Some(n.identifier().as_str()) == filename.strip_suffix(".java")
                    })
                })
                .or_else(|| {
                    (uri.scheme() != "file" && !filename.ends_with(".java"))
                        .then(|| ast.root().list("types").into_iter().next())
                        .flatten()
                });
            let Some(primary) = primary.and_then(|n| n.child("name")) else {
                continue;
            };
            let package_decl = if package.is_empty() {
                String::new()
            } else {
                format!("package {package};")
            };
            let dummy = format!(
                "{package_decl}public class {}{{\n static {{\n{}",
                primary.identifier(),
                name.identifier()
            );
            let offset = dummy.encode_utf16().count();
            let mut completion_ctx = ctx.clone();
            completion_ctx
                .files
                .insert(uri.to_string(), format!("{dummy}\n}}\n }}"));
            let result = d
                .code_assist(
                    &completion_ctx,
                    uri.as_str(),
                    offset,
                    serde_json::json!({
                        "op": "complete", "favorites": favorites,
                    }),
                )
                .await?;
            let raw: EngineResult = serde_json::from_value(result)?;
            let is_method = name
                .parent()
                .is_some_and(|p| p.is(NodeKind::MethodInvocation));
            let required_kind = if is_method {
                kind::METHOD_IMPORT
            } else {
                kind::FIELD_IMPORT
            };
            if let Some(required) = raw
                .proposals
                .iter()
                .filter(|p| p.name() == name.identifier())
                .flat_map(|p| p.required())
                .find(|p| p.kind == required_kind)
            {
                let owner = required
                    .declaration_signature
                    .as_deref()
                    .and_then(|s| crate::features::completion::signature::to_string(s).ok());
                if let Some(owner) = owner {
                    imports.add_static_import(
                        &owner,
                        &name.identifier(),
                        !is_method,
                        &DefaultContext,
                    );
                }
            }
        }
    }
    if !imports.has_recorded_changes() {
        return Ok(None);
    }
    let tree = imports
        .rewrite_imports(&types)
        .map_err(|e| anyhow::anyhow!(e.0))?;
    if tree.apply(&ast.source) == ast.source {
        return Ok(None);
    }
    Ok(Some(crate::correction::CuChange::edits(ast, tree)))
}

fn full_name(t: &TypeEntry) -> String {
    format!("{}.{}", t.container(), t.name)
}
fn visible(t: &TypeEntry, package: &str) -> bool {
    t.modifiers & modifier::PRIVATE as u32 == 0
        && (t.modifiers & (modifier::PUBLIC | modifier::PROTECTED) as u32 != 0
            || t.package == package)
}
const CLASS: u8 = 1;
const INTERFACE: u8 = 2;
const ENUM: u8 = 4;
const ANNOTATION: u8 = 8;
const ALL: u8 = CLASS | INTERFACE | ENUM | ANNOTATION;
fn of_kind(t: &TypeEntry, kinds: u8) -> bool {
    let kind = if t.modifiers & ACC_ANNOTATION != 0 {
        ANNOTATION
    } else if t.modifiers & ACC_ENUM != 0 {
        ENUM
    } else if t.modifiers & ACC_INTERFACE != 0 {
        INTERFACE
    } else {
        CLASS
    };
    kinds & kind != 0
}

/// ASTResolving.getPossibleTypeKinds (the searchable reference-type bits).
fn possible_type_kinds(mut node: Node<'_>) -> u8 {
    let mut mask = ALL;
    while let Some(parent) = node.parent() {
        match parent.kind() {
            NodeKind::QualifiedName | NodeKind::QualifiedType | NodeKind::NameQualifiedType => {
                if node.location_is("qualifier") {
                    return mask;
                }
            }
            NodeKind::ParameterizedType => {
                if node.location_is("typeArguments") {
                    return mask;
                }
                mask &= CLASS | INTERFACE;
            }
            NodeKind::WildcardType => {
                if node.location_is("bound") {
                    return mask;
                }
            }
            _ if !parent.kind().is_type() => break,
            _ => {}
        }
        node = parent;
    }
    let Some(parent) = node.parent() else {
        return mask;
    };
    let kinds = match parent.kind() {
        NodeKind::TypeDeclaration if node.location_is("superInterfaceTypes") => INTERFACE,
        NodeKind::TypeDeclaration if node.location_is("superclassType") => CLASS,
        NodeKind::TypeDeclaration if node.location_is("permittedTypes") => {
            if parent.flag("interface") {
                CLASS | INTERFACE
            } else {
                CLASS
            }
        }
        NodeKind::EnumDeclaration => INTERFACE,
        NodeKind::MethodDeclaration if node.location_is("thrownExceptionTypes") => CLASS,
        NodeKind::AnnotationTypeMemberDeclaration => ANNOTATION | ENUM,
        NodeKind::ThrowStatement => CLASS,
        NodeKind::ClassInstanceCreation => {
            if parent.child("anonymousClassDeclaration").is_some() {
                CLASS | INTERFACE
            } else {
                CLASS
            }
        }
        NodeKind::SingleVariableDeclaration
            if parent.parent().is_some_and(|p| p.is(NodeKind::CatchClause)) =>
        {
            CLASS
        }
        NodeKind::MarkerAnnotation
        | NodeKind::SingleMemberAnnotation
        | NodeKind::NormalAnnotation => ANNOTATION,
        NodeKind::TypeParameter
            if parent
                .list("typeBounds")
                .iter()
                .position(|n| n.id == node.id)
                .is_some_and(|i| i > 0) =>
        {
            INTERFACE
        }
        _ => ALL,
    };
    kinds & mask
}

async fn search_types(d: &Dispatcher, uri: &Url, ctx: &RequestContext) -> SearchTypes {
    let ws = d
        .workspace
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let include_tests = crate::features::code_lens::is_test_source(&ws, uri.as_str());
    // Dispatcher already resolves the owning project's VM, including the
    // default project's VM for virtual buffers. Explicit runtimes are searched
    // through their libraries, never supplemented with the running VM.
    let uses_jdk = ctx
        .options
        .get(crate::project::INCLUDE_RUNNING_VM)
        .map_or(true, |value| value == "true");
    let mut types = Vec::new();
    let mut files: Vec<_> = ctx.files.iter().collect();
    files.sort_by_key(|(u, _)| *u);
    for (file, text) in files {
        if !include_tests && crate::features::code_lens::is_test_source(&ws, file) {
            continue;
        }
        types.extend(type_index::source_entries(file, text).0);
    }
    let archives: Vec<_> = ctx
        .classpath
        .iter()
        .filter(|a| {
            include_tests
                || !ws.projects.iter().any(|p| {
                    p.libraries
                        .iter()
                        .any(|l| l.path.to_string_lossy() == a.as_str() && l.is_test)
                })
        })
        .cloned()
        .collect();
    let missing: Vec<_> = archives
        .iter()
        .filter(|a| type_index::cached_archive(a).is_none())
        .cloned()
        .collect();
    let need_jdk = uses_jdk && type_index::cached_jdk().is_none();
    if !missing.is_empty() || need_jdk {
        if let Ok(listing) = d
            .semantic_search(
                ctx,
                serde_json::json!({ "op": "listTypes", "archives": missing, "jdk": need_jdk }),
            )
            .await
        {
            type_index::store_listing(&listing);
        }
    }
    for archive in archives {
        types.extend(type_index::cached_archive(&archive).unwrap_or_default());
    }
    if uses_jdk {
        types.extend(type_index::cached_jdk().unwrap_or_default());
    }
    SearchTypes(types)
}

/// Check lexical and inherited declarations before importing a member type
/// or static member (`ScopeAnalyzer.isDeclaredInScope`).
fn declared_in_scope(name: Node<'_>, binding: BindingRef<'_>) -> bool {
    let Some(owner) = binding.declaring_class() else {
        return false;
    };
    let mut parent = name.parent();
    while let Some(node) = parent {
        if node.kind().is_abstract_type_declaration()
            || node.is(NodeKind::AnonymousClassDeclaration)
        {
            if let Some(enclosing) = node.binding() {
                if hierarchy_contains(enclosing, owner.key(), &mut HashSet::new()) {
                    return true;
                }
            }
        }
        parent = node.parent();
    }
    false
}
fn hierarchy_contains(binding: BindingRef<'_>, key: &str, seen: &mut HashSet<String>) -> bool {
    if binding.key() == key {
        return true;
    }
    if !seen.insert(binding.key().to_owned()) {
        return false;
    }
    binding
        .superclass()
        .is_some_and(|s| hierarchy_contains(s, key, seen))
        || binding
            .interfaces()
            .iter()
            .any(|s| hierarchy_contains(*s, key, seen))
}

/// Port of ImportReferencesCollector's ASTVisitor (whole compilation unit,
/// including method bodies and Javadoc; no range restriction).
#[derive(Default)]
struct References<'a> {
    types: Vec<Node<'a>>,
    statics: Vec<Node<'a>>,
}
impl<'a> References<'a> {
    fn type_ref(&mut self, name: Option<Node<'a>>, possible: bool) {
        let Some(mut name) = name else {
            return;
        };
        if name.is(NodeKind::ModuleQualifiedName) {
            let Some(n) = name.child("name") else {
                return;
            };
            name = n;
        }
        while name.is(NodeKind::QualifiedName) {
            let Some(qualifier) = name.child("qualifier") else {
                return;
            };
            name = qualifier;
        }
        if name.is(NodeKind::SimpleName)
            && (!possible || name.binding().is_none_or(|b| b.is_type()))
        {
            self.types.push(name);
        }
    }
    fn static_ref(&mut self, mut name: Node<'a>) {
        while name.is(NodeKind::QualifiedName) {
            let Some(q) = name.child("qualifier") else {
                return;
            };
            name = q;
        }
        if !name.is(NodeKind::SimpleName) {
            return;
        }
        if let Some(b) = name.binding() {
            if b.is_type() || !b.is_static() || name.flag("declaration") {
                return;
            }
            if !(b.is_method() || b.is_variable() && b.is_field()) {
                return;
            }
            if b.declaring_class().is_none_or(|t| t.is_local()) || declared_in_scope(name, b) {
                return;
            }
        }
        self.statics.push(name);
    }
    fn child(&mut self, node: Node<'a>, prop: &str) {
        if let Some(child) = node.child(prop) {
            self.visit(child);
        }
    }
    fn list(&mut self, node: Node<'a>, prop: &str) {
        for child in node.list(prop) {
            self.visit(child);
        }
    }
    fn qualifying(&mut self, node: Node<'a>, prop: &str, selector: Option<Node<'a>>) {
        if let Some(expr) = node.child(prop) {
            if matches!(
                expr.kind(),
                NodeKind::SimpleName | NodeKind::QualifiedName | NodeKind::ModuleQualifiedName
            ) {
                self.type_ref(Some(expr), true);
                self.static_ref(expr);
            } else {
                self.visit(expr);
            }
        } else if let Some(selector) = selector {
            self.static_ref(selector);
        }
    }
    fn visit(&mut self, node: Node<'a>) {
        match node.kind() {
            NodeKind::ImportDeclaration
            | NodeKind::ContinueStatement
            | NodeKind::BreakStatement => {}
            NodeKind::PackageDeclaration => {
                self.child(node, "javadoc");
                self.list(node, "annotations");
            }
            NodeKind::SimpleType => {
                if !node.flag("var") {
                    self.type_ref(node.child("name"), false);
                }
                self.list(node, "annotations");
            }
            NodeKind::NameQualifiedType => {
                self.type_ref(node.child("qualifier"), true);
                self.list(node, "annotations");
            }
            NodeKind::QualifiedType => {
                self.child(node, "qualifier");
                self.list(node, "annotations");
            }
            NodeKind::QualifiedName | NodeKind::ModuleQualifiedName => {
                self.type_ref(Some(node), true);
                self.static_ref(node);
            }
            NodeKind::LabeledStatement => self.child(node, "body"),
            NodeKind::YieldStatement => self.qualifying(node, "expression", None),
            NodeKind::ThisExpression | NodeKind::SuperFieldAccess => {
                self.type_ref(node.child("qualifier"), false)
            }
            NodeKind::ClassInstanceCreation => {
                self.list(node, "typeArguments");
                self.child(node, "type");
                self.qualifying(node, "expression", None);
                self.child(node, "anonymousClassDeclaration");
                self.list(node, "arguments");
            }
            NodeKind::MethodInvocation => {
                self.qualifying(node, "expression", node.child("name"));
                self.list(node, "typeArguments");
                self.list(node, "arguments");
            }
            NodeKind::ExpressionMethodReference => {
                self.qualifying(node, "expression", node.child("name"));
                self.list(node, "typeArguments");
            }
            NodeKind::CreationReference | NodeKind::TypeMethodReference => {
                self.child(node, "type");
                self.list(node, "typeArguments");
            }
            NodeKind::SuperMethodReference => {
                self.child(node, "qualifier");
                self.list(node, "typeArguments");
            }
            NodeKind::SuperConstructorInvocation => {
                self.qualifying(node, "expression", None);
                self.list(node, "typeArguments");
                self.list(node, "arguments");
            }
            NodeKind::FieldAccess => self.qualifying(node, "expression", node.child("name")),
            NodeKind::SimpleName => self.static_ref(node),
            NodeKind::MarkerAnnotation => self.type_ref(node.child("typeName"), false),
            NodeKind::NormalAnnotation => {
                self.type_ref(node.child("typeName"), false);
                self.list(node, "values");
            }
            NodeKind::SingleMemberAnnotation => {
                self.type_ref(node.child("typeName"), false);
                self.child(node, "value");
            }
            NodeKind::MethodDeclaration => {
                self.child(node, "javadoc");
                self.list(node, "modifiers");
                self.list(node, "typeParameters");
                if !node.flag("constructor") {
                    self.child(node, "returnType2");
                }
                self.child(node, "receiverType");
                self.list(node, "parameters");
                self.list(node, "extraDimensions");
                self.list(node, "thrownExceptionTypes");
                self.child(node, "body");
            }
            NodeKind::TagElement => {
                let fragments = node.list("fragments");
                let mut skip = 0;
                if let Some(first) = fragments.first().filter(|n| {
                    matches!(
                        n.kind(),
                        NodeKind::SimpleName
                            | NodeKind::QualifiedName
                            | NodeKind::ModuleQualifiedName
                    )
                }) {
                    match node.simple("tagName") {
                        Some("@throws" | "@exception") => self.type_ref(Some(*first), false),
                        Some("@see" | "@link" | "@linkplain") => self.type_ref(Some(*first), true),
                        _ => {}
                    }
                    if node.simple("tagName").is_some() {
                        skip = 1;
                    }
                }
                for fragment in fragments.into_iter().skip(skip) {
                    self.visit(fragment);
                }
            }
            NodeKind::MemberRef => self.type_ref(node.child("qualifier"), false),
            NodeKind::MethodRef => {
                self.type_ref(node.child("qualifier"), false);
                self.list(node, "parameters");
            }
            NodeKind::MethodRefParameter => self.child(node, "type"),
            NodeKind::UsesDirective => self.type_ref(node.child("name"), true),
            NodeKind::ProvidesDirective => {
                self.type_ref(node.child("name"), true);
                for n in node.list("implementations") {
                    self.type_ref(Some(n), true);
                }
            }
            _ => {
                for child in node.children() {
                    self.visit(child);
                }
            }
        }
    }
}
