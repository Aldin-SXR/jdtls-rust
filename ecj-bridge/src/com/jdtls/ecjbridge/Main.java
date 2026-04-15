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
                    req.files, orDefault(req.sourceLevel), orEmpty(req.classpath), req.uri, req.offset);
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
            case "codeLens" -> {
                List<BridgeCodeLens> lenses = navigation.codeLens(
                    req.files, orDefault(req.sourceLevel), req.uri);
                yield new CodeLensResponse(req.id, lenses);
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
        boolean hasOverlappingDiagnostic = false;

        for (BridgeDiagnostic d : diags) {
            if (!req.uri.equals(d.uri)) continue;
            if (req.range != null) {
                boolean overlaps = d.startLine <= req.range.endLine && d.endLine >= req.range.startLine;
                if (!overlaps) continue;
            }
            hasOverlappingDiagnostic = true;

            String msg = d.message != null ? d.message : "";
            int problemId = problemId(d);

            // ── Detect unused local variable (handled separately with rich actions) ──
            boolean isUnusedLocal = d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("local variable") || msg.contains("The value of the local"));
            boolean isUnusedParameter = problemId == org.eclipse.jdt.core.compiler.IProblem.ArgumentIsNeverUsed
                    || (d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("parameter") && (msg.contains("not used") || msg.contains("never read"))));

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

                BridgeAction renameTypeAction = makeRenameToExistingTypeAction(req.uri, source, cu, d, simpleName);
                if (renameTypeAction != null) actions.add(renameTypeAction);

                BridgeAction typeParameterAction = makeAddTypeParameterAction(req.uri, source, cu, d, simpleName);
                if (typeParameterAction != null) actions.add(typeParameterAction);

                BridgeAction newClassAction = makeCreateTopLevelTypeAction("Create class '" + simpleName + "'", req.uri, source, simpleName, "class");
                if (newClassAction != null) actions.add(newClassAction);
                BridgeAction newInterfaceAction = makeCreateTopLevelTypeAction("Create interface '" + simpleName + "'", req.uri, source, simpleName, "interface");
                if (newInterfaceAction != null) actions.add(newInterfaceAction);
                BridgeAction newEnumAction = makeCreateTopLevelTypeAction("Create enum '" + simpleName + "'", req.uri, source, simpleName, "enum");
                if (newEnumAction != null) actions.add(newEnumAction);
                BridgeAction newRecordAction = makeCreateTopLevelTypeAction("Create record '" + simpleName + "'", req.uri, source, simpleName, "record");
                if (newRecordAction != null) actions.add(newRecordAction);
            }

            Matcher mMissingVar = Pattern.compile("([a-zA-Z_$][\\w$]*)\\s+cannot be resolved to a variable").matcher(msg);
            if (mMissingVar.find()) {
                String variableName = mMissingVar.group(1);
                BridgeAction localAction = makeCreateLocalVariableAction(req.uri, source, cu, d, variableName);
                if (localAction != null) actions.add(localAction);
                BridgeAction fieldAction = makeCreateFieldAction(req.uri, source, cu, d, variableName, false);
                if (fieldAction != null) actions.add(fieldAction);
                BridgeAction constantAction = makeCreateFieldAction(req.uri, source, cu, d, variableName, true);
                if (constantAction != null) actions.add(constantAction);
                BridgeAction parameterAction = makeCreateParameterAction(req.uri, source, cu, d, variableName);
                if (parameterAction != null) actions.add(parameterAction);
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

            // ── Unused parameter — jdtls-style quick fixes ──────────────────────
            if (isUnusedParameter) {
                Matcher mParam = Pattern.compile("(?:parameter) (\\w+)").matcher(msg);
                String paramName = mParam.find() ? mParam.group(1) : null;

                BridgeAction changeSignatureAction = makeChangeSignatureAction(req.uri, source, cu, d, paramName);
                if (changeSignatureAction != null) actions.add(changeSignatureAction);

                BridgeAction assignFieldAction = makeAssignParameterToFieldAction(req.uri, source, cu, d, paramName);
                if (assignFieldAction != null) actions.add(assignFieldAction);

                BridgeAction documentParamAction = makeDocumentParameterAction(req.uri, source, cu, d, paramName);
                if (documentParamAction != null) actions.add(documentParamAction);

                BridgeAction finalAction = makeAddFinalParameterAction(req.uri, source, cu, d, paramName);
                if (finalAction != null) actions.add(finalAction);
            }

            // ── Add missing @Override ─────────────────────────────────────────
            // Only for "must override or implement a superclass method" (not "must implement abstract method")
            if ((msg.contains("must override") || (msg.contains("override") && msg.contains("annotation")))
                    && !msg.contains("must implement the inherited")) {
                BridgeAction a = makeAddOverrideAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Unused private members ───────────────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateMethod
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateConstructor
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateType) {
                BridgeAction a = makeRemoveUnusedMemberAction(req.uri, source, cu, d);
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

            // ── Remove unnecessary cast ──────────────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnnecessaryCast) {
                BridgeAction a = makeRemoveUnnecessaryCastAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Superfluous semicolon ────────────────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.SuperfluousSemicolon) {
                BridgeAction a = makeRemoveTextAction("Remove semicolon", req.uri, source,
                        CompilationService.lineColToOffset(source, d.startLine, d.startChar),
                        CompilationService.lineColToOffset(source, d.endLine, d.endChar));
                if (a != null) actions.add(a);
            }

            // ── Unreachable code ─────────────────────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.CodeCannotBeReached
                    || msg.contains("Dead code")
                    || msg.contains("Unreachable code")) {
                BridgeAction a = makeRemoveUnreachableCodeAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (msg.contains("Implicit super constructor") && msg.contains("is undefined")) {
                BridgeAction a = makeAddExplicitSuperConstructorCallAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Remove unused declared thrown exception ──────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedMethodDeclaredThrownException
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedConstructorDeclaredThrownException) {
                BridgeAction a = makeRemoveUnusedThrownExceptionAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Initialize uninitialized local variable ──────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UninitializedLocalVariable
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UninitializedLocalVariableHintMissingDefault) {
                BridgeAction a = makeInitializeLocalVariableAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            // ── Missing Javadoc tags ─────────────────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingParamTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingReturnTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingThrowsTag) {
                BridgeAction a = makeAddMissingJavadocTagAction(req.uri, source, cu, d, problemId);
                if (a != null) actions.add(a);
                BridgeAction all = makeAddAllMissingJavadocTagsAction(req.uri, source, cu, d);
                if (all != null) actions.add(all);
            }

            // ── Invalid / duplicate Javadoc tags ─────────────────────────────
            if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocInvalidThrowsClassName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocDuplicateThrowsClassName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocDuplicateReturnTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocDuplicateParamName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocInvalidParamName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocInvalidTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocDuplicateTag) {
                BridgeAction a = makeRemoveInvalidJavadocTagAction(req.uri, source, cu, d);
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

        if (!hasOverlappingDiagnostic) {
            BridgeAction javadocAction = makeAddJavadocAction(req.uri, source, cu, req.range);
            if (javadocAction != null) {
                actions.add(javadocAction);
            }
        }

        return dedupeActions(actions);
    }

    private static int problemId(BridgeDiagnostic d) {
        try {
            return d.code != null ? Integer.parseInt(d.code) : 0;
        } catch (Exception ignored) {
            return 0;
        }
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
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_DOC_COMMENT_SUPPORT, org.eclipse.jdt.core.JavaCore.ENABLED);
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

    private static BridgeAction makeAddJavadocAction(String uri, String source,
                                                      org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                      BridgeRange range) {
        if (range == null) return null;

        org.eclipse.jdt.core.dom.BodyDeclaration target = findJavadocTarget(source, cu, range);
        if (target == null || target.getJavadoc() != null) return null;

        int line = cu.getLineNumber(target.getStartPosition()) - 1;
        if (line < 0) return null;

        String indent = indentOf(source, line);
        int insertOffset = lineStart(source, line);
        int[] lc = CompilationService.offsetToLineCol(source, insertOffset);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0];
        edit.startChar = 0;
        edit.endLine = lc[0];
        edit.endChar = 0;
        edit.newText = buildJavadocStub(target, indent);

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Add Javadoc comment";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = false;
        return action;
    }

    private static org.eclipse.jdt.core.dom.BodyDeclaration findJavadocTarget(
            String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu,
            BridgeRange range) {
        JavadocTargetLocator locator = new JavadocTargetLocator(cu, range.startLine, range.endLine);
        cu.accept(locator);
        if (locator.best != null) {
            return locator.best;
        }

        int startOffset = CompilationService.lineColToOffset(source, range.startLine, range.startChar);
        int endOffset = CompilationService.lineColToOffset(source, range.endLine, range.endChar);
        if (endOffset < startOffset) {
            endOffset = startOffset;
        }

        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(
            cu, startOffset, Math.max(0, endOffset - startOffset));
        if (node == null && startOffset > 0) {
            node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, startOffset - 1, 1);
        }
        return node != null ? enclosingJavadocTarget(node) : null;
    }

    private static org.eclipse.jdt.core.dom.BodyDeclaration enclosingJavadocTarget(org.eclipse.jdt.core.dom.ASTNode node) {
        org.eclipse.jdt.core.dom.ASTNode current = node;
        while (current != null) {
            if (current instanceof org.eclipse.jdt.core.dom.MethodDeclaration method) {
                return method;
            }
            if (current instanceof org.eclipse.jdt.core.dom.FieldDeclaration field) {
                return field;
            }
            if (current instanceof org.eclipse.jdt.core.dom.EnumConstantDeclaration constant) {
                return constant;
            }
            if (current instanceof org.eclipse.jdt.core.dom.AbstractTypeDeclaration type) {
                return type;
            }
            current = current.getParent();
        }
        return null;
    }

    private static String buildJavadocStub(org.eclipse.jdt.core.dom.BodyDeclaration target, String indent) {
        StringBuilder out = new StringBuilder();
        out.append(indent).append("/**\n");
        out.append(indent).append(" * \n");

        if (target instanceof org.eclipse.jdt.core.dom.MethodDeclaration method) {
            @SuppressWarnings("unchecked")
            List<org.eclipse.jdt.core.dom.SingleVariableDeclaration> params = method.parameters();
            for (org.eclipse.jdt.core.dom.SingleVariableDeclaration param : params) {
                out.append(indent).append(" * @param ")
                        .append(param.getName().getIdentifier())
                        .append("\n");
            }
            org.eclipse.jdt.core.dom.Type returnType = method.getReturnType2();
            if (!method.isConstructor() && returnType != null && !"void".equals(returnType.toString())) {
                out.append(indent).append(" * @return\n");
            }
            @SuppressWarnings("unchecked")
            List<org.eclipse.jdt.core.dom.Type> thrown = method.thrownExceptionTypes();
            for (org.eclipse.jdt.core.dom.Type ex : thrown) {
                out.append(indent).append(" * @throws ")
                        .append(ex.toString())
                        .append("\n");
            }
        }

        out.append(indent).append(" */\n");
        return out.toString();
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

    private static BridgeAction makeRemoveUnnecessaryCastAction(String uri, String source,
                                                                 org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                 BridgeDiagnostic d) {
        org.eclipse.jdt.core.dom.CastExpression cast = findCastExpressionAtDiagnostic(source, cu, d);
        if (cast == null) {
            return null;
        }

        org.eclipse.jdt.core.dom.Expression inner = cast.getExpression();
        String replacement = source.substring(inner.getStartPosition(), inner.getStartPosition() + inner.getLength());
        return makeReplaceNodeAction("Remove unnecessary cast", uri, source, cast, replacement);
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

    private static BridgeAction makeChangeSignatureAction(String uri, String source,
                                                           org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                           BridgeDiagnostic d, String paramName) {
        org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter = findMethodParameter(source, cu, d, paramName);
        if (parameter == null || !(parameter.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration method)) {
            return null;
        }

        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.SingleVariableDeclaration> parameters = method.parameters();
        int index = parameters.indexOf(parameter);
        if (index < 0) {
            return null;
        }

        List<BridgeTextEdit> edits = new ArrayList<>();
        BridgeTextEdit paramEdit = deletionEditForNodes(source, new ArrayList<>(parameters), index);
        if (paramEdit == null) {
            return null;
        }
        edits.add(paramEdit);

        String methodName = method.getName().getIdentifier();
        int originalArity = parameters.size();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.MethodInvocation node) {
                if (!methodName.equals(node.getName().getIdentifier())) {
                    return true;
                }
                org.eclipse.jdt.core.dom.Expression expr = node.getExpression();
                if (expr != null && !(expr instanceof org.eclipse.jdt.core.dom.ThisExpression)) {
                    return true;
                }
                if (node.arguments().size() != originalArity) {
                    return true;
                }
                BridgeTextEdit edit = deletionEditForNodes(source, castAstNodes(node.arguments()), index);
                if (edit != null) edits.add(edit);
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.SuperMethodInvocation node) {
                if (!methodName.equals(node.getName().getIdentifier())) {
                    return true;
                }
                if (node.arguments().size() != originalArity) {
                    return true;
                }
                BridgeTextEdit edit = deletionEditForNodes(source, castAstNodes(node.arguments()), index);
                if (edit != null) edits.add(edit);
                return true;
            }
        });

        edits.sort((a, b) -> {
            int lineCmp = Integer.compare(b.startLine, a.startLine);
            return lineCmp != 0 ? lineCmp : Integer.compare(b.startChar, a.startChar);
        });

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = edits;

        BridgeAction action = new BridgeAction();
        action.title = "Change signature for '" + methodName + "'";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeAssignParameterToFieldAction(String uri, String source,
                                                                  org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                  BridgeDiagnostic d, String paramName) {
        org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter = findMethodParameter(source, cu, d, paramName);
        if (parameter == null || !(parameter.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration method)) {
            return null;
        }
        org.eclipse.jdt.core.dom.AbstractTypeDeclaration type = enclosingType(method);
        if (type == null || method.getBody() == null) {
            return null;
        }

        String baseName = parameter.getName().getIdentifier();
        String fieldName = chooseFieldName(type, baseName);
        String typeName = parameter.getType().toString() + (parameter.isVarargs() ? "[]" : "");
        boolean isStaticMethod = java.lang.reflect.Modifier.isStatic(method.getModifiers());
        String typeIndent = indentOf(source, cu.getLineNumber(type.getStartPosition()) - 1);
        String memberIndent = typeIndent + "    ";

        int fieldInsertOffset = bodyInsertOffset(type, source);
        if (fieldInsertOffset < 0) {
            return null;
        }

        BridgeTextEdit fieldEdit = insertionEdit(source, fieldInsertOffset,
                memberIndent + "private " + (isStaticMethod ? "static " : "") + typeName + " " + fieldName + ";\n");

        int assignInsertOffset = assignmentInsertOffset(method, source);
        String owner = isStaticMethod ? type.getName().getIdentifier() + "." : "this.";
        BridgeTextEdit assignEdit = insertionEdit(source, assignInsertOffset,
                memberIndent + owner + fieldName + " = " + parameter.getName().getIdentifier() + ";\n");

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(assignEdit, fieldEdit);

        BridgeAction action = new BridgeAction();
        action.title = "Assign parameter to new field";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeDocumentParameterAction(String uri, String source,
                                                             org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                             BridgeDiagnostic d, String paramName) {
        org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter = findMethodParameter(source, cu, d, paramName);
        if (parameter == null || !(parameter.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration method)) {
            return null;
        }

        if (hasParamTag(method.getJavadoc(), paramName)) {
            return null;
        }

        BridgeTextEdit edit;
        if (method.getJavadoc() == null) {
            int line = cu.getLineNumber(method.getStartPosition()) - 1;
            String indent = indentOf(source, line);
            edit = insertionEdit(source, lineStart(source, line),
                    buildParameterDocumentationStub(method, paramName, indent));
        } else {
            String methodIndent = indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1);
            String lineText = methodIndent + " * @param " + paramName + "\n";
            edit = insertionEdit(source, javadocClosingOffset(method.getJavadoc()),
                    formatInsertedJavadocLines(source, method.getJavadoc(), methodIndent, lineText));
        }

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Document parameter to avoid 'unused' warning";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeAddFinalParameterAction(String uri, String source,
                                                             org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                             BridgeDiagnostic d, String paramName) {
        org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter = findMethodParameter(source, cu, d, paramName);
        if (parameter == null) {
            return null;
        }

        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.IExtendedModifier> mods = parameter.modifiers();
        boolean alreadyFinal = mods.stream().anyMatch(
            m -> m instanceof org.eclipse.jdt.core.dom.Modifier mod && mod.isFinal());
        if (alreadyFinal) {
            return null;
        }

        int typeStart = parameter.getType().getStartPosition();
        BridgeTextEdit edit = insertionEdit(source, typeStart, "final ");

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Add final modifier for '" + parameter.getName().getIdentifier() + "'";
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

    private static BridgeAction makeRemoveUnusedMemberAction(String uri, String source,
                                                              org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                              BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, offset, 0);
        while (node != null) {
            if (node instanceof org.eclipse.jdt.core.dom.FieldDeclaration
                    || node instanceof org.eclipse.jdt.core.dom.MethodDeclaration
                    || node instanceof org.eclipse.jdt.core.dom.AbstractTypeDeclaration) {
                return makeRemoveDeclarationNodeAction("Remove unused member", uri, source, node);
            }
            node = node.getParent();
        }
        return null;
    }

    private static BridgeAction makeRemoveUnusedThrownExceptionAction(String uri, String source,
                                                                       org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                       BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator methodLoc = new MethodLocator(offset);
        cu.accept(methodLoc);
        org.eclipse.jdt.core.dom.MethodDeclaration method = methodLoc.found;
        if (method == null) {
            return null;
        }

        org.eclipse.jdt.core.dom.Type thrownType = findThrownExceptionType(method, offset);
        if (thrownType == null) {
            return null;
        }

        List<BridgeTextEdit> edits = new ArrayList<>();
        BridgeTextEdit throwsEdit = removalEditForThrownType(source, method, thrownType);
        if (throwsEdit == null) {
            return null;
        }
        edits.add(throwsEdit);

        BridgeTextEdit tagEdit = removeThrowsJavadocTagEdit(source, method.getJavadoc(), thrownType.toString());
        if (tagEdit != null) {
            edits.add(tagEdit);
        }

        edits.sort((a, b) -> {
            int lineCmp = Integer.compare(b.startLine, a.startLine);
            return lineCmp != 0 ? lineCmp : Integer.compare(b.startChar, a.startChar);
        });

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = edits;

        BridgeAction action = new BridgeAction();
        action.title = "Remove unused exception '" + thrownType + "'";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeInitializeLocalVariableAction(String uri, String source,
                                                                   org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                   BridgeDiagnostic d) {
        Matcher matcher = Pattern.compile("local variable (\\w+)").matcher(d.message != null ? d.message : "");
        if (!matcher.find()) {
            return null;
        }

        String variableName = matcher.group(1);
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        LocalVariableFragmentLocator locator = new LocalVariableFragmentLocator(offset, variableName);
        cu.accept(locator);
        if (locator.found == null || locator.found.getInitializer() != null) {
            return null;
        }

        String defaultValue = defaultInitializerFor(locator.statement.getType());
        if (defaultValue == null) {
            return null;
        }

        int insertOffset = locator.found.getStartPosition() + locator.found.getLength();
        BridgeTextEdit edit = insertionEdit(source, insertOffset, " = " + defaultValue);

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Initialize variable '" + variableName + "'";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        action.isPreferred = true;
        return action;
    }

    private static BridgeAction makeAddMissingJavadocTagAction(String uri, String source,
                                                                org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                BridgeDiagnostic d,
                                                                int problemId) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        org.eclipse.jdt.core.dom.MethodDeclaration method = loc.found;
        if (method == null || method.getJavadoc() == null) {
            return null;
        }

        String methodIndent = indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1);
        String lineIndent = methodIndent + " * ";
        int insertOffset = javadocClosingOffset(method.getJavadoc());
        String newText = null;
        String title = null;

        if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingParamTag) {
            String paramName = firstMissingParamTag(method);
            if (paramName == null) {
                return null;
            }
            newText = lineIndent + "@param " + paramName + "\n";
            title = "Add missing '@param " + paramName + "' tag";
        } else if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingReturnTag) {
            if (method.isConstructor() || method.getReturnType2() == null || "void".equals(method.getReturnType2().toString())) {
                return null;
            }
            newText = lineIndent + "@return\n";
            title = "Add missing '@return' tag";
        } else if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingThrowsTag) {
            String throwsName = firstMissingThrowsTag(method);
            if (throwsName == null) {
                return null;
            }
            newText = lineIndent + "@throws " + throwsName + "\n";
            title = "Add missing '@throws " + throwsName + "' tag";
        }

        if (newText == null || title == null) {
            return null;
        }

        BridgeTextEdit edit = insertionEdit(source, insertOffset,
                formatInsertedJavadocLines(source, method.getJavadoc(), methodIndent, newText));
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title;
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeAddAllMissingJavadocTagsAction(String uri, String source,
                                                                    org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                    BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        org.eclipse.jdt.core.dom.MethodDeclaration method = loc.found;
        if (method == null || method.getJavadoc() == null) {
            return null;
        }

        List<String> lines = new ArrayList<>();
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.SingleVariableDeclaration> params = method.parameters();
        for (org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter : params) {
            String name = parameter.getName().getIdentifier();
            if (!hasParamTag(method.getJavadoc(), name)) {
                lines.add("@param " + name);
            }
        }
        if (!method.isConstructor() && method.getReturnType2() != null
                && !"void".equals(method.getReturnType2().toString()) && !hasReturnTag(method.getJavadoc())) {
            lines.add("@return");
        }
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrownTypes = method.thrownExceptionTypes();
        for (org.eclipse.jdt.core.dom.Type thrownType : thrownTypes) {
            if (!hasThrowsTag(method.getJavadoc(), thrownType.toString())) {
                lines.add("@throws " + thrownType);
            }
        }

        if (lines.size() <= 1) {
            return null;
        }

        String methodIndent = indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1);
        String indent = methodIndent + " * ";
        StringBuilder newText = new StringBuilder();
        for (String line : lines) {
            newText.append(indent).append(line).append("\n");
        }

        BridgeTextEdit edit = insertionEdit(source, javadocClosingOffset(method.getJavadoc()),
                formatInsertedJavadocLines(source, method.getJavadoc(), methodIndent, newText.toString()));
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = "Add all missing Javadoc tags";
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeRemoveInvalidJavadocTagAction(String uri, String source,
                                                                   org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                   BridgeDiagnostic d) {
        int start = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        int end = CompilationService.lineColToOffset(source, d.endLine, d.endChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, start, Math.max(1, end - start));
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.TagElement)) {
            node = node.getParent();
        }
        if (!(node instanceof org.eclipse.jdt.core.dom.TagElement tag)) {
            return null;
        }

        int removeStart = lineStart(source, CompilationService.offsetToLineCol(source, tag.getStartPosition())[0]);
        int removeEnd = tag.getStartPosition() + tag.getLength();
        int[] endLc = CompilationService.offsetToLineCol(source, removeEnd);
        removeEnd = lineStart(source, endLc[0] + 1);
        if (removeEnd <= removeStart) {
            removeEnd = tag.getStartPosition() + tag.getLength();
        }

        return makeRemoveTextAction("Remove invalid Javadoc tag", uri, source, removeStart, removeEnd);
    }

    private static BridgeAction makeRemoveUnreachableCodeAction(String uri, String source,
                                                                 org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                 BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(offset);
        cu.accept(loc);
        if (loc.found == null) {
            return null;
        }
        return makeRemoveDeclarationNodeAction("Remove unreachable code", uri, source, loc.found);
    }

    private static BridgeAction makeRemoveDeclarationNodeAction(String title, String uri, String source,
                                                                 org.eclipse.jdt.core.dom.ASTNode node) {
        int start = node.getStartPosition();
        int end = start + node.getLength();
        if (end < source.length() && source.charAt(end) == '\n') {
            end++;
        }
        return makeRemoveTextAction(title, uri, source, start, end);
    }

    private static BridgeAction makeRemoveTextAction(String title, String uri, String source, int start, int end) {
        if (start < 0 || end < start) {
            return null;
        }
        int[] startLc = CompilationService.offsetToLineCol(source, start);
        int[] endLc = CompilationService.offsetToLineCol(source, end);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLc[0];
        edit.startChar = startLc[1];
        edit.endLine = endLc[0];
        edit.endChar = endLc[1];
        edit.newText = "";

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title;
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeReplaceNodeAction(String title, String uri, String source,
                                                       org.eclipse.jdt.core.dom.ASTNode node,
                                                       String newText) {
        int start = node.getStartPosition();
        int end = start + node.getLength();
        int[] startLc = CompilationService.offsetToLineCol(source, start);
        int[] endLc = CompilationService.offsetToLineCol(source, end);

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLc[0];
        edit.startChar = startLc[1];
        edit.endLine = endLc[0];
        edit.endChar = endLc[1];
        edit.newText = newText;

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title;
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static BridgeAction makeRenameToExistingTypeAction(String uri, String source,
                                                                org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                BridgeDiagnostic d,
                                                                String missingTypeName) {
        org.eclipse.jdt.core.dom.ASTNode node = findNodeAtDiagnostic(source, cu, d);
        if (node == null) {
            return null;
        }
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.Name)) {
            node = node.getParent();
        }
        if (!(node instanceof org.eclipse.jdt.core.dom.Name nameNode)) {
            return null;
        }

        String replacement = nearestTypeName(cu, missingTypeName);
        if (replacement == null || replacement.equals(missingTypeName)) {
            return null;
        }
        return makeReplaceNodeAction("Change to '" + replacement + "'", uri, source, nameNode, replacement);
    }

    private static BridgeAction makeAddTypeParameterAction(String uri, String source,
                                                            org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                            BridgeDiagnostic d,
                                                            String typeName) {
        org.eclipse.jdt.core.dom.ASTNode node = findNodeAtDiagnostic(source, cu, d);
        if (node == null) {
            return null;
        }

        org.eclipse.jdt.core.dom.ASTNode current = node;
        while (current != null) {
            if (current instanceof org.eclipse.jdt.core.dom.MethodDeclaration method) {
                if (hasTypeParameter(method.typeParameters(), typeName)) {
                    return null;
                }
                int insertOffset;
                String insertText;
                if (method.typeParameters().isEmpty()) {
                    insertOffset = method.isConstructor()
                            ? method.getName().getStartPosition()
                            : method.getReturnType2().getStartPosition();
                    insertText = "<" + typeName + "> ";
                } else {
                    org.eclipse.jdt.core.dom.TypeParameter last = (org.eclipse.jdt.core.dom.TypeParameter)
                            method.typeParameters().get(method.typeParameters().size() - 1);
                    insertOffset = last.getStartPosition() + last.getLength();
                    insertText = ", " + typeName;
                }
                return singleInsertionAction(
                        "Add type parameter '" + typeName + "' to '" + method.getName().getIdentifier() + "'",
                        uri, source, insertOffset, insertText);
            }
            if (current instanceof org.eclipse.jdt.core.dom.AbstractTypeDeclaration type) {
                if (type instanceof org.eclipse.jdt.core.dom.TypeDeclaration td) {
                    if (hasTypeParameter(td.typeParameters(), typeName)) {
                        return null;
                    }
                    int insertOffset;
                    String insertText;
                    if (td.typeParameters().isEmpty()) {
                        insertOffset = td.getName().getStartPosition() + td.getName().getLength();
                        insertText = "<" + typeName + ">";
                    } else {
                        org.eclipse.jdt.core.dom.TypeParameter last = (org.eclipse.jdt.core.dom.TypeParameter)
                                td.typeParameters().get(td.typeParameters().size() - 1);
                        insertOffset = last.getStartPosition() + last.getLength();
                        insertText = ", " + typeName;
                    }
                    return singleInsertionAction(
                            "Add type parameter '" + typeName + "' to '" + td.getName().getIdentifier() + "'",
                            uri, source, insertOffset, insertText);
                }
            }
            current = current.getParent();
        }
        return null;
    }

    private static BridgeAction makeCreateTopLevelTypeAction(String title, String uri, String source,
                                                              String typeName, String kind) {
        StringBuilder stub = new StringBuilder();
        if (!source.endsWith("\n")) {
            stub.append("\n");
        }
        stub.append("\n");
        switch (kind) {
            case "interface" -> stub.append("interface ").append(typeName).append(" {\n}\n");
            case "enum" -> stub.append("enum ").append(typeName).append(" {\n}\n");
            case "record" -> stub.append("record ").append(typeName).append("() {\n}\n");
            default -> stub.append("class ").append(typeName).append(" {\n}\n");
        }
        return singleInsertionAction(title, uri, source, source.length(), stub.toString());
    }

    private static BridgeAction makeCreateLocalVariableAction(String uri, String source,
                                                               org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                               BridgeDiagnostic d,
                                                               String variableName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(offset);
        cu.accept(loc);
        if (loc.found == null) {
            return null;
        }

        InferredValue inferred = inferValueFromStatement(source, loc.found, variableName);
        String indent = indentOf(source, CompilationService.offsetToLineCol(source, loc.found.getStartPosition())[0]);
        if (loc.found instanceof org.eclipse.jdt.core.dom.ExpressionStatement es
                && es.getExpression() instanceof org.eclipse.jdt.core.dom.Assignment assignment
                && assignment.getLeftHandSide() instanceof org.eclipse.jdt.core.dom.SimpleName lhs
                && variableName.equals(lhs.getIdentifier())) {
            return makeReplaceNodeAction("Create local variable '" + variableName + "'", uri, source, es,
                    indent + inferred.typeName + " " + variableName + " = " + source.substring(
                            assignment.getRightHandSide().getStartPosition(),
                            assignment.getRightHandSide().getStartPosition() + assignment.getRightHandSide().getLength()
                    ) + ";");
        }

        return singleInsertionAction("Create local variable '" + variableName + "'", uri, source,
                loc.found.getStartPosition(),
                indent + inferred.typeName + " " + variableName + " = " + inferred.initializer + ";\n");
    }

    private static BridgeAction makeCreateFieldAction(String uri, String source,
                                                       org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                       BridgeDiagnostic d,
                                                       String variableName,
                                                       boolean constant) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, offset, 0);
        org.eclipse.jdt.core.dom.AbstractTypeDeclaration type = enclosingType(node);
        if (type == null) {
            return null;
        }
        int insertOffset = bodyInsertOffset(type, source);
        if (insertOffset < 0) {
            return null;
        }

        StatementLocator loc = new StatementLocator(offset);
        cu.accept(loc);
        InferredValue inferred = inferValueFromStatement(source, loc.found, variableName);
        String indent = indentOf(source, cu.getLineNumber(type.getStartPosition()) - 1) + "    ";
        String declaration = constant
                ? indent + "private static final " + inferred.typeName + " " + variableName + " = " + inferred.initializer + ";\n"
                : indent + "private " + inferred.typeName + " " + variableName + ";\n";
        String title = constant ? "Create constant '" + variableName + "'" : "Create field '" + variableName + "'";
        return singleInsertionAction(title, uri, source, insertOffset, declaration);
    }

    private static BridgeAction makeCreateParameterAction(String uri, String source,
                                                           org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                           BridgeDiagnostic d,
                                                           String variableName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        org.eclipse.jdt.core.dom.MethodDeclaration method = loc.found;
        if (method == null) {
            return null;
        }
        if (method.parameters().stream().anyMatch(p ->
                p instanceof org.eclipse.jdt.core.dom.SingleVariableDeclaration svd
                        && variableName.equals(svd.getName().getIdentifier()))) {
            return null;
        }

        StatementLocator stmtLoc = new StatementLocator(offset);
        cu.accept(stmtLoc);
        InferredValue inferred = inferValueFromStatement(source, stmtLoc.found, variableName);
        int insertOffset;
        String insertText;
        if (method.parameters().isEmpty()) {
            int paren = source.indexOf('(', method.getName().getStartPosition() + method.getName().getLength());
            if (paren < 0) return null;
            insertOffset = paren + 1;
            insertText = inferred.typeName + " " + variableName;
        } else {
            org.eclipse.jdt.core.dom.SingleVariableDeclaration last =
                    (org.eclipse.jdt.core.dom.SingleVariableDeclaration) method.parameters().get(method.parameters().size() - 1);
            insertOffset = last.getStartPosition() + last.getLength();
            insertText = ", " + inferred.typeName + " " + variableName;
        }
        return singleInsertionAction("Create parameter '" + variableName + "'", uri, source, insertOffset, insertText);
    }

    private static BridgeAction makeAddExplicitSuperConstructorCallAction(String uri, String source,
                                                                           org.eclipse.jdt.core.dom.CompilationUnit cu,
                                                                           BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        org.eclipse.jdt.core.dom.MethodDeclaration constructor = loc.found;
        if (constructor == null || !constructor.isConstructor() || constructor.getBody() == null) {
            return null;
        }
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Statement> statements = constructor.getBody().statements();
        if (!statements.isEmpty() && (statements.get(0) instanceof org.eclipse.jdt.core.dom.SuperConstructorInvocation
                || statements.get(0) instanceof org.eclipse.jdt.core.dom.ConstructorInvocation)) {
            return null;
        }

        org.eclipse.jdt.core.dom.AbstractTypeDeclaration type = enclosingType(constructor);
        if (!(type instanceof org.eclipse.jdt.core.dom.TypeDeclaration td) || td.getSuperclassType() == null) {
            return null;
        }
        org.eclipse.jdt.core.dom.AbstractTypeDeclaration superType = findTypeByName(cu, simpleTypeName(td.getSuperclassType().toString()));
        if (!(superType instanceof org.eclipse.jdt.core.dom.TypeDeclaration superDecl)) {
            return null;
        }

        int paramCount = constructor.parameters().size();
        org.eclipse.jdt.core.dom.MethodDeclaration targetCtor = null;
        for (org.eclipse.jdt.core.dom.MethodDeclaration method : superDecl.getMethods()) {
            if (method.isConstructor() && method.parameters().size() == paramCount) {
                targetCtor = method;
                break;
            }
        }
        if (targetCtor == null) {
            for (org.eclipse.jdt.core.dom.MethodDeclaration method : superDecl.getMethods()) {
                if (method.isConstructor()) {
                    targetCtor = method;
                    break;
                }
            }
        }
        if (targetCtor == null) {
            return null;
        }

        String args;
        if (targetCtor.parameters().size() == constructor.parameters().size()) {
            List<String> names = new ArrayList<>();
            for (Object param : constructor.parameters()) {
                if (param instanceof org.eclipse.jdt.core.dom.SingleVariableDeclaration svd) {
                    names.add(svd.getName().getIdentifier());
                }
            }
            args = String.join(", ", names);
        } else {
            args = "";
        }
        String indent = indentOf(source, cu.getLineNumber(constructor.getStartPosition()) - 1) + "    ";
        String invocation = indent + "super(" + args + ");\n";
        return singleInsertionAction("Add explicit constructor invocation 'super(" + args + ")'", uri, source,
                assignmentInsertOffset(constructor, source), invocation);
    }

    private static String getEnclosingMethodName(org.eclipse.jdt.core.dom.CompilationUnit cu, int offset) {
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        return loc.found != null ? loc.found.getName().getIdentifier() : null;
    }

    private static org.eclipse.jdt.core.dom.SingleVariableDeclaration findMethodParameter(
            String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu,
            BridgeDiagnostic d,
            String paramName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ParameterLocator loc = new ParameterLocator(offset, paramName);
        cu.accept(loc);
        return loc.found;
    }

    private static org.eclipse.jdt.core.dom.AbstractTypeDeclaration enclosingType(org.eclipse.jdt.core.dom.ASTNode node) {
        org.eclipse.jdt.core.dom.ASTNode current = node;
        while (current != null) {
            if (current instanceof org.eclipse.jdt.core.dom.AbstractTypeDeclaration type) {
                return type;
            }
            current = current.getParent();
        }
        return null;
    }

    private static String chooseFieldName(org.eclipse.jdt.core.dom.AbstractTypeDeclaration type, String preferred) {
        Set<String> existing = new HashSet<>();
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof org.eclipse.jdt.core.dom.FieldDeclaration field) {
                for (Object fragObj : field.fragments()) {
                    if (fragObj instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment frag) {
                        existing.add(frag.getName().getIdentifier());
                    }
                }
            }
        }
        if (!existing.contains(preferred)) {
            return preferred;
        }
        String fallback = preferred + "Field";
        if (!existing.contains(fallback)) {
            return fallback;
        }
        int i = 2;
        while (existing.contains(fallback + i)) {
            i++;
        }
        return fallback + i;
    }

    private static int bodyInsertOffset(org.eclipse.jdt.core.dom.AbstractTypeDeclaration type, String source) {
        int brace = source.indexOf('{', type.getStartPosition());
        if (brace < 0) {
            return -1;
        }
        int newline = source.indexOf('\n', brace);
        return newline >= 0 ? newline + 1 : brace + 1;
    }

    private static int assignmentInsertOffset(org.eclipse.jdt.core.dom.MethodDeclaration method, String source) {
        org.eclipse.jdt.core.dom.Block body = method.getBody();
        if (body == null) {
            return method.getStartPosition();
        }

        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Statement> statements = body.statements();
        if (method.isConstructor() && !statements.isEmpty()) {
            org.eclipse.jdt.core.dom.Statement first = statements.get(0);
            if (first instanceof org.eclipse.jdt.core.dom.ConstructorInvocation
                    || first instanceof org.eclipse.jdt.core.dom.SuperConstructorInvocation) {
                int end = first.getStartPosition() + first.getLength();
                return end < source.length() && source.charAt(end) == '\n' ? end + 1 : end;
            }
        }

        int brace = body.getStartPosition();
        int newline = source.indexOf('\n', brace);
        return newline >= 0 ? newline + 1 : brace + 1;
    }

    private static boolean hasParamTag(org.eclipse.jdt.core.dom.Javadoc javadoc, String paramName) {
        if (javadoc == null || paramName == null) {
            return false;
        }
        for (Object tagObj : javadoc.tags()) {
            if (!(tagObj instanceof org.eclipse.jdt.core.dom.TagElement tag)) continue;
            if (!"@param".equals(tag.getTagName())) continue;
            List<?> fragments = tag.fragments();
            if (!fragments.isEmpty() && fragments.get(0) instanceof org.eclipse.jdt.core.dom.SimpleName name
                    && paramName.equals(name.getIdentifier())) {
                return true;
            }
        }
        return false;
    }

    private static boolean hasReturnTag(org.eclipse.jdt.core.dom.Javadoc javadoc) {
        if (javadoc == null) {
            return false;
        }
        for (Object tagObj : javadoc.tags()) {
            if (tagObj instanceof org.eclipse.jdt.core.dom.TagElement tag
                    && "@return".equals(tag.getTagName())) {
                return true;
            }
        }
        return false;
    }

    private static boolean hasThrowsTag(org.eclipse.jdt.core.dom.Javadoc javadoc, String typeName) {
        return findThrowsTag(javadoc, typeName) != null;
    }

    private static String buildParameterDocumentationStub(org.eclipse.jdt.core.dom.MethodDeclaration method,
                                                           String paramName,
                                                           String indent) {
        StringBuilder out = new StringBuilder();
        out.append(indent).append("/**\n");
        out.append(indent).append(" * \n");
        out.append(indent).append(" * @param ").append(paramName).append("\n");
        if (!method.isConstructor() && method.getReturnType2() != null
                && !"void".equals(method.getReturnType2().toString())) {
            out.append(indent).append(" * @return\n");
        }
        out.append(indent).append(" */\n");
        return out.toString();
    }

    private static org.eclipse.jdt.core.dom.ASTNode findNodeAtDiagnostic(
            String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu,
            BridgeDiagnostic d) {
        int start = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        int end = CompilationService.lineColToOffset(source, d.endLine, d.endChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, start, Math.max(1, end - start));
        if (node == null && start > 0) {
            node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, start - 1, 1);
        }
        return node;
    }

    private static String nearestTypeName(org.eclipse.jdt.core.dom.CompilationUnit cu, String missingTypeName) {
        Set<String> names = new LinkedHashSet<>();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration node) {
                names.add(node.getName().getIdentifier());
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration node) {
                names.add(node.getName().getIdentifier());
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.AnnotationTypeDeclaration node) {
                names.add(node.getName().getIdentifier());
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.RecordDeclaration node) {
                names.add(node.getName().getIdentifier());
                return true;
            }
        });

        String best = null;
        int bestScore = Integer.MAX_VALUE;
        for (String name : names) {
            if (name.equals(missingTypeName)) continue;
            int score = levenshteinDistance(missingTypeName, name);
            if (score < bestScore) {
                best = name;
                bestScore = score;
            }
        }
        return bestScore <= Math.max(4, missingTypeName.length()) ? best : null;
    }

    private static int levenshteinDistance(String a, String b) {
        int[][] dp = new int[a.length() + 1][b.length() + 1];
        for (int i = 0; i <= a.length(); i++) dp[i][0] = i;
        for (int j = 0; j <= b.length(); j++) dp[0][j] = j;
        for (int i = 1; i <= a.length(); i++) {
            for (int j = 1; j <= b.length(); j++) {
                int cost = Character.toLowerCase(a.charAt(i - 1)) == Character.toLowerCase(b.charAt(j - 1)) ? 0 : 1;
                dp[i][j] = Math.min(
                        Math.min(dp[i - 1][j] + 1, dp[i][j - 1] + 1),
                        dp[i - 1][j - 1] + cost
                );
            }
        }
        return dp[a.length()][b.length()];
    }

    private static boolean hasTypeParameter(List<?> typeParameters, String typeName) {
        for (Object param : typeParameters) {
            if (param instanceof org.eclipse.jdt.core.dom.TypeParameter tp
                    && typeName.equals(tp.getName().getIdentifier())) {
                return true;
            }
        }
        return false;
    }

    private static BridgeAction singleInsertionAction(String title, String uri, String source, int offset, String newText) {
        BridgeTextEdit edit = insertionEdit(source, offset, newText);
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = List.of(edit);

        BridgeAction action = new BridgeAction();
        action.title = title;
        action.kind = "quickfix";
        action.edits = List.of(fe);
        return action;
    }

    private static InferredValue inferValueFromStatement(String source,
                                                          org.eclipse.jdt.core.dom.Statement statement,
                                                          String variableName) {
        if (statement instanceof org.eclipse.jdt.core.dom.ExpressionStatement es
                && es.getExpression() instanceof org.eclipse.jdt.core.dom.Assignment assignment
                && assignment.getLeftHandSide() instanceof org.eclipse.jdt.core.dom.SimpleName lhs
                && variableName.equals(lhs.getIdentifier())) {
            org.eclipse.jdt.core.dom.Expression rhs = assignment.getRightHandSide();
            return inferValueFromExpression(source, rhs);
        }
        return new InferredValue("int", "0");
    }

    private static InferredValue inferValueFromExpression(String source, org.eclipse.jdt.core.dom.Expression expression) {
        if (expression == null) {
            return new InferredValue("int", "0");
        }
        if (expression instanceof org.eclipse.jdt.core.dom.StringLiteral) {
            return new InferredValue("String", source.substring(expression.getStartPosition(), expression.getStartPosition() + expression.getLength()));
        }
        if (expression instanceof org.eclipse.jdt.core.dom.BooleanLiteral) {
            return new InferredValue("boolean", source.substring(expression.getStartPosition(), expression.getStartPosition() + expression.getLength()));
        }
        if (expression instanceof org.eclipse.jdt.core.dom.CharacterLiteral) {
            return new InferredValue("char", source.substring(expression.getStartPosition(), expression.getStartPosition() + expression.getLength()));
        }
        if (expression instanceof org.eclipse.jdt.core.dom.NumberLiteral num) {
            String token = num.getToken();
            String lower = token.toLowerCase(Locale.ROOT);
            if (lower.endsWith("l")) return new InferredValue("long", token);
            if (lower.endsWith("f")) return new InferredValue("float", token);
            if (lower.endsWith("d") || token.contains(".")) return new InferredValue("double", token);
            return new InferredValue("int", token);
        }
        if (expression instanceof org.eclipse.jdt.core.dom.ClassInstanceCreation cic) {
            String text = source.substring(expression.getStartPosition(), expression.getStartPosition() + expression.getLength());
            return new InferredValue(cic.getType().toString(), text);
        }
        if (expression instanceof org.eclipse.jdt.core.dom.NullLiteral) {
            return new InferredValue("Object", "null");
        }
        String text = source.substring(expression.getStartPosition(), expression.getStartPosition() + expression.getLength());
        return new InferredValue("Object", "null".equals(text) ? "null" : text);
    }

    private static String formatInsertedJavadocLines(String source,
                                                      org.eclipse.jdt.core.dom.Javadoc javadoc,
                                                      String methodIndent,
                                                      String lines) {
        int startLine = CompilationService.offsetToLineCol(source, javadoc.getStartPosition())[0];
        int endLine = CompilationService.offsetToLineCol(source, javadoc.getStartPosition() + javadoc.getLength())[0];
        if (startLine == endLine) {
            return "\n" + lines + methodIndent + " ";
        }
        return lines;
    }

    private static org.eclipse.jdt.core.dom.CastExpression findCastExpressionAtDiagnostic(
            String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu,
            BridgeDiagnostic d) {
        int start = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        int end = CompilationService.lineColToOffset(source, d.endLine, d.endChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, start, Math.max(1, end - start));
        while (node != null) {
            if (node instanceof org.eclipse.jdt.core.dom.CastExpression cast) {
                return cast;
            }
            node = node.getParent();
        }
        return null;
    }

    private static org.eclipse.jdt.core.dom.Type findThrownExceptionType(
            org.eclipse.jdt.core.dom.MethodDeclaration method,
            int offset) {
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrownTypes = method.thrownExceptionTypes();
        if (thrownTypes.isEmpty()) {
            return null;
        }
        for (org.eclipse.jdt.core.dom.Type thrownType : thrownTypes) {
            int start = thrownType.getStartPosition();
            int end = start + thrownType.getLength();
            if (start <= offset && offset <= end) {
                return thrownType;
            }
        }
        return thrownTypes.size() == 1 ? thrownTypes.get(0) : null;
    }

    private static BridgeTextEdit removalEditForThrownType(String source,
                                                            org.eclipse.jdt.core.dom.MethodDeclaration method,
                                                            org.eclipse.jdt.core.dom.Type thrownType) {
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrownTypes = method.thrownExceptionTypes();
        int index = thrownTypes.indexOf(thrownType);
        if (index < 0) {
            return null;
        }
        if (thrownTypes.size() > 1) {
            return deletionEditForNodes(source, castAstNodes(thrownTypes), index);
        }

        int methodStart = method.getStartPosition();
        int searchEnd = method.getBody() != null
                ? method.getBody().getStartPosition()
                : methodStart + method.getLength();
        int throwsStart = source.indexOf("throws", methodStart);
        if (throwsStart < 0 || throwsStart > searchEnd) {
            return null;
        }
        while (throwsStart > 0 && Character.isWhitespace(source.charAt(throwsStart - 1))) {
            throwsStart--;
        }
        return removalEditFromOffsets(source, throwsStart, thrownType.getStartPosition() + thrownType.getLength());
    }

    private static BridgeTextEdit removeThrowsJavadocTagEdit(String source,
                                                              org.eclipse.jdt.core.dom.Javadoc javadoc,
                                                              String typeName) {
        org.eclipse.jdt.core.dom.TagElement tag = findThrowsTag(javadoc, typeName);
        if (tag == null) {
            return null;
        }
        int startLine = CompilationService.offsetToLineCol(source, tag.getStartPosition())[0];
        int endLine = CompilationService.offsetToLineCol(source, tag.getStartPosition() + tag.getLength())[0];
        int start = lineStart(source, startLine);
        int end = lineStart(source, endLine + 1);
        if (end <= start) {
            end = tag.getStartPosition() + tag.getLength();
        }
        return removalEditFromOffsets(source, start, end);
    }

    private static org.eclipse.jdt.core.dom.TagElement findThrowsTag(org.eclipse.jdt.core.dom.Javadoc javadoc,
                                                                      String typeName) {
        if (javadoc == null || typeName == null) {
            return null;
        }
        String simpleName = simpleTypeName(typeName);
        for (Object tagObj : javadoc.tags()) {
            if (!(tagObj instanceof org.eclipse.jdt.core.dom.TagElement tag)
                    || !"@throws".equals(tag.getTagName())) {
                continue;
            }
            for (Object fragmentObj : tag.fragments()) {
                if (fragmentObj instanceof org.eclipse.jdt.core.dom.Name name) {
                    String candidate = name.getFullyQualifiedName();
                    if (typeName.equals(candidate) || simpleName.equals(candidate)) {
                        return tag;
                    }
                } else if (fragmentObj instanceof org.eclipse.jdt.core.dom.SimpleName name) {
                    if (simpleName.equals(name.getIdentifier())) {
                        return tag;
                    }
                }
            }
        }
        return null;
    }

    private static String firstMissingParamTag(org.eclipse.jdt.core.dom.MethodDeclaration method) {
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.SingleVariableDeclaration> params = method.parameters();
        for (org.eclipse.jdt.core.dom.SingleVariableDeclaration parameter : params) {
            String name = parameter.getName().getIdentifier();
            if (!hasParamTag(method.getJavadoc(), name)) {
                return name;
            }
        }
        return null;
    }

    private static String firstMissingThrowsTag(org.eclipse.jdt.core.dom.MethodDeclaration method) {
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrownTypes = method.thrownExceptionTypes();
        for (org.eclipse.jdt.core.dom.Type thrownType : thrownTypes) {
            String typeName = thrownType.toString();
            if (!hasThrowsTag(method.getJavadoc(), typeName)) {
                return typeName;
            }
        }
        return null;
    }

    private static org.eclipse.jdt.core.dom.AbstractTypeDeclaration findTypeByName(
            org.eclipse.jdt.core.dom.CompilationUnit cu,
            String typeName) {
        final org.eclipse.jdt.core.dom.AbstractTypeDeclaration[] result = new org.eclipse.jdt.core.dom.AbstractTypeDeclaration[1];
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration node) {
                if (typeName.equals(node.getName().getIdentifier())) {
                    result[0] = node;
                    return false;
                }
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration node) {
                if (typeName.equals(node.getName().getIdentifier())) {
                    result[0] = node;
                    return false;
                }
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.RecordDeclaration node) {
                if (typeName.equals(node.getName().getIdentifier())) {
                    result[0] = node;
                    return false;
                }
                return true;
            }
        });
        return result[0];
    }

    private static int javadocClosingOffset(org.eclipse.jdt.core.dom.Javadoc javadoc) {
        return Math.max(javadoc.getStartPosition(), javadoc.getStartPosition() + javadoc.getLength() - 3);
    }

    private static String simpleTypeName(String typeName) {
        int dot = typeName.lastIndexOf('.');
        return dot >= 0 ? typeName.substring(dot + 1) : typeName;
    }

    private static String defaultInitializerFor(org.eclipse.jdt.core.dom.Type type) {
        if (type.isPrimitiveType()) {
            org.eclipse.jdt.core.dom.PrimitiveType.Code code =
                    ((org.eclipse.jdt.core.dom.PrimitiveType) type).getPrimitiveTypeCode();
            if (org.eclipse.jdt.core.dom.PrimitiveType.BOOLEAN.equals(code)) return "false";
            if (org.eclipse.jdt.core.dom.PrimitiveType.CHAR.equals(code)) return "'\\0'";
            if (org.eclipse.jdt.core.dom.PrimitiveType.LONG.equals(code)) return "0L";
            if (org.eclipse.jdt.core.dom.PrimitiveType.FLOAT.equals(code)) return "0.0f";
            if (org.eclipse.jdt.core.dom.PrimitiveType.DOUBLE.equals(code)) return "0.0d";
            return "0";
        }
        return "null";
    }

    private static BridgeTextEdit insertionEdit(String source, int offset, String newText) {
        int[] lc = CompilationService.offsetToLineCol(source, offset);
        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = lc[0];
        edit.startChar = lc[1];
        edit.endLine = lc[0];
        edit.endChar = lc[1];
        edit.newText = newText;
        return edit;
    }

    private static BridgeTextEdit removalEditFromOffsets(String source, int start, int end) {
        int[] startLc = CompilationService.offsetToLineCol(source, start);
        int[] endLc = CompilationService.offsetToLineCol(source, end);
        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLc[0];
        edit.startChar = startLc[1];
        edit.endLine = endLc[0];
        edit.endChar = endLc[1];
        edit.newText = "";
        return edit;
    }

    private static List<BridgeAction> dedupeActions(List<BridgeAction> actions) {
        Map<String, BridgeAction> deduped = new LinkedHashMap<>();
        for (BridgeAction action : actions) {
            deduped.putIfAbsent(actionKey(action), action);
        }
        return new ArrayList<>(deduped.values());
    }

    private static String actionKey(BridgeAction action) {
        StringBuilder key = new StringBuilder();
        key.append(action.title).append('\n').append(action.kind).append('\n');
        if (action.edits != null) {
            for (BridgeFileEdit fileEdit : action.edits) {
                key.append(fileEdit.uri).append('\n');
                if (fileEdit.edits == null) continue;
                for (BridgeTextEdit edit : fileEdit.edits) {
                    key.append(edit.startLine).append(':').append(edit.startChar).append('-')
                            .append(edit.endLine).append(':').append(edit.endChar).append('=')
                            .append(edit.newText).append('\n');
                }
            }
        }
        return key.toString();
    }

    private static BridgeTextEdit deletionEditForNodes(String source, List<? extends org.eclipse.jdt.core.dom.ASTNode> nodes, int index) {
        if (index < 0 || index >= nodes.size()) {
            return null;
        }

        org.eclipse.jdt.core.dom.ASTNode current = nodes.get(index);
        int start = current.getStartPosition();
        int end = start + current.getLength();

        if (nodes.size() > 1) {
            if (index == 0) {
                end = nodes.get(1).getStartPosition();
            } else if (index == nodes.size() - 1) {
                org.eclipse.jdt.core.dom.ASTNode previous = nodes.get(index - 1);
                start = previous.getStartPosition() + previous.getLength();
                while (start > 0 && start < current.getStartPosition() && source.charAt(start) != ',') {
                    start++;
                }
            } else {
                end = nodes.get(index + 1).getStartPosition();
            }
        }

        int[] startLc = CompilationService.offsetToLineCol(source, start);
        int[] endLc = CompilationService.offsetToLineCol(source, end);
        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = startLc[0];
        edit.startChar = startLc[1];
        edit.endLine = endLc[0];
        edit.endChar = endLc[1];
        edit.newText = "";
        return edit;
    }

    private static List<org.eclipse.jdt.core.dom.ASTNode> castAstNodes(List<?> nodes) {
        List<org.eclipse.jdt.core.dom.ASTNode> result = new ArrayList<>(nodes.size());
        for (Object node : nodes) {
            if (node instanceof org.eclipse.jdt.core.dom.ASTNode astNode) {
                result.add(astNode);
            }
        }
        return result;
    }

    private record InferredValue(String typeName, String initializer) {}

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

    static class JavadocTargetLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final org.eclipse.jdt.core.dom.CompilationUnit cu;
        final int startLine;
        final int endLine;
        org.eclipse.jdt.core.dom.BodyDeclaration best;
        int bestPriority = Integer.MAX_VALUE;

        JavadocTargetLocator(org.eclipse.jdt.core.dom.CompilationUnit cu, int startLine, int endLine) {
            this.cu = cu;
            this.startLine = startLine;
            this.endLine = endLine;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.MethodDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.FieldDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.EnumConstantDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.AnnotationTypeDeclaration node) {
            consider(node);
            return true;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.RecordDeclaration node) {
            consider(node);
            return true;
        }

        private void consider(org.eclipse.jdt.core.dom.BodyDeclaration declaration) {
            int start = declaration.getStartPosition();
            int end = start + declaration.getLength();
            int declarationStartLine = cu.getLineNumber(start) - 1;
            int declarationEndLine = cu.getLineNumber(Math.max(start, end - 1)) - 1;
            if (declarationStartLine < 0 || declarationEndLine < 0) {
                return;
            }
            if (declarationStartLine > endLine || declarationEndLine < startLine) {
                return;
            }

            int priority = declarationStartLine <= startLine && startLine <= declarationEndLine
                ? Math.max(0, startLine - declarationStartLine)
                : 1000 + Math.max(Math.abs(declarationStartLine - startLine),
                                  Math.abs(declarationEndLine - endLine));

            if (best == null
                    || priority < bestPriority
                    || (priority == bestPriority && declaration.getLength() < best.getLength())) {
                best = declaration;
                bestPriority = priority;
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

    static class ParameterLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int offset;
        final String name;
        org.eclipse.jdt.core.dom.SingleVariableDeclaration found;

        ParameterLocator(int offset, String name) {
            this.offset = offset;
            this.name = name;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.SingleVariableDeclaration node) {
            if (!(node.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration)) {
                return true;
            }
            if (name != null && !name.equals(node.getName().getIdentifier())) {
                return true;
            }
            int start = node.getStartPosition();
            int end = start + node.getLength();
            if (start <= offset && offset <= end) {
                found = node;
                return false;
            }
            if (found == null) {
                found = node;
            }
            return true;
        }
    }

    static class LocalVariableFragmentLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        final int usageOffset;
        final String variableName;
        org.eclipse.jdt.core.dom.VariableDeclarationFragment found;
        org.eclipse.jdt.core.dom.VariableDeclarationStatement statement;
        org.eclipse.jdt.core.dom.MethodDeclaration method;

        LocalVariableFragmentLocator(int usageOffset, String variableName) {
            this.usageOffset = usageOffset;
            this.variableName = variableName;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.MethodDeclaration node) {
            if (method != null) {
                return false;
            }
            int start = node.getStartPosition();
            int end = start + node.getLength();
            if (start <= usageOffset && usageOffset <= end) {
                method = node;
                return true;
            }
            return false;
        }

        @Override
        public boolean visit(org.eclipse.jdt.core.dom.VariableDeclarationStatement node) {
            if (method == null) {
                return false;
            }
            @SuppressWarnings("unchecked")
            List<org.eclipse.jdt.core.dom.VariableDeclarationFragment> fragments = node.fragments();
            for (org.eclipse.jdt.core.dom.VariableDeclarationFragment fragment : fragments) {
                if (variableName.equals(fragment.getName().getIdentifier())) {
                    found = fragment;
                    statement = node;
                    return false;
                }
            }
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
