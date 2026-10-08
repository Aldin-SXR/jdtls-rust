//! AssignToVariableAssistProposalCore expression assignments and refactor proposals.
use super::uncaught;
use crate::{
    correction::{edit::Env, kind, messages, relevance, Change, Context, CuChange, Proposal},
    features::{accessors, completion::naming},
    rewrite::{
        import_rewrite::{ImportRewrite, TypeLocation},
        scanner::{Tok, TokenScanner},
        ASTRewrite, RNode,
    },
    semantic_ast::{
        modifier, nflag,
        resolve::{find_parent_body_declaration, find_parent_statement, find_parent_type},
        BindingRef, Node, NodeKind,
    },
};
use std::collections::{BTreeMap, HashSet};

pub(crate) fn used_names(node: Node<'_>) -> Vec<String> {
    let mut names = uncaught::visible_locals(node);
    if let Some(block) = node.ancestors().find(|n| n.is(NodeKind::Block)) {
        fn after(node: Node<'_>, end: usize, names: &mut HashSet<String>) {
            if matches!(
                node.kind(),
                NodeKind::AnonymousClassDeclaration | NodeKind::TypeDeclarationStatement
            ) {
                return;
            }
            if matches!(
                node.kind(),
                NodeKind::VariableDeclarationFragment | NodeKind::SingleVariableDeclaration
            ) {
                if node.start() > end {
                    if let Some(name) = node.child("name") {
                        names.insert(name.identifier());
                    }
                }
                return;
            }
            for child in node.children() {
                after(child, end, names);
            }
        }
        after(block, node.end(), &mut names);
    }
    let Some(context) = find_parent_type(node).and_then(|n| n.binding()) else {
        return names.into_iter().collect();
    };
    for owner in std::iter::once(node).chain(node.ancestors()).filter(|n| {
        n.kind().is_abstract_type_declaration() || n.is(NodeKind::AnonymousClassDeclaration)
    }) {
        fn fields(
            typ: BindingRef<'_>,
            context: BindingRef<'_>,
            names: &mut HashSet<String>,
            seen: &mut HashSet<String>,
        ) {
            if !seen.insert(typ.key().to_owned()) {
                return;
            }
            for field in typ.declared_fields().unwrap_or_default() {
                let declared = typ.type_declaration().unwrap_or(typ);
                let context = context.type_declaration().unwrap_or(context);
                let visible = !(context.is_class()
                    && context.is_static()
                    && !field.is_static()
                    && declared != context)
                    && (field.modifiers() & modifier::PUBLIC != 0
                        || typ.is_interface()
                        || if field.modifiers() & modifier::PRIVATE == 0 {
                            typ.package_name() == context.package_name()
                                || type_in_scope(
                                    declared,
                                    context,
                                    field.modifiers() & modifier::PROTECTED != 0,
                                )
                        } else {
                            type_in_scope(declared, context, false)
                        });
                if visible {
                    names.insert(field.name().into());
                }
            }
            if let Some(parent) = typ.superclass() {
                fields(parent, context, names, seen);
            }
            for parent in typ.interfaces() {
                fields(parent, context, names, seen);
            }
        }
        if let Some(typ) = owner.binding() {
            fields(typ, context, &mut names, &mut HashSet::new());
        }
    }
    for import in node
        .root()
        .list("imports")
        .iter()
        .filter(|n| n.flag("static") && !n.flag("onDemand"))
    {
        if let Some(name) = import.child("name") {
            names.insert(name.identifier().rsplit('.').next().unwrap_or("").into());
        }
    }
    names.into_iter().collect()
}

// ScopeAnalyzer.isTypeInScope / isInSuperTypeHierarchy compare declarations.
fn type_in_scope(declared: BindingRef<'_>, context: BindingRef<'_>, hierarchy: bool) -> bool {
    fn in_hierarchy(
        declared: BindingRef<'_>,
        typ: BindingRef<'_>,
        seen: &mut HashSet<String>,
    ) -> bool {
        let typ = typ.type_declaration().unwrap_or(typ);
        if !seen.insert(typ.key().into()) {
            return false;
        }
        typ == declared
            || typ
                .superclass()
                .is_some_and(|p| in_hierarchy(declared, p, seen))
            || declared.is_interface()
                && typ
                    .interfaces()
                    .iter()
                    .any(|p| in_hierarchy(declared, *p, seen))
    }
    let mut current = Some(context.type_declaration().unwrap_or(context));
    let mut seen = HashSet::<String>::new();
    while let Some(typ) = current.filter(|t| seen.insert(t.key().into())) {
        if typ == declared || hierarchy && in_hierarchy(declared, typ, &mut HashSet::new()) {
            return true;
        }
        current = typ
            .declaring_class()
            .map(|t| t.type_declaration().unwrap_or(t));
    }
    false
}

fn strip_affixes(name: &str, preference: &str, options: &BTreeMap<String, String>) -> String {
    let mut result = name.to_owned();
    for (suffix, end) in [("Prefixes", false), ("Suffixes", true)] {
        if let Some(value) = options.get(&format!(
            "org.eclipse.jdt.core.codeComplete.{preference}{suffix}"
        )) {
            let mut parts: Vec<_> = value.split(',').filter(|s| !s.is_empty()).collect();
            parts.sort_by_key(|s| std::cmp::Reverse(s.len()));
            if let Some(part) = parts.into_iter().find(|s| {
                result.len() > s.len()
                    && if end {
                        result.ends_with(s)
                    } else {
                        result.starts_with(s)
                    }
            }) {
                if end {
                    result.truncate(result.len() - part.len());
                } else {
                    result = result[part.len()..].into();
                }
            }
        }
    }
    result
}

fn receiver_base(receiver: Node<'_>) -> Option<String> {
    if !receiver.is(NodeKind::SimpleName) {
        return None;
    }
    let mut name = receiver.identifier();
    if name.len() > 3 && name.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("all")) {
        let next = name[3..].chars().next()?;
        if next.is_uppercase() || next == '_' {
            name = name[3..].trim_start_matches('_').into();
        }
    }
    for (plural, singular) in [
        ("Children", "Child"),
        ("Entries", "Entry"),
        ("Proxies", "Proxy"),
        ("Indices", "Index"),
        ("People", "Person"),
        ("Properties", "Property"),
        ("Factories", "Factory"),
        ("Archives", "archive"),
        ("Aliases", "Alias"),
        ("Alternatives", "Alternative"),
        ("Capabilities", "Capability"),
        ("Hashes", "Hash"),
        ("Directories", "Directory"),
        ("Statuses", "Status"),
        ("Instances", "Instance"),
        ("Classes", "Class"),
        ("Deliveries", "Delivery"),
        ("Vertices", "Vertex"),
    ] {
        if name.to_lowercase().ends_with(&plural.to_lowercase()) {
            return Some(format!("{}{singular}", &name[..name.len() - plural.len()]));
        }
    }
    if [
        "ints", "floats", "doubles", "booleans", "bytes", "chars", "shorts", "longs",
    ]
    .iter()
    .any(|s| name.eq_ignore_ascii_case(s))
        || [
            "xes", "ies", "oes", "ses", "hes", "zes", "ves", "ces", "ss", "is", "us", "os", "as",
        ]
        .iter()
        .any(|s| name.to_lowercase().ends_with(s))
    {
        return None;
    }
    if name.len() > 2 && name.ends_with('s') {
        name.pop();
        Some(name)
    } else {
        None
    }
}

fn expression_name(mut expression: Node<'_>, options: &BTreeMap<String, String>) -> Option<String> {
    if expression.is(NodeKind::CastExpression) {
        expression = expression.child("expression")?;
    }
    match expression.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName => {
            let name = expression.identifier().rsplit('.').next()?.to_owned();
            Some(
                if let Some(b) = expression.binding().filter(|b| b.is_variable()) {
                    strip_affixes(
                        &name,
                        if b.is_field() {
                            if b.is_static() {
                                "staticField"
                            } else {
                                "field"
                            }
                        } else if b.is_parameter() {
                            "argument"
                        } else {
                            "local"
                        },
                        options,
                    )
                } else {
                    name
                },
            )
        }
        NodeKind::FieldAccess => expression.child("name").map(|n| n.identifier()),
        NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation => {
            let name = expression.child("name")?.identifier();
            if name == "next" {
                if let Some(base) = expression.child("expression").and_then(receiver_base) {
                    return Some(base);
                }
            }
            for prefix in [
                "get", "is", "to", "create", "load", "find", "build", "generate", "prepare",
                "parse", "current", "read", "resolve", "retrieve", "make", "add", "extract",
            ] {
                if let Some(rest) = name.strip_prefix(prefix) {
                    if rest.is_empty() {
                        return None;
                    }
                    if rest.chars().next().is_some_and(char::is_uppercase) {
                        return Some(rest.into());
                    }
                }
            }
            Some(name)
        }
        _ => None,
    }
}

pub(crate) fn variable_name(
    typ: BindingRef<'_>,
    expression: Node<'_>,
    preference: &str,
    options: &BTreeMap<String, String>,
    used: &[String],
) -> String {
    if let Some(name) = expression_name(expression, options) {
        if let Some(name) =
            naming::suggest_names_with_affixes(&name, 0, used, options, preference).first()
        {
            return name.clone();
        }
    }
    let dimensions = typ.data().dimensions.max(0) as usize;
    let base = typ
        .element_type()
        .unwrap_or(typ)
        .type_declaration()
        .unwrap_or(typ.element_type().unwrap_or(typ));
    naming::suggest_names_with_affixes(base.name(), dimensions, used, options, preference)
        .into_iter()
        .next()
        .unwrap_or_else(|| "name".into())
}

fn needs_semicolon(statement: Node<'_>, expression: Node<'_>) -> bool {
    statement.flags() & nflag::RECOVERED != 0
        && TokenScanner::new(&statement.ast.source)
            .read_next_at(expression.end() as i32, true)
            .is_ok_and(|t| t != Tok::Op(";"))
}

fn control_body(statement: Node<'_>) -> bool {
    statement.parent().is_some_and(|p| match p.kind() {
        NodeKind::IfStatement => {
            statement.location_is("thenStatement") || statement.location_is("elseStatement")
        }
        NodeKind::ForStatement
        | NodeKind::EnhancedForStatement
        | NodeKind::WhileStatement
        | NodeKind::DoStatement => statement.location_is("body"),
        _ => false,
    })
}

pub async fn proposals(env: &Env<'_>, ctx: &Context, selected: Node<'_>, out: &mut Vec<Proposal>) {
    expression_proposals(env, ctx, selected, false, out).await;
}

/// RefactorProcessor / RefactorProposalUtility.getAssign{Variable,Field}Proposal.
pub async fn refactors(
    env: &Env<'_>,
    req: &crate::correction::handler::Request<'_>,
) -> Vec<Proposal> {
    let Some(selected) = req.context.covering_node() else {
        return Vec::new();
    };
    // RefactorProcessor.noErrorsAtLocation: configurable errors are optional.
    if req.locations.iter().any(|p| {
        p.problem_id == crate::semantic_ast::problem::UnusedObjectAllocation
            || p.is_error
                && p.offset <= selected.end()
                && p.offset + p.length >= selected.start()
                && crate::semantic_ast::irritants::option_key_for_problem(p.problem_id).is_none()
    }) {
        return Vec::new();
    }
    let mut proposals = Vec::new();
    expression_proposals(env, &req.context, selected, true, &mut proposals).await;
    let rank = if req.context.selection_length == 0 {
        relevance::EXTRACT_LOCAL_ZERO_SELECTION
    } else if !req.locations.is_empty() {
        relevance::EXTRACT_LOCAL_ERROR
    } else {
        relevance::EXTRACT_LOCAL
    };
    for proposal in &mut proposals {
        let field = proposal.relevance == relevance::ASSIGN_TO_FIELD;
        proposal.kind = if field {
            kind::REFACTOR_ASSIGN_FIELD
        } else {
            kind::REFACTOR_ASSIGN_VARIABLE
        }
        .into();
        proposal.relevance = rank;
        if crate::features::preferences::extended_capability("advancedExtractRefactoringSupport") {
            proposal.command = Some((
                "java.action.applyRefactoringCommand".into(),
                vec![
                    serde_json::json!(if field {
                        "assignField"
                    } else {
                        "assignVariable"
                    }),
                    serde_json::to_value(req.params).expect("serializable code action parameters"),
                ],
            ));
        }
    }
    proposals
}

async fn expression_proposals(
    env: &Env<'_>,
    ctx: &Context,
    selected: Node<'_>,
    refactor: bool,
    out: &mut Vec<Proposal>,
) {
    let Some(statement) =
        find_parent_statement(selected).filter(|n| n.is(NodeKind::ExpressionStatement))
    else {
        return;
    };
    let Some(expression) = statement
        .child("expression")
        .filter(|n| !n.is(NodeKind::Assignment))
    else {
        return;
    };
    let Some(typ) = expression
        .type_binding()
        .filter(|t| !t.is_null_type() && t.name() != "void")
    else {
        return;
    };
    let typ = uncaught::normalize(typ);
    let options = env.options(&ctx.ast.uri).await;
    let context = uncaught::import_context(ctx, statement, &options);
    let used = used_names(statement);
    let local_name = variable_name(typ, expression, "local", &options, &used);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
    let imported = imports.add_import_type(typ, &mut rw, &context, TypeLocation::LocalVariable);
    let copied = rw.create_copy_target(expression.id);
    let fragment = rw.new_variable_declaration_fragment(&local_name, Some(copied));
    if control_body(statement) {
        let moved = rw.create_move_target(statement.id);
        let block = rw.new_block(vec![moved]);
        rw.replace(RNode::Orig(statement.id), Some(block));
    }
    let final_pref = crate::features::preferences::add_final_for_new_declaration();
    let declaration = rw.new_node(if needs_semicolon(statement, expression) {
        NodeKind::VariableDeclarationStatement
    } else {
        NodeKind::VariableDeclarationExpression
    });
    if refactor && matches!(final_pref.as_str(), "all" | "variables") {
        let modifiers = rw.new_modifiers(modifier::FINAL);
        rw.put_list(declaration, "modifiers", modifiers);
    }
    rw.put_child(declaration, "type", imported);
    rw.put_list(declaration, "fragments", vec![fragment]);
    rw.replace(RNode::Orig(expression.id), Some(declaration));
    push(
        rw,
        imports,
        "AssignToVariableAssistProposal_assigntolocal_description",
        relevance::ASSIGN_TO_LOCAL,
        out,
    );

    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ctx.ast.uri) else {
        return;
    };
    let profile = accessors::profile(env.dispatcher, &uri).await;
    if !refactor && uncaught::is_autocloseable(typ, &mut HashSet::new()) {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        let imported = imports.add_import_type(typ, &mut rw, &context, TypeLocation::LocalVariable);
        let copied = rw.create_copy_target(expression.id);
        let fragment = rw.new_variable_declaration_fragment(&local_name, Some(copied));
        let declaration = rw.new_node(NodeKind::VariableDeclarationExpression);
        rw.put_child(declaration, "type", imported);
        rw.put_list(declaration, "fragments", vec![fragment]);
        let modify = statement
            .ancestors()
            .find(|n| n.is(NodeKind::TryStatement))
            .filter(|n| {
                n.child("body").is_some_and(|b| {
                    b.list("statements")
                        .first()
                        .is_some_and(|s| s.id == statement.id)
                })
            });
        let clauses = resource_catches(
            &mut rw,
            &mut imports,
            ctx,
            expression,
            typ,
            modify.is_none(),
            &options,
            &profile,
        );
        if let Some(existing) = modify {
            rw.list_insert_last(RNode::Orig(existing.id), "resources", declaration);
            for clause in clauses {
                rw.list_insert_last(RNode::Orig(existing.id), "catchClauses", clause);
            }
            rw.remove(RNode::Orig(statement.id));
        } else {
            let blank = rw.create_string_placeholder("", NodeKind::EmptyStatement);
            let body = rw.new_block(vec![blank]);
            let statement = rw.new_node(NodeKind::TryStatement);
            rw.put_child(statement, "body", body);
            rw.put_list(statement, "resources", vec![declaration]);
            rw.put_list(statement, "catchClauses", clauses);
            rw.replace(RNode::Orig(expression.id), Some(statement));
        }
        push(
            rw,
            imports,
            "AssignToVariableAssistProposal_assignintrywithresources_description",
            relevance::ASSIGN_IN_TRY_WITH_RESOURCES,
            out,
        );
    }
    if let (Some(owner), Some(body)) = (
        find_parent_type(expression),
        find_parent_body_declaration(expression)
            .filter(|n| n.is(NodeKind::MethodDeclaration) || n.is(NodeKind::Initializer)),
    ) {
        let static_field = body.modifiers() & modifier::STATIC != 0
            && !owner.is(NodeKind::AnonymousClassDeclaration);
        let name = variable_name(
            typ,
            expression,
            if static_field { "staticField" } else { "field" },
            &options,
            &used,
        );
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        let imported = imports.add_import_type(typ, &mut rw, &context, TypeLocation::Field);
        let fragment = rw.new_variable_declaration_fragment(&name, None);
        let modifiers = rw.new_modifiers(
            modifier::PRIVATE
                | if static_field { modifier::STATIC } else { 0 }
                | if refactor && matches!(final_pref.as_str(), "all" | "fields") {
                    modifier::FINAL
                } else {
                    0
                },
        );
        let declaration = rw.new_field_declaration(fragment, modifiers, imported);
        let declarations = owner.list("bodyDeclarations");
        let index = declarations
            .iter()
            .rposition(|n| n.is(NodeKind::FieldDeclaration) && n.end() < statement.start())
            .map_or(0, |i| i + 1);
        rw.list_insert_at(
            RNode::Orig(owner.id),
            "bodyDeclarations",
            declaration,
            index as i32,
        );
        let mut lhs = rw.new_simple_name(&name);
        if profile.use_this {
            let receiver = if static_field {
                rw.new_simple_name(
                    &owner
                        .child("name")
                        .map(|n| n.identifier())
                        .unwrap_or_default(),
                )
            } else {
                rw.new_this_expression()
            };
            lhs = rw.new_field_access(receiver, lhs);
        }
        let rhs = rw.create_copy_target(expression.id);
        let mut assignment = rw.new_assignment(lhs, "=", rhs);
        if needs_semicolon(statement, expression) {
            assignment = rw.new_expression_statement(assignment);
        }
        rw.replace(RNode::Orig(expression.id), Some(assignment));
        push(
            rw,
            imports,
            "AssignToVariableAssistProposal_assigntofield_description",
            relevance::ASSIGN_TO_FIELD,
            out,
        );
    }
}

fn resource_catches(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    ctx: &Context,
    expression: Node<'_>,
    typ: BindingRef<'_>,
    rethrow: bool,
    options: &BTreeMap<String, String>,
    profile: &accessors::templates::Profile,
) -> Vec<RNode> {
    let Some(root) = uncaught::enclosing(expression) else {
        return Vec::new();
    };
    let Some(decl) = find_parent_body_declaration(expression) else {
        return Vec::new();
    };
    let mut thrown = uncaught::exceptions(
        expression.parent().unwrap_or(expression),
        expression.start(),
        expression.end(),
        false,
    );
    let mut current = Some(typ);
    let mut seen = HashSet::new();
    while let Some(typ) = current.filter(|t| seen.insert(t.key().to_owned())) {
        if let Some(close) = typ
            .declared_methods()
            .unwrap_or_default()
            .iter()
            .find(|m| m.name() == "close" && m.parameter_types().is_empty())
            .copied()
        {
            for exception in close.exception_types() {
                uncaught::add(&mut thrown, exception);
            }
            break;
        }
        current = typ.superclass();
    }
    let context = uncaught::import_context(ctx, decl, options);
    let (catch_types, rethrows) = uncaught::resource_exception_types(expression, root, &thrown);
    if catch_types.is_empty() {
        return Vec::new();
    }
    let name = uncaught::exception_name(
        decl,
        expression.start(),
        expression.end(),
        &profile.exception_variable,
        expression.end(),
    );
    let mut clauses = Vec::new();
    if rethrow {
        for typ in rethrows {
            let clause = uncaught::catch_clause(
                rw,
                imports,
                &context,
                &[typ],
                &name,
                expression,
                options,
                profile,
                uncaught::CatchTemplate::General,
            );
            let statement = rw.new_node(NodeKind::ThrowStatement);
            let name = rw.new_simple_name(&name);
            rw.put_child(statement, "expression", name);
            let body = rw.new_block(vec![statement]);
            rw.put_child(clause, "body", body);
            clauses.push(clause);
        }
    }
    clauses.push(uncaught::catch_clause(
        rw,
        imports,
        &context,
        &catch_types,
        &name,
        expression,
        options,
        profile,
        uncaught::CatchTemplate::General,
    ));
    clauses
}

fn push(rw: ASTRewrite, imports: ImportRewrite, key: &str, rank: i32, out: &mut Vec<Proposal>) {
    out.push(Proposal::new(
        messages::correction(key),
        kind::QUICK_FIX,
        rank,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
}

/// GetRefactorEditHandler's assignment commands, including the linked name
/// position after imports, formatting and copied control-flow bodies.
pub async fn get_refactor_edit(
    env: &Env<'_>,
    params: serde_json::Value,
) -> Option<serde_json::Value> {
    use crate::analysis::semantic::diagnostics::Doc16;
    use crate::rewrite::{text_edit::EditKind, Placeholder};
    use serde_json::json;
    let field = match params["command"].as_str()? {
        "assignField" => true,
        "assignVariable" => false,
        _ => return None,
    };
    let action: tower_lsp::lsp_types::CodeActionParams =
        serde_json::from_value(params["context"].clone()).ok()?;
    let uri = &action.text_document.uri;
    let ast = crate::semantic_ast::fetch(env.dispatcher, uri).await.ok()?;
    let doc = Doc16::new(ast.text());
    let locations =
        crate::correction::handler::problem_locations(&doc, &action.context.diagnostics);
    if locations
        .iter()
        .any(|p| p.problem_id == crate::semantic_ast::problem::UnusedObjectAllocation)
    {
        return None;
    }
    let start = doc
        .to_offset(action.range.start.line, action.range.start.character)
        .max(0) as usize;
    let end = doc
        .to_offset(action.range.end.line, action.range.end.character)
        .max(0) as usize;
    let ctx = Context::new(ast.clone(), start, end.saturating_sub(start));
    let mut proposals = Vec::new();
    expression_proposals(env, &ctx, ctx.covering_node()?, true, &mut proposals).await;
    let mut proposal = proposals
        .into_iter()
        .find(|p| (p.relevance == relevance::ASSIGN_TO_FIELD) == field)?;
    let Change::Cu(cus) = &mut proposal.change else {
        return None;
    };
    let cu = cus.first_mut()?;
    let rw = cu.rewrite.as_mut()?;
    let owner = rw.new_nodes.iter().position(|n| {
        n.kind
            == if field {
                NodeKind::Assignment
            } else {
                NodeKind::VariableDeclarationFragment
            }
    })?;
    let mut name_node = rw
        .new_value(
            RNode::New(owner as u32),
            if field { "leftHandSide" } else { "name" },
        )
        .node()?;
    if rw.kind(name_node) == NodeKind::FieldAccess {
        name_node = rw.new_value(name_node, "name").node()?;
    }
    let name = rw.new_value(name_node, "identifier").simple()?.to_owned();
    // A string placeholder creates an individual insertion for this tracked
    // SimpleName; the formatter still sees the real identifier.
    let RNode::New(id) = name_node else {
        return None;
    };
    rw.new_nodes[id as usize].placeholder = Some(Placeholder::Str(name.clone()));
    let tree = crate::correction::edit::cu_tree(env, cu).await.ok()?;
    let insertion = tree
        .edits
        .iter()
        .position(|e| matches!(&e.kind, EditKind::Insert(s) if s == &name))?;
    let result = String::from_utf16_lossy(&tree.apply(&ast.source));
    let mut marker = String::from("\0jdtls-rename\0");
    while result.contains(&marker) {
        marker.push('\0');
    }
    let mut tracked = tree.clone();
    tracked.edits[insertion].kind = EditKind::Insert(marker.clone());
    let tracked_result = String::from_utf16_lossy(&tracked.apply(&ast.source));
    let offset = tracked_result[..tracked_result.find(&marker)?]
        .encode_utf16()
        .count();
    let mut change = Change::Cu(vec![CuChange::edits(ast.clone(), tree)]);
    let edit = crate::correction::edit::to_workspace_edit(env, &mut change)
        .await
        .ok()?;
    Some(
        json!({"edit":edit,"command":{"title":"Rename","command":"java.action.rename","arguments":[{"uri":uri,"offset":offset,"length":name.encode_utf16().count()}]}}),
    )
}
