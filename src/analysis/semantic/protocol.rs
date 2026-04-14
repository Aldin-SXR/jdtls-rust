//! JSON protocol between the Rust LSP server and the ecj-bridge Java process.
//! Each request/response is a single JSON line on stdin/stdout.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    InlayHintsResult {
        id: u64,
        hints: Vec<BridgeInlayHint>,
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
            | BridgeResponse::InlayHintsResult { id, .. }
            | BridgeResponse::Ok { id }
            | BridgeResponse::Error { id, .. } => *id,
        }
    }
}

// ─── Shared data types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
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
