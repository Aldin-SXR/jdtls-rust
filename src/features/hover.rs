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
use crate::javadoc::doc_ast::{ClassFileRef, Constant, DocContext, DocSource, InheritData, Location};
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
    pub class_file: Option<ClassFileRef>,
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
    /// The JDK home (for "Java <version>").
    pub java_home: Option<String>,
}

fn marked(language: &str, value: String) -> MarkedString {
    MarkedString::LanguageString(LanguageString { language: language.to_owned(), value })
}

/// The hover for a `hoverInfo` bridge answer.
pub fn hover(status: &str, element: Option<&Value>, env: &HoverEnv) -> Hover {
    let contents = match status {
        "ok" => element
            .and_then(|e| match serde_json::from_value::<Element>(e.clone()) {
                Ok(e) => Some(e),
                Err(err) => {
                    tracing::warn!("hover: bad element data: {err}");
                    None
                }
            })
            .map(|e| compute_hover(&e, env))
            .unwrap_or_else(|| vec![MarkedString::String(String::new())]),
        "unresolved" => Vec::new(),
        // no element / no unit: `cancelled(res)` / `singletonList("")`
        _ => vec![MarkedString::String(String::new())],
    };
    Hover { contents: HoverContents::Array(contents), range: None }
}

/// The hover for a document the server does not know (`unit == null`).
pub fn empty_hover() -> Hover {
    Hover { contents: HoverContents::Array(vec![MarkedString::String(String::new())]), range: None }
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
fn constant_value(c: &Constant) -> String {
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
        if let Some(uri) = class_file_uri(cf, env) {
            return format!("{uri}#{}", loc.line.unwrap_or(0) + 1);
        }
    }
    String::new()
}

/// `JDTUtils.toUri(IClassFile)` (only when the client supports class file
/// contents).
pub fn class_file_uri(cf: &ClassFileRef, env: &HoverEnv) -> Option<String> {
    if !env.class_file_support {
        return None;
    }
    let root = cf.root.as_deref()?;
    let jar_name = match cf.root_kind.as_deref() {
        Some("jrt") => cf.module.clone().unwrap_or_default(),
        _ => Path::new(root).file_name()?.to_string_lossy().into_owned(),
    };
    // Class files of member types are named after their top-level type.
    let top = cf.class_file_name.split('$').next().unwrap_or(&cf.class_file_name);
    let file_name = if top.ends_with(".class") { top.to_owned() } else { format!("{top}.class") };
    let mut path = format!("/{jar_name}");
    if !cf.package_name.is_empty() {
        path.push('/');
        path.push_str(&cf.package_name);
    }
    path.push('/');
    path.push_str(&file_name);
    let root_handle = root.replace('/', "\\/");
    let handle = format!("={}/{}<{}({}", env_project_placeholder(), root_handle, cf.package_name, cf.class_file_name);
    let uri = format!("jdt://contents{}?{}", encode_uri_component(&path, false), encode_uri_component(&handle, true));
    Some(uri.replace('(', "%28"))
}

fn env_project_placeholder() -> &'static str {
    crate::project::DEFAULT_PROJECT_NAME
}

/// `java.net.URI` multi-argument constructor quoting (illegal characters only).
fn encode_uri_component(s: &str, query: bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let ok = c.is_ascii_alphanumeric()
            || "-_.!~*'()".contains(c)
            || ";/?:@&=+$,".contains(c)
            || (query && c == '[')
            || (query && c == ']');
        if ok {
            out.push(c);
        } else {
            let mut b = [0u8; 4];
            for byte in c.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// `JavadocContentAccess2.getMarkdownContent(member)`
fn member_markdown(e: &Element, env: &HoverEnv) -> Option<String> {
    let ctx = e.doc_context.clone().unwrap_or_default();
    let link = |l: &Location| link_target(env, l);
    if let Some(doc) = &e.javadoc {
        if doc.raw.starts_with("///") {
            return Some(MarkdownComment::new(doc, &link).render());
        }
    }
    if !e.has_source {
        // no source attachment: attached Javadoc is not supported
        return None;
    }
    let can_inherit = ctx.kind == "method" && !ctx.is_constructor;
    let doc_root = e
        .javadoc
        .as_ref()
        .and_then(|d| d.uri.clone())
        .or_else(|| e.location.as_ref().and_then(|l| l.uri.clone()))
        .and_then(|u| (env.source_folder)(&u))
        .and_then(|p| url::Url::from_directory_path(&p).ok())
        .map(|u| u.to_string());
    let access_env = Env { inherit: e.inherit.as_ref(), doc_root, link: &link };
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
    javadoc_to_markdown(html.as_deref())
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
        let access_env = Env { inherit: None, doc_root: None, link: &link };
        access::html_content(&access_env, DocElement { doc, ctx: &ctx, type_key: None }, false)
    } else if e.is_source {
        let html = e
            .source_uri
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok())
            .and_then(|u| u.to_file_path().ok())
            .and_then(|p| p.parent().map(|d| d.join("package.html")))
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
fn annotation_value(v: &Value) -> String {
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
        match cf.root_kind.as_deref() {
            Some("jrt") => {
                let version = cf.root.as_deref().or(env.java_home.as_deref()).and_then(java_version).unwrap_or_default();
                let mut s = format!("Java {version}");
                if let Some(m) = &cf.module {
                    s.push_str(&format!(" (module: {m})"));
                }
                s
            }
            Some("archive") => Path::new(cf.root.as_deref()?).file_name()?.to_string_lossy().into_owned(),
            _ => return None,
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
    let mut standalone = None;
    if dispatcher.store.get(uri).is_none() {
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
    let response = match dispatcher.hover_info(uri, pos.line, pos.character, standalone).await {
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
    let ws = dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
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
        java_home: config.java_home.clone(),
    };
    Some(Some(hover(&status, element.as_ref(), &env)))
}

/// `IVMInstall2.getJavaVersion()` from `<home>/release`.
fn java_version(home: &str) -> Option<String> {
    let release = std::fs::read_to_string(Path::new(home).join("release")).ok()?;
    release
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))
        .map(|v| v.trim_matches('"').to_owned())
}
