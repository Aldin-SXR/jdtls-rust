//! `NewMethodCorrectionProposalCore` / `NewAbstractMethodCorrectionProposalCore`
//! over `AbstractMethodCorrectionProposalCore.getRewrite()`.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::names;
use super::scope;
use super::types::{self, normalize, normalize_wildcard, well_known};
use crate::correction::edit::Env;
use crate::correction::{CuChange, LazyChange};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeId, NodeKind};

/// A new method / constructor in the sender type.
pub struct NewMethod {
    /// The AST of the invocation.
    pub source: Arc<Ast>,
    /// `MethodInvocation`, `SuperMethodInvocation`, `ClassInstanceCreation`,
    /// `ConstructorInvocation`, `SuperConstructorInvocation` or
    /// `EnumConstantDeclaration`.
    pub invocation: NodeId,
    pub arguments: Vec<NodeId>,
    /// Key of the sender type declaration binding.
    pub sender: String,
    /// The target unit when it is not the invocation's unit.
    pub target_uri: Option<String>,
    /// `NewAbstractMethodCorrectionProposalCore`.
    pub is_abstract: bool,
}

impl NewMethod {
    fn is_constructor(&self) -> bool {
        !matches!(self.source.node(self.invocation).kind(), NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation)
    }
}

/// The ASTs of a change in another unit: the invocation AST plus the target.
pub async fn target_ast(env: &Env<'_>, source: &Arc<Ast>, target_uri: &Option<String>) -> anyhow::Result<Arc<Ast>> {
    match target_uri {
        None => Ok(source.clone()),
        Some(uri) => {
            let url = tower_lsp::lsp_types::Url::parse(uri)?;
            crate::semantic_ast::fetch(env.dispatcher, &url).await
        }
    }
}

#[tower_lsp::async_trait]
impl LazyChange for NewMethod {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let target = target_ast(env, &self.source, &self.target_uri).await?;
        let options = env.options(&target.uri).await;
        let profile = crate::features::accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&target.uri)?).await;
        let source = self.source.clone();
        let sender = source.binding_by_key(&self.sender).ok_or_else(|| anyhow::anyhow!("no sender binding"))?;
        let type_decl = target
            .binding_by_key(&self.sender)
            .and_then(|b| b.declaring_node())
            .ok_or_else(|| anyhow::anyhow!("no sender declaration"))?;
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        let context = ConstructorImportContext {
            ast: target.clone(),
            declaration: Some(type_decl.id),
            nullness: crate::rewrite::import_rewrite::nullness::Filter::create(&target, Some(type_decl.id), &options),
        };
        let mut rw = ASTRewrite::new(target.clone());
        let stub = self.stub(&mut rw, &mut imports, &context, sender, type_decl, &options, &profile)?;
        let members = type_decl.list("bodyDeclarations");
        let index = if self.is_constructor() {
            constructor_insert_index(&members)
        } else if self.target_uri.is_none() {
            method_insert_index(&members, source.node(self.invocation).start())
        } else {
            members.len()
        };
        rw.list_insert_at(RNode::Orig(type_decl.id), "bodyDeclarations", stub, index as i32);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}

fn method_insert_index(members: &[Node<'_>], pos: usize) -> usize {
    for (i, m) in members.iter().enumerate() {
        if m.is(NodeKind::MethodDeclaration) && pos < m.start() + m.length() {
            return i + 1;
        }
    }
    members.len()
}

fn constructor_insert_index(members: &[Node<'_>]) -> usize {
    let mut last_method = 0;
    for i in (0..members.len()).rev() {
        let m = members[i];
        if m.is(NodeKind::MethodDeclaration) {
            if m.flag("constructor") {
                return i + 1;
            }
            last_method = i;
        }
    }
    last_method
}

/// `JdtFlags.isAbstract(methodBinding)`.
fn is_abstract_method(m: BindingRef<'_>) -> bool {
    if m.modifiers() & modifier::ABSTRACT != 0 {
        return true;
    }
    m.declaring_class().is_some_and(|c| c.is_interface() || c.is_annotation())
        && m.modifiers() & (modifier::STATIC | modifier::DEFAULT | modifier::PRIVATE) == 0
}

/// `NewMethodCorrectionProposalCore.getInterfaceMethodModifiers`.
fn interface_method_modifiers(target: Node<'_>, create_abstract: bool) -> i32 {
    if !target.is(NodeKind::TypeDeclaration) {
        return 0;
    }
    let body = target.list("bodyDeclarations");
    let methods: Vec<_> = body.iter().filter(|b| b.is(NodeKind::MethodDeclaration)).collect();
    if !methods.is_empty() {
        if create_abstract {
            for m in &methods {
                if m.binding().is_some_and(is_abstract_method) {
                    return m.modifiers();
                }
            }
        }
        return methods[0].modifiers() & modifier::PUBLIC;
    }
    if let Some(first) = body.first() {
        return first.modifiers() & modifier::PUBLIC;
    }
    0
}

impl NewMethod {
    /// `NewMethodCorrectionProposalCore.evaluateModifiers`.
    fn evaluate_modifiers(&self, sender: BindingRef<'_>, target_decl: Node<'_>) -> i32 {
        if self.is_abstract {
            return modifier::ABSTRACT | modifier::PROTECTED;
        }
        if sender.is_annotation() || sender.is_enum() {
            return 0;
        }
        let target_interface = sender.is_interface();
        let invocation = self.source.node(self.invocation);
        if !invocation.is(NodeKind::MethodInvocation) {
            return modifier::PUBLIC;
        }
        let mut mods = 0;
        let expression = invocation.child("expression");
        match expression {
            Some(e) => {
                if matches!(e.kind(), NodeKind::SimpleName | NodeKind::QualifiedName) && e.binding().is_some_and(|b| b.is_type()) {
                    mods |= modifier::STATIC;
                }
            }
            None => {
                if scope::is_in_static_context(invocation) {
                    mods |= modifier::STATIC;
                }
            }
        }
        let node = crate::semantic_ast::resolve::find_parent_type(invocation);
        let same_decl = |n: Option<Node<'_>>| self.target_uri.is_none() && n.is_some_and(|n| n.id == target_decl.id);
        let parent_interface = node.is_some_and(|n| n.is(NodeKind::TypeDeclaration) && n.flag("interface"));
        if target_interface || parent_interface {
            if expression.is_none() && !same_decl(node) {
                mods |= modifier::STATIC;
                if target_interface {
                    mods |= interface_method_modifiers(target_decl, false);
                } else {
                    mods |= modifier::PROTECTED;
                }
            } else if mods == modifier::STATIC {
                mods = interface_method_modifiers(target_decl, false) | modifier::STATIC;
            } else {
                mods = interface_method_modifiers(target_decl, true);
            }
        } else if same_decl(node) {
            mods |= modifier::PRIVATE;
        } else if node.is_some_and(|n| {
            n.is(NodeKind::AnonymousClassDeclaration) && self.target_uri.is_none() && n.ancestors().any(|a| a.id == target_decl.id)
        }) {
            mods |= modifier::PROTECTED;
            if node.is_some_and(scope::is_in_static_context) && expression.is_none() {
                mods |= modifier::STATIC;
            }
        } else {
            mods |= modifier::PUBLIC;
        }
        mods
    }

    /// `fTypeMapping` + `getClassTypeParameterBinding`.
    fn class_type_parameter<'a>(&self, binding: BindingRef<'a>) -> BindingRef<'a> {
        let params = match binding.declaring_class() {
            Some(c) if c.is_generic_type() => c.type_parameters(),
            _ => binding.declaring_method().map(|m| m.type_parameters()).unwrap_or_default(),
        };
        if params.iter().any(|p| *p == binding) {
            if let Some(mapped) = self.type_mapping(binding) {
                return mapped;
            }
        }
        binding
    }

    fn type_mapping<'a>(&self, binding: BindingRef<'a>) -> Option<BindingRef<'a>> {
        let ast = binding.ast;
        let invocation = ast.node(self.invocation);
        if !invocation.is(NodeKind::MethodInvocation) {
            return None;
        }
        let expression_type = invocation.child("expression")?.type_binding()?;
        if !expression_type.is_parameterized_type() {
            return None;
        }
        let params = types::declaration(expression_type).type_parameters();
        let args = expression_type.type_arguments();
        args.iter().zip(params).find(|(a, _)| **a == binding).map(|(_, p)| p)
    }

    /// `NewMethodCorrectionProposalCore.getTypeParameters`.
    fn collect_type_parameters<'a>(&self, binding: BindingRef<'a>, found: &mut Vec<BindingRef<'a>>) {
        if self.class_type_parameter(binding) != binding {
            return;
        }
        if let Some(m) = binding.declaring_method() {
            for p in m.type_parameters() {
                if p == binding && !found.contains(&p) {
                    found.push(p);
                    for b in p.type_bounds() {
                        self.collect_type_parameters(b, found);
                    }
                }
            }
        } else if let Some(c) = binding.declaring_class() {
            let invocation = self.source.node(self.invocation);
            if invocation.is(NodeKind::MethodInvocation)
                && invocation.child("expression").is_some_and(|e| !e.is(NodeKind::ThisExpression))
            {
                for p in c.type_parameters() {
                    if p == binding && !found.contains(&p) {
                        found.push(p);
                        for b in p.type_bounds() {
                            self.collect_type_parameters(b, found);
                        }
                    }
                }
            }
        }
    }

    /// The guessed return type binding (`getNewMethodType` / `addNewTypeParameters`);
    /// `Err(true)` means `void` (expression statement).
    fn return_type_binding(&self) -> (Option<BindingRef<'_>>, bool) {
        let sender = self.source.binding_by_key(&self.sender);
        let node = self.source.node(self.invocation);
        if let Some(parent) = node.parent() {
            if parent.is(NodeKind::MethodInvocation) && node.location_is("expression") {
                let selector = parent.child("name").map(|n| n.identifier()).unwrap_or_default();
                let n_args = parent.list("arguments").len();
                let guesses = scope::qualifier_guess(node.root(), &selector, n_args, sender);
                if let Some(first) = guesses.first() {
                    return (Some(*first), true);
                }
            }
        }
        let mut binding = scope::guess_binding_for_reference(node);
        if let Some(b) = binding.filter(|b| b.is_wildcard_type()) {
            binding = normalize_wildcard(b, false);
        }
        (binding, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn stub(
        &self,
        rw: &mut ASTRewrite,
        imports: &mut ImportRewrite,
        context: &ConstructorImportContext,
        sender: BindingRef<'_>,
        target_decl: Node<'_>,
        options: &BTreeMap<String, String>,
        profile: &crate::features::accessors::templates::Profile,
    ) -> anyhow::Result<RNode> {
        let source = &*self.source;
        let invocation = source.node(self.invocation);
        let is_constructor = self.is_constructor();
        let decl = rw.new_node(NodeKind::MethodDeclaration);
        let name = match invocation.kind() {
            NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation => invocation.child("name").map(|n| n.identifier()).unwrap_or_default(),
            _ => sender.name().to_owned(),
        };
        rw.put_simple(decl, "constructor", if is_constructor { "true" } else { "false" });
        let mods = self.evaluate_modifiers(sender, target_decl);
        let modifiers = rw.new_modifiers(mods);
        rw.put_list(decl, "modifiers", modifiers);

        // addNewTypeParameters
        let mut taken: Vec<String> = Vec::new();
        let mut found = Vec::new();
        if !is_constructor {
            let (ret, _) = self.return_type_binding();
            if let Some(r) = ret {
                self.collect_type_parameters(r, &mut found);
            }
        }
        for arg in &self.arguments {
            let mut binding = normalize(source.node(*arg).type_binding());
            if let Some(b) = binding.filter(|b| b.is_wildcard_type()) {
                binding = normalize_wildcard(b, true);
            }
            if let Some(b) = binding {
                self.collect_type_parameters(b, &mut found);
            }
        }
        let mut type_params = Vec::new();
        for p in &found {
            let tp = rw.new_node(NodeKind::TypeParameter);
            let n = rw.new_simple_name(p.name());
            rw.put_child(tp, "name", n);
            let bounds: Vec<_> = p.type_bounds().into_iter().map(|b| imports.add_import_type(b, rw, context, TypeLocation::TypeParameter)).collect();
            rw.put_list(tp, "typeBounds", bounds);
            type_params.push(tp);
        }
        rw.put_list(decl, "typeParameters", type_params);
        let name_node = rw.new_simple_name(&name);
        rw.put_child(decl, "name", name_node);
        for f in sender.declared_fields().unwrap_or_default() {
            taken.push(f.name().to_owned());
        }

        let mut body_statement = String::new();
        let is_abstract_method = mods & modifier::ABSTRACT != 0
            || (sender.is_interface() && mods & (modifier::STATIC | modifier::DEFAULT) == 0);
        if !is_constructor {
            let (ret, guessed) = self.return_type_binding();
            let (return_type, default) = match ret {
                Some(b) => {
                    let b = if guessed { self.class_type_parameter(b) } else { self.class_type_parameter(b) };
                    let t = imports.add_import_type(b, rw, context, TypeLocation::ReturnType);
                    let default = if b.is_primitive() {
                        match b.name() {
                            "boolean" => Some("false"),
                            "void" => None,
                            _ => Some("0"),
                        }
                    } else {
                        Some("null")
                    };
                    (t, default)
                }
                None => {
                    let node = source.node(self.invocation);
                    if node.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement)) {
                        (rw.new_primitive_type("void"), None)
                    } else if let Some((element, dims)) = scope::guess_array_reference(node) {
                        let element = imports.add_import_type(element, rw, context, TypeLocation::ReturnType);
                        let array = rw.new_node(NodeKind::ArrayType);
                        rw.put_child(array, "elementType", element);
                        let dimensions = (0..dims)
                            .map(|_| {
                                let d = rw.new_node(NodeKind::Dimension);
                                rw.put_list(d, "annotations", Vec::new())
                            })
                            .collect();
                        rw.put_list(array, "dimensions", dimensions);
                        (array, Some("null"))
                    } else {
                        match guess_type_for_reference(rw, node) {
                            Some((t, primitive)) => (t, Some(match primitive.as_deref() {
                                Some("boolean") => "false",
                                Some(_) => "0",
                                None => "null",
                            })),
                            None => {
                                let n = rw.new_simple_name("Object");
                                (rw.new_simple_type(n), Some("null"))
                            }
                        }
                    }
                }
            };
            rw.put_child(decl, "returnType2", return_type);
            if !is_abstract_method {
                if let Some(d) = default {
                    body_statement = format!("return {d};");
                }
            }
        }

        // addNewParameters
        let mut params = Vec::new();
        for arg in &self.arguments {
            let arg = source.node(*arg);
            let mut binding = normalize(arg.type_binding());
            if let Some(b) = binding.filter(|b| b.is_wildcard_type()) {
                binding = normalize_wildcard(b, true);
            }
            let (typ, type_binding) = match binding {
                Some(b) => {
                    let b = self.class_type_parameter(b);
                    (imports.add_import_type(b, rw, context, TypeLocation::Parameter), Some(b))
                }
                None => {
                    let n = rw.new_simple_name("Object");
                    (rw.new_simple_type(n), None)
                }
            };
            let type_name = match type_binding {
                Some(b) => names::type_base_name(b),
                None => Some(("Object".to_owned(), 0)),
            };
            let name = names::parameter_name(arg, type_name, &taken, options);
            taken.push(name.clone());
            let param = rw.new_node(NodeKind::SingleVariableDeclaration);
            rw.put_child(param, "type", typ);
            let n = rw.new_simple_name(&name);
            rw.put_child(param, "name", n);
            rw.put_simple(param, "varargs", "false");
            params.push(param);
        }
        rw.put_list(decl, "parameters", params);
        rw.put_list(decl, "thrownExceptionTypes", Vec::new());

        if !is_abstract_method && mods & modifier::ABSTRACT == 0 {
            let placeholder = method_body_content(profile, options, is_constructor, sender.name(), &name, &body_statement)?;
            let statements = match placeholder {
                Some(p) => vec![rw.create_string_placeholder(&p, NodeKind::ReturnStatement)],
                None => Vec::new(),
            };
            let block = rw.new_block(statements);
            rw.put_child(decl, "body", block);
        }
        let _ = well_known;
        Ok(decl)
    }
}

/// `ASTResolving.guessTypeForReference(ast, node)`: a copy of a declared
/// variable type (the primitive code, if any, decides the default value).
pub(super) fn guess_type_for_reference(rw: &mut ASTRewrite, node: Node<'_>) -> Option<(RNode, Option<String>)> {
    let mut node = node;
    let mut parent = node.parent();
    while let Some(p) = parent {
        match p.kind() {
            NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration => {
                if !node.location_is("initializer") {
                    return None;
                }
                let typ = if p.is(NodeKind::SingleVariableDeclaration) {
                    p.child("type")
                } else {
                    p.parent().and_then(|d| d.child("type"))
                }?;
                let dims = p.list("extraDimensions2").len();
                let mut text = typ.source_text();
                for _ in 0..dims {
                    text.push_str("[]");
                }
                let primitive = (typ.is(NodeKind::PrimitiveType) && dims == 0).then(|| typ.simple("primitiveTypeCode").unwrap_or("").to_owned());
                let kind = typ.kind();
                return Some((rw.create_string_placeholder(&text, kind), primitive));
            }
            NodeKind::FieldAccess | NodeKind::QualifiedName => {
                if !node.location_is("name") {
                    return None;
                }
                node = p;
                parent = p.parent();
            }
            NodeKind::SuperFieldAccess | NodeKind::ParenthesizedExpression => {
                node = p;
                parent = p.parent();
            }
            _ => return None,
        }
    }
    None
}

/// `CodeGeneration.getMethodBodyContent` (`StubUtility.getMethodBodyContent`).
pub fn method_body_content(
    profile: &crate::features::accessors::templates::Profile,
    options: &BTreeMap<String, String>,
    is_constructor: bool,
    type_name: &str,
    method_name: &str,
    body_statement: &str,
) -> anyhow::Result<Option<String>> {
    let template = if is_constructor {
        profile.template("constructorbody", "${body_statement}\n//${todo} Auto-generated constructor stub")
    } else {
        profile.template(
            "methodbody",
            "// ${todo} Auto-generated method stub\nthrow new UnsupportedOperationException(\"Unimplemented method '${enclosing_method}'\");",
        )
    };
    let todo = options
        .get("org.eclipse.jdt.core.compiler.taskTags")
        .and_then(|s| s.split(',').next())
        .unwrap_or("TODO")
        .to_owned();
    let result = crate::features::accessors::templates::expand_template(template, |key| match key {
        "todo" => Some(todo.as_str()),
        "body_statement" => Some(body_statement),
        "enclosing_type" => Some(type_name),
        "enclosing_method" => Some(method_name),
        "dollar" => Some("$"),
        _ => None,
    })?;
    if result.trim().is_empty() {
        if !body_statement.trim().is_empty() {
            return Ok(Some(body_statement.to_owned()));
        }
        return Ok(None);
    }
    Ok(Some(result))
}
