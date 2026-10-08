//! Port of `org.eclipse.jdt.internal.corext.refactoring.structure.ImportRemover`.

use std::collections::{HashMap, HashSet};

use crate::features::organize_imports::operation::import_references;
use crate::rewrite::import_rewrite::ImportRewrite;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{Ast, BindingRef, Node, NodeId, NodeKind};

#[derive(Default, Clone)]
pub struct ImportRemover {
    removed: HashSet<NodeId>,
    retained: HashSet<NodeId>,
    added_imports: HashSet<String>,
    added_static_imports: HashSet<(String, String, bool)>,
    inlined_static_imports: Vec<NodeId>,
    has_removed_nodes: bool,
}

impl ImportRemover {
    pub fn register_removed_node(&mut self, node: NodeId) {
        self.has_removed_nodes = true;
        self.removed.insert(node);
    }

    pub fn register_retained_node(&mut self, node: NodeId) {
        self.retained.insert(node);
    }

    pub fn register_inlined_static_import(&mut self, node: NodeId) {
        self.inlined_static_imports.push(node);
    }

    pub fn register_added_import(&mut self, type_name: &str) {
        let simple = type_name.rsplit('.').next().unwrap_or(type_name);
        self.added_imports.insert(simple.to_owned());
    }

    /// `registerAddedImports(Type)` for a type node of the rewrite.
    pub fn register_added_imports(&mut self, rw: &ASTRewrite, node: RNode) {
        match rw.kind(node) {
            NodeKind::NameQualifiedType | NodeKind::QualifiedType | NodeKind::QualifiedName => {
                if let Some(name) = rw.new_value(node, "name").node() {
                    self.add_name(rw, name);
                }
            }
            NodeKind::SimpleName => self.add_name(rw, node),
            _ => {
                for prop in rw.property_names(node) {
                    let value = rw.new_value(node, prop);
                    let children = match &value {
                        crate::rewrite::Value::Node(Some(c)) => vec![*c],
                        crate::rewrite::Value::List(l) => l.clone(),
                        _ => Vec::new(),
                    };
                    for c in children {
                        self.register_added_imports(rw, c);
                    }
                }
            }
        }
    }

    fn add_name(&mut self, rw: &ASTRewrite, name: RNode) {
        if let Some(id) = rw.new_value(name, "identifier").simple() {
            self.added_imports.insert(id.to_owned());
        }
    }

    pub fn register_added_static_import(&mut self, declaring_type: &str, member: &str, field: bool) {
        self.added_static_imports.insert((declaring_type.to_owned(), member.to_owned(), field));
    }

    pub fn has_removed_nodes(&self) -> bool {
        self.has_removed_nodes || !self.inlined_static_imports.is_empty()
    }

    fn has_added_static_import(&self, name: Node<'_>) -> bool {
        let Some(binding) = name.binding() else { return false };
        let Some(declaring) = binding.declaring_class() else { return false };
        if binding.is_variable() {
            self.added_static_imports.contains(&(declaring.qualified_name().to_owned(), binding.name().to_owned(), true))
        } else if binding.is_method() {
            self.added_static_imports.contains(&(declaring.qualified_name().to_owned(), binding.name().to_owned(), false))
        } else {
            false
        }
    }

    /// `divideTypeRefs`: the removed ranges of the unit.
    fn removed_ranges(&self, root: Node<'_>) -> Vec<(usize, usize)> {
        struct State<'s> {
            remover: &'s ImportRemover,
            removing_start: Option<usize>,
            ranges: Vec<(usize, usize)>,
            ignored: HashSet<NodeId>,
        }
        fn visit(state: &mut State<'_>, node: Node<'_>) {
            if state.remover.removed.contains(&node.id) {
                if state.removing_start.is_none() {
                    state.removing_start = Some(node.start());
                } else {
                    state.ignored.insert(node.id);
                }
            } else if state.remover.retained.contains(&node.id) {
                if let Some(start) = state.removing_start.take() {
                    state.ranges.push((start, node.start()));
                } else {
                    state.ignored.insert(node.id);
                }
            }
            for child in node.children() {
                visit(state, child);
            }
            if state.remover.retained.contains(&node.id) && !state.ignored.contains(&node.id) {
                state.removing_start = Some(node.end());
            } else if state.remover.removed.contains(&node.id) && !state.ignored.contains(&node.id) {
                if let Some(start) = state.removing_start.take() {
                    state.ranges.push((start, node.end()));
                }
            }
        }
        let mut state = State { remover: self, removing_start: None, ranges: Vec::new(), ignored: HashSet::new() };
        visit(&mut state, root);
        state.ranges
    }

    /// `getImportsToRemove()`: `(type bindings, static member bindings)`.
    fn imports_to_remove<'a>(&self, ast: &'a Ast) -> Vec<BindingRef<'a>> {
        let root = ast.root();
        let (import_names, static_names) = import_references(root);
        let ranges = self.removed_ranges(root);
        let in_removed = |n: Node<'_>| ranges.iter().any(|(s, e)| n.start() >= *s && n.end() <= *e);
        let mut removed_refs = Vec::new();
        let mut unremoved_refs = Vec::new();
        for name in import_names.iter().chain(static_names.iter()) {
            if in_removed(*name) {
                removed_refs.push(*name);
            } else {
                unremoved_refs.push(*name);
            }
        }
        for declaration in &self.inlined_static_imports {
            let mut name = ast.node(*declaration).child("name");
            if let Some(n) = name.filter(|n| n.is(NodeKind::QualifiedName)) {
                name = n.child("name");
            }
            if let Some(n) = name {
                removed_refs.push(n);
            }
        }
        if removed_refs.is_empty() {
            return Vec::new();
        }
        let mut potential: HashMap<String, BindingRef<'a>> = HashMap::new();
        let mut order = Vec::new();
        for name in &removed_refs {
            let identifier = name.identifier();
            if self.added_imports.contains(&identifier) || self.has_added_static_import(*name) {
                continue;
            }
            if let Some(binding) = name.binding() {
                if potential.insert(identifier.clone(), binding).is_none() {
                    order.push(identifier);
                }
            }
        }
        for name in &unremoved_refs {
            potential.remove(&name.identifier());
        }
        order.into_iter().filter_map(|k| potential.remove(&k)).collect()
    }

    /// `applyRemoves(importRewrite)`.
    pub fn apply_removes(&self, ast: &Ast, imports: &mut ImportRewrite) {
        for binding in self.imports_to_remove(ast) {
            if binding.is_type() {
                let declaration = binding.type_declaration().unwrap_or(binding);
                imports.remove_import(declaration.qualified_name());
            } else if let Some(declaring) = binding.declaring_class() {
                imports.remove_static_import(&format!("{}.{}", declaring.qualified_name(), binding.name()));
            }
        }
    }
}
