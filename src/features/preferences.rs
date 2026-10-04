//! The jdt.ls `Preferences` and `ClientPreferences` that code lens,
//! workspace symbols and the hierarchies read.  Settings arrive through
//! `initializationOptions.settings` and are updated by
//! `workspace/didChangeConfiguration` the way `Preferences.updateFrom` does:
//! only the keys present in the change are replaced.

use serde_json::Value;
use std::sync::RwLock;
use tower_lsp::lsp_types::InitializeParams;

static SETTINGS: RwLock<Option<Value>> = RwLock::new(None);
static CLASS_FILE_CONTENTS: RwLock<bool> = RwLock::new(false);

pub fn init(params: &InitializeParams) {
    let opts = params.initialization_options.as_ref();
    let settings = opts.and_then(|o| o.get("settings")).cloned();
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = settings;
    // `ClientPreferences.isClassFileContentSupported`
    let class_files = opts
        .and_then(|o| o.pointer("/extendedClientCapabilities/classFileContentsSupport"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    *CLASS_FILE_CONTENTS.write().unwrap_or_else(|e| e.into_inner()) = class_files;
    *EXTENDED.write().unwrap_or_else(|e| e.into_inner()) = opts.and_then(|o| o.get("extendedClientCapabilities")).cloned();
}

/// `workspace/didChangeConfiguration`.
pub fn update(change: &Value) {
    let mut guard = SETTINGS.write().unwrap_or_else(|e| e.into_inner());
    let current = guard.get_or_insert_with(|| Value::Object(Default::default()));
    merge(current, change);
}

fn merge(into: &mut Value, from: &Value) {
    match (into, from) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => merge(existing, v),
                    _ => {
                        a.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// A dotted preference key (`java.referencesCodeLens.enabled`), looked up
/// as a flat key or as nested objects.
pub fn get(key: &str) -> Option<Value> {
    let guard = SETTINGS.read().unwrap_or_else(|e| e.into_inner());
    let settings = guard.as_ref()?;
    if let Some(v) = settings.get(key) {
        return Some(v.clone());
    }
    let parts: Vec<&str> = key.split('.').collect();
    lookup(settings, &parts)
}

fn lookup(v: &Value, parts: &[&str]) -> Option<Value> {
    if parts.is_empty() {
        return Some(v.clone());
    }
    let obj = v.as_object()?;
    // Try progressively longer dotted prefixes ("java" / "java.referencesCodeLens" …).
    for n in 1..=parts.len() {
        let k = parts[..n].join(".");
        if let Some(child) = obj.get(&k) {
            if let Some(found) = lookup(child, &parts[n..]) {
                return Some(found);
            }
        }
    }
    None
}

pub fn get_bool(key: &str) -> Option<bool> {
    let v = get(key)?;
    v.as_bool().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

pub fn get_string(key: &str) -> Option<String> {
    get(key)?.as_str().map(str::to_owned)
}

/// `Preferences.isReferencesCodeLensEnabled` (default `true`).
pub fn references_code_lens_enabled() -> bool {
    get_bool("java.referencesCodeLens.enabled").unwrap_or(true)
}

/// `Preferences.getImplementationsCodeLens`: `java.implementationCodeLens`
/// (`none`, `types`, `methods` or `all`; default `none`).  The older
/// boolean `java.implementationsCodeLens.enabled` maps to `types`.
pub fn implementations_code_lens() -> String {
    if let Some(s) = get_string("java.implementationCodeLens") {
        return s;
    }
    match get_bool("java.implementationsCodeLens.enabled") {
        Some(true) => "types".to_owned(),
        _ => "none".to_owned(),
    }
}

/// `Preferences.isCodeLensEnabled`.
pub fn code_lens_enabled() -> bool {
    references_code_lens_enabled() || implementations_code_lens() != "none"
}

/// `Preferences.isIncludeSourceMethodDeclarations` (default `false`).
pub fn include_source_method_declarations() -> bool {
    get_bool("java.symbols.includeSourceMethodDeclarations").unwrap_or(false)
}

/// `Preferences.getSearchScope() == SearchScope.main`: exclude test code.
pub fn search_scope_main() -> bool {
    get_string("java.search.scope").as_deref() == Some("main")
}

/// `PreferenceManager.isClientSupportsClassFileContent`.
pub fn class_file_contents_supported() -> bool {
    *CLASS_FILE_CONTENTS.read().unwrap_or_else(|e| e.into_inner())
}

/// `ClientPreferences.isSymbolTagSupported` (`textDocument.documentSymbol.tagSupport`).
pub fn symbol_tags_supported() -> bool {
    super::client_caps::symbol_tags()
}

/// `Preferences.getImportOrder()` (`java.completion.importOrder`; default
/// `java`, `javax`, `org`, `com`).
pub fn import_order() -> Vec<String> {
    let order: Vec<String> = get("java.completion.importOrder")
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()))
        .unwrap_or_default();
    if order.is_empty() {
        vec!["java".into(), "javax".into(), "org".into(), "com".into()]
    } else {
        order
    }
}

/// `java.sources.organizeImports.starThreshold` / `staticStarThreshold`
/// (default 99; non-positive values reset to the default).
pub fn import_thresholds() -> (i32, i32) {
    let get_int = |k: &str| {
        get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).filter(|n| *n > 0).map(|n| n as i32)
    };
    (
        get_int("java.sources.organizeImports.starThreshold").unwrap_or(99),
        get_int("java.sources.organizeImports.staticStarThreshold").unwrap_or(99),
    )
}

/// `Preferences.getJavaQuickFixShowAt()` (`java.quickfix.showAt`: `line` or
/// `problem`; default `line`).
pub fn quickfix_show_at() -> String {
    get_string("java.quickfix.showAt").unwrap_or_else(|| "line".into())
}

/// `Preferences.isCodeGenerationTemplateGenerateComments()`
/// (`java.codeGeneration.generateComments`; default `false`).
pub fn generate_comments() -> bool {
    get_bool("java.codeGeneration.generateComments").unwrap_or(false)
}

/// `Preferences.isValidateAllOpenBuffersOnChanges()`
/// (`java.edit.validateAllOpenBuffersOnChanges`; default `false`).
pub fn validate_all_open_buffers_on_changes() -> bool {
    get_bool("java.edit.validateAllOpenBuffersOnChanges").unwrap_or(false)
}

/// `Preferences.getCodeGenerationAddFinalForNewDeclaration()`
/// (`java.codeGeneration.addFinalForNewDeclaration`: `none`, `fields`,
/// `variables` or `all`; default `none`).
pub fn add_final_for_new_declaration() -> String {
    get_string("java.codeGeneration.addFinalForNewDeclaration").unwrap_or_else(|| "none".into())
}

static EXTENDED: RwLock<Option<Value>> = RwLock::new(None);

/// An `extendedClientCapabilities` flag (`ClientPreferences`).
pub fn extended_capability(name: &str) -> bool {
    EXTENDED.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|v| v.get(name)).is_some_and(|v| {
        v.as_bool().unwrap_or_else(|| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")))
    })
}
