//! Port of `org.eclipse.jdt.core.dom.rewrite.ImportRewrite` and its
//! `ImportRewriteAnalyzer` / `ImportEditor` (JDT's import placement:
//! import groups from the import order, order-preserving insertion, on-demand
//! thresholds, static imports, line delimiters between groups).
//!
//! Configure it like `CodeStyleConfiguration.createImportRewrite(astRoot,
//! true)` does with [`ImportRewrite::create`] + [`ImportRewrite::configure`]
//! (jdt.ls `java.completion.importOrder`,
//! `java.sources.organizeImports.starThreshold` / `staticStarThreshold`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::indent;
use super::text_edit::{EditKind, EditTree, MalformedTree};
use crate::semantic_ast::{Ast, BindingRef, NodeId, NodeKind};

/// `ImportRewriteContext` results.
pub const RES_NAME_FOUND: i32 = 1;
pub const RES_NAME_UNKNOWN: i32 = 2;
pub const RES_NAME_CONFLICT: i32 = 3;
pub const RES_NAME_UNKNOWN_NEEDS_EXPLICIT_IMPORT: i32 = 4;
/// `ImportRewriteContext` kinds.
pub const KIND_TYPE: i32 = 1;
pub const KIND_STATIC_FIELD: i32 = 2;
pub const KIND_STATIC_METHOD: i32 = 3;

/// `ImportRewrite.ImportRewriteContext`.
pub trait ImportRewriteContext {
    /// `findInContext(qualifier, name, kind)`; `None` defers to the import
    /// rewrite's own imports (`findInImports`).
    fn find_in_context(&self, imports: &ImportRewrite, qualifier: &str, name: &str, kind: i32) -> i32;
}

/// The default context (`ImportRewrite.defaultContext`).
pub struct DefaultContext;

impl ImportRewriteContext for DefaultContext {
    fn find_in_context(&self, imports: &ImportRewrite, qualifier: &str, name: &str, kind: i32) -> i32 {
        imports.find_in_imports(qualifier, name, kind)
    }
}

/// Answers whether a type `container.simpleName` exists (for import
/// conflicts with on-demand imports: `TypeConflictingSimpleNameFinder`).
pub trait TypeLookup {
    fn type_exists(&self, container: &str, simple_name: &str) -> bool;
}

pub struct NoTypes;
impl TypeLookup for NoTypes {
    fn type_exists(&self, _: &str, _: &str) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImportName {
    pub is_static: bool,
    pub is_module: bool,
    pub container_name: String,
    pub simple_name: String,
    pub qualified_name: String,
}

impl ImportName {
    fn new(is_static: bool, is_module: bool, container: &str, simple: &str) -> Self {
        let qualified = if container.is_empty() { simple.to_owned() } else { format!("{container}.{simple}") };
        ImportName { is_static, is_module, container_name: container.to_owned(), simple_name: simple.to_owned(), qualified_name: qualified }
    }

    /// `ImportName.createFor(isStatic, isModule, qualifiedName)`.
    pub fn create_for(is_static: bool, is_module: bool, qualified: &str) -> Self {
        let (container, simple) = match qualified.rfind('.') {
            Some(i) => (&qualified[..i], &qualified[i + 1..]),
            None => ("", qualified),
        };
        Self::new(is_static, is_module, container, simple)
    }

    pub fn create_on_demand(is_static: bool, container: &str) -> Self {
        Self::new(is_static, false, container, "*")
    }

    pub fn is_on_demand(&self) -> bool {
        self.simple_name == "*"
    }

    pub fn container_on_demand(&self) -> ImportName {
        if self.is_on_demand() {
            self.clone()
        } else {
            Self::create_on_demand(self.is_static, &self.container_name)
        }
    }

    /// `ImportDeclarationWriter.writeImportDeclaration`.
    fn declaration(&self, space_before_semicolon: bool) -> String {
        let mut s = String::from("import ");
        if self.is_static {
            s.push_str("static ");
        }
        if self.is_module {
            s.push_str("module ");
        }
        s.push_str(&self.qualified_name);
        if space_before_semicolon {
            s.push(' ');
        }
        s.push(';');
        s
    }
}

// ─── ImportGroupComparator ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ImportGroup {
    name: String,
    index: usize,
    prefix: Option<usize>,
}

#[derive(Clone, Debug)]
struct ImportGroupComparator {
    type_groups: Vec<ImportGroup>,
    static_groups: Vec<ImportGroup>,
}

fn is_whole_segment_prefix(prefix: &str, name: &str) -> bool {
    name.starts_with(prefix) && (prefix.is_empty() || name.len() == prefix.len() || name.as_bytes()[prefix.len()] == b'.')
}

impl ImportGroupComparator {
    fn new(order: &[String]) -> Self {
        let mut order: Vec<String> = order.to_vec();
        let needs_type = !order.iter().any(|s| s.is_empty());
        let needs_static = !order.iter().any(|s| s == "#");
        if needs_static {
            order.insert(0, "#".into());
        }
        if needs_type {
            order.push(String::new());
        }
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        let mut statics: BTreeMap<String, usize> = BTreeMap::new();
        for (i, g) in order.iter().enumerate() {
            if let Some(s) = g.strip_prefix('#') {
                statics.insert(s.to_owned(), i);
            } else {
                types.insert(g.clone(), i);
            }
        }
        ImportGroupComparator { type_groups: Self::map(types), static_groups: Self::map(statics) }
    }

    fn map(mut groups: BTreeMap<String, usize>) -> Vec<ImportGroup> {
        if groups.is_empty() {
            groups.insert(String::new(), 0);
        }
        let mut out: Vec<ImportGroup> = Vec::new();
        let mut prefixing: Vec<usize> = Vec::new();
        for (name, index) in groups {
            while let Some(&last) = prefixing.last() {
                if is_whole_segment_prefix(&out[last].name, &name) {
                    break;
                }
                prefixing.pop();
            }
            let prefix = prefixing.last().copied();
            out.push(ImportGroup { name, index, prefix });
            prefixing.push(out.len() - 1);
        }
        out
    }

    fn sort_position(&self, n: &ImportName) -> usize {
        let name = if n.is_on_demand() { &n.container_name } else { &n.qualified_name };
        let groups = if n.is_static { &self.static_groups } else { &self.type_groups };
        // floorEntry(name)
        let mut idx = groups.iter().rposition(|g| g.name.as_str() <= name.as_str()).unwrap_or(0);
        while !is_whole_segment_prefix(&groups[idx].name, name) {
            match groups[idx].prefix {
                Some(p) => idx = p,
                None => break,
            }
        }
        groups[idx].index
    }

    fn compare(&self, a: &ImportName, b: &ImportName) -> std::cmp::Ordering {
        self.sort_position(a).cmp(&self.sort_position(b))
    }
}

/// `ImportComparator` (containers sorted by package and containing type).
fn compare_imports(groups: &ImportGroupComparator, a: &ImportName, b: &ImportName) -> std::cmp::Ordering {
    groups
        .compare(a, b)
        .then_with(|| a.container_name.cmp(&b.container_name))
        .then_with(|| a.qualified_name.cmp(&b.qualified_name))
}

// ─── ImportRewrite ───────────────────────────────────────────────────────────

pub struct ImportRewrite {
    ast: Arc<Ast>,
    restore_existing_imports: bool,
    existing_imports: Vec<String>,
    imports_kind_map: HashMap<String, i32>,
    import_order: Vec<String>,
    on_demand_threshold: i32,
    static_on_demand_threshold: i32,
    added_imports: Vec<String>,
    removed_imports: Vec<String>,
    type_explicit_simple_names: HashSet<String>,
    static_explicit_simple_names: HashSet<String>,
    pub filter_implicit_imports: bool,
    pub use_context_to_filter_implicit_imports: bool,
    /// Package of the unit (`compilationUnit.getParent().getElementName()`).
    package_name: String,
    /// `JavaCore.removeJavaLikeExtension(compilationUnit.getElementName())`.
    main_type_name: String,
    /// Formatter options of the unit (blank lines between import groups, ...).
    options: BTreeMap<String, String>,
    pub created_imports: Vec<String>,
    pub created_static_imports: Vec<String>,
}

impl ImportRewrite {
    /// `ImportRewrite.create(astRoot, restoreExistingImports)`.
    pub fn create(ast: Arc<Ast>, restore_existing_imports: bool) -> Self {
        let mut existing = Vec::new();
        if restore_existing_imports {
            for imp in ast.root().list("imports") {
                let is_static = imp.flag("static");
                let mut buf = String::new();
                buf.push(if is_static { 's' } else { 'n' });
                buf.push_str(&imp.child("name").map(|n| n.identifier()).unwrap_or_default());
                if imp.flag("onDemand") {
                    if buf.len() > 1 {
                        buf.push('.');
                    }
                    buf.push('*');
                }
                existing.push(buf);
            }
        }
        let package_name = ast.root().child("package").and_then(|p| p.child("name")).map(|n| n.identifier()).unwrap_or_default();
        let file = ast.uri.rsplit('/').next().unwrap_or("").to_owned();
        let main_type_name = file.strip_suffix(".java").unwrap_or(&file).to_owned();
        ImportRewrite {
            ast,
            restore_existing_imports: !existing.is_empty(),
            existing_imports: existing,
            imports_kind_map: HashMap::new(),
            import_order: Vec::new(),
            on_demand_threshold: 99,
            static_on_demand_threshold: 99,
            added_imports: Vec::new(),
            removed_imports: Vec::new(),
            type_explicit_simple_names: HashSet::new(),
            static_explicit_simple_names: HashSet::new(),
            filter_implicit_imports: true,
            use_context_to_filter_implicit_imports: false,
            package_name,
            main_type_name,
            options: BTreeMap::new(),
            created_imports: Vec::new(),
            created_static_imports: Vec::new(),
        }
    }

    /// `CodeStyleConfiguration.configureImportRewrite` with the jdt.ls
    /// preferences, plus the unit's formatter options.
    pub fn configure(mut self, import_order: &[String], on_demand: i32, static_on_demand: i32, options: &BTreeMap<String, String>) -> Self {
        self.import_order = import_order.to_vec();
        self.on_demand_threshold = if on_demand <= 0 { 1 } else { on_demand };
        self.static_on_demand_threshold = if static_on_demand <= 0 { 1 } else { static_on_demand };
        self.options = options.clone();
        self
    }

    /// `CodeStyleConfiguration.createImportRewrite(astRoot, true)` with the
    /// current jdt.ls preferences.
    pub fn create_for_corrections(ast: Arc<Ast>, options: &BTreeMap<String, String>) -> Self {
        let order = crate::features::preferences::import_order();
        let (t, s) = crate::features::preferences::import_thresholds();
        Self::create(ast, true).configure(&order, t, s, options)
    }

    pub fn ast(&self) -> &Arc<Ast> {
        &self.ast
    }

    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    /// `compareImport(prefix, qualifier, name, curr)`.
    fn compare_import(prefix: char, qualifier: &str, name: &str, curr: &str) -> i32 {
        if !curr.starts_with('m') {
            if curr.starts_with(prefix) && curr.ends_with(name) {
                let curr = &curr[1..];
                if curr.len() == name.len() {
                    return if qualifier.is_empty() { RES_NAME_FOUND } else { RES_NAME_CONFLICT };
                }
                let dot_pos = curr.len() - name.len() - 1;
                if curr.as_bytes()[dot_pos] != b'.' {
                    return RES_NAME_UNKNOWN;
                }
                if qualifier.len() == dot_pos && curr.starts_with(qualifier) {
                    RES_NAME_FOUND
                } else {
                    RES_NAME_CONFLICT
                }
            } else {
                RES_NAME_UNKNOWN
            }
        } else {
            RES_NAME_UNKNOWN
        }
    }

    /// `ImportRewrite.findInImports`.
    pub fn find_in_imports(&self, qualifier: &str, name: &str, kind: i32) -> i32 {
        let allow_ambiguity = kind == KIND_STATIC_METHOD || name == "*";
        let prefix = if kind == KIND_TYPE { 'n' } else { 's' };
        for curr in self.existing_imports.iter().rev() {
            let res = Self::compare_import(prefix, qualifier, name, curr);
            if res != RES_NAME_UNKNOWN && (!allow_ambiguity || res == RES_NAME_FOUND) {
                if prefix != 's' {
                    return res;
                }
                let curr_kind = self.imports_kind_map.get(&curr[1..]);
                if curr_kind.is_some() && curr_kind == self.imports_kind_map.get(&format!("{qualifier}.{name}")) {
                    return res;
                }
            }
        }
        if kind == KIND_TYPE && self.filter_implicit_imports && self.use_context_to_filter_implicit_imports {
            let main = concat_name(&self.package_name, &self.main_type_name);
            if qualifier == self.package_name || main == concat_name(qualifier, name) {
                return RES_NAME_FOUND;
            }
            for t in self.ast.root().list("types") {
                if t.child("name").is_some_and(|n| n.identifier() == name) {
                    return if qualifier == self.package_name { RES_NAME_FOUND } else { RES_NAME_CONFLICT };
                }
            }
        }
        RES_NAME_UNKNOWN
    }

    /// `ImportRewrite.addImport(ITypeBinding, context)`: the name to use.
    pub fn add_import_binding(&mut self, binding: BindingRef<'_>, context: &dyn ImportRewriteContext) -> String {
        if binding.is_primitive() || binding.is_type_variable() || binding.is_recovered() {
            return binding.name().to_owned();
        }
        let Some(normalized) = normalize_type_binding(binding) else { return "invalid".into() };
        if normalized.is_wildcard_type() {
            let mut res = String::from("?");
            if let Some(bound) = normalized.bound() {
                if !bound.is_wildcard_type() && !bound.is_capture() {
                    res.push_str(if normalized.has(crate::semantic_ast::bflag::UPPERBOUND) { " extends " } else { " super " });
                    res.push_str(&self.add_import_binding(bound, context));
                }
            }
            return res;
        }
        if normalized.is_array() {
            let mut res = match normalized.element_type() {
                Some(e) => self.add_import_binding(e, context),
                None => normalized.name().to_owned(),
            };
            for _ in 0..normalized.dimensions() {
                res.push_str("[]");
            }
            return res;
        }
        let decl = normalized.type_declaration().unwrap_or(normalized);
        let qualified = decl.qualified_name().to_owned();
        if qualified.is_empty() {
            return decl.name().to_owned();
        }
        let unnamed = is_type_in_unnamed_package(normalized);
        let s = self.internal_add_import(&qualified, context, unnamed);
        let args = normalized.type_arguments();
        if args.is_empty() {
            return s;
        }
        let mut res = s;
        res.push('<');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                res.push(',');
            }
            if contains_nested_capture(*a, false) {
                res.push('?');
            } else {
                res.push_str(&self.add_import_binding(*a, context));
            }
        }
        res.push('>');
        res
    }

    /// `ImportRewrite.addImport(String qualifiedTypeName, context)`.
    pub fn add_import(&mut self, qualified_type_name: &str, context: &dyn ImportRewriteContext) -> String {
        if let Some(i) = qualified_type_name.find('<') {
            return self.internal_add_import(&qualified_type_name[..i], context, false) + &qualified_type_name[i..];
        }
        if let Some(i) = qualified_type_name.find('[') {
            return self.internal_add_import(&qualified_type_name[..i], context, false) + &qualified_type_name[i..];
        }
        self.internal_add_import(qualified_type_name, context, false)
    }

    /// `ImportRewrite.addStaticImport(declaringTypeName, simpleName, isField, context)`.
    pub fn add_static_import(&mut self, declaring_type: &str, simple_name: &str, is_field: bool, context: &dyn ImportRewriteContext) -> String {
        let key = format!("{declaring_type}.{simple_name}");
        if !declaring_type.contains('.') {
            return key;
        }
        let kind = if is_field { KIND_STATIC_FIELD } else { KIND_STATIC_METHOD };
        self.imports_kind_map.insert(key.clone(), kind);
        let res = context.find_in_context(self, declaring_type, simple_name, kind);
        if res == RES_NAME_CONFLICT {
            return key;
        }
        if res == RES_NAME_UNKNOWN {
            self.add_entry(format!("s{key}"));
        }
        if res == RES_NAME_UNKNOWN_NEEDS_EXPLICIT_IMPORT {
            self.add_entry(format!("s{key}"));
            self.static_explicit_simple_names.insert(simple_name.to_owned());
        }
        simple_name.to_owned()
    }

    fn internal_add_import(&mut self, full: &str, context: &dyn ImportRewriteContext, unnamed: bool) -> String {
        let (container, name) = match full.rfind('.') {
            Some(i) => (&full[..i], &full[i + 1..]),
            None => ("", full),
        };
        if container.is_empty() && is_primitive(name) {
            return full.to_owned();
        }
        let res = context.find_in_context(self, container, name, KIND_TYPE);
        if res != RES_NAME_CONFLICT && !unnamed {
            if res == RES_NAME_UNKNOWN {
                self.add_entry(format!("n{full}"));
            }
            if res == RES_NAME_UNKNOWN_NEEDS_EXPLICIT_IMPORT {
                self.add_entry(format!("n{full}"));
                self.type_explicit_simple_names.insert(name.to_owned());
            }
            name.to_owned()
        } else {
            full.to_owned()
        }
    }

    fn add_entry(&mut self, entry: String) {
        self.existing_imports.push(entry.clone());
        if let Some(i) = self.removed_imports.iter().position(|e| *e == entry) {
            self.removed_imports.remove(i);
        } else {
            self.added_imports.push(entry);
        }
    }

    fn remove_entry(&mut self, entry: &str) -> bool {
        if let Some(i) = self.existing_imports.iter().position(|e| e == entry) {
            self.existing_imports.remove(i);
            if let Some(j) = self.added_imports.iter().position(|e| e == entry) {
                self.added_imports.remove(j);
            } else {
                self.removed_imports.push(entry.to_owned());
            }
            true
        } else {
            false
        }
    }

    pub fn remove_import(&mut self, qualified_name: &str) -> bool {
        self.remove_entry(&format!("n{qualified_name}"))
    }

    pub fn remove_static_import(&mut self, qualified_name: &str) -> bool {
        self.remove_entry(&format!("s{qualified_name}"))
    }

    pub fn has_recorded_changes(&self) -> bool {
        !self.restore_existing_imports || !self.added_imports.is_empty() || !self.removed_imports.is_empty()
    }

    pub fn added_imports(&self) -> Vec<String> {
        self.added_imports.iter().filter(|e| e.starts_with('n')).map(|e| e[1..].to_owned()).collect()
    }

    pub fn added_static_imports(&self) -> Vec<String> {
        self.added_imports.iter().filter(|e| e.starts_with('s')).map(|e| e[1..].to_owned()).collect()
    }

    /// `ImportRewrite.rewriteImports(monitor)`.
    pub fn rewrite_imports(&mut self, types: &dyn TypeLookup) -> Result<EditTree, MalformedTree> {
        if !self.has_recorded_changes() {
            self.created_imports.clear();
            self.created_static_imports.clear();
            return Ok(EditTree::new());
        }
        let mut analyzer = ImportRewriteAnalyzer::new(self);
        for added in &self.added_imports.clone() {
            let is_static = added.starts_with('s');
            analyzer.add_import(ImportName::create_for(is_static, added.starts_with('m'), &added[1..]));
        }
        for removed in &self.removed_imports.clone() {
            let is_static = removed.starts_with('s');
            analyzer.remove_import(ImportName::create_for(is_static, removed.starts_with('m'), &removed[1..]));
        }
        for n in &self.type_explicit_simple_names {
            analyzer.type_explicit.insert(n.clone());
        }
        for n in &self.static_explicit_simple_names {
            analyzer.static_explicit.insert(n.clone());
        }
        let (edit, created) = analyzer.analyze_rewrite(types)?;
        self.created_imports = created.iter().filter(|n| !n.is_static && !n.is_module).map(|n| n.qualified_name.clone()).collect();
        self.created_static_imports = created.iter().filter(|n| n.is_static).map(|n| n.qualified_name.clone()).collect();
        Ok(edit)
    }
}

fn concat_name(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_owned()
    } else if b.is_empty() {
        a.to_owned()
    } else {
        format!("{a}.{b}")
    }
}

fn is_primitive(name: &str) -> bool {
    matches!(name, "int" | "long" | "short" | "byte" | "char" | "float" | "double" | "boolean" | "void")
}

fn normalize_type_binding(b: BindingRef<'_>) -> Option<BindingRef<'_>> {
    if b.is_null_type() || b.name() == "void" {
        return None;
    }
    if b.is_anonymous() {
        let ifs = b.interfaces();
        return if let Some(f) = ifs.first() { Some(*f) } else { b.superclass() };
    }
    Some(b)
}

fn is_type_in_unnamed_package(b: BindingRef<'_>) -> bool {
    let mut t = b;
    while let Some(d) = t.declaring_class() {
        t = d;
    }
    !t.is_type_variable() && t.package_name().is_some_and(str::is_empty) && !t.is_primitive() && !t.is_array()
}

fn contains_nested_capture(b: BindingRef<'_>, nested: bool) -> bool {
    if b.is_primitive() || b.is_type_variable() {
        return false;
    }
    if b.is_capture() {
        return nested || true;
    }
    if b.is_wildcard_type() {
        return b.bound().is_some_and(|x| contains_nested_capture(x, true));
    }
    if b.is_array() {
        return b.element_type().is_some_and(|x| contains_nested_capture(x, true));
    }
    b.type_arguments().into_iter().any(|a| contains_nested_capture(a, true))
}

// ─── ImportRewriteAnalyzer / ImportEditor ────────────────────────────────────

#[derive(Clone, Debug)]
struct ImportComment {
    offset: usize,
    length: usize,
    succeeding_line_delimiters: i32,
}

#[derive(Clone, Debug)]
struct OriginalImportEntry {
    name: ImportName,
    comments: Vec<ImportComment>,
    preceding_line_delimiters: i32,
    leading_delimiter: (usize, usize),
    declaration_and_comments: (usize, usize),
}

#[derive(Clone, Debug)]
enum Entry {
    Original(usize),
    New(ImportName),
}

struct ImportRewriteAnalyzer<'r> {
    ir: &'r ImportRewrite,
    originals: Vec<OriginalImportEntry>,
    to_add: Vec<ImportName>,
    to_remove: Vec<ImportName>,
    report_all_as_created: bool,
    type_explicit: HashSet<String>,
    static_explicit: HashSet<String>,
    implicit_containers: HashSet<String>,
    groups: ImportGroupComparator,
    preserve: bool,
}

fn push_unique(v: &mut Vec<ImportName>, n: ImportName) {
    if !v.contains(&n) {
        v.push(n);
    }
}

impl<'r> ImportRewriteAnalyzer<'r> {
    fn new(ir: &'r ImportRewrite) -> Self {
        let originals = read_original_imports(&ir.ast);
        let preserve = ir.restore_existing_imports;
        let mut to_remove = Vec::new();
        if !preserve {
            for o in &originals {
                push_unique(&mut to_remove, o.name.clone());
            }
        }
        let mut implicit = HashSet::new();
        if ir.filter_implicit_imports {
            implicit.insert("java.lang".to_owned());
            implicit.insert(ir.package_name.clone());
        }
        ImportRewriteAnalyzer {
            ir,
            originals,
            to_add: Vec::new(),
            to_remove,
            report_all_as_created: !preserve,
            type_explicit: HashSet::new(),
            static_explicit: HashSet::new(),
            implicit_containers: implicit,
            groups: ImportGroupComparator::new(&ir.import_order),
            preserve,
        }
    }

    fn add_import(&mut self, n: ImportName) {
        self.to_remove.retain(|r| *r != n);
        push_unique(&mut self.to_add, n);
    }

    fn remove_import(&mut self, n: ImportName) {
        self.to_add.retain(|r| *r != n);
        push_unique(&mut self.to_remove, n);
    }

    fn original_names(&self) -> Vec<ImportName> {
        self.originals.iter().map(|o| o.name.clone()).collect()
    }

    fn touched_containers(&self) -> HashSet<ImportName> {
        self.to_add.iter().chain(self.to_remove.iter()).filter(|n| !n.is_module).map(ImportName::container_on_demand).collect()
    }

    /// `OnDemandComputer.identifyPossibleReductions`.
    fn reductions(&self, imports: &HashSet<ImportName>, touched: &HashSet<ImportName>, type_explicit: &HashSet<String>, static_explicit: &HashSet<String>) -> Vec<(ImportName, Vec<ImportName>)> {
        let mut by_container: Vec<(ImportName, Vec<ImportName>)> = Vec::new();
        for n in imports {
            if n.is_module {
                continue;
            }
            let c = n.container_on_demand();
            match by_container.iter_mut().find(|(k, _)| *k == c) {
                Some((_, v)) => v.push(n.clone()),
                None => by_container.push((c, vec![n.clone()])),
            }
        }
        by_container.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = Vec::new();
        for (container, members) in by_container {
            if !touched.contains(&container) || container.container_name.is_empty() {
                continue;
            }
            let explicit = if container.is_static { static_explicit } else { type_explicit };
            let threshold = if container.is_static { self.ir.static_on_demand_threshold } else { self.ir.on_demand_threshold };
            let mut has_on_demand = false;
            let mut reducible = Vec::new();
            for m in members {
                if m.is_on_demand() {
                    has_on_demand = true;
                } else if !explicit.contains(&m.simple_name) {
                    reducible.push(m);
                }
            }
            if has_on_demand || reducible.len() as i32 >= threshold {
                out.push((container, reducible));
            }
        }
        out
    }

    fn analyze_rewrite(&mut self, types: &dyn TypeLookup) -> Result<(EditTree, Vec<ImportName>), MalformedTree> {
        let order = self.compute_import_order(types);
        let entries: Vec<Entry> = order
            .iter()
            .map(|n| match self.originals.iter().position(|o| o.name == *n) {
                Some(i) => Entry::Original(i),
                None => Entry::New(n.clone()),
            })
            .collect();
        let edit = self.create_text_edit(&entries)?;
        let original_set: HashSet<ImportName> = self.original_names().into_iter().collect();
        let mut created: Vec<ImportName> = Vec::new();
        for n in order {
            if self.report_all_as_created || !original_set.contains(&n) {
                push_unique(&mut created, n);
            }
        }
        Ok((edit, created))
    }

    fn compute_import_order(&self, types: &dyn TypeLookup) -> Vec<ImportName> {
        let mut all: HashSet<ImportName> = self.original_names().into_iter().collect();
        all.extend(self.to_add.iter().cloned());
        for r in &self.to_remove {
            all.remove(r);
        }
        let touched = self.touched_containers();
        // ConflictIdentifier.identifyConflicts
        let candidates = self.reductions(&all, &touched, &self.type_explicit, &self.static_explicit);
        let mut type_containers: HashSet<String> = candidates.iter().filter(|c| !c.0.is_static).map(|c| c.0.container_name.clone()).collect();
        let mut static_containers: HashSet<String> = candidates.iter().filter(|c| c.0.is_static).map(|c| c.0.container_name.clone()).collect();
        if !type_containers.is_empty() {
            type_containers.extend(all.iter().filter(|n| n.is_on_demand() && !n.is_static).map(|n| n.container_name.clone()));
            type_containers.extend(self.implicit_containers.iter().cloned());
            type_containers.extend(static_containers.iter().cloned());
        }
        if !static_containers.is_empty() {
            static_containers.extend(all.iter().filter(|n| n.is_on_demand() && n.is_static).map(|n| n.container_name.clone()));
        }
        let find_conflicts = |is_static: bool, containers: &HashSet<String>| -> HashSet<String> {
            if containers.is_empty() || all.is_empty() {
                return HashSet::new();
            }
            let names: HashSet<String> = all.iter().filter(|n| n.is_static == is_static).map(|n| n.simple_name.clone()).collect();
            names
                .into_iter()
                .filter(|name| containers.iter().filter(|c| types.type_exists(c, name)).count() > 1)
                .collect()
        };
        let mut all_type_explicit = self.type_explicit.clone();
        all_type_explicit.extend(find_conflicts(false, &type_containers));
        let mut all_static_explicit = self.static_explicit.clone();
        all_static_explicit.extend(find_conflicts(true, &static_containers));
        // identifyImplicitImports
        let implicit: Vec<ImportName> = self
            .to_add
            .iter()
            .filter(|n| self.implicit_containers.contains(&n.container_name) && !all_type_explicit.contains(&n.simple_name))
            .cloned()
            .collect();
        let without_implicit: HashSet<ImportName> = all.iter().filter(|n| !implicit.contains(n)).cloned().collect();
        let reductions = self.reductions(&without_implicit, &touched, &all_type_explicit, &all_static_explicit);
        // computeDelta
        let mut additions: Vec<ImportName> = self.to_add.clone();
        let mut removals: Vec<ImportName> = self.to_remove.clone();
        removals.extend(implicit.iter().cloned());
        additions.retain(|a| !removals.contains(a));
        for (container, reducible) in reductions {
            additions.retain(|a| !reducible.contains(a));
            removals.extend(reducible.iter().cloned());
            additions.push(container.clone());
            removals.retain(|r| *r != container);
        }
        let with_removals: Vec<ImportName> = self.original_names().into_iter().filter(|n| !removals.contains(n)).collect();
        if self.preserve {
            self.add_preserving(&with_removals, &additions)
        } else {
            let mut set: Vec<ImportName> = Vec::new();
            for n in with_removals.into_iter().chain(additions) {
                push_unique(&mut set, n);
            }
            set.sort_by(|a, b| compare_imports(&self.groups, a, b));
            set
        }
    }

    /// `OrderPreservingImportAdder.addImports`.
    fn add_preserving(&self, existing: &[ImportName], to_add: &[ImportName]) -> Vec<ImportName> {
        if to_add.is_empty() {
            return existing.to_vec();
        }
        let mut new_sorted: Vec<ImportName> = Vec::new();
        for n in to_add {
            if !existing.contains(n) {
                push_unique(&mut new_sorted, n.clone());
            }
        }
        new_sorted.sort_by(|a, b| compare_imports(&self.groups, a, b));
        if existing.is_empty() {
            return new_sorted;
        }
        let mut sorted_existing: Vec<ImportName> = Vec::new();
        for e in existing {
            if !sorted_existing.iter().any(|x| compare_imports(&self.groups, x, e) == std::cmp::Ordering::Equal) {
                sorted_existing.push(e.clone());
            }
        }
        sorted_existing.sort_by(|a, b| compare_imports(&self.groups, a, b));
        let mut before: HashMap<ImportName, Vec<ImportName>> = HashMap::new();
        let mut after: HashMap<ImportName, Vec<ImportName>> = HashMap::new();
        for n in new_sorted {
            let lower = sorted_existing.iter().rev().find(|e| compare_imports(&self.groups, e, &n) == std::cmp::Ordering::Less).cloned();
            let higher = sorted_existing.iter().find(|e| compare_imports(&self.groups, e, &n) == std::cmp::Ordering::Greater).cloned();
            let group_with_succeeding = match (&lower, &higher) {
                (None, _) => true,
                (_, None) => false,
                (Some(l), Some(h)) => count_matching_prefix_segments(&n.container_name, &h.container_name) > count_matching_prefix_segments(&n.container_name, &l.container_name),
            };
            if group_with_succeeding {
                before.entry(higher.unwrap()).or_default().push(n);
            } else {
                after.entry(lower.unwrap()).or_default().push(n);
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

    // ── ImportEditor ──

    fn line_delimiter(&self) -> String {
        super::analyzer::default_line_delimiter(&self.ir.ast.source)
    }

    fn create_delimiter(&self, n: i32) -> String {
        self.line_delimiter().repeat(n.max(1) as usize)
    }

    fn lines_between_groups(&self) -> i32 {
        let n: i32 = self.ir.options.get("org.eclipse.jdt.core.formatter.blank_lines_between_import_groups").and_then(|v| v.parse().ok()).unwrap_or(1);
        (if n >= 0 { n } else { 1 }) + 1
    }

    fn space_before_semicolon(&self) -> bool {
        self.ir.options.get("org.eclipse.jdt.core.formatter.insert_space_before_semicolon").map(String::as_str) == Some("insert")
    }

    fn create_text_edit(&self, resultant: &[Entry]) -> Result<EditTree, MalformedTree> {
        let site = determine_rewrite_site(&self.ir.ast, &self.originals);
        let mut tree = EditTree::new();
        let mut edits: Vec<(i32, i32, EditKind, Option<usize>)> = Vec::new();
        let (surround_off, surround_len) = site.surrounding;
        if resultant.is_empty() {
            if !self.originals.is_empty() {
                let ws = if site.has_preceding {
                    self.create_delimiter(if site.has_succeeding { 2 } else { 1 })
                } else {
                    String::new()
                };
                edits.push((surround_off as i32, surround_len as i32, EditKind::Replace(ws), None));
            }
        } else if self.originals.is_empty() {
            let import_edits = self.edits_for_imports((surround_off, surround_len), resultant);
            if site.has_preceding {
                edits.push((surround_off as i32, 0, EditKind::Insert(self.create_delimiter(2)), None));
            }
            edits.extend(import_edits);
            let delims = if site.has_succeeding { 2 } else { 1 };
            edits.push((surround_off as i32, 0, EditKind::Insert(self.create_delimiter(delims)), None));
        } else {
            edits.extend(self.edits_for_imports(site.imports.unwrap(), resultant));
        }
        // Materialise (move targets reference move sources by position in `edits`).
        let mut ids = Vec::new();
        for (o, l, k, _) in &edits {
            let kind = match k {
                EditKind::MoveTarget(src) => EditKind::MoveTarget(ids[*src]),
                other => other.clone(),
            };
            ids.push(tree.new_edit(*o, *l, kind));
        }
        for id in ids {
            tree.add_child(EditTree::ROOT, id)?;
        }
        Ok(tree)
    }

    /// `determineEditsForImports`: edits as `(offset, length, kind, _)`;
    /// move targets reference the index of their source in the returned list.
    fn edits_for_imports(&self, region: (usize, usize), resultant: &[Entry]) -> Vec<(i32, i32, EditKind, Option<usize>)> {
        let mut edits: Vec<(i32, i32, EditKind, Option<usize>)> = Vec::new();
        let reassignments = self.reassign_comments(resultant);
        // OriginalImportsCursor
        let mut cursor_index = 0usize;
        let mut cursor_position = region.0;
        let mut last: Option<&Entry> = None;
        let original_preceding: HashMap<usize, Option<usize>> = if self.preserve {
            (0..self.originals.len()).map(|i| (i, if i == 0 { None } else { Some(i - 1) })).collect()
        } else {
            HashMap::new()
        };
        for entry in resultant {
            if let Entry::Original(oi) = entry {
                while cursor_index < self.originals.len() && cursor_index != *oi {
                    let (o, l) = self.originals[cursor_index].declaration_and_comments;
                    cursor_position = o + l;
                    cursor_index += 1;
                }
            }
            let reassigned: Vec<ImportComment> = match entry {
                Entry::Original(oi) => reassignments.get(oi).cloned().unwrap_or_default(),
                Entry::New(_) => Vec::new(),
            };
            // Placement: (leading delimiter edits, comment and declaration edits)
            let mut leading: Vec<(i32, i32, EditKind, Option<usize>)> = Vec::new();
            let mut decl: Vec<(i32, i32, EditKind, Option<usize>)> = Vec::new();
            match entry {
                Entry::Original(oi) => {
                    let orig = &self.originals[*oi];
                    if cursor_index == *oi {
                        leading.push((orig.leading_delimiter.0 as i32, orig.leading_delimiter.1 as i32, EditKind::RangeMarker, None));
                        decl.push((orig.declaration_and_comments.0 as i32, orig.declaration_and_comments.1 as i32, EditKind::RangeMarker, None));
                    } else {
                        leading.push((orig.leading_delimiter.0 as i32, orig.leading_delimiter.1 as i32, EditKind::MoveSource, None));
                        leading.push((cursor_position as i32, 0, EditKind::MoveTarget(usize::MAX), Some(0)));
                        decl.push((orig.declaration_and_comments.0 as i32, orig.declaration_and_comments.1 as i32, EditKind::MoveSource, None));
                        decl.push((cursor_position as i32, 0, EditKind::MoveTarget(usize::MAX), Some(0)));
                    }
                }
                Entry::New(n) => {
                    decl.push((cursor_position as i32, 0, EditKind::Insert(n.declaration(self.space_before_semicolon())), None));
                }
            }
            let new_delimiter = self.determine_new_delimiter(last, entry, &reassigned, &original_preceding);
            match new_delimiter {
                None => push_relative(&mut edits, leading),
                Some(d) if !d.is_empty() => edits.push((cursor_position as i32, 0, EditKind::Insert(d), None)),
                _ => {}
            }
            if !reassigned.is_empty() {
                let mut last_comment: Option<&ImportComment> = None;
                for c in &reassigned {
                    edits.push((c.offset as i32, c.length as i32, EditKind::MoveSource, None));
                    let src = edits.len() - 1;
                    if let Some(lc) = last_comment {
                        let n = if lc.succeeding_line_delimiters > 1 { 2 } else { 1 };
                        edits.push((cursor_position as i32, 0, EditKind::Insert(self.create_delimiter(n)), None));
                    }
                    edits.push((cursor_position as i32, 0, EditKind::MoveTarget(src), None));
                    last_comment = Some(c);
                }
                let floating = matches!(entry, Entry::Original(oi) if self.originals[*oi].comments.iter().any(|c| c.succeeding_line_delimiters > 1));
                let d = if floating { self.create_delimiter(2) } else { self.line_delimiter() };
                edits.push((cursor_position as i32, 0, EditKind::Insert(d), None));
            }
            push_relative(&mut edits, decl);
            if let Entry::Original(oi) = entry {
                if cursor_index == *oi && cursor_index < self.originals.len() {
                    let (o, l) = self.originals[cursor_index].declaration_and_comments;
                    cursor_position = o + l;
                    cursor_index += 1;
                }
            }
            last = Some(entry);
        }
        // deleteRemainingText
        let mut sorted: Vec<(i32, i32)> = edits.iter().map(|(o, l, _, _)| (*o, *l)).collect();
        sorted.sort_by_key(|e| e.0);
        let mut delete_position = region.0 as i32;
        let mut deletes = Vec::new();
        for (o, l) in sorted {
            if o > delete_position {
                deletes.push((delete_position, o - delete_position, EditKind::Delete, None));
            }
            delete_position = delete_position.max(o + l);
        }
        let region_end = (region.0 + region.1) as i32;
        if delete_position < region_end {
            deletes.push((delete_position, region_end - delete_position, EditKind::Delete, None));
        }
        edits.extend(deletes);
        // Drop range markers (indices of move targets are adjusted).
        let mut remap: Vec<Option<usize>> = Vec::new();
        let mut out = Vec::new();
        for (o, l, k, x) in edits {
            if matches!(k, EditKind::RangeMarker) {
                remap.push(None);
            } else {
                remap.push(Some(out.len()));
                out.push((o, l, k, x));
            }
        }
        for e in out.iter_mut() {
            if let EditKind::MoveTarget(src) = e.2 {
                e.2 = EditKind::MoveTarget(remap[src].unwrap_or(src));
            }
        }
        out
    }

    fn determine_new_delimiter(&self, last: Option<&Entry>, current: &Entry, reassigned: &[ImportComment], original_preceding: &HashMap<usize, Option<usize>>) -> Option<String> {
        let last = match last {
            None => return Some(String::new()),
            Some(l) => l,
        };
        let has_reassigned = !reassigned.is_empty();
        // needsStandardDelimiter
        let needs = if !self.preserve {
            true
        } else {
            match current {
                Entry::New(_) => true,
                Entry::Original(ci) => {
                    if has_reassigned {
                        true
                    } else {
                        let prev = original_preceding.get(ci).copied().flatten();
                        let last_name = self.entry_name(last);
                        match prev {
                            None => true,
                            Some(p) => self.originals[p].name != last_name,
                        }
                    }
                }
            }
        };
        if !needs {
            return None;
        }
        let mut n = 1;
        let leading: Vec<ImportComment> = if has_reassigned {
            reassigned.to_vec()
        } else if let Entry::Original(ci) = current {
            self.originals[*ci].comments.clone()
        } else {
            Vec::new()
        };
        if leading.iter().any(|c| c.succeeding_line_delimiters > 1) {
            n = 2;
        }
        if self.groups.compare(&self.entry_name(last), &self.entry_name(current)) != std::cmp::Ordering::Equal {
            n = n.max(self.lines_between_groups());
        }
        let standard = self.create_delimiter(n);
        if let (Entry::Original(ci), false) = (current, has_reassigned) {
            let orig = &self.originals[*ci];
            if orig.preceding_line_delimiters == n && orig.leading_delimiter.1 == indent::len16(&standard) {
                return None;
            }
        }
        Some(standard)
    }

    fn entry_name(&self, e: &Entry) -> ImportName {
        match e {
            Entry::Original(i) => self.originals[*i].name.clone(),
            Entry::New(n) => n.clone(),
        }
    }

    /// `RemovedImportCommentReassigner.reassignComments` (keyed by original index).
    fn reassign_comments(&self, resultant: &[Entry]) -> HashMap<usize, Vec<ImportComment>> {
        let kept: HashSet<usize> = resultant.iter().filter_map(|e| if let Entry::Original(i) = e { Some(*i) } else { None }).collect();
        let removed: Vec<usize> = (0..self.originals.len()).filter(|i| !self.originals[*i].comments.is_empty() && !kept.contains(i)).collect();
        let mut out: HashMap<usize, Vec<ImportComment>> = HashMap::new();
        if removed.is_empty() {
            return out;
        }
        let first_single = |container: &ImportName| -> Option<usize> {
            resultant.iter().find_map(|e| match e {
                Entry::Original(i) if !self.originals[*i].name.is_on_demand() && self.originals[*i].name.container_on_demand() == *container => Some(*i),
                _ => None,
            })
        };
        let first_occurrence = |name: &ImportName| -> Option<usize> {
            resultant.iter().find_map(|e| match e {
                Entry::Original(i) if self.originals[*i].name == *name => Some(*i),
                _ => None,
            })
        };
        let mut assigned: HashMap<usize, Vec<usize>> = HashMap::new();
        for r in removed {
            let name = &self.originals[r].name;
            let target = if name.is_on_demand() { first_single(name) } else { first_occurrence(&name.container_on_demand()) };
            if let Some(t) = target {
                assigned.entry(t).or_default().push(r);
            }
        }
        for (target, imports) in assigned {
            let mut comments: Vec<ImportComment> = Vec::new();
            for (k, &imp) in imports.iter().enumerate() {
                comments.extend(self.originals[imp].comments.iter().cloned());
                if let Some(&next) = imports.get(k + 1) {
                    if self.originals[next].comments.iter().any(|c| c.succeeding_line_delimiters > 1) {
                        if let Some(lc) = comments.last_mut() {
                            lc.succeeding_line_delimiters = 2;
                        }
                    }
                }
            }
            out.insert(target, comments);
        }
        out
    }
}

/// Appends `group` (whose move targets reference a source at offset `x`
/// within the group) to `edits`, fixing the source indices.
fn push_relative(edits: &mut Vec<(i32, i32, EditKind, Option<usize>)>, group: Vec<(i32, i32, EditKind, Option<usize>)>) {
    let base = edits.len();
    let mut last_source = None;
    for (i, (o, l, k, x)) in group.into_iter().enumerate() {
        let k = match k {
            EditKind::MoveSource => {
                last_source = Some(base + i);
                EditKind::MoveSource
            }
            EditKind::MoveTarget(_) if x.is_some() => EditKind::MoveTarget(last_source.unwrap_or(base)),
            other => other,
        };
        edits.push((o, l, k, None));
    }
}

fn count_matching_prefix_segments(a: &str, b: &str) -> i32 {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut matching = 0;
    let mut i = 0;
    while i <= a.len() && i <= b.len() {
        let end_a = i == a.len() || a[i] == b'.';
        let end_b = i == b.len() || b[i] == b'.';
        if end_a && end_b {
            matching += 1;
        } else if end_a || end_b || a[i] != b[i] {
            break;
        }
        i += 1;
    }
    matching
}

struct RewriteSite {
    surrounding: (usize, usize),
    imports: Option<(usize, usize)>,
    has_preceding: bool,
    has_succeeding: bool,
}

fn read_original_imports(ast: &Ast) -> Vec<OriginalImportEntry> {
    let root = ast.root();
    let imports = root.list("imports");
    if imports.is_empty() {
        return Vec::new();
    }
    let comments: Vec<NodeId> = {
        let mut c = ast.comments.clone();
        c.sort_by_key(|&n| ast.node(n).start());
        c
    };
    let mut current = 0usize;
    let first_start = match root.child("package") {
        Some(p) => p.extended_start() + p.extended_length(),
        None => imports[0].start(),
    };
    while current < comments.len() && ast.node(comments[current]).start() < first_start {
        current += 1;
    }
    let mut out = Vec::new();
    let mut previous_end: Option<usize> = None;
    for imp in imports {
        let extended_end = imp.extended_start() + imp.extended_length();
        let mut after = current;
        while after < comments.len() && ast.node(comments[after]).start() < extended_end {
            after += 1;
        }
        let import_comments = if after == current { Vec::new() } else { select_import_comments(ast, &comments[current..after], imp.start()) };
        let start = if import_comments.is_empty() { imp.start() } else { imp.start().min(import_comments[0].offset) };
        let (leading, preceding) = match previous_end {
            None => ((start, 0), 0),
            Some(pe) => {
                let first_line = ast.line_number(start);
                let last_line_prev = ast.line_number(pe.saturating_sub(1));
                ((pe, start - pe), first_line - last_line_prev)
            }
        };
        out.push(OriginalImportEntry {
            name: import_name_for(imp),
            comments: import_comments,
            preceding_line_delimiters: preceding,
            leading_delimiter: leading,
            declaration_and_comments: (start, extended_end - start),
        });
        current = after;
        previous_end = Some(extended_end);
    }
    out
}

fn import_name_for(imp: crate::semantic_ast::Node<'_>) -> ImportName {
    let name = imp.child("name").map(|n| n.identifier()).unwrap_or_default();
    let is_static = imp.flag("static");
    if imp.flag("onDemand") {
        ImportName::create_on_demand(is_static, &name)
    } else {
        ImportName::create_for(is_static, false, &name)
    }
}

fn select_import_comments(ast: &Ast, comments: &[NodeId], import_start: usize) -> Vec<ImportComment> {
    let mut out = Vec::new();
    for (i, &c) in comments.iter().enumerate() {
        let node = ast.node(c);
        let next_start = comments.get(i + 1).map(|&n| ast.node(n).start()).unwrap_or(usize::MAX);
        let next = import_start.min(next_start);
        let succeeding = if next == usize::MAX { 0 } else { ast.line_number(next) - ast.line_number(node.end()) };
        out.push(ImportComment { offset: node.start(), length: node.length(), succeeding_line_delimiters: succeeding });
    }
    out
}

fn determine_rewrite_site(ast: &Ast, originals: &[OriginalImportEntry]) -> RewriteSite {
    let imports_region = match (originals.first(), originals.last()) {
        (Some(f), Some(l)) => {
            let start = f.declaration_and_comments.0;
            let end = l.declaration_and_comments.0 + l.declaration_and_comments.1;
            Some((start, end - start))
        }
        _ => None,
    };
    let root = ast.root();
    // mapTopLevelNodes
    let mut top: Vec<(usize, NodeId, bool)> = Vec::new();
    if let Some(p) = root.child("package") {
        top.push((p.start(), p.id, false));
    }
    for i in root.list("imports") {
        top.push((i.start(), i.id, false));
    }
    for t in root.list("types") {
        top.push((t.start(), t.id, false));
    }
    for &c in &ast.comments {
        let n = ast.node(c);
        if n.parent().is_none() {
            top.push((n.start(), c, true));
        }
    }
    top.sort_by_key(|t| t.0);
    top.dedup_by_key(|t| t.0);
    let (surrounding_start, after_imports) = match imports_region {
        None => {
            let start = match root.child("package") {
                Some(p) => p.extended_start() + p.extended_length(),
                None => {
                    let mut s = 0;
                    if top.first().is_some_and(|t| t.2) {
                        for t in &top {
                            if !t.2 {
                                break;
                            }
                            s = ast.node(t.1).end();
                        }
                    }
                    s
                }
            };
            (start, start)
        }
        Some((io, il)) => {
            let lower = top.iter().rev().find(|t| t.0 < io);
            let s = lower.map_or(0, |t| ast.node(t.1).end());
            (s, io + il)
        }
    };
    let mut end = after_imports;
    while end < ast.source.len() && char::from_u32(ast.source[end] as u32).is_some_and(char::is_whitespace) {
        end += 1;
    }
    let len = ast.source.len();
    RewriteSite {
        surrounding: (surrounding_start, end - surrounding_start),
        imports: imports_region,
        has_preceding: surrounding_start != 0,
        has_succeeding: end != len,
    }
}

/// The type a node would resolve to for `ImportRewrite.addImport` users
/// that start from a [`NodeKind`] name (kept for API symmetry).
pub fn is_import_declaration(kind: NodeKind) -> bool {
    kind == NodeKind::ImportDeclaration
}
