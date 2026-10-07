//! Binding helpers of the unresolved-element processors: `Bindings`,
//! `ASTResolving` type utilities and `BindingLabelProviderCore` labels
//! (`DEFAULT_TEXTFLAGS`), over the semantic AST bindings.

use std::collections::HashSet;

use crate::semantic_ast::{bflag, modifier, Ast, BindingRef, Node, NodeKind};

/// `AST.resolveWellKnownType(name)` (the bridge exports the well-known types
/// for units with unresolved invocations).
pub fn well_known<'a>(ast: &'a Ast, name: &str) -> Option<BindingRef<'a>> {
    ast.type_by_name(name)
}

/// `Bindings.normalizeTypeBinding`.
pub fn normalize(t: Option<BindingRef<'_>>) -> Option<BindingRef<'_>> {
    let t = t?;
    if t.is_null_type() || (t.is_primitive() && t.name() == "void") {
        return None;
    }
    if t.is_anonymous() {
        let interfaces = t.interfaces();
        return interfaces.first().copied().or_else(|| t.superclass());
    }
    if t.is_capture() {
        return t.wildcard();
    }
    Some(t)
}

/// `ITypeBinding.getErasure()`.
pub fn erasure(t: BindingRef<'_>) -> BindingRef<'_> {
    t.erasure().unwrap_or(t)
}

/// `ITypeBinding.getTypeDeclaration()`.
pub fn declaration(t: BindingRef<'_>) -> BindingRef<'_> {
    t.type_declaration().unwrap_or(t)
}

/// `IMethodBinding.getMethodDeclaration()`.
pub fn method_declaration(m: BindingRef<'_>) -> BindingRef<'_> {
    m.method_declaration().unwrap_or(m)
}

/// `ITypeBinding.isAssignmentCompatible(target)` (compiler relation).
pub fn can_assign(t: BindingRef<'_>, target: BindingRef<'_>) -> bool {
    t == target || t.data().assignment_targets.contains(&target.id) || {
        // The relation is exported per binding object; compare by key too.
        t.data().assignment_targets.iter().any(|b| t.ast.binding(*b) == target)
    }
}

/// `ITypeBinding.isCastCompatible(target)` (compiler relation).
pub fn is_cast_compatible(t: BindingRef<'_>, target: BindingRef<'_>) -> bool {
    t == target || t.data().cast_targets.iter().any(|b| t.ast.binding(*b) == target)
}

/// `ITypeBinding.isUpperbound()`.
pub fn is_upperbound(t: BindingRef<'_>) -> bool {
    t.has(bflag::UPPERBOUND)
}

/// `ASTResolving.normalizeWildcardType`.
pub fn normalize_wildcard(t: BindingRef<'_>, is_binding_to_assign: bool) -> Option<BindingRef<'_>> {
    let bound = t.bound();
    if is_binding_to_assign {
        if bound.is_none() || !is_upperbound(t) {
            let bounds = t.type_bounds();
            return Some(bounds.first().copied().unwrap_or_else(|| erasure(t)));
        }
    } else if bound.is_none() || is_upperbound(t) {
        return None;
    }
    bound
}

/// `TypeMismatchBaseSubProcessor.boxOrUnboxPrimitives`.
pub fn box_or_unbox<'a>(cast_type: BindingRef<'a>, to_cast: BindingRef<'a>) -> BindingRef<'a> {
    if cast_type.is_primitive() && !to_cast.is_primitive() {
        boxed(cast_type).unwrap_or(cast_type)
    } else if !cast_type.is_primitive() && to_cast.is_primitive() {
        unboxed(cast_type).unwrap_or(cast_type)
    } else {
        cast_type
    }
}

const BOXES: [(&str, &str); 8] = [
    ("boolean", "java.lang.Boolean"),
    ("byte", "java.lang.Byte"),
    ("char", "java.lang.Character"),
    ("short", "java.lang.Short"),
    ("int", "java.lang.Integer"),
    ("long", "java.lang.Long"),
    ("float", "java.lang.Float"),
    ("double", "java.lang.Double"),
];

/// `Bindings.getBoxedTypeBinding`.
pub fn boxed(t: BindingRef<'_>) -> Option<BindingRef<'_>> {
    if !t.is_primitive() {
        return Some(t);
    }
    BOXES.iter().find(|(p, _)| *p == t.name()).and_then(|(_, b)| well_known(t.ast, b)).or(Some(t))
}

/// `Bindings.getUnboxedTypeBinding`.
pub fn unboxed(t: BindingRef<'_>) -> Option<BindingRef<'_>> {
    if !t.is_class() {
        return Some(t);
    }
    let q = erasure(t).qualified_name();
    BOXES.iter().find(|(_, b)| *b == q).and_then(|(p, _)| well_known(t.ast, p)).or(Some(t))
}

/// `ITypeBinding.createArrayType(1)` when the AST knows the array type.
pub fn array_of(t: BindingRef<'_>) -> Option<BindingRef<'_>> {
    t.ast.binding_by_key(&format!("[{}", t.key()))
}

/// `ITypeBinding.isEqualTo`.
pub fn same(a: BindingRef<'_>, b: BindingRef<'_>) -> bool {
    a == b
}

/// `ASTResolving.isUseableTypeInContext(type, context, noWildcards)`;
/// `context` is a method, type or variable binding.
pub fn is_useable_in_context(t: BindingRef<'_>, context: BindingRef<'_>, no_wildcards: bool) -> bool {
    let mut t = t;
    if t.is_array() {
        if let Some(e) = t.element_type() {
            t = e;
        }
    }
    if t.is_anonymous() {
        return false;
    }
    if t.is_raw_type() || t.is_primitive() {
        return true;
    }
    if t.is_type_variable() {
        return variable_defined_in_context(context, t);
    }
    if t.is_generic_type() {
        return t.type_parameters().into_iter().all(|p| is_useable_in_context(p, context, no_wildcards));
    }
    if t.is_parameterized_type() {
        return t.type_arguments().into_iter().all(|p| is_useable_in_context(p, context, no_wildcards));
    }
    if t.is_capture() {
        if let Some(w) = t.wildcard() {
            t = w;
        }
    }
    if t.is_wildcard_type() {
        if no_wildcards {
            return false;
        }
        if let Some(b) = t.bound() {
            return is_useable_in_context(b, context, no_wildcards);
        }
    }
    true
}

fn variable_defined_in_context(binding: BindingRef<'_>, type_variable: BindingRef<'_>) -> bool {
    let mut b = Some(binding);
    if binding.is_variable() {
        b = binding.declaring_method().or_else(|| binding.declaring_class());
    }
    if let Some(m) = b.filter(|m| m.is_method()) {
        if type_variable.declaring_method().is_some_and(|d| d == m) {
            return true;
        }
        b = m.declaring_class();
    }
    let mut seen = HashSet::new();
    while let Some(t) = b.filter(|t| t.is_type() && seen.insert(t.key().to_owned())) {
        if type_variable.declaring_class().is_some_and(|d| d == t) {
            return true;
        }
        if t.modifiers() & modifier::STATIC != 0 {
            break;
        }
        b = t.declaring_class();
    }
    false
}

/// `Bindings.getBindingOfParentType(node)`.
pub fn parent_type_binding<'a>(node: Node<'a>) -> Option<BindingRef<'a>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        n = x.parent();
    }
    None
}

/// `Bindings.isSuperType(possibleSuperType, type)`.
pub fn is_super_type(possible: BindingRef<'_>, t: BindingRef<'_>) -> bool {
    fn walk(possible: BindingRef<'_>, t: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(t.key().to_owned()) {
            return false;
        }
        if declaration(t) == declaration(possible) {
            return true;
        }
        if let Some(s) = t.superclass() {
            if walk(possible, s, seen) {
                return true;
            }
        }
        if possible.is_interface() {
            for i in t.interfaces() {
                if walk(possible, i, seen) {
                    return true;
                }
            }
        }
        false
    }
    walk(possible, t, &mut HashSet::new())
}

/// `Bindings.findMethodInHierarchy(type, name, parameters)` with
/// `parameters == None` matching any method of that name.
pub fn find_method_in_hierarchy<'a>(t: BindingRef<'a>, name: &str, parameters: Option<&[BindingRef<'a>]>) -> Option<BindingRef<'a>> {
    fn walk<'a>(t: BindingRef<'a>, name: &str, parameters: Option<&[BindingRef<'a>]>, seen: &mut HashSet<String>) -> Option<BindingRef<'a>> {
        if !seen.insert(t.key().to_owned()) {
            return None;
        }
        for m in t.declared_methods().unwrap_or_default() {
            if m.name() != name {
                continue;
            }
            match parameters {
                None => return Some(m),
                Some(params) => {
                    let mine = m.parameter_types();
                    if mine.len() == params.len() && mine.iter().zip(params).all(|(a, b)| erasure(*a) == erasure(*b)) {
                        return Some(m);
                    }
                }
            }
        }
        if let Some(s) = t.superclass() {
            if let Some(m) = walk(s, name, parameters, seen) {
                return Some(m);
            }
        }
        for i in t.interfaces() {
            if let Some(m) = walk(i, name, parameters, seen) {
                return Some(m);
            }
        }
        None
    }
    walk(t, name, parameters, &mut HashSet::new())
}

/// `Bindings.findFieldInHierarchy(type, name)`.
pub fn find_field_in_hierarchy<'a>(t: BindingRef<'a>, name: &str) -> Option<BindingRef<'a>> {
    fn walk<'a>(t: BindingRef<'a>, name: &str, seen: &mut HashSet<String>) -> Option<BindingRef<'a>> {
        if !seen.insert(t.key().to_owned()) {
            return None;
        }
        if let Some(f) = t.declared_fields().unwrap_or_default().into_iter().find(|f| f.name() == name) {
            return Some(f);
        }
        if let Some(s) = t.superclass() {
            if let Some(f) = walk(s, name, seen) {
                return Some(f);
            }
        }
        t.interfaces().into_iter().find_map(|i| walk(i, name, seen))
    }
    walk(t, name, &mut HashSet::new())
}

// ─── Labels ───────────────────────────────────────────────────────────────────

/// `BindingLabelProviderCore.getBindingLabel(type, DEFAULT_TEXTFLAGS)`
/// (= `ASTResolving.getTypeSignature`).
pub fn type_label(t: BindingRef<'_>) -> String {
    let mut buf = String::new();
    type_label_into(t, true, &mut buf);
    buf
}

fn type_label_into(t: BindingRef<'_>, type_parameters: bool, buf: &mut String) {
    if t.is_capture() {
        if let Some(w) = t.wildcard() {
            type_label_into(w, type_parameters, buf);
            return;
        }
    }
    if t.is_wildcard_type() {
        buf.push('?');
        if let Some(bound) = t.bound() {
            buf.push_str(if is_upperbound(t) { " extends " } else { " super " });
            type_label_into(bound, type_parameters, buf);
        }
        return;
    }
    if t.is_array() {
        if let Some(e) = t.element_type() {
            type_label_into(e, type_parameters, buf);
        }
        for _ in 0..t.dimensions() {
            buf.push_str("[]");
        }
        return;
    }
    let name = declaration(t).name();
    if name.is_empty() {
        if t.is_enum() {
            buf.push_str("{...}");
        } else if t.is_anonymous() {
            let base = t.interfaces().first().copied().or_else(|| t.superclass());
            match base {
                Some(b) => {
                    let mut inner = String::new();
                    type_label_into(b, type_parameters, &mut inner);
                    buf.push_str(&format!("new {inner}() {{...}}"));
                }
                None => buf.push_str("new Anonymous"),
            }
        } else {
            buf.push_str("UNKNOWN");
        }
    } else {
        buf.push_str(name);
    }
    if type_parameters {
        if t.is_generic_type() {
            let params = t.type_parameters();
            if !params.is_empty() {
                buf.push('<');
                buf.push_str(&params.iter().map(|p| p.name().to_owned()).collect::<Vec<_>>().join(", "));
                buf.push('>');
            }
        } else if t.is_parameterized_type() {
            let args = t.type_arguments();
            if !args.is_empty() {
                buf.push('<');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    type_label_into(*a, type_parameters, buf);
                }
                buf.push('>');
            }
        }
    }
}

/// `ASTResolving.getMethodSignature(binding)` (`DEFAULT_TEXTFLAGS` label).
pub fn method_label(m: BindingRef<'_>) -> String {
    let mut buf = String::new();
    buf.push_str(m.name());
    if m.has(bflag::PARAMETERIZED_METHOD) {
        let args = m.type_arguments();
        if !args.is_empty() {
            buf.push_str(" <");
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                type_label_into(*a, true, &mut buf);
            }
            buf.push('>');
        }
    }
    buf.push('(');
    let params = m.parameter_types();
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        if m.is_varargs() && i == params.len() - 1 {
            if let Some(e) = p.element_type() {
                type_label_into(e, true, &mut buf);
            }
            for _ in 0..(p.dimensions() - 1).max(0) {
                buf.push_str("[]");
            }
            buf.push_str("...");
        } else {
            type_label_into(*p, true, &mut buf);
        }
    }
    buf.push(')');
    if m.has(bflag::GENERIC_METHOD) {
        let params = m.type_parameters();
        if !params.is_empty() {
            buf.push_str(" <");
            buf.push_str(&params.iter().map(|p| p.name().to_owned()).collect::<Vec<_>>().join(", "));
            buf.push('>');
        }
    }
    buf
}

/// `ASTResolving.getMethodSignature(name, params, isVarArgs)`.
pub fn method_signature(name: &str, params: &[BindingRef<'_>], varargs: bool) -> String {
    let mut buf = format!("{name}(");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        if varargs && i == params.len() - 1 {
            buf.push_str(&type_label(p.element_type().unwrap_or(*p)));
            buf.push_str("...");
        } else {
            buf.push_str(&type_label(*p));
        }
    }
    buf.push(')');
    buf
}

/// `getTypeNames(types)`.
pub fn type_names(types: &[BindingRef<'_>]) -> String {
    types.iter().map(|t| type_label(*t)).collect::<Vec<_>>().join(", ")
}
