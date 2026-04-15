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
        public String uri;
        public int offset;
        public String kind;     // NavKind for Navigate requests
        public BridgeRange range;
        public String newName;
        public String importPrefix; // pre-computed import prefix from Rust (avoids race condition)
        public String source;   // for Format requests
        public int tabSize;
        public boolean insertSpaces;
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

    public static class SignatureHelpResponse extends Response {
        public List<BridgeSignature> signatures;
        public int activeSignature, activeParameter;
        public SignatureHelpResponse(long id, List<BridgeSignature> sigs, int as_, int ap) {
            this.id = id; this.method = "signatureHelp";
            this.signatures = sigs; this.activeSignature = as_; this.activeParameter = ap;
        }
    }

    public static class WorkspaceEditResponse extends Response {
        public List<BridgeFileEdit> changes;
        public WorkspaceEditResponse(long id, List<BridgeFileEdit> changes) {
            this.id = id; this.method = "workspaceEdit"; this.changes = changes;
        }
    }

    public static class TextEditsResponse extends Response {
        public String uri;
        public List<BridgeTextEdit> edits;
        public TextEditsResponse(long id, String uri, List<BridgeTextEdit> edits) {
            this.id = id; this.method = "textEdits"; this.uri = uri; this.edits = edits;
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

    public static class BridgeSignature {
        public String label;
        public String documentation;
        public List<BridgeParameter> parameters;
    }

    public static class BridgeParameter {
        public String label;
        public String documentation;
    }

    public static class BridgeInlayHint {
        public int line;
        public int character;
        public String label;
        public int kind; // 1=Type, 2=Parameter
    }

    public static class InlayHintsResponse extends Response {
        public List<BridgeInlayHint> hints;
        public InlayHintsResponse(long id, List<BridgeInlayHint> hints) {
            this.id = id; this.method = "inlayHints"; this.hints = hints;
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
}
