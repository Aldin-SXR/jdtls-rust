//! GenerateAccessorsHandler / GenerateGetterSetterOperation, with Java-model
//! discovery and ASTRewrite edit computation in Rust.
pub(crate) mod actions;
pub(crate) mod naming;
pub(crate) mod templates;

use super::java_model::{self, flags, FieldDecl, Member, TypeDecl, TypeKind};
use crate::analysis::dispatcher::Dispatcher;
use crate::correction::{edit::Env, Context, CuChange};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{finder::NodeFinder, Ast, Node, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{CodeActionParams, Range, Url, WorkspaceEdit};

#[derive(Clone, Copy, Debug)]
pub enum AccessorKind {
    GETTER,
    SETTER,
    BOTH,
}

// JDT LS's enum adapter accepts names and serializes prompt arguments as ordinals.
impl Serialize for AccessorKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(match self {
            Self::GETTER => 0,
            Self::SETTER => 1,
            Self::BOTH => 2,
        })
    }
}
impl<'de> Deserialize<'de> for AccessorKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        match (value.as_str(), value.as_u64()) {
            (Some("GETTER"), _) | (_, Some(0)) => Ok(Self::GETTER),
            (Some("SETTER"), _) | (_, Some(1)) => Ok(Self::SETTER),
            (Some("BOTH"), _) | (_, Some(2)) => Ok(Self::BOTH),
            _ => Err(serde::de::Error::custom("Invalid accessor kind")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessorField {
    pub field_name: String,
    #[serde(default)]
    pub is_static: bool,
    #[serde(default)]
    pub generate_getter: bool,
    #[serde(default)]
    pub generate_setter: bool,
    #[serde(default)]
    pub type_name: String,
}

#[derive(Deserialize)]
pub struct AccessorParams {
    #[serde(flatten)]
    pub context: CodeActionParams,
    pub kind: AccessorKind,
}
#[derive(Deserialize)]
pub struct GenerateAccessorsParams {
    pub context: CodeActionParams,
    pub accessors: Option<Vec<AccessorField>>,
}

#[derive(Clone)]
pub(crate) struct Selection {
    pub declaration: NodeId,
    pub model: TypeDecl,
}

/// SourceAssistProcessor.getSelectionType, falling back to findPrimaryType.
pub(crate) fn selection(context: &Context, uri: &Url) -> Option<Selection> {
    let ast = &context.ast;
    let cu = java_model::parse(ast.text());
    let types = all_types(&cu.types);
    let mut node = context.covered_node().or_else(|| context.covering_node());
    while let Some(n) = node {
        if n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration) {
            let found = if n.is(NodeKind::AnonymousClassDeclaration) {
                types
                    .iter()
                    .filter(|t| t.anonymous)
                    .filter(|t| {
                        utf16(ast.text(), t.source.0) <= n.start()
                            && utf16(ast.text(), t.source.1) >= n.end()
                    })
                    .min_by_key(|t| t.source.1 - t.source.0)
                    .copied()
            } else {
                let name = n.child("name")?;
                types
                    .iter()
                    .find(|t| !t.anonymous && utf16(ast.text(), t.name_range.0) == name.start())
                    .copied()
            };
            if let Some(model) = found {
                return Some(Selection {
                    declaration: n.id,
                    model: model.clone(),
                });
            }
        }
        node = n.parent();
    }
    let filename = crate::classfile::percent_decode(uri.path().rsplit('/').next()?);
    let primary = if let Some(name) = filename.strip_suffix(".java") {
        cu.types.iter().find(|t| t.name == name)?
    } else if uri.scheme() != "file" {
        cu.types.first()?
    } else {
        return None;
    };
    let declaration = ast.root().list("types").into_iter().find(|n| {
        n.child("name")
            .is_some_and(|n| n.identifier() == primary.name)
    })?;
    Some(Selection {
        declaration: declaration.id,
        model: primary.clone(),
    })
}

fn all_types(types: &[TypeDecl]) -> Vec<&TypeDecl> {
    fn visit<'a>(t: &'a TypeDecl, out: &mut Vec<&'a TypeDecl>) {
        out.push(t);
        for member in &t.members {
            match member {
                Member::Type(t) => visit(t, out),
                Member::Field(f) => {
                    for t in &f.children {
                        visit(t, out);
                    }
                }
                Member::Method(m) => {
                    for t in &m.children {
                        visit(t, out);
                    }
                }
                Member::Initializer(i) => {
                    for t in &i.children {
                        visit(t, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for t in types {
        visit(t, &mut out);
    }
    out
}
fn utf16(source: &str, byte: usize) -> usize {
    source[..byte].encode_utf16().count()
}

pub(crate) fn context(ast: Arc<Ast>, params: &CodeActionParams) -> Context {
    let start = ast.offset_of(params.range.start).unwrap_or(0);
    let end = ast.offset_of(params.range.end).unwrap_or(start);
    Context::new(ast, start, end.saturating_sub(start))
}

pub(crate) async fn profile(d: &Dispatcher, uri: &Url) -> templates::Profile {
    let root = d
        .workspace
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .project_for_uri(uri)
        .map(|p| p.root.clone());
    templates::Profile::load(root.as_deref())
}

pub async fn resolve(d: &Dispatcher, params: AccessorParams) -> Vec<AccessorField> {
    let uri = &params.context.text_document.uri;
    let Ok(ast) = crate::semantic_ast::fetch(d, uri).await else {
        return Vec::new();
    };
    let context = context(ast, &params.context);
    let Some(selected) = selection(&context, uri) else {
        return Vec::new();
    };
    let (options, _) = d.options_for(Some(uri)).await;
    unimplemented(
        &selected.model,
        params.kind,
        &options,
        &profile(d, uri).await,
    )
}

pub(crate) fn unimplemented(
    t: &TypeDecl,
    kind: AccessorKind,
    options: &BTreeMap<String, String>,
    profile: &templates::Profile,
) -> Vec<AccessorField> {
    if matches!(t.kind, TypeKind::Interface | TypeKind::Annotation) {
        return Vec::new();
    }
    t.members
        .iter()
        .filter_map(|m| {
            let Member::Field(f) = m else {
                return None;
            };
            // Records expose record components, excluding ordinary static fields.
            if f.enum_constant || (t.kind == TypeKind::Record && !f.record_component) {
                return None;
            }
            let generate_getter = !has_getter(t, f, options);
            let generate_setter =
                f.flags & flags::FINAL == 0 && !has_setter(t, f, options, profile);
            let (generate_getter, generate_setter) = match kind {
                AccessorKind::GETTER => (generate_getter, false),
                AccessorKind::SETTER => (false, generate_setter),
                AccessorKind::BOTH => (generate_getter, generate_setter),
            };
            (generate_getter || generate_setter).then(|| AccessorField {
                field_name: f.name.clone(),
                is_static: f.flags & flags::STATIC != 0,
                generate_getter,
                generate_setter,
                type_name: naming::signature_simple_type(f.type_signature.as_deref().unwrap_or("")),
            })
        })
        .collect()
}
fn has_getter(t: &TypeDecl, f: &FieldDecl, options: &BTreeMap<String, String>) -> bool {
    let first = naming::getter(t, f, options, true);
    let second = naming::getter(t, f, options, false);
    t.members.iter().any(|m| matches!(m, Member::Method(m) if !m.constructor && m.params.is_empty() && (m.name == first || f.type_label.as_deref() == Some("boolean") && m.name == second)))
}
fn has_setter(
    t: &TypeDecl,
    f: &FieldDecl,
    options: &BTreeMap<String, String>,
    profile: &templates::Profile,
) -> bool {
    let name = naming::setter(f, options, profile.use_is);
    t.members.iter().any(|m| matches!(m, Member::Method(m) if !m.constructor && m.name == name && m.params.len() == 1 && naming::method_type(&m.params[0]) == naming::method_type(f.type_label.as_deref().unwrap_or(""))))
}

pub async fn generate(env: &Env<'_>, params: GenerateAccessorsParams) -> Option<WorkspaceEdit> {
    let fields = params.accessors?;
    if fields.is_empty() {
        return None;
    }
    let uri = &params.context.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let context = context(ast.clone(), &params.context);
    let selected = selection(&context, uri)?;
    let node = NodeFinder::perform(
        ast.root(),
        context.selection_offset,
        context.selection_length,
    );
    let cursor = if declaration_node(node).is_some() {
        None
    } else {
        Some(params.context.range)
    };
    let comments =
        super::preferences::get_bool("java.codeGeneration.generateComments").unwrap_or(false);
    let mut change = create_change(env, ast.clone(), &selected, &fields, comments, cursor)
        .await
        .ok()?;
    let tree = crate::correction::edit::cu_tree(env, &mut change)
        .await
        .ok()?;
    let edits = crate::correction::edit::tree_to_text_edits(&ast.source, &tree);
    Some(WorkspaceEdit {
        changes: Some([(uri.clone(), edits)].into()),
        ..Default::default()
    })
}

pub(crate) fn declaration_node(mut node: Option<Node<'_>>) -> Option<Node<'_>> {
    if node.is_some_and(|n| n.kind().is_body_declaration()) {
        return None;
    }
    while let Some(n) = node {
        if n.kind().is_body_declaration() || n.kind().is_statement() {
            return n.is(NodeKind::TypeDeclaration).then_some(n);
        }
        node = n.parent();
    }
    None
}

pub(crate) async fn create_change(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    accessors: &[AccessorField],
    comments: bool,
    cursor: Option<Range>,
) -> anyhow::Result<CuChange> {
    let uri = Url::parse(&ast.uri)?;
    let options = env.options(&ast.uri).await;
    let profile = profile(env.dispatcher, &uri).await;
    let before = insert_before(&ast, selected, cursor);
    let eol = crate::rewrite::analyzer::default_line_delimiter(&ast.source);
    let mut enclosing = Vec::new();
    let mut node = Some(ast.node(selected.declaration));
    while let Some(n) = node {
        if n.kind().is_abstract_type_declaration() {
            if let Some(name) = n.child("name") {
                enclosing.push(name.identifier());
            }
        }
        node = n.parent();
    }
    enclosing.reverse();
    let enclosing_type = enclosing.join(".");
    let mut rw = ASTRewrite::new(ast.clone());
    for accessor in accessors {
        let Some(field) = selected.model.members.iter().find_map(|m| match m {
            Member::Field(f) if f.name == accessor.field_name => Some(f),
            _ => None,
        }) else {
            anyhow::bail!("Field {} does not exist", accessor.field_name);
        };
        for getter in [true, false] {
            if getter && (!accessor.generate_getter || has_getter(&selected.model, field, &options))
                || !getter
                    && (!accessor.generate_setter
                        || has_setter(&selected.model, field, &options, &profile))
            {
                continue;
            }
            let stub = templates::stub(
                &selected.model,
                field,
                getter,
                comments,
                &options,
                &profile,
                &ast,
                &enclosing_type,
            )?;
            let formatted = match env
                .dispatcher
                .format_source(
                    &stub,
                    crate::rewrite::formatter::K_CLASS_BODY_DECLARATIONS,
                    0,
                    stub.encode_utf16().count(),
                    &eol,
                    options.clone(),
                )
                .await?
            {
                Some(edits) => {
                    let edits: Vec<_> = edits
                        .into_iter()
                        .map(|e| (e.offset, e.length, e.text))
                        .collect();
                    String::from_utf16_lossy(&crate::rewrite::text_edit::apply_flat(
                        &stub.encode_utf16().collect::<Vec<_>>(),
                        &edits,
                    ))
                }
                None => stub,
            };
            let placeholder = rw.create_string_placeholder(&formatted, NodeKind::MethodDeclaration);
            match before {
                Some(before) => rw.list_insert_before(
                    RNode::Orig(selected.declaration),
                    "bodyDeclarations",
                    placeholder,
                    RNode::Orig(before),
                ),
                None => rw.list_insert_last(
                    RNode::Orig(selected.declaration),
                    "bodyDeclarations",
                    placeholder,
                ),
            }
        }
    }
    Ok(CuChange::rewrite(rw))
}

pub(crate) fn insert_before(
    ast: &Ast,
    selected: &Selection,
    cursor: Option<Range>,
) -> Option<NodeId> {
    let cursor = cursor?;
    let location = super::preferences::get_string("java.codeGeneration.insertionLocation")
        .unwrap_or_else(|| "afterCursor".into());
    if location == "lastMember" {
        return None;
    }
    let before = location == "beforeCursor";
    let offset = ast.offset_of(if before { cursor.start } else { cursor.end })?;
    let member = selected.model.members.iter().find(|m| {
        let range = match m {
            Member::Field(f) => f.source,
            Member::Method(m) => m.source,
            Member::Type(t) => t.source,
            Member::Initializer(i) => i.source,
        };
        offset <= utf16(ast.text(), if before { range.1 } else { range.0 })
    })?;
    let byte = match member {
        Member::Field(f) => f.name_range.0,
        Member::Method(m) => m.name_range.0,
        Member::Type(t) => t.name_range.0,
        Member::Initializer(i) => i.source.0,
    };
    let pos = utf16(ast.text(), byte);
    ast.node(selected.declaration)
        .list("bodyDeclarations")
        .into_iter()
        .find(|n| n.start() <= pos && pos < n.end())
        .map(|n| n.id)
}
