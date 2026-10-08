//! Port of jdt.ls `ConfigurationHandler`: the client's per-document
//! formatting settings (`workspace/configuration`), which
//! `CodeActionHandler` stores as the working copy's custom options
//! (`ICompilationUnit.setOptions`) for the edits code actions compute.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use serde_json::Value;
use tower_lsp::lsp_types::{ConfigurationItem, Url};
use tower_lsp::Client;

/// `Preferences.JAVA_CONFIGURATION_TABSIZE`.
pub const JAVA_CONFIGURATION_TABSIZE: &str = "java.format.tabSize";
/// `Preferences.JAVA_CONFIGURATION_INSERTSPACES`.
pub const JAVA_CONFIGURATION_INSERTSPACES: &str = "java.format.insertSpaces";

const FORMATTER_TAB_SIZE: &str = "org.eclipse.jdt.core.formatter.tabulation.size";
const FORMATTER_TAB_CHAR: &str = "org.eclipse.jdt.core.formatter.tabulation.char";

/// `ConfigurationHandler.getFormattingOptions(uri)`: `None` when the client
/// doesn't support `workspace/configuration`; otherwise the answered
/// settings (`null` for a setting the client doesn't have).
pub async fn get_formatting_options(client: &Client, uri: &str) -> Option<HashMap<String, Value>> {
    if !crate::features::client_caps::workspace_configuration() {
        return None;
    }
    let setting_keys = [JAVA_CONFIGURATION_TABSIZE, JAVA_CONFIGURATION_INSERTSPACES];
    let scope_uri = Url::parse(uri).ok();
    let items = setting_keys
        .iter()
        .map(|k| ConfigurationItem { scope_uri: scope_uri.clone(), section: Some((*k).to_owned()) })
        .collect();
    let response = client.configuration(items).await.unwrap_or_default();
    Some(setting_keys.iter().zip(response).map(|(k, v)| ((*k).to_owned(), v)).collect())
}

/// The custom options of working copies (`ICompilationUnit.setOptions`).
static UNIT_OPTIONS: Mutex<Option<HashMap<Url, BTreeMap<String, String>>>> = Mutex::new(None);

/// `cu.setOptions(options)`: replaces the unit's custom options.
pub fn set_unit_options(uri: &Url, options: BTreeMap<String, String>) {
    UNIT_OPTIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uri.clone(), options);
}

/// The custom options of the working copy of `uri`.
pub fn unit_options(uri: &Url) -> Option<BTreeMap<String, String>> {
    UNIT_OPTIONS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(uri).cloned()
}

/// `discardWorkingCopy`: the custom options go with the working copy.
pub fn discard_unit_options(uri: &Url) {
    if let Some(map) = UNIT_OPTIONS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(uri);
    }
}

/// `CodeActionHandler.getCodeActionCommands`: the client's tab size and
/// insert-spaces settings become the unit's formatter options.
pub async fn apply_client_formatting_options(client: &Client, uri: &Url) {
    let Some(formatting_options) = get_formatting_options(client, uri.as_str()).await else { return };
    if formatting_options.is_empty() {
        return;
    }
    let mut custom_options = BTreeMap::new();
    if let Some(tab_size_value) = formatting_options.get(JAVA_CONFIGURATION_TABSIZE).filter(|v| !v.is_null()) {
        let text = match tab_size_value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if let Ok(tab_size) = text.parse::<i32>() {
            if tab_size > 0 {
                custom_options.insert(FORMATTER_TAB_SIZE.to_owned(), tab_size.to_string());
            }
        }
    }
    if let Some(insert_spaces_value) = formatting_options.get(JAVA_CONFIGURATION_INSERTSPACES).filter(|v| !v.is_null()) {
        // `Boolean.parseBoolean(String.valueOf(value))`.
        let insert_spaces = match insert_spaces_value {
            Value::Bool(b) => *b,
            Value::String(s) => s.eq_ignore_ascii_case("true"),
            _ => false,
        };
        custom_options.insert(FORMATTER_TAB_CHAR.to_owned(), if insert_spaces { "space" } else { "tab" }.to_owned());
    }
    if !custom_options.is_empty() {
        set_unit_options(uri, custom_options);
    }
}

#[cfg(test)]
mod configuration_handler_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.handlers.ConfigurationHandlerTest`.

    use super::*;

    #[tokio::test]
    async fn test_get_configuration_when_it_is_not_supported() {
        // No client capabilities: `workspace.configuration` isn't supported.
        let (client, _socket, _service) = crate::features::client_connection::test_support::connect().await;
        assert!(get_formatting_options(&client, "fakeUri").await.is_none());
    }
}
