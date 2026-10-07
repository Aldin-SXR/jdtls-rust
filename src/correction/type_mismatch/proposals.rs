//! The proposals TypeMismatchSubProcessor creates: `CastCorrectionProposalCore`,
//! `TypeChangeCorrectionProposalCore`, `ImplementInterfaceProposalCore`,
//! `ChangeMethodSignatureProposalCore` (exception changes) and the local
//! loop variable of `NewVariableCorrectionProposalCore`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use tower_lsp::lsp_types::Url;

use super::bindings::{self, Ty};
use crate::correction::edit::Env;
use crate::correction::{kind, messages, Change, CuChange, Proposal, ProposalType};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::find_parent_type;
use crate::semantic_ast::{Ast, BindingKind, BindingRef, Node, NodeKind};

/// `ContextSensitiveImportRewriteContext(node, imports)`: the scope of the
/// type enclosing `node`.
pub fn import_context(ast: &Arc<Ast>, node: Node<'_>, options: &BTreeMap<String, String>) -> ConstructorImportContext {
    ConstructorImportContext {
        ast: ast.clone(),
        declaration: find_parent_type(node).map(|n| n.id),
        nullness: crate::rewrite::import_rewrite::nullness::Filter::create(ast, Some(node.id), options),
    }
}

/// `ImportRewrite.addImport(type, ast, context, location)` for a binding or a
/// well-known type known only by name (`java.lang` boxes, primitives).
pub fn add_import(imports: &mut ImportRewrite, rw: &mut ASTRewrite, context: &ConstructorImportContext, typ: Ty<'_>, location: TypeLocation) -> RNode {
    match typ {
        Ty::Binding(b) => imports.add_import_type(b, rw, context, location),
        Ty::Named(name) => {
            if !name.contains('.') {
                rw.new_primitive_type(name)
            } else {
                let simple = name.rsplit('.').next().unwrap_or(name);
                let n = rw.new_simple_name(simple);
                rw.new_simple_type(n)
            }
        }
    }
}

/// The source unit of a type binding (`ASTResolving.findCompilationUnitForBinding`).
pub async fn compilation_unit_for(env: &Env<'_>, ast: &Arc<Ast>, binding: BindingRef<'_>) -> Option<Arc<Ast>> {
    if !binding.is_from_source() || binding.is_type_variable() || binding.is_wildcard_type() {
        return None;
    }
    let declaration = binding.type_declaration().unwrap_or(binding);
    if let Some(node) = declaration.declaring_node() {
        return (node.kind().is_abstract_type_declaration() || node.is(NodeKind::AnonymousClassDeclaration)).then(|| ast.clone());
    }
    // `Bindings.findCompilationUnit`: the unit of the top-level type.
    let key = declaration.key();
    let key = key.strip_prefix('L')?;
    let end = key.find([';', '<', '$', '~']).unwrap_or(key.len());
    let path = format!("/{}.java", &key[..end]);
    let url = Url::parse(&ast.uri).ok()?;
    let context = env.dispatcher.context_for(Some(&url)).await;
    let mut candidates: Vec<&String> = context.files.keys().filter(|u| u.ends_with(&path)).collect();
    candidates.sort();
    let uri = Url::parse(candidates.first()?).ok()?;
    crate::semantic_ast::fetch(env.dispatcher, &uri).await.ok()
}

/// The declaring node of `binding` in `target` (`findDeclaringNode(key)`).
fn declaring_node<'a>(target: &'a Ast, binding: BindingRef<'_>) -> Option<Node<'a>> {
    target.binding_by_key(binding.key())?.declaring_node()
}

fn single_change(rw: ASTRewrite, imports: ImportRewrite) -> Change {
    Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)])
}

/// `CastCorrectionProposalCore` with a cast type.
pub async fn cast_proposal(env: &Env<'_>, ast: &Arc<Ast>, label: String, node_to_cast: Node<'_>, cast_type: Ty<'_>, relevance: i32) -> Proposal {
    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = import_context(ast, node_to_cast, &options);
    let typ = add_import(&mut imports, &mut rw, &context, cast_type, TypeLocation::Cast);
    if node_to_cast.is(NodeKind::CastExpression) {
        if let Some(t) = node_to_cast.child("type") {
            rw.replace(RNode::Orig(t.id), Some(typ));
        }
    } else {
        let mut copy = rw.create_copy_target(node_to_cast.id);
        if matches!(node_to_cast.kind(), NodeKind::InfixExpression | NodeKind::ConditionalExpression | NodeKind::Assignment | NodeKind::InstanceofExpression) {
            copy = rw.new_parenthesized_expression(copy);
        }
        let cast = rw.new_node(NodeKind::CastExpression);
        rw.put_child(cast, "type", typ);
        rw.put_child(cast, "expression", copy);
        let mut replacing = cast;
        let outer = node_to_cast.parent().is_some_and(|p| match p.kind() {
            NodeKind::MethodInvocation | NodeKind::FieldAccess => p.child("expression") == Some(node_to_cast),
            NodeKind::QualifiedName => p.child("qualifier") == Some(node_to_cast),
            _ => false,
        });
        if outer {
            replacing = rw.new_parenthesized_expression(cast);
        }
        rw.replace(RNode::Orig(node_to_cast.id), Some(replacing));
    }
    Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports))
}

/// `TypeChangeCorrectionProposalCore(targetCU, binding, astRoot, newType, false, relevance)`.
/// `binding` is a declaration binding of `ast`; the change applies to `target`.
pub async fn type_change_proposal(
    env: &Env<'_>,
    target: &Arc<Ast>,
    binding: BindingRef<'_>,
    new_type: BindingRef<'_>,
    offer_super_type_proposals: bool,
    relevance: i32,
    display_name: Option<String>,
) -> Option<Proposal> {
    let new_type = if offer_super_type_proposals {
        // `getRelaxingTypes` + `sortTypes`: the first proposed type.
        let mut types = bindings::relaxing_types(new_type);
        let old = if binding.kind() == BindingKind::Method { binding.return_type() } else { binding.var_type() };
        if let Some(old) = old.filter(|o| o.is_parameterized_type()) {
            let declaration = old.type_declaration().unwrap_or(old);
            types.sort_by_key(|t| if t.type_declaration().unwrap_or(*t) == declaration { 0 } else { 1 });
        }
        types[0]
    } else {
        new_type
    };
    let options = env.options(&target.uri).await;
    let decl = declaring_node(target, binding)?;
    let type_name = if bindings::contains_nested_capture(Some(new_type), false) {
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
        let context = import_context(target, decl, &options);
        imports.add_import_type_string(new_type, &context, TypeLocation::Unknown)
    } else {
        bindings::type_label(new_type)
    };
    let (label, location) = if binding.kind() == BindingKind::Variable {
        let args = [binding.name(), type_name.as_str()];
        if binding.is_field() {
            (messages::format(messages::correction("TypeChangeCompletionProposal_field_name"), &args), TypeLocation::Field)
        } else if binding.declaring_node().is_some_and(|n| n.is(NodeKind::SingleVariableDeclaration)) {
            (messages::format(messages::correction("TypeChangeCompletionProposal_param_name"), &args), TypeLocation::Parameter)
        } else {
            (messages::format(messages::correction("TypeChangeCompletionProposal_variable_name"), &args), TypeLocation::LocalVariable)
        }
    } else {
        (messages::format(messages::correction("TypeChangeCompletionProposal_method_name"), &[binding.name(), type_name.as_str()]), TypeLocation::ReturnType)
    };
    let label = display_name.unwrap_or(label);

    let mut rw = ASTRewrite::new(target.clone());
    let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
    let context = import_context(target, decl, &options);
    let mut typ = imports.add_import_type(new_type, &mut rw, &context, location);
    let remove_dimensions = |rw: &mut ASTRewrite, node: Node<'_>| {
        for d in node.list("extraDimensions2") {
            rw.remove(RNode::Orig(d.id));
        }
    };
    match decl.kind() {
        NodeKind::MethodDeclaration => {
            if new_type.is_type_variable() {
                if let Some(real) = new_type.declaring_method() {
                    let type_parameters = real.type_parameters();
                    let declared = decl.list("typeParameters");
                    if !declared.is_empty() {
                        let map: HashMap<String, String> = type_parameters
                            .iter()
                            .zip(declared.iter())
                            .map(|(p, d)| (p.name().to_owned(), d.child("name").map(|n| n.identifier()).unwrap_or_default()))
                            .collect();
                        if let Some(existing) = map.get(new_type.name()) {
                            let n = rw.new_simple_name(existing);
                            typ = rw.new_simple_type(n);
                        }
                    } else {
                        for parameter in &type_parameters {
                            let tp = rw.new_node(NodeKind::TypeParameter);
                            let n = rw.new_simple_name(parameter.name());
                            rw.put_child(tp, "name", n);
                            let bounds = parameter
                                .type_bounds()
                                .into_iter()
                                .map(|b| type_node_from_binding(&mut rw, &mut imports, &context, b, location))
                                .collect();
                            rw.put_list(tp, "typeBounds", bounds);
                            rw.list_insert_last(RNode::Orig(decl.id), "typeParameters", tp);
                        }
                        let parameter_types = real.parameter_types();
                        for (i, svd) in decl.list("parameters").into_iter().enumerate() {
                            if let Some(t) = parameter_types.get(i) {
                                let node = type_node_from_binding(&mut rw, &mut imports, &context, *t, location);
                                rw.set(RNode::Orig(svd.id), "type", Some(node));
                            }
                        }
                    }
                }
            }
            rw.set(RNode::Orig(decl.id), "returnType2", Some(typ));
            remove_dimensions(&mut rw, decl);
        }
        NodeKind::AnnotationTypeMemberDeclaration => {
            rw.set(RNode::Orig(decl.id), "type", Some(typ));
        }
        NodeKind::VariableDeclarationFragment => {
            let parent = decl.parent()?;
            match parent.kind() {
                NodeKind::FieldDeclaration => {
                    let fragments = parent.list("fragments");
                    let owner = parent.parent()?;
                    if fragments.len() > 1 && owner.kind().is_abstract_type_declaration() {
                        let placeholder = rw.create_move_target(decl.id);
                        let field = rw.new_node(NodeKind::FieldDeclaration);
                        rw.put_child(field, "type", typ);
                        rw.put_list(field, "fragments", vec![placeholder]);
                        let prop = crate::semantic_ast::resolve::BODY_DECLARATIONS;
                        if fragments[0] == decl {
                            rw.list_insert_before(RNode::Orig(owner.id), prop, field, RNode::Orig(parent.id));
                        } else {
                            rw.list_insert_after(RNode::Orig(owner.id), prop, field, RNode::Orig(parent.id));
                        }
                    } else {
                        rw.set(RNode::Orig(parent.id), "type", Some(typ));
                        remove_dimensions(&mut rw, decl);
                    }
                }
                NodeKind::VariableDeclarationStatement => {
                    let fragments = parent.list("fragments");
                    let block = parent.parent()?;
                    if fragments.len() > 1 && block.is(NodeKind::Block) {
                        let placeholder = rw.create_move_target(decl.id);
                        let statement = rw.new_node(NodeKind::VariableDeclarationStatement);
                        rw.put_child(statement, "type", typ);
                        rw.put_list(statement, "fragments", vec![placeholder]);
                        if fragments[0] == decl {
                            rw.list_insert_before(RNode::Orig(block.id), "statements", statement, RNode::Orig(parent.id));
                        } else {
                            rw.list_insert_after(RNode::Orig(block.id), "statements", statement, RNode::Orig(parent.id));
                        }
                    } else {
                        rw.set(RNode::Orig(parent.id), "type", Some(typ));
                        remove_dimensions(&mut rw, decl);
                    }
                }
                NodeKind::VariableDeclarationExpression => {
                    rw.set(RNode::Orig(parent.id), "type", Some(typ));
                    remove_dimensions(&mut rw, decl);
                }
                _ => {}
            }
        }
        NodeKind::SingleVariableDeclaration => {
            rw.set(RNode::Orig(decl.id), "type", Some(typ));
            remove_dimensions(&mut rw, decl);
        }
        _ => return None,
    }
    Some(Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports)))
}

/// `TypeChangeCorrectionProposalCore.getTypeNodeFromBinding`.
fn type_node_from_binding(rw: &mut ASTRewrite, imports: &mut ImportRewrite, context: &ConstructorImportContext, b: BindingRef<'_>, location: TypeLocation) -> RNode {
    if b.is_wildcard_type() {
        let w = rw.new_node(NodeKind::WildcardType);
        if let Some(bound) = b.bound() {
            let n = type_node_from_binding(rw, imports, context, bound, location);
            rw.put_child(w, "bound", n);
            rw.put_simple(w, "upperBound", if b.has(crate::semantic_ast::bflag::UPPERBOUND) { "true" } else { "false" });
        }
        return w;
    }
    if b.is_array() {
        let element = b.element_type().map(|e| type_node_from_binding(rw, imports, context, e, location));
        let array = rw.new_node(NodeKind::ArrayType);
        if let Some(e) = element {
            rw.put_child(array, "elementType", e);
        }
        let dims = (0..b.dimensions()).map(|_| rw.new_node(NodeKind::Dimension)).collect();
        rw.put_list(array, "dimensions", dims);
        return array;
    }
    if b.is_type_variable() {
        let n = rw.new_simple_name(b.name());
        return rw.new_simple_type(n);
    }
    let _ = imports.add_import_type_string(b, context, location);
    let erasure_name = b.erasure().unwrap_or(b).name().to_owned();
    let n = rw.new_simple_name(&erasure_name);
    let simple = rw.new_simple_type(n);
    if b.is_parameterized_type() {
        let p = rw.new_node(NodeKind::ParameterizedType);
        rw.put_child(p, "type", simple);
        let args = b.type_arguments().into_iter().map(|a| type_node_from_binding(rw, imports, context, a, location)).collect();
        rw.put_list(p, "typeArguments", args);
        return p;
    }
    simple
}

/// `TypeChangeCorrectionProposalCore(targetCU, nodeToChange, astRoot, variableType, relevance)`:
/// replaces the type of a class instance creation with a compatible type.
pub async fn constructor_type_proposal(env: &Env<'_>, ast: &Arc<Ast>, creation: Node<'_>, variable_type: BindingRef<'_>, relevance: i32) -> Option<Proposal> {
    let name = bindings::resolve_expression_binding(creation, false).map(|b| b.name().to_owned()).unwrap_or_else(|| "constructor".into());
    let label = messages::format(messages::correction("TypeChangeCompletionProposal_constructor_name"), &[&name]);
    // `getNewConstructorProposals`: non-abstract types proposed at `new `
    // that are subtypes of the variable's type.
    let declaration = variable_type.type_declaration().unwrap_or(variable_type);
    let candidate = (declaration.is_class() && declaration.modifiers() & crate::semantic_ast::modifier::ABSTRACT == 0 && !declaration.is_anonymous())
        .then_some(declaration)?;
    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = import_context(ast, creation, &options);
    let typ = imports.add_import_type(candidate, &mut rw, &context, TypeLocation::New);
    rw.set(RNode::Orig(creation.id), "type", Some(typ));
    Some(Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports)))
}

/// `ImplementInterfaceProposalCore`.
pub async fn implement_interface_proposal(env: &Env<'_>, target: &Arc<Ast>, binding: BindingRef<'_>, new_interface: BindingRef<'_>, relevance: i32) -> Option<Proposal> {
    let label = messages::format(messages::correction("ImplementInterfaceProposal_name"), &[binding.name(), &bindings::raw_name(new_interface)]);
    let decl = declaring_node(target, binding)?;
    if !decl.is(NodeKind::TypeDeclaration) {
        return None;
    }
    let options = env.options(&target.uri).await;
    let mut rw = ASTRewrite::new(target.clone());
    let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
    let context = import_context(target, decl, &options);
    let typ = imports.add_import_type(new_interface, &mut rw, &context, TypeLocation::Other);
    rw.list_insert_last(RNode::Orig(decl.id), "superInterfaceTypes", typ);
    Some(Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports)))
}

/// An exception change of `ChangeMethodSignatureProposalCore`.
pub enum ExceptionChange<'a> {
    Keep,
    Remove,
    Insert(BindingRef<'a>),
}

/// `ChangeMethodSignatureProposalCore` with exception changes only.
pub async fn change_exceptions_proposal(
    env: &Env<'_>,
    target: &Arc<Ast>,
    label: String,
    method: BindingRef<'_>,
    changes: &[ExceptionChange<'_>],
    relevance: i32,
) -> Option<Proposal> {
    use crate::correction::local_corrections::{exception_type_name, insert_throws_tag, javadoc_tag_argument, type_references};
    let decl = declaring_node(target, method)?;
    if !decl.is(NodeKind::MethodDeclaration) {
        return None;
    }
    let options = env.options(&target.uri).await;
    let mut rw = ASTRewrite::new(target.clone());
    let mut imports = ImportRewrite::create_for_corrections(target.clone(), &options);
    let context = import_context(target, decl, &options);
    let exceptions = decl.list("thrownExceptionTypes");
    let javadoc = decl.child("javadoc");
    let find_throws_tag = |name: &str| {
        javadoc.and_then(|doc| {
            doc.list("tags").into_iter().find(|tag| matches!(tag.simple("tagName"), Some("@throws" | "@exception")) && javadoc_tag_argument(*tag).as_deref() == Some(name))
        })
    };
    let mut k = 0;
    for (i, change) in changes.iter().enumerate() {
        match change {
            ExceptionChange::Keep => k += 1,
            ExceptionChange::Insert(typ) => {
                let name = imports.add_import_binding(*typ, &context);
                let node = imports.add_import_type(*typ, &mut rw, &context, TypeLocation::Exception);
                rw.list_insert_at(RNode::Orig(decl.id), "thrownExceptionTypes", node, i as i32);
                if let Some(doc) = javadoc {
                    if find_throws_tag(&name).is_none() {
                        insert_throws_tag(&mut rw, doc, &name, &exceptions[..k.min(exceptions.len())]);
                    }
                }
            }
            ExceptionChange::Remove => {
                let Some(node) = exceptions.get(k).copied() else { continue };
                if let Some(binding) = node.binding() {
                    if type_references(binding) == 1 {
                        imports.remove_import(binding.qualified_name());
                    }
                }
                rw.remove(RNode::Orig(node.id));
                k += 1;
                if let Some(tag) = find_throws_tag(&exception_type_name(node, false)) {
                    rw.remove(RNode::Orig(tag.id));
                }
            }
        }
    }
    Some(Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports)))
}

/// `NewVariableCorrectionProposalCore(LOCAL)` for the recovered enhanced for
/// statement `for (x: collection)`: `for (T x: collection)`.
pub async fn loop_variable_proposal(env: &Env<'_>, ast: &Arc<Ast>, label: String, name: Node<'_>, relevance: i32) -> Option<Proposal> {
    let typ_node = name.parent()?;
    let parameter = typ_node.parent()?;
    let statement = parameter.parent().filter(|s| s.is(NodeKind::EnhancedForStatement))?;
    if parameter.child("type") != Some(typ_node) {
        return None;
    }
    let options = env.options(&ast.uri).await;
    let mut rw = ASTRewrite::new(ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = import_context(ast, name, &options);
    let moved = rw.create_move_target(name.id);
    rw.set(RNode::Orig(parameter.id), "name", Some(moved));
    let expression = statement.child("expression")?;
    let mut element = None;
    if let Some(t) = expression.type_binding() {
        if t.is_array() {
            element = t.element_type();
        } else if let Some(iterable) = bindings::find_type_in_hierarchy(t, "java.lang.Iterable") {
            let args = iterable.type_arguments();
            if args.len() == 1 {
                element = bindings::normalize_for_declaration_use(args[0]);
            }
        }
    }
    let typ = match element {
        Some(e) => imports.add_import_type(e, &mut rw, &context, TypeLocation::LocalVariable),
        None => {
            let n = rw.new_simple_name("Object");
            rw.new_simple_type(n)
        }
    };
    rw.set(RNode::Orig(parameter.id), "type", Some(typ));
    let mut p = Proposal::new(label, kind::QUICK_FIX, relevance, single_change(rw, imports));
    p.proposal_type = ProposalType::NewElement;
    Some(p)
}
