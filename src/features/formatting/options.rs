//! Formatter settings and option resolution — Rust port of the option
//! handling in jdt.ls `Preferences` (`java.format.*`), `FormatterManager`
//! (Eclipse XML profiles), `StandardProjectsManager.configureSettings` and
//! `FormatterHandler.getOptions`.
//!
//! Resolution order (as in jdt.ls):
//! 1. Eclipse default formatter settings (part of `JavaCore.getDefaultOptions()`),
//! 2. jdt.ls formatter defaults (`FormatterHandler.getJavaLSDefaultFormatterSettings`),
//! 3. the profile from `java.format.settings.url`/`profile` (completed with
//!    1 + 2 and upgraded by `ProfileVersionerCore` when older than the
//!    current version),
//! 4. the JDT options of the project owning the document (compiler options,
//!    `.settings/org.eclipse.jdt.core.prefs`),
//! 5. the client's `FormattingOptions` (every entry, then tab size and tab char).

use super::defaults::ECLIPSE_DEFAULTS;
use super::versioner;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{FormattingOptions, FormattingProperty};
use tracing::{error, info, warn};

pub const FORMATTER_OPTION_PREFIX: &str = "org.eclipse.jdt.core.formatter";
pub const FORMATTER_TAB_SIZE: &str = "org.eclipse.jdt.core.formatter.tabulation.size";
pub const FORMATTER_TAB_CHAR: &str = "org.eclipse.jdt.core.formatter.tabulation.char";
pub const FORMATTER_CONTINUATION_INDENTATION: &str = "org.eclipse.jdt.core.formatter.continuation_indentation";

/// The `java.format.*` preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSettings {
    /// `java.format.enabled` (default `true`).
    pub enabled: bool,
    /// `java.format.onType.enabled` (default `false`).
    pub on_type_enabled: bool,
    /// `java.format.comments.enabled` (default `true`).
    pub comments_enabled: bool,
    /// `java.format.settings.url` (after `ResourceUtils.expandPath`).
    pub settings_url: Option<String>,
    /// `java.format.settings.profile`.
    pub profile: Option<String>,
}

impl Default for FormatSettings {
    fn default() -> Self {
        Self { enabled: true, on_type_enabled: false, comments_enabled: true, settings_url: None, profile: None }
    }
}

impl FormatSettings {
    /// `Preferences.updateFrom`: only keys present in `configuration` change.
    pub fn update_from(&mut self, configuration: &Value) {
        if contains_key(configuration, "java.format.enabled") {
            self.enabled = get_boolean(configuration, "java.format.enabled", self.enabled);
        }
        if contains_key(configuration, "java.format.onType.enabled") {
            self.on_type_enabled = get_boolean(configuration, "java.format.onType.enabled", self.on_type_enabled);
        }
        if contains_key(configuration, "java.format.settings.url") {
            self.settings_url = get_string(configuration, "java.format.settings.url").map(|s| expand_path(&s));
        }
        if contains_key(configuration, "java.format.settings.profile") {
            self.profile = get_string(configuration, "java.format.settings.profile");
        }
        if contains_key(configuration, "java.format.comments.enabled") {
            self.comments_enabled = get_boolean(configuration, "java.format.comments.enabled", self.comments_enabled);
        }
    }
}

// ── MapFlattener ─────────────────────────────────────────────────────────────

/// `MapFlattener.getValue`: a flat dotted key first, then nested maps.
pub fn get_value<'a>(configuration: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = configuration.get(key).filter(|v| !v.is_null()) {
        return Some(v);
    }
    let mut current = configuration;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        let v = current.get(*part)?;
        if i == parts.len() - 1 {
            return (!v.is_null()).then_some(v);
        }
        if !v.is_object() {
            return None;
        }
        current = v;
    }
    None
}

/// `MapFlattener.containsKey`.
pub fn contains_key(configuration: &Value, key: &str) -> bool {
    if configuration.as_object().is_some_and(|m| m.contains_key(key)) {
        return true;
    }
    let mut current = configuration;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        let Some(map) = current.as_object() else { return false };
        if i == parts.len() - 1 {
            return map.contains_key(*part);
        }
        match map.get(*part) {
            Some(v) if v.is_object() => current = v,
            _ => return false,
        }
    }
    false
}

fn get_string(configuration: &Value, key: &str) -> Option<String> {
    get_value(configuration, key).and_then(Value::as_str).map(str::to_owned)
}

fn get_boolean(configuration: &Value, key: &str, def: bool) -> bool {
    match get_value(configuration, key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => def,
    }
}

/// `ResourceUtils.expandPath`: `~/` and `${name}` (system property or
/// environment variable) expansion.
pub fn expand_path(path: &str) -> String {
    let mut path = path.to_owned();
    let home = std::env::var("HOME").unwrap_or_default();
    if let Some(rest) = path.strip_prefix(&format!("~{}", std::path::MAIN_SEPARATOR)) {
        path = format!("{home}{}{rest}", std::path::MAIN_SEPARATOR);
    }
    let re = regex::Regex::new(r"\$\{([^}]*)\}").unwrap();
    re.replace_all(&path, |c: &regex::Captures| {
        let key = &c[1];
        let value = match key {
            "user.home" => Some(home.clone()),
            "user.dir" => std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned()),
            "file.separator" => Some(std::path::MAIN_SEPARATOR.to_string()),
            _ if key.is_empty() => None,
            _ => std::env::var(key).ok(),
        };
        value.unwrap_or_else(|| c[0].to_owned())
    })
    .into_owned()
}

// ── Defaults ─────────────────────────────────────────────────────────────────

/// `DefaultCodeFormatterConstants.getEclipseDefaultSettings()`.
pub fn eclipse_defaults() -> BTreeMap<String, String> {
    ECLIPSE_DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// `FormatterHandler.getJavaLSDefaultFormatterSettings()`.
pub fn jdtls_default_formatter_settings() -> BTreeMap<String, String> {
    [
        ("org.eclipse.jdt.core.formatter.join_wrapped_lines", "false"),
        ("org.eclipse.jdt.core.formatter.join_lines_in_comments", "false"),
        ("org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_switch", "true"),
        ("org.eclipse.jdt.core.formatter.use_on_off_tags", "true"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// `FormatterHandler.getCombinedDefaultFormatterSettings()`.
pub fn combined_default_formatter_settings() -> BTreeMap<String, String> {
    let mut options = eclipse_defaults();
    options.extend(jdtls_default_formatter_settings());
    options
}

/// The formatter part of `JavaCore.getOptions()` once jdt.ls has initialized
/// it (`PreferenceManager.initializeJavaCoreOptions`) and applied the
/// configured profile (`StandardProjectsManager.configureSettings`).
pub fn workspace_formatter_options(settings: &FormatSettings, roots: &[PathBuf]) -> BTreeMap<String, String> {
    let mut java_options = combined_default_formatter_settings();
    let Some(path) = settings.settings_url.as_deref().and_then(|u| formatter_path(u, roots)) else {
        return java_options;
    };
    let formatter_options = match std::fs::read_to_string(&path) {
        Ok(xml) => read_settings_from_stream(&xml, settings.profile.as_deref()),
        Err(e) => {
            error!("{e}");
            None
        }
    };
    let mut default_options = combined_default_formatter_settings();
    if let Some(fo) = formatter_options.filter(|m| !m.is_empty()) {
        default_options.extend(fo);
    }
    for (k, v) in default_options {
        if k.starts_with(FORMATTER_OPTION_PREFIX) {
            java_options.insert(k, v);
        }
    }
    java_options
}

// ── Profile location (`Preferences.getFormatterAsURI`) ───────────────────────

/// `BaseInitHandler`: the root paths are `initializationOptions.workspaceFolders`,
/// else `rootUri`, else `rootPath`.
pub fn jdtls_root_paths(workspace_folders: Option<&[String]>, root_uri: Option<&url::Url>, root_path: Option<&str>) -> Vec<PathBuf> {
    let to_path = |u: &url::Url| (u.scheme() == "file").then(|| u.to_file_path().ok()).flatten().map(|p| crate::project::canonicalize_lenient(&p));
    if let Some(folders) = workspace_folders.filter(|f| !f.is_empty()) {
        return folders.iter().filter_map(|f| url::Url::parse(f).ok()).filter_map(|u| to_path(&u)).collect();
    }
    if let Some(uri) = root_uri {
        return to_path(uri).into_iter().collect();
    }
    root_path.map(PathBuf::from).into_iter().collect()
}

/// Resolve `java.format.settings.url` the way `Preferences.asURI` does and
/// return the local file it designates.
pub fn formatter_path(url: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if url.trim().is_empty() {
        return None;
    }
    let resolved = match java_uri_scheme(url) {
        Ok(Some(_)) => absolute_uri_path(url),
        Ok(None) | Err(()) => find_file(url, roots),
    };
    if resolved.is_none() {
        info!("Cannot resolve '{url}'.");
    }
    resolved
}

/// Mimics `new java.net.URI(s)`: `Err` on a syntax error, otherwise the scheme
/// of an absolute URI (or `None` for a relative reference).
fn java_uri_scheme(s: &str) -> Result<Option<String>, ()> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let ok = i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit();
            if !ok {
                return Err(());
            }
            i += 3;
            continue;
        }
        if b < 0x80 && (b <= b' ' || b == 0x7f || b"\"<>\\^`{|}".contains(&b)) {
            return Err(());
        }
        i += 1;
    }
    let scheme_re = regex::Regex::new(r"^([A-Za-z][A-Za-z0-9+.\-]*):").unwrap();
    match scheme_re.captures(s) {
        Some(c) => {
            if s.len() == c[0].len() {
                return Err(()); // "scheme:" with an empty scheme-specific part
            }
            Ok(Some(c[1].to_owned()))
        }
        None => Ok(None),
    }
}

fn absolute_uri_path(url: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "file" {
        warn!("Formatter settings from '{url}' are not supported (only local files are)");
        return None;
    }
    parsed.to_file_path().ok()
}

/// `Preferences.getURI`/`findFile`: the path itself, or relative to a root.
fn find_file(path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let file = Path::new(path);
    let found = if file.exists() {
        Some(file.to_path_buf())
    } else {
        roots.iter().map(|r| r.join(path)).find(|f| f.is_file())
    };
    let found = found.filter(|f| f.is_file())?;
    Some(if found.is_absolute() {
        found
    } else {
        std::env::current_dir().map(|d| d.join(&found)).unwrap_or(found)
    })
}

// ── Profile parsing (`FormatterManager.readSettingsFromStream`) ──────────────

/// Read the settings of `profile_name` (or of the last profile when no name
/// is given) from an Eclipse formatter profile XML.  `None` when the XML is
/// not readable (upstream throws a `CoreException`), an empty map when the
/// profile is missing.
pub fn read_settings_from_stream(xml: &str, profile_name: Option<&str>) -> Option<BTreeMap<String, String>> {
    let profile_name = profile_name.filter(|p| !p.trim().is_empty());
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            error!("{e}");
            return None;
        }
    };
    let mut last_name: Option<String> = None;
    let mut settings: Option<BTreeMap<String, String>> = None;
    let mut version = versioner::CURRENT_VERSION;
    for node in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "profile") {
        let name = node.attribute("name").map(str::to_owned);
        last_name = name.clone();
        if profile_name.is_none() || profile_name == name.as_deref() {
            let mut map = BTreeMap::new();
            for setting in node.descendants().filter(|n| n.is_element() && n.tag_name().name() == "setting") {
                if let (Some(id), Some(value)) = (setting.attribute("id"), setting.attribute("value")) {
                    map.insert(id.to_owned(), value.to_owned());
                }
            }
            settings = Some(map);
            version = node
                .attribute("version")
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(versioner::CURRENT_VERSION);
        }
    }
    let Some(settings) = settings else {
        if profile_name != last_name.as_deref() {
            let p = profile_name.unwrap_or("null");
            error!("Invalid settings: java.format.settings.profile={p}. The '{p}' profile doesn't exist.");
        } else {
            error!("Invalid Formatter settings. Check 'java.format.settings.url' and 'java.format.settings.profile'");
        }
        return Some(BTreeMap::new());
    };
    if version == versioner::CURRENT_VERSION {
        return Some(settings);
    }
    Some(versioner::update_and_complete(&settings, version))
}

// ── Client options (`FormatterHandler.getOptions`) ───────────────────────────

/// Apply the client's `FormattingOptions` on top of the JDT options.
pub fn apply_formatting_options(eclipse_options: &mut BTreeMap<String, String>, options: &FormattingOptions) {
    // Every entry of the options map (lsp4j `FormattingOptions` is a map).
    eclipse_options.insert("tabSize".to_owned(), options.tab_size.to_string());
    eclipse_options.insert("insertSpaces".to_owned(), options.insert_spaces.to_string());
    for (key, value) in [
        ("trimTrailingWhitespace", options.trim_trailing_whitespace),
        ("insertFinalNewline", options.insert_final_newline),
        ("trimFinalNewlines", options.trim_final_newlines),
    ] {
        if let Some(v) = value {
            eclipse_options.insert(key.to_owned(), v.to_string());
        }
    }
    for (key, value) in &options.properties {
        let v = match value {
            FormattingProperty::Bool(b) => b.to_string(),
            FormattingProperty::Number(n) => n.to_string(),
            FormattingProperty::String(s) => s.clone(),
        };
        eclipse_options.insert(key.clone(), v);
    }
    if options.tab_size > 0 {
        eclipse_options.insert(FORMATTER_TAB_SIZE.to_owned(), options.tab_size.to_string());
    }
    eclipse_options.insert(
        FORMATTER_TAB_CHAR.to_owned(),
        if options.insert_spaces { "space" } else { "tab" }.to_owned(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_uri_syntax() {
        assert_eq!(java_uri_scheme("../../formatter/test.xml"), Ok(None));
        assert_eq!(java_uri_scheme("/a/formatter resources/x.xml"), Err(()));
        assert_eq!(java_uri_scheme("file:///a/b%20c.xml"), Ok(Some("file".into())));
        assert_eq!(java_uri_scheme("file:/a/b c.xml"), Err(()));
        assert_eq!(java_uri_scheme("a%2"), Err(()));
    }

    #[test]
    fn profile_selection() {
        let xml = r#"<profiles version="13"><profile kind="CodeFormatterProfile" name="GoogleStyle" version="23"><setting id="a" value="1"/></profile></profiles>"#;
        assert_eq!(read_settings_from_stream(xml, Some("GoogleStyle")).unwrap().get("a").map(String::as_str), Some("1"));
        assert_eq!(read_settings_from_stream(xml, Some("")).unwrap().get("a").map(String::as_str), Some("1"));
        assert!(read_settings_from_stream(xml, Some("Invalid")).unwrap().is_empty());
    }

    #[test]
    fn nested_and_flat_keys() {
        let v = serde_json::json!({ "java": { "format": { "enabled": false, "settings": { "url": null } } }, "java.format.comments.enabled": "false" });
        let mut s = FormatSettings { settings_url: Some("x".into()), ..Default::default() };
        s.update_from(&v);
        assert!(!s.enabled);
        assert!(!s.comments_enabled);
        assert_eq!(s.settings_url, None);
        assert!(!s.on_type_enabled);
    }
}
