//! Port of jdt.ls `ChainCompletionProposalComputer`: completion proposals
//! for call chains (`Collections.emptyList()`, `stream.toList()`) that
//! produce the expected type.
//!
//! The `ChainFinder` search over the Java model runs in the bridge
//! (`ChainCompletionService`); this module selects the entry points, reads
//! the `recommenders.chain.*` preferences and turns the chains into
//! `CompletionProposal`s (`createCompletionProposal`).

use super::handler::UnitInfo;
use super::proposal::{kind, Context, Proposal};
use super::Env;
use crate::analysis::dispatcher::RequestContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tower_lsp::lsp_types::Url;
use tracing::warn;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChainResult {
    perform: bool,
    first_type_offset: i32,
    chains: Vec<Chain>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Chain {
    expected_dimensions: i32,
    elements: Vec<ChainElement>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChainElement {
    kind: String,
    name: String,
    declaring_type: Option<String>,
    type_signature: Option<String>,
    return_type: Option<String>,
    parameter_types: Option<Vec<String>>,
    parameter_names: Option<Vec<String>>,
    dimension: i32,
    requires_this: bool,
}

/// `JavaManipulation.getPreference("recommenders.chain.*", project)`: the
/// project's `org.eclipse.jdt.ls.core` node, then jdt.ls's defaults
/// (`PreferenceManager.initialize`).
fn chain_preferences(env: &Env, uri: &Url) -> BTreeMap<String, String> {
    let mut prefs: BTreeMap<String, String> = [
        ("recommenders.chain.min_chain_length", "2"),
        ("recommenders.chain.max_chain_length", "3"),
        ("recommenders.chain.max_chains", "20"),
        ("recommenders.chain.timeout", "1"),
        ("recommenders.chain.ignore_types", ""),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let location = crate::project::uri_to_path(uri).and_then(|path| {
        let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner());
        ws.project_for_path(&path).map(|p| p.location.clone())
    });
    if let Some(project) = crate::project::prefs::read_properties(&location.unwrap_or_default().join(".settings/org.eclipse.jdt.ls.core.prefs")) {
        for (k, v) in project {
            if k.starts_with("recommenders.chain.") {
                prefs.insert(k, v);
            }
        }
    }
    prefs
}

/// `ChainCompletionProposalComputer.computeCompletionProposals`: the chain
/// proposals to add to the collector, which holds `accepted`.
pub async fn compute(
    env: &Env,
    ctx: &RequestContext,
    unit: &UnitInfo,
    offset: usize,
    context: &Context,
    accepted: &[Proposal],
    test_uris: &[String],
    unit_package: Option<String>,
    snippet_string_supported: bool,
) -> Vec<Proposal> {
    // shouldPerformCompletionOnExpectedType: the cheap checks first
    if context.token.as_deref() == Some("new") || context.token_location == super::proposal::tl::CONSTRUCTOR_START {
        return Vec::new();
    }
    if context.expected_types_signatures.as_ref().is_none_or(|s| s.is_empty()) {
        return Vec::new();
    }
    let prefs = chain_preferences(env, &unit.uri);
    let int = |k: &str, d: i64| prefs.get(k).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(d);
    let entrypoints: Vec<Value> = accepted
        .iter()
        .filter(|p| matches!(p.kind, kind::FIELD_REF | kind::METHOD_REF | kind::ANNOTATION_ATTRIBUTE_REF))
        .map(|p| {
            json!({
                "kind": p.kind,
                "declarationSignature": p.declaration_signature,
                "name": p.name,
                // resolveMethod uses the original method's signature
                "signature": p.binding_signature.as_ref().or(p.signature.as_ref()),
            })
        })
        .collect();
    let query = json!({
        "op": "chains",
        "uriOffset": offset,
        "testUris": test_uris,
        "unitPackage": unit_package,
        "entrypoints": entrypoints,
        "maxChains": int("recommenders.chain.max_chains", 20),
        "minDepth": int("recommenders.chain.min_chain_length", 2),
        "maxDepth": int("recommenders.chain.max_chain_length", 3),
        "timeout": int("recommenders.chain.timeout", 1),
        "ignoreTypes": prefs.get("recommenders.chain.ignore_types").cloned().unwrap_or_default(),
    });
    let raw = match env.dispatcher.code_assist(ctx, unit.uri.as_str(), offset, query).await {
        Ok(v) => v,
        Err(e) => {
            warn!("chain completion failed: {e}");
            return Vec::new();
        }
    };
    let result: ChainResult = serde_json::from_value(raw).unwrap_or_default();
    if !result.perform {
        return Vec::new();
    }
    result
        .chains
        .iter()
        .filter_map(|chain| create_completion_proposal(chain, context, result.first_type_offset, snippet_string_supported))
        .collect()
}

fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}

/// `createCompletionProposal`.
fn create_completion_proposal(chain: &Chain, context: &Context, first_type_offset: i32, snippets: bool) -> Option<Proposal> {
    let edge = chain.elements.last()?;
    let root = chain.elements.first()?;
    let (insert_text, display_text) = create_chain_text(chain, chain.expected_dimensions, snippets);
    let has_token = context.token.as_deref().is_some_and(|t| !t.is_empty());
    let declaration_signature = edge.declaring_type.as_ref().map(|t| format!("L{t};"));
    let mut cp = match edge.kind.as_str() {
        "FIELD" => Proposal {
            kind: kind::FIELD_REF,
            name: Some(display_text),
            signature: edge.type_signature.clone(),
            declaration_signature,
            ..Default::default()
        },
        "METHOD" => {
            let params: String = edge.parameter_types.clone().unwrap_or_default().concat();
            Proposal {
                kind: kind::METHOD_REF,
                name: Some(display_text),
                signature: Some(format!("({params}){}", edge.return_type.clone().unwrap_or_default())),
                declaration_signature,
                parameter_names: Some(edge.parameter_names.clone().unwrap_or_default()),
                ..Default::default()
            }
        }
        _ => return None,
    };
    cp.relevance = 1;
    cp.completion_location = context.offset;
    cp.replace_start = context.offset;
    cp.replace_end = context.offset + utf16_len(&insert_text);
    cp.completion = Some(insert_text);
    if has_token {
        cp.token_start = context.token_start;
        cp.token_end = context.token_end;
        // set replace rage if applicable
        cp.replace_start = context.token_start;
        cp.replace_end = context.token_end;
    }
    if root.kind == "TYPE" {
        let fqn = root.declaring_type.clone().unwrap_or_default();
        cp.required_proposals = Some(vec![Proposal {
            kind: kind::TYPE_REF,
            relevance: 1,
            completion_location: context.offset,
            signature: Some(format!("L{fqn};")),
            completion: Some(fqn),
            replace_start: first_type_offset,
            replace_end: first_type_offset,
            ..Default::default()
        }]);
    }
    Some(cp)
}

/// `createChainText`: (insert text, display text).
fn create_chain_text(chain: &Chain, expected_dimension: i32, snippets: bool) -> (String, String) {
    let mut insert = String::new();
    let mut display = String::new();
    for edge in &chain.elements {
        match edge.kind.as_str() {
            "FIELD" | "TYPE" | "LOCAL_VARIABLE" => {
                append_variable_string(edge, &mut insert);
                append_variable_string(edge, &mut display);
            }
            "METHOD" => {
                insert.push_str(&edge.name);
                insert.push('(');
                if snippets {
                    insert.push_str(&edge.parameter_names.clone().unwrap_or_default().join(", "));
                }
                insert.push(')');
                display.push_str(&edge.name);
            }
            _ => {}
        }
        append_array_dimensions(&mut insert, edge.dimension, expected_dimension, snippets);
        insert.push('.');
        append_array_dimensions(&mut display, edge.dimension, expected_dimension, false);
        display.push('.');
    }
    insert.pop();
    display.pop();
    (insert, display)
}

fn append_variable_string(edge: &ChainElement, sb: &mut String) {
    if edge.requires_this && sb.is_empty() {
        sb.push_str("this.");
    }
    sb.push_str(&edge.name);
}

fn append_array_dimensions(sb: &mut String, dimension: i32, expected_dimension: i32, snippets: bool) {
    let mut i = dimension;
    while i > expected_dimension {
        i -= 1;
        sb.push('[');
        if snippets {
            sb.push_str(&format!("${{{dimension}:i}}"));
        }
        sb.push(']');
    }
}
