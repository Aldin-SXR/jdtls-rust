package com.jdtls.ecjbridge;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

import java.io.*;
import java.util.*;
import java.util.logging.*;
import java.util.regex.*;

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
        root.setLevel(Level.ALL);
        // Re-direct root handler to stderr explicitly
        java.util.logging.StreamHandler stderrHandler =
            new java.util.logging.StreamHandler(System.err, new java.util.logging.SimpleFormatter());
        stderrHandler.setLevel(Level.ALL);
        root.addHandler(stderrHandler);

        // Line-buffered I/O
        BufferedReader stdin = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        PrintWriter stdout = new PrintWriter(new OutputStreamWriter(System.out, "UTF-8"), true);

        CompilationService compiler = new CompilationService();
        CompletionService completer = new CompletionService();
        FormatterService formatter = new FormatterService();
        AstNavigationService navigation = new AstNavigationService();

        LOG.info("ecj-bridge ready");

        String line;
        while ((line = stdin.readLine()) != null) {
            if (line.trim().isEmpty()) continue;
            try {
                Request req = GSON.fromJson(line, Request.class);
                Object response = dispatch(req, compiler, completer, formatter, navigation);
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
                                   FormatterService formatter,
                                   AstNavigationService navigation) {
        return switch (req.method) {
            case "compile" -> {
                List<BridgeDiagnostic> diags = compiler.compile(
                    req.files, orEmpty(req.classpath), orDefault(req.sourceLevel));
                yield new DiagnosticsResponse(req.id, diags);
            }
            case "complete" -> {
                List<BridgeCompletion> items = completer.complete(
                    req.files, orEmpty(req.classpath), orDefault(req.sourceLevel),
                    req.uri, req.offset, req.importPrefix);
                yield new CompletionsResponse(req.id, items);
            }
            case "hover" -> {
                String hover = navigation.hover(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new HoverResponse(req.id, hover);
            }
            case "navigate" -> {
                List<BridgeLocation> locs = navigation.navigate(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset, req.kind);
                yield new LocationsResponse(req.id, locs);
            }
            case "findReferences" -> {
                List<BridgeLocation> refs = navigation.findReferences(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new LocationsResponse(req.id, refs);
            }
            case "codeAction" -> {
                List<BridgeAction> actions = codeActions(req, compiler);
                yield new CodeActionsResponse(req.id, actions);
            }
            case "signatureHelp" -> {
                AstNavigationService.SignatureResult result = navigation.signatureHelp(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new SignatureHelpResponse(
                    req.id, result.signatures, result.activeSignature, result.activeParameter);
            }
            case "rename" -> {
                List<BridgeFileEdit> edits = rename(req, compiler);
                yield new WorkspaceEditResponse(req.id, edits);
            }
            case "inlayHints" -> {
                List<BridgeInlayHint> hints = navigation.inlayHints(
                    req.files, orDefault(req.sourceLevel), req.uri);
                yield new InlayHintsResponse(req.id, hints);
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

    // ── Implementations ───────────────────────────────────────────────────────

    // ── Code actions ──────────────────────────────────────────────────────────────

    private static List<BridgeAction> codeActions(Request req, CompilationService compiler) {
        if (req.files == null || req.uri == null) return List.of();
        String source = req.files.get(req.uri);
        if (source == null) return List.of();

        List<BridgeDiagnostic> diags = compiler.compile(
            req.files, orEmpty(req.classpath), orDefault(req.sourceLevel));

        // Parse AST once for structural fixes
        org.eclipse.jdt.core.dom.CompilationUnit cu = parseForFixes(source, orDefault(req.sourceLevel));

        List<BridgeAction> actions = new ArrayList<>();

        for (BridgeDiagnostic d : diags) {
            if (!req.uri.equals(d.uri)) continue;
            if (req.range != null) {
                boolean overlaps = d.startLine <= req.range.endLine && d.endLine >= req.range.startLine;
                if (!overlaps) continue;
            }

            String msg = d.message != null ? d.message : "";

            // ── Detect unused local variable (handled separately with rich actions) ──
            boolean isUnusedLocal = d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("local variable") || msg.contains("The value of the local"));

            // ── @SuppressWarnings — driven entirely by ECJ's category, no hardcoded IDs ──
            // Unused local variable gets its own suppress actions below (with explicit names).
            if (d.severity == 2 && !isUnusedLocal) {
                String token = suppressToken(d, msg);
                if (token != null) {
                    actions.add(makeSuppressAction(token, req.uri, source, d, cu));
                }
            }

            // ── Add import for unresolved type / variable / method ────────────────
            Matcher mType = Pattern.compile("([A-Z][\\w$]*)\\s+cannot be resolved").matcher(msg);
            if (mType.find()) {
                String simpleName = mType.group(1);
                CompletionService.ensureJrtIndex();
                List<String> candidates = CompletionService.searchBySimpleName(simpleName);
                boolean first = true;
                for (String fqn : candidates) {
                    BridgeAction a = makeImportAction("Import '" + fqn + "'", req.uri, source, fqn, first, cu);
                    actions.add(a);
                    first = false;
                }
            }

            // ── Remove unused import ──────────────────────────────────────────
            if (d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("import") && (msg.contains("never used") || msg.contains("not used")))) {
                actions.add(makeRemoveLineAction("Remove unused import", req.uri, source, d.startLine));
            }

            // ── Unused local variable — rich set of actions matching jdtls ───────
            if (isUnusedLocal) {
                Matcher mVar = Pattern.compile("(?:value of the local variable|local variable) (\\w+)").matcher(msg);
                String varName = mVar.find() ? mVar.group(1) : null;
                int diagOffset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);

                // 1. Remove variable and all its assignments (preferred, listed first)
                BridgeAction removeAllAction = makeRemoveAllAssignmentsAction(req.uri, source, cu, d, varName);
                if (removeAllAction != null) actions.add(removeAllAction);
                else {
                    BridgeAction removeAction = makeRemoveDeclarationAction(req.uri, source, cu, d);
                    if (removeAction != null) actions.add(removeAction);
                }

                // 2. @SuppressWarnings at variable-declaration level
                BridgeAction varSuppressAction = makeLocalVarSuppressAction("unused", req.uri, source, cu, d, varName);
                if (varSuppressAction != null) actions.add(varSuppressAction);

                // 3. @SuppressWarnings at enclosing method/class level
                String methodName = getEnclosingMethodName(cu, diagOffset);
                BridgeAction methodSuppressAction = makeSuppressAction("unused", req.uri, source, d, cu);
                if (methodName != null) methodSuppressAction.title = "Add @SuppressWarnings(\"unused\") to '" + methodName + "'";
                actions.add(methodSuppressAction);

                // 4. Add 'final' modifier
                BridgeAction finalAction = makeAddFinalAction(req.uri, source, cu, d, varName);
                if (finalAction != null) actions.add(finalAction);
            }

            // ── Add missing @Override ─────────────────────────────────────────
            // Only for "must override or implement a superclass method" (not "must implement abstract method")
            if ((msg.contains("must override") || (msg.contains("override") && msg.contains("annotation")))
                    && !msg.contains("must implement the inherited")) {
                BridgeAction a = makeAddOverrideAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Unhandled exception ───────────────────────────────────────────
            Matcher mEx = Pattern.compile("Unhandled exception type ([\\w.$<>\\[\\]]+)").matcher(msg);
            if (mEx.find()) {
                String exType = mEx.group(1).trim();
                BridgeAction throwsAction = makeAddThrowsAction(req.uri, source, cu, d, exType);
                if (throwsAction != null) actions.add(throwsAction);
                BridgeAction tryCatchAction = makeTryCatchAction(req.uri, source, cu, d, exType);
                if (tryCatchAction != null) actions.add(tryCatchAction);
            }

            // ── Add cast for type mismatch ────────────────────────────────────
            Matcher mCast = Pattern.compile(
                "Type mismatch: cannot convert from ([\\w.<>\\[\\],\\s]+) to ([\\w.<>\\[\\],\\s]+)").matcher(msg);
            if (mCast.find()) {
                String targetType = mCast.group(2).trim();
                // Locate the expression and wrap it
                BridgeAction a = makeCastAction(req.uri, source, cu, d, targetType);
                if (a != null) actions.add(a);
            }
        }

        // ── Source actions (always available, not tied to a diagnostic) ───────────
        // Organize Imports — compute the actual edit now so clicking applies immediately
        List<BridgeTextEdit> orgEdits = organizeImports(req, compiler);
        if (!orgEdits.isEmpty()) {
            BridgeFileEdit orgFe = new BridgeFileEdit();
            orgFe.uri = req.uri;
            orgFe.edits = orgEdits;
            BridgeAction organizeImports = new BridgeAction();
            organizeImports.title = "Organize Imports";
            organizeImports.kind = "source.organizeImports";
            organizeImports.edits = List.of(orgFe);
            actions.add(organizeImports);
        }

        return actions;
    }

    /**
     * Map ECJ problem category to a @SuppressWarnings token.
     * Uses CategorizedProblem.getCategoryID() — no hardcoded problem IDs.
     */
    private static String suppressToken(BridgeDiagnostic d, String msg) {
        return switch (d.categoryId) {
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE -> "unused";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_DEPRECATION     -> "deprecation";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_NLS             -> "nls";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_RESTRICTION     -> "restriction";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNCHECKED_RAW   -> {
                // rawtypes for raw type references, unchecked for unsafe operations
                int id = 0;
                try { id = Integer.parseInt(d.code); } catch (Exception ignored) {}
                yield id == org.eclipse.jdt.core.compiler.IProblem.RawTypeReference ? "rawtypes" : "unchecked";
            }
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_POTENTIAL_PROGRAMMING_PROBLEM -> {
                // Null pointer and similar — map to known tokens if possible
                yield msg.contains("null") ? "null" : null;
            }
            default -> null;
        };
    }

    private static org.eclipse.jdt.core.dom.CompilationUnit parseForFixes(String source, String sourceLevel) {
        org.eclipse.jdt.core.dom.ASTParser parser =
            org.eclipse.jdt.core.dom.ASTParser.newParser(org.eclipse.jdt.core.dom.AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(org.eclipse.jdt.core.dom.ASTParser.K_COMPILATION_UNIT);
        parser.setStatementsRecovery(true);
        java.util.Map<String, String> opts = new java.util.HashMap<>();
        String ver = resolveSourceLevel(sourceLevel);
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_SOURCE, ver);
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_COMPLIANCE, ver);
        parser.setCompilerOptions(opts);
        return (org.eclipse.jdt.core.dom.CompilationUnit) parser.createAST(null);
    }

    private static String resolveSourceLevel(String level) {
        return switch (level.trim()) {
            case "8", "1.8" -> "1.8";
            case "11" -> "11";
            case "17" -> "17";
            case "21" -> "21";
            default -> "21";
        };
    }

    private static BridgeAction makeSuppressAction(String tag, String uri, String source,
                                                    BridgeDiagnostic d,
                                                    org.eclipse.jdt.core.dom.CompilationUnit cu) {
        // Walk up AST to find the nearest annotatable declaration (method, field, type, local)
        // and insert @SuppressWarnings before it.
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        int insertLine = findAnnotatableLineAbove(source, cu, offset, d.startLine);

        // Detect indentation of the target line
        String indent = indentOf(source, insertLine);
        int[] lc = CompilationService.offsetToLineCol(source, lineStart(source, insertLine));

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0]; edit.startChar = 0;
        edit.endLine   = lc[0]; edit.endChar   = 0;
        edit.newText   = indent + "@SuppressWarnings(\"" + tag + "\")\n";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "@SuppressWarnings(\"" + tag + "\")";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    /** Find the line of the nearest annotatable node (method/field/type decl) enclosing offset. */
    private static int findAnnotatableLineAbove(String source,
                                                 org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                 int offset, int fallbackLine) {
        // Walk up from the node at offset to find the nearest BodyDeclaration
        AnnotatableLocator loc = new AnnotatableLocator(offset);
        cu.accept(loc);
        if (loc.bestNode != null) {
            int start = loc.bestNode.getStartPosition();
            return CompilationService.offsetToLineCol(source, start)[0];
        }
        return fallbackLine;
    }

    private static BridgeAction makeImportAction(String title, String uri, String source,
                                                  String fqn, boolean preferred,
                                                  org.eclipse.jdt.core.dom.CompilationUnit cu) {
        int insertOffset = findSortedImportInsertPoint(source, cu, fqn);
        int[] lc = CompilationService.offsetToLineCol(source, insertOffset);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0]; edit.startChar = lc[1];
        edit.endLine   = lc[0]; edit.endChar   = lc[1];
        edit.newText   = "import " + fqn + ";\n";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title;
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = preferred;
        return action;
    }

    /** Find the correct alphabetically-sorted insertion offset for a new import. */
    private static int findSortedImportInsertPoint(String source,
                                                    org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                    String fqn) {
        @SuppressWarnings("unchecked")
        java.util.List<org.eclipse.jdt.core.dom.ImportDeclaration> imports = cu.imports();
        if (imports.isEmpty()) {
            return findImportInsertPoint(source);
        }
        // Find first existing import that sorts after fqn → insert before it
        for (var imp : imports) {
            if (imp.isStatic()) continue; // put non-static before static imports (skip static)
            String existing = imp.getName().getFullyQualifiedName();
            if (fqn.compareTo(existing) < 0) {
                // Insert before this import (start of its line)
                int lineNum = cu.getLineNumber(imp.getStartPosition()) - 1;
                return lineStart(source, lineNum);
            }
        }
        // Append after last import
        var last = imports.get(imports.size() - 1);
        int end = last.getStartPosition() + last.getLength();
        if (end < source.length() && source.charAt(end) == '\n') end++;
        return end;
    }

    private static BridgeAction makeRemoveLineAction(String title, String uri, String source, int line) {
        int start = lineStart(source, line);
        int end = lineStart(source, line + 1); // start of next line (includes the '\n')

        int[] startLC = CompilationService.offsetToLineCol(source, start);
        int[] endLC   = CompilationService.offsetToLineCol(source, end);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLC[0]; edit.startChar = 0;
        edit.endLine   = endLC[0];   edit.endChar   = 0;
        edit.newText   = "";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title; action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeRemoveDeclarationAction(String uri, String source,
                                                              org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                              BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        // Find VariableDeclarationStatement enclosing this offset
        VarDeclLocator loc = new VarDeclLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        org.eclipse.jdt.core.dom.VariableDeclarationStatement stmt = loc.found;
        int start = stmt.getStartPosition();
        int end = start + stmt.getLength();
        // Include trailing newline
        if (end < source.length() && source.charAt(end) == '\n') end++;

        int[] startLC = CompilationService.offsetToLineCol(source, start);
        int[] endLC   = CompilationService.offsetToLineCol(source, end);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLC[0]; edit.startChar = 0;  // delete from line start (include indent)
        edit.endLine   = endLC[0];   edit.endChar   = endLC[1];
        edit.newText   = "";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Remove unused variable";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeAddOverrideAction(String uri, String source,
                                                        org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                        BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        int methodStart = loc.found.getStartPosition();
        int[] lc = CompilationService.offsetToLineCol(source, methodStart);
        String indent = indentOf(source, lc[0]);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0]; edit.startChar = 0;
        edit.endLine   = lc[0]; edit.endChar   = 0;
        edit.newText   = indent + "@Override\n";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Add @Override annotation";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeCastAction(String uri, String source,
                                                 org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                 BridgeDiagnostic d, String targetType) {
        // Find the expression at the diagnostic range and wrap with (targetType)
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ExpressionLocator loc = new ExpressionLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        int start = loc.found.getStartPosition();
        int end = start + loc.found.getLength();
        int[] startLC = CompilationService.offsetToLineCol(source, start);
        int[] endLC   = CompilationService.offsetToLineCol(source, end);

        String exprText = source.substring(start, end);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLC[0]; edit.startChar = startLC[1];
        edit.endLine   = endLC[0];   edit.endChar   = endLC[1];
        edit.newText   = "(" + targetType + ") " + exprText;

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Cast to '" + targetType + "'";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeAddThrowsAction(String uri, String source,
                                                      org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                      BridgeDiagnostic d, String exType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        if (loc.found == null || loc.found.getBody() == null) return null;

        org.eclipse.jdt.core.dom.MethodDeclaration method = loc.found;
        String simpleName = exType.contains(".") ? exType.substring(exType.lastIndexOf('.') + 1) : exType;

        BridgeTextEdit edit = new BridgeTextEdit();
        @SuppressWarnings("unchecked")
        java.util.List<org.eclipse.jdt.core.dom.Type> thrown = method.thrownExceptionTypes();
        if (!thrown.isEmpty()) {
            // Append to existing throws clause
            org.eclipse.jdt.core.dom.Type lastType = thrown.get(thrown.size() - 1);
            int end = lastType.getStartPosition() + lastType.getLength();
            int[] lc = CompilationService.offsetToLineCol(source, end);
            edit.startLine = lc[0]; edit.startChar = lc[1];
            edit.endLine   = lc[0]; edit.endChar   = lc[1];
            edit.newText   = ", " + simpleName;
        } else {
            // Insert "throws ExType " before method body opening brace
            int bodyStart = method.getBody().getStartPosition();
            int[] lc = CompilationService.offsetToLineCol(source, bodyStart);
            edit.startLine = lc[0]; edit.startChar = lc[1];
            edit.endLine   = lc[0]; edit.endChar   = lc[1];
            edit.newText   = "throws " + simpleName + " ";
        }

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Add throws declaration for '" + simpleName + "'";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeTryCatchAction(String uri, String source,
                                                     org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                     BridgeDiagnostic d, String exType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        org.eclipse.jdt.core.dom.Statement stmt = loc.found;
        int stmtStart = stmt.getStartPosition();
        int stmtEnd   = stmtStart + stmt.getLength();
        if (stmtEnd < source.length() && source.charAt(stmtEnd) == '\n') stmtEnd++;

        int[] startLC = CompilationService.offsetToLineCol(source, stmtStart);
        int[] endLC   = CompilationService.offsetToLineCol(source, stmtEnd);

        String indent      = indentOf(source, startLC[0]);
        String innerIndent = indent + "    ";
        String stmtText    = source.substring(stmtStart, stmtStart + stmt.getLength());
        // Re-indent inner statement lines
        String indentedStmt = stmtText.replace("\n", "\n    ");
        String simpleName  = exType.contains(".") ? exType.substring(exType.lastIndexOf('.') + 1) : exType;

        String newText = indent + "try {\n" +
                         innerIndent + indentedStmt + "\n" +
                         indent + "} catch (" + simpleName + " e) {\n" +
                         innerIndent + "e.printStackTrace();\n" +
                         indent + "}\n";

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLC[0]; edit.startChar = 0;
        edit.endLine   = endLC[0];   edit.endChar   = endLC[1];
        edit.newText   = newText;

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Surround with try/catch";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeRemoveAllAssignmentsAction(String uri, String source,
                                                                org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                BridgeDiagnostic d, String varName) {
        if (varName == null) return null;
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator varLoc = new VarDeclLocator(offset);
        cu.accept(varLoc);
        if (varLoc.found == null) return null;

        AllAssignmentsLocator assignLoc = new AllAssignmentsLocator(varName);
        cu.accept(assignLoc);

        List<BridgeTextEdit> edits = new ArrayList<>();

        // Remove all pure assignment statements for this variable (excluding the declaration line)
        int declStart = varLoc.found.getStartPosition();
        int declLineNum = CompilationService.offsetToLineCol(source, declStart)[0];
        for (org.eclipse.jdt.core.dom.ExpressionStatement es : assignLoc.pureAssignments) {
            int esLineNum = CompilationService.offsetToLineCol(source, es.getStartPosition())[0];
            if (esLineNum == declLineNum) continue; // will be handled by declaration removal
            int start = es.getStartPosition();
            int end = start + es.getLength();
            if (end < source.length() && source.charAt(end) == '\n') end++;
            int[] startLC = CompilationService.offsetToLineCol(source, start);
            int[] endLC   = CompilationService.offsetToLineCol(source, end);
            BridgeTextEdit edit = new BridgeTextEdit();
            edit.startLine = startLC[0]; edit.startChar = 0;
            edit.endLine   = endLC[0];   edit.endChar   = endLC[1];
            edit.newText   = "";
            edits.add(edit);
        }

        // Remove the declaration itself
        int declEnd = declStart + varLoc.found.getLength();
        if (declEnd < source.length() && source.charAt(declEnd) == '\n') declEnd++;
        int[] declStartLC = CompilationService.offsetToLineCol(source, declStart);
        int[] declEndLC   = CompilationService.offsetToLineCol(source, declEnd);
        BridgeTextEdit declEdit = new BridgeTextEdit();
        declEdit.startLine = declStartLC[0]; declEdit.startChar = 0;
        declEdit.endLine   = declEndLC[0];   declEdit.endChar   = declEndLC[1];
        declEdit.newText   = "";
        edits.add(declEdit);

        // Sort bottom-to-top so applying edits doesn't shift remaining positions
        edits.sort((a2, b2) -> Integer.compare(b2.startLine, a2.startLine));

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = edits;

        BridgeAction action = new BridgeAction();
        action.title = "Remove '" + varName + "' and all assignments";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeLocalVarSuppressAction(String tag, String uri, String source,
                                                            org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                            BridgeDiagnostic d, String varName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        int[] lc = CompilationService.offsetToLineCol(source, loc.found.getStartPosition());
        String indent = indentOf(source, lc[0]);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0]; edit.startChar = 0;
        edit.endLine   = lc[0]; edit.endChar   = 0;
        edit.newText   = indent + "@SuppressWarnings(\"" + tag + "\")\n";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = varName != null
            ? "Add @SuppressWarnings(\"" + tag + "\") to '" + varName + "'"
            : "@SuppressWarnings(\"" + tag + "\")";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeAddFinalAction(String uri, String source,
                                                    org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                    BridgeDiagnostic d, String varName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;

        // Don't offer if already final
        @SuppressWarnings("unchecked")
        java.util.List<org.eclipse.jdt.core.dom.IExtendedModifier> mods = loc.found.modifiers();
        boolean alreadyFinal = mods.stream().anyMatch(
            m -> m instanceof org.eclipse.jdt.core.dom.Modifier mod && mod.isFinal());
        if (alreadyFinal) return null;

        int typeStart = loc.found.getType().getStartPosition();
        int[] lc = CompilationService.offsetToLineCol(source, typeStart);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0]; edit.startChar = lc[1];
        edit.endLine   = lc[0]; edit.endChar   = lc[1];
        edit.newText   = "final ";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri; fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = varName != null ? "Add 'final' modifier to '" + varName + "'" : "Add 'final' modifier";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static String getEnclosingMethodName(org.eclipse.jdt.core.dom.CompilationUnit cu, int offset) {
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        return loc.found != null ? loc.found.getName().getIdentifier() : null;
    }

    // ── String helpers ────────────────────────────────────────────────────────────

    /** Find the character offset at which a new import statement should be inserted. */
    private static int findImportInsertPoint(String source) {
        Matcher m = Pattern.compile("^\\s*(import|package)\\s+[\\w.*]+\\s*;",
                                    Pattern.MULTILINE).matcher(source);
        int end = 0;
        while (m.find()) end = m.end();
        if (end > 0) {
            while (end < source.length() && source.charAt(end) == '\n') end++;
            return end;
        }
        return 0;
    }

    /** Offset of the first character on 0-based line number. */
    private static int lineStart(String source, int line) {
        int cur = 0;
        for (int i = 0; i < line && cur < source.length(); i++) {
            int nl = source.indexOf('\n', cur);
            if (nl < 0) return source.length();
            cur = nl + 1;
        }
        return cur;
    }

    /** Leading whitespace of 0-based line. */
    private static String indentOf(String source, int line) {
        int start = lineStart(source, line);
        int i = start;
        while (i < source.length() && (source.charAt(i) == ' ' || source.charAt(i) == '\t')) i++;
        return source.substring(start, i);
    }

    private static List<BridgeFileEdit> rename(Request req, CompilationService compiler) {
        if (req.files == null || req.uri == null || req.newName == null) return Collections.emptyList();

        AstNavigationService nav = new AstNavigationService();
        List<BridgeFileEdit> result = new ArrayList<>();

        // Find references in every open file
        for (String fileUri : req.files.keySet()) {
            // For same-file, use the actual offset; for other files search all occurrences by name
            int offset = fileUri.equals(req.uri) ? req.offset : -1;
            List<BridgeLocation> refs;
            if (offset >= 0) {
                refs = nav.findReferences(req.files, orDefault(req.sourceLevel), req.uri, offset);
            } else {
                // Cross-file: search for any reference whose URI matches
                refs = nav.findReferences(req.files, orDefault(req.sourceLevel), req.uri, req.offset)
                    .stream().filter(l -> fileUri.equals(l.uri)).toList();
            }

            if (refs.isEmpty()) continue;

            List<BridgeTextEdit> edits = new ArrayList<>();
            for (BridgeLocation loc : refs) {
                if (!fileUri.equals(loc.uri)) continue;
                BridgeTextEdit edit = new BridgeTextEdit();
                edit.startLine = loc.startLine;
                edit.startChar = loc.startChar;
                edit.endLine = loc.endLine;
                edit.endChar = loc.endChar;
                edit.newText = req.newName;
                edits.add(edit);
            }

            if (!edits.isEmpty()) {
                BridgeFileEdit fe = new BridgeFileEdit();
                fe.uri = fileUri;
                fe.edits = edits;
                result.add(fe);
            }
        }
        return result;
    }

    private static List<BridgeTextEdit> organizeImports(Request req, CompilationService compiler) {
        if (req.files == null || req.uri == null) return Collections.emptyList();
        String source = req.files.get(req.uri);
        if (source == null) return Collections.emptyList();

        CompletionService.ensureJrtIndex();

        // Parse the file to find used simple type names
        org.eclipse.jdt.core.dom.ASTParser parser =
            org.eclipse.jdt.core.dom.ASTParser.newParser(org.eclipse.jdt.core.dom.AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(org.eclipse.jdt.core.dom.ASTParser.K_COMPILATION_UNIT);
        parser.setStatementsRecovery(true);
        org.eclipse.jdt.core.dom.CompilationUnit cu =
            (org.eclipse.jdt.core.dom.CompilationUnit) parser.createAST(null);

        // Collect all simple names that look like types (Capitalized)
        Set<String> usedTypes = new LinkedHashSet<>();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.SimpleType node) {
                org.eclipse.jdt.core.dom.Name name = node.getName();
                String id = name.isSimpleName()
                    ? ((org.eclipse.jdt.core.dom.SimpleName) name).getIdentifier()
                    : name.getFullyQualifiedName();
                if (Character.isUpperCase(id.charAt(0))) usedTypes.add(id);
                return true;
            }
        });

        // Collect already-explicit imports keyed by simple name so we can prefer them
        // for ambiguous types (e.g. List → java.util.List already imported).
        Map<String, String> existingBySimpleName = new LinkedHashMap<>();
        for (Object imp : cu.imports()) {
            if (imp instanceof org.eclipse.jdt.core.dom.ImportDeclaration id && !id.isStatic() && !id.isOnDemand()) {
                String fqn = id.getName().getFullyQualifiedName();
                int dot = fqn.lastIndexOf('.');
                String simpleName = dot >= 0 ? fqn.substring(dot + 1) : fqn;
                existingBySimpleName.put(simpleName, fqn);
            }
        }

        // For each used type: prefer an already-imported FQN; otherwise require a unique JRT match.
        Set<String> importSet = new TreeSet<>();
        for (String typeName : usedTypes) {
            String existing = existingBySimpleName.get(typeName);
            if (existing != null) {
                // Already imported → keep it (even if ambiguous in JRT)
                importSet.add(existing);
                continue;
            }
            List<String> matches = CompletionService.searchBySimpleName(typeName);
            if (matches.size() == 1) {
                importSet.add(matches.get(0));
            }
            // Multiple matches and not already imported → ambiguous, skip
        }

        if (importSet.isEmpty()) return Collections.emptyList();

        // Find the span of existing imports
        int importStart = -1, importEnd = -1;
        @SuppressWarnings("unchecked")
        java.util.List<org.eclipse.jdt.core.dom.ImportDeclaration> imports = cu.imports();
        if (!imports.isEmpty()) {
            importStart = imports.get(0).getStartPosition();
            org.eclipse.jdt.core.dom.ImportDeclaration last = imports.get(imports.size() - 1);
            importEnd = last.getStartPosition() + last.getLength();
        }

        // Build new import block
        StringBuilder sb = new StringBuilder();
        for (String fqn : importSet) {
            sb.append("import ").append(fqn).append(";\n");
        }

        BridgeTextEdit edit = new BridgeTextEdit();
        if (importStart >= 0) {
            int[] startLC = CompilationService.offsetToLineCol(source, importStart);
            int[] endLC = CompilationService.offsetToLineCol(source, importEnd);
            edit.startLine = startLC[0];
            edit.startChar = startLC[1];
            edit.endLine = endLC[0];
            edit.endChar = endLC[1];
        } else {
            int insertPt = findImportInsertPoint(source);
            int[] lc = CompilationService.offsetToLineCol(source, insertPt);
            edit.startLine = lc[0];
            edit.startChar = lc[1];
            edit.endLine = lc[0];
            edit.endChar = lc[1];
        }
        edit.newText = sb.toString();
        return List.of(edit);
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

    // ── AST visitor helpers ───────────────────────────────────────────────────

    static class AnnotatableLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        org.eclipse.jdt.core.dom.BodyDeclaration bestNode;

        AnnotatableLocator(int offset) { this.offset = offset; }

        @Override
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode node) {
            if (node instanceof org.eclipse.jdt.core.dom.BodyDeclaration bd) {
                int start = bd.getStartPosition();
                int end = start + bd.getLength();
                if (start <= offset && offset <= end) {
                    // More specific (deeper) node wins
                    if (bestNode == null ||
                        bd.getStartPosition() >= bestNode.getStartPosition()) {
                        bestNode = bd;
                    }
                }
            }
        }
    }

    static class VarDeclLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        org.eclipse.jdt.core.dom.VariableDeclarationStatement found;

        VarDeclLocator(int offset) { this.offset = offset; }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.VariableDeclarationStatement node) {
            int start = node.getStartPosition();
            int end = start + node.getLength();
            if (start <= offset && offset <= end) found = node;
            return true;
        }
    }

    static class MethodLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        org.eclipse.jdt.core.dom.MethodDeclaration found;

        MethodLocator(int offset) { this.offset = offset; }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.MethodDeclaration node) {
            int start = node.getStartPosition();
            int end = start + node.getLength();
            if (start <= offset && offset <= end) found = node;
            return true;
        }
    }

    static class AllAssignmentsLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final String varName;
        final List<org.eclipse.jdt.core.dom.ExpressionStatement> pureAssignments = new ArrayList<>();

        AllAssignmentsLocator(String varName) { this.varName = varName; }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.ExpressionStatement node) {
            if (node.getExpression() instanceof org.eclipse.jdt.core.dom.Assignment a) {
                org.eclipse.jdt.core.dom.Expression lhs = a.getLeftHandSide();
                if (lhs instanceof org.eclipse.jdt.core.dom.SimpleName sn
                        && varName.equals(sn.getIdentifier())) {
                    pureAssignments.add(node);
                }
            }
            return true;
        }
    }

    static class StatementLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        org.eclipse.jdt.core.dom.Statement found;

        StatementLocator(int offset) { this.offset = offset; }

        @Override
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode node) {
            if (node instanceof org.eclipse.jdt.core.dom.Statement stmt
                    && !(node instanceof org.eclipse.jdt.core.dom.Block)) {
                int start = stmt.getStartPosition();
                int end = start + stmt.getLength();
                if (start <= offset && offset <= end) {
                    // Prefer deepest (most specific) non-block statement
                    if (found == null || stmt.getStartPosition() >= found.getStartPosition()) {
                        found = stmt;
                    }
                }
            }
        }
    }

    static class ExpressionLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        org.eclipse.jdt.core.dom.Expression found;

        ExpressionLocator(int offset) { this.offset = offset; }

        @Override
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode node) {
            if (node instanceof org.eclipse.jdt.core.dom.Expression expr) {
                int start = expr.getStartPosition();
                int end = start + expr.getLength();
                if (start <= offset && offset <= end) {
                    if (found == null || expr.getLength() < found.getLength()) {
                        found = expr;
                    }
                }
            }
        }
    }
}
