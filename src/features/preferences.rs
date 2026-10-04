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
