//! AddDelegateMethodsOperation and StubUtility2Core.createDelegationStub.
use super::{erasure, Entry};
use crate::{
    correction::{edit::Env, CuChange},
    features::{
        accessors::{self, Selection},
        constructors::ConstructorImportContext,
        preferences,
    },
    rewrite::{import_rewrite::{ImportRewrite, TypeLocation}, ASTRewrite, RNode},
    semantic_ast::{modifier, Ast, BindingRef, NodeId, NodeKind},
};
use std::{collections::BTreeMap, sync::Arc};
fn replace(mut t: BindingRef<'_>) -> BindingRef<'_> {
    let mut seen = std::collections::HashSet::new();
    while (t.is_wildcard_type()
        || t.is_capture()
        || t.is_array() && t.element_type().is_some_and(|e| e.is_capture()))
        && seen.insert(t.key().to_owned())
    {
        t = t.bound().unwrap_or_else(|| erasure(t));
    }
    t
}
pub(crate) fn inherited_comment(
    ast: &Ast,
    owner: BindingRef<'_>,
    method: BindingRef<'_>,
    result: &str,
    names: &[String],
    throws: &[String],
    profile: &accessors::templates::Profile,
    template: &str,
    enclosing: &str,
    see_variable: &str,
) -> anyhow::Result<String> {
    let decl = method.method_declaration().unwrap_or(method);
    let declaring = decl
        .declaring_class()
        .map(|t| t.qualified_name())
        .unwrap_or("");
    let see = format!(
        "@see {declaring}#{}({})",
        decl.name(),
        decl.parameter_types()
            .iter()
            .map(|p| erasure(*p).qualified_name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut tags = method
        .type_parameters()
        .iter()
        .map(|t| format!("@param <{}>", t.name()))
        .chain(names.iter().map(|n| format!("@param {n}")))
        .collect::<Vec<_>>();
    if result != "void" {
        tags.push("@return".into());
    }
    tags.extend(
        throws
            .iter()
            .map(|t| format!("@throws {}", t.rsplit('.').next().unwrap_or(t))),
    );
    if decl.is_deprecated() {
        tags.push("@deprecated".into());
    }
    let file = tower_lsp::lsp_types::Url::parse(&ast.uri)
        .ok()
        .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
        .unwrap_or_default();
    let marker = "@@JDT_DELEGATE_TAGS@@";
    let mut text = accessors::templates::expand_template(template, |key| {
        if key == see_variable {
            return Some(see.as_str());
        }
        Some(match key {
            "tags" => marker,
            "enclosing_type" => enclosing,
            "enclosing_method" => method.name(),
            "return_type" => result,
            "file_name" => &file,
            "package_name" => owner.package_name().unwrap_or(""),
            "project_name" => &profile.project_name,
            _ => return None,
        })
    })?;
    for i in text
        .match_indices(marker)
        .map(|(i, _)| i)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let start = text[..i].rfind('\n').map_or(0, |p| p + 1);
        let prefix = text[start..i].replace(marker, "@");
        // StubUtility.insertTag clears an empty tag line only when its
        // preceding line is beyond line zero. Other blank comment lines stay.
        if tags.is_empty()
            && text[..start].bytes().filter(|b| *b == b'\n').count() > 1
            && prefix.chars().all(|c| c.is_whitespace() || c == '*')
        {
            text.replace_range(start - 1..i + marker.len(), "");
        } else {
            text.replace_range(i..i + marker.len(), &tags.join(&format!("\n{prefix}")));
        }
    }
    Ok(text)
}
fn stub(
    ast: &Ast,
    e: Entry<'_>,
    imports: &mut ImportRewrite,
    context: &ConstructorImportContext,
    options: &BTreeMap<String, String>,
    profile: &accessors::templates::Profile,
) -> anyhow::Result<String> {
    let m = e.method;
    let mut source = String::new();
    let result = imports.add_import_type_string(
        m.return_type()
            .ok_or_else(|| anyhow::anyhow!("Missing delegate return type"))?,
        context,
        TypeLocation::ReturnType,
    );
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
        let t = replace(*p);
        let varargs = m.is_varargs() && i == params.len() - 1 && t.is_array();
        parameters.push(format!(
            "{} {name}",
            imports.add_import_parameter_type_string(t, context, varargs),
        ));
    }
    let throws: Vec<_> = m
        .exception_types()
        .iter()
        .map(|t| imports.add_import_type_string(*t, context, TypeLocation::Exception))
        .collect();
    if preferences::generate_comments() {
        let owner = e
            .field
            .declaring_class()
            .ok_or_else(|| anyhow::anyhow!("Missing delegate declaring class"))?;
        let text = inherited_comment(
            ast,
            owner,
            m,
            &result,
            &names,
            &throws,
            profile,
            profile.template(
                "delegatecomment",
                "/**\n * ${tags}\n * ${see_to_target}\n */",
            ),
            owner
                .qualified_name()
                .trim_start_matches(owner.package_name().unwrap_or("")),
            "see_to_target",
        )?;
        if !text.trim().is_empty() {
            source.push_str(&text);
            source.push('\n');
        }
    }
    let mods = m.modifiers()
        & !(modifier::DEFAULT | modifier::SYNCHRONIZED | modifier::ABSTRACT | modifier::NATIVE);
    for (flag, name) in [
        (modifier::PUBLIC, "public"),
        (modifier::PROTECTED, "protected"),
        (modifier::PRIVATE, "private"),
        (modifier::STATIC, "static"),
        (modifier::FINAL, "final"),
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
            let bounds = t.type_bounds();
            if bounds.len() == 1 && bounds[0].qualified_name() == "java.lang.Object" {
                t.name().into()
            } else if bounds.is_empty() {
                t.name().into()
            } else {
                format!(
                    "{} extends {}",
                    t.name(),
                    bounds
                        .iter()
                        .map(|b| imports.add_import_type_string(*b, context, TypeLocation::TypeBound))
                        .collect::<Vec<_>>()
                        .join(" & ")
                )
            }
        })
        .collect();
    if !generic.is_empty() {
        source.push_str(&format!("<{}> ", generic.join(", ")));
    }
    source.push_str(&format!("{result} {}({})", m.name(), parameters.join(", ")));
    if !throws.is_empty() {
        source.push_str(&format!(" throws {}", throws.join(", ")));
    }
    source.push_str(&format!(
        " {{\n{}{}.{}({});\n}}",
        if result == "void" { "" } else { "return " },
        e.field.name(),
        m.name(),
        names.join(", ")
    ));
    Ok(source)
}
pub(super) async fn create(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    entries: &[Entry<'_>],
    before: Option<NodeId>,
) -> anyhow::Result<CuChange> {
    let options = env.options(&ast.uri).await;
    let profile =
        accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&ast.uri)?).await;
    let mut imports = ImportRewrite::create_for_corrections(ast.clone(), &options);
    let context = ConstructorImportContext {
        ast: ast.clone(),
        declaration: Some(selected.declaration),
    };
    let mut rewrite = ASTRewrite::new(ast.clone());
    let eol = if ast.source.windows(2).any(|w| w == [13, 10]) {
        "\r\n"
    } else {
        "\n"
    };
    for e in entries {
        let method = stub(&ast, *e, &mut imports, &context, &options, &profile)?;
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
        let node = rewrite.create_string_placeholder(&formatted, NodeKind::MethodDeclaration);
        let parent = RNode::Orig(selected.declaration);
        if let Some(b) = before {
            rewrite.list_insert_before(parent, "bodyDeclarations", node, RNode::Orig(b));
        } else {
            rewrite.list_insert_last(parent, "bodyDeclarations", node);
        }
    }
    Ok(CuChange::rewrite(rewrite).with_imports(imports))
}
