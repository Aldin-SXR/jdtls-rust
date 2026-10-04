//! LSP 3.17 completion items and list (`itemDefaults`, `textEditText`), which
//! lsp-types 0.94 does not model.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_lsp::lsp_types::{Command, Documentation, InsertReplaceEdit, InsertTextFormat, InsertTextMode, Range, TextEdit};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemTextEdit {
    InsertReplace(InsertReplaceEdit),
    Edit(TextEdit),
}

impl ItemTextEdit {
    pub fn new_text(&self) -> &str {
        match self {
            ItemTextEdit::Edit(e) => &e.new_text,
            ItemTextEdit::InsertReplace(e) => &e.new_text,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_details: Option<LabelDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Documentation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<InsertTextFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_mode: Option<InsertTextMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_edit: Option<ItemTextEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_edit_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_text_edits: Option<Vec<TextEdit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preselect: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_characters: Option<Vec<String>>,
}

/// `CompletionItemKind` values.
pub mod item_kind {
    pub const TEXT: u32 = 1;
    pub const METHOD: u32 = 2;
    pub const CONSTRUCTOR: u32 = 4;
    pub const FIELD: u32 = 5;
    pub const VARIABLE: u32 = 6;
    pub const CLASS: u32 = 7;
    pub const INTERFACE: u32 = 8;
    pub const MODULE: u32 = 9;
    pub const PROPERTY: u32 = 10;
    pub const KEYWORD: u32 = 14;
    pub const SNIPPET: u32 = 15;
    pub const REFERENCE: u32 = 18;
    pub const ENUM_MEMBER: u32 = 20;
    pub const CONSTANT: u32 = 21;
    pub const STRUCT: u32 = 22;
    pub const ENUM: u32 = 13;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EditRange {
    InsertReplace { insert: Range, replace: Range },
    Range(Range),
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_range: Option<EditRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<InsertTextFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_mode: Option<InsertTextMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ItemDefaults {
    pub fn is_empty(&self) -> bool {
        self.edit_range.is_none() && self.insert_text_format.is_none() && self.insert_text_mode.is_none() && self.data.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub is_incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_defaults: Option<ItemDefaults>,
    pub items: Vec<Item>,
}
