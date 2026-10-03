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
    SignatureHelp {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        offset: usize,
    },
    /// The element at `offset` (rename and prepareRename selection).
    RenameTarget {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uri: String,
        /// UTF-16 offset into `files[uri]`.
        offset: usize,
    },
    /// Name occurrences (with binding keys) of `names` in `uris`, the method
    /// override relations in their hierarchies and the references to
    /// `package_name`.  Every other entry of `files` is only visible to
    /// binding resolution.
    RenameOccurrences {
        id: u64,
        files: HashMap<String, String>,
        classpath: Vec<String>,
        source_level: String,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        uris: Vec<String>,
        names: Vec<String>,
        #[serde(rename = "packageName", skip_serializing_if = "Option::is_none")]
        package_name: Option<String>,
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
        /// Source folders on disk, for binding resolution across files.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        sourcepath: Vec<String>,
        /// Include expression text (`toString()`) needed for format hints.
        #[serde(rename = "formatParameters")]
        format_parameters: bool,
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
    RenameTarget {
        id: u64,
        select: Option<BridgeRenameElement>,
        prepare: Option<BridgeRenameElement>,
        #[serde(rename = "packageName")]
        package_name: Option<String>,
    },
    RenameOccurrences {
        id: u64,
        files: Vec<BridgeRenameFile>,
        methods: Vec<BridgeRenameMethod>,
        relations: Vec<Vec<String>>,
    },
    InlayHints {
        id: u64,
        #[serde(default)]
        nodes: Vec<BridgeInlayNode>,
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
            | BridgeResponse::RenameTarget { id, .. }
            | BridgeResponse::RenameOccurrences { id, .. }
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

/// An AST node visited by the jdt.ls `InlayHintVisitor`, with its bindings
/// (see `InlayHintService.java`).  Offsets are UTF-16 offsets in the source.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeInlayNode {
    /// DOM node class simple name (`MethodInvocation`, `LambdaExpression`, ...).
    pub kind: String,
    pub start: usize,
    pub length: usize,
    pub method: Option<BridgeInlayMethod>,
    pub arguments: Option<Vec<BridgeInlayExpr>>,
    /// Receiver of a `MethodInvocation`.
    pub expression: Option<BridgeInlayExpr>,
    /// `LambdaExpression`: parameter type names of its method binding.
    pub lambda_parameter_types: Option<Vec<String>>,
    pub lambda_parameters: Option<Vec<BridgeLambdaParameter>>,
    /// `VariableDeclarationStatement`: `getType().isVar()`.
    pub is_var: bool,
    pub fragments: Option<Vec<BridgeVariableFragment>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeInlayExpr {
    pub start: usize,
    pub length: usize,
    pub node: String,
    pub identifier: Option<String>,
    pub literal_value: Option<String>,
    pub text: Option<String>,
    pub inner: Option<Box<BridgeInlayExpr>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeInlayMethod {
    pub name: String,
    pub declaring_type: Option<String>,
    pub declaring_package: Option<String>,
    pub declaring_type_qualified_name: Option<String>,
    pub from_source: bool,
    pub in_target_unit: bool,
    pub synthetic: bool,
    pub record: bool,
    pub varargs: bool,
    pub constructor: bool,
    pub parameter_names: Option<Vec<String>>,
    pub parameter_types: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeLambdaParameter {
    pub node: String,
    pub name_start: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeVariableFragment {
    pub resolved: bool,
    pub initializer: Option<String>,
    pub type_name: Option<String>,
    pub name_start: usize,
    pub name_length: usize,
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

/// An element selected for rename (`RenameBindingService.Element`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeRenameElement {
    /// local, field, enumConstant, method, type, typeParameter, package, other
    pub kind: Option<String>,
    pub key: Option<String>,
    pub name: Option<String>,
    /// UTF-16 offset of the selected name in the target file.
    pub name_start: i64,
    pub name_length: i64,
    pub from_source: bool,
    pub recovered: bool,
    pub anonymous: bool,
    pub is_static: bool,
    pub is_private: bool,
    pub record_component: bool,
    pub top_level: bool,
    pub declaring_type_key: Option<String>,
    pub declaring_type_name: Option<String>,
    pub package_name: Option<String>,
    /// Field type, or method return type (erasure, qualified).
    pub type_name: Option<String>,
    pub param_count: i32,
    pub param_types: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeRenameOccurrence {
    /// UTF-16 offset.
    pub start: usize,
    pub length: usize,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub key: Option<String>,
    /// decl, ref, constructorName, packageDecl, import
    pub role: Option<String>,
    pub declaring_type_key: Option<String>,
    pub param_count: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeRenameFile {
    pub uri: String,
    pub package_name: Option<String>,
    pub occurrences: Vec<BridgeRenameOccurrence>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BridgeRenameMethod {
    pub key: String,
    pub name: Option<String>,
    pub declaring_type_key: Option<String>,
    pub declaring_type_name: Option<String>,
    pub from_source: bool,
    pub is_static: bool,
    pub is_private: bool,
    pub param_types: Option<Vec<String>>,
}
