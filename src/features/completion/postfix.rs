//! Port of jdt.ls postfix completion: `SnippetCompletionProposal.getPostfixSnippets`,
//! `PostfixCompletionProposalComputer`, `PostfixTemplateEngine`,
//! `JavaPostfixContext`, the `PostfixTemplate` definitions and the template
//! variable resolvers they use (`InnerExpressionResolver`, `NameResolver`,
//! `TypeResolver`, `ActualTypeResolver`).
//!
//! The DOM questions (the expression in front of the `.`, its type and
//! supertypes, name suggestions) are answered from the resolved AST of the
//! unit ([`crate::semantic_ast`]); the template evaluation itself is pure and
//! repeated by `completionItem/resolve`.

use super::doc::Doc;
use super::handler::{self, UnitInfo};
use super::import_context::ImportContext;
use super::Env;
use crate::analysis::dispatcher::RequestContext;
use super::imports::{ContainerTypes, CuStructure, ImportRewrite};
use super::item::{item_kind, Item, ItemDefaults, LabelDetails};
use super::prefs::{Client, Prefs};
use super::proposal::{tl, Context};
use super::snippets::{beautify_document, set_insert_text_format, set_insert_text_mode, template_to_snippet, translate, Template};
use super::sort_text::convert_relevance;
use super::template_store;
use crate::semantic_ast::{Ast, BindingRef, Node, NodeId, NodeKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_lsp::lsp_types::{Range, TextEdit};

/// `JavaPostfixContextType.ID_ALL`.
pub const ID_ALL: &str = "postfix";

/// `ASTNode.RECOVERED`.
const RECOVERED: u32 = 8;

/// `InnerExpressionResolver.FLAGS`.
const INNER_EXPRESSION_FLAGS: [&str; 1] = ["novalue"];

const ASSERT_CONTENT: &str = "assert ${i:inner_expression(boolean,java.lang.Boolean)};";
const CAST_CONTENT: &str = "(($${1})${inner_expression})$${0}";
const ELSE_CONTENT: &str = "if (!${i:inner_expression(boolean)}) {\n\t$${0}\n}";
const FOR_CONTENT: &str = "for (${type:newActualType(i)} $${1:${n:newName(i)}} : ${i:inner_expression(java.util.Collection,array)}) {\n\t$${0}\n}";
const FORI_CONTENT: &str = "for (int $${1:${index}} = 0; $${1:${index}} < ${i:inner_expression(array)}.length; $${1:${index}}++) {\n\t$${0}\n}";
const FORR_CONTENT: &str = "for (int $${1:${index}} = ${i:inner_expression(array)}.length - 1; $${1:${index}} >= 0; $${1:${index}}--) {\n\t$${0}\n}";
const FORMAT_CONTENT: &str = "String.format(${i:inner_expression(java.lang.String)}${}, $${0});";
const IF_CONTENT: &str = "if (${i:inner_expression(boolean)}) {\n\t$${0}\n}";
const NNULL_CONTENT: &str = "if (${i:inner_expression(java.lang.Object,array)} != null) {\n\t$${0}\n}";
const NULL_CONTENT: &str = "if (${i:inner_expression(java.lang.Object,array)} == null) {\n\t$${0}\n}";
const NOT_CONTENT: &str = "!${i:inner_expression(boolean)}${}";
const OPT_CONTENT: &str = "Optional.ofNullable(${i:inner_expression(java.lang.Object)}${})";
const SYSOUT_CONTENT: &str = "System.out.println(${i:inner_expression(java.lang.Object)}${});$${0}";
const SYSOUTV_CONTENT: &str = "System.out.println(\"${i:inner_expression(java.lang.Object)}${} = \" + ${i:inner_expression(java.lang.Object)}${});$${0}";
const SYSOUF_CONTENT: &str = "System.out.printf(\"\", ${i:inner_expression(java.lang.Object)}${});$${0}";
const SYSERR_CONTENT: &str = "System.err.println(${i:inner_expression(java.lang.Object)}${});$${0}";
const THROW_CONTENT: &str = "throw ${true:inner_expression(java.lang.Throwable)};";
const VAR_CONTENT: &str = "${field:newType(inner_expression)} $${1:${var:newName(inner_expression)}} = ${inner_expression};$${0}";
const PAR_CONTENT: &str = "(${i:inner_expression}${})";
const WHILE_CONTENT: &str = "while (${i:inner_expression(boolean)}) {\n\t$${0}\n}";

/// `PostfixTemplate.values()` in declaration order (`createTemplate`).
pub fn templates() -> Vec<Template> {
    let t = |id: &str, name: &str, pattern: &str, desc: &str| Template {
        id: id.to_owned(),
        name: name.to_owned(),
        description: desc.to_owned(),
        context_type: ID_ALL.to_owned(),
        pattern: pattern.to_owned(),
    };
    vec![
        t("org.eclipse.jdt.postfixcompletion.assert", "assert", ASSERT_CONTENT, "Creates an assert statement"),
        t("org.eclipse.jdt.postfixcompletion.cast", "cast", CAST_CONTENT, "Casts the expression to a new type"),
        t("org.eclipse.jdt.ls.postfixcompletion.if", "if", IF_CONTENT, "Creates a if statement"),
        t("org.eclipse.jdt.ls.postfixcompletion.else", "else", ELSE_CONTENT, "Creates a negated if statement"),
        t("org.eclipse.jdt.postfixcompletion.for", "for", FOR_CONTENT, "Creates a for statement"),
        t("org.eclipse.jdt.postfixcompletion.fori", "fori", FORI_CONTENT, "Creates a for statement which iterates over an array"),
        t("org.eclipse.jdt.postfixcompletion.forr", "forr", FORR_CONTENT, "Creates a for statement which iterates over an array in reverse order"),
        t("org.eclipse.jdt.postfixcompletion.format", "format", FORMAT_CONTENT, "Sends the affected object to the String.format(..) method"),
        t("org.eclipse.jdt.postfixcompletion.nnull", "nnull", NNULL_CONTENT, "Creates an if statement and checks if the expression does not resolve to null"),
        t("org.eclipse.jdt.postfixcompletion.null", "null", NULL_CONTENT, "Creates an if statement which checks if expression resolves to null"),
        t("org.eclipse.jdt.postfixcompletion.not", "not", NOT_CONTENT, "Negates the expression"),
        t("org.eclipse.jdt.postfixcompletion.opt", "opt", OPT_CONTENT, "Creates an Optional.ofNullable(..) call"),
        t("org.eclipse.jdt.postfixcompletion.sysout", "sysout", SYSOUT_CONTENT, "Sends the affected object to a System.out.println(..) call"),
        t("org.eclipse.jdt.postfixcompletion.sysouf", "sysouf", SYSOUF_CONTENT, "Sends the affected object to a System.out.printf(..) call"),
        t("org.eclipse.jdt.postfixcompletion.sysoutv", "sysoutv", SYSOUTV_CONTENT, "Sends the affected object to a System.out.println(..) call"),
        t("org.eclipse.jdt.postfixcompletion.syserr", "syserr", SYSERR_CONTENT, "Sends the affected object to a System.err.println(..) call"),
        t("org.eclipse.jdt.postfixcompletion.throw", "throw", THROW_CONTENT, "Throws the given Exception"),
        t("org.eclipse.jdt.postfixcompletion.var", "var", VAR_CONTENT, "Creates a new variable"),
        t("org.eclipse.jdt.postfixcompletion.par", "par", PAR_CONTENT, "Places the expression in parentheses"),
        t("org.eclipse.jdt.postfixcompletion.while", "while", WHILE_CONTENT, "Creates a while loop"),
    ]
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `SnippetCompletionProposal.canResolvePostfix`: triggered by `.`, a
/// template starts with the token, not in an import declaration.
pub fn can_resolve_postfix(context: &Context, doc: &Doc) -> bool {
    let Some(token) = context.token.as_deref() else { return false };
    if context.token_location & tl::IN_IMPORT != 0 {
        return false;
    }
    let token_start = context.offset - utf16_len(token) as i32 - 1;
    if token_start < 0 {
        return false;
    }
    let sequence = doc.get(token_start as usize, (context.offset - token_start) as usize);
    if !sequence.starts_with('.') {
        return false;
    }
    let token = token.to_lowercase();
    template_store::templates_of(ID_ALL).iter().any(|t| t.name.to_lowercase().starts_with(&token))
}

// ─── The postfix context (data the template evaluation needs) ───────────────

/// Name sources of `StubUtility.getVariableNameSuggestions(VK_LOCAL, project,
/// typeBinding, expression, excluded)` for the selected node.
#[derive(Debug, Clone, Default)]
pub struct NameSources {
    /// `selectedNode instanceof Expression`.
    pub is_expression: bool,
    pub from_expression: Option<String>,
    pub from_parent: Option<String>,
    /// Element type name and dimensions of the normalized type binding.
    pub from_type: Option<(String, usize)>,
}

/// What `JavaPostfixContext.addImport` needs: the import rewrite of the unit
/// and the `ContextSensitiveImportRewriteContext` at the completion offset.
#[derive(Debug, Clone)]
pub struct ImportEnv {
    pub cu: Arc<CuStructure>,
    pub context: ImportContext,
    pub container_types: ContainerTypes,
    pub import_order: Vec<String>,
    pub on_demand_threshold: i64,
    pub static_on_demand_threshold: i64,
    pub line_delimiter: String,
    pub blank_lines_between_import_groups: usize,
    pub space_before_semicolon: bool,
}

/// `JavaPostfixContext`.
#[derive(Debug, Clone)]
pub struct PostfixContext {
    /// `getStart()`.
    pub start: usize,
    /// `getEnd()`: the completion offset.
    pub end: usize,
    /// `getAffectedStatement()`.
    pub affected_statement: String,
    /// `getInnerExpressionTypeSignature()`.
    pub inner_type: String,
    pub names: NameSources,
    /// `getCompletion().getLocalVariableNames()` at the template start;
    /// computed on first use (`None` until then).
    pub local_names: Option<Vec<String>>,
    pub imports: ImportEnv,
    /// `additionalTextEdits`: the import edits recorded per template name by
    /// every evaluation of this context (completion and each resolve). They
    /// are shared, mutable `TextEdit`s: see [`PostfixContext::convert_additional_text_edits`].
    pub recorded_edits: Arc<Mutex<HashMap<String, Vec<RawEdit>>>>,
}

/// A stored postfix proposal (`PostfixCompletionProposal`).
#[derive(Debug, Clone)]
pub struct PostfixProposal {
    pub template: Template,
    pub context: PostfixContext,
}

/// An import edit: (offset, length, new text).
pub type RawEdit = (usize, usize, String);

/// Result of a template evaluation.
pub struct Evaluation {
    /// `TemplateBuffer.getString()`, `None` when only whitespace.
    pub content: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Resolved {
    value: String,
    /// `JavaVariable.getParamType()`.
    param_type: Option<String>,
    /// A `JavaVariable` (all translated variables are; looked up by name).
    exists: bool,
}

struct Evaluator<'a> {
    ctx: &'a PostfixContext,
    vars: Vec<super::snippets::Var>,
    values: HashMap<String, Resolved>,
    used: Vec<String>,
    import_edits: Vec<RawEdit>,
}

impl PostfixContext {
    /// `PostfixTemplateEngine.evaluateGenericTemplate`.
    pub fn evaluate(&self, template: &Template) -> Evaluation {
        let (segments, vars) = translate(&template.pattern);
        let mut ev = Evaluator { ctx: self, vars: vars.clone(), values: HashMap::new(), used: Vec::new(), import_edits: Vec::new() };
        for v in &vars {
            ev.resolve(&v.name);
        }
        let mut occ: Vec<(usize, String)> = Vec::new();
        for v in &vars {
            for &o in &v.offsets {
                occ.push((o, v.name.clone()));
            }
        }
        occ.sort();
        let mut out = String::new();
        for (i, seg) in segments.iter().enumerate() {
            out.push_str(seg);
            if let Some((_, name)) = occ.get(i) {
                out.push_str(ev.values.get(name).map(|r| r.value.as_str()).unwrap_or(name));
            }
        }
        let content = if out.trim().is_empty() { None } else { Some(out) };
        // addImport: the edits are recorded under the active template name
        if !template.name.trim().is_empty() {
            let mut recorded = self.recorded_edits.lock().unwrap_or_else(|e| e.into_inner());
            recorded.entry(template.name.clone()).or_default().extend(ev.import_edits.iter().cloned());
        }
        Evaluation { content }
    }

    /// `getAdditionalTextEdits(name)` converted by `TextEditConverter`. Its
    /// `visit(MultiTextEdit)` applies each edit with `UPDATE_REGIONS`, so an
    /// edit converted again covers the text it inserted (`offset`, length
    /// of its new text) instead of its original region.
    pub fn convert_additional_text_edits(&self, name: &str) -> Vec<RawEdit> {
        let mut recorded = self.recorded_edits.lock().unwrap_or_else(|e| e.into_inner());
        let Some(edits) = recorded.get_mut(name) else { return Vec::new() };
        let converted = edits.clone();
        for edit in edits.iter_mut() {
            edit.1 = utf16_len(&edit.2);
        }
        converted
    }
}

impl Evaluator<'_> {
    /// `JavaContextCore.getTemplateVariable(name)` (resolving on demand).
    fn resolve(&mut self, name: &str) -> Option<Resolved> {
        if let Some(r) = self.values.get(name) {
            return Some(r.clone());
        }
        let var = self.vars.iter().find(|v| v.name == name)?.clone();
        let mut resolved = self.resolve_var(&var);
        resolved.exists = true;
        self.values.insert(name.to_owned(), resolved.clone());
        Some(resolved)
    }

    fn resolve_var(&mut self, var: &super::snippets::Var) -> Resolved {
        match var.ty.as_str() {
            // InnerExpressionResolver
            "inner_expression" => {
                let value = if var.params.iter().any(|p| p == INNER_EXPRESSION_FLAGS[0]) {
                    String::new()
                } else {
                    self.ctx.affected_statement.clone()
                };
                Resolved { value, param_type: Some(self.ctx.inner_type.clone()), exists: true }
            }
            // NameResolver ("newName", default java.lang.Object) and AbstractJavaContextTypeCore.Index ("int")
            "newName" | "index" => {
                let default = if var.ty == "index" { "int" } else { "java.lang.Object" };
                let param = var.params.first().cloned().unwrap_or_else(|| default.to_owned());
                let names = if self.vars.iter().any(|v| v.name == param) {
                    let reference = self.resolve(&param).unwrap_or_default();
                    self.suggest_variable_names(&reference.value)
                } else {
                    self.add_import(&param);
                    self.suggest_variable_names(&param)
                };
                let value = names.first().cloned().unwrap_or_else(|| var.name.clone());
                self.used.push(value.clone());
                Resolved { value, ..Default::default() }
            }
            "newType" => self.resolve_type(var),
            "newActualType" => {
                if let Some(param) = var.params.first().cloned() {
                    if self.vars.iter().any(|v| v.name == param) {
                        let reference = self.resolve(&param).unwrap_or_default();
                        if let Some(mut p) = reference.param_type.filter(|p| !p.is_empty()) {
                            p = p.replace("? extends ", "");
                            if let Some(s) = p.strip_suffix("[]") {
                                // String[] => String, List<String>[] => List<String>
                                p = s.to_owned();
                            } else if p.ends_with('>') {
                                // List<Integer> => Integer, Map<Integer,String> => Integer
                                let open = p.find('<').unwrap_or(0);
                                let close = p.rfind('>').unwrap_or(p.len());
                                p = p[open + 1..close].to_owned();
                                if !p.contains('<') && p.contains(',') {
                                    p = p[..p.find(',').unwrap_or(p.len())].to_owned();
                                }
                            }
                            let value = self.add_import_generic_class(&p);
                            return Resolved { value, ..Default::default() };
                        }
                    }
                }
                self.resolve_type(var)
            }
            // TemplateVariableResolver default: the variable name
            _ => Resolved { value: var.name.clone(), ..Default::default() },
        }
    }

    /// jdt.ls `TypeResolver.resolve` (default type `java.lang.Object`).
    fn resolve_type(&mut self, var: &super::snippets::Var) -> Resolved {
        let mut param = "java.lang.Object".to_owned();
        if let Some(p) = var.params.first().cloned() {
            param = p;
            if self.vars.iter().any(|v| v.name == param) {
                let reference = self.resolve(&param).unwrap_or_default();
                match reference.param_type {
                    Some(p) if !p.is_empty() => {
                        let value = self.add_import_generic_class(&p);
                        return Resolved { value, ..Default::default() };
                    }
                    other => param = other.unwrap_or_default(),
                }
            }
        }
        let value = self.add_import(&param);
        Resolved { value, ..Default::default() }
    }

    /// `JavaContextCore.computeExcludes`: local variable names and used names.
    fn excludes(&self) -> Vec<String> {
        let mut ex = self.ctx.local_names.clone().unwrap_or_default();
        ex.extend(self.used.iter().cloned());
        ex
    }

    /// `JavaPostfixContext.suggestVariableNames`.
    fn suggest_variable_names(&self, ty: &str) -> Vec<String> {
        let excludes = self.excludes();
        let mut res: Vec<String> = Vec::new();
        let n = &self.ctx.names;
        if n.is_expression {
            // StubUtility.getVariableNameSuggestions(VK_LOCAL, project, tb, expression, excludes)
            let mut set: Vec<String> = Vec::new();
            let mut add = |names: Vec<String>| {
                for name in names {
                    if !set.contains(&name) {
                        set.push(name);
                    }
                }
            };
            if let Some(base) = &n.from_expression {
                add(super::naming::suggest_variable_names(base, 0, &excludes, false));
            }
            if let Some(base) = &n.from_parent {
                add(super::naming::suggest_variable_names(base, 0, &excludes, false));
            }
            if let Some((base, dim)) = &n.from_type {
                add(super::naming::suggest_variable_names(base, *dim, &excludes, false));
            }
            if set.is_empty() {
                // getDefaultVariableNameSuggestions(VK_LOCAL, excluded)
                let mut name = "x".to_owned();
                let mut i = 1;
                while excludes.contains(&name) {
                    name = format!("x{i}");
                    i += 1;
                }
                set.push(name);
            }
            res.extend(set);
        }
        // JavaContextCore.suggestVariableNames(type)
        let mut t = ty.to_owned();
        let mut dim = 0;
        while let Some(s) = t.strip_suffix("[]") {
            t = s.to_owned();
            dim += 1;
        }
        res.extend(super::naming::suggest_variable_names(&t, dim, &excludes, true));
        res
    }

    /// `JavaPostfixContext.addImport`: unqualified names stay; qualified ones
    /// go through a fresh import rewrite whose edit is recorded.
    fn add_import(&mut self, ty: &str) -> String {
        if !ty.contains('.') {
            return ty.to_owned();
        }
        let env = &self.ctx.imports;
        let mut rewrite = ImportRewrite::create(env.cu.clone(), env.import_order.clone(), env.on_demand_threshold, env.static_on_demand_threshold);
        let context = &env.context;
        let ctx = move |rw: &ImportRewrite, qualifier: &str, name: &str, kind: i32| -> i32 { context.find_in_context(rw, qualifier, name, kind) };
        let name = rewrite.add_import_with(ty, Some(&ctx));
        // rewriteImports is recorded even without changes: TextEditConverter
        // turns the empty MultiTextEdit into an empty edit at offset 0.
        let edit = rewrite
            .rewrite(&env.container_types, &env.line_delimiter, env.blank_lines_between_import_groups, env.space_before_semicolon)
            .unwrap_or((0, 0, String::new()));
        self.import_edits.push(edit);
        name
    }

    /// `JavaPostfixContext.addImportGenericClass`.
    fn add_import_generic_class(&mut self, class_name: &str) -> String {
        const ID_SEPARATOR: &str = "\u{FFFF}\u{FFFE}";
        let re = regex::Regex::new(r"[a-zA-Z0-9$_\.]+").unwrap();
        let mut class_names: Vec<String> = re.find_iter(class_name).map(|m| m.as_str().to_owned()).collect();
        // Collections.sort by length, longest first (stable)
        class_names.sort_by(|a, b| utf16_len(b).cmp(&utf16_len(a)));
        let mut name = class_name.to_owned();
        let mut mapping: HashMap<String, String> = HashMap::new();
        for (i, c) in class_names.iter().enumerate() {
            name = name.replace(c.as_str(), &format!("{ID_SEPARATOR}{i}{ID_SEPARATOR}"));
            let imported = self.add_import(c);
            mapping.insert(c.clone(), imported);
        }
        for (i, c) in class_names.iter().enumerate() {
            name = name.replace(&format!("{ID_SEPARATOR}{i}{ID_SEPARATOR}"), &mapping[c]);
        }
        name
    }
}

// ─── DOM analysis (PostfixCompletionProposalComputer + JavaPostfixContext) ──

/// The parts of the postfix context computed from the AST.
pub struct Analysis {
    pub start: usize,
    pub end: usize,
    pub affected_statement: String,
    pub inner_type: String,
    pub names: NameSources,
    /// `canEvaluate` for each of [`templates`].
    pub can_evaluate: Vec<bool>,
}

/// Visits the subtree of `node` in preorder; `visit` returns whether to descend.
fn visit_preorder<'a>(node: Node<'a>, mut visit: impl FnMut(Node<'a>) -> bool) {
    let ast = node.ast;
    let end = ast.subtree_end(node.id).0;
    let mut i = node.id.0;
    while i < end {
        let n = ast.node(NodeId(i));
        i = if visit(n) { i + 1 } else { ast.subtree_end(n.id).0 };
    }
}

/// The member the completion context reports as enclosing element (its
/// source range found back with `NodeFinder`).
fn enclosing_member(ast: &Ast, offset: usize) -> Option<Node<'_>> {
    let mut best: Option<Node<'_>> = None;
    visit_preorder(ast.root(), |n| {
        if n.start() > offset || n.end() < offset {
            return false;
        }
        if n.kind().is_body_declaration() {
            best = Some(n);
        }
        true
    });
    best
}

/// `findBestMatchingParentNode`.
fn find_best_matching_parent_node(node: Node<'_>) -> Option<Node<'_>> {
    let mut result = node.parent();
    if let Some(r) = result.filter(|r| r.is(NodeKind::InfixExpression)) {
        let mut result_node = r;
        let mut grand_parent = r.parent();
        let mut safe_guard = 0;
        while let Some(gp) = grand_parent.filter(|gp| gp.is(NodeKind::ParenthesizedExpression)) {
            if safe_guard >= 64 {
                break;
            }
            safe_guard += 1;
            result_node = gp;
            grand_parent = gp.parent();
        }
        result = Some(result_node);
    }
    if node.is(NodeKind::SimpleName) {
        if let Some(r) = result.filter(|r| r.is(NodeKind::SimpleType)) {
            if let Some(gp) = r.parent().filter(|gp| gp.is(NodeKind::ClassInstanceCreation)) {
                result = Some(gp);
            }
        }
    }
    result
}

/// `getNodeBegin`.
fn node_begin(node: Node<'_>) -> i64 {
    if let Some(p) = node.parent() {
        if p.is(NodeKind::MethodInvocation) || p.is(NodeKind::FieldAccess) || p.is(NodeKind::SuperFieldAccess) {
            return p.start() as i64;
        }
    }
    if node.kind().is_name() {
        let mut n = node;
        while let Some(p) = n.parent().filter(|p| p.is(NodeKind::QualifiedName)) {
            n = p;
        }
        return n.start() as i64;
    }
    node.start() as i64
}

/// `resolveNodeToBinding`.
fn resolve_node_to_binding(node: Node<'_>) -> Option<BindingRef<'_>> {
    if node.is(NodeKind::StringLiteral) {
        return node.type_binding();
    }
    let mut res: Option<BindingRef<'_>> = None;
    visit_preorder(node, |n| match n.kind() {
        NodeKind::MethodInvocation => {
            res = n.type_binding();
            false
        }
        NodeKind::SimpleName => {
            if let Some(b) = n.binding() {
                if b.is_variable() {
                    res = b.var_type();
                } else if b.is_method() {
                    res = b.return_type();
                }
            }
            false
        }
        NodeKind::QualifiedName => {
            if let Some(b) = n.binding().filter(|b| b.is_variable()) {
                res = b.var_type();
            }
            false
        }
        NodeKind::FieldAccess => {
            res = n.child("name").and_then(|c| c.type_binding()).or_else(|| n.child("expression").and_then(|e| e.type_binding()));
            false
        }
        NodeKind::Assignment => match n.child("leftHandSide").and_then(|l| l.type_binding()) {
            Some(t) => {
                res = Some(t);
                false
            }
            None => true,
        },
        NodeKind::BooleanLiteral | NodeKind::InfixExpression | NodeKind::ClassInstanceCreation | NodeKind::ArrayAccess => {
            res = n.type_binding();
            false
        }
        _ => true,
    });
    res
}

/// `resolveNodeToTypeString`.
fn resolve_node_to_type_string(node: Node<'_>) -> String {
    let Some(b) = resolve_node_to_binding(node) else { return "java.lang.Object".to_owned() };
    let result = b.qualified_name().to_owned();
    if result.is_empty() && b.is_capture() {
        for tb in b.type_bounds() {
            let r = tb.qualified_name();
            if !r.is_empty() {
                return r.to_owned();
            }
        }
    }
    result
}

/// `isNodeResolvingTo`.
fn is_node_resolving_to(node: Node<'_>, signature: &str) -> bool {
    if signature.trim().is_empty() {
        return true;
    }
    let tb = resolve_node_to_binding(node);
    match tb {
        Some(t) if t.is_primitive() => t.qualified_name() == signature,
        _ => resolves_reference_binding_to(tb, signature, 0),
    }
}

/// `resolvesReferenceBindingTo`.
fn resolves_reference_binding_to(sb: Option<BindingRef<'_>>, signature: &str, depth: usize) -> bool {
    let Some(sb) = sb else { return false };
    if depth > 64 {
        return false;
    }
    if sb.qualified_name().starts_with(signature) || (sb.is_array() && signature == "array") {
        return true;
    }
    if signature == "java.lang.Object" {
        return true;
    }
    let mut bindings: Vec<Option<BindingRef<'_>>> = sb.interfaces().into_iter().map(Some).collect();
    bindings.push(sb.superclass());
    bindings.into_iter().any(|b| resolves_reference_binding_to(b, signature, depth + 1))
}

const KNOWN_METHOD_NAME_PREFIXES: [&str; 17] = [
    "get", "is", "to", "create", "load", "find", "build", "generate", "prepare", "parse", "current", "read", "resolve", "retrieve", "make",
    "add", "extract",
];

/// `NamingConventions.getBaseName(getKind(binding), name)` with jdt.ls's
/// default (empty) prefixes and suffixes; static final fields become camel case.
fn variable_base_name(b: BindingRef<'_>) -> String {
    let static_final = crate::semantic_ast::modifier::STATIC | crate::semantic_ast::modifier::FINAL;
    if b.is_field() && b.modifiers() & static_final == static_final {
        let mut out = String::new();
        for (i, part) in b.name().split('_').filter(|p| !p.is_empty()).enumerate() {
            let lower = part.to_lowercase();
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                if i == 0 {
                    out.push(first);
                } else {
                    out.extend(first.to_uppercase());
                }
                out.push_str(chars.as_str());
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    b.name().to_owned()
}

/// `StubUtility.getBaseNameFromExpression(project, expression, VK_LOCAL)`.
fn base_name_from_expression(expression: Node<'_>) -> Option<String> {
    let mut e = expression;
    if e.is(NodeKind::CastExpression) {
        e = e.child("expression")?;
    }
    let name = match e.kind() {
        NodeKind::SimpleName | NodeKind::QualifiedName => {
            if let Some(b) = e.binding().filter(|b| b.is_variable()) {
                return Some(variable_base_name(b));
            }
            let simple = if e.is(NodeKind::QualifiedName) { e.child("name")? } else { e };
            return Some(simple.identifier());
        }
        NodeKind::MethodInvocation => {
            let name = e.child("name")?.identifier();
            if name == "next" {
                let modified = match e.child("expression") {
                    Some(r) if r.is(NodeKind::SimpleName) => modify_base_name(&r.identifier()),
                    _ => "element".to_owned(),
                };
                if modified != "element" {
                    return Some(modified);
                }
            }
            name
        }
        NodeKind::SuperMethodInvocation => e.child("name")?.identifier(),
        NodeKind::FieldAccess => return Some(e.child("name")?.identifier()),
        _ => return None,
    };
    for prefix in KNOWN_METHOD_NAME_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            if rest.chars().next().is_some_and(char::is_uppercase) {
                return Some(rest.to_owned());
            }
        }
    }
    Some(name)
}

/// `ConvertLoopOperation.modifyBaseName`.
fn modify_base_name(suggested_name: &str) -> String {
    const ELEMENT: &str = "element";
    const IRREG_NOUNS: [(&str, &str); 18] = [
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
    ];
    const NO_BASE_TYPES: [&str; 8] = ["integers", "floats", "doubles", "booleans", "bytes", "chars", "shorts", "longs"];
    const IRREG_ENDINGS: [&str; 13] = ["xes", "ies", "oes", "ses", "hes", "zes", "ves", "ces", "ss", "is", "us", "os", "as"];
    let mut name = suggested_name.to_owned();
    for prefix in ["all"] {
        if prefix.len() >= suggested_name.chars().count() {
            continue;
        }
        let after_prefix = suggested_name.chars().nth(prefix.len()).unwrap_or(' ');
        if (after_prefix.is_uppercase() || after_prefix == '_') && suggested_name.to_lowercase().starts_with(prefix) {
            let without: String = suggested_name.chars().skip(prefix.len()).collect();
            name = if without.starts_with('_') && without.chars().count() > 1 { without[1..].to_owned() } else { without };
            if name.chars().count() == 1 {
                return name;
            }
            break;
        }
    }
    let lower = name.to_lowercase();
    for (suffix, singular) in IRREG_NOUNS {
        if lower.ends_with(&suffix.to_lowercase()) {
            return format!("{}{singular}", &name[..name.len() - suffix.len()]);
        }
    }
    if NO_BASE_TYPES.iter().any(|v| v.eq_ignore_ascii_case(&name)) {
        return ELEMENT.to_owned();
    }
    if IRREG_ENDINGS.iter().any(|s| lower.ends_with(s)) {
        return ELEMENT.to_owned();
    }
    if name.chars().count() > 2 && name.ends_with('s') {
        return name[..name.len() - 1].to_owned();
    }
    ELEMENT.to_owned()
}

/// `StubUtility.getBaseNameFromLocationInParent(expression)`.
fn base_name_from_location_in_parent(expression: Node<'_>) -> Option<String> {
    if !expression.location_is("arguments") {
        return None;
    }
    let parent = expression.parent()?;
    if !matches!(
        parent.kind(),
        NodeKind::MethodInvocation
            | NodeKind::ClassInstanceCreation
            | NodeKind::SuperMethodInvocation
            | NodeKind::ConstructorInvocation
            | NodeKind::SuperConstructorInvocation
    ) {
        return None;
    }
    let binding = parent.method_binding()?;
    let arguments = parent.list("arguments");
    let params = binding.parameter_types();
    if params.len() != arguments.len() {
        return None;
    }
    let index = arguments.iter().position(|a| *a == expression)?;
    let declaration = binding.method_declaration().unwrap_or(binding);
    declaration.data().parameter_names.get(index).cloned()
}

/// `JavaContextCore.getStart()` with a zero completion length.
fn java_context_start(doc: &Doc, offset: usize) -> usize {
    let mut start = offset;
    while start != 0 && super::replacement::is_unicode_identifier_part(doc.char_at(start - 1)) {
        start -= 1;
    }
    let end = offset;
    while start != end && doc.char_at(start).is_whitespace() {
        start += 1;
    }
    if start == end {
        start = offset;
    }
    start
}

/// `PostfixCompletionProposalComputer.computeCompletionEngine` and the
/// `JavaPostfixContext` it creates; `None` when there is no engine.
pub fn analyze(ast: &Ast, context: &Context, doc: &Doc, templates: &[Template]) -> Option<Analysis> {
    if !context.extended {
        return None;
    }
    let token_length = context.token.as_deref().map(utf16_len).unwrap_or(0) as i64;
    let completion_offset = context.offset.max(0) as usize;
    let inv_offset = context.offset as i64 - token_length - 1;
    let member = enclosing_member(ast, completion_offset)?;

    let mut best = member;
    visit_preorder(member, |n| {
        let start = n.start() as i64;
        match n.kind() {
            NodeKind::StringLiteral | NodeKind::ExpressionStatement | NodeKind::SimpleName | NodeKind::QualifiedName | NodeKind::BooleanLiteral => {
                if inv_offset > start && start >= best.start() as i64 {
                    best = n;
                }
                true
            }
            NodeKind::Javadoc => {
                if inv_offset > start && start >= best.start() as i64 {
                    best = n;
                }
                false
            }
            NodeKind::MethodInvocation | NodeKind::SuperMethodInvocation | NodeKind::ClassInstanceCreation => {
                if n.flags() & RECOVERED == 0 {
                    let end = start + n.length() as i64 - 1;
                    if inv_offset > start && inv_offset == end + 1 {
                        best = n;
                        return false;
                    }
                }
                true
            }
            _ => true,
        }
    });
    let current = best;
    let parent = find_best_matching_parent_node(current);

    // JavaPostfixContext
    let prefix_key = if context.token_end >= context.token_start && context.token_start >= 0 {
        doc.get(context.token_start as usize, (context.token_end - context.token_start + 1) as usize)
    } else {
        String::new()
    };
    let region_length = |n: Node<'_>| completion_offset as i64 - utf16_len(&prefix_key) as i64 - node_begin(n) - 1;
    // findBestASTNodeSelection
    let mut selected = current;
    let mut curr_max = node_begin(current) + current.length() as i64;
    for n in [Some(current), parent].into_iter().flatten() {
        let end = node_begin(n) + n.length() as i64;
        if end > curr_max && end <= inv_offset {
            curr_max = end;
            selected = n;
        }
    }
    let affected_length = region_length(selected).max(0) as usize;
    let affected_offset = (completion_offset as i64 - utf16_len(&prefix_key) as i64 - affected_length as i64 - 1).max(0) as usize;
    let affected_statement = doc.get(affected_offset, affected_length);
    let start = (java_context_start(doc, completion_offset) as i64 - (affected_length as i64 + 1)).max(0) as usize;

    // canEvaluate
    let inner_expression = regex::Regex::new(r"\$\{([a-zA-Z]*):inner_expression\(([^\$|\{|\}]*)\)\}").unwrap();
    let skip_name = selected.is(NodeKind::SimpleName)
        && selected.binding().is_some_and(|b| !b.is_variable() && !b.is_recovered());
    let can_evaluate = templates
        .iter()
        .map(|t| {
            if t.context_type != ID_ALL || selected.is(NodeKind::Javadoc) {
                return false;
            }
            if !t.name.to_lowercase().starts_with(&prefix_key.to_lowercase()) {
                return false;
            }
            if skip_name {
                return false;
            }
            let mut result = true;
            for caps in inner_expression.captures_iter(&t.pattern) {
                for s in caps[2].split(',') {
                    if !INNER_EXPRESSION_FLAGS.contains(&s) {
                        result = false;
                        if is_node_resolving_to(selected, s.trim()) {
                            return true;
                        }
                    }
                }
            }
            result
        })
        .collect();

    let mut names = NameSources { is_expression: selected.kind().is_expression(), ..Default::default() };
    if names.is_expression {
        names.from_expression = base_name_from_expression(selected);
        names.from_parent = base_name_from_location_in_parent(selected);
        let tb = crate::correction::type_mismatch::bindings::normalize_type_binding(resolve_node_to_binding(selected));
        if let Some(mut t) = tb {
            let mut dim = 0;
            if t.is_array() {
                dim = t.dimensions().max(0) as usize;
                if let Some(e) = t.element_type() {
                    t = e;
                }
            }
            if t.is_parameterized_type() {
                if let Some(d) = t.type_declaration() {
                    t = d;
                }
            }
            let name = t.name().to_owned();
            if !name.is_empty() {
                names.from_type = Some((name, dim));
            }
        }
    }
    Some(Analysis {
        start,
        end: completion_offset,
        affected_statement,
        inner_type: resolve_node_to_type_string(selected),
        names,
        can_evaluate,
    })
}

// ─── PostfixTemplateEngine.complete ──────────────────────────────────────────

fn to_text_edit(doc: &Doc, edit: &RawEdit) -> TextEdit {
    TextEdit::new(doc.range(edit.0, edit.1), edit.2.clone())
}

/// `PostfixTemplateEngine.setAdditionalTextEdit`.
pub fn additional_text_edits(doc: &Doc, range: Range, import_edits: &[RawEdit]) -> Vec<TextEdit> {
    let mut edits = vec![TextEdit::new(range, String::new())];
    edits.extend(import_edits.iter().map(|e| to_text_edit(doc, e)));
    edits
}

/// `PostfixTemplateEngine.complete`: the items and the proposals to store
/// (indexed like the items' `pid`); `None` when an evaluation fails the way
/// upstream's does (the whole postfix list is dropped).
pub fn complete(
    doc: &Doc,
    context: &PostfixContext,
    available: &[Template],
    client: &Client,
    prefs: &Prefs,
    defaults: &ItemDefaults,
    request_id: u64,
) -> Option<(Vec<Item>, Vec<PostfixProposal>)> {
    let range = doc.range(context.start, context.end.saturating_sub(context.start));
    let mut items = Vec::new();
    let mut proposals = Vec::new();
    for (i, template) in available.iter().enumerate() {
        let mut item = Item { label: template.name.clone(), kind: Some(item_kind::SNIPPET), ..Default::default() };
        set_insert_text_format(&mut item, client, defaults);
        set_insert_text_mode(&mut item, client, defaults);
        let content = if prefs.lazy_resolve_text_edit {
            Some(template_to_snippet(&template.pattern))
        } else {
            context.evaluate(template).content
        };
        if client.item_defaults_support() && defaults.edit_range.is_some() {
            item.text_edit_text = content.clone();
        } else {
            item.insert_text = content.clone();
        }
        if !client.resolve_additional_text_edits() {
            item.additional_text_edits = Some(additional_text_edits(doc, range, &context.convert_additional_text_edits(&template.name)));
        }
        if client.label_details {
            item.label_details = Some(LabelDetails { detail: None, description: Some(template.description.clone()) });
        }
        if !client.resolve_supports("detail") {
            item.detail = Some(template.description.clone());
        }
        if !client.resolve_documentation() {
            // SnippetUtils.beautifyDocument(null) fails the whole computation
            item.documentation = Some(beautify_document(content.as_deref()?, client.documentation_markdown));
        }
        // we hope postfix shows at the bottom of the completion list.
        item.sort_text = Some(convert_relevance(0));
        item.data = Some(serde_json::json!({ "rid": request_id.to_string(), "pid": i.to_string() }));
        proposals.push(PostfixProposal { template: template.clone(), context: context.clone() });
        items.push(item);
    }
    Some((items, proposals))
}

/// Builds the [`PostfixContext`] of an [`Analysis`].
pub fn context_of(analysis: &Analysis, unit: &UnitInfo, prefs: &Prefs, import_context: ImportContext, container_types: ContainerTypes) -> PostfixContext {
    PostfixContext {
        start: analysis.start,
        end: analysis.end,
        affected_statement: analysis.affected_statement.clone(),
        inner_type: analysis.inner_type.clone(),
        names: analysis.names.clone(),
        local_names: None,
        imports: ImportEnv {
            cu: unit.cu.clone(),
            context: import_context,
            container_types,
            import_order: prefs.import_order.clone(),
            on_demand_threshold: prefs.on_demand_threshold,
            static_on_demand_threshold: prefs.static_on_demand_threshold,
            line_delimiter: unit.line_delimiter(),
            blank_lines_between_import_groups: unit.blank_lines_between_import_groups(),
            space_before_semicolon: unit.space_before_semicolon(),
        },
        recorded_edits: Arc::default(),
    }
}

/// Whether evaluating `template` asks for variable names (`computeExcludes`).
pub fn needs_local_names(template: &Template) -> bool {
    template.pattern.contains("newName") || template.pattern.contains("${index}")
}

/// `CompilationUnitCompletion.getLocalVariableNames()` of a code completion
/// at the template start.
pub async fn local_variable_names(env: &Env, ctx: &RequestContext, unit: &UnitInfo, start: usize) -> Vec<String> {
    handler::template_scope_at(env, ctx, unit, start, start)
        .await
        .map(|scope| scope.locals.into_iter().map(|v| v.name).collect())
        .unwrap_or_default()
}

/// `SnippetCompletionProposal.getPostfixSnippets`: the postfix items and the
/// `CompletionResponse` that `PostfixTemplateEngine.complete` stores.
pub async fn postfix_snippets(
    env: &Env,
    ctx: &RequestContext,
    unit: &UnitInfo,
    context: &Context,
    client: &Client,
    prefs: &Prefs,
    defaults: &ItemDefaults,
) -> Option<(Vec<Item>, handler::Response)> {
    if !prefs.postfix || !can_resolve_postfix(context, &unit.doc) {
        return None;
    }
    // PostfixCompletionProposalComputer.computeCompletionEngine
    let ast = crate::semantic_ast::fetch_with(&env.dispatcher, unit.uri.as_str(), ctx.clone()).await.ok()?;
    let all = template_store::templates_of(ID_ALL);
    let analysis = analyze(&ast, context, &unit.doc, &all)?;
    let available: Vec<Template> = all.into_iter().zip(&analysis.can_evaluate).filter(|(_, ok)| **ok).map(|(t, _)| t).collect();
    let container_types = handler::container_types(env, ctx, unit).await;
    let mut import_context = ImportContext::collect(ast.root(), analysis.end);
    import_context.package_types = container_types.get(&unit.cu.package_name).cloned();
    let mut postfix_context = context_of(&analysis, unit, prefs, import_context, container_types.clone());
    if !prefs.lazy_resolve_text_edit && available.iter().any(needs_local_names) {
        postfix_context.local_names = Some(local_variable_names(env, ctx, unit, postfix_context.start).await);
    }
    let request_id = handler::next_id();
    let (items, proposals) = complete(&unit.doc, &postfix_context, &available, client, prefs, defaults, request_id)?;
    let response = handler::Response {
        id: request_id,
        uri: unit.uri.to_string(),
        offset: context.offset.max(0) as usize,
        context: context.clone(),
        proposals: proposals.into_iter().map(handler::StoredProposal::Postfix).collect(),
        visible_elements: Default::default(),
        stubs: Default::default(),
        container_types,
        source_level: unit.compiler_source(),
        template_scope: None,
        items: Vec::new(),
        completion_item_data: Vec::new(),
        common_data: Default::default(),
    };
    Some((items, response))
}
