//! ImportRewrite's AST overload, including type-use annotations and their values.
use super::{
    contains_nested_capture, is_type_in_unnamed_package, normalize_type_binding, ImportRewrite,
    ImportRewriteContext,
};
use crate::{
    rewrite::{flattener::Flattener, ASTRewrite, RNode},
    semantic_ast::{
        annotation::{Annotation, Value},
        bflag, BindingRef, NodeKind,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeLocation {
    Parameter,
    ReturnType,
    Field,
    TypeParameter,
    TypeBound,
    TypeArgument,
    ArrayContents,
    LocalVariable,
    Cast,
    Instanceof,
    New,
    Receiver,
    Exception,
    Other,
    Unknown,
}

impl ImportRewrite {
    pub fn add_annotation_string(
        &mut self,
        annotation: &Annotation,
        context: &dyn ImportRewriteContext,
    ) -> String {
        let mut rw = ASTRewrite::new(self.ast.clone());
        let node = self.add_annotation(annotation, &mut rw, context);
        Flattener::as_string(&rw, node)
    }
    /// `addImport(ITypeBinding, AST, context, TypeLocation)`. Unlike the string
    /// overload this retains owner types, generic arguments and annotations.
    pub fn add_import_type(
        &mut self,
        binding: BindingRef<'_>,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
        location: TypeLocation,
    ) -> RNode {
        let mut point = None;
        let mut current = Some(binding);
        while let Some(t) = current {
            if !t.data().type_annotations.is_empty() || !t.type_arguments().is_empty() {
                point = Some(t);
            }
            current = if t.is_member() {
                t.declaring_class()
            } else {
                None
            };
        }
        let base = point.unwrap_or(binding);
        let different = base.id != binding.id;
        let typ = self.import_type(
            base,
            rw,
            context,
            None,
            true,
            if different {
                TypeLocation::Other
            } else {
                location
            },
        );
        if different {
            self.build_type(binding, base, rw, context, typ, location)
        } else {
            typ
        }
    }

    pub fn add_import_type_string(
        &mut self,
        binding: BindingRef<'_>,
        context: &dyn ImportRewriteContext,
        location: TypeLocation,
    ) -> String {
        let mut rw = ASTRewrite::new(self.ast.clone());
        let typ = self.add_import_type(binding, &mut rw, context, location);
        Flattener::as_string(&rw, typ)
    }

    /// StubUtility2Core.createParameters moves the innermost dimension's
    /// annotations to `...`, retaining the other dimensions in source order.
    pub fn add_import_parameter_type(
        &mut self,
        binding: BindingRef<'_>,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
        varargs: bool,
    ) -> (RNode, Vec<RNode>) {
        if !varargs || !binding.is_array() {
            return (
                self.add_import_type(binding, rw, context, TypeLocation::Parameter),
                Vec::new(),
            );
        }
        let mut component = binding;
        let mut dimensions = Vec::new();
        let mut varargs_annotations = Vec::new();
        for i in 0..binding.dimensions() {
            let annotations = component
                .data()
                .type_annotations
                .iter()
                .map(|a| self.add_annotation(a, rw, context))
                .collect();
            if i + 1 == binding.dimensions() {
                varargs_annotations = annotations;
            } else {
                let dimension = rw.new_node(NodeKind::Dimension);
                rw.put_list(dimension, "annotations", annotations);
                dimensions.push(dimension);
            }
            if let Some(next) = component.component_type() {
                component = next;
            }
        }
        let element = self.add_import_type(component, rw, context, TypeLocation::Unknown);
        let typ = if dimensions.is_empty() {
            element
        } else {
            let array = rw.new_node(NodeKind::ArrayType);
            rw.put_child(array, "elementType", element);
            rw.put_list(array, "dimensions", dimensions)
        };
        (typ, varargs_annotations)
    }

    pub fn add_import_parameter_type_string(
        &mut self,
        binding: BindingRef<'_>,
        context: &dyn ImportRewriteContext,
        varargs: bool,
    ) -> String {
        let mut rw = ASTRewrite::new(self.ast.clone());
        let (typ, annotations) = self.add_import_parameter_type(binding, &mut rw, context, varargs);
        let mut text = Flattener::as_string(&rw, typ);
        for annotation in annotations {
            text.push(' ');
            text.push_str(&Flattener::as_string(&rw, annotation));
        }
        if varargs {
            if !binding.data().type_annotations.is_empty() || binding.dimensions() > 1 {
                text.push(' ');
            }
            text.push_str("...");
        }
        text
    }

    fn build_type(
        &mut self,
        binding: BindingRef<'_>,
        point: BindingRef<'_>,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
        qualifier: RNode,
        location: TypeLocation,
    ) -> RNode {
        if binding.id == point.id {
            return qualifier;
        }
        let owner = if binding.is_member() {
            binding
                .declaring_class()
                .map(|b| self.build_type(b, point, rw, context, qualifier, TypeLocation::Other))
        } else {
            None
        };
        self.import_type(binding, rw, context, owner, false, location)
    }

    fn annotate(
        &mut self,
        node: RNode,
        binding: BindingRef<'_>,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
        location: TypeLocation,
    ) {
        let annotations = context
            .remove_redundant_type_annotations(&binding.data().type_annotations, location, binding)
            .into_iter()
            .map(|a| self.add_annotation(a, rw, context))
            .collect();
        rw.put_list(node, "annotations", annotations);
    }

    fn import_type(
        &mut self,
        binding: BindingRef<'_>,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
        current: Option<RNode>,
        base: bool,
        location: TypeLocation,
    ) -> RNode {
        if binding.is_primitive() {
            let node = rw.new_primitive_type(binding.name());
            self.annotate(node, binding, rw, context, location);
            return node;
        }
        let Some(t) = normalize_type_binding(binding) else {
            let name = rw.new_simple_name("invalid");
            return rw.new_simple_type(name);
        };
        if t.is_type_variable() {
            let name = rw.new_simple_name(binding.name());
            let node = rw.new_simple_type(name);
            self.annotate(node, t, rw, context, location);
            return node;
        }
        if t.is_wildcard_type() {
            let node = rw.new_node(NodeKind::WildcardType);
            if let Some(bound) = t
                .bound()
                .filter(|b| !b.is_wildcard_type() && !b.is_capture())
            {
                let bound = self.add_import_type(bound, rw, context, TypeLocation::TypeBound);
                rw.put_child(node, "bound", bound);
                rw.put_simple(
                    node,
                    "upperBound",
                    if t.has(bflag::UPPERBOUND) {
                        "true"
                    } else {
                        "false"
                    },
                );
            }
            self.annotate(node, t, rw, context, location);
            return node;
        }
        if t.is_array() {
            let element = self.add_import_type(
                t.element_type().unwrap_or(t),
                rw,
                context,
                TypeLocation::ArrayContents,
            );
            let node = rw.new_node(NodeKind::ArrayType);
            rw.put_child(node, "elementType", element);
            let mut component = Some(t);
            let dimensions = (0..t.dimensions())
                .map(|i| {
                    let dimension = rw.new_node(NodeKind::Dimension);
                    if let Some(c) = component {
                        self.annotate(
                            dimension,
                            c,
                            rw,
                            context,
                            if i == 0 {
                                location
                            } else {
                                TypeLocation::ArrayContents
                            },
                        );
                        component = c.component_type();
                    }
                    dimension
                })
                .collect();
            return rw.put_list(node, "dimensions", dimensions);
        }
        let declaration = t.type_declaration().unwrap_or(t);
        let node = if base {
            let qualified = declaration.qualified_name();
            let name = if qualified.is_empty() {
                declaration.name().to_owned()
            } else {
                self.internal_add_import(qualified, context, is_type_in_unnamed_package(t))
            };
            if !t.data().type_annotations.is_empty() && name.rfind('.').is_some_and(|i| i > 0) {
                let (qualifier, name) = name.rsplit_once('.').unwrap();
                let node = rw.new_node(NodeKind::NameQualifiedType);
                let qualifier = rw.new_name(qualifier);
                let name = rw.new_simple_name(name);
                rw.put_child(node, "qualifier", qualifier);
                rw.put_child(node, "name", name)
            } else {
                let name = rw.new_name(&name);
                rw.new_simple_type(name)
            }
        } else if let Some(qualifier) = current {
            let node = rw.new_node(NodeKind::QualifiedType);
            let name = rw.new_simple_name(declaration.name());
            rw.put_child(node, "qualifier", qualifier);
            rw.put_child(node, "name", name)
        } else {
            let name = rw.new_name(declaration.name());
            rw.new_simple_type(name)
        };
        self.annotate(node, t, rw, context, location);
        let arguments = t.type_arguments();
        if arguments.is_empty() {
            return node;
        }
        let arguments = arguments
            .into_iter()
            .map(|a| {
                if contains_nested_capture(a, false) {
                    rw.new_node(NodeKind::WildcardType)
                } else {
                    self.add_import_type(a, rw, context, TypeLocation::TypeArgument)
                }
            })
            .collect();
        let parameterized = rw.new_node(NodeKind::ParameterizedType);
        rw.put_child(parameterized, "type", node);
        rw.put_list(parameterized, "typeArguments", arguments)
    }

    /// `addAnnotation(IAnnotationBinding, AST, context)`: only explicitly
    /// declared member values are copied; defaults remain implicit.
    pub fn add_annotation(
        &mut self,
        annotation: &Annotation,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
    ) -> RNode {
        let ast = self.ast.clone();
        let typ = self.add_import_type(
            ast.binding(annotation.annotation_type),
            rw,
            context,
            TypeLocation::Other,
        );
        let name = if rw.kind(typ) == NodeKind::SimpleType {
            rw.new_value(typ, "name").node().unwrap()
        } else {
            rw.new_name("invalid")
        };
        let single = annotation.members.len() == 1 && annotation.members[0].0 == "value";
        let node = rw.new_node(if annotation.members.is_empty() {
            NodeKind::MarkerAnnotation
        } else if single {
            NodeKind::SingleMemberAnnotation
        } else {
            NodeKind::NormalAnnotation
        });
        rw.put_child(node, "typeName", name);
        if single {
            if let Some(value) = self.annotation_value(&annotation.members[0].1, rw, context) {
                rw.put_child(node, "value", value);
            }
        } else if !annotation.members.is_empty() {
            let values = annotation
                .members
                .iter()
                .map(|(name, value)| {
                    let pair = rw.new_node(NodeKind::MemberValuePair);
                    let name = rw.new_simple_name(name);
                    rw.put_child(pair, "name", name);
                    if let Some(value) = self.annotation_value(value, rw, context) {
                        rw.put_child(pair, "value", value);
                    }
                    pair
                })
                .collect();
            rw.put_list(node, "values", values);
        }
        node
    }

    fn annotation_value(
        &mut self,
        value: &Value,
        rw: &mut ASTRewrite,
        context: &dyn ImportRewriteContext,
    ) -> Option<RNode> {
        Some(match value {
            Value::Missing => return None,
            Value::Boolean(value) => {
                let n = rw.new_node(NodeKind::BooleanLiteral);
                rw.put_simple(n, "booleanValue", if *value { "true" } else { "false" })
            }
            Value::Number(value) => rw.new_number_literal(value),
            Value::Character(value) => {
                let n = rw.new_node(NodeKind::CharacterLiteral);
                let escaped = char::from_u32(*value as u32)
                    .map(crate::javadoc::access::escaped_character_literal)
                    .unwrap_or_else(|| format!("'\\u{value:04x}'"));
                rw.put_simple(n, "escapedValue", &escaped)
            }
            Value::String(value) => {
                let n = rw.new_node(NodeKind::StringLiteral);
                rw.put_simple(
                    n,
                    "escapedValue",
                    &crate::javadoc::access::escaped_string_literal(value),
                )
            }
            Value::Type(id) => {
                let ast = self.ast.clone();
                let typ = self.add_import_type(ast.binding(*id), rw, context, TypeLocation::Other);
                let node = rw.new_node(NodeKind::TypeLiteral);
                rw.put_child(node, "type", typ)
            }
            Value::Enum(id) => {
                let ast = self.ast.clone();
                let binding = ast.binding(*id);
                let name = rw.new_simple_name(binding.name());
                let expression = binding
                    .var_type()
                    .map(|t| self.add_import_type(t, rw, context, TypeLocation::Other));
                let expression = expression
                    .filter(|t| rw.kind(*t) == NodeKind::SimpleType)
                    .and_then(|t| rw.new_value(t, "name").node())
                    .unwrap_or_else(|| rw.new_name("invalid"));
                rw.new_field_access(expression, name)
            }
            Value::Annotation(annotation) => self.add_annotation(annotation, rw, context),
            Value::Array(values) if values.len() == 1 => {
                return self.annotation_value(&values[0], rw, context)
            }
            Value::Array(values) => {
                let expressions = values
                    .iter()
                    .filter_map(|v| self.annotation_value(v, rw, context))
                    .collect();
                let node = rw.new_node(NodeKind::ArrayInitializer);
                rw.put_list(node, "expressions", expressions)
            }
        })
    }
}
