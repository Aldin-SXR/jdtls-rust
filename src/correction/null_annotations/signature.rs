//! `NullAnnotationsRewriteOperations` (`Builder` and the signature annotation
//! operations) over `NullAnnotationsFixCore.createNullAnnotationInSignatureFix`.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::{annotation_name, ChangeKind};
use crate::correction::edit::Env;
use crate::correction::modifier_corrections::{find_declaring_node, Units};
use crate::correction::{messages, CuChange, LazyChange, ProblemLocation};
use crate::rewrite::import_rewrite::nullness::default_locations;
use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{problem as p, Ast, BindingRef, Node, NodeId, NodeKind, PropValue};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Position {
    Return,
    Parameter(usize),
}

/// `SignatureAnnotationRewriteOperation` (return / parameter variants).
pub struct SignatureOperation {
    unit: Arc<Ast>,
    /// The method declaration or lambda expression.
    declaration: NodeId,
    position: Position,
    pub message: String,
    to_add: String,
    to_remove: String,
    allow_remove: bool,
    use_type_annotations: bool,
    require_explicit: bool,
    pub remove_if_non_null_default: bool,
    pub non_null_default_names: Option<HashSet<String>>,
}

pub(super) struct IndexedParameter {
    declaration: NodeId,
    index: usize,
    name: String,
}

/// `NullAnnotationsRewriteOperations.Builder`.
pub struct Builder<'a> {
    problem: &'a ProblemLocation,
    change_kind: ChangeKind,
    unit: Arc<Ast>,
    to_add: String,
    to_remove: String,
    allow_remove: bool,
    affects_parameter: bool,
    use_type_annotations: bool,
}

/// `ASTNodes.getParent(node, VariableDeclaration.class)`.
fn variable_declaration_parent(node: Node<'_>) -> Option<Node<'_>> {
    node.ancestors().find(|a| a.kind().is_variable_declaration())
}

fn is_list_property(parent: Node<'_>, location: &str) -> bool {
    matches!(parent.prop(location), Some(PropValue::List(_)))
}

/// `getDeclaringNode`: the enclosing method or lambda.
fn declaring_node(selected: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(selected);
    while let Some(n) = current {
        if matches!(n.kind(), NodeKind::MethodDeclaration | NodeKind::LambdaExpression) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

impl<'a> Builder<'a> {
    pub fn new(
        problem: &'a ProblemLocation,
        unit: Arc<Ast>,
        to_add: String,
        to_remove: String,
        allow_remove: bool,
        affects_parameter: bool,
        change_kind: ChangeKind,
    ) -> Self {
        let use_type_annotations = unit.nullable_type_use;
        Builder { problem, change_kind, unit, to_add, to_remove, allow_remove, affects_parameter, use_type_annotations }
    }

    pub fn swap_annotations(&mut self) {
        std::mem::swap(&mut self.to_add, &mut self.to_remove);
    }

    fn requires_explicit_annotation(&self) -> bool {
        match self.problem.problem_id {
            p::ConflictingInheritedNullAnnotations | p::ConflictingNullAnnotations => self.change_kind != ChangeKind::Overridden,
            _ => false,
        }
    }

    fn operation(&self, unit: Arc<Ast>, declaration: NodeId, position: Position, message: String) -> SignatureOperation {
        SignatureOperation {
            unit,
            declaration,
            position,
            message,
            to_add: self.to_add.clone(),
            to_remove: self.to_remove.clone(),
            allow_remove: self.allow_remove,
            use_type_annotations: self.use_type_annotations,
            require_explicit: self.requires_explicit_annotation(),
            remove_if_non_null_default: false,
            non_null_default_names: None,
        }
    }

    fn label(&self) -> String {
        self.to_add.rsplit('.').next().unwrap_or(&self.to_add).to_owned()
    }

    /// `createAddAnnotationOperation(handledPositions, thisUnitOnly, changeKind)`.
    pub async fn create_add_annotation_operation(&self, env: &Env<'_>) -> Option<SignatureOperation> {
        if self.change_kind == ChangeKind::Overridden {
            self.create_to_overridden(env).await
        } else {
            self.create_add(env, self.change_kind == ChangeKind::Target).await
        }
    }

    async fn create_add(&self, env: &Env<'_>, change_target_method: bool) -> Option<SignatureOperation> {
        let unit = self.unit.clone();
        let selected = self.problem.covering_node(&unit)?;
        let mut declaring = declaring_node(selected);
        match self.problem.problem_id {
            p::IllegalDefinitionToNonNullParameter => {}
            p::IllegalReturnNullityRedefinition => {
                if declaring.is_none() {
                    declaring = Some(selected);
                }
            }
            _ => {}
        }
        let label = self.label();
        if change_target_method {
            let invocation = if self.affects_parameter {
                selected.parent().filter(|n| n.is(NodeKind::MethodInvocation))
            } else {
                Some(selected).filter(|n| n.is(NodeKind::MethodInvocation))
            };
            let invocation = invocation?;
            let method_binding = invocation.method_binding()?;
            let (target, declaration) = self.find_method_declaration(env, method_binding).await?;
            let decl = target.node(declaration);
            if self.affects_parameter {
                let index = invocation.list("arguments").iter().position(|a| *a == selected)?;
                if method_binding.is_varargs() && index + 1 >= decl.list("parameters").len() {
                    return None;
                }
                let name = invocation.child("name").map(|n| n.identifier()).unwrap_or_default();
                let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_target_method_parameter_nullness"), &[&name, &label]);
                return Some(self.operation(target.clone(), declaration, Position::Parameter(index), message));
            }
            let name = decl.child("name").map(|n| n.identifier()).unwrap_or_default();
            let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_method_return_nullness"), &[&name, &label]);
            return Some(self.operation(target.clone(), declaration, Position::Return, message));
        }
        let declaring = declaring.filter(|d| matches!(d.kind(), NodeKind::MethodDeclaration | NodeKind::LambdaExpression));
        let declaring = declaring?;
        match self.problem.problem_id {
            p::ParameterLackingNonNullAnnotation
            | p::ParameterLackingNullableAnnotation
            | p::IllegalDefinitionToNonNullParameter
            | p::IllegalRedefinitionToNonNullParameter => {
                let parameter = find_parameter_declaration(selected)?;
                self.parameter_operation(&unit, parameter, &label)
            }
            p::NullityUncheckedTypeAnnotation
            | p::SpecdNonNullLocalVariableComparisonYieldsFalse
            | p::RedundantNullCheckOnSpecdNonNullLocalVariable
            | p::RequiredNonNullButProvidedNull
            | p::RequiredNonNullButProvidedPotentialNull
            | p::RequiredNonNullButProvidedSpecdNullable
            | p::RequiredNonNullButProvidedUnknown
            | p::ConflictingNullAnnotations
            | p::ConflictingInheritedNullAnnotations
            | p::RedundantNullCheckAgainstNonNullType => {
                if self.affects_parameter {
                    if selected.is(NodeKind::SimpleName) {
                        let parameter = find_referenced_parameter(selected)?;
                        return self.parameter_operation(&unit, parameter, &label);
                    }
                    return None;
                }
                self.return_operation(&unit, declaring, &label)
            }
            p::IllegalReturnNullityRedefinition => self.return_operation(&unit, declaring, &label),
            _ => None,
        }
    }

    fn return_operation(&self, unit: &Arc<Ast>, declaring: Node<'_>, label: &str) -> Option<SignatureOperation> {
        if !declaring.is(NodeKind::MethodDeclaration) {
            return None;
        }
        let name = declaring.child("name").map(|n| n.identifier()).unwrap_or_default();
        let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_method_return_nullness"), &[&name, label]);
        Some(self.operation(unit.clone(), declaring.id, Position::Return, message))
    }

    fn parameter_operation(&self, unit: &Arc<Ast>, parameter: IndexedParameter, label: &str) -> Option<SignatureOperation> {
        let declaration = unit.node(parameter.declaration);
        let parameters = declaration.list("parameters");
        // type elided lambda
        if !parameters.get(parameter.index)?.is(NodeKind::SingleVariableDeclaration) {
            return None;
        }
        let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_method_parameter_nullness"), &[&parameter.name, label]);
        Some(self.operation(unit.clone(), parameter.declaration, Position::Parameter(parameter.index), message))
    }

    async fn create_to_overridden(&self, env: &Env<'_>) -> Option<SignatureOperation> {
        let unit = self.unit.clone();
        let selected = self.problem.covering_node(&unit)?;
        let mut declaring = declaring_node(selected);
        match self.problem.problem_id {
            p::IllegalDefinitionToNonNullParameter | p::IllegalRedefinitionToNonNullParameter => {}
            p::IllegalReturnNullityRedefinition | p::ConflictingNullAnnotations => {
                if declaring.is_none() {
                    declaring = Some(selected);
                }
            }
            _ => return None,
        }
        let label = self.label();
        let declaration = declaring.filter(|d| d.is(NodeKind::MethodDeclaration))?;
        match self.problem.problem_id {
            p::IllegalReturnNullityRedefinition | p::IllegalDefinitionToNonNullParameter | p::IllegalRedefinitionToNonNullParameter | p::ConflictingNullAnnotations => {
                if self.problem.problem_id == p::IllegalReturnNullityRedefinition && !self.has_null_annotation(declaration) {
                    return None;
                }
                let overridden = crate::correction::javadoc_tags::find_overridden_method(declaration.binding()?, false)?;
                let (target, overridden_declaration) = self.find_method_declaration(env, overridden).await?;
                if self.affects_parameter {
                    let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_overridden_parameter_nullness"), &[overridden.name(), &label]);
                    let parameter = find_parameter_declaration(selected)?;
                    Some(self.operation(target, overridden_declaration, Position::Parameter(parameter.index), message))
                } else {
                    let message = messages::format(messages::fix("NullAnnotationsRewriteOperations_change_overridden_return_nullness"), &[overridden.name(), &label]);
                    Some(self.operation(target, overridden_declaration, Position::Return, message))
                }
            }
            _ => None,
        }
    }

    /// `hasNullAnnotation(decl)`.
    fn has_null_annotation(&self, declaration: Node<'_>) -> bool {
        let (nonnull, nullable) = (&self.to_remove, &self.to_add);
        // The builder's annotations are swapped for some problems; the
        // original pair is the unordered pair.
        declaration.list("modifiers").into_iter().filter(|m| m.kind().is_annotation()).any(|annotation| {
            let Some(name) = annotation.child("typeName") else { return false };
            let full = name.identifier();
            let simple = name.is(NodeKind::SimpleName);
            [nonnull, nullable].into_iter().any(|candidate| if simple { candidate.ends_with(&full) } else { full == *candidate })
        })
    }

    /// `findMethodDeclarationInUnit(cu, method, false)`.
    async fn find_method_declaration(&self, env: &Env<'_>, method: BindingRef<'_>) -> Option<(Arc<Ast>, NodeId)> {
        let declaration = method.method_declaration().unwrap_or(method);
        let key = declaration.key().to_owned();
        if let Some(node) = find_declaring_node(&self.unit, &key) {
            return Some((self.unit.clone(), node.id));
        }
        let declaring = declaration.declaring_class()?;
        let declaring = declaring.type_declaration().unwrap_or(declaring);
        if !declaring.is_from_source() {
            return None;
        }
        let units: Units = Units::load(env, &self.unit.uri).await;
        let uri = units.find(&self.unit, declaring)??;
        let target = crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&uri).ok()?).await.ok()?;
        let node = find_declaring_node(&target, &key)?.id;
        Some((target, node))
    }
}

/// `findParameterDeclaration`.
pub(super) fn find_parameter_declaration(selected: Node<'_>) -> Option<IndexedParameter> {
    let argument = if selected.kind().is_variable_declaration() { selected } else { variable_declaration_parent(selected)? };
    let location = argument.location()?;
    let declaration = argument.parent()?;
    if !is_list_property(declaration, location) {
        return None;
    }
    if !matches!(declaration.kind(), NodeKind::MethodDeclaration | NodeKind::LambdaExpression) {
        return None;
    }
    let index = declaration.list(location).iter().position(|p| *p == argument)?;
    Some(IndexedParameter { declaration: declaration.id, index, name: argument.child("name").map(|n| n.identifier()).unwrap_or_default() })
}

/// `findReferencedParameter`.
fn find_referenced_parameter(selected: Node<'_>) -> Option<IndexedParameter> {
    let binding = selected.binding().filter(|b| b.is_variable() && b.is_parameter())?;
    let mut current = selected.parent();
    while let Some(node) = current {
        if matches!(node.kind(), NodeKind::MethodDeclaration | NodeKind::LambdaExpression) {
            for (i, parameter) in node.list("parameters").into_iter().enumerate() {
                if parameter.binding().is_some_and(|b| b.key() == binding.key()) {
                    return Some(IndexedParameter { declaration: node.id, index: i, name: binding.name().to_owned() });
                }
            }
        }
        current = node.parent();
    }
    None
}

impl SignatureOperation {
    pub fn unit(&self) -> &Arc<Ast> {
        &self.unit
    }

    /// `checkExisting(listRewrite, editGroup)`.
    fn check_existing(&self, rw: &mut ASTRewrite, parent: RNode, prop: &'static str) -> bool {
        let originals = rw.original_value(parent, prop).list();
        for modifier in originals {
            let RNode::Orig(id) = modifier else { continue };
            let node = self.unit.node(id);
            if !node.is(NodeKind::MarkerAnnotation) {
                continue;
            }
            let existing = node.child("typeName").map(|n| n.identifier()).unwrap_or_default();
            let remove_dot = self.to_remove.rfind('.');
            if existing == self.to_remove || remove_dot.is_some_and(|d| self.to_remove[d + 1..] == existing) {
                if !self.allow_remove {
                    return false;
                }
                rw.list_remove(parent, prop, modifier);
                return true;
            }
            let add_dot = self.to_add.rfind('.');
            if existing == self.to_add || add_dot.is_some_and(|d| self.to_add[d + 1..] == existing) {
                return false;
            }
        }
        true
    }

    /// `hasNonNullDefault`.
    fn has_non_null_default(&self, node: Node<'_>, enclosing: Option<BindingRef<'_>>, parameter_rank: usize, location: TypeLocation) -> bool {
        let Some(names) = self.non_null_default_names.as_ref().filter(|_| self.remove_if_non_null_default) else { return false };
        let affected = match (enclosing, location) {
            (Some(b), TypeLocation::ReturnType) if b.is_method() => b.return_type(),
            (Some(b), TypeLocation::Parameter) if b.is_method() => b.parameter_types().get(parameter_rank).copied(),
            (Some(b), _) if b.is_variable() => b.var_type(),
            _ => None,
        };
        if affected.is_some_and(|t| t.is_type_variable() || t.is_wildcard_type()) {
            return false;
        }
        default_locations(&self.unit, node.id, names).contains(&location)
    }

    fn marker_annotation(&self, rw: &mut ASTRewrite, imports: &mut ImportRewrite) -> RNode {
        let resolvable = imports.add_import(&self.to_add, &DefaultContext);
        let annotation = rw.new_node(NodeKind::MarkerAnnotation);
        let name = rw.new_name(&resolvable);
        rw.put_child(annotation, "typeName", name)
    }

    /// `getAnnotationListRewrite`: the list the annotation is inserted into.
    fn annotation_list(&self, rw: &ASTRewrite, typ: Option<Node<'_>>, declaration: Node<'_>) -> (RNode, &'static str) {
        if self.use_type_annotations {
            if let Some(array) = typ.filter(|t| t.is(NodeKind::ArrayType)) {
                if let Some(outer) = array.list("dimensions").first() {
                    return (RNode::Orig(outer.id), "annotations");
                }
            }
        }
        let _ = rw;
        (RNode::Orig(declaration.id), "modifiers")
    }

    fn rewrite_return(&self, rw: &mut ASTRewrite, imports: &mut ImportRewrite) {
        let declaration = self.unit.node(self.declaration);
        let (parent, prop) = self.annotation_list(rw, declaration.child("returnType2"), declaration);
        if !self.check_existing(rw, parent, prop) {
            return;
        }
        if !self.require_explicit && self.has_non_null_default(declaration, declaration.binding(), 0, TypeLocation::ReturnType) {
            return;
        }
        let annotation = self.marker_annotation(rw, imports);
        rw.list_insert_last(parent, prop, annotation);
    }

    fn rewrite_parameter(&self, rw: &mut ASTRewrite, imports: &mut ImportRewrite, index: usize) {
        let declaration = self.unit.node(self.declaration);
        let Some(argument) = declaration.list("parameters").get(index).copied() else { return };
        let Some(arg_type) = argument.child("type") else { return };
        if arg_type.is(NodeKind::SimpleType) && arg_type.child("name").is_some_and(|n| n.is(NodeKind::QualifiedName)) {
            let qualified = arg_type.child("name").unwrap();
            let qualifier = rw.create_move_target(qualified.child("qualifier").unwrap().id);
            let name = rw.create_move_target(qualified.child("name").unwrap().id);
            let annotation = self.marker_annotation(rw, imports);
            let qualified_type = rw.new_node(NodeKind::NameQualifiedType);
            rw.put_child(qualified_type, "qualifier", qualifier);
            rw.put_list(qualified_type, "annotations", vec![annotation]);
            rw.put_child(qualified_type, "name", name);
            rw.replace(RNode::Orig(arg_type.id), Some(qualified_type));
        } else if arg_type.is(NodeKind::NameQualifiedType) {
            let annotation = self.marker_annotation(rw, imports);
            rw.list_insert_last(RNode::Orig(arg_type.id), "annotations", annotation);
        } else {
            let (parent, prop) = self.annotation_list(rw, Some(arg_type), argument);
            if !self.check_existing(rw, parent, prop) {
                return;
            }
            if !self.require_explicit && self.has_non_null_default(argument, argument.binding(), index, TypeLocation::Parameter) {
                return;
            }
            let annotation = self.marker_annotation(rw, imports);
            rw.list_insert_last(parent, prop, annotation);
        }
    }
}

#[tower_lsp::async_trait]
impl LazyChange for SignatureOperation {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let options: BTreeMap<String, String> = env.options(&self.unit.uri).await;
        let mut rw = ASTRewrite::new(self.unit.clone());
        let mut imports = ImportRewrite::create_for_corrections(self.unit.clone(), &options);
        match self.position {
            Position::Return => self.rewrite_return(&mut rw, &mut imports),
            Position::Parameter(index) => self.rewrite_parameter(&mut rw, &mut imports, index),
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

pub(super) fn qualified_names(options: &BTreeMap<String, String>) -> Option<(String, String)> {
    Some((annotation_name(options, "nullable")?, annotation_name(options, "nonnull")?))
}
