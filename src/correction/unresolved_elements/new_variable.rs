//! `NewVariableCorrectionProposalCore`: a new local variable, field,
//! parameter, constant or enum constant for an unresolved name.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::new_method::{guess_type_for_reference, target_ast};
use super::scope;
use super::types::{self, normalize_wildcard, well_known};
use crate::correction::edit::Env;
use crate::correction::{CuChange, LazyChange};
use crate::features::constructors::ConstructorImportContext;
use crate::rewrite::import_rewrite::{ImportRewrite, TypeLocation};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_body_declaration, find_parent_statement, find_parent_type};
use crate::semantic_ast::{modifier, problem as p, Ast, BindingRef, Node, NodeId, NodeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    Local,
    Field,
    Param,
    ConstField,
    EnumConst,
}

pub struct NewVariable {
    pub source: Arc<Ast>,
    pub kind: VariableKind,
    /// The unresolved `SimpleName` (`fOriginalNode`).
    pub node: NodeId,
    /// Key of the sender type declaration binding (fields and enum constants).
    pub sender: Option<String>,
    /// The target unit when it is not the source unit.
    pub target_uri: Option<String>,
    pub add_final: bool,
}

#[tower_lsp::async_trait]
impl LazyChange for NewVariable {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        match self.kind {
            VariableKind::Param => {
                let options = env.options(&self.source.uri).await;
                self.do_add_param(&options)
            }
            VariableKind::Local => {
                let options = env.options(&self.source.uri).await;
                self.do_add_local(&options)
            }
            VariableKind::Field | VariableKind::ConstField => {
                let target = target_ast(env, &self.source, &self.target_uri).await?;
                let options = env.options(&target.uri).await;
                self.do_add_field(&target, &options)
            }
            VariableKind::EnumConst => {
                let target = target_ast(env, &self.source, &self.target_uri).await?;
                self.do_add_enum_const(&target)
            }
        }
    }
}

fn context_at(ast: &Arc<Ast>, node: Node<'_>) -> ConstructorImportContext {
    ConstructorImportContext { ast: ast.clone(), declaration: find_parent_type(node).map(|n| n.id), nullness: None }
}

/// `ASTNodes.isControlStatementBody(locationInParent)`.
pub fn is_control_statement_body(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        NodeKind::IfStatement => node.location_is("thenStatement") || node.location_is("elseStatement"),
        NodeKind::WhileStatement | NodeKind::ForStatement | NodeKind::EnhancedForStatement | NodeKind::DoStatement => node.location_is("body"),
        _ => false,
    }
}

/// `ASTNodes.isParent(node, parent)`: `parent` is a strict ancestor of `node`.
fn is_parent(node: Node<'_>, parent: Node<'_>) -> bool {
    node.ancestors().any(|a| a.id == parent.id)
}

/// `LinkedNodeFinder.getProblemKind`.
fn problem_kind(id: i32) -> i32 {
    match id {
        p::UndefinedField => 1,
        p::UndefinedMethod => 2,
        p::UndefinedLabel => 8,
        p::UndefinedName | p::UnresolvedVariable => 16,
        p::UndefinedType => 4,
        _ => 0,
    }
}

/// `LinkedNodeFinder.findByProblems(parent, nameNode)`.
pub fn find_by_problems<'a>(parent: Node<'a>, name: Node<'a>) -> Option<Vec<Node<'a>>> {
    let ast = parent.ast;
    let name_kind = ast
        .problems
        .iter()
        .find(|pr| pr.source_start as usize == name.start() && (pr.source_end + 1) as usize == name.end())
        .map(|pr| problem_kind(pr.id))
        .unwrap_or(0);
    if name_kind == 0 {
        return None;
    }
    let (body_start, body_end) = (parent.start(), parent.end());
    let identifier = name.identifier();
    let mut res = Vec::new();
    for pr in &ast.problems {
        let (start, end) = (pr.source_start as usize, (pr.source_end + 1) as usize);
        if start > body_start && end < body_end && name_kind & problem_kind(pr.id) != 0 {
            let finder = crate::semantic_ast::finder::NodeFinder::new(parent, start, end - start);
            if let Some(n) = finder.covered.or(finder.covering) {
                if n.is(NodeKind::SimpleName) && n.identifier() == identifier {
                    res.push(n);
                }
            }
        }
    }
    Some(res)
}

/// `ASTNodeFactory.newDefaultExpression(ast, type, 0)` for a type node
/// (`primitive` is the code of a `PrimitiveType` node).
fn default_expression_for(rw: &mut ASTRewrite, primitive: Option<&str>) -> RNode {
    match primitive {
        Some("boolean") => {
            let n = rw.new_node(NodeKind::BooleanLiteral);
            rw.put_simple(n, "booleanValue", "false")
        }
        Some(_) => rw.new_number_literal("0"),
        None => rw.new_node(NodeKind::NullLiteral),
    }
}

/// `BodyDeclarationRewrite.getOrderPreference` with the default member sort
/// order (`T,SF,SI,SM,F,I,C,M`; enum constants first).
fn order_preference(kind: NodeKind, mods: i32, is_constructor: bool) -> i32 {
    // category offsets: enum constants 0, T 1, SF 2, SI 3, SM 4, F 5, I 6, C 7, M 8
    let is_static = mods & modifier::STATIC != 0;
    match kind {
        NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration | NodeKind::RecordDeclaration => 2,
        NodeKind::FieldDeclaration => {
            if is_static {
                let index = 2 * 2;
                if mods & modifier::FINAL != 0 {
                    return index;
                }
                return index + 1;
            }
            5 * 2
        }
        NodeKind::Initializer => {
            if is_static {
                3 * 2
            } else {
                6 * 2
            }
        }
        NodeKind::AnnotationTypeMemberDeclaration => 8 * 2,
        NodeKind::MethodDeclaration => {
            if is_static {
                4 * 2
            } else if is_constructor {
                7 * 2
            } else {
                8 * 2
            }
        }
        _ => 100,
    }
}

/// `BodyDeclarationRewrite.getInsertionIndex(member, container)`.
fn insertion_index(order_index: i32, container: &[Node<'_>]) -> usize {
    let mut insert_pos = container.len();
    let mut insert_pos_order_index = -1;
    for i in (0..container.len()).rev() {
        let c = container[i];
        let curr = order_preference(c.kind(), c.modifiers(), c.flag("constructor"));
        if order_index == curr {
            if insert_pos_order_index != order_index {
                insert_pos = i + 1;
                insert_pos_order_index = order_index;
            }
        } else if insert_pos_order_index != order_index {
            if curr < order_index {
                if insert_pos_order_index == -1 {
                    insert_pos = i + 1;
                    insert_pos_order_index = curr;
                }
            } else {
                insert_pos = i;
                insert_pos_order_index = curr;
            }
        }
    }
    insert_pos
}

/// `NewVariableCorrectionProposalCore.findFieldInsertIndex`.
fn field_insert_index(decls: &[Node<'_>], order_index: i32, max_offset: Option<usize>) -> usize {
    if let Some(max) = max_offset {
        for i in (0..decls.len()).rev() {
            let curr = decls[i];
            if max > curr.start() + curr.length() {
                return insertion_index(order_index, &decls[..=i]);
            }
        }
        return 0;
    }
    insertion_index(order_index, decls)
}

impl NewVariable {
    fn original<'a>(&self, ast: &'a Ast) -> Node<'a> {
        ast.node(self.node)
    }

    /// `isVariableAssigned()`.
    fn is_variable_assigned(&self) -> bool {
        let node = self.original(&self.source);
        node.location_is("leftHandSide") && node.parent().is_some_and(|p| p.is(NodeKind::Assignment))
    }

    /// `evaluateVariableType(ast, imports, importRewriteContext, targetContext, location)`.
    fn evaluate_variable_type(
        &self,
        rw: &mut ASTRewrite,
        imports: &mut ImportRewrite,
        context: &ConstructorImportContext,
        target_context: Option<BindingRef<'_>>,
        location: TypeLocation,
    ) -> RNode {
        self.evaluate_variable_type_code(rw, imports, context, target_context, location).0
    }

    /// `evaluateVariableType` plus the primitive type code of the result.
    fn evaluate_variable_type_code(
        &self,
        rw: &mut ASTRewrite,
        imports: &mut ImportRewrite,
        context: &ConstructorImportContext,
        target_context: Option<BindingRef<'_>>,
        location: TypeLocation,
    ) -> (RNode, Option<String>) {
        let prim = |b: BindingRef<'_>| b.is_primitive().then(|| b.name().to_owned());
        let source = &*self.source;
        let node = self.original(source);
        if let Some(parent) = node.parent().filter(|p| p.is(NodeKind::MethodInvocation)) {
            if node.location_is("expression") {
                let selector = parent.child("name").map(|n| n.identifier()).unwrap_or_default();
                let n_args = parent.list("arguments").len();
                // the target context binding lives in the source AST
                let target_context = target_context.and_then(|t| source.binding_by_key(t.key()));
                let guesses = scope::qualifier_guess(node.root(), &selector, n_args, target_context);
                if let Some(first) = guesses.first() {
                    return (imports.add_import_type(*first, rw, context, location), prim(*first));
                }
            }
        }
        let mut binding = scope::guess_binding_for_reference(node);
        if let Some(b) = binding.filter(|b| b.is_wildcard_type()) {
            binding = normalize_wildcard(b, self.is_variable_assigned()).or_else(|| well_known(source, "java.lang.Object"));
        }
        if let Some(b) = binding {
            return (imports.add_import_type(b, rw, context, location), prim(b));
        }
        // the guessed array type may be unknown to the AST: build it from its element type
        if let Some((mut element, mut dims)) = scope::guess_array_reference(node) {
            if element.is_array() {
                dims += element.dimensions().max(0) as usize;
                element = element.element_type().unwrap_or(element);
            }
            let element = imports.add_import_type(element, rw, context, location);
            let array = rw.new_node(NodeKind::ArrayType);
            rw.put_child(array, "elementType", element);
            let dimensions = (0..dims)
                .map(|_| {
                    let d = rw.new_node(NodeKind::Dimension);
                    rw.put_list(d, "annotations", Vec::new())
                })
                .collect();
            rw.put_list(array, "dimensions", dimensions);
            return (array, None);
        }
        if let Some(t) = guess_type_for_reference(rw, node) {
            return t;
        }
        let name = if self.kind == VariableKind::ConstField { "String" } else { "Object" };
        let n = rw.new_simple_name(name);
        (rw.new_simple_type(n), None)
    }

    /// `doAddParam`.
    fn do_add_param(&self, options: &BTreeMap<String, String>) -> anyhow::Result<Vec<CuChange>> {
        let ast = self.source.clone();
        let node = self.original(&ast);
        let decl = find_parent_body_declaration(node).filter(|d| d.is(NodeKind::MethodDeclaration)).ok_or_else(|| anyhow::anyhow!("no method"))?;
        let mut rw = ASTRewrite::new(ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ast.clone(), options);
        let context = context_at(&ast, decl);
        let new_decl = rw.new_node(NodeKind::SingleVariableDeclaration);
        let typ = self.evaluate_variable_type(&mut rw, &mut imports, &context, decl.binding(), TypeLocation::Parameter);
        rw.put_child(new_decl, "type", typ);
        let name = rw.new_simple_name(&node.identifier());
        rw.put_child(new_decl, "name", name);
        rw.put_simple(new_decl, "varargs", "false");
        let params = decl.list("parameters");
        match params.last() {
            Some(last) if last.is(NodeKind::SingleVariableDeclaration) && last.flag("varargs") => {
                rw.list_insert_at(RNode::Orig(decl.id), "parameters", new_decl, params.len() as i32 - 1);
            }
            _ => rw.list_insert_last(RNode::Orig(decl.id), "parameters", new_decl),
        }
        if let Some(doc) = decl.child("javadoc") {
            let leading: HashSet<String> = params.iter().map(|p| p.child("name").map(|n| n.identifier()).unwrap_or_default()).collect();
            let tag = rw.new_node(NodeKind::TagElement);
            rw.put_simple(tag, "tagName", "@param");
            let arg = rw.new_simple_name(&node.identifier());
            let text = rw.new_node(NodeKind::TextElement);
            rw.put_simple(text, "text", "");
            rw.put_list(tag, "fragments", vec![arg, text]);
            super::proposals::insert_tag(&mut rw, doc, tag, "@param", &leading);
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }

    fn final_modifiers(&self, rw: &mut ASTRewrite) -> Vec<RNode> {
        if self.add_final {
            rw.new_modifiers(modifier::FINAL)
        } else {
            Vec::new()
        }
    }

    /// `doAddLocal`.
    fn do_add_local(&self, options: &BTreeMap<String, String>) -> anyhow::Result<Vec<CuChange>> {
        let ast = self.source.clone();
        let original = self.original(&ast);
        let decl = find_parent_body_declaration(original).ok_or_else(|| anyhow::anyhow!("no body declaration"))?;
        let (body, target_context) = match decl.kind() {
            NodeKind::MethodDeclaration => (decl.child("body"), decl.binding()),
            NodeKind::Initializer => (decl.child("body"), types::parent_type_binding(decl)),
            _ => anyhow::bail!("not in a method or initializer"),
        };
        let body = body.ok_or_else(|| anyhow::anyhow!("no body"))?;
        let mut rw = ASTRewrite::new(ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ast.clone(), options);

        // getAllReferences
        let mut names = find_by_problems(body, original).unwrap_or_else(|| vec![original]);
        if names.is_empty() {
            // `LinkedNodeFinder` returned an empty array: `names[0]` would fail.
            anyhow::bail!("no references");
        }
        names.sort_by_key(|n| n.start());
        let dominant = dominant_node(&names);
        let mut dominant_statement = find_parent_statement(dominant).ok_or_else(|| anyhow::anyhow!("no statement"))?;
        if is_control_statement_body(dominant_statement) {
            dominant_statement = dominant_statement.parent().unwrap();
        }
        let node = names[0];
        let context = context_at(&ast, node);

        let is_assigned = dominant_statement.is(NodeKind::ExpressionStatement)
            && dominant_statement.child("expression").is_some_and(|e| e.is(NodeKind::Assignment) && e.child("leftHandSide") == Some(node));
        let is_for_init = || {
            dominant_statement.is(NodeKind::ForStatement) && {
                let inits = dominant_statement.list("initializers");
                inits.len() == 1 && inits[0].is(NodeKind::Assignment) && inits[0].child("leftHandSide") == Some(node)
            }
        };
        let is_enhanced_for = || {
            dominant_statement.is(NodeKind::EnhancedForStatement)
                && dominant_statement.child("parameter").and_then(|p| p.child("type")).is_some_and(|t| Some(t) == node.parent())
        };
        if is_assigned || (dominant.id != dominant_statement.id && is_for_init()) {
            // x = 1; -> int x = 1;   /   for (x = 1;;) -> for (int x = 1;;)
            let assignment = node.parent().unwrap();
            let rhs = assignment.child("rightHandSide").ok_or_else(|| anyhow::anyhow!("no right hand side"))?;
            let placeholder = rw.create_copy_target(rhs.id);
            let frag = rw.new_variable_declaration_fragment(&node.identifier(), Some(placeholder));
            let new_decl = rw.new_node(NodeKind::VariableDeclarationExpression);
            let mods = self.final_modifiers(&mut rw);
            rw.put_list(new_decl, "modifiers", mods);
            let typ = self.evaluate_variable_type(&mut rw, &mut imports, &context, target_context, TypeLocation::LocalVariable);
            rw.put_child(new_decl, "type", typ);
            rw.put_list(new_decl, "fragments", vec![frag]);
            rw.replace(RNode::Orig(assignment.id), Some(new_decl));
            return Ok(vec![CuChange::rewrite(rw).with_imports(imports)]);
        }
        if dominant.id != dominant_statement.id && is_enhanced_for() {
            // for (x: collectionOfT) -> for (T x: collectionOfT)
            let parameter = dominant_statement.child("parameter").unwrap();
            if self.add_final {
                let m = rw.new_modifier("final");
                rw.list_insert_last(RNode::Orig(parameter.id), "modifiers", m);
            }
            let expression = dominant_statement.child("expression").ok_or_else(|| anyhow::anyhow!("no expression"))?;
            let new_name = rw.create_move_target(node.id);
            rw.set(RNode::Orig(parameter.id), "name", Some(new_name));
            let mut element = None;
            if let Some(t) = expression.type_binding() {
                if t.is_array() {
                    element = t.element_type();
                } else if let Some(iterable) = crate::correction::type_mismatch::bindings::find_type_in_hierarchy(t, "java.lang.Iterable") {
                    let args = iterable.type_arguments();
                    if args.len() == 1 {
                        element = crate::correction::type_mismatch::bindings::normalize_for_declaration_use(args[0]);
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
            return Ok(vec![CuChange::rewrite(rw).with_imports(imports)]);
        }

        // foo(x) -> int x; foo(x)
        let frag = rw.new_variable_declaration_fragment(&node.identifier(), None);
        let new_decl = rw.new_node(NodeKind::VariableDeclarationStatement);
        let typ = self.evaluate_variable_type(&mut rw, &mut imports, &context, target_context, TypeLocation::LocalVariable);
        let mods = self.final_modifiers(&mut rw);
        rw.put_list(new_decl, "modifiers", mods);
        rw.put_child(new_decl, "type", typ);
        rw.put_list(new_decl, "fragments", vec![frag]);

        let mut statement = dominant_statement;
        let containing = |s: Node<'_>| s.parent().is_some_and(|p| (p.is(NodeKind::Block) || p.is(NodeKind::SwitchStatement)) && s.location_is("statements"));
        while !containing(statement) && statement.parent().is_some_and(|p| p.kind().is_statement()) {
            statement = statement.parent().unwrap();
        }
        if containing(statement) {
            let parent = statement.parent().unwrap();
            // `ListRewrite.insertBefore`: statements are bound to the previous element.
            rw.set_insert_bound_to_previous(new_decl);
            rw.list_insert_before(RNode::Orig(parent.id), "statements", new_decl, RNode::Orig(statement.id));
        }
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }

    /// `evaluateFieldModifiers(newTypeDecl)`.
    fn evaluate_field_modifiers(&self, sender: BindingRef<'_>, new_type_decl: Node<'_>) -> i32 {
        if sender.is_annotation() {
            return 0;
        }
        if sender.is_interface() {
            return new_type_decl.list("bodyDeclarations").into_iter().find(|b| b.is(NodeKind::FieldDeclaration)).map(|f| f.modifiers()).unwrap_or(0);
        }
        let node = self.original(&self.source);
        let mut mods = 0;
        if self.add_final {
            mods |= modifier::FINAL;
        }
        if self.kind == VariableKind::ConstField {
            mods |= modifier::FINAL | modifier::STATIC;
        } else if let Some(parent) = node.parent().filter(|p| p.is(NodeKind::QualifiedName)) {
            if parent.child("qualifier").and_then(|q| q.binding()).is_some_and(|b| b.is_type()) {
                mods |= modifier::STATIC;
            }
        } else if scope::is_in_static_context(node) {
            mods |= modifier::STATIC;
        }
        let parent_type = find_parent_type_treat_modifiers(node);
        if self.target_uri.is_none() && parent_type.is_some_and(|t| t.id == new_type_decl.id) {
            mods |= modifier::PRIVATE;
        } else if parent_type.is_some_and(|t| t.is(NodeKind::AnonymousClassDeclaration)) {
            mods |= modifier::PROTECTED;
        } else {
            mods |= modifier::PUBLIC;
        }
        mods
    }

    /// `doAddField`.
    fn do_add_field(&self, target: &Arc<Ast>, options: &BTreeMap<String, String>) -> anyhow::Result<Vec<CuChange>> {
        let sender_key = self.sender.as_deref().ok_or_else(|| anyhow::anyhow!("no sender"))?;
        let sender = self.source.binding_by_key(sender_key).ok_or_else(|| anyhow::anyhow!("no sender binding"))?;
        let new_type_decl = target.binding_by_key(sender_key).and_then(|b| b.declaring_node()).ok_or_else(|| anyhow::anyhow!("no sender declaration"))?;
        let is_in_different_cu = self.target_uri.is_some();
        let mut imports = ImportRewrite::create_for_corrections(target.clone(), options);
        let context = ConstructorImportContext { ast: target.clone(), declaration: Some(new_type_decl.id), nullness: None };
        let mut rw = ASTRewrite::new(target.clone());
        let node = self.original(&self.source);
        let (typ, primitive) = self.evaluate_variable_type_code(&mut rw, &mut imports, &context, Some(sender), TypeLocation::Field);
        let initializer = if sender.is_interface() || self.kind == VariableKind::ConstField { Some(default_expression_for(&mut rw, primitive.as_deref())) } else { None };
        let fragment = rw.new_variable_declaration_fragment(&node.identifier(), initializer);
        let mods = self.evaluate_field_modifiers(sender, new_type_decl);
        let modifiers = rw.new_modifiers(mods);
        let new_decl = rw.new_field_declaration(fragment, modifiers, typ);
        let decls = new_type_decl.list("bodyDeclarations");
        let max_offset = if is_in_different_cu { None } else { Some(node.start()) };
        let index = field_insert_index(&decls, order_preference(NodeKind::FieldDeclaration, mods, false), max_offset);
        // `ListRewrite.insertAt`: field declarations are bound to the previous element.
        rw.set_insert_bound_to_previous(new_decl);
        rw.list_insert_at(RNode::Orig(new_type_decl.id), "bodyDeclarations", new_decl, index as i32);
        Ok(vec![CuChange::rewrite(rw).with_imports(imports)])
    }

    /// `doAddEnumConst`.
    fn do_add_enum_const(&self, target: &Arc<Ast>) -> anyhow::Result<Vec<CuChange>> {
        let sender_key = self.sender.as_deref().ok_or_else(|| anyhow::anyhow!("no sender"))?;
        let new_type_decl = target.binding_by_key(sender_key).and_then(|b| b.declaring_node()).ok_or_else(|| anyhow::anyhow!("no sender declaration"))?;
        let mut rw = ASTRewrite::new(target.clone());
        let node = self.original(&self.source);
        let constant = rw.new_node(NodeKind::EnumConstantDeclaration);
        let name = rw.new_simple_name(&node.identifier());
        rw.put_child(constant, "name", name);
        rw.list_insert_last(RNode::Orig(new_type_decl.id), "enumConstants", constant);
        Ok(vec![CuChange::rewrite(rw)])
    }
}

/// `NewVariableCorrectionProposalCore.getDominantNode`.
fn dominant_node<'a>(names: &[Node<'a>]) -> Node<'a> {
    let mut dominator = names[0];
    for curr in names.iter().skip(1).copied() {
        if curr.id != dominator.id {
            // getCommonParent(curr, dominator)
            let mut parent = curr.parent();
            while let Some(p) = parent {
                if is_parent(dominator, p) {
                    break;
                }
                parent = p.parent();
            }
            if curr.start() < dominator.start() {
                dominator = curr;
            }
            while dominator.parent().map(|p| p.id) != parent.map(|p| p.id) {
                match dominator.parent() {
                    Some(p) => dominator = p,
                    None => break,
                }
            }
        }
    }
    let parent_kind = dominator.parent().map(|p| p.kind());
    if !matches!(parent_kind, Some(NodeKind::Block | NodeKind::ForStatement | NodeKind::EnhancedForStatement)) {
        return dominator.parent().unwrap_or(dominator);
    }
    dominator
}

/// `ASTResolving.findParentType(node, true)`: modifiers of a type belong to
/// its parent.
pub fn find_parent_type_treat_modifiers(node: Node<'_>) -> Option<Node<'_>> {
    let mut last: Option<&'static str> = None;
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() {
            if last != Some("modifiers") {
                return Some(x);
            }
        } else if x.is(NodeKind::AnonymousClassDeclaration) {
            return Some(x);
        }
        last = x.location();
        n = x.parent();
    }
    None
}
