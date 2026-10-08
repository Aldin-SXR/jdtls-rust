//! Port of `GetterSetterCorrectionSubProcessor` (jdt.ls) /
//! `GetterSetterCorrectionBaseSubProcessor` (jdt.core.manipulation): replace
//! an access to a field with its getter / setter, or propose the
//! `SelfEncapsulateFieldRefactoring` ("Create getter and setter for ...").
//! The refactoring and its `AccessAnalyzer` are ported in [`sef`] as far as
//! the quick fix uses them (public accessors, insertion index 0, declaring
//! class encapsulated).

use std::sync::Arc;

use super::edit::Env;
use super::parentheses;
use super::{kind, messages, Change, Context, ProblemLocation, Proposal};
use crate::features::accessors::naming;
use crate::features::java_model::{FieldDecl, TypeDecl, TypeKind};
use crate::refactoring::checks;
use crate::rewrite::flattener::Flattener;
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier as m, Ast, BindingRef, Node, NodeKind};

/// `addGetterSetterProposal(context, location, proposals, relevance)`.
pub async fn add_getter_setter_proposal(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, relevance: i32) {
    let ast = ctx.ast.clone();
    let Some(node) = problem.covering_node(&ast) else { return };
    if let Some(p) = getter_setter_proposal(env, &ast, node, relevance).await {
        proposals.push(p);
    }
}

/// `addGetterSetterProposals(context, coveringNode, proposals, relevance)`.
async fn getter_setter_proposal(env: &Env<'_>, ast: &Arc<Ast>, node: Node<'_>, relevance: i32) -> Option<Proposal> {
    if !node.is(NodeKind::SimpleName) {
        return None;
    }
    let binding = node.binding()?;
    if !binding.is_variable() || !binding.is_field() {
        return None;
    }
    get_proposal(env, ast, node, binding, relevance).await
}

/// `ProposalParameter`.
struct Parameter<'a> {
    use_super: bool,
    access_node: Node<'a>,
    qualifier: Option<Node<'a>>,
    variable: BindingRef<'a>,
}

async fn get_proposal<'a>(env: &Env<'_>, ast: &Arc<Ast>, sn: Node<'a>, variable: BindingRef<'a>, relevance: i32) -> Option<Proposal> {
    let mut access_node = sn;
    let mut qualifier = None;
    let mut use_super = false;
    let parent = sn.parent()?;
    match parent.kind() {
        NodeKind::QualifiedName => {
            access_node = parent;
            qualifier = parent.child("qualifier");
        }
        NodeKind::SuperFieldAccess => {
            access_node = parent;
            qualifier = parent.child("qualifier");
            use_super = true;
        }
        _ => {}
    }
    let p = Parameter { use_super, access_node, qualifier, variable };
    let options = env.options(&ast.uri).await;
    let use_is = match tower_lsp::lsp_types::Url::parse(&ast.uri) {
        Ok(url) => crate::features::accessors::profile(env.dispatcher, &url).await.use_is,
        Err(_) => true,
    };
    let names = Names { options: &options, use_is };
    if crate::correction::unresolved_elements::is_write_access(sn) {
        create_setter_proposal(env, ast, &p, &names, relevance).await
    } else {
        create_getter_proposal(env, ast, &p, &names, relevance).await
    }
}

struct Names<'o> {
    options: &'o std::collections::BTreeMap<String, String>,
    use_is: bool,
}

/// A `FieldDecl` carrying what `NamingConventions` needs of a field binding.
fn field_model(name: &str, flags: i32, boolean: bool) -> FieldDecl {
    FieldDecl {
        name: name.to_owned(),
        name_range: (0, 0),
        source: (0, 0),
        flags: flags as u32,
        enum_constant: false,
        record_component: false,
        type_label: boolean.then(|| "boolean".to_owned()),
        type_signature: None,
        children: Vec::new(),
    }
}

fn type_model(kind: TypeKind, name: &str) -> TypeDecl {
    TypeDecl {
        kind,
        name: name.to_owned(),
        name_range: (0, 0),
        source: (0, 0),
        flags: 0,
        type_params: Vec::new(),
        anonymous: false,
        anon_super: None,
        enum_body: false,
        members: Vec::new(),
    }
}

impl Names<'_> {
    /// `GetterSetterUtil.getGetterName(variableBinding, project, null, isBoolean)`.
    fn getter(&self, v: BindingRef<'_>, is_boolean: bool) -> String {
        let f = field_model(v.name(), v.modifiers(), is_boolean);
        naming::getter(&type_model(TypeKind::Class, ""), &f, self.options, self.use_is)
    }

    /// `GetterSetterUtil.getSetterName(variableBinding, project, null, isBoolean)`.
    fn setter(&self, v: BindingRef<'_>, is_boolean: bool) -> String {
        let f = field_model(v.name(), v.modifiers(), is_boolean);
        naming::setter(&f, self.options, self.use_is)
    }
}

/// `GetterSetterUtil.getGetterName(variableBinding, project, null, isBoolean)`.
pub(crate) fn getter_name(options: &std::collections::BTreeMap<String, String>, use_is: bool, v: BindingRef<'_>, is_boolean: bool) -> String {
    Names { options, use_is }.getter(v, is_boolean)
}

/// `GetterSetterUtil.getSetterName(variableBinding, project, null, isBoolean)`.
pub(crate) fn setter_name(options: &std::collections::BTreeMap<String, String>, use_is: bool, v: BindingRef<'_>, is_boolean: bool) -> String {
    Names { options, use_is }.setter(v, is_boolean)
}

/// `isBoolean(context)`: `boolean` or `java.lang.Boolean`.
fn is_boolean(p: &Parameter<'_>) -> bool {
    p.variable.var_type().is_some_and(|t| {
        let qn = t.qualified_name();
        qn == "boolean" || qn == "java.lang.Boolean"
    })
}

/// `ASTNodes.asString(node)`.
fn as_string(ast: &Arc<Ast>, node: Node<'_>) -> String {
    let rw = ASTRewrite::new(ast.clone());
    Flattener::as_string(&rw, RNode::Orig(node.id))
}

fn same_static(a: i32, b: i32) -> bool {
    (a & m::STATIC != 0) == (b & m::STATIC != 0)
}

/// `findGetter(context)`.
fn find_getter<'a>(p: &Parameter<'a>, names: &Names<'_>) -> Option<BindingRef<'a>> {
    let return_type = p.variable.var_type()?;
    let getter_name = names.getter(p.variable, is_boolean(p));
    let declaring = p.variable.declaring_class()?;
    let getter = crate::correction::unresolved_elements::find_method_in_hierarchy(declaring, &getter_name, Some(&[]))?;
    let compatible = getter.return_type().is_some_and(|r| checks::is_assignment_compatible(r, return_type));
    (compatible && same_static(getter.modifiers(), p.variable.modifiers())).then_some(getter)
}

/// `createMethodInvocation(context, method, argument)`.
fn create_method_invocation(rw: &mut ASTRewrite, p: &Parameter<'_>, method: BindingRef<'_>, argument: Option<RNode>) -> RNode {
    let qualifier = p.qualifier.map(|q| rw.create_copy_target(q.id));
    let arguments: Vec<RNode> = argument.into_iter().collect();
    if p.use_super {
        let invocation = rw.new_node(NodeKind::SuperMethodInvocation);
        if let Some(q) = qualifier {
            rw.put_child(invocation, "qualifier", q);
        }
        let name = rw.new_simple_name(method.name());
        rw.put_child(invocation, "name", name);
        rw.put_list(invocation, "arguments", arguments)
    } else {
        rw.new_method_invocation(qualifier, method.name(), arguments)
    }
}

/// `createGetterProposal(context, relevance)`.
async fn create_getter_proposal(env: &Env<'_>, ast: &Arc<Ast>, p: &Parameter<'_>, names: &Names<'_>, relevance: i32) -> Option<Proposal> {
    if let Some(method) = find_getter(p, names) {
        let mut rw = ASTRewrite::new(ast.clone());
        let mi = create_method_invocation(&mut rw, p, method, None);
        rw.replace(RNode::Orig(p.access_node.id), Some(mi));
        let label = messages::format(messages::correction("GetterSetterCorrectionSubProcessor_replacewithgetter_description"), &[&as_string(ast, p.access_node)]);
        return Some(Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw));
    }
    self_encapsulate_proposal(env, ast, p.variable, relevance).await
}

/// `createSetterProposal(context, relevance)`.
async fn create_setter_proposal(env: &Env<'_>, ast: &Arc<Ast>, p: &Parameter<'_>, names: &Names<'_>, relevance: i32) -> Option<Proposal> {
    let setter_name = names.setter(p.variable, is_boolean(p));
    let declaring = p.variable.declaring_class()?;
    let var_type = p.variable.var_type()?;
    let method = crate::correction::unresolved_elements::find_method_in_hierarchy(declaring, &setter_name, Some(&[var_type]));
    if let Some(method) = method.filter(|mb| checks::is_void(mb.return_type()) && same_static(mb.modifiers(), p.variable.modifiers())) {
        let mut rw = ASTRewrite::new(ast.clone());
        let assigned = assigned_value(&mut rw, p, names)?;
        let mi = create_method_invocation(&mut rw, p, method, Some(assigned));
        let parent = p.access_node.parent()?;
        rw.replace(RNode::Orig(parent.id), Some(mi));
        let label = messages::format(messages::correction("GetterSetterCorrectionSubProcessor_replacewithsetter_description"), &[&as_string(ast, p.access_node)]);
        return Some(Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw));
    }
    self_encapsulate_proposal(env, ast, p.variable, relevance).await
}

/// `getAssignedValue(context)` with `GetterSetterUtil.getAssignedValue`.
fn assigned_value(rw: &mut ASTRewrite, p: &Parameter<'_>, names: &Names<'_>) -> Option<RNode> {
    let node = p.access_node.parent()?;
    let variable_type = p.variable.var_type()?;
    if is_not_in_block(node) {
        return None;
    }
    let getter = find_getter(p, names);
    let op = match node.kind() {
        NodeKind::Assignment => {
            let rhs = node.child("rightHandSide")?;
            let operator = node.simple("operator").unwrap_or("=");
            if operator == "=" {
                let copied = rw.create_copy_target(rhs.id);
                let parens = cast_needs_parentheses(rhs);
                return Some(narrow_cast_if_necessary(rw, copied, parens, rhs.type_binding(), variable_type));
            }
            let getter_expression = create_method_invocation(rw, p, getter?, None);
            let infix_op = operator.trim_end_matches('=');
            let mut copied = rw.create_copy_target(rhs.id);
            if parentheses::needs_parentheses_for_right_operand_of_new_infix(rhs, infix_op, Some(variable_type)) {
                copied = rw.new_parenthesized_expression(copied);
            }
            let infix = rw.new_infix_expression(getter_expression, infix_op, copied);
            return Some(narrow_cast_if_necessary(rw, infix, true, None, variable_type));
        }
        NodeKind::PostfixExpression | NodeKind::PrefixExpression => match node.simple("operator") {
            Some("++") => "+",
            Some("--") => "-",
            _ => return None,
        },
        _ => return None,
    };
    let getter_expression = create_method_invocation(rw, p, getter?, None);
    let one = rw.new_number_literal("1");
    let infix = rw.new_infix_expression(getter_expression, op, one);
    Some(narrow_cast_if_necessary(rw, infix, true, None, variable_type))
}

/// `NecessaryParenthesesChecker.needsParentheses(expression, castExpression, EXPRESSION_PROPERTY)`
/// for (a copy of) an original expression.
fn cast_needs_parentheses(expression: Node<'_>) -> bool {
    if !parentheses::expression_type_needs_parentheses(expression.kind()) || expression.is(NodeKind::PrefixExpression) {
        return false;
    }
    parentheses::expression_precedence(expression) < parentheses::expression_precedence_of(NodeKind::CastExpression, None)
}

/// `GetterSetterUtil.isNotInBlock(parent)`.
fn is_not_in_block(parent: Node<'_>) -> bool {
    let Some(statement) = parent.parent() else { return true };
    let is_statement = !statement.is(NodeKind::ExpressionStatement);
    let block = statement.parent();
    let is_block = block.is_some_and(|b| b.is(NodeKind::Block) || b.is(NodeKind::SwitchStatement));
    let is_control_statement_body = checks::is_control_statement_body(statement.location(), block);
    is_statement || (!is_block && !is_control_statement_body)
}

/// `GetterSetterUtil.createNarrowCastIfNessecary(expression, expressionType, ast, variableType)`.
fn narrow_cast_if_necessary(rw: &mut ASTRewrite, expression: RNode, needs_parentheses: bool, expression_type: Option<BindingRef<'_>>, variable_type: BindingRef<'_>) -> RNode {
    if expression_type.is_some_and(|t| t.key() == variable_type.key()) {
        return expression;
    }
    let cast_to = match variable_type.qualified_name() {
        "java.lang.Character" | "char" => "char",
        "java.lang.Byte" | "byte" => "byte",
        "java.lang.Short" | "short" => "short",
        _ => return expression,
    };
    let cast = rw.new_node(NodeKind::CastExpression);
    let operand = if needs_parentheses { rw.new_parenthesized_expression(expression) } else { expression };
    let typ = rw.new_primitive_type(cast_to);
    rw.put_child(cast, "type", typ);
    rw.put_child(cast, "expression", operand)
}

/// `createFieldGetterProposal` / `createFieldSetterProposal`: a
/// `SelfEncapsulateFieldProposal` when `RefactoringAvailabilityTesterCore.isSelfEncapsulateAvailable(field)`.
async fn self_encapsulate_proposal(env: &Env<'_>, ast: &Arc<Ast>, variable: BindingRef<'_>, relevance: i32) -> Option<Proposal> {
    let field = variable.variable_declaration().unwrap_or(variable);
    let declaring = field.declaring_class()?;
    if !field.is_from_source() || field.is_enum_constant() || declaring.is_interface() || declaring.is_annotation() {
        return None;
    }
    let owner = crate::correction::modifier_corrections::Units::load(env, &ast.uri).await.find(ast, declaring)?;
    let label = messages::format(messages::correction("GetterSetterCorrectionSubProcessor_creategetterunsingencapsulatefield_description"), &[field.name()]);
    let generate_javadoc = crate::features::preferences::get_bool("java.codeGeneration.generateComments").unwrap_or(false);
    let change = sef::SelfEncapsulateField { ast: ast.clone(), owner_uri: owner, field_key: field.key().to_owned(), generate_javadoc };
    Some(Proposal::new(label, kind::REFACTOR, relevance, Change::Lazy(Box::new(change))))
}

mod sef {
    //! `SelfEncapsulateFieldRefactoring` + `AccessAnalyzer`.

    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::{field_model, type_model, Env};
    use crate::correction::{parentheses, CuChange, LazyChange};
    use crate::features::accessors::{naming, templates};
    use crate::features::java_model::TypeKind;
    use crate::rewrite::flattener::Flattener;
    use crate::rewrite::import_rewrite::{DefaultContext, ImportRewrite};
    use crate::rewrite::{ASTRewrite, RNode};
    use crate::semantic_ast::{modifier as m, Ast, BindingRef, Node, NodeKind};

    pub(super) struct SelfEncapsulateField {
        /// The invocation unit.
        pub ast: Arc<Ast>,
        /// The unit declaring the field (`None`: the invocation unit).
        pub owner_uri: Option<String>,
        pub field_key: String,
        pub generate_javadoc: bool,
    }

    /// The refactoring's state after `checkInitialConditions`.
    struct Refactoring<'a> {
        field: BindingRef<'a>,
        fragment: Node<'a>,
        field_decl: Node<'a>,
        getter: String,
        setter: String,
        arg_name: String,
        options: BTreeMap<String, String>,
        profile: templates::Profile,
        generate_javadoc: bool,
    }

    #[tower_lsp::async_trait]
    impl LazyChange for SelfEncapsulateField {
        async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
            let owner = match &self.owner_uri {
                None => self.ast.clone(),
                Some(uri) => crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(uri)?).await?,
            };
            let owner_url = tower_lsp::lsp_types::Url::parse(&owner.uri)?;
            let options = env.options(&owner.uri).await;
            let profile = crate::features::accessors::profile(env.dispatcher, &owner_url).await;
            let Some(r) = Refactoring::initialize(&owner, &self.field_key, options, profile, self.generate_javadoc) else {
                return Ok(Vec::new());
            };
            // checkFinalConditions: checkMethodNames
            let field_type = r.field.var_type();
            let (read_names, modify_names) = r.used_names(field_type);
            let using_local_getter = read_names.iter().any(|m| m.name() == r.getter);
            let using_local_setter = modify_names.iter().any(|m| m.name() == r.setter);
            let field_static = r.field.modifiers() & m::STATIC != 0;
            for (name, used) in [(&r.getter, &read_names), (&r.setter, &modify_names)] {
                if let Some(method) = used.iter().find(|m| m.name() == name) {
                    if method.modifiers() & m::STATIC == 0 && field_static {
                        // SelfEncapsulateFieldRefactoring_nonstatic_method_but_static_field
                        return Ok(Vec::new());
                    }
                }
            }
            // RefactoringSearchEngine.findAffectedCompilationUnits
            let ctx = env.dispatcher.context_for(Some(&owner_url)).await;
            let field_name = r.field.name();
            let mut others: Vec<&String> = ctx.files.iter().filter(|(u, src)| **u != owner.uri && src.contains(field_name)).map(|(u, _)| u).collect();
            others.sort();
            let mut units = vec![owner.clone()];
            for uri in others {
                let Ok(ast) = crate::semantic_ast::fetch_with(env.dispatcher, uri, ctx.clone()).await else { continue };
                if references(&ast, &self.field_key) {
                    units.push(ast);
                }
            }
            let declaring_type = r.field.declaring_class().map(|t| t.qualified_name().to_owned()).unwrap_or_default();
            let is_field_final = r.field.modifiers() & m::FINAL != 0;
            let mut setter_must_return_value = false;
            let mut rewrites = Vec::new();
            for unit in &units {
                let mut rw = ASTRewrite::new(unit.clone());
                let mut analyzer = AccessAnalyzer {
                    rw: &mut rw,
                    field_key: &self.field_key,
                    getter: &r.getter,
                    setter: &r.setter,
                    is_field_final,
                    setter_must_return_value: false,
                    remove_static_import: false,
                    referencing_getter: false,
                    referencing_setter: false,
                };
                analyzer.visit(unit.root());
                setter_must_return_value |= analyzer.setter_must_return_value;
                let imports = analyzer.remove_static_import.then(|| {
                    let mut imports = ImportRewrite::create_for_corrections(unit.clone(), &r.options);
                    imports.remove_static_import(&format!("{declaring_type}.{field_name}"));
                    if analyzer.referencing_getter {
                        imports.add_static_import(&declaring_type, &r.getter, false, &DefaultContext);
                    }
                    if analyzer.referencing_setter {
                        imports.add_static_import(&declaring_type, &r.setter, false, &DefaultContext);
                    }
                    imports
                });
                rewrites.push((rw, imports));
            }
            r.add_getter_setter_changes(&mut rewrites[0].0, using_local_getter, using_local_setter, setter_must_return_value)?;
            // The owner's change is created last.
            rewrites.rotate_left(1);
            Ok(rewrites
                .into_iter()
                .map(|(rw, imports)| {
                    let change = CuChange::rewrite(rw);
                    match imports {
                        Some(i) => change.with_imports(i),
                        None => change,
                    }
                })
                .collect())
        }
    }

    /// Whether `ast` references the field (the search engine's match).
    fn references(ast: &Ast, field_key: &str) -> bool {
        ast.all_nodes().any(|n| {
            matches!(n.kind(), NodeKind::SimpleName | NodeKind::ImportDeclaration)
                && !(n.is(NodeKind::SimpleName) && crate::refactoring::extract_temp::is_declaration(n))
                && is_field(n.binding(), field_key)
        })
    }

    fn is_field(binding: Option<BindingRef<'_>>, field_key: &str) -> bool {
        binding.is_some_and(|b| b.is_variable() && b.variable_declaration().unwrap_or(b).key() == field_key)
    }

    impl<'a> Refactoring<'a> {
        /// The constructor's `initialize(field)` and `checkInitialConditions`.
        fn initialize(owner: &'a Ast, field_key: &str, options: BTreeMap<String, String>, profile: templates::Profile, generate_javadoc: bool) -> Option<Self> {
            let field = owner.binding_by_key(field_key)?;
            let fragment = field.declaring_node()?;
            if !fragment.is(NodeKind::VariableDeclarationFragment) {
                return None;
            }
            let field_decl = fragment.parent().filter(|p| p.is(NodeKind::FieldDeclaration))?;
            let declaring = field.declaring_class()?;
            let is_boolean = field.var_type().is_some_and(|t| t.is_primitive() && t.name() == "boolean");
            let model = field_model(field.name(), field.modifiers(), is_boolean);
            let kind = if declaring.is_record() { TypeKind::Record } else { TypeKind::Class };
            let getter = naming::getter(&type_model(kind, declaring.name()), &model, &options, profile.use_is);
            let setter = naming::setter(&model, &options, profile.use_is);
            let mut arg_name = naming::argument(&model, &options);
            // checkArgName
            if field.modifiers() & m::STATIC != 0 && arg_name == field.name() && field.name() == declaring.name() {
                arg_name = format!("_{arg_name}");
            }
            Some(Refactoring { field, fragment, field_decl, getter, setter, arg_name, options, profile, generate_javadoc })
        }

        /// `computeUsedNames()`.
        fn used_names(&self, field_type: Option<BindingRef<'a>>) -> (Vec<BindingRef<'a>>, Vec<BindingRef<'a>>) {
            let mut read = Vec::new();
            let mut modify = Vec::new();
            let methods = self.field.declaring_class().and_then(|c| c.declared_methods()).unwrap_or_default();
            for method in methods {
                let parameters = method.parameter_types();
                if parameters.is_empty() {
                    read.push(method);
                } else if parameters.len() == 1 && field_type.is_some_and(|t| t.key() == parameters[0].key()) {
                    modify.push(method);
                }
            }
            (read, modify)
        }

        /// `addGetterSetterChanges(root, rewriter, lineDelimiter, usingLocalSetter, usingLocalGetter)`.
        fn add_getter_setter_changes(&self, rw: &mut ASTRewrite, using_local_getter: bool, using_local_setter: bool, setter_must_return_value: bool) -> anyhow::Result<()> {
            let Some(type_node) = self.field_decl.parent() else { return Ok(()) };
            // fInsertionIndex == 0: after the first method, else at the end.
            let mut position = 0;
            for member in type_node.list("bodyDeclarations") {
                position += 1;
                if member.is(NodeKind::MethodDeclaration) {
                    break;
                }
            }
            let eol = crate::rewrite::analyzer::default_line_delimiter(&rw.ast.source);
            if !using_local_getter {
                let getter = self.create_getter_method(rw, &eol)?;
                rw.list_insert_at(RNode::Orig(type_node.id), "bodyDeclarations", getter, position);
                position += 1;
            }
            if self.field.modifiers() & m::FINAL == 0 && !using_local_setter {
                let setter = self.create_setter_method(rw, &eol, setter_must_return_value)?;
                rw.list_insert_at(RNode::Orig(type_node.id), "bodyDeclarations", setter, position);
            }
            if self.field.modifiers() & m::PRIVATE == 0 {
                // makeDeclarationPrivate
                crate::correction::modifier_corrections::rewrite_field_modifiers(rw, self.field_decl, self.fragment, m::PRIVATE, m::PROTECTED | m::PUBLIC);
            }
            Ok(())
        }

        /// `createModifiers()` (visibility `public`).
        fn modifiers(&self) -> i32 {
            m::PUBLIC | (self.field.modifiers() & m::STATIC)
        }

        /// `getTypeName(field.getParent())`.
        fn type_name(&self, rw: &ASTRewrite) -> String {
            let Some(parent) = self.field_decl.parent() else { return String::new() };
            if parent.is(NodeKind::AnonymousClassDeclaration) {
                return parent
                    .parent()
                    .and_then(|c| c.child("type"))
                    .map(|t| Flattener::as_string(rw, RNode::Orig(t.id)))
                    .unwrap_or_default();
            }
            parent.child("name").map(|n| n.identifier()).unwrap_or_default()
        }

        fn template_text(text: &str, eol: &str) -> String {
            if eol == "\n" {
                text.to_owned()
            } else {
                text.replace('\n', eol)
            }
        }

        /// The `NonNull` / `Nullable` annotations of the field, copied.
        fn null_annotations(&self, rw: &mut ASTRewrite) -> Vec<RNode> {
            self.field_decl
                .list("modifiers")
                .into_iter()
                .filter(|a| a.kind().is_annotation())
                .filter(|a| a.child("typeName").is_some_and(|n| matches!(n.identifier().as_str(), "NonNull" | "Nullable")))
                .map(|a| rw.create_copy_target(a.id))
                .collect()
        }

        fn vars<'v>(&'v self, method: &'v str, field_access: &'v str, field_type: &'v str, bare: &'v str, type_name: &'v str) -> templates::AccessorVars<'v> {
            templates::AccessorVars {
                field: self.field.name(),
                field_access,
                bare_field_name: bare,
                field_type,
                param: &self.arg_name,
                method,
                enclosing_type: type_name,
            }
        }

        fn bare_name(&self) -> String {
            let model = field_model(self.field.name(), self.field.modifiers(), false);
            naming::base(&model, &self.options, true)
        }

        /// `createGetterMethod(ast, rewriter, lineDelimiter)`.
        fn create_getter_method(&self, rw: &mut ASTRewrite, eol: &str) -> anyhow::Result<RNode> {
            let typ = self.field_decl.child("type").ok_or_else(|| anyhow::anyhow!("field without type"))?;
            let method = rw.new_node(NodeKind::MethodDeclaration);
            let name = rw.new_simple_name(&self.getter);
            rw.put_child(method, "name", name);
            let mut modifiers = rw.new_modifiers(self.modifiers());
            modifiers.extend(self.null_annotations(rw));
            rw.put_list(method, "modifiers", modifiers);
            // DimensionRewrite.copyTypeAndAddDimensions
            let dimensions = self.fragment.list("extraDimensions");
            let return_type = if dimensions.is_empty() {
                rw.create_copy_target(typ.id)
            } else {
                let code = format!("{}{}", Flattener::as_string(rw, RNode::Orig(typ.id)), "[]".repeat(dimensions.len()));
                rw.create_string_placeholder(&code, NodeKind::ArrayType)
            };
            rw.put_child(method, "returnType2", return_type);
            rw.put_list(method, "parameters", Vec::new());
            let type_name = self.type_name(rw);
            let type_string = Flattener::as_string(rw, RNode::Orig(typ.id));
            let bare = self.bare_name();
            let body = {
                let vars = self.vars(&self.getter, self.field.name(), &type_string, &bare, &type_name);
                templates::accessor_body(true, &vars, &self.options, &self.profile, &rw.ast)?
            };
            let statement = if !body.trim().is_empty() {
                rw.create_string_placeholder(&Self::template_text(&body, eol), NodeKind::Block)
            } else {
                let name = rw.new_simple_name(self.field.name());
                rw.new_return_statement(Some(name))
            };
            let block = rw.new_block(vec![statement]);
            rw.put_child(method, "body", block);
            if self.generate_javadoc {
                let vars = self.vars(&self.getter, self.field.name(), &type_string, &bare, &type_name);
                if let Some(comment) = templates::accessor_comment(true, &vars, &self.options, &self.profile, &rw.ast)? {
                    let javadoc = rw.create_string_placeholder(&Self::template_text(&comment, eol), NodeKind::Javadoc);
                    rw.put_child(method, "javadoc", javadoc);
                }
            }
            Ok(method)
        }

        /// `createFieldAccess()`.
        fn field_access(&self) -> String {
            let field_name = self.field.name();
            let name_conflict = self.arg_name == field_name;
            if self.field.modifiers() & m::STATIC != 0 {
                if name_conflict {
                    let declaring = self.field.declaring_class().map(|c| c.name()).unwrap_or("");
                    return format!("{declaring}.{field_name}");
                }
            } else if name_conflict || self.profile.use_this {
                return format!("this.{field_name}");
            }
            field_name.to_owned()
        }

        /// `createSetterMethod(ast, rewriter, lineDelimiter)`.
        fn create_setter_method(&self, rw: &mut ASTRewrite, eol: &str, setter_must_return_value: bool) -> anyhow::Result<RNode> {
            let typ = self.field_decl.child("type").ok_or_else(|| anyhow::anyhow!("field without type"))?;
            let method = rw.new_node(NodeKind::MethodDeclaration);
            let name = rw.new_simple_name(&self.setter);
            rw.put_child(method, "name", name);
            let modifiers = rw.new_modifiers(self.modifiers());
            rw.put_list(method, "modifiers", modifiers);
            let return_type = if setter_must_return_value { rw.create_copy_target(typ.id) } else { rw.new_primitive_type("void") };
            rw.put_child(method, "returnType2", return_type);
            let param = rw.new_node(NodeKind::SingleVariableDeclaration);
            let annotations = self.null_annotations(rw);
            rw.put_list(param, "modifiers", annotations);
            let param_type = rw.create_copy_target(typ.id);
            rw.put_child(param, "type", param_type);
            rw.put_simple(param, "varargs", "false");
            let param_name = rw.new_simple_name(&self.arg_name);
            rw.put_child(param, "name", param_name);
            let dimensions: Vec<RNode> = self.fragment.list("extraDimensions").into_iter().map(|d| rw.create_copy_target(d.id)).collect();
            rw.put_list(param, "extraDimensions", dimensions);
            rw.put_list(method, "parameters", vec![param]);
            let field_access = self.field_access();
            let type_name = self.type_name(rw);
            let type_string = Flattener::as_string(rw, RNode::Orig(typ.id));
            let bare = self.bare_name();
            let body = {
                let vars = self.vars(&self.setter, &field_access, &type_string, &bare, &type_name);
                templates::accessor_body(false, &vars, &self.options, &self.profile, &rw.ast)?
            };
            let mut statements = Vec::new();
            if !body.trim().is_empty() {
                statements.push(rw.create_string_placeholder(&Self::template_text(&body, eol), NodeKind::Block));
            } else {
                let lhs = rw.create_string_placeholder(&field_access, NodeKind::QualifiedName);
                let rhs = rw.new_simple_name(&self.arg_name);
                statements.push(rw.new_assignment(lhs, "=", rhs));
            }
            if setter_must_return_value {
                let name = rw.new_simple_name(&self.arg_name);
                statements.push(rw.new_return_statement(Some(name)));
            }
            let block = rw.new_block(statements);
            rw.put_child(method, "body", block);
            if self.generate_javadoc {
                let vars = self.vars(&self.setter, &field_access, &type_string, &bare, &type_name);
                if let Some(comment) = templates::accessor_comment(false, &vars, &self.options, &self.profile, &rw.ast)? {
                    let javadoc = rw.create_string_placeholder(&Self::template_text(&comment, eol), NodeKind::Javadoc);
                    rw.put_child(method, "javadoc", javadoc);
                }
            }
            Ok(method)
        }
    }

    /// `AccessAnalyzer` (getter and setter names are never empty here).
    struct AccessAnalyzer<'r> {
        rw: &'r mut ASTRewrite,
        field_key: &'r str,
        getter: &'r str,
        setter: &'r str,
        is_field_final: bool,
        setter_must_return_value: bool,
        remove_static_import: bool,
        referencing_getter: bool,
        referencing_setter: bool,
    }

    impl AccessAnalyzer<'_> {
        fn visit(&mut self, node: Node<'_>) {
            let descend = match node.kind() {
                NodeKind::Assignment => self.visit_assignment(node),
                NodeKind::SimpleName => {
                    self.visit_simple_name(node);
                    true
                }
                NodeKind::ImportDeclaration => {
                    if is_field(node.binding(), self.field_key) {
                        self.remove_static_import = true;
                    }
                    false
                }
                NodeKind::PrefixExpression => self.visit_prefix(node),
                NodeKind::PostfixExpression => self.visit_postfix(node),
                NodeKind::MethodDeclaration => {
                    let name = node.child("name").map(|n| n.identifier()).unwrap_or_default();
                    name != self.getter && name != self.setter
                }
                // ASTVisitor() does not visit doc tags.
                NodeKind::Javadoc => false,
                _ => true,
            };
            if descend {
                for child in node.children() {
                    self.visit(child);
                }
            }
        }

        fn consider(&self, binding: Option<BindingRef<'_>>) -> bool {
            is_field(binding, self.field_key)
        }

        fn check_parent(&mut self, node: Node<'_>) {
            if !node.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement)) {
                self.setter_must_return_value = true;
            }
        }

        fn visit_assignment(&mut self, node: Node<'_>) -> bool {
            let Some(lhs) = node.child("leftHandSide") else { return true };
            if !self.consider(resolve_binding(lhs)) {
                return true;
            }
            self.check_parent(node);
            let Some(rhs) = node.child("rightHandSide") else { return false };
            if !self.is_field_final {
                let receiver = receiver(lhs);
                let operator = node.simple("operator").unwrap_or("=");
                let argument = if operator == "=" {
                    self.rw.create_copy_target(rhs.id)
                } else {
                    let infix_op = operator.trim_end_matches('=');
                    let getter_receiver = receiver.map(|r| self.rw.create_copy_target(r.id));
                    let getter = self.rw.new_method_invocation(getter_receiver, self.getter, Vec::new());
                    self.referencing_getter = true;
                    let mut copied = self.rw.create_copy_target(rhs.id);
                    if parentheses::needs_parentheses_for_right_operand_of_new_infix(rhs, infix_op, lhs.type_binding()) {
                        copied = self.rw.new_parenthesized_expression(copied);
                    }
                    self.rw.new_infix_expression(getter, infix_op, copied)
                };
                let setter_receiver = receiver.map(|r| self.rw.create_copy_target(r.id));
                let invocation = self.rw.new_method_invocation(setter_receiver, self.setter, vec![argument]);
                self.referencing_setter = true;
                self.rw.replace(RNode::Orig(node.id), Some(invocation));
            }
            self.visit(rhs);
            false
        }

        fn visit_simple_name(&mut self, node: Node<'_>) {
            if !crate::refactoring::extract_temp::is_declaration(node) && self.consider(node.binding()) {
                self.referencing_getter = true;
                let placeholder = self.rw.create_string_placeholder(&format!("{}()", self.getter), NodeKind::MethodInvocation);
                self.rw.replace(RNode::Orig(node.id), Some(placeholder));
            }
        }

        fn visit_prefix(&mut self, node: Node<'_>) -> bool {
            let Some(operand) = node.child("operand") else { return true };
            if !self.consider(resolve_binding(operand)) {
                return true;
            }
            let operator = node.simple("operator").unwrap_or("");
            if operator != "++" && operator != "--" {
                return true;
            }
            self.check_parent(node);
            let invocation = self.create_invocation(operand, operator);
            self.rw.replace(RNode::Orig(node.id), Some(invocation));
            false
        }

        fn visit_postfix(&mut self, node: Node<'_>) -> bool {
            let Some(operand) = node.child("operand") else { return true };
            if !self.consider(resolve_binding(operand)) {
                return true;
            }
            if !node.parent().is_some_and(|p| p.is(NodeKind::ExpressionStatement)) {
                // SelfEncapsulateField_AccessAnalyzer_cannot_convert_postfix_expression (error)
                return false;
            }
            let operator = node.simple("operator").unwrap_or("");
            let invocation = self.create_invocation(operand, operator);
            self.rw.replace(RNode::Orig(node.id), Some(invocation));
            false
        }

        /// `createInvocation(ast, operand, operator)`.
        fn create_invocation(&mut self, operand: Node<'_>, operator: &str) -> RNode {
            let receiver = receiver(operand);
            let infix_op = if operator == "--" { "-" } else { "+" };
            let getter_receiver = receiver.map(|r| self.rw.create_copy_target(r.id));
            let getter = self.rw.new_method_invocation(getter_receiver, self.getter, Vec::new());
            let one = self.rw.new_number_literal("1");
            let argument = self.rw.new_infix_expression(getter, infix_op, one);
            let setter_receiver = receiver.map(|r| self.rw.create_copy_target(r.id));
            self.referencing_getter = true;
            self.referencing_setter = true;
            self.rw.new_method_invocation(setter_receiver, self.setter, vec![argument])
        }
    }

    /// `AccessAnalyzer.resolveBinding(expression)`.
    fn resolve_binding(expression: Node<'_>) -> Option<BindingRef<'_>> {
        match expression.kind() {
            NodeKind::SimpleName | NodeKind::QualifiedName => expression.binding(),
            NodeKind::FieldAccess | NodeKind::SuperFieldAccess => expression.child("name").and_then(|n| n.binding()),
            NodeKind::ParenthesizedExpression => expression.child("expression").and_then(resolve_binding),
            _ => None,
        }
    }

    /// `AccessAnalyzer.getReceiver(expression)`.
    fn receiver(expression: Node<'_>) -> Option<Node<'_>> {
        match expression.kind() {
            NodeKind::QualifiedName => expression.child("qualifier"),
            NodeKind::FieldAccess => expression.child("expression"),
            NodeKind::ParenthesizedExpression => expression.child("expression").and_then(receiver),
            _ => None,
        }
    }
}
