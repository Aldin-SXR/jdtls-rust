//! `textDocument/signatureHelp`: a port of jdt.ls `SignatureHelpHandler`,
//! `SignatureHelpUtils`, `SignatureHelpContext` and `SignatureHelpRequestor`.
//!
//! jdt.ls asks the JDT completion engine for method proposals.  Here the
//! bridge (`SignatureHelpService.java`) returns the same information as data —
//! the method-like DOM nodes around the offset and the method bindings the
//! completion engine would propose for them — and everything else (context
//! guessing, argument ranges, active signature / parameter selection, labels,
//! Javadoc text and LSP shaping) happens here.
//!
//! All offsets are UTF-16 code-unit offsets, like Java `String` indices.
//! jdt.ls ignores the request's `context` (`triggerKind`, `isRetrigger`,
//! `activeSignatureHelp`), and so does this port.

use crate::analysis::dispatcher::Dispatcher;
use crate::analysis::semantic::protocol::{SigCandidate, SigNode};
use crate::analysis::semantic::BridgeResponse;
use serde_json::Value;
use tower_lsp::lsp_types::{
    Documentation, ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation, Url,
};

/// `SignatureHelpHandler.SEARCH_BOUND`.
const SEARCH_BOUND: i64 = 2000;

/// The `java.signatureHelp.*` preferences.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub enabled: bool,
    pub description: bool,
}

impl Settings {
    /// Read from a jdt.ls `settings` object.  `java.signatureHelp.enabled`
    /// defaults to `true` (the vscode-java default; jdt.ls itself defaults to
    /// `false` and relies on the client sending it).
    pub fn from_settings(settings: Option<&Value>) -> Self {
        let get = |path: &[&str]| -> Option<bool> {
            let mut cur = settings?;
            for key in path {
                cur = cur.get(*key)?;
            }
            cur.as_bool().or_else(|| cur.as_str().map(|s| s == "true"))
        };
        Settings {
            enabled: get(&["java", "signatureHelp", "enabled"]).unwrap_or(true),
            description: get(&["java", "signatureHelp", "description", "enabled"]).unwrap_or(false),
        }
    }
}

fn empty() -> SignatureHelp {
    SignatureHelp { signatures: Vec::new(), active_signature: None, active_parameter: None }
}

/// Entry point (`SignatureHelpHandler.signatureHelp`).
pub async fn signature_help(dispatcher: &Dispatcher, uri: &Url, position: Position, settings: Settings) -> SignatureHelp {
    if !settings.enabled {
        return empty();
    }
    let content = match dispatcher.store.get(uri) {
        Some(state) => state.content_string(),
        None => return empty(),
    };
    if !dispatcher.is_ecj_ready().await {
        return empty();
    }
    let text: Vec<u16> = content.encode_utf16().collect();
    let offset = to_offset(&text, position);

    let search = search_offset(&text, offset);
    let context = context_information(&text, offset);
    let fallback_name = if context.0 >= 0 { method_name_before(&text, context.0) } else { None };
    let response = dispatcher
        .signature_help_data(
            uri,
            search,
            (context.0 >= 0).then_some(context.0 as usize),
            fallback_name.clone(),
            settings.description,
        )
        .await;
    let (chain, fallback) = match response {
        Ok(BridgeResponse::SignatureHelpData { chain, fallback, .. }) => (chain, fallback),
        Ok(BridgeResponse::Error { message, .. }) => {
            tracing::warn!("signature help failed: {message}");
            return empty();
        }
        _ => return empty(),
    };
    let offset = offset as i64;
    if let Some(search) = search {
        if let Some(help) = from_ast_node(&text, offset, search as i64, &chain, settings) {
            return help;
        }
    }
    from_heuristics(&text, offset, context, fallback.as_ref(), fallback_name.as_deref(), settings)
}

// ── Text helpers ─────────────────────────────────────────────────────────────

/// `JsonRpcHelpers.toOffset`: line start + UTF-16 column, clamped to the text.
fn to_offset(text: &[u16], pos: Position) -> usize {
    let mut line = 0u32;
    let mut i = 0usize;
    while line < pos.line && i < text.len() {
        if text[i] == b'\n' as u16 {
            line += 1;
        }
        i += 1;
    }
    (i + pos.character as usize).min(text.len())
}

fn ch(text: &[u16], i: i64) -> u16 {
    if i < 0 || i as usize >= text.len() { 0 } else { text[i as usize] }
}

fn is_char(c: u16, expected: char) -> bool {
    c == expected as u16
}

/// `Character.isWhitespace`.
fn is_whitespace(c: u16) -> bool {
    match c {
        0x09..=0x0D | 0x1C..=0x1F | 0x20 => true,
        0xA0 | 0x2007 | 0x202F => false,
        _ => char::from_u32(c as u32).is_some_and(char::is_whitespace),
    }
}

/// `Character.isJavaIdentifierPart` (BMP approximation).
fn is_java_identifier_part(c: u16) -> bool {
    match char::from_u32(c as u32) {
        Some(ch) => ch.is_alphanumeric() || ch == '_' || ch == '$',
        None => false,
    }
}

fn is_java_identifier_start(c: u16) -> bool {
    match char::from_u32(c as u32) {
        Some(ch) => ch.is_alphabetic() || ch == '_' || ch == '$',
        None => false,
    }
}

const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue",
    "default", "do", "double", "else", "enum", "extends", "final", "finally", "float", "for", "goto", "if",
    "implements", "import", "instanceof", "int", "interface", "long", "native", "new", "package", "private",
    "protected", "public", "return", "short", "static", "strictfp", "super", "switch", "synchronized", "this",
    "throw", "throws", "transient", "try", "void", "volatile", "while", "true", "false", "null", "_",
];

/// `JavaConventionsUtil.validateMethodName(name).isOK()`: a valid identifier
/// that does not trigger the "should start with a lowercase letter" warning.
fn is_valid_method_name(name: &str) -> bool {
    let units: Vec<u16> = name.encode_utf16().collect();
    let Some(&first) = units.first() else { return false };
    if !is_java_identifier_start(first) || !units.iter().all(|&c| is_java_identifier_part(c)) {
        return false;
    }
    if KEYWORDS.contains(&name) {
        return false;
    }
    !name.chars().next().is_some_and(char::is_uppercase)
}

// ── SignatureHelpContext ────────────────────────────────────────────────────

/// The offset `SignatureHelpContext.findTargetNode` searches the AST at, or
/// `None` when the trigger offset is at/after the end of the source.
fn search_offset(text: &[u16], trigger: usize) -> Option<usize> {
    if trigger >= text.len() {
        return None;
    }
    let mut s = trigger;
    while s > 0 {
        let cur = text[s];
        let prev = text[s - 1];
        if is_whitespace(cur) && !is_char(prev, ';') {
            s -= 1;
        } else if is_char(cur, ')') && (is_whitespace(prev) || is_char(prev, ',')) {
            s -= 1;
        } else {
            break;
        }
    }
    Some(if s == 0 { trigger } else { s })
}

fn node_end(node: &SigNode) -> i64 {
    node.start + node.length
}

fn arguments(node: &SigNode) -> &[[i64; 2]] {
    node.arguments.as_deref().unwrap_or(&[])
}

/// `SignatureHelpContext.findArgumentRange`.
fn argument_range(node: &SigNode, text: &[u16]) -> Option<(i64, i64)> {
    let args = node.arguments.as_ref()?;
    if let (Some(first), Some(last)) = (args.first(), args.last()) {
        return Some((first[0], last[0] + last[1]));
    }
    if node.name_end < 0 {
        return None;
    }
    let mut i = node.name_end;
    while i < node_end(node) {
        if is_char(ch(text, i), '(') {
            return Some((i + 1, node_end(node) - 1));
        }
        i += 1;
    }
    None
}

fn is_in_argument_list(node: &SigNode, text: &[u16], offset: i64) -> bool {
    argument_range(node, text).is_some_and(|(s, e)| s <= offset && e >= offset)
}

/// `SignatureHelpContext.findEnclosingMethodNode`; `chain` holds the
/// method-like ancestors (innermost first) up to the enclosing block.
fn enclosing_method_node<'a>(chain: &'a [SigNode], text: &[u16], offset: i64) -> &'a SigNode {
    let node = &chain[0];
    if is_in_argument_list(node, text, offset) {
        return node;
    }
    chain[1..].iter().find(|p| is_in_argument_list(p, text, offset)).unwrap_or(node)
}

fn is_method_kind(kind: &str) -> bool {
    matches!(kind, "MethodInvocation" | "SuperMethodInvocation" | "MethodRef")
}

fn is_constructor_kind(kind: &str) -> bool {
    matches!(kind, "ClassInstanceCreation" | "ConstructorInvocation" | "SuperConstructorInvocation")
}

/// `SignatureHelpContext.guessCompletionOffset`: (completion, secondary).
fn guess_completion_offset(node: &SigNode, text: &[u16]) -> (i64, i64) {
    let start = node.start + node.optional_expression_length;
    for i in start..node_end(node) {
        if is_char(ch(text, i), '(') {
            if is_method_kind(&node.kind) {
                return (i, i + 1);
            } else if is_constructor_kind(&node.kind) {
                return (i + 1, -1);
            }
        }
    }
    (-1, -1)
}

fn is_argument_char(c: u16) -> bool {
    !is_whitespace(c) && !is_char(c, ')') && !is_char(c, ']') && !is_char(c, '}') && !is_char(c, ';')
}

fn index_of(hay: &[u16], needle: &[u16], from: usize) -> i64 {
    if needle.is_empty() || from > hay.len() {
        return -1;
    }
    (from..=hay.len().saturating_sub(needle.len()))
        .find(|&i| hay[i..].starts_with(needle))
        .map_or(-1, |i| i as i64)
}

/// `SignatureHelpContext.guessArgumentRanges`.
fn guess_argument_ranges(text: &[u16], completion: i64) -> Vec<(i64, i64)> {
    if completion < 0 {
        return Vec::new();
    }
    let mut start = completion;
    if is_char(ch(text, start), '(') {
        start += 1;
    }
    let lits: &[u16] = if (start as usize) <= text.len() { &text[start as usize..] } else { &[] };
    let n = lits.len() as i64;
    let at = |i: i64| -> u16 { if i >= 0 && i < n { lits[i as usize] } else { 0 } };
    let mut list = Vec::new();
    let mut range = (start, start);
    let mut stack: Vec<u16> = Vec::new();
    let mut has_argument = false;
    let triple: Vec<u16> = "\"\"\"".encode_utf16().collect();
    let mut i: i64 = 0;
    while i < n {
        let mut c = at(i);
        if !has_argument && is_argument_char(c) {
            has_argument = true;
        }
        let close = |open: char, stack: &mut Vec<u16>, list: &mut Vec<(i64, i64)>, range: &mut (i64, i64)| -> bool {
            if stack.last().is_some_and(|&t| is_char(t, open)) {
                stack.pop();
                false
            } else {
                if has_argument {
                    range.1 = start + i;
                    list.push(*range);
                }
                true
            }
        };
        match char::from_u32(c as u32).unwrap_or('\0') {
            ',' => {
                if stack.is_empty() {
                    range.1 = start + i;
                    list.push(range);
                    range = (start + i + 1, start + i + 1);
                }
            }
            '\'' => {
                i += 1;
                while i < n {
                    c = at(i);
                    if is_char(c, '\'') {
                        break;
                    } else if is_char(c, '\\') {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            '"' => {
                if lits[i as usize..].starts_with(&triple) {
                    let mut end = index_of(lits, &triple, (i + 3) as usize);
                    while end > 0 && is_char(at(end - 1), '\\') {
                        end = index_of(lits, &triple, (end + 3) as usize);
                    }
                    i = if end > 0 { end + 2 } else { n - 1 };
                } else {
                    i += 1;
                    while i < n {
                        c = at(i);
                        if is_char(c, '"') {
                            break;
                        } else if is_char(c, '\\') {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            '(' | '[' | '{' => stack.push(c),
            ')' => {
                if close('(', &mut stack, &mut list, &mut range) {
                    return list;
                }
            }
            ']' => {
                if close('[', &mut stack, &mut list, &mut range) {
                    return list;
                }
            }
            '}' => {
                if close('{', &mut stack, &mut list, &mut range) {
                    return list;
                }
            }
            '<' => {
                i += 1;
                while i < n {
                    if is_char(at(i), '>') {
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if has_argument {
        range.1 = start + n - 1;
        list.push(range);
    }
    list
}

// ── SignatureHelpRequestor ──────────────────────────────────────────────────

/// A signature and the proposal it came from (`SignatureHelpRequestor.infoProposals`).
struct Info<'a> {
    proposal: &'a SigCandidate,
    info: SignatureInformation,
}

/// `SignatureHelpRequestor`: proposals keyed by signature (first wins),
/// shaped into signatures sorted by parameter count.
#[derive(Default)]
struct Collector<'a> {
    proposals: Vec<&'a SigCandidate>,
}

impl<'a> Collector<'a> {
    fn accept_all(&mut self, candidates: Option<&'a Vec<SigCandidate>>) {
        for c in candidates.into_iter().flatten() {
            if !self.proposals.iter().any(|p| p.key == c.key) {
                self.proposals.push(c);
            }
        }
    }

    fn signature_help(&self, settings: Settings) -> Vec<Info<'a>> {
        let mut infos: Vec<Info<'a>> =
            self.proposals.iter().map(|p| Info { proposal: p, info: to_signature_information(p, settings) }).collect();
        infos.sort_by_key(|i| i.proposal.parameter_types.len());
        infos
    }
}

/// `CompletionProposalDescriptionProvider.convertToVararg`.
fn convert_to_vararg(name: &str) -> String {
    match name.strip_suffix("[]") {
        Some(base) => format!("{base}..."),
        None => name.to_owned(),
    }
}

fn parameter_types(p: &SigCandidate) -> Vec<String> {
    let mut types = p.parameter_types.clone();
    if p.varargs {
        if let Some(last) = types.last_mut() {
            *last = convert_to_vararg(last);
        }
    }
    types
}

/// `SignatureHelpRequestor.toSignatureInformation`.
fn to_signature_information(p: &SigCandidate, settings: Settings) -> SignatureInformation {
    let types = parameter_types(p);
    let params: Vec<String> = types
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{t} {}", p.parameter_names.get(i).map_or("", String::as_str)))
        .collect();
    let mut label = format!("{}({})", p.name, params.join(", "));
    if !p.constructor {
        label.push_str(" : ");
        label.push_str(p.return_type.as_deref().unwrap_or("void"));
    }
    let documentation = if settings.description {
        p.javadoc
            .as_deref()
            .and_then(crate::javadoc::comment_reader::plain_text_content)
            .map(Documentation::String)
    } else {
        None
    };
    SignatureInformation {
        label,
        documentation,
        parameters: Some(
            params.into_iter().map(|l| ParameterInformation { label: ParameterLabel::Simple(l), documentation: None }).collect(),
        ),
        active_parameter: None,
    }
}

// ── SignatureHelpUtils.getSignatureHelpFromASTNode ──────────────────────────

struct AstContext<'a> {
    node: &'a SigNode,
    completion: i64,
    ranges: Vec<(i64, i64)>,
}

fn from_ast_node(text: &[u16], trigger: i64, search: i64, chain: &[SigNode], settings: Settings) -> Option<SignatureHelp> {
    if chain.is_empty() {
        return None;
    }
    let node = enclosing_method_node(chain, text, search);
    let (completion, secondary) = guess_completion_offset(node, text);
    let ranges = guess_argument_ranges(text, completion);
    let ctx = AstContext { node, completion, ranges };

    let mut help = empty();
    let args = arguments(node);
    if node.arguments.is_some() && args.is_empty() {
        let end = node_end(node);
        if is_char(ch(text, end - 1), ')') && end <= trigger {
            return Some(help);
        }
    } else if let Some(last) = ctx.ranges.last() {
        if last.1 < trigger {
            return Some(help);
        }
    }

    let mut collector = Collector::default();
    if node.method_name.is_some() && completion >= 0 {
        collector.accept_all(node.candidates.as_ref());
        if secondary > -1 {
            collector.accept_all(node.secondary_candidates.as_ref());
        }
    }
    let mut infos = collector.signature_help(settings);
    if infos.is_empty() && node.kind == "ClassInstanceCreation" {
        infos = fix_2097(node, settings);
    }
    if infos.is_empty() {
        return Some(help);
    }
    if let Some(selected) = crate::features::completion::handler::selected_signature_key() {
        for (i, info) in infos.iter().enumerate() {
            if info.proposal.key == selected {
                let parameter = active_parameter(trigger, info.proposal, &ctx);
                if parameter >= 0 {
                    help.active_signature = Some(i as u32);
                    help.active_parameter = Some(parameter as u32);
                    help.signatures = infos.into_iter().map(|i| i.info).collect();
                    return Some(help);
                }
            }
        }
    }
    crate::features::completion::handler::clear_selected_proposal();
    for (i, info) in infos.iter().enumerate() {
        if is_matched(info.proposal, &ctx) {
            help.active_signature = Some(i as u32);
            help.active_parameter = Some(active_parameter(trigger, info.proposal, &ctx) as u32);
            break;
        }
    }
    help.signatures = infos.into_iter().map(|i| i.info).collect();
    Some(help)
}

/// `SignatureHelpUtils.fix2097`: the declared constructors of the type.
fn fix_2097<'a>(node: &'a SigNode, settings: Settings) -> Vec<Info<'a>> {
    let mut infos: Vec<Info<'a>> = node
        .declared_constructors
        .iter()
        .flatten()
        .map(|p| Info { proposal: p, info: to_signature_information(p, settings) })
        .collect();
    infos.sort_by_key(|i| i.proposal.parameter_types.len());
    infos
}

/// `SignatureHelpUtils.isMatched`.
fn is_matched(proposal: &SigCandidate, ctx: &AstContext) -> bool {
    if proposal.parameter_types.len() < ctx.ranges.len() && !proposal.varargs {
        return false;
    }
    let args = arguments(ctx.node);
    if args.is_empty() {
        return true;
    }
    let types = ctx.node.parameter_types.as_ref();
    let from_binding = ctx.node.parameter_types_from_binding.as_ref();
    let param_num = types.or(from_binding).map_or(0, Vec::len);
    let mut matched = 0;
    let mut start_index = 0;
    let mut i = 0;
    while i < args.len() && i < param_num {
        let mut j = start_index;
        while j < ctx.ranges.len() {
            let start = args[i][0];
            if start >= ctx.ranges[j].0 && start <= ctx.ranges[j].1 {
                start_index = j + 1;
                break;
            }
            j += 1;
        }
        if j >= proposal.match_types.len() {
            break;
        }
        let proposed = &proposal.match_types[j];
        if types.and_then(|t| t.get(i)).is_some_and(|t| t == proposed)
            || from_binding.and_then(|t| t.get(i)).is_some_and(|t| t == proposed)
        {
            matched += 1;
        }
        i += 1;
    }
    matched == param_num.min(args.len())
}

/// `SignatureHelpUtils.getActiveParameter`.
fn active_parameter(trigger: i64, proposal: &SigCandidate, ctx: &AstContext) -> i64 {
    let count = proposal.parameter_types.len() as i64;
    if trigger >= ctx.completion {
        if count > 0 && ctx.ranges.is_empty() {
            return 0;
        }
        for (i, r) in ctx.ranges.iter().enumerate() {
            if r.0 <= trigger && r.1 >= trigger {
                if i as i64 >= count && proposal.varargs {
                    return count - 1;
                }
                return i as i64;
            }
        }
    }
    count
}

// ── SignatureHelpHandler heuristics ─────────────────────────────────────────

/// `SignatureHelpHandler.getContextInfomation`: (offset of the opening
/// parenthesis, current parameter index), or -1s.
fn context_information(text: &[u16], offset: usize) -> (i64, i64) {
    let offset = offset as i64;
    let mut result = (-1i64, -1i64);
    let mut depth = 1;
    let mut i = offset - 1;
    while i >= 0 && (offset - i) < SEARCH_BOUND {
        let c = ch(text, i);
        if is_char(c, '{') || is_char(c, '}') {
            return (-1, -1);
        }
        if is_char(c, ')') {
            depth += 1;
        }
        if is_char(c, '(') {
            depth -= 1;
        }
        if is_char(c, ',') && depth == 1 {
            result.1 += 1;
        }
        if depth == 0 {
            result.0 = i;
            break;
        }
        i -= 1;
    }
    if result.0 + 1 != offset {
        let mut i = 1;
        while result.0 + i < offset {
            if !is_whitespace(ch(text, result.0 + i)) {
                result.1 += 1;
                break;
            }
            i += 1;
        }
    }
    result
}

/// `SignatureHelpHandler.getMethodName` for blocks: the identifier before
/// the parenthesis at `pos`, if it is a valid method name.
fn method_name_before(text: &[u16], pos: i64) -> Option<String> {
    let mut pos = pos;
    while pos >= 0 {
        let c = ch(text, pos);
        if is_char(c, '(') || is_whitespace(c) {
            pos -= 1;
        } else {
            break;
        }
    }
    let end = pos + 1;
    while pos >= 0 && is_java_identifier_part(ch(text, pos)) {
        pos -= 1;
    }
    let start = pos + 1;
    let name = String::from_utf16_lossy(&text[start as usize..end as usize]);
    is_valid_method_name(&name).then_some(name)
}

/// `JDTUtils.isSameParameters`.
fn same_parameters(m1: &SigCandidate, m2: &SigCandidate) -> bool {
    m1.declared_types.len() == m2.declared_types.len()
        && m1.declared_types.iter().zip(&m2.declared_types).all(|(a, b)| a == b)
}

/// `SignatureHelpHandler.isSameParameters(IMethod, IMethod, null)`.
fn same_parameter_prefix(m1: &SigCandidate, m2: &SigCandidate) -> bool {
    if m1.name != m2.name {
        return false;
    }
    let (p1, p2) = (&m1.declared_types, &m2.declared_types);
    if (p2.len() as i64) <= p1.len() as i64 - 1 {
        for i in 0..p2.len() {
            if p2[i] != p1[i] {
                return false;
            }
        }
    }
    true
}

fn identifier_prefix(text: &[u16], offset: i64) -> String {
    let mut s = offset;
    while s > 0 && is_java_identifier_part(ch(text, s - 1)) {
        s -= 1;
    }
    String::from_utf16_lossy(&text[s as usize..offset as usize])
}

fn from_heuristics(
    text: &[u16],
    offset: i64,
    context: (i64, i64),
    node: Option<&SigNode>,
    fallback_name: Option<&str>,
    settings: Settings,
) -> SignatureHelp {
    let mut help = empty();
    let Some(node) = node.filter(|_| context.0 != -1) else { return help };
    let is_block = node.kind == "Block";
    if is_block && fallback_name.is_none() {
        return help;
    }
    let method = node.bound_method.as_ref();
    let name = match method {
        Some(m) => Some(m.name.clone()),
        None if is_block => fallback_name.map(str::to_owned),
        None => None,
    };
    let Some(name) = name else { return help };

    // The proposals the completion engine makes at `pos`: right after the
    // method name when the node is bound, otherwise inside the parentheses.
    let mut collector = Collector::default();
    if method.is_some() {
        if node.kind != "ClassInstanceCreation" {
            collector.accept_all(node.candidates.as_ref());
        }
    } else {
        collector.accept_all(node.candidates.as_ref());
        if node.kind != "Block" {
            collector.accept_all(node.secondary_candidates.as_ref());
        }
    }
    let mut infos = collector.signature_help(settings);
    if method.is_some_and(|m| m.constructor) && infos.is_empty() && node.kind == "ClassInstanceCreation" {
        infos = fix_2097(node, settings);
    }

    // `collector2`: proposals named `name` at the trigger offset.
    let help2: Option<Vec<&SigCandidate>> = (context.0 + 1 != offset).then(|| {
        if name.starts_with(&identifier_prefix(text, offset)) {
            node.scope_candidates.iter().flatten().collect()
        } else {
            Vec::new()
        }
    });

    let current = context.1;
    let size = current + 1;
    let active_param = current.max(0);
    let params_len = |i: &Info| i.proposal.parameter_types.len() as i64;
    let select = |infos: Vec<Info>, index: usize, param: i64| -> SignatureHelp {
        SignatureHelp {
            signatures: infos.into_iter().map(|i| i.info).collect(),
            active_signature: Some(index as u32),
            active_parameter: Some(param.max(0) as u32),
        }
    };

    if let Some(help2) = &help2 {
        if let Some(method) = method {
            if let Some(i) = infos.iter().position(|i| params_len(i) >= size && same_parameters(i.proposal, method)) {
                return select(infos, i, active_param);
            }
        }
        if let Some(i) = infos.iter().position(|i| {
            params_len(i) >= size && help2.iter().any(|h| h.name == i.proposal.name && same_parameters(h, i.proposal))
        }) {
            return select(infos, i, active_param);
        }
    }
    if let Some(method) = method {
        if let Some(i) = infos.iter().position(|i| params_len(i) >= size && same_parameters(method, i.proposal)) {
            return select(infos, i, active_param);
        }
    }
    if let Some(i) = infos.iter().position(|i| i.proposal.varargs) {
        let count = params_len(&infos[i]);
        let param = if count <= active_param { count - 1 } else { active_param };
        return select(infos, i, param);
    }
    if is_block {
        if let Some(i) = infos.iter().position(|i| params_len(i) >= active_param && i.proposal.name == name) {
            return select(infos, i, active_param);
        }
    }
    if let Some(method) = method {
        if let Some(i) = infos.iter().position(|i| params_len(i) >= size && same_parameter_prefix(i.proposal, method)) {
            return select(infos, i, active_param);
        }
    }
    help.signatures = infos.into_iter().map(|i| i.info).collect();
    help
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn argument_ranges() {
        let t = u("foo(\"\",)");
        assert_eq!(guess_argument_ranges(&t, 3), vec![(4, 6), (7, 7)]);
        let t = u("new String(,);");
        assert_eq!(guess_argument_ranges(&t, 11), vec![(11, 11), (12, 12)]);
    }

    #[test]
    fn context_info() {
        let t = u("foo(2,  )");
        assert_eq!(context_information(&t, 7), (3, 1));
    }

    #[test]
    fn method_names() {
        assert!(is_valid_method_name("foo"));
        assert!(!is_valid_method_name("Foo"));
        assert!(!is_valid_method_name("if"));
        assert_eq!(method_name_before(&u("  foo (x"), 6), Some("foo".into()));
    }
}
