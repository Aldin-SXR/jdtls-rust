//! Port of jdt.ls `JavadocTagsSubProcessor` over jdt.core.manipulation's
//! `JavadocTagsBaseSubProcessor` and `JavadocTagsSubProcessorCore`, with the
//! proposals it creates (`AddMissingJavadocTagProposalCore`,
//! `AddAllMissingJavadocTagsProposalCore`, `AddJavadocCommentProposalCore`)
//! and the `CodeGeneration` / `StubUtility` comment templates they use.

use std::collections::{BTreeMap, HashSet};

use super::edit::Env;
use super::{kind, messages, relevance, Change, Context, CuChange, ProblemLocation, Proposal};
use crate::features::accessors::templates::{expand_template, Profile};
use crate::rewrite::text_edit::{EditKind, EditTree};
use crate::rewrite::{indent, ASTRewrite, RNode};
use crate::semantic_ast::resolve::{find_parent_body_declaration, normalized_node};
use crate::semantic_ast::{modifier, problem as p, BindingRef, Node, NodeKind};

use super::local_corrections::exception_type_name as ast_type_name;

const TAG_PARAM: &str = "@param";
const TAG_RETURN: &str = "@return";
const TAG_THROWS: &str = "@throws";
const TAG_EXCEPTION: &str = "@exception";

pub(crate) fn text(rw: &mut ASTRewrite, value: &str) -> RNode {
    let node = rw.new_node(NodeKind::TextElement);
    rw.put_simple(node, "text", value);
    node
}

pub(crate) fn new_tag(rw: &mut ASTRewrite, name: &str, fragments: Vec<RNode>) -> RNode {
    let tag = rw.new_node(NodeKind::TagElement);
    rw.put_simple(tag, "tagName", name);
    rw.put_list(tag, "fragments", fragments);
    tag
}

/// `ASTNodes.getSimpleNameIdentifier(name)` of a rewrite node.
fn simple_name_identifier(rw: &ASTRewrite, name: RNode) -> String {
    let name = if rw.kind(name) == NodeKind::QualifiedName { rw.new_value(name, "name").node().unwrap_or(name) } else { name };
    rw.new_value(name, "identifier").simple().unwrap_or("").to_owned()
}

/// `JavadocTagsSubProcessorCore.getArgument(tag)` (original or new tag).
fn rw_argument(rw: &ASTRewrite, tag: RNode) -> Option<String> {
    let fragments = rw.new_value(tag, "fragments").list();
    let first = *fragments.first()?;
    let tag_name = rw.new_value(tag, "tagName").simple().map(str::to_owned);
    if rw.kind(first).is_name() {
        return Some(simple_name_identifier(rw, first));
    }
    if rw.kind(first) != NodeKind::TextElement {
        return None;
    }
    let value = rw.new_value(first, "text").simple().unwrap_or("").to_owned();
    match tag_name.as_deref() {
        Some(TAG_PARAM) => {
            if value == "<" && fragments.len() >= 3 {
                let (second, third) = (fragments[1], fragments[2]);
                if rw.kind(second).is_name() && rw.kind(third) == NodeKind::TextElement && rw.new_value(third, "text").simple() == Some(">") {
                    return Some(format!("<{}>", simple_name_identifier(rw, second)));
                }
            } else if value.starts_with('<') && value.ends_with('>') && value.encode_utf16().count() > 2 {
                return Some(value[1..value.len() - 1].to_owned());
            }
            None
        }
        Some("@uses") | Some("@provides") => Some(value.trim().to_owned()),
        _ => None,
    }
}

/// `JavadocTagsSubProcessorCore.getArgument(tag)` of an original tag.
pub(crate) fn argument(tag: Node<'_>) -> Option<String> {
    let fragments = tag.list("fragments");
    let first = fragments.first()?;
    if first.kind().is_name() {
        return Some(first.child("name").unwrap_or(*first).identifier());
    }
    if first.is(NodeKind::TextElement) && tag.simple("tagName") == Some(TAG_PARAM) {
        let value = first.simple("text")?;
        if value == "<" && fragments.len() >= 3 {
            if fragments[1].kind().is_name() && fragments[2].is(NodeKind::TextElement) && fragments[2].simple("text") == Some(">") {
                return Some(format!("<{}>", fragments[1].child("name").unwrap_or(fragments[1]).identifier()));
            }
        } else if value.starts_with('<') && value.ends_with('>') && value.encode_utf16().count() > 2 {
            return Some(value[1..value.len() - 1].into());
        }
        return None;
    }
    if first.is(NodeKind::TextElement) && matches!(tag.simple("tagName"), Some("@uses") | Some("@provides")) {
        return first.simple("text").map(|t| t.trim().to_owned());
    }
    None
}

/// `JavadocTagsSubProcessorCore.getTagRanking`.
fn rank(tag: &str) -> usize {
    let tag = if tag == TAG_EXCEPTION { TAG_THROWS } else { tag };
    ["@author", "@version", TAG_PARAM, TAG_RETURN, TAG_THROWS, "@see", "@since", "@serial", "@deprecated"].iter().position(|t| *t == tag).unwrap_or(9)
}

/// `JavadocTagsSubProcessorCore.isSameTag`.
fn is_same_tag(inserted: &str, tag: &str) -> bool {
    inserted == tag || (tag == TAG_EXCEPTION && inserted == TAG_THROWS)
}

/// `JavadocTagsSubProcessorCore.insertTag(rewriter, newElement, sameKindLeadingNames)`
/// over the rewritten tag list.
pub(crate) fn insert_tag(rw: &mut ASTRewrite, doc: RNode, tag: RNode, leading: Option<&HashSet<String>>) {
    let tags = rw.list_rewritten(doc, "tags");
    let inserted = rw.new_value(tag, "tagName").simple().unwrap_or("").to_owned();
    let ranking = rank(&inserted);
    let mut after = None;
    for &curr in tags.iter().rev() {
        let name = rw.new_value(curr, "tagName").simple().map(str::to_owned);
        let Some(name) = name else {
            after = Some(curr);
            break;
        };
        if ranking > rank(&name) {
            after = Some(curr);
            break;
        }
        if let Some(leading) = leading {
            if is_same_tag(&inserted, &name) && rw_argument(rw, curr).is_some_and(|a| leading.contains(&a)) {
                after = Some(curr);
                break;
            }
        }
    }
    match after {
        Some(after) => rw.list_insert_after(doc, "tags", tag, after),
        None => rw.list_insert_first(doc, "tags", tag),
    }
}

/// `JavadocTagsSubProcessorCore.findTag(javadoc, name, arg)`.
pub(crate) fn find_tag<'a>(javadoc: Node<'a>, name: &str, arg: Option<&str>) -> Option<Node<'a>> {
    javadoc.list("tags").into_iter().find(|t| t.simple("tagName") == Some(name) && arg.is_none_or(|arg| argument(*t).as_deref() == Some(arg)))
}

/// `JavadocTagsSubProcessorCore.findThrowsTag(javadoc, arg)`.
fn find_throws_tag<'a>(javadoc: Node<'a>, arg: &str) -> Option<Node<'a>> {
    javadoc.list("tags").into_iter().find(|t| matches!(t.simple("tagName"), Some(TAG_THROWS) | Some(TAG_EXCEPTION)) && argument(*t).as_deref() == Some(arg))
}

/// `getPreviousParamNames(params, missingNode)`.
fn previous_param_names(params: &[Node<'_>], missing: Node<'_>) -> HashSet<String> {
    params.iter().take_while(|n| n.id != missing.id).filter_map(|n| n.child("name")).map(|n| n.identifier()).collect()
}

/// `getPreviousTypeParamNames(typeParams, missingNode)`.
fn previous_type_param_names(params: &[Node<'_>], missing: Node<'_>) -> HashSet<String> {
    params.iter().take_while(|n| n.id != missing.id).filter_map(|n| n.child("name")).map(|n| format!("<{}>", n.identifier())).collect()
}

/// `getPreviousExceptionNames(list, missingNode)`.
fn previous_exception_names(list: &[Node<'_>], missing: Node<'_>) -> HashSet<String> {
    list.iter().take_while(|n| n.id != missing.id).map(|t| ast_type_name(*t, false)).collect()
}

/// `AddMissingJavadocTagProposalCore.insertMissingJavadocTag`.
fn insert_missing_javadoc_tag(rw: &mut ASTRewrite, missing: Node<'_>, body: Node<'_>) {
    let original_doc = body.child("javadoc");
    let doc = match original_doc {
        Some(doc) => RNode::Orig(doc.id),
        None => {
            let doc = rw.new_node(NodeKind::Javadoc);
            rw.set(RNode::Orig(body.id), "javadoc", Some(doc));
            doc
        }
    };
    let Some(owner) = missing.parent() else { return };
    let location = missing.location();
    let mut fragments;
    let tag;
    if owner.is(NodeKind::SingleVariableDeclaration) && location == Some("name") {
        fragments = vec![rw.new_simple_name(&missing.identifier())];
        tag = new_tag(rw, TAG_PARAM, Vec::new());
        let params = match body.kind() {
            NodeKind::MethodDeclaration => Some(body.list("parameters")),
            NodeKind::RecordDeclaration => Some(body.list("recordComponents")),
            _ => None,
        };
        if let Some(params) = params {
            let mut leading = previous_param_names(&params, owner);
            for type_param in body.list("typeParameters") {
                if let Some(name) = type_param.child("name") {
                    leading.insert(format!("<{}>", name.identifier()));
                }
            }
            insert_tag(rw, doc, tag, Some(&leading));
        }
    } else if owner.is(NodeKind::TypeParameter) && location == Some("name") {
        fragments = vec![text(rw, &format!("<{}>", missing.identifier()))];
        tag = new_tag(rw, TAG_PARAM, Vec::new());
        let leading = previous_type_param_names(&body.list("typeParameters"), owner);
        insert_tag(rw, doc, tag, Some(&leading));
    } else if location == Some("returnType2") && owner.is(NodeKind::MethodDeclaration) {
        fragments = Vec::new();
        tag = new_tag(rw, TAG_RETURN, Vec::new());
        insert_tag(rw, doc, tag, None);
    } else if location == Some("thrownExceptionTypes") && owner.is(NodeKind::MethodDeclaration) {
        fragments = vec![text(rw, &ast_type_name(missing, true))];
        tag = new_tag(rw, TAG_THROWS, Vec::new());
        let leading = previous_exception_names(&body.list("thrownExceptionTypes"), missing);
        insert_tag(rw, doc, tag, Some(&leading));
    } else {
        return;
    }
    fragments.push(text(rw, ""));
    if original_doc.is_none() {
        // otherwise the linked position spans over a line delimiter
        fragments.push(text(rw, ""));
    }
    rw.put_list(tag, "fragments", fragments);
}

/// `AddMissingJavadocTagProposalCore`.
fn add_missing_javadoc_tag_proposal(ctx: &Context, label: &str, body: Node<'_>, missing: Node<'_>, relevance: i32) -> Proposal {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    insert_missing_javadoc_tag(&mut rw, missing, body);
    Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw)
}

/// `AddAllMissingJavadocTagsProposalCore`.
fn add_all_missing_javadoc_tags_proposal(ctx: &Context, label: &str, decl: Node<'_>, relevance: i32) -> Proposal {
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    if let Some(javadoc) = decl.child("javadoc") {
        match decl.kind() {
            NodeKind::MethodDeclaration => insert_all_missing_method_tags(&mut rw, decl, javadoc),
            NodeKind::RecordDeclaration => insert_all_missing_record_type_tags(&mut rw, decl, javadoc),
            _ => insert_all_missing_type_params(&mut rw, decl, javadoc, &mut Vec::new()),
        }
    }
    Proposal::rewrite(label, kind::QUICK_FIX, relevance, rw)
}

/// The type parameter loop of `insertAllMissingMethodTags` /
/// `insertAllMissingTypeTags` / `insertAllMissingRecordTypeTags`.
fn insert_all_missing_type_params(rw: &mut ASTRewrite, decl: Node<'_>, javadoc: Node<'_>, names: &mut Vec<String>) {
    let doc = RNode::Orig(javadoc.id);
    let type_params = decl.list("typeParameters");
    for param in type_params.iter().rev() {
        let name = format!("<{}>", param.child("name").map(|n| n.identifier()).unwrap_or_default());
        if find_tag(javadoc, TAG_PARAM, Some(&name)).is_none() {
            let fragments = vec![text(rw, &name), text(rw, "")];
            let tag = new_tag(rw, TAG_PARAM, fragments);
            let leading = previous_type_param_names(&type_params, *param);
            insert_tag(rw, doc, tag, Some(&leading));
        }
        names.push(name);
    }
}

/// `AddAllMissingJavadocTagsProposalCore.insertAllMissingMethodTags`.
fn insert_all_missing_method_tags(rw: &mut ASTRewrite, method: Node<'_>, javadoc: Node<'_>) {
    let doc = RNode::Orig(javadoc.id);
    let mut type_param_names = Vec::new();
    insert_all_missing_type_params(rw, method, javadoc, &mut type_param_names);
    let params = method.list("parameters");
    for param in params.iter().rev() {
        let name = param.child("name").map(|n| n.identifier()).unwrap_or_default();
        if find_tag(javadoc, TAG_PARAM, Some(&name)).is_none() {
            let fragments = vec![rw.new_simple_name(&name), text(rw, "")];
            let tag = new_tag(rw, TAG_PARAM, fragments);
            let mut leading = previous_param_names(&params, *param);
            leading.extend(type_param_names.iter().cloned());
            insert_tag(rw, doc, tag, Some(&leading));
        }
    }
    if !method.flag("constructor") {
        let void = method.child("returnType2").is_some_and(|t| t.is(NodeKind::PrimitiveType) && t.simple("primitiveTypeCode") == Some("void"));
        if !void && find_tag(javadoc, TAG_RETURN, None).is_none() {
            let fragments = vec![text(rw, "")];
            let tag = new_tag(rw, TAG_RETURN, fragments);
            insert_tag(rw, doc, tag, None);
        }
    }
    let thrown = method.list("thrownExceptionTypes");
    for exception in thrown.iter().rev() {
        let Some(binding) = exception.type_binding().or_else(|| exception.binding()) else { continue };
        if find_throws_tag(javadoc, binding.name()).is_none() {
            let fragments = vec![text(rw, &ast_type_name(*exception, true)), text(rw, "")];
            let tag = new_tag(rw, TAG_THROWS, fragments);
            let leading = previous_exception_names(&thrown, *exception);
            insert_tag(rw, doc, tag, Some(&leading));
        }
    }
}

/// `AddAllMissingJavadocTagsProposalCore.insertAllMissingRecordTypeTags`.
fn insert_all_missing_record_type_tags(rw: &mut ASTRewrite, record: Node<'_>, javadoc: Node<'_>) {
    let doc = RNode::Orig(javadoc.id);
    let components = record.list("recordComponents");
    for component in components.iter().rev() {
        let name = component.child("name").map(|n| n.identifier()).unwrap_or_default();
        if find_tag(javadoc, TAG_PARAM, Some(&name)).is_none() {
            let fragments = vec![text(rw, &name), text(rw, "")];
            let tag = new_tag(rw, TAG_PARAM, fragments);
            let leading = previous_param_names(&components, *component);
            insert_tag(rw, doc, tag, Some(&leading));
        }
    }
    insert_all_missing_type_params(rw, record, javadoc, &mut Vec::new());
}

/// `JavadocTagsSubProcessor.getMissingJavadocTagProposals`
/// (`addMissingJavadocTagProposals(context, node, proposals)`).
pub fn missing_javadoc_tag_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(node) = problem.covering_node(ctx.ast()) else { return };
    // jdt.ls: only when the declaration already has a Javadoc comment.
    let Some(body) = find_parent_body_declaration(normalized_node(node)) else { return };
    if body.child("javadoc").is_none() {
        return;
    }
    let node = normalized_node(node);
    let Some(parent) = node.parent() else { return };
    let location = node.location();
    if parent.is(NodeKind::ModuleDeclaration) && location == Some("moduleDirectives") {
        // Unreachable in jdt.ls: a module directive has no parent body declaration.
        return;
    }
    let Some(decl) = find_parent_body_declaration(node) else { return };
    let Some(_) = decl.child("javadoc") else { return };
    let key = if location == Some("name") && parent.is(NodeKind::SingleVariableDeclaration) {
        let in_decl = parent.parent().is_some_and(|d| {
            (d.is(NodeKind::MethodDeclaration) && parent.location() == Some("parameters"))
                || (d.is(NodeKind::RecordDeclaration) && parent.location() == Some("recordComponents"))
        });
        if !in_decl {
            return; // paranoia checks
        }
        "JavadocTagsSubProcessor_addjavadoc_paramtag_description"
    } else if location == Some("name") && parent.is(NodeKind::TypeParameter) {
        let in_decl = parent.location() == Some("typeParameters")
            && parent.parent().is_some_and(|d| matches!(d.kind(), NodeKind::MethodDeclaration | NodeKind::TypeDeclaration | NodeKind::RecordDeclaration));
        if !in_decl {
            return; // paranoia checks
        }
        "JavadocTagsSubProcessor_addjavadoc_paramtag_description"
    } else if location == Some("returnType2") && parent.is(NodeKind::MethodDeclaration) {
        "JavadocTagsSubProcessor_addjavadoc_returntag_description"
    } else if location == Some("thrownExceptionTypes") && parent.is(NodeKind::MethodDeclaration) {
        "JavadocTagsSubProcessor_addjavadoc_throwstag_description"
    } else {
        return;
    };
    proposals.push(add_missing_javadoc_tag_proposal(ctx, messages::correction(key), decl, node, relevance::ADD_MISSING_TAG));
    let label = messages::correction("JavadocTagsSubProcessor_addjavadoc_allmissing_description");
    proposals.push(add_all_missing_javadoc_tags_proposal(ctx, label, decl, relevance::ADD_ALL_MISSING_TAGS));
}

/// `JavadocTagsSubProcessor.getUnusedAndUndocumentedParameterOrExceptionProposals`.
pub async fn unused_and_undocumented_parameter_or_exception_proposals(env: &Env<'_>, ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let options = env.options(&ctx.ast.uri).await;
    if options.get("org.eclipse.jdt.core.compiler.doc.comment.support").map(String::as_str) != Some("enabled") {
        return;
    }
    let type_param = problem.problem_id == p::UnusedTypeParameter;
    let parameter = type_param || problem.problem_id == p::ArgumentIsNeverUsed;
    let key = if parameter {
        "org.eclipse.jdt.core.compiler.problem.unusedParameterIncludeDocCommentReference"
    } else {
        "org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionIncludeDocCommentReference"
    };
    if options.get(key).map(String::as_str) != Some("enabled") {
        return;
    }
    let Some(mut node) = problem.covering_node(ctx.ast()) else { return };
    let Some(declaration) = find_parent_body_declaration(node).filter(|n| n.binding().is_some()) else { return };
    let key = if type_param {
        "JavadocTagsSubProcessor_document_type_parameter_description"
    } else if parameter {
        "JavadocTagsSubProcessor_document_parameter_description"
    } else {
        node = normalized_node(node);
        "JavadocTagsSubProcessor_document_exception_description"
    };
    proposals.push(add_missing_javadoc_tag_proposal(ctx, messages::correction(key), declaration, node, relevance::DOCUMENT_UNUSED_ITEM));
}

/// `JavadocTagsSubProcessor.getRemoveJavadocTagProposals`.
pub fn remove_javadoc_tag_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let mut node = problem.covering_node(ctx.ast());
    while let Some(n) = node.filter(|n| !n.is(NodeKind::TagElement)) {
        node = n.parent();
    }
    let Some(tag) = node else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    rw.remove(RNode::Orig(tag.id));
    let label = messages::correction("JavadocTagsSubProcessor_removetag_description");
    proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::REMOVE_TAG, rw));
}

/// `JavadocTagsSubProcessor.getInvalidQualificationProposals`.
pub fn invalid_qualification_proposals(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>) {
    let Some(name) = problem.covering_node(ctx.ast()).filter(|n| n.kind().is_name()) else { return };
    let Some(binding) = name.binding().filter(|b| b.is_type()) else { return };
    let mut rw = ASTRewrite::new(ctx.ast.clone());
    let replacement = rw.new_name(binding.qualified_name());
    rw.replace(RNode::Orig(name.id), Some(replacement));
    let label = messages::correction("JavadocTagsSubProcessor_qualifylinktoinner_description");
    proposals.push(Proposal::rewrite(label, kind::QUICK_FIX, relevance::QUALIFY_INNER_TYPE_NAME, rw));
}

/// `JavadocTagsSubProcessor.getMissingJavadocCommentProposals(context, coveringNode, proposals, kind)`.
pub async fn missing_javadoc_comment_proposals(env: &Env<'_>, ctx: &Context, covering: Node<'_>, kind: &str, proposals: &mut Vec<Proposal>) {
    // jdt.ls: only when the declaration has no Javadoc comment yet.
    let Some(body) = find_parent_body_declaration(normalized_node(covering)) else { return };
    if body.child("javadoc").is_some() {
        return;
    }
    let Some(declaration) = find_parent_body_declaration(covering) else { return };
    let Some(binding) = binding_of_parent_type(declaration) else { return };
    let uri = ctx.ast.uri.clone();
    let options = env.options(&uri).await;
    let profile = match tower_lsp::lsp_types::Url::parse(&uri) {
        Ok(url) => crate::features::accessors::profile(env.dispatcher, &url).await,
        Err(_) => return,
    };
    let templates = Templates { profile: &profile, options: &options, ast: &ctx.ast };
    let (key, rel, comment) = match declaration.kind() {
        NodeKind::MethodDeclaration => {
            let overridden = declaration.binding().and_then(|m| find_overridden_method(m, true));
            let comment = templates.method_comment(binding.name(), declaration, overridden);
            ("JavadocTagsSubProcessor_addjavadoc_method_description", relevance::ADD_JAVADOC_METHOD, comment)
        }
        k if k.is_abstract_type_declaration() => {
            let type_qualified_name = type_qualified_name(binding);
            let names = |list: Vec<Node<'_>>| list.iter().map(|n| n.child("name").map(|n| n.identifier()).unwrap_or_default()).collect::<Vec<_>>();
            let (type_params, params) = match k {
                NodeKind::TypeDeclaration => (names(declaration.list("typeParameters")), Vec::new()),
                NodeKind::RecordDeclaration => (names(declaration.list("typeParameters")), names(declaration.list("recordComponents"))),
                _ => (Vec::new(), Vec::new()),
            };
            let comment = templates.type_comment(&type_qualified_name, &type_params, &params);
            ("JavadocTagsSubProcessor_addjavadoc_type_description", relevance::ADD_JAVADOC_TYPE, comment)
        }
        NodeKind::FieldDeclaration => {
            let mut comment = Some("/**\n *\n */\n".to_owned());
            if let Some(fragment) = declaration.list("fragments").first() {
                let field_name = fragment.child("name").map(|n| n.identifier()).unwrap_or_default();
                comment = templates.field_comment(binding.name(), &field_name);
            }
            ("JavadocTagsSubProcessor_addjavadoc_field_description", relevance::ADD_JAVADOC_FIELD, comment)
        }
        NodeKind::EnumConstantDeclaration => {
            let id = declaration.child("name").map(|n| n.identifier()).unwrap_or_default();
            let comment = templates.field_comment(binding.name(), &id);
            ("JavadocTagsSubProcessor_addjavadoc_enumconst_description", relevance::ADD_JAVADOC_ENUM, comment)
        }
        _ => return,
    };
    let Some(comment) = comment else { return };
    proposals.push(add_javadoc_comment_proposal(ctx, &options, messages::correction(key), kind, rel, declaration.start(), &comment));
}

/// `AddJavadocCommentProposalCore.addEdits`.
fn add_javadoc_comment_proposal(ctx: &Context, options: &BTreeMap<String, String>, label: &str, kind: &str, relevance: i32, insert_position: usize, comment: &str) -> Proposal {
    let ast = &ctx.ast;
    let line_delimiter = crate::rewrite::analyzer::default_line_delimiter(&ast.source);
    let line = ast.line_of(insert_position);
    let start = ast.line_start(line);
    let mut end = if line + 1 < ast.line_count() { ast.line_start(line + 1) } else { ast.source.len() };
    while end > start && indent::is_line_delimiter_char(ast.source[end - 1]) {
        end -= 1;
    }
    let line_content = indent::from_u16(&ast.source[start..end]);
    let (tab_width, indent_width) = (indent::tab_width(options), indent::indent_width(options));
    let indent_string = indent::extract_indent_string(&line_content, tab_width, indent_width);
    let str = indent::change_indent(comment, 0, tab_width, indent_width, &indent_string, &line_delimiter);
    let mut tree = EditTree::new();
    let mut insert = |text: &str| {
        let e = tree.new_edit(insert_position as i32, 0, EditKind::Insert(text.to_owned()));
        let _ = tree.add_child(EditTree::ROOT, e);
    };
    insert(&str);
    if !comment.ends_with('\n') {
        insert(&line_delimiter);
        insert(&indent_string);
    }
    Proposal::new(label, kind, relevance, Change::Cu(vec![CuChange::edits(ast.clone(), tree)]))
}

/// `Bindings.getBindingOfParentType(node)`.
fn binding_of_parent_type(node: Node<'_>) -> Option<BindingRef<'_>> {
    let mut n = Some(node);
    while let Some(x) = n {
        if x.kind().is_abstract_type_declaration() || x.is(NodeKind::AnonymousClassDeclaration) {
            return x.binding();
        }
        n = x.parent();
    }
    None
}

/// `Bindings.getTypeQualifiedName(type)`.
fn type_qualified_name(binding: BindingRef<'_>) -> String {
    fn create_name(t: BindingRef<'_>, out: &mut Vec<String>) {
        let mut base = t;
        while base.is_array() {
            match base.element_type() {
                Some(e) if e.id != base.id => base = e,
                _ => break,
            }
        }
        if !base.is_primitive() && !base.is_null_type() {
            if let Some(declaring) = base.declaring_class() {
                create_name(declaring, out);
            }
        }
        if !base.is_anonymous() {
            out.push(t.name().to_owned());
        } else {
            out.push("$local$".to_owned());
        }
    }
    let mut out = Vec::new();
    create_name(binding, &mut out);
    out.join(".")
}

/// `Bindings.findOverriddenMethod(overriding, testVisibility)`.
fn find_overridden_method(overriding: BindingRef<'_>, test_visibility: bool) -> Option<BindingRef<'_>> {
    fn in_hierarchy<'a>(t: BindingRef<'a>, method: BindingRef<'a>, seen: &mut HashSet<crate::semantic_ast::BindingId>) -> Option<BindingRef<'a>> {
        if !seen.insert(t.id) {
            return None;
        }
        if let Some(found) = t.declared_methods().unwrap_or_default().into_iter().find(|m| method.name() == m.name() && method.data().method_subsignatures.contains(&m.id)) {
            return Some(found);
        }
        if let Some(found) = t.superclass().and_then(|s| in_hierarchy(s, method, seen)) {
            return Some(found);
        }
        t.interfaces().into_iter().find_map(|i| in_hierarchy(i, method, seen))
    }
    let visible = |res: BindingRef<'_>| {
        if !test_visibility {
            return true;
        }
        // `isVisibleInHierarchy(member, pack)`.
        let flags = res.modifiers();
        let declaring = res.declaring_class();
        if flags & (modifier::PUBLIC | modifier::PROTECTED) != 0 || declaring.is_some_and(|d| d.is_interface()) {
            true
        } else if flags & modifier::PRIVATE != 0 {
            false
        } else {
            declaring.is_some_and(|d| d.package_name() == overriding.declaring_class().and_then(|c| c.package_name()))
        }
    };
    let modifiers = overriding.modifiers();
    if modifiers & (modifier::PRIVATE | modifier::STATIC) != 0 || overriding.is_constructor() {
        return None;
    }
    let typ = overriding.declaring_class()?;
    if let Some(superclass) = typ.superclass() {
        if let Some(res) = in_hierarchy(superclass, overriding, &mut HashSet::new()) {
            if res.modifiers() & modifier::PRIVATE == 0 && visible(res) {
                return Some(res);
            }
        }
    }
    for interface in typ.interfaces() {
        if let Some(res) = in_hierarchy(interface, overriding, &mut HashSet::new()) {
            if visible(res) {
                return Some(res);
            }
        }
    }
    None
}

/// `Signature.getQualifier(name)`.
fn signature_qualifier(name: &str) -> &str {
    let limit = name.find('<').unwrap_or(name.len());
    match name[..limit].rfind('.') {
        Some(dot) => &name[..dot],
        None => "",
    }
}

/// `Signature.getSimpleName(name)` for names without type arguments.
fn signature_simple_name(name: &str) -> &str {
    if name.contains('<') {
        return name;
    }
    name.rsplit('.').next().unwrap_or(name)
}

/// jdt.ls' default code templates (`CodeTemplatePreferences`).
fn default_template(key: &str) -> Option<&'static str> {
    Some(match key {
        "fieldcomment" => "/**\n *\n */",
        "constructorcomment" => "/**\n * ${tags}\n */",
        "delegatecomment" => "/**\n * ${tags}\n * ${see_to_target}\n */\n",
        "overridecomment" => "",
        "methodcomment" => "/**\n * ${tags}\n */\n",
        _ => return None,
    })
}

/// The marker standing for `${tags}` (`TagsVariableResolver` resolves to `@`).
const TAGS_MARKER: &str = "\u{e000}tags\u{e001}";

/// `CodeTemplateContext` over the unit for the comment templates.
struct Templates<'a> {
    profile: &'a Profile,
    options: &'a BTreeMap<String, String>,
    ast: &'a std::sync::Arc<crate::semantic_ast::Ast>,
}

impl Templates<'_> {
    /// `StubUtility.useMarkdown(project)`.
    fn use_markdown(&self) -> bool {
        self.profile.use_markdown && self.options.get("org.eclipse.jdt.core.compiler.compliance").and_then(|s| s.parse::<f32>().ok()).is_some_and(|n| n >= 23.0)
    }

    /// `StubUtility.getCodeTemplate(id, project)`.
    fn template(&self, key: &str) -> Option<String> {
        if let Some(t) = self.profile.project_template(key) {
            return Some(t.to_owned());
        }
        if key == "typecomment" {
            return crate::features::preferences::code_template(&format!("org.eclipse.jdt.ui.text.codetemplates.{key}"));
        }
        default_template(key).map(str::to_owned)
    }

    /// Evaluates `template` with the unit variables, the given variables
    /// and `${tags}` as [`TAGS_MARKER`] (when `tags` is set).
    fn evaluate(&self, template: &str, vars: &[(&str, &str)], tags: bool) -> Option<String> {
        let file_name = tower_lsp::lsp_types::Url::parse(&self.ast.uri)
            .ok()
            .map(|u| crate::classfile::percent_decode(u.path().rsplit('/').next().unwrap_or("")))
            .unwrap_or_default();
        let package = self.ast.root().child("package").and_then(|p| p.child("name")).map(|n| n.identifier()).unwrap_or_default();
        let todo = self.options.get("org.eclipse.jdt.core.compiler.taskTags").and_then(|s| s.split(',').next()).unwrap_or("TODO").to_owned();
        expand_template(template, |key| match key {
            "tags" if tags => Some(TAGS_MARKER),
            "file_name" => Some(file_name.as_str()),
            "package_name" => Some(package.as_str()),
            "project_name" => Some(self.profile.project_name.as_str()),
            "todo" => Some(todo.as_str()),
            "dollar" => Some("$"),
            _ => vars.iter().find(|(k, _)| *k == key).map(|(_, v)| *v),
        })
        .ok()
    }

    /// `StubUtility.getMethodComment(cu, typeName, decl, isDeprecated, ...)`
    /// via `CodeGeneration.getMethodComment(cu, typeName, decl, overridden, "\n")`.
    fn method_comment(&self, type_name: &str, decl: Node<'_>, overridden: Option<BindingRef<'_>>) -> Option<String> {
        let markdown = self.use_markdown();
        let constructor = decl.flag("constructor");
        let overridden = overridden.map(|o| o.method_declaration().unwrap_or(o));
        let key = if constructor {
            if markdown { "markdownconstructorcomment" } else { "constructorcomment" }
        } else if overridden.is_some() {
            "overridecomment"
        } else if markdown {
            "markdownmethodcomment"
        } else {
            "methodcomment"
        };
        let template = self.template(key)?;
        let method_name = decl.child("name").map(|n| n.identifier()).unwrap_or_default();
        let return_type = (!constructor).then(|| match decl.child("returnType2") {
            Some(t) => crate::rewrite::flattener::Flattener::as_string(&ASTRewrite::new(self.ast.clone()), RNode::Orig(t.id)),
            None => "void".to_owned(),
        });
        let see = overridden.map(|o| {
            let declaring = o.declaring_class().map(|c| c.qualified_name()).unwrap_or("");
            let params = o.parameter_types().iter().map(|t| t.erasure().unwrap_or(*t).qualified_name().to_owned()).collect::<Vec<_>>();
            see_tag(declaring, o.name(), &params)
        });
        let mut vars = vec![("enclosing_type", type_name), ("enclosing_method", method_name.as_str())];
        if let Some(r) = &return_type {
            vars.push(("return_type", r.as_str()));
        }
        if let Some(see) = &see {
            vars.push(("see_to_overridden", see.as_str()));
        }
        let text = self.evaluate(&template, &vars, true)?;
        if text.replace(TAGS_MARKER, "@").trim().is_empty() {
            return None;
        }
        let names = |list: Vec<Node<'_>>| list.iter().map(|n| n.child("name").map(|n| n.identifier()).unwrap_or_default()).collect::<Vec<_>>();
        let type_params = names(decl.list("typeParameters"));
        let params = names(decl.list("parameters"));
        let exceptions: Vec<String> = decl.list("thrownExceptionTypes").iter().map(|t| ast_type_name(*t, false)).collect();
        let deprecated = overridden.is_some_and(|o| o.is_deprecated());
        let tags = tag_lines(&params, &exceptions, return_type.as_deref(), &type_params, deprecated);
        Some(insert_tags(text, &tags, "\n"))
    }

    /// `StubUtility.getTypeComment(cu, typeQualifiedName, typeParameterNames, params, "\n")`.
    fn type_comment(&self, type_qualified_name: &str, type_params: &[String], params: &[String]) -> Option<String> {
        let template = self.template(if self.use_markdown() { "markdowntypecomment" } else { "typecomment" })?;
        let vars = [("enclosing_type", signature_qualifier(type_qualified_name)), ("type_name", signature_simple_name(type_qualified_name))];
        let text = self.evaluate(&template, &vars, true)?;
        if text.replace(TAGS_MARKER, "@").trim().is_empty() {
            return None;
        }
        let tags = tag_lines(params, &[], None, type_params, false);
        Some(insert_tags(text, &tags, "\n"))
    }

    /// `StubUtility.getFieldComment(cu, typeName, fieldName, "\n")`.
    fn field_comment(&self, type_name: &str, field_name: &str) -> Option<String> {
        let template = self.template(if self.use_markdown() { "markdownfieldcomment" } else { "fieldcomment" })?;
        let text = self.evaluate(&template, &[("field_type", type_name), ("field", field_name)], false)?;
        (!text.trim().is_empty()).then_some(text)
    }
}

/// `StubUtility.getSeeTag(declaringClassQualifiedName, methodName, parameterTypesQualifiedNames)`.
fn see_tag(declaring: &str, method: &str, params: &[String]) -> String {
    format!("@see {declaring}#{method}({})", params.join(", "))
}

/// The tags `StubUtility.insertTag` writes, in order.
fn tag_lines(params: &[String], exceptions: &[String], return_type: Option<&str>, type_params: &[String], deprecated: bool) -> Vec<String> {
    let mut tags: Vec<String> = type_params.iter().map(|t| format!("@param <{t}>")).collect();
    tags.extend(params.iter().map(|p| format!("@param {p}")));
    if return_type.is_some_and(|r| r != "void") {
        tags.push("@return".to_owned());
    }
    tags.extend(exceptions.iter().map(|e| format!("@throws {e}")));
    if deprecated {
        tags.push("@deprecated".to_owned());
    }
    tags
}

/// `StubUtility.insertTag` for each `${tags}` position, from last to first.
fn insert_tags(mut text: String, tags: &[String], line_delimiter: &str) -> String {
    let offsets: Vec<usize> = text.match_indices(TAGS_MARKER).map(|(i, _)| i).collect();
    for offset in offsets.into_iter().rev() {
        let line_start_offset = text[..offset].rfind('\n').map_or(0, |p| p + 1);
        let line_start = text[line_start_offset..offset].to_owned();
        let buf = tags.join(&format!("{line_delimiter}{line_start}"));
        let end = offset + TAGS_MARKER.len();
        if buf.is_empty() && line_start.chars().all(|c| c.is_whitespace() || c == '*') {
            let line = text[..offset].matches('\n').count();
            if line >= 2 {
                // clear full line: from the end of the previous line
                let prev_line_end = line_start_offset - 1;
                let prev_line_end = if prev_line_end > 0 && text.as_bytes()[prev_line_end - 1] == b'\r' { prev_line_end - 1 } else { prev_line_end };
                text.replace_range(prev_line_end..end, "");
                continue;
            }
        }
        text.replace_range(offset..end, &buf);
    }
    text
}

/// `ChangeMethodSignatureProposalCore` inserts new throws tags using the old
/// exception list as ordering context, without adding documentation to a
/// method that had no Javadoc.
pub(crate) fn insert_throws_tag(rw: &mut ASTRewrite, doc: Node<'_>, name: &str, original: &[Node<'_>]) {
    let leading: HashSet<_> = original.iter().map(|t| ast_type_name(*t, false)).collect();
    let reference = rw.new_name(name);
    let comment = text(rw, "");
    let tag = new_tag(rw, TAG_THROWS, vec![reference, comment]);
    insert_tag(rw, RNode::Orig(doc.id), tag, Some(&leading));
}
