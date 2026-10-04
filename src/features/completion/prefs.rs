//! The jdt.ls `Preferences` and `ClientPreferences` completion reads.

use crate::features::preferences as p;
use serde_json::Value;
use std::sync::RwLock;
use tower_lsp::lsp_types::{ClientCapabilities, InitializeParams, InsertTextMode, MarkupKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessMode {
    Off,
    InsertParameterNames,
    InsertBestGuessedArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCase {
    Off,
    FirstLetter,
}

/// Snapshot of the completion preferences (jdt.ls `Preferences`).
#[derive(Debug, Clone)]
pub struct Prefs {
    pub enabled: bool,
    pub max_results: usize,
    pub guess_mode: GuessMode,
    pub collapse: bool,
    pub match_case: MatchCase,
    pub favorite_members: Vec<String>,
    pub import_order: Vec<String>,
    pub filtered_types: Vec<String>,
    pub postfix: bool,
    pub lazy_resolve_text_edit: bool,
    pub overwrite: bool,
    pub on_demand_threshold: i64,
    pub static_on_demand_threshold: i64,
    pub generate_comments: bool,
    pub chain: bool,
    pub signature_help: bool,
}

const FAVORITES_DEFAULT: &[&str] = &[
    "org.junit.Assert.*",
    "org.junit.Assume.*",
    "org.junit.jupiter.api.Assertions.*",
    "org.junit.jupiter.api.Assumptions.*",
    "org.junit.jupiter.api.DynamicContainer.*",
    "org.junit.jupiter.api.DynamicTest.*",
];
const IMPORT_ORDER_DEFAULT: &[&str] = &["java", "javax", "org", "com"];
// `Preferences.JAVA_COMPLETION_FILTERED_TYPES_DEFAULT` is only the value of
// the `filteredTypes` field: the type filter itself reads
// `org.eclipse.jdt.ui.typefilter.enabled`, which only `setFilteredTypes`
// (i.e. a configuration containing `java.completion.filteredTypes`) writes.
// Without that key jdt.ls filters nothing.

fn list(key: &str) -> Option<Vec<String>> {
    match p::get(key)? {
        Value::Array(a) => Some(a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()),
        Value::String(s) => Some(s.split(',').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect()),
        _ => None,
    }
}

fn int(key: &str) -> Option<i64> {
    let v = p::get(key)?;
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn upper_camel_to_upper_underscore(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_uppercase());
    }
    out
}

impl Prefs {
    pub fn load() -> Self {
        let guess_mode = match p::get("java.completion.guessMethodArguments") {
            Some(Value::Bool(true)) => GuessMode::InsertBestGuessedArguments,
            Some(Value::Bool(false)) => GuessMode::InsertParameterNames,
            Some(Value::String(s)) => match upper_camel_to_upper_underscore(&s).as_str() {
                "OFF" => GuessMode::Off,
                "INSERT_PARAMETER_NAMES" => GuessMode::InsertParameterNames,
                "INSERT_BEST_GUESSED_ARGUMENTS" => GuessMode::InsertBestGuessedArguments,
                _ => GuessMode::InsertParameterNames,
            },
            _ => GuessMode::InsertParameterNames,
        };
        let match_case = match p::get_string("java.completion.matchCase").map(|s| s.to_uppercase()).as_deref() {
            Some("FIRSTLETTER") => MatchCase::FirstLetter,
            _ => MatchCase::Off,
        };
        let favorite_members = list("java.completion.favoriteStaticMembers")
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| FAVORITES_DEFAULT.iter().map(|s| s.to_string()).collect());
        let import_order = list("java.completion.importOrder")
            .unwrap_or_else(|| IMPORT_ORDER_DEFAULT.iter().map(|s| s.to_string()).collect());
        let filtered_types = list("java.completion.filteredTypes")
            .unwrap_or_default();
        Prefs {
            enabled: p::get_bool("java.completion.enabled").unwrap_or(true),
            max_results: int("java.completion.maxResults").map(|n| if n <= 0 { usize::MAX } else { n as usize }).unwrap_or(50),
            guess_mode,
            collapse: p::get_bool("java.completion.collapseCompletionItems").unwrap_or(false),
            match_case,
            favorite_members,
            import_order,
            filtered_types,
            postfix: p::get_bool("java.completion.postfix.enabled").unwrap_or(true),
            lazy_resolve_text_edit: p::get_bool("java.completion.lazyResolveTextEdit.enabled").unwrap_or(false),
            overwrite: p::get_bool("java.completion.overwrite").unwrap_or(true),
            on_demand_threshold: int("java.sources.organizeImports.starThreshold").filter(|n| *n > 0).unwrap_or(99),
            static_on_demand_threshold: int("java.sources.organizeImports.staticStarThreshold").filter(|n| *n > 0).unwrap_or(99),
            generate_comments: p::get_bool("java.codeGeneration.generateComments").unwrap_or(false),
            chain: p::get_bool("java.completion.chain.enabled").unwrap_or(false),
            signature_help: p::get_bool("java.signatureHelp.enabled").unwrap_or(false),
        }
    }
}

// ─── Client preferences ──────────────────────────────────────────────────────

static CAPS: RwLock<Option<(ClientCapabilities, Value)>> = RwLock::new(None);

pub fn init(params: &InitializeParams) {
    let extended = params
        .initialization_options
        .as_ref()
        .and_then(|o| o.get("extendedClientCapabilities"))
        .cloned()
        .unwrap_or(Value::Null);
    *CAPS.write().unwrap_or_else(|e| e.into_inner()) = Some((params.capabilities.clone(), extended));
}

/// Snapshot of the `ClientPreferences` completion reads.
#[derive(Debug, Clone, Default)]
pub struct Client {
    pub snippets: bool,
    pub label_details: bool,
    pub insert_replace: bool,
    pub resolve_properties: Vec<String>,
    pub documentation_markdown: bool,
    pub tag_support: bool,
    pub item_defaults: Vec<String>,
    pub insert_text_mode_support: Vec<InsertTextMode>,
    pub insert_text_mode_default: Option<InsertTextMode>,
    pub signature_help: bool,
    pub resolve_additional_text_edits_extended: bool,
    pub completion_item_command: String,
    pub execute_client_command: bool,
}

impl Client {
    pub fn load() -> Self {
        let guard = CAPS.read().unwrap_or_else(|e| e.into_inner());
        let Some((caps, ext)) = guard.as_ref() else { return Client::default() };
        let td = caps.text_document.as_ref();
        let completion = td.and_then(|t| t.completion.as_ref());
        let item = completion.and_then(|c| c.completion_item.as_ref());
        let flag = |b: Option<bool>| b.unwrap_or(false);
        let ext_bool = |k: &str| {
            ext.get(k).is_some_and(|v| v.as_bool().unwrap_or(false) || v.as_str() == Some("true"))
        };
        Client {
            snippets: flag(item.and_then(|i| i.snippet_support)),
            label_details: flag(item.and_then(|i| i.label_details_support)),
            insert_replace: flag(item.and_then(|i| i.insert_replace_support)),
            resolve_properties: item
                .and_then(|i| i.resolve_support.as_ref())
                .map(|r| r.properties.clone())
                .unwrap_or_default(),
            documentation_markdown: item
                .and_then(|i| i.documentation_format.as_ref())
                .is_some_and(|f| f.contains(&MarkupKind::Markdown)),
            tag_support: item.and_then(|i| i.tag_support.as_ref()).is_some(),
            item_defaults: completion
                .and_then(|c| c.completion_list.as_ref())
                .and_then(|l| l.item_defaults.clone())
                .unwrap_or_default(),
            insert_text_mode_support: item
                .and_then(|i| i.insert_text_mode_support.as_ref())
                .map(|s| s.value_set.clone())
                .unwrap_or_default(),
            insert_text_mode_default: completion.and_then(|c| c.insert_text_mode),
            signature_help: td.and_then(|t| t.signature_help.as_ref()).is_some(),
            resolve_additional_text_edits_extended: ext_bool("resolveAdditionalTextEditsSupport"),
            completion_item_command: ext
                .get("onCompletionItemSelectedCommand")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            execute_client_command: ext_bool("executeClientCommandSupport"),
        }
    }

    pub fn resolve_supports(&self, property: &str) -> bool {
        self.resolve_properties.iter().any(|p| p == property)
    }
    /// `isResolveAdditionalTextEditsSupport`.
    pub fn resolve_additional_text_edits(&self) -> bool {
        self.resolve_supports("additionalTextEdits") || self.resolve_additional_text_edits_extended
    }
    /// `isCompletionResolveDocumentSupport`.
    pub fn resolve_documentation(&self) -> bool {
        self.resolve_supports("documentation")
    }
    /// `isCompletionListItemDefaultsPropertySupport`.
    pub fn item_defaults_property(&self, p: &str) -> bool {
        self.item_defaults.iter().any(|x| x == p)
    }
    /// `isCompletionListItemDefaultsSupport`.
    pub fn item_defaults_support(&self) -> bool {
        self.item_defaults_property("editRange")
            || self.item_defaults_property("insertTextFormat")
            || self.item_defaults_property("insertTextMode")
    }
    pub fn insert_text_mode_supported(&self, mode: InsertTextMode) -> bool {
        self.insert_text_mode_support.contains(&mode)
    }
}
