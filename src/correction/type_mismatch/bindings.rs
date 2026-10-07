//! The `Bindings` / `ASTResolving` / `BindingLabelProviderCore` helpers the
//! type mismatch corrections use, over the semantic AST's binding graph.

use std::collections::HashSet;

use crate::semantic_ast::{bflag, modifier, Ast, BindingKind, BindingRef, Node, NodeKind};

/// `AST.resolveWellKnownType(name)` (primitives and `java.lang` types the
/// bridge exported).
pub fn well_known<'a>(ast: &'a Ast, name: &str) -> Option<BindingRef<'a>> {
    ast.type_by_name(name)
}

/// `Bindings.isVoidType`.
pub fn is_void(b: BindingRef<'_>) -> bool {
    b.name() == "void"
}

/// `Bindings.normalizeTypeBinding`.
pub fn normalize_type_binding(b: Option<BindingRef<'_>>) -> Option<BindingRef<'_>> {
    let b = b?;
    if b.is_null_type() || is_void(b) {
        return None;
    }
    if b.is_anonymous() {
        return b.interfaces().first().copied().or_else(|| b.superclass());
    }
    if b.is_capture() {
        return b.wildcard();
    }
    Some(b)
}

/// `ITypeBinding.isUpperbound()`.
fn is_upperbound(b: BindingRef<'_>) -> bool {
    b.has(bflag::UPPERBOUND)
}

/// `ASTResolving.normalizeWildcardType`.
pub fn normalize_wildcard_type(wildcard: BindingRef<'_>, is_binding_to_assign: bool) -> Option<BindingRef<'_>> {
    let bound = wildcard.bound();
    if is_binding_to_assign {
        if bound.is_none() || !is_upperbound(wildcard) {
            return wildcard.type_bounds().first().copied().or_else(|| wildcard.erasure());
        }
    } else if bound.is_none() || is_upperbound(wildcard) {
        return None;
    }
    bound
}

/// `Bindings.normalizeForDeclarationUse`.
pub fn normalize_for_declaration_use<'a>(b: BindingRef<'a>) -> Option<BindingRef<'a>> {
    if b.is_null_type() {
        return well_known(b.ast, "java.lang.Object");
    }
    if b.is_primitive() {
        return Some(b);
    }
    let b = normalize_type_binding(Some(b))?;
    if b.is_array() {
        let component = b.component_type()?;
        let normalized = normalize_for_declaration_use(component)?;
        if normalized == component {
            return Some(b);
        }
        // `createArrayType(1)`: only when the graph knows that array type.
        return b.ast.binding_by_key(&format!("[{}", normalized.key())).or(Some(b));
    }
    if !b.is_wildcard_type() {
        return Some(b);
    }
    match b.bound() {
        Some(bound) if is_upperbound(b) => Some(bound),
        _ => b.type_bounds().first().copied().or_else(|| b.erasure()),
    }
}

/// `Bindings.isSuperType(possibleSuperType, type, considerTypeArguments)`.
pub fn is_super_type(possible: BindingRef<'_>, typ: BindingRef<'_>, consider_type_arguments: bool) -> bool {
    if typ.is_array() || typ.is_primitive() {
        return false;
    }
    let typ = if consider_type_arguments { typ } else { typ.type_declaration().unwrap_or(typ) };
    if typ == possible {
        return true;
    }
    if let Some(superclass) = typ.superclass() {
        if is_super_type(possible, superclass, consider_type_arguments) {
            return true;
        }
    }
    if possible.is_interface() {
        for interface in typ.interfaces() {
            if is_super_type(possible, interface, consider_type_arguments) {
                return true;
            }
        }
    }
    false
}

/// `Bindings.findTypeInHierarchy(type, qualifiedName)`.
pub fn find_type_in_hierarchy<'a>(typ: BindingRef<'a>, qualified: &str) -> Option<BindingRef<'a>> {
    fn find<'a>(typ: BindingRef<'a>, qualified: &str, seen: &mut HashSet<String>) -> Option<BindingRef<'a>> {
        if !seen.insert(typ.key().to_owned()) {
            return None;
        }
        if typ.is_array() || typ.is_primitive() {
            return None;
        }
        if typ.type_declaration().unwrap_or(typ).qualified_name() == qualified {
            return Some(typ);
        }
        typ.superclass()
            .and_then(|s| find(s, qualified, seen))
            .or_else(|| typ.interfaces().into_iter().find_map(|i| find(i, qualified, seen)))
    }
    find(typ, qualified, &mut HashSet::new())
}

/// `Bindings.findMethodInHierarchy(type, name, parameters)` for a method
/// without parameters (the declared members the graph exports).
pub fn find_method_in_hierarchy<'a>(typ: BindingRef<'a>, name: &str) -> Option<BindingRef<'a>> {
    fn find<'a>(typ: BindingRef<'a>, name: &str, seen: &mut HashSet<String>) -> Option<BindingRef<'a>> {
        if !seen.insert(typ.key().to_owned()) {
            return None;
        }
        typ.declared_methods()
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.name() == name && m.parameter_types().is_empty())
            .or_else(|| typ.superclass().and_then(|s| find(s, name, seen)))
            .or_else(|| typ.interfaces().into_iter().find_map(|i| find(i, name, seen)))
    }
    find(typ, name, &mut HashSet::new())
}

/// `Bindings.findMethodInType(type, name, null)`.
pub fn find_method_in_type<'a>(typ: BindingRef<'a>, name: &str) -> Option<BindingRef<'a>> {
    let typ = if typ.declared_methods().is_some() { typ } else { typ.type_declaration().unwrap_or(typ) };
    typ.declared_methods().unwrap_or_default().into_iter().find(|m| m.name() == name)
}

/// `Bindings.isSubsignature(method, candidate)` (compiler relation exported
/// by the bridge).
fn is_subsignature(method: BindingRef<'_>, candidate: BindingRef<'_>) -> bool {
    method.data().method_subsignatures.iter().any(|&id| method.ast.binding(id) == candidate)
}

/// `Bindings.findOverriddenMethodInHierarchy`.
fn find_overridden_in_hierarchy<'a>(typ: BindingRef<'a>, method: BindingRef<'a>, seen: &mut HashSet<String>) -> Option<BindingRef<'a>> {
    if !seen.insert(typ.key().to_owned()) {
        return None;
    }
    let declaration = if typ.declared_methods().is_some() { typ } else { typ.type_declaration().unwrap_or(typ) };
    if let Some(found) = declaration
        .declared_methods()
        .unwrap_or_default()
        .into_iter()
        .chain(typ.declared_methods().unwrap_or_default())
        .find(|m| is_subsignature(method, *m))
    {
        return Some(found);
    }
    if let Some(found) = typ.superclass().and_then(|s| find_overridden_in_hierarchy(s, method, seen)) {
        return Some(found);
    }
    typ.interfaces().into_iter().find_map(|i| find_overridden_in_hierarchy(i, method, seen))
}

/// `Bindings.findOverriddenMethod(overriding, false)`.
pub fn find_overridden_method<'a>(overriding: BindingRef<'a>) -> Option<BindingRef<'a>> {
    let modifiers = overriding.modifiers();
    if modifiers & (modifier::PRIVATE | modifier::STATIC) != 0 || overriding.is_constructor() {
        return None;
    }
    let typ = overriding.declaring_class()?;
    if let Some(superclass) = typ.superclass() {
        if let Some(res) = find_overridden_in_hierarchy(superclass, overriding, &mut HashSet::new()) {
            if res.modifiers() & modifier::PRIVATE == 0 {
                return Some(res);
            }
        }
    }
    typ.interfaces().into_iter().find_map(|i| find_overridden_in_hierarchy(i, overriding, &mut HashSet::new()))
}

/// `Bindings.getBoxedTypeName`.
pub fn boxed_type_name(primitive: &str) -> Option<&'static str> {
    Some(match primitive {
        "long" => "java.lang.Long",
        "int" => "java.lang.Integer",
        "short" => "java.lang.Short",
        "char" => "java.lang.Character",
        "byte" => "java.lang.Byte",
        "boolean" => "java.lang.Boolean",
        "float" => "java.lang.Float",
        "double" => "java.lang.Double",
        _ => return None,
    })
}

/// `Bindings.getUnboxedTypeName`.
pub fn unboxed_type_name(boxed: &str) -> Option<&'static str> {
    Some(match boxed {
        "java.lang.Long" => "long",
        "java.lang.Integer" => "int",
        "java.lang.Short" => "short",
        "java.lang.Character" => "char",
        "java.lang.Byte" => "byte",
        "java.lang.Boolean" => "boolean",
        "java.lang.Float" => "float",
        "java.lang.Double" => "double",
        _ => return None,
    })
}

/// A type for compatibility checks: a binding of the graph, or a well-known
/// boxed / unboxed type identified by name.
#[derive(Clone, Copy)]
pub enum Ty<'a> {
    Binding(BindingRef<'a>),
    Named(&'static str),
}

impl<'a> Ty<'a> {
    fn qualified(&self) -> String {
        match self {
            Ty::Binding(b) => erased(*b).qualified_name().to_owned(),
            Ty::Named(n) => (*n).to_owned(),
        }
    }
    fn is_primitive(&self) -> bool {
        match self {
            Ty::Binding(b) => b.is_primitive(),
            Ty::Named(n) => !n.contains('.'),
        }
    }
}

/// `TypeMismatchBaseSubProcessor.boxOrUnboxPrimitives`.
pub fn box_or_unbox<'a>(cast_type: BindingRef<'a>, to_cast: BindingRef<'a>) -> Ty<'a> {
    if cast_type.is_primitive() && !to_cast.is_primitive() {
        if let Some(name) = boxed_type_name(cast_type.name()) {
            return well_known(cast_type.ast, name).map(Ty::Binding).unwrap_or(Ty::Named(name));
        }
    } else if !cast_type.is_primitive() && to_cast.is_primitive() && cast_type.is_class() {
        if let Some(name) = unboxed_type_name(cast_type.qualified_name()) {
            return well_known(cast_type.ast, name).map(Ty::Binding).unwrap_or(Ty::Named(name));
        }
    }
    Ty::Binding(cast_type)
}

/// The erasure used for reference cast checks (type variables and captures
/// stand for their first bound).
fn erased(b: BindingRef<'_>) -> BindingRef<'_> {
    let mut b = b;
    let mut guard = 0;
    while (b.is_type_variable() || b.is_capture() || b.is_wildcard_type()) && guard < 8 {
        guard += 1;
        match b.erasure().filter(|e| *e != b) {
            Some(e) => b = e,
            None => break,
        }
    }
    b.type_declaration().filter(|_| b.is_parameterized_type() || b.is_raw_type()).unwrap_or(b)
}

/// Erasure subtype test (`type` is `sub` or inherits from `sup`).
fn is_subtype_erasure(sub: BindingRef<'_>, sup_qualified: &str) -> bool {
    if sup_qualified == "java.lang.Object" {
        return true;
    }
    fn walk(t: BindingRef<'_>, target: &str, seen: &mut HashSet<String>) -> bool {
        let t = erased(t);
        if !seen.insert(t.key().to_owned()) {
            return false;
        }
        if t.qualified_name() == target {
            return true;
        }
        t.superclass().is_some_and(|s| walk(s, target, seen)) || t.interfaces().into_iter().any(|i| walk(i, target, seen))
    }
    walk(sub, sup_qualified, &mut HashSet::new())
}

fn is_numeric(name: &str) -> bool {
    matches!(name, "byte" | "short" | "char" | "int" | "long" | "float" | "double")
}

fn widens(from: &str, to: &str) -> bool {
    if from == to {
        return true;
    }
    let order = |n: &str| match n {
        "byte" => 1,
        "short" => 2,
        "char" => 2,
        "int" => 3,
        "long" => 4,
        "float" => 5,
        "double" => 6,
        _ => 0,
    };
    from != "char" && to != "char" && order(from) > 0 && order(from) < order(to) || from == "char" && order(to) >= 3
}

fn is_final(b: BindingRef<'_>) -> bool {
    let b = erased(b);
    b.modifiers() & modifier::FINAL != 0 || b.is_enum() || b.is_record()
}

const ARRAY_SUPERTYPES: [&str; 3] = ["java.lang.Object", "java.lang.Cloneable", "java.io.Serializable"];

/// `castType.isCastCompatible(expressionType)` (JLS 5.5): whether an
/// expression of `expression` type may be cast to `cast`.
pub fn is_cast_compatible(cast: Ty<'_>, expression: BindingRef<'_>) -> bool {
    if is_void(expression) {
        return false;
    }
    if let Ty::Binding(b) = cast {
        if is_void(b) || b.is_null_type() {
            return false;
        }
    }
    if expression.is_null_type() {
        return !cast.is_primitive();
    }
    match (cast.is_primitive(), expression.is_primitive()) {
        (true, true) => {
            let (c, e) = (cast.qualified(), expression.name().to_owned());
            c == e || is_numeric(&c) && is_numeric(&e)
        }
        (true, false) => {
            let c = cast.qualified();
            let e = erased(expression);
            if let Some(unboxed) = unboxed_type_name(e.qualified_name()) {
                return widens(unboxed, &c);
            }
            // `(int) object`: the boxed cast type is a subtype of the expression type.
            let Some(boxed) = boxed_type_name(&c) else { return false };
            match well_known(expression.ast, boxed) {
                Some(b) => is_subtype_erasure(b, e.qualified_name()),
                None => matches!(e.qualified_name(), "java.lang.Object" | "java.io.Serializable" | "java.lang.Comparable")
                    || e.qualified_name() == "java.lang.Number" && !matches!(c.as_str(), "boolean" | "char"),
            }
        }
        (false, true) => {
            let Some(boxed) = boxed_type_name(expression.name()) else { return false };
            let c = cast.qualified();
            if c == boxed {
                return true;
            }
            match well_known(expression.ast, boxed) {
                Some(b) => is_subtype_erasure(b, &c),
                None => matches!(c.as_str(), "java.lang.Object" | "java.io.Serializable" | "java.lang.Comparable"),
            }
        }
        (false, false) => {
            let Ty::Binding(cast) = cast else {
                let c = cast.qualified();
                return is_subtype_erasure(expression, &c);
            };
            reference_cast_compatible(cast, expression)
        }
    }
}

fn reference_cast_compatible(cast: BindingRef<'_>, expression: BindingRef<'_>) -> bool {
    if expression.is_array() || cast.is_array() {
        if expression.is_array() && cast.is_array() {
            let (Some(c), Some(e)) = (cast.component_type(), expression.component_type()) else { return false };
            if c.is_primitive() || e.is_primitive() {
                return c.name() == e.name();
            }
            return reference_cast_compatible(c, e);
        }
        let other = if expression.is_array() { cast } else { expression };
        let other = erased(other);
        if other.is_type_variable() {
            return true;
        }
        return ARRAY_SUPERTYPES.contains(&other.qualified_name());
    }
    let (c, e) = (erased(cast), erased(expression));
    if c.is_type_variable() || e.is_type_variable() {
        return true;
    }
    let (ci, ei) = (c.is_interface(), e.is_interface());
    match (ci, ei) {
        (false, false) => is_subtype_erasure(e, c.qualified_name()) || is_subtype_erasure(c, e.qualified_name()),
        (true, false) => !is_final(e) || is_subtype_erasure(e, c.qualified_name()),
        (false, true) => !is_final(c) || is_subtype_erasure(c, e.qualified_name()),
        (true, true) => true,
    }
}

/// `ASTResolving.isUseableTypeInContext`.
pub fn is_useable_type_in_context(typ: BindingRef<'_>, context: Option<BindingRef<'_>>, no_wildcards: bool) -> bool {
    let mut typ = typ;
    if typ.is_array() {
        typ = typ.element_type().unwrap_or(typ);
    }
    if typ.is_anonymous() {
        return false;
    }
    if typ.is_raw_type() || typ.is_primitive() {
        return true;
    }
    if typ.is_type_variable() {
        return context.is_some_and(|c| is_variable_defined_in_context(c, typ));
    }
    if typ.is_generic_type() {
        return typ.type_parameters().into_iter().all(|t| is_useable_type_in_context(t, context, no_wildcards));
    }
    if typ.is_parameterized_type() {
        return typ.type_arguments().into_iter().all(|t| is_useable_type_in_context(t, context, no_wildcards));
    }
    if typ.is_capture() {
        typ = typ.wildcard().unwrap_or(typ);
    }
    if typ.is_wildcard_type() {
        if no_wildcards {
            return false;
        }
        if let Some(bound) = typ.bound() {
            return is_useable_type_in_context(bound, context, no_wildcards);
        }
    }
    true
}

fn is_variable_defined_in_context(binding: BindingRef<'_>, type_variable: BindingRef<'_>) -> bool {
    let mut binding = Some(binding);
    if let Some(var) = binding.filter(|b| b.kind() == BindingKind::Variable) {
        binding = var.declaring_method().or_else(|| var.declaring_class());
    }
    if let Some(method) = binding.filter(|b| b.kind() == BindingKind::Method) {
        if type_variable.declaring_method().is_some_and(|m| m == method) {
            return true;
        }
        binding = method.declaring_class();
    }
    while let Some(typ) = binding.filter(|b| b.kind() == BindingKind::Type) {
        if type_variable.declaring_class().is_some_and(|c| c == typ) {
            return true;
        }
        if typ.modifiers() & modifier::STATIC != 0 {
            break;
        }
        binding = typ.declaring_class();
    }
    false
}

/// `Bindings.resolveExpressionBinding(expression, goIntoCast)`.
pub fn resolve_expression_binding(expression: Node<'_>, go_into_cast: bool) -> Option<BindingRef<'_>> {
    match expression.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName => expression.binding(),
        NodeKind::FieldAccess | NodeKind::SuperFieldAccess => expression.binding(),
        NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation | NodeKind::ClassInstanceCreation => expression.method_binding(),
        NodeKind::ArrayAccess => resolve_expression_binding(expression.child("array")?, go_into_cast),
        NodeKind::CastExpression if go_into_cast => resolve_expression_binding(expression.child("expression")?, true),
        NodeKind::ParenthesizedExpression => resolve_expression_binding(expression.child("expression")?, go_into_cast),
        NodeKind::PrefixExpression | NodeKind::PostfixExpression => resolve_expression_binding(expression.child("operand")?, go_into_cast),
        _ => None,
    }
}

/// `ASTResolving.findParentMethodDeclaration`.
pub fn find_parent_method_declaration(node: Node<'_>) -> Option<Node<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.is(NodeKind::MethodDeclaration) {
            return Some(x);
        }
        if x.kind().is_body_declaration() || x.is(NodeKind::AnonymousClassDeclaration) || x.is(NodeKind::LambdaExpression) {
            return None;
        }
        n = x.parent();
    }
    None
}

/// `ASTResolving.findAnnotationMember(annotation, name)`.
fn find_annotation_member<'a>(annotation: Node<'a>, name: &str) -> Option<BindingRef<'a>> {
    let typ = annotation.type_binding().or_else(|| annotation.binding())?;
    find_method_in_type(typ, name)
}

/// `ASTResolving.getParameterTypeBinding(node, args, binding)`.
fn parameter_type_binding<'a>(node: Node<'a>, arguments: &[Node<'a>], method: BindingRef<'a>) -> Option<BindingRef<'a>> {
    let index = arguments.iter().position(|a| *a == node)?;
    let types = method.parameter_types();
    if index < types.len() {
        let typ = types[index];
        if method.is_varargs() && index == types.len() - 1 {
            // A single argument for the varargs parameter is its element.
            return typ.component_type().or(Some(typ));
        }
        return Some(typ);
    }
    if method.is_varargs() {
        return types.last().and_then(|t| t.component_type());
    }
    None
}

/// `ASTResolving.guessBindingForReference(node)`.
pub fn guess_binding_for_reference(node: Node<'_>) -> Option<BindingRef<'_>> {
    normalize_type_binding(possible_reference_binding(node))
}

fn possible_reference_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    let parent = node.parent()?;
    let ast = node.ast;
    match parent.kind() {
        NodeKind::Assignment => {
            if parent.child("leftHandSide") == Some(node) {
                parent.child("rightHandSide")?.type_binding()
            } else {
                parent.child("leftHandSide")?.type_binding()
            }
        }
        NodeKind::InfixExpression => {
            let op = parent.simple("operator").unwrap_or("");
            if op == "&&" || op == "||" {
                return well_known(ast, "boolean");
            } else if op == "<<" || op == ">>>" || op == ">>" {
                return well_known(ast, "int");
            }
            let other = if parent.child("leftOperand") == Some(node) { parent.child("rightOperand") } else { parent.child("leftOperand") };
            if let Some(b) = other.and_then(|o| o.type_binding()) {
                return Some(b);
            }
            if op != "==" && op != "!=" {
                return well_known(ast, "int");
            }
            None
        }
        NodeKind::InstanceofExpression => parent.child("rightOperand")?.binding(),
        NodeKind::VariableDeclarationFragment => {
            if parent.child("initializer") == Some(node) {
                parent.child("name")?.type_binding()
            } else {
                None
            }
        }
        NodeKind::SuperMethodInvocation | NodeKind::MethodInvocation | NodeKind::SuperConstructorInvocation | NodeKind::ConstructorInvocation | NodeKind::ClassInstanceCreation => {
            let method = parent.method_binding()?;
            parameter_type_binding(node, &parent.list("arguments"), method)
        }
        NodeKind::ParenthesizedExpression => guess_binding_for_reference(parent),
        NodeKind::ArrayAccess => {
            if parent.child("index") == Some(node) {
                well_known(ast, "int")
            } else {
                None
            }
        }
        NodeKind::ArrayCreation => {
            if parent.list("dimensions").contains(&node) {
                well_known(ast, "int")
            } else {
                None
            }
        }
        NodeKind::ConditionalExpression => {
            if node.location_is("expression") {
                return well_known(ast, "boolean");
            }
            let parent_type = possible_reference_binding(parent);
            if let Some(t) = parent_type.filter(|t| !t.is_null_type()) {
                return Some(t);
            }
            if node.location_is("thenExpression") {
                if let Some(t) = parent.child("elseExpression").and_then(|e| e.type_binding()).filter(|t| !t.is_null_type()) {
                    return Some(t);
                }
            }
            if node.location_is("elseExpression") {
                if let Some(t) = parent.child("thenExpression").and_then(|e| e.type_binding()).filter(|t| !t.is_null_type()) {
                    return Some(t);
                }
            }
            possible_reference_binding(parent)
        }
        NodeKind::PostfixExpression => well_known(ast, "int"),
        NodeKind::PrefixExpression => {
            if parent.simple("operator") == Some("!") {
                well_known(ast, "boolean")
            } else {
                well_known(ast, "int")
            }
        }
        NodeKind::IfStatement | NodeKind::WhileStatement | NodeKind::DoStatement => {
            if node.kind().is_expression() {
                well_known(ast, "boolean")
            } else {
                None
            }
        }
        NodeKind::SwitchStatement => {
            if parent.child("expression") == Some(node) {
                well_known(ast, "int")
            } else {
                None
            }
        }
        NodeKind::ReturnStatement => {
            if let Some(decl) = find_parent_method_declaration(parent) {
                if !decl.flag("constructor") {
                    return decl.child("returnType2")?.binding();
                }
            }
            let lambda = parent.ancestors().find(|a| a.is(NodeKind::LambdaExpression))?;
            lambda.method_binding()?.return_type()
        }
        NodeKind::CastExpression => parent.child("type")?.binding(),
        NodeKind::ThrowStatement | NodeKind::CatchClause => well_known(ast, "java.lang.Exception"),
        NodeKind::FieldAccess => {
            if parent.child("name") == Some(node) {
                possible_reference_binding(parent)
            } else {
                None
            }
        }
        NodeKind::SuperFieldAccess => possible_reference_binding(parent),
        NodeKind::QualifiedName => {
            if parent.child("name") == Some(node) {
                possible_reference_binding(parent)
            } else {
                None
            }
        }
        NodeKind::AssertStatement => {
            if node.location_is("expression") {
                well_known(ast, "boolean")
            } else {
                well_known(ast, "java.lang.String")
            }
        }
        NodeKind::SingleMemberAnnotation => find_annotation_member(parent, "value")?.return_type(),
        NodeKind::MemberValuePair => {
            let name = parent.child("name")?.identifier();
            find_annotation_member(parent.parent()?, &name)?.return_type()
        }
        _ => None,
    }
}

/// `BindingLabelProviderCore.getBindingLabel(type, ALL_DEFAULT)` for types
/// (`T_TYPE_PARAMETERS`: simple names with type arguments / parameters).
pub fn type_label(b: BindingRef<'_>) -> String {
    let mut out = String::new();
    append_type_label(b, true, &mut out);
    out
}

fn append_type_label(b: BindingRef<'_>, type_parameters: bool, out: &mut String) {
    if b.is_capture() {
        if let Some(w) = b.wildcard() {
            append_type_label(w, type_parameters, out);
        }
    } else if b.is_wildcard_type() {
        out.push('?');
        if let Some(bound) = b.bound() {
            out.push_str(if is_upperbound(b) { " extends " } else { " super " });
            append_type_label(bound, type_parameters, out);
        }
    } else if b.is_array() {
        if let Some(e) = b.element_type() {
            append_type_label(e, type_parameters, out);
        }
        for _ in 0..b.dimensions() {
            out.push_str("[]");
        }
    } else {
        let name = b.type_declaration().unwrap_or(b).name();
        if name.is_empty() {
            if b.is_enum() {
                out.push_str("{...}");
            } else if b.is_anonymous() {
                let base = b.interfaces().first().copied().or_else(|| b.superclass());
                match base {
                    Some(base) => {
                        let mut s = String::new();
                        append_type_label(base, type_parameters, &mut s);
                        out.push_str(&format!("new {s}() {{...}}"));
                    }
                    None => out.push_str("{...}"),
                }
            } else {
                out.push_str("UNKNOWN");
            }
        } else {
            out.push_str(name);
        }
        if type_parameters {
            if b.is_generic_type() {
                let params = b.type_parameters();
                if !params.is_empty() {
                    out.push('<');
                    out.push_str(&params.iter().map(|p| p.name().to_owned()).collect::<Vec<_>>().join(", "));
                    out.push('>');
                }
            } else if b.is_parameterized_type() {
                let args = b.type_arguments();
                if !args.is_empty() {
                    out.push('<');
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        append_type_label(*a, type_parameters, out);
                    }
                    out.push('>');
                }
            }
        }
    }
}

/// `Bindings.getRawName(type)`.
pub fn raw_name(b: BindingRef<'_>) -> String {
    let name = b.name();
    if b.is_parameterized_type() || b.is_generic_type() {
        if let Some(i) = name.find('<') {
            return name[..i].to_owned();
        }
    }
    name.to_owned()
}

/// `TypeChangeCorrectionProposalCore.containsNestedCapture`.
pub fn contains_nested_capture(b: Option<BindingRef<'_>>, nested: bool) -> bool {
    let Some(b) = b else { return false };
    if b.is_primitive() || b.is_type_variable() {
        return false;
    }
    if b.is_capture() {
        if nested {
            return true;
        }
        return contains_nested_capture(b.wildcard(), true);
    }
    if b.is_wildcard_type() {
        return contains_nested_capture(b.bound(), true);
    }
    if b.is_array() {
        return contains_nested_capture(b.element_type(), true);
    }
    b.type_arguments().into_iter().any(|a| contains_nested_capture(Some(a), true))
}
