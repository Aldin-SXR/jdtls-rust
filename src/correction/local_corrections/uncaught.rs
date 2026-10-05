//! LocalCorrectionsBaseSubProcessor's uncaught-exception proposals and the
//! SurroundWithTryCatch refactoring's exception and lexical-scope analysis.
use crate::{
    correction::{
        edit::Env, kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal,
    },
    features::{accessors, constructors::ConstructorImportContext},
    rewrite::{
        import_rewrite::{ImportRewrite, TypeLocation},
        ASTRewrite, RNode,
    },
    semantic_ast::{
        bflag, modifier,
        resolve::{find_parent_body_declaration, find_parent_type},
        BindingRef, Node, NodeKind,
    },
};
use std::collections::{BTreeMap, HashSet};

fn subtype(mut typ: BindingRef<'_>, target: BindingRef<'_>) -> bool {
    let mut seen = HashSet::new();
    loop {
        if typ == target {
            return true;
        }
        if !seen.insert(typ.key().to_owned()) {
            return false;
        }
        let Some(parent) = typ.superclass() else {
            return false;
        };
        typ = parent;
    }
}
fn covered(node: Node<'_>, start: usize, end: usize) -> bool {
    node.start() >= start && node.end() <= end
}
fn lambda_body(node: Node<'_>) -> bool {
    node.location_is("body")
        && node
            .parent()
            .is_some_and(|n| n.is(NodeKind::LambdaExpression))
}
fn functional_method(node: Node<'_>) -> Option<BindingRef<'_>> {
    let typ = if lambda_body(node) {
        node.parent()?.type_binding()?
    } else {
        node.type_binding()?
    };
    typ.data().functional_method.map(|m| typ.ast.binding(m))
}
fn enclosing(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind().is_method_reference() {
        return Some(node);
    }
    if let Some(lambda) = node.ancestors().find(|n| n.is(NodeKind::LambdaExpression)) {
        return lambda.child("body");
    }
    find_parent_body_declaration(node)
}
fn normalize(mut typ: BindingRef<'_>) -> BindingRef<'_> {
    let mut seen = HashSet::new();
    while seen.insert(typ.key().to_owned()) {
        let replacement = if typ.is_null_type() {
            typ.ast.binding_by_key("Ljava/lang/Object;")
        } else if typ.is_anonymous() {
            typ.interfaces()
                .first()
                .copied()
                .or_else(|| typ.superclass())
        } else if typ.is_capture() {
            typ.wildcard()
        } else if typ.is_wildcard_type() {
            if typ.has(bflag::UPPERBOUND) && typ.bound().is_some() {
                typ.bound()
            } else {
                typ.type_bounds().first().copied().or_else(|| typ.erasure())
            }
        } else {
            None
        };
        let Some(replacement) = replacement else {
            break;
        };
        typ = replacement;
    }
    typ
}
fn add<'a>(out: &mut Vec<BindingRef<'a>>, typ: BindingRef<'a>) {
    let typ = normalize(typ);
    if !out.contains(&typ) {
        out.push(typ);
    }
}
fn method_in_hierarchy<'a>(
    typ: BindingRef<'a>,
    name: &str,
    seen: &mut HashSet<String>,
) -> Option<BindingRef<'a>> {
    if !seen.insert(typ.key().to_owned()) {
        return None;
    }
    typ.declared_methods()
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.name() == name && m.parameter_types().is_empty())
        .or_else(|| {
            typ.superclass()
                .and_then(|t| method_in_hierarchy(t, name, seen))
        })
        .or_else(|| {
            typ.interfaces()
                .into_iter()
                .find_map(|t| method_in_hierarchy(t, name, seen))
        })
}
fn caught_types(clause: Node<'_>) -> Vec<BindingRef<'_>> {
    let Some(typ) = clause.child("exception").and_then(|e| e.child("type")) else {
        return Vec::new();
    };
    if typ.is(NodeKind::UnionType) {
        typ.list("types")
            .into_iter()
            .filter_map(|t| t.binding())
            .collect()
    } else {
        typ.binding().into_iter().collect()
    }
}
fn visit_exceptions<'a>(
    node: Node<'a>,
    root: Node<'a>,
    start: usize,
    end: usize,
    out: &mut Vec<BindingRef<'a>>,
) {
    let kind = node.kind();
    if matches!(
        kind,
        NodeKind::AnonymousClassDeclaration
            | NodeKind::LambdaExpression
            | NodeKind::TypeDeclarationStatement
    ) {
        return;
    }
    if kind.is_method_reference() && node.id != root.id {
        return;
    }
    if kind.is_method_reference()
        && functional_method(node).is_none_or(|m| !m.type_parameters().is_empty())
    {
        return;
    }
    if kind == NodeKind::TryStatement {
        let mut nested = Vec::new();
        if let Some(body) = node.child("body") {
            visit_exceptions(body, root, start, end, &mut nested);
        }
        for resource in node.list("resources") {
            visit_exceptions(resource, root, start, end, &mut nested);
        }
        for clause in node.list("catchClauses") {
            for caught in caught_types(clause) {
                nested.retain(|t| !subtype(*t, caught));
            }
        }
        for typ in nested {
            add(out, typ);
        }
        for clause in node.list("catchClauses") {
            visit_exceptions(clause, root, start, end, out);
        }
        if let Some(finally) = node.child("finally") {
            visit_exceptions(finally, root, start, end, out);
        }
        return;
    }
    if matches!(
        kind,
        NodeKind::MethodInvocation
            | NodeKind::SuperMethodInvocation
            | NodeKind::ClassInstanceCreation
            | NodeKind::ConstructorInvocation
            | NodeKind::SuperConstructorInvocation
            | NodeKind::VariableDeclarationExpression
    ) || kind.is_method_reference()
    {
        if !covered(node, start, end) {
            return;
        }
        if let Some(method) = node.method_binding() {
            for typ in method.exception_types() {
                add(out, typ);
            }
        }
        if kind == NodeKind::VariableDeclarationExpression && node.location_is("resources") {
            if let Some(close) = node
                .child("type")
                .and_then(|n| n.binding())
                .and_then(|t| method_in_hierarchy(t, "close", &mut HashSet::new()))
            {
                for typ in close.exception_types() {
                    add(out, typ);
                }
            }
        }
    }
    if kind == NodeKind::ThrowStatement && covered(node, start, end) {
        if let Some(typ) = node
            .child("expression")
            .and_then(|e| e.type_binding())
            .filter(|t| {
                let mut t = Some(*t);
                while let Some(current) = t {
                    if current.qualified_name() == "java.lang.RuntimeException" {
                        return false;
                    }
                    t = current.superclass();
                }
                true
            })
        {
            add(out, typ);
        }
    }
    for child in node.children() {
        visit_exceptions(child, root, start, end, out);
    }
}
fn exceptions(
    root: Node<'_>,
    start: usize,
    end: usize,
    remove_declared: bool,
) -> Vec<BindingRef<'_>> {
    let mut out = Vec::new();
    visit_exceptions(root, root, start, end, &mut out);
    let declared = if root.is(NodeKind::MethodDeclaration) && remove_declared {
        root.list("thrownExceptionTypes")
            .into_iter()
            .filter_map(|t| t.binding())
            .collect::<Vec<_>>()
    } else if lambda_body(root) || root.kind().is_method_reference() {
        functional_method(root)
            .map(|m| m.exception_types())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    out.retain(|t| !declared.iter().any(|d| subtype(*t, *d)));
    fn depth(mut typ: BindingRef<'_>) -> usize {
        let mut count = 0;
        let mut seen = HashSet::new();
        while seen.insert(typ.key().to_owned()) {
            count += 1;
            let Some(parent) = typ.superclass() else {
                break;
            };
            typ = parent;
        }
        count
    }
    out.sort_by_key(|t| std::cmp::Reverse(depth(*t)));
    out
}
fn filter_subtypes<'a>(types: &[BindingRef<'a>]) -> Vec<BindingRef<'a>> {
    types
        .iter()
        .copied()
        .filter(|t| !types.iter().any(|s| t != s && subtype(*t, *s)))
        .collect()
}

fn lambda_statement(rw: &mut ASTRewrite, expression: RNode, method: BindingRef<'_>) -> RNode {
    if method.return_type().is_some_and(|t| t.name() == "void") {
        rw.new_expression_statement(expression)
    } else {
        rw.new_return_statement(Some(expression))
    }
}

/// ASTNodes.getVisibleLocalVariablesInScope: declarations in ancestor lexical
/// scopes, including method/lambda parameters, but excluding fields and siblings.
fn visible_locals(node: Node<'_>) -> HashSet<String> {
    node.root()
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment
            ) && n.start() < node.start()
                && n.binding()
                    .is_some_and(|b| b.is_variable() && !b.is_field())
                && n.ancestors()
                    .find(|p| {
                        matches!(
                            p.kind(),
                            NodeKind::Block
                                | NodeKind::CatchClause
                                | NodeKind::ForStatement
                                | NodeKind::EnhancedForStatement
                                | NodeKind::LambdaExpression
                                | NodeKind::MethodDeclaration
                        )
                    })
                    .is_some_and(|scope| scope.is_ancestor_or_self_of(node))
        })
        .filter_map(|n| n.child("name"))
        .map(|n| n.identifier())
        .collect()
}

fn escapes(statement: Node<'_>, root: Node<'_>, end: usize) -> bool {
    statement.is(NodeKind::VariableDeclarationStatement)
        && statement
            .list("fragments")
            .iter()
            .filter_map(|f| f.binding())
            .any(|binding| {
                root.descendants().any(|n| {
                    n.start() >= end && n.is(NodeKind::SimpleName) && n.binding() == Some(binding)
                })
            })
}

/// LocalDeclarationAnalyzer and SurroundWithTryCatchRefactoring split a
/// declaration whose variable is used after the selection. Its declaration
/// stays in the original scope and its initializers become assignments.
fn split_local(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    context: &ConstructorImportContext,
    statement: Node<'_>,
) -> Option<(RNode, Vec<RNode>)> {
    let declaration = rw.new_node(NodeKind::VariableDeclarationStatement);
    let typ = statement.child("type")?;
    let copied_type = if typ.source_text() == "var" {
        imports.add_import_type(typ.binding()?, rw, context, TypeLocation::LocalVariable)
    } else {
        rw.create_copy_target(typ.id)
    };
    rw.put_child(declaration, "type", copied_type);
    let modifiers = statement
        .list("modifiers")
        .iter()
        .filter(|m| !m.is(NodeKind::Modifier) || m.simple("keyword") != Some("final"))
        .map(|m| rw.create_copy_target(m.id))
        .collect();
    rw.put_list(declaration, "modifiers", modifiers);
    let mut fragments = Vec::new();
    let mut assignments = Vec::new();
    for fragment in statement.list("fragments") {
        let name = fragment.child("name")?;
        let copy = rw.new_variable_declaration_fragment(&name.identifier(), None);
        let dimensions = fragment
            .list("extraDimensions")
            .iter()
            .map(|d| rw.create_copy_target(d.id))
            .collect();
        rw.put_list(copy, "extraDimensions", dimensions);
        fragments.push(copy);
        if let Some(initializer) = fragment.child("initializer") {
            let lhs = rw.create_copy_target(name.id);
            let rhs = rw.create_copy_target(initializer.id);
            let assignment = rw.new_assignment(lhs, "=", rhs);
            assignments.push(rw.new_expression_statement(assignment));
        }
    }
    rw.put_list(declaration, "fragments", fragments);
    Some((declaration, assignments))
}

/// SelectionAwareSourceRangeComputer preserves only the comments that belong
/// to the selected text, and caps extended ranges of the last node's children.
fn selection_ranges(rw: &mut ASTRewrite, nodes: &[Node<'_>], start: usize, end: usize) {
    use crate::rewrite::scanner::{Tok, TokenScanner};
    let (Some(first), Some(last)) = (nodes.first(), nodes.last()) else {
        return;
    };
    let Some(source) = first.ast.source.get(start..end) else {
        return;
    };
    let mut scanner = TokenScanner::new(source);
    let Ok(offset) = scanner.next_start_offset(0, false) else {
        return;
    };
    let new_start = (start + offset as usize).min(first.start());
    rw.set_source_range(
        first.id,
        new_start,
        first.extended_start() + first.extended_length() - new_start,
    );
    let scanner_start = last.end().saturating_sub(start);
    scanner.set_offset(scanner_start as i32);
    let mut pos = scanner_start;
    let mut token = None;
    while let Ok(next) = scanner.read_next(false) {
        token = Some(next);
        pos = scanner.current_end_offset() as usize;
    }
    if token == Some(Tok::CommentLine) {
        while pos > 0 && matches!(source[pos - 1], 10 | 13) {
            pos -= 1;
        }
    }
    let new_end = (start + pos).max(last.end());
    let (range_start, _) = rw.extended_range(last.id);
    rw.set_source_range(last.id, range_start, new_end - range_start);
    let mut node = *last;
    while let Some(child) = node.children().last().copied() {
        if child.extended_start() + child.extended_length() <= new_end {
            break;
        }
        rw.set_source_range(
            child.id,
            child.extended_start(),
            new_end - child.extended_start(),
        );
        node = child;
    }
}

/// QuickAssistProcessorUtil.convertMethodRefernceToLambda. All method and type
/// decisions use the compiler's bindings; the AST transformation lives here.
fn reference_lambda<'a>(
    rw: &mut ASTRewrite,
    node: Node<'a>,
) -> Option<(RNode, RNode, BindingRef<'a>)> {
    let functional = functional_method(node)?;
    if !functional.type_parameters().is_empty() {
        return None;
    }
    let referred = node.method_binding()?;
    let mut excluded = visible_locals(node);
    let original = (0..functional.parameter_types().len())
        .map(|i| {
            functional
                .data()
                .parameter_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("arg{i}"))
        })
        .collect::<Vec<_>>();
    let names = original
        .iter()
        .map(|name| {
            if !excluded.contains(name) {
                return name.clone();
            }
            let candidate = (1..)
                .map(|i| format!("{name}{i}"))
                .find(|n| !excluded.contains(n) && !original.contains(n))
                .unwrap();
            excluded.insert(candidate.clone());
            candidate
        })
        .collect::<Vec<_>>();
    let lambda = rw.new_node(NodeKind::LambdaExpression);
    let parameters = names
        .iter()
        .map(|n| rw.new_variable_declaration_fragment(n, None))
        .collect();
    rw.put_list(lambda, "parameters", parameters);
    rw.put_simple(
        lambda,
        "parentheses",
        if names.len() == 1 { "false" } else { "true" },
    );
    let type_arguments = node
        .list("typeArguments")
        .iter()
        .map(|t| rw.create_copy_target(t.id))
        .collect::<Vec<_>>();
    let mut argument_start = 0;
    let invocation = if node.is(NodeKind::CreationReference) {
        let typ = node.child("type")?;
        if typ.is(NodeKind::ArrayType) {
            let creation = rw.new_node(NodeKind::ArrayCreation);
            let copied = rw.create_copy_target(typ.id);
            rw.put_child(creation, "type", copied);
            let dimension = rw.new_simple_name(names.first()?);
            rw.put_list(creation, "dimensions", vec![dimension]);
            return Some((lambda, creation, functional));
        }
        let creation = rw.new_node(NodeKind::ClassInstanceCreation);
        let mut copied = rw.create_copy_target(typ.id);
        if !typ.is(NodeKind::ParameterizedType)
            && typ
                .binding()
                .and_then(|t| t.type_declaration())
                .is_some_and(|t| t.is_generic_type())
        {
            let parameterized = rw.new_node(NodeKind::ParameterizedType);
            rw.put_child(parameterized, "type", copied);
            copied = parameterized;
        }
        rw.put_child(creation, "type", copied);
        creation
    } else if node.is(NodeKind::SuperMethodReference) && !referred.is_static() {
        let invocation = rw.new_node(NodeKind::SuperMethodInvocation);
        if let Some(qualifier) = node.child("qualifier") {
            let copy = rw.create_copy_target(qualifier.id);
            rw.put_child(invocation, "qualifier", copy);
        }
        let name = rw.create_copy_target(node.child("name")?.id);
        rw.put_child(invocation, "name", name);
        invocation
    } else {
        let invocation = rw.new_node(NodeKind::MethodInvocation);
        let expression = node.child("expression");
        let unbound = node.is(NodeKind::TypeMethodReference)
            || expression.is_some_and(|e| e.binding().is_some_and(|b| b.is_type()));
        let receiver = if referred.is_static() {
            let owner = referred.declaring_class()?;
            let current = find_parent_type(node).and_then(|t| t.binding());
            let conflict = current
                .and_then(|t| t.declared_methods())
                .unwrap_or_default()
                .iter()
                .any(|m| m.name() == referred.name() && *m != referred);
            if !conflict && current.is_some_and(|t| subtype(t, owner)) && type_arguments.is_empty()
            {
                None
            } else {
                expression
                    .or_else(|| node.child("type"))
                    .map(|e| rw.create_copy_target(e.id))
            }
        } else if unbound {
            argument_start = 1;
            Some(rw.new_simple_name(names.first()?))
        } else {
            expression
                .filter(|e| !e.is(NodeKind::ThisExpression) || !type_arguments.is_empty())
                .map(|e| rw.create_copy_target(e.id))
        };
        if let Some(receiver) = receiver {
            rw.put_child(invocation, "expression", receiver);
        }
        let name = rw.create_copy_target(node.child("name")?.id);
        rw.put_child(invocation, "name", name);
        invocation
    };
    let arguments = names
        .iter()
        .skip(argument_start)
        .map(|n| rw.new_simple_name(n))
        .collect();
    rw.put_list(invocation, "arguments", arguments);
    rw.put_list(invocation, "typeArguments", type_arguments);
    Some((lambda, invocation, functional))
}

#[derive(Default)]
struct Scope {
    start: usize,
    end: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    names: HashSet<String>,
}
fn exception_name(
    decl: Node<'_>,
    start: usize,
    end: usize,
    candidate: &str,
    ignore_end: usize,
) -> String {
    fn walk(node: Node<'_>, scope: usize, scopes: &mut Vec<Scope>, start: usize, end: usize) {
        let scope = if matches!(
            node.kind(),
            NodeKind::Block | NodeKind::CatchClause | NodeKind::ForStatement
        ) {
            let index = scopes.len();
            scopes.push(Scope {
                start: node.start(),
                end: node.end(),
                parent: Some(scope),
                ..Scope::default()
            });
            scopes[scope].children.push(index);
            index
        } else {
            scope
        };
        match node.kind() {
            NodeKind::SimpleName => {
                if !covered(node, start, end) {
                    scopes[scope].names.insert(node.identifier());
                }
            }
            NodeKind::QualifiedName => {
                if let Some(q) = node.child("qualifier") {
                    walk(q, scope, scopes, start, end);
                }
            }
            NodeKind::MethodInvocation => {
                if let Some(receiver) = node.child("expression").or_else(|| node.child("name")) {
                    walk(receiver, scope, scopes, start, end);
                }
                for arg in node.list("arguments") {
                    walk(arg, scope, scopes, start, end);
                }
            }
            NodeKind::TypeDeclarationStatement => {
                if let Some(name) = node.child("declaration").and_then(|d| d.child("name")) {
                    scopes[scope].names.insert(name.identifier());
                }
            }
            _ => {
                for child in node.children() {
                    walk(child, scope, scopes, start, end);
                }
            }
        }
    }
    fn find(scopes: &[Scope], index: usize, start: usize, end: usize) -> usize {
        scopes[index]
            .children
            .iter()
            .copied()
            .find(|i| scopes[*i].start <= start && scopes[*i].end >= end)
            .map(|i| find(scopes, i, start, end))
            .unwrap_or(index)
    }
    fn down(scopes: &[Scope], index: usize, name: &str) -> bool {
        scopes[index].names.contains(name)
            || scopes[index]
                .children
                .iter()
                .any(|i| down(scopes, *i, name))
    }
    let mut scopes = vec![Scope {
        start: decl.start(),
        end: decl.end(),
        ..Scope::default()
    }];
    walk(decl, 0, &mut scopes, start, ignore_end);
    let index = find(&scopes, 0, start, end);
    let in_use = |name: &str| {
        let mut scope = Some(index);
        while let Some(i) = scope {
            if scopes[i].names.contains(name) {
                return true;
            }
            scope = scopes[i].parent;
        }
        scopes[index]
            .children
            .iter()
            .any(|i| start < scopes[*i].start && down(&scopes, *i, name))
    };
    if !in_use(candidate) {
        return candidate.into();
    }
    (1..)
        .map(|i| format!("{candidate}{i}"))
        .find(|s| !in_use(s))
        .unwrap()
}
fn import_context(
    ctx: &Context,
    node: Node<'_>,
    options: &BTreeMap<String, String>,
) -> ConstructorImportContext {
    ConstructorImportContext {
        ast: ctx.ast.clone(),
        declaration: find_parent_type(node).map(|n| n.id),
        nullness: crate::rewrite::import_rewrite::nullness::Filter::create(
            ctx.ast(),
            Some(node.id),
            options,
        ),
    }
}
enum CatchTemplate {
    Binding,
    Imported,
    General,
}
fn catch_clause(
    rw: &mut ASTRewrite,
    imports: &mut ImportRewrite,
    context: &ConstructorImportContext,
    types: &[BindingRef<'_>],
    name: &str,
    location: Node<'_>,
    options: &BTreeMap<String, String>,
    profile: &accessors::templates::Profile,
    template: CatchTemplate,
) -> RNode {
    let type_nodes = types
        .iter()
        .map(|t| imports.add_import_type(*t, rw, context, TypeLocation::Exception))
        .collect::<Vec<_>>();
    let typ = if type_nodes.len() == 1 {
        type_nodes[0]
    } else {
        let union = rw.new_node(NodeKind::UnionType);
        rw.put_list(union, "types", type_nodes)
    };
    let parameter = rw.new_node(NodeKind::SingleVariableDeclaration);
    let nm = rw.new_simple_name(name);
    rw.put_child(parameter, "name", nm);
    rw.put_child(parameter, "type", typ);
    let enclosing_type = find_parent_type(location)
        .and_then(|t| t.child("name"))
        .map(|n| n.identifier())
        .unwrap_or_default();
    let enclosing_method = location
        .ancestor_or_self(|k| k == NodeKind::MethodDeclaration)
        .and_then(|m| m.child("name"))
        .map(|n| n.identifier())
        .unwrap_or_default();
    let imported_name;
    let type_name = match template {
        CatchTemplate::General => "Exception",
        CatchTemplate::Binding => types[0].name(),
        CatchTemplate::Imported => {
            imported_name = crate::rewrite::flattener::Flattener::as_string(rw, typ);
            &imported_name
        }
    };
    let todo = options
        .get("org.eclipse.jdt.core.compiler.taskTags")
        .and_then(|s| s.split(',').next())
        .unwrap_or("XXX");
    let body = accessors::templates::expand_template(
        profile.template(
            "catchblock",
            "// ${todo} Auto-generated catch block\n${exception_var}.printStackTrace();",
        ),
        |key| match key {
            "dollar" => Some("$"),
            "todo" => Some(todo),
            "exception_type" => Some(type_name),
            "exception_var" => Some(name),
            "enclosing_type" => Some(enclosing_type.as_str()),
            "enclosing_method" => Some(enclosing_method.as_str()),
            _ => None,
        },
    )
    .unwrap_or_default();
    let statements = if body.trim().is_empty() {
        Vec::new()
    } else {
        vec![rw.create_string_placeholder(&body, NodeKind::ReturnStatement)]
    };
    let block = rw.new_block(statements);
    let clause = rw.new_node(NodeKind::CatchClause);
    rw.put_child(clause, "exception", parameter);
    rw.put_child(clause, "body", block)
}
fn push(
    rw: ASTRewrite,
    imports: ImportRewrite,
    key: &str,
    rank: i32,
    proposals: &mut Vec<Proposal>,
) {
    proposals.push(Proposal::new(
        messages::correction(key),
        kind::QUICK_FIX,
        rank,
        Change::Cu(vec![CuChange::rewrite(rw).with_imports(imports)]),
    ));
}
fn overridden_method(method: BindingRef<'_>) -> Option<BindingRef<'_>> {
    if method.is_constructor() || method.modifiers() & (modifier::STATIC | modifier::PRIVATE) != 0 {
        return None;
    }
    fn find<'a>(
        owner: BindingRef<'a>,
        method: BindingRef<'a>,
        seen: &mut HashSet<String>,
    ) -> Option<BindingRef<'a>> {
        if !seen.insert(owner.key().to_owned()) {
            return None;
        }
        owner
            .declared_methods()
            .unwrap_or_default()
            .into_iter()
            .find(|m| method.data().method_overrides.contains(&m.id))
            .or_else(|| owner.superclass().and_then(|t| find(t, method, seen)))
            .or_else(|| {
                owner
                    .interfaces()
                    .into_iter()
                    .find_map(|t| find(t, method, seen))
            })
    }
    let owner = method.declaring_class()?;
    owner
        .superclass()
        .and_then(|t| find(t, method, &mut HashSet::new()))
        .or_else(|| {
            owner
                .interfaces()
                .into_iter()
                .find_map(|t| find(t, method, &mut HashSet::new()))
        })
}
fn throws_proposal(
    ctx: &Context,
    method: Node<'_>,
    types: &[BindingRef<'_>],
    options: &BTreeMap<String, String>,
    proposals: &mut Vec<Proposal>,
) {
    let Some(binding) = method.binding() else {
        return;
    };
    let mut types = types.to_vec();
    if let Some(parent) = overridden_method(binding) {
        if parent.declaring_class().is_none_or(|c| !c.is_from_source()) {
            types.retain(|t| parent.exception_types().iter().any(|s| subtype(*t, *s)));
        }
    }
    types.retain(|t| !binding.exception_types().iter().any(|s| subtype(*t, *s)));
    if types.is_empty() {
        return;
    }
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), options);
    let context = import_context(ctx, method, options);
    let original = method.list("thrownExceptionTypes");
    for typ in &original {
        if typ
            .binding()
            .is_some_and(|t| types.iter().any(|s| subtype(t, *s)))
        {
            rw.remove(RNode::Orig(typ.id));
            if let Some(t) = typ
                .binding()
                .filter(|t| super::exceptions::type_references(*t) == 1)
            {
                imports.remove_import(t.qualified_name());
            }
            if let Some(tag) = method.child("javadoc").and_then(|doc| {
                doc.list("tags").into_iter().find(|tag| {
                    matches!(tag.simple("tagName"), Some("@throws" | "@exception"))
                        && super::javadoc::argument(*tag).as_deref()
                            == Some(super::exceptions::type_name(*typ, false).as_str())
                })
            }) {
                rw.remove(RNode::Orig(tag.id));
            }
        }
    }
    for typ in types {
        let name = imports.add_import_binding(typ, &context);
        let node = imports.add_import_type(typ, &mut rw, &context, TypeLocation::Exception);
        rw.list_insert_last(RNode::Orig(method.id), "thrownExceptionTypes", node);
        if let Some(doc) = method.child("javadoc") {
            if !doc.list("tags").iter().any(|tag| {
                matches!(tag.simple("tagName"), Some("@throws" | "@exception"))
                    && super::javadoc::argument(*tag).as_deref() == Some(name.as_str())
            }) {
                super::javadoc::insert_throws_tag(&mut rw, doc, &name, &original);
            }
        }
    }
    push(
        rw,
        imports,
        "LocalCorrectionsSubProcessor_addthrows_description",
        relevance::ADD_THROWS_DECLARATION,
        proposals,
    );
}

pub async fn proposals(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    let selected = if ctx.selection_length > 0 {
        ctx.covered_node()
    } else {
        problem.covering_node(ctx.ast())
    };
    let Some(selected) = selected.and_then(|n| {
        std::iter::once(n).chain(n.ancestors()).find(|n| {
            n.kind().is_statement()
                || n.is(NodeKind::VariableDeclarationExpression)
                || n.kind().is_method_reference()
                || lambda_body(*n)
        })
    }) else {
        return;
    };
    let Some(root) = enclosing(selected) else {
        return;
    };
    let Some(decl) = find_parent_body_declaration(selected) else {
        return;
    };
    let start = selected.start();
    let end = if ctx.selection_length > 0 {
        selected
            .end()
            .max(ctx.selection_offset + ctx.selection_length)
    } else {
        selected.end()
    };
    let options = env.options(&ctx.ast.uri).await;
    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ctx.ast.uri) else {
        return;
    };
    let profile = accessors::profile(env.dispatcher, &uri).await;
    let name = exception_name(decl, start, end, &profile.exception_variable, end);
    let context = import_context(ctx, selected, &options);
    let uncaught = exceptions(root, start, end, true);
    let statements = if selected.location_is("statements") {
        selected
            .parent()
            .map(|p| {
                p.list("statements")
                    .into_iter()
                    .filter(|n| n.start() >= start && n.end() <= end)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        vec![selected]
    };
    let valid = !statements.is_empty()
        && statements.iter().all(|n| {
            (n.kind().is_statement() || lambda_body(*n) || n.kind().is_method_reference())
                && !matches!(
                    n.kind(),
                    NodeKind::ConstructorInvocation | NodeKind::SuperConstructorInvocation
                )
        });
    if valid {
        let name = exception_name(decl, start, end, &profile.exception_variable, decl.end());
        let mut surround_types = exceptions(root, start, end, false);
        if surround_types.is_empty() {
            if selected.kind().is_method_reference() {
                return;
            }
            if let Some(exception) = ctx.ast.binding_by_key("Ljava/lang/Exception;") {
                surround_types.push(exception);
            }
        }
        for multi in [false, true] {
            if surround_types.is_empty()
                || multi && surround_types.len() <= 1
                || !multi && selected.is(NodeKind::ThrowStatement)
            {
                continue;
            }
            let types = if multi {
                filter_subtypes(&surround_types)
            } else {
                surround_types.clone()
            };
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            selection_ranges(&mut rw, &statements, start, end);
            let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
            let catch = if multi {
                vec![catch_clause(
                    &mut rw,
                    &mut imports,
                    &context,
                    &types,
                    &name,
                    selected,
                    &options,
                    &profile,
                    CatchTemplate::General,
                )]
            } else {
                types
                    .iter()
                    .map(|t| {
                        catch_clause(
                            &mut rw,
                            &mut imports,
                            &context,
                            &[*t],
                            &name,
                            selected,
                            &options,
                            &profile,
                            CatchTemplate::Imported,
                        )
                    })
                    .collect()
            };
            let mut converted_lambda = None;
            let mut declarations = Vec::new();
            let contents = if selected.kind().is_method_reference() {
                let Some((lambda, expression, method)) = reference_lambda(&mut rw, selected) else {
                    continue;
                };
                converted_lambda = Some(lambda);
                vec![lambda_statement(&mut rw, expression, method)]
            } else if lambda_body(selected) && !selected.is(NodeKind::Block) {
                let Some(method) = selected.parent().and_then(|n| n.method_binding()) else {
                    continue;
                };
                let expression = rw.create_copy_target(selected.id);
                vec![lambda_statement(&mut rw, expression, method)]
            } else {
                let mut contents = Vec::new();
                for s in &statements {
                    if escapes(*s, root, end) {
                        if let Some((declaration, assignments)) =
                            split_local(&mut rw, &mut imports, &context, *s)
                        {
                            declarations.push(declaration);
                            contents.extend(assignments);
                            continue;
                        }
                    }
                    contents.push(rw.create_copy_target(s.id));
                }
                contents
            };
            let body = rw.new_block(contents);
            let statement = rw.new_node(NodeKind::TryStatement);
            rw.put_child(statement, "body", body);
            rw.put_list(statement, "catchClauses", catch);
            let replacement = if let Some(lambda) = converted_lambda {
                let block = rw.new_block(vec![statement]);
                rw.put_child(lambda, "body", block)
            } else if lambda_body(selected) && !selected.is(NodeKind::Block) {
                rw.new_block(vec![statement])
            } else {
                if declarations.is_empty() {
                    statement
                } else {
                    declarations.push(statement);
                    rw.create_group_node(declarations)
                }
            };
            rw.replace(RNode::Orig(statements[0].id), Some(replacement));
            for other in statements.iter().skip(1) {
                rw.remove(RNode::Orig(other.id));
            }
            push(
                rw,
                imports,
                if multi {
                    "LocalCorrectionsSubProcessor_surroundwith_trymulticatch_description"
                } else {
                    "LocalCorrectionsSubProcessor_surroundwith_trycatch_description"
                },
                if multi {
                    relevance::SURROUND_WITH_TRY_MULTICATCH
                } else {
                    relevance::SURROUND_WITH_TRY_CATCH
                },
                proposals,
            );
            if selected
                .child("type")
                .is_some_and(|t| t.source_text() == "var")
                && escapes(selected, root, end)
            {
                if let (Some(name), Some(typ)) = (
                    selected
                        .list("fragments")
                        .last()
                        .and_then(|f| f.child("name")),
                    selected.child("type").and_then(|t| t.binding()),
                ) {
                    proposals.last_mut().unwrap().name = messages::format(
                        messages::correction(if multi {
                            "LocalCorrectionsSubProcessor_surroundwith_trymulticatch_var_description"
                        } else {
                            "LocalCorrectionsSubProcessor_surroundwith_trycatch_var_description"
                        }),
                        &[&name.identifier(), typ.name()],
                    );
                }
            }
        }
    }
    if uncaught.is_empty() {
        return;
    }
    if let Some(surrounding) = selected
        .ancestors()
        .find(|n| n.is(NodeKind::TryStatement))
        .filter(|n| {
            n.child("body")
                .is_some_and(|b| b.is_ancestor_or_self_of(selected))
                || selected.location_is("resources")
        })
    {
        let mut rw = ASTRewrite::new(ctx.ast.clone());
        let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
        for typ in &uncaught {
            let clause = catch_clause(
                &mut rw,
                &mut imports,
                &context,
                &[*typ],
                &name,
                selected,
                &options,
                &profile,
                CatchTemplate::Binding,
            );
            rw.list_insert_last(RNode::Orig(surrounding.id), "catchClauses", clause);
        }
        push(
            rw,
            imports,
            "LocalCorrectionsSubProcessor_addadditionalcatch_description",
            relevance::ADD_ADDITIONAL_CATCH,
            proposals,
        );
        let clauses = surrounding.list("catchClauses");
        let filtered = filter_subtypes(&uncaught);
        if clauses.len() == 1 {
            if let Some(typ) = clauses[0].child("exception").and_then(|e| e.child("type")) {
                let mut rw = ASTRewrite::new(ctx.ast.clone());
                let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
                let existing = if typ.is(NodeKind::UnionType) {
                    typ.list("types")
                } else {
                    vec![typ]
                };
                let mut result = existing
                    .iter()
                    .map(|t| rw.create_copy_target(t.id))
                    .collect::<Vec<_>>();
                for added in &filtered {
                    let added_node =
                        imports.add_import_type(*added, &mut rw, &context, TypeLocation::Exception);
                    if let Some(index) = existing
                        .iter()
                        .position(|t| t.binding().is_some_and(|t| subtype(t, *added)))
                    {
                        result[index] = added_node;
                    } else {
                        result.push(added_node);
                    }
                }
                let replacement = if result.len() == 1 {
                    result[0]
                } else {
                    let union = rw.new_node(NodeKind::UnionType);
                    rw.put_list(union, "types", result)
                };
                rw.replace(RNode::Orig(typ.id), Some(replacement));
                push(
                    rw,
                    imports,
                    if filtered.len() > 1 {
                        "LocalCorrectionsSubProcessor_addexceptionstoexistingcatch_description"
                    } else {
                        "LocalCorrectionsSubProcessor_addexceptiontoexistingcatch_description"
                    },
                    relevance::ADD_EXCEPTIONS_TO_EXISTING_CATCH,
                    proposals,
                );
            }
        } else if clauses.is_empty() && filtered.len() > 1 {
            let mut rw = ASTRewrite::new(ctx.ast.clone());
            let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
            let clause = catch_clause(
                &mut rw,
                &mut imports,
                &context,
                &filtered,
                &name,
                selected,
                &options,
                &profile,
                CatchTemplate::General,
            );
            rw.list_insert_first(RNode::Orig(surrounding.id), "catchClauses", clause);
            push(
                rw,
                imports,
                "LocalCorrectionsSubProcessor_addadditionalmulticatch_description",
                relevance::ADD_EXCEPTIONS_TO_EXISTING_CATCH,
                proposals,
            );
        }
    }
    if root.is(NodeKind::MethodDeclaration) {
        throws_proposal(ctx, root, &uncaught, &options, proposals);
    }
}

fn is_autocloseable(typ: BindingRef<'_>, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(typ.key().into()) {
        return false;
    }
    typ.qualified_name() == "java.lang.AutoCloseable"
        || typ.superclass().is_some_and(|t| is_autocloseable(t, seen))
        || typ.interfaces().iter().any(|t| is_autocloseable(*t, seen))
        || typ.type_bounds().iter().any(|t| is_autocloseable(*t, seen))
}
fn resource_type(node: Node<'_>) -> Option<BindingRef<'_>> {
    if !node.is(NodeKind::VariableDeclarationStatement) {
        return None;
    }
    let typ = node.child("type")?.binding()?;
    is_autocloseable(typ, &mut HashSet::new()).then_some(typ)
}
/// QuickAssistProcessor.findEndPosition repeatedly extends through uses of
/// local declarations so moving a resource also moves code that depends on it.
fn lifetime_end(node: Node<'_>, body: Node<'_>) -> usize {
    let mut end = node.end();
    let first = node
        .descendants()
        .find(|n| n.is(NodeKind::VariableDeclarationFragment));
    if let Some(binding) = first.and_then(|n| n.binding()) {
        for reference in body
            .descendants()
            .filter(|n| n.is(NodeKind::SimpleName) && n.binding() == Some(binding))
        {
            if let Some(statement) = reference.ancestors().find(|n| {
                n.kind().is_statement()
                    && n.location_is("statements")
                    && n.parent().is_some_and(|p| p.id == body.id)
            }) {
                end = end.max(statement.end());
            }
        }
    }
    end
}
async fn resources(env: &Env<'_>, ctx: &Context, node: Node<'_>, proposals: &mut Vec<Proposal>) {
    let Some(selected) = node.ancestor_or_self(|k| k == NodeKind::VariableDeclarationStatement)
    else {
        return;
    };
    let Some(body) = selected.parent().filter(|n| n.is(NodeKind::Block)) else {
        return;
    };
    let Some(root) = enclosing(selected) else {
        return;
    };
    let Some(decl) = find_parent_body_declaration(selected) else {
        return;
    };
    let options = env.options(&ctx.ast.uri).await;
    if options
        .get("org.eclipse.jdt.core.compiler.compliance")
        .is_some_and(|s| {
            s.trim_start_matches("1.")
                .parse::<u32>()
                .is_ok_and(|v| v < 7)
        })
    {
        return;
    }
    let start = selected.start();
    let selection_end = if ctx.selection_length > 0 {
        selected
            .end()
            .max(ctx.selection_offset + ctx.selection_length)
    } else {
        selected.end()
    };
    let statements = body.list("statements");
    let resource_statements = statements
        .iter()
        .copied()
        .filter(|n| n.start() >= start && n.end() <= selection_end)
        .take_while(|n| resource_type(*n).is_some())
        .collect::<Vec<_>>();
    if resource_statements.is_empty() {
        return;
    }
    let mut end = resource_statements.last().unwrap().end();
    loop {
        let extended = statements
            .iter()
            .filter(|n| n.start() >= start && n.end() <= end)
            .map(|n| lifetime_end(*n, body))
            .max()
            .unwrap_or(end);
        if extended <= end {
            break;
        }
        end = extended;
    }
    let moved = statements
        .iter()
        .copied()
        .filter(|n| {
            n.start() >= start
                && n.end() <= end
                && !resource_statements.iter().any(|r| r.id == n.id)
        })
        .collect::<Vec<_>>();
    let mut thrown = exceptions(root, start, end, false);
    for resource in &resource_statements {
        let mut typ = resource_type(*resource);
        let mut seen = HashSet::new();
        while let Some(t) = typ.filter(|t| seen.insert(t.key().to_owned())) {
            if let Some(close) = t
                .declared_methods()
                .unwrap_or_default()
                .into_iter()
                .find(|m| m.name() == "close" && m.parameter_types().is_empty())
            {
                for exception in close.exception_types() {
                    add(&mut thrown, exception);
                }
                break;
            }
            typ = t.superclass();
        }
    }
    let surrounding = selected.ancestors().find(|n| n.is(NodeKind::TryStatement));
    let modify = surrounding.filter(|n| {
        n.child("body").is_some_and(|b| {
            b.list("statements")
                .first()
                .is_some_and(|s| s.id == selected.id)
        })
    });
    let caught = selected
        .ancestors()
        .filter(|n| n.is(NodeKind::TryStatement))
        .flat_map(|n| n.list("catchClauses"))
        .flat_map(caught_types)
        .collect::<Vec<_>>();
    let declared = root
        .binding()
        .filter(|_| root.is(NodeKind::MethodDeclaration))
        .map(|m| m.exception_types())
        .unwrap_or_default();
    let mut catch_types = filter_subtypes(&thrown);
    catch_types.retain(|t| !caught.iter().chain(&declared).any(|s| subtype(*t, *s)));
    let mut rethrow = Vec::new();
    for typ in &catch_types {
        for covered in caught.iter().chain(&declared) {
            if subtype(*covered, *typ) {
                add(&mut rethrow, *covered);
            }
        }
    }
    let Ok(uri) = tower_lsp::lsp_types::Url::parse(&ctx.ast.uri) else {
        return;
    };
    let profile = accessors::profile(env.dispatcher, &uri).await;
    let name = exception_name(decl, start, end, &profile.exception_variable, end);
    let context = import_context(ctx, selected, &options);
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let mut imports = ImportRewrite::create_for_corrections(ctx.ast.clone(), &options);
    let mut resources = Vec::new();
    for statement in &resource_statements {
        let Some(typ) = statement.child("type") else {
            continue;
        };
        for fragment in statement.list("fragments") {
            let Some(initializer) = fragment.child("initializer") else {
                continue;
            };
            let Some(variable) = fragment.child("name") else {
                continue;
            };
            let copied = rw.create_copy_target(initializer.id);
            let fragment =
                rw.new_variable_declaration_fragment(&variable.identifier(), Some(copied));
            let declaration = rw.new_node(NodeKind::VariableDeclarationExpression);
            let typ = rw.create_copy_target(typ.id);
            rw.put_child(declaration, "type", typ);
            rw.put_list(declaration, "fragments", vec![fragment]);
            resources.push(declaration);
        }
    }
    if resources.is_empty() {
        return;
    }
    let mut catch = Vec::new();
    if modify.is_none() {
        for typ in rethrow {
            let clause = catch_clause(
                &mut rw,
                &mut imports,
                &context,
                &[typ],
                &name,
                selected,
                &options,
                &profile,
                CatchTemplate::General,
            );
            let throw = rw.new_node(NodeKind::ThrowStatement);
            let nm = rw.new_simple_name(&name);
            rw.put_child(throw, "expression", nm);
            let block = rw.new_block(vec![throw]);
            rw.put_child(clause, "body", block);
            catch.push(clause);
        }
    }
    if !catch_types.is_empty() {
        catch.push(catch_clause(
            &mut rw,
            &mut imports,
            &context,
            &catch_types,
            &name,
            selected,
            &options,
            &profile,
            CatchTemplate::General,
        ));
    }
    if let Some(existing) = modify {
        for resource in resources {
            rw.list_insert_last(RNode::Orig(existing.id), "resources", resource);
        }
        for clause in catch {
            rw.list_insert_last(RNode::Orig(existing.id), "catchClauses", clause);
        }
        for original in resource_statements {
            rw.remove(RNode::Orig(original.id));
        }
    } else {
        let contents = moved.iter().map(|n| rw.create_copy_target(n.id)).collect();
        let block = rw.new_block(contents);
        let statement = rw.new_node(NodeKind::TryStatement);
        rw.put_child(statement, "body", block);
        rw.put_list(statement, "resources", resources);
        rw.put_list(statement, "catchClauses", catch);
        rw.replace(RNode::Orig(resource_statements[0].id), Some(statement));
        for original in resource_statements.iter().skip(1).chain(&moved) {
            rw.remove(RNode::Orig(original.id));
        }
    }
    push(
        rw,
        imports,
        "QuickAssistProcessor_convert_to_try_with_resource",
        relevance::SURROUND_WITH_TRY_CATCH,
        proposals,
    );
}
pub async fn resource_proposals(
    env: &Env<'_>,
    ctx: &Context,
    problem: &ProblemLocation,
    proposals: &mut Vec<Proposal>,
) {
    if let Some(node) = problem.covering_node(ctx.ast()) {
        resources(env, ctx, node, proposals).await;
    }
}
pub async fn resource_assist(env: &Env<'_>, ctx: &Context, proposals: &mut Vec<Proposal>) {
    if let Some(node) = ctx.covering_node() {
        resources(env, ctx, node, proposals).await;
    }
}
