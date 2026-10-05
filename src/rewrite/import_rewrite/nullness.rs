//! RedundantNullnessTypeAnnotationsFilter and StubUtility2Core's copy policy.
use super::TypeLocation;
use crate::semantic_ast::{
    annotation::{Annotation, Value},
    Ast, BindingRef, NodeId, NodeKind,
};
use std::collections::{BTreeMap, HashSet};

const PREFIX: &str = "org.eclipse.jdt.core.compiler.annotation.";
fn option<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    options.get(&format!("{PREFIX}{name}")).map(String::as_str)
}
#[derive(Clone, Debug)]
pub struct Filter {
    nonnull: String,
    nullable: String,
    pub defaults: HashSet<TypeLocation>,
}
impl Filter {
    pub fn create(
        ast: &Ast,
        node: Option<NodeId>,
        options: &BTreeMap<String, String>,
    ) -> Option<Self> {
        if option(options, "nullanalysis") != Some("enabled") {
            return None;
        }
        let node = node?;
        let nonnull = option(options, "nonnull")?.to_owned();
        let nullable = option(options, "nullable")?.to_owned();
        let mut names = HashSet::from([option(options, "nonnullbydefault")?.to_owned()]);
        if let Some(secondary) = option(options, "nonnullbydefault.secondary") {
            names.extend(
                secondary
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            );
        }
        Some(Self {
            nonnull,
            nullable,
            defaults: default_locations(ast, node, &names),
        })
    }
    pub fn remove<'a>(
        &self,
        annotations: &'a [Annotation],
        location: TypeLocation,
        typ: BindingRef<'_>,
    ) -> Vec<&'a Annotation> {
        if location == TypeLocation::Other {
            return Vec::new();
        }
        if typ.is_type_variable() || typ.is_wildcard_type() {
            return annotations.iter().collect();
        }
        let exclude_all = matches!(
            location,
            TypeLocation::LocalVariable
                | TypeLocation::Cast
                | TypeLocation::Exception
                | TypeLocation::New
                | TypeLocation::Instanceof
                | TypeLocation::Receiver
        );
        annotations
            .iter()
            .filter(|a| {
                let name = typ.ast.binding(a.annotation_type).qualified_name();
                !(exclude_all || self.defaults.contains(&location))
                    || name != self.nonnull && (!exclude_all || name != self.nullable)
            })
            .collect()
    }
}
fn named<'a>(
    ast: &Ast,
    annotations: impl IntoIterator<Item = &'a Annotation>,
    names: &HashSet<String>,
) -> Vec<&'a Annotation> {
    annotations
        .into_iter()
        .filter(|a| names.contains(ast.binding(a.annotation_type).qualified_name()))
        .collect()
}
/// Nearest explicit default wins, including an empty/false default that cancels
/// the enclosing one. Package annotations precede module annotations.
pub fn default_locations(
    ast: &Ast,
    start: NodeId,
    names: &HashSet<String>,
) -> HashSet<TypeLocation> {
    let mut node = Some(ast.node(start));
    while let Some(current) = node {
        if current.kind() == NodeKind::CompilationUnit {
            if let Some(package) = current.child("package").and_then(|p| p.binding()) {
                let annotations = named(ast, &package.data().annotations, names);
                if !annotations.is_empty() {
                    return annotation_locations(ast, &annotations);
                }
                if let Some(module) = package.data().module.map(|id| ast.binding(id)) {
                    let annotations = named(ast, &module.data().annotations, names);
                    if !annotations.is_empty() {
                        return annotation_locations(ast, &annotations);
                    }
                }
            }
        } else {
            let modifiers = if current.kind() == NodeKind::VariableDeclarationFragment {
                current
                    .parent()
                    .filter(|p| {
                        matches!(
                            p.kind(),
                            NodeKind::FieldDeclaration
                                | NodeKind::MethodDeclaration
                                | NodeKind::TypeDeclaration
                                | NodeKind::EnumDeclaration
                                | NodeKind::AnnotationTypeDeclaration
                        )
                    })
                    .map(|p| p.list("modifiers"))
                    .unwrap_or_default()
            } else if matches!(
                current.kind(),
                NodeKind::TypeDeclaration
                    | NodeKind::AnnotationTypeDeclaration
                    | NodeKind::EnumDeclaration
                    | NodeKind::AnnotationTypeMemberDeclaration
                    | NodeKind::MethodDeclaration
                    | NodeKind::FieldDeclaration
                    | NodeKind::EnumConstantDeclaration
                    | NodeKind::VariableDeclarationStatement
                    | NodeKind::VariableDeclarationExpression
                    | NodeKind::SingleVariableDeclaration
            ) {
                current.list("modifiers")
            } else {
                Vec::new()
            };
            let annotations = named(
                ast,
                modifiers
                    .iter()
                    .filter_map(|m| ast.data(m.id).annotation.as_ref()),
                names,
            );
            if !annotations.is_empty() {
                return annotation_locations(ast, &annotations);
            }
        }
        node = current.parent();
    }
    HashSet::new()
}
fn annotation_locations(ast: &Ast, annotations: &[&Annotation]) -> HashSet<TypeLocation> {
    let mut locations = HashSet::new();
    for annotation in annotations {
        if annotation.all_members.is_empty() {
            let annotation_type = ast.binding(annotation.annotation_type);
            if let Some(meta) = annotation_type
                .data()
                .annotations
                .iter()
                .find(|a| ast.binding(a.annotation_type).name() == "TypeQualifierDefault")
            {
                for (_, value) in &meta.all_members {
                    add_value(ast, value, &mut locations, true);
                }
            } else {
                add_value(ast, &Value::Boolean(true), &mut locations, false);
            }
        } else {
            // IMemberValuePairBinding.getKey currently always returns null,
            // so JDT processes each resolved member, not only one named value.
            for (_, value) in &annotation.all_members {
                add_value(ast, value, &mut locations, false);
            }
        }
    }
    locations
}
fn add_value(ast: &Ast, value: &Value, locations: &mut HashSet<TypeLocation>, element_types: bool) {
    if let Value::Array(values) = value {
        for value in values {
            add_scalar_value(ast, value, locations, element_types);
        }
    } else {
        add_scalar_value(ast, value, locations, element_types);
    }
}
fn add_scalar_value(
    ast: &Ast,
    value: &Value,
    locations: &mut HashSet<TypeLocation>,
    element_types: bool,
) {
    match value {
        Value::Boolean(true) if !element_types => locations.extend([
            TypeLocation::ReturnType,
            TypeLocation::Parameter,
            TypeLocation::Field,
        ]),
        Value::Enum(id) => {
            let name = ast.binding(*id).name();
            let location = if element_types {
                match name {
                    "METHOD" => Some(TypeLocation::ReturnType),
                    "PARAMETER" => Some(TypeLocation::Parameter),
                    "FIELD" => Some(TypeLocation::Field),
                    _ => None,
                }
            } else {
                match name {
                    "PARAMETER" => Some(TypeLocation::Parameter),
                    "RETURN_TYPE" => Some(TypeLocation::ReturnType),
                    "FIELD" => Some(TypeLocation::Field),
                    "TYPE_PARAMETER" => Some(TypeLocation::TypeParameter),
                    "TYPE_BOUND" => Some(TypeLocation::TypeBound),
                    "TYPE_ARGUMENT" => Some(TypeLocation::TypeArgument),
                    "ARRAY_CONTENTS" => Some(TypeLocation::ArrayContents),
                    "LOCAL_VARIABLE" => Some(TypeLocation::LocalVariable),
                    "CAST" => Some(TypeLocation::Cast),
                    "INSTANCEOF" => Some(TypeLocation::Instanceof),
                    "NEW" => Some(TypeLocation::New),
                    "RECEIVER" => Some(TypeLocation::Receiver),
                    "EXCEPTION" => Some(TypeLocation::Exception),
                    "OTHER" => Some(TypeLocation::Other),
                    "UNKNOWN" => Some(TypeLocation::Unknown),
                    _ => None,
                }
            };
            locations.extend(location);
        }
        _ => {}
    }
}
/// Only the configured primary nullness annotation types are copied. This is
/// independent of null-analysis enablement, as in StubUtility2Core.
pub fn copy_on_inherit(
    annotation: &Annotation,
    ast: &Ast,
    options: &BTreeMap<String, String>,
    defaults: Option<&HashSet<TypeLocation>>,
    location: TypeLocation,
) -> bool {
    if option(options, "inheritNullAnnotations") == Some("enabled") {
        return false;
    }
    let name = ast.binding(annotation.annotation_type).qualified_name();
    if option(options, "nonnull") == Some(name) {
        return defaults.is_none_or(|d| !d.contains(&location));
    }
    option(options, "nullable") == Some(name)
}

pub fn inherited_parameter_annotations<'a>(
    method: BindingRef<'a>,
    index: usize,
    options: &BTreeMap<String, String>,
    defaults: Option<&HashSet<TypeLocation>>,
) -> Vec<&'a Annotation> {
    method
        .data()
        .parameter_annotations
        .get(index)
        .into_iter()
        .flatten()
        .filter(|a| copy_on_inherit(a, method.ast, options, defaults, TypeLocation::Parameter))
        .collect()
}
