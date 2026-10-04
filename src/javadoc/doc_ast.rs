//! The JDT Javadoc DOM (`org.eclipse.jdt.core.dom.Javadoc`) as serialized by
//! the bridge (`HoverService.DocWriter`), plus the binding data hover needs.
//!
//! Node positions are UTF-16 offsets relative to the start of the raw comment,
//! which is exactly what `CoreJavadocContentAccessUtility.getJavadocNode`
//! produces when it parses `rawJavadoc + "class C{}"`.

use serde::Deserialize;
use std::collections::BTreeMap;

/// A Javadoc comment with its DOM.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocSource {
    /// The raw comment text (`/** ... */` or the `///` block).
    pub raw: String,
    /// The compilation unit declaring the documented element.
    pub uri: Option<String>,
    /// Or the class file whose attached source declares it.
    pub class_file: Option<crate::classfile::ClassFileDesc>,
    /// `Javadoc.tags()` (node ids).
    pub tags: Vec<usize>,
    pub nodes: Vec<DocNode>,
    pub markdown: bool,
}

/// One `ASTNode` of the Javadoc DOM.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocNode {
    pub id: usize,
    /// Parent node id (`None` for top-level tags).
    pub p: Option<usize>,
    /// Start (UTF-16, relative to the comment) and length.
    pub s: i64,
    pub l: i64,
    pub malformed: bool,
    /// Kind: `tag`, `text`, `doctext`, `region`, `tagProperty`, `name`,
    /// `memberRef`, `methodRef`, `other`.
    pub t: String,
    pub tag_name: Option<String>,
    pub fragments: Vec<usize>,
    pub tag_properties: Vec<usize>,
    pub props: Option<SnippetProps>,
    pub text: Option<String>,
    /// `Name.getFullyQualifiedName()`.
    pub fqn: Option<String>,
    pub identifier: Option<String>,
    /// MemberRef / MethodRef qualifier (`None` when absent).
    pub qualifier: Option<String>,
    /// MemberRef / MethodRef member name, or TagProperty name.
    pub name: Option<String>,
    pub params: Vec<MethodRefParam>,
    /// Resolved reference target (`Some(empty)` = unresolved reference).
    pub link: Option<Location>,
    /// `{@value}` constant of a member reference.
    pub value: Option<ValueRef>,
    /// JavaDocRegion tags.
    pub tags: Vec<usize>,
    pub dummy: bool,
    pub string_value: Option<String>,
    pub node_value: Option<usize>,
}

impl DocNode {
    pub fn is_tag(&self) -> bool {
        self.t == "tag"
    }
    pub fn is_text(&self) -> bool {
        self.t == "text"
    }
    /// `AbstractTextElement` (TextElement or JavaDocTextElement).
    pub fn is_abstract_text(&self) -> bool {
        self.t == "text" || self.t == "doctext"
    }
    pub fn is_name(&self) -> bool {
        self.t == "name"
    }
    pub fn is_simple_name(&self) -> bool {
        self.t == "name" && self.fqn.as_deref() == self.identifier.as_deref()
    }
    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }
    pub fn tag_name(&self) -> Option<&str> {
        self.tag_name.as_deref()
    }
    pub fn end(&self) -> i64 {
        self.s + self.l
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SnippetProps {
    pub valid: Option<bool>,
    pub error: Option<String>,
    pub id: Option<String>,
    pub inline_tag_count: Option<i64>,
    /// Node id of `TAG_PROPERTY_SNIPPET_REGION_TEXT`.
    pub region_text: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MethodRefParam {
    /// `ASTNodes.asString(param.getType())`
    #[serde(rename = "type")]
    pub ty: String,
    pub name: Option<String>,
}

/// `JDTUtils.toLocation(element)` as data.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Location {
    pub uri: Option<String>,
    /// 0-based line of the element name.
    pub line: Option<u32>,
    pub class_file: Option<crate::classfile::ClassFileDesc>,
}

impl Location {
    pub fn is_empty(&self) -> bool {
        self.uri.is_none() && self.class_file.is_none()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ValueRef {
    pub constant: Option<Constant>,
    pub location: Option<Location>,
}

/// A compile-time constant value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Constant {
    /// `string`, `char` or `other`.
    pub kind: String,
    pub value: String,
}

/// What `CoreJavadocAccessImpl` knows about the documented element.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocContext {
    /// `method`, `field`, `type`, `package`.
    pub kind: String,
    pub is_constructor: bool,
    pub type_parameter_names: Vec<String>,
    pub parameter_names: Vec<String>,
    pub exception_names: Vec<String>,
    pub exception_links: BTreeMap<String, Location>,
    pub returns_void: bool,
    pub static_final: bool,
    pub constant: Option<Constant>,
}

/// Supertype hierarchy for `{@inheritDoc}` (`InheritDocVisitor` input).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InheritData {
    pub start: String,
    pub object: Option<String>,
    pub types: Vec<HierarchyType>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HierarchyType {
    pub key: String,
    pub name: String,
    pub is_interface: bool,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    /// The method of this type overridden by the hovered method.
    pub overridden: Option<OverriddenMethod>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverriddenMethod {
    pub name: String,
    pub has_parameters: bool,
    pub declaring_type_name: String,
    pub has_source: bool,
    pub doc_context: DocContext,
    pub javadoc: Option<DocSource>,
}

/// UTF-16 indexed view of a string (Java `String.substring` semantics).
#[derive(Debug, Clone)]
pub struct Utf16 {
    units: Vec<u16>,
}

impl Utf16 {
    pub fn new(s: &str) -> Self {
        Self { units: s.encode_utf16().collect() }
    }
    pub fn len(&self) -> usize {
        self.units.len()
    }
    pub fn substring(&self, start: i64, end: i64) -> String {
        let len = self.units.len() as i64;
        let s = start.clamp(0, len) as usize;
        let e = end.clamp(0, len) as usize;
        if s >= e {
            return String::new();
        }
        String::from_utf16_lossy(&self.units[s..e])
    }
}
