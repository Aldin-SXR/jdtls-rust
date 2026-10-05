//! Port of jdt.ls `CodeActionHandler.getCodeActionCommands` and
//! `CodeActionResolveHandler.resolve`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, CodeActionParams, Command, Diagnostic, DiagnosticSeverity, NumberOrString, Url};

use super::edit::{self, Env};
use super::{compare_proposals, java_sort, kind, Change, Context, ProblemLocation, Proposal, ProposalType};
use crate::analysis::semantic::diagnostics::{Doc16, DIAG_ARGUMENTS, SERVER_SOURCE_ID};

/// `BaseDiagnosticsHandler.DIAG_ECJ_PROBLEM_ID`.
const DIAG_ECJ_PROBLEM_ID: &str = "ecjProblemId";
/// `CodeActionResolveHandler.DATA_FIELD_REQUEST_ID` / `DATA_FIELD_PROPOSAL_ID`.
const DATA_FIELD_REQUEST_ID: &str = "rid";
const DATA_FIELD_PROPOSAL_ID: &str = "pid";

/// `CodeActionComparator` priorities.
pub mod priority {
    pub const ORGANIZE_IMPORTS: i32 = 0;
    pub const ADD_ALL_MISSING_IMPORTS: i32 = 5;
    pub const GENERATE_ACCESSORS: i32 = 10;
    pub const GENERATE_CONSTRUCTORS: i32 = 20;
    pub const GENERATE_HASHCODE_EQUALS: i32 = 30;
    pub const GENERATE_TOSTRING: i32 = 40;
    pub const GENERATE_OVERRIDE_IMPLEMENT: i32 = 50;
    pub const GENERATE_DELEGATE_METHOD: i32 = 60;
    pub const SORT_MEMBERS: i32 = 65;
    pub const CHANGE_MODIFIER_TO_FINAL: i32 = 70;
    pub const LOWEST: i32 = 100;
}

/// `CodeActionHandler.CodeActionData` while sorting.
#[derive(Clone, Copy, Debug)]
pub struct ActionData {
    /// Index into the response's proposals (`None`: no resolvable proposal).
    pub proposal: Option<usize>,
    pub priority: i32,
}

/// A code action (or command) under construction.
pub struct Entry {
    pub action: CodeActionOrCommand,
    pub data: Option<ActionData>,
}

impl Entry {
    pub fn command(c: Command) -> Self {
        Entry { action: CodeActionOrCommand::Command(c), data: None }
    }
}

/// `ResponseStore` of the latest code action responses.
struct Stored {
    id: u64,
    proposals: Vec<Arc<tokio::sync::Mutex<Proposal>>>,
}

static STORE: Lazy<Mutex<(u64, VecDeque<Stored>)>> = Lazy::new(|| Mutex::new((0, VecDeque::new())));
static ACTIVE_UNIT: Lazy<Mutex<Option<Url>>> = Lazy::new(|| Mutex::new(None));

/// `BaseDocumentLifeCycleHandler.handleChanged`: proposals for the active
/// AST are invalid once its working copy changes.
pub fn document_changed(uri: &Url) {
    let mut active = ACTIVE_UNIT.lock().unwrap_or_else(|e| e.into_inner());
    if active.as_ref() == Some(uri) {
        STORE.lock().unwrap_or_else(|e| e.into_inner()).1.clear();
        *active = None;
    }
}

fn store_capacity() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(8)
}

/// Source of the processors' proposals; implemented by the processor
/// modules and by tests.
pub struct Request<'a> {
    pub params: &'a CodeActionParams,
    pub context: Context,
    pub locations: Vec<ProblemLocation>,
    pub diagnostics: Vec<Diagnostic>,
    pub uri: Url,
}

/// `CodeActionHandler.getProblemLocationCores`.
pub fn problem_locations(doc: &Doc16, diagnostics: &[Diagnostic]) -> Vec<ProblemLocation> {
    diagnostics
        .iter()
        .map(|d| {
            let start = doc.to_offset(d.range.start.line, d.range.start.character);
            let end = doc.to_offset(d.range.end.line, d.range.end.character);
            let mut problem_id = problem_id(d);
            let mut arguments = Vec::new();
            if let Some(Value::Object(data)) = &d.data {
                if let Some(Value::Array(args)) = data.get(DIAG_ARGUMENTS) {
                    arguments = args.iter().map(|a| a.as_str().map(str::to_owned).unwrap_or_else(|| a.to_string())).collect();
                }
                if let Some(Value::String(id)) = data.get(DIAG_ECJ_PROBLEM_ID) {
                    if let Ok(id) = id.parse() {
                        problem_id = id;
                    }
                }
            }
            ProblemLocation {
                offset: start.max(0) as usize,
                length: (end - start).max(0) as usize,
                problem_id,
                arguments,
                is_error: d.severity == Some(DiagnosticSeverity::ERROR),
            }
        })
        .collect()
}

fn problem_id(d: &Diagnostic) -> i32 {
    match &d.code {
        Some(NumberOrString::String(s)) => s.parse().unwrap_or(0),
        Some(NumberOrString::Number(n)) => *n,
        None => 0,
    }
}

/// `containsKind(codeActionKinds, baseKind)`.
fn contains_kind(kinds: &[String], base: &str) -> bool {
    kinds.iter().any(|k| k.starts_with(base))
}

/// `CodeActionHandler.getCodeActionCommands`.
pub async fn code_actions(env: &Env<'_>, params: &CodeActionParams) -> Vec<CodeActionOrCommand> {
    let uri = params.text_document.uri.clone();
    let Ok(ast) = crate::semantic_ast::fetch(env.dispatcher, &uri).await else { return Vec::new() };
    *ACTIVE_UNIT.lock().unwrap_or_else(|e| e.into_inner()) = Some(uri.clone());
    let doc = Doc16::new(ast.text());
    let start = doc.to_offset(params.range.start.line, params.range.start.character);
    let end = doc.to_offset(params.range.end.line, params.range.end.character);
    let context = Context::new(ast.clone(), start.max(0) as usize, (end - start).max(0) as usize);
    let diagnostics: Vec<Diagnostic> =
        params.context.diagnostics.iter().filter(|d| d.source.as_deref() == Some(SERVER_SOURCE_ID)).cloned().collect();
    let locations = problem_locations(&doc, &diagnostics);
    let kinds: Vec<String> = match &params.context.only {
        Some(only) if !only.is_empty() => only.iter().map(|k| k.as_str().to_owned()).collect(),
        _ => vec![kind::QUICK_FIX.into(), kind::REFACTOR.into(), kind::QUICK_ASSIST.into(), kind::SOURCE.into()],
    };
    let req = Request { params, context, locations, diagnostics, uri: uri.clone() };

    let mut entries: Vec<Entry> = Vec::new();
    let mut proposals: Vec<Proposal> = Vec::new();
    if contains_kind(&kinds, kind::QUICK_FIX) {
        entries.extend(super::quick_fix::non_project_fixes(env, &req));
        let mut quick = super::quick_fix::corrections(env, &req).await;
        super::quick_fix::add_all_missing_imports_proposal(env, &req, &mut quick).await;
        quick.extend(ignore_compiler_problems_proposal(&req.diagnostics));
        // `TreeSet<>(comparator)`: sorted, first of equal elements kept.
        let mut set: Vec<Proposal> = Vec::new();
        for p in quick {
            match set.binary_search_by(|x| compare_proposals(x, &p)) {
                Ok(_) => {}
                Err(i) => set.insert(i, p),
            }
        }
        proposals.extend(set);
    }
    if contains_kind(&kinds, kind::REFACTOR) {
        let mut refactors = super::quick_assist::refactor_proposals(env, &req).await;
        java_sort(&mut refactors, compare_proposals);
        proposals.extend(refactors);
    }
    if contains_kind(&kinds, kind::QUICK_ASSIST) {
        let mut assists = super::quick_assist::assists(env, &req).await;
        java_sort(&mut assists, compare_proposals);
        proposals.extend(assists);
    }

    let supports_resolve = crate::features::client_caps::resolve_code_action();
    let mut stored: Vec<Proposal> = Vec::new();
    for mut p in proposals {
        if let Some(entry) = code_action_from_proposal(env, &uri, &mut p, &params.context.diagnostics, supports_resolve, stored.len()).await {
            if !entries.iter().any(|e| same_action(&e.action, &entry.action)) {
                if entry.data.is_some_and(|d| d.proposal.is_some()) {
                    stored.push(p);
                }
                entries.push(entry);
            }
        }
    }
    if contains_kind(&kinds, kind::SOURCE) {
        let source = super::source_assist::source_actions(env, &req, stored.len()).await;
        for (entry, proposal) in source {
            if let Some(p) = proposal {
                stored.push(p);
            }
            entries.push(entry);
        }
    }
    java_sort(&mut entries, compare_entries);
    populate_data_fields(entries, stored)
}

fn same_action(a: &CodeActionOrCommand, b: &CodeActionOrCommand) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// `CodeActionComparator`.
fn compare_entries(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    let (CodeActionOrCommand::CodeAction(x), CodeActionOrCommand::CodeAction(y)) = (&a.action, &b.action) else {
        return std::cmp::Ordering::Equal;
    };
    let ord = |k: &Option<tower_lsp::lsp_types::CodeActionKind>| -> i32 {
        let k = k.as_ref().map(|k| k.as_str()).unwrap_or("");
        if k == kind::QUICK_FIX {
            0
        } else if k.starts_with(kind::REFACTOR) {
            1000
        } else if k == kind::QUICK_ASSIST {
            2000
        } else if k.starts_with(kind::SOURCE) {
            3000
        } else {
            4000
        }
    };
    let diff = ord(&x.kind) - ord(&y.kind);
    if diff != 0 {
        return diff.cmp(&0);
    }
    match (a.data, b.data) {
        (Some(d1), Some(d2)) => (d1.priority - d2.priority).cmp(&0),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        _ => std::cmp::Ordering::Equal,
    }
}

/// `CodeActionHandler.populateDataFields` (+ storing the response).
fn populate_data_fields(entries: Vec<Entry>, proposals: Vec<Proposal>) -> Vec<CodeActionOrCommand> {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    guard.0 += 1;
    let rid = guard.0;
    let mut out = Vec::new();
    let mut slots: Vec<Option<Proposal>> = proposals.into_iter().map(Some).collect();
    let mut kept: Vec<Arc<tokio::sync::Mutex<Proposal>>> = Vec::new();
    for e in entries {
        match e.action {
            CodeActionOrCommand::CodeAction(mut ca) => {
                match e.data.and_then(|d| d.proposal) {
                    Some(i) if slots.get(i).is_some_and(Option::is_some) => {
                        let p = slots[i].take().unwrap();
                        ca.data = Some(json!({ DATA_FIELD_REQUEST_ID: rid.to_string(), DATA_FIELD_PROPOSAL_ID: kept.len().to_string() }));
                        kept.push(Arc::new(tokio::sync::Mutex::new(p)));
                    }
                    _ => ca.data = None,
                }
                out.push(CodeActionOrCommand::CodeAction(ca));
            }
            other => out.push(other),
        }
    }
    if !kept.is_empty() {
        guard.1.push_back(Stored { id: rid, proposals: kept });
        let cap = store_capacity();
        while guard.1.len() > cap {
            guard.1.pop_front();
        }
    }
    out
}

/// `CodeActionHandler.getCodeActionFromProposal`.  `next_proposal` is the
/// index the proposal gets in the response store when it is kept.
pub async fn code_action_from_proposal(
    env: &Env<'_>,
    uri: &Url,
    p: &mut Proposal,
    diagnostics: &[Diagnostic],
    supports_resolve: bool,
    next_proposal: usize,
) -> Option<Entry> {
    let mut command = p.command.as_ref().map(|(id, args)| Command { title: p.name.clone(), command: id.clone(), arguments: Some(args.clone()) });
    let mut edit = None;
    if command.is_none() && !supports_resolve {
        let we = match edit::to_workspace_edit(env, &mut p.change).await {
            Ok(edit) => edit,
            Err(error) => { tracing::debug!(title = %p.name, %error, "Cannot compute correction edit"); return None; }
        };
        if !edit::has_changes(&we) && p.proposal_type != ProposalType::ChangeCompliance {
            return None;
        }
        edit = Some(we);
    }
    if !crate::features::client_caps::supported_code_action_kind(&p.kind) {
        return command.map(Entry::command);
    }
    let missing_command = command.is_none();
    if missing_command && !crate::features::preferences::validate_all_open_buffers_on_changes() && p.proposal_type == ProposalType::NewElement {
        command = Some(Command {
            title: "refresh Diagnostics".into(),
            command: "java.project.refreshDiagnostics".into(),
            arguments: Some(vec![json!(uri.as_str()), json!("thisFile"), json!(false), json!(true)]),
        });
    }
    let mut ca = CodeAction { title: p.name.clone(), kind: Some(p.kind.clone().into()), ..Default::default() };
    if !supports_resolve {
        ca.edit = edit;
    }
    ca.command = command;
    let data = if supports_resolve {
        Some(ActionData { proposal: if missing_command { Some(next_proposal) } else { None }, priority: -p.relevance })
    } else {
        None
    };
    if p.kind != kind::QUICK_ASSIST {
        ca.diagnostics = Some(diagnostics.to_vec());
    }
    Some(Entry { action: CodeActionOrCommand::CodeAction(ca), data })
}

/// `CodeActionHandler.getIgnoreCompilerProblemsProposal`.
fn ignore_compiler_problems_proposal(diagnostics: &[Diagnostic]) -> Option<Proposal> {
    let mut keys: Vec<&'static str> = Vec::new();
    for d in diagnostics {
        if let Some(key) = crate::semantic_ast::irritants::option_key_for_problem(problem_id(d)) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    if keys.is_empty() {
        return None;
    }
    let label = super::messages::ls_correction("CodeActionHandler_ignore_compiler_problems");
    let settings = crate::features::preferences::get_string("java.settings.url");
    let settings_file = settings.as_deref().and_then(|s| Url::parse(s).ok()).and_then(|u| u.to_file_path().ok()).filter(|p| p.exists());
    match settings_file {
        Some(path) => {
            let content: String = keys.iter().map(|k| format!("{k}=ignore\n")).collect();
            let uri = Url::from_file_path(&path).ok()?;
            let te = tower_lsp::lsp_types::TextEdit { range: Default::default(), new_text: content };
            let we = if crate::features::client_caps::resource_operations() {
                tower_lsp::lsp_types::WorkspaceEdit {
                    document_changes: Some(tower_lsp::lsp_types::DocumentChanges::Edits(vec![tower_lsp::lsp_types::TextDocumentEdit {
                        text_document: tower_lsp::lsp_types::OptionalVersionedTextDocumentIdentifier { uri, version: None },
                        edits: vec![tower_lsp::lsp_types::OneOf::Left(te)],
                    }])),
                    ..Default::default()
                }
            } else {
                tower_lsp::lsp_types::WorkspaceEdit { changes: Some([(uri, vec![te])].into_iter().collect()), ..Default::default() }
            };
            Some(Proposal::new(label, kind::QUICK_FIX, super::relevance::ADD_SUPPRESSWARNINGS, Change::WorkspaceEdit(we)))
        }
        None => {
            // Without a settings file jdt.ls updates the project settings
            // when the change is computed; the action carries no edit, so it
            // is only offered to clients that resolve code actions.
            if !crate::features::client_caps::resolve_code_action() {
                return None;
            }
            let entries: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
            Some(Proposal::new(label, kind::QUICK_FIX, super::relevance::ADD_SUPPRESSWARNINGS, Change::Lazy(Box::new(IgnoreProblems(entries)))))
        }
    }
}

struct IgnoreProblems(Vec<String>);

#[tower_lsp::async_trait]
impl super::LazyChange for IgnoreProblems {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<super::CuChange>> {
        let roots: Vec<std::path::PathBuf> = {
            let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
            ws.projects.iter().map(|p| p.root.clone()).collect()
        };
        for root in roots {
            let prefs = root.join(".settings").join("org.eclipse.jdt.core.prefs");
            let mut text = std::fs::read_to_string(&prefs).unwrap_or_else(|_| "eclipse.preferences.version=1\n".into());
            for k in &self.0 {
                text.push_str(&format!("{k}=ignore\n"));
            }
            let _ = std::fs::create_dir_all(root.join(".settings"));
            let _ = std::fs::write(&prefs, text);
        }
        Ok(Vec::new())
    }
}

/// Titles of bridge (legacy Java) code actions that the Rust processors
/// replace; they are no longer forwarded.
pub fn is_superseded_legacy_action(title: &str) -> bool {
    const SUPERSEDED: &[&str] = &["Organize Imports", "Add serialVersionUID field", "Remove unnecessary cast", "Remove redundant superinterface"];
    SUPERSEDED.contains(&title) || title.starts_with("Generate Getter") || title.starts_with("Generate Setter") || title.starts_with("Generate Constructor") || title == "Generate constructor from fields" || title.starts_with("Generate toString()") || title.starts_with("Generate hashCode() and equals()") || title.starts_with("Generate Delegate Methods")
}

/// `CodeActionResolveHandler.resolve`.
pub async fn resolve(env: &Env<'_>, mut action: CodeAction) -> CodeAction {
    // JDTLanguageServer returns the original action (including data) when
    // handleChanged invalidated all stored proposals.
    if STORE.lock().unwrap_or_else(|e| e.into_inner()).1.is_empty() {
        return action;
    }
    let data = action.data.take();
    let (Some(rid), Some(pid)) = (
        data.as_ref().and_then(|d| d.get(DATA_FIELD_REQUEST_ID)).and_then(Value::as_str).and_then(|s| s.parse::<u64>().ok()),
        data.as_ref().and_then(|d| d.get(DATA_FIELD_PROPOSAL_ID)).and_then(Value::as_str).and_then(|s| s.parse::<usize>().ok()),
    ) else {
        return action;
    };
    let proposal = {
        let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        guard.1.iter().find(|s| s.id == rid).and_then(|s| s.proposals.get(pid).cloned())
    };
    let Some(proposal) = proposal else { return action };
    let mut p = proposal.lock().await;
    if let Ok(we) = edit::to_workspace_edit(env, &mut p.change).await {
        if edit::has_changes(&we) {
            action.edit = Some(we);
        }
    }
    action
}
