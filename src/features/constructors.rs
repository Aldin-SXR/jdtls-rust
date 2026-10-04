//! GenerateConstructorsHandler and AddCustomConstructorOperation in Rust.
//! The bridge supplies constructor bindings and parameter names, never edits.
pub(crate) mod actions;
use super::accessors::{self, Selection};
use super::java_model::{FieldDecl, Member};
use crate::analysis::dispatcher::Dispatcher;
use crate::correction::{edit::Env, Context, CuChange};
use crate::rewrite::import_rewrite::{
    ImportRewrite, ImportRewriteContext, KIND_TYPE, RES_NAME_CONFLICT, RES_NAME_FOUND,
};
use crate::rewrite::text_edit::EditTree;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{bflag, finder::NodeFinder, modifier, Ast, BindingRef, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use tower_lsp::lsp_types::{CodeActionParams, Range, WorkspaceEdit};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspMethodBinding {
    #[serde(default)]
    pub binding_key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parameters: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspVariableBinding {
    #[serde(default)]
    pub binding_key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub is_field: bool,
    #[serde(default)]
    pub is_selected: bool,
}
#[derive(Default, Serialize)]
pub struct CheckConstructorsResponse {
    pub constructors: Vec<LspMethodBinding>,
    pub fields: Vec<LspVariableBinding>,
}
#[derive(Deserialize)]
pub struct GenerateConstructorsParams {
    pub context: CodeActionParams,
    pub constructors: Option<Vec<LspMethodBinding>>,
    pub fields: Option<Vec<LspVariableBinding>>,
}

pub async fn check(d: &Dispatcher, params: CodeActionParams) -> CheckConstructorsResponse {
    let Ok(ast) = crate::semantic_ast::fetch(d, &params.text_document.uri).await else {
        return CheckConstructorsResponse::default();
    };
    let context = accessors::context(ast, &params);
    let Some(selected) = accessors::selection(&context, &params.text_document.uri) else {
        return CheckConstructorsResponse::default();
    };
    status(&context, &selected)
}
fn selected_binding<'a>(ast: &'a Ast, selected: &Selection) -> Option<BindingRef<'a>> {
    ast.node(selected.declaration).binding()
}
pub(crate) fn visible_constructors<'a>(
    ast: &'a Ast,
    binding: BindingRef<'a>,
) -> Vec<BindingRef<'a>> {
    let object = || {
        ast.binding_by_key("Ljava/lang/Object;")
            .map(|b| b.constructors())
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.name() == "Object" && m.parameter_types().is_empty())
    };
    if binding.is_enum() {
        return object().into_iter().collect();
    }
    let Some(parent) = binding.superclass() else {
        return Vec::new();
    };
    let visible: Vec<_> = parent
        .constructors()
        .into_iter()
        .filter(|c| {
            let modifiers = c.modifiers();
            modifiers & modifier::PUBLIC != 0
                || modifiers & modifier::PROTECTED != 0
                || modifiers & modifier::PRIVATE == 0
                    && c.declaring_class().and_then(|t| t.package_name()) == binding.package_name()
        })
        .collect();
    if visible.is_empty() {
        object().into_iter().collect()
    } else {
        visible
    }
}
pub(crate) fn status(context: &Context, selected: &Selection) -> CheckConstructorsResponse {
    let ast = &context.ast;
    let Some(binding) = selected_binding(ast, selected) else {
        return CheckConstructorsResponse::default();
    };
    let covered = accessors::actions::fully_covered_context(context);
    let nodes = if covered.is_empty() {
        context.covering_node().into_iter().collect()
    } else {
        covered
    };
    let names: Vec<_> = nodes
        .into_iter()
        .flat_map(accessors::actions::field_names)
        .collect();
    let bindings = binding.declared_fields().unwrap_or_default();
    let fields = selected
        .model
        .members
        .iter()
        .filter_map(|m| {
            let Member::Field(f) = m else {
                return None;
            };
            // IType.getFields() excludes record components, which the shared
            // Java model exposes for accessor generation.
            if f.record_component {
                return None;
            }
            let binding = bindings.iter().find(|b| b.name() == f.name)?;
            if binding.has(bflag::SYNTHETIC) || binding.is_static() {
                return None;
            }
            if binding.modifiers() & modifier::FINAL != 0
                && binding.declaring_node().is_some_and(|n| {
                    n.is(NodeKind::VariableDeclarationFragment) && n.child("initializer").is_some()
                })
            {
                return None;
            }
            Some(LspVariableBinding {
                binding_key: binding.key().into(),
                name: binding.name().into(),
                type_name: binding
                    .var_type()
                    .map(|t| t.name().into())
                    .unwrap_or_default(),
                is_field: binding.is_field(),
                is_selected: names.iter().any(|n| n == binding.name()),
            })
        })
        .collect();
    let constructors = visible_constructors(ast, binding)
        .iter()
        .map(|c| LspMethodBinding {
            binding_key: c.key().into(),
            name: c.name().into(),
            parameters: c
                .parameter_types()
                .iter()
                .map(|t| t.name().into())
                .collect(),
        })
        .collect();
    CheckConstructorsResponse {
        constructors,
        fields,
    }
}

pub async fn generate(env: &Env<'_>, params: GenerateConstructorsParams) -> Option<WorkspaceEdit> {
    let constructors = params.constructors?;
    if constructors.is_empty() {
        return None;
    }
    let fields = params.fields?;
    let uri = &params.context.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let context = accessors::context(ast.clone(), &params.context);
    let selected = accessors::selection(&context, uri)?;
    let mut change = create_change(
        env,
        ast.clone(),
        &selected,
        &constructors,
        &fields,
        Some(params.context.range),
    )
    .await
    .ok()?;
    let tree = crate::correction::edit::cu_tree(env, &mut change)
        .await
        .ok()?;
    let mut edits = crate::correction::edit::tree_to_text_edits(&ast.source, &tree);
    if edits.is_empty() {
        // TextEditConverter visits an empty MultiTextEdit as a no-op edit.
        edits.push(tower_lsp::lsp_types::TextEdit::new(
            Range::default(),
            String::new(),
        ));
    }
    Some(WorkspaceEdit {
        changes: Some([(uri.clone(), edits)].into()),
        ..Default::default()
    })
}

pub(crate) async fn create_change(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    constructors: &[LspMethodBinding],
    fields: &[LspVariableBinding],
    cursor: Option<Range>,
) -> anyhow::Result<CuChange> {
    let Some(binding) = selected_binding(&ast, selected) else {
        anyhow::bail!("No constructor type binding");
    };
    let available = visible_constructors(&ast, binding);
    let field_bindings = binding.declared_fields().unwrap_or_default();
    let fields: Vec<_> = fields
        .iter()
        .filter_map(|f| {
            field_bindings
                .iter()
                .find(|b| b.key() == f.binding_key)
                .copied()
        })
        .collect();
    let options = env.options(&ast.uri).await;
    let before = insertion(&ast, selected, cursor);
    let profile =
        accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&ast.uri)?).await;
    let comments = super::preferences::generate_comments();
    let mut combined = EditTree::new();
    // The upstream operation creates each rewrite against the original AST.
    for wanted in constructors {
        let Some(constructor) = available
            .iter()
            .find(|c| {
                c.parameter_types()
                    .iter()
                    .map(|t| t.name().to_owned())
                    .collect::<Vec<_>>()
                    == wanted.parameters
            })
            .copied()
        else {
            continue;
        };
        let mut rewrite = ASTRewrite::new(ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
        let method = constructor_stub(
            &mut rewrite,
            &mut imports,
            selected,
            binding,
            constructor,
            &fields,
            &options,
            comments,
            &profile,
        )?;
        if let Some(before) = before {
            rewrite.list_insert_before(
                RNode::Orig(selected.declaration),
                "bodyDeclarations",
                method,
                RNode::Orig(before),
            );
        } else {
            rewrite.list_insert_last(
                RNode::Orig(selected.declaration),
                "bodyDeclarations",
                method,
            );
        }
        let mut change = CuChange::rewrite(rewrite).with_imports(imports);
        let tree = crate::correction::edit::cu_tree(env, &mut change).await?;
        combined.add_tree(&tree).map_err(|e| anyhow::anyhow!(e.0))?;
    }
    Ok(CuChange::edits(ast, combined))
}
fn insertion(ast: &Ast, selected: &Selection, cursor: Option<Range>) -> Option<NodeId> {
    let cursor = cursor?;
    let start = ast.offset_of(cursor.start)?;
    let end = ast.offset_of(cursor.end)?;
    if accessors::declaration_node(NodeFinder::perform(
        ast.root(),
        start,
        end.saturating_sub(start),
    ))
    .is_none()
    {
        return accessors::insert_before(ast, selected, Some(cursor));
    }
    // A type-header selection inserts after the last source field, regardless
    // of insertionLocation, as CodeGenerationUtils does for constructors.
    let last = selected
        .model
        .members
        .iter()
        .filter_map(|m| match m {
            Member::Field(f) => Some(f.source.1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let offset = ast.text()[..last].encode_utf16().count();
    ast.node(selected.declaration)
        .list("bodyDeclarations")
        .into_iter()
        .find(|n| n.start() >= offset)
        .map(|n| n.id)
}
fn imported_type(
    rewrite: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    binding: BindingRef<'_>,
    context: &ConstructorImportContext,
) -> RNode {
    let label = imports.add_import_binding(binding, context);
    rewrite.create_string_placeholder(&label, NodeKind::SimpleType)
}
/// ContextSensitiveImportRewriteContext's source type scope. Bindings remain
/// data; Rust determines whether a simple imported name is shadowed.
struct ConstructorImportContext {
    ast: Arc<Ast>,
    declaration: Option<NodeId>,
}
impl ImportRewriteContext for ConstructorImportContext {
    fn find_in_context(
        &self,
        imports: &ImportRewrite,
        qualifier: &str,
        name: &str,
        kind: i32,
    ) -> i32 {
        if kind == KIND_TYPE {
            let qualified = format!("{qualifier}.{name}");
            let mut types: Vec<_> = self
                .ast
                .root()
                .list("types")
                .into_iter()
                .filter_map(|n| n.binding())
                .collect();
            if let Some(declaration) = self.declaration {
                let mut node = Some(self.ast.node(declaration));
                while let Some(n) = node {
                    if n.kind().is_abstract_type_declaration() {
                        if let Some(binding) = n.binding() {
                            types.push(binding);
                            types.extend(binding.declared_types().unwrap_or_default());
                        }
                    }
                    node = n.parent();
                }
            }
            for binding in types {
                if binding.name() == name {
                    return if binding.qualified_name() == qualified {
                        RES_NAME_FOUND
                    } else {
                        RES_NAME_CONFLICT
                    };
                }
            }
        }
        imports.find_in_imports(qualifier, name, kind)
    }
}
fn parameter(rewrite: &mut ASTRewrite, typ: RNode, name: &str, varargs: bool) -> RNode {
    let parameter = rewrite.new_node(NodeKind::SingleVariableDeclaration);
    rewrite.put_child(parameter, "type", typ);
    let name = rewrite.new_simple_name(name);
    rewrite.put_child(parameter, "name", name);
    rewrite.put_simple(parameter, "varargs", if varargs { "true" } else { "false" });
    parameter
}
fn constructor_stub(
    rewrite: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    selected: &Selection,
    binding: BindingRef<'_>,
    super_constructor: BindingRef<'_>,
    fields: &[BindingRef<'_>],
    options: &BTreeMap<String, String>,
    comments: bool,
    profile: &accessors::templates::Profile,
) -> anyhow::Result<RNode> {
    let context = ConstructorImportContext {
        ast: rewrite.ast.clone(),
        declaration: Some(selected.declaration),
    };
    let method = rewrite.new_node(NodeKind::MethodDeclaration);
    rewrite.put_simple(method, "constructor", "true");
    // The 1.58.0 manipulation library emits an ordinary declaration even for
    // records. Later JDT versions set compactConstructor here.
    let name = rewrite.new_simple_name(binding.name());
    rewrite.put_child(method, "name", name);
    let modifier = rewrite.new_modifier(if binding.is_enum() {
        "private"
    } else {
        "public"
    });
    rewrite.put_list(method, "modifiers", vec![modifier]);
    let mut parameters = Vec::new();
    let mut names = Vec::new();
    let mut body = Vec::new();
    let mut type_parameters = Vec::new();
    let mut thrown = Vec::new();
    let types = super_constructor.parameter_types();
    if !types.is_empty() {
        for t in super_constructor.type_parameters() {
            let node = rewrite.new_node(NodeKind::TypeParameter);
            let name = rewrite.new_simple_name(t.name());
            rewrite.put_child(node, "name", name);
            let type_bounds = t.type_bounds();
            let implicit_object =
                type_bounds.len() == 1 && type_bounds[0].qualified_name() == "java.lang.Object";
            let bounds = type_bounds
                .into_iter()
                .filter(|_| !implicit_object)
                .map(|b| imported_type(rewrite, imports, b, &context))
                .collect();
            rewrite.put_list(node, "typeBounds", bounds);
            type_parameters.push(node);
        }
        for (i, typ) in types.iter().enumerate() {
            let raw = super_constructor
                .data()
                .parameter_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("arg{i}"));
            let name = accessors::naming::method_argument(&raw, options, &names);
            names.push(name.clone());
            let varargs = super_constructor.is_varargs() && i == types.len() - 1 && typ.is_array();
            let typ = if varargs {
                typ.component_type().unwrap_or(*typ)
            } else {
                *typ
            };
            let typ = imported_type(rewrite, imports, typ, &context);
            parameters.push(parameter(rewrite, typ, &name, varargs));
        }
        thrown = super_constructor
            .exception_types()
            .into_iter()
            .map(|t| imported_type(rewrite, imports, t, &context))
            .collect();
        let invocation = rewrite.new_node(NodeKind::SuperConstructorInvocation);
        let arguments = names.iter().map(|n| rewrite.new_simple_name(n)).collect();
        rewrite.put_list(invocation, "arguments", arguments);
        body.push(invocation);
    }
    for binding in fields {
        let source_field = selected.model.members.iter().find_map(|m| match m {
            Member::Field(f) if f.name == binding.name() => Some(f),
            _ => None,
        });
        let fallback = FieldDecl {
            name: binding.name().into(),
            name_range: (0, 0),
            source: (0, 0),
            flags: binding.modifiers() as u32,
            enum_constant: false,
            record_component: false,
            type_label: None,
            type_signature: None,
            children: Vec::new(),
        };
        let name = accessors::naming::constructor_argument(
            source_field.unwrap_or(&fallback),
            options,
            &names,
        );
        names.push(name.clone());
        let typ = binding
            .var_type()
            .ok_or_else(|| anyhow::anyhow!("No field type"))?;
        let typ = imported_type(rewrite, imports, typ, &context);
        parameters.push(parameter(rewrite, typ, &name, false));
        let field_name = rewrite.new_simple_name(binding.name());
        let lhs = if binding.name() == name {
            let this = rewrite.new_this_expression();
            rewrite.new_field_access(this, field_name)
        } else {
            field_name
        };
        let rhs = rewrite.new_simple_name(&name);
        let assignment = rewrite.new_assignment(lhs, "=", rhs);
        body.push(rewrite.new_expression_statement(assignment));
    }
    rewrite.put_list(method, "typeParameters", type_parameters);
    rewrite.put_list(method, "parameters", parameters);
    rewrite.put_list(method, "thrownExceptionTypes", thrown);
    let block = rewrite.new_block(body);
    rewrite.put_child(method, "body", block);
    if comments {
        let markdown = profile.use_markdown
            && options
                .get("org.eclipse.jdt.core.compiler.compliance")
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|n| n >= 23);
        let pattern = if markdown {
            profile.template("markdownconstructorcomment", "")
        } else {
            profile.template("constructorcomment", "/**\n * ${tags}\n */")
        };
        let mut tags: Vec<_> = if types.is_empty() {
            Vec::new()
        } else {
            super_constructor
                .type_parameters()
                .iter()
                .map(|t| format!("@param <{}>", t.name()))
                .collect()
        };
        tags.extend(names.iter().map(|n| format!("@param {n}")));
        if !types.is_empty() {
            tags.extend(
                super_constructor
                    .exception_types()
                    .iter()
                    .map(|t| format!("@throws {}", t.name())),
            );
        }
        let file_name = tower_lsp::lsp_types::Url::parse(&rewrite.ast.uri)
            .ok()
            .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
            .unwrap_or_default();
        let package = rewrite
            .ast
            .root()
            .child("package")
            .and_then(|p| p.child("name"))
            .map(|n| n.identifier())
            .unwrap_or_default();
        let declaration = super_constructor
            .method_declaration()
            .unwrap_or(super_constructor);
        let is_deprecated = !types.is_empty() && declaration.is_deprecated();
        if is_deprecated {
            tags.push("@deprecated".into());
        }
        let see = if types.is_empty() {
            String::new()
        } else {
            let parameters = declaration
                .parameter_types()
                .into_iter()
                .map(|t| t.erasure().unwrap_or(t).qualified_name().to_owned())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "@see {}#{}({})",
                declaration
                    .declaring_class()
                    .map(|t| t.qualified_name())
                    .unwrap_or(""),
                declaration.name(),
                parameters
            )
        };
        let mut first_tags = None::<String>;
        let marker = "\u{e000}constructor_tags\u{e001}";
        let mut comment =
            accessors::templates::expand_named_template(pattern, |name, key| match key {
                "tags" => {
                    let first = first_tags.get_or_insert_with(|| name.to_owned());
                    Some(if first == name { marker } else { "@" })
                }
                "enclosing_type" => Some(binding.name()),
                "enclosing_method" => Some(binding.name()),
                "project_name" => Some(profile.project_name.as_str()),
                "file_name" => Some(file_name.as_str()),
                "package_name" => Some(package.as_str()),
                "dollar" => Some("$"),
                "see_to_overridden" if !types.is_empty() => Some(see.as_str()),
                _ => None,
            })?;
        // TemplateBuffer records offsets before insertion and processes them
        // backwards. A previous tag on the same line still contains its
        // unresolved "@" value when used as the continuation prefix.
        let offsets: Vec<_> = comment.match_indices(marker).map(|(i, _)| i).collect();
        for i in offsets.into_iter().rev() {
            let line_start = comment[..i].rfind('\n').map_or(0, |p| p + 1);
            let prefix = comment[line_start..i].replace(marker, "@");
            let inserted = tags.join(&format!("\n{prefix}"));
            comment.replace_range(i..i + marker.len(), &inserted);
        }
        if !comment.trim().is_empty() {
            let comment = rewrite.create_string_placeholder(&comment, NodeKind::Javadoc);
            rewrite.put_child(method, "javadoc", comment);
        }
    }
    Ok(method)
}
