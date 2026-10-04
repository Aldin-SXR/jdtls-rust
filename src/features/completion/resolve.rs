//! Port of jdt.ls `CompletionResolveHandler`.

use super::description::required_type_proposal;
use super::handler::{self, StoredProposal};
use super::item::Item;
use super::prefs::{Client, GuessMode, Prefs};
use super::proposal::{kind, Proposal};
use super::replacement::ReplacementProvider;
use super::requestor::{supported_kind, DATA_FIELD_PROPOSAL_ID, DATA_FIELD_REQUEST_ID};
use super::signature as sig;
use super::snippets::{beautify_document, evaluate, set_text_edit};
use super::Env;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_lsp::lsp_types::{Documentation, MarkupContent, MarkupKind, Url};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct MemberDoc {
    found: bool,
    javadoc: Option<String>,
    constant_value: Option<String>,
    default_value: Option<String>,
}

fn data_str(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string())))
}

/// `completionItem/resolve`.
pub async fn resolve(env: &Env, mut item: Item) -> Item {
    let data = item.data.take();
    let Some(data) = data else { return item };
    let kind_ok = item.kind.is_some_and(supported_kind);
    let (Some(rid), Some(pid)) = (data_str(&data, DATA_FIELD_REQUEST_ID), data_str(&data, DATA_FIELD_PROPOSAL_ID)) else { return item };
    if !kind_ok {
        return item;
    }
    let (Ok(rid), Ok(pid)) = (rid.parse::<u64>(), pid.parse::<usize>()) else { return item };
    let Some(response) = handler::get(rid) else { return item };
    let Some(stored) = response.proposals.get(pid).cloned() else { return item };
    let prefs = Prefs::load();
    let client = Client::load();
    let Ok(uri) = Url::parse(&response.uri) else { return item };
    let Some((unit, ctx)) = handler::unit_info(env, &uri).await else { return item };

    if item.kind == Some(super::item::item_kind::SNIPPET) {
        if let StoredProposal::Snippet(sp) = &stored {
            let scope = match &response.template_scope {
                Some(s) => Some(s.clone()),
                None => handler::template_scope(env, &ctx, &unit, &response.context).await,
            };
            let content = scope.as_ref().and_then(|s| evaluate(&sp.template.pattern, s));
            if client.resolve_documentation() {
                item.documentation = Some(beautify_document(content.as_deref().unwrap_or("null"), client.documentation_markdown));
            }
            if prefs.lazy_resolve_text_edit {
                set_text_edit(&response.context, &unit.doc, &mut item, content.unwrap_or_else(|| "null".into()));
            }
        }
        item.data = None;
        return item;
    }
    let StoredProposal::Jdt(mut proposal) = stored else { return item };

    if client.resolve_additional_text_edits() {
        let context_types: Vec<(String, String)> = Vec::new();
        let provider = ReplacementProvider {
            doc: &unit.doc,
            cu: unit.cu.clone(),
            context: &response.context,
            offset: response.offset,
            prefs: &prefs,
            client: &client,
            resolving: true,
            source_level: &response.source_level,
            container_types: &response.container_types,
            visible_elements: &response.visible_elements,
            stubs: &response.stubs,
            line_delimiter: unit.line_delimiter(),
            blank_lines_between_import_groups: unit.blank_lines_between_import_groups(),
            space_before_semicolon: unit.space_before_semicolon(),
            is_package_info: unit.file_name == "package-info.java",
            main_type_name: unit.unit_name.clone(),
            context_types: Some(&context_types),
        };
        provider.update_replacement(&proposal, &mut item, '\0');
    }
    if !client.resolve_documentation() {
        return item;
    }
    if prefs.guess_mode == GuessMode::Off || prefs.collapse {
        if let Some(r) = required_type_proposal(&proposal) {
            proposal = r.clone();
        }
    }
    let query = member_query(&proposal);
    let Some(mut query) = query else { return item };
    query["op"] = json!("memberDoc");
    query["markdown"] = json!(client.documentation_markdown);
    let tests = handler::test_uris(env, &ctx);
    query["testUris"] = json!(tests);
    let doc: MemberDoc = match env.dispatcher.code_assist(&ctx, uri.as_str(), response.offset, query).await {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(_) => return item,
    };
    if !doc.found {
        return item;
    }
    let markdown = client.documentation_markdown;
    let mut javadoc = doc.javadoc.as_deref().and_then(|raw| {
        if markdown {
            super::javadoc_text::markdown(raw)
        } else {
            super::javadoc_text::plain_text(raw)
        }
    });
    if proposal.kind == kind::FIELD_REF {
        if let Some(v) = &doc.constant_value {
            javadoc = Some(if markdown {
                format!("{}\n\nValue: {v}", javadoc.unwrap_or_default())
            } else {
                format!("{}Value: {v}", javadoc.unwrap_or_default())
            });
        }
    }
    if proposal.kind == kind::METHOD_REF || proposal.kind == kind::ANNOTATION_ATTRIBUTE_REF {
        if let Some(v) = &doc.default_value {
            javadoc = Some(if markdown {
                format!("{}\n\nDefault: {v}", javadoc.unwrap_or_default())
            } else {
                format!("{}Default: {v}", javadoc.unwrap_or_default())
            });
        }
    }
    if let Some(j) = javadoc {
        item.documentation = Some(if markdown {
            Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: j })
        } else {
            Documentation::String(j)
        });
    }
    item
}

/// The member a proposal refers to (type, module, or method/field of a type).
fn member_query(p: &Proposal) -> Option<Value> {
    if p.kind == kind::TYPE_REF {
        let s = p.signature.as_deref()?;
        let fqn = sig::strip_signature_to_fqn(s).ok()?;
        return Some(json!({ "type": fqn }));
    }
    if p.kind == kind::MODULE_REF || p.kind == kind::MODULE_DECLARATION {
        let name = p.declaration_signature.clone().filter(|d| !d.is_empty()).or_else(|| p.completion.clone())?;
        return Some(json!({ "module": name }));
    }
    let decl = p.declaration_signature.as_deref()?;
    let type_name = sig::strip_signature_to_fqn(decl).unwrap_or_else(|_| decl.to_owned());
    let mut name = p.name.clone()?;
    let mut params: Vec<String> = Vec::new();
    let mut have_binding = false;
    if let Some(bs) = p.binding_signature.as_deref() {
        have_binding = true;
        params = sig::get_parameter_types(&sig::fix83600(bs)).unwrap_or_default().iter().map(|t| sig::get_lower_bound(t)).collect();
    } else if p.kind == kind::METHOD_REF || p.kind == kind::FIELD_REF {
        if let Some(i) = name.rfind('.') {
            name = name[i + 1..].to_owned();
            if p.kind == kind::METHOD_REF {
                params = sig::get_parameter_types(&sig::fix83600(p.signature())).unwrap_or_default().iter().map(|t| sig::get_lower_bound(t)).collect();
            }
        }
    }
    Some(json!({
        "type": type_name,
        "name": name,
        "params": params,
        "hasBinding": have_binding,
        "field": p.kind == kind::FIELD_REF,
        "constant": p.kind == kind::FIELD_REF,
        "annotationDefault": p.kind == kind::METHOD_REF || p.kind == kind::ANNOTATION_ATTRIBUTE_REF,
    }))
}
