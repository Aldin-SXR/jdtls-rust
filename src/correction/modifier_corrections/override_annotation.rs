//! `getRemoveOverrideAnnotationProposal` with
//! `QuickAssistProcessorUtil.getCreateInSuperClassProposals` and
//! `NewDefiningMethodProposalCore`.

use std::sync::Arc;

use super::visibility::{find_overridden_method_in_type, Units};
use crate::correction::edit::Env;
use crate::correction::{kind, messages, relevance, Change, Context, CuChange, LazyChange, ProblemLocation, Proposal, ProposalType};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, BindingRef, Node, NodeKind};

/// `getRemoveOverrideAnnotationProposal(context, problem, proposals)`.
pub async fn remove_override_annotation(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(decl) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    // StubUtility2Core.findAnnotation("java.lang.Override", modifiers)
    let annotation = decl.list("modifiers").into_iter().find(|m| {
        m.kind().is_annotation()
            && m.type_binding()
                .or_else(|| m.child("typeName").and_then(|n| n.binding()))
                .is_some_and(|t| t.qualified_name() == "java.lang.Override")
    });
    let Some(annotation) = annotation else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.remove(RNode::Orig(annotation.id));
    let label = messages::correction("ModifierCorrectionSubProcessor_remove_override").to_owned();
    proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::REMOVE_OVERRIDE, rw));
    if let Some(name) = decl.child("name") {
        create_in_super_class(env, ctx, name, proposals).await;
    }
}

/// `JavadocTagsSubProcessorCore.getTagRanking(tagName)`.
fn tag_ranking(tag: &str) -> usize {
    let tag = if tag == "@exception" { "@throws" } else { tag };
    const TAG_ORDER: [&str; 9] = ["@author", "@version", "@param", "@return", "@throws", "@see", "@since", "@serial", "@deprecated"];
    TAG_ORDER.iter().position(|t| *t == tag).unwrap_or(TAG_ORDER.len())
}

/// `getOverridingDeprecatedMethodProposal(context, problem, proposals)`.
pub fn overriding_deprecated_method(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(decl) = problem.covering_node(ctx.ast()).filter(|n| n.is(NodeKind::MethodDeclaration)) else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let annotation = rw.new_node(NodeKind::MarkerAnnotation);
    let name = rw.new_name("Deprecated");
    rw.put_child(annotation, "typeName", name);
    rw.list_insert_first(RNode::Orig(decl.id), "modifiers", annotation);
    if let Some(javadoc) = decl.child("javadoc") {
        let tag = rw.new_node(NodeKind::TagElement);
        rw.put_simple(tag, "tagName", "@deprecated");
        // JavadocTagsSubProcessorCore.insertTag(tags, newTag, null)
        let ranking = tag_ranking("@deprecated");
        let after = javadoc
            .list("tags")
            .into_iter()
            .rev()
            .find(|t| t.simple("tagName").is_none_or(|n| ranking > tag_ranking(n)));
        match after {
            Some(after) => rw.list_insert_after(RNode::Orig(javadoc.id), "tags", tag, RNode::Orig(after.id)),
            None => rw.list_insert_first(RNode::Orig(javadoc.id), "tags", tag),
        }
    }
    let label = messages::correction("ModifierCorrectionSubProcessor_overrides_deprecated_description").to_owned();
    proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::OVERRIDES_DEPRECATED, rw));
}

/// `Bindings.getAllSuperTypes(type)`.
fn all_super_types(t: BindingRef<'_>) -> Vec<BindingRef<'_>> {
    fn visit<'a>(t: BindingRef<'a>, out: &mut Vec<BindingRef<'a>>, root: &str) {
        if t.key() != root {
            if out.iter().any(|o| o.key() == t.key()) {
                return;
            }
            out.push(t);
        }
        if let Some(s) = t.superclass() {
            visit(s, out, root);
        }
        for i in t.interfaces() {
            visit(i, out, root);
        }
    }
    let mut out = Vec::new();
    visit(t, &mut out, t.key());
    out
}

/// `QuickAssistProcessorUtil.getCreateInSuperClassProposals(context, node, proposals)`.
pub async fn create_in_super_class(env: &Env<'_>, ctx: &Context, node: Node<'_>, proposals: &mut Vec<Proposal>) -> bool {
    if !node.is(NodeKind::SimpleName) {
        return false;
    }
    let Some(decl) = node.parent().filter(|p| p.is(NodeKind::MethodDeclaration)) else { return false };
    if !node.location_is("name") || decl.modifiers() & modifier::PRIVATE != 0 {
        return false;
    }
    let Some(binding) = decl.binding() else { return false };
    let Some(declaring) = binding.declaring_class() else { return false };
    let param_names: Vec<String> = decl.list("parameters").iter().map(|p| p.child("name").map(|n| n.identifier()).unwrap_or_default()).collect();
    let ast = ctx.ast();
    let mut units: Option<Units> = None;
    for curr in all_super_types(declaring) {
        if !curr.is_from_source() || find_overridden_method_in_type(curr, binding).is_some() {
            continue;
        }
        let type_decl = curr.type_declaration().unwrap_or(curr);
        if units.is_none() {
            units = Some(Units::load(env, &ast.uri).await);
        }
        let Some(target_uri) = units.as_ref().and_then(|u| u.find(ast, type_decl)) else { continue };
        let label = messages::format(messages::correction("QuickAssistProcessor_createmethodinsuper_description"), &[curr.name(), binding.name()]);
        let change = NewDefiningMethod {
            source: ctx.ast.clone(),
            target_uri,
            sender: type_decl.key().to_owned(),
            method: binding.key().to_owned(),
            param_names: param_names.clone(),
        };
        let mut p = Proposal::new(label, kind::QUICK_FIX, relevance::CREATE_METHOD_IN_SUPER, Change::Lazy(Box::new(change)));
        p.proposal_type = ProposalType::NewElement;
        proposals.push(p);
    }
    true
}

/// `NewDefiningMethodProposalCore`: a declaration of `method` in the super
/// type `sender`.
struct NewDefiningMethod {
    source: Arc<Ast>,
    target_uri: Option<String>,
    sender: String,
    method: String,
    param_names: Vec<String>,
}

impl NewDefiningMethod {
    #[allow(clippy::too_many_arguments)]
    fn stub(
        &self,
        rw: &mut ASTRewrite,
        imports: &mut ImportRewrite,
        context: &ConstructorImportContext,
        sender: BindingRef<'_>,
        method: BindingRef<'_>,
        options: &std::collections::BTreeMap<String, String>,
        profile: &crate::features::accessors::templates::Profile,
    ) -> anyhow::Result<RNode> {
        let decl = rw.new_node(NodeKind::MethodDeclaration);
        let is_constructor = method.is_constructor();
        rw.put_simple(decl, "constructor", if is_constructor { "true" } else { "false" });
        // evaluateModifiers
        let mods = if sender.is_interface() {
            0
        } else {
            let mut m = method.modifiers();
            if m & modifier::PRIVATE != 0 {
                m |= modifier::PROTECTED;
            }
            m & (modifier::PUBLIC | modifier::PROTECTED | modifier::ABSTRACT | modifier::STRICTFP)
        };
        let modifiers = rw.new_modifiers(mods);
        rw.put_list(decl, "modifiers", modifiers);
        // addNewTypeParameters
        let mut type_params = Vec::new();
        for current in method.type_parameters() {
            let tp = rw.new_node(NodeKind::TypeParameter);
            let n = rw.new_simple_name(current.name());
            rw.put_child(tp, "name", n);
            let bounds = current.type_bounds();
            let bounds = if bounds.len() != 1 || bounds[0].qualified_name() != "java.lang.Object" {
                bounds.into_iter().map(|b| imports.add_import_type(b, rw, context, TypeLocation::TypeBound)).collect()
            } else {
                Vec::new()
            };
            rw.put_list(tp, "typeBounds", bounds);
            type_params.push(tp);
        }
        rw.put_list(decl, "typeParameters", type_params);
        let name = method.name().to_owned();
        let name_node = rw.new_simple_name(&name);
        rw.put_child(decl, "name", name_node);

        let mut body_statement = String::new();
        let is_abstract_method = mods & modifier::ABSTRACT != 0 || (sender.is_interface() && mods & (modifier::STATIC | modifier::DEFAULT) == 0);
        if !is_constructor {
            if let Some(ret) = method.return_type() {
                let typ = imports.add_import_type(ret, rw, context, TypeLocation::ReturnType);
                rw.put_child(decl, "returnType2", typ);
                let is_void = ret.is_primitive() && ret.name() == "void";
                if !is_abstract_method && !is_void {
                    let default = if ret.is_primitive() {
                        if ret.name() == "boolean" {
                            "false"
                        } else {
                            "0"
                        }
                    } else {
                        "null"
                    };
                    body_statement = format!("return {default};");
                }
            }
        }
        // addNewParameters
        let names = suggest_argument_names(&self.param_names, options);
        let mut params = Vec::new();
        let parameter_types = method.parameter_types();
        for (i, curr) in parameter_types.iter().enumerate() {
            let param = rw.new_node(NodeKind::SingleVariableDeclaration);
            let mut typ = imports.add_import_type(*curr, rw, context, TypeLocation::Parameter);
            let varargs = method.is_varargs() && i + 1 == parameter_types.len();
            if varargs && rw.kind(typ) == NodeKind::ArrayType {
                // remove last dimension added by vararg conversion
                let dims = rw.new_value(typ, "dimensions").list();
                if dims.len() > 1 {
                    let kept = dims[..dims.len() - 1].to_vec();
                    rw.put_list(typ, "dimensions", kept);
                } else if let Some(element) = rw.new_value(typ, "elementType").node() {
                    typ = element;
                }
            }
            rw.put_child(param, "type", typ);
            let n = rw.new_simple_name(names.get(i).map(String::as_str).unwrap_or("arg"));
            rw.put_child(param, "name", n);
            rw.put_simple(param, "varargs", if varargs { "true" } else { "false" });
            params.push(param);
        }
        rw.put_list(decl, "parameters", params);
        // addNewExceptions
        let exceptions = method.exception_types().into_iter().map(|e| imports.add_import_type(e, rw, context, TypeLocation::Exception)).collect();
        rw.put_list(decl, "thrownExceptionTypes", exceptions);
        // addNewJavaDoc: the javadoc of the overriding declaration
        if let Some(javadoc) = method.declaring_node().and_then(|d| d.child("javadoc")) {
            let text = javadoc.source_text();
            let doc = rw.create_string_placeholder(&text, NodeKind::Javadoc);
            rw.put_child(decl, "javadoc", doc);
        }
        if !is_abstract_method && mods & modifier::ABSTRACT == 0 {
            let placeholder = crate::correction::unresolved_elements::method_body_content(profile, options, is_constructor, sender.name(), &name, &body_statement)?;
            let statements = match placeholder {
                Some(p) => vec![rw.create_string_placeholder(&p, NodeKind::ReturnStatement)],
                None => Vec::new(),
            };
            let block = rw.new_block(statements);
            rw.put_child(decl, "body", block);
        }
        Ok(decl)
    }
}

/// `StubUtility.suggestArgumentNamesWithProposals(project, paramNames)`: the
/// first proposal of each name.
fn suggest_argument_names(param_names: &[String], options: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    let mut taken: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for curr in param_names {
        let proposed = crate::correction::unresolved_elements::variable_name_suggestions(curr, &taken, options);
        let first = proposed.into_iter().next().unwrap_or_else(|| curr.clone());
        taken.push(first.clone());
        out.push(first);
    }
    out
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
        if members[i].is(NodeKind::MethodDeclaration) {
            if members[i].flag("constructor") {
                return i + 1;
            }
            last_method = i;
        }
    }
    last_method
}

#[tower_lsp::async_trait]
impl LazyChange for NewDefiningMethod {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let source = self.source.clone();
        let in_source = source.binding_by_key(&self.sender).and_then(|b| b.declaring_node()).is_some();
        let target = match (&self.target_uri, in_source) {
            (_, true) | (None, _) => source.clone(),
            (Some(uri), false) => crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(uri)?).await?,
        };
        let type_decl = target
            .binding_by_key(&self.sender)
            .and_then(|b| b.declaring_node())
            .ok_or_else(|| anyhow::anyhow!("no sender declaration"))?;
        let sender = source.binding_by_key(&self.sender).ok_or_else(|| anyhow::anyhow!("no sender binding"))?;
        let method = source.binding_by_key(&self.method).ok_or_else(|| anyhow::anyhow!("no method binding"))?;
        let options = env.options(&target.uri).await;
        let profile = crate::features::accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&target.uri)?).await;
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        let context = ConstructorImportContext { ast: target.clone(), declaration: Some(type_decl.id), nullness: None };
        let mut rw = ASTRewrite::new(target.clone());
        let stub = self.stub(&mut rw, &mut imports, &context, sender, method, &options, &profile)?;
        let members = type_decl.list("bodyDeclarations");
        let index = if method.is_constructor() {
            constructor_insert_index(&members)
        } else if in_source {
            // fNode is the invocation's CompilationUnit (start 0).
            method_insert_index(&members, 0)
        } else {
            members.len()
        };
        rw.list_insert_at(RNode::Orig(type_decl.id), "bodyDeclarations", stub, index as i32);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }
}
