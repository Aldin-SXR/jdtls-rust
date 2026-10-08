//! A resolved JDT DOM of one compilation unit, built from the bridge's
//! data-only `semanticAst` request (`SemanticAstService.java`).
//!
//! The bridge exports every node with all of its structural properties (in
//! JDT descriptor order), source ranges, extended ranges, parent links and
//! the bindings JDT resolves for it; bindings form a table that references
//! other bindings by index.  Rust code navigates this model the way jdt.ls
//! code navigates `org.eclipse.jdt.core.dom`:
//!
//! * [`Ast`] owns the nodes; [`Node`] is a cheap `Copy` handle with
//!   `get_parent`, `child("name")`, `list("bodyDeclarations")`, `simple(..)`
//!   and binding accessors.
//! * [`Binding`] / [`BindingRef`] mirror `ITypeBinding`, `IMethodBinding`,
//!   `IVariableBinding` and `IPackageBinding`.
//! * [`finder`] ports `NodeFinder` (covering / covered node).
//! * [`resolve`] ports the `ASTNodes` / `ASTResolving` helpers used by the
//!   correction processors.
//!
//! Positions are JDT positions: UTF-16 code unit offsets into
//! [`Ast::source`].

pub mod finder;
pub mod annotation;
pub mod irritants;
pub mod node_kind;
pub mod problem;
pub mod resolve;
pub mod wire;

pub use node_kind::NodeKind;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;

// ─── Constants ────────────────────────────────────────────────────────────────

/// `org.eclipse.jdt.core.dom.Modifier` flags.
pub mod modifier {
    pub const PUBLIC: i32 = 0x0001;
    pub const PRIVATE: i32 = 0x0002;
    pub const PROTECTED: i32 = 0x0004;
    pub const STATIC: i32 = 0x0008;
    pub const FINAL: i32 = 0x0010;
    pub const SYNCHRONIZED: i32 = 0x0020;
    pub const VOLATILE: i32 = 0x0040;
    pub const TRANSIENT: i32 = 0x0080;
    pub const NATIVE: i32 = 0x0100;
    pub const SEALED: i32 = 0x0200;
    pub const ABSTRACT: i32 = 0x0400;
    pub const STRICTFP: i32 = 0x0800;
    pub const NON_SEALED: i32 = 0x1000;
    pub const DEFAULT: i32 = 0x10000;

    /// `Modifier.ModifierKeyword.toKeyword(..)`: flag of a modifier keyword.
    pub fn flag_of(keyword: &str) -> i32 {
        match keyword {
            "public" => PUBLIC,
            "private" => PRIVATE,
            "protected" => PROTECTED,
            "static" => STATIC,
            "final" => FINAL,
            "synchronized" => SYNCHRONIZED,
            "volatile" => VOLATILE,
            "transient" => TRANSIENT,
            "native" => NATIVE,
            "sealed" => SEALED,
            "abstract" => ABSTRACT,
            "strictfp" => STRICTFP,
            "non-sealed" => NON_SEALED,
            "default" => DEFAULT,
            _ => 0,
        }
    }

    /// Keywords of the flags in `flags`, in `AST.newModifiers` order
    /// (`ASTNodeFactory.newModifiers`).
    pub fn keywords(flags: i32) -> Vec<&'static str> {
        let order: [(&str, i32); 14] = [
            ("public", PUBLIC),
            ("protected", PROTECTED),
            ("private", PRIVATE),
            ("abstract", ABSTRACT),
            ("default", DEFAULT),
            ("static", STATIC),
            ("final", FINAL),
            ("synchronized", SYNCHRONIZED),
            ("native", NATIVE),
            ("strictfp", STRICTFP),
            ("transient", TRANSIENT),
            ("volatile", VOLATILE),
            ("sealed", SEALED),
            ("non-sealed", NON_SEALED),
        ];
        order.iter().filter(|(_, f)| flags & f != 0).map(|(k, _)| *k).collect()
    }
}

/// `IBinding` kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Package,
    Type,
    Variable,
    Method,
    Annotation,
    MemberValuePair,
    Module,
    Unknown,
}

impl BindingKind {
    fn from_jdt(k: i32) -> Self {
        match k {
            1 => BindingKind::Package,
            2 => BindingKind::Type,
            3 => BindingKind::Variable,
            4 => BindingKind::Method,
            5 => BindingKind::Annotation,
            6 => BindingKind::MemberValuePair,
            7 => BindingKind::Module,
            _ => BindingKind::Unknown,
        }
    }
}

/// Binding flags (`SemanticAstService` constants).
pub mod bflag {
    pub const DEPRECATED: u64 = 1;
    pub const RECOVERED: u64 = 1 << 1;
    pub const SYNTHETIC: u64 = 1 << 2;
    pub const FROM_SOURCE: u64 = 1 << 3;
    pub const PRIMITIVE: u64 = 1 << 4;
    pub const ARRAY: u64 = 1 << 5;
    pub const CLASS: u64 = 1 << 6;
    pub const INTERFACE: u64 = 1 << 7;
    pub const ENUM: u64 = 1 << 8;
    pub const RECORD: u64 = 1 << 9;
    pub const ANNOTATION: u64 = 1 << 10;
    pub const TYPE_VARIABLE: u64 = 1 << 11;
    pub const WILDCARD: u64 = 1 << 12;
    pub const CAPTURE: u64 = 1 << 13;
    pub const PARAMETERIZED: u64 = 1 << 14;
    pub const RAW: u64 = 1 << 15;
    pub const GENERIC: u64 = 1 << 16;
    pub const NULL_TYPE: u64 = 1 << 17;
    pub const ANONYMOUS: u64 = 1 << 18;
    pub const LOCAL: u64 = 1 << 19;
    pub const MEMBER: u64 = 1 << 20;
    pub const NESTED: u64 = 1 << 21;
    pub const TOP_LEVEL: u64 = 1 << 22;
    pub const INTERSECTION: u64 = 1 << 23;
    pub const UPPERBOUND: u64 = 1 << 24;
    pub const FIELD: u64 = 1 << 25;
    pub const ENUM_CONSTANT: u64 = 1 << 26;
    pub const PARAMETER: u64 = 1 << 27;
    pub const RECORD_COMPONENT: u64 = 1 << 28;
    pub const EFFECTIVELY_FINAL: u64 = 1 << 29;
    pub const CONSTRUCTOR: u64 = 1 << 30;
    pub const DEFAULT_CONSTRUCTOR: u64 = 1 << 31;
    pub const VARARGS: u64 = 1 << 32;
    pub const ANNOTATION_MEMBER: u64 = 1 << 33;
    pub const GENERIC_METHOD: u64 = 1 << 34;
    pub const PARAMETERIZED_METHOD: u64 = 1 << 35;
    pub const RAW_METHOD: u64 = 1 << 36;
    pub const COMPACT_CONSTRUCTOR: u64 = 1 << 37;
    pub const CANONICAL_CONSTRUCTOR: u64 = 1 << 38;
    pub const SYNTHETIC_RECORD_METHOD: u64 = 1 << 39;
}

/// `ASTNode` flags plus the bridge's extra node flags.
pub mod nflag {
    pub const MALFORMED: u32 = 1;
    pub const ORIGINAL: u32 = 2;
    pub const PROTECT: u32 = 4;
    pub const RECOVERED: u32 = 8;
    pub const BOXING: u32 = 1 << 8;
    pub const UNBOXING: u32 = 1 << 9;
    pub const COMMENT_ROOT: u32 = 1 << 10;
    /// `isResolvedTypeInferredFromExpectedType()` of an invocation.
    pub const INFERRED_FROM_EXPECTED: u32 = 1 << 11;
}

// ─── Model ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingId(pub u32);

#[derive(Clone, Debug)]
pub enum PropValue {
    Child(Option<NodeId>),
    List(Vec<NodeId>),
    Simple(Option<String>),
}

#[derive(Clone, Debug)]
pub struct NodeData {
    pub kind: NodeKind,
    pub kind_name: String,
    pub start: usize,
    pub length: usize,
    pub parent: Option<NodeId>,
    pub location: Option<&'static str>,
    pub extended_start: usize,
    pub extended_length: usize,
    pub binding: Option<BindingId>,
    pub type_binding: Option<BindingId>,
    pub method_binding: Option<BindingId>,
    pub annotation: Option<annotation::Annotation>,
    pub constant_expression: bool,
    pub flags: u32,
    pub props: Vec<(&'static str, PropValue)>,
}

#[derive(Clone, Debug, Default)]
pub struct Binding {
    pub kind: Option<BindingKind>,
    pub key: String,
    pub name: String,
    pub modifiers: i32,
    pub flags: u64,
    pub qualified_name: String,
    pub binary_name: Option<String>,
    pub package: Option<String>,
    pub erasure: Option<BindingId>,
    pub type_declaration: Option<BindingId>,
    pub declaring_class: Option<BindingId>,
    pub declaring_method: Option<BindingId>,
    pub superclass: Option<BindingId>,
    pub interfaces: Vec<BindingId>,
    pub type_arguments: Vec<BindingId>,
    pub type_parameters: Vec<BindingId>,
    pub type_bounds: Vec<BindingId>,
    pub element_type: Option<BindingId>,
    pub component_type: Option<BindingId>,
    pub annotations: Vec<annotation::Annotation>,
    pub type_annotations: Vec<annotation::Annotation>,
    pub parameter_annotations: Vec<Vec<annotation::Annotation>>,
    pub source_modifiers: Option<Vec<String>>,
    pub module: Option<BindingId>,
    pub bound: Option<BindingId>,
    pub wildcard: Option<BindingId>,
    pub generic_type_of_wildcard: Option<BindingId>,
    pub dimensions: i32,
    /// Declared members of source types and their superclass hierarchy.
    pub declared_methods: Option<Vec<BindingId>>,
    pub declared_fields: Option<Vec<BindingId>>,
    pub declared_types: Option<Vec<BindingId>>,
    /// Constructor members of a selected type and its superclass, including binaries.
    pub constructors: Option<Vec<BindingId>>,
    /// Variable: type; method: unused.
    pub var_type: Option<BindingId>,
    pub variable_id: i32,
    pub constant_value: Option<String>,
    pub variable_declaration: Option<BindingId>,
    pub return_type: Option<BindingId>,
    pub method_declaration: Option<BindingId>,
    pub parameter_types: Vec<BindingId>,
    pub exception_types: Vec<BindingId>,
    pub parameter_names: Vec<String>,
    /// Compiler IMethodBinding.isSubsignature/overrides relations in this AST.
    pub method_subsignatures: Vec<BindingId>,
    pub method_overrides: Vec<BindingId>,
    pub assignment_targets: Vec<BindingId>,
    /// `isCastCompatible` targets (exported for unresolved invocations).
    pub cast_targets: Vec<BindingId>,
    pub functional_method: Option<BindingId>,
    pub name_offset: i32,
    pub source_offset: i32,
}

/// `IProblem` of the AST (`CompilationUnit.getProblems()`).
#[derive(Clone, Debug)]
pub struct AstProblem {
    pub id: i32,
    pub source_start: i32,
    pub source_end: i32,
    pub line: i32,
    pub is_error: bool,
    pub is_warning: bool,
    pub message: String,
    pub category: i32,
    pub arguments: Vec<String>,
}

pub struct Ast {
    pub uri: String,
    /// The unit's source as UTF-16 code units (JDT positions index this).
    pub source: Vec<u16>,
    text: String,
    nodes: Vec<NodeData>,
    bindings: Vec<Binding>,
    pub problems: Vec<AstProblem>,
    pub comments: Vec<NodeId>,
    line_starts: Vec<usize>,
    /// Preorder index one past the last node of each node's subtree.
    subtree_end: Vec<u32>,
    /// Bridge cache key of this AST (for follow-up queries).
    pub cache_key: Option<String>,
    /// The configured nullable / nonnull annotations are `@Target(TYPE_USE)`.
    pub nullable_type_use: bool,
    pub non_null_type_use: bool,
}

static PROP_NAMES: Lazy<Mutex<HashMap<String, &'static str>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Interns a structural property id (`StructuralPropertyDescriptor.getId()`).
pub fn intern(name: &str) -> &'static str {
    let mut map = PROP_NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = map.get(name) {
        return s;
    }
    let s: &'static str = Box::leak(name.to_owned().into_boxed_str());
    map.insert(name.to_owned(), s);
    s
}

fn opt_id(i: i32) -> Option<u32> {
    (i >= 0).then_some(i as u32)
}

impl Ast {
    /// Decodes a bridge response for `source` (the text the bridge parsed).
    pub fn from_wire(uri: &str, source: &str, data: wire::SemanticAstData) -> Ast {
        let source_length = source.encode_utf16().count();
        let strings = data.strings;
        let s = |i: i32| -> Option<String> { (i >= 0).then(|| strings.get(i as usize).cloned()).flatten() };
        let b = |i: i32| opt_id(i).map(BindingId);
        let bl = |v: &Option<Vec<i32>>| v.as_ref().map(|v| v.iter().filter_map(|&i| opt_id(i).map(BindingId)).collect::<Vec<_>>());
        let nodes = data
            .nodes
            .iter()
            .map(|n| {
                let kind_name = s(n.t).unwrap_or_default();
                let mut props = Vec::with_capacity(n.pr.len() / 3);
                for tri in n.pr.chunks(3) {
                    if tri.len() < 3 {
                        break;
                    }
                    let name = intern(&s(tri[0]).unwrap_or_default());
                    let value = match tri[1] {
                        0 => PropValue::Child(opt_id(tri[2]).map(NodeId)),
                        1 => PropValue::List(
                            n.ls.get(tri[2].max(0) as usize)
                                .map(|l| l.iter().filter_map(|&i| opt_id(i).map(NodeId)).collect())
                                .unwrap_or_default(),
                        ),
                        _ => PropValue::Simple(s(tri[2])),
                    };
                    props.push((name, value));
                }
                // Recovered declarations can carry JDT sentinel extended ranges.
                // Use their ordinary source range before passing offsets to rewrite.
                let start = (n.s.max(0) as usize).min(source_length);
                let length = (n.l.max(0) as usize).min(source_length - start);
                let (es, el) = if n.es >= 0 && n.el >= 0
                    && (n.es as usize).saturating_add(n.el as usize) <= source_length {
                    (n.es as usize, n.el as usize)
                } else { (start, length) };
                NodeData {
                    kind: NodeKind::from_name(&kind_name),
                    kind_name,
                    start,
                    length,
                    parent: opt_id(n.p).map(NodeId),
                    location: s(n.loc).map(|l| intern(&l)),
                    extended_start: es,
                    extended_length: el,
                    binding: b(n.b),
                    type_binding: b(n.tb),
                    method_binding: b(n.mb),
                    annotation: n.annotation.as_ref().and_then(|a| annotation::decode(a, &s)),
                    constant_expression: n.constant_expression,
                    flags: n.f as u32,
                    props,
                }
            })
            .collect();
        let bindings = data
            .bindings
            .iter()
            .map(|o| Binding {
                kind: Some(BindingKind::from_jdt(o.k)),
                key: s(o.key).unwrap_or_default(),
                name: s(o.n).unwrap_or_default(),
                modifiers: o.m,
                flags: o.f as u64,
                qualified_name: s(o.qn).unwrap_or_default(),
                binary_name: s(o.bn),
                package: s(o.pkg),
                erasure: b(o.er),
                type_declaration: b(o.td),
                declaring_class: b(o.dc),
                declaring_method: b(o.dm),
                superclass: b(o.sc),
                interfaces: bl(&o.it).unwrap_or_default(),
                type_arguments: bl(&o.ta).unwrap_or_default(),
                type_parameters: bl(&o.tp).unwrap_or_default(),
                type_bounds: bl(&o.tbs).unwrap_or_default(),
                element_type: b(o.el),
                component_type: b(o.cmp),
                annotations: o.ann.as_deref().unwrap_or_default().iter().filter_map(|a| annotation::decode(a, &s)).collect(),
                type_annotations: o.tann.as_deref().unwrap_or_default().iter().filter_map(|a| annotation::decode(a, &s)).collect(),
                parameter_annotations: o.pann.as_deref().unwrap_or_default().iter().map(|v| v.iter().filter_map(|a| annotation::decode(a, &s)).collect()).collect(),
                bound: b(o.bound),
                wildcard: b(o.wc),
                source_modifiers: o.sm.as_ref().map(|v| v.iter().filter_map(|&i| s(i)).collect()),
                module: b(o.module),
                generic_type_of_wildcard: b(o.gt),
                dimensions: o.dim,
                declared_methods: bl(&o.dmeth),
                declared_fields: bl(&o.dfld),
                declared_types: bl(&o.dtyp),
                constructors: bl(&o.ctors),
                var_type: b(o.typ),
                variable_id: o.vid,
                constant_value: s(o.cv),
                variable_declaration: b(o.vd),
                return_type: b(o.rt),
                method_declaration: b(o.md),
                parameter_types: bl(&o.pt).unwrap_or_default(),
                exception_types: bl(&o.et).unwrap_or_default(),
                parameter_names: o.pn.as_ref()
                    .map(|v| v.iter().filter_map(|&i| s(i)).collect())
                    .unwrap_or_default(),
                name_offset: o.name_offset,
                source_offset: o.source_offset,
                method_subsignatures: bl(&o.ss).unwrap_or_default(),
                method_overrides: bl(&o.ov).unwrap_or_default(),
                assignment_targets: bl(&o.assign).unwrap_or_default(),
                cast_targets: bl(&o.cast).unwrap_or_default(),
                functional_method: b(o.fim),
            })
            .collect();
        let problems = data
            .problems
            .iter()
            .map(|p| AstProblem {
                id: p.id,
                source_start: p.s,
                source_end: p.e,
                line: p.line,
                is_error: p.sev == 0,
                is_warning: p.sev == 1,
                message: s(p.msg).unwrap_or_default(),
                category: p.cat,
                arguments: p.args.iter().map(|&a| s(a).unwrap_or_default()).collect(),
            })
            .collect();
        let source: Vec<u16> = source.encode_utf16().collect();
        let mut line_starts = vec![0];
        let mut i = 0;
        while i < source.len() {
            let c = source[i];
            if c == b'\r' as u16 {
                if source.get(i + 1) == Some(&(b'\n' as u16)) {
                    i += 1;
                }
                line_starts.push(i + 1);
            } else if c == b'\n' as u16 {
                line_starts.push(i + 1);
            }
            i += 1;
        }
        let nodes: Vec<NodeData> = nodes;
        let mut subtree_end: Vec<u32> = (1..=nodes.len() as u32).collect();
        for i in (0..nodes.len()).rev() {
            if let Some(p) = nodes[i].parent {
                let e = subtree_end[i];
                if subtree_end[p.0 as usize] < e {
                    subtree_end[p.0 as usize] = e;
                }
            }
        }
        Ast {
            subtree_end,
            uri: uri.to_owned(),
            text: String::from_utf16_lossy(&source),
            source,
            nodes,
            bindings,
            problems,
            comments: data.comments.iter().filter_map(|&i| opt_id(i).map(NodeId)).collect(),
            line_starts,
            cache_key: data.cache_key,
            nullable_type_use: data.nullable_type_use,
            non_null_type_use: data.non_null_type_use,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn root(&self) -> Node<'_> {
        Node { ast: self, id: NodeId(0) }
    }

    pub fn node(&self, id: NodeId) -> Node<'_> {
        Node { ast: self, id }
    }

    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.nodes[id.0 as usize]
    }

    /// One past the last preorder index of `id`'s subtree.
    pub fn subtree_end(&self, id: NodeId) -> NodeId {
        NodeId(self.subtree_end[id.0 as usize])
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn binding_data(&self, id: BindingId) -> &Binding {
        &self.bindings[id.0 as usize]
    }

    pub fn binding(&self, id: BindingId) -> BindingRef<'_> {
        BindingRef { ast: self, id }
    }

    /// The binding with `key`, if this AST knows it.
    pub fn binding_by_key(&self, key: &str) -> Option<BindingRef<'_>> {
        self.bindings.iter().position(|b| b.key == key).map(|i| self.binding(BindingId(i as u32)))
    }

    /// A type already resolved in the semantic graph (including well-known primitives).
    pub(crate) fn type_by_name(&self, name: &str) -> Option<BindingRef<'_>> {
        self.bindings.iter().position(|b| b.kind == Some(BindingKind::Type) && b.qualified_name == name)
            .map(|i| self.binding(BindingId(i as u32)))
    }

    /// Source substring `[start, end)` in UTF-16 units.
    pub fn substring(&self, start: usize, end: usize) -> String {
        let end = end.min(self.source.len());
        let start = start.min(end);
        String::from_utf16_lossy(&self.source[start..end])
    }

    pub fn char_at(&self, offset: usize) -> Option<u16> {
        self.source.get(offset).copied()
    }

    /// `CompilationUnit.getLineNumber(position)` (1-based; -1 outside).
    pub fn line_number(&self, offset: usize) -> i32 {
        if offset > self.source.len() {
            return -1;
        }
        (self.line_of(offset) + 1) as i32
    }

    /// 0-based line of `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(self.source.len())
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// `JDTUtils.toRange`-style LSP position of `offset`.
    pub fn position(&self, offset: usize) -> tower_lsp::lsp_types::Position {
        let offset = offset.min(self.source.len());
        let line = self.line_of(offset);
        tower_lsp::lsp_types::Position { line: line as u32, character: (offset - self.line_starts[line]) as u32 }
    }

    /// Offset of an LSP position (`DiagnosticsHelper.getStartOffset`); `None`
    /// when the line does not exist.
    pub fn offset_of(&self, pos: tower_lsp::lsp_types::Position) -> Option<usize> {
        let start = *self.line_starts.get(pos.line as usize)?;
        Some(start + pos.character as usize)
    }

    /// All nodes in preorder (`ASTVisitor` order), the unit first and then
    /// unattached comments.
    pub fn all_nodes(&self) -> impl Iterator<Item = Node<'_>> {
        (0..self.nodes.len()).map(move |i| self.node(NodeId(i as u32)))
    }
}

// ─── Node handle ──────────────────────────────────────────────────────────────

/// A node of an [`Ast`] (`org.eclipse.jdt.core.dom.ASTNode`).
#[derive(Clone, Copy)]
pub struct Node<'a> {
    pub ast: &'a Ast,
    pub id: NodeId,
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.ast, other.ast)
    }
}
impl Eq for Node<'_> {}

impl std::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}[{},{}]", self.kind().name(), self.id.0, self.start(), self.length())
    }
}

impl<'a> Node<'a> {
    fn d(&self) -> &'a NodeData {
        self.ast.data(self.id)
    }

    pub fn kind(&self) -> NodeKind {
        self.d().kind
    }

    pub fn is(&self, kind: NodeKind) -> bool {
        self.d().kind == kind
    }

    /// `getNodeType()` name (`getClass().getSimpleName()`).
    pub fn kind_name(&self) -> &'a str {
        &self.d().kind_name
    }

    pub fn start(&self) -> usize {
        self.d().start
    }

    pub fn length(&self) -> usize {
        self.d().length
    }

    pub fn end(&self) -> usize {
        self.d().start + self.d().length
    }

    pub fn extended_start(&self) -> usize {
        self.d().extended_start
    }

    pub fn extended_length(&self) -> usize {
        self.d().extended_length
    }

    pub fn is_constant_expression(&self) -> bool {
        self.d().constant_expression
    }

    pub fn flags(&self) -> u32 {
        self.d().flags
    }

    pub fn parent(&self) -> Option<Node<'a>> {
        self.d().parent.map(|p| self.ast.node(p))
    }

    /// `getLocationInParent().getId()`.
    pub fn location(&self) -> Option<&'static str> {
        self.d().location
    }

    pub fn location_is(&self, prop: &str) -> bool {
        self.d().location == Some(prop)
    }

    pub fn props(&self) -> &'a [(&'static str, PropValue)] {
        &self.d().props
    }

    pub fn prop(&self, name: &str) -> Option<&'a PropValue> {
        self.d().props.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn has_prop(&self, name: &str) -> bool {
        self.prop(name).is_some()
    }

    /// A child-node property (`getStructuralProperty(ChildPropertyDescriptor)`).
    pub fn child(&self, name: &str) -> Option<Node<'a>> {
        match self.prop(name) {
            Some(PropValue::Child(Some(id))) => Some(self.ast.node(*id)),
            _ => None,
        }
    }

    /// A child-list property.
    pub fn list(&self, name: &str) -> Vec<Node<'a>> {
        match self.prop(name) {
            Some(PropValue::List(ids)) => ids.iter().map(|&i| self.ast.node(i)).collect(),
            _ => Vec::new(),
        }
    }

    pub fn list_ids(&self, name: &str) -> &'a [NodeId] {
        match self.prop(name) {
            Some(PropValue::List(ids)) => ids,
            _ => &[],
        }
    }

    /// A simple property value, as `String.valueOf(value)`.
    pub fn simple(&self, name: &str) -> Option<&'a str> {
        match self.prop(name) {
            Some(PropValue::Simple(Some(s))) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        self.simple(name) == Some("true")
    }

    /// All direct children in property order.
    pub fn children(&self) -> Vec<Node<'a>> {
        let mut out = Vec::new();
        for (_, v) in self.props() {
            match v {
                PropValue::Child(Some(id)) => out.push(self.ast.node(*id)),
                PropValue::List(ids) => out.extend(ids.iter().map(|&i| self.ast.node(i))),
                _ => {}
            }
        }
        out
    }

    /// Source text of the node.
    pub fn source_text(&self) -> String {
        self.ast.substring(self.start(), self.end())
    }

    /// `SimpleName.getIdentifier()` / `Name.getFullyQualifiedName()`.
    pub fn identifier(&self) -> String {
        match self.kind() {
            NodeKind::SimpleName => self.simple("identifier").unwrap_or("").to_owned(),
            NodeKind::QualifiedName => {
                let q = self.child("qualifier").map(|q| q.identifier()).unwrap_or_default();
                let n = self.child("name").map(|n| n.identifier()).unwrap_or_default();
                format!("{q}.{n}")
            }
            _ => self.source_text(),
        }
    }

    /// `BodyDeclaration.getModifiers()` etc.: flags of the `modifiers` list.
    pub fn modifiers(&self) -> i32 {
        self.list("modifiers")
            .iter()
            .filter(|m| m.is(NodeKind::Modifier))
            .map(|m| modifier::flag_of(m.simple("keyword").unwrap_or("")))
            .fold(0, |a, b| a | b)
    }

    pub fn binding(&self) -> Option<BindingRef<'a>> {
        self.d().binding.map(|b| self.ast.binding(b))
    }

    /// `Expression.resolveTypeBinding()` / `Name.resolveTypeBinding()`.
    pub fn type_binding(&self) -> Option<BindingRef<'a>> {
        self.d().type_binding.map(|b| self.ast.binding(b))
    }

    /// `resolveMethodBinding()` / `resolveConstructorBinding()`.
    pub fn method_binding(&self) -> Option<BindingRef<'a>> {
        self.d().method_binding.map(|b| self.ast.binding(b))
    }

    /// Ancestors, nearest first (excluding `self`).
    pub fn ancestors(&self) -> impl Iterator<Item = Node<'a>> {
        let mut cur = self.parent();
        std::iter::from_fn(move || {
            let n = cur?;
            cur = n.parent();
            Some(n)
        })
    }

    /// `ASTNodes.getParent(node, kind)`: nearest ancestor-or-self of `kind`.
    pub fn ancestor_or_self(&self, pred: impl Fn(NodeKind) -> bool) -> Option<Node<'a>> {
        std::iter::once(*self).chain(self.ancestors()).find(|n| pred(n.kind()))
    }

    /// Preorder (`ASTVisitor`) traversal of the subtree, `self` first.
    pub fn descendants(&self) -> impl Iterator<Item = Node<'a>> {
        let ast = self.ast;
        (self.id.0..ast.subtree_end(self.id).0).map(move |i| ast.node(NodeId(i)))
    }

    /// Whether `other` lies in this node's subtree (or is this node).
    pub fn is_ancestor_or_self_of(&self, other: Node<'_>) -> bool {
        other.id.0 >= self.id.0 && other.id.0 < self.ast.subtree_end(self.id).0
    }

    /// `ASTNode.getRoot()`.
    pub fn root(&self) -> Node<'a> {
        self.ancestors().last().unwrap_or(*self)
    }

    pub fn covers(&self, offset: usize, length: usize) -> bool {
        self.start() <= offset && offset + length <= self.end()
    }
}

// ─── Binding handle ───────────────────────────────────────────────────────────

/// A binding of an [`Ast`] (`IBinding` and sub-interfaces).
#[derive(Clone, Copy)]
pub struct BindingRef<'a> {
    pub ast: &'a Ast,
    pub id: BindingId,
}

impl PartialEq for BindingRef<'_> {
    /// `Bindings.equals`: same key.
    fn eq(&self, other: &Self) -> bool {
        self.data().key == other.data().key
    }
}

impl std::fmt::Debug for BindingRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Binding({:?} {})", self.data().kind, self.data().key)
    }
}

impl<'a> BindingRef<'a> {
    pub fn data(&self) -> &'a Binding {
        self.ast.binding_data(self.id)
    }

    fn opt(&self, id: Option<BindingId>) -> Option<BindingRef<'a>> {
        id.map(|i| self.ast.binding(i))
    }

    fn many(&self, ids: &[BindingId]) -> Vec<BindingRef<'a>> {
        ids.iter().map(|&i| self.ast.binding(i)).collect()
    }

    pub fn kind(&self) -> BindingKind {
        self.data().kind.unwrap_or(BindingKind::Unknown)
    }

    pub fn key(&self) -> &'a str {
        &self.data().key
    }

    pub fn name(&self) -> &'a str {
        &self.data().name
    }

    pub fn modifiers(&self) -> i32 {
        self.data().modifiers
    }

    pub fn has(&self, flag: u64) -> bool {
        self.data().flags & flag != 0
    }

    pub fn is_type(&self) -> bool {
        self.kind() == BindingKind::Type
    }
    pub fn is_variable(&self) -> bool {
        self.kind() == BindingKind::Variable
    }
    pub fn is_method(&self) -> bool {
        self.kind() == BindingKind::Method
    }
    pub fn is_static(&self) -> bool {
        self.modifiers() & modifier::STATIC != 0
    }
    pub fn is_deprecated(&self) -> bool {
        self.has(bflag::DEPRECATED)
    }
    pub fn is_recovered(&self) -> bool {
        self.has(bflag::RECOVERED)
    }
    pub fn is_from_source(&self) -> bool {
        self.has(bflag::FROM_SOURCE)
    }
    pub fn is_primitive(&self) -> bool {
        self.has(bflag::PRIMITIVE)
    }
    pub fn is_array(&self) -> bool {
        self.has(bflag::ARRAY)
    }
    pub fn is_class(&self) -> bool {
        self.has(bflag::CLASS)
    }
    pub fn is_interface(&self) -> bool {
        self.has(bflag::INTERFACE)
    }
    pub fn is_enum(&self) -> bool {
        self.has(bflag::ENUM)
    }
    pub fn is_record(&self) -> bool {
        self.has(bflag::RECORD)
    }
    pub fn is_annotation(&self) -> bool {
        self.has(bflag::ANNOTATION)
    }
    pub fn is_type_variable(&self) -> bool {
        self.has(bflag::TYPE_VARIABLE)
    }
    pub fn is_wildcard_type(&self) -> bool {
        self.has(bflag::WILDCARD)
    }
    pub fn is_capture(&self) -> bool {
        self.has(bflag::CAPTURE)
    }
    pub fn is_parameterized_type(&self) -> bool {
        self.has(bflag::PARAMETERIZED)
    }
    pub fn is_raw_type(&self) -> bool {
        self.has(bflag::RAW)
    }
    pub fn is_generic_type(&self) -> bool {
        self.has(bflag::GENERIC)
    }
    pub fn is_null_type(&self) -> bool {
        self.has(bflag::NULL_TYPE)
    }
    pub fn is_anonymous(&self) -> bool {
        self.has(bflag::ANONYMOUS)
    }
    pub fn is_local(&self) -> bool {
        self.has(bflag::LOCAL)
    }
    pub fn is_member(&self) -> bool {
        self.has(bflag::MEMBER)
    }
    pub fn is_nested(&self) -> bool {
        self.has(bflag::NESTED)
    }
    pub fn is_top_level(&self) -> bool {
        self.has(bflag::TOP_LEVEL)
    }
    pub fn is_field(&self) -> bool {
        self.has(bflag::FIELD)
    }
    pub fn is_enum_constant(&self) -> bool {
        self.has(bflag::ENUM_CONSTANT)
    }
    pub fn is_parameter(&self) -> bool {
        self.has(bflag::PARAMETER)
    }
    pub fn is_constructor(&self) -> bool {
        self.has(bflag::CONSTRUCTOR)
    }
    pub fn is_varargs(&self) -> bool {
        self.has(bflag::VARARGS)
    }

    /// `ITypeBinding.getQualifiedName()` (package name for packages).
    pub fn qualified_name(&self) -> &'a str {
        &self.data().qualified_name
    }
    pub fn binary_name(&self) -> Option<&'a str> {
        self.data().binary_name.as_deref()
    }
    /// `getPackage().getName()`.
    pub fn package_name(&self) -> Option<&'a str> {
        self.data().package.as_deref()
    }
    pub fn erasure(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().erasure)
    }
    pub fn type_declaration(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().type_declaration)
    }
    pub fn declaring_class(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().declaring_class)
    }
    pub fn declaring_method(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().declaring_method)
    }
    pub fn superclass(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().superclass)
    }
    pub fn interfaces(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().interfaces)
    }
    pub fn type_arguments(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().type_arguments)
    }
    pub fn type_parameters(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().type_parameters)
    }
    pub fn type_bounds(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().type_bounds)
    }
    pub fn element_type(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().element_type)
    }
    pub fn component_type(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().component_type)
    }
    pub fn bound(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().bound)
    }
    pub fn wildcard(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().wildcard)
    }
    pub fn dimensions(&self) -> i32 {
        self.data().dimensions
    }
    pub fn declared_methods(&self) -> Option<Vec<BindingRef<'a>>> {
        self.data().declared_methods.as_ref().map(|v| self.many(v))
    }
    pub fn constructors(&self) -> Vec<BindingRef<'a>> {
        self.data().constructors.as_ref()
            .map(|v| self.many(v)).unwrap_or_default()
    }
    pub fn declared_fields(&self) -> Option<Vec<BindingRef<'a>>> {
        self.data().declared_fields.as_ref().map(|v| self.many(v))
    }
    pub fn declared_types(&self) -> Option<Vec<BindingRef<'a>>> {
        self.data().declared_types.as_ref().map(|v| self.many(v))
    }
    /// `IVariableBinding.getType()`.
    pub fn var_type(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().var_type)
    }
    pub fn variable_declaration(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().variable_declaration)
    }
    pub fn constant_value(&self) -> Option<&'a str> {
        self.data().constant_value.as_deref()
    }
    pub fn return_type(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().return_type)
    }
    pub fn method_declaration(&self) -> Option<BindingRef<'a>> {
        self.opt(self.data().method_declaration)
    }
    pub fn parameter_types(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().parameter_types)
    }
    pub fn exception_types(&self) -> Vec<BindingRef<'a>> {
        self.many(&self.data().exception_types)
    }

    /// The declaring node of this binding in its AST
    /// (`CompilationUnit.findDeclaringNode(binding)`).
    pub fn declaring_node(&self) -> Option<Node<'a>> {
        let key = self.key();
        self.ast.all_nodes().find(|n| {
            let declares = n.kind().is_abstract_type_declaration()
                || matches!(
                    n.kind(),
                    NodeKind::AnonymousClassDeclaration
                        | NodeKind::MethodDeclaration
                        | NodeKind::VariableDeclarationFragment
                        | NodeKind::SingleVariableDeclaration
                        | NodeKind::EnumConstantDeclaration
                        | NodeKind::TypeParameter
                        | NodeKind::AnnotationTypeMemberDeclaration
                );
            declares && n.binding().is_some_and(|b| b.key() == key)
        })
    }
}

// ─── Fetching ─────────────────────────────────────────────────────────────────

const MAX_CACHED: usize = 16;

static CACHE: Lazy<Mutex<Vec<(u64, Arc<Ast>)>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Hash of everything an AST depends on (the cache key).
pub fn content_hash(uri: &str, ctx: &crate::analysis::dispatcher::RequestContext) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut h);
    let mut keys: Vec<&String> = ctx.files.keys().collect();
    keys.sort();
    for k in keys {
        k.hash(&mut h);
        ctx.files[k].hash(&mut h);
    }
    ctx.classpath.hash(&mut h);
    ctx.source_level.hash(&mut h);
    ctx.options.hash(&mut h);
    h.finish()
}

/// The resolved AST of `uri` (cached by content hash).
pub async fn fetch(dispatcher: &crate::analysis::dispatcher::Dispatcher, uri: &tower_lsp::lsp_types::Url) -> anyhow::Result<Arc<Ast>> {
    let ctx = dispatcher.context_for(Some(uri)).await;
    fetch_with(dispatcher, uri.as_str(), ctx).await
}

/// As [`fetch`], for a prepared request context (the unit must be one of
/// `ctx.files`).
pub async fn fetch_with(
    dispatcher: &crate::analysis::dispatcher::Dispatcher,
    uri: &str,
    ctx: crate::analysis::dispatcher::RequestContext,
) -> anyhow::Result<Arc<Ast>> {
    use crate::analysis::semantic::{ecj_process::next_id, BridgeRequest, BridgeResponse};
    let hash = content_hash(uri, &ctx);
    if let Some(ast) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(h, _)| *h == hash).map(|(_, a)| a.clone()) {
        return Ok(ast);
    }
    let source = ctx.files.get(uri).cloned().ok_or_else(|| anyhow::anyhow!("{uri} is not part of the request"))?;
    let resp = dispatcher
        .send_request(BridgeRequest::SemanticAst {
            id: next_id(),
            files: ctx.files,
            classpath: ctx.classpath,
            source_level: ctx.source_level,
            options: ctx.options,
            uri: uri.to_owned(),
            data: Some(format!("{hash:016x}")),
        })
        .await?;
    let data = match resp {
        BridgeResponse::SemanticAst { data, .. } => data,
        BridgeResponse::Error { message, .. } => anyhow::bail!("semanticAst failed: {message}"),
        _ => anyhow::bail!("unexpected semanticAst response"),
    };
    let ast = Arc::new(Ast::from_wire(uri, &source, data));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|(h, _)| *h != hash);
    cache.push((hash, ast.clone()));
    if cache.len() > MAX_CACHED {
        cache.remove(0);
    }
    Ok(ast)
}
