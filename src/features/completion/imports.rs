//! Port of JDT's `ImportRewrite` (add-only use, as jdt.ls completion uses it:
//! `ImportRewrite.create(cu, true)` with the jdt.ls import order and
//! on-demand thresholds) including `ImportRewriteAnalyzer`, `ImportEditor`
//! and `OrderPreservingImportAdder`, and of jdt.ls `TextEditConverter` for
//! the resulting `MultiTextEdit` (one edit covering the changed region).

use super::doc::Doc;
use crate::features::scanner::{scan, TokKind};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

const NORMAL_PREFIX: char = 'n';
const STATIC_PREFIX: char = 's';

pub const KIND_TYPE: i32 = 1;
pub const KIND_STATIC_FIELD: i32 = 2;
pub const KIND_STATIC_METHOD: i32 = 3;

pub const RES_NAME_FOUND: i32 = 1;
pub const RES_NAME_UNKNOWN: i32 = 2;
pub const RES_NAME_CONFLICT: i32 = 3;

// ─── Compilation unit structure ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ImportDecl {
    name: ImportName,
    start: usize,
    end: usize,
}

/// The top-level structure of a compilation unit (UTF-16 offsets).
#[derive(Debug, Clone, Default)]
pub struct CuStructure {
    pub package_name: String,
    package: Option<(usize, usize)>,
    imports: Vec<ImportDecl>,
    types: Vec<(usize, usize)>,
    /// Top-level type names (in declaration order).
    pub type_names: Vec<String>,
    comments: Vec<(usize, usize)>,
    /// Comments not attached to declarations (`Comment.getParent() == null`).
    free_comments: Vec<(usize, usize)>,
    len: usize,
    line_starts: Vec<usize>,
    text: Vec<u16>,
}

impl CuStructure {
    pub fn parse(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        // byte → utf16 offset map
        let mut b2u = vec![0usize; text.len() + 1];
        let mut u = 0usize;
        for (b, c) in text.char_indices() {
            b2u[b] = u;
            u += c.len_utf16();
        }
        b2u[text.len()] = u;
        let fix = |b: usize| b2u[b];
        let toks = scan(text);
        let mut st = CuStructure { len: units.len(), ..Default::default() };
        let doc = Doc::new(text);
        st.line_starts = (0..doc.line_count()).map(|l| doc.line_offset(l)).collect();
        st.text = units;
        let mut i = 0;
        let mut depth = 0i32;
        let mut pending_javadoc: Option<(usize, usize)> = None;
        let mut type_start: Option<usize> = None;
        while i < toks.len() {
            let t = toks[i];
            let s = t.text(text);
            if t.is_comment() {
                let r = (fix(t.start), fix(t.end));
                st.comments.push(r);
                if depth == 0 && t.kind == TokKind::Javadoc && type_start.is_none() {
                    if let Some(prev) = pending_javadoc.take() {
                        st.free_comments.push(prev);
                    }
                    pending_javadoc = Some(r);
                } else if t.kind != TokKind::Javadoc {
                    st.free_comments.push(r);
                }
                i += 1;
                continue;
            }
            if depth == 0 && type_start.is_none() {
                if s == "package" {
                    let start = pending_javadoc.take().map(|j| j.0).unwrap_or(fix(t.start));
                    let mut j = i + 1;
                    let mut name = String::new();
                    while j < toks.len() && toks[j].text(text) != ";" {
                        if !toks[j].is_comment() {
                            name.push_str(toks[j].text(text));
                        }
                        j += 1;
                    }
                    let end = if j < toks.len() { fix(toks[j].end) } else { fix(toks[j - 1].end) };
                    st.package = Some((start, end));
                    st.package_name = name;
                    i = j + 1;
                    continue;
                }
                if s == "import" {
                    if let Some(prev) = pending_javadoc.take() {
                        st.free_comments.push(prev);
                    }
                    let mut j = i + 1;
                    let mut is_static = false;
                    let mut is_module = false;
                    let mut name = String::new();
                    while j < toks.len() && toks[j].text(text) != ";" {
                        let w = toks[j].text(text);
                        if toks[j].is_comment() {
                            st.comments.push((fix(toks[j].start), fix(toks[j].end)));
                            st.free_comments.push((fix(toks[j].start), fix(toks[j].end)));
                        } else if w == "static" && name.is_empty() {
                            is_static = true;
                        } else if w == "module" && name.is_empty() && !is_static {
                            is_module = true;
                        } else {
                            name.push_str(w);
                        }
                        j += 1;
                    }
                    let end = if j < toks.len() { fix(toks[j].end) } else { fix(toks[j - 1].end) };
                    let iname = if let Some(container) = name.strip_suffix(".*") {
                        ImportName::on_demand(is_static, container)
                    } else {
                        ImportName::create(is_static, is_module, &name)
                    };
                    st.imports.push(ImportDecl { name: iname, start: fix(t.start), end });
                    i = j + 1;
                    continue;
                }
                if s == ";" {
                    i += 1;
                    continue;
                }
                type_start = Some(pending_javadoc.take().map(|j| j.0).unwrap_or(fix(t.start)));
            }
            if s == "{" {
                depth += 1;
            } else if s == "}" {
                depth -= 1;
                if depth == 0 {
                    if let Some(ts) = type_start.take() {
                        st.types.push((ts, fix(t.end)));
                    }
                }
            } else if depth == 0 {
                if matches!(s, "class" | "interface" | "enum" | "record") || (s == "@" && toks.get(i + 1).is_some_and(|n| n.text(text) == "interface")) {
                    if let Some(n) = toks.get(if s == "@" { i + 2 } else { i + 1 }) {
                        if n.kind == TokKind::Ident || n.kind == TokKind::Keyword {
                            if s != "@" {
                                st.type_names.push(n.text(text).to_owned());
                            }
                        }
                    }
                }
                if s == "@" && toks.get(i + 1).is_some_and(|n| n.text(text) == "interface") {
                    if let Some(n) = toks.get(i + 2) {
                        st.type_names.push(n.text(text).to_owned());
                    }
                }
            }
            i += 1;
        }
        if let Some(ts) = type_start {
            // Unterminated type: runs to the end.
            st.types.push((ts, st.len));
        }
        if let Some(p) = pending_javadoc {
            st.free_comments.push(p);
        }
        st.comments.sort();
        st.comments.dedup();
        st.free_comments.sort();
        st.free_comments.dedup();
        st
    }

    pub fn has_package(&self) -> bool {
        self.package.is_some()
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    fn is_whitespace_gap(&self, from: usize, to: usize) -> Option<usize> {
        // Some(number of '\n') when text[from..to] is whitespace only.
        let mut n = 0;
        for &c in &self.text[from.min(self.len)..to.min(self.len)] {
            if c == b'\n' as u16 {
                n += 1;
            }
            let ch = char::from_u32(c as u32).unwrap_or('x');
            if !ch.is_whitespace() {
                return None;
            }
        }
        Some(n)
    }

    /// Top-level DOM children in order: package, imports, types.
    fn children(&self) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        if let Some(p) = self.package {
            v.push(p);
        }
        v.extend(self.imports.iter().map(|i| (i.start, i.end)));
        v.extend(self.types.iter().copied());
        v
    }

    /// `DefaultCommentMapper` extended range of top-level child `index`.
    fn extended(&self, index: usize) -> (usize, usize) {
        let children = self.children();
        let (start, end) = children[index];
        let previous_end = if index == 0 { 0 } else { children[index - 1].1 };
        let next_start = if index + 1 < children.len() { children[index + 1].0 } else { self.len };
        let last_child = index + 1 == children.len();
        (self.leading(start, previous_end), self.trailing(start, end, next_start, last_child) + 1)
    }

    fn leading(&self, node_start: usize, previous_end: usize) -> usize {
        let prev_end_line = self.line_of(previous_end);
        let node_start_line = self.line_of(node_start);
        // last comment starting before node_start
        let Some(mut idx) = self.comments.iter().rposition(|c| c.0 < node_start) else { return node_start };
        let end_idx = idx as isize;
        let mut start_idx: isize = -1;
        let mut previous_start = node_start;
        loop {
            if previous_start < previous_end {
                break;
            }
            let (cs, ce) = self.comments[idx];
            let end = ce - 1;
            let comment_line = self.line_of(cs);
            if end <= previous_end || (comment_line == prev_end_line && comment_line != node_start_line) {
                break;
            } else if end + 1 < previous_start {
                match self.is_whitespace_gap(end + 1, previous_start) {
                    None => {
                        if idx as isize == end_idx {
                            return node_start;
                        }
                        break;
                    }
                    Some(lines) if lines > 1 => break,
                    _ => {}
                }
            }
            previous_start = cs;
            start_idx = idx as isize;
            if idx == 0 {
                break;
            }
            idx -= 1;
        }
        if start_idx != -1 {
            let mut start_idx = start_idx as usize;
            let comment_start = self.comments[start_idx].0;
            if previous_end < comment_start && prev_end_line != node_start_line {
                let last_token_line = prev_end_line;
                while start_idx < self.comments.len()
                    && last_token_line == self.line_of(self.comments[start_idx].0)
                    && node_start_line != last_token_line
                {
                    start_idx += 1;
                }
            }
            if start_idx as isize <= end_idx {
                return self.comments[end_idx as usize].0.min(self.comments[start_idx].0);
            }
        }
        node_start
    }

    /// Returns the inclusive extended end.
    fn trailing(&self, node_start: usize, node_end_excl: usize, next_start: usize, last_child: bool) -> usize {
        let _ = node_start;
        let node_end = node_end_excl - 1;
        if node_end == next_start {
            return node_end;
        }
        let node_end_line = self.line_of(node_end);
        let Some(start) = self.comments.iter().position(|c| c.0 > node_end) else { return node_end };
        let mut idx = start;
        let mut end_idx: isize = -1;
        let mut previous_end = node_end + 1;
        let mut same_line_idx: isize = -1;
        while idx < self.comments.len() {
            let (cs, ce) = self.comments[idx];
            if cs >= next_start {
                break;
            } else if previous_end < cs {
                match self.is_whitespace_gap(previous_end, cs) {
                    None => {
                        if idx == start {
                            return node_end;
                        }
                        break;
                    }
                    Some(lines) if lines > 1 => break,
                    _ => {}
                }
            }
            if self.line_of(cs) == node_end_line {
                same_line_idx = idx as isize;
            }
            previous_end = ce;
            end_idx = idx as isize;
            idx += 1;
        }
        if end_idx != -1 {
            if !last_child {
                let next_line = self.line_of(next_start);
                let previous_line = self.line_of(previous_end);
                if next_line as isize - previous_line as isize <= 1 {
                    if same_line_idx == -1 {
                        return node_end;
                    }
                    end_idx = same_line_idx;
                }
            }
            return self.comments[end_idx as usize].1 - 1;
        }
        node_end
    }
}

// ─── Import names ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportName {
    pub is_static: bool,
    pub is_module: bool,
    pub container: String,
    pub simple: String,
    pub qualified: String,
}

impl ImportName {
    pub fn create(is_static: bool, is_module: bool, qualified: &str) -> Self {
        let container = super::signature::get_qualifier(qualified);
        let simple = super::signature::get_simple_name(qualified);
        Self::new(is_static, is_module, container, simple)
    }
    fn on_demand(is_static: bool, container: &str) -> Self {
        Self::new(is_static, false, container.to_owned(), "*".to_owned())
    }
    fn new(is_static: bool, is_module: bool, container: String, simple: String) -> Self {
        let qualified = if container.is_empty() { simple.clone() } else { format!("{container}.{simple}") };
        ImportName { is_static, is_module, container, simple, qualified }
    }
    fn is_on_demand(&self) -> bool {
        self.simple == "*"
    }
    fn container_on_demand(&self) -> ImportName {
        if self.is_on_demand() {
            self.clone()
        } else {
            ImportName::on_demand(self.is_static, &self.container)
        }
    }
}

// ─── Import group ordering ───────────────────────────────────────────────────

struct GroupComparator {
    types: BTreeMap<String, (usize, Option<String>)>,
    statics: BTreeMap<String, (usize, Option<String>)>,
}

fn is_whole_segment_prefix(prefix: &str, name: &str) -> bool {
    if !name.starts_with(prefix) {
        return false;
    }
    prefix.is_empty() || name.len() == prefix.len() || name.as_bytes()[prefix.len()] == b'.'
}

impl GroupComparator {
    fn new(order: &[String]) -> Self {
        let mut order: Vec<String> = order.to_vec();
        let needs_type = !order.iter().any(|o| o.is_empty());
        let needs_static = !order.iter().any(|o| o == "#");
        if needs_static {
            order.insert(0, "#".to_owned());
        }
        if needs_type {
            order.push(String::new());
        }
        let mut types: HashMap<String, usize> = HashMap::new();
        let mut statics: HashMap<String, usize> = HashMap::new();
        for (i, g) in order.iter().enumerate() {
            if let Some(s) = g.strip_prefix('#') {
                statics.insert(s.to_owned(), i);
            } else {
                types.insert(g.clone(), i);
            }
        }
        GroupComparator { types: Self::map(types), statics: Self::map(statics) }
    }

    fn map(mut groups: HashMap<String, usize>) -> BTreeMap<String, (usize, Option<String>)> {
        if groups.is_empty() {
            groups.insert(String::new(), 0);
        }
        let mut names: Vec<String> = groups.keys().cloned().collect();
        names.sort();
        let mut prefixing: Vec<String> = Vec::new();
        let mut out = BTreeMap::new();
        for name in names {
            while prefixing.last().is_some_and(|p| !is_whole_segment_prefix(p, &name)) {
                prefixing.pop();
            }
            let prefix = prefixing.last().cloned();
            out.insert(name.clone(), (groups[&name], prefix));
            prefixing.push(name);
        }
        out
    }

    fn position(&self, n: &ImportName) -> usize {
        let name = if n.is_on_demand() { &n.container } else { &n.qualified };
        let groups = if n.is_static { &self.statics } else { &self.types };
        let mut key = groups.range(..=name.clone()).next_back().map(|(k, _)| k.clone()).unwrap_or_default();
        loop {
            let (index, prefix) = &groups[&key];
            if is_whole_segment_prefix(&key, name) {
                return *index;
            }
            match prefix {
                Some(p) => key = p.clone(),
                None => return *index,
            }
        }
    }

    fn compare(&self, a: &ImportName, b: &ImportName) -> Ordering {
        self.position(a).cmp(&self.position(b))
    }

    /// `ImportComparator` (container sorting: by package and containing type).
    fn compare_imports(&self, a: &ImportName, b: &ImportName) -> Ordering {
        self.compare(a, b)
            .then_with(|| a.container.cmp(&b.container))
            .then_with(|| a.qualified.cmp(&b.qualified))
    }
}

fn count_matching_prefix_segments(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i <= a.len() && i <= b.len() {
        let end_a = i == a.len() || a[i] == b'.';
        let end_b = i == b.len() || b[i] == b'.';
        if end_a && end_b {
            n += 1;
        } else if end_a || end_b {
            break;
        } else if a[i] != b[i] {
            break;
        }
        i += 1;
    }
    n
}

// ─── ImportRewrite ───────────────────────────────────────────────────────────

/// Types visible in on-demand / implicit containers, for conflict detection
/// (`TypeConflictingSimpleNameFinder`): container → simple type names.
pub type ContainerTypes = HashMap<String, HashSet<String>>;

#[derive(Debug, Clone)]
pub struct ImportRewrite {
    cu: std::sync::Arc<CuStructure>,
    existing: Vec<String>,
    added: Vec<String>,
    kinds: HashMap<String, i32>,
    pub import_order: Vec<String>,
    pub on_demand_threshold: i64,
    pub static_on_demand_threshold: i64,
}

impl ImportRewrite {
    /// `ImportRewrite.create(cu, true)` + jdt.ls settings (`TypeProposalUtils.createImportRewrite`).
    pub fn create(cu: std::sync::Arc<CuStructure>, import_order: Vec<String>, threshold: i64, static_threshold: i64) -> Self {
        let existing = cu
            .imports
            .iter()
            .map(|i| format!("{}{}", if i.name.is_static { STATIC_PREFIX } else { NORMAL_PREFIX }, i.name.qualified))
            .collect();
        ImportRewrite {
            cu,
            existing,
            added: Vec::new(),
            kinds: HashMap::new(),
            import_order,
            on_demand_threshold: threshold,
            static_on_demand_threshold: static_threshold,
        }
    }

    pub fn added_imports(&self) -> Vec<String> {
        self.added.iter().filter(|a| a.starts_with(NORMAL_PREFIX)).map(|a| a[1..].to_owned()).collect()
    }

    /// `findInImports` (default context).
    pub fn find_in_imports(&self, qualifier: &str, name: &str, kind: i32) -> i32 {
        let allow_ambiguity = kind == KIND_STATIC_METHOD || name == "*";
        let prefix = if kind == KIND_TYPE { NORMAL_PREFIX } else { STATIC_PREFIX };
        for curr in self.existing.iter().rev() {
            let res = compare_import(prefix, qualifier, name, curr);
            if res != RES_NAME_UNKNOWN && (!allow_ambiguity || res == RES_NAME_FOUND) {
                if prefix != STATIC_PREFIX {
                    return res;
                }
                let curr_kind = self.kinds.get(&curr[1..]);
                if curr_kind.is_some() && curr_kind == self.kinds.get(&format!("{qualifier}.{name}")) {
                    return res;
                }
            }
        }
        RES_NAME_UNKNOWN
    }

    /// `addImport(qualifiedTypeName, context)`; `context` overrides the
    /// default context's answer when it returns something.
    pub fn add_import_with(&mut self, qualified: &str, context: Option<&dyn Fn(&ImportRewrite, &str, &str, i32) -> i32>) -> String {
        if let Some(i) = qualified.find('<') {
            return self.internal_add(&qualified[..i], context) + &qualified[i..];
        }
        if let Some(i) = qualified.find('[') {
            return self.internal_add(&qualified[..i], context) + &qualified[i..];
        }
        self.internal_add(qualified, context)
    }

    pub fn add_import(&mut self, qualified: &str) -> String {
        self.add_import_with(qualified, None)
    }

    fn internal_add(&mut self, full: &str, context: Option<&dyn Fn(&ImportRewrite, &str, &str, i32) -> i32>) -> String {
        let (container, name) = match full.rfind('.') {
            Some(i) => (&full[..i], &full[i + 1..]),
            None => ("", full),
        };
        if container.is_empty()
            && matches!(name, "byte" | "short" | "char" | "int" | "long" | "float" | "double" | "boolean" | "void")
        {
            return full.to_owned();
        }
        let res = match context {
            Some(c) => c(self, container, name, KIND_TYPE),
            None => self.find_in_imports(container, name, KIND_TYPE),
        };
        if res == RES_NAME_CONFLICT {
            return full.to_owned();
        }
        if res == RES_NAME_UNKNOWN {
            self.add_entry(format!("{NORMAL_PREFIX}{full}"));
        }
        name.to_owned()
    }

    /// `addStaticImport(declaringTypeName, simpleName, isField, context)`.
    pub fn add_static_import(&mut self, declaring: &str, simple: &str, is_field: bool) -> String {
        let key = format!("{declaring}.{simple}");
        if !declaring.contains('.') {
            return key;
        }
        let kind = if is_field { KIND_STATIC_FIELD } else { KIND_STATIC_METHOD };
        self.kinds.insert(key.clone(), kind);
        let res = self.find_in_imports(declaring, simple, kind);
        if res == RES_NAME_CONFLICT {
            return key;
        }
        if res == RES_NAME_UNKNOWN {
            self.add_entry(format!("{STATIC_PREFIX}{key}"));
        }
        simple.to_owned()
    }

    fn add_entry(&mut self, entry: String) {
        self.existing.push(entry.clone());
        self.added.push(entry);
    }

    pub fn has_recorded_changes(&self) -> bool {
        !self.added.is_empty()
    }

    /// `rewriteImports` converted by `TextEditConverter`: `None` when there is
    /// no change, else (offset, length, newText).
    pub fn rewrite(&self, container_types: &ContainerTypes, line_delimiter: &str, blank_lines_between_groups: usize, space_before_semicolon: bool) -> Option<(usize, usize, String)> {
        if self.added.is_empty() {
            return None;
        }
        let cu = &self.cu;
        let originals: Vec<OriginalEntry> = read_original_imports(cu);
        let original_names: Vec<ImportName> = originals.iter().map(|o| o.name.clone()).collect();
        let original_set: HashSet<ImportName> = original_names.iter().cloned().collect();
        let to_add: Vec<ImportName> = {
            let mut v: Vec<ImportName> = Vec::new();
            for a in &self.added {
                let n = ImportName::create(a.starts_with(STATIC_PREFIX), false, &a[1..]);
                if !v.contains(&n) {
                    v.push(n);
                }
            }
            v
        };
        let groups = GroupComparator::new(&self.import_order);
        let mut implicit: HashSet<String> = HashSet::new();
        implicit.insert("java.lang".to_owned());
        implicit.insert(cu.package_name.clone());

        // computeImportOrder
        let mut all: HashSet<ImportName> = original_set.clone();
        all.extend(to_add.iter().cloned());
        let touched: HashSet<ImportName> = to_add.iter().filter(|a| !a.is_module).map(|a| a.container_on_demand()).collect();
        let empty: HashSet<String> = HashSet::new();
        let candidates = self.reductions(&all, &touched, &empty, &empty);
        let mut type_on_demand: HashSet<String> = candidates.iter().filter(|c| !c.0.is_static).map(|c| c.0.container.clone()).collect();
        let mut static_on_demand: HashSet<String> = candidates.iter().filter(|c| c.0.is_static).map(|c| c.0.container.clone()).collect();
        if !type_on_demand.is_empty() {
            type_on_demand.extend(all.iter().filter(|i| i.is_on_demand() && !i.is_static).map(|i| i.container.clone()));
            type_on_demand.extend(implicit.iter().cloned());
            type_on_demand.extend(static_on_demand.iter().cloned());
        }
        if !static_on_demand.is_empty() {
            static_on_demand.extend(all.iter().filter(|i| i.is_on_demand() && i.is_static).map(|i| i.container.clone()));
        }
        let type_conflicts = find_conflicts(&all, false, &type_on_demand, container_types);
        let static_conflicts = if static_on_demand.is_empty() { HashSet::new() } else { HashSet::new() };
        let implicit_imports: HashSet<ImportName> = to_add
            .iter()
            .filter(|a| implicit.contains(&a.container) && !type_conflicts.contains(&a.simple))
            .cloned()
            .collect();
        let without_implicits: HashSet<ImportName> = all.iter().filter(|i| !implicit_imports.contains(i)).cloned().collect();
        let reductions = self.reductions(&without_implicits, &touched, &type_conflicts, &static_conflicts);
        // computeDelta
        let mut removals: Vec<ImportName> = implicit_imports.iter().cloned().collect();
        let mut additions: Vec<ImportName> = to_add.iter().filter(|a| !removals.contains(a)).cloned().collect();
        for (container, reducible) in &reductions {
            additions.retain(|a| !reducible.contains(a));
            removals.extend(reducible.iter().cloned());
            if !additions.contains(container) {
                additions.push(container.clone());
            }
            removals.retain(|r| r != container);
        }
        let removal_set: HashSet<ImportName> = removals.into_iter().collect();
        let with_removals: Vec<ImportName> = original_names.iter().filter(|n| !removal_set.contains(n)).cloned().collect();
        let addition_set: Vec<ImportName> = additions;
        let result = order_preserving_add(&with_removals, &addition_set, &groups);

        // ImportEditor
        let site = rewrite_site(cu, &originals);
        let mut edits: Vec<Edit> = Vec::new();
        let delim2 = format!("{line_delimiter}{line_delimiter}");
        let delimiter = |n: usize| -> String { line_delimiter.repeat(n.max(1)) };
        let write = |n: &ImportName| -> String {
            let mut s = String::from("import ");
            if n.is_static {
                s.push_str("static ");
            }
            s.push_str(&n.qualified);
            if space_before_semicolon {
                s.push(' ');
            }
            s.push(';');
            s
        };
        if result.is_empty() {
            if !originals.is_empty() {
                let ws = if site.has_preceding {
                    delimiter(if site.has_succeeding { 2 } else { 1 })
                } else {
                    String::new()
                };
                edits.push(Edit::replace(site.surrounding.0, site.surrounding.1, ws));
            }
        } else {
            let lines_between = blank_lines_between_groups + 1;
            if originals.is_empty() {
                let import_edits = determine_edits(site.surrounding, &result, &originals, &groups, lines_between, line_delimiter, &write);
                if site.has_preceding {
                    edits.push(Edit::insert(site.surrounding.0, delim2.clone()));
                }
                edits.extend(import_edits);
                let succ = delimiter(if site.has_succeeding { 2 } else { 1 });
                edits.push(Edit::insert(site.surrounding.0, succ));
            } else {
                let region = site.imports.unwrap();
                edits.extend(determine_edits(region, &result, &originals, &groups, lines_between, line_delimiter, &write));
            }
        }
        if edits.is_empty() {
            return None;
        }
        // TextEditConverter.visit(MultiTextEdit): one edit covering all children.
        let start = edits.iter().map(|e| e.offset).min().unwrap();
        let end = edits.iter().map(|e| e.offset + e.length).max().unwrap();
        let mut sorted: Vec<(usize, Edit)> = edits.into_iter().enumerate().collect();
        sorted.sort_by(|a, b| a.1.offset.cmp(&b.1.offset).then(a.0.cmp(&b.0)));
        let mut out = String::new();
        let mut pos = start;
        for (_, e) in &sorted {
            if e.offset > pos {
                out.push_str(&String::from_utf16_lossy(&cu.text[pos..e.offset]));
                pos = e.offset;
            }
            out.push_str(&e.text);
            pos = pos.max(e.offset + e.length);
        }
        if pos < end {
            out.push_str(&String::from_utf16_lossy(&cu.text[pos..end]));
        }
        if start == end && out.is_empty() {
            return None;
        }
        Some((start, end - start, out))
    }

    /// `OnDemandComputer.identifyPossibleReductions`.
    fn reductions(
        &self,
        imports: &HashSet<ImportName>,
        touched: &HashSet<ImportName>,
        type_explicit: &HashSet<String>,
        static_explicit: &HashSet<String>,
    ) -> Vec<(ImportName, Vec<ImportName>)> {
        let mut by_container: HashMap<ImportName, Vec<ImportName>> = HashMap::new();
        for i in imports {
            if !i.is_module {
                by_container.entry(i.container_on_demand()).or_default().push(i.clone());
            }
        }
        let mut out = Vec::new();
        for (container, list) in by_container {
            if touched.contains(&container) && !container.container.is_empty() {
                let explicit = if container.is_static { static_explicit } else { type_explicit };
                let threshold = if container.is_static { self.static_on_demand_threshold } else { self.on_demand_threshold };
                let mut has_on_demand = false;
                let mut reducible = Vec::new();
                for i in &list {
                    if i.is_on_demand() {
                        has_on_demand = true;
                    } else if !explicit.contains(&i.simple) {
                        reducible.push(i.clone());
                    }
                }
                if has_on_demand || reducible.len() as i64 >= threshold {
                    out.push((container, reducible));
                }
            }
        }
        out
    }
}

fn compare_import(prefix: char, qualifier: &str, name: &str, curr: &str) -> i32 {
    if !curr.starts_with(prefix) || !curr.ends_with(name) {
        return RES_NAME_UNKNOWN;
    }
    let curr = &curr[1..];
    if curr.len() == name.len() {
        if qualifier.is_empty() {
            return RES_NAME_FOUND;
        }
        return RES_NAME_CONFLICT;
    }
    let dot = curr.len() as isize - name.len() as isize - 1;
    if dot < 0 || curr.as_bytes()[dot as usize] != b'.' {
        return RES_NAME_UNKNOWN;
    }
    if qualifier.len() != dot as usize || !curr.starts_with(qualifier) {
        return RES_NAME_CONFLICT;
    }
    RES_NAME_FOUND
}

/// `TypeConflictingSimpleNameFinder`: simple names found in more than one
/// of `containers`.
fn find_conflicts(imports: &HashSet<ImportName>, is_static: bool, containers: &HashSet<String>, types: &ContainerTypes) -> HashSet<String> {
    let mut out = HashSet::new();
    if containers.is_empty() || imports.is_empty() {
        return out;
    }
    let names: HashSet<&str> = imports.iter().filter(|i| i.is_static == is_static).map(|i| i.simple.as_str()).collect();
    for n in names {
        let count = containers.iter().filter(|c| types.get(*c).is_some_and(|s| s.contains(n))).count();
        if count > 1 {
            out.insert(n.to_owned());
        }
    }
    out
}

fn order_preserving_add(existing: &[ImportName], to_add: &[ImportName], groups: &GroupComparator) -> Vec<ImportName> {
    if to_add.is_empty() {
        return existing.to_vec();
    }
    let mut sorted: Vec<ImportName> = to_add.iter().filter(|a| !existing.contains(a)).cloned().collect();
    sorted.sort_by(|a, b| groups.compare_imports(a, b));
    if existing.is_empty() {
        return sorted;
    }
    let mut tree: Vec<ImportName> = existing.to_vec();
    tree.sort_by(|a, b| groups.compare_imports(a, b));
    tree.dedup_by(|a, b| groups.compare_imports(a, b) == Ordering::Equal);
    let mut before: HashMap<ImportName, Vec<ImportName>> = HashMap::new();
    let mut after: HashMap<ImportName, Vec<ImportName>> = HashMap::new();
    for n in &sorted {
        let preceding = tree.iter().filter(|e| groups.compare_imports(e, n) == Ordering::Less).last().cloned();
        let succeeding = tree.iter().find(|e| groups.compare_imports(e, n) == Ordering::Greater).cloned();
        let with_succeeding = match (&preceding, &succeeding) {
            (None, _) => true,
            (_, None) => false,
            (Some(p), Some(s)) => count_matching_prefix_segments(&n.container, &s.container) > count_matching_prefix_segments(&n.container, &p.container),
        };
        if with_succeeding {
            before.entry(succeeding.unwrap()).or_default().push(n.clone());
        } else {
            after.entry(preceding.unwrap()).or_default().push(n.clone());
        }
    }
    let mut out = Vec::new();
    for e in existing {
        if let Some(b) = before.remove(e) {
            out.extend(b);
        }
        out.push(e.clone());
        if let Some(a) = after.remove(e) {
            out.extend(a);
        }
    }
    out
}

// ─── Original imports and the rewrite site ───────────────────────────────────

#[derive(Debug, Clone)]
struct OriginalEntry {
    name: ImportName,
    /// (offset, length) of the whitespace before the import (after the previous one).
    leading: (usize, usize),
    preceding_line_delimiters: usize,
    /// (offset, length) of the declaration and its comments.
    decl: (usize, usize),
    has_floating_comment: bool,
}

fn read_original_imports(cu: &CuStructure) -> Vec<OriginalEntry> {
    let mut out = Vec::new();
    let base = if cu.package.is_some() { 1 } else { 0 };
    let mut previous_end: Option<usize> = None;
    for (i, imp) in cu.imports.iter().enumerate() {
        let (ext_start, ext_end) = cu.extended(base + i);
        let start = ext_start.min(imp.start);
        let (leading, delims) = match previous_end {
            None => ((start, 0), 0),
            Some(pe) => ((pe, start - pe), cu.line_of(start) - cu.line_of(pe - 1)),
        };
        out.push(OriginalEntry {
            name: imp.name.clone(),
            leading,
            preceding_line_delimiters: delims,
            decl: (start, ext_end - start),
            has_floating_comment: false,
        });
        previous_end = Some(ext_end);
    }
    out
}

struct RewriteSite {
    surrounding: (usize, usize),
    imports: Option<(usize, usize)>,
    has_preceding: bool,
    has_succeeding: bool,
}

fn rewrite_site(cu: &CuStructure, originals: &[OriginalEntry]) -> RewriteSite {
    let imports_region = if originals.is_empty() {
        None
    } else {
        let first = originals[0].decl.0;
        let last = originals.last().unwrap();
        Some((first, last.decl.0 + last.decl.1 - first))
    };
    // mapTopLevelNodes
    let mut nodes: BTreeMap<usize, usize> = BTreeMap::new();
    if let Some(p) = cu.package {
        nodes.insert(p.0, p.1);
    }
    for i in &cu.imports {
        nodes.insert(i.start, i.end);
    }
    for t in &cu.types {
        nodes.insert(t.0, t.1);
    }
    for c in &cu.free_comments {
        nodes.insert(c.0, c.1);
    }
    let (surrounding_start, position_after) = match imports_region {
        None => {
            let start = if cu.package.is_some() {
                cu.extended(0).1
            } else {
                let mut s = 0;
                let mut iter = nodes.iter();
                if let Some((first_start, _)) = iter.next() {
                    if cu.free_comments.iter().any(|c| c.0 == *first_start) {
                        for (ns, ne) in nodes.iter() {
                            if !cu.free_comments.iter().any(|c| c.0 == *ns) {
                                break;
                            }
                            s = *ne;
                        }
                    }
                }
                s
            };
            (start, start)
        }
        Some((off, len)) => {
            let start = nodes.range(..off).next_back().map(|(_, e)| *e).unwrap_or(0);
            (start, off + len)
        }
    };
    let mut end = position_after;
    while end < cu.len && char::from_u32(cu.text[end] as u32).is_some_and(char::is_whitespace) {
        end += 1;
    }
    RewriteSite {
        surrounding: (surrounding_start, end - surrounding_start),
        imports: imports_region,
        has_preceding: surrounding_start != 0,
        has_succeeding: end != cu.len,
    }
}

#[derive(Debug, Clone)]
struct Edit {
    offset: usize,
    length: usize,
    text: String,
    /// RangeMarker: kept for delete computation, dropped afterwards.
    marker: bool,
}

impl Edit {
    fn insert(offset: usize, text: String) -> Self {
        Edit { offset, length: 0, text, marker: false }
    }
    fn replace(offset: usize, length: usize, text: String) -> Self {
        Edit { offset, length, text, marker: false }
    }
    fn marker(offset: usize, length: usize) -> Self {
        Edit { offset, length, text: String::new(), marker: true }
    }
}

#[allow(clippy::too_many_arguments)]
fn determine_edits(
    region: (usize, usize),
    result: &[ImportName],
    originals: &[OriginalEntry],
    groups: &GroupComparator,
    lines_between_groups: usize,
    line_delimiter: &str,
    write: &dyn Fn(&ImportName) -> String,
) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    let mut cursor_idx = 0usize;
    let mut cursor_pos = region.0;
    let preceding: HashMap<ImportName, Option<ImportName>> = {
        let mut m = HashMap::new();
        let mut prev: Option<ImportName> = None;
        for o in originals {
            m.insert(o.name.clone(), prev.clone());
            prev = Some(o.name.clone());
        }
        m
    };
    let original_of = |n: &ImportName| originals.iter().position(|o| &o.name == n);
    let mut last: Option<ImportName> = None;
    for current in result {
        let orig_idx = original_of(current);
        if orig_idx.is_some() {
            while cursor_idx < originals.len() && Some(cursor_idx) != orig_idx {
                let d = originals[cursor_idx].decl;
                cursor_pos = d.0 + d.1;
                cursor_idx += 1;
            }
        }
        let (leading_edits, decl_edits): (Vec<Edit>, Vec<Edit>) = match orig_idx {
            Some(i) if i == cursor_idx => {
                let o = &originals[i];
                (vec![Edit::marker(o.leading.0, o.leading.1)], vec![Edit::marker(o.decl.0, o.decl.1)])
            }
            Some(_) => {
                // Moving original imports does not happen when only adding imports.
                (vec![], vec![])
            }
            None => (vec![], vec![Edit::insert(cursor_pos, write(current))]),
        };
        // determineNewDelimiter
        let new_delimiter: Option<String> = match &last {
            None => Some(String::new()),
            Some(last_name) => {
                let needs_standard = match orig_idx {
                    None => true,
                    Some(_) => preceding.get(current).cloned().flatten().as_ref() != Some(last_name),
                };
                if !needs_standard {
                    None
                } else {
                    let mut n = 1;
                    if let Some(i) = orig_idx {
                        if originals[i].has_floating_comment {
                            n = 2;
                        }
                    }
                    if groups.compare(last_name, current) != Ordering::Equal {
                        n = n.max(lines_between_groups);
                    }
                    let standard = line_delimiter.repeat(n);
                    match orig_idx {
                        Some(i) if originals[i].preceding_line_delimiters == n && originals[i].leading.1 == standard.encode_utf16().count() => None,
                        _ => Some(standard),
                    }
                }
            }
        };
        match new_delimiter {
            None => edits.extend(leading_edits),
            Some(d) if !d.is_empty() => edits.push(Edit::insert(cursor_pos, d)),
            _ => {}
        }
        edits.extend(decl_edits);
        if orig_idx == Some(cursor_idx) && cursor_idx < originals.len() {
            let d = originals[cursor_idx].decl;
            cursor_pos = d.0 + d.1;
            cursor_idx += 1;
        }
        last = Some(current.clone());
    }
    // deleteRemainingText
    let mut sorted = edits.clone();
    sorted.sort_by_key(|e| e.offset);
    let mut delete_pos = region.0;
    let mut deletes = Vec::new();
    for e in &sorted {
        if e.offset > delete_pos {
            deletes.push(Edit::replace(delete_pos, e.offset - delete_pos, String::new()));
        }
        delete_pos = delete_pos.max(e.offset + e.length);
    }
    let region_end = region.0 + region.1;
    if delete_pos < region_end {
        deletes.push(Edit::replace(delete_pos, region_end - delete_pos, String::new()));
    }
    edits.extend(deletes);
    edits.into_iter().filter(|e| !e.marker).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn rewrite(src: &str, add: &[&str]) -> Option<(usize, usize, String)> {
        let cu = Arc::new(CuStructure::parse(src));
        let mut r = ImportRewrite::create(cu, vec!["java".into(), "javax".into(), "org".into(), "com".into()], 99, 99);
        for a in add {
            r.add_import(a);
        }
        r.rewrite(&HashMap::new(), "\n", 1, false)
    }

    #[test]
    fn insert_after_package() {
        let src = "package org.sample;\n\npublic class Test {\n}\n";
        let (o, l, t) = rewrite(src, &["java.io.File"]).unwrap();
        assert_eq!((o, l), (19, 2));
        assert_eq!(t, "\n\nimport java.io.File;\n\n");
    }

    #[test]
    fn insert_into_existing() {
        let src = "package p;\n\nimport java.util.List;\n\npublic class A {}\n";
        let (o, l, t) = rewrite(src, &["java.util.Map"]).unwrap();
        assert_eq!(&src[o..o + l], "");
        assert_eq!(t, "\nimport java.util.Map;");
        let src = "package p;\n\nimport java.util.List;\n\npublic class A {}\n";
        let (_, _, t) = rewrite(src, &["org.junit.Test"]).unwrap();
        assert_eq!(t, "\n\nimport org.junit.Test;");
    }

    #[test]
    fn implicit_imports_are_skipped() {
        let src = "package p;\npublic class A {}\n";
        assert!(rewrite(src, &["java.lang.String"]).is_none());
        assert!(rewrite(src, &["p.B"]).is_none());
    }
}
