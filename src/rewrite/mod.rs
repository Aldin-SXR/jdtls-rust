//! Rust port of `org.eclipse.jdt.core.dom.rewrite.ASTRewrite` (plus
//! `ListRewrite`, `ImportRewrite` and the internal rewrite analyzer,
//! flattener and formatter) over the [`semantic_ast`](crate::semantic_ast)
//! model.
//!
//! Usage mirrors JDT:
//!
//! ```ignore
//! let mut rw = ASTRewrite::new(ast.clone());
//! let frag = rw.new_variable_declaration_fragment("serialVersionUID", Some(lit));
//! let field = rw.new_field_declaration(frag);
//! rw.list_insert_at(RNode::Orig(type_decl), "bodyDeclarations", field, 0);
//! let edits = rw.rewrite_ast(&mut formatter_env)?;   // `rewriteAST(document, options)`
//! ```
//!
//! New nodes are [`RNode::New`]; nodes of the original AST are
//! [`RNode::Orig`].  `rewrite_ast` produces an [`text_edit::EditTree`] with
//! exactly the edits JDT's `ASTRewriteAnalyzer` creates; new code is
//! flattened (`ASTRewriteFlattener`) and formatted with the Eclipse code
//! formatter through the bridge (`ASTRewriteFormatter`), so the result
//! matches JDT byte for byte.

pub mod analyzer;
pub mod flattener;
pub mod formatter;
pub mod import_rewrite;
pub mod indent;
pub mod scanner;
pub mod text_edit;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::semantic_ast::{Ast, NodeId, NodeKind, PropValue};

/// A node referenced by a rewrite: original or created by the rewrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RNode {
    Orig(NodeId),
    New(u32),
}

/// A structural property value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Node(Option<RNode>),
    List(Vec<RNode>),
    Simple(Option<String>),
}

impl Value {
    pub fn node(&self) -> Option<RNode> {
        match self {
            Value::Node(n) => *n,
            _ => None,
        }
    }
    pub fn list(&self) -> Vec<RNode> {
        match self {
            Value::List(l) => l.clone(),
            _ => Vec::new(),
        }
    }
    pub fn simple(&self) -> Option<&str> {
        match self {
            Value::Simple(s) => s.as_deref(),
            _ => None,
        }
    }
    pub fn flag(&self) -> bool {
        self.simple() == Some("true")
    }
}

#[derive(Clone, Debug)]
pub enum Placeholder {
    /// `createStringPlaceholder(code, type)`.
    Str(String),
    /// `createCopyTarget` / `createMoveTarget`: index into the copy sources.
    Copy(usize),
}

#[derive(Clone, Debug)]
pub struct NewNode {
    pub kind: NodeKind,
    pub props: Vec<(&'static str, Value)>,
    pub placeholder: Option<Placeholder>,
    /// `NodeInfoStore.createCollapsePlaceholder` (group node).
    pub collapsed: bool,
}

/// `RewriteEventStore.CopySourceInfo`.
#[derive(Clone, Debug)]
pub struct CopySourceInfo {
    pub location: Option<(RNode, &'static str)>,
    pub node: NodeId,
    pub is_move: bool,
    /// A contiguous child-list range copied while its enclosing block is removed.
    pub range: Option<(NodeId, NodeId)>,
}

/// Rewrite change kinds (`RewriteEvent`).
pub mod change {
    pub const UNCHANGED: i32 = 0;
    pub const INSERTED: i32 = 1;
    pub const REMOVED: i32 = 2;
    pub const REPLACED: i32 = 4;
    pub const CHILDREN_CHANGED: i32 = 8;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub original: Option<RNode>,
    pub new: Option<RNode>,
}

impl ListEntry {
    pub fn change_kind(&self) -> i32 {
        node_change_kind(self.original, self.new)
    }
}

fn node_change_kind(original: Option<RNode>, new: Option<RNode>) -> i32 {
    match (original, new) {
        (o, n) if o == n => change::UNCHANGED,
        (None, _) => change::INSERTED,
        (_, None) => change::REMOVED,
        _ => change::REPLACED,
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Node { original: Option<RNode>, new: Option<RNode> },
    List { original: Vec<RNode>, entries: Vec<ListEntry> },
    Simple { original: Option<String>, new: Option<String> },
}

impl Event {
    pub fn change_kind(&self) -> i32 {
        match self {
            Event::Node { original, new } => node_change_kind(*original, *new),
            Event::List { entries, .. } => {
                if entries.iter().any(|e| e.change_kind() != change::UNCHANGED) {
                    change::CHILDREN_CHANGED
                } else {
                    change::UNCHANGED
                }
            }
            Event::Simple { original, new } => {
                if original == new {
                    change::UNCHANGED
                } else {
                    change::REPLACED
                }
            }
        }
    }

    pub fn original_value(&self) -> Value {
        match self {
            Event::Node { original, .. } => Value::Node(*original),
            Event::List { original, .. } => Value::List(original.clone()),
            Event::Simple { original, .. } => Value::Simple(original.clone()),
        }
    }

    pub fn new_value(&self) -> Value {
        match self {
            Event::Node { new, .. } => Value::Node(*new),
            Event::List { entries, .. } => Value::List(entries.iter().filter_map(|e| e.new).collect()),
            Event::Simple { new, .. } => Value::Simple(new.clone()),
        }
    }

    /// `ListRewriteEvent.getChildren()`.
    pub fn children(&self) -> Vec<ListEntry> {
        match self {
            Event::List { entries, .. } => entries.clone(),
            _ => Vec::new(),
        }
    }
}

/// Failure while rewriting (`IllegalArgumentException("Document does not
/// match the AST")`, malformed edits, ...).
#[derive(Debug, Clone)]
pub struct RewriteError(pub String);

impl From<scanner::ScanError> for RewriteError {
    fn from(e: scanner::ScanError) -> Self {
        RewriteError(format!("Document does not match the AST: {}", e.0))
    }
}

impl From<text_edit::MalformedTree> for RewriteError {
    fn from(e: text_edit::MalformedTree) -> Self {
        RewriteError(e.0)
    }
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `ASTRewrite`.
#[derive(Clone)]
pub struct ASTRewrite {
    pub ast: Arc<Ast>,
    pub new_nodes: Vec<NewNode>,
    events: Vec<(RNode, &'static str, Event)>,
    pub copy_sources: Vec<CopySourceInfo>,
    insert_bound_to_previous: HashSet<RNode>,
    /// `TightSourceRangeComputer` nodes (use the plain node range).
    tight_nodes: HashSet<NodeId>,
    /// Overrides from a feature's `TargetSourceRangeComputer`.
    source_ranges: std::collections::HashMap<NodeId, (usize, usize)>,
}

impl ASTRewrite {
    pub fn new(ast: Arc<Ast>) -> Self {
        ASTRewrite {
            ast,
            new_nodes: Vec::new(),
            events: Vec::new(),
            copy_sources: Vec::new(),
            insert_bound_to_previous: HashSet::new(),
            tight_nodes: HashSet::new(),
            source_ranges: std::collections::HashMap::new(),
        }
    }

    pub fn has_changes(&self) -> bool {
        self.events.iter().any(|(_, _, e)| e.change_kind() != change::UNCHANGED)
    }

    // ── Node access ─────────────────────────────────────────────────────────

    pub fn kind(&self, n: RNode) -> NodeKind {
        match n {
            RNode::Orig(id) => self.ast.data(id).kind,
            RNode::New(i) => self.new_nodes[i as usize].kind,
        }
    }

    pub fn is_new(&self, n: RNode) -> bool {
        matches!(n, RNode::New(_))
    }

    pub fn new_node_data(&self, n: RNode) -> Option<&NewNode> {
        match n {
            RNode::New(i) => self.new_nodes.get(i as usize),
            _ => None,
        }
    }

    fn access_original(&self, parent: RNode, prop: &str) -> Value {
        match parent {
            RNode::Orig(id) => match self.ast.node(id).prop(prop) {
                Some(PropValue::Child(c)) => Value::Node(c.map(RNode::Orig)),
                Some(PropValue::List(l)) => Value::List(l.iter().map(|&i| RNode::Orig(i)).collect()),
                Some(PropValue::Simple(s)) => Value::Simple(s.clone()),
                None => Value::Simple(None),
            },
            RNode::New(i) => {
                let nn = &self.new_nodes[i as usize];
                nn.props
                    .iter()
                    .find(|(p, _)| *p == prop)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| default_value(nn.kind, prop))
            }
        }
    }

    pub fn event(&self, parent: RNode, prop: &str) -> Option<&Event> {
        self.events.iter().find(|(p, n, _)| *p == parent && *n == prop).map(|(_, _, e)| e)
    }

    fn event_mut(&mut self, parent: RNode, prop: &str) -> Option<&mut Event> {
        self.events.iter_mut().find(|(p, n, _)| *p == parent && *n == prop).map(|(_, _, e)| e)
    }

    /// `RewriteEventStore.getOriginalValue`.
    pub fn original_value(&self, parent: RNode, prop: &str) -> Value {
        match self.event(parent, prop) {
            Some(e) => e.original_value(),
            None => self.access_original(parent, prop),
        }
    }

    /// `RewriteEventStore.getNewValue`.
    pub fn new_value(&self, parent: RNode, prop: &str) -> Value {
        match self.event(parent, prop) {
            Some(e) => e.new_value(),
            None => self.access_original(parent, prop),
        }
    }

    pub fn change_kind(&self, parent: RNode, prop: &str) -> i32 {
        self.event(parent, prop).map_or(change::UNCHANGED, Event::change_kind)
    }

    pub fn is_changed(&self, parent: RNode, prop: &str) -> bool {
        self.change_kind(parent, prop) != change::UNCHANGED
    }

    /// `RewriteEventStore.hasChangedProperties`.
    pub fn has_changed_properties(&self, parent: RNode) -> bool {
        self.events.iter().any(|(p, _, e)| *p == parent && e.change_kind() != change::UNCHANGED)
    }

    /// `RewriteEventStore.getChangedPropertieEvents`.
    pub fn changed_property_events(&self, parent: RNode) -> Vec<&Event> {
        self.events.iter().filter(|(p, _, e)| *p == parent && e.change_kind() != change::UNCHANGED).map(|(_, _, e)| e).collect()
    }

    /// Structural property ids of a node, in descriptor order.
    pub fn property_names(&self, n: RNode) -> Vec<&'static str> {
        match n {
            RNode::Orig(id) => self.ast.node(id).props().iter().map(|(p, _)| *p).collect(),
            RNode::New(i) => {
                let nn = &self.new_nodes[i as usize];
                let mut names: Vec<&'static str> = property_order(nn.kind).to_vec();
                for (p, _) in &nn.props {
                    if !names.contains(p) {
                        names.push(p);
                    }
                }
                names
            }
        }
    }

    /// Whether `prop` of `n` is a child-list property.
    pub fn is_list_property(&self, n: RNode, prop: &str) -> bool {
        matches!(self.access_original(n, prop), Value::List(_))
    }

    pub fn is_insert_bound_to_previous(&self, n: RNode) -> bool {
        self.insert_bound_to_previous.contains(&n)
    }

    /// `RewriteEventStore.setInsertBoundToPrevious`.
    pub fn set_insert_bound_to_previous(&mut self, n: RNode) {
        self.insert_bound_to_previous.insert(n);
    }

    /// `TightSourceRangeComputer.addTightSourceNode`.
    pub fn add_tight_source_node(&mut self, n: NodeId) {
        self.tight_nodes.insert(n);
        let ast = self.ast.clone();
        let node = ast.node(n);
        let (ps, pe) = (node.start(), node.end());
        for c in node.children() {
            let (es, ee) = (c.extended_start(), c.extended_start() + c.extended_length());
            if ps > es || pe < ee {
                self.add_tight_source_node(c.id);
            }
        }
    }

    /// `TargetSourceRangeComputer.computeSourceRange`: `(start, length)`.
    pub fn extended_range(&self, n: NodeId) -> (usize, usize) {
        if let Some(range) = self.source_ranges.get(&n) {
            return *range;
        }
        let node = self.ast.node(n);
        if self.tight_nodes.contains(&n) {
            (node.start(), node.length())
        } else {
            (node.extended_start(), node.extended_length())
        }
    }

    pub fn set_source_range(&mut self, n: NodeId, start: usize, length: usize) {
        self.source_ranges.insert(n, (start, length));
    }

    // ── Events ──────────────────────────────────────────────────────────────

    fn node_event(&mut self, parent: RNode, prop: &'static str) -> &mut Event {
        if self.event(parent, prop).is_none() {
            let original = self.access_original(parent, prop).node();
            self.events.push((parent, prop, Event::Node { original, new: original }));
        }
        self.event_mut(parent, prop).unwrap()
    }

    fn list_event(&mut self, parent: RNode, prop: &'static str) -> &mut Event {
        if self.event(parent, prop).is_none() {
            let original = self.access_original(parent, prop).list();
            let entries = original.iter().map(|&o| ListEntry { original: Some(o), new: Some(o) }).collect();
            self.events.push((parent, prop, Event::List { original, entries }));
        }
        self.event_mut(parent, prop).unwrap()
    }

    /// `ASTRewrite.set(node, property, value)` for a child property.
    pub fn set(&mut self, parent: RNode, prop: &'static str, value: Option<RNode>) {
        if let RNode::New(i) = parent {
            set_prop(&mut self.new_nodes[i as usize], prop, Value::Node(value));
            return;
        }
        if let Event::Node { new, .. } = self.node_event(parent, prop) {
            *new = value;
        }
    }

    /// `ASTRewrite.set(node, property, value)` for a simple property.
    pub fn set_simple(&mut self, parent: RNode, prop: &'static str, value: Option<&str>) {
        if let RNode::New(i) = parent {
            set_prop(&mut self.new_nodes[i as usize], prop, Value::Simple(value.map(str::to_owned)));
            return;
        }
        if self.event(parent, prop).is_none() {
            let original = self.access_original(parent, prop).simple().map(str::to_owned);
            self.events.push((parent, prop, Event::Simple { original: original.clone(), new: original }));
        }
        if let Some(Event::Simple { new, .. }) = self.event_mut(parent, prop) {
            *new = value.map(str::to_owned);
        }
    }

    /// Location `(parent, property)` of `node` (`getPropertyLocation`).
    pub fn location_of(&self, node: RNode) -> Option<(RNode, &'static str)> {
        match node {
            RNode::Orig(id) => {
                let n = self.ast.node(id);
                Some((RNode::Orig(n.parent()?.id), n.location()?))
            }
            RNode::New(_) => {
                for (p, prop, e) in &self.events {
                    match e {
                        Event::Node { new, .. } if *new == Some(node) => return Some((*p, prop)),
                        Event::List { entries, .. } if entries.iter().any(|en| en.new == Some(node)) => return Some((*p, prop)),
                        _ => {}
                    }
                }
                for (i, nn) in self.new_nodes.iter().enumerate() {
                    for (prop, v) in &nn.props {
                        let hit = match v {
                            Value::Node(Some(c)) => *c == node,
                            Value::List(l) => l.contains(&node),
                            _ => false,
                        };
                        if hit {
                            return Some((RNode::New(i as u32), prop));
                        }
                    }
                }
                None
            }
        }
    }

    /// `ASTRewrite.replace(node, replacement)`.
    pub fn replace(&mut self, node: RNode, replacement: Option<RNode>) {
        let Some((parent, prop)) = self.location_of(node) else { return };
        if self.is_list_property(parent, prop) {
            match replacement {
                Some(r) => self.list_replace(parent, prop, node, r),
                None => self.list_remove(parent, prop, node),
            }
        } else {
            self.set(parent, prop, replacement);
        }
    }

    /// `ASTRewrite.remove(node)`.
    pub fn remove(&mut self, node: RNode) {
        self.replace(node, None);
    }

    // ── ListRewrite ─────────────────────────────────────────────────────────

    /// `ListRewrite.insertAt(node, index)` (`-1` appends).
    pub fn list_insert_at(&mut self, parent: RNode, prop: &'static str, node: RNode, index: i32) {
        if let RNode::New(i) = parent {
            let nn = &mut self.new_nodes[i as usize];
            let mut list = nn.props.iter().find(|(p, _)| *p == prop).map(|(_, v)| v.list()).unwrap_or_default();
            if index < 0 || index as usize >= list.len() {
                list.push(node);
            } else {
                list.insert(index as usize, node);
            }
            set_prop(nn, prop, Value::List(list));
            return;
        }
        if let Event::List { entries, .. } = self.list_event(parent, prop) {
            let entry = ListEntry { original: None, new: Some(node) };
            if index < 0 || index as usize > entries.len() {
                entries.push(entry);
            } else {
                entries.insert(index as usize, entry);
            }
        }
    }

    pub fn list_insert_first(&mut self, parent: RNode, prop: &'static str, node: RNode) {
        self.list_insert_at(parent, prop, node, 0);
    }

    pub fn list_insert_last(&mut self, parent: RNode, prop: &'static str, node: RNode) {
        self.list_insert_at(parent, prop, node, -1);
    }

    /// `ListRewriteEvent.getIndex(node, BOTH)`.
    fn list_index(&mut self, parent: RNode, prop: &'static str, element: RNode) -> i32 {
        if let RNode::New(i) = parent {
            let nn = &self.new_nodes[i as usize];
            let list = nn.props.iter().find(|(p, _)| *p == prop).map(|(_, v)| v.list()).unwrap_or_default();
            return list.iter().position(|&n| n == element).map_or(-1, |p| p as i32);
        }
        if let Event::List { entries, .. } = self.list_event(parent, prop) {
            for i in (0..entries.len()).rev() {
                if entries[i].original == Some(element) || entries[i].new == Some(element) {
                    return i as i32;
                }
            }
        }
        -1
    }

    pub fn list_insert_before(&mut self, parent: RNode, prop: &'static str, node: RNode, element: RNode) {
        let index = self.list_index(parent, prop, element);
        self.list_insert_at(parent, prop, node, index.max(0));
    }

    pub fn list_insert_after(&mut self, parent: RNode, prop: &'static str, node: RNode, element: RNode) {
        let index = self.list_index(parent, prop, element);
        self.list_insert_at(parent, prop, node, if index < 0 { -1 } else { index + 1 });
    }

    /// `ListRewrite.replace(node, replacement)`.
    pub fn list_replace(&mut self, parent: RNode, prop: &'static str, node: RNode, replacement: RNode) {
        self.list_replace_entry(parent, prop, node, Some(replacement));
    }

    /// `ListRewrite.remove(node)`.
    pub fn list_remove(&mut self, parent: RNode, prop: &'static str, node: RNode) {
        self.list_replace_entry(parent, prop, node, None);
    }

    fn list_replace_entry(&mut self, parent: RNode, prop: &'static str, node: RNode, replacement: Option<RNode>) {
        if let RNode::New(i) = parent {
            let nn = &mut self.new_nodes[i as usize];
            let mut list = nn.props.iter().find(|(p, _)| *p == prop).map(|(_, v)| v.list()).unwrap_or_default();
            if let Some(pos) = list.iter().position(|&n| n == node) {
                match replacement {
                    Some(r) => list[pos] = r,
                    None => {
                        list.remove(pos);
                    }
                }
            }
            set_prop(nn, prop, Value::List(list));
            return;
        }
        if let Event::List { entries, .. } = self.list_event(parent, prop) {
            for i in 0..entries.len() {
                if entries[i].original == Some(node) || entries[i].new == Some(node) {
                    entries[i].new = replacement;
                    if entries[i].new.is_none() && entries[i].original.is_none() {
                        entries.remove(i);
                    }
                    return;
                }
            }
        }
    }

    /// `ListRewrite.getRewrittenList()`.
    pub fn list_rewritten(&self, parent: RNode, prop: &str) -> Vec<RNode> {
        self.new_value(parent, prop).list()
    }

    // ── Node creation ───────────────────────────────────────────────────────

    pub fn new_node(&mut self, kind: NodeKind) -> RNode {
        self.new_nodes.push(NewNode { kind, props: Vec::new(), placeholder: None, collapsed: false });
        RNode::New(self.new_nodes.len() as u32 - 1)
    }

    /// Sets a property of a new node (`node.setXxx(..)`).
    pub fn put(&mut self, n: RNode, prop: &'static str, value: Value) -> RNode {
        if let RNode::New(i) = n {
            set_prop(&mut self.new_nodes[i as usize], prop, value);
        }
        n
    }

    pub fn put_child(&mut self, n: RNode, prop: &'static str, child: RNode) -> RNode {
        self.put(n, prop, Value::Node(Some(child)))
    }

    pub fn put_list(&mut self, n: RNode, prop: &'static str, list: Vec<RNode>) -> RNode {
        self.put(n, prop, Value::List(list))
    }

    pub fn put_simple(&mut self, n: RNode, prop: &'static str, value: &str) -> RNode {
        self.put(n, prop, Value::Simple(Some(value.to_owned())))
    }

    /// `ast.newSimpleName(identifier)`.
    pub fn new_simple_name(&mut self, identifier: &str) -> RNode {
        let n = self.new_node(NodeKind::SimpleName);
        self.put_simple(n, "identifier", identifier)
    }

    /// `ast.newName(qualifiedName)`.
    pub fn new_name(&mut self, qualified: &str) -> RNode {
        let mut parts = qualified.split('.');
        let mut name = self.new_simple_name(parts.next().unwrap_or(""));
        for p in parts {
            let simple = self.new_simple_name(p);
            let q = self.new_node(NodeKind::QualifiedName);
            self.put_child(q, "qualifier", name);
            self.put_child(q, "name", simple);
            name = q;
        }
        name
    }

    /// `ast.newPrimitiveType(code)`.
    pub fn new_primitive_type(&mut self, code: &str) -> RNode {
        let n = self.new_node(NodeKind::PrimitiveType);
        self.put_simple(n, "primitiveTypeCode", code)
    }

    /// `ast.newSimpleType(name)`.
    pub fn new_simple_type(&mut self, name: RNode) -> RNode {
        let n = self.new_node(NodeKind::SimpleType);
        self.put_child(n, "name", name)
    }

    /// `ast.newModifier(keyword)`.
    pub fn new_modifier(&mut self, keyword: &str) -> RNode {
        let n = self.new_node(NodeKind::Modifier);
        self.put_simple(n, "keyword", keyword)
    }

    /// `ASTNodeFactory.newModifiers(ast, flags)`.
    pub fn new_modifiers(&mut self, flags: i32) -> Vec<RNode> {
        crate::semantic_ast::modifier::keywords(flags).into_iter().map(|k| self.new_modifier(k)).collect()
    }

    pub fn new_number_literal(&mut self, token: &str) -> RNode {
        let n = self.new_node(NodeKind::NumberLiteral);
        self.put_simple(n, "token", token)
    }

    pub fn new_this_expression(&mut self) -> RNode {
        self.new_node(NodeKind::ThisExpression)
    }

    /// `ast.newFieldAccess()` with expression and name.
    pub fn new_field_access(&mut self, expression: RNode, name: RNode) -> RNode {
        let n = self.new_node(NodeKind::FieldAccess);
        self.put_child(n, "expression", expression);
        self.put_child(n, "name", name)
    }

    /// `ast.newAssignment()` (operator `=`).
    pub fn new_assignment(&mut self, lhs: RNode, operator: &str, rhs: RNode) -> RNode {
        let n = self.new_node(NodeKind::Assignment);
        self.put_child(n, "leftHandSide", lhs);
        self.put_simple(n, "operator", operator);
        self.put_child(n, "rightHandSide", rhs)
    }

    pub fn new_expression_statement(&mut self, expression: RNode) -> RNode {
        let n = self.new_node(NodeKind::ExpressionStatement);
        self.put_child(n, "expression", expression)
    }

    /// `ast.newVariableDeclarationFragment()`.
    pub fn new_variable_declaration_fragment(&mut self, name: &str, initializer: Option<RNode>) -> RNode {
        let n = self.new_node(NodeKind::VariableDeclarationFragment);
        let nm = self.new_simple_name(name);
        self.put_child(n, "name", nm);
        if let Some(i) = initializer {
            self.put_child(n, "initializer", i);
        }
        n
    }

    /// `ast.newFieldDeclaration(fragment)`.
    pub fn new_field_declaration(&mut self, fragment: RNode, modifiers: Vec<RNode>, typ: RNode) -> RNode {
        let n = self.new_node(NodeKind::FieldDeclaration);
        self.put_list(n, "modifiers", modifiers);
        self.put_child(n, "type", typ);
        self.put_list(n, "fragments", vec![fragment])
    }

    pub fn new_method_invocation(&mut self, expression: Option<RNode>, name: &str, arguments: Vec<RNode>) -> RNode {
        let n = self.new_node(NodeKind::MethodInvocation);
        if let Some(e) = expression {
            self.put_child(n, "expression", e);
        }
        let nm = self.new_simple_name(name);
        self.put_child(n, "name", nm);
        self.put_list(n, "arguments", arguments)
    }

    pub fn new_parenthesized_expression(&mut self, expression: RNode) -> RNode {
        let n = self.new_node(NodeKind::ParenthesizedExpression);
        self.put_child(n, "expression", expression)
    }

    pub fn new_infix_expression(&mut self, left: RNode, operator: &str, right: RNode) -> RNode {
        let n = self.new_node(NodeKind::InfixExpression);
        self.put_child(n, "leftOperand", left);
        self.put_simple(n, "operator", operator);
        self.put_child(n, "rightOperand", right)
    }

    pub fn new_block(&mut self, statements: Vec<RNode>) -> RNode {
        let n = self.new_node(NodeKind::Block);
        self.put_list(n, "statements", statements)
    }

    pub fn new_return_statement(&mut self, expression: Option<RNode>) -> RNode {
        let n = self.new_node(NodeKind::ReturnStatement);
        if let Some(e) = expression {
            self.put_child(n, "expression", e);
        }
        n
    }

    /// `ASTNode.copySubtree(ast, node)`.
    pub fn copy_subtree(&mut self, node: RNode) -> RNode {
        let kind = self.kind(node);
        let names = self.property_names(node);
        let copy = self.new_node(kind);
        for prop in names {
            let v = self.access_original(node, prop);
            let nv = match v {
                Value::Node(Some(c)) => Value::Node(Some(self.copy_subtree(c))),
                Value::List(l) => Value::List(l.into_iter().map(|c| self.copy_subtree(c)).collect()),
                other => other,
            };
            self.put(copy, prop, nv);
        }
        copy
    }

    /// `ASTRewrite.createCopyTarget(node)`.
    pub fn create_copy_target(&mut self, node: NodeId) -> RNode {
        self.create_target(node, false)
    }

    /// `ASTRewrite.createMoveTarget(node)`.
    pub fn create_move_target(&mut self, node: NodeId) -> RNode {
        self.create_target(node, true)
    }

    fn create_target(&mut self, node: NodeId, is_move: bool) -> RNode {
        let location = self.location_of(RNode::Orig(node));
        self.copy_sources.push(CopySourceInfo { location, node, is_move, range: None });
        let idx = self.copy_sources.len() - 1;
        let kind = self.ast.data(node).kind;
        let placeholder = self.new_placeholder_node(kind);
        if let RNode::New(i) = placeholder {
            self.new_nodes[i as usize].placeholder = Some(Placeholder::Copy(idx));
        }
        placeholder
    }

    /// ListRewrite.createMoveTarget(first, last) for a block whose ancestor
    /// is removed/replaced. Anchor the source at the enclosing block so the
    /// whole range (including comments and separators) stays under that edit.
    pub(crate) fn move_removed_block_contents(&mut self, block: NodeId) -> RNode {
        self.removed_block_contents(block, true)
    }

    /// ListRewrite.createCopyTarget(first, last) under a removed/replaced ancestor.
    pub(crate) fn copy_removed_block_contents(&mut self, block: NodeId) -> RNode {
        self.removed_block_contents(block, false)
    }

    fn removed_block_contents(&mut self, block: NodeId, is_move: bool) -> RNode {
        let list = self.ast.node(block).list("statements");
        let range = (list.first().expect("nonempty block").id, list.last().unwrap().id);
        self.copy_sources.push(CopySourceInfo { location: None, node: block, is_move, range: Some(range) });
        let info = self.copy_sources.len() - 1;
        let placeholder = self.new_placeholder_node(NodeKind::Block);
        if let RNode::New(i) = placeholder {
            self.new_nodes[i as usize].placeholder = Some(Placeholder::Copy(info));
        }
        placeholder
    }

    /// `ASTRewrite.createStringPlaceholder(code, nodeType)`.
    pub fn create_string_placeholder(&mut self, code: &str, kind: NodeKind) -> RNode {
        let placeholder = self.new_placeholder_node(kind);
        if let RNode::New(i) = placeholder {
            self.new_nodes[i as usize].placeholder = Some(Placeholder::Str(code.to_owned()));
        }
        placeholder
    }

    /// `ASTRewrite.createGroupNode(targetNodes)`.
    pub fn create_group_node(&mut self, targets: Vec<RNode>) -> RNode {
        let block = self.new_node(NodeKind::Block);
        if let RNode::New(i) = block {
            self.new_nodes[i as usize].collapsed = true;
        }
        self.put_list(block, "statements", targets)
    }

    /// `NodeInfoStore.newPlaceholderNode(nodeType)`.
    fn new_placeholder_node(&mut self, kind: NodeKind) -> RNode {
        let n = self.new_node(kind);
        match kind {
            NodeKind::FieldDeclaration | NodeKind::VariableDeclarationExpression | NodeKind::VariableDeclarationStatement => {
                let f = self.new_node(NodeKind::VariableDeclarationFragment);
                self.put_list(n, "fragments", vec![f]);
            }
            NodeKind::TryStatement => {
                let b = self.new_block(Vec::new());
                self.put_child(n, "finally", b);
            }
            NodeKind::ParameterizedType => {
                let w = self.new_node(NodeKind::WildcardType);
                self.put_list(n, "typeArguments", vec![w]);
            }
            NodeKind::Modifier => {
                self.put_simple(n, "keyword", "abstract");
            }
            _ => {}
        }
        n
    }

    /// Runs the rewrite analyzer: `ASTRewrite.rewriteAST(document, options)`.
    /// `formatter` supplies the code formatter results (see
    /// [`formatter::FormatterCache`]).
    pub fn rewrite_ast(&self, options: &BTreeMap<String, String>, formatter: &mut dyn formatter::CodeFormatter) -> Result<text_edit::EditTree, RewriteError> {
        let mut rw = self.clone();
        rw.prepare_moved_nodes();
        analyzer::rewrite(&rw, options, formatter)
    }

    /// `RewriteEventStore.prepareMovedNodes` (single node copies): moved
    /// nodes that are otherwise unchanged are marked as removed.
    fn prepare_moved_nodes(&mut self) {
        let moves: Vec<CopySourceInfo> = self.copy_sources.iter().filter(|c| c.is_move && c.location.is_some()).cloned().collect();
        for info in moves {
            let (parent, prop) = info.location.unwrap();
            let node = RNode::Orig(info.node);
            if self.is_list_property(parent, prop) {
                if let Event::List { entries, .. } = self.list_event(parent, prop) {
                    let idx = (0..entries.len()).rev().find(|&i| entries[i].original == Some(node));
                    if let Some(i) = idx {
                        if entries[i].change_kind() == change::UNCHANGED {
                            entries[i].new = None;
                        }
                    }
                }
            } else if let Event::Node { original, new } = self.node_event(parent, prop) {
                if node_change_kind(*original, *new) == change::UNCHANGED {
                    *new = None;
                }
            }
        }
    }

    /// Copy sources of `node`, sorted (`getNodeCopySources`).
    pub fn node_copy_sources(&self, node: NodeId) -> Vec<usize> {
        let mut res: Vec<usize> = (0..self.copy_sources.len()).filter(|&i| self.copy_sources[i].node == node).collect();
        res.sort_by(|&a, &b| {
            let (ca, cb) = (&self.copy_sources[a], &self.copy_sources[b]);
            match (ca.is_move, cb.is_move) {
                (x, y) if x == y => std::cmp::Ordering::Equal,
                (true, _) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Greater,
            }
        });
        res
    }
}

fn set_prop(nn: &mut NewNode, prop: &'static str, value: Value) {
    if let Some(slot) = nn.props.iter_mut().find(|(p, _)| *p == prop) {
        slot.1 = value;
    } else {
        nn.props.push((prop, value));
    }
}

/// Structural property order of node types (JDT descriptor order) for new
/// nodes; properties not listed are appended in insertion order.
pub fn property_order(kind: NodeKind) -> &'static [&'static str] {
    use NodeKind::*;
    match kind {
        AnonymousClassDeclaration => &["bodyDeclarations"],
        ArrayAccess => &["array", "index"],
        ArrayCreation => &["type", "dimensions", "initializer"],
        ArrayInitializer => &["expressions"],
        ArrayType => &["elementType", "dimensions"],
        AssertStatement => &["expression", "message"],
        Assignment => &["leftHandSide", "operator", "rightHandSide"],
        Block => &["statements"],
        BooleanLiteral => &["booleanValue"],
        BreakStatement => &["label"],
        CastExpression => &["type", "expression"],
        CatchClause => &["exception", "body"],
        CharacterLiteral => &["escapedValue"],
        ClassInstanceCreation => &["expression", "typeArguments", "type", "arguments", "anonymousClassDeclaration"],
        CompilationUnit => &["package", "imports", "types", "module"],
        ConditionalExpression => &["expression", "thenExpression", "elseExpression"],
        ConstructorInvocation => &["typeArguments", "arguments"],
        ContinueStatement => &["label"],
        DoStatement => &["body", "expression"],
        ExpressionStatement => &["expression"],
        FieldAccess => &["expression", "name"],
        FieldDeclaration => &["javadoc", "modifiers", "type", "fragments"],
        ForStatement => &["initializers", "expression", "updaters", "body"],
        IfStatement => &["expression", "thenStatement", "elseStatement"],
        ImportDeclaration => &["static", "modifiers", "name", "onDemand"],
        InfixExpression => &["leftOperand", "operator", "rightOperand", "extendedOperands"],
        Initializer => &["javadoc", "modifiers", "body"],
        Javadoc => &["tags"],
        LabeledStatement => &["label", "body"],
        MethodDeclaration => &[
            "javadoc", "modifiers", "constructor", "typeParameters", "returnType2", "name", "receiverType", "receiverQualifier",
            "parameters", "extraDimensions2", "thrownExceptionTypes", "body",
        ],
        MethodInvocation => &["expression", "typeArguments", "name", "arguments"],
        NumberLiteral => &["token"],
        PackageDeclaration => &["javadoc", "annotations", "name"],
        ParenthesizedExpression => &["expression"],
        PostfixExpression => &["operand", "operator"],
        PrefixExpression => &["operator", "operand"],
        PrimitiveType => &["annotations", "primitiveTypeCode"],
        QualifiedName => &["qualifier", "name"],
        ReturnStatement => &["expression"],
        SimpleName => &["identifier"],
        SimpleType => &["annotations", "name"],
        SingleVariableDeclaration => &["modifiers", "type", "varargsAnnotations", "varargs", "name", "extraDimensions2", "initializer"],
        StringLiteral => &["escapedValue"],
        SuperConstructorInvocation => &["expression", "typeArguments", "arguments"],
        SuperFieldAccess => &["qualifier", "name"],
        SuperMethodInvocation => &["qualifier", "typeArguments", "name", "arguments"],
        SwitchCase => &["expression", "switchLabeledRule"],
        SwitchStatement => &["expression", "statements"],
        SynchronizedStatement => &["expression", "body"],
        ThisExpression => &["qualifier"],
        ThrowStatement => &["expression"],
        TryStatement => &["resources", "body", "catchClauses", "finally"],
        TypeDeclaration => &[
            "javadoc", "modifiers", "interface", "name", "typeParameters", "superclassType", "superInterfaceTypes", "permitsTypes",
            "bodyDeclarations",
        ],
        TypeDeclarationStatement => &["declaration"],
        TypeLiteral => &["type"],
        VariableDeclarationExpression => &["modifiers", "type", "fragments"],
        VariableDeclarationFragment => &["name", "extraDimensions2", "initializer"],
        VariableDeclarationStatement => &["modifiers", "type", "fragments"],
        WhileStatement => &["expression", "body"],
        InstanceofExpression => &["leftOperand", "rightOperand"],
        TagElement => &["tagName", "fragments"],
        TextElement => &["text"],
        MemberRef => &["qualifier", "name"],
        MethodRef => &["qualifier", "name", "parameters"],
        MethodRefParameter => &["type", "varargs", "name"],
        EnhancedForStatement => &["parameter", "expression", "body"],
        EnumDeclaration => &["javadoc", "modifiers", "name", "superInterfaceTypes", "enumConstants", "bodyDeclarations"],
        EnumConstantDeclaration => &["javadoc", "modifiers", "name", "arguments", "anonymousClassDeclaration"],
        TypeParameter => &["modifiers", "name", "typeBounds"],
        ParameterizedType => &["type", "typeArguments"],
        QualifiedType => &["qualifier", "annotations", "name"],
        WildcardType => &["annotations", "bound", "upperBound"],
        NormalAnnotation => &["typeName", "values"],
        MarkerAnnotation => &["typeName"],
        SingleMemberAnnotation => &["typeName", "value"],
        MemberValuePair => &["name", "value"],
        AnnotationTypeDeclaration => &["javadoc", "modifiers", "name", "bodyDeclarations"],
        AnnotationTypeMemberDeclaration => &["javadoc", "modifiers", "type", "name", "default"],
        Modifier => &["keyword"],
        UnionType => &["types"],
        Dimension => &["annotations"],
        LambdaExpression => &["parentheses", "parameters", "body"],
        IntersectionType => &["types"],
        NameQualifiedType => &["qualifier", "annotations", "name"],
        CreationReference => &["type", "typeArguments"],
        ExpressionMethodReference => &["expression", "typeArguments", "name"],
        SuperMethodReference => &["qualifier", "typeArguments", "name"],
        TypeMethodReference => &["type", "typeArguments", "name"],
        SwitchExpression => &["expression", "statements"],
        YieldStatement => &["expression"],
        TextBlock => &["escapedValue"],
        RecordDeclaration => &["javadoc", "modifiers", "name", "typeParameters", "recordComponents", "superInterfaceTypes", "bodyDeclarations"],
        PatternInstanceofExpression => &["leftOperand", "pattern"],
        _ => &[],
    }
}

/// Default property values of a freshly created JDT node.
pub fn default_value(kind: NodeKind, prop: &str) -> Value {
    use NodeKind::*;
    let list_props = [
        "bodyDeclarations", "dimensions", "expressions", "statements", "typeArguments", "arguments", "imports", "types", "fragments",
        "initializers", "updaters", "modifiers", "extendedOperands", "tags", "typeParameters", "parameters", "extraDimensions2",
        "thrownExceptionTypes", "annotations", "varargsAnnotations", "resources", "catchClauses", "superInterfaceTypes",
        "permitsTypes", "enumConstants", "typeBounds", "values", "recordComponents", "moduleDirectives", "modules", "implementations",
        "expression2",
    ];
    if list_props.contains(&prop) && !(kind == SwitchCase && prop == "expression") {
        return Value::List(Vec::new());
    }
    let simple = |s: &str| Value::Simple(Some(s.to_owned()));
    match (kind, prop) {
        (SimpleName, "identifier") => simple("MISSING"),
        (PrimitiveType, "primitiveTypeCode") => simple("int"),
        (Modifier, "keyword") => simple("public"),
        (NumberLiteral, "token") => simple("0"),
        (StringLiteral, "escapedValue") => simple("\"\""),
        (CharacterLiteral, "escapedValue") => simple("'X'"),
        (TextBlock, "escapedValue") => simple("\"\"\"\n\"\"\""),
        (BooleanLiteral, "booleanValue") => simple("false"),
        (Assignment, "operator") => simple("="),
        (InfixExpression, "operator") => simple("+"),
        (PrefixExpression, "operator") | (PostfixExpression, "operator") => simple("++"),
        (WildcardType, "upperBound") => simple("true"),
        (LambdaExpression, "parentheses") => simple("true"),
        (TextElement, "text") => simple(""),
        (_, "interface" | "constructor" | "varargs" | "static" | "onDemand" | "switchLabeledRule" | "compactConstructor" | "open") => {
            simple("false")
        }
        _ => Value::Node(None),
    }
}
