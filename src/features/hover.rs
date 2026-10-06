//! `textDocument/hover`: port of jdt.ls `HoverHandler` / `HoverInfoProvider`.
//!
//! The bridge (`HoverService.hoverInfo`) identifies the element under the
//! cursor (JDT `codeSelect` semantics) and returns its label parts, its
//! Javadoc as a serialized JDT DOM with resolved references, and the
//! supertype data for `{@inheritDoc}`. Everything else happens here:
//! signature labels (`JavaElementLabelsCore`), Javadoc → HTML
//! (`JdtLsJavadocAccessImpl`), HTML → Markdown (`JavaDoc2MarkdownConverter`)
//! and the "Source:" line.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{Hover, HoverContents, LanguageString, MarkedString};

use crate::javadoc::access::{self, DocElement, Env};
use crate::javadoc::converter::javadoc_to_markdown;
use crate::classfile::ClassFileDesc;
use crate::javadoc::doc_ast::{Constant, DocContext, DocSource, InheritData, Location};
use crate::javadoc::labels::{self, FieldLabel, MemberLabel, MethodFlags, MethodLabel, TypeLabel, TypeRef};
use crate::javadoc::markdown_comment::MarkdownComment;

const LANGUAGE_ID: &str = "java";
/// `CompletionResolveHandler.DEFAULT`
const DEFAULT: &str = "Default: ";

/// Element data from the bridge.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Element {
    /// `type`, `method`, `field`, `localVariable`, `typeParameter`, `package`.
    pub kind: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub ty: Option<Value>,
    pub method: Option<MethodLabel>,
    pub field: Option<FieldLabel>,
    pub bounds: Vec<TypeRef>,
    pub declaring_member: Option<MemberLabel>,
    pub location: Option<Location>,
    pub has_source: bool,
    pub javadoc: Option<DocSource>,
    pub doc_context: Option<DocContext>,
    pub inherit: Option<InheritData>,
    pub default_value: Option<Value>,
    // packages
    pub is_source: bool,
    pub source_uri: Option<String>,
    /// Folder of a source package fragment (for `package.html`).
    pub package_dir: Option<String>,
    pub class_file: Option<ClassFileDesc>,
    pub is_enum: bool,
    pub is_annotation: bool,
    /// Attached Javadoc HTML of a binary element without source (filled by
    /// the handler, `IMember.getAttachedJavadoc`).
    #[serde(skip)]
    pub attached_javadoc: Option<String>,
}

/// Client/server state hover depends on.
pub struct HoverEnv<'a> {
    /// `extendedClientCapabilities.classFileContentsSupport`
    pub class_file_support: bool,
    /// `isSupportsCompletionDocumentationMarkdown`
    pub completion_markdown: bool,
    /// Project name owning a source URI (default project otherwise).
    pub project_name: &'a dyn Fn(&str) -> String,
    /// Source folder containing a source URI (`{@docRoot}`).
    pub source_folder: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// `JDTUtils.toUri(IClassFile)` of a class file seen from the hovered unit.
    pub class_file_uri: &'a dyn Fn(&ClassFileDesc) -> String,
    /// Images extracted from jars for the element's Javadoc (`src` → URI).
    pub images: std::collections::HashMap<String, String>,
}

fn marked(language: &str, value: String) -> MarkedString {
    MarkedString::LanguageString(LanguageString { language: language.to_owned(), value })
}

/// The hover for a `hoverInfo` bridge answer.
pub fn hover(status: &str, element: Option<&Element>, env: &HoverEnv) -> Hover {
    let contents = match status {
        "ok" => element
            .map(|e| compute_hover(e, env))
            .unwrap_or_else(|| vec![MarkedString::String(String::new())]),
        "unresolved" => Vec::new(),
        // no element / no unit: `cancelled(res)` / `singletonList("")`
        _ => vec![MarkedString::String(String::new())],
    };
    to_hover(contents)
}

/// lsp4j's `HoverTypeAdapter` writes a one-element contents list as the
/// element itself.
fn to_hover(mut contents: Vec<MarkedString>) -> Hover {
    let contents = if contents.len() == 1 {
        HoverContents::Scalar(contents.pop().unwrap())
    } else {
        HoverContents::Array(contents)
    };
    Hover { contents, range: None }
}

/// The hover for a document the server does not know (`unit == null`).
pub fn empty_hover() -> Hover {
    to_hover(vec![MarkedString::String(String::new())])
}

/// `HoverInfoProvider.computeHover` after element selection.
pub fn compute_hover(e: &Element, env: &HoverEnv) -> Vec<MarkedString> {
    let mut res = Vec::new();
    if let Some(sig) = compute_signature(e) {
        res.push(marked(LANGUAGE_ID, sig));
    }
    if let Some(doc) = compute_javadoc(e, env) {
        if !doc.trim().is_empty() {
            res.push(MarkedString::String(doc));
        }
    }
    if let Some(source) = source_info(e, env) {
        res.push(MarkedString::String(format!("Source: *{source}*")));
    }
    res
}

// ─── computeSignature ────────────────────────────────────────────────────────

pub fn compute_signature(e: &Element) -> Option<String> {
    let mut s = String::new();
    match e.kind.as_str() {
        "type" => {
            let t: TypeLabel = serde_json::from_value(e.ty.clone()?).ok()?;
            labels::type_label(&mut s, &t, true, true);
        }
        "method" => labels::method_label(&mut s, e.method.as_ref()?, MethodFlags::HOVER),
        "field" => {
            let f = e.field.as_ref()?;
            labels::field_label(&mut s, f);
            if f.static_final && !f.is_enum_constant {
                if let Some(c) = &f.constant {
                    s.push_str(" = ");
                    s.push_str(&constant_value(c));
                }
            }
        }
        "localVariable" => {
            let ty: TypeRef = serde_json::from_value(e.ty.clone()?).ok()?;
            labels::local_variable_label(&mut s, e.name.as_deref()?, &ty, e.declaring_member.as_ref());
        }
        "typeParameter" => labels::type_parameter_label(&mut s, e.name.as_deref()?, &e.bounds),
        "package" => s.push_str(e.name.as_deref()?),
        _ => return None,
    }
    Some(s)
}

/// `JDTUtils.getConstantValue` formatting.
pub(crate) fn constant_value(c: &Constant) -> String {
    match c.kind.as_str() {
        "string" => access::escaped_string_literal(&c.value),
        "char" => format!("'{}'", c.value),
        _ => c.value.clone(),
    }
}

// ─── computeJavadoc ──────────────────────────────────────────────────────────

pub fn compute_javadoc(e: &Element, env: &HoverEnv) -> Option<String> {
    let mut result = match e.kind.as_str() {
        "type" | "method" | "field" | "typeParameter" => member_markdown(e, env),
        "package" => package_markdown(e, env),
        _ => return None,
    };
    if e.kind == "method" {
        if let Some(dv) = &e.default_value {
            let value = annotation_value(dv);
            let base = result.unwrap_or_default();
            result = Some(if env.completion_markdown {
                format!("{base}\n{DEFAULT}{value}")
            } else {
                format!("{base}{DEFAULT}{value}")
            });
        }
    }
    result
}

/// Formats a location the way `createLinkURIHelper` does.
fn link_target(env: &HoverEnv, loc: &Location) -> String {
    if let Some(uri) = &loc.uri {
        return format!("{uri}#{}", loc.line.unwrap_or(0) + 1);
    }
    if let Some(cf) = &loc.class_file {
        if env.class_file_support {
            return format!("{}#{}", (env.class_file_uri)(cf), loc.line.unwrap_or(0) + 1);
        }
    }
    String::new()
}

/// A member's documentation before the final conversion: the Javadoc HTML,
/// or a Markdown (`///`) comment rendered directly.
enum MemberDoc {
    Html(Option<String>),
    Markdown(String),
}

/// `JavadocContentAccess2.getMarkdownContent(member)`
fn member_markdown(e: &Element, env: &HoverEnv) -> Option<String> {
    match member_doc(e, env) {
        MemberDoc::Markdown(md) => Some(md),
        MemberDoc::Html(html) => javadoc_to_markdown(html.as_deref()),
    }
}

/// `JavadocContentAccess2.getPlainTextContent(member)` (completion item
/// documentation for clients without Markdown support):
/// `CoreJavadocContentAccessUtility.getHTMLContentReader(member, true, true)`,
/// i.e. the comment text through `CoreJavaDoc2HTMLTextReader` (not the DOM
/// based access), the attached Javadoc of a member without source, or the
/// comment of the first overridden method that has one.
fn member_plain_text(e: &Element) -> Option<String> {
    use crate::javadoc::comment_reader::plain_text_content;
    use crate::javadoc::converter::javadoc_to_plain_text;
    if !e.has_source {
        return javadoc_to_plain_text(e.attached_javadoc.as_deref());
    }
    if let Some(text) = e.javadoc.as_ref().and_then(|d| plain_text_content(&d.raw)) {
        return Some(text);
    }
    if e.kind == "method" {
        // findDocInHierarchy(method, true, true)
        let start = e.inherit.as_ref().map(|i| i.start.as_str());
        for t in e.inherit.iter().flat_map(|i| i.types.iter()) {
            if Some(t.key.as_str()) == start {
                continue;
            }
            let Some(o) = &t.overridden else { continue };
            if let Some(text) = o.javadoc.as_ref().and_then(|d| plain_text_content(&d.raw)) {
                return Some(text);
            }
        }
    }
    None
}

fn member_doc(e: &Element, env: &HoverEnv) -> MemberDoc {
    let ctx = e.doc_context.clone().unwrap_or_default();
    let link = |l: &Location| link_target(env, l);
    if let Some(doc) = &e.javadoc {
        if doc.raw.starts_with("///") {
            return MemberDoc::Markdown(MarkdownComment::new(doc, &link).render());
        }
    }
    if !e.has_source {
        // no source attachment: the attached Javadoc (getAttachedJavadoc)
        return MemberDoc::Html(e.attached_javadoc.clone());
    }
    let can_inherit = ctx.kind == "method" && !ctx.is_constructor;
    // handleDocRoot: the Javadoc base location of a binary member, else the
    // source folder (`File.toURI().toASCIIString()`, i.e. `file:/...`).
    let class_file = e.javadoc.as_ref().and_then(|d| d.class_file.as_ref());
    let doc_root = match class_file {
        Some(cf) if cf.module.is_some() => crate::classfile::jdk_javadoc_location(Path::new(&cf.root)),
        Some(_) => None,
        None => e
            .javadoc
            .as_ref()
            .and_then(|d| d.uri.clone())
            .or_else(|| e.location.as_ref().and_then(|l| l.uri.clone()))
            .and_then(|u| (env.source_folder)(&u))
            .and_then(|p| url::Url::from_directory_path(&p).ok())
            .map(|u| u.to_string().replacen("file:///", "file:/", 1)),
    };
    let access_env = Env { inherit: e.inherit.as_ref(), doc_root, link: &link, images: &env.images };
    let type_key = e.inherit.as_ref().map(|i| i.start.as_str());
    let html = match &e.javadoc {
        Some(doc) => {
            let el = DocElement { doc, ctx: &ctx, type_key };
            access::html_content(&access_env, el, can_inherit)
        }
        None if can_inherit => {
            // javadoc2HTML(member, element, "/***/")
            let empty = DocSource { raw: "/***/".to_owned(), ..Default::default() };
            let el = DocElement { doc: &empty, ctx: &ctx, type_key };
            access::html_content(&access_env, el, true).filter(|h| !h.is_empty())
        }
        None => None,
    };
    MemberDoc::Html(html)
}

/// `getMarkdownContent(packageFragment)`: package-info.java, package.html,
/// attached Javadoc.
fn package_markdown(e: &Element, env: &HoverEnv) -> Option<String> {
    let link = |l: &Location| link_target(env, l);
    let html: Option<String> = if let Some(doc) = &e.javadoc {
        if doc.raw.starts_with("///") {
            return Some(MarkdownComment::new(doc, &link).render());
        }
        let ctx = DocContext { kind: "package".to_owned(), ..Default::default() };
        let access_env = Env { inherit: None, doc_root: None, link: &link, images: &env.images };
        access::html_content(&access_env, DocElement { doc, ctx: &ctx, type_key: None }, false)
    } else if let Some(attached) = &e.attached_javadoc {
        // IPackageFragment.getAttachedJavadoc
        Some(attached.clone())
    } else if e.is_source {
        let html = e
            .package_dir
            .as_deref()
            .map(|d| Path::new(d).join("package.html"))
            .and_then(|p| std::fs::read_to_string(p).ok());
        Some(html.unwrap_or_default())
    } else {
        None
    };
    javadoc_to_markdown(html.map(|h| sanitize_package_javadoc(&h)).as_deref())
}

/// jdt.ls `sanitizePackageJavadoc`
fn sanitize_package_javadoc(content: &str) -> String {
    const UL_CLASS_BLOCK_LIST: &str = "<ul class=\"blockList\">";
    const CONTENT_CONTAINER: &str = "<div class=\"contentContainer\">";
    if content.find(CONTENT_CONTAINER) == Some(0) {
        if let Some(next) = content.find(UL_CLASS_BLOCK_LIST) {
            if next > 0 {
                return content[CONTENT_CONTAINER.len()..next].to_owned();
            }
        }
    }
    content.to_owned()
}

/// `JDTUtils.addValue` for annotation member default values (no links).
pub(crate) fn annotation_value(v: &Value) -> String {
    let kind = v["kind"].as_str().unwrap_or("");
    let value = v["value"].as_str().unwrap_or("");
    match kind {
        "type" => format!("{value}.class"),
        "enum" => value.to_owned(),
        "annotation" => {
            let mut s = format!("@{value}");
            let pairs = v["pairs"].as_array().cloned().unwrap_or_default();
            if !pairs.is_empty() {
                s.push('(');
                for (i, p) in pairs.iter().enumerate() {
                    if i > 0 {
                        s.push_str(labels::COMMA_STRING);
                    }
                    s.push_str(p["name"].as_str().unwrap_or(""));
                    s.push('=');
                    s.push_str(&annotation_value(&p["value"]));
                }
                s.push(')');
            }
            s
        }
        "array" => {
            let items = v["items"].as_array().cloned().unwrap_or_default();
            let parts: Vec<String> = items.iter().map(annotation_value).collect();
            format!("{{{}}}", parts.join(labels::COMMA_STRING))
        }
        "string" => access::escaped_string_literal(value),
        "char" => value.chars().next().map(access::escaped_character_literal).unwrap_or_default(),
        _ => value.to_owned(),
    }
}

// ─── getSourceInfo ───────────────────────────────────────────────────────────

fn source_info(e: &Element, env: &HoverEnv) -> Option<String> {
    let class_file = e
        .class_file
        .as_ref()
        .or_else(|| e.location.as_ref().and_then(|l| l.class_file.as_ref()));
    let source_uri = e
        .location
        .as_ref()
        .and_then(|l| l.uri.clone())
        .or_else(|| e.source_uri.clone())
        .or_else(|| e.javadoc.as_ref().and_then(|d| d.uri.clone()));
    let info = if let Some(uri) = &source_uri {
        (env.project_name)(uri)
    } else if let Some(cf) = class_file {
        if let Some(module) = &cf.module {
            // isSystemLibrary: the JRE container; JrtPackageFragmentRoot adds the module
            let home = std::path::Path::new(&cf.root).parent().and_then(|p| p.parent());
            let version = home.and_then(|h| java_version(&h.to_string_lossy())).unwrap_or_default();
            format!("Java {version} (module: {module})")
        } else {
            Path::new(&cf.root).file_name()?.to_string_lossy().into_owned()
        }
    } else {
        return None;
    };
    if e.kind == "package" {
        return Some(info);
    }
    if let Some(loc) = &e.location {
        let target = link_target(env, loc);
        if !target.is_empty() {
            return Some(format!("[{info}]({target})"));
        }
    }
    Some(info)
}

// ─── HoverHandler ────────────────────────────────────────────────────────────

/// `HoverHandler.hover`. Returns `None` when the bridge is unavailable (the
/// caller falls back to syntax-only hover).
pub async fn handle(
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
    config: &crate::config::Config,
    params: &tower_lsp::lsp_types::HoverParams,
) -> Option<Option<Hover>> {
    use crate::analysis::semantic::BridgeResponse;

    if config.setting("java.hover.javadoc.enabled").and_then(Value::as_bool) == Some(false) {
        return Some(None);
    }
    let uri = &params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    // JDTUtils.resolveTypeRoot: open/workspace documents, or a `.java` file
    // on disk outside every project.
    let ws = dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let class_file = if crate::classfile::is_class_file_uri(uri) {
        match crate::features::navigation::class_file_target(&ws, uri.as_str()) {
            Some((desc, r)) => Some((desc, r.project)),
            None => return Some(Some(empty_hover())),
        }
    } else {
        None
    };
    let mut standalone = None;
    if class_file.is_none() && dispatcher.store.get(uri).is_none() {
        let disk = uri
            .to_file_path()
            .ok()
            .filter(|p| p.extension().is_some_and(|e| e == "java"))
            .and_then(|p| std::fs::read_to_string(p).ok());
        match disk {
            Some(content) => standalone = Some(content),
            None => return Some(Some(empty_hover())),
        }
    }
    if !dispatcher.is_ecj_ready().await {
        return None;
    }
    let hovered_project = match &class_file {
        Some((_, project)) => project.clone(),
        None => ws
            .project_for_uri(uri)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| crate::project::DEFAULT_PROJECT_NAME.to_owned()),
    };
    let attachments = crate::features::navigation::source_attachments(&ws);
    let response = match dispatcher.hover_info(uri, pos.line, pos.character, standalone, class_file, attachments).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("hover error: {e}");
            return None;
        }
    };
    let BridgeResponse::HoverInfo { status, element, .. } = response else {
        if let BridgeResponse::Error { message, .. } = response {
            tracing::warn!("hover bridge error: {message}");
        }
        return None;
    };
    let mut element = element.and_then(|e| match serde_json::from_value::<Element>(e) {
        Ok(e) => Some(e),
        Err(err) => {
            tracing::warn!("hover: bad element data: {err}");
            None
        }
    });
    let images = match &element {
        Some(e) => extract_jar_images(dispatcher, &ws, &hovered_project, e).await,
        None => Default::default(),
    };
    if let Some(e) = element.as_mut() {
        let package_without_doc = e.kind == "package" && e.javadoc.is_none() && !e.is_source;
        if (!e.has_source && matches!(e.kind.as_str(), "type" | "field")) || package_without_doc {
            e.attached_javadoc = attached_javadoc(dispatcher, &ws, &hovered_project, e).await;
        }
    }
    let class_file_uri =
        |desc: &ClassFileDesc| -> String { crate::features::navigation::class_file_uri(&ws, &hovered_project, desc) };
    let project_name = |uri: &str| -> String {
        url::Url::parse(uri)
            .ok()
            .and_then(|u| ws.project_for_uri(&u).map(|p| p.name.clone()))
            .unwrap_or_else(|| crate::project::DEFAULT_PROJECT_NAME.to_owned())
    };
    let source_folder = |uri: &str| -> Option<PathBuf> {
        let u = url::Url::parse(uri).ok()?;
        let path = u.to_file_path().ok()?;
        let project = ws.project_for_uri(&u)?;
        project.source_folder_for(&path).map(|sf| sf.path.clone())
    };
    let env = HoverEnv {
        class_file_support: config.extended_capability("classFileContentsSupport"),
        completion_markdown: config.completion_documentation_markdown,
        project_name: &project_name,
        source_folder: &source_folder,
        class_file_uri: &class_file_uri,
        images,
    };
    Some(Some(hover(&status, element.as_ref(), &env)))
}

/// Completion item documentation (`CompletionResolveHandler`): the
/// `JavadocContentAccess2.getMarkdownContent` / `getPlainTextContent` of the
/// member whose hover element the bridge returned (`memberElement`).
pub async fn completion_documentation(
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
    config: &crate::config::Config,
    project: &str,
    element: &Element,
    markdown: bool,
) -> Option<String> {
    let ws = dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let mut e = element.clone();
    let images = extract_jar_images(dispatcher, &ws, project, &e).await;
    if !e.has_source && matches!(e.kind.as_str(), "type" | "field") {
        e.attached_javadoc = attached_javadoc(dispatcher, &ws, project, &e).await;
    }
    let class_file_uri = |desc: &ClassFileDesc| -> String { crate::features::navigation::class_file_uri(&ws, project, desc) };
    let project_name = |uri: &str| -> String {
        url::Url::parse(uri)
            .ok()
            .and_then(|u| ws.project_for_uri(&u).map(|p| p.name.clone()))
            .unwrap_or_else(|| crate::project::DEFAULT_PROJECT_NAME.to_owned())
    };
    let source_folder = |uri: &str| -> Option<PathBuf> {
        let u = url::Url::parse(uri).ok()?;
        let path = u.to_file_path().ok()?;
        let project = ws.project_for_uri(&u)?;
        project.source_folder_for(&path).map(|sf| sf.path.clone())
    };
    let env = HoverEnv {
        class_file_support: config.extended_capability("classFileContentsSupport"),
        completion_markdown: markdown,
        project_name: &project_name,
        source_folder: &source_folder,
        class_file_uri: &class_file_uri,
        images,
    };
    match e.kind.as_str() {
        "type" | "method" | "field" | "typeParameter" => {
            if markdown {
                member_markdown(&e, &env)
            } else {
                member_plain_text(&e)
            }
        }
        _ => None,
    }
}

/// `javadoc_location` of a library of `project` (`CoreJavaDocLocations.getJavadocBaseLocation`).
fn javadoc_location(ws: &crate::project::Workspace, project: &str, root: &Path) -> Option<String> {
    let vm = ws.project(project).and_then(|p| p.runtime.as_ref())
        .or_else(|| ws.runtime_registry.as_ref().and_then(|r| r.default_install()));
    if let Some(url) = vm.and_then(|vm| vm.libraries.iter().find(|l| l.path == root))
        .and_then(|l| l.javadoc.clone()) {
        return Some(url);
    }
    let attributes = match ws.project(project) {
        Some(p) if p.kind == crate::project::ProjectKind::Maven => p
            .libraries
            .iter()
            .find(|l| l.path == root)
            .map(|lib| crate::classfile::maven_attributes(root, lib.is_test, &crate::project::maven::local_repository()))
            .unwrap_or_default(),
        Some(p) => crate::classfile::eclipse_library_attributes(&p.root, root),
        None => Vec::new(),
    };
    attributes.into_iter().find(|(k, _)| k == "javadoc_location").map(|(_, v)| v)
}

/// `BinaryType` / `BinaryField.getAttachedJavadoc`: the type's HTML page
/// under the Javadoc base location, sliced by `JavadocContents`.
async fn attached_javadoc(
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
    ws: &crate::project::Workspace,
    project: &str,
    e: &Element,
) -> Option<String> {
    use crate::analysis::semantic::{BridgeRequest, BridgeResponse};
    let cf = match e.kind.as_str() {
        "package" => e.class_file.as_ref()?,
        _ => e.location.as_ref()?.class_file.as_ref()?,
    };
    let base = javadoc_location(ws, project, Path::new(&cf.root))?;
    let base = if base.ends_with('/') { base } else { format!("{base}/") };
    let rel = if e.kind == "package" {
        format!("{}/package-summary.html", cf.package_name.replace('.', "/"))
    } else {
        let type_qualified_name = cf.class_file_name.strip_suffix(".class")?.replace('$', ".");
        format!("{}/{}.html", cf.package_name.replace('.', "/"), type_qualified_name)
    };
    let html = if let Some(archive) = base.strip_prefix("jar:").and_then(|_| jar_path_from_uri(&base)) {
        let inner = base.split_once("!/").map(|(_, r)| r).unwrap_or("");
        let entry = format!("{inner}{rel}");
        match dispatcher
            .send_request(BridgeRequest::ReadJarEntry {
                id: crate::analysis::semantic::ecj_process::next_id(),
                archive,
                entry,
            })
            .await
        {
            Ok(BridgeResponse::ClassFileContents { contents, .. }) => contents,
            _ => return None,
        }
    } else if base.starts_with("file:") {
        let dir = url::Url::parse(&base).ok()?.to_file_path().ok()?;
        std::fs::read_to_string(dir.join(&rel)).ok()?
    } else {
        // remote Javadoc locations are not fetched
        return None;
    };
    let mut contents = crate::javadoc::attached::JavadocContents::for_html(&html);
    match e.kind.as_str() {
        "type" => contents.type_doc(e.is_enum, e.is_annotation),
        "field" => contents.field_doc(&e.field.as_ref()?.name),
        "package" => contents.package_doc(),
        _ => None,
    }
}

/// The jar branch of `JavaDocHTMLPathHandler.getValidatedHTMLSrcAttribute`:
/// images of a binary member's Javadoc are extracted from its Javadoc jar,
/// else its source jar, to `EXTRACTED_JAR_IMAGES_FOLDER/<jar>/<file>`.
async fn extract_jar_images(
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
    ws: &crate::project::Workspace,
    project: &str,
    e: &Element,
) -> std::collections::HashMap<String, String> {
    use crate::analysis::semantic::{BridgeRequest, BridgeResponse};
    let mut out = std::collections::HashMap::new();
    let Some(doc) = &e.javadoc else { return out };
    let Some(cf) = &doc.class_file else { return out };
    let root = PathBuf::from(&cf.root);
    // CoreJavaDocLocations.getJavadocBaseLocation: the `javadoc_location` attribute
    let attributes = match ws.project(project) {
        Some(p) if p.kind == crate::project::ProjectKind::Maven => p
            .libraries
            .iter()
            .find(|l| l.path == root)
            .map(|lib| crate::classfile::maven_attributes(&root, lib.is_test, &crate::project::maven::local_repository()))
            .unwrap_or_default(),
        Some(p) => crate::classfile::eclipse_library_attributes(&p.root, &root),
        None => Vec::new(),
    };
    let javadoc_jar = attributes.iter().find(|(k, _)| k == "javadoc_location").and_then(|(_, v)| jar_path_from_uri(v));
    let _ = javadoc_location;
    let source_jar = crate::features::navigation::source_attachments(ws).get(&cf.root).cloned();
    let jar_root = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let jar_root = jar_root.strip_suffix(".jar").unwrap_or(&jar_root).to_owned();
    for node in &doc.nodes {
        let Some(text) = node.text.as_deref() else { continue };
        if !node.is_text() || !crate::javadoc::path_handler::contains_html_tag(text) {
            continue;
        }
        let Some((src, entry, file_name)) = crate::javadoc::path_handler::jar_image_candidate(text, &cf.package_name) else {
            continue;
        };
        if out.contains_key(&src) {
            continue;
        }
        let output = crate::javadoc::path_handler::extracted_jar_images_folder().join(&jar_root).join(&file_name);
        for archive in javadoc_jar.iter().chain(source_jar.iter()) {
            let fresh = match (std::fs::metadata(&output), std::fs::metadata(archive)) {
                (Ok(o), Ok(a)) => match (o.created(), a.created()) {
                    (Ok(oc), Ok(ac)) => ac <= oc,
                    _ => true,
                },
                _ => false,
            };
            let ok = fresh
                || matches!(
                    dispatcher
                        .send_request(BridgeRequest::ExtractJarEntry {
                            id: crate::analysis::semantic::ecj_process::next_id(),
                            archive: archive.clone(),
                            entry: entry.clone(),
                            output: output.to_string_lossy().into_owned(),
                        })
                        .await,
                    Ok(BridgeResponse::Ok { .. })
                );
            if ok {
                if let Some(uri) = crate::javadoc::path_handler::file_uri(&output) {
                    out.insert(src.clone(), uri);
                }
                break;
            }
        }
    }
    out
}

/// `JavaDocHTMLPathHandler.getJarPathFromURI`
fn jar_path_from_uri(uri: &str) -> Option<String> {
    let ssp = uri.split_once(':').map(|(_, r)| r)?;
    let path = ssp.split_once(':').map(|(_, r)| r).unwrap_or(ssp);
    let i = path.rfind(".jar")?;
    Some(path[..i + 4].to_owned())
}

/// `IVMInstall2.getJavaVersion()` from `<home>/release`.
fn java_version(home: &str) -> Option<String> {
    let release = std::fs::read_to_string(Path::new(home).join("release")).ok()?;
    release
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))
        .map(|v| v.trim_matches('"').to_owned())
}
