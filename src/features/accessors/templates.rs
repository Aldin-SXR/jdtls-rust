//! ProjectTemplateStore and the accessor CodeGeneration templates. The project
//! store shares JDT LS's JavaManipulation preference node and XML persistence.
use super::{flags, naming, FieldDecl, TypeDecl};
use crate::semantic_ast::Ast;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) struct Profile {
    pub use_is: bool,
    pub(crate) use_this: bool,
    pub(crate) use_markdown: bool,
    templates: BTreeMap<String, String>,
    pub(crate) project_name: String,
    pub(crate) override_annotation: bool,
    pub(crate) create_comments: bool,
    pub(crate) exception_variable: String,
}
impl Profile {
    pub(super) fn load(root: Option<&Path>) -> Self {
        let prefs = root
            .and_then(|r| {
                crate::project::prefs::read_properties(
                    &r.join(".settings/org.eclipse.jdt.ls.core.prefs"),
                )
            })
            .unwrap_or_default();
        let mut templates = BTreeMap::new();
        if let Some(xml) = prefs.get("org.eclipse.jdt.ui.text.custom_code_templates") {
            if let Ok(doc) = roxmltree::Document::parse(xml) {
                for node in doc.descendants().filter(|n| n.has_tag_name("template")) {
                    if node.attribute("deleted") == Some("true") {
                        continue;
                    }
                    if let Some(id) = node.attribute("id") {
                        let pattern: String = node.children().filter_map(|n| n.text()).collect();
                        templates.insert(id.to_owned(), pattern);
                    }
                }
            }
        }
        Self {
            exception_variable: prefs
                .get("org.eclipse.jdt.ui.exception.name")
                .cloned()
                .unwrap_or_else(|| "e".into()),
            override_annotation: prefs
                .get("org.eclipse.jdt.ui.overrideannotation")
                .is_none_or(|s| s == "true"),
            create_comments: prefs
                .get("org.eclipse.jdt.ui.javadoc")
                .is_some_and(|s| s == "true"),
            use_is: prefs
                .get("org.eclipse.jdt.ui.gettersetter.use.is")
                .is_none_or(|s| s == "true"),
            use_this: prefs
                .get("org.eclipse.jdt.ui.keywordthis")
                .is_some_and(|s| s == "true"),
            use_markdown: prefs
                .get("org.eclipse.jdt.ui.usemarkdown")
                .is_some_and(|s| s == "true"),
            templates,
            project_name: root
                .and_then(|r| r.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .into(),
        }
    }
    pub(crate) fn template<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.templates
            .get(&format!("org.eclipse.jdt.ui.text.codetemplates.{key}"))
            .map(String::as_str)
            .unwrap_or(default)
    }
}

pub(super) fn stub(
    t: &TypeDecl,
    f: &FieldDecl,
    getter: bool,
    comments: bool,
    options: &BTreeMap<String, String>,
    profile: &Profile,
    ast: &Ast,
    enclosing_type: &str,
) -> anyhow::Result<String> {
    let is_static = f.flags & flags::STATIC != 0;
    let type_name = f.type_signature.as_deref().unwrap_or("");
    let method = if getter {
        naming::getter(t, f, options, profile.use_is)
    } else {
        naming::setter(f, options, profile.use_is)
    };
    let param = naming::argument(f, options);
    let bare = naming::base(f, options, true);
    let field = if !is_static && profile.use_this || !getter && f.name == param {
        if is_static {
            format!("{}.{}", t.name, f.name)
        } else {
            format!("this.{}", f.name)
        }
    } else {
        f.name.clone()
    };
    let vars = AccessorVars {
        field: &f.name,
        field_access: &field,
        bare_field_name: &bare,
        field_type: type_name,
        param: &param,
        method: &method,
        enclosing_type,
    };
    let mut stub = String::new();
    if comments {
        if let Some(comment) = accessor_comment(getter, &vars, options, profile, ast)? {
            stub.push_str(&comment);
            stub.push('\n');
        }
    }
    stub.push_str("public ");
    if is_static {
        stub.push_str("static ");
    }
    if getter {
        stub.push_str(&format!("{type_name} {method}() {{\n"));
        stub.push_str(&accessor_body(true, &vars, options, profile, ast)?);
    } else {
        stub.push_str(&format!("void {method}({type_name} {param}) {{\n"));
        stub.push_str(&accessor_body(false, &vars, options, profile, ast)?);
    }
    stub.push('}');
    Ok(stub)
}

/// The variables of the getter / setter code templates.
pub(crate) struct AccessorVars<'a> {
    /// The field name (`${field}` in comments).
    pub field: &'a str,
    /// The field access used in bodies (`${field}` in bodies).
    pub field_access: &'a str,
    pub bare_field_name: &'a str,
    pub field_type: &'a str,
    pub param: &'a str,
    pub method: &'a str,
    pub enclosing_type: &'a str,
}

fn expand_accessor(
    template: &str,
    body: bool,
    vars: &AccessorVars<'_>,
    options: &BTreeMap<String, String>,
    profile: &Profile,
    ast: &Ast,
) -> anyhow::Result<String> {
    let file_name = tower_lsp::lsp_types::Url::parse(&ast.uri)
        .ok()
        .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
        .unwrap_or_default();
    let package = ast
        .root()
        .child("package")
        .and_then(|p| p.child("name"))
        .map(|n| n.identifier())
        .unwrap_or_default();
    let replacements = [
        ("field", vars.field),
        ("bare_field_name", vars.bare_field_name),
        ("field_type", vars.field_type),
        ("param", vars.param),
        ("enclosing_type", vars.enclosing_type),
        ("enclosing_method", vars.method),
        ("file_name", file_name.as_str()),
        ("package_name", package.as_str()),
        ("project_name", profile.project_name.as_str()),
    ];
    let todo = options
        .get("org.eclipse.jdt.core.compiler.taskTags")
        .and_then(|s| s.split(',').next())
        .unwrap_or("XXX");
    expand_template(template, |key| match key {
        "dollar" => Some("$"),
        "todo" => Some(todo),
        "field" if body => Some(vars.field_access),
        _ => replacements
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, value)| {
                // Bodies don't register compilation-unit or comment-only variables.
                (!body || matches!(key, "field" | "param" | "enclosing_type" | "enclosing_method"))
                    .then_some(*value)
            }),
    })
}

/// `CodeGeneration.getGetterComment` / `getSetterComment`: `None` when the
/// template expands to whitespace only.
pub(crate) fn accessor_comment(
    getter: bool,
    vars: &AccessorVars<'_>,
    options: &BTreeMap<String, String>,
    profile: &Profile,
    ast: &Ast,
) -> anyhow::Result<Option<String>> {
    let markdown = profile.use_markdown
        && options
            .get("org.eclipse.jdt.core.compiler.compliance")
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|n| n >= 23);
    let comment = if markdown {
        if getter {
            profile.template("markdowngettercomment", "")
        } else {
            profile.template("markdownsettercomment", "")
        }
    } else if getter {
        profile.template("gettercomment", "/**\n * @return the ${bare_field_name}\n */")
    } else {
        profile.template("settercomment", "/**\n * @param ${param} the ${bare_field_name} to set\n */")
    };
    let comment = expand_accessor(comment, false, vars, options, profile, ast)?;
    Ok((!comment.trim().is_empty()).then_some(comment))
}

/// `CodeGeneration.getGetterMethodBodyContent` / `getSetterMethodBodyContent`:
/// the expanded `getterbody` / `setterbody` template.
pub(crate) fn accessor_body(
    getter: bool,
    vars: &AccessorVars<'_>,
    options: &BTreeMap<String, String>,
    profile: &Profile,
    ast: &Ast,
) -> anyhow::Result<String> {
    let template = if getter {
        profile.template("getterbody", "return ${field};\n")
    } else {
        profile.template("setterbody", "${field} = ${param};\n")
    };
    expand_accessor(template, true, vars, options, profile, ast)
}

/// TemplateTranslator's dollar escapes, named variables and resolver aliases.
/// Expansion scans the original pattern once; variable values are literal text.
pub(crate) fn expand_template<'a>(
    template: &str,
    resolve: impl Fn(&str) -> Option<&'a str>,
) -> anyhow::Result<String> {
    expand_named_template(template, |_, resolver| resolve(resolver))
}
pub(crate) fn expand_named_template<'a>(
    template: &str,
    mut resolve: impl FnMut(&str, &str) -> Option<&'a str>,
) -> anyhow::Result<String> {
    let normalized = template.replace("\r\n", "\n").replace('\r', "\n");
    let mut rest = normalized.as_str();
    let mut out = String::new();
    while let Some(dollar) = rest.find('$') {
        out.push_str(&rest[..dollar]);
        rest = &rest[dollar + 1..];
        if let Some(tail) = rest.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('{') {
            let Some(end) = tail.find('}') else {
                anyhow::bail!("Unclosed template variable");
            };
            let expr = tail[..end].trim();
            let (name, resolver) = expr.split_once(':').unwrap_or((expr, expr));
            let name = name.trim();
            let resolver = resolver.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                anyhow::bail!("Invalid template variable");
            }
            out.push_str(resolve(name, resolver).unwrap_or(name));
            rest = &tail[end + 1..];
        } else {
            anyhow::bail!("Unescaped dollar in template");
        }
    }
    out.push_str(rest);
    Ok(out)
}
