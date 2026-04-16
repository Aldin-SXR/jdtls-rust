//! JSON protocol between the Rust LSP server and the ecj-bridge Java process.
//! Each request/response is a single JSON line on stdin/stdout.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_json::Value;

// ─── Requests (Rust → Java) ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum BridgeRequest {
    Compile {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
    },
    Complete {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
        /// If the cursor is inside an `import` statement, the prefix typed so far
        /// (e.g. "java." or "java.util."). Computed by the Rust server from the
        /// authoritative document-store content to avoid race conditions.
        #[serde(skip_serializing_if = "Option::is_none")]
        import_prefix: Option<String>,
    },
    Hover {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    Navigate {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
        kind: NavKind,
    },
    FindReferences {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    CodeAction {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        range: BridgeRange,
        #[serde(default)]
        diagnostics: Vec<BridgeDiagnostic>,
    },
    SignatureHelp {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    Rename {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
        #[serde(rename = "newName")]
        new_name: String,
    },
    OrganizeImports {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
    },
    Format {
        id: u64,
        source: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    },
    InlayHints {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
    },
    CodeLens {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
    },
    TypeHierarchyPrepare {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    TypeHierarchySupertypes {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        /// Opaque data from `BridgeTypeHierarchyItem.data` (uri + "\t" + offset)
        data: String,
    },
    TypeHierarchySubtypes {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        data: String,
    },
    CallHierarchyPrepare {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    CallHierarchyIncoming {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    CallHierarchyOutgoing {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        uri: String,
        offset: usize,
    },
    Shutdown { id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NavKind {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRange {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
}

// ─── Responses (Java → Rust) ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum BridgeResponse {
    Diagnostics {
        id: u64,
        items: Vec<BridgeDiagnostic>,
    },
    Completions {
        id: u64,
        items: Vec<BridgeCompletion>,
    },
    Hover {
        id: u64,
        contents: String,
    },
    Locations {
        id: u64,
        locations: Vec<BridgeLocation>,
    },
    CodeActions {
        id: u64,
        actions: Vec<BridgeAction>,
    },
    SignatureHelp {
        id: u64,
        signatures: Vec<BridgeSignature>,
        #[serde(rename = "activeSignature")]
        active_signature: u32,
        #[serde(rename = "activeParameter")]
        active_parameter: u32,
    },
    WorkspaceEdit {
        id: u64,
        changes: Vec<BridgeFileEdit>,
    },
    TextEdits {
        id: u64,
        uri: String,
        edits: Vec<BridgeTextEdit>,
    },
    InlayHints {
        id: u64,
        hints: Vec<BridgeInlayHint>,
    },
    CodeLenses {
        id: u64,
        lenses: Vec<BridgeCodeLens>,
    },
    TypeHierarchyPrepare {
        id: u64,
        items: Vec<BridgeTypeHierarchyItem>,
    },
    TypeHierarchySupertypes {
        id: u64,
        items: Vec<BridgeTypeHierarchyItem>,
    },
    TypeHierarchySubtypes {
        id: u64,
        items: Vec<BridgeTypeHierarchyItem>,
    },
    CallHierarchyPrepare {
        id: u64,
        items: Vec<BridgeCallHierarchyItem>,
    },
    CallHierarchyIncomingCalls {
        id: u64,
        calls: Vec<BridgeCallHierarchyIncomingCall>,
    },
    CallHierarchyOutgoingCalls {
        id: u64,
        calls: Vec<BridgeCallHierarchyOutgoingCall>,
    },
    Ok { id: u64 },
    Error {
        id: u64,
        message: String,
    },
}

impl BridgeResponse {
    pub fn id(&self) -> u64 {
        match self {
            BridgeResponse::Diagnostics { id, .. }
            | BridgeResponse::Completions { id, .. }
            | BridgeResponse::Hover { id, .. }
            | BridgeResponse::Locations { id, .. }
            | BridgeResponse::CodeActions { id, .. }
            | BridgeResponse::SignatureHelp { id, .. }
            | BridgeResponse::WorkspaceEdit { id, .. }
            | BridgeResponse::TextEdits { id, .. }
            | BridgeResponse::InlayHints { id, .. }
            | BridgeResponse::CodeLenses { id, .. }
            | BridgeResponse::TypeHierarchyPrepare { id, .. }
            | BridgeResponse::TypeHierarchySupertypes { id, .. }
            | BridgeResponse::TypeHierarchySubtypes { id, .. }
            | BridgeResponse::CallHierarchyPrepare { id, .. }
            | BridgeResponse::CallHierarchyIncomingCalls { id, .. }
            | BridgeResponse::CallHierarchyOutgoingCalls { id, .. }
            | BridgeResponse::Ok { id }
            | BridgeResponse::Error { id, .. } => *id,
        }
    }
}

// ─── Shared data types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeDiagnostic {
    pub uri: String,
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub severity: u8, // 1=Error 2=Warning 3=Info 4=Hint
    pub message: String,
    pub code: Option<String>,
    #[serde(default)]
    pub category_id: u32,
    pub tags: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCompletion {
    pub label: String,
    pub kind: u8, // LSP CompletionItemKind numeric value
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: Option<String>,
    pub insert_text_format: Option<u8>, // 1=plain 2=snippet
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub additional_edits: Option<Vec<BridgeTextEdit>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLocation {
    pub uri: String,
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edits: Vec<BridgeFileEdit>,
    #[serde(default)]
    pub is_preferred: bool,
}

#[derive(Debug, Deserialize)]
pub struct BridgeFileEdit {
    pub uri: String,
    pub edits: Vec<BridgeTextEdit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTextEdit {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub new_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSignature {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<BridgeParameter>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeParameter {
    pub label: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInlayHint {
    pub line: u32,
    pub character: u32,
    pub label: String,
    pub kind: u8, // 1=Type, 2=Parameter
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCodeLens {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub title: String,
    /// Command ID to invoke when clicked, or `None` for an informational-only lens.
    pub command: Option<String>,
    /// Arguments forwarded to the command.
    pub args: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTypeHierarchyItem {
    pub name: String,
    pub kind: u8, // 5=Class, 10=Enum, 11=Interface
    pub detail: Option<String>,
    pub uri: String,
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub sel_start_line: u32,
    pub sel_start_char: u32,
    pub sel_end_line: u32,
    pub sel_end_char: u32,
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCallHierarchyItem {
    pub name: String,
    pub kind: u8, // 6=Method, 9=Constructor, 5=Class
    pub detail: Option<String>,
    pub uri: String,
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub sel_start_line: u32,
    pub sel_start_char: u32,
    pub sel_end_line: u32,
    pub sel_end_char: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCallFromRange {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCallHierarchyIncomingCall {
    pub from: BridgeCallHierarchyItem,
    pub from_ranges: Vec<BridgeCallFromRange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCallHierarchyOutgoingCall {
    pub to: BridgeCallHierarchyItem,
    pub from_ranges: Vec<BridgeCallFromRange>,
}
