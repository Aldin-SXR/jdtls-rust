//! Port of jdt.ls `CompletionProposalRequestor`: proposal filtering
//! (type filters, case matching, collapsing), ordering, the result limit,
//! `itemDefaults` and the conversion of proposals into completion items.

use super::description::{required_type_proposal, DescriptionProvider};
use super::item::{item_kind, EditRange, Item, ItemDefaults, ItemTextEdit};
use super::prefs::{Client, GuessMode, MatchCase, Prefs};
use super::proposal::{flags, kind, Context, EnclosingField, GetterSetter, Proposal};
use super::replacement::{is_import_completion, ReplacementProvider};
use super::signature as sig;
use super::sort_text;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use tower_lsp::lsp_types::{InsertTextMode, Range};

pub const DATA_FIELD_URI: &str = "uri";
pub const DATA_FIELD_REQUEST_ID: &str = "rid";
pub const DATA_FIELD_PROPOSAL_ID: &str = "pid";

/// Eclipse `StringMatcher(pattern, ignoreCase=false, ignoreWildCards=false)` full match.
pub fn string_matcher(pattern: &str, text: &str) -> bool {
    fn m(p: &[char], t: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            '*' => (0..=t.len()).any(|i| m(&p[1..], &t[i..])),
            '?' => !t.is_empty() && m(&p[1..], &t[1..]),
            c => !t.is_empty() && t[0] == c && m(&p[1..], &t[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    m(&p, &t)
}

/// jdt.ls `TypeFilter` (after `removeFilterIfMatched` with the unit's imports).
#[derive(Debug, Clone, Default)]
pub struct TypeFilter {
    pub patterns: Vec<String>,
}

impl TypeFilter {
    pub fn new(patterns: &[String], imports: &[String]) -> Self {
        let patterns = patterns
            .iter()
            .filter(|p| !p.is_empty())
            .filter(|p| !imports.iter().any(|i| string_matcher(p, i)))
            .cloned()
            .collect();
        TypeFilter { patterns }
    }
    pub fn is_filtered(&self, name: &str) -> bool {
        self.patterns.iter().any(|p| string_matcher(p, name))
    }
}

/// `CompletionProposalRequestor.mapKind`.
pub fn map_kind(p: &Proposal) -> u32 {
    let f = p.flags;
    match p.kind {
        kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::CONSTRUCTOR_INVOCATION => item_kind::CONSTRUCTOR,
        kind::ANONYMOUS_CLASS_DECLARATION | kind::TYPE_REF => {
            if flags::is(f, flags::INTERFACE) {
                item_kind::INTERFACE
            } else if flags::is(f, flags::ENUM) {
                item_kind::ENUM
            } else if flags::is(f, flags::RECORD) {
                item_kind::STRUCT
            } else {
                item_kind::CLASS
            }
        }
        kind::FIELD_IMPORT | kind::METHOD_IMPORT | kind::PACKAGE_REF | kind::TYPE_IMPORT | kind::MODULE_DECLARATION | kind::MODULE_REF => {
            item_kind::MODULE
        }
        kind::FIELD_REF => {
            if flags::is(f, flags::ENUM) {
                item_kind::ENUM_MEMBER
            } else if flags::is(f, flags::STATIC) && flags::is(f, flags::FINAL) {
                item_kind::CONSTANT
            } else {
                item_kind::FIELD
            }
        }
        kind::ANNOTATION_ATTRIBUTE_REF | kind::FIELD_REF_WITH_CASTED_RECEIVER => item_kind::FIELD,
        kind::KEYWORD => item_kind::KEYWORD,
        kind::LABEL_REF => item_kind::REFERENCE,
        kind::LOCAL_VARIABLE_REF | kind::VARIABLE_DECLARATION => item_kind::VARIABLE,
        kind::METHOD_DECLARATION
        | kind::METHOD_REF
        | kind::METHOD_REF_WITH_CASTED_RECEIVER
        | kind::METHOD_NAME_REFERENCE
        | kind::POTENTIAL_METHOD_DECLARATION
        | kind::LAMBDA_EXPRESSION => item_kind::METHOD,
        _ => item_kind::TEXT,
    }
}

/// `CompletionProposalRequestor.SUPPORTED_KINDS`.
pub fn supported_kind(k: u32) -> bool {
    matches!(
        k,
        item_kind::CONSTRUCTOR
            | item_kind::CLASS
            | item_kind::CONSTANT
            | item_kind::INTERFACE
            | item_kind::ENUM
            | item_kind::ENUM_MEMBER
            | item_kind::MODULE
            | item_kind::FIELD
            | item_kind::KEYWORD
            | item_kind::REFERENCE
            | item_kind::VARIABLE
            | item_kind::METHOD
            | item_kind::TEXT
            | item_kind::SNIPPET
            | item_kind::PROPERTY
            | item_kind::STRUCT
    )
}

pub struct Collector<'a> {
    pub prefs: &'a Prefs,
    pub client: &'a Client,
    pub context: &'a Context,
    pub filter: &'a TypeFilter,
    /// Package of the unit (`unit.getParent().getElementName()`).
    pub package_name: &'a str,
    pub proposals: Vec<Proposal>,
    pub collapsed: HashMap<String, i32>,
    pub completion_kinds: BTreeSet<i32>,
    pub is_complete: bool,
}

impl<'a> Collector<'a> {
    /// `accept(CompletionProposal)`.
    pub fn accept(&mut self, mut p: Proposal) {
        if self.is_filtered(&p) {
            return;
        }
        if !self.match_case(&p) {
            return;
        }
        if self.need_to_collapse(&p) {
            return;
        }
        if p.kind == kind::POTENTIAL_METHOD_DECLARATION {
            self.accept_potential_method_declaration(&p);
        } else {
            if p.kind == kind::PACKAGE_REF && !self.package_name.is_empty() && p.completion() == self.package_name {
                p.relevance += 1;
            }
            self.proposals.push(p);
        }
    }

    fn is_filtered(&self, p: &Proposal) -> bool {
        match p.kind {
            kind::CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::JAVADOC_TYPE_REF | kind::TYPE_REF => {
                self.is_type_filtered(p)
            }
            kind::METHOD_REF if p.has_required() => self.is_type_filtered(p),
            _ => false,
        }
    }

    fn is_type_filtered(&self, p: &Proposal) -> bool {
        if is_import_completion(p) {
            return false;
        }
        match declaring_type(p) {
            Some(t) => self.filter.is_filtered(&t),
            None => false,
        }
    }

    fn match_case(&self, proposal: &Proposal) -> bool {
        if self.prefs.match_case != MatchCase::FirstLetter {
            return true;
        }
        let Some(token) = self.context.token.as_deref() else { return true };
        if proposal.completion.is_none() {
            return true;
        }
        if token.is_empty() || proposal.completion().is_empty() {
            return true;
        }
        let mut p = proposal;
        if matches!(
            p.kind,
            kind::CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_DECLARATION
        ) {
            for r in p.required() {
                if r.kind == kind::TYPE_REF {
                    p = r;
                }
            }
        }
        let first = if p.kind == kind::TYPE_REF {
            sig::simple_type_name(p.signature()).chars().next()
        } else if p.kind == kind::METHOD_DECLARATION {
            p.name().chars().next()
        } else {
            p.completion().chars().next()
        };
        let Some(first) = first else { return true };
        let t0 = token.chars().next().unwrap();
        if p.kind == kind::PACKAGE_REF || p.kind == kind::MODULE_REF {
            return t0.is_uppercase() == first.is_uppercase();
        }
        t0 == first
    }

    fn need_to_collapse(&mut self, p: &Proposal) -> bool {
        if self.prefs.collapse || self.prefs.guess_mode == GuessMode::Off {
            if p.kind == kind::METHOD_REF && self.prefs.collapse {
                let c = self.collapsed.entry(p.name().to_owned()).or_insert(0);
                *c += 1;
                return *c > 1;
            }
            let Some(r) = required_type_proposal(p) else { return false };
            let c = self.collapsed.entry(r.signature().to_owned()).or_insert(0);
            *c += 1;
            return *c > 1;
        }
        false
    }

    /// `acceptPotentialMethodDeclaration` → `GetterSetterCompletionProposal.evaluateProposals`.
    fn accept_potential_method_declaration(&mut self, p: &Proposal) {
        if self.context.enclosing_type_name.is_none() {
            return;
        }
        let prefix = p.name().to_owned();
        let start = p.replace_start;
        let end = p.replace_end;
        // Upstream computes a relevance (`proposal.getRelevance() + 6`, minus
        // one for an empty prefix or a static final field) but never sets it on
        // the new proposals, so they keep `InternalCompletionProposal`'s default.
        const DEFAULT_RELEVANCE: i32 = 1;
        let fields: Vec<EnclosingField> = self.context.enclosing_fields.clone();
        let methods = &self.context.enclosing_methods;
        let has_method = |n: &str| methods.iter().any(|m| m == n);
        let _ = end;
        for f in &fields {
            if f.is_enum_constant {
                continue;
            }
            let getter = super::accessors::getter_name(&f.name, &f.type_signature);
            if starts_with_ignore_case(&getter, &prefix) && !has_method(&getter) {
                let prop = Proposal {
                    kind: kind::POTENTIAL_METHOD_DECLARATION,
                    name: Some(getter.clone()),
                    signature: Some(format!("(){}", f.type_signature)),
                    completion: Some(getter.clone()),
                    declaration_signature: Some(f.type_signature.clone()),
                    replace_start: start,
                    replace_end: start + prefix.encode_utf16().count() as i32,
                    relevance: DEFAULT_RELEVANCE,
                    completion_location: start - 1,
                    parameter_names: Some(Vec::new()),
                    getter_setter: Some(GetterSetter { field: f.clone(), is_getter: true }),
                    ..Default::default()
                };
                self.proposals.push(prop);
            }
            if !flags::is(f.flags, flags::FINAL) {
                let setter = super::accessors::setter_name(&f.name);
                if starts_with_ignore_case(&setter, &prefix) && !has_method(&setter) {
                    let prop = Proposal {
                        kind: kind::POTENTIAL_METHOD_DECLARATION,
                        name: Some(setter.clone()),
                        signature: Some(format!("({})V", f.type_signature)),
                        completion: Some(getter.clone()),
                        declaration_signature: Some(f.type_signature.clone()),
                        replace_start: start,
                        replace_end: start + prefix.encode_utf16().count() as i32,
                        relevance: DEFAULT_RELEVANCE,
                        completion_location: start - 1,
                        parameter_names: Some(vec![f.name.clone()]),
                        getter_setter: Some(GetterSetter { field: f.clone(), is_getter: false }),
                        ..Default::default()
                    };
                    self.proposals.push(prop);
                }
            }
        }
    }

    /// Sort with `ProposalComparator` and apply the result limit; returns the
    /// proposals kept (in item order).
    pub fn sorted_limited(&mut self, uri: &str) -> Vec<Proposal> {
        let mut props = std::mem::take(&mut self.proposals);
        let aggregated_ranks = super::ranking::aggregated_ranking_result(&props, self.context, uri);
        for (proposal, rank) in props.iter_mut().zip(aggregated_ranks) {
            if let Some(rank) = rank {
                // we assume there won't be overflow for now since the the score from
                // each provider can only be 100 at most.
                proposal.relevance += rank.score();
                proposal.ranking = Some(rank);
            }
        }
        props.sort_by(compare_proposals);
        let limit = props.len().min(self.prefs.max_results);
        if props.len() > limit {
            self.is_complete = false;
        }
        props.truncate(limit);
        props
    }
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// `getDeclaringType`.
fn declaring_type(p: &Proposal) -> Option<String> {
    match p.kind {
        kind::METHOD_DECLARATION
        | kind::METHOD_NAME_REFERENCE
        | kind::JAVADOC_METHOD_REF
        | kind::METHOD_REF
        | kind::CONSTRUCTOR_INVOCATION
        | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION
        | kind::METHOD_REF_WITH_CASTED_RECEIVER
        | kind::ANNOTATION_ATTRIBUTE_REF
        | kind::POTENTIAL_METHOD_DECLARATION
        | kind::ANONYMOUS_CLASS_DECLARATION
        | kind::FIELD_REF
        | kind::FIELD_REF_WITH_CASTED_RECEIVER
        | kind::JAVADOC_FIELD_REF
        | kind::JAVADOC_VALUE_REF => match p.declaration_signature.as_deref() {
            None => Some("java.lang.Object".to_owned()),
            Some(d) => sig::to_string(d).ok(),
        },
        kind::PACKAGE_REF | kind::MODULE_REF | kind::MODULE_DECLARATION => p.declaration_signature.clone(),
        kind::JAVADOC_TYPE_REF | kind::TYPE_REF => sig::to_string(p.signature()).ok(),
        _ => None,
    }
}

/// `ProposalComparator`.
pub fn compare_proposals(p1: &Proposal, p2: &Proposal) -> Ordering {
    let mut res = p2.relevance - p1.relevance;
    if res == 0 {
        res = p1.kind - p2.kind;
    }
    if res == 0 {
        let c1: Vec<u16> = (if p1.kind == kind::METHOD_DECLARATION { p1.name() } else { p1.completion() }).encode_utf16().collect();
        let c2: Vec<u16> = (if p2.kind == kind::METHOD_DECLARATION { p2.name() } else { p2.completion() }).encode_utf16().collect();
        let mut decided = false;
        for i in 0..c1.len() {
            if i >= c2.len() {
                return Ordering::Less;
            }
            let r = c1[i] as i32 - c2[i] as i32;
            if r != 0 {
                res = r;
                decided = true;
                break;
            }
        }
        if !decided {
            res = c2.len() as i32 - c1.len() as i32;
        }
    }
    let k1 = map_kind(p1);
    let k2 = map_kind(p2);
    if res == 0
        && (k1 == item_kind::METHOD || k1 == item_kind::CONSTRUCTOR)
        && (k2 == item_kind::METHOD || k2 == item_kind::CONSTRUCTOR)
    {
        let n1 = sig::get_parameter_count(p1.signature()).unwrap_or(0) as i32;
        let n2 = sig::get_parameter_count(p2.signature()).unwrap_or(0) as i32;
        res = n1 - n2;
    }
    res.cmp(&0)
}

/// `getEditRange`.
pub fn edit_range(item: &Item, client: &Client) -> Option<EditRange> {
    let te = item.text_edit.as_ref()?;
    if client.insert_replace {
        if let ItemTextEdit::InsertReplace(e) = te {
            return Some(EditRange::InsertReplace { insert: e.insert, replace: e.replace });
        }
        return None;
    }
    Some(EditRange::Range(match te {
        ItemTextEdit::Edit(e) => e.range,
        ItemTextEdit::InsertReplace(e) => e.insert,
    }))
}

/// `initializeCompletionListItemDefaults`.
pub fn initialize_item_defaults(first: &Proposal, provider: &ReplacementProvider, client: &Client, defaults: &mut ItemDefaults) {
    let mut item = Item::default();
    provider.update_replacement(first, &mut item, '\0');
    if item.insert_text_format.is_some() && client.item_defaults_property("insertTextFormat") {
        defaults.insert_text_format = item.insert_text_format;
    }
    if item.text_edit.is_some() && client.item_defaults_property("editRange") {
        defaults.edit_range = edit_range(&item, client);
    }
    if client.item_defaults_property("insertTextMode")
        && client.insert_text_mode_supported(InsertTextMode::ADJUST_INDENTATION)
        && client.insert_text_mode_default != Some(InsertTextMode::ADJUST_INDENTATION)
    {
        defaults.insert_text_mode = Some(InsertTextMode::ADJUST_INDENTATION);
    }
}

/// `toCompletionItem`.
#[allow(clippy::too_many_arguments)]
pub fn to_completion_item(
    proposal: &Proposal,
    index: usize,
    request_id: u64,
    description: &DescriptionProvider,
    provider: &ReplacementProvider,
    client: &Client,
    defaults: &ItemDefaults,
) -> Item {
    let mut item = Item { kind: Some(map_kind(proposal)), ..Default::default() };
    if flags::is(proposal.flags, flags::DEPRECATED) {
        if client.tag_support {
            item.tags = Some(vec![1]);
        } else {
            item.deprecated = Some(true);
        }
    }
    item.data = Some(json!({ DATA_FIELD_REQUEST_ID: request_id.to_string(), DATA_FIELD_PROPOSAL_ID: index.to_string() }));
    description.update_description(proposal, &mut item);
    item.sort_text = Some(sort_text::compute(proposal));
    provider.update_replacement(proposal, &mut item, '\0');
    if let Some(te) = item.text_edit.clone() {
        let new_text = te.new_text().to_owned();
        let range = match &te {
            ItemTextEdit::Edit(e) => e.range,
            ItemTextEdit::InsertReplace(e) => e.insert,
        };
        let label_details = client.label_details;
        let mut filter = String::new();
        if label_details && item.kind == Some(item_kind::METHOD) {
            filter = super::description::method_proposal_description(proposal);
        } else if label_details && item.kind == Some(item_kind::CONSTRUCTOR) {
            if let Some(d) = item.label_details.as_ref().and_then(|d| d.detail.clone()) {
                filter = format!("{new_text}{d}");
            }
        }
        if !filter.is_empty() {
            item.filter_text = Some(filter);
        } else {
            item.filter_text = Some(new_text.clone());
        }
        // multi-line replace ranges collapse to their start
        let fix = |r: &mut Range| {
            if r.end.line != r.start.line {
                r.end = r.start;
            }
        };
        match item.text_edit.as_mut().unwrap() {
            ItemTextEdit::Edit(e) => fix(&mut e.range),
            ItemTextEdit::InsertReplace(e) => {
                if client.insert_replace {
                    fix(&mut e.replace);
                } else {
                    fix(&mut e.insert);
                }
            }
        }
        let _ = range;
        if let Some(er) = &defaults.edit_range {
            if Some(er) == edit_range(&item, client).as_ref() {
                item.text_edit_text = Some(new_text);
                item.text_edit = None;
            }
        }
    }
    if defaults.insert_text_format.is_some() && defaults.insert_text_format == item.insert_text_format {
        item.insert_text_format = None;
    }
    item
}
