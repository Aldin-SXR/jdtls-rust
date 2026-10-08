//! Port of jdt.ls `CompletionHandler.completion` / `computeContentAssist`.

use super::description::DescriptionProvider;
use super::doc::Doc;
use super::imports::{ContainerTypes, CuStructure};
use super::item::{item_kind, EditRange, Item, ItemDefaults, List};
use super::prefs::{Client, GuessMode, Prefs};
use super::proposal::{kind, Context, EngineResult, Proposal};
use super::replacement::{override_key, ReplacementProvider, Stubs};
use super::requestor::{initialize_item_defaults, to_completion_item, Collector, TypeFilter};
use super::snippets::{SnippetProposal, TemplateScope};
use super::Env;
use crate::analysis::dispatcher::RequestContext;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tower_lsp::lsp_types::{Command, InsertTextMode, Position, Url};
use tracing::warn;

/// A proposal kept for `completionItem/resolve`.
#[derive(Debug, Clone)]
pub enum StoredProposal {
    Jdt(Proposal),
    Snippet(SnippetProposal),
    Postfix(super::postfix::PostfixProposal),
}

/// jdt.ls `CompletionResponse`.
#[derive(Debug, Clone)]
pub struct Response {
    pub id: u64,
    pub uri: String,
    pub offset: usize,
    pub context: Context,
    pub proposals: Vec<StoredProposal>,
    pub visible_elements: BTreeMap<String, Vec<super::proposal::VisibleElement>>,
    pub stubs: Stubs,
    pub container_types: ContainerTypes,
    pub source_level: String,
    pub template_scope: Option<TemplateScope>,
}

/// `CompletionHandler.selectedProposal`, consumed by signature-help selection.
static SELECTED_PROPOSAL: Mutex<Option<Proposal>> = Mutex::new(None);

pub fn clear_selected_proposal() {
    *SELECTED_PROPOSAL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Identity in the binding data returned by SignatureHelpService.
pub fn selected_signature_key() -> Option<String> {
    let selected = SELECTED_PROPOSAL.lock().unwrap_or_else(|e| e.into_inner());
    let proposal = selected.as_ref()?;
    let signature = proposal.signature.as_deref()?;
    let params = super::signature::get_parameter_types(signature).ok()?;
    let mut key = String::from("(");
    for param in params {
        key.push_str(&super::signature::to_string(&param).ok()?);
        key.push(';');
    }
    key.push(')');
    if proposal.kind == kind::CONSTRUCTOR_INVOCATION {
        key.push('V');
    } else {
        key.push_str(&super::signature::to_string(&super::signature::get_return_type(signature).ok()?).ok()?);
    }
    Some(key)
}

/// `CompletionHandler.onDidCompletionItemSelect`.
pub async fn on_did_select(env: &Env, request_id: &str, proposal_id: &str) -> tower_lsp::jsonrpc::Result<()> {
    let prefs = Prefs::load();
    let client = Client::load();
    if prefs.signature_help && !client.completion_item_command.is_empty() && client.execute_client_command {
        let _ = env.client.send_request::<crate::features::formatting::ExecuteClientCommand>(
            tower_lsp::lsp_types::ExecuteCommandParams {
                command: client.completion_item_command,
                arguments: Vec::new(), work_done_progress_params: Default::default(),
            }).await;
    }
    if request_id.is_empty() || proposal_id.is_empty() { return Ok(()) }
    let invalid = || super::protocol_error("Cannot get completion responses.");
    let request_id = request_id.parse::<u64>().map_err(|_| invalid())?;
    let proposal_id = proposal_id.parse::<usize>().map_err(|_| invalid())?;
    let response = get(request_id).ok_or_else(invalid)?;
    let proposal = response.proposals.get(proposal_id).ok_or_else(invalid)?;
    match proposal {
        StoredProposal::Jdt(proposal) if matches!(proposal.kind,
            kind::METHOD_REF | kind::CONSTRUCTOR_INVOCATION | kind::METHOD_REF_WITH_CASTED_RECEIVER) => {
            *SELECTED_PROPOSAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(proposal.clone());
        }
        _ => {},
    }
    Ok(())
}

static ID_SEED: AtomicU64 = AtomicU64::new(0);
static RESPONSES: Mutex<Vec<Response>> = Mutex::new(Vec::new());
/// `Preferences.DISCOVERED_STATIC_IMPORTS`.
static DISCOVERED_STATIC_IMPORTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn next_id() -> u64 {
    ID_SEED.fetch_add(1, Ordering::SeqCst)
}

pub fn store(r: Response) {
    RESPONSES.lock().unwrap_or_else(|e| e.into_inner()).push(r);
}

pub fn get(id: u64) -> Option<Response> {
    RESPONSES.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|r| r.id == id).cloned()
}

fn clear() {
    RESPONSES.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Clears `DISCOVERED_STATIC_IMPORTS`.
pub fn clear_discovered_static_imports() {
    DISCOVERED_STATIC_IMPORTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The `java.completion.favoriteStaticMembers` plus discovered static imports.
fn favorites(prefs: &Prefs) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in &prefs.favorite_members {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    for f in DISCOVERED_STATIC_IMPORTS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// `CompletionProposalUtils.addStaticImportsAsFavoriteImports`.
fn add_static_imports_as_favorites(text: &str) {
    let mut set = DISCOVERED_STATIC_IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
    for name in static_import_names(text) {
        let fav = match name.rfind('.') {
            Some(i) => format!("{}.*", &name[..i]),
            None => name,
        };
        if !set.contains(&fav) {
            set.push(fav);
        }
    }
}

/// Element names of the unit's import declarations: (static, name).
pub fn import_element_names(text: &str) -> Vec<(bool, String)> {
    let toks = crate::features::scanner::scan(text);
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth = 0;
    while i < toks.len() {
        let t = toks[i];
        let s = t.text(text);
        if t.is_comment() {
            i += 1;
            continue;
        }
        if s == "{" {
            depth += 1;
        } else if s == "}" {
            depth -= 1;
        }
        if depth == 0 && s == "import" {
            let mut j = i + 1;
            let mut is_static = false;
            let mut name = String::new();
            while j < toks.len() && toks[j].text(text) != ";" {
                let w = toks[j].text(text);
                if toks[j].is_comment() {
                } else if w == "static" && name.is_empty() {
                    is_static = true;
                } else if matches!(w, "class" | "interface" | "enum" | "record" | "public" | "import" | "@") {
                    break;
                } else {
                    name.push_str(w);
                }
                j += 1;
            }
            if !name.is_empty() {
                out.push((is_static, name));
            }
            i = j;
        }
        i += 1;
    }
    out
}

fn static_import_names(text: &str) -> Vec<String> {
    import_element_names(text).into_iter().filter(|(s, _)| *s).map(|(_, n)| n).collect()
}

/// URIs of `ctx.files` that live in test source folders.
pub fn test_uris(env: &Env, ctx: &RequestContext) -> Vec<String> {
    let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
    ctx.files
        .keys()
        .filter(|u| {
            Url::parse(u).ok().is_some_and(|url| {
                crate::project::uri_to_path(&url).is_some_and(|p| {
                    ws.project_for_path(&p).and_then(|proj| proj.source_folder_for(&p)).is_some_and(|sf| sf.is_test)
                })
            })
        })
        .cloned()
        .collect()
}

/// Everything the conversion needs about the unit and its project.
pub struct UnitInfo {
    pub uri: Url,
    pub text: String,
    pub doc: Doc,
    pub cu: Arc<CuStructure>,
    pub options: BTreeMap<String, String>,
    pub source_level: String,
    pub file_name: String,
    pub unit_name: String,
    /// `cu.getParent().getElementName()`: the package of the unit's folder.
    pub folder_package: String,
    pub has_package_declaration: bool,
    /// `cu.getAllTypes()` names (top-level and member types).
    pub all_type_names: Vec<String>,
}

fn collect_types(t: &crate::features::java_model::TypeDecl, out: &mut Vec<String>) {
    if t.anonymous {
        return;
    }
    out.push(t.name.clone());
    for m in &t.members {
        if let crate::features::java_model::Member::Type(mt) = m {
            collect_types(mt, out);
        }
    }
}

/// The package fragment of a project unit (`None` outside source folders).
pub fn unit_package(env: &Env, uri: &Url) -> Option<String> {
    folder_package(env, uri)
}

/// Package implied by the unit's location in a source folder.
fn folder_package(env: &Env, uri: &Url) -> Option<String> {
    let path = crate::project::uri_to_path(uri)?;
    let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
    let project = ws.project_for_path(&path)?;
    let sf = project.source_folder_for(&path)?;
    let rel = path.parent()?.strip_prefix(&sf.path).ok()?;
    Some(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("."))
}

impl UnitInfo {
    pub fn line_delimiter(&self) -> String {
        // ICompilationUnit.findRecommendedLineSeparator: the first delimiter in the buffer.
        self.doc.default_line_delimiter()
    }
    pub fn blank_lines_between_import_groups(&self) -> usize {
        self.options
            .get("org.eclipse.jdt.core.formatter.blank_lines_between_import_groups")
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|n| *n >= 0)
            .unwrap_or(1) as usize
    }
    pub fn space_before_semicolon(&self) -> bool {
        self.options.get("org.eclipse.jdt.core.formatter.insert_space_before_semicolon").map(String::as_str) == Some("insert")
    }
    pub fn compiler_source(&self) -> String {
        self.options.get("org.eclipse.jdt.core.compiler.source").cloned().unwrap_or_else(|| self.source_level.clone())
    }
}

pub async fn unit_info(env: &Env, uri: &Url) -> Option<(UnitInfo, RequestContext)> {
    let text = crate::features::source_text(&env.store, uri)?;
    let mut ctx = env.dispatcher.context_for(Some(uri)).await;
    ctx.files.insert(uri.to_string(), text.clone());
    let doc = Doc::new(&text);
    let mut structure = CuStructure::parse(&text);
    let has_package_declaration = structure.has_package();
    let folder_package = folder_package(env, uri).unwrap_or_else(|| structure.package_name.clone());
    structure.package_name = folder_package.clone();
    let cu = Arc::new(structure);
    let model = crate::features::java_model::parse(&text);
    let mut all_type_names = Vec::new();
    for t in &model.types {
        collect_types(t, &mut all_type_names);
    }
    let file_name = uri.path().rsplit('/').next().unwrap_or("").to_owned();
    let file_name = crate::classfile::percent_decode(&file_name);
    let unit_name = file_name.strip_suffix(".java").unwrap_or(&file_name).to_owned();
    let mut options = ctx.options.clone();
    let level = crate::project::normalize_java_version(&ctx.source_level).unwrap_or(ctx.source_level.clone());
    options.entry("org.eclipse.jdt.core.compiler.source".into()).or_insert(level.clone());
    options.entry("org.eclipse.jdt.core.compiler.compliance".into()).or_insert(level.clone());
    Some((
        UnitInfo {
            uri: uri.clone(),
            text,
            doc,
            cu,
            options,
            source_level: level,
            file_name,
            unit_name,
            folder_package,
            has_package_declaration,
            all_type_names,
        },
        ctx,
    ))
}

async fn bridge(env: &Env, ctx: &RequestContext, uri: &Url, offset: usize, query: Value) -> Option<Value> {
    let mut q = query;
    q["uriOffset"] = json!(offset);
    match env.dispatcher.code_assist(ctx, uri.as_str(), offset, q).await {
        Ok(v) => Some(v),
        Err(e) => {
            warn!("code assist failed: {e}");
            None
        }
    }
}

/// `isCompletionForConstructor`: `new |` outside string literals and names.
fn is_completion_for_constructor(text: &str, doc: &Doc, offset: usize) -> bool {
    if offset < 4 {
        return false;
    }
    if doc.get(offset - 4, 4) != "new " {
        return false;
    }
    // NodeFinder.perform(root, offset - 4, 0): a StringLiteral or SimpleName covering the 'n'.
    let byte_of = |u16off: usize| -> usize {
        let mut u = 0;
        for (b, c) in text.char_indices() {
            if u >= u16off {
                return b;
            }
            u += c.len_utf16();
        }
        text.len()
    };
    let pos = byte_of(offset - 4);
    for t in crate::features::scanner::scan(text) {
        if t.start <= pos && pos < t.end {
            return !matches!(t.kind, crate::features::scanner::TokKind::StringLit | crate::features::scanner::TokKind::TextBlock | crate::features::scanner::TokKind::Ident);
        }
        if t.start <= pos && pos <= t.end && t.kind == crate::features::scanner::TokKind::Ident {
            return false;
        }
    }
    true
}

/// `textDocument/completion`.
pub async fn completion(env: &Env, uri: &Url, position: Position, trigger_char: Option<&str>, trigger_kind: Option<i64>) -> List {
    clear();
    let prefs = Prefs::load();
    let client = Client::load();
    let Some((unit, ctx)) = unit_info(env, uri).await else { return List::default() };
    let offset = unit.doc.offset(position);
    let mut completion_for_constructor = false;
    if trigger_char == Some(" ") {
        completion_for_constructor = is_completion_for_constructor(&unit.text, &unit.doc, offset);
        if !completion_for_constructor {
            return List::default();
        }
    }
    add_static_imports_as_favorites(&unit.text);
    let imports: Vec<String> = import_element_names(&unit.text).into_iter().map(|(_, n)| n).collect();
    let filter = TypeFilter::new(&prefs.filtered_types, &imports);
    let favorites = favorites(&prefs);
    let tests = test_uris(env, &ctx);
    let query = json!({
        "op": "complete",
        "testUris": tests,
        "favorites": favorites,
        "typeFilters": filter.patterns,
        "visibleElements": prefs.guess_mode == GuessMode::InsertBestGuessedArguments,
        "unitPackage": unit_package(env, uri),
    });
    let Some(raw) = bridge(env, &ctx, uri, offset, query).await else { return List::default() };
    let result: EngineResult = serde_json::from_value(raw).unwrap_or_default();
    let context = result.context.clone().unwrap_or_default();
    let request_id = next_id();
    let mut collector = Collector {
        prefs: &prefs,
        client: &client,
        context: &context,
        filter: &filter,
        package_name: &unit.cu.package_name,
        proposals: Vec::new(),
        collapsed: HashMap::new(),
        completion_kinds: Default::default(),
        is_complete: true,
    };
    for p in result.proposals.clone() {
        collector.accept(p);
    }
    // chain completions are added into collector while computing, so we need me compute before adding completion items to proposals.
    if prefs.chain && trigger_kind != Some(2) {
        let accepted = collector.proposals.clone();
        let chains = super::chain::compute(
            env, &ctx, &unit, offset, &context, &accepted, &tests, unit_package(env, uri), client.snippets,
        )
        .await;
        for p in chains {
            collector.accept(p);
        }
    }
    let kept = collector.sorted_limited();
    let is_complete = collector.is_complete;
    let collapsed = collector.collapsed.clone();
    let completion_kinds: Vec<i32> = result.completion_kinds.clone();

    let stubs = compute_stubs(env, &ctx, &unit, &kept, &client, &prefs).await;
    let container_types = container_types(env, &ctx, &unit).await;

    let mut items: Vec<Item> = Vec::new();
    let mut defaults = ItemDefaults::default();
    {
        let provider = ReplacementProvider {
            doc: &unit.doc,
            cu: unit.cu.clone(),
            context: &context,
            offset,
            prefs: &prefs,
            client: &client,
            resolving: false,
            source_level: &unit.compiler_source(),
            container_types: &container_types,
            visible_elements: &result.visible_elements,
            stubs: &stubs,
            line_delimiter: unit.line_delimiter(),
            blank_lines_between_import_groups: unit.blank_lines_between_import_groups(),
            space_before_semicolon: unit.space_before_semicolon(),
            is_package_info: unit.file_name == "package-info.java",
            main_type_name: unit.unit_name.clone(),
            context_types: None,
        };
        let description = DescriptionProvider {
            context: Some(&context),
            doc: Some(&unit.doc),
            collapsed: Some(&collapsed),
            label_details_support: client.label_details,
            use_required_type_for_constructors: prefs.guess_mode == GuessMode::Off || prefs.collapse,
        };
        if let Some(first) = kept.first() {
            initialize_item_defaults(first, &provider, &client, &mut defaults);
        }
        for (i, p) in kept.iter().enumerate() {
            items.push(to_completion_item(p, i, request_id, &description, &provider, &client, &defaults));
        }
    }
    // see https://github.com/eclipse/eclipse.jdt.ls/issues/2669
    if let Some(er) = &defaults.edit_range {
        let r = match er {
            EditRange::Range(r) => *r,
            EditRange::InsertReplace { insert, .. } => *insert,
        };
        let line = unit.doc.position(offset).line;
        if r.start.line != line {
            defaults.edit_range = None;
            defaults.insert_text_format = None;
        }
    }
    if !completion_kinds.is_empty() {
        defaults.data = Some(json!({ "completionKinds": completion_kinds }));
    }
    store(Response {
        id: request_id,
        uri: uri.to_string(),
        offset,
        context: context.clone(),
        proposals: kept.iter().cloned().map(StoredProposal::Jdt).collect(),
        visible_elements: result.visible_elements.clone(),
        stubs: stubs.clone(),
        container_types: container_types.clone(),
        source_level: unit.compiler_source(),
        template_scope: None,
    });

    let unsupported = matches!(unit.file_name.as_str(), "module-info.java" | "package-info.java");
    if client.snippets && !unsupported {
        let scope = if !prefs.lazy_resolve_text_edit && context.token_location & (super::proposal::tl::STATEMENT_START | super::proposal::tl::MEMBER_START) != 0 {
            template_scope(env, &ctx, &unit, &context).await
        } else {
            None
        };
        let snippet_id = next_id();
        let (snippet_items, snippet_props) =
            super::snippets::generic_snippets(&unit.doc, &context, &client, &defaults, prefs.lazy_resolve_text_edit, snippet_id, scope.as_ref());
        store(Response {
            id: snippet_id,
            uri: uri.to_string(),
            offset: context.offset.max(0) as usize,
            context: context.clone(),
            proposals: snippet_props.into_iter().map(StoredProposal::Snippet).collect(),
            visible_elements: BTreeMap::new(),
            stubs: Stubs::default(),
            container_types: ContainerTypes::new(),
            source_level: unit.compiler_source(),
            template_scope: scope,
        });
        items.extend(snippet_items);
        items.extend(super::javadoc_proposal::type_definition_snippets(&unit, &context, &client, &defaults));
        if let Some((postfix_items, response)) = super::postfix::postfix_snippets(env, &ctx, &unit, &context, &client, &prefs, &defaults).await {
            store(response);
            items.extend(postfix_items);
        }
    }
    items.extend(super::javadoc_proposal::javadoc_proposals(env, &ctx, &unit, offset, &context, &client, &defaults).await);

    // When a snippet has the same label as a keyword, raise all snippets above the keyword.
    {
        let mut idx: Vec<usize> = (0..items.len())
            .filter(|&i| matches!(items[i].kind, Some(item_kind::KEYWORD) | Some(item_kind::SNIPPET)))
            .collect();
        idx.sort_by(|&a, &b| items[a].label.cmp(&items[b].label));
        let mut new_sort: i64 = super::sort_text::CEILING;
        for w in 0..idx.len().saturating_sub(1) {
            let cur = &items[idx[w]];
            let next = &items[idx[w + 1]];
            if cur.label == next.label {
                let s = if cur.kind == Some(item_kind::KEYWORD) {
                    cur.sort_text.as_deref().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) - 1
                } else {
                    next.sort_text.as_deref().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) - 1
                };
                if s < new_sort {
                    new_sort = s;
                }
            }
        }
        if new_sort != -1 {
            let s = new_sort.to_string();
            for &i in &idx {
                if items[i].kind == Some(item_kind::SNIPPET) {
                    items[i].sort_text = Some(s.clone());
                }
            }
        }
    }
    let default_sort = super::sort_text::MAX_RELEVANCE_VALUE.to_string();
    items.sort_by(|a, b| a.sort_text.as_deref().unwrap_or(&default_sort).cmp(b.sort_text.as_deref().unwrap_or(&default_sort)));

    // CompletionHandler.completion: onDidSelect command for items with resolve data.
    for item in &mut items {
        let rid = item.data.as_ref().and_then(|d| d.get("rid")).and_then(Value::as_str).unwrap_or("").to_owned();
        let pid = item.data.as_ref().and_then(|d| d.get("pid")).and_then(Value::as_str).unwrap_or("").to_owned();
        if rid.is_empty() || pid.is_empty() {
            continue;
        }
        item.command = Some(Command { title: String::new(), command: "java.completion.onDidSelect".into(), arguments: Some(vec![json!(rid), json!(pid)]) });
    }
    let _ = trigger_kind;
    List {
        is_incomplete: !is_complete || completion_for_constructor,
        item_defaults: if client.item_defaults_support() { Some(defaults) } else { None },
        items,
    }
}

/// Containers whose types matter for import conflict detection.
pub async fn container_types(env: &Env, ctx: &RequestContext, unit: &UnitInfo) -> ContainerTypes {
    let on_demand: Vec<String> = import_element_names(&unit.text)
        .into_iter()
        .filter_map(|(s, n)| if s { None } else { n.strip_suffix(".*").map(str::to_owned) })
        .collect();
    if on_demand.is_empty() {
        return ContainerTypes::new();
    }
    let mut containers: Vec<String> = on_demand;
    containers.push("java.lang".into());
    containers.push(unit.cu.package_name.clone());
    let q = json!({ "op": "packageTypes", "packages": containers });
    let Some(v) = bridge(env, ctx, &unit.uri, 0, q).await else { return ContainerTypes::new() };
    let mut out = ContainerTypes::new();
    if let Some(obj) = v.as_object() {
        for (k, names) in obj {
            let set: HashSet<String> = names.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            out.insert(k.clone(), set);
        }
    }
    out
}

/// Bridge-side template variable scope (`CompilationUnitCompletion`) at the template start.
pub async fn template_scope(env: &Env, ctx: &RequestContext, unit: &UnitInfo, context: &Context) -> Option<TemplateScope> {
    let offset = context.offset.max(0) as usize;
    let mut start = offset;
    while start > 0 && super::replacement::is_unicode_identifier_part(unit.doc.char_at(start - 1)) {
        start -= 1;
    }
    template_scope_at(env, ctx, unit, start, offset).await
}

/// The template variable scope of a code completion at `start`
/// (`CompilationUnitCompletion`); `offset` is the completion offset.
pub async fn template_scope_at(env: &Env, ctx: &RequestContext, unit: &UnitInfo, start: usize, offset: usize) -> Option<TemplateScope> {
    let tests = test_uris(env, ctx);
    let q = json!({ "op": "templateScope", "testUris": tests, "contextOffset": offset, "unitPackage": unit_package(env, &unit.uri) });
    let v = bridge(env, ctx, &unit.uri, start, q).await?;
    serde_json::from_value(v).ok()
}

/// Build replacements in Rust from JDT bindings and formatter results.
pub async fn compute_stubs(env: &Env, ctx: &RequestContext, unit: &UnitInfo, kept: &[Proposal], client: &Client, prefs: &Prefs) -> Stubs {
    let mut stubs = Stubs::default();
    let overrides: Vec<&Proposal> = kept.iter().filter(|p| p.kind == kind::METHOD_DECLARATION).collect();
    if !overrides.is_empty() {
        let list: Vec<Value> = overrides
            .iter()
            .map(|p| json!({ "key": override_key(p), "name": p.name(), "signature": p.signature(), "replaceStart": p.replace_start, "completion": p.completion() }))
            .collect();
        let tests = test_uris(env, ctx);
        let q = json!({ "op": "overrideBindings", "methods": list, "snippets": client.snippets, "testUris": tests, "generateComments": prefs.generate_comments });
        let options = format_options(env, &unit.uri).await;
        if let Some(v) = bridge(env, ctx, &unit.uri, 0, q).await {
            if let Some(obj) = v.as_object() {
                for (k, s) in obj {
                    if let Ok(method) = serde_json::from_value::<super::overrides::MethodData>(s.clone()) {
                        let stub = super::overrides::replacement(&method, &unit.doc.text(), client.snippets, &options);
                        stubs.overrides.insert(k.clone(), stub);
                    }
                }
            }
        }
    }
    let needs_anonymous = kept.iter().any(|p| matches!(p.kind, kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_DECLARATION));
    let accessor_props: Vec<&Proposal> = kept.iter().filter(|p| p.getter_setter.is_some()).collect();
    if needs_anonymous || !accessor_props.is_empty() {
        let options = format_options(env, &unit.uri).await;
        let delim = unit.doc.default_line_delimiter();
        if needs_anonymous {
            let body = if client.snippets { "{\n\t${0}\n}" } else { "{\n\n}" };
            let src = format!("new A() {body}");
            stubs.anonymous_body = Some(format_code(env, &src, 1, &delim, &options).await);
        }
        for p in accessor_props {
            let gs = p.getter_setter.as_ref().unwrap();
            let type_name = unit.cu.type_names.first().cloned().unwrap_or_default();
            let raw = super::accessors::stub(&gs.field, gs.is_getter, prefs.generate_comments, &type_name);
            let mut formatted = format_code(env, &raw, 4, &delim, &options).await;
            if formatted.ends_with(&delim) {
                formatted.truncate(formatted.len() - delim.len());
            }
            let key = format!("{}{}", if gs.is_getter { "get:" } else { "set:" }, gs.field.name);
            stubs.accessors.insert(key, formatted);
        }
    }
    stubs
}

pub async fn format_options(env: &Env, uri: &Url) -> BTreeMap<String, String> {
    let cfg = env.config.read().await;
    let fenv = crate::features::formatting::FormatEnv {
        dispatcher: &env.dispatcher,
        client: &env.client,
        settings: cfg.format.clone(),
        roots: cfg.root_paths.clone(),
        extended_client_capabilities: cfg.extended_client_capabilities.clone(),
    };
    fenv.jdt_options(Some(uri)).await
}

/// `CodeFormatterUtil.format(kind, source, 0, lineDelim, options)`.
pub async fn format_code(env: &Env, source: &str, kind: i32, delim: &str, options: &BTreeMap<String, String>) -> String {
    let len = source.encode_utf16().count();
    match env.dispatcher.format_source(source, kind, 0, len, delim, options.clone()).await {
        Ok(Some(edits)) => {
            let units: Vec<u16> = source.encode_utf16().collect();
            let mut out: Vec<u16> = Vec::new();
            let mut pos = 0;
            let mut sorted = edits;
            sorted.sort_by_key(|e| e.offset);
            for e in sorted {
                if e.offset > pos {
                    out.extend_from_slice(&units[pos..e.offset]);
                }
                out.extend(e.text.encode_utf16());
                pos = e.offset + e.length;
            }
            if pos < units.len() {
                out.extend_from_slice(&units[pos..]);
            }
            String::from_utf16_lossy(&out)
        }
        _ => source.to_owned(),
    }
}

#[allow(dead_code)]
fn unused(_: InsertTextMode) {}
