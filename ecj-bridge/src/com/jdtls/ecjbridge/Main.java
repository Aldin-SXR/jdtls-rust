package com.jdtls.ecjbridge;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

import java.io.*;
import java.util.*;
import java.util.logging.*;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * Entry point for the ecj-bridge process.
 *
 * Reads newline-delimited JSON requests from stdin, writes newline-delimited
 * JSON responses to stdout.  Logs to stderr so stdout stays clean for the
 * Rust LSP server.
 *
 * Protocol: each request has a "method" field that dispatches to the
 * appropriate service.  Responses mirror the id from the request.
 */
public class Main {

    private static final Logger LOG = Logger.getLogger(Main.class.getName());
    private static final Gson GSON = new GsonBuilder().serializeNulls().create();

    public static void main(String[] args) throws Exception {
        // Configure logging to stderr only
        // Route JUL to stderr (default ConsoleHandler already writes to System.err)
        LogManager.getLogManager().reset();
        Logger root = Logger.getLogger("");
        root.setLevel(Level.WARNING);
        // Re-direct root handler to stderr explicitly
        java.util.logging.StreamHandler stderrHandler =
            new java.util.logging.StreamHandler(System.err, new java.util.logging.SimpleFormatter());
        stderrHandler.setLevel(Level.WARNING);
        root.addHandler(stderrHandler);

        // Line-buffered I/O
        BufferedReader stdin = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        PrintWriter stdout = new PrintWriter(new OutputStreamWriter(System.out, "UTF-8"), true);

        CompilationService compiler = new CompilationService();
        CompletionService completer = new CompletionService();
        FormatterService formatter = new FormatterService();

        LOG.info("ecj-bridge ready");

        String line;
        while ((line = stdin.readLine()) != null) {
            if (line.trim().isEmpty()) continue;
            try {
                Request req = GSON.fromJson(line, Request.class);
                Object response = dispatch(req, compiler, completer, formatter);
                stdout.println(GSON.toJson(response));
            } catch (Exception e) {
                // Try to extract the id from raw JSON
                long id = extractId(line);
                ErrorResponse err = new ErrorResponse(id, e.getClass().getSimpleName() + ": " + e.getMessage());
                stdout.println(GSON.toJson(err));
                LOG.log(Level.SEVERE, "Error processing request: " + line, e);
            }
        }

        LOG.info("ecj-bridge stdin closed, exiting");
    }

    private static Object dispatch(Request req,
                                   CompilationService compiler,
                                   CompletionService completer,
                                   FormatterService formatter) {
        return switch (req.method) {
            case "compile" -> {
                List<BridgeDiagnostic> diags = compiler.compile(
                    req.files, orEmpty(req.classpath), orDefault(req.sourceLevel));
                yield new DiagnosticsResponse(req.id, diags);
            }
            case "complete" -> {
                List<BridgeCompletion> items = completer.complete(
                    req.files, orEmpty(req.classpath), orDefault(req.sourceLevel),
                    req.uri, req.offset);
                yield new CompletionsResponse(req.id, items);
            }
            case "hover" -> {
                // Hover: compile and extract Javadoc for element at offset
                String hover = extractHover(req, compiler);
                yield new HoverResponse(req.id, hover);
            }
            case "navigate" -> {
                List<BridgeLocation> locs = navigate(req, compiler);
                yield new LocationsResponse(req.id, locs);
            }
            case "findReferences" -> {
                List<BridgeLocation> refs = findReferences(req, compiler);
                yield new LocationsResponse(req.id, refs);
            }
            case "codeAction" -> {
                List<BridgeAction> actions = codeActions(req, compiler);
                yield new CodeActionsResponse(req.id, actions);
            }
            case "signatureHelp" -> {
                List<BridgeSignature> sigs = signatureHelp(req, compiler);
                yield new SignatureHelpResponse(req.id, sigs, 0, 0);
            }
            case "rename" -> {
                List<BridgeFileEdit> edits = rename(req, compiler);
                yield new WorkspaceEditResponse(req.id, edits);
            }
            case "organizeImports" -> {
                List<BridgeTextEdit> edits = organizeImports(req, compiler);
                yield new TextEditsResponse(req.id, req.uri, edits);
            }
            case "format" -> {
                List<BridgeTextEdit> edits = formatter.format(req.source, req.tabSize, req.insertSpaces);
                yield new TextEditsResponse(req.id, req.uri, edits);
            }
            case "shutdown" -> new OkResponse(req.id);
            default -> new ErrorResponse(req.id, "Unknown method: " + req.method);
        };
    }

    // ── Stub implementations (to be fleshed out) ─────────────────────────────

    private static String extractHover(Request req, CompilationService compiler) {
        // For a full implementation: parse the AST at the given offset and look up
        // the binding's Javadoc. For now we return a placeholder.
        // TODO: implement via ECJ ASTParser + IBinding.getJavaDoc()
        return "";
    }

    private static List<BridgeLocation> navigate(Request req, CompilationService compiler) {
        // TODO: implement via ECJ ASTParser + SearchEngine
        return Collections.emptyList();
    }

    private static List<BridgeLocation> findReferences(Request req, CompilationService compiler) {
        // TODO: implement via ECJ SearchEngine (in-memory scope)
        return Collections.emptyList();
    }

    private static List<BridgeAction> codeActions(Request req, CompilationService compiler) {
        // Quick-fix: re-compile and suggest fixes for problems in the range
        // TODO: implement ECJ quick-fix infrastructure
        return Collections.emptyList();
    }

    private static List<BridgeSignature> signatureHelp(Request req, CompilationService compiler) {
        // TODO: implement via ECJ CompletionEngine METHOD_REF proposals
        return Collections.emptyList();
    }

    private static List<BridgeFileEdit> rename(Request req, CompilationService compiler) {
        // TODO: text-based rename across all open files using binding info
        return Collections.emptyList();
    }

    private static List<BridgeTextEdit> organizeImports(Request req, CompilationService compiler) {
        // TODO: implement via ECJ ImportRewrite
        return Collections.emptyList();
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private static List<String> orEmpty(List<String> list) {
        return list != null ? list : Collections.emptyList();
    }

    private static String orDefault(String level) {
        return (level != null && !level.isEmpty()) ? level : "21";
    }

    private static long extractId(String json) {
        try {
            // Minimal extraction without full parse
            int idx = json.indexOf("\"id\"");
            if (idx < 0) return 0;
            int colon = json.indexOf(':', idx);
            int start = colon + 1;
            while (start < json.length() && json.charAt(start) == ' ') start++;
            int end = start;
            while (end < json.length() && (Character.isDigit(json.charAt(end)) || json.charAt(end) == '-')) end++;
            return Long.parseLong(json.substring(start, end));
        } catch (Exception e) {
            return 0;
        }
    }
}
