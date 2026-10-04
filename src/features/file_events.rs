//! Port of jdt.ls `FileEventHandler.handleWillRenameFiles`
//! (`workspace/willRenameFiles`): renaming a `.java` file renames its
//! primary type, renaming a package folder renames the package (with its
//! subpackages), and moving files to another package folder runs the move
//! refactoring.  Like `ChangeUtil.mergeChanges(…, ignoreResourceChange)`
//! only the text edits are returned (the client moves the files), each
//! refactoring text change as one edit (`TextEditConverter` on the change's
//! `MultiTextEdit`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tower_lsp::lsp_types::{Position, Url};

use crate::analysis::dispatcher::Dispatcher;
use crate::document_store::DocumentStore;

/// UTF-16 line/column ↔ byte offset over a text.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { text, starts }
    }

    fn position(&self, offset: usize) -> Value {
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let col = self.text[self.starts[line]..offset].encode_utf16().count();
        json!({ "line": line, "character": col })
    }

    fn range(&self, start: usize, end: usize) -> Value {
        json!({ "start": self.position(start), "end": self.position(end) })
    }

    fn offset(&self, pos: &Value) -> usize {
        let line = pos["line"].as_u64().unwrap_or(0) as usize;
        let ch = pos["character"].as_u64().unwrap_or(0) as usize;
        let Some(&start) = self.starts.get(line) else { return self.text.len() };
        let end = self.text[start..].find('\n').map_or(self.text.len(), |i| start + i);
        let mut units = 0;
        for (i, c) in self.text[start..end].char_indices() {
            if units >= ch {
                return start + i;
            }
            units += c.len_utf16();
        }
        end
    }
}

/// A text change of one compilation unit: its edits merged into one, like
/// `TextEditConverter.visit(MultiTextEdit)`.
fn merged_edit(text: &str, mut edits: Vec<(usize, usize, String)>) -> Option<Value> {
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|e| e.0);
    let start = edits[0].0;
    let end = edits.iter().map(|e| e.1).max().unwrap_or(start);
    let mut content = String::new();
    let mut cur = start;
    for (s, e, t) in &edits {
        content.push_str(&text[cur..*s]);
        content.push_str(t);
        cur = *e;
    }
    content.push_str(&text[cur..end]);
    let lines = Lines::new(text);
    Some(json!({ "range": lines.range(start, end), "newText": content }))
}

fn document_edit(uri: &str, edit: Value) -> Value {
    json!({ "textDocument": { "uri": uri, "version": null }, "edits": [edit] })
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_java_file(name: &str) -> bool {
    name.ends_with(".java")
}

/// `BuildPathCommand.listSourcePaths`, longest path first.
fn source_paths(ws: &crate::project::Workspace) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = ws
        .projects
        .iter()
        .flat_map(|p| p.source_folders.iter().map(move |sf| (sf.path.clone(), p.name.clone())))
        .collect();
    out.sort_by(|a, b| b.0.to_string_lossy().len().cmp(&a.0.to_string_lossy().len()));
    out
}

/// `resolvePackage`: the package (and project) of a folder under a source path.
fn resolve_package(location: &Path, sources: &[(PathBuf, String)]) -> Option<(String, PathBuf, String)> {
    let (root, project) = sources.iter().find(|(s, _)| location.starts_with(s))?;
    let rel = location.strip_prefix(root).ok()?;
    let name = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join(".");
    Some((name, root.clone(), project.clone()))
}

fn path_of(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok().and_then(|u| crate::project::uri_to_path(&u))
}

fn uri_of(path: &Path) -> String {
    Url::from_file_path(path).map(|u| u.to_string()).unwrap_or_default()
}

/// `FileEventHandler.handleWillRenameFiles`.
pub async fn will_rename_files(dispatcher: &Dispatcher, store: &DocumentStore, files: &[(String, String)]) -> Option<Value> {
    if files.is_empty() {
        return None;
    }
    let paths: Vec<(PathBuf, PathBuf)> = files.iter().filter_map(|(o, n)| Some((path_of(o)?, path_of(n)?))).collect();
    let is_file_rename = |(o, n): &(PathBuf, PathBuf)| {
        (o.is_file() || n.is_file()) && is_java_file(&file_name(o)) && is_java_file(&file_name(n)) && o.parent() == n.parent()
    };
    let is_folder_rename = |(o, n): &(PathBuf, PathBuf)| o.is_dir() || n.is_dir();
    let is_move = |(o, n): &(PathBuf, PathBuf)| o.is_file() && is_java_file(&file_name(o)) && file_name(o) == file_name(n);

    let (mut renames, mut folders, mut moves) = (Vec::new(), Vec::new(), Vec::new());
    if paths.len() == 1 {
        let e = paths[0].clone();
        if is_file_rename(&e) {
            renames.push(e);
        } else if is_folder_rename(&e) {
            folders.push(e);
        } else if is_move(&e) {
            moves.push(e);
        }
    } else {
        moves = paths.iter().filter(|e| is_move(e)).cloned().collect();
    }
    if renames.is_empty() && folders.is_empty() && moves.is_empty() {
        return None;
    }
    let ws = dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    let sources = source_paths(&ws);
    if sources.is_empty() {
        return None;
    }

    let mut changes: Vec<Value> = Vec::new();
    for (old, new) in &renames {
        changes.extend(file_rename_changes(dispatcher, store, old, new).await);
    }
    for (old, new) in &folders {
        changes.extend(package_rename_changes(store, &ws, &sources, old, new));
    }
    if !moves.is_empty() {
        changes.extend(move_changes(store, &ws, &sources, &moves));
    }
    if changes.is_empty() {
        return None;
    }
    Some(json!({ "changes": {}, "documentChanges": changes }))
}

fn text_of(store: &DocumentStore, path: &Path) -> Option<String> {
    let uri = Url::from_file_path(path).ok()?;
    crate::features::source_text(store, &uri)
}

// ─── File rename: rename the primary type ───────────────────────────────────

/// Top-level type declarations of `text`: (name, name start offset).
fn top_level_types(text: &str) -> Vec<(String, usize)> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_java::language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .filter(|n| n.kind().ends_with("_declaration") && !matches!(n.kind(), "package_declaration" | "import_declaration"))
        .filter_map(|n| n.child_by_field_name("name"))
        .map(|name| (name.utf8_text(text.as_bytes()).unwrap_or("").to_owned(), name.start_byte()))
        .collect()
}

/// `computeFileRenameEdit`: rename the type named after the old file when
/// no type is named after the new one.
async fn file_rename_changes(dispatcher: &Dispatcher, store: &DocumentStore, old: &Path, new: &Path) -> Vec<Value> {
    let Some(text) = text_of(store, old) else { return Vec::new() };
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (old_type, new_type) = (stem(old), stem(new));
    let types = top_level_types(&text);
    if types.iter().any(|(n, _)| *n == new_type) {
        return Vec::new();
    }
    let Some((_, offset)) = types.iter().find(|(n, _)| *n == old_type) else { return Vec::new() };
    let Ok(uri) = Url::from_file_path(old) else { return Vec::new() };
    let lines = Lines::new(&text);
    let p = lines.position(*offset);
    let pos = Position::new(p["line"].as_u64().unwrap_or(0) as u32, p["character"].as_u64().unwrap_or(0) as u32);
    let client = crate::features::rename::RenameClient { resource_operations: true };
    let Ok(edit) = crate::features::rename::rename(dispatcher, &uri, &text, pos, &new_type, client, true).await else {
        return Vec::new();
    };
    let edit = serde_json::to_value(edit).unwrap_or_default();
    let mut out = Vec::new();
    for change in edit["documentChanges"].as_array().cloned().unwrap_or_default() {
        let (Some(doc_uri), Some(edits)) = (change["textDocument"]["uri"].as_str(), change["edits"].as_array()) else { continue };
        let Some(doc_text) = path_of(doc_uri).and_then(|p| text_of(store, &p)) else { continue };
        let lines = Lines::new(&doc_text);
        let fine: Vec<(usize, usize, String)> = edits
            .iter()
            .map(|e| (lines.offset(&e["range"]["start"]), lines.offset(&e["range"]["end"]), e["newText"].as_str().unwrap_or("").to_owned()))
            .collect();
        if let Some(e) = merged_edit(&doc_text, fine) {
            out.push(document_edit(doc_uri, e));
        }
    }
    out
}

// ─── Compilation unit structure ─────────────────────────────────────────────

struct UnitInfo {
    /// Package declaration: (name start, name end, statement end) offsets.
    package: Option<(usize, usize, usize)>,
    package_name: String,
    /// Import declarations: (start, end, name, is_static, on_demand).
    imports: Vec<(usize, usize, String, bool, bool)>,
    /// Start of the first type declaration.
    first_type: Option<usize>,
    /// Qualified type references outside imports: (start, end, text).
    qualified: Vec<(usize, usize, String)>,
    /// Simple identifiers used as type names.
    simple_types: BTreeSet<String>,
}

fn unit_info(text: &str) -> UnitInfo {
    let mut info = UnitInfo { package: None, package_name: String::new(), imports: Vec::new(), first_type: None, qualified: Vec::new(), simple_types: BTreeSet::new() };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_java::language()).is_err() {
        return info;
    }
    let Some(tree) = parser.parse(text, None) else { return info };
    let root = tree.root_node();
    let src = text.as_bytes();
    let mut cursor = root.walk();
    for n in root.children(&mut cursor) {
        match n.kind() {
            "package_declaration" => {
                let mut c = n.walk();
                if let Some(name) = n.named_children(&mut c).find(|x| matches!(x.kind(), "scoped_identifier" | "identifier")) {
                    info.package = Some((name.start_byte(), name.end_byte(), n.end_byte()));
                    info.package_name = name.utf8_text(src).unwrap_or("").to_owned();
                };
            }
            "import_declaration" => {
                let t = n.utf8_text(src).unwrap_or("");
                let is_static = t.split_whitespace().nth(1) == Some("static");
                let on_demand = t.trim_end_matches(';').trim_end().ends_with('*');
                let mut c = n.walk();
                let name = n
                    .named_children(&mut c)
                    .find(|x| matches!(x.kind(), "scoped_identifier" | "identifier"))
                    .map(|x| x.utf8_text(src).unwrap_or("").to_owned())
                    .unwrap_or_default();
                info.imports.push((n.start_byte(), n.end_byte(), name, is_static, on_demand));
            }
            k if k.ends_with("_declaration") => {
                info.first_type.get_or_insert(n.start_byte());
                collect_types(n, src, &mut info);
            }
            _ => {}
        }
    }
    info
}

fn collect_types(node: tree_sitter::Node, src: &[u8], info: &mut UnitInfo) {
    match node.kind() {
        "scoped_type_identifier" | "scoped_identifier" => {
            info.qualified.push((node.start_byte(), node.end_byte(), node.utf8_text(src).unwrap_or("").to_owned()));
            return;
        }
        "type_identifier" => {
            info.simple_types.insert(node.utf8_text(src).unwrap_or("").to_owned());
        }
        "identifier" => {
            // `B.foo()`, `new B()` receivers and other simple names.
            info.simple_types.insert(node.utf8_text(src).unwrap_or("").to_owned());
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect_types(child, src, info);
    }
}

/// The source files of the workspace's source folders.
fn source_units(ws: &crate::project::Workspace) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = ws.java_files().into_keys().collect();
    out.sort();
    out
}

/// Line delimiter used by `text` (`TextUtilities.getDefaultLineDelimiter`).
fn delimiter(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => "\r\n",
        Some(_) => "\n",
        None => {
            if cfg!(windows) {
                "\r\n"
            } else {
                "\n"
            }
        }
    }
}

// ─── Package (folder) rename ────────────────────────────────────────────────

/// `computePackageRenameEdit`: `RenameSupport` on the package fragment with
/// `setRenameSubpackages(true)`.
fn package_rename_changes(
    store: &DocumentStore,
    ws: &crate::project::Workspace,
    sources: &[(PathBuf, String)],
    old: &Path,
    new: &Path,
) -> Vec<Value> {
    let Some((old_pkg, root, _)) = resolve_package(old, sources) else { return Vec::new() };
    if old_pkg.is_empty() || !old.is_dir() {
        return Vec::new();
    }
    let Some((new_pkg, _, _)) = resolve_package(new, sources) else { return Vec::new() };
    let renamed = |p: &str| -> Option<String> {
        if p == old_pkg {
            Some(new_pkg.clone())
        } else {
            p.strip_prefix(&format!("{old_pkg}.")).map(|rest| format!("{new_pkg}.{rest}"))
        }
    };
    // Packages of the source folder (folders holding compilation units).
    let units = source_units(ws);
    let mut packages: BTreeSet<String> = BTreeSet::new();
    for u in &units {
        if let Some(dir) = u.parent() {
            if let Some((p, _, _)) = resolve_package(dir, sources) {
                packages.insert(p);
            }
        }
    }
    // The package part of a dotted name (longest known package prefix).
    let package_part = |name: &str| -> Option<String> {
        let segs: Vec<&str> = name.split('.').collect();
        (1..=segs.len()).rev().map(|n| segs[..n].join(".")).find(|p| packages.contains(p))
    };

    let mut texts: HashMap<PathBuf, String> = HashMap::new();
    let mut reference_changes: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut declaration_changes: BTreeMap<(String, String), Value> = BTreeMap::new();
    for unit in &units {
        let Some(text) = text_of(store, unit) else { continue };
        let info = unit_info(&text);
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for (start, end, name, is_static, on_demand) in &info.imports {
            let pkg = if *on_demand && !*is_static { Some(name.clone()) } else { package_part(name) };
            let Some(pkg) = pkg else { continue };
            let Some(new_name) = renamed(&pkg) else { continue };
            let rewritten = format!("{new_name}{}", &name[pkg.len()..]);
            let decl = &text[*start..*end];
            let new_decl = decl.replacen(name.as_str(), &rewritten, 1);
            edits.push((*start, *end, new_decl));
        }
        for (start, end, name) in &info.qualified {
            let Some(pkg) = package_part(name).filter(|p| p.len() < name.len()) else { continue };
            if let Some(new_name) = renamed(&pkg) {
                edits.push((*start, *start + pkg.len(), new_name));
                let _ = end;
            }
        }
        let uri = uri_of(unit);
        let key = (file_name(unit), uri.clone());
        if let Some(e) = merged_edit(&text, edits) {
            reference_changes.insert(key.clone(), document_edit(&uri, e));
        }
        if unit.starts_with(&root) {
            if let (Some((s, e, _)), Some(new_name)) = (info.package, renamed(&info.package_name)) {
                if let Some(edit) = merged_edit(&text, vec![(s, e, new_name)]) {
                    declaration_changes.insert((info.package_name.clone() + "/" + &file_name(unit), uri.clone()), document_edit(&uri, edit));
                }
            }
        }
        texts.insert(unit.clone(), text);
    }
    reference_changes.into_values().chain(declaration_changes.into_values()).collect()
}

// ─── Move to another package ────────────────────────────────────────────────

/// `computeMoveEdit`: the move refactoring of compilation units into the
/// destination package (`MoveHandler.move` with reference updates).
fn move_changes(
    store: &DocumentStore,
    ws: &crate::project::Workspace,
    sources: &[(PathBuf, String)],
    moves: &[(PathBuf, PathBuf)],
) -> Vec<Value> {
    // `ResourceUtils.getLongestCommonPath` of the new locations.
    let parents: Vec<PathBuf> = moves.iter().filter_map(|(_, n)| n.parent().map(Path::to_path_buf)).collect();
    let Some(destination) = parents.first().cloned() else { return Vec::new() };
    let destination = parents.iter().fold(destination, |acc, p| {
        let mut common = PathBuf::new();
        for (a, b) in acc.components().zip(p.components()) {
            if a != b {
                break;
            }
            common.push(a.as_os_str());
        }
        common
    });
    if moves.iter().any(|(o, n)| destination.join(file_name(o)) != *n) {
        return Vec::new();
    }
    let Some((dest_pkg, _, _)) = resolve_package(&destination, sources) else { return Vec::new() };

    // Moved units: their package and the types they declare.
    struct Moved {
        path: PathBuf,
        text: String,
        info: UnitInfo,
        types: Vec<String>,
    }
    let mut moved: Vec<Moved> = Vec::new();
    for (old, _) in moves {
        if !old.is_file() || ws.project_for_path(old).is_none() {
            continue;
        }
        let Some(text) = text_of(store, old) else { continue };
        let info = unit_info(&text);
        let types = top_level_types(&text).into_iter().map(|(n, _)| n).collect();
        moved.push(Moved { path: old.clone(), text, info, types });
    }
    if moved.is_empty() {
        return Vec::new();
    }
    let moved_paths: Vec<&PathBuf> = moved.iter().map(|m| &m.path).collect();
    // Moved types by old qualified name.
    let mut moved_types: Vec<(String, String, String)> = Vec::new(); // (old package, simple, old fqn)
    for m in &moved {
        for t in &m.types {
            let fqn = if m.info.package_name.is_empty() { t.clone() } else { format!("{}.{t}", m.info.package_name) };
            moved_types.push((m.info.package_name.clone(), t.clone(), fqn));
        }
    }

    let mut out: Vec<Value> = Vec::new();
    // Other units referencing the moved types.
    let mut refs: BTreeMap<(String, String), Value> = BTreeMap::new();
    for unit in source_units(ws) {
        if moved_paths.contains(&&unit) {
            continue;
        }
        let Some(text) = text_of(store, &unit) else { continue };
        let info = unit_info(&text);
        let d = delimiter(&text);
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let mut needed: Vec<String> = Vec::new();
        for (old_pkg, simple, fqn) in &moved_types {
            let new_fqn = if dest_pkg.is_empty() { simple.clone() } else { format!("{dest_pkg}.{simple}") };
            if let Some(imp) = info.imports.iter().find(|i| i.2 == *fqn && !i.3) {
                let decl = &text[imp.0..imp.1];
                edits.push((imp.0, imp.1, decl.replacen(fqn.as_str(), &new_fqn, 1)));
            } else if info.package_name == *old_pkg && info.package_name != dest_pkg && info.simple_types.contains(simple) {
                needed.push(new_fqn);
            }
        }
        if !needed.is_empty() {
            needed.sort();
            let imports: String = needed.iter().map(|n| format!("import {n};{d}")).collect();
            match (info.imports.last(), info.package, info.first_type) {
                (Some(last), _, _) => edits.push((last.1, last.1, format!("{d}{}", imports.trim_end_matches(d)))),
                (None, Some((_, _, pkg_end)), Some(first)) => {
                    let first_line = text[..first].rfind('\n').map_or(first, |i| i + 1).max(pkg_end);
                    edits.push((pkg_end, first_line, format!("{d}{d}{imports}{d}")));
                }
                (None, None, Some(first)) => edits.push((first, first, format!("{imports}{d}"))),
                _ => {}
            }
        }
        let uri = uri_of(&unit);
        if let Some(e) = merged_edit(&text, edits) {
            refs.insert((file_name(&unit), uri.clone()), document_edit(&uri, e));
        }
    }
    out.extend(refs.into_values());
    // The moved units: package declaration (and imports of the types left
    // behind in their old package).
    for m in &moved {
        let d = delimiter(&m.text);
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        match m.info.package {
            Some((s, e, _)) if !dest_pkg.is_empty() => edits.push((s, e, dest_pkg.clone())),
            Some((_, _, end)) => edits.push((0, end, String::new())),
            None if !dest_pkg.is_empty() => edits.push((0, 0, format!("package {dest_pkg};{d}"))),
            None => {}
        }
        if let Some(e) = merged_edit(&m.text, edits) {
            out.push(document_edit(&uri_of(&m.path), e));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_edits_like_multi_text_edit() {
        let text = "a ObjectA x = new ObjectA();";
        let e = merged_edit(text, vec![(2, 9, "ObjectA1".into()), (18, 25, "ObjectA1".into())]).unwrap();
        assert_eq!(e["newText"], "ObjectA1 x = new ObjectA1");
        assert_eq!(e["range"]["start"]["character"], 2);
        assert_eq!(e["range"]["end"]["character"], 25);
    }

    #[test]
    fn unit_structure() {
        let info = unit_info("package a.b;\nimport c.D;\npublic class X { D d; }\n");
        assert_eq!("a.b", info.package_name);
        assert_eq!(1, info.imports.len());
        assert_eq!("c.D", info.imports[0].2);
        assert!(info.simple_types.contains("D"));
    }
}
