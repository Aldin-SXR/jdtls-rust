//! JSON protocol between the Rust LSP server and the ecj-bridge Java process.
//! Each request/response is a single JSON line on stdin/stdout.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

// ─── Requests (Rust → Java) ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "method", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BridgeRequest {
    Compile {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
    },
    Complete {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
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
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    Navigate {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
        kind: NavKind,
    },
    FindReferences {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    CodeAction {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        range: BridgeRange,
        #[serde(default)]
        diagnostics: Vec<BridgeDiagnostic>,
    },
    /// Data for signature help: the method-like nodes around `search_offset`
    /// (`SignatureHelpContext`) and around `context_offset` (the heuristic
    /// fallback of `SignatureHelpHandler`), with their candidate methods.
    SignatureHelpData {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        search_offset: i64,
        context_offset: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback_name: Option<String>,
        description: bool,
    },
    Rename {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
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
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
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
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
    },
    CodeLens {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
    },
    TypeHierarchyPrepare {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    TypeHierarchySupertypes {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        /// Opaque data from `BridgeTypeHierarchyItem.data` (uri + "\t" + offset)
        data: String,
    },
    TypeHierarchySubtypes {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        data: String,
    },
    CallHierarchyPrepare {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    CallHierarchyIncoming {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    CallHierarchyOutgoing {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
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
#[serde(tag = "method", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
    SignatureHelpData {
        id: u64,
        #[serde(default)]
        chain: Vec<SigNode>,
        fallback: Option<SigNode>,
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
            | BridgeResponse::SignatureHelpData { id, .. }
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

/// A method-like AST node for signature help (see `SignatureHelpService.java`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigNode {
    pub kind: String,
    pub start: i64,
    pub length: i64,
    pub name_end: i64,
    /// `[start, length]` of each AST argument; `None` for non-invocations.
    pub arguments: Option<Vec<[i64; 2]>>,
    pub optional_expression_length: i64,
    pub method_name: Option<String>,
    pub parameter_types: Option<Vec<String>>,
    pub parameter_types_from_binding: Option<Vec<String>>,
    pub bound_method: Option<SigCandidate>,
    pub candidates: Option<Vec<SigCandidate>>,
    pub secondary_candidates: Option<Vec<SigCandidate>>,
    pub declared_constructors: Option<Vec<SigCandidate>>,
    pub scope_candidates: Option<Vec<SigCandidate>>,
}

/// A candidate method, shaped like a JDT completion proposal.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigCandidate {
    pub name: String,
    pub constructor: bool,
    pub varargs: bool,
    /// Identity of the proposal signature (dedup key).
    pub key: String,
    /// Display names, lower bound applied (`SignatureUtil.getLowerBound`).
    pub parameter_types: Vec<String>,
    /// Display name of the return type, upper bound applied; `None` for constructors.
    pub return_type: Option<String>,
    pub parameter_names: Vec<String>,
    /// `SignatureHelpUtils.getSimpleTypeName` of each proposal parameter type.
    pub match_types: Vec<String>,
    /// Simple names of the declared (unsubstituted) parameter types (`IMethod.getParameterTypes`).
    pub declared_types: Vec<String>,
    /// Raw Javadoc comment of the declaration, when requested and available.
    pub javadoc: Option<String>,
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
    /// Command ID to invoke when clicked, or `None` for an informational-only lens.
    pub command: Option<String>,
    /// Human-readable label computed by the bridge (e.g. "2 references").
    pub title: Option<String>,
    /// Pre-computed command arguments from the bridge (avoids a second find_references round-trip).
    pub args: Option<serde_json::Value>,
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
