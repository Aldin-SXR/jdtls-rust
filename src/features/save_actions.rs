//! Rust port of SaveActionHandler. Save requests return edits; applying them
//! and saving the editor buffer remain the client's responsibility.

use super::{cleanup, formatting::FormatEnv, organize_imports::operation, preferences};
use std::collections::{BTreeMap, HashSet};
use tower_lsp::lsp_types::{TextEdit, Url, WorkspaceEdit};

const SAVE_PARTICIPANT: &str =
    "editor_save_participant_org.eclipse.jdt.ui.postsavelistener.cleanup";

fn project_preferences(env: &FormatEnv<'_>, uri: &Url) -> BTreeMap<String, String> {
    let workspace = env
        .dispatcher
        .workspace
        .read()
        .unwrap_or_else(|e| e.into_inner());
    workspace
        .project_for_uri(uri)
        .and_then(|project| {
            crate::project::prefs::read_properties(
                &project.location.join(".settings/org.eclipse.jdt.ui.prefs"),
            )
        })
        .unwrap_or_default()
}

fn enabled(prefs: &BTreeMap<String, String>, key: &str) -> bool {
    prefs
        .get(key)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn cleanup_ids(prefs: &BTreeMap<String, String>, on_save: bool) -> Vec<String> {
    let internal = preferences::extended_capability("canUseInternalSettings");
    let ids = if internal {
        if enabled(prefs, SAVE_PARTICIPANT) {
            prefs
                .iter()
                .filter(|(key, _)| enabled(prefs, key))
                .filter_map(|(key, _)| key.strip_prefix("sp_").map(str::to_owned))
                .collect()
        } else {
            Vec::new()
        }
    } else if !on_save || preferences::get_bool("java.saveActions.cleanup").unwrap_or(false) {
        preferences::cleanup_actions()
    } else {
        Vec::new()
    };
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| id != "renameFileToType" && seen.insert(id.clone()))
        .collect()
}

pub async fn will_save(env: &FormatEnv<'_>, uri: &Url) -> Vec<TextEdit> {
    let prefs = project_preferences(env, uri);
    let options = env.jdt_options(Some(uri)).await;
    let mut edits = Vec::new();
    if preferences::get_bool("java.saveActions.organizeImports").unwrap_or(false)
        || (preferences::extended_capability("canUseInternalSettings")
            && enabled(&prefs, "sp_cleanup.organize_imports"))
    {
        match operation::organize(env.dispatcher, uri, &options).await {
            Ok(Some(change)) => edits.extend(crate::correction::edit::tree_to_text_edits(
                &change.ast.source,
                change.edits.as_ref().expect("import edits"),
            )),
            Ok(None) => {}
            Err(error) => tracing::warn!(%uri, %error, "Save action organize imports failed"),
        }
    }
    // Like Eclipse, cleanups run against the original working copy. The registry
    // composes its own operations before returning one full-document edit.
    edits.extend(cleanup::edits(env, uri, &cleanup_ids(&prefs, true)).await);
    edits
}

pub async fn manual_cleanup(env: &FormatEnv<'_>, uri: &Url) -> WorkspaceEdit {
    let prefs = project_preferences(env, uri);
    let edits = cleanup::edits(env, uri, &cleanup_ids(&prefs, false)).await;
    WorkspaceEdit {
        changes: Some([(uri.clone(), edits)].into()),
        ..Default::default()
    }
}
