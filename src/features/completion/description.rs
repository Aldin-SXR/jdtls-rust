//! Port of jdt.ls `CompletionProposalDescriptionProvider`: labels, label
//! details, details and filter texts of completion items.

use super::doc::Doc;
use super::item::{Item, LabelDetails};
use super::proposal::{flags, kind, Context, Proposal};
use super::signature as sig;
use std::collections::HashMap;

const RETURN_TYPE_SEPARATOR: &str = " : ";
const PACKAGE_NAME_SEPARATOR: &str = " - ";
const VAR_TYPE_SEPARATOR: &str = " : ";
const OBJECT: &str = "java.lang.Object";

pub struct DescriptionProvider<'a> {
    pub context: Option<&'a Context>,
    pub doc: Option<&'a Doc>,
    pub collapsed: Option<&'a HashMap<String, i32>>,
    pub label_details_support: bool,
    /// `guessMethodArguments == OFF || collapseCompletionItems`.
    pub use_required_type_for_constructors: bool,
}

/// `createMethodProposalDescription`.
pub fn method_proposal_description(p: &Proposal) -> String {
    let mut d = String::new();
    match p.kind {
        kind::METHOD_REF
        | kind::METHOD_NAME_REFERENCE
        | kind::POTENTIAL_METHOD_DECLARATION
        | kind::CONSTRUCTOR_INVOCATION
        | kind::METHOD_DECLARATION => {
            d.push_str(p.name());
            d.push('(');
            append_unbounded_parameter_list(&mut d, p);
            d.push(')');
            if !p.constructor {
                d.push_str(RETURN_TYPE_SEPARATOR);
                append_return_type(&mut d, p);
            }
        }
        _ => {}
    }
    d
}

fn append_return_type(d: &mut String, p: &Proposal) {
    let s = sig::fix83600(p.signature());
    if let Ok(rt) = sig::get_return_type(&s) {
        d.push_str(&type_display_name(&sig::get_upper_bound(&rt)));
    }
}

fn type_display_name(type_signature: &str) -> String {
    sig::to_string(type_signature).map(|s| sig::get_simple_name(&s)).unwrap_or_default()
}

pub fn append_unbounded_parameter_list(buf: &mut String, p: &Proposal) {
    let s = sig::fix83600(p.signature());
    let names = p.parameter_names();
    let mut types: Vec<String> = sig::get_parameter_types(&s)
        .unwrap_or_default()
        .iter()
        .map(|t| type_display_name(&sig::get_lower_bound(t)))
        .collect();
    if flags::is(p.flags, flags::VARARGS) && !types.is_empty() {
        let i = types.len() - 1;
        types[i] = convert_to_vararg(&types[i]);
    }
    append_parameter_signature(buf, &types, Some(&names));
}

fn convert_to_vararg(t: &str) -> String {
    if t.len() < 2 || !t.ends_with("[]") {
        return t.to_owned();
    }
    format!("{}...", &t[..t.len() - 2])
}

fn append_parameter_signature(buf: &mut String, types: &[String], names: Option<&[String]>) {
    for (i, t) in types.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push_str(t);
        if let Some(n) = names.and_then(|n| n.get(i)) {
            buf.push(' ');
            buf.push_str(n);
        }
    }
}

fn set_label_details(item: &mut Item, label: Option<String>, detail: Option<String>, description: Option<String>) {
    if let Some(l) = label {
        item.label = l;
    }
    item.label_details = Some(LabelDetails { detail, description });
}

/// `extractDeclaringTypeFQN`.
fn declaring_type_fqn(p: &Proposal) -> String {
    match p.declaration_signature.as_deref() {
        None => OBJECT.to_owned(),
        Some(s) => sig::strip_signature_to_fqn(s).unwrap_or_default(),
    }
}

/// `findSimpleNameStart`.
fn simple_name_start(a: &[char]) -> usize {
    let mut last_dot = 0;
    for (i, &c) in a.iter().enumerate() {
        if c == '<' {
            return last_dot;
        } else if c == '.' {
            last_dot = i + 1;
        }
    }
    last_dot
}

/// `CompletionProposalUtils.getRequiredTypeProposal`.
pub fn required_type_proposal(p: &Proposal) -> Option<&Proposal> {
    if !matches!(
        p.kind,
        kind::CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION | kind::ANONYMOUS_CLASS_DECLARATION
    ) {
        return None;
    }
    p.required().iter().find(|r| r.kind == kind::TYPE_REF)
}

impl<'a> DescriptionProvider<'a> {
    fn in_javadoc(&self) -> bool {
        self.context.is_some_and(|c| c.in_javadoc)
    }

    /// `updateDescription`.
    pub fn update_description(&self, proposal: &Proposal, item: &mut Item) {
        let mut proposal = proposal;
        if self.use_required_type_for_constructors {
            if let Some(r) = required_type_proposal(proposal) {
                proposal = r;
            }
        }
        match proposal.kind {
            kind::METHOD_NAME_REFERENCE
            | kind::METHOD_REF
            | kind::CONSTRUCTOR_INVOCATION
            | kind::METHOD_REF_WITH_CASTED_RECEIVER
            | kind::POTENTIAL_METHOD_DECLARATION => {
                if self.in_javadoc() {
                    self.javadoc_method_label(proposal, item);
                } else {
                    self.method_label(proposal, item);
                }
            }
            kind::METHOD_DECLARATION => self.override_method_label(proposal, item),
            kind::ANONYMOUS_CLASS_DECLARATION | kind::ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION => {
                self.anonymous_type_label(proposal, item)
            }
            kind::TYPE_REF => self.type_proposal_label(proposal, item),
            kind::JAVADOC_TYPE_REF => self.javadoc_type_label(proposal, item),
            kind::JAVADOC_FIELD_REF
            | kind::JAVADOC_VALUE_REF
            | kind::JAVADOC_BLOCK_TAG
            | kind::JAVADOC_INLINE_TAG
            | kind::JAVADOC_PARAM_REF => item.label = proposal.completion().to_owned(),
            kind::JAVADOC_METHOD_REF => self.javadoc_method_label(proposal, item),
            kind::PACKAGE_REF | kind::MODULE_DECLARATION | kind::MODULE_REF => self.package_label(proposal, item),
            kind::ANNOTATION_ATTRIBUTE_REF | kind::FIELD_REF | kind::FIELD_REF_WITH_CASTED_RECEIVER => {
                self.label_with_type_and_declaration(proposal, item)
            }
            kind::LOCAL_VARIABLE_REF | kind::VARIABLE_DECLARATION => self.simple_label_with_type(proposal, item),
            kind::KEYWORD | kind::LABEL_REF => item.label = proposal.completion().to_owned(),
            kind::LAMBDA_EXPRESSION => self.lambda_label(proposal, item),
            _ => {}
        }
    }

    fn method_label(&self, p: &Proposal, item: &mut Item) {
        let mut description = method_proposal_description(p);
        let name = p.name().to_owned();
        let mut skip_detail = false;
        let collapsed_count = self.collapsed.and_then(|c| c.get(&name)).copied().unwrap_or(0);
        if self.label_details_support {
            if collapsed_count > 1 && p.kind != kind::CONSTRUCTOR_INVOCATION {
                set_label_details(item, Some(name.clone()), Some("(...)".into()), Some(format!("{collapsed_count} overloads")));
                skip_detail = true;
            } else {
                let mut params = String::from("(");
                append_unbounded_parameter_list(&mut params, p);
                params.push(')');
                if p.kind != kind::CONSTRUCTOR_INVOCATION {
                    let mut rt = String::new();
                    append_return_type(&mut rt, p);
                    set_label_details(item, Some(name.clone()), Some(params), Some(rt));
                } else {
                    set_label_details(item, Some(name.clone()), Some(params), None);
                }
            }
        } else if collapsed_count > 1 && p.kind != kind::CONSTRUCTOR_INVOCATION {
            item.label = format!("{name}(...)");
            item.detail = Some(format!("{collapsed_count} overloads"));
            skip_detail = true;
        } else {
            item.label = description.clone();
        }
        item.insert_text = Some(name.clone());

        let mut type_info = String::new();
        let declaring = declaring_type_fqn(p);
        let mut qualifier = None;
        if p.has_required() {
            let q = sig::get_qualifier(&declaring);
            if !q.is_empty() {
                type_info.push_str(&q);
                type_info.push('.');
                qualifier = Some(format!("{q}."));
            } else {
                qualifier = Some(q);
            }
        }
        type_info.push_str(&sig::get_simple_name(&declaring));
        if !skip_detail {
            let mut detail = String::new();
            if !type_info.is_empty() {
                detail.push_str(&type_info);
                detail.push('.');
            }
            detail.push_str(&description);
            item.detail = Some(detail);
        }

        if self.doc.is_some() && p.constructor && !type_info.is_empty() && item.data.is_some() && !p.required().is_empty() {
            let r = &p.required()[0];
            let doc = self.doc.unwrap();
            let prefix = doc.get(r.replace_start.max(0) as usize, (r.replace_end - r.replace_start).max(0) as usize);
            if prefix.contains('.') {
                description.insert_str(0, qualifier.as_deref().unwrap_or("null"));
                item.filter_text = Some(description);
            }
        } else if let Some(i) = name.rfind('.') {
            item.filter_text = Some(name[i + 1..].to_owned());
        }
    }

    fn javadoc_method_label(&self, p: &Proposal, item: &mut Item) {
        item.label = p.completion().to_owned();
        item.detail = Some(sig::get_simple_name(&declaring_type_fqn(p)));
        if self.label_details_support && p.kind != kind::CONSTRUCTOR_INVOCATION {
            let mut rt = String::new();
            append_return_type(&mut rt, p);
            set_label_details(item, None, None, Some(rt));
        }
    }

    fn override_method_label(&self, p: &Proposal, item: &mut Item) {
        let name = p.name().to_owned();
        item.insert_text = Some(name.clone());
        let mut params = String::from("(");
        append_unbounded_parameter_list(&mut params, p);
        params.push(')');
        let s = sig::fix83600(p.signature());
        let rt = sig::get_return_type(&s).map(|r| type_display_name(&sig::get_upper_bound(&r))).unwrap_or_default();
        if self.label_details_support {
            set_label_details(item, Some(name.clone()), Some(params), Some(rt));
        } else {
            item.label = format!("{name}{params}{RETURN_TYPE_SEPARATOR}{rt}");
        }
        item.filter_text = Some(name);
        let declaring = sig::get_simple_name(&declaring_type_fqn(p));
        item.detail = Some(format!("Override method in '{declaring}'"));
    }

    fn type_proposal_label(&self, p: &Proposal, item: &mut Item) {
        let signature = if self.in_javadoc() {
            let e = sig::get_type_erasure(p.signature()).unwrap_or_default();
            if p.array_dimensions > 0 {
                sig::create_array_signature(&e, p.array_dimensions as usize)
            } else {
                e
            }
        } else if p.array_dimensions > 0 {
            sig::create_array_signature(p.signature(), p.array_dimensions as usize)
        } else {
            p.signature().to_owned()
        };
        let full = sig::to_string(&signature).unwrap_or_default();
        self.type_label_from_name(&full, item);
    }

    pub fn type_label_from_name(&self, full: &str, item: &mut Item) {
        let chars: Vec<char> = full.chars().collect();
        let q = simple_name_start(&chars);
        let name: String = chars[q..].iter().collect();
        item.filter_text = Some(name.clone());
        item.insert_text = Some(name.clone());
        item.detail = Some(full.to_owned());
        let package = if q > 0 { Some(chars[..q - 1].iter().collect::<String>()) } else { None };
        if self.label_details_support {
            set_label_details(item, Some(name), None, package);
        } else {
            let mut l = name;
            if let Some(pk) = package {
                l.push_str(PACKAGE_NAME_SEPARATOR);
                l.push_str(&pk);
            }
            item.label = l;
        }
    }

    fn javadoc_type_label(&self, p: &Proposal, item: &mut Item) {
        let full: Vec<char> = sig::to_string(p.signature()).unwrap_or_default().chars().collect();
        let q = simple_name_start(&full);
        let name: String = full[q..].iter().collect();
        item.label = format!("{{@link {name}}}");
        item.filter_text = Some(name);
        if q > 0 {
            item.detail = Some(full[..q - 1].iter().collect());
        }
    }

    fn simple_label_with_type(&self, p: &Proposal, item: &mut Item) {
        let type_name = sig::get_signature_simple_name(p.signature());
        let name = p.completion().to_owned();
        item.insert_text = Some(name.clone());
        if self.label_details_support {
            set_label_details(item, Some(name), None, Some(type_name));
        } else {
            let mut l = name;
            if !type_name.is_empty() {
                l.push_str(VAR_TYPE_SEPARATOR);
                l.push_str(&type_name);
            }
            item.label = l;
        }
    }

    fn label_with_type_and_declaration(&self, p: &Proposal, item: &mut Item) {
        let name = if p.completion().starts_with("this.") { p.completion().to_owned() } else { p.name().to_owned() };
        let type_name = sig::get_signature_simple_name(p.signature());
        let mut buf = name.clone();
        item.insert_text = Some(buf.clone());
        if !type_name.is_empty() {
            buf.push_str(VAR_TYPE_SEPARATOR);
            buf.push_str(&type_name);
        }
        if self.label_details_support {
            set_label_details(item, Some(name), None, Some(type_name));
        } else {
            item.label = buf.clone();
        }
        let mut detail = String::new();
        if let Some(decl) = p.declaration_signature.as_deref() {
            let simple = sig::get_signature_simple_name(decl);
            if !simple.is_empty() {
                if p.has_required() {
                    let q = sig::get_qualifier(&declaring_type_fqn(p));
                    if !q.is_empty() {
                        detail.push_str(&q);
                        detail.push('.');
                    }
                }
                detail.push_str(&simple);
            }
        }
        if !detail.is_empty() {
            detail.push('.');
        }
        detail.push_str(&buf);
        item.detail = Some(detail);
    }

    fn package_label(&self, p: &Proposal, item: &mut Item) {
        let decl = p.declaration_signature.clone().unwrap_or_else(|| "null".into());
        item.label = decl.clone();
        let tag = if p.kind == kind::PACKAGE_REF { "(package)" } else { "(module)" };
        item.detail = Some(format!("{tag} {decl}"));
        if self.label_details_support {
            set_label_details(item, None, None, Some(tag.to_owned()));
        }
    }

    fn anonymous_type_label(&self, p: &Proposal, item: &mut Item) {
        let decl = sig::get_type_erasure(p.declaration_signature.as_deref().unwrap_or("")).unwrap_or_default();
        let name = sig::get_signature_simple_name(&decl);
        item.insert_text = Some(name.clone());
        let mut params = String::from("(");
        append_unbounded_parameter_list(&mut params, p);
        params.push(')');
        if self.label_details_support {
            set_label_details(item, Some(name.clone()), Some(params), Some("Anonymous Inner Type".into()));
        } else {
            item.label = format!("{name}{params}  Anonymous Inner Type");
        }
        if p.has_required() {
            let q = sig::get_signature_qualifier(&decl);
            if !q.is_empty() {
                item.detail = Some(format!("{q}.{name}"));
            }
        }
    }

    fn lambda_label(&self, p: &Proposal, item: &mut Item) {
        let mut label = String::from("(");
        append_unbounded_parameter_list(&mut label, p);
        label.push_str(") ->");
        let s = sig::fix83600(p.signature());
        let rt = sig::get_return_type(&s).map(|r| type_display_name(&sig::get_upper_bound(&r))).unwrap_or_default();
        if self.label_details_support {
            set_label_details(item, Some(label), None, Some(rt));
        } else {
            label.push_str(RETURN_TYPE_SEPARATOR);
            label.push_str(&rt);
            item.label = label;
        }
    }
}
