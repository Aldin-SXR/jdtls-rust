//! StubUtility2Core.createImplementationStubCore for handler-generated methods.
use crate::{
    correction::{edit::Env, CuChange},
    features::{
        accessors::{self, Selection},
        constructors::ConstructorImportContext,
        delegates::erasure,
    },
    rewrite::{
        import_rewrite::{DefaultContext, ImportRewrite},
        ASTRewrite, RNode,
    },
    semantic_ast::{modifier, Ast, BindingRef, NodeId, NodeKind},
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
fn replace(mut t: BindingRef<'_>) -> BindingRef<'_> {
    let mut seen = HashSet::new();
    while (t.is_wildcard_type()
        || t.is_capture()
        || t.is_array() && t.element_type().is_some_and(|e| e.is_capture()))
        && seen.insert(t.key().to_owned())
    {
        t = t.bound().unwrap_or_else(|| erasure(t));
    }
    t
}
fn immediate<'a>(
    t: BindingRef<'a>,
    qualified: &str,
    seen: &mut HashSet<String>,
) -> Option<BindingRef<'a>> {
    if !seen.insert(t.key().into()) {
        return None;
    }
    let parents: Vec<_> = t.superclass().into_iter().chain(t.interfaces()).collect();
    parents
        .iter()
        .copied()
        .find(|p| erasure(*p).qualified_name() == qualified)
        .or_else(|| {
            parents
                .into_iter()
                .find(|p| immediate(*p, qualified, seen).is_some())
        })
}
fn stub(
    m: BindingRef<'_>,
    owner: BindingRef<'_>,
    imports: &mut ImportRewrite,
    context: &ConstructorImportContext,
    options: &BTreeMap<String, String>,
    profile: &accessors::templates::Profile,
    quick_fix: bool,
) -> anyhow::Result<String> {
    let dc = m
        .declaring_class()
        .ok_or_else(|| anyhow::anyhow!("No overridden declaring type"))?;
    let in_interface = !quick_fix && owner.is_interface();
    let is_object = dc.qualified_name() == "java.lang.Object";
    let mut source = String::new();
    let skip = in_interface && is_object && m.modifiers() & modifier::PUBLIC == 0;
    let override_enabled=!dc.is_interface() || options.get("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation").is_none_or(|s|s!="disabled");
    if !skip && override_enabled && (!quick_fix || profile.override_annotation) {
        source.push_str(&format!(
            "@{}\n",
            imports.add_import("java.lang.Override", &DefaultContext)
        ));
    }
    let mut mods = m.modifiers();
    if in_interface {
        mods &= !(modifier::PROTECTED | modifier::PUBLIC);
        if mods & modifier::ABSTRACT != 0 {
            mods |= modifier::DEFAULT;
        }
    } else {
        mods &= !modifier::DEFAULT;
    }
    mods &= !(modifier::ABSTRACT | modifier::NATIVE | modifier::PRIVATE);
    for (flag, name) in [
        (modifier::PUBLIC, "public"),
        (modifier::PROTECTED, "protected"),
        (modifier::DEFAULT, "default"),
        (modifier::STATIC, "static"),
        (modifier::FINAL, "final"),
        (modifier::SYNCHRONIZED, "synchronized"),
        (modifier::STRICTFP, "strictfp"),
    ] {
        if mods & flag != 0 {
            source.push_str(name);
            source.push(' ');
        }
    }
    let generic: Vec<_> = m
        .type_parameters()
        .iter()
        .map(|t| {
            let bs = t.type_bounds();
            if bs.is_empty() || bs.len() == 1 && bs[0].qualified_name() == "java.lang.Object" {
                t.name().into()
            } else {
                format!(
                    "{} extends {}",
                    t.name(),
                    bs.iter()
                        .map(|b| imports.add_import_binding(*b, context))
                        .collect::<Vec<_>>()
                        .join(" & ")
                )
            }
        })
        .collect();
    if !generic.is_empty() {
        source.push_str(&format!("<{}> ", generic.join(", ")));
    }
    let ret = replace(
        m.return_type()
            .ok_or_else(|| anyhow::anyhow!("No overridden return type"))?,
    );
    let result = imports.add_import_binding(ret, context);
    let mut names = Vec::new();
    let params = m.parameter_types();
    let mut parameters = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let raw = m
            .data()
            .parameter_names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("arg{i}"));
        let name = accessors::naming::method_argument(&raw, options, &names);
        names.push(name.clone());
        let mut t = replace(*p);
        let varargs = m.is_varargs() && i == params.len() - 1 && t.is_array();
        if varargs {
            t = t.component_type().unwrap_or(t);
        }
        parameters.push(format!(
            "{}{} {name}",
            imports.add_import_binding(t, context),
            if varargs { "..." } else { "" }
        ));
    }
    source.push_str(&format!("{result} {}({})", m.name(), parameters.join(", ")));
    let throws: Vec<_> = m
        .exception_types()
        .iter()
        .map(|t| imports.add_import_binding(*t, context))
        .collect();
    if !throws.is_empty() {
        source.push_str(&format!(" throws {}", throws.join(", ")));
    }
    if in_interface && is_object {
        source.push(';');
        return Ok(source);
    }
    let statement = if m.modifiers() & modifier::ABSTRACT != 0 {
        if ret.is_primitive() {
            match ret.name() {
                "void" => String::new(),
                "boolean" => "return false;".into(),
                _ => "return 0;".into(),
            }
        } else if ret.is_parameterized_type()
            && erasure(ret).qualified_name() == "java.util.Optional"
        {
            "return Optional.empty();".into()
        } else {
            "return null;".into()
        }
    } else {
        let qualifier = if dc.is_interface() {
            let t =
                immediate(owner, erasure(dc).qualified_name(), &mut HashSet::new()).unwrap_or(dc);
            if t.is_interface() {
                format!(
                    "{}.",
                    imports.add_import_binding(t.type_declaration().unwrap_or(t), context)
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        format!(
            "{}{}super.{}({});",
            if result == "void" { "" } else { "return " },
            qualifier,
            m.name(),
            names.join(", ")
        )
    };
    let mut enclosing = Vec::new();
    let mut typ = Some(owner);
    while let Some(t) = typ {
        enclosing.push(if t.is_anonymous() {
            "$local$"
        } else {
            t.name()
        });
        typ = t.declaring_class();
    }
    enclosing.reverse();
    let type_name = enclosing.join(".");
    if quick_fix
        && profile.create_comments
        && context.declaration.is_some_and(|n| {
            !matches!(
                context.ast.node(n).kind(),
                NodeKind::AnonymousClassDeclaration | NodeKind::EnumConstantDeclaration
            )
        })
    {
        let comment = crate::features::delegates::operation::inherited_comment(
            &context.ast,
            owner,
            m,
            &result,
            &names,
            &throws,
            profile,
            profile.template("overridecomment", ""),
            &type_name,
            "see_to_overridden",
        )?;
        if !comment.trim().is_empty() {
            source = format!("{comment}\n{source}");
        }
    }
    let todo = options
        .get("org.eclipse.jdt.core.compiler.taskTags")
        .and_then(|s| s.split(',').next())
        .unwrap_or("TODO");
    let template = if in_interface || quick_fix {
        profile.template("methodbody","// ${todo} Auto-generated method stub\nthrow new UnsupportedOperationException(\"Unimplemented method '${enclosing_method}'\");")
    } else {
        profile.template(
            "methodbodyalternative",
            "// ${todo} Auto-generated method stub\n${body_statement}",
        )
    };
    let mut body = accessors::templates::expand_template(template, |key| match key {
        "todo" => Some(todo),
        "body_statement" => Some(statement.as_str()),
        "enclosing_type" => Some(type_name.as_str()),
        "dollar" => Some("$"),
        "enclosing_method" => Some(m.name()),
        _ => None,
    })?;
    if body.trim().is_empty() && !statement.trim().is_empty() {
        body = statement;
    }
    source.push_str(&format!(" {{\n{body}\n}}"));
    Ok(source)
}
// JDT inserts method-body template text as a statement placeholder. Its blank
// lines receive the body's base indentation, including an empty body_statement.
fn indent_body_blanks(source: &str) -> String {
    let Some(start) = source
        .find('{')
        .and_then(|i| source[i..].find('\n').map(|j| i + j + 1))
    else {
        return source.into();
    };
    let Some(end) = source.rfind('}') else {
        return source.into();
    };
    if start >= end {
        return source.into();
    }
    let body = &source[start..end];
    let Some(line) = body.lines().find(|l| !l.trim().is_empty()) else {
        return source.into();
    };
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut out = source[..start].to_owned();
    for line in body.split_inclusive('\n') {
        if line.ends_with('\n') && line.trim().is_empty() {
            out.push_str(&indent);
            out.push_str(if line.ends_with("\r\n") { "\r\n" } else { "\n" });
        } else {
            out.push_str(line);
        }
    }
    out.push_str(&source[end..]);
    out
}
pub(super) async fn create(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    methods: &[BindingRef<'_>],
    before: Option<NodeId>,
) -> anyhow::Result<CuChange> {
    create_impl(env, ast, selected.declaration, methods, before, false).await
}
pub(crate) async fn create_unimplemented(
    env: &Env<'_>,
    ast: Arc<Ast>,
    declaration: NodeId,
    methods: &[BindingRef<'_>],
) -> anyhow::Result<CuChange> {
    create_impl(env, ast, declaration, methods, None, true).await
}
async fn create_impl(
    env: &Env<'_>,
    ast: Arc<Ast>,
    declaration: NodeId,
    methods: &[BindingRef<'_>],
    before: Option<NodeId>,
    quick_fix: bool,
) -> anyhow::Result<CuChange> {
    let owner = ast
        .node(declaration)
        .binding()
        .and_then(|b| {
            if b.is_variable() {
                b.declaring_class()
            } else {
                Some(b)
            }
        })
        .ok_or_else(|| anyhow::anyhow!("No override target binding"))?;
    let options = env.options(&ast.uri).await;
    let profile =
        accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&ast.uri)?).await;
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = ConstructorImportContext {
        ast: ast.clone(),
        declaration: Some(declaration),
    };
    let mut rewrite = ASTRewrite::new(ast.clone());
    let parent = if ast.node(declaration).is(NodeKind::EnumConstantDeclaration) {
        let anonymous = rewrite.new_node(NodeKind::AnonymousClassDeclaration);
        rewrite.put_list(anonymous, "bodyDeclarations", Vec::new());
        rewrite.set(
            RNode::Orig(declaration),
            "anonymousClassDeclaration",
            Some(anonymous),
        );
        anonymous
    } else {
        RNode::Orig(declaration)
    };
    let eol = if ast.source.windows(2).any(|w| w == [13, 10]) {
        "\r\n"
    } else {
        "\n"
    };
    for m in methods {
        let target = if quick_fix {
            m.declaring_class().unwrap_or(owner)
        } else {
            owner
        };
        let method = stub(
            *m,
            target,
            &mut imports,
            &context,
            &options,
            &profile,
            quick_fix,
        )?;
        let formatted = match env
            .dispatcher
            .format_source(
                &method,
                crate::rewrite::formatter::K_CLASS_BODY_DECLARATIONS,
                0,
                method.encode_utf16().count(),
                eol,
                options.clone(),
            )
            .await?
        {
            Some(edits) => String::from_utf16_lossy(&crate::rewrite::text_edit::apply_flat(
                &method.encode_utf16().collect::<Vec<_>>(),
                &edits
                    .into_iter()
                    .map(|e| (e.offset, e.length, e.text))
                    .collect::<Vec<_>>(),
            )),
            None => method,
        };
        let formatted = indent_body_blanks(&formatted);
        let node = rewrite.create_string_placeholder(&formatted, NodeKind::MethodDeclaration);
        if let Some(b) = before {
            rewrite.list_insert_before(parent, "bodyDeclarations", node, RNode::Orig(b));
        } else {
            rewrite.list_insert_last(parent, "bodyDeclarations", node);
        }
    }
    Ok(CuChange::rewrite(rewrite).with_imports(imports))
}
