//! GenerateToStringOperation and the four JDT LS output strategies.
use super::{
    parents,
    template::{Template, DEFAULT},
};
use crate::{
    correction::{edit::Env, CuChange},
    features::{accessors, preferences},
    rewrite::{
        import_rewrite::{DefaultContext, ImportRewrite},
        ASTRewrite, RNode,
    },
    semantic_ast::{modifier, Ast, BindingRef, NodeId, NodeKind},
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, PartialEq)]
enum Style {
    Concat,
    Builder,
    Chain,
    Format,
}
struct Settings {
    style: Style,
    skip: bool,
    arrays: bool,
    limit: i64,
    blocks: bool,
    comments: bool,
    template: String,
}
impl Settings {
    fn load() -> Self {
        let style =
            match preferences::get_string("java.codeGeneration.toString.codeStyle").as_deref() {
                Some("STRING_BUILDER") => Style::Builder,
                Some("STRING_BUILDER_CHAINED") => Style::Chain,
                Some("STRING_FORMAT") => Style::Format,
                _ => Style::Concat,
            };
        let template = preferences::get_string("java.codeGeneration.toString.template")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT.into());
        Self {
            style,
            skip: preferences::get_bool("java.codeGeneration.toString.skipNullValues")
                .unwrap_or(false),
            arrays: preferences::get_bool("java.codeGeneration.toString.listArrayContents")
                .unwrap_or(true),
            limit: preferences::get("java.codeGeneration.toString.limitElements")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .max(0),
            blocks: preferences::get_bool("java.codeGeneration.useBlocks").unwrap_or(false),
            comments: preferences::generate_comments(),
            template,
        }
    }
}
#[derive(Clone)]
enum Element {
    Text(String),
    Expr(String),
}
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn collapse(elements: &[Element]) -> Vec<Element> {
    let mut out = Vec::new();
    let mut text = String::new();
    for e in elements {
        match e {
            Element::Text(s) => text.push_str(s),
            Element::Expr(s) => {
                if !text.is_empty() {
                    out.push(Element::Text(std::mem::take(&mut text)))
                }
                out.push(Element::Expr(s.clone()));
            }
        }
    }
    if !text.is_empty() {
        out.push(Element::Text(text))
    }
    out
}
fn sum(elements: &[Element]) -> String {
    collapse(elements)
        .into_iter()
        .map(|e| match e {
            Element::Text(s) => quote(&s),
            Element::Expr(s) => s,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}
struct Generator<'a> {
    settings: Settings,
    template: Template,
    imports: ImportRewrite,
    binding: BindingRef<'a>,
    options: BTreeMap<String, String>,
    excluded: Vec<String>,
    max_len: String,
    builder: String,
    need_max: bool,
    helper: bool,
}
impl<'a> Generator<'a> {
    fn new(
        settings: Settings,
        imports: ImportRewrite,
        binding: BindingRef<'a>,
        members: &[BindingRef<'a>],
        options: BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        let template = Template::parse(&settings.template)?;
        let mut excluded = Vec::new();
        for t in std::iter::once(binding).chain(parents(binding)) {
            for f in t
                .declared_fields()
                .unwrap_or_default()
                .into_iter()
                .chain(t.declared_types().unwrap_or_default())
            {
                if t == binding || f.modifiers() & modifier::PRIVATE == 0 {
                    excluded.push(f.name().into())
                }
            }
        }
        let helper = settings.limit > 0
            && members.iter().any(|m| {
                let t = member_type(*m);
                (implements(t, "java.util.Collection") || implements(t, "java.util.Map"))
                    && !implements(t, "java.util.List")
            });
        let mut g = Self {
            settings,
            template,
            imports,
            binding,
            options,
            excluded,
            max_len: String::new(),
            builder: String::new(),
            need_max: false,
            helper,
        };
        g.max_len = g.name("maxLen", false);
        g.builder = g.name("builder", false);
        Ok(g)
    }
    fn name(&self, base: &str, argument: bool) -> String {
        let mut options = self.options.clone();
        if !argument {
            for key in ["Prefixes", "Suffixes"] {
                if let Some(value) = self
                    .options
                    .get(&format!("org.eclipse.jdt.core.codeComplete.local{key}"))
                {
                    options.insert(
                        format!("org.eclipse.jdt.core.codeComplete.argument{key}"),
                        value.clone(),
                    );
                } else {
                    options.remove(&format!("org.eclipse.jdt.core.codeComplete.argument{key}"));
                }
            }
        }
        accessors::naming::argument_excluding(base, &options, &self.excluded)
    }
    fn import(&mut self, name: &str) -> String {
        self.imports.add_import(name, &DefaultContext)
    }
    fn raw(&self, m: BindingRef<'_>) -> String {
        if m.return_type().is_some() {
            format!(
                "{}{}()",
                if m.name() == "toString" { "super." } else { "" },
                m.name()
            )
        } else {
            m.name().into()
        }
    }
    fn access(&mut self, m: BindingRef<'_>, ignore_nulls: bool) -> String {
        let raw = self.raw(m);
        let t = member_type(m);
        let collection = implements(t, "java.util.Collection");
        let list = implements(t, "java.util.List");
        let map = implements(t, "java.util.Map");
        if self.settings.limit > 0 && (collection || map || t.is_array() && self.settings.arrays) {
            self.need_max = true;
            let max = self.max_len.clone();
            let math = self.import("java.lang.Math");
            let access = if list && !self.helper {
                format!("{raw}.subList(0, {math}.min({raw}.size(), {max}))")
            } else if collection || map {
                format!(
                    "toString({raw}{}, {max})",
                    if map { ".entrySet()" } else { "" }
                )
            } else {
                let arrays = self.import("java.util.Arrays");
                if t.component_type().is_some_and(|t| t.is_primitive()) {
                    format!("{arrays}.toString({arrays}.copyOf({raw}, {math}.min({raw}.length, {max})))")
                } else {
                    format!("{arrays}.asList({raw}).subList(0, {math}.min({raw}.length, {max}))")
                }
            };
            if ignore_nulls {
                access
            } else {
                format!("({raw} != null ? {access} : null)")
            }
        } else if t.is_array() && self.settings.arrays {
            let arrays = self.import("java.util.Arrays");
            format!("{arrays}.toString({raw})")
        } else {
            raw
        }
    }
    fn process(&mut self, token: &str, member: Option<BindingRef<'_>>) -> Element {
        match token {
            "${object.className}" => Element::Text(self.binding.name().into()),
            "${object.getClassName}" => Element::Expr("getClass().getName()".into()),
            "${object.superToString}" => Element::Expr("super.toString()".into()),
            "${object.hashCode}" => Element::Expr("hashCode()".into()),
            "${object.identityHashCode}" => {
                let system = self.import("java.lang.System");
                Element::Expr(format!("{system}.identityHashCode(this)"))
            }
            "${member.name}" | "${member.name()}" => {
                let m = member.unwrap();
                Element::Text(format!(
                    "{}{}",
                    m.name(),
                    if token == "${member.name()}" && m.return_type().is_some() {
                        "()"
                    } else {
                        ""
                    }
                ))
            }
            "${member.value}" => {
                let m = member.unwrap();
                let ignore = self.settings.style != Style::Format && self.settings.skip;
                Element::Expr(self.access(m, ignore))
            }
            _ => Element::Text(token.into()),
        }
    }
    fn elements(&mut self, tokens: &[String], member: Option<BindingRef<'_>>) -> Vec<Element> {
        tokens.iter().map(|t| self.process(t, member)).collect()
    }
    fn appended(&self, elements: &[Element]) -> Vec<String> {
        let elements = collapse(elements);
        if self.settings.style == Style::Chain {
            if elements.is_empty() {
                return vec![];
            }
            vec![format!(
                "{}{};",
                self.builder,
                elements
                    .iter()
                    .map(|e| format!(
                        ".append({})",
                        match e {
                            Element::Text(s) => quote(s),
                            Element::Expr(s) => s.clone(),
                        }
                    ))
                    .collect::<String>()
            )]
        } else {
            elements
                .iter()
                .map(|e| {
                    format!(
                        "{}.append({});",
                        self.builder,
                        match e {
                            Element::Text(s) => quote(s),
                            Element::Expr(s) => s.clone(),
                        }
                    )
                })
                .collect()
        }
    }
    fn body(&mut self, members: &[BindingRef<'_>]) -> String {
        let mut whole = self.elements(&self.template.beginning.clone(), None);
        let mut lines = Vec::new();
        if matches!(self.settings.style, Style::Builder | Style::Chain) {
            let builder_type = self.import("java.lang.StringBuilder");
            lines.push(format!(
                "{builder_type} {} = new {builder_type}();",
                self.builder
            ));
        }
        for (i, m) in members.iter().enumerate() {
            let mut elements = self.elements(&self.template.body.clone(), Some(*m));
            if i + 1 < members.len() {
                elements.push(Element::Text(self.template.separator.clone()));
            }
            let check = self.settings.skip
                && !member_type(*m).is_primitive()
                && self.settings.style != Style::Format;
            if check {
                let raw = self.raw(*m);
                if self.settings.style == Style::Concat {
                    whole.push(Element::Expr(format!(
                        "({raw} != null ? {} : \"\")",
                        sum(&elements)
                    )));
                } else {
                    lines.extend(self.appended(&whole));
                    whole.clear();
                    let inner = self.appended(&elements);
                    if inner.len() == 1 && !self.settings.blocks {
                        lines.push(format!("if ({raw} != null)\n{}", inner[0]));
                    } else {
                        lines.push(format!("if ({raw} != null) {{\n{}\n}}", inner.join("\n")));
                    }
                }
            } else {
                whole.extend(elements)
            }
        }
        whole.extend(self.elements(&self.template.ending.clone(), None));
        match self.settings.style {
            Style::Concat => lines.push(format!("return {};", sum(&whole))),
            Style::Format => {
                let mut pattern = String::new();
                let mut args = Vec::new();
                for e in whole {
                    match e {
                        Element::Text(s) => pattern.push_str(&s),
                        Element::Expr(s) => {
                            pattern.push_str("%s");
                            args.push(s)
                        }
                    }
                }
                let string = self.import("java.lang.String");
                lines.push(format!(
                    "return {string}.format({}{});",
                    quote(&pattern),
                    if args.is_empty() {
                        String::new()
                    } else {
                        format!(", {}", args.join(", "))
                    }
                ));
            }
            _ => {
                lines.extend(self.appended(&whole));
                lines.push(format!("return {}.toString();", self.builder));
            }
        }
        if self.need_max {
            lines.insert(
                0,
                format!("final int {} = {};", self.max_len, self.settings.limit),
            );
        }
        lines.join("\n")
    }
    fn helper(&mut self) -> String {
        let collection = self.name("collection", true);
        let max = self.name("maxLen", true);
        let builder = self.name("builder", false);
        let iterator = self.name("iterator", false);
        let i = self.name("i", false);
        let c = self.import("java.util.Collection");
        let it = self.import("java.util.Iterator");
        let b = self.import("java.lang.StringBuilder");
        let conditional = if self.settings.blocks {
            format!("if ({i} > 0) {{\n{builder}.append(\", \");\n}}")
        } else {
            format!("if ({i} > 0)\n{builder}.append(\", \");")
        };
        format!("private String toString({c}<?> {collection}, int {max}) {{\n{b} {builder} = new {b}();\n{builder}.append(\"[\");\nint {i} = 0;\nfor ({it}<?> {iterator} = {collection}.iterator(); {iterator}.hasNext() && {i} < {max}; {i}++) {{\n{conditional}\n{builder}.append({iterator}.next());\n}}\n{builder}.append(\"]\");\nreturn {builder}.toString();\n}}")
    }
}
fn member_type(b: BindingRef<'_>) -> BindingRef<'_> {
    b.return_type()
        .or_else(|| b.var_type())
        .expect("field or method type")
}
fn implements(t: BindingRef<'_>, name: &str) -> bool {
    let t = t.erasure().unwrap_or(t);
    t.qualified_name() == name || t.interfaces().iter().any(|i| implements(*i, name))
}
pub(super) async fn create(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &accessors::Selection,
    members: &[BindingRef<'_>],
    before: Option<NodeId>,
) -> anyhow::Result<CuChange> {
    let binding = ast
        .node(selected.declaration)
        .binding()
        .ok_or_else(|| anyhow::anyhow!("No toString binding"))?;
    let options = env.options(&ast.uri).await;
    let mut generator = Generator::new(
        Settings::load(),
        ImportRewrite::create_for_corrections(ast.clone(), &options),
        binding,
        members,
        options.clone(),
    )?;
    let body = generator.body(members);
    let profile =
        accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&ast.uri)?).await;
    let mut comment = String::new();
    if generator.settings.comments {
        // 1.58.0 StubUtility always selects the ordinary override template,
        // including Java 23 projects with Markdown comments enabled.
        let pattern = profile.template("overridecomment", "");
        let file_name = tower_lsp::lsp_types::Url::parse(&ast.uri)
            .ok()
            .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
            .unwrap_or_default();
        let package_name = binding.package_name().unwrap_or("");
        comment = accessors::templates::expand_template(pattern, |key| match key {
            "see_to_overridden" => Some("@see java.lang.Object#toString()"),
            "enclosing_type" => Some(binding.qualified_name()),
            "enclosing_method" => Some("toString"),
            "project_name" => Some(profile.project_name.as_str()),
            "file_name" => Some(file_name.as_str()),
            "package_name" => Some(package_name),
            "dollar" => Some("$"),
            _ => None,
        })?;
        if !comment.trim().is_empty() {
            comment.push('\n');
        }
    }
    let level = options
        .get("org.eclipse.jdt.core.compiler.source")
        .and_then(|s| s.trim_start_matches("1.").parse::<u32>().ok())
        .unwrap_or(21);
    let method = format!(
        "{comment}{}public String toString() {{\n{body}\n}}",
        if level >= 5 { "@Override\n" } else { "" }
    );
    let mut methods = vec![(method, true)];
    if generator.helper {
        methods.push((generator.helper(), false));
    }
    let helper_type = generator
        .helper
        .then(|| format!("{}<?>", generator.import("java.util.Collection")));
    let mut rewrite = ASTRewrite::new(ast.clone());
    let eol = if ast.text().contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    for (method, overwrite) in methods {
        let existing = ast
            .node(selected.declaration)
            .list("bodyDeclarations")
            .into_iter()
            .find(|n| {
                n.is(NodeKind::MethodDeclaration)
                    && n.child("name")
                        .is_some_and(|n| n.identifier() == "toString")
                    && if overwrite {
                        n.list("parameters").is_empty()
                    } else {
                        let p = n.list("parameters");
                        p.len() == 2
                            && p[0].child("type").is_some_and(|t| {
                                t.source_text()
                                    .chars()
                                    .filter(|c| !c.is_whitespace())
                                    .collect::<String>()
                                    == *helper_type.as_ref().unwrap()
                            })
                            && p[1].child("type").is_some_and(|t| t.source_text() == "int")
                    }
            });
        if existing.is_some() && !overwrite {
            continue;
        }
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
            Some(edits) => {
                let edits: Vec<_> = edits
                    .into_iter()
                    .map(|e| (e.offset, e.length, e.text))
                    .collect();
                String::from_utf16_lossy(&crate::rewrite::text_edit::apply_flat(
                    &method.encode_utf16().collect::<Vec<_>>(),
                    &edits,
                ))
            }
            None => method,
        };
        let node = rewrite.create_string_placeholder(&formatted, NodeKind::MethodDeclaration);
        if let Some(existing) = existing {
            rewrite.replace(RNode::Orig(existing.id), Some(node));
        } else if let Some(before) = before {
            rewrite.list_insert_before(
                RNode::Orig(selected.declaration),
                "bodyDeclarations",
                node,
                RNode::Orig(before),
            );
        } else {
            rewrite.list_insert_last(RNode::Orig(selected.declaration), "bodyDeclarations", node);
        }
    }
    Ok(CuChange::rewrite(rewrite).with_imports(generator.imports))
}
