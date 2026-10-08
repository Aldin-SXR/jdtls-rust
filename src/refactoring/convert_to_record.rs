//! Port of jdt.core.manipulation's `ConvertToRecordRefactoring` (behind the
//! "Convert to record" quick assist, `ConvertRecordSubProcessor`).
//!
//! The search engine queries of the refactoring (sub-classes of the type,
//! references to the replaced getters) run over the project's units: the
//! candidate units are those whose text mentions the searched name, and the
//! matches are decided on their resolved ASTs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::Status;
use crate::correction::edit::Env;
use crate::correction::messages;
use crate::correction::CuChange;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier as m, Ast, BindingRef, Node, NodeId, NodeKind};

fn msg(key: &str) -> String {
    messages::refactoring(key).to_owned()
}

/// `VisitException`.
struct VisitException(String);

type Visit<T> = Result<T, VisitException>;

fn throw<T>(key: &str) -> Visit<T> {
    Err(VisitException(msg(key)))
}

/// The key identifying a variable binding (`isEqualTo`).
fn variable_key(b: BindingRef<'_>) -> String {
    b.variable_declaration().unwrap_or(b).key().to_owned()
}

/// The key identifying a method binding (`isEqualTo` on declarations).
fn method_key(b: BindingRef<'_>) -> String {
    b.method_declaration().unwrap_or(b).key().to_owned()
}

/// A field whose getter is replaced by the record accessor.
#[derive(Clone, Debug)]
struct Getter {
    field_key: String,
    field_name: String,
    method_key: String,
    method_name: String,
}

/// The analysis `checkInitialConditions` leaves for `createChange`.
#[derive(Clone)]
pub struct ConvertToRecord {
    ast: Arc<Ast>,
    type_declaration: NodeId,
    constructor: NodeId,
    getters: Vec<Getter>,
    methods_to_copy: Vec<NodeId>,
    fields_to_copy: Vec<NodeId>,
    initializers_to_copy: Vec<NodeId>,
}

/// The outer `ASTVisitor` of `checkInitialConditions`.
struct Checker<'a> {
    type_key: String,
    fields_to_check: Vec<String>,
    fields_to_set: Vec<BindingRef<'a>>,
    getters: Vec<Getter>,
    constructor: Option<NodeId>,
    methods_to_copy: Vec<NodeId>,
    fields_to_copy: Vec<NodeId>,
    initializers_to_copy: Vec<NodeId>,
}

/// `getFieldBinding(exp)`.
fn field_binding<'a>(exp: Option<Node<'a>>) -> Option<BindingRef<'a>> {
    let exp = exp?;
    let binding = match exp.kind() {
        NodeKind::FieldAccess => exp.binding().or_else(|| exp.child("name").and_then(|n| n.binding())),
        NodeKind::SimpleName => exp.binding(),
        _ => None,
    }?;
    (binding.is_variable() && binding.is_field()).then_some(binding)
}

/// `ASTNodes.asList(statement)`.
fn statements_of(body: Option<Node<'_>>) -> Vec<Node<'_>> {
    match body {
        None => Vec::new(),
        Some(b) if b.is(NodeKind::Block) => b.list("statements"),
        Some(b) => vec![b],
    }
}

/// State of the constructor's `checkFields` visitor.
#[derive(Default)]
struct FieldsState {
    then_statement: Option<(usize, usize)>,
    else_statement: Option<usize>,
    in_if: bool,
    then_fields: HashSet<String>,
    assigned: HashSet<String>,
    need_to_copy: bool,
}

impl<'a> Checker<'a> {
    fn walk(&mut self, n: Node<'a>) -> Visit<()> {
        // `ASTVisitor()` does not visit doc tags.
        if n.is(NodeKind::Javadoc) {
            return Ok(());
        }
        if self.visit(n)? {
            for c in n.children() {
                self.walk(c)?;
            }
        }
        Ok(())
    }

    fn visit(&mut self, n: Node<'a>) -> Visit<bool> {
        match n.kind() {
            NodeKind::FieldAccess => {
                if !self.fields_to_check.is_empty() {
                    let Some(binding) = n.binding().or_else(|| n.child("name").and_then(|c| c.binding())) else {
                        return throw("ConvertToRecordRefactoring_unexpected_error");
                    };
                    self.check_for_field_change(n, binding)?;
                }
                Ok(false)
            }
            NodeKind::SimpleName => {
                let Some(binding) = n.binding() else {
                    return throw("ConvertToRecordRefactoring_unexpected_error");
                };
                if !self.fields_to_check.is_empty() && binding.is_variable() && binding.is_field() {
                    self.check_for_field_change(n, binding)?;
                }
                Ok(false)
            }
            NodeKind::TypeDeclaration => {
                let Some(binding) = n.binding() else {
                    return throw("ConvertToRecordRefactoring_unexpected_error");
                };
                if !binding.is_local() && binding.key() != self.type_key {
                    return throw("ConvertToRecordRefactoring_member_types_not_supported");
                }
                Ok(!binding.is_local())
            }
            NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration => throw("ConvertToRecordRefactoring_not_simple_case"),
            NodeKind::Initializer => {
                if n.modifiers() & m::STATIC != 0 {
                    self.initializers_to_copy.push(n.id);
                    Ok(false)
                } else {
                    throw("ConvertToRecordRefactoring_has_initializer")
                }
            }
            NodeKind::FieldDeclaration => {
                if n.modifiers() & m::STATIC != 0 {
                    self.fields_to_copy.push(n.id);
                } else if n.list("fragments").iter().any(|f| f.child("initializer").is_some()) {
                    return throw("ConvertToRecordRefactoring_fields_initialized");
                }
                Ok(false)
            }
            NodeKind::MethodDeclaration => self.visit_method(n),
            _ => Ok(true),
        }
    }

    fn check_for_field_change(&self, node: Node<'a>, binding: BindingRef<'a>) -> Visit<()> {
        if binding.modifiers() & m::PRIVATE != 0 && self.fields_to_check.contains(&variable_key(binding)) && node.location() == Some("leftHandSide") && node.parent().is_some_and(|p| p.is(NodeKind::Assignment)) {
            return throw("ConvertToRecordRefactoring_cannot_convert_fields");
        }
        Ok(())
    }

    fn visit_method(&mut self, node: Node<'a>) -> Visit<bool> {
        let statements = statements_of(node.child("body"));
        if node.flag("constructor") {
            if statements.len() == 1 && statements[0].is(NodeKind::ConstructorInvocation) {
                self.methods_to_copy.push(node.id);
                return Ok(false);
            }
            if self.constructor.is_some_and(|c| c != node.id) {
                return throw("ConvertToRecordRefactoring_not_simple_case");
            }
            if !node.list("thrownExceptionTypes").is_empty() {
                return throw("ConvertToRecordRefactoring_nonstandard_constructor");
            }
            self.constructor = Some(node.id);
            let parameters = node.list("parameters");
            for field in &self.fields_to_set {
                let found = parameters.iter().any(|p| p.child("name").is_some_and(|n| n.identifier() == field.name()));
                if !found {
                    return throw("ConvertToRecordRefactoring_nonstandard_constructor");
                }
            }
            let mut state = FieldsState::default();
            if let Some(body) = node.child("body") {
                check_fields(body, &mut state)?;
            }
            if state.assigned.is_empty() || state.assigned.len() != self.fields_to_set.len() {
                return throw("ConvertToRecordRefactoring_not_simple_case");
            }
            if state.need_to_copy {
                self.methods_to_copy.push(node.id);
            }
            return Ok(false);
        }
        let modifiers = node.modifiers();
        if modifiers & m::NATIVE != 0 {
            return throw("ConvertToRecordRefactoring_not_simple_case");
        }
        if modifiers & m::STATIC != 0 {
            self.methods_to_copy.push(node.id);
        } else if modifiers & m::PUBLIC != 0 {
            if statements.len() == 1 && statements[0].is(NodeKind::ReturnStatement) {
                if let Some(field) = field_binding(statements[0].child("expression")) {
                    let Some(method) = node.binding() else {
                        return throw("ConvertToRecordRefactoring_unexpected_error");
                    };
                    let field_key = variable_key(field);
                    match self.getters.iter().find(|g| g.field_key == field_key) {
                        None => self.getters.push(Getter {
                            field_key,
                            field_name: field.name().to_owned(),
                            method_key: method_key(method),
                            method_name: method.name().to_owned(),
                        }),
                        Some(g) if g.method_key != method_key(method) => return throw("ConvertToRecordRefactoring_not_simple_case"),
                        Some(_) => {}
                    }
                } else {
                    self.methods_to_copy.push(node.id);
                }
            } else {
                self.methods_to_copy.push(node.id);
            }
        } else {
            self.methods_to_copy.push(node.id);
        }
        Ok(!self.fields_to_check.is_empty())
    }
}

/// The constructor body's `checkFields` visitor (`preVisit2`, `visit` and
/// `endVisit`).
fn check_fields(n: Node<'_>, st: &mut FieldsState) -> Visit<()> {
    // preVisit2
    if n.kind().is_statement() {
        match n.kind() {
            NodeKind::IfStatement => st.need_to_copy = true,
            NodeKind::Block | NodeKind::ExpressionStatement => {}
            NodeKind::TypeDeclarationStatement => {
                st.need_to_copy = true;
                return Ok(());
            }
            _ => return throw("ConvertToRecordRefactoring_nonstandard_constructor"),
        }
    }
    let descend = match n.kind() {
        NodeKind::Assignment => {
            let Some(field) = field_binding(n.child("leftHandSide")) else {
                return throw("ConvertToRecordRefactoring_nonstandard_constructor");
            };
            let key = variable_key(field);
            let start = n.start();
            if let Some((then_start, _)) = st.then_statement {
                if start >= then_start && st.else_statement.is_none_or(|e| start < e) {
                    st.then_fields.insert(key.clone());
                    if !st.assigned.insert(key) {
                        return throw("ConvertToRecordRefactoring_nonstandard_constructor");
                    }
                } else {
                    // `elseStatement` is null for an assignment in the condition (an NPE in JDT).
                    let Some(else_start) = st.else_statement else {
                        return throw("ConvertToRecordRefactoring_unexpected_error");
                    };
                    if start >= else_start && !st.then_fields.remove(&key) {
                        return throw("ConvertToRecordRefactoring_nonstandard_constructor");
                    }
                }
            } else if !st.assigned.insert(key) {
                return throw("ConvertToRecordRefactoring_nonstandard_constructor");
            }
            false
        }
        NodeKind::IfStatement => {
            if st.in_if {
                return throw("ConvertToRecordRefactoring_nonstandard_constructor");
            }
            st.in_if = true;
            st.then_statement = n.child("thenStatement").map(|t| (t.start(), t.end()));
            st.else_statement = n.child("elseStatement").map(|e| e.start());
            true
        }
        _ => true,
    };
    if descend {
        for c in n.children() {
            check_fields(c, st)?;
        }
    }
    if n.is(NodeKind::IfStatement) {
        st.in_if = false;
        st.then_statement = None;
        st.else_statement = None;
        if !st.then_fields.is_empty() {
            return throw("ConvertToRecordRefactoring_nonstandard_constructor");
        }
    }
    Ok(())
}

/// `ASTNodes.getFirstAncestorOrNull(node, AbstractTypeDeclaration, RecordDeclaration, AnonymousClassDeclaration)`.
fn first_type_ancestor(node: Node<'_>) -> Option<Node<'_>> {
    node.ancestors().find(|a| a.kind().is_abstract_type_declaration() || a.is(NodeKind::AnonymousClassDeclaration))
}

/// `isBoolean(field)`.
fn is_boolean(field: BindingRef<'_>) -> bool {
    field.var_type().is_some_and(|t| matches!(t.qualified_name(), "boolean" | "java.lang.Boolean"))
}

/// The units of the project that mention `name` (the search engine's
/// candidates), with the current unit's AST reused.
async fn candidate_units(env: &Env<'_>, ast: &Arc<Ast>, names: &[&str]) -> Vec<Arc<Ast>> {
    let Ok(url) = tower_lsp::lsp_types::Url::parse(&ast.uri) else { return vec![ast.clone()] };
    let ctx = env.dispatcher.context_for(Some(&url)).await;
    let mut uris: Vec<&String> = ctx.files.iter().filter(|(u, src)| **u != ast.uri && u.ends_with(".java") && names.iter().any(|n| src.contains(n))).map(|(u, _)| u).collect();
    uris.sort();
    let mut units = vec![ast.clone()];
    for uri in uris {
        if let Ok(unit) = crate::semantic_ast::fetch_with(env.dispatcher, uri, ctx.clone()).await {
            units.push(unit);
        }
    }
    units
}

impl ConvertToRecord {
    /// `checkInitialConditions` (and the always-OK `checkFinalConditions`):
    /// the refactoring when the status is OK, otherwise the status.
    pub async fn check_all_conditions(env: &Env<'_>, ast: &Arc<Ast>, selection_start: usize, selection_length: usize) -> Result<ConvertToRecord, Status> {
        let options = env.options(&ast.uri).await;
        let use_is = match tower_lsp::lsp_types::Url::parse(&ast.uri) {
            Ok(url) => crate::features::accessors::profile(env.dispatcher, &url).await.use_is,
            Err(_) => true,
        };
        let fatal = |key: &str| Status::fatal(msg(key));
        let root = ast.root();
        let analyzer = super::selection::SelectionAnalyzer::analyze(super::selection::Selection::from_start_length(selection_start, selection_length), false, root);
        let selected = analyzer.first_selected(root);
        let selected_type = selected.and_then(first_type_ancestor);
        let Some(type_decl) = selected_type.filter(|t| t.is(NodeKind::TypeDeclaration)) else {
            return Err(fatal("ConvertToRecordRefactoring_no_type"));
        };
        let Some(type_binding) = type_decl.binding() else {
            return Err(fatal("ConvertToRecordRefactoring_unexpected_error"));
        };
        if type_binding.superclass().is_some_and(|s| s.qualified_name() != "java.lang.Object") {
            return Err(fatal("ConvertToRecordRefactoring_cannot_extend"));
        }
        let type_modifiers = type_binding.modifiers();
        if type_modifiers & (m::SEALED | m::ABSTRACT) != 0 {
            return Err(fatal("ConvertToRecordRefactoring_not_simple_case"));
        }
        let fields = type_binding.declared_fields().unwrap_or_default();
        if fields.is_empty() {
            return Err(fatal("ConvertToRecordRefactoring_not_simple_case"));
        }
        let mut fields_to_set = Vec::new();
        let mut fields_to_check = Vec::new();
        for field in &fields {
            let modifiers = field.modifiers();
            if modifiers & m::PRIVATE == 0 && modifiers & m::STATIC == 0 {
                return Err(fatal("ConvertToRecordRefactoring_not_private"));
            }
            if modifiers & m::PRIVATE != 0 && modifiers & m::FINAL == 0 {
                fields_to_check.push(variable_key(*field));
            }
            if modifiers & m::STATIC == 0 {
                fields_to_set.push(*field);
            }
        }
        let methods = type_binding.declared_methods().unwrap_or_default();
        if !methods.iter().any(|mb| mb.is_constructor()) {
            return Err(fatal("ConvertToRecordRefactoring_not_simple_case"));
        }
        let mut checker = Checker {
            type_key: type_binding.key().to_owned(),
            fields_to_check,
            fields_to_set: fields_to_set.clone(),
            getters: Vec::new(),
            constructor: None,
            methods_to_copy: Vec::new(),
            fields_to_copy: Vec::new(),
            initializers_to_copy: Vec::new(),
        };
        // `sourceType.getMethods()`: the declared methods of the source.
        let source_methods = type_decl.list("bodyDeclarations").iter().filter(|b| b.is(NodeKind::MethodDeclaration)).count();
        for field in &fields {
            if field.modifiers() & m::STATIC != 0 {
                continue;
            }
            if let Some(getter) = find_getter(&options, use_is, type_binding, *field) {
                checker.getters.push(Getter {
                    field_key: variable_key(*field),
                    field_name: field.name().to_owned(),
                    method_key: method_key(getter),
                    method_name: getter.name().to_owned(),
                });
            }
            if find_setter(&options, use_is, type_binding, *field).is_some() {
                return Err(Status::fatal(messages::format(messages::refactoring("ConvertToRecordRefactoring_setter_found"), &[field.name()])));
            }
        }
        if let Err(VisitException(message)) = checker.walk(type_decl) {
            return Err(Status::fatal(message));
        }
        if source_methods > fields.len() + checker.methods_to_copy.len() + 1 {
            return Err(fatal("ConvertToRecordRefactoring_not_simple_case"));
        }
        let Some(constructor) = checker.constructor else {
            return Err(fatal("ConvertToRecordRefactoring_not_simple_case"));
        };
        let mut result = Status::ok();
        if checker.getters.len() < fields_to_set.len() {
            result.merge(Status::warning(msg("ConvertToRecordRefactoring_not_enough_getters")));
        }
        if !result.has_error() && type_modifiers & m::FINAL == 0 {
            // Search for a sub-class (super type references to the type).
            let type_name = type_binding.name().to_owned();
            for unit in candidate_units(env, ast, &[type_name.as_str()]).await {
                if let Some(element) = first_subclass(&unit, type_binding.key()) {
                    result.merge(Status::fatal(messages::format(messages::refactoring("ConvertToRecordRefactoring_subclassed_error"), &[element.as_str()])));
                    break;
                }
            }
        }
        if !result.is_ok() {
            return Err(result);
        }
        Ok(ConvertToRecord {
            ast: ast.clone(),
            type_declaration: type_decl.id,
            constructor,
            getters: checker.getters,
            methods_to_copy: checker.methods_to_copy,
            fields_to_copy: checker.fields_to_copy,
            initializers_to_copy: checker.initializers_to_copy,
        })
    }

    /// `createChange`: the record replacing the type and the replaced getter
    /// invocations, one change per unit (sorted by unit name).
    pub async fn create_change(&self, env: &Env<'_>) -> Vec<CuChange> {
        let mut base = ASTRewrite::new(self.ast.clone());
        self.create_new_record(&mut base);
        let mut others = Vec::new();
        if !self.getters.is_empty() {
            let names: Vec<&str> = self.getters.iter().map(|g| g.method_name.as_str()).collect();
            for unit in candidate_units(env, &self.ast, &names).await {
                if Arc::ptr_eq(&unit, &self.ast) {
                    self.replace_references(&mut base);
                } else {
                    let mut rw = ASTRewrite::new(unit.clone());
                    if self.replace_references(&mut rw) {
                        others.push(rw);
                    }
                }
            }
        }
        let mut changes: Vec<(String, CuChange)> = Vec::new();
        for rw in others.into_iter().chain(std::iter::once(base)) {
            let name = rw.ast.uri.rsplit('/').next().unwrap_or("").to_owned();
            changes.push((name, CuChange::rewrite(rw)));
        }
        changes.sort_by(|a, b| crate::correction::compare_utf16(&a.0, &b.0));
        changes.into_iter().map(|(_, c)| c).collect()
    }

    /// `createNewRecord()`.
    fn create_new_record(&self, rw: &mut ASTRewrite) {
        let ast = self.ast.clone();
        let type_decl = ast.node(self.type_declaration);
        let record = rw.new_node(NodeKind::RecordDeclaration);
        if let Some(name) = type_decl.child("name") {
            let copy = rw.create_copy_target(name.id);
            rw.put_child(record, "name", copy);
        }
        if let Some(javadoc) = type_decl.child("javadoc") {
            let copy = rw.create_copy_target(javadoc.id);
            rw.put_child(record, "javadoc", copy);
        }
        let mut modifiers = Vec::new();
        for modifier in type_decl.list("modifiers") {
            let keep = !modifier.is(NodeKind::Modifier) || !matches!(modifier.simple("keyword"), Some("final") | Some("non-sealed"));
            if keep {
                modifiers.push(rw.create_copy_target(modifier.id));
            }
        }
        rw.put_list(record, "modifiers", modifiers);
        // Field name → its annotations (in declaration order).
        let mut field_annotations: Vec<(String, Vec<Node<'_>>)> = Vec::new();
        for field in type_decl.list("bodyDeclarations").into_iter().filter(|b| b.is(NodeKind::FieldDeclaration)) {
            for modifier in field.list("modifiers").into_iter().filter(|m| m.kind().is_annotation()) {
                for fragment in field.list("fragments") {
                    let name = fragment.child("name").map(|n| n.identifier()).unwrap_or_default();
                    let index = match field_annotations.iter().position(|(n, _)| *n == name) {
                        Some(i) => i,
                        None => {
                            field_annotations.push((name, Vec::new()));
                            field_annotations.len() - 1
                        }
                    };
                    if !field_annotations[index].1.iter().any(|a| a.id == modifier.id) {
                        field_annotations[index].1.push(modifier);
                    }
                }
            }
        }
        let type_parameters = type_decl.list("typeParameters").iter().map(|t| rw.create_copy_target(t.id)).collect();
        rw.put_list(record, "typeParameters", type_parameters);
        let components = self.create_components(rw, &mut field_annotations);
        rw.put_list(record, "recordComponents", components);
        let interfaces = type_decl.list("superInterfaceTypes").iter().map(|t| rw.create_copy_target(t.id)).collect();
        rw.put_list(record, "superInterfaceTypes", interfaces);
        let mut body = Vec::new();
        for id in self.initializers_to_copy.iter().chain(&self.fields_to_copy).chain(&self.methods_to_copy) {
            body.push(rw.create_copy_target(*id));
        }
        rw.put_list(record, "bodyDeclarations", body);
        // `ASTNodes.replaceButKeepComment`: the type's own range, without its comments.
        rw.set_source_range(type_decl.id, type_decl.start(), type_decl.length());
        rw.replace(RNode::Orig(type_decl.id), Some(record));
    }

    /// `createComponents(...)`.
    fn create_components(&self, rw: &mut ASTRewrite, field_annotations: &mut [(String, Vec<Node<'_>>)]) -> Vec<RNode> {
        let ast = self.ast.clone();
        let constructor = ast.node(self.constructor);
        let mut components = Vec::new();
        for parameter in constructor.list("parameters") {
            let name = parameter.child("name").map(|n| n.identifier()).unwrap_or_default();
            let mut annotation_set = field_annotations.iter_mut().find(|(n, _)| *n == name).map(|(_, set)| set);
            if let Some(set) = annotation_set.as_deref_mut() {
                for parameter_annotation in parameter.list("modifiers").into_iter().filter(|m| m.kind().is_annotation()) {
                    let type_name = annotation_type_name(parameter_annotation);
                    set.retain(|a| annotation_type_name(*a) != type_name);
                }
            }
            let svd = rw.new_node(NodeKind::SingleVariableDeclaration);
            let simple_name = rw.new_simple_name(&name);
            rw.put_child(svd, "name", simple_name);
            if let Some(t) = parameter.child("type") {
                let copy = rw.create_copy_target(t.id);
                rw.put_child(svd, "type", copy);
            }
            if parameter.flag("varargs") {
                rw.put_simple(svd, "varargs", "true");
            }
            let mut modifiers: Vec<RNode> = parameter.list("modifiers").iter().map(|m| rw.create_copy_target(m.id)).collect();
            if let Some(set) = annotation_set {
                for annotation in set.iter() {
                    modifiers.push(rw.create_copy_target(annotation.id));
                }
            }
            rw.put_list(svd, "modifiers", modifiers);
            let dimensions = parameter.list("extraDimensions2").iter().map(|d| rw.create_copy_target(d.id)).collect();
            rw.put_list(svd, "extraDimensions2", dimensions);
            let varargs_annotations = parameter.list("varargsAnnotations").iter().map(|a| rw.create_copy_target(a.id)).collect();
            rw.put_list(svd, "varargsAnnotations", varargs_annotations);
            components.push(svd);
        }
        components
    }

    /// `replaceReferences(group, cuRewrite)` for the getter invocations of
    /// one unit; whether anything was replaced.
    fn replace_references(&self, rw: &mut ASTRewrite) -> bool {
        let ast = rw.ast.clone();
        let mut changed = false;
        for n in ast.all_nodes().filter(|n| n.is(NodeKind::MethodInvocation)) {
            let Some(binding) = n.method_binding().or_else(|| n.binding()) else { continue };
            let key = method_key(binding);
            if !self.getters.iter().any(|g| g.method_key == key) {
                continue;
            }
            let Some(name) = n.child("name") else { continue };
            let identifier = name.identifier();
            let new_name = self.getters.iter().find(|g| g.method_name == identifier).map(|g| g.field_name.clone()).unwrap_or(identifier);
            let replacement = rw.new_simple_name(&new_name);
            rw.replace(RNode::Orig(name.id), Some(replacement));
            changed = true;
        }
        changed
    }
}

/// `Annotation.getTypeName().getFullyQualifiedName()`.
fn annotation_type_name(annotation: Node<'_>) -> String {
    annotation.child("typeName").map(|n| n.identifier()).unwrap_or_default()
}

/// `findGetter(declaringType, field)`.
fn find_getter<'a>(options: &std::collections::BTreeMap<String, String>, use_is: bool, declaring: BindingRef<'a>, field: BindingRef<'a>) -> Option<BindingRef<'a>> {
    let name = crate::correction::getter_setter::getter_name(options, use_is, field, is_boolean(field));
    let getter = crate::correction::unresolved_elements::find_method_in_hierarchy(declaring, &name, Some(&[]))?;
    let return_type = getter.return_type()?;
    let field_type = field.var_type()?;
    (super::checks::is_assignment_compatible(return_type, field_type) && getter.modifiers() & m::PUBLIC != 0).then_some(getter)
}

/// `findSetter(declaringType, field)`.
fn find_setter<'a>(options: &std::collections::BTreeMap<String, String>, use_is: bool, declaring: BindingRef<'a>, field: BindingRef<'a>) -> Option<BindingRef<'a>> {
    let name = crate::correction::getter_setter::setter_name(options, use_is, field, is_boolean(field));
    let field_type = field.var_type()?;
    crate::correction::unresolved_elements::find_method_in_hierarchy(declaring, &name, Some(&[field_type]))
}

/// The first `SUPERTYPE_TYPE_REFERENCE` match of the type in `unit`: the
/// name of the enclosing element (`IType.getFullyQualifiedName('.')` for a
/// type).
fn first_subclass(unit: &Ast, type_key: &str) -> Option<String> {
    let mut matches: HashMap<usize, String> = HashMap::new();
    for n in unit.all_nodes() {
        if !n.kind().is_type() || !matches!(n.location(), Some("superclassType") | Some("superInterfaceTypes")) {
            continue;
        }
        let Some(binding) = n.binding().or_else(|| n.type_binding()) else { continue };
        let declaration = binding.type_declaration().unwrap_or(binding);
        if declaration.key() != type_key && binding.key() != type_key {
            continue;
        }
        let Some(owner) = n.parent().and_then(|p| p.binding()) else { continue };
        let name = owner.qualified_name().to_owned();
        matches.entry(n.start()).or_insert(name);
    }
    let first = matches.keys().min().copied()?;
    matches.remove(&first)
}
