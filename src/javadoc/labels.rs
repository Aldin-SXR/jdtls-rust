//! Port of `JavaElementLabelComposerCore` for the flags jdt.ls hover uses
//! (`HoverInfoProvider.COMMON_SIGNATURE_FLAGS` / `LOCAL_VARIABLE_FLAGS`),
//! over element data from the bridge. Elements returned by `codeSelect` are
//! resolved, so the `USE_RESOLVED` branches apply: type parameters render as
//! plain names (no bounds), type names as simple erasure names plus type
//! arguments.

use serde::Deserialize;

pub const CONCAT_STRING: &str = " - ";
pub const COMMA_STRING: &str = ", ";
pub const ELLIPSIS_STRING: &str = "...";
pub const DEFAULT_PACKAGE: &str = "(default package)";

/// A type signature (`appendTypeSignatureLabel` input).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypeRef {
    /// `base`, `array`, `class`, `tv`, `wild`, `inter`.
    pub k: String,
    pub n: Option<String>,
    /// Array element type.
    pub e: Option<Box<TypeRef>>,
    /// Array dimensions.
    pub d: Option<u32>,
    /// Type arguments.
    pub a: Option<Vec<TypeRef>>,
    /// Wildcard / intersection bounds.
    pub b: Option<serde_json::Value>,
    /// Wildcard upper bound (`? extends`).
    pub up: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Container {
    /// `type` or `method`.
    pub kind: String,
    pub name: String,
    pub has_params: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeLabel {
    pub package: String,
    pub containers: Vec<Container>,
    pub name: String,
    pub anonymous_super: Option<String>,
    pub type_parameters: Option<Vec<String>>,
    /// Bounds of `type_parameters` (unresolved declarations only).
    pub type_parameter_bounds: Option<Vec<Vec<TypeRef>>>,
    pub type_arguments: Option<Vec<TypeRef>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParamLabel {
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MethodLabel {
    pub name: String,
    pub is_constructor: bool,
    pub declaring_type: TypeLabel,
    pub type_parameters: Option<Vec<String>>,
    pub type_parameter_bounds: Option<Vec<Vec<TypeRef>>>,
    pub type_arguments: Option<Vec<TypeRef>>,
    /// Type arguments of the parameterized declaring type of a constructor.
    pub constructor_type_arguments: Option<Vec<TypeRef>>,
    pub return_type: Option<TypeRef>,
    pub parameters: Vec<ParamLabel>,
    pub varargs: bool,
    pub exceptions: Vec<TypeRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldLabel {
    pub name: String,
    pub is_enum_constant: bool,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub declaring_type: Option<TypeLabel>,
    pub static_final: bool,
    pub constant: Option<crate::javadoc::doc_ast::Constant>,
}

/// Label of a declaring member (post-qualification of locals / type params).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemberLabel {
    pub kind: String,
    pub method: Option<MethodLabel>,
    #[serde(rename = "type")]
    pub ty: Option<TypeLabel>,
    pub field: Option<FieldLabel>,
}

// ─── appendTypeSignatureLabel ────────────────────────────────────────────────

pub fn type_ref(buf: &mut String, t: &TypeRef) {
    match t.k.as_str() {
        "base" | "tv" => buf.push_str(t.n.as_deref().unwrap_or("")),
        "array" => {
            if let Some(e) = &t.e {
                type_ref(buf, e);
            }
            for _ in 0..t.d.unwrap_or(1) {
                buf.push_str("[]");
            }
        }
        "class" => {
            buf.push_str(t.n.as_deref().unwrap_or(""));
            if let Some(args) = &t.a {
                type_arguments(buf, args);
            }
        }
        "wild" => match bound(t) {
            None => buf.push('?'),
            Some(b) => {
                buf.push_str(if t.up.unwrap_or(true) { "? extends " } else { "? super " });
                type_ref(buf, &b);
            }
        },
        "inter" => {
            let bounds: Vec<TypeRef> = t
                .b
                .clone()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            for (i, b) in bounds.iter().enumerate() {
                if i > 0 {
                    buf.push_str(" & ");
                }
                type_ref(buf, b);
            }
        }
        _ => {}
    }
}

fn bound(t: &TypeRef) -> Option<TypeRef> {
    t.b.clone().and_then(|v| serde_json::from_value(v).ok())
}

pub fn type_ref_string(t: &TypeRef) -> String {
    let mut s = String::new();
    type_ref(&mut s, t);
    s
}

fn type_arguments(buf: &mut String, args: &[TypeRef]) {
    if args.is_empty() {
        return;
    }
    buf.push('<');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            buf.push_str(COMMA_STRING);
        }
        type_ref(buf, a);
    }
    buf.push('>');
}

fn type_parameter_names(buf: &mut String, names: &[String], bounds: Option<&Vec<Vec<TypeRef>>>) {
    if names.is_empty() {
        return;
    }
    buf.push('<');
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            buf.push_str(COMMA_STRING);
        }
        match bounds.and_then(|b| b.get(i)) {
            Some(b) => type_parameter_label(buf, n, b),
            None => buf.push_str(n),
        }
    }
    buf.push('>');
}

// ─── appendTypeLabel ─────────────────────────────────────────────────────────

/// `appendTypeLabel(type, T_FULLY_QUALIFIED [| T_TYPE_PARAMETERS])`.
pub fn type_label(buf: &mut String, t: &TypeLabel, fully_qualified: bool, type_params: bool) {
    if fully_qualified {
        if !t.package.is_empty() {
            buf.push_str(&t.package);
            buf.push('.');
        }
        for c in &t.containers {
            if c.kind == "field" {
                buf.push_str(&c.name);
            } else if c.kind == "method" {
                // anonymous or local: appendElementLabel(parent, 0)
                buf.push_str(&c.name);
                buf.push('(');
                if c.has_params {
                    buf.push_str(ELLIPSIS_STRING);
                }
                buf.push(')');
            } else {
                buf.push_str(&c.name);
            }
            buf.push('.');
        }
    }
    if t.name.is_empty() {
        match &t.anonymous_super {
            Some(s) => buf.push_str(&format!("new {s}() {{...}}")),
            None => buf.push_str("new Anonymous"),
        }
    } else {
        buf.push_str(&t.name);
    }
    if type_params {
        if let Some(args) = &t.type_arguments {
            type_arguments(buf, args);
        } else if let Some(names) = &t.type_parameters {
            type_parameter_names(buf, names, t.type_parameter_bounds.as_ref());
        }
    }
}

/// `JavaElementLabelsCore.getElementLabel(type, 0)`.
pub fn simple_type_name(t: &TypeLabel) -> String {
    let mut s = String::new();
    type_label(&mut s, t, false, false);
    s
}

// ─── appendMethodLabel ───────────────────────────────────────────────────────

/// Method label flags that matter for hover.
#[derive(Clone, Copy, Debug)]
pub struct MethodFlags {
    pub pre_type_parameters: bool,
    pub pre_return_type: bool,
    pub fully_qualified: bool,
    pub parameter_types: bool,
    pub parameter_names: bool,
    pub exceptions: bool,
}

impl MethodFlags {
    /// `COMMON_SIGNATURE_FLAGS`
    pub const HOVER: MethodFlags = MethodFlags {
        pre_type_parameters: true,
        pre_return_type: true,
        fully_qualified: true,
        parameter_types: true,
        parameter_names: true,
        exceptions: true,
    };
    /// `M_PARAMETER_TYPES | M_FULLY_QUALIFIED | T_FULLY_QUALIFIED`
    pub const POST_QUALIFIER: MethodFlags = MethodFlags {
        pre_type_parameters: false,
        pre_return_type: false,
        fully_qualified: true,
        parameter_types: true,
        parameter_names: false,
        exceptions: false,
    };
}

pub fn method_label(buf: &mut String, m: &MethodLabel, f: MethodFlags) {
    if f.pre_type_parameters {
        if let Some(args) = m.type_arguments.as_ref().filter(|a| !a.is_empty()) {
            type_arguments(buf, args);
            buf.push(' ');
        } else if let Some(names) = m.type_parameters.as_ref().filter(|a| !a.is_empty()) {
            type_parameter_names(buf, names, m.type_parameter_bounds.as_ref());
            buf.push(' ');
        }
    }
    if f.pre_return_type && !m.is_constructor {
        if let Some(r) = &m.return_type {
            type_ref(buf, r);
            buf.push(' ');
        }
    }
    if f.fully_qualified {
        type_label(buf, &m.declaring_type, true, false);
        buf.push('.');
    }
    buf.push_str(&m.name);
    // T_TYPE_PARAMETERS: constructor of a parameterized type
    if f.pre_type_parameters && m.is_constructor {
        if let Some(args) = &m.constructor_type_arguments {
            type_arguments(buf, args);
        }
    }
    buf.push('(');
    if f.parameter_types || f.parameter_names {
        let n = m.parameters.len();
        for (i, p) in m.parameters.iter().enumerate() {
            let last = i + 1 == n;
            if f.parameter_types {
                if m.varargs && last && p.ty.k == "array" {
                    let dims = p.ty.d.unwrap_or(1);
                    if let Some(e) = &p.ty.e {
                        type_ref(buf, e);
                    }
                    for _ in 1..dims {
                        buf.push_str("[]");
                    }
                    buf.push_str(ELLIPSIS_STRING);
                } else {
                    type_ref(buf, &p.ty);
                }
            }
            if f.parameter_names {
                if let Some(name) = &p.name {
                    if f.parameter_types {
                        buf.push(' ');
                    }
                    buf.push_str(name);
                }
            }
            if !last {
                buf.push_str(COMMA_STRING);
            }
        }
    } else if !m.parameters.is_empty() {
        buf.push_str(ELLIPSIS_STRING);
    }
    buf.push(')');
    if f.exceptions && !m.exceptions.is_empty() {
        buf.push_str(" throws ");
        for (i, e) in m.exceptions.iter().enumerate() {
            if i > 0 {
                buf.push_str(COMMA_STRING);
            }
            type_ref(buf, e);
        }
    }
}

/// `getElementLabel(method, 0)`: `name(...)`.
pub fn method_short_label(name: &str, has_params: bool) -> String {
    format!("{name}({})", if has_params { ELLIPSIS_STRING } else { "" })
}

// ─── appendFieldLabel / locals / type parameters ─────────────────────────────

/// `appendFieldLabel(field, COMMON_SIGNATURE_FLAGS)`: type + name (no
/// qualification, F_FULLY_QUALIFIED is cleared).
pub fn field_label(buf: &mut String, f: &FieldLabel) {
    if !f.is_enum_constant {
        type_ref(buf, &f.ty);
        buf.push(' ');
    }
    buf.push_str(&f.name);
}

/// `appendElementLabel(member, M_PARAMETER_TYPES | M_FULLY_QUALIFIED | T_FULLY_QUALIFIED)`.
pub fn member_post_qualifier(buf: &mut String, m: &MemberLabel) {
    match m.kind.as_str() {
        "method" => {
            if let Some(method) = &m.method {
                method_label(buf, method, MethodFlags::POST_QUALIFIER);
            }
        }
        "type" => {
            if let Some(t) = &m.ty {
                type_label(buf, t, true, false);
            }
        }
        "field" => {
            if let Some(f) = &m.field {
                // F_* flags are not set: plain name
                buf.push_str(&f.name);
            }
        }
        _ => {}
    }
}

/// `appendLocalVariableLabel(local, LOCAL_VARIABLE_FLAGS)`.
pub fn local_variable_label(buf: &mut String, name: &str, ty: &TypeRef, declaring: Option<&MemberLabel>) {
    type_ref(buf, ty);
    buf.push(' ');
    buf.push_str(name);
    if let Some(m) = declaring {
        buf.push_str(CONCAT_STRING);
        member_post_qualifier(buf, m);
    }
}

/// `appendTypeParameterLabel(typeParameter, COMMON_SIGNATURE_FLAGS)`:
/// name and bounds (TP_POST_QUALIFIED is not set).
pub fn type_parameter_label(buf: &mut String, name: &str, bounds: &[TypeRef]) {
    buf.push_str(name);
    let only_object = bounds.len() == 1 && bounds[0].k == "class" && bounds[0].n.as_deref() == Some("Object");
    if !bounds.is_empty() && !only_object {
        buf.push_str(" extends ");
        for (i, b) in bounds.iter().enumerate() {
            if i > 0 {
                buf.push_str(" & ");
            }
            type_ref(buf, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(n: &str) -> TypeRef {
        TypeRef { k: "class".into(), n: Some(n.into()), ..Default::default() }
    }

    #[test]
    fn method_labels() {
        let m = MethodLabel {
            name: "bar".into(),
            declaring_type: TypeLabel { package: "test1".into(), name: "E".into(), ..Default::default() },
            type_parameters: Some(vec!["U".into()]),
            return_type: Some(TypeRef { k: "tv".into(), n: Some("U".into()), ..Default::default() }),
            parameters: vec![ParamLabel { ty: TypeRef { k: "tv".into(), n: Some("U".into()), ..Default::default() }, name: Some("s".into()) }],
            ..Default::default()
        };
        let mut s = String::new();
        method_label(&mut s, &m, MethodFlags::HOVER);
        assert_eq!(s, "<U> U test1.E.bar(U s)");
        let mut s = String::new();
        let arr = TypeRef { k: "array".into(), e: Some(Box::new(class("String"))), d: Some(1), ..Default::default() };
        let main = MethodLabel {
            name: "main".into(),
            declaring_type: TypeLabel { package: "java".into(), name: "Foo".into(), ..Default::default() },
            parameters: vec![ParamLabel { ty: arr.clone(), name: Some("args".into()) }],
            return_type: Some(TypeRef { k: "base".into(), n: Some("void".into()), ..Default::default() }),
            ..Default::default()
        };
        local_variable_label(&mut s, "args", &arr, Some(&MemberLabel { kind: "method".into(), method: Some(main), ..Default::default() }));
        assert_eq!(s, "String[] args - java.Foo.main(String[])");
    }
}
