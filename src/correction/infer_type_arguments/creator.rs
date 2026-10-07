//! `InferTypeArgumentsConstraintCreator`: walks the unit and records the
//! type constraints of the declarations and expressions.

use std::collections::HashMap;

use super::model::{is_a_generic_type, CvId, Model, TT};
use crate::semantic_ast::{modifier, nflag, BindingRef, Node, NodeId, NodeKind};

pub struct Creator<'m, 'a> {
    pub model: &'m mut Model<'a>,
    cvs: HashMap<NodeId, CvId>,
}

fn boxing(node: Node<'_>) -> bool {
    node.flags() & nflag::BOXING != 0
}

/// `Bindings.getAllSuperTypes(type)`.
fn all_super_types<'a>(t: BindingRef<'a>) -> Vec<BindingRef<'a>> {
    let mut out: Vec<BindingRef<'a>> = Vec::new();
    fn collect<'a>(t: BindingRef<'a>, out: &mut Vec<BindingRef<'a>>) {
        if let Some(s) = t.superclass() {
            if !out.contains(&s) {
                out.push(s);
                collect(s, out);
            }
        }
        for i in t.interfaces() {
            if !out.contains(&i) {
                out.push(i);
                collect(i, out);
            }
        }
    }
    collect(t, &mut out);
    out
}

impl<'m, 'a> Creator<'m, 'a> {
    pub fn new(model: &'m mut Model<'a>) -> Self {
        Creator { model, cvs: HashMap::new() }
    }

    fn cv(&self, node: Option<Node<'_>>) -> Option<CvId> {
        self.cvs.get(&node?.id).copied()
    }

    fn set(&mut self, node: Node<'_>, cv: Option<CvId>) {
        match cv {
            Some(cv) => {
                self.cvs.insert(node.id, cv);
            }
            None => {
                self.cvs.remove(&node.id);
            }
        }
    }

    pub fn walk(&mut self, node: Node<'a>) {
        if self.visit(node) {
            for child in node.children() {
                self.walk(child);
            }
        }
        self.end_visit(node);
    }

    fn visit(&mut self, node: Node<'a>) -> bool {
        match node.kind() {
            NodeKind::Javadoc => false,
            k if k.is_type() => false,
            NodeKind::CatchClause => {
                if let Some(exception) = node.child("exception") {
                    let cv = self.model.make_variable_variable(exception.binding(), true);
                    self.set(exception, cv);
                }
                true
            }
            _ => true,
        }
    }

    fn end_visit(&mut self, node: Node<'a>) {
        let kind = node.kind();
        if kind.is_type() {
            let cv = if kind == NodeKind::ParameterizedType {
                self.model.make_immutable_from(node.binding(), false)
            } else {
                self.model.make_type_variable(node.id, node.binding())
            };
            self.set(node, cv);
            return;
        }
        match kind {
            NodeKind::SimpleName => self.end_simple_name(node),
            NodeKind::FieldAccess | NodeKind::QualifiedName => {
                if boxing(node) {
                    let cv = self.model.make_immutable_from(node.type_binding(), true);
                    self.set(node, cv);
                    return;
                }
                let cv = self.cv(node.child("name"));
                self.set(node, cv);
            }
            NodeKind::ArrayAccess => {
                if boxing(node) {
                    let cv = self.model.make_immutable_from(node.type_binding(), true);
                    self.set(node, cv);
                    return;
                }
                let Some(array) = self.cv(node.child("array")) else { return };
                let element = self.model.array_element_variable(Some(array));
                self.set(node, element);
            }
            NodeKind::Assignment => {
                let lhs = node.child("leftHandSide");
                let left = self.cv(lhs);
                let right = self.cv(node.child("rightHandSide"));
                if boxing(node) {
                    let cv = self.model.make_immutable_from(node.type_binding(), true);
                    self.set(node, cv);
                } else {
                    self.set(node, left);
                }
                if left.is_none() || right.is_none() {
                    return;
                }
                let string_concat = node.simple("operator") == Some("+=") && lhs.and_then(|l| l.type_binding()).is_some_and(|t| t.qualified_name() == "java.lang.String");
                if !string_concat {
                    self.model.create_element_equals_constraints(left, right);
                    self.model.create_subtype_constraint(right, left);
                }
            }
            NodeKind::CastExpression => {
                let Some(typ) = node.child("type") else { return };
                let Some(type_binding) = typ.binding() else { return };
                if type_binding.is_primitive() {
                    let cv = self.model.make_immutable_from(Some(type_binding), boxing(node));
                    self.set(node, cv);
                    return;
                }
                let Some(type_cv) = self.cv(Some(typ)) else { return };
                self.set(node, Some(type_cv));
                let Some(expression_cv) = self.cv(node.child("expression")) else { return };
                use super::model::CvKind;
                let expression_kind = &self.model.cvs[expression_cv].kind;
                if matches!(expression_kind, CvKind::Immutable) {
                    return;
                }
                if !matches!(expression_kind, CvKind::Type(_) | CvKind::Independent | CvKind::Element(..))
                    && self.model.cvs[expression_cv].elements.is_empty()
                    && self.model.cvs[expression_cv].array_element.is_none()
                {
                    return;
                }
                self.model.create_assignment_element_constraints(Some(type_cv), Some(expression_cv));
            }
            NodeKind::ParenthesizedExpression => {
                if boxing(node) {
                    let cv = self.model.make_immutable_from(node.type_binding(), true);
                    self.set(node, cv);
                    return;
                }
                let cv = self.cv(node.child("expression"));
                self.set(node, cv);
            }
            NodeKind::ConditionalExpression => {
                let cv = self.model.make_immutable_from(node.type_binding(), boxing(node));
                self.set(node, cv);
            }
            NodeKind::StringLiteral | NodeKind::ThisExpression | NodeKind::TypeLiteral => {
                let cv = self.model.make_immutable_from(node.type_binding(), false);
                self.set(node, cv);
            }
            NodeKind::NumberLiteral | NodeKind::BooleanLiteral | NodeKind::CharacterLiteral => {
                let cv = self.model.make_immutable_from(node.type_binding(), boxing(node));
                self.set(node, cv);
            }
            NodeKind::MethodDeclaration => self.end_method_declaration(node),
            NodeKind::MethodInvocation => self.end_method_invocation(node),
            NodeKind::ClassInstanceCreation => {
                let receiver = node.child("expression");
                let created = node.child("type");
                let type_cv = if node.child("anonymousClassDeclaration").is_none() {
                    self.cv(created)
                } else {
                    let cv = self.model.make_immutable_from(created.and_then(|c| c.binding()), false);
                    if let Some(c) = created {
                        self.set(c, cv);
                    }
                    cv
                };
                self.set(node, type_cv);
                let Some(method) = node.method_binding() else { return };
                let method_type_variables = self.method_type_arguments(method);
                self.method_invocation_arguments(method, &node.list("arguments"), receiver, &method_type_variables, created);
            }
            NodeKind::ArrayCreation => {
                let cv = self.cv(node.child("type"));
                if cv.is_some() {
                    self.set(node, cv);
                }
            }
            NodeKind::ReturnStatement => {
                let Some(expression_cv) = self.cv(node.child("expression")) else { return };
                let Some(method) = node.ancestors().find(|a| a.is(NodeKind::MethodDeclaration)).and_then(|m| m.binding()) else { return };
                let Some(return_cv) = self.model.make_return_type_variable(Some(method), false) else { return };
                self.model.create_element_equals_constraints(Some(return_cv), Some(expression_cv));
            }
            NodeKind::VariableDeclarationExpression => {
                let Some(type_cv) = self.cv(node.child("type")) else { return };
                self.set(node, Some(type_cv));
                for fragment in node.list("fragments") {
                    let fragment_cv = self.cv(Some(fragment));
                    self.model.create_element_equals_constraints(Some(type_cv), fragment_cv);
                }
            }
            NodeKind::VariableDeclarationStatement | NodeKind::FieldDeclaration => {
                let Some(type_cv) = self.cv(node.child("type")) else { return };
                for fragment in node.list("fragments") {
                    let fragment_cv = self.cv(Some(fragment));
                    self.model.create_element_equals_constraints(Some(type_cv), fragment_cv);
                }
            }
            NodeKind::VariableDeclarationFragment => {
                let Some(cv) = self.model.make_variable_variable(node.binding(), true) else { return };
                self.set(node, Some(cv));
                let Some(initializer_cv) = self.cv(node.child("initializer")) else { return };
                self.model.create_element_equals_constraints(Some(cv), Some(initializer_cv));
            }
            _ => {}
        }
    }

    fn simple_name_receiver(node: Node<'a>) -> Option<Node<'a>> {
        let parent = node.parent()?;
        let receiver = if parent.is(NodeKind::QualifiedName) && node.location_is("name") {
            parent.child("qualifier")
        } else if parent.is(NodeKind::FieldAccess) && node.location_is("name") {
            parent.child("expression")
        } else {
            None
        };
        receiver.filter(|r| !r.is(NodeKind::ThisExpression))
    }

    fn end_simple_name(&mut self, node: Node<'a>) {
        if boxing(node) {
            let cv = self.model.make_immutable_from(node.type_binding(), true);
            self.set(node, cv);
            return;
        }
        let Some(variable) = node.binding().filter(|b| b.is_variable()) else { return };
        let declared = variable.variable_declaration().unwrap_or(variable).var_type();
        if let Some(declared) = declared {
            if declared.is_type_variable() {
                if let Some(receiver) = Self::simple_name_receiver(node) {
                    let receiver_cv = self.cv(Some(receiver));
                    let element = self.model.get_element_variable(receiver_cv, declared.key());
                    self.set(node, element);
                    return;
                }
            } else if declared.is_parameterized_type() {
                if let Some(receiver) = Self::simple_name_receiver(node) {
                    if let Some(receiver_cv) = self.cv(Some(receiver)) {
                        let return_cv = self.model.make_parameterized_type_variable(Some(declared));
                        self.set(node, return_cv);
                        let declared_tt = self.model.tt(declared);
                        self.model.create_type_variables_equality_constraints(Some(receiver_cv), &HashMap::new(), return_cv, declared_tt);
                        return;
                    }
                }
            }
        }
        let cv = self.model.make_variable_variable(Some(variable), false);
        self.set(node, cv);
    }

    fn end_method_declaration(&mut self, node: Node<'a>) {
        let Some(method) = node.binding() else { return };
        let parameters = node.list("parameters");
        let mut parameter_cvs = Vec::new();
        for (i, parameter) in parameters.iter().enumerate() {
            let cv = self.model.make_parameter_type_variable(Some(method), i, true);
            parameter_cvs.push(cv);
            if cv.is_none() {
                continue;
            }
            let type_cv = self.cv(parameter.child("type"));
            self.model.create_element_equals_constraints(cv, type_cv);
            let name_cv = self.cv(parameter.child("name"));
            self.model.create_element_equals_constraints(cv, name_cv);
        }
        let mut return_cv = None;
        if !method.is_constructor() {
            let binding_cv = self.model.make_return_type_variable(Some(method), true);
            if binding_cv.is_some() {
                return_cv = self.cv(node.child("returnType2"));
                self.model.create_element_equals_constraints(binding_cv, return_cv);
            }
        }
        // MethodChecks.isVirtual
        let virtual_method = !method.is_constructor() && method.modifiers() & (modifier::PRIVATE | modifier::STATIC) == 0;
        if virtual_method {
            self.constraints_for_overriding(method, return_cv, &parameter_cvs);
        }
    }

    fn constraints_for_overriding(&mut self, method: BindingRef<'a>, return_cv: Option<CvId>, parameter_cvs: &[Option<CvId>]) {
        if return_cv.is_none() && parameter_cvs.iter().all(Option::is_none) {
            return;
        }
        let Some(owner) = method.declaring_class() else { return };
        for super_type in all_super_types(owner) {
            let super_method = super_type
                .declared_methods()
                .unwrap_or_default()
                .into_iter()
                .find(|m| method.data().method_subsignatures.iter().any(|&id| method.ast.binding(id) == *m));
            let Some(super_method) = super_method else { continue };
            for (p, cv) in parameter_cvs.iter().enumerate() {
                if cv.is_none() {
                    continue;
                }
                let super_cv = self.model.make_parameter_type_variable(Some(super_method), p, false);
                self.model.create_element_equals_constraints(super_cv, *cv);
            }
            if return_cv.is_some() {
                let super_return = self.model.make_return_type_variable(Some(super_method), false);
                self.model.create_element_equals_constraints(super_return, return_cv);
            }
        }
    }

    /// `createMethodTypeArguments`.
    fn method_type_arguments(&mut self, method: BindingRef<'a>) -> HashMap<String, CvId> {
        let mut out = HashMap::new();
        for parameter in method.method_declaration().unwrap_or(method).type_parameters() {
            let tt = self.model.tt(parameter);
            let cv = self.model.make_independent_type_variable(tt);
            out.insert(parameter.key().to_owned(), cv);
        }
        out
    }

    fn is_special_clone(method: BindingRef<'a>, receiver: Option<Node<'a>>) -> bool {
        method.name() == "clone"
            && method.parameter_types().is_empty()
            && receiver.is_some_and(|r| r.type_binding() != method.method_declaration().unwrap_or(method).return_type())
    }

    fn end_method_invocation(&mut self, node: Node<'a>) {
        let Some(method) = node.method_binding() else { return };
        let receiver = if method.is_static() { None } else { node.child("expression") };
        if Self::is_special_clone(method, receiver) {
            let cv = self.cv(receiver);
            self.set(node, cv);
        } else if method.name() == "getClass" && method.parameter_types().is_empty() {
            let Some(return_type) = node.type_binding() else { return };
            let declaration = return_type.type_declaration().unwrap_or(return_type);
            let expression_cv = self.model.make_parameterized_type_variable(Some(declaration));
            self.set(node, expression_cv);
            let Some(parameter) = declaration.type_parameters().first().copied() else { return };
            let class_type_variable = self.model.get_element_variable(expression_cv, parameter.key());
            let Some(capture) = return_type.type_arguments().first().copied() else { return };
            let Some(wildcard) = capture.wildcard() else { return };
            if wildcard.bound().is_none() {
                return;
            }
            let wildcard_tt = self.model.tt(wildcard);
            let wildcard_cv = self.model.make_immutable(wildcard_tt);
            self.model.create_subtype_constraint(class_type_variable, Some(wildcard_cv));
        } else {
            let method_type_variables = self.method_type_arguments(method);
            self.method_invocation_return_type(node, method, receiver, &method_type_variables);
            self.method_invocation_arguments(method, &node.list("arguments"), receiver, &method_type_variables, None);
        }
    }

    fn method_invocation_return_type(&mut self, node: Node<'a>, method: BindingRef<'a>, receiver: Option<Node<'a>>, method_type_variables: &HashMap<String, CvId>) {
        let declaration = method.method_declaration().unwrap_or(method);
        let Some(declared) = declaration.return_type() else { return };
        if declared.is_primitive() {
            let cv = self.model.make_immutable_from(Some(declared), boxing(node));
            self.set(node, cv);
        } else if declared.is_type_variable() {
            if let Some(cv) = method_type_variables.get(declared.key()) {
                self.set(node, Some(*cv));
            } else {
                if receiver.is_none() {
                    return;
                }
                let expression_cv = self.cv(receiver);
                let element = self.model.get_element_variable(expression_cv, declared.key());
                self.set(node, element);
            }
        } else if declared.is_parameterized_type() {
            let return_cv = self.model.make_parameterized_type_variable(declared.type_declaration());
            self.set(node, return_cv);
            let receiver_cv = self.cv(receiver);
            let declared_tt = self.model.tt(declared);
            self.model.create_type_variables_equality_constraints(receiver_cv, method_type_variables, return_cv, declared_tt);
        } else if declared.is_array() {
            let return_cv = self.model.make_array_type_variable(Some(declared));
            self.set(node, return_cv);
            let receiver_cv = self.cv(receiver);
            let declared_tt = self.model.tt(declared);
            self.model.create_type_variables_equality_constraints(receiver_cv, method_type_variables, return_cv, declared_tt);
        } else {
            let cv = self.model.make_return_type_variable(Some(method), false);
            self.set(node, cv);
        }
    }

    fn method_invocation_arguments(&mut self, method: BindingRef<'a>, arguments: &[Node<'a>], receiver: Option<Node<'a>>, method_type_variables: &HashMap<String, CvId>, created_type: Option<Node<'a>>) {
        let declaration = method.method_declaration().unwrap_or(method);
        let declared_parameters = declaration.parameter_types();
        if declared_parameters.is_empty() {
            return;
        }
        let last = declared_parameters.len() - 1;
        for (i, argument) in arguments.iter().enumerate() {
            let Some(argument_cv) = self.cv(Some(*argument)) else { continue };
            let (parameter_index, mut declared) = if !method.is_varargs() || i < last {
                let Some(p) = declared_parameters.get(i) else { continue };
                (i, self.model.tt(*p))
            } else {
                (last, self.model.tt(declared_parameters[last]))
            };
            if method.is_varargs() && i >= last {
                let assignable = i == last
                    && argument.type_binding().is_some_and(|t| {
                        let arg = TT::B(t).erasure();
                        super::solver::is_subtype(&arg, &declared.erasure())
                    });
                if !assignable {
                    if let Some(component) = declared.component_type() {
                        declared = component;
                    }
                }
            }
            if declared.is_type_variable() {
                if let Some(cv) = method_type_variables.get(&declared.key()) {
                    self.model.create_subtype_constraint(Some(argument_cv), Some(*cv));
                } else {
                    if created_type.is_some() {
                        let created_cv = self.cv(created_type);
                        let element = self.model.get_element_variable(created_cv, &declared.key());
                        self.model.create_subtype_constraint(Some(argument_cv), element);
                    }
                    if receiver.is_some() {
                        let expression_cv = self.cv(receiver);
                        let element = self.model.get_element_variable(expression_cv, &declared.key());
                        self.model.create_subtype_constraint(Some(argument_cv), element);
                    }
                }
            } else if declared.is_parameterized() {
                let type_arguments = declared.type_arguments();
                let type_parameters = declared.decl().type_parameters();
                for (ta, type_argument) in type_arguments.iter().enumerate() {
                    let Some(parameter) = type_parameters.get(ta) else { continue };
                    let argument_element = self.model.get_element_variable(Some(argument_cv), &parameter.key());
                    if type_argument.is_wildcard() {
                        let Some(bound) = type_argument.bound().filter(|b| b.is_type_variable()) else { continue };
                        let extends = type_argument.is_extends_wildcard();
                        let wildcard_constraint = |model: &mut Model<'a>, other: Option<CvId>| {
                            if extends {
                                model.create_subtype_constraint(argument_element, other);
                            } else {
                                model.create_subtype_constraint(other, argument_element);
                            }
                        };
                        if let Some(cv) = method_type_variables.get(&bound.key()) {
                            wildcard_constraint(self.model, Some(*cv));
                        } else {
                            if created_type.is_some() {
                                let created_cv = self.cv(created_type);
                                let element = self.model.get_element_variable(created_cv, &parameter.key());
                                wildcard_constraint(self.model, element);
                            }
                            if receiver.is_some() {
                                let expression_cv = self.cv(receiver);
                                let element = self.model.get_element_variable(expression_cv, &parameter.key());
                                wildcard_constraint(self.model, element);
                            }
                        }
                    } else if type_argument.is_type_variable() {
                        if let Some(cv) = method_type_variables.get(&type_argument.key()) {
                            self.model.create_equals_constraint(argument_element, Some(*cv));
                        } else {
                            if created_type.is_some() {
                                let created_cv = self.cv(created_type);
                                let element = self.model.get_element_variable(created_cv, &type_argument.key());
                                self.model.create_equals_constraint(argument_element, element);
                            }
                            if receiver.is_some() {
                                let expression_cv = self.cv(receiver);
                                let element = self.model.get_element_variable(expression_cv, &type_argument.key());
                                self.model.create_equals_constraint(argument_element, element);
                            }
                        }
                    } else {
                        let immutable = self.model.make_immutable(*type_argument);
                        self.model.create_equals_constraint(argument_element, Some(immutable));
                    }
                }
            } else if declared.is_array() {
                if let Some(element) = declared.element_type().filter(|e| e.is_type_variable()) {
                    if let Some(cv) = method_type_variables.get(&element.key()) {
                        let argument_element = self.model.array_element_variable(Some(argument_cv));
                        self.model.create_equals_constraint(argument_element, Some(*cv));
                    }
                }
            } else {
                if !is_a_generic_type(&declared) {
                    continue;
                }
                let parameter_cv = self.model.make_parameter_type_variable(Some(method), parameter_index, false);
                self.model.create_element_equals_constraints(parameter_cv, Some(argument_cv));
            }
        }
    }
}
