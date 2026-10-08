//! Port of `ImportRemover`: removes imports no longer required once some
//! nodes are removed from the AST.

use std::collections::{HashMap, HashSet};

use super::import_rewrite::ImportRewrite;
use crate::features::organize_imports::operation::import_references;
use crate::semantic_ast::{Ast, BindingKind, BindingRef, Node, NodeId};

#[derive(Default)]
pub struct ImportRemover {
    removed: Vec<NodeId>,
    retained: Vec<NodeId>,
    added_imports: HashSet<String>,
    added_static_imports: HashSet<(String, String, bool)>,
}

impl ImportRemover {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_removed_node(&mut self, node: NodeId) {
        self.removed.push(node);
    }

    pub fn register_retained_node(&mut self, node: NodeId) {
        self.retained.push(node);
    }

    /// `registerAddedImport(typeName)`.
    pub fn register_added_import(&mut self, type_name: &str) {
        self.added_imports.insert(type_name.rsplit('.').next().unwrap_or(type_name).to_owned());
    }

    pub fn register_added_static_import(&mut self, qualifier: &str, member: &str, field: bool) {
        self.added_static_imports.insert((qualifier.to_owned(), member.to_owned(), field));
    }

    pub fn has_removed_nodes(&self) -> bool {
        !self.removed.is_empty()
    }

    fn removed_ranges(&self, root: Node<'_>) -> Vec<(usize, usize)> {
        struct State<'s> {
            remover: &'s ImportRemover,
            removing_start: Option<usize>,
            ranges: Vec<(usize, usize)>,
        }
        fn walk(state: &mut State<'_>, node: Node<'_>) {
            let removed = state.remover.removed.contains(&node.id);
            let retained = state.remover.retained.contains(&node.id);
            let mut active_removed = removed;
            let mut active_retained = retained;
            if removed {
                if state.removing_start.is_none() {
                    state.removing_start = Some(node.start());
                } else {
                    active_removed = false;
                }
            } else if retained {
                if let Some(start) = state.removing_start.take() {
                    state.ranges.push((start, node.start()));
                } else {
                    active_retained = false;
                }
            }
            for child in node.children() {
                walk(state, child);
            }
            if active_retained {
                state.removing_start = Some(node.end());
            } else if active_removed {
                if let Some(start) = state.removing_start.take() {
                    state.ranges.push((start, node.end()));
                }
            }
        }
        let mut state = State { remover: self, removing_start: None, ranges: Vec::new() };
        walk(&mut state, root);
        state.ranges
    }

    fn has_added_static_import(&self, name: Node<'_>) -> bool {
        match name.binding() {
            Some(b) if b.kind() == BindingKind::Variable => b.declaring_class().is_some_and(|c| self.added_static_imports.contains(&(c.qualified_name().to_owned(), b.name().to_owned(), true))),
            Some(b) if b.kind() == BindingKind::Method => b.declaring_class().is_some_and(|c| self.added_static_imports.contains(&(c.qualified_name().to_owned(), b.name().to_owned(), false))),
            _ => false,
        }
    }

    /// `getImportsToRemove()`.
    pub fn imports_to_remove<'a>(&self, ast: &'a Ast) -> Vec<BindingRef<'a>> {
        let root = ast.root();
        let (import_names, static_names) = import_references(root);
        let ranges = self.removed_ranges(root);
        let in_removed = |n: Node<'_>| ranges.iter().any(|(s, e)| n.start() >= *s && n.end() <= *e);
        let mut removed_refs = Vec::new();
        let mut unremoved = HashSet::new();
        for n in import_names.into_iter().chain(static_names) {
            if in_removed(n) {
                removed_refs.push(n);
            } else {
                unremoved.insert(n.identifier());
            }
        }
        if removed_refs.is_empty() {
            return Vec::new();
        }
        let mut potential: HashMap<String, BindingRef<'a>> = HashMap::new();
        let mut order = Vec::new();
        for name in removed_refs {
            let identifier = name.identifier();
            if self.added_imports.contains(&identifier) || self.has_added_static_import(name) {
                continue;
            }
            if let Some(binding) = name.binding() {
                if potential.insert(identifier.clone(), binding).is_none() {
                    order.push(identifier);
                }
            }
        }
        order.into_iter().filter(|i| !unremoved.contains(i)).filter_map(|i| potential.get(&i).copied()).collect()
    }

    /// `applyRemoves(importRewrite)`.
    pub fn apply_removes(&self, ast: &Ast, imports: &mut ImportRewrite) {
        for b in self.imports_to_remove(ast) {
            match b.kind() {
                BindingKind::Type => {
                    imports.remove_import(b.type_declaration().unwrap_or(b).qualified_name());
                }
                BindingKind::Method | BindingKind::Variable => {
                    if let Some(c) = b.declaring_class() {
                        imports.remove_static_import(&format!("{}.{}", c.qualified_name(), b.name()));
                    }
                }
                _ => {}
            }
        }
    }
}
