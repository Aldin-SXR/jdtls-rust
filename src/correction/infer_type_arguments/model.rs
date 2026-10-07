//! `InferTypeArgumentsTCModel` with the `typeconstraints2` constraint
//! variables and the `TType` view of the semantic AST bindings.

use std::collections::{HashMap, HashSet};

use crate::semantic_ast::{BindingRef, NodeId};

/// A `TType` of the `TypeEnvironment`: a type binding of the unit's graph,
/// or the environment's `VOID`.
#[derive(Clone, Copy, Debug)]
pub enum TT<'a> {
    B(BindingRef<'a>),
    Void,
}

impl PartialEq for TT<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TT::B(a), TT::B(b)) => a == b,
            (TT::Void, TT::Void) => true,
            _ => false,
        }
    }
}

impl<'a> TT<'a> {
    pub fn binding(&self) -> Option<BindingRef<'a>> {
        match self {
            TT::B(b) => Some(*b),
            TT::Void => None,
        }
    }
    pub fn key(&self) -> String {
        match self {
            TT::B(b) => b.key().to_owned(),
            TT::Void => "V".into(),
        }
    }
    /// `getPrettySignature()`.
    pub fn pretty(&self) -> String {
        match self {
            TT::B(b) => b.qualified_name().to_owned(),
            TT::Void => "void".into(),
        }
    }
    fn test(&self, f: impl Fn(BindingRef<'a>) -> bool) -> bool {
        self.binding().is_some_and(f)
    }
    pub fn is_void(&self) -> bool {
        matches!(self, TT::Void) || self.test(|b| b.name() == "void")
    }
    pub fn is_type_variable(&self) -> bool {
        self.test(|b| b.is_type_variable())
    }
    pub fn is_wildcard(&self) -> bool {
        self.test(|b| b.is_wildcard_type())
    }
    pub fn is_array(&self) -> bool {
        self.test(|b| b.is_array())
    }
    pub fn is_primitive(&self) -> bool {
        self.test(|b| b.is_primitive())
    }
    pub fn is_generic(&self) -> bool {
        self.test(|b| b.is_generic_type())
    }
    pub fn is_parameterized(&self) -> bool {
        self.test(|b| b.is_parameterized_type())
    }
    pub fn is_raw(&self) -> bool {
        self.test(|b| b.is_raw_type())
    }
    pub fn is_interface(&self) -> bool {
        self.test(|b| b.is_interface())
    }
    /// `getTypeDeclaration()`.
    pub fn decl(&self) -> TT<'a> {
        match self {
            TT::B(b) => TT::B(b.type_declaration().unwrap_or(*b)),
            TT::Void => TT::Void,
        }
    }
    pub fn erasure(&self) -> TT<'a> {
        match self {
            TT::B(b) => TT::B(b.erasure().unwrap_or(*b)),
            TT::Void => TT::Void,
        }
    }
    pub fn superclass(&self) -> Option<TT<'a>> {
        self.binding()?.superclass().map(TT::B)
    }
    pub fn interfaces(&self) -> Vec<TT<'a>> {
        self.binding().map(|b| b.interfaces().into_iter().map(TT::B).collect()).unwrap_or_default()
    }
    pub fn type_parameters(&self) -> Vec<TT<'a>> {
        self.binding().map(|b| b.type_parameters().into_iter().map(TT::B).collect()).unwrap_or_default()
    }
    pub fn type_arguments(&self) -> Vec<TT<'a>> {
        self.binding().map(|b| b.type_arguments().into_iter().map(TT::B).collect()).unwrap_or_default()
    }
    pub fn component_type(&self) -> Option<TT<'a>> {
        self.binding()?.component_type().map(TT::B)
    }
    pub fn element_type(&self) -> Option<TT<'a>> {
        self.binding()?.element_type().map(TT::B)
    }
    pub fn bound(&self) -> Option<TT<'a>> {
        self.binding()?.bound().map(TT::B)
    }
    pub fn is_extends_wildcard(&self) -> bool {
        self.test(|b| b.is_wildcard_type() && b.bound().is_some() && b.has(crate::semantic_ast::bflag::UPPERBOUND))
    }
}

/// `InferTypeArgumentsTCModel.isAGenericType`.
pub fn is_a_generic_type(t: &TT<'_>) -> bool {
    t.is_generic() || t.is_parameterized() || (t.is_raw() && t.decl().is_generic())
}

pub type CvId = usize;

/// The constraint variable classes of `typeconstraints2`.
#[derive(Clone, Debug, PartialEq)]
pub enum CvKind {
    Variable,
    /// `TypeVariable2`: a type reference in the unit.
    Type(NodeId),
    Immutable,
    Independent,
    ParameterizedType,
    ArrayType,
    Parameter,
    Return,
    /// `CollectionElementVariable2` (parent, type variable key, declaration index).
    Element(CvId, String, i32),
    ArrayElement(CvId),
}

/// `CollectionElementVariable2.NOT_DECLARED_TYPE_VARIABLE_INDEX`.
pub const NOT_DECLARED: i32 = -1;

#[derive(Clone, PartialEq, Eq, Hash)]
enum CvKey {
    Variable(String),
    Type(NodeId, String),
    Immutable(String),
    Parameter(String, usize),
    Return(String),
    Element(CvId, String),
    ArrayElement(CvId),
}

pub struct Cv<'a> {
    pub kind: CvKind,
    pub ty: Option<TT<'a>>,
    /// `ISourceConstraintVariable.getCompilationUnit() != null`.
    pub has_cu: bool,
    pub elements: Vec<(String, CvId)>,
    pub array_element: Option<CvId>,
    pub used_in: Vec<usize>,
    pub set: Option<usize>,
}

/// `TypeEquivalenceSet`.
pub struct EqSet<'a> {
    pub members: Vec<CvId>,
    pub estimate: Option<super::solver::TypeSet<'a>>,
}

#[derive(Default)]
pub struct Model<'a> {
    pub cvs: Vec<Cv<'a>>,
    /// The stored constraint variables, in insertion order (`fConstraintVariables`).
    pub stored: Vec<CvId>,
    index: HashMap<CvKey, CvId>,
    /// `SubTypeConstraint2` (left <= right).
    pub constraints: Vec<(CvId, CvId)>,
    constraint_index: HashMap<(CvId, CvId), usize>,
    stored_set: HashSet<CvId>,
    pub sets: Vec<EqSet<'a>>,
    /// Types of the `TypeEnvironment` (for enumerating subtypes).
    pub known: Vec<TT<'a>>,
}

impl<'a> Model<'a> {
    /// `createTType` (registers the type in the environment).
    pub fn tt(&mut self, b: BindingRef<'a>) -> TT<'a> {
        let t = TT::B(b);
        self.register(t);
        t
    }

    pub fn register(&mut self, t: TT<'a>) {
        if !self.known.contains(&t) {
            self.known.push(t);
        }
    }

    pub fn ty(&self, cv: CvId) -> Option<TT<'a>> {
        self.cvs[cv].ty
    }

    fn new_cv(&mut self, kind: CvKind, ty: Option<TT<'a>>) -> CvId {
        if let Some(t) = ty {
            self.register(t);
        }
        self.cvs.push(Cv { kind, ty, has_cu: false, elements: Vec::new(), array_element: None, used_in: Vec::new(), set: None });
        self.cvs.len() - 1
    }

    fn store(&mut self, cv: CvId) {
        if self.stored_set.insert(cv) {
            self.stored.push(cv);
        }
    }

    /// `storedCv`: the stored equal variable, or `None` when `create` makes a new one.
    fn stored_or_new(&mut self, key: Option<CvKey>, kind: CvKind, ty: Option<TT<'a>>) -> (CvId, bool) {
        if let Some(k) = &key {
            if let Some(&id) = self.index.get(k) {
                return (id, false);
            }
        }
        let id = self.new_cv(kind, ty);
        if let Some(k) = key {
            self.index.insert(k, id);
        }
        self.store(id);
        (id, true)
    }

    /// `keep(cv1, cv2)`.
    fn keep(&self, a: CvId, b: CvId) -> bool {
        if a == b {
            return false;
        }
        let is_special = |cv: CvId| matches!(self.cvs[cv].kind, CvKind::Element(..) | CvKind::Independent);
        if is_special(a) || is_special(b) {
            return true;
        }
        self.ty(a).is_some_and(|t| is_a_generic_type(&t)) || self.ty(b).is_some_and(|t| is_a_generic_type(&t))
    }

    /// `createSubtypeConstraint(cv1, cv2)`: cv1 <= cv2.
    pub fn create_subtype_constraint(&mut self, a: Option<CvId>, b: Option<CvId>) {
        let (Some(a), Some(b)) = (a, b) else { return };
        if !self.keep(a, b) {
            return;
        }
        self.store(a);
        self.store(b);
        let tc = match self.constraint_index.get(&(a, b)) {
            Some(&i) => i,
            None => {
                self.constraints.push((a, b));
                let i = self.constraints.len() - 1;
                self.constraint_index.insert((a, b), i);
                i
            }
        };
        self.cvs[a].used_in.push(tc);
        self.cvs[b].used_in.push(tc);
    }

    /// `createEqualsConstraint(left, right)`.
    pub fn create_equals_constraint(&mut self, left: Option<CvId>, right: Option<CvId>) {
        let (Some(left), Some(right)) = (left, right) else { return };
        match (self.cvs[left].set, self.cvs[right].set) {
            (None, None) => {
                let members = if left == right { vec![left] } else { vec![left, right] };
                self.sets.push(EqSet { members, estimate: None });
                let s = self.sets.len() - 1;
                self.cvs[left].set = Some(s);
                self.cvs[right].set = Some(s);
            }
            (None, Some(rs)) => {
                if !self.sets[rs].members.contains(&left) {
                    self.sets[rs].members.push(left);
                }
                self.cvs[left].set = Some(rs);
            }
            (Some(ls), None) => {
                if !self.sets[ls].members.contains(&right) {
                    self.sets[ls].members.push(right);
                }
                self.cvs[right].set = Some(ls);
            }
            (Some(ls), Some(rs)) => {
                if ls == rs {
                    return;
                }
                let cvs = self.sets[rs].members.clone();
                for cv in cvs {
                    if !self.sets[ls].members.contains(&cv) {
                        self.sets[ls].members.push(cv);
                    }
                    self.cvs[cv].set = Some(ls);
                }
            }
        }
    }

    pub fn get_element_variable(&self, cv: Option<CvId>, type_variable_key: &str) -> Option<CvId> {
        let cv = cv?;
        self.cvs[cv].elements.iter().find(|(k, _)| k == type_variable_key).map(|(_, v)| *v)
    }

    pub fn element_variables(&self, cv: CvId) -> Vec<(String, CvId)> {
        self.cvs[cv].elements.clone()
    }

    fn set_element_variable(&mut self, cv: CvId, element: CvId, type_variable_key: &str) {
        if let Some(e) = self.cvs[cv].elements.iter_mut().find(|(k, _)| k == type_variable_key) {
            e.1 = element;
        } else {
            self.cvs[cv].elements.push((type_variable_key.to_owned(), element));
        }
    }

    pub fn array_element_variable(&self, cv: Option<CvId>) -> Option<CvId> {
        self.cvs[cv?].array_element
    }

    /// `getBoxedType(typeBinding, expression)`.
    pub fn boxed_type(&mut self, b: Option<BindingRef<'a>>, boxing: bool) -> Option<TT<'a>> {
        let b = b?;
        if !b.is_primitive() {
            return Some(self.tt(b));
        }
        if !boxing {
            return None;
        }
        let name = crate::correction::type_mismatch::bindings::boxed_type_name(b.name())?;
        let boxed = b.ast.type_by_name(name)?;
        Some(self.tt(boxed))
    }

    /// `makeVariableVariable` (+ `makeDeclaredVariableVariable` when `declared`).
    pub fn make_variable_variable(&mut self, var: Option<BindingRef<'a>>, declared: bool) -> Option<CvId> {
        let var = var?;
        let ty = self.boxed_type(var.var_type(), false)?;
        let (cv, new) = self.stored_or_new(Some(CvKey::Variable(var.key().to_owned())), CvKind::Variable, Some(ty));
        if new {
            self.make_element_variables(cv, ty);
            self.make_array_element_variable(cv);
        }
        if declared {
            self.cvs[cv].has_cu = true;
        }
        Some(cv)
    }

    /// `makeTypeVariable(Type)`.
    pub fn make_type_variable(&mut self, node: NodeId, binding: Option<BindingRef<'a>>) -> Option<CvId> {
        let ty = self.boxed_type(binding, false)?;
        let (cv, new) = self.stored_or_new(Some(CvKey::Type(node, ty.key())), CvKind::Type(node), Some(ty));
        if new {
            self.cvs[cv].has_cu = true;
            if is_a_generic_type(&ty) {
                self.make_element_variables(cv, ty);
            }
            self.make_array_element_variable(cv);
        }
        Some(cv)
    }

    pub fn make_independent_type_variable(&mut self, ty: TT<'a>) -> CvId {
        self.stored_or_new(None, CvKind::Independent, Some(ty)).0
    }

    pub fn make_parameterized_type_variable(&mut self, b: Option<BindingRef<'a>>) -> Option<CvId> {
        let ty = self.tt(b?);
        let (cv, _) = self.stored_or_new(None, CvKind::ParameterizedType, Some(ty));
        self.make_element_variables(cv, ty);
        Some(cv)
    }

    pub fn make_array_type_variable(&mut self, b: Option<BindingRef<'a>>) -> Option<CvId> {
        let ty = self.tt(b?);
        let (cv, _) = self.stored_or_new(None, CvKind::ArrayType, Some(ty));
        self.make_array_element_variable(cv);
        Some(cv)
    }

    pub fn make_parameter_type_variable(&mut self, method: Option<BindingRef<'a>>, index: usize, declared: bool) -> Option<CvId> {
        let method = method?;
        let ty = self.boxed_type(method.parameter_types().get(index).copied(), false)?;
        let (cv, new) = self.stored_or_new(Some(CvKey::Parameter(method.key().to_owned(), index)), CvKind::Parameter, Some(ty));
        if new {
            self.make_element_variables(cv, ty);
            self.make_array_element_variable(cv);
        }
        if declared {
            self.cvs[cv].has_cu = true;
        }
        Some(cv)
    }

    pub fn make_return_type_variable(&mut self, method: Option<BindingRef<'a>>, declared: bool) -> Option<CvId> {
        let method = method?;
        let ty = self.boxed_type(method.return_type(), false)?;
        let (cv, new) = self.stored_or_new(Some(CvKey::Return(method.key().to_owned())), CvKind::Return, Some(ty));
        if new {
            self.make_element_variables(cv, ty);
            self.make_array_element_variable(cv);
        }
        if declared {
            self.cvs[cv].has_cu = true;
        }
        Some(cv)
    }

    /// `makeImmutableTypeVariable(typeBinding, expression)`.
    pub fn make_immutable_from(&mut self, b: Option<BindingRef<'a>>, boxing: bool) -> Option<CvId> {
        let ty = self.boxed_type(b, boxing)?;
        Some(self.make_immutable(ty))
    }

    pub fn make_immutable(&mut self, ty: TT<'a>) -> CvId {
        let (cv, new) = self.stored_or_new(Some(CvKey::Immutable(ty.key())), CvKind::Immutable, Some(ty));
        if new {
            self.make_fixed_element_variables(cv, ty);
            self.make_array_element_variable(cv);
        }
        cv
    }

    pub fn make_array_element_variable(&mut self, cv: CvId) {
        let Some(ty) = self.ty(cv).filter(|t| t.is_array()) else { return };
        if self.cvs[cv].array_element.is_some() {
            return;
        }
        let (element, _) = self.stored_or_new(Some(CvKey::ArrayElement(cv)), CvKind::ArrayElement(cv), ty.component_type());
        self.cvs[cv].array_element = Some(element);
        self.make_array_element_variable(element);
    }

    pub fn make_element_variables(&mut self, cv: CvId, ty: TT<'a>) {
        if is_a_generic_type(&ty) {
            for (i, tv) in ty.decl().type_parameters().into_iter().enumerate() {
                self.make_element_variable(Some(cv), &tv.key(), i as i32);
            }
        }
        self.make_element_variables_from_supertypes(cv, ty.decl(), &mut HashSet::new());
    }

    fn make_element_variables_from_supertypes(&mut self, cv: CvId, ty: TT<'a>, seen: &mut HashSet<String>) {
        if !seen.insert(ty.key()) {
            return;
        }
        if let Some(superclass) = ty.superclass() {
            self.make_supertype_element_variables(cv, superclass, seen);
        }
        for interface in ty.interfaces() {
            self.make_supertype_element_variables(cv, interface, seen);
        }
    }

    fn make_supertype_element_variables(&mut self, cv: CvId, supertype: TT<'a>, seen: &mut HashSet<String>) {
        if supertype.is_parameterized() || supertype.is_raw() {
            let arguments = supertype.is_parameterized().then(|| supertype.type_arguments());
            for (i, parameter) in supertype.decl().type_parameters().into_iter().enumerate() {
                let argument = match &arguments {
                    None => parameter.erasure(),
                    Some(a) => match a.get(i) {
                        Some(a) => *a,
                        None => continue,
                    },
                };
                if argument.is_type_variable() {
                    if let Some(existing) = self.get_element_variable(Some(cv), &argument.key()) {
                        self.set_element_variable(cv, existing, &parameter.key());
                        continue;
                    }
                }
                self.make_element_variable(Some(cv), &parameter.key(), NOT_DECLARED);
            }
        }
        self.make_element_variables_from_supertypes(cv, supertype, seen);
    }

    pub fn make_fixed_element_variables(&mut self, cv: CvId, ty: TT<'a>) {
        if is_a_generic_type(&ty) {
            let arguments = ty.is_parameterized().then(|| ty.type_arguments());
            for (i, tv) in ty.decl().type_parameters().into_iter().enumerate() {
                let element = self.make_element_variable(Some(cv), &tv.key(), i as i32);
                let Some(arguments) = &arguments else { continue };
                if let Some(argument) = arguments.get(i) {
                    let immutable = self.make_immutable(*argument);
                    self.create_equals_constraint(element, Some(immutable));
                }
            }
        }
        self.make_fixed_element_variables_from_supertypes(cv, ty.decl(), &mut HashSet::new());
    }

    fn make_fixed_element_variables_from_supertypes(&mut self, cv: CvId, ty: TT<'a>, seen: &mut HashSet<String>) {
        if !seen.insert(ty.key()) {
            return;
        }
        if let Some(superclass) = ty.superclass() {
            self.make_fixed_supertype_element_variables(cv, superclass, seen);
        }
        for interface in ty.interfaces() {
            self.make_fixed_supertype_element_variables(cv, interface, seen);
        }
    }

    fn make_fixed_supertype_element_variables(&mut self, cv: CvId, supertype: TT<'a>, seen: &mut HashSet<String>) {
        if supertype.is_parameterized() {
            let arguments = supertype.type_arguments();
            for (i, parameter) in supertype.decl().type_parameters().into_iter().enumerate() {
                let Some(argument) = arguments.get(i).copied() else { continue };
                if argument.is_type_variable() {
                    if let Some(existing) = self.get_element_variable(Some(cv), &argument.key()) {
                        self.set_element_variable(cv, existing, &parameter.key());
                    }
                } else {
                    let element = self.make_element_variable(Some(cv), &parameter.key(), NOT_DECLARED);
                    let immutable = self.make_immutable(argument);
                    self.create_equals_constraint(element, Some(immutable));
                }
            }
        }
        self.make_fixed_element_variables_from_supertypes(cv, supertype, seen);
    }

    /// `createTypeVariablesEqualityConstraints`.
    pub fn create_type_variables_equality_constraints(&mut self, expression: Option<CvId>, method_type_variables: &HashMap<String, CvId>, reference_cv: Option<CvId>, reference: TT<'a>) {
        if reference.is_parameterized() || reference.is_raw() {
            let arguments = reference.is_parameterized().then(|| reference.type_arguments());
            for (i, parameter) in reference.decl().type_parameters().into_iter().enumerate() {
                let argument = match &arguments {
                    None => parameter.erasure(),
                    Some(a) => match a.get(i) {
                        Some(a) => *a,
                        None => continue,
                    },
                };
                if argument.is_type_variable() {
                    let argument_cv = self.element_type_cv(argument, expression, method_type_variables);
                    let parameter_cv = self.get_element_variable(reference_cv, &parameter.key());
                    self.create_equals_constraint(argument_cv, parameter_cv);
                } else if argument.is_wildcard() {
                    let void = self.make_immutable(TT::Void);
                    let parameter_cv = self.get_element_variable(reference_cv, &parameter.key());
                    self.create_equals_constraint(Some(void), parameter_cv);
                }
            }
        } else if reference.is_array() {
            let Some(mut element) = reference.element_type() else { return };
            if element.is_raw() {
                element = element.erasure();
            }
            let element_cv = self.element_type_cv(element, expression, method_type_variables);
            let array_element = self.array_element_variable(reference_cv);
            self.create_equals_constraint(element_cv, array_element);
        }
    }

    fn element_type_cv(&self, element: TT<'a>, expression: Option<CvId>, method_type_variables: &HashMap<String, CvId>) -> Option<CvId> {
        if element.is_type_variable() {
            if let Some(cv) = method_type_variables.get(&element.key()) {
                return Some(*cv);
            }
            return self.get_element_variable(expression, &element.key());
        }
        None
    }

    pub fn make_element_variable(&mut self, cv: Option<CvId>, type_variable_key: &str, index: i32) -> Option<CvId> {
        let cv = cv?;
        if let Some(existing) = self.get_element_variable(Some(cv), type_variable_key) {
            return Some(existing);
        }
        let (element, _) = self.stored_or_new(Some(CvKey::Element(cv, type_variable_key.to_owned())), CvKind::Element(cv, type_variable_key.to_owned(), index), None);
        self.set_element_variable(cv, element, type_variable_key);
        Some(element)
    }

    pub fn create_element_equals_constraints(&mut self, cv: Option<CvId>, initializer: Option<CvId>) {
        self.internal_element_equals(cv, initializer, false, &mut HashSet::new());
    }

    pub fn create_assignment_element_constraints(&mut self, cv: Option<CvId>, initializer: Option<CvId>) {
        self.internal_element_equals(cv, initializer, true, &mut HashSet::new());
    }

    fn internal_element_equals(&mut self, cv: Option<CvId>, initializer: Option<CvId>, is_assignment: bool, seen: &mut HashSet<(CvId, CvId)>) {
        let (Some(cv), Some(initializer)) = (cv, initializer) else { return };
        if !seen.insert((cv, initializer)) {
            return;
        }
        let right = self.element_variables(initializer);
        for (key, left_element) in self.element_variables(cv) {
            if let Some((_, right_element)) = right.iter().find(|(k, _)| *k == key) {
                self.create_equals_constraint(Some(left_element), Some(*right_element));
                self.internal_element_equals(Some(left_element), Some(*right_element), false, seen);
            }
        }
        let (left_array, right_array) = (self.cvs[cv].array_element, self.cvs[initializer].array_element);
        if let (Some(l), Some(r)) = (left_array, right_array) {
            if is_assignment {
                self.create_subtype_constraint(Some(r), Some(l));
            } else {
                self.create_equals_constraint(Some(l), Some(r));
            }
            self.internal_element_equals(Some(l), Some(r), false, seen);
        }
    }

    /// `ISourceConstraintVariable.getCompilationUnit() != null`.
    pub fn has_compilation_unit(&self, cv: CvId) -> bool {
        match &self.cvs[cv].kind {
            CvKind::Element(parent, ..) => self.has_compilation_unit(*parent),
            _ => self.cvs[cv].has_cu,
        }
    }
}
