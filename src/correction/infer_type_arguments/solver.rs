//! `ParametricStructureComputer` and `InferTypeArgumentsConstraintsSolver`
//! with the type sets of `typeconstraints.typesets`.
//!
//! Type sets are kept as the universe or as finite sets over the types of the
//! `TypeEnvironment` (the types the constraint model created and their
//! supertypes), which is also what the JDT sets enumerate.

use std::collections::{HashMap, HashSet, VecDeque};

use super::model::{is_a_generic_type, CvId, CvKind, Model, NOT_DECLARED, TT};

/// A `TypeSet`: the universe or a finite (possibly empty) set of types.
#[derive(Clone, Debug)]
pub enum TypeSet<'a> {
    Universe,
    Set(Vec<TT<'a>>),
}

impl PartialEq for TypeSet<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TypeSet::Universe, TypeSet::Universe) => true,
            (TypeSet::Set(a), TypeSet::Set(b)) => a.len() == b.len() && a.iter().all(|t| b.contains(t)),
            _ => false,
        }
    }
}

/// `TTypes.canAssignTo(sub, sup)` over the binding hierarchy.
pub fn is_subtype(sub: &TT<'_>, sup: &TT<'_>) -> bool {
    if sub == sup {
        return true;
    }
    if sub.is_primitive() || sup.is_primitive() || sub.is_void() || sup.is_void() {
        return false;
    }
    if sup.pretty() == "java.lang.Object" {
        return true;
    }
    if sub.is_array() {
        if sup.is_array() {
            return match (sub.component_type(), sup.component_type()) {
                (Some(a), Some(b)) => !a.is_primitive() && is_subtype(&a, &b) || a == b,
                _ => false,
            };
        }
        return matches!(sup.pretty().as_str(), "java.lang.Cloneable" | "java.io.Serializable");
    }
    fn walk<'a>(t: TT<'a>, sup: &TT<'a>, seen: &mut HashSet<String>) -> bool {
        if t == *sup {
            return true;
        }
        if !seen.insert(t.key()) {
            return false;
        }
        let Some(b) = t.binding() else { return false };
        let bounds: Vec<TT<'a>> = if b.is_type_variable() || b.is_capture() { b.type_bounds().into_iter().map(TT::B).collect() } else { Vec::new() };
        t.superclass().is_some_and(|s| walk(s, sup, seen)) || t.interfaces().into_iter().any(|i| walk(i, sup, seen)) || bounds.into_iter().any(|i| walk(i, sup, seen))
    }
    walk(*sub, sup, &mut HashSet::new())
}

fn supertypes_closure<'a>(t: TT<'a>, out: &mut Vec<TT<'a>>, object: Option<TT<'a>>) {
    let mut queue = VecDeque::from([t]);
    while let Some(t) = queue.pop_front() {
        if out.contains(&t) {
            continue;
        }
        out.push(t);
        if let Some(s) = t.superclass() {
            queue.push_back(s);
        }
        queue.extend(t.interfaces());
    }
    if !t.is_primitive() && !t.is_void() {
        if let Some(o) = object {
            if !out.contains(&o) {
                out.push(o);
            }
        }
    }
}

impl<'a> TypeSet<'a> {
    pub fn empty() -> Self {
        TypeSet::Set(Vec::new())
    }

    pub fn is_universe(&self) -> bool {
        matches!(self, TypeSet::Universe)
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, TypeSet::Set(v) if v.is_empty())
    }

    /// `superTypes()` (registers new supertypes in the environment).
    fn super_types(&self, model: &mut Model<'a>) -> TypeSet<'a> {
        match self {
            TypeSet::Universe => TypeSet::Universe,
            TypeSet::Set(v) => {
                let object = model.known.iter().find(|t| t.pretty() == "java.lang.Object").copied();
                let mut out = Vec::new();
                for t in v {
                    supertypes_closure(*t, &mut out, object);
                }
                for t in &out {
                    model.register(*t);
                }
                TypeSet::Set(out)
            }
        }
    }

    /// `subTypes()`: the environment's types below a member.
    fn sub_types(&self, model: &Model<'a>) -> TypeSet<'a> {
        match self {
            TypeSet::Universe => TypeSet::Universe,
            TypeSet::Set(v) => {
                let mut out: Vec<TT<'a>> = v.clone();
                for t in &model.known {
                    if !out.contains(t) && v.iter().any(|s| is_subtype(t, s)) {
                        out.push(*t);
                    }
                }
                TypeSet::Set(out)
            }
        }
    }

    fn contains_all(&self, other: &TypeSet<'a>) -> bool {
        match (self, other) {
            (TypeSet::Universe, _) => true,
            (TypeSet::Set(_), TypeSet::Universe) => false,
            (TypeSet::Set(a), TypeSet::Set(b)) => b.iter().all(|t| a.contains(t)),
        }
    }

    fn intersected_with(&self, other: &TypeSet<'a>) -> TypeSet<'a> {
        match (self, other) {
            (TypeSet::Universe, x) | (x, TypeSet::Universe) => x.clone(),
            (TypeSet::Set(a), TypeSet::Set(b)) => TypeSet::Set(a.iter().filter(|t| b.contains(t)).copied().collect()),
        }
    }

    /// `lowerBound()`: the members without a proper subtype in the set.
    fn lower_bound(&self) -> Vec<TT<'a>> {
        match self {
            TypeSet::Universe => Vec::new(),
            TypeSet::Set(v) => v.iter().filter(|t| !v.iter().any(|o| o != *t && is_subtype(o, t))).copied().collect(),
        }
    }

    /// `InferTypeArgumentsConstraintsSolver.chooseSingleType`.
    pub fn choose_single_type(&self) -> Option<TT<'a>> {
        if self.is_universe() || self.is_empty() {
            return None;
        }
        let lower = self.lower_bound();
        if lower.len() == 1 {
            return Some(lower[0]);
        }
        let mut interfaces = Vec::new();
        for t in lower {
            if !t.is_interface() {
                return Some(t);
            }
            interfaces.push(t);
        }
        if interfaces.len() <= 1 {
            return interfaces.first().copied();
        }
        let non_tagging: Vec<TT<'a>> = interfaces
            .iter()
            .filter(|t| t.binding().and_then(|b| b.declared_methods()).is_some_and(|m| !m.is_empty()))
            .copied()
            .collect();
        let pool = if non_tagging.is_empty() { interfaces } else { non_tagging };
        pool.into_iter().min_by(|a, b| crate::correction::compare_utf16(&a.pretty(), &b.pretty()))
    }
}

// ─── ParametricStructureComputer ────────────────────────────────────────────

const NONE: usize = 0;

struct Structure<'a> {
    base: Option<TT<'a>>,
    params: Vec<Option<usize>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Equals,
    SubType,
    SuperType,
}

struct Parametric<'m, 'a> {
    model: &'m mut Model<'a>,
    structures: Vec<Structure<'a>>,
    env: HashMap<CvId, usize>,
    work: Vec<CvId>,
}

impl<'a> Parametric<'_, 'a> {
    fn new_structure(&mut self, ty: TT<'a>) -> usize {
        let base = ty.decl();
        let n = base.type_parameters().len();
        self.structures.push(Structure { base: Some(base), params: vec![None; n] });
        self.structures.len() - 1
    }

    fn set_and_push(&mut self, v: CvId, s: usize) {
        self.env.insert(v, s);
        self.work.push(v);
    }

    fn compute(&mut self, all: &[CvId]) {
        for &v in all {
            if let Some(ty) = self.model.ty(v) {
                if is_a_generic_type(&ty) {
                    let s = self.new_structure(ty);
                    self.set_and_push(v, s);
                } else {
                    self.set_and_push(v, NONE);
                }
            }
        }
        while let Some(v) = self.work.pop() {
            let used: Vec<usize> = self.model.cvs[v].used_in.clone();
            for tc in used {
                let (lhs, rhs) = self.model.constraints[tc];
                self.unify(lhs, rhs);
            }
            if let Some(set) = self.model.cvs[v].set {
                let members = self.model.sets[set].members.clone();
                for pair in members.windows(2) {
                    self.unify(pair[0], pair[1]);
                }
            }
        }
    }

    fn unify(&mut self, lhs: CvId, rhs: CvId) {
        let rs = self.env.get(&rhs).copied();
        if self.update_var(lhs, rs, Op::SubType) {
            if let CvKind::Element(..) = self.model.cvs[lhs].kind {
                self.update_parent_from(lhs, rhs);
            }
            self.update_elements_from_parent(lhs);
        }
        let ls = self.env.get(&lhs).copied();
        if self.update_var(rhs, ls, Op::SuperType) {
            if let CvKind::Element(..) = self.model.cvs[rhs].kind {
                self.update_parent_from(rhs, lhs);
            }
            self.update_elements_from_parent(rhs);
        }
    }

    fn update_ith_param(&mut self, s1: usize, i: usize, other: Option<usize>) -> bool {
        let Some(other) = other else { return false };
        if s1 == other {
            return false;
        }
        let Some(param) = self.structures[s1].params[i] else {
            self.structures[s1].params[i] = Some(other);
            return true;
        };
        if param != NONE && other != NONE {
            if self.structures[param].base == self.structures[other].base {
                return self.update_type(param, other);
            }
            self.structures[s1].params[i] = Some(NONE);
            return true;
        }
        false
    }

    fn update_elements_from_parent(&mut self, v: CvId) {
        let Some(t) = self.env.get(&v).copied() else { return };
        if t == NONE {
            return;
        }
        for (_, element) in self.model.element_variables(v) {
            if let CvKind::Element(_, _, index) = self.model.cvs[element].kind {
                if index != NOT_DECLARED {
                    let param = self.structures[t].params.get(index as usize).copied().flatten();
                    self.update_var(element, param, Op::Equals);
                }
            }
        }
    }

    fn contains_sub(&self, containing: Option<usize>, sub: usize) -> bool {
        let Some(c) = containing else { return false };
        let mut stack = vec![c];
        let mut seen = HashSet::new();
        while let Some(s) = stack.pop() {
            if !seen.insert(s) {
                continue;
            }
            for p in self.structures[s].params.iter().flatten() {
                if *p == sub {
                    return true;
                }
                stack.push(*p);
            }
        }
        false
    }

    fn update_parent_from(&mut self, element: CvId, v1: CvId) {
        let CvKind::Element(container, _, index) = self.model.cvs[element].kind.clone() else { return };
        let mut container_structure = self.env.get(&container).copied();
        if container_structure == Some(NONE) {
            return;
        }
        if container_structure.is_none() {
            let Some(ty) = self.model.ty(container) else { return };
            let s = self.new_structure(ty);
            self.set_and_push(container, s);
            container_structure = Some(s);
        }
        let container_structure = container_structure.unwrap();
        let v1_structure = self.env.get(&v1).copied();
        if index == NOT_DECLARED {
            return;
        }
        let index = index as usize;
        if index >= self.structures[container_structure].params.len() {
            return;
        }
        if Some(container_structure) == v1_structure || self.contains_sub(v1_structure, container_structure) {
            if self.env.get(&element).copied() != Some(NONE) {
                self.set_and_push(element, NONE);
            }
            if self.structures[container_structure].params[index].is_none() {
                self.structures[container_structure].params[index] = Some(NONE);
                self.work.push(container);
            }
        } else if self.update_ith_param(container_structure, index, v1_structure) {
            let p = self.structures[container_structure].params[index].unwrap_or(NONE);
            self.set_and_push(element, p);
            self.work.push(container);
        }
    }

    fn update_type(&mut self, t1: usize, t2: usize) -> bool {
        let n = self.structures[t1].params.len().min(self.structures[t2].params.len());
        let mut change = false;
        for i in 0..n {
            if self.structures[t2].params[i] == Some(t1) {
                if self.structures[t1].params[i] != Some(NONE) {
                    self.structures[t1].params[i] = Some(NONE);
                    change = true;
                }
            } else if self.update_ith_param(t1, i, self.structures[t2].params[i]) {
                change = true;
            }
        }
        change
    }

    fn update_var(&mut self, v: CvId, t2: Option<usize>, op: Op) -> bool {
        let Some(t2) = t2 else { return false };
        let Some(vs) = self.env.get(&v).copied() else {
            self.set_and_push(v, t2);
            return true;
        };
        let (v_structured, t2_structured) = (vs != NONE, t2 != NONE);
        if v_structured && !t2_structured {
            if op == Op::Equals || op == Op::SuperType {
                self.set_and_push(v, t2);
                return true;
            }
        } else if v_structured && t2_structured {
            if self.structures[vs].base != self.structures[t2].base {
                if op == Op::SuperType {
                    self.set_and_push(v, NONE);
                    return true;
                }
            } else if self.update_type(vs, t2) {
                self.work.push(v);
                return true;
            }
        }
        false
    }

    /// `createElemConstraintVariables`.
    fn create_element_variables(&mut self, all: &[CvId]) -> Vec<CvId> {
        self.compute(all);
        let mut out = Vec::new();
        for &v in all {
            self.create_variables_for(v, &mut out, 0);
        }
        out
    }

    fn create_variables_for(&mut self, v: CvId, out: &mut Vec<CvId>, depth: usize) {
        let Some(t) = self.env.get(&v).copied() else { return };
        if t == NONE || depth > 16 {
            return;
        }
        let Some(base) = self.structures[t].base else { return };
        self.model.make_element_variables(v, base);
        let elements = self.model.element_variables(v);
        let params = self.structures[t].params.clone();
        for (_, child) in &elements {
            if let CvKind::Element(_, _, index) = self.model.cvs[*child].kind {
                if index != NOT_DECLARED {
                    let s = if params.is_empty() { Some(NONE) } else { params.get(index as usize).copied().flatten() };
                    match s {
                        Some(s) => {
                            self.env.insert(*child, s);
                        }
                        None => {
                            self.env.remove(child);
                        }
                    }
                }
            }
        }
        for (_, child) in elements {
            if let CvKind::Element(_, _, index) = self.model.cvs[child].kind {
                if index != NOT_DECLARED {
                    out.push(child);
                    self.create_variables_for(child, out, depth + 1);
                }
            }
        }
    }
}

// ─── InferTypeArgumentsConstraintsSolver ────────────────────────────────────

/// The solved model: chosen types per constraint variable and the
/// declarations to update (`InferTypeArgumentsUpdate`).
pub struct Solution<'a> {
    pub chosen: HashMap<CvId, Option<TT<'a>>>,
    pub declarations: Vec<CvId>,
}

fn initial_estimate<'a>(model: &Model<'a>, cv: CvId) -> TypeSet<'a> {
    let ty = model.ty(cv);
    let kind = &model.cvs[cv].kind;
    match ty {
        None => TypeSet::Universe,
        Some(_) if matches!(kind, CvKind::Independent | CvKind::ArrayType) => TypeSet::Universe,
        Some(t) if matches!(kind, CvKind::ArrayElement(_)) => {
            if t.is_type_variable() {
                TypeSet::Universe
            } else {
                TypeSet::Set(vec![t])
            }
        }
        Some(t) if t.is_void() => TypeSet::empty(),
        Some(t) => TypeSet::Set(vec![t]),
    }
}

pub fn solve<'a>(model: &mut Model<'a>) -> Solution<'a> {
    let mut solution = Solution { chosen: HashMap::new(), declarations: Vec::new() };
    let mut all: Vec<CvId> = model.stored.clone();
    if all.is_empty() {
        return solution;
    }
    let new_vars = {
        let mut parametric = Parametric { model, structures: vec![Structure { base: None, params: Vec::new() }], env: HashMap::new(), work: Vec::new() };
        parametric.create_element_variables(&all)
    };
    for v in new_vars {
        if !all.contains(&v) {
            all.push(v);
        }
    }

    let mut sets: Vec<usize> = Vec::new();
    for &cv in &all {
        if let Some(s) = model.cvs[cv].set {
            if !sets.contains(&s) {
                sets.push(s);
            }
        }
    }
    for s in sets {
        let members = model.sets[s].members.clone();
        for i in 0..members.len() {
            for j in i + 1..members.len() {
                model.create_element_equals_constraints(Some(members[i]), Some(members[j]));
            }
        }
    }
    let constraints = model.constraints.clone();
    for (left, right) in constraints {
        model.create_element_equals_constraints(Some(left), Some(right));
    }

    // initializeTypeEstimates
    for &cv in &all {
        match model.cvs[cv].set {
            None => {
                let estimate = initial_estimate(model, cv);
                model.sets.push(super::model::EqSet { members: vec![cv], estimate: Some(estimate) });
                let s = model.sets.len() - 1;
                model.cvs[cv].set = Some(s);
            }
            Some(s) => {
                if model.sets[s].estimate.is_none() {
                    let mut estimate = TypeSet::Universe;
                    for c in model.sets[s].members.clone() {
                        estimate = estimate.intersected_with(&initial_estimate(model, c));
                    }
                    model.sets[s].estimate = Some(estimate);
                }
            }
        }
    }

    // runSolver
    let mut work: VecDeque<CvId> = all.iter().copied().collect();
    let mut guard = 0usize;
    while let Some(cv) = work.pop_front() {
        guard += 1;
        if guard > 200_000 {
            break;
        }
        for tc in model.cvs[cv].used_in.clone() {
            let (left, right) = model.constraints[tc];
            let (Some(ls), Some(rs)) = (model.cvs[left].set, model.cvs[right].set) else { continue };
            let left_estimate = model.sets[ls].estimate.clone().unwrap_or(TypeSet::Universe);
            let right_estimate = model.sets[rs].estimate.clone().unwrap_or(TypeSet::Universe);
            if left_estimate.is_universe() && right_estimate.is_universe() || left_estimate == right_estimate {
                continue;
            }
            let lhs_super = left_estimate.super_types(model);
            let rhs_sub = right_estimate.sub_types(model);
            if !rhs_sub.contains_all(&left_estimate) {
                model.sets[ls].estimate = Some(left_estimate.intersected_with(&rhs_sub));
                work.extend(model.sets[ls].members.iter().copied());
            }
            if !lhs_super.contains_all(&right_estimate) {
                model.sets[rs].estimate = Some(right_estimate.intersected_with(&lhs_super));
                work.extend(model.sets[rs].members.iter().copied());
            }
        }
    }

    // chooseTypes
    for &cv in &all {
        let Some(s) = model.cvs[cv].set else { continue };
        let chosen = model.sets[s].estimate.as_ref().and_then(|e| e.choose_single_type());
        solution.chosen.insert(cv, chosen);
        if matches!(model.cvs[cv].kind, CvKind::Element(..)) && model.has_compilation_unit(cv) {
            solution.declarations.push(cv);
        }
    }
    solution
}

impl<'a> Solution<'a> {
    /// `InferTypeArgumentsConstraintsSolver.getChosenType(cv)`.
    pub fn chosen_type(&self, model: &Model<'a>, cv: CvId) -> Option<TT<'a>> {
        if let Some(Some(t)) = self.chosen.get(&cv) {
            return Some(*t);
        }
        let s = model.cvs[cv].set?;
        model.sets[s].estimate.as_ref()?.choose_single_type()
    }
}
