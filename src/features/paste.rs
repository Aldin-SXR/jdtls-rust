//! Rust port of jdt.ls `PasteEventHandler`: string escaping, import edits
//! against an isolated working copy, and Java file destination selection.

use super::organize_imports::operation as imports;

use super::completion::doc::Doc;
use super::java_model::{self, TypeKind};
use crate::analysis::dispatcher::Dispatcher;
use crate::project::{Workspace, DEFAULT_PROJECT_NAME};
use crate::semantic_ast::NodeKind;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{FormattingOptions, Location, WorkspaceEdit};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteEventParams {
    location: Location,
    text: String,
    copied_document_uri: Option<String>,
    formatting_options: FormattingOptions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPasteEdit {
    insert_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_edit: Option<WorkspaceEdit>,
}

pub async fn handle(d: &Dispatcher, params: PasteEventParams) -> Option<DocumentPasteEdit> {
    let uri = &params.location.uri;
    let source = super::source_text(&d.store, uri)?;
    let mut ctx = d.context_for(Some(uri)).await;
    ctx.files.insert(uri.to_string(), source.clone());
    let ast = crate::semantic_ast::fetch_with(d, uri.as_str(), ctx.clone())
        .await
        .ok()?;
    let doc = Doc::new(&source);
    let start = doc.offset(params.location.range.start);
    let end = doc.offset(params.location.range.end);
    if end < start {
        return None;
    }
    // StringRangeFinder visits StringLiteral only, with both endpoints
    // strictly inside its range. TextBlock and quoted comments don't qualify.
    if let Some(literal) = ast
        .all_nodes()
        .filter(|n| n.is(NodeKind::StringLiteral))
        .filter(|n| start > n.start() && end < n.end())
        .last()
    {
        let line = doc.line_of(literal.start());
        let mut indent: String = doc
            .get(doc.line_offset(line), doc.line_length(line))
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        if params.formatting_options.insert_spaces {
            indent.push_str(&" ".repeat(2 * params.formatting_options.tab_size as usize));
        } else {
            indent.push_str("\t\t");
        }
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let insert_text = string_paste(&params.text, eol, &indent);
        return Some(DocumentPasteEdit {
            insert_text,
            additional_edit: None,
        });
    }
    if !super::preferences::get_bool("java.updateImportsOnPaste.enabled").unwrap_or(true)
        || params.copied_document_uri.as_deref() == Some(uri.as_str())
    {
        return None;
    }
    // findPrimaryType matches the compilation unit's filename, including
    // virtual documents; a different top-level type isn't a substitute.
    let filename = crate::classfile::percent_decode(uri.path().rsplit('/').next()?);
    let model = java_model::parse(&source);
    let primary = match filename.strip_suffix(".java") {
        Some(name) => model.types.iter().find(|t| t.name == name),
        // Editors give untitled buffers names such as Untitled-1. They have
        // no compilation-unit filename; their first type owns the buffer.
        None if uri.scheme() != "file" => model.types.first(),
        None => None,
    }?;
    let type_start = source[..primary.source.0].encode_utf16().count();
    let type_end = source[..primary.source.1].encode_utf16().count();
    if start <= type_start || end >= type_end {
        return None;
    }
    let mut units = doc.units;
    units.splice(start..end, params.text.encode_utf16());
    let temporary = String::from_utf16_lossy(&units);
    ctx.files.insert(uri.to_string(), temporary);
    let mut copied_imports = std::collections::HashSet::new();
    if let Some(copied) = params
        .copied_document_uri
        .as_deref()
        .and_then(|s| s.parse().ok())
    {
        if let Some(text) = super::document_text(d, &copied).await {
            copied_imports = imports::import_names(&text);
        }
    }
    let additional_edit = imports::missing_imports(d, uri, ctx, &copied_imports)
        .await
        .ok()??;
    Some(DocumentPasteEdit {
        insert_text: params.text,
        additional_edit: Some(additional_edit),
    })
}

fn escape_java(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{8}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn string_paste(text: &str, eol: &str, indent: &str) -> String {
    if !text.contains(['\r', '\n']) {
        return escape_java(text);
    }
    // Preserve upstream's private-use marker translation, including its
    // treatment of those characters when they already occur in copied text.
    let marked = text
        .replace("\r\n", "\u{e000}")
        .replace('\n', "\u{e001}")
        .replace('\r', "\u{e002}");
    escape_java(&marked)
        .replace('\u{e000}', &format!("\\r\\n\" + //{eol}{indent}\""))
        .replace('\u{e001}', &format!("\\n\" + //{eol}{indent}\""))
        .replace('\u{e002}', &format!("\\r\" + //{eol}{indent}\""))
}

/// `handleFilePasteEvent` (`java.project.resolveText`): suggest a path;
/// creating the file remains the editor's responsibility.
pub fn file_paste(ws: &Workspace, path: &str, content: &str) -> Option<String> {
    let cu = java_model::parse(content);
    if cu.package.is_none() && cu.imports.is_empty() && cu.types.is_empty() {
        return None;
    }
    let folder = cu
        .package
        .as_ref()
        .map(|p| matching_package(ws, &p.name, Path::new(path)))
        .unwrap_or_else(|| PathBuf::from(path));
    let name = cu
        .types
        .first()
        .filter(|t| matches!(t.kind, TypeKind::Class | TypeKind::Interface))
        .map(|t| t.name.as_str())
        .unwrap_or("Untitled");
    let mut candidate = folder.join(format!("{name}.java"));
    let mut counter = 1;
    while candidate.exists() {
        candidate = folder.join(format!("{name}{counter}.java"));
        counter += 1;
    }
    Some(candidate.to_string_lossy().into_owned())
}

fn matching_package(ws: &Workspace, name: &str, path: &Path) -> PathBuf {
    let mut root = path.to_path_buf();
    let mut prefix: Option<(PathBuf, String)> = None;
    for project in &ws.projects {
        if project.name == DEFAULT_PROJECT_NAME || !project.is_java() {
            continue;
        }
        for source in &project.source_folders {
            // Package fragments include empty folders and intermediate
            // packages, not only folders containing a compilation unit.
            let mut packages: Vec<PathBuf> = walkdir::WalkDir::new(&source.path)
                .follow_links(true)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_dir())
                .map(|e| e.into_path())
                .filter(|p| {
                    p != &source.path
                        && project
                            .source_folder_for(p)
                            .is_some_and(|sf| sf.path == source.path)
                })
                .collect();
            packages.sort();
            for folder in packages {
                let Some(segments) = folder
                    .strip_prefix(&source.path)
                    .ok()
                    .and_then(|p| p.iter().map(|s| s.to_str()).collect::<Option<Vec<_>>>())
                else {
                    continue;
                };
                if !segments.iter().all(|s| java_identifier(s)) {
                    continue;
                }
                root = source.path.clone();
                let package = segments.join(".");
                if package == name {
                    return folder;
                }
                if let Some(rest) = name.strip_prefix(&format!("{package}.")) {
                    if prefix.as_ref().is_none_or(|(p, _)| {
                        folder.to_string_lossy().encode_utf16().count()
                            > p.to_string_lossy().encode_utf16().count()
                    }) {
                        prefix = Some((folder, rest.replace('.', std::path::MAIN_SEPARATOR_STR)));
                    }
                }
            }
        }
    }
    prefix
        .map(|(p, rest)| p.join(rest))
        .unwrap_or_else(|| root.join(name.replace('.', std::path::MAIN_SEPARATOR_STR)))
}

fn java_identifier(s: &str) -> bool {
    if super::scanner::is_keyword(s) {
        return false;
    }
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}
