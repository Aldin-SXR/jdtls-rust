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
use crate::features::hover::{self, Element};
use serde_json::{json, Value};
use tower_lsp::lsp_types::{Documentation, MarkupContent, MarkupKind, Url};

/// `CompletionResolveHandler.VALUE` / `DEFAULT`
const VALUE: &str = "Value: ";
const DEFAULT: &str = "Default: ";

fn data_str(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string())))
}

/// `completionItem/resolve`.
pub async fn resolve(env: &Env, mut item: Item) -> tower_lsp::jsonrpc::Result<Item> {
    let data = item.data.take();
    let Some(data) = data else { return Ok(item) };
    let kind_ok = item.kind.is_some_and(supported_kind);
    let (Some(rid), Some(pid)) = (data_str(&data, DATA_FIELD_REQUEST_ID), data_str(&data, DATA_FIELD_PROPOSAL_ID)) else { return Ok(item) };
    if !kind_ok {
        return Ok(item);
    }
    let (Ok(rid), Ok(pid)) = (rid.parse::<u64>(), pid.parse::<usize>()) else {
        return Err(super::protocol_error("Invalid completion proposal"));
    };
    let response = handler::get(rid).ok_or_else(|| super::protocol_error("Invalid completion proposal"))?;
    let stored = response.proposals.get(pid).cloned().ok_or_else(|| super::protocol_error("Invalid completion proposal"))?;
    let prefs = Prefs::load();
    let client = Client::load();
    let Ok(uri) = Url::parse(&response.uri) else { return Ok(item) };
    let (unit, ctx) = handler::unit_info(env, &uri).await.ok_or_else(||
        super::protocol_error(&format!("Unable to match Compilation Unit from {} ", response.uri)))?;

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
        return Ok(item);
    }
    let StoredProposal::Jdt(mut proposal) = stored else { return Ok(item) };

    if client.resolve_additional_text_edits() {
        // ContextSensitiveImportRewriteContext over the unit's AST (jdt.ls
        // uses it when the shared AST is available).
        let mut context_types = match crate::semantic_ast::fetch_with(&env.dispatcher, uri.as_str(), ctx.clone()).await {
            Ok(ast) => super::import_context::ImportContext::collect(ast.root(), response.offset),
            Err(_) => super::import_context::ImportContext::default(),
        };
        context_types.package_types = response.container_types.get(&unit.cu.package_name).cloned();
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
        return Ok(item);
    }
    if prefs.guess_mode == GuessMode::Off || prefs.collapse {
        if let Some(r) = required_type_proposal(&proposal) {
            proposal = r.clone();
        }
    }
    let Some(mut query) = member_query(&proposal) else { return Ok(item) };
    query["op"] = json!("memberElement");
    let ws = env.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone();
    query["sourceAttachments"] = json!(crate::features::navigation::source_attachments(&ws));
    let project = ws
        .project_for_uri(&uri)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| crate::project::DEFAULT_PROJECT_NAME.to_owned());
    drop(ws);
    let answer = match env.dispatcher.code_assist(&ctx, uri.as_str(), response.offset, query).await {
        Ok(v) => v,
        Err(_) => return Ok(item),
    };
    // member == null || !member.exists()
    if answer["status"] != "ok" {
        return Ok(item);
    }
    let Ok(element) = serde_json::from_value::<Element>(answer["element"].clone()) else { return Ok(item) };
    let markdown = client.documentation_markdown;
    let mut javadoc = {
        let config = env.config.read().await;
        hover::completion_documentation(&env.dispatcher, &config, &project, &element, markdown).await
    };
    // JDTUtils.getConstantValue
    if proposal.kind == kind::FIELD_REF {
        let constant = element.field.as_ref().filter(|f| f.static_final && !f.is_enum_constant).and_then(|f| f.constant.as_ref());
        if let Some(c) = constant {
            let v = hover::constant_value(c);
            javadoc = Some(if markdown {
                format!("{}\n\n{VALUE}{v}", javadoc.unwrap_or_default())
            } else {
                format!("{}{VALUE}{v}", javadoc.unwrap_or_default())
            });
        }
    }
    // JDTUtils.getAnnotationMemberDefaultValue
    if proposal.kind == kind::METHOD_REF || proposal.kind == kind::ANNOTATION_ATTRIBUTE_REF {
        if let Some(dv) = &element.default_value {
            let v = hover::annotation_value(dv);
            javadoc = Some(if markdown {
                format!("{}\n\n{DEFAULT}{v}", javadoc.unwrap_or_default())
            } else {
                format!("{}{DEFAULT}{v}", javadoc.unwrap_or_default())
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
    Ok(item)
}

/// The member a proposal refers to (type, module, or method/field of a type).
fn member_query(p: &Proposal) -> Option<Value> {
    if p.kind == kind::TYPE_REF {
        let s = p.signature.as_deref()?;
        let fqn = sig::strip_signature_to_fqn(s).ok()?;
        return Some(json!({ "type": fqn }));
    }
    if p.kind == kind::MODULE_REF || p.kind == kind::MODULE_DECLARATION {
        // IJavaProject.findModule + module Javadoc: not supported by the
        // bridge's hover element data yet.
        return None;
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
    let _ = have_binding;
    Some(json!({ "type": type_name, "name": name, "params": params }))
}
