//! Port of `ConvertForLoopOperation` (index based `for` loop over an array
//! or `Collection` to an enhanced `for` loop).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::extract_temp::{import_context, CuRewrite};
use super::naming::{self, VarKind};
use super::scope::ScopeAnalyzer;
use crate::rewrite::import_rewrite::TypeLocation;
use crate::rewrite::RNode;
use crate::semantic_ast::resolve::subtree_match;
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeId, NodeKind};

pub struct ConvertForLoop {
    ast: Arc<Ast>,
    options: BTreeMap<String, String>,
    statement: NodeId,
    used_names: Vec<String>,
    make_final: bool,
    check_loop_var_used: bool,
    index_binding: Option<String>,
    length_binding: Option<String>,
    array_binding: Option<String>,
    array_access: Option<NodeId>,
    element_declaration: Option<NodeId>,
    is_collection: bool,
    size_method_type: Option<String>,
    get_method: Option<String>,
    size_method_access: Option<NodeId>,
    loop_var_referenced: bool,
}

fn int_literal(expression: Option<Node<'_>>) -> Option<i64> {
    let e = expression?;
    match e.kind() {
        NodeKind::NumberLiteral => {
            let token = e.simple("token")?.replace('_', "");
            let token = token.trim_end_matches(['l', 'L']);
            if let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
                i64::from_str_radix(hex, 16).ok()
            } else {
                token.parse().ok()
            }
        }
        NodeKind::ParenthesizedExpression => int_literal(e.child("expression")),
        _ => None,
    }
}

fn binding_of<'a>(expression: Node<'a>) -> Option<BindingRef<'a>> {
    match expression.kind() {
        NodeKind::FieldAccess => expression.child("name").and_then(|n| n.binding()),
        NodeKind::SimpleName | NodeKind::QualifiedName => expression.binding(),
        _ => None,
    }
}

fn same(binding: Option<&String>, other: Option<BindingRef<'_>>) -> bool {
    matches!((binding, other), (Some(k), Some(b)) if b.key() == k)
}

fn is_collection(class: BindingRef<'_>) -> bool {
    if class.erasure().unwrap_or(class).qualified_name().starts_with("java.util.Collection") {
        return true;
    }
    if class.superclass().is_some_and(is_collection) {
        return true;
    }
    class.interfaces().into_iter().any(is_collection)
}

fn type_binding_equal(a: Option<BindingRef<'_>>, b: Option<BindingRef<'_>>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    if a == b {
        return true;
    }
    if a.type_arguments().len() != b.type_arguments().len() || a.dimensions() != b.dimensions() || a.is_anonymous() || b.is_anonymous() {
        return false;
    }
    match (a.erasure(), b.erasure()) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

impl ConvertForLoop {
    pub fn new(ast: Arc<Ast>, options: BTreeMap<String, String>, statement: Node<'_>) -> Self {
        Self::with(ast, options, statement, Vec::new(), false, false)
    }

    pub fn with(ast: Arc<Ast>, options: BTreeMap<String, String>, statement: Node<'_>, used_names: Vec<String>, make_final: bool, check_loop_var_used: bool) -> Self {
        ConvertForLoop {
            ast,
            options,
            statement: statement.id,
            used_names,
            make_final,
            check_loop_var_used,
            index_binding: None,
            length_binding: None,
            array_binding: None,
            array_access: None,
            element_declaration: None,
            is_collection: false,
            size_method_type: None,
            get_method: None,
            size_method_access: None,
            loop_var_referenced: false,
        }
    }

    fn statement(&self) -> Node<'_> {
        self.ast.node(self.statement)
    }

    /// `satisfiesPreconditions().isOK()`.
    pub fn satisfies_preconditions(&mut self) -> bool {
        let ast = self.ast.clone();
        let statement = ast.node(self.statement);
        if !self.validate_initializers(statement) || !self.validate_expression(statement) || !self.validate_updaters(statement) || !self.validate_body(statement) {
            return false;
        }
        !(self.check_loop_var_used && !self.loop_var_referenced)
    }

    fn validate_initializers(&mut self, statement: Node<'_>) -> bool {
        let initializers = statement.list("initializers");
        if initializers.len() != 1 || !initializers[0].is(NodeKind::VariableDeclarationExpression) {
            return false;
        }
        let declaration = initializers[0];
        let declaration_type = declaration.type_binding().or_else(|| declaration.child("type").and_then(|t| t.type_binding()));
        let Some(binding) = declaration_type else { return false };
        if !binding.is_primitive() || binding.qualified_name() != "int" {
            return false;
        }
        let fragments = declaration.list("fragments");
        let index_binding = |f: Node<'_>| -> Option<String> {
            if int_literal(f.child("initializer")) != Some(0) {
                return None;
            }
            f.child("name").and_then(|n| n.binding()).map(|b| b.key().to_owned())
        };
        match fragments.len() {
            1 => match index_binding(fragments[0]) {
                Some(b) => {
                    self.index_binding = Some(b);
                    true
                }
                None => false,
            },
            2 => {
                let (index, length_fragment) = match index_binding(fragments[0]) {
                    Some(b) => (b, fragments[1]),
                    None => match index_binding(fragments[1]) {
                        Some(b) => (b, fragments[0]),
                        None => return false,
                    },
                };
                if !self.validate_length_fragment(length_fragment) {
                    return false;
                }
                self.index_binding = Some(index);
                true
            }
            _ => false,
        }
    }

    fn validate_length_fragment(&mut self, fragment: Node<'_>) -> bool {
        let Some(initializer) = fragment.child("initializer") else { return false };
        if !self.validate_length_query(initializer) {
            return false;
        }
        match fragment.child("name").and_then(|n| n.binding()) {
            Some(b) => {
                self.length_binding = Some(b.key().to_owned());
                true
            }
            None => false,
        }
    }

    fn validate_expression(&mut self, statement: Node<'_>) -> bool {
        let Some(infix) = statement.child("expression").filter(|e| e.is(NodeKind::InfixExpression)) else { return false };
        let (Some(left), Some(right)) = (infix.child("leftOperand"), infix.child("rightOperand")) else { return false };
        let operator = infix.simple("operator").unwrap_or("");
        let index = self.index_binding.clone();
        let index = index.as_ref();
        if left.is(NodeKind::SimpleName) && right.is(NodeKind::SimpleName) {
            let Some(length) = self.length_binding.as_ref() else { return false };
            if same(index, left.binding()) {
                return same(Some(length), right.binding()) && matches!(operator, "<" | "!=");
            }
            if same(index, right.binding()) {
                return same(Some(length), left.binding()) && matches!(operator, ">" | "!=");
            }
        } else if left.is(NodeKind::SimpleName) {
            if !same(index, left.binding()) || !matches!(operator, "<" | "!=") {
                return false;
            }
            return self.validate_length_query(right);
        } else if right.is(NodeKind::SimpleName) {
            if !same(index, right.binding()) || !matches!(operator, ">" | "!=") {
                return false;
            }
            return self.validate_length_query(left);
        }
        false
    }

    fn validate_length_query(&mut self, query: Node<'_>) -> bool {
        match query.kind() {
            NodeKind::QualifiedName => {
                if query.child("name").map(|n| n.identifier()).as_deref() != Some("length") {
                    return false;
                }
                let Some(array) = query.child("qualifier") else { return false };
                if !array.type_binding().is_some_and(|t| t.is_array()) {
                    return false;
                }
                let Some(binding) = array.binding() else { return false };
                self.array_binding = Some(binding.key().to_owned());
                self.array_access = Some(array.id);
                true
            }
            NodeKind::FieldAccess => {
                if query.child("name").map(|n| n.identifier()).as_deref() != Some("length") {
                    return false;
                }
                let Some(array) = query.child("expression") else { return false };
                if !array.type_binding().is_some_and(|t| t.is_array()) {
                    return false;
                }
                let Some(binding) = binding_of(array) else { return false };
                self.array_binding = Some(binding.key().to_owned());
                self.array_access = Some(array.id);
                true
            }
            NodeKind::MethodInvocation => {
                if query.child("name").map(|n| n.identifier()).as_deref() != Some("size") || !query.list("arguments").is_empty() {
                    return false;
                }
                let Some(method) = query.method_binding() else { return false };
                let class = method.declaring_class();
                let Some(expression) = query.child("expression") else { return false };
                let Some(expression_type) = expression.type_binding() else { return false };
                if class.is_some_and(is_collection) {
                    self.is_collection = true;
                    self.size_method_type = Some(expression_type.key().to_owned());
                    self.size_method_access = Some(query.id);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn validate_updaters(&self, statement: Node<'_>) -> bool {
        let updaters = statement.list("updaters");
        if updaters.len() != 1 {
            return false;
        }
        let updater = updaters[0];
        let index = self.index_binding.as_ref();
        let is_index = |e: Option<Node<'_>>| e.is_some_and(|e| same(index, binding_of(e)));
        match updater.kind() {
            NodeKind::PostfixExpression => updater.simple("operator") == Some("++") && is_index(updater.child("operand")),
            NodeKind::PrefixExpression => updater.simple("operator") == Some("++") && is_index(updater.child("operand")),
            NodeKind::Assignment => {
                if !is_index(updater.child("leftHandSide")) {
                    return false;
                }
                match updater.simple("operator") {
                    Some("+=") => int_literal(updater.child("rightHandSide")) == Some(1),
                    Some("=") => {
                        let Some(infix) = updater.child("rightHandSide").filter(|r| r.is(NodeKind::InfixExpression)) else { return false };
                        let (left, right) = (infix.child("leftOperand"), infix.child("rightOperand"));
                        if is_index(left) {
                            int_literal(right) == Some(1)
                        } else if is_index(right) {
                            int_literal(left) == Some(1)
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn validate_body(&mut self, statement: Node<'_>) -> bool {
        let Some(body) = statement.child("body") else { return false };
        let ast = self.ast.clone();
        self.visit_body(ast.node(body.id)).is_ok()
    }

    fn visit_body(&mut self, node: Node<'_>) -> Result<(), ()> {
        match node.kind() {
            NodeKind::ArrayAccess => self.visit_array_access(node),
            NodeKind::MethodInvocation => self.visit_method_invocation(node),
            _ => {}
        }
        if node.is(NodeKind::ContinueStatement) {
            return Ok(());
        }
        if matches!(node.kind(), NodeKind::SimpleName | NodeKind::QualifiedName) {
            self.check_name(node)?;
        } else if self.is_collection && node.is(NodeKind::MethodInvocation) {
            let binding = node.method_binding().ok_or(())?;
            if type_binding_equal(self.size_method_type_binding(), binding.declaring_class()) {
                let name = node.child("name").map(|n| n.identifier()).unwrap_or_default();
                if !matches!(name.as_str(), "size" | "get" | "isEmpty") {
                    return Err(());
                }
            }
        }
        for child in node.children() {
            self.visit_body(child)?;
        }
        Ok(())
    }

    fn size_method_type_binding(&self) -> Option<BindingRef<'_>> {
        self.size_method_type.as_deref().and_then(|k| self.ast.binding_by_key(k))
    }

    fn visit_array_access(&mut self, node: Node<'_>) {
        if self.element_declaration.is_some() {
            return;
        }
        let binding = node.child("array").and_then(binding_of);
        if !self.is_collection && same(self.array_binding.as_ref(), binding) {
            let index = node.child("index").and_then(binding_of);
            if same(self.index_binding.as_ref(), index) && node.location_is("initializer") {
                if let Some(p) = node.parent().filter(|p| p.is(NodeKind::VariableDeclarationFragment)) {
                    self.element_declaration = Some(p.id);
                }
            }
        }
    }

    fn visit_method_invocation(&mut self, node: Node<'_>) {
        if self.element_declaration.is_some() || !self.is_collection {
            return;
        }
        let Some(binding) = node.method_binding() else { return };
        let parameters = binding.parameter_types();
        if binding.name() == "get" && parameters.len() == 1 && parameters[0].name() == "int" && type_binding_equal(binding.declaring_class(), self.size_method_type_binding()) {
            let index = node.list("arguments").first().copied().and_then(binding_of);
            if same(self.index_binding.as_ref(), index) && node.location_is("initializer") {
                if let Some(p) = node.parent().filter(|p| p.is(NodeKind::VariableDeclarationFragment)) {
                    self.element_declaration = Some(p.id);
                }
            }
        }
    }

    fn is_assigned(mut current: Node<'_>) -> bool {
        loop {
            if current.kind().is_statement() {
                return false;
            }
            if current.location_is("leftHandSide") && current.parent().is_some_and(|p| p.is(NodeKind::Assignment)) {
                return true;
            }
            if current.is(NodeKind::PrefixExpression) && current.simple("operator") != Some("!") {
                return true;
            }
            if current.is(NodeKind::PostfixExpression) {
                return true;
            }
            match current.parent() {
                Some(p) => current = p,
                None => return false,
            }
        }
    }

    fn check_name(&mut self, node: Node<'_>) -> Result<(), ()> {
        let binding = node.binding().ok_or(())?;
        let key = binding.key();
        if self.index_binding.as_deref() == Some(key) {
            self.loop_var_referenced = true;
            let parent = node.parent();
            if node.location_is("index") && parent.is_some_and(|p| p.is(NodeKind::ArrayAccess)) {
                if self.is_collection {
                    return Err(());
                }
                let array = parent.and_then(|p| p.child("array")).ok_or(())?;
                let access = self.array_access.map(|id| self.ast.node(id)).ok_or(())?;
                match array.kind() {
                    NodeKind::QualifiedName => {
                        if !access.is(NodeKind::QualifiedName) {
                            return Err(());
                        }
                        let b1 = array.child("qualifier").and_then(|q| q.binding()).ok_or(())?;
                        let b2 = access.child("qualifier").and_then(|q| q.binding());
                        if Some(b1) != b2 {
                            return Err(());
                        }
                    }
                    NodeKind::FieldAccess => {
                        let expression = array.child("expression").ok_or(())?;
                        if expression.is(NodeKind::ThisExpression) {
                            if access.is(NodeKind::FieldAccess) {
                                if !access.child("expression").is_some_and(|e| e.is(NodeKind::ThisExpression)) {
                                    return Err(());
                                }
                            } else if access.is(NodeKind::QualifiedName) {
                                return Err(());
                            }
                        } else {
                            if !access.is(NodeKind::FieldAccess) {
                                return Err(());
                            }
                            let other = access.child("expression").ok_or(())?;
                            if !subtree_match(expression, other) {
                                return Err(());
                            }
                        }
                    }
                    _ => {
                        if access.is(NodeKind::QualifiedName) {
                            return Err(());
                        }
                        if access.is(NodeKind::FieldAccess) && !access.child("expression").is_some_and(|e| e.is(NodeKind::ThisExpression)) {
                            return Err(());
                        }
                    }
                }
                let array_binding = binding_of(array).ok_or(())?;
                if self.array_binding.as_deref() != Some(array_binding.key()) {
                    return Err(());
                }
            } else if node.location_is("arguments") && parent.is_some_and(|p| p.is(NodeKind::MethodInvocation)) {
                let method = parent.ok_or(())?;
                let method_binding = method.method_binding().ok_or(())?;
                let parameters = method_binding.parameter_types();
                let size_access = self.size_method_access.map(|id| self.ast.node(id));
                let matches_size = size_access.and_then(|s| s.child("expression")).is_some_and(|e| method.child("expression").is_some_and(|m| subtree_match(e, m)));
                if !self.is_collection
                    || method.child("name").map(|n| n.identifier()).as_deref() != Some("get")
                    || parameters.len() != 1
                    || parameters[0].name() != "int"
                    || !type_binding_equal(self.size_method_type_binding(), method_binding.declaring_class())
                    || !matches_size
                {
                    return Err(());
                }
                self.get_method = Some(method_binding.key().to_owned());
            } else {
                return Err(());
            }
        } else if self.array_binding.as_deref() == Some(key) {
            if Self::is_assigned(node) {
                return Err(());
            }
        } else if self.length_binding.as_deref() == Some(key) {
            return Err(());
        } else if let Some(element) = self.element_declaration {
            let element_key = self.ast.node(element).child("name").and_then(|n| n.binding()).map(|b| b.key().to_owned());
            if element_key.as_deref() == Some(key) && Self::is_assigned(node) {
                self.element_declaration = None;
            }
        }
        Ok(())
    }

    fn used_variable_names(&self) -> Vec<String> {
        let statement = self.statement();
        let mut results: Vec<String> = ScopeAnalyzer::new(self.ast.root()).used_variable_names(statement.start(), statement.length()).into_iter().collect();
        super::walk(statement, &mut |n| {
            if matches!(n.kind(), NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment) {
                if let Some(name) = n.child("name") {
                    results.push(name.identifier());
                }
            }
            true
        });
        results.extend(self.used_names.iter().cloned());
        results
    }

    fn variable_name_proposals(&self) -> Vec<String> {
        let used = self.used_variable_names();
        let ast = &self.ast;
        let mut result;
        if self.is_collection {
            let access = ast.node(self.size_method_access.expect("size access"));
            let expression = access.child("expression");
            let name = expression.filter(|e| e.is(NodeKind::SimpleName)).map(|e| e.identifier()).unwrap_or_default();
            let base = naming::modify_base_name(&name);
            result = naming::suggest_variable_names(VarKind::Local, &base, 0, &used, &self.options, true);
            let type_arguments = self.size_method_type_binding().map(|b| b.type_arguments()).unwrap_or_default();
            let type_name = match type_arguments.first() {
                None => "Object".to_owned(),
                Some(t) if t.is_capture() => crate::correction::type_mismatch::bindings::normalize_wildcard_type(*t, true).map(|b| b.name().to_owned()).unwrap_or_else(|| "Object".to_owned()),
                Some(t) => t.name().to_owned(),
            };
            result.extend(naming::suggest_variable_names(VarKind::Local, &type_name, 0, &used, &self.options, true));
        } else {
            let array = self.array_binding.as_deref().and_then(|k| ast.binding_by_key(k));
            let base = naming::modify_base_name(array.map(|a| a.name()).unwrap_or(""));
            result = naming::suggest_variable_names(VarKind::Local, &base, 0, &used, &self.options, true);
            if let Some(array_type) = self.array_access.and_then(|id| ast.node(id).type_binding()) {
                let element = array_type.element_type().unwrap_or(array_type);
                result.extend(naming::suggest_variable_names(VarKind::Local, element.name(), (array_type.dimensions() - 1).max(0) as usize, &used, &self.options, true));
            }
        }
        result
    }

    /// `getIntroducedVariableName()`.
    pub fn introduced_variable_name(&self) -> String {
        if let Some(element) = self.element_declaration {
            return self.ast.node(element).child("name").map(|n| n.identifier()).unwrap_or_default();
        }
        self.variable_name_proposals().into_iter().next().unwrap_or_default()
    }

    /// `rewriteAST`: replaces the `for` statement.
    pub fn rewrite(&self, cu: &mut CuRewrite) {
        let ast = self.ast.clone();
        let statement = ast.node(self.statement);
        cu.rewrite.add_tight_source_node(statement.id);
        let result = self.convert(cu, statement);
        cu.rewrite.replace(RNode::Orig(statement.id), Some(result));
    }

    fn convert(&self, cu: &mut CuRewrite, statement: Node<'_>) -> RNode {
        let ast = self.ast.clone();
        let proposals = self.variable_name_proposals();
        let parameter_name = match self.element_declaration {
            Some(e) => ast.node(e).child("name").map(|n| n.identifier()).unwrap_or_default(),
            None => proposals.first().cloned().unwrap_or_default(),
        };
        let result = cu.rewrite.new_node(NodeKind::EnhancedForStatement);
        let (parameter, expression) = if self.is_collection {
            let access = ast.node(self.size_method_access.expect("size access"));
            let expression = access.child("expression").expect("expression");
            (self.create_parameter_collection(cu, &parameter_name, statement, expression), expression)
        } else {
            let array = ast.node(self.array_access.expect("array access"));
            (self.create_parameter(cu, &parameter_name, statement, array), array)
        };
        cu.rewrite.put_child(result, "parameter", parameter);
        let copy = cu.rewrite.create_copy_target(expression.id);
        cu.rewrite.put_child(result, "expression", copy);
        let body = statement.child("body").expect("body");
        self.convert_body(cu, body, &parameter_name);
        let body_target = cu.rewrite.create_move_target(body.id);
        cu.rewrite.put_child(result, "body", body_target);
        result
    }

    fn parameter_modifiers(&self, cu: &mut CuRewrite, parameter: RNode) {
        let ast = self.ast.clone();
        let mut modifiers = Vec::new();
        let mut has_final = false;
        if let Some(fragment) = self.element_declaration.map(|id| ast.node(id)) {
            if let Some(declaration) = fragment.parent() {
                for m in declaration.list("modifiers") {
                    if m.is(NodeKind::Modifier) && m.simple("keyword") == Some("final") {
                        has_final = true;
                    }
                    modifiers.push(cu.rewrite.create_copy_target(m.id));
                }
            }
        }
        if self.make_final && !has_final {
            modifiers.extend(cu.rewrite.new_modifiers(modifier::FINAL));
        }
        cu.rewrite.put_list(parameter, "modifiers", modifiers);
    }

    fn create_parameter(&self, cu: &mut CuRewrite, name: &str, statement: Node<'_>, array: Node<'_>) -> RNode {
        let rw = &mut cu.rewrite;
        let parameter = rw.new_node(NodeKind::SingleVariableDeclaration);
        let name_node = rw.new_simple_name(name);
        rw.put_child(parameter, "name", name_node);
        let array_type = array.type_binding().expect("array type");
        let element = array_type.element_type().unwrap_or(array_type);
        let location = if array_type.dimensions() == 1 { TypeLocation::LocalVariable } else { TypeLocation::ArrayContents };
        let context = import_context(&self.ast, statement, &self.options);
        let mut typ = cu.imports.add_import_type(element, &mut cu.rewrite, &context, location);
        if array_type.dimensions() != 1 {
            let array = cu.rewrite.new_node(NodeKind::ArrayType);
            cu.rewrite.put_child(array, "elementType", typ);
            let dimensions = (1..array_type.dimensions()).map(|_| cu.rewrite.new_node(NodeKind::Dimension)).collect();
            cu.rewrite.put_list(array, "dimensions", dimensions);
            typ = array;
        }
        cu.rewrite.put_child(parameter, "type", typ);
        self.parameter_modifiers(cu, parameter);
        parameter
    }

    fn create_parameter_collection(&self, cu: &mut CuRewrite, name: &str, statement: Node<'_>, expression: Node<'_>) -> RNode {
        let parameter = cu.rewrite.new_node(NodeKind::SingleVariableDeclaration);
        let name_node = cu.rewrite.new_simple_name(name);
        cu.rewrite.put_child(parameter, "name", name_node);
        let type_arguments = expression.type_binding().map(|b| b.type_arguments()).unwrap_or_default();
        let element = match type_arguments.first() {
            None => crate::correction::type_mismatch::bindings::well_known(&self.ast, "java.lang.Object"),
            Some(t) if t.is_capture() => crate::correction::type_mismatch::bindings::normalize_wildcard_type(*t, true),
            Some(t) => Some(*t),
        };
        let context = import_context(&self.ast, statement, &self.options);
        let typ = match element {
            Some(e) => cu.imports.add_import_type(e, &mut cu.rewrite, &context, TypeLocation::LocalVariable),
            None => {
                let n = cu.rewrite.new_simple_name("Object");
                cu.rewrite.new_simple_type(n)
            }
        };
        cu.rewrite.put_child(parameter, "type", typ);
        self.parameter_modifiers(cu, parameter);
        parameter
    }

    fn convert_body(&self, cu: &mut CuRewrite, body: Node<'_>, parameter_name: &str) {
        super::walk(body, &mut |n| {
            if self.is_collection {
                if n.is(NodeKind::MethodInvocation) {
                    let matches_get = n.method_binding().is_some_and(|b| self.get_method.as_deref() == Some(b.key()));
                    let arguments = n.list("arguments");
                    if matches_get && arguments.len() == 1 && arguments[0].is(NodeKind::SimpleName) && same(self.index_binding.as_ref(), arguments[0].binding()) {
                        self.replace_access(cu, n, parameter_name, true);
                    }
                }
            } else if n.is(NodeKind::ArrayAccess) {
                let array = n.child("array").and_then(binding_of);
                let index = n.child("index").and_then(binding_of);
                if same(self.array_binding.as_ref(), array) && same(self.index_binding.as_ref(), index) {
                    self.replace_access(cu, n, parameter_name, false);
                }
            }
            !n.is(NodeKind::ContinueStatement) || true
        });
    }

    fn replace_access(&self, cu: &mut CuRewrite, node: Node<'_>, parameter_name: &str, collection: bool) {
        let rw = &mut cu.rewrite;
        if self.element_declaration.is_some() && node.location_is("initializer") {
            if let Some(fragment) = node.parent().filter(|p| p.is(NodeKind::VariableDeclarationFragment)) {
                let name = fragment.child("name");
                let removable = if collection {
                    name.is_some_and(|n| n.binding().is_some() && n.identifier() == parameter_name)
                } else {
                    name.is_some_and(|n| n.binding().is_some())
                };
                if removable {
                    if let Some(statement) = fragment.parent().filter(|p| p.is(NodeKind::VariableDeclarationStatement)) {
                        if statement.list("fragments").len() == 1 {
                            rw.remove(RNode::Orig(statement.id));
                        } else {
                            rw.list_remove(RNode::Orig(statement.id), "fragments", RNode::Orig(fragment.id));
                        }
                    }
                    return;
                }
            }
        }
        let name = rw.new_simple_name(parameter_name);
        rw.replace(RNode::Orig(node.id), Some(name));
    }
}
