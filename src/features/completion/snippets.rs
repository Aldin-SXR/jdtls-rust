//! Port of jdt.ls `SnippetCompletionProposal` (code snippet templates and
//! the class/interface/record snippets), `SnippetUtils` and the parts of
//! the JFace/JDT template engine (`TemplateTranslator`, `JavaContextCore`
//! variable resolvers) the jdt.ls templates use.

use super::doc::Doc;
use super::item::{item_kind, Item, ItemDefaults, LabelDetails};
use super::naming::suggest_variable_names;
use super::prefs::Client;
use super::proposal::{tl, Context};
use super::signature as sig;
use super::sort_text::convert_relevance;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::{Documentation, InsertTextFormat, InsertTextMode, MarkupContent, MarkupKind, Range, TextEdit};

pub const ID_ALL: &str = "java";
pub const ID_MEMBERS: &str = "java-members";
pub const ID_STATEMENTS: &str = "java-statements";

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub context_type: String,
    pub pattern: String,
}

const SYSOUT_CONTENT: &str = "System.out.println($${0});";
const SYSERR_CONTENT: &str = "System.err.println($${0});";
const SYSTRACE_CONTENT: &str = "System.out.println(\"${enclosing_type}.${enclosing_method}()\");";
const FOREACH_CONTENT: &str = "for ($${1:${iterable_type}} $${2:${iterable_element}} : $${3:${iterable}}) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const FORI_CONTENT: &str = "for ($${1:int} $${2:${index}} = $${3:0}; $${2:${index}} < $${4:${array}.length}; $${2:${index}}++) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const WHILE_CONTENT: &str = "while ($${1:${condition:var(boolean)}}) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const DOWHILE_CONTENT: &str = "do {\n\t$$TM_SELECTED_TEXT$${0}\n} while ($${1:${condition:var(boolean)}});";
const IF_CONTENT: &str = "if ($${1:${condition:var(boolean)}}) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const IFELSE_CONTENT: &str = "if ($${1:${condition:var(boolean)}}) {\n\t$${2}\n} else {\n\t$${0}\n}";
const IFNULL_CONTENT: &str = "if ($${1:${name:var}} == null) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const IFNOTNULL_CONTENT: &str = "if ($${1:${name:var}} != null) {\n\t$$TM_SELECTED_TEXT$${0}\n}";
const SWITCH_CONTENT: &str = "switch ($${1:${key:var}}) {\n\tcase $${2:value}:\n\t\t$${0}\n\t\tbreak;\n\n\tdefault:\n\t\tbreak;\n}";
const TRYCATCH_CONTENT: &str = "try {\n\t$$TM_SELECTED_TEXT$${1}\n} catch ($${2:Exception} $${3:e}) {\n\t$${0}// TODO: handle exception\n}";
const TRYRESOURCES_CONTENT: &str = "try ($${1}) {\n\t$$TM_SELECTED_TEXT$${2}\n} catch ($${3:Exception} $${4:e}) {\n\t$${0}// TODO: handle exception\n}";
const MAIN_CONTENT: &str = "public static void main(String[] args) {\n\t$${0}\n}";
const CTOR_CONTENT: &str = "$${1|public,protected,private|} ${enclosing_simple_type}($${2}) {\n\t$${3:super();}$${0}\n}";
const METHOD_CONTENT: &str = "$${1|public,protected,private|}$${2| , static |}$${3:void} $${4:name}($${5}) {\n\t$${0}\n}";
const STATIC_METHOD_CONTENT: &str = "$${1|public,private|} static $${2:void} $${3:name}($${4}) {\n\t$${0}\n}";
const NEW_CONTENT: &str = "$${1:Object} $${2:foo} = new $${1}($${3});\n$${0}";
const FIELD_CONTENT: &str = "$${1|public,protected,private|} $${2:String} $${3:name};";
const INTERFACE_METHOD_SNIPPET: &str = "$${1|public,private|} $${2:void} $${3:name}($${4});";

/// `CodeSnippetTemplate.values()` in declaration order.
pub fn templates() -> Vec<Template> {
    let t = |name: &str, ctx: &str, pattern: &str, desc: &str| Template {
        name: name.to_owned(),
        description: desc.to_owned(),
        context_type: ctx.to_owned(),
        pattern: pattern.to_owned(),
    };
    vec![
        t("sysout", ID_STATEMENTS, SYSOUT_CONTENT, "print to standard out"),
        t("syserr", ID_STATEMENTS, SYSERR_CONTENT, "print to standard err"),
        t("systrace", ID_STATEMENTS, SYSTRACE_CONTENT, "print current method to standard out"),
        t("foreach", ID_STATEMENTS, FOREACH_CONTENT, "iterate over an array or Iterable"),
        t("fori", ID_STATEMENTS, FORI_CONTENT, "iterate over array"),
        t("while", ID_STATEMENTS, WHILE_CONTENT, "while statement"),
        t("dowhile", ID_STATEMENTS, DOWHILE_CONTENT, "do-while statement"),
        t("if", ID_STATEMENTS, IF_CONTENT, "if statement"),
        t("ifelse", ID_STATEMENTS, IFELSE_CONTENT, "if-else statement"),
        t("ifnull", ID_STATEMENTS, IFNULL_CONTENT, "if statement checking for null"),
        t("ifnotnull", ID_STATEMENTS, IFNOTNULL_CONTENT, "if statement checking for not null"),
        t("switch", ID_STATEMENTS, SWITCH_CONTENT, "switch statement"),
        t("try_catch", ID_STATEMENTS, TRYCATCH_CONTENT, "try/catch block"),
        t("try_resources", ID_STATEMENTS, TRYRESOURCES_CONTENT, "try/catch block with resources"),
        t("ctor", ID_MEMBERS, CTOR_CONTENT, "constructor"),
        t("method", ID_MEMBERS, METHOD_CONTENT, "method"),
        t("static_method", ID_MEMBERS, STATIC_METHOD_CONTENT, "static method"),
        t("field", ID_MEMBERS, FIELD_CONTENT, "field"),
        t("main", ID_MEMBERS, MAIN_CONTENT, "public static main method"),
        t("new", ID_ALL, NEW_CONTENT, "create new object"),
        t("sout", ID_STATEMENTS, SYSOUT_CONTENT, "print to standard out"),
        t("serr", ID_STATEMENTS, SYSERR_CONTENT, "print to standard err"),
        t("soutm", ID_STATEMENTS, SYSTRACE_CONTENT, "print current method to standard out"),
        t("iter", ID_STATEMENTS, FOREACH_CONTENT, "iterate over an array or Iterable"),
        t("psvm", ID_MEMBERS, MAIN_CONTENT, "public static main method"),
        t("System.out.println()", ID_STATEMENTS, SYSOUT_CONTENT, "print to standard out"),
        t("System.err.println()", ID_STATEMENTS, SYSERR_CONTENT, "print to standard err"),
        t("public static void main(String[] args)", ID_MEMBERS, MAIN_CONTENT, "public static main method"),
    ]
}

// ─── SnippetUtils ────────────────────────────────────────────────────────────

/// `SnippetUtils.templateToSnippet`.
pub fn template_to_snippet(pattern: &str) -> String {
    // $${1:${variable}} -> ${1:variable}
    let re = regex::Regex::new(r"\$\$\{(\d):\$\{(.*?)\}(.*?)\}").unwrap();
    let evaluated = re.replace_all(pattern, "$${$1:$2$3}").into_owned();
    evaluated.replace("$$", "$")
}

/// `SnippetUtils.beautifyDocument`.
pub fn beautify_document(raw: &str, markdown: bool) -> Documentation {
    let re1 = regex::Regex::new(r"\$\{\d\|(.*?),.*?\}").unwrap();
    let s = re1.replace_all(raw, "$1").into_owned();
    let re2 = regex::Regex::new(r"\$\{\d:?(.*?)\}").unwrap();
    let s = re2.replace_all(&s, "$1").into_owned();
    let s = s.replace("$TM_SELECTED_TEXT", "").replace("$TM_FILENAME_BASE", "");
    if markdown {
        Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: format!("```java\n{s}\n```") })
    } else {
        Documentation::String(s)
    }
}

/// `CompletionUtils.setInsertTextFormat`.
pub fn set_insert_text_format(item: &mut Item, client: &Client, defaults: &ItemDefaults) {
    let fmt = if client.snippets { InsertTextFormat::SNIPPET } else { InsertTextFormat::PLAIN_TEXT };
    if !client.item_defaults_property("insertTextFormat") || defaults.insert_text_format.is_none() || defaults.insert_text_format != Some(fmt) {
        item.insert_text_format = Some(fmt);
    }
}

/// `CompletionUtils.setInsertTextMode`.
pub fn set_insert_text_mode(item: &mut Item, client: &Client, defaults: &ItemDefaults) {
    if (!client.item_defaults_property("insertTextMode")
        || defaults.insert_text_mode.is_none()
        || defaults.insert_text_mode != Some(InsertTextMode::ADJUST_INDENTATION))
        && client.insert_text_mode_default != Some(InsertTextMode::ADJUST_INDENTATION)
    {
        item.insert_text_mode = Some(InsertTextMode::ADJUST_INDENTATION);
    }
}

// ─── Template evaluation ─────────────────────────────────────────────────────

/// A variable visible at the template location (`CompilationUnitCompletion.Variable`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopeVariable {
    pub name: String,
    pub signature: String,
    pub is_array: bool,
    pub is_iterable: bool,
    /// `getMemberTypeNames()`.
    pub member_type_names: Vec<String>,
    /// Fully qualified names of the type and all its supertypes.
    pub supertypes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateScope {
    /// Local variables in proposal order.
    pub locals: Vec<ScopeVariable>,
    pub fields: Vec<ScopeVariable>,
    pub enclosing_type: Option<String>,
    pub enclosing_method: Option<String>,
}

impl ScopeVariable {
    /// `Variable.isSubtypeOf`.
    fn is_subtype_of(&self, supertype: &str) -> bool {
        let Ok(implementor) = sig::strip_signature_to_fqn(&self.signature) else { return false };
        if implementor.is_empty() {
            return false;
        }
        let implementor_dims = sig::get_array_count(&self.signature).unwrap_or(0);
        let (supertype, super_dims) = match supertype.find("[]") {
            Some(i) => (&supertype[..i], (supertype.len() - i) / 2),
            None => (supertype, 0),
        };
        if implementor_dims > super_dims {
            return supertype == "java.lang.Object";
        } else if super_dims != implementor_dims {
            return false;
        }
        let qualified = supertype.contains('.');
        if implementor == supertype || (!qualified && sig::get_simple_name(&implementor) == supertype) {
            return true;
        }
        if qualified {
            self.supertypes.iter().any(|s| s == supertype)
        } else {
            self.supertypes.iter().any(|s| sig::get_simple_name(s) == supertype)
        }
    }
}

#[derive(Debug, Clone)]
struct Var {
    name: String,
    ty: String,
    params: Vec<String>,
    offsets: Vec<usize>,
}

/// `TemplateTranslator.translate`: the pattern text with `$$` unescaped and
/// variables (name, type, params) at their occurrence offsets.
fn translate(pattern: &str) -> (Vec<String>, Vec<Var>) {
    // segments: literal text interleaved with variable placeholders (by index)
    let mut segments: Vec<String> = Vec::new();
    let mut vars: Vec<Var> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut occurrence = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            if i + 1 < chars.len() && chars[i + 1] == '$' {
                cur.push('$');
                i += 2;
                continue;
            }
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                if let Some(end) = chars[i + 2..].iter().position(|&c| c == '}') {
                    let body: String = chars[i + 2..i + 2 + end].iter().collect();
                    let (name, rest) = match body.find(':') {
                        Some(c) => (body[..c].trim().to_owned(), body[c + 1..].trim().to_owned()),
                        None => (body.trim().to_owned(), String::new()),
                    };
                    let (ty, params) = if rest.is_empty() {
                        (name.clone(), Vec::new())
                    } else if let Some(p) = rest.find('(') {
                        let ty = rest[..p].trim().to_owned();
                        let inner = rest[p + 1..].trim_end_matches(')');
                        let params = inner.split(',').map(|s| s.trim().trim_matches('\'').to_owned()).filter(|s| !s.is_empty()).collect();
                        (ty, params)
                    } else {
                        (rest.clone(), Vec::new())
                    };
                    segments.push(std::mem::take(&mut cur));
                    match vars.iter_mut().find(|v| v.name == name) {
                        Some(v) => v.offsets.push(occurrence),
                        None => vars.push(Var { name, ty, params, offsets: vec![occurrence] }),
                    }
                    occurrence += 1;
                    i += 2 + end + 1;
                    continue;
                }
            }
        }
        cur.push(chars[i]);
        i += 1;
    }
    segments.push(cur);
    (segments, vars)
}

struct Evaluator<'a> {
    scope: &'a TemplateScope,
    used: HashSet<String>,
    values: HashMap<String, String>,
    vars: Vec<Var>,
}

impl<'a> Evaluator<'a> {
    fn arrange(&self, mut v: Vec<&'a ScopeVariable>) -> Vec<&'a ScopeVariable> {
        v.sort_by_key(|x| if self.used.contains(&x.name) { 1 } else { 0 });
        v
    }
    fn locals_rev(&self) -> Vec<&'a ScopeVariable> {
        self.scope.locals.iter().rev().collect()
    }
    fn fields_rev(&self) -> Vec<&'a ScopeVariable> {
        self.scope.fields.iter().rev().collect()
    }
    fn arrays(&self) -> Vec<&'a ScopeVariable> {
        let v: Vec<&ScopeVariable> = self.locals_rev().into_iter().chain(self.fields_rev()).filter(|x| x.is_array).collect();
        self.arrange(v)
    }
    fn iterables(&self) -> Vec<&'a ScopeVariable> {
        let v: Vec<&ScopeVariable> = self.locals_rev().into_iter().chain(self.fields_rev()).filter(|x| x.is_array || x.is_iterable).collect();
        self.arrange(v)
    }
    fn local_variables(&self, ty: &str) -> Vec<&'a ScopeVariable> {
        let v = self.locals_rev().into_iter().filter(|x| x.is_subtype_of(ty)).collect();
        self.arrange(v)
    }
    fn field_variables(&self, ty: &str) -> Vec<&'a ScopeVariable> {
        let v = self.fields_rev().into_iter().filter(|x| x.is_subtype_of(ty)).collect();
        self.arrange(v)
    }
    fn excludes(&self) -> Vec<String> {
        let mut ex: Vec<String> = self.scope.locals.iter().rev().map(|l| l.name.clone()).collect();
        ex.extend(self.used.iter().cloned());
        ex
    }
    fn suggest(&self, ty: &str) -> Vec<String> {
        let mut t = ty.to_owned();
        let mut dim = 0;
        while let Some(s) = t.strip_suffix("[]") {
            t = s.to_owned();
            dim += 1;
        }
        suggest_variable_names(&t, dim, &self.excludes(), true)
    }

    /// Resolve variable `name` (once); returns its value.
    fn resolve(&mut self, name: &str) -> Option<String> {
        if let Some(v) = self.values.get(name) {
            return Some(v.clone());
        }
        let var = self.vars.iter().find(|v| v.name == name)?.clone();
        let value = self.resolve_var(&var);
        self.values.insert(name.to_owned(), value.clone());
        Some(value)
    }

    fn resolve_var(&mut self, var: &Var) -> String {
        let name = var.name.clone();
        let first = |v: &[&ScopeVariable]| v.first().map(|x| x.name.clone());
        match var.ty.as_str() {
            "cursor" | "word_selection" | "line_selection" => String::new(),
            "dollar" => "$".to_owned(),
            "enclosing_type" | "enclosing_simple_type" => self.scope.enclosing_type.clone().unwrap_or(name),
            "enclosing_method" => self.scope.enclosing_method.clone().unwrap_or(name),
            "todo" => "TODO".to_owned(),
            "array" | "iterable" => {
                let list = if var.ty == "array" { self.arrays() } else { self.iterables() };
                match first(&list) {
                    Some(n) => {
                        self.used.insert(n.clone());
                        n
                    }
                    None => name,
                }
            }
            "array_type" | "iterable_type" => {
                let master = if var.ty == "array_type" { "array" } else { "iterable" };
                let list = if var.ty == "array_type" { self.arrays() } else { self.iterables() };
                if list.is_empty() {
                    return name;
                }
                let master_value = self.master_value(master, &list);
                list.iter()
                    .find(|v| Some(&v.name) == master_value.as_ref())
                    .or(list.first())
                    .and_then(|v| v.member_type_names.first().cloned())
                    .unwrap_or(name)
            }
            "array_element" | "iterable_element" => {
                let master = if var.ty == "array_element" { "array" } else { "iterable" };
                let list = if var.ty == "array_element" { self.arrays() } else { self.iterables() };
                if list.is_empty() {
                    return name;
                }
                let master_value = self.master_value(master, &list);
                let chosen = list.iter().find(|v| Some(&v.name) == master_value.as_ref()).or(list.first()).copied();
                let member = chosen.and_then(|v| v.member_type_names.first().cloned()).unwrap_or_default();
                let names = self.suggest(&member);
                match names.first() {
                    Some(n) => {
                        self.used.insert(n.clone());
                        n.clone()
                    }
                    None => name,
                }
            }
            "index" | "iterator" => {
                let param = var.params.first().cloned().unwrap_or_else(|| if var.ty == "index" { "int".into() } else { "java.util.Iterator".into() });
                let names = self.suggest(&param);
                match names.first() {
                    Some(n) => {
                        self.used.insert(n.clone());
                        n.clone()
                    }
                    None => name,
                }
            }
            "var" | "localVar" | "field" | "collection" => {
                let default_type = if var.ty == "collection" { "java.util.Collection" } else { "java.lang.Object" };
                let types: Vec<String> = if var.params.is_empty() { vec![default_type.to_owned()] } else { var.params.clone() };
                let mut found: Vec<&ScopeVariable> = Vec::new();
                for t in &types {
                    if var.ty != "field" {
                        found.extend(self.local_variables(t));
                    }
                    if var.ty != "localVar" {
                        found.extend(self.field_variables(t));
                    }
                }
                match first(&found) {
                    Some(n) => {
                        self.used.insert(n.clone());
                        n
                    }
                    None => name,
                }
            }
            _ => name,
        }
    }

    fn master_value(&mut self, master: &str, list: &[&ScopeVariable]) -> Option<String> {
        if self.vars.iter().any(|v| v.ty == master || v.name == master) {
            let mname = self.vars.iter().find(|v| v.name == master || v.ty == master).map(|v| v.name.clone())?;
            return self.resolve(&mname);
        }
        list.first().map(|v| v.name.clone())
    }
}

/// `JavaContextCore.evaluate(template)` → buffer string; `None` when only whitespace.
pub fn evaluate(pattern: &str, scope: &TemplateScope) -> Option<String> {
    let (segments, vars) = translate(pattern);
    let mut ev = Evaluator { scope, used: HashSet::new(), values: HashMap::new(), vars: vars.clone() };
    for v in &vars {
        ev.resolve(&v.name);
    }
    // occurrence index → variable name
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
            out.push_str(ev.values.get(name).map(String::as_str).unwrap_or(name));
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

// ─── Generic snippets ────────────────────────────────────────────────────────

/// `JavaContextCore.getKey()`: the identifier prefix before the completion offset.
fn template_key(doc: &Doc, offset: usize) -> String {
    let mut start = offset;
    while start > 0 && super::replacement::is_unicode_identifier_part(doc.char_at(start - 1)) {
        start -= 1;
    }
    doc.get(start, offset - start)
}

fn is_after_dot(doc: &Doc, offset: usize) -> bool {
    offset > 0 && doc.char_at(offset - 1) == '.'
}

fn can_evaluate(t: &Template, context_id: &str, key: &str, after_dot: bool) -> bool {
    let compatible = t.context_type == context_id || (context_id != ID_ALL && t.context_type == ID_ALL);
    if !compatible {
        return false;
    }
    // `JavaContextCore.canEvaluate` with `CODEASSIST_SUBSTRING_MATCH_ENABLED`
    // (system property `jdt.codeCompleteSubstringMatch`, default true).
    if !key.is_empty() || !after_dot {
        return t.name.to_lowercase().contains(&key.to_lowercase());
    }
    false
}

/// Stored for resolve: the template behind a snippet item.
#[derive(Debug, Clone)]
pub struct SnippetProposal {
    pub template: Template,
}

/// `getGenericSnippets`: items and the response proposals (indexed like upstream).
pub fn generic_snippets(
    doc: &Doc,
    context: &Context,
    client: &Client,
    defaults: &ItemDefaults,
    lazy_resolve: bool,
    request_id: u64,
    scope: Option<&TemplateScope>,
) -> (Vec<Item>, Vec<SnippetProposal>) {
    let token_location = context.token_location;
    let offset = context.offset.max(0) as usize;
    let token_len = context.token.as_ref().map(|t| t.encode_utf16().count()).unwrap_or(0);
    let context_id = if token_location & tl::STATEMENT_START != 0 {
        ID_STATEMENTS
    } else if token_location & tl::MEMBER_START != 0 {
        ID_MEMBERS
    } else {
        return (Vec::new(), Vec::new());
    };
    let all = templates();
    let key = if token_len == 0 { template_key(doc, offset) } else { template_key(doc, offset) };
    let after_dot = is_after_dot(doc, offset);
    let specific: Vec<Template> = all.iter().filter(|t| t.context_type == context_id).cloned().collect();
    if specific.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let available: Vec<Template> = specific.into_iter().filter(|t| can_evaluate(t, context_id, &key, after_dot)).collect();
    let available_all: Vec<Template> =
        all.iter().filter(|t| t.context_type == ID_ALL).filter(|t| can_evaluate(t, ID_ALL, &key, after_dot)).cloned().collect();
    let mut items = Vec::new();
    let mut proposals: Vec<SnippetProposal> = Vec::new();
    let enclosing_interface = context.enclosing_kind.as_deref() == Some("type") && context.enclosing_interface;
    let mut reduction = 0usize;
    let total = available.len() + available_all.len();
    for i in 0..total {
        let mut template = if i < available.len() { available[i].clone() } else { available_all[i - available.len()].clone() };
        if enclosing_interface {
            if template.name == "method" {
                template.pattern = INTERFACE_METHOD_SNIPPET.to_owned();
            } else if template.name == "ctor" {
                reduction += 1;
                continue;
            }
        } else if template.name == "static_method" {
            reduction += 1;
            continue;
        }
        let mut item = Item { label: template.name.clone(), kind: Some(item_kind::SNIPPET), ..Default::default() };
        set_insert_text_format(&mut item, client, defaults);
        set_insert_text_mode(&mut item, client, defaults);
        if lazy_resolve {
            let insert = template_to_snippet(&template.pattern);
            if client.item_defaults_support() && defaults.edit_range.is_some() {
                item.text_edit_text = Some(insert);
            } else {
                item.insert_text = Some(insert);
            }
        } else {
            let content = scope.and_then(|s| evaluate(&template.pattern, s));
            if client.item_defaults_support() && defaults.edit_range.is_some() {
                item.text_edit_text = content;
            } else {
                set_text_edit(context, doc, &mut item, content.unwrap_or_else(|| "null".into()));
            }
        }
        item.detail = Some(template.description.clone());
        if client.label_details {
            item.label_details = Some(LabelDetails { detail: None, description: Some(template.description.clone()) });
        }
        item.data = Some(serde_json::json!({ "rid": request_id.to_string(), "pid": i.to_string() }));
        let index = i - reduction;
        proposals.insert(index.min(proposals.len()), SnippetProposal { template });
        items.push(item);
    }
    (items, proposals)
}

/// `SnippetCompletionProposal.setTextEdit`.
pub fn set_text_edit(context: &Context, doc: &Doc, item: &mut Item, content: String) {
    let length = (context.token_end - context.token_start + 1).max(0) as usize;
    let range: Range = doc.range(context.token_start.max(0) as usize, length);
    item.text_edit = Some(super::item::ItemTextEdit::Edit(TextEdit::new(range, content)));
}

// ─── Type definition snippets ────────────────────────────────────────────────

pub struct TypeSnippetEnv<'a> {
    pub context: &'a Context,
    pub doc: &'a Doc,
    pub client: &'a Client,
    pub defaults: &'a ItemDefaults,
    /// Compliance of the project (`COMPILER_COMPLIANCE`).
    pub compliance: &'a str,
    /// File name without extension.
    pub unit_name: &'a str,
    /// Package of the unit's folder, and whether the unit declares one.
    pub package_name: &'a str,
    pub has_package_declaration: bool,
    /// All type names declared in the unit (`cu.getAllTypes()`).
    pub all_types: &'a [String],
    pub has_types: bool,
    pub line_delimiter: &'a str,
    /// `needsPublic(...)`, computed by the caller.
    pub needs_public: bool,
    /// `accept(cu, context, acceptClass)` results: (class, other).
    pub accept_class: bool,
    pub accept_other: bool,
    pub markdown: bool,
}

pub fn type_definition_snippets(env: &TypeSnippetEnv) -> Vec<Item> {
    let token = env.context.token.clone().unwrap_or_default();
    let (mut is_interface, mut is_class, mut is_record) = (true, true, true);
    if !token.is_empty() {
        is_interface = "interface".starts_with(&token);
        is_class = "class".starts_with(&token);
        is_record = "record".starts_with(&token);
    }
    if !is_interface && !is_class && !is_record {
        return Vec::new();
    }
    let mut res = Vec::new();
    if is_class && env.accept_class {
        let t = if env.needs_public { "${filecomment}${package_header}${typecomment}public class ${type_name} {\n\n\t${cursor}\n}" } else { "${filecomment}${package_header}class ${type_name} {\n\n\t${cursor}\n}" };
        res.push(type_snippet(env, "class", 1, t));
    }
    if is_interface && env.accept_other {
        let t = if env.needs_public { "${filecomment}${package_header}${typecomment}public interface ${type_name} {\n\n\t${cursor}\n}" } else { "${filecomment}${package_header}interface ${type_name} {\n\n\t${cursor}\n}" };
        res.push(type_snippet(env, "interface", 0, t));
    }
    if is_record && !crate::features::completion::version_less_than(env.compliance, "14") && env.accept_other {
        let t = if env.needs_public { "${filecomment}${package_header}${typecomment}public record ${type_name}(${cursor}) {\n}" } else { "${filecomment}${package_header}record ${type_name}(${cursor}) {\n}" };
        res.push(type_snippet(env, "record", 0, t));
    }
    for item in &mut res {
        item.kind = Some(item_kind::SNIPPET);
        set_insert_text_format(item, env.client, env.defaults);
        set_insert_text_mode(item, env.client, env.defaults);
        if let Some(t) = item.insert_text.clone() {
            item.documentation = Some(beautify_document(&t, env.markdown));
        }
    }
    res
}

fn type_snippet(env: &TypeSnippetEnv, label: &str, relevance: i64, pattern: &str) -> Item {
    let mut item = Item { label: label.to_owned(), filter_text: Some(label.to_owned()), sort_text: Some(convert_relevance(relevance)), ..Default::default() };
    let content = snippet_content(env, pattern);
    item.insert_text = content.clone();
    if env.client.item_defaults_support() {
        item.text_edit_text = content;
    }
    item
}

fn snippet_content(env: &TypeSnippetEnv, pattern: &str) -> Option<String> {
    let mut type_name = env.unit_name.to_owned();
    let mut postfix = 0;
    while env.all_types.iter().any(|t| *t == type_name) {
        type_name = format!("Inner{}{}", env.unit_name, if postfix == 0 { String::new() } else { format!("_{postfix}") });
        postfix += 1;
    }
    if postfix > 0 {
        type_name = format!("${{1:{type_name}}}");
    }
    let d = env.line_delimiter;
    // `CodeGeneration.getFileComment` / `getTypeComment` with jdt.ls' default
    // code templates: the file comment template is empty (whitespace only →
    // null → ""), the type comment is `/**\n * ${type_name}\n * ${tags}\n */`
    // whose `${tags}` line is removed when a type has no tags.
    let file_comment = "";
    let type_comment = format!("/**{d} * {type_name}{d} */{d}");
    let package_header = if !env.package_name.is_empty() && !env.has_package_declaration {
        format!("package {};{d}{d}", env.package_name)
    } else {
        String::new()
    };
    let out = pattern
        .replace("${filecomment}", file_comment)
        .replace("${typecomment}", &type_comment)
        .replace("${package_header}", &package_header)
        .replace("${type_name}", &type_name)
        .replace("${cursor}", "${0}");
    let _ = env.has_types;
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_snippet() {
        assert_eq!(template_to_snippet(FOREACH_CONTENT), "for (${1:iterable_type} ${2:iterable_element} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}");
        assert_eq!(template_to_snippet(SYSOUT_CONTENT), "System.out.println(${0});");
        assert_eq!(template_to_snippet(WHILE_CONTENT), "while (${1:condition:var(boolean)}) {\n\t$TM_SELECTED_TEXT${0}\n}");
        assert_eq!(template_to_snippet(FORI_CONTENT), "for (${1:int} ${2:index} = ${3:0}; ${2:index} < ${4:array.length}; ${2:index}++) {\n\t$TM_SELECTED_TEXT${0}\n}");
    }

    #[test]
    fn evaluate_templates() {
        let args = ScopeVariable {
            name: "args".into(),
            signature: "[Ljava.lang.String;".into(),
            is_array: true,
            is_iterable: false,
            member_type_names: vec!["String".into()],
            supertypes: vec![],
        };
        let scope = TemplateScope { locals: vec![args], fields: vec![], enclosing_type: Some("Test".into()), enclosing_method: Some("testMethod".into()) };
        assert_eq!(evaluate(FOREACH_CONTENT, &scope).unwrap(), "for (${1:String} ${2:string} : ${3:args}) {\n\t$TM_SELECTED_TEXT${0}\n}");
        assert_eq!(evaluate(FORI_CONTENT, &scope).unwrap(), "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:args.length}; ${2:i}++) {\n\t$TM_SELECTED_TEXT${0}\n}");
        assert_eq!(evaluate(SYSTRACE_CONTENT, &scope).unwrap(), "System.out.println(\"Test.testMethod()\");");
        let con = ScopeVariable { name: "con".into(), signature: "Z".into(), ..Default::default() };
        let scope = TemplateScope { locals: vec![con], ..Default::default() };
        assert_eq!(evaluate(WHILE_CONTENT, &scope).unwrap(), "while (${1:con}) {\n\t$TM_SELECTED_TEXT${0}\n}");
    }
}
