//! Raw JDT code-assist data returned by the bridge (`CodeAssistService`):
//! `CompletionProposal`s, the `CompletionContext` and the visible elements
//! used for argument guessing.

use serde::Deserialize;
use std::collections::BTreeMap;

/// `org.eclipse.jdt.core.CompletionProposal` kinds.
pub mod kind {
    pub const ANONYMOUS_CLASS_DECLARATION: i32 = 1;
    pub const FIELD_REF: i32 = 2;
    pub const KEYWORD: i32 = 3;
    pub const LABEL_REF: i32 = 4;
    pub const LOCAL_VARIABLE_REF: i32 = 5;
    pub const METHOD_REF: i32 = 6;
    pub const METHOD_DECLARATION: i32 = 7;
    pub const PACKAGE_REF: i32 = 8;
    pub const TYPE_REF: i32 = 9;
    pub const VARIABLE_DECLARATION: i32 = 10;
    pub const POTENTIAL_METHOD_DECLARATION: i32 = 11;
    pub const METHOD_NAME_REFERENCE: i32 = 12;
    pub const ANNOTATION_ATTRIBUTE_REF: i32 = 13;
    pub const JAVADOC_FIELD_REF: i32 = 14;
    pub const JAVADOC_METHOD_REF: i32 = 15;
    pub const JAVADOC_TYPE_REF: i32 = 16;
    pub const JAVADOC_VALUE_REF: i32 = 17;
    pub const JAVADOC_PARAM_REF: i32 = 18;
    pub const JAVADOC_BLOCK_TAG: i32 = 19;
    pub const JAVADOC_INLINE_TAG: i32 = 20;
    pub const FIELD_IMPORT: i32 = 21;
    pub const METHOD_IMPORT: i32 = 22;
    pub const TYPE_IMPORT: i32 = 23;
    pub const METHOD_REF_WITH_CASTED_RECEIVER: i32 = 24;
    pub const FIELD_REF_WITH_CASTED_RECEIVER: i32 = 25;
    pub const CONSTRUCTOR_INVOCATION: i32 = 26;
    pub const ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION: i32 = 27;
    pub const MODULE_DECLARATION: i32 = 28;
    pub const MODULE_REF: i32 = 29;
    pub const LAMBDA_EXPRESSION: i32 = 30;
}

/// `org.eclipse.jdt.core.Flags`.
pub mod flags {
    pub const PUBLIC: i32 = 0x0001;
    pub const PRIVATE: i32 = 0x0002;
    pub const PROTECTED: i32 = 0x0004;
    pub const STATIC: i32 = 0x0008;
    pub const FINAL: i32 = 0x0010;
    pub const VARARGS: i32 = 0x0080;
    pub const INTERFACE: i32 = 0x0200;
    pub const ABSTRACT: i32 = 0x0400;
    pub const ANNOTATION: i32 = 0x2000;
    pub const ENUM: i32 = 0x4000;
    pub const DEFAULT_METHOD: i32 = 0x0001_0000;
    pub const DEPRECATED: i32 = 0x0010_0000;
    pub const RECORD: i32 = 0x0100_0000;

    pub fn is(f: i32, bit: i32) -> bool {
        f & bit != 0
    }
}

/// `CompletionContext` token locations.
pub mod tl {
    pub const MEMBER_START: i32 = 1;
    pub const STATEMENT_START: i32 = 2;
    pub const CONSTRUCTOR_START: i32 = 4;
    pub const IN_IMPORT: i32 = 8;
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Proposal {
    pub kind: i32,
    pub completion: Option<String>,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub original_signature: Option<String>,
    pub declaration_signature: Option<String>,
    pub declaration_key: Option<String>,
    pub key: Option<String>,
    pub declaration_package_name: Option<String>,
    pub declaration_type_name: Option<String>,
    pub package_name: Option<String>,
    pub type_name: Option<String>,
    pub parameter_names: Option<Vec<String>>,
    pub flags: i32,
    pub additional_flags: i32,
    pub relevance: i32,
    pub replace_start: i32,
    pub replace_end: i32,
    pub token_start: i32,
    pub token_end: i32,
    pub completion_location: i32,
    pub receiver_signature: Option<String>,
    pub receiver_start: i32,
    pub receiver_end: i32,
    pub constructor: bool,
    pub array_dimensions: i32,
    pub accessibility: i32,
    pub can_use_diamond: bool,
    pub compatible: bool,
    pub binding_signature: Option<String>,
    pub declaration_type_variables: Option<Vec<String>>,
    pub required_proposals: Option<Vec<Proposal>>,
    pub type_arguments: Option<Vec<String>>,
    /// Set for proposals jdt.ls creates itself (getter/setter): the field.
    #[serde(skip)]
    pub getter_setter: Option<GetterSetter>,
    /// The aggregated result of the ranking providers (`proposalToRankingResult`).
    #[serde(skip)]
    pub ranking: Option<super::ranking::CompletionRankingAggregation>,
}

#[derive(Debug, Clone)]
pub struct GetterSetter {
    pub field: EnclosingField,
    pub is_getter: bool,
}

impl Proposal {
    pub fn completion(&self) -> &str {
        self.completion.as_deref().unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
    pub fn signature(&self) -> &str {
        self.signature.as_deref().unwrap_or("")
    }
    pub fn required(&self) -> &[Proposal] {
        self.required_proposals.as_deref().unwrap_or(&[])
    }
    pub fn has_required(&self) -> bool {
        self.required_proposals.is_some()
    }
    /// `findParameterNames` (names are always computed by the bridge).
    pub fn parameter_names(&self) -> Vec<String> {
        if let Some(n) = &self.parameter_names {
            return n.clone();
        }
        let count = self
            .signature
            .as_deref()
            .and_then(|s| super::signature::get_parameter_count(&super::signature::fix83600(s)).ok())
            .unwrap_or(0);
        default_parameter_names(count)
    }
}

/// `CompletionEngine.createDefaultParameterNames`.
pub fn default_parameter_names(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("arg{i}")).collect()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Context {
    pub offset: i32,
    pub token: Option<String>,
    pub token_start: i32,
    pub token_end: i32,
    pub token_kind: i32,
    pub token_location: i32,
    pub in_javadoc: bool,
    pub in_javadoc_text: bool,
    pub in_javadoc_formal_reference: bool,
    pub extended: bool,
    pub expected_types_signatures: Option<Vec<String>>,
    pub expected_types_keys: Option<Vec<String>>,
    pub enclosing_kind: Option<String>,
    pub enclosing_type_name: Option<String>,
    pub enclosing_static: bool,
    pub enclosing_interface: bool,
    pub enclosing_method_name: Option<String>,
    /// The engine's completion node class (`CompletionOnKeyword2`, ...).
    pub completion_node: Option<String>,
    pub completion_node_parent: Option<String>,
    pub enclosing_fields: Vec<EnclosingField>,
    pub enclosing_methods: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnclosingField {
    pub name: String,
    pub type_signature: String,
    pub flags: i32,
    pub is_enum_constant: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VisibleElement {
    /// 0 local, 1 field, 2 method.
    pub kind: i32,
    pub name: String,
    pub type_signature: Option<String>,
    pub parameter_count: i32,
    pub return_type: Option<String>,
    pub inherited: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineResult {
    pub context: Option<Context>,
    pub proposals: Vec<Proposal>,
    pub visible_elements: BTreeMap<String, Vec<VisibleElement>>,
    /// Proposal kinds the engine asked about (`isIgnored(kind)`).
    pub completion_kinds: Vec<i32>,
}
