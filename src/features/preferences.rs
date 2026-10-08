//! The jdt.ls `Preferences` and `ClientPreferences` that code lens,
//! workspace symbols and the hierarchies read.  Settings arrive through
//! `initializationOptions.settings` and are updated by
//! `workspace/didChangeConfiguration` the way `Preferences.updateFrom` does:
//! only the keys present in the change are replaced.

pub mod manager;
pub mod map_flattener;
pub mod model;

use serde_json::Value;
use std::sync::RwLock;
use tower_lsp::lsp_types::InitializeParams;

static SETTINGS: RwLock<Option<Value>> = RwLock::new(None);
static ORGANIZE_IMPORT_FAVORITES: RwLock<Vec<String>> = RwLock::new(Vec::new());
static CLEANUP_ACTIONS: RwLock<Vec<String>> = RwLock::new(Vec::new());
static CLASS_FILE_CONTENTS: RwLock<bool> = RwLock::new(false);

pub fn init(params: &InitializeParams) {
    let opts = params.initialization_options.as_ref();
    let settings = opts.and_then(|o| o.get("settings")).cloned();
    // Preferences.setJavaCompletionFavoriteMembers writes the preference
    // manager's current value while updateFrom is still constructing its
    // replacement. OrganizeImportsOperation reads that JavaManipulation
    // preference, rather than the replacement's favorite-members field.
    *ORGANIZE_IMPORT_FAVORITES.write().unwrap_or_else(|e| e.into_inner()) =
        if settings
            .as_ref()
            .and_then(|s| lookup(s, &["java", "completion", "favoriteStaticMembers"]))
            .is_some()
        {
            super::completion::prefs::FAVORITES_DEFAULT
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };
    *CLEANUP_ACTIONS.write().unwrap_or_else(|e| e.into_inner()) =
        settings.as_ref().map(cleanup_actions_from).unwrap_or_default();
    // InitHandler: `preferenceManager.update(Preferences.createFrom(settings))`.
    let preferences = model::Preferences::create_from(settings.as_ref().unwrap_or(&Value::Null));
    manager_write().update(preferences);
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
    if lookup(change, &["java", "cleanup", "actions"]).is_some()
        || lookup(change, &["java", "cleanup", "actionsOnSave"]).is_some()
    {
        // Preferences.updateFrom selects the list from this notification,
        // defaulting absent aliases to empty rather than reviving an old list.
        *CLEANUP_ACTIONS.write().unwrap_or_else(|e| e.into_inner()) = cleanup_actions_from(change);
    }
    if lookup(change, &["java", "completion", "favoriteStaticMembers"]).is_some() {
        *ORGANIZE_IMPORT_FAVORITES.write().unwrap_or_else(|e| e.into_inner()) =
            super::completion::prefs::Prefs::load().favorite_members;
    }
    {
        let mut manager = manager_write();
        let preferences = model::Preferences::update_from(manager.get_preferences(), change);
        manager.update(preferences);
    }
    let mut guard = SETTINGS.write().unwrap_or_else(|e| e.into_inner());
    let current = guard.get_or_insert_with(|| Value::Object(Default::default()));
    merge(current, change);
}

/// `JavaLanguageServerPlugin.getPreferencesManager()`.
static MANAGER: once_cell::sync::Lazy<RwLock<manager::PreferenceManager>> =
    once_cell::sync::Lazy::new(|| RwLock::new(manager::PreferenceManager::new()));

fn manager_write() -> std::sync::RwLockWriteGuard<'static, manager::PreferenceManager> {
    MANAGER.write().unwrap_or_else(|e| e.into_inner())
}

/// `PreferenceManager.getPreferences()` (a snapshot).
pub fn current() -> model::Preferences {
    MANAGER.read().unwrap_or_else(|e| e.into_inner()).get_preferences().clone()
}

/// `JavaManipulation.getCodeTemplateStore().findTemplateById(id)`: the
/// pattern of a template the preference manager registers (`typecomment`,
/// `filecomment`, `newtype`).
pub fn code_template(id: &str) -> Option<String> {
    MANAGER.read().unwrap_or_else(|e| e.into_inner()).code_template_store().find_template_by_id(id).map(|t| t.pattern.clone())
}

fn cleanup_actions_from(settings: &Value) -> Vec<String> {
    let strings = |key| lookup(settings, &["java", "cleanup", key])
        .and_then(|v| v.as_array().cloned()).unwrap_or_default()
        .iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>();
    let actions = strings("actions");
    if actions.is_empty() { strings("actionsOnSave") } else { actions }
}

pub fn cleanup_actions() -> Vec<String> {
    CLEANUP_ACTIONS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn organize_import_favorites() -> Vec<String> {
    ORGANIZE_IMPORT_FAVORITES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
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

/// `Preferences.getMavenUserSettings` from a settings object
/// (`java.configuration.maven.userSettings`, `~/` expanded like
/// `ResourceUtils.expandPath`).
pub fn maven_user_settings_from(settings: &Value) -> Option<String> {
    let v = settings.get("java.configuration.maven.userSettings").cloned().or_else(|| {
        lookup(settings, &["java", "configuration", "maven", "userSettings"])
    })?;
    v.as_str().map(crate::features::formatting::options::expand_path)
}

/// `Preferences.getMavenUserSettings`.
pub fn maven_user_settings() -> Option<String> {
    let guard = SETTINGS.read().unwrap_or_else(|e| e.into_inner());
    maven_user_settings_from(guard.as_ref()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Port of `InitHandlerTest.testMavenSettings`.
    #[test]
    fn test_maven_settings() {
        let test = format!("{}test", std::path::MAIN_SEPARATOR);
        let mut initialization_options = json!({ "java.configuration.maven.userSettings": format!("~{test}") });
        let home = std::env::var("HOME").unwrap();
        assert_eq!(Some(format!("{home}{test}")), maven_user_settings_from(&initialization_options));
        initialization_options["java.configuration.maven.userSettings"] = Value::Null;
        assert_eq!(None, maven_user_settings_from(&initialization_options));
        let tilde_test = "~test";
        initialization_options["java.configuration.maven.userSettings"] = json!(tilde_test);
        assert_eq!(Some(tilde_test.to_owned()), maven_user_settings_from(&initialization_options));
    }
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
    let manager = MANAGER.read().unwrap_or_else(|e| e.into_inner());
    let preferences = manager.get_preferences();
    (preferences.get_import_on_demand_threshold(), preferences.get_static_import_on_demand_threshold())
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
/// (`java.edit.validateAllOpenBuffersOnChanges`; `Preferences` defaults it
/// to `true`).
pub fn validate_all_open_buffers_on_changes() -> bool {
    get_bool("java.edit.validateAllOpenBuffersOnChanges").unwrap_or(true)
}

/// `Preferences.getCodeGenerationAddFinalForNewDeclaration()`
/// (`java.codeGeneration.addFinalForNewDeclaration`: `none`, `fields`,
/// `variables` or `all`; default `none`).
pub fn add_final_for_new_declaration() -> String {
    get_string("java.codeGeneration.addFinalForNewDeclaration").unwrap_or_else(|| "none".into())
}

static EXTENDED: RwLock<Option<Value>> = RwLock::new(None);

/// An `extendedClientCapabilities` list containing `item` (e.g.
/// `ClientPreferences.isExtractVariableInferSelectionSupported`).
pub fn extended_capability_list_contains(name: &str, item: &str) -> bool {
    EXTENDED.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|v| v.get(name)).and_then(Value::as_array).is_some_and(|l| l.iter().any(|v| v.as_str() == Some(item)))
}

/// `ClientPreferences.excludedMarkerTypes()`: the string entries of the
/// `excludedMarkerTypes` list.
pub fn excluded_marker_types() -> Vec<String> {
    EXTENDED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|v| v.get("excludedMarkerTypes"))
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// An `extendedClientCapabilities` flag (`ClientPreferences`).
pub fn extended_capability(name: &str) -> bool {
    EXTENDED.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|v| v.get(name)).is_some_and(|v| {
        v.as_bool().unwrap_or_else(|| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")))
    })
}
