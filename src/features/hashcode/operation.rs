//! GenerateHashCodeEqualsOperation's member, array, primitive and Objects strategies.
use super::signature;
use crate::{
    correction::{edit::Env, CuChange},
    features::{
        accessors::{self, Selection},
        constructors::ConstructorImportContext,
        preferences,
    },
    rewrite::{import_rewrite::ImportRewrite, ASTRewrite, RNode},
    semantic_ast::{modifier, Ast, BindingRef, NodeId, NodeKind},
};
use std::{collections::HashSet, sync::Arc};
struct Generator<'a> {
    binding: BindingRef<'a>,
    objects: bool,
    instanceof: bool,
    blocks: bool,
    imports: ImportRewrite,
    context: ConstructorImportContext,
    double_count: usize,
}
impl Generator<'_> {
    fn import(&mut self, name: &str) -> String {
        self.imports.add_import(name, &self.context)
    }
    fn member(&self) -> bool {
        self.binding.is_member() && !self.binding.is_static()
    }
    fn no_super(&self, name: &str) -> bool {
        let mut parent = self.binding.superclass();
        let mut seen = HashSet::new();
        while let Some(p) = parent {
            if !seen.insert(p.id) {
                break;
            }
            if let Some(m) = p
                .declared_methods()
                .unwrap_or_default()
                .into_iter()
                .find(|m| signature(*m, name))
            {
                return m.modifiers() & modifier::ABSTRACT != 0
                    || p.qualified_name() == "java.lang.Object";
            }
            parent = p.superclass();
        }
        true
    }
    fn access(&self, name: &str, hash: bool) -> String {
        let qualify = if hash {
            name == "prime" || name == "result" || name == "temp" && self.double_count > 0
        } else {
            name == "obj" || name == "other"
        };
        if qualify {
            format!("this.{name}")
        } else {
            name.into()
        }
    }
    fn returning_if(&self, condition: &str, value: bool) -> String {
        if self.blocks {
            format!("if ({condition}) {{\nreturn {value};\n}}")
        } else {
            format!("if ({condition})\nreturn {value};")
        }
    }
    fn floating(&mut self, name: &str, access: &str) -> String {
        let (class, method) = if name == "float" {
            ("java.lang.Float", "floatToIntBits")
        } else {
            ("java.lang.Double", "doubleToLongBits")
        };
        format!("{}.{method}({access})", self.import(class))
    }
    fn array_method(&mut self, t: BindingRef<'_>, name: &str, args: &str) -> String {
        let erasure = t.erasure().unwrap_or(t);
        let element = erasure.element_type().unwrap_or(erasure);
        let deep = t.data().dimensions > 1
            || matches!(element.name(), "Cloneable" | "Serializable" | "Object");
        let arrays = self.import("java.util.Arrays");
        let name = if deep {
            if name == "hashCode" {
                "deepHashCode"
            } else {
                "deepEquals"
            }
        } else {
            name
        };
        format!("{arrays}.{name}({args})")
    }
    fn comparison(&mut self, f: BindingRef<'_>, equal: bool) -> String {
        let t = f.var_type().unwrap();
        let a = self.access(f.name(), false);
        let b = format!("other.{}", f.name());
        if t.is_primitive() || t.is_enum() {
            let (a, b) = if matches!(t.name(), "float" | "double") {
                (self.floating(t.name(), &a), self.floating(t.name(), &b))
            } else {
                (a, b)
            };
            format!("{a} {} {b}", if equal { "==" } else { "!=" })
        } else {
            let call = if t.is_array() {
                self.array_method(t, "equals", &format!("{a}, {b}"))
            } else {
                format!("{}.equals({a}, {b})", self.import("java.util.Objects"))
            };
            if equal {
                call
            } else {
                format!("!{call}")
            }
        }
    }
    fn equals(&mut self, fields: &[BindingRef<'_>]) -> String {
        let mut lines = vec![self.returning_if("this == obj", true)];
        if !self.instanceof {
            lines.push(self.returning_if("obj == null", false));
        }
        if !self.no_super("equals") {
            lines.push(self.returning_if("!super.equals(obj)", false));
        }
        if self.instanceof {
            let typ = self.imports.add_import_binding(self.binding, &self.context);
            lines.push(self.returning_if(&format!("!(obj instanceof {typ})"), false));
        } else {
            lines.push(self.returning_if("getClass() != obj.getClass()", false));
        }
        if self.member() || !fields.is_empty() {
            lines.push(format!(
                "{} other = ({}) obj;",
                self.binding.name(),
                self.binding.name()
            ));
        }
        if self.member() {
            lines.push(self.returning_if(
                "!getEnclosingInstance().equals(other.getEnclosingInstance())",
                false,
            ));
        }
        if self.objects && !fields.is_empty() {
            let tests: Vec<_> = fields.iter().map(|f| self.comparison(*f, true)).collect();
            lines.push(format!("return {};", tests.join(" && ")));
        } else {
            for f in fields {
                let t = f.var_type().unwrap();
                if t.is_primitive() || t.is_enum() || t.is_array() {
                    let test = self.comparison(*f, false);
                    lines.push(self.returning_if(&test, false));
                } else {
                    let a = self.access(f.name(), false);
                    let b = format!("other.{}", f.name());
                    lines.push(format!(
                        "if ({a} == null) {{\n{}\n}} else {}",
                        self.returning_if(&format!("{b} != null"), false),
                        self.returning_if(&format!("!{a}.equals({b})"), false)
                    ));
                }
            }
            lines.push("return true;".into());
        }
        lines.join("\n")
    }
    fn hash(&mut self, fields: &[BindingRef<'_>]) -> String {
        if !self.member() && fields.is_empty() {
            return "return super.hashCode();".into();
        }
        let no_super = self.no_super("hashCode");
        if self.objects
            && no_super
            && !self.member()
            && fields.iter().all(|f| !f.var_type().unwrap().is_array())
        {
            let objects = self.import("java.util.Objects");
            let args: Vec<_> = fields
                .iter()
                .map(|f| {
                    let t = f.var_type().unwrap();
                    if t.is_primitive() {
                        let wrapper = match t.name() {
                            "boolean" => "Boolean",
                            "byte" => "Byte",
                            "short" => "Short",
                            "char" => "Character",
                            "int" => "Integer",
                            "long" => "Long",
                            "float" => "Float",
                            _ => "Double",
                        };
                        format!("{wrapper}.valueOf({})", f.name())
                    } else {
                        f.name().into()
                    }
                })
                .collect();
            return format!("return {objects}.hash({});", args.join(", "));
        }
        let mut lines = vec![
            "final int prime = 31;".into(),
            format!(
                "int result = {};",
                if no_super { "1" } else { "super.hashCode()" }
            ),
        ];
        if self.member() {
            lines.push("result = prime * result + getEnclosingInstance().hashCode();".into());
        }
        let mut args = Vec::new();
        for f in fields {
            let t = f.var_type().unwrap();
            if !t.is_array() && self.objects {
                args.push(self.access(f.name(), true));
                continue;
            }
            if t.name() == "double" && t.is_primitive() {
                if self.double_count == 0 {
                    lines.push("long temp;".into());
                }
                self.double_count += 1;
                let a = self.access(f.name(), true);
                let expr = self.floating("double", &a);
                lines.push(format!("temp = {expr};"));
                lines.push("result = prime * result + (int) (temp ^ (temp >>> 32));".into());
                continue;
            }
            let a = self.access(f.name(), true);
            let expr = if t.is_array() {
                self.array_method(t, "hashCode", &a)
            } else if t.is_primitive() {
                match t.name() {
                    "boolean" => format!("({a} ? 1231 : 1237)"),
                    "long" => format!("(int) ({a} ^ ({a} >>> 32))"),
                    "float" => self.floating("float", &a),
                    _ => a,
                }
            } else {
                format!("(({a} == null) ? 0 : {a}.hashCode())")
            };
            lines.push(format!("result = prime * result + {expr};"));
        }
        if !args.is_empty() {
            let objects = self.import("java.util.Objects");
            lines.push(format!(
                "result = prime * result + {objects}.hash({});",
                args.join(", ")
            ));
        }
        lines.push("return result;".into());
        lines.join("\n")
    }
}
pub(super) async fn create(
    env: &Env<'_>,
    ast: Arc<Ast>,
    selected: &Selection,
    fields: &[BindingRef<'_>],
    before: Option<NodeId>,
    regenerate: bool,
) -> anyhow::Result<CuChange> {
    let binding = ast
        .node(selected.declaration)
        .binding()
        .ok_or_else(|| anyhow::anyhow!("Missing hashCode/equals type binding"))?;
    let options = env.options(&ast.uri).await;
    let mut generator = Generator {
        binding,
        objects: preferences::get_bool("java.codeGeneration.hashCodeEquals.useJava7Objects")
            .unwrap_or(false),
        instanceof: preferences::get_bool("java.codeGeneration.hashCodeEquals.useInstanceof")
            .unwrap_or(false),
        blocks: preferences::get_bool("java.codeGeneration.useBlocks").unwrap_or(false),
        imports: ImportRewrite::create_for_corrections(ast.clone(), &options),
        context: ConstructorImportContext {
            ast: ast.clone(),
            declaration: Some(selected.declaration),
        },
        double_count: 0,
    };
    // JDT creates equals first, then inserts hashCode before that generated node.
    let equals = generator.equals(fields);
    let hash = generator.hash(fields);
    let object = generator.import("java.lang.Object");
    let profile =
        accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&ast.uri)?).await;
    let comments = preferences::generate_comments();
    let level = options
        .get("org.eclipse.jdt.core.compiler.source")
        .and_then(|s| s.trim_start_matches("1.").parse::<u32>().ok())
        .unwrap_or(21);
    let annotation = if level >= 5 { "@Override\n" } else { "" };
    let file_name = tower_lsp::lsp_types::Url::parse(&ast.uri)
        .ok()
        .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
        .unwrap_or_default();
    let comment = |name: &str, see: &str| -> anyhow::Result<String> {
        if !comments {
            return Ok(String::new());
        }
        let c = accessors::templates::expand_template(
            profile.template("overridecomment", ""),
            |key| match key {
                "see_to_overridden" => Some(see),
                "enclosing_type" => Some(binding.qualified_name()),
                "enclosing_method" => Some(name),
                "file_name" => Some(file_name.as_str()),
                "package_name" => Some(binding.package_name().unwrap_or("")),
                "project_name" => Some(profile.project_name.as_str()),
                "return_type" => Some(if name == "equals" { "boolean" } else { "int" }),
                "dollar" => Some("$"),
                _ => None,
            },
        )?;
        Ok(if c.trim().is_empty() {
            String::new()
        } else {
            format!("{c}\n")
        })
    };
    let methods = [
        (
            "equals",
            format!(
                "{}{annotation}public boolean equals({object} obj) {{\n{equals}\n}}",
                comment("equals", "@see java.lang.Object#equals(java.lang.Object)")?
            ),
        ),
        (
            "hashCode",
            format!(
                "{}{annotation}public int hashCode() {{\n{hash}\n}}",
                comment("hashCode", "@see java.lang.Object#hashCode()")?
            ),
        ),
    ];
    let mut rewrite = ASTRewrite::new(ast.clone());
    let parent = RNode::Orig(selected.declaration);
    let mut equals_node = None;
    let eol = if ast.text().contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    for (name, method) in methods {
        let existing = if regenerate {
            ast.node(selected.declaration)
                .list("bodyDeclarations")
                .into_iter()
                .find(|n| {
                    n.is(NodeKind::MethodDeclaration)
                        && n.binding().is_some_and(|m| signature(m, name))
                })
        } else {
            None
        };
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
        if let Some(existing) = existing {
            rewrite.replace(RNode::Orig(existing.id), Some(node));
        } else if let Some(anchor) = equals_node {
            rewrite.list_insert_before(parent, "bodyDeclarations", node, anchor);
        } else if let Some(before) = before {
            rewrite.list_insert_before(parent, "bodyDeclarations", node, RNode::Orig(before));
        } else {
            rewrite.list_insert_last(parent, "bodyDeclarations", node);
        }
        if name == "equals" {
            equals_node = Some(node);
        }
    }
    if generator.member()
        && !ast
            .node(selected.declaration)
            .list("bodyDeclarations")
            .iter()
            .any(|n| {
                n.binding()
                    .is_some_and(|m| signature(m, "getEnclosingInstance"))
            })
    {
        let outer = binding
            .declaring_class()
            .unwrap()
            .type_declaration()
            .unwrap_or(binding.declaring_class().unwrap())
            .name();
        let helper = format!("private {outer} getEnclosingInstance() {{\nreturn {outer}.this;\n}}");
        // Apply the same formatter as the two public methods.
        let helper = match env
            .dispatcher
            .format_source(
                &helper,
                crate::rewrite::formatter::K_CLASS_BODY_DECLARATIONS,
                0,
                helper.encode_utf16().count(),
                eol,
                options.clone(),
            )
            .await?
        {
            Some(edits) => String::from_utf16_lossy(&crate::rewrite::text_edit::apply_flat(
                &helper.encode_utf16().collect::<Vec<_>>(),
                &edits
                    .into_iter()
                    .map(|e| (e.offset, e.length, e.text))
                    .collect::<Vec<_>>(),
            )),
            None => helper,
        };
        let node = rewrite.create_string_placeholder(&helper, NodeKind::MethodDeclaration);
        rewrite.list_insert_last(parent, "bodyDeclarations", node);
    }
    Ok(CuChange::rewrite(rewrite).with_imports(generator.imports))
}
