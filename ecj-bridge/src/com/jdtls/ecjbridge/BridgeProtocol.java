package com.jdtls.ecjbridge;

import java.util.List;
import java.util.Map;

/**
 * JSON protocol data classes for communication with the Rust LSP server.
 * All classes use Gson for serialization; field names match Rust's serde camelCase convention.
 */
public class BridgeProtocol {

    // ─── Requests ────────────────────────────────────────────────────────────

    public static class Request {
        public long id;
        public String method;
        public Map<String, String> files;
        public List<String> classpath;
        public String sourceLevel;
        public Map<String, String> options; // effective JDT core options (computed by Rust)
        public String uri;
        public int offset;
        public String kind;     // NavKind for Navigate requests
        public BridgeRange range;
        public String newName;
        public String importPrefix; // pre-computed import prefix from Rust (avoids race condition)
        public String source;   // for Format requests
        public int formatKind;  // CodeFormatter kind | flags (Format)
        public int length;      // region length (Format), UTF-16 units
        public int indentationLevel;
        public String lineSeparator;
        public List<BridgeDiagnostic> diagnostics;
        public String data;    // opaque data passed back for typeHierarchy supertypes/subtypes
        // navData / classFileContents / classFileInfo (NavigationDataService)
        public String op;
        public int line, character;
        public ClassFileService.ClassFileDesc classFile;
        public Map<String, String> sourceAttachments; // library path -> source attachment path
        public boolean includeClassFiles;
        public Boolean includeDecompiled;
        public boolean includeDeclaration;
        public Boolean includeAccessors;
        public String fqn;
        public List<String> libraries;     // navData references: library roots in search order ("jrt" = JDK)
        public List<String> skipLibraries; // navData references: library roots already searched
        public List<String> searchKeys;    // navData referencesByKeys: "<includeDeclaration>|<binding key>"
        public List<String> uris;        // renameOccurrences: units to resolve; compile: units to compile
        public Map<String, String> expectedPackages; // compile: package each unit must declare
        public List<String> names;       // renameOccurrences: identifiers of interest
        public String packageName;       // renameOccurrences: package whose references to collect
        public com.google.gson.JsonObject query; // semanticSearch: SemanticIndexService query
        public List<String> sourcepath; // source folders on disk (inlayHints binding environment)
        public boolean formatParameters; // inlayHints: include expression text for format hints
        // signatureHelpData
        public int searchOffset = -1;
        public int contextOffset = -1;
        public String fallbackName;
        public boolean description;
    }

    public static class BridgeRange {
        public int startLine, startChar, endLine, endChar;
    }

    // ─── Responses ───────────────────────────────────────────────────────────

    public static class Response {
        public long id;
        public String method;
    }

    public static class DiagnosticsResponse extends Response {
        public List<BridgeDiagnostic> items;
        public DiagnosticsResponse(long id, List<BridgeDiagnostic> items) {
            this.id = id; this.method = "diagnostics"; this.items = items;
        }
    }

    public static class CompletionsResponse extends Response {
        public List<BridgeCompletion> items;
        public CompletionsResponse(long id, List<BridgeCompletion> items) {
            this.id = id; this.method = "completions"; this.items = items;
        }
    }

    public static class HoverResponse extends Response {
        public String contents;
        public HoverResponse(long id, String contents) {
            this.id = id; this.method = "hover"; this.contents = contents;
        }
    }

    public static class LocationsResponse extends Response {
        public List<BridgeLocation> locations;
        public LocationsResponse(long id, List<BridgeLocation> locations) {
            this.id = id; this.method = "locations"; this.locations = locations;
        }
    }

    public static class CodeActionsResponse extends Response {
        public List<BridgeAction> actions;
        public CodeActionsResponse(long id, List<BridgeAction> actions) {
            this.id = id; this.method = "codeActions"; this.actions = actions;
        }
    }

    /** Data for signature help (selection and shaping happen in Rust). */
    public static class SignatureHelpDataResponse extends Response {
        public List<SigNode> chain = new java.util.ArrayList<>();
        public SigNode fallback;
        public SignatureHelpDataResponse(long id) {
            this.id = id; this.method = "signatureHelpData";
        }
    }

    /** A method-like AST node and what the completion engine proposes for it. */
    public static class SigNode {
        public String kind;
        public int start, length;
        public int nameEnd = -1;
        public List<int[]> arguments;
        public int optionalExpressionLength;
        public String methodName;
        public List<String> parameterTypes;
        public List<String> parameterTypesFromBinding;
        public SigCandidate boundMethod;
        public List<SigCandidate> candidates;
        public List<SigCandidate> secondaryCandidates;
        public List<SigCandidate> declaredConstructors;
        public List<SigCandidate> scopeCandidates;
    }

    /** One method binding, as a completion proposal would describe it. */
    public static class SigCandidate {
        public String name;
        public boolean constructor;
        public boolean varargs;
        public String key;
        public List<String> parameterTypes;
        public String returnType;
        public List<String> parameterNames;
        public List<String> matchTypes;
        public List<String> declaredTypes;
        public String javadoc;
    }

    public static class WorkspaceEditResponse extends Response {
        public List<BridgeFileEdit> changes;
        public WorkspaceEditResponse(long id, List<BridgeFileEdit> changes) {
            this.id = id; this.method = "workspaceEdit"; this.changes = changes;
        }
    }

    public static class RenameTargetResponse extends Response {
        public RenameBindingService.Element select;
        public RenameBindingService.Element prepare;
        public String packageName;
        public RenameTargetResponse(long id, RenameBindingService.TargetResult r) {
            this.id = id; this.method = "renameTarget";
            this.select = r.select; this.prepare = r.prepare; this.packageName = r.packageName;
        }
    }

    public static class RenameOccurrencesResponse extends Response {
        public List<RenameBindingService.FileOccurrences> files;
        public List<RenameBindingService.MethodInfo> methods;
        public List<List<String>> relations;
        public RenameOccurrencesResponse(long id, RenameBindingService.OccurrencesResult r) {
            this.id = id; this.method = "renameOccurrences";
            this.files = r.files; this.methods = r.methods; this.relations = r.relations;
        }
    }

    public static class TextEditsResponse extends Response {
        public String uri;
        public List<BridgeTextEdit> edits;
        public TextEditsResponse(long id, String uri, List<BridgeTextEdit> edits) {
            this.id = id; this.method = "textEdits"; this.uri = uri; this.edits = edits;
        }
    }

    public static class FormatEditsResponse extends Response {
        public List<BridgeFormatEdit> edits; // null when the formatter returned null
        public FormatEditsResponse(long id, List<BridgeFormatEdit> edits) {
            this.id = id; this.method = "formatEdits"; this.edits = edits;
        }
    }

    public static class SemanticSearchResponse extends Response {
        public Object result;
        public SemanticSearchResponse(long id, Object result) {
            this.id = id; this.method = "semanticSearch"; this.result = result;
        }
    }

    public static class OkResponse extends Response {
        public OkResponse(long id) { this.id = id; this.method = "ok"; }
    }

    public static class ErrorResponse extends Response {
        public String message;
        public ErrorResponse(long id, String message) {
            this.id = id; this.method = "error"; this.message = message;
        }
    }

    // ─── Shared data types ────────────────────────────────────────────────────

    public static class BridgeDiagnostic {
        public String uri;
        public int startLine, startChar, endLine, endChar;
        public int severity;  // 1=Error 2=Warning 3=Info 4=Hint
        public String message;
        public String code;
        public int categoryId;   // CategorizedProblem.CAT_* constant
        public List<Integer> tags;
    }

    public static class BridgeCompletion {
        public String label;
        public int kind;  // LSP CompletionItemKind
        public String detail;
        public String documentation;
        public String insertText;
        public Integer insertTextFormat;  // 1=plain 2=snippet
        public String sortText;
        public String filterText;
        public List<BridgeTextEdit> additionalEdits;
    }

    public static class BridgeLocation {
        public String uri;
        public int startLine, startChar, endLine, endChar;
    }

    public static class BridgeAction {
        public String title;
        public String kind;
        public List<BridgeFileEdit> edits;
        public boolean isPreferred;
    }

    public static class BridgeFileEdit {
        public String uri;
        public List<BridgeTextEdit> edits;
    }

    public static class BridgeTextEdit {
        public int startLine, startChar, endLine, endChar;
        public String newText;
    }

    /** A formatter leaf edit: replace {@code length} chars at {@code offset}. */
    public static class BridgeFormatEdit {
        public int offset, length;
        public String text;
    }

    public static class BridgeSignature {
        public String label;
        public String documentation;
        public List<BridgeParameter> parameters;
    }

    public static class BridgeParameter {
        public String label;
        public String documentation;
    }

    public static class InlayHintsResponse extends Response {
        public List<InlayHintService.Node> nodes;
        public InlayHintsResponse(long id, List<InlayHintService.Node> nodes) {
            this.id = id; this.method = "inlayHints"; this.nodes = nodes;
        }
    }

    public static class BridgeCodeLens {
        public int startLine, startChar, endLine, endChar;
        public String title;
        public String command;    // null = informational (no click action)
        public List<Object> args; // arguments passed to the command
    }

    public static class CodeLensResponse extends Response {
        public List<BridgeCodeLens> lenses;
        public CodeLensResponse(long id, List<BridgeCodeLens> lenses) {
            this.id = id; this.method = "codeLenses"; this.lenses = lenses;
        }
    }

    // ─── Call Hierarchy ───────────────────────────────────────────────────────

    public static class BridgeCallHierarchyItem {
        public String name;
        public int kind;       // LSP SymbolKind: 6=Method, 9=Constructor, 5=Class
        public String detail;  // declaring class name
        public String uri;
        // full method range
        public int startLine, startChar, endLine, endChar;
        // name-only (selectionRange)
        public int selStartLine, selStartChar, selEndLine, selEndChar;
    }

    public static class BridgeCallFromRange {
        public int startLine, startChar, endLine, endChar;
    }

    public static class BridgeCallHierarchyIncomingCall {
        public BridgeCallHierarchyItem from;
        public List<BridgeCallFromRange> fromRanges;
    }

    public static class BridgeCallHierarchyOutgoingCall {
        public BridgeCallHierarchyItem to;
        public List<BridgeCallFromRange> fromRanges;
    }

    public static class CallHierarchyPrepareResponse extends Response {
        public List<BridgeCallHierarchyItem> items;
        public CallHierarchyPrepareResponse(long id, List<BridgeCallHierarchyItem> items) {
            this.id = id; this.method = "callHierarchyPrepare"; this.items = items;
        }
    }

    public static class CallHierarchyIncomingCallsResponse extends Response {
        public List<BridgeCallHierarchyIncomingCall> calls;
        public CallHierarchyIncomingCallsResponse(long id, List<BridgeCallHierarchyIncomingCall> calls) {
            this.id = id; this.method = "callHierarchyIncomingCalls"; this.calls = calls;
        }
    }

    public static class CallHierarchyOutgoingCallsResponse extends Response {
        public List<BridgeCallHierarchyOutgoingCall> calls;
        public CallHierarchyOutgoingCallsResponse(long id, List<BridgeCallHierarchyOutgoingCall> calls) {
            this.id = id; this.method = "callHierarchyOutgoingCalls"; this.calls = calls;
        }
    }

    // ─── Type Hierarchy ───────────────────────────────────────────────────────

    public static class BridgeTypeHierarchyItem {
        public String name;
        public int kind;       // LSP SymbolKind: 5=Class, 11=Interface, 10=Enum
        public String detail;  // package name
        public String uri;
        // full type range
        public int startLine, startChar, endLine, endChar;
        // name-only (selectionRange)
        public int selStartLine, selStartChar, selEndLine, selEndChar;
        public String data;    // opaque: "uri\toffset" used to re-resolve in supertypes/subtypes calls
    }

    public static class TypeHierarchyPrepareResponse extends Response {
        public List<BridgeTypeHierarchyItem> items;
        public TypeHierarchyPrepareResponse(long id, List<BridgeTypeHierarchyItem> items) {
            this.id = id; this.method = "typeHierarchyPrepare"; this.items = items;
        }
    }

    public static class TypeHierarchyItemsResponse extends Response {
        public List<BridgeTypeHierarchyItem> items;
        public TypeHierarchyItemsResponse(long id, String method, List<BridgeTypeHierarchyItem> items) {
            this.id = id; this.method = method; this.items = items;
        }
    }
}
