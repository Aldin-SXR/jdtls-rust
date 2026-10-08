//! Port of `org.eclipse.jdt.internal.corext.refactoring.code.flow`
//! (`FlowInfo` and subclasses, `FlowContext`, `FlowAnalyzer`,
//! `InOutFlowAnalyzer`, `InputFlowAnalyzer` and its `LoopReentranceVisitor`).
//!
//! Java shares access mode arrays and branch / type variable sets between
//! flow infos (`fAccessModes= others`); the port keeps that aliasing with
//! reference-counted cells.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::semantic_ast::{BindingId, Node, NodeId, NodeKind};

use super::selection::Selection;

// Return statement handling.
const NOT_POSSIBLE: usize = 0;
const UNDEFINED: usize = 1;
const NO_RETURN: usize = 2;
const PARTIAL_RETURN: usize = 3;
const VOID_RETURN: usize = 4;
const VALUE_RETURN: usize = 5;
const THROW: usize = 6;

// Local access handling.
pub const UNUSED: i32 = 1 << 0;
pub const READ: i32 = 1 << 1;
pub const READ_POTENTIAL: i32 = 1 << 2;
pub const WRITE: i32 = 1 << 3;
pub const WRITE_POTENTIAL: i32 = 1 << 4;
pub const UNKNOWN: i32 = 1 << 5;

const ACCESS_MODE_CONDITIONAL_TABLE: [[i32; 6]; 6] = [
    [UNUSED, READ_POTENTIAL, READ_POTENTIAL, WRITE_POTENTIAL, WRITE_POTENTIAL, UNKNOWN],
    [READ_POTENTIAL, READ, READ_POTENTIAL, UNKNOWN, UNKNOWN, UNKNOWN],
    [READ_POTENTIAL, READ_POTENTIAL, READ_POTENTIAL, UNKNOWN, UNKNOWN, UNKNOWN],
    [WRITE_POTENTIAL, UNKNOWN, UNKNOWN, WRITE, WRITE_POTENTIAL, UNKNOWN],
    [WRITE_POTENTIAL, UNKNOWN, UNKNOWN, WRITE_POTENTIAL, WRITE_POTENTIAL, UNKNOWN],
    [UNKNOWN, UNKNOWN, UNKNOWN, UNKNOWN, UNKNOWN, UNKNOWN],
];

const ACCESS_MODE_OPEN_BRANCH_TABLE: [i32; 6] = [UNUSED, READ_POTENTIAL, READ_POTENTIAL, WRITE_POTENTIAL, WRITE_POTENTIAL, UNKNOWN];

const RETURN_KIND_CONDITIONAL_TABLE: [[usize; 7]; 7] = [
    [NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE],
    [NOT_POSSIBLE, UNDEFINED, NO_RETURN, PARTIAL_RETURN, VOID_RETURN, VALUE_RETURN, THROW],
    [NOT_POSSIBLE, NO_RETURN, NO_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, NO_RETURN],
    [NOT_POSSIBLE, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN],
    [NOT_POSSIBLE, VOID_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, VOID_RETURN, NOT_POSSIBLE, VOID_RETURN],
    [NOT_POSSIBLE, VALUE_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, NOT_POSSIBLE, VALUE_RETURN, VALUE_RETURN],
    [NOT_POSSIBLE, THROW, NO_RETURN, PARTIAL_RETURN, VOID_RETURN, VALUE_RETURN, THROW],
];

const RETURN_KIND_SEQUENTIAL_TABLE: [[usize; 7]; 7] = [
    [NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE, NOT_POSSIBLE],
    [NOT_POSSIBLE, UNDEFINED, NO_RETURN, PARTIAL_RETURN, VOID_RETURN, VALUE_RETURN, THROW],
    [NOT_POSSIBLE, NO_RETURN, NO_RETURN, PARTIAL_RETURN, VOID_RETURN, VALUE_RETURN, THROW],
    [NOT_POSSIBLE, PARTIAL_RETURN, PARTIAL_RETURN, PARTIAL_RETURN, VOID_RETURN, VALUE_RETURN, VALUE_RETURN],
    [NOT_POSSIBLE, VOID_RETURN, VOID_RETURN, PARTIAL_RETURN, VOID_RETURN, NOT_POSSIBLE, NOT_POSSIBLE],
    [NOT_POSSIBLE, VALUE_RETURN, VALUE_RETURN, PARTIAL_RETURN, NOT_POSSIBLE, VALUE_RETURN, NOT_POSSIBLE],
    [NOT_POSSIBLE, THROW, THROW, VALUE_RETURN, VOID_RETURN, VALUE_RETURN, THROW],
];

const UNLABELED: &str = "@unlabeled";

fn index_of(mode: i32) -> usize {
    match mode {
        UNUSED => 0,
        READ => 1,
        READ_POTENTIAL => 2,
        WRITE => 3,
        WRITE_POTENTIAL => 4,
        _ => 5,
    }
}

type Shared<T> = Rc<RefCell<T>>;

/// `FlowContext.MERGE` / `ARGUMENTS` / `RETURN_VALUES`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComputeMode {
    None,
    Merge,
    Arguments,
    ReturnValues,
}

/// `FlowContext`.
pub struct FlowContext {
    start: i32,
    length: i32,
    consider_access_mode: bool,
    loop_reentrance: Cell<bool>,
    compute_mode: ComputeMode,
    locals: RefCell<Option<Vec<Option<BindingId>>>>,
}

impl FlowContext {
    pub fn new(start: i32, length: i32) -> Self {
        FlowContext { start, length, consider_access_mode: false, loop_reentrance: Cell::new(false), compute_mode: ComputeMode::None, locals: RefCell::new(None) }
    }

    pub fn set_consider_access_mode(&mut self, b: bool) {
        self.consider_access_mode = b;
    }

    pub fn set_compute_mode(&mut self, mode: ComputeMode) {
        self.compute_mode = mode;
    }

    /// `getLocalFromIndex(index)`.
    pub fn local_from_index(&self, index: usize) -> Option<BindingId> {
        self.locals.borrow().as_ref().and_then(|l| l.get(index).copied().flatten())
    }

    /// `getIndexFromLocal(local)`.
    pub fn index_from_local(&self, local: BindingId) -> Option<usize> {
        self.locals.borrow().as_ref().and_then(|l| l.iter().position(|b| *b == Some(local)))
    }

    fn manage_local(&self, local: BindingId, variable_id: i32) {
        let mut locals = self.locals.borrow_mut();
        let l = locals.get_or_insert_with(|| vec![None; self.length.max(0) as usize]);
        let i = variable_id - self.start;
        if i >= 0 && (i as usize) < l.len() {
            l[i as usize] = Some(local);
        }
    }
}

/// Subclass state of a `FlowInfo`.
#[derive(Clone)]
enum Extra {
    None,
    Local { variable_id: i32 },
    DoWhile { action_branches: bool },
    Switch { cases: FlowRef, has_null_case: bool },
}

/// `FlowInfo`.
pub struct FlowInfo {
    return_kind: usize,
    access: Option<Shared<Vec<i32>>>,
    branches: Option<Shared<Vec<String>>>,
    type_variables: Option<Shared<Vec<BindingId>>>,
    extra: Extra,
}

pub type FlowRef = Rc<RefCell<FlowInfo>>;

fn new_info(return_kind: usize, extra: Extra) -> FlowRef {
    Rc::new(RefCell::new(FlowInfo { return_kind, access: None, branches: None, type_variables: None, extra }))
}

/// `new GenericSequentialFlowInfo()`.
pub fn sequential() -> FlowRef {
    new_info(NO_RETURN, Extra::None)
}

fn conditional_info() -> FlowRef {
    new_info(UNDEFINED, Extra::None)
}

fn merge_sets<T: Clone + PartialEq>(this: &mut Option<Shared<Vec<T>>>, other: &Option<Shared<Vec<T>>>) {
    if let Some(o) = other {
        match this {
            None => *this = Some(o.clone()),
            Some(t) => {
                if !Rc::ptr_eq(t, o) {
                    let items = o.borrow().clone();
                    let mut t = t.borrow_mut();
                    for i in items {
                        if !t.contains(&i) {
                            t.push(i);
                        }
                    }
                }
            }
        }
    }
}

impl FlowInfo {
    pub fn is_undefined(&self) -> bool {
        self.return_kind == UNDEFINED
    }
    pub fn is_no_return(&self) -> bool {
        self.return_kind == NO_RETURN
    }
    pub fn is_partial_return(&self) -> bool {
        self.return_kind == PARTIAL_RETURN
    }
    pub fn is_void_return(&self) -> bool {
        self.return_kind == VOID_RETURN
    }
    pub fn is_value_return(&self) -> bool {
        self.return_kind == VALUE_RETURN
    }
    pub fn is_throw(&self) -> bool {
        self.return_kind == THROW
    }
    pub fn is_return(&self) -> bool {
        self.return_kind == VOID_RETURN || self.return_kind == VALUE_RETURN
    }
    pub fn branches(&self) -> bool {
        self.branches.as_ref().is_some_and(|b| !b.borrow().is_empty())
    }

    /// `getTypeVariables()`.
    pub fn type_variables(&self) -> Vec<BindingId> {
        self.type_variables.as_ref().map(|t| t.borrow().clone()).unwrap_or_default()
    }

    /// `get(context, mode)`.
    pub fn get(&self, context: &FlowContext, mode: i32) -> Vec<BindingId> {
        let Some(access) = &self.access else { return Vec::new() };
        let access = access.borrow();
        let mut result = Vec::new();
        for (i, &m) in access.iter().enumerate() {
            if m & mode != 0 {
                if let Some(b) = context.local_from_index(i) {
                    result.push(b);
                }
            }
        }
        result
    }

    /// `hasAccessMode(context, local, mode)`.
    pub fn has_access_mode(&self, context: &FlowContext, local: BindingId, mode: i32) -> bool {
        let unused_mode = mode & UNUSED != 0;
        let Some(access) = &self.access else { return unused_mode };
        match context.index_from_local(local) {
            None => unused_mode,
            Some(i) => access.borrow().get(i).is_some_and(|m| m & mode != 0),
        }
    }

    fn set_no_return(&mut self) {
        self.return_kind = NO_RETURN;
    }

    fn remove_label(&mut self, label: Option<&str>) {
        if let Some(b) = &self.branches {
            let key = label.unwrap_or(UNLABELED).to_owned();
            let empty = {
                let mut b = b.borrow_mut();
                b.retain(|x| *x != key);
                b.is_empty()
            };
            if empty {
                self.branches = None;
            }
        }
    }

    fn add_type_variable(&mut self, t: BindingId) {
        let set = self.type_variables.get_or_insert_with(|| Rc::new(RefCell::new(Vec::new())));
        let mut s = set.borrow_mut();
        if !s.contains(&t) {
            s.push(t);
        }
    }

    fn create_access_mode_array(&mut self, context: &FlowContext) {
        self.access = Some(Rc::new(RefCell::new(vec![UNUSED; context.length.max(0) as usize])));
    }

    fn clear_access_mode(&mut self, variable_id: i32, context: &FlowContext) {
        if let Some(a) = &self.access {
            let i = variable_id - context.start;
            let mut a = a.borrow_mut();
            if i >= 0 && (i as usize) < a.len() {
                a[i as usize] = UNUSED;
            }
        }
    }
}

// ─── Merging (borrow the infos only as long as needed) ──────────────────────

fn assign_execution_flow(this: &FlowRef, right: &FlowRef) {
    let (rk, br) = {
        let r = right.borrow();
        (r.return_kind, r.branches.clone())
    };
    let mut t = this.borrow_mut();
    t.return_kind = rk;
    t.branches = br;
}

fn assign_access_mode(this: &FlowRef, right: &FlowRef) {
    let a = right.borrow().access.clone();
    this.borrow_mut().access = a;
}

fn assign(this: &FlowRef, right: &FlowRef) {
    assign_execution_flow(this, right);
    assign_access_mode(this, right);
}

fn merge_conditional(this: &FlowRef, info: &FlowRef, context: &FlowContext) {
    merge_access_mode_conditional(this, info, context);
    merge_execution_flow_conditional(this, info);
    let other = info.borrow().type_variables.clone();
    merge_sets(&mut this.borrow_mut().type_variables, &other);
}

fn merge_sequential(this: &FlowRef, info: &FlowRef, context: &FlowContext) {
    merge_access_mode_sequential(this, info, context);
    merge_execution_flow_sequential(this, info);
    let other = info.borrow().type_variables.clone();
    merge_sets(&mut this.borrow_mut().type_variables, &other);
}

fn merge_execution_flow_sequential(this: &FlowRef, info: &FlowRef) {
    let (mut other, other_branches) = {
        let o = info.borrow();
        (o.return_kind, o.branches.clone())
    };
    let mut t = this.borrow_mut();
    if t.branches() && other == VALUE_RETURN {
        other = PARTIAL_RETURN;
    }
    t.return_kind = RETURN_KIND_SEQUENTIAL_TABLE[t.return_kind][other];
    merge_sets(&mut t.branches, &other_branches);
}

fn merge_execution_flow_conditional(this: &FlowRef, info: &FlowRef) {
    let (other, other_branches) = {
        let o = info.borrow();
        (o.return_kind, o.branches.clone())
    };
    let mut t = this.borrow_mut();
    t.return_kind = RETURN_KIND_CONDITIONAL_TABLE[t.return_kind][other];
    merge_sets(&mut t.branches, &other_branches);
}

fn merge_access_mode_sequential(this: &FlowRef, info: &FlowRef, context: &FlowContext) {
    if !context.consider_access_mode {
        return;
    }
    let Some(others) = info.borrow().access.clone() else { return };
    if this.borrow().branches() {
        let mut o = others.borrow_mut();
        for m in o.iter_mut() {
            *m = ACCESS_MODE_OPEN_BRANCH_TABLE[index_of(*m)];
        }
    }
    let mine = this.borrow().access.clone();
    let Some(mine) = mine else {
        this.borrow_mut().access = Some(others);
        return;
    };
    let others: Vec<i32> = others.borrow().clone();
    let mut a = mine.borrow_mut();
    match context.compute_mode {
        ComputeMode::Arguments => {
            for i in 0..a.len().min(others.len()) {
                let (access, other) = (a[i], others[i]);
                if access == UNUSED || (access == WRITE_POTENTIAL && (other == READ || other == READ_POTENTIAL)) {
                    a[i] = other;
                } else if access == WRITE_POTENTIAL && other == WRITE {
                    a[i] = WRITE;
                }
            }
        }
        ComputeMode::ReturnValues => {
            for i in 0..a.len().min(others.len()) {
                let (access, other) = (a[i], others[i]);
                if access == WRITE {
                    continue;
                }
                if access == WRITE_POTENTIAL {
                    if other == WRITE {
                        a[i] = WRITE;
                    }
                    continue;
                }
                if other != UNUSED {
                    a[i] = other;
                }
            }
        }
        ComputeMode::Merge => {
            for i in 0..a.len().min(others.len()) {
                a[i] = ACCESS_MODE_CONDITIONAL_TABLE[index_of(a[i])][index_of(others[i])];
            }
        }
        ComputeMode::None => {}
    }
}

fn merge_access_mode_conditional(this: &FlowRef, info: &FlowRef, context: &FlowContext) {
    if !context.consider_access_mode {
        return;
    }
    let others = info.borrow().access.clone();
    let mine = this.borrow().access.clone();
    let Some(mine) = mine else {
        match others {
            Some(o) => this.borrow_mut().access = Some(o),
            None => this.borrow_mut().create_access_mode_array(context),
        }
        return;
    };
    let others: Option<Vec<i32>> = others.map(|o| o.borrow().clone());
    let mut a = mine.borrow_mut();
    for i in 0..a.len() {
        let other = others.as_ref().map_or(UNUSED, |o| o.get(i).copied().unwrap_or(UNUSED));
        a[i] = ACCESS_MODE_CONDITIONAL_TABLE[index_of(a[i])][index_of(other)];
    }
}

fn merge_empty_condition(this: &FlowRef, context: &FlowContext) {
    {
        let mut t = this.borrow_mut();
        if t.return_kind == VALUE_RETURN || t.return_kind == VOID_RETURN {
            t.return_kind = PARTIAL_RETURN;
        }
    }
    if !context.consider_access_mode {
        return;
    }
    let mine = this.borrow().access.clone();
    match mine {
        None => this.borrow_mut().create_access_mode_array(context),
        Some(a) => {
            let mut a = a.borrow_mut();
            for m in a.iter_mut() {
                *m = ACCESS_MODE_CONDITIONAL_TABLE[index_of(*m)][0];
            }
        }
    }
}

/// `GenericSequentialFlowInfo.merge(info, context)`.
pub fn seq_merge(this: &FlowRef, info: Option<&FlowRef>, context: &FlowContext) {
    if let Some(info) = info {
        merge_sequential(this, info, context);
    }
}

/// `new LocalFlowInfo(binding, mode, context)`.
fn local_info(binding: BindingId, variable_id: i32, mode: i32, context: &FlowContext) -> FlowRef {
    let info = new_info(NO_RETURN, Extra::Local { variable_id });
    if context.consider_access_mode {
        let mut i = info.borrow_mut();
        i.create_access_mode_array(context);
        set_mode(&i, variable_id, mode, context);
        context.manage_local(binding, variable_id);
    }
    info
}

/// `new LocalFlowInfo(info, mode, context)`.
fn local_info_from(variable_id: i32, mode: i32, context: &FlowContext) -> FlowRef {
    let info = new_info(NO_RETURN, Extra::Local { variable_id });
    if context.consider_access_mode {
        let mut i = info.borrow_mut();
        i.create_access_mode_array(context);
        set_mode(&i, variable_id, mode, context);
    }
    info
}

fn set_mode(info: &FlowInfo, variable_id: i32, mode: i32, context: &FlowContext) {
    if let Some(a) = &info.access {
        let i = variable_id - context.start;
        let mut a = a.borrow_mut();
        if i >= 0 && (i as usize) < a.len() {
            a[i as usize] = mode;
        }
    }
}

fn local_variable_id(info: &FlowRef) -> Option<i32> {
    match info.borrow().extra {
        Extra::Local { variable_id } => Some(variable_id),
        _ => None,
    }
}

// ─── Analyzers ─────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Mode {
    /// `InOutFlowAnalyzer`.
    InOut { first_selected_start: Option<usize> },
    /// `InputFlowAnalyzer`.
    Input { selection: Selection, do_loop_reentrance: bool },
    /// `InputFlowAnalyzer.LoopReentranceVisitor`.
    LoopReentrance { selection: Selection, loop_node: NodeId },
}

/// `FlowAnalyzer`.
pub struct FlowAnalyzer<'c> {
    context: &'c FlowContext,
    data: HashMap<NodeId, FlowRef>,
    mode: Mode,
}

impl<'c> FlowAnalyzer<'c> {
    /// `new InOutFlowAnalyzer(context).perform(selectedNodes)`.
    pub fn in_out(context: &'c FlowContext, selected: &[Node<'_>]) -> FlowRef {
        let mut a = FlowAnalyzer { context, data: HashMap::new(), mode: Mode::InOut { first_selected_start: selected.first().map(|n| n.start()) } };
        let result = sequential();
        for n in selected {
            a.accept(*n);
            let info = a.take(Some(*n));
            seq_merge(&result, info.as_ref(), context);
        }
        result
    }

    /// `new InputFlowAnalyzer(context, selection, doLoopReentrance).perform(declaration)`.
    pub fn input(context: &'c FlowContext, selection: Selection, do_loop_reentrance: bool, declaration: Node<'_>) -> Option<FlowRef> {
        let mut a = FlowAnalyzer { context, data: HashMap::new(), mode: Mode::Input { selection, do_loop_reentrance } };
        a.accept(declaration);
        a.take(Some(declaration))
    }

    fn take(&mut self, n: Option<Node<'_>>) -> Option<FlowRef> {
        n.and_then(|n| self.data.remove(&n.id))
    }

    fn set(&mut self, n: Node<'_>, info: Option<FlowRef>) {
        match info {
            Some(i) => {
                self.data.insert(n.id, i);
            }
            None => {
                self.data.remove(&n.id);
            }
        }
    }

    fn access(&self, n: Node<'_>) -> Option<FlowRef> {
        self.data.get(&n.id).cloned()
    }

    fn assign_flow_info(&mut self, target: Node<'_>, source: Option<Node<'_>>) -> Option<FlowRef> {
        let r = self.take(source);
        self.set(target, r.clone());
        r
    }

    fn create_sequential(&mut self, parent: Node<'_>) -> FlowRef {
        let r = sequential();
        self.data.insert(parent.id, r.clone());
        r
    }

    fn process(&mut self, info: &FlowRef, nodes: &[Node<'_>]) {
        for n in nodes {
            let i = self.take(Some(*n));
            seq_merge(info, i.as_ref(), self.context);
        }
    }

    fn process_opt(&mut self, info: &FlowRef, n: Option<Node<'_>>) {
        let i = self.take(n);
        seq_merge(info, i.as_ref(), self.context);
    }

    fn process_sequential(&mut self, parent: Node<'_>, nodes: &[Node<'_>]) -> FlowRef {
        let r = self.create_sequential(parent);
        self.process(&r, nodes);
        r
    }

    fn process_sequential2(&mut self, parent: Node<'_>, a: Option<Node<'_>>, b: Option<Node<'_>>) -> FlowRef {
        let r = self.create_sequential(parent);
        self.process_opt(&r, a);
        self.process_opt(&r, b);
        r
    }

    fn sequential_of(&mut self, nodes: &[Node<'_>]) -> FlowRef {
        let r = sequential();
        self.process(&r, nodes);
        r
    }

    fn traverse_node(&self, n: Node<'_>) -> bool {
        match &self.mode {
            Mode::InOut { .. } | Mode::LoopReentrance { .. } => true,
            Mode::Input { selection, .. } => n.end() as i64 > selection.inclusive_end(),
        }
    }

    fn skip(&self, n: Node<'_>) -> bool {
        !self.traverse_node(n)
    }

    fn create_return_flow_info(&self, n: Node<'_>) -> bool {
        match &self.mode {
            Mode::InOut { first_selected_start } => {
                if n.is(NodeKind::YieldStatement) {
                    let parent = n.ancestors().find(|a| a.is(NodeKind::SwitchExpression));
                    return parent.is_some_and(|p| first_selected_start.is_some_and(|s| p.start() < s));
                }
                true
            }
            Mode::Input { selection, .. } => n.start() as i64 >= selection.inclusive_end(),
            Mode::LoopReentrance { selection, .. } => n.end() as i64 <= selection.exclusive_end(),
        }
    }

    /// `node.accept(this)`.
    pub fn accept(&mut self, n: Node<'_>) {
        if self.visit(n) {
            for c in n.children() {
                self.accept(c);
            }
        }
        self.end_visit(n);
    }

    fn visit(&mut self, n: Node<'_>) -> bool {
        match n.kind() {
            NodeKind::EmptyStatement | NodeKind::Javadoc => false,
            NodeKind::TryStatement => {
                if self.traverse_node(n) {
                    for r in n.list("resources") {
                        self.accept(r);
                    }
                    if let Some(b) = n.child("body") {
                        self.accept(b);
                    }
                    for c in n.list("catchClauses") {
                        self.accept(c);
                    }
                    if let Some(f) = n.child("finally") {
                        self.accept(f);
                    }
                }
                false
            }
            _ => self.traverse_node(n),
        }
    }

    fn end_visit(&mut self, n: Node<'_>) {
        match self.mode.clone() {
            Mode::InOut { .. } => self.end_visit_in_out(n),
            Mode::Input { selection, do_loop_reentrance } => self.end_visit_input(n, selection, do_loop_reentrance),
            Mode::LoopReentrance { selection, loop_node } => self.end_visit_loop(n, selection, loop_node),
        }
    }

    // ── InOutFlowAnalyzer ───────────────────────────────────────────────────

    fn end_visit_in_out(&mut self, n: Node<'_>) {
        self.end_visit_base(n);
        let Some(info) = self.access(n) else { return };
        let clear_fragments = |this: &Self, nodes: Vec<Node<'_>>| {
            for node in nodes {
                if matches!(node.kind(), NodeKind::VariableDeclarationStatement | NodeKind::VariableDeclarationExpression) {
                    for f in node.list("fragments") {
                        this.clear(&info, f);
                    }
                }
            }
        };
        match n.kind() {
            NodeKind::Block => clear_fragments(self, n.list("statements")),
            NodeKind::CatchClause => {
                if let Some(e) = n.child("exception") {
                    self.clear(&info, e);
                }
            }
            NodeKind::EnhancedForStatement => {
                if let Some(p) = n.child("parameter") {
                    self.clear(&info, p);
                }
            }
            NodeKind::ForStatement => clear_fragments(self, n.list("initializers")),
            NodeKind::MethodDeclaration => {
                for p in n.list("parameters") {
                    self.clear(&info, p);
                }
            }
            _ => {}
        }
    }

    fn clear(&self, info: &FlowRef, declaration: Node<'_>) {
        if let Some(b) = declaration.binding().filter(|b| !b.is_field()) {
            info.borrow_mut().clear_access_mode(b.data().variable_id, self.context);
        }
    }

    // ── InputFlowAnalyzer ───────────────────────────────────────────────────

    fn end_visit_input(&mut self, n: Node<'_>, selection: Selection, do_loop_reentrance: bool) {
        match n.kind() {
            NodeKind::ConditionalExpression | NodeKind::IfStatement => {
                if self.skip(n) {
                    return;
                }
                let (cond, then_part, else_part) = if n.is(NodeKind::IfStatement) {
                    (n.child("expression"), n.child("thenStatement"), n.child("elseStatement"))
                } else {
                    (n.child("expression"), n.child("thenExpression"), n.child("elseExpression"))
                };
                if then_part.is_some_and(|t| selection.covered_by(t)) || else_part.is_some_and(|e| selection.covered_by(e)) {
                    let info = sequential();
                    self.set(n, Some(info.clone()));
                    self.process_opt(&info, cond);
                    for branch in [then_part, else_part].into_iter().flatten() {
                        if selection.covered_by(branch) {
                            self.process_opt(&info, Some(branch));
                            break;
                        }
                    }
                } else {
                    self.end_visit_base(n);
                }
            }
            NodeKind::DoStatement | NodeKind::EnhancedForStatement | NodeKind::ForStatement | NodeKind::WhileStatement => {
                self.end_visit_base(n);
                if do_loop_reentrance && selection.covered_by(n) && !selection.covers(n) {
                    let mut lr = FlowAnalyzer { context: self.context, data: HashMap::new(), mode: Mode::LoopReentrance { selection, loop_node: n.id } };
                    self.context.loop_reentrance.set(true);
                    lr.accept(n);
                    self.context.loop_reentrance.set(false);
                    let info = sequential();
                    let own = self.take(Some(n));
                    seq_merge(&info, own.as_ref(), self.context);
                    let other = lr.take(Some(n));
                    seq_merge(&info, other.as_ref(), self.context);
                    self.set(n, Some(info));
                }
            }
            NodeKind::SwitchStatement | NodeKind::SwitchExpression => {
                if self.skip(n) {
                    return;
                }
                let data = self.create_switch_data(&n.list("statements"));
                for (i, (start, length)) in data.ranges.iter().enumerate() {
                    if selection.covered_by_region(*start, *length) {
                        let info = sequential();
                        self.set(n, Some(info.clone()));
                        self.process_opt(&info, n.child("expression"));
                        seq_merge(&info, data.infos[i].as_ref(), self.context);
                        info.borrow_mut().remove_label(None);
                        return;
                    }
                }
                self.end_visit_switch(n, data);
            }
            _ => self.end_visit_base(n),
        }
    }

    // ── LoopReentranceVisitor ───────────────────────────────────────────────

    fn end_visit_loop(&mut self, n: Node<'_>, selection: Selection, loop_node: NodeId) {
        match n.kind() {
            NodeKind::BreakStatement => {
                if n.end() as i64 <= selection.exclusive_end() {
                    return;
                }
                self.end_visit_base(n);
            }
            NodeKind::DoStatement => {
                if self.skip(n) {
                    return;
                }
                let info = new_info(UNDEFINED, Extra::DoWhile { action_branches: false });
                self.set(n, Some(info.clone()));
                let action = self.take(n.child("body"));
                do_merge_action(&info, action.as_ref());
                info.borrow_mut().remove_label(None);
            }
            NodeKind::EnhancedForStatement => {
                if self.skip(n) {
                    return;
                }
                let param = self.take(n.child("parameter"));
                let expression = self.take(n.child("expression"));
                let action = self.take(n.child("body"));
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                if n.id == loop_node {
                    merge_action(&info, action.as_ref(), self.context);
                } else {
                    merge_access(&info, expression.as_ref(), self.context);
                    merge_access(&info, param.as_ref(), self.context);
                    merge_action(&info, action.as_ref(), self.context);
                }
                info.borrow_mut().remove_label(None);
            }
            NodeKind::ForStatement => {
                if self.skip(n) {
                    return;
                }
                let init = self.sequential_of(&n.list("initializers"));
                let condition = self.take(n.child("expression"));
                let increment = self.sequential_of(&n.list("updaters"));
                let action = self.take(n.child("body"));
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                if n.id == loop_node {
                    merge_increment(&info, Some(&increment), self.context);
                    merge_access(&info, condition.as_ref(), self.context);
                    merge_action(&info, action.as_ref(), self.context);
                } else {
                    let init_incr = conditional_info();
                    merge_conditional(&init_incr, &init, self.context);
                    merge_conditional(&init_incr, &increment, self.context);
                    merge_access_mode_sequential(&info, &init_incr, self.context);
                    merge_access(&info, condition.as_ref(), self.context);
                    merge_action(&info, action.as_ref(), self.context);
                }
                info.borrow_mut().remove_label(None);
            }
            _ => self.end_visit_base(n),
        }
    }

    // ── FlowAnalyzer ────────────────────────────────────────────────────────

    fn end_visit_base(&mut self, n: Node<'_>) {
        use NodeKind::*;
        let ctx = self.context;
        if self.skip(n) {
            return;
        }
        match n.kind() {
            AnnotationTypeDeclaration | AnonymousClassDeclaration => {
                let info = self.process_sequential(n, &n.list("bodyDeclarations"));
                info.borrow_mut().set_no_return();
            }
            AnnotationTypeMemberDeclaration => {
                let info = self.process_sequential2(n, n.child("type"), n.child("default"));
                info.borrow_mut().set_no_return();
            }
            ArrayAccess => {
                self.process_sequential2(n, n.child("array"), n.child("index"));
            }
            ArrayCreation => {
                let info = self.process_sequential2(n, n.child("type"), None);
                self.process(&info, &n.list("dimensions"));
                self.process_opt(&info, n.child("initializer"));
            }
            ArrayInitializer => {
                self.process_sequential(n, &n.list("expressions"));
            }
            ArrayType => {
                self.process_sequential2(n, n.child("elementType"), None);
            }
            AssertStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                let cond = self.take(n.child("expression"));
                merge_access(&info, cond.as_ref(), ctx);
                let message = self.take(n.child("message"));
                if_merge(&info, message.as_ref(), None, ctx);
            }
            Assignment => {
                let lhs = self.take(n.child("leftHandSide"));
                let mut rhs = self.take(n.child("rightHandSide"));
                if let Some(id) = lhs.as_ref().and_then(local_variable_id) {
                    if ctx.consider_access_mode {
                        set_mode(&lhs.as_ref().unwrap().borrow(), id, WRITE, ctx);
                    }
                    if n.simple("operator") != Some("=") {
                        let tmp = sequential();
                        seq_merge(&tmp, Some(&local_info_from(id, READ, ctx)), ctx);
                        seq_merge(&tmp, rhs.as_ref(), ctx);
                        rhs = Some(tmp);
                    }
                }
                let info = self.create_sequential(n);
                seq_merge(&info, rhs.as_ref(), ctx);
                seq_merge(&info, lhs.as_ref(), ctx);
            }
            Block => {
                let info = self.create_sequential(n);
                self.process(&info, &n.list("statements"));
            }
            BreakStatement | ContinueStatement => {
                let info = new_info(NO_RETURN, Extra::None);
                let label = n.child("label").map(|l| l.identifier()).unwrap_or_else(|| UNLABELED.to_owned());
                info.borrow_mut().branches = Some(Rc::new(RefCell::new(vec![label])));
                self.set(n, Some(info));
            }
            CastExpression => {
                self.process_sequential2(n, n.child("type"), n.child("expression"));
            }
            CatchClause => {
                self.process_sequential2(n, n.child("exception"), n.child("body"));
            }
            ClassInstanceCreation => {
                let info = self.process_sequential2(n, n.child("expression"), None);
                self.process_opt(&info, n.child("type"));
                self.process(&info, &n.list("arguments"));
                self.process_opt(&info, n.child("anonymousClassDeclaration"));
            }
            CompilationUnit => {
                let info = self.process_sequential(n, &n.list("imports"));
                self.process(&info, &n.list("types"));
            }
            ConditionalExpression => {
                let info = new_info(NO_RETURN, Extra::None);
                self.set(n, Some(info.clone()));
                let cond = self.take(n.child("expression"));
                merge_access(&info, cond.as_ref(), ctx);
                let t = self.take(n.child("thenExpression"));
                let e = self.take(n.child("elseExpression"));
                if t.is_some() || e.is_some() {
                    let c = conditional_info();
                    if let Some(t) = &t {
                        merge_access_mode_conditional(&c, t, ctx);
                    }
                    if let Some(e) = &e {
                        merge_access_mode_conditional(&c, e, ctx);
                    }
                    if t.is_none() || e.is_none() {
                        merge_empty_condition(&c, ctx);
                    }
                    merge_access_mode_sequential(&info, &c, ctx);
                }
            }
            ConstructorInvocation => {
                self.process_sequential(n, &n.list("arguments"));
            }
            DoStatement => {
                let info = new_info(UNDEFINED, Extra::DoWhile { action_branches: false });
                self.set(n, Some(info.clone()));
                let action = self.take(n.child("body"));
                do_merge_action(&info, action.as_ref());
                let cond = self.take(n.child("expression"));
                let skip = {
                    let i = info.borrow();
                    matches!(i.extra, Extra::DoWhile { action_branches: true }) || i.return_kind == VALUE_RETURN || i.return_kind == VOID_RETURN
                };
                if !skip {
                    if let Some(c) = &cond {
                        merge_access_mode_sequential(&info, c, ctx);
                    }
                }
                info.borrow_mut().remove_label(None);
            }
            EnhancedForStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                let p = self.take(n.child("parameter"));
                merge_access(&info, p.as_ref(), ctx);
                let e = self.take(n.child("expression"));
                merge_access(&info, e.as_ref(), ctx);
                let a = self.take(n.child("body"));
                merge_action(&info, a.as_ref(), ctx);
                info.borrow_mut().remove_label(None);
            }
            EnumConstantDeclaration => {
                let info = self.process_sequential(n, &n.list("arguments"));
                self.process_opt(&info, n.child("anonymousClassDeclaration"));
            }
            EnumDeclaration => {
                let info = self.process_sequential(n, &n.list("superInterfaceTypes"));
                self.process(&info, &n.list("enumConstants"));
                self.process(&info, &n.list("bodyDeclarations"));
                info.borrow_mut().set_no_return();
            }
            ExpressionStatement | ParenthesizedExpression => {
                self.assign_flow_info(n, n.child("expression"));
            }
            FieldAccess => {
                self.process_sequential2(n, n.child("expression"), n.child("name"));
            }
            FieldDeclaration | VariableDeclarationExpression | VariableDeclarationStatement => {
                let info = self.process_sequential2(n, n.child("type"), None);
                self.process(&info, &n.list("fragments"));
            }
            ForStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                let init = self.sequential_of(&n.list("initializers"));
                merge_access_mode_sequential(&info, &init, ctx);
                let cond = self.take(n.child("expression"));
                merge_access(&info, cond.as_ref(), ctx);
                let action = self.take(n.child("body"));
                merge_action(&info, action.as_ref(), ctx);
                let incr = self.sequential_of(&n.list("updaters"));
                merge_increment(&info, Some(&incr), ctx);
                info.borrow_mut().remove_label(None);
            }
            IfStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                let cond = self.take(n.child("expression"));
                merge_access(&info, cond.as_ref(), ctx);
                let t = self.take(n.child("thenStatement"));
                let e = self.take(n.child("elseStatement"));
                if_merge(&info, t.as_ref(), e.as_ref(), ctx);
            }
            ImportDeclaration | PackageDeclaration | SimpleType => {
                self.assign_flow_info(n, n.child("name"));
            }
            InfixExpression => {
                let info = self.process_sequential2(n, n.child("leftOperand"), n.child("rightOperand"));
                self.process(&info, &n.list("extendedOperands"));
            }
            InstanceofExpression => {
                self.process_sequential2(n, n.child("leftOperand"), n.child("rightOperand"));
            }
            Initializer => {
                self.assign_flow_info(n, n.child("body"));
            }
            LabeledStatement => {
                if let Some(info) = self.assign_flow_info(n, n.child("body")) {
                    let label = n.child("label").map(|l| l.identifier());
                    info.borrow_mut().remove_label(Some(label.as_deref().unwrap_or(UNLABELED)));
                }
            }
            LambdaExpression => {
                let info = self.create_sequential(n);
                self.process(&info, &n.list("parameters"));
                self.process_opt(&info, n.child("body"));
                info.borrow_mut().set_no_return();
            }
            MemberValuePair => {
                let name = self.take(n.child("name"));
                let value = self.take(n.child("value"));
                if let Some(id) = name.as_ref().and_then(local_variable_id) {
                    if ctx.consider_access_mode {
                        set_mode(&name.as_ref().unwrap().borrow(), id, WRITE, ctx);
                    }
                }
                let info = self.create_sequential(n);
                seq_merge(&info, value.as_ref(), ctx);
                seq_merge(&info, name.as_ref(), ctx);
            }
            MethodDeclaration => {
                let info = self.process_sequential2(n, n.child("returnType2"), None);
                self.process(&info, &n.list("parameters"));
                self.process(&info, &n.list("thrownExceptionTypes"));
                self.process_opt(&info, n.child("body"));
            }
            MethodInvocation | SuperConstructorInvocation => self.end_visit_method_invocation(n, n.child("expression")),
            SuperMethodInvocation => self.end_visit_method_invocation(n, n.child("qualifier")),
            NameQualifiedType | QualifiedName | QualifiedType => {
                self.process_sequential2(n, n.child("qualifier"), n.child("name"));
            }
            SuperFieldAccess => {
                self.process_sequential2(n, n.child("qualifier"), n.child("name"));
            }
            NormalAnnotation => {
                let info = self.process_sequential2(n, n.child("typeName"), None);
                self.process(&info, &n.list("values"));
            }
            ParameterizedType => {
                let info = self.process_sequential2(n, n.child("type"), None);
                self.process(&info, &n.list("typeArguments"));
            }
            PostfixExpression => self.end_visit_inc_dec(n),
            PrefixExpression => {
                if matches!(n.simple("operator"), Some("++" | "--")) {
                    self.end_visit_inc_dec(n);
                } else {
                    self.assign_flow_info(n, n.child("operand"));
                }
            }
            ReturnStatement | YieldStatement => {
                if self.create_return_flow_info(n) {
                    let kind = if n.is(YieldStatement) {
                        VALUE_RETURN
                    } else {
                        match n.child("expression") {
                            None => VOID_RETURN,
                            Some(e) if super::checks::is_void(e.type_binding()) => VOID_RETURN,
                            Some(_) => VALUE_RETURN,
                        }
                    };
                    let info = new_info(kind, Extra::None);
                    self.set(n, Some(info.clone()));
                    if let Some(e) = self.take(n.child("expression")) {
                        assign_access_mode(&info, &e);
                    }
                } else {
                    self.assign_flow_info(n, n.child("expression"));
                }
            }
            SimpleName => {
                if super::extract_temp::is_declaration(n) {
                    return;
                }
                if let Some(b) = n.binding() {
                    if b.is_variable() {
                        if !b.is_field() {
                            let info = local_info(b.id, b.data().variable_id, READ, ctx);
                            self.set(n, Some(info));
                        }
                    } else if b.is_type() && b.is_type_variable() {
                        let info = new_info(NO_RETURN, Extra::None);
                        info.borrow_mut().add_type_variable(b.id);
                        self.set(n, Some(info));
                    }
                }
            }
            SingleMemberAnnotation => {
                self.assign_flow_info(n, n.child("value"));
            }
            SingleVariableDeclaration | VariableDeclarationFragment => {
                let initializer = n.child("initializer");
                let name_info = match (n.binding(), initializer) {
                    (Some(b), Some(_)) if !b.is_field() => Some(local_info(b.id, b.data().variable_id, WRITE, ctx)),
                    _ => None,
                };
                let info = if n.is(SingleVariableDeclaration) {
                    self.process_sequential2(n, n.child("type"), initializer)
                } else {
                    self.process_sequential2(n, initializer, None)
                };
                seq_merge(&info, name_info.as_ref(), ctx);
            }
            SwitchStatement | SwitchExpression => {
                let data = self.create_switch_data(&n.list("statements"));
                self.end_visit_switch(n, data);
            }
            SynchronizedStatement => {
                let info = self.process_sequential2(n, n.child("expression"), None);
                self.process_opt(&info, n.child("body"));
            }
            ThisExpression => {
                self.assign_flow_info(n, n.child("qualifier"));
            }
            ThrowStatement => {
                let info = new_info(THROW, Extra::None);
                self.set(n, Some(info.clone()));
                if let Some(e) = self.take(n.child("expression")) {
                    assign_access_mode(&info, &e);
                }
            }
            TryStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                for r in n.list("resources") {
                    let i = self.take(Some(r));
                    seq_merge(&info, i.as_ref(), ctx);
                }
                let body = self.take(n.child("body"));
                seq_merge(&info, body.as_ref(), ctx);
                for c in n.list("catchClauses") {
                    if let Some(i) = self.take(Some(c)) {
                        merge_conditional(&info, &i, ctx);
                    }
                }
                let f = self.take(n.child("finally"));
                seq_merge(&info, f.as_ref(), ctx);
            }
            TypeDeclaration => {
                let info = self.process_sequential2(n, n.child("superclassType"), None);
                self.process(&info, &n.list("superInterfaceTypes"));
                self.process(&info, &n.list("bodyDeclarations"));
                info.borrow_mut().set_no_return();
            }
            TypeDeclarationStatement => {
                self.assign_flow_info(n, n.child("declaration"));
            }
            TypeLiteral => {
                self.assign_flow_info(n, n.child("type"));
            }
            TypeParameter => {
                let info = self.process_sequential2(n, n.child("name"), None);
                self.process(&info, &n.list("typeBounds"));
            }
            WhileStatement => {
                let info = new_info(UNDEFINED, Extra::None);
                self.set(n, Some(info.clone()));
                let cond = self.take(n.child("expression"));
                merge_access(&info, cond.as_ref(), ctx);
                let action = self.take(n.child("body"));
                merge_action(&info, action.as_ref(), ctx);
                info.borrow_mut().remove_label(None);
            }
            WildcardType => {
                self.assign_flow_info(n, n.child("bound"));
            }
            _ => {}
        }
    }

    fn end_visit_method_invocation(&mut self, n: Node<'_>, receiver: Option<Node<'_>>) {
        let info = new_info(NO_RETURN, Extra::None);
        self.set(n, Some(info.clone()));
        for mut arg in n.list("arguments") {
            if arg.is(NodeKind::ExpressionMethodReference) {
                if let Some(e) = arg.child("expression") {
                    arg = e;
                }
            }
            let i = self.take(Some(arg));
            seq_merge(&info, i.as_ref(), self.context);
        }
        let r = self.take(receiver);
        seq_merge(&info, r.as_ref(), self.context);
    }

    fn end_visit_inc_dec(&mut self, n: Node<'_>) {
        let info = self.take(n.child("operand"));
        match info.as_ref().and_then(local_variable_id) {
            Some(id) => {
                let result = self.create_sequential(n);
                seq_merge(&result, info.as_ref(), self.context);
                seq_merge(&result, Some(&local_info_from(id, WRITE, self.context)), self.context);
            }
            None => self.set(n, info),
        }
    }

    /// `createSwitchData(statements)`.
    fn create_switch_data(&mut self, statements: &[Node<'_>]) -> SwitchData {
        let mut result = SwitchData::default();
        if statements.is_empty() {
            return result;
        }
        let (mut start, mut end): (i64, i64) = (-1, -1);
        let mut info: Option<FlowRef> = None;
        for s in statements {
            if s.is(NodeKind::SwitchCase) {
                if is_default_case(*s) {
                    result.has_default_case = true;
                }
                match &info {
                    None => {
                        info = Some(sequential());
                        start = s.start() as i64;
                    }
                    Some(i) => {
                        let split = {
                            let i = i.borrow();
                            i.is_return() || i.is_partial_return() || i.branches()
                        };
                        if split {
                            result.ranges.push((start, end - start + 1));
                            result.infos.push(info.take());
                            info = Some(sequential());
                            start = s.start() as i64;
                        }
                    }
                }
            } else if let Some(i) = &info {
                let f = self.take(Some(*s));
                seq_merge(i, f.as_ref(), self.context);
            }
            end = s.end() as i64 - 1;
        }
        result.ranges.push((start, end - start + 1));
        result.infos.push(info);
        result
    }

    fn end_visit_switch(&mut self, n: Node<'_>, data: SwitchData) {
        let cases = conditional_info();
        let info = new_info(UNDEFINED, Extra::Switch { cases: cases.clone(), has_null_case: false });
        self.set(n, Some(info.clone()));
        let test = self.take(n.child("expression"));
        seq_merge(&info, test.as_ref(), self.context);
        let mut has_null_case = false;
        for case in &data.infos {
            match case {
                None => has_null_case = true,
                Some(c) => merge_conditional(&cases, c, self.context),
            }
        }
        if !data.has_default_case || has_null_case {
            merge_empty_condition(&cases, self.context);
        }
        merge_sequential(&info, &cases, self.context);
        info.borrow_mut().remove_label(None);
    }
}

/// `SwitchCase.isDefault()`.
fn is_default_case(n: Node<'_>) -> bool {
    n.flag("default") || (n.list("expressions").is_empty() && n.child("expression").is_none())
}

#[derive(Default)]
struct SwitchData {
    has_default_case: bool,
    ranges: Vec<(i64, i64)>,
    infos: Vec<Option<FlowRef>>,
}

/// `*FlowInfo.mergeCondition` / `mergeParameter` / `mergeExpression` /
/// `mergeInitializer`: access modes only.
fn merge_access(this: &FlowRef, info: Option<&FlowRef>, context: &FlowContext) {
    if let Some(info) = info {
        merge_access_mode_sequential(this, info, context);
    }
}

/// `WhileFlowInfo` / `ForFlowInfo` / `EnhancedForFlowInfo.mergeAction`.
fn merge_action(this: &FlowRef, info: Option<&FlowRef>, context: &FlowContext) {
    if let Some(info) = info {
        merge_empty_condition(info, context);
        merge_sequential(this, info, context);
    }
}

/// `ForFlowInfo.mergeIncrement`.
fn merge_increment(this: &FlowRef, info: Option<&FlowRef>, context: &FlowContext) {
    if let Some(info) = info {
        merge_empty_condition(info, context);
        merge_access_mode_sequential(this, info, context);
    }
}

/// `DoWhileFlowInfo.mergeAction`.
fn do_merge_action(this: &FlowRef, info: Option<&FlowRef>) {
    let Some(info) = info else { return };
    let action_branches = info.borrow().branches();
    assign(this, info);
    let mut t = this.borrow_mut();
    t.extra = Extra::DoWhile { action_branches };
    if action_branches && t.return_kind == VALUE_RETURN {
        t.return_kind = PARTIAL_RETURN;
    }
}

/// `IfFlowInfo.merge(thenPart, elsePart, context)`.
fn if_merge(this: &FlowRef, then_part: Option<&FlowRef>, else_part: Option<&FlowRef>, context: &FlowContext) {
    if then_part.is_none() && else_part.is_none() {
        return;
    }
    let cond = conditional_info();
    if let Some(t) = then_part {
        merge_conditional(&cond, t, context);
    }
    if let Some(e) = else_part {
        merge_conditional(&cond, e, context);
    }
    if then_part.is_none() || else_part.is_none() {
        merge_empty_condition(&cond, context);
    }
    merge_sequential(this, &cond, context);
}

/// `LocalVariableIndex.perform(declaration)`.
pub fn local_variable_index(declaration: Node<'_>) -> i32 {
    let mut target = declaration;
    for a in declaration.ancestors() {
        if matches!(a.kind(), NodeKind::MethodDeclaration | NodeKind::Initializer | NodeKind::FieldDeclaration) {
            target = a;
        }
    }
    let mut top = 0;
    super::walk(target, &mut |n| {
        if matches!(n.kind(), NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment) {
            if let Some(b) = n.binding() {
                top = top.max(b.data().variable_id);
            }
        }
        !n.is(NodeKind::Javadoc)
    });
    top
}
