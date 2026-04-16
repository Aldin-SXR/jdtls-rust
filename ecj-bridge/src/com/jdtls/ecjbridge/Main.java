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
 */
public class Main {

    private static final Logger LOG = Logger.getLogger(Main.class.getName());
    private static final Gson GSON = new GsonBuilder().serializeNulls().create();

    public static void main(String[] args) throws Exception {
        LogManager.getLogManager().reset();
        Logger root = Logger.getLogger("");
        root.setLevel(Level.ALL);
        java.util.logging.StreamHandler stderrHandler =
            new java.util.logging.StreamHandler(System.err, new java.util.logging.SimpleFormatter());
        stderrHandler.setLevel(Level.ALL);
        root.addHandler(stderrHandler);

        BufferedReader stdin = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        PrintWriter stdout = new PrintWriter(new OutputStreamWriter(System.out, "UTF-8"), true);

        CompilationService compiler = new CompilationService();
        CompletionService completer = new CompletionService();
        FormatterService formatter = new FormatterService();
        AstNavigationService navigation = new AstNavigationService();

        // Build jrt:/ index eagerly in background so the first import completion
        // doesn't block the request thread (~1 s on cold start).
        Thread indexThread = new Thread(() -> CompletionService.ensureJrtIndex(), "jrt-index-builder");
        indexThread.setDaemon(true);
        indexThread.start();

        LOG.info("ecj-bridge ready");

        String line;
        while ((line = stdin.readLine()) != null) {
            if (line.trim().isEmpty()) continue;
            try {
                Request req = GSON.fromJson(line, Request.class);
                Object response = dispatch(req, compiler, completer, formatter, navigation);
                stdout.println(GSON.toJson(response));
            } catch (Exception e) {
                long id = extractId(line);
                ErrorResponse err = new ErrorResponse(id, e.getClass().getSimpleName() + ": " + e.getMessage());
                stdout.println(GSON.toJson(err));
                LOG.log(Level.SEVERE, "Error processing request: " + line, e);
            }
        }
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
                    req.files, orEmpty(req.classpath), orDefault(req.sourceLevel), req.uri);
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
            case "typeHierarchyPrepare" -> {
                List<BridgeTypeHierarchyItem> items = navigation.prepareTypeHierarchy(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new TypeHierarchyPrepareResponse(req.id, items);
            }
            case "typeHierarchySupertypes" -> {
                List<BridgeTypeHierarchyItem> items = navigation.supertypes(
                    req.files, orDefault(req.sourceLevel), req.data != null ? req.data : "");
                yield new TypeHierarchyItemsResponse(req.id, "typeHierarchySupertypes", items);
            }
            case "typeHierarchySubtypes" -> {
                List<BridgeTypeHierarchyItem> items = navigation.subtypes(
                    req.files, orDefault(req.sourceLevel), req.data != null ? req.data : "");
                yield new TypeHierarchyItemsResponse(req.id, "typeHierarchySubtypes", items);
            }
            case "callHierarchyPrepare" -> {
                List<BridgeCallHierarchyItem> items = navigation.prepareCallHierarchy(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new CallHierarchyPrepareResponse(req.id, items);
            }
            case "callHierarchyIncoming" -> {
                List<BridgeCallHierarchyIncomingCall> calls = navigation.incomingCalls(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new CallHierarchyIncomingCallsResponse(req.id, calls);
            }
            case "callHierarchyOutgoing" -> {
                List<BridgeCallHierarchyOutgoingCall> calls = navigation.outgoingCalls(
                    req.files, orDefault(req.sourceLevel), req.uri, req.offset);
                yield new CallHierarchyOutgoingCallsResponse(req.id, calls);
            }
            case "shutdown" -> new OkResponse(req.id);
            default -> new ErrorResponse(req.id, "Unknown method: " + req.method);
        };
    }

    private static List<BridgeAction> codeActions(Request req, CompilationService compiler) {
        if (req.files == null || req.uri == null) return List.of();
        String source = req.files.get(req.uri);
        if (source == null) return List.of();

        // Always re-compile to get full diagnostic metadata (problemId, categoryId)
        List<BridgeDiagnostic> diags = compiler.compile(
            req.files, orEmpty(req.classpath), orDefault(req.sourceLevel));

        org.eclipse.jdt.core.dom.CompilationUnit cu = parseForFixes(source, orDefault(req.sourceLevel), orEmpty(req.classpath));

        List<BridgeAction> actions = new ArrayList<>();
        boolean hasOverlappingDiagnostic = false;

        // Collect all unresolved type names from the entire file (for "Add all missing imports").
        // Pre-scan: collect ALL unresolved type names across the whole file (not just the range).
        // Used for "Add all missing imports". Uses the same broad detection as the per-diag loop.
        List<String> allFileUnresolvedTypes = new ArrayList<>();
        String targetUriNorm = req.uri.replace("file:///", "file:/").replace("\\", "/");
        for (BridgeDiagnostic d : diags) {
            String diagUriNorm = d.uri.replace("file:///", "file:/").replace("\\", "/");
            boolean uriMatch2 = targetUriNorm.equalsIgnoreCase(diagUriNorm)
                    || targetUriNorm.endsWith(diagUriNorm) || diagUriNorm.endsWith(targetUriNorm);
            if (!uriMatch2) continue;
            String msg2 = d.message != null ? d.message : "";
            int pid2 = problemId(d);
            boolean isUndefinedType = pid2 == org.eclipse.jdt.core.compiler.IProblem.UndefinedType
                    || pid2 == org.eclipse.jdt.core.compiler.IProblem.JavadocUndefinedType
                    || (msg2.contains("cannot be resolved") && (msg2.contains("type")
                            || (!msg2.isEmpty() && Character.isUpperCase(msg2.charAt(0)))));
            if (!isUndefinedType) continue;
            Matcher m2 = Pattern.compile("([A-Z][\\w$]*) cannot be resolved").matcher(msg2);
            if (m2.find()) allFileUnresolvedTypes.add(m2.group(1));
        }

        for (BridgeDiagnostic d : diags) {
            String targetUri = req.uri.replace("file:///", "file:/").replace("\\", "/");
            String diagUri = d.uri.replace("file:///", "file:/").replace("\\", "/");
            
            boolean uriMatch = targetUri.equalsIgnoreCase(diagUri) 
                || targetUri.endsWith(diagUri) 
                || diagUri.endsWith(targetUri);

            if (!uriMatch) continue;

            if (req.range != null) {
                // Diagnostic line matching (0-based)
                boolean overlaps = d.startLine <= req.range.endLine && d.endLine >= req.range.startLine;
                if (!overlaps) continue;
            }
            hasOverlappingDiagnostic = true;

            String msg = d.message != null ? d.message : "";
            int problemId = problemId(d);

            boolean isUnusedLocal = d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("local variable") || msg.contains("The value of the local"));
            boolean isUnusedParameter = problemId == org.eclipse.jdt.core.compiler.IProblem.ArgumentIsNeverUsed
                    || (d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("parameter") && (msg.contains("not used") || msg.contains("never read"))));

            if (d.severity == 2 && !isUnusedLocal) {
                String token = suppressToken(d, msg);
                if (token != null) {
                    actions.add(makeSuppressAction(token, req.uri, source, d, cu));
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.PublicClassMustMatchFileName) {
                Matcher m = Pattern.compile("The public type ([\\w$]+) must be defined").matcher(msg);
                if (m.find()) {
                    String typeName = m.group(1);
                    String fileName = req.uri.substring(req.uri.lastIndexOf('/') + 1);
                    if (fileName.endsWith(".java")) {
                        String expectedName = fileName.substring(0, fileName.length() - 5);
                        for (Object o : cu.types()) {
                            if (o instanceof org.eclipse.jdt.core.dom.TypeDeclaration td && td.getName().getIdentifier().equals(typeName)) {
                                actions.add(makeReplaceNodeAction("Rename type to '" + expectedName + "'", req.uri, source, td.getName(), expectedName));
                            }
                        }
                    }
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.PackageIsNotExpectedPackage) {
                Matcher mExp = Pattern.compile("expected is \"([\\w.]+)\"").matcher(msg);
                if (mExp.find()) {
                    actions.add(makeChangePackageAction(req.uri, source, cu, mExp.group(1)));
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnterminatedString) {
                actions.add(makeAddQuoteAction(req.uri, source, d));
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.VoidMethodReturnsValue
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.MethodReturnsVoid
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.MissingReturnType) {
                Matcher m = Pattern.compile("change return type to ([\\w.$]+)").matcher(msg);
                if (m.find()) {
                    actions.add(makeChangeReturnTypeAction(req.uri, source, cu, d, m.group(1)));
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.ShouldReturnValue
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.ShouldReturnValueHintMissingDefault) {
                actions.add(makeAddReturnStatementAction(req.uri, source, cu, d));
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.NonStaticAccessToStaticField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.NonStaticAccessToStaticMethod
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.IndirectAccessToStaticField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.IndirectAccessToStaticMethod) {
                Matcher m = Pattern.compile("The static (?:field|method) [\\w$]+(?:\\(.*\\))? from the type ([\\w$]+) should be accessed in a static way").matcher(msg);
                if (m.find()) {
                    actions.add(makeCorrectStaticAccessAction(req.uri, source, cu, d, m.group(1)));
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UndefinedType
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocUndefinedType
                    || msg.contains("cannot be resolved") && (msg.contains("type") || (msg.length() > 0 && Character.isUpperCase(msg.charAt(0))))) {
                String simpleName = null;
                Matcher m = Pattern.compile("([A-Z][\\w$]*) cannot be resolved").matcher(msg);
                if (m.find()) {
                    simpleName = m.group(1);
                } else {
                    Matcher m2 = Pattern.compile("type ([\\w$]+) cannot be resolved").matcher(msg);
                    if (m2.find()) simpleName = m2.group(1);
                }

                if (simpleName != null) {
                    CompletionService.ensureJrtIndex();
                    List<String> candidates = CompletionService.searchBySimpleName(simpleName);
                    boolean first = true;
                    for (String fqn : candidates) {
                        actions.add(makeImportAction("Import '" + fqn + "'", req.uri, source, fqn, first, cu));
                        first = false;
                    }

                    actions.add(makeCreateTopLevelTypeAction("Create class '" + simpleName + "'", req.uri, source, simpleName, "class"));
                    actions.add(makeCreateTopLevelTypeAction("Create interface '" + simpleName + "'", req.uri, source, simpleName, "interface"));

                    // "Change to 'X' (pkg)" — similar JRT types with different names
                    List<String> similar = CompletionService.searchSimilarTypeNames(simpleName, 5);
                    for (String fqn : similar) {
                        BridgeAction a = makeChangeToTypeAction(req.uri, source, cu, d, fqn);
                        if (a != null) actions.add(a);
                    }

                    BridgeAction renameTypeAction = makeRenameToExistingTypeAction(req.uri, source, cu, d, simpleName);
                    if (renameTypeAction != null) actions.add(renameTypeAction);
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UndefinedMethod || msg.contains("is undefined for the type")) {
                String methodName = null;
                // Broader regex: "The method methodName(Args) is undefined for the type TypeName"
                Matcher m = Pattern.compile("The method ([\\w$]+)\\(.*\\) is undefined").matcher(msg);
                if (m.find()) {
                    methodName = m.group(1);
                } else {
                    int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
                    ExpressionLocator eloc = new ExpressionLocator(off);
                    cu.accept(eloc);
                    if (eloc.found instanceof org.eclipse.jdt.core.dom.MethodInvocation mi) {
                        methodName = mi.getName().getIdentifier();
                    }
                }

                if (methodName != null) {
                    BridgeAction a = makeCreateMethodAction(req.uri, source, cu, d, methodName);
                    if (a != null) actions.add(a);
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UndefinedField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UndefinedName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnresolvedVariable
                    || msg.contains("cannot be resolved to a variable")) {
                String variableName = null;
                Matcher m = Pattern.compile("([a-zA-Z_$][\\w$]*) cannot be resolved").matcher(msg);
                if (m.find()) {
                    variableName = m.group(1);
                }

                if (variableName != null) {
                    int diagOff = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
                    MethodLocator mLoc = new MethodLocator(diagOff); cu.accept(mLoc);
                    boolean insideMethod = mLoc.found != null;

                    // Local variable and parameter only make sense inside a method/constructor body
                    if (insideMethod) {
                        BridgeAction localAction = makeCreateLocalVariableAction(req.uri, source, cu, d, variableName);
                        if (localAction != null) actions.add(localAction);
                        BridgeAction paramAction = makeCreateParameterAction(req.uri, source, cu, d, variableName);
                        if (paramAction != null) actions.add(paramAction);
                    }
                    // Field and constant always make sense when there's an enclosing class
                    BridgeAction fieldAction = makeCreateFieldAction(req.uri, source, cu, d, variableName, false);
                    if (fieldAction != null) actions.add(fieldAction);
                    BridgeAction constAction = makeCreateFieldAction(req.uri, source, cu, d, variableName, true);
                    if (constAction != null) actions.add(constAction);
                }
            }

            if (d.categoryId == org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE
                    && (msg.contains("import") && (msg.contains("never used") || msg.contains("not used")))) {
                actions.add(makeRemoveLineAction("Remove unused import", req.uri, source, d.startLine));
            }

            if (isUnusedLocal) {
                Matcher mVar = Pattern.compile("(?:value of the local variable|local variable) (\\w+)").matcher(msg);
                String varName = mVar.find() ? mVar.group(1) : null;
                int diagOffset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);

                BridgeAction removeAllAction = makeRemoveAllAssignmentsAction(req.uri, source, cu, d, varName);
                if (removeAllAction != null) actions.add(removeAllAction);
                else {
                    BridgeAction removeAction = makeRemoveDeclarationAction(req.uri, source, cu, d);
                    if (removeAction != null) actions.add(removeAction);
                }

                BridgeAction varSuppressAction = makeLocalVarSuppressAction("unused", req.uri, source, cu, d, varName);
                if (varSuppressAction != null) actions.add(varSuppressAction);

                String methodName = getEnclosingMethodName(cu, diagOffset);
                BridgeAction methodSuppressAction = makeSuppressAction("unused", req.uri, source, d, cu);
                if (methodName != null) methodSuppressAction.title = "Add @SuppressWarnings(\"unused\") to '" + methodName + "'";
                actions.add(methodSuppressAction);

                BridgeAction finalAction = makeAddFinalAction(req.uri, source, cu, d, varName);
                if (finalAction != null) actions.add(finalAction);
            }

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

            if ((msg.contains("must override") || (msg.contains("override") && msg.contains("annotation")))
                    && !msg.contains("must implement the inherited")) {
                BridgeAction a = makeAddOverrideAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateMethod
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateConstructor
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedPrivateType) {
                BridgeAction a = makeRemoveUnusedMemberAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            Matcher mEx = Pattern.compile("Unhandled exception type ([\\w.$<>\\[\\]]+)").matcher(msg);
            if (mEx.find()) {
                String exType = mEx.group(1).trim();
                actions.add(makeAddThrowsAction(req.uri, source, cu, d, exType));
                actions.add(makeTryCatchAction(req.uri, source, cu, d, exType));
            }

            Matcher mCast = Pattern.compile("Type mismatch: cannot convert from ([\\w.<>\\[\\],\\s]+) to ([\\w.<>\\[\\],\\s]+)").matcher(msg);
            if (mCast.find()) {
                actions.add(makeCastAction(req.uri, source, cu, d, mCast.group(2).trim()));
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnnecessaryCast) {
                BridgeAction a = makeRemoveUnnecessaryCastAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.SuperfluousSemicolon) {
                actions.add(makeRemoveTextAction("Remove semicolon", req.uri, source,
                        CompilationService.lineColToOffset(source, d.startLine, d.startChar),
                        CompilationService.lineColToOffset(source, d.endLine, d.endChar)));
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.CodeCannotBeReached
                    || msg.contains("Dead code")
                    || msg.contains("Unreachable code")) {
                BridgeAction a = makeRemoveUnreachableCodeAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (msg.contains("Implicit super constructor") && msg.contains("is undefined")) {
                // Try adding super() to an existing constructor first
                BridgeAction a = makeAddExplicitSuperConstructorCallAction(req.uri, source, cu, d);
                if (a != null) {
                    actions.add(a);
                } else {
                    // No explicit constructor - generate one matching the super class constructor
                    actions.addAll(makeAddMatchingConstructorActions(req.uri, source, cu, d, req.files, orDefault(req.sourceLevel), orEmpty(req.classpath)));
                }
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.MissingSerialVersion) {
                BridgeAction a = makeAddSerialVersionUIDAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.NotVisibleMethod
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.NotVisibleField
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.NotVisibleConstructor
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.NotVisibleType) {
                actions.addAll(makeChangeVisibilityActions(req.uri, source, req.files,
                        orDefault(req.sourceLevel), orEmpty(req.classpath), problemId, msg));
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedMethodDeclaredThrownException
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.UnusedConstructorDeclaredThrownException) {
                BridgeAction a = makeRemoveUnusedThrownExceptionAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingParamTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingReturnTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocMissingThrowsTag) {
                BridgeAction single = makeAddMissingJavadocTagAction(req.uri, source, cu, d, problemId);
                if (single != null) actions.add(single);
                BridgeAction all = makeAddAllMissingJavadocTagsAction(req.uri, source, cu, d);
                if (all != null) actions.add(all);
            }

            if (problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocInvalidThrowsClassName
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocUnexpectedTag
                    || problemId == org.eclipse.jdt.core.compiler.IProblem.JavadocInvalidParamName) {
                BridgeAction a = makeRemoveInvalidJavadocTagAction(req.uri, source, cu, d);
                if (a != null) actions.add(a);
            }
        }

        // "Add all missing imports" — unambiguous imports for every unresolved type in the file.
        if (!allFileUnresolvedTypes.isEmpty()) {
            List<BridgeTextEdit> allImportEdits = new ArrayList<>();
            // Re-parse so insert offsets are computed against an unmodified source.
            org.eclipse.jdt.core.dom.CompilationUnit freshCu = parseForFixes(source, orDefault(req.sourceLevel), orEmpty(req.classpath));
            Set<String> addedFqns = new LinkedHashSet<>();
            for (String typeName : allFileUnresolvedTypes) {
                List<String> found = CompletionService.searchBySimpleName(typeName);
                if (found.size() == 1 && addedFqns.add(found.get(0))) {
                    int off = findSortedImportInsertPoint(source, freshCu, found.get(0));
                    allImportEdits.add(insertionEdit(source, off, "import " + found.get(0) + ";\n"));
                }
            }
            if (!allImportEdits.isEmpty()) {
                BridgeAction addAll = new BridgeAction();
                addAll.title = "Add all missing imports";
                addAll.kind = "quickfix";
                BridgeFileEdit fe = new BridgeFileEdit();
                fe.uri = req.uri;
                fe.edits = allImportEdits;
                addAll.edits = List.of(fe);
                actions.add(addAll);
            }
        }

        // "Remove all unused imports" — scan every file diagnostic, collect unused import lines.
        {
            List<Integer> unusedLines = new ArrayList<>();
            for (BridgeDiagnostic d : diags) {
                String du = d.uri.replace("file:///", "file:/").replace("\\", "/");
                if (!targetUriNorm.equalsIgnoreCase(du) && !targetUriNorm.endsWith(du) && !du.endsWith(targetUriNorm)) continue;
                String m2 = d.message != null ? d.message : "";
                if (m2.contains("import") && (m2.contains("never used") || m2.contains("not used"))) {
                    unusedLines.add(d.startLine);
                }
            }
            if (unusedLines.size() > 1) {
                // Bulk action only when there are 2+ unused imports; single-import is handled
                // per-diagnostic as "Remove unused import".
                unusedLines.sort(Collections.reverseOrder()); // reverse so offsets don't shift
                BridgeAction removeAll = new BridgeAction();
                removeAll.title = "Remove all unused imports";
                removeAll.kind = "source";
                BridgeFileEdit fe = new BridgeFileEdit();
                fe.uri = req.uri;
                List<BridgeTextEdit> edits = new ArrayList<>();
                for (int line : unusedLines) edits.add(removalEditFromLine(source, line));
                fe.edits = edits;
                removeAll.edits = List.of(fe);
                actions.add(removeAll);
            }
        }

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

        // Generate Getter / Setter — available when cursor is on a field declaration.
        actions.addAll(makeGetterSetterActions(req.uri, source, cu, req.range));

        // Generate toString() — offered as quickassist (lightbulb) and source.generate.toString (source menu).
        BridgeAction toStringAction = makeToStringAction(req.uri, source, cu, req.range);
        if (toStringAction != null) {
            actions.add(toStringAction);
            // Also add source.generate.toString variant (same edit, different kind).
            BridgeAction toStringSource = new BridgeAction();
            toStringSource.title = "Generate toString()";
            toStringSource.kind  = "source.generate.toString";
            toStringSource.edits = toStringAction.edits;
            actions.add(toStringSource);
        }

        // "Change modifiers to final where possible" — whole-file source action.
        BridgeAction finalModifiers = makeFinalModifiersAction(req.uri, source, cu);
        if (finalModifiers != null) actions.add(finalModifiers);

        if (!hasOverlappingDiagnostic) {
            BridgeAction javadocAction = makeAddJavadocAction(req.uri, source, cu, req.range);
            if (javadocAction != null) actions.add(javadocAction);
        }

        // Quickassists — position-based, no diagnostic required.
        if (req.range != null) {
            BridgeAction extractVar = makeExtractLocalVariableAction(req.uri, source, cu, req.range);
            if (extractVar != null) actions.add(extractVar);

            BridgeAction inlineVar = makeInlineLocalVariableAction(req.uri, source, cu, req.range);
            if (inlineVar != null) actions.add(inlineVar);

            BridgeAction extractMethod = makeExtractMethodAction(req.uri, source, cu, req.range);
            if (extractMethod != null) actions.add(extractMethod);
        }

        return dedupeActions(actions);
    }

    private static int problemId(BridgeDiagnostic d) {
        try { return d.code != null ? Integer.parseInt(d.code) : 0; }
        catch (Exception ignored) { return 0; }
    }

    private static String suppressToken(BridgeDiagnostic d, String msg) {
        return switch (d.categoryId) {
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNNECESSARY_CODE -> "unused";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_DEPRECATION     -> "deprecation";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_NLS             -> "nls";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_RESTRICTION     -> "restriction";
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_UNCHECKED_RAW   -> {
                int id = 0;
                try { id = Integer.parseInt(d.code); } catch (Exception ignored) {}
                yield id == org.eclipse.jdt.core.compiler.IProblem.RawTypeReference ? "rawtypes" : "unchecked";
            }
            case org.eclipse.jdt.core.compiler.CategorizedProblem.CAT_POTENTIAL_PROGRAMMING_PROBLEM -> msg.contains("null") ? "null" : null;
            default -> null;
        };
    }

    private static org.eclipse.jdt.core.dom.CompilationUnit parseForFixes(String source, String sourceLevel, List<String> classpath) {
        org.eclipse.jdt.core.dom.ASTParser parser =
            org.eclipse.jdt.core.dom.ASTParser.newParser(org.eclipse.jdt.core.dom.AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(org.eclipse.jdt.core.dom.ASTParser.K_COMPILATION_UNIT);
        parser.setStatementsRecovery(true);
        parser.setResolveBindings(true);
        parser.setUnitName("Main.java");
        
        // Provide environment for binding resolution
        String[] cp = classpath.toArray(new String[0]);
        parser.setEnvironment(cp, new String[]{}, null, true);

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

    private static BridgeAction makeReplaceNodeAction(String title, String uri, String source, org.eclipse.jdt.core.dom.ASTNode node, String newText) {
        BridgeAction a = new BridgeAction();
        a.title = title; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        BridgeTextEdit te = new BridgeTextEdit();
        int start = node.getStartPosition();
        int end = start + node.getLength();
        int[] startLC = CompilationService.offsetToLineCol(source, start);
        int[] endLC = CompilationService.offsetToLineCol(source, end);
        te.startLine = startLC[0]; te.startChar = startLC[1];
        te.endLine = endLC[0]; te.endChar = endLC[1];
        te.newText = newText;
        fe.edits = List.of(te);
        a.edits = List.of(fe);
        return a;
    }

    private static BridgeAction makeChangePackageAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, String newPackage) {
        org.eclipse.jdt.core.dom.PackageDeclaration pkg = cu.getPackage();
        if (pkg == null) {
            BridgeAction a = new BridgeAction();
            a.title = "Add package declaration '" + newPackage + "'";
            a.kind = "quickfix";
            BridgeFileEdit fe = new BridgeFileEdit();
            fe.uri = uri;
            BridgeTextEdit te = new BridgeTextEdit();
            te.startLine = 0; te.startChar = 0;
            te.endLine = 0; te.endChar = 0;
            te.newText = "package " + newPackage + ";\n\n";
            fe.edits = List.of(te);
            a.edits = List.of(fe);
            return a;
        } else {
            return makeReplaceNodeAction("Change package to '" + newPackage + "'", uri, source, pkg.getName(), newPackage);
        }
    }

    private static BridgeAction makeChangeReturnTypeAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String newType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.Type returnType = loc.found.getReturnType2();
        if (returnType == null) return null;
        return makeReplaceNodeAction("Change return type to '" + newType + "'", uri, source, returnType, newType);
    }

    private static BridgeAction makeAddReturnStatementAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset);
        cu.accept(loc);
        if (loc.found == null || loc.found.getBody() == null) return null;
        org.eclipse.jdt.core.dom.MethodDeclaration method = loc.found;
        org.eclipse.jdt.core.dom.Block body = method.getBody();
        int insertOffset = body.getStartPosition() + body.getLength() - 1;
        while (insertOffset > 0 && source.charAt(insertOffset) != '}') insertOffset--;
        int line = cu.getLineNumber(insertOffset) - 1;
        String indent = indentOf(source, line);
        String returnVal = "null";
        org.eclipse.jdt.core.dom.Type rt = method.getReturnType2();
        if (rt != null && rt.isPrimitiveType()) {
            String pt = rt.toString();
            if ("boolean".equals(pt)) returnVal = "false";
            else if ("void".equals(pt)) return null;
            else returnVal = "0";
        }
        BridgeAction a = new BridgeAction();
        a.title = "Add return statement"; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        BridgeTextEdit te = new BridgeTextEdit();
        te.startLine = line; te.startChar = cu.getColumnNumber(insertOffset);
        te.endLine = line; te.endChar = te.startChar;
        te.newText = "\n" + indent + "    return " + returnVal + ";";
        fe.edits = List.of(te);
        a.edits = List.of(fe);
        return a;
    }

    private static BridgeAction makeAddQuoteAction(String uri, String source, BridgeDiagnostic d) {
        int start = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        int pos = start;
        while (pos < source.length() && source.charAt(pos) != '\n' && source.charAt(pos) != '\r') pos++;
        BridgeAction a = new BridgeAction();
        a.title = "Add missing quote"; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        BridgeTextEdit te = new BridgeTextEdit();
        int[] lc = CompilationService.offsetToLineCol(source, pos);
        te.startLine = lc[0]; te.startChar = lc[1];
        te.endLine = lc[0]; te.endChar = lc[1];
        te.newText = "\"";
        fe.edits = List.of(te);
        a.edits = List.of(fe);
        return a;
    }

    private static BridgeAction makeCorrectStaticAccessAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String typeName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ExpressionLocator loc = new ExpressionLocator(offset);
        cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.Expression expr = loc.found;
        if (expr instanceof org.eclipse.jdt.core.dom.MethodInvocation mi) {
            if (mi.getExpression() != null) return makeReplaceNodeAction("Access static method via '" + typeName + "'", uri, source, mi.getExpression(), typeName);
        } else if (expr instanceof org.eclipse.jdt.core.dom.FieldAccess fa) {
            return makeReplaceNodeAction("Access static field via '" + typeName + "'", uri, source, fa.getExpression(), typeName);
        } else if (expr instanceof org.eclipse.jdt.core.dom.QualifiedName qn) {
            return makeReplaceNodeAction("Access static field via '" + typeName + "'", uri, source, qn.getQualifier(), typeName);
        }
        return null;
    }

    private static BridgeAction makeAddJavadocAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        if (range == null) return null;
        JavadocTargetLocator locator = new JavadocTargetLocator(cu, range.startLine, range.endLine);
        cu.accept(locator);
        if (locator.best == null || locator.best.getJavadoc() != null) return null;
        org.eclipse.jdt.core.dom.BodyDeclaration target = locator.best;
        int line = cu.getLineNumber(target.getStartPosition()) - 1;
        String indent = indentOf(source, line);
        int insertOffset = lineStart(source, line);
        BridgeAction action = new BridgeAction();
        action.title = "Add Javadoc comment"; action.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        BridgeTextEdit edit = insertionEdit(source, insertOffset, buildJavadocStub(target, indent));
        fe.edits = List.of(edit);
        action.edits = List.of(fe);
        return action;
    }

    private static String buildJavadocStub(org.eclipse.jdt.core.dom.BodyDeclaration target, String indent) {
        StringBuilder out = new StringBuilder();
        out.append(indent).append("/**\n").append(indent).append(" * \n");
        if (target instanceof org.eclipse.jdt.core.dom.MethodDeclaration method) {
            for (Object param : method.parameters()) {
                out.append(indent).append(" * @param ").append(((org.eclipse.jdt.core.dom.SingleVariableDeclaration)param).getName().getIdentifier()).append("\n");
            }
            if (!method.isConstructor() && method.getReturnType2() != null && !"void".equals(method.getReturnType2().toString())) {
                out.append(indent).append(" * @return\n");
            }
            for (Object ex : method.thrownExceptionTypes()) {
                out.append(indent).append(" * @throws ").append(ex.toString()).append("\n");
            }
        }
        out.append(indent).append(" */\n");
        return out.toString();
    }

    private static BridgeAction makeSuppressAction(String tag, String uri, String source, BridgeDiagnostic d, org.eclipse.jdt.core.dom.CompilationUnit cu) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        AnnotatableLocator loc = new AnnotatableLocator(offset);
        cu.accept(loc);
        int insertLine = (loc.bestNode != null) ? CompilationService.offsetToLineCol(source, loc.bestNode.getStartPosition())[0] : d.startLine;
        String indent = indentOf(source, insertLine);
        return singleInsertionAction("@SuppressWarnings(\"" + tag + "\")", uri, source, lineStart(source, insertLine), indent + "@SuppressWarnings(\"" + tag + "\")\n");
    }

    private static BridgeAction makeImportAction(String title, String uri, String source, String fqn, boolean preferred, org.eclipse.jdt.core.dom.CompilationUnit cu) {
        int insertOffset = findSortedImportInsertPoint(source, cu, fqn);
        BridgeAction action = singleInsertionAction(title, uri, source, insertOffset, "import " + fqn + ";\n");
        action.isPreferred = preferred;
        return action;
    }

    private static int findSortedImportInsertPoint(String source, org.eclipse.jdt.core.dom.CompilationUnit cu, String fqn) {
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.ImportDeclaration> imports = cu.imports();
        if (imports.isEmpty()) return findImportInsertPoint(source);
        for (var imp : imports) {
            if (imp.isStatic()) continue;
            if (fqn.compareTo(imp.getName().getFullyQualifiedName()) < 0) return lineStart(source, cu.getLineNumber(imp.getStartPosition()) - 1);
        }
        var last = imports.get(imports.size() - 1);
        int end = last.getStartPosition() + last.getLength();
        if (end < source.length() && source.charAt(end) == '\n') end++;
        return end;
    }

    private static BridgeAction makeRemoveLineAction(String title, String uri, String source, int line) {
        int start = lineStart(source, line);
        int end = lineStart(source, line + 1);
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "quickfix"; a.isPreferred = true;
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(removalEditFromOffsets(source, start, end));
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeRemoveDeclarationAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int start = loc.found.getStartPosition(), end = start + loc.found.getLength();
        if (end < source.length() && source.charAt(end) == '\n') end++;
        BridgeAction a = new BridgeAction(); a.title = "Remove unused variable"; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(removalEditFromOffsets(source, start, end));
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeAddOverrideAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int methodStart = loc.found.getStartPosition();
        String indent = indentOf(source, cu.getLineNumber(methodStart) - 1);
        return singleInsertionAction("Add @Override annotation", uri, source, lineStart(source, cu.getLineNumber(methodStart) - 1), indent + "@Override\n");
    }

    private static BridgeAction makeCastAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String targetType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ExpressionLocator loc = new ExpressionLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int start = loc.found.getStartPosition(), end = start + loc.found.getLength();
        return makeReplaceNodeAction("Cast to '" + targetType + "'", uri, source, loc.found, "(" + targetType + ") " + source.substring(start, end));
    }

    private static BridgeAction makeRemoveUnnecessaryCastAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, offset, 1);
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.CastExpression)) node = node.getParent();
        if (node == null) return null;
        org.eclipse.jdt.core.dom.CastExpression cast = (org.eclipse.jdt.core.dom.CastExpression)node;
        return makeReplaceNodeAction("Remove unnecessary cast", uri, source, cast, source.substring(cast.getExpression().getStartPosition(), cast.getExpression().getStartPosition() + cast.getExpression().getLength()));
    }

    private static BridgeAction makeAddThrowsAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String exType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null || loc.found.getBody() == null) return null;
        String simpleName = exType.contains(".") ? exType.substring(exType.lastIndexOf('.') + 1) : exType;
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrown = loc.found.thrownExceptionTypes();
        int insertPos = thrown.isEmpty() ? loc.found.getBody().getStartPosition() : (thrown.get(thrown.size() - 1).getStartPosition() + thrown.get(thrown.size() - 1).getLength());
        return singleInsertionAction("Add throws declaration for '" + simpleName + "'", uri, source, insertPos, (thrown.isEmpty() ? "throws " : ", ") + simpleName + (thrown.isEmpty() ? " " : ""));
    }

    private static BridgeAction makeTryCatchAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String exType) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.Statement stmt = loc.found;
        int start = stmt.getStartPosition(), end = start + stmt.getLength();
        if (end < source.length() && source.charAt(end) == '\n') end++;
        String indent = indentOf(source, cu.getLineNumber(start) - 1), simpleName = simpleTypeName(exType);
        String newText = indent + "try {\n    " + indent + source.substring(start, start + stmt.getLength()).replace("\n", "\n    ") + "\n" + indent + "} catch (" + simpleName + " e) {\n    " + indent + "e.printStackTrace();\n" + indent + "}\n";
        return makeReplaceNodeAction("Surround with try/catch", uri, source, stmt, newText);
    }

    private static BridgeAction makeRemoveAllAssignmentsAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String varName) {
        if (varName == null) return null;
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator vloc = new VarDeclLocator(offset); cu.accept(vloc);
        if (vloc.found == null) return null;
        AllAssignmentsLocator aloc = new AllAssignmentsLocator(varName); cu.accept(aloc);
        List<BridgeTextEdit> edits = new ArrayList<>();
        for (var es : aloc.pureAssignments) {
            if (cu.getLineNumber(es.getStartPosition()) == cu.getLineNumber(vloc.found.getStartPosition())) continue;
            edits.add(removalEditFromLine(source, cu.getLineNumber(es.getStartPosition()) - 1));
        }
        edits.add(removalEditFromLine(source, cu.getLineNumber(vloc.found.getStartPosition()) - 1));
        edits.sort((a,b) -> Integer.compare(b.startLine, a.startLine));
        BridgeAction act = new BridgeAction(); act.title = "Remove '" + varName + "' and all assignments"; act.kind = "quickfix"; act.isPreferred = true;
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits; act.edits = List.of(fe); return act;
    }

    private static BridgeAction makeLocalVarSuppressAction(String tag, String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String varName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int line = cu.getLineNumber(loc.found.getStartPosition()) - 1;
        return singleInsertionAction("Add @SuppressWarnings(\"" + tag + "\") to '" + (varName != null ? varName : "variable") + "'", uri, source, lineStart(source, line), indentOf(source, line) + "@SuppressWarnings(\"" + tag + "\")\n");
    }

    private static BridgeAction makeChangeSignatureAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String paramName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ParameterLocator loc = new ParameterLocator(offset, paramName); cu.accept(loc);
        if (loc.found == null) return null;
        BridgeAction a = new BridgeAction(); a.title = "Remove parameter '" + paramName + "'"; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri;
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.SingleVariableDeclaration> params = ((org.eclipse.jdt.core.dom.MethodDeclaration)loc.found.getParent()).parameters();
        fe.edits = List.of(deletionEditForNodes(source, castAstNodes(params), params.indexOf(loc.found)));
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeAssignParameterToFieldAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String paramName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ParameterLocator loc = new ParameterLocator(offset, paramName); cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.MethodDeclaration method = (org.eclipse.jdt.core.dom.MethodDeclaration)loc.found.getParent();
        org.eclipse.jdt.core.dom.TypeDeclaration type = (org.eclipse.jdt.core.dom.TypeDeclaration)method.getParent();
        String fieldName = chooseFieldName(type, paramName);
        BridgeAction a = new BridgeAction(); a.title = "Assign parameter to new field '" + fieldName + "'"; a.kind = "quickfix";
        List<BridgeTextEdit> edits = new ArrayList<>();
        edits.add(insertionEdit(source, bodyInsertOffset(type, source), indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1) + "private " + loc.found.getType().toString() + " " + fieldName + ";\n"));
        edits.add(insertionEdit(source, assignmentInsertOffset(method, source), indentOf(source, cu.getLineNumber(method.getBody().getStartPosition())) + "this." + fieldName + " = " + paramName + ";\n"));
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits; a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeDocumentParameterAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String paramName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ParameterLocator loc = new ParameterLocator(offset, paramName); cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.MethodDeclaration method = (org.eclipse.jdt.core.dom.MethodDeclaration)loc.found.getParent();
        if (method.getJavadoc() == null) return singleInsertionAction("Add Javadoc with @param " + paramName, uri, source, lineStart(source, cu.getLineNumber(method.getStartPosition()) - 1), buildParameterDocumentationStub(method, paramName, indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1)));
        return singleInsertionAction("Add @param " + paramName + " to Javadoc", uri, source, javadocClosingOffset(method.getJavadoc()), " * @param " + paramName + "\n" + indentOf(source, cu.getLineNumber(method.getStartPosition()) - 1));
    }

    private static BridgeAction makeAddFinalParameterAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String paramName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ParameterLocator loc = new ParameterLocator(offset, paramName); cu.accept(loc);
        if (loc.found == null) return null;
        if ((loc.found.getModifiers() & org.eclipse.jdt.core.dom.Modifier.FINAL) != 0) return null;
        return singleInsertionAction("Make parameter '" + paramName + "' final", uri, source, loc.found.getStartPosition(), "final ");
    }

    private static BridgeAction makeRemoveUnusedMemberAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        AnnotatableLocator loc = new AnnotatableLocator(offset); cu.accept(loc);
        if (loc.bestNode == null) return null;
        int s = loc.bestNode.getStartPosition(), e = s + loc.bestNode.getLength();
        if (e < source.length() && source.charAt(e) == '\n') e++;
        BridgeAction a = new BridgeAction(); a.title = "Remove unused member"; a.kind = "quickfix"; a.isPreferred = true;
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(removalEditFromOffsets(source, s, e));
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeRemoveTextAction(String title, String uri, String source, int start, int end) {
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(removalEditFromOffsets(source, start, end));
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeRemoveUnreachableCodeAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int stmtStart = loc.found.getStartPosition();
        int e = stmtStart + loc.found.getLength();
        if (e < source.length() && source.charAt(e) == '\n') e++;
        // Start from the beginning of the line so the leading whitespace is also
        // removed; without this the orphaned indent merges with the next line.
        int lineNum = cu.getLineNumber(stmtStart) - 1; // 0-based
        int s = lineStart(source, lineNum);
        return makeRemoveTextAction("Remove unreachable code", uri, source, s, e);
    }

    private static List<BridgeAction> makeAddMatchingConstructorActions(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, Map<String, String> files, String sourceLevel, List<String> classpath) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        TypeDeclLocator tdl = new TypeDeclLocator(offset);
        cu.accept(tdl);
        if (tdl.found == null || tdl.found.getSuperclassType() == null) return List.of();

        String className = tdl.found.getName().getIdentifier();
        String superName = tdl.found.getSuperclassType().toString();
        int lt = superName.indexOf('<');
        if (lt >= 0) superName = superName.substring(0, lt);
        final String superSimple = superName;

        // Collect non-default (non-zero-arg) constructors from superclass.
        // Check the current compilation unit first (superclass in same file).
        List<org.eclipse.jdt.core.dom.MethodDeclaration> superCtors = new ArrayList<>();
        for (Object t : cu.types()) {
            if (t instanceof org.eclipse.jdt.core.dom.TypeDeclaration td && td.getName().getIdentifier().equals(superSimple)) {
                for (org.eclipse.jdt.core.dom.MethodDeclaration m : td.getMethods()) {
                    if (m.isConstructor() && !m.parameters().isEmpty()) superCtors.add(m);
                }
            }
        }

        // If not in the same file, search other open files
        if (superCtors.isEmpty() && files != null) {
            for (Map.Entry<String, String> entry : files.entrySet()) {
                String key = entry.getKey().replace('\\', '/');
                if (key.endsWith("/" + superSimple + ".java") || key.equals(superSimple + ".java")) {
                    org.eclipse.jdt.core.dom.CompilationUnit superCu = parseForFixes(entry.getValue(), sourceLevel, classpath);
                    for (Object t : superCu.types()) {
                        if (t instanceof org.eclipse.jdt.core.dom.TypeDeclaration td && td.getName().getIdentifier().equals(superSimple)) {
                            for (org.eclipse.jdt.core.dom.MethodDeclaration m : td.getMethods()) {
                                if (m.isConstructor() && !m.parameters().isEmpty()) superCtors.add(m);
                            }
                        }
                    }
                    break;
                }
            }
        }
        if (superCtors.isEmpty()) return List.of();

        // Insert after the opening '{' of the class body
        int braceOffset = classBodyOpenBrace(tdl.found, source);
        if (braceOffset < 0) return List.of();
        int insertOffset = braceOffset + 1;

        String baseIndent = indentOf(source, cu.getLineNumber(tdl.found.getStartPosition()) - 1);
        String memberIndent = baseIndent + "    ";

        List<BridgeAction> result = new ArrayList<>();
        for (org.eclipse.jdt.core.dom.MethodDeclaration superCtor : superCtors) {
            List<String> types = new ArrayList<>();
            List<String> names = new ArrayList<>();
            for (Object p : superCtor.parameters()) {
                org.eclipse.jdt.core.dom.SingleVariableDeclaration svd = (org.eclipse.jdt.core.dom.SingleVariableDeclaration) p;
                types.add(svd.getType().toString());
                names.add(svd.getName().getIdentifier());
            }

            StringBuilder sb = new StringBuilder();
            sb.append("\n").append(memberIndent).append("public ").append(className).append("(");
            for (int i = 0; i < types.size(); i++) {
                if (i > 0) sb.append(", ");
                sb.append(types.get(i)).append(" ").append(names.get(i));
            }
            sb.append(") {\n").append(memberIndent).append("    super(");
            for (int i = 0; i < names.size(); i++) {
                if (i > 0) sb.append(", ");
                sb.append(names.get(i));
            }
            sb.append(");\n").append(memberIndent).append("}\n");

            String title = "Add constructor '" + className + "(" + String.join(", ", types) + ")'";
            result.add(singleInsertionAction(title, uri, source, insertOffset, sb.toString()));
        }
        return result;
    }

    private static int classBodyOpenBrace(org.eclipse.jdt.core.dom.TypeDeclaration td, String source) {
        // Scan forward from the end of the class header (after superclass type or implements list, or class name)
        int start;
        List<?> ifaces = td.superInterfaceTypes();
        if (!ifaces.isEmpty()) {
            org.eclipse.jdt.core.dom.ASTNode last = (org.eclipse.jdt.core.dom.ASTNode) ifaces.get(ifaces.size() - 1);
            start = last.getStartPosition() + last.getLength();
        } else if (td.getSuperclassType() != null) {
            org.eclipse.jdt.core.dom.Type sc = td.getSuperclassType();
            start = sc.getStartPosition() + sc.getLength();
        } else {
            start = td.getName().getStartPosition() + td.getName().getLength();
        }
        for (int i = start; i < source.length(); i++) {
            if (source.charAt(i) == '{') return i;
        }
        return -1;
    }

    private static BridgeAction makeAddExplicitSuperConstructorCallAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null || !loc.found.isConstructor()) return null;
        String invocation = indentOf(source, cu.getLineNumber(loc.found.getBody().getStartPosition())) + "super();\n";
        return singleInsertionAction("Add explicit super() call", uri, source, assignmentInsertOffset(loc.found, source), invocation);
    }

    // ── serialVersionUID ─────────────────────────────────────────────────────

    private static BridgeAction makeAddSerialVersionUIDAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        TypeDeclLocator loc = new TypeDeclLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        int insertOffset = bodyInsertOffset(loc.found, source);
        if (insertOffset < 0) return null;
        String indent = indentOf(source, cu.getLineNumber(loc.found.getStartPosition()) - 1) + "    ";
        BridgeAction a = singleInsertionAction("Add serialVersionUID field", uri, source, insertOffset,
                indent + "private static final long serialVersionUID = 1L;\n");
        a.isPreferred = true;
        return a;
    }

    // ── Visibility change ────────────────────────────────────────────────────

    private static List<BridgeAction> makeChangeVisibilityActions(String uri, String source,
            Map<String, String> files, String sourceLevel, List<String> classpath,
            int problemId, String msg) {
        // Parse declaring type and member name from the ECJ error message.
        String declaringType = null;
        String memberName    = null;
        boolean isCtor       = false;

        Matcher mMethod = Pattern.compile("The method ([\\w$]+)(?:\\(.*?\\))? from the type ([\\w$]+) is not visible").matcher(msg);
        Matcher mField  = Pattern.compile("The field ([\\w$]+)\\.([\\w$]+) is not visible").matcher(msg);
        Matcher mCtor   = Pattern.compile("The constructor ([\\w$]+)\\(.*?\\) is not visible").matcher(msg);
        Matcher mType   = Pattern.compile("The type ([\\w$]+) is not visible").matcher(msg);

        if (mMethod.find()) { memberName = mMethod.group(1); declaringType = mMethod.group(2); }
        else if (mField.find()) { declaringType = mField.group(1); memberName = mField.group(2); }
        else if (mCtor.find()) { declaringType = mCtor.group(1); isCtor = true; }
        else if (mType.find()) { declaringType = mType.group(1); }
        if (declaringType == null) return List.of();

        final String typeName  = declaringType;
        final String mName     = memberName;
        final boolean isCtor2  = isCtor;
        List<BridgeAction> results = new ArrayList<>();

        for (Map.Entry<String, String> entry : files.entrySet()) {
            String fileSource = entry.getValue();
            org.eclipse.jdt.core.dom.CompilationUnit fileCu = parseForFixes(fileSource, sourceLevel, classpath);
            for (Object o : fileCu.types()) {
                if (!(o instanceof org.eclipse.jdt.core.dom.TypeDeclaration td)) continue;
                if (!td.getName().getIdentifier().equals(typeName)) continue;
                if (mName == null && !isCtor2) {
                    // Type itself is not visible
                    BridgeAction a = changeMemberVisibility(entry.getKey(), fileSource, td, "type", typeName);
                    if (a != null) results.add(a);
                    continue;
                }
                for (Object bd : td.bodyDeclarations()) {
                    if (isCtor2 && bd instanceof org.eclipse.jdt.core.dom.MethodDeclaration md && md.isConstructor()) {
                        BridgeAction a = changeMemberVisibility(entry.getKey(), fileSource, md, "constructor", typeName);
                        if (a != null) results.add(a);
                    } else if (!isCtor2 && mName != null && bd instanceof org.eclipse.jdt.core.dom.MethodDeclaration md
                            && md.getName().getIdentifier().equals(mName)) {
                        BridgeAction a = changeMemberVisibility(entry.getKey(), fileSource, md, "method", mName);
                        if (a != null) results.add(a);
                    } else if (!isCtor2 && mName != null && bd instanceof org.eclipse.jdt.core.dom.FieldDeclaration fd) {
                        for (Object frag : fd.fragments()) {
                            if (((org.eclipse.jdt.core.dom.VariableDeclarationFragment) frag).getName().getIdentifier().equals(mName)) {
                                BridgeAction a = changeMemberVisibility(entry.getKey(), fileSource, fd, "field", mName);
                                if (a != null) results.add(a);
                            }
                        }
                    }
                }
            }
        }
        return results;
    }

    private static BridgeAction changeMemberVisibility(String uri, String source,
            org.eclipse.jdt.core.dom.BodyDeclaration node, String kind, String name) {
        int flags = node.getModifiers();
        if ((flags & org.eclipse.jdt.core.dom.Modifier.PUBLIC) != 0) return null; // already public
        boolean isPrivate   = (flags & org.eclipse.jdt.core.dom.Modifier.PRIVATE)   != 0;
        boolean isProtected = (flags & org.eclipse.jdt.core.dom.Modifier.PROTECTED) != 0;
        int start = node.getStartPosition();
        List<BridgeTextEdit> edits = new ArrayList<>();
        if (isPrivate || isProtected) {
            String kw = isPrivate ? "private" : "protected";
            int idx = source.indexOf(kw, start);
            if (idx >= 0 && idx < start + 30) {
                int[] sl = CompilationService.offsetToLineCol(source, idx);
                int[] el = CompilationService.offsetToLineCol(source, idx + kw.length());
                BridgeTextEdit e = new BridgeTextEdit();
                e.startLine = sl[0]; e.startChar = sl[1]; e.endLine = el[0]; e.endChar = el[1];
                e.newText = "public";
                edits.add(e);
            }
        } else {
            // Package-private: prepend "public "
            edits.add(insertionEdit(source, start, "public "));
        }
        if (edits.isEmpty()) return null;
        BridgeAction a = new BridgeAction();
        a.title = "Change visibility of '" + name + "' to public";
        a.kind  = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits;
        a.edits = List.of(fe);
        return a;
    }

    // ── Extract local variable ────────────────────────────────────────────────

    private static BridgeAction makeExtractLocalVariableAction(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        // Only offer when there is a genuine selection (not a cursor-only click).
        if (range.startLine == range.endLine && range.startChar == range.endChar) return null;
        int startOff = CompilationService.lineColToOffset(source, range.startLine, range.startChar);
        int endOff   = CompilationService.lineColToOffset(source, range.endLine,   range.endChar);
        if (startOff >= endOff) return null;

        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, startOff, endOff - startOff);
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.Expression)) node = node.getParent();
        if (!(node instanceof org.eclipse.jdt.core.dom.Expression expr)) return null;
        // Skip trivial / already-named things
        if (expr instanceof org.eclipse.jdt.core.dom.SimpleName
                || expr instanceof org.eclipse.jdt.core.dom.QualifiedName
                || expr instanceof org.eclipse.jdt.core.dom.NumberLiteral
                || expr instanceof org.eclipse.jdt.core.dom.BooleanLiteral
                || expr instanceof org.eclipse.jdt.core.dom.NullLiteral) return null;
        // Skip if already the RHS of a declaration
        if (expr.getParent() instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment) return null;

        org.eclipse.jdt.core.dom.ASTNode stmt = expr;
        while (stmt != null && !(stmt instanceof org.eclipse.jdt.core.dom.Statement)) stmt = stmt.getParent();
        if (stmt == null) return null;

        String exprText = source.substring(expr.getStartPosition(), expr.getStartPosition() + expr.getLength());
        String varName  = suggestVariableName(expr, exprText);
        int    lineNum  = cu.getLineNumber(stmt.getStartPosition()) - 1;
        String indent   = indentOf(source, lineNum);

        // Edit order matters: replace expression first (higher offset), then insert declaration.
        int[] sl = CompilationService.offsetToLineCol(source, expr.getStartPosition());
        int[] el = CompilationService.offsetToLineCol(source, expr.getStartPosition() + expr.getLength());
        BridgeTextEdit replEdit = new BridgeTextEdit();
        replEdit.startLine = sl[0]; replEdit.startChar = sl[1];
        replEdit.endLine   = el[0]; replEdit.endChar   = el[1];
        replEdit.newText   = varName;

        BridgeTextEdit declEdit = insertionEdit(source, lineStart(source, lineNum),
                indent + "var " + varName + " = " + exprText + ";\n");

        // Apply higher-line edit first so offsets stay valid
        List<BridgeTextEdit> edits = new ArrayList<>(List.of(replEdit, declEdit));
        edits.sort((a, b) -> {
            int c = Integer.compare(b.startLine, a.startLine);
            return c != 0 ? c : Integer.compare(b.startChar, a.startChar);
        });

        BridgeAction a = new BridgeAction();
        a.title = "Extract to local variable";
        a.kind  = "refactor.extract.variable";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits;
        a.edits = List.of(fe);
        return a;
    }

    private static String suggestVariableName(org.eclipse.jdt.core.dom.Expression expr, String text) {
        if (expr instanceof org.eclipse.jdt.core.dom.MethodInvocation mi) {
            String n = mi.getName().getIdentifier();
            if (n.startsWith("get") && n.length() > 3)
                return Character.toLowerCase(n.charAt(3)) + n.substring(4);
            return n;
        }
        if (expr instanceof org.eclipse.jdt.core.dom.ClassInstanceCreation cic) {
            String n = cic.getType().toString();
            int lt = n.indexOf('<'); if (lt > 0) n = n.substring(0, lt);
            return Character.toLowerCase(n.charAt(0)) + (n.length() > 1 ? n.substring(1) : "");
        }
        if (expr instanceof org.eclipse.jdt.core.dom.FieldAccess fa)
            return fa.getName().getIdentifier();
        return "value";
    }

    // ── Inline local variable ────────────────────────────────────────────────

    private static BridgeAction makeInlineLocalVariableAction(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        int offset = CompilationService.lineColToOffset(source, range.startLine, range.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset); cu.accept(loc);
        if (loc.found == null || loc.found.fragments().isEmpty()) return null;
        @SuppressWarnings("unchecked")
        org.eclipse.jdt.core.dom.VariableDeclarationFragment frag =
                (org.eclipse.jdt.core.dom.VariableDeclarationFragment) loc.found.fragments().get(0);
        if (frag.getInitializer() == null) return null;

        String varName  = frag.getName().getIdentifier();
        String initText = source.substring(frag.getInitializer().getStartPosition(),
                frag.getInitializer().getStartPosition() + frag.getInitializer().getLength());

        // Collect all reads of the variable in the enclosing block.
        org.eclipse.jdt.core.dom.ASTNode block = loc.found;
        while (block != null && !(block instanceof org.eclipse.jdt.core.dom.Block)) block = block.getParent();
        if (block == null) return null;

        List<org.eclipse.jdt.core.dom.SimpleName> reads = new ArrayList<>();
        final org.eclipse.jdt.core.dom.SimpleName declName = frag.getName();
        block.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.SimpleName n) {
                if (n.getIdentifier().equals(varName) && n != declName
                        && !(n.getParent() instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment))
                    reads.add(n);
                return true;
            }
        });

        // Don't offer inline when there are no usages — would just delete the declaration.
        if (reads.isEmpty()) return null;

        List<BridgeTextEdit> edits = new ArrayList<>();
        for (org.eclipse.jdt.core.dom.SimpleName read : reads) {
            int[] sl = CompilationService.offsetToLineCol(source, read.getStartPosition());
            int[] el = CompilationService.offsetToLineCol(source, read.getStartPosition() + read.getLength());
            BridgeTextEdit e = new BridgeTextEdit();
            e.startLine = sl[0]; e.startChar = sl[1]; e.endLine = el[0]; e.endChar = el[1];
            e.newText = initText;
            edits.add(e);
        }

        // Remove the declaration line.
        int lineNum   = cu.getLineNumber(loc.found.getStartPosition()) - 1;
        int declStart = lineStart(source, lineNum);
        int declEnd   = loc.found.getStartPosition() + loc.found.getLength();
        if (declEnd < source.length() && source.charAt(declEnd) == ';') declEnd++;
        if (declEnd < source.length() && source.charAt(declEnd) == '\n') declEnd++;
        edits.add(removalEditFromOffsets(source, declStart, declEnd));

        edits.sort((a, b) -> {
            int c = Integer.compare(b.startLine, a.startLine);
            return c != 0 ? c : Integer.compare(b.startChar, a.startChar);
        });

        BridgeAction action = new BridgeAction();
        action.title = "Inline local variable";
        action.kind  = "refactor.inline";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits;
        action.edits = List.of(fe);
        return action;
    }

    // ── Extract method ───────────────────────────────────────────────────────

    private static BridgeAction makeExtractMethodAction(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        if (range.startLine == range.endLine && range.startChar == range.endChar) return null;
        int startOff = CompilationService.lineColToOffset(source, range.startLine, range.startChar);
        int endOff   = CompilationService.lineColToOffset(source, range.endLine,   range.endChar);
        if (startOff >= endOff) return null;

        List<org.eclipse.jdt.core.dom.Statement> stmts = findStatementsInRange(cu, startOff, endOff);
        if (stmts.isEmpty()) return null;

        MethodLocator mLoc = new MethodLocator(startOff); cu.accept(mLoc);
        if (mLoc.found == null || mLoc.found.getBody() == null) return null;
        boolean isStatic = (mLoc.found.getModifiers() & org.eclipse.jdt.core.dom.Modifier.STATIC) != 0;

        // Variables referenced inside selection but declared outside = parameters.
        Set<String> declaredIn   = new LinkedHashSet<>();
        Set<String> referencedIn = new LinkedHashSet<>();
        for (org.eclipse.jdt.core.dom.Statement s : stmts) {
            collectDeclaredNames(s, declaredIn);
            collectReferencedNames(s, referencedIn);
        }
        Set<String> params = new LinkedHashSet<>();
        for (String n : referencedIn) if (!declaredIn.contains(n)) params.add(n);

        int firstStart    = stmts.get(0).getStartPosition();
        int lastEnd       = stmts.get(stmts.size() - 1).getStartPosition()
                          + stmts.get(stmts.size() - 1).getLength();
        // Use the line start (including leading whitespace) so all lines in
        // stmtBody share a consistent baseline indentation that can be stripped.
        int stmtLineStart = lineStart(source, cu.getLineNumber(firstStart) - 1);
        String stmtBody   = source.substring(stmtLineStart, lastEnd);

        String methodIndent = indentOf(source, cu.getLineNumber(mLoc.found.getStartPosition()) - 1);
        String bodyIndent   = methodIndent + "    ";

        StringBuilder paramList = new StringBuilder();
        for (String p : params) { if (paramList.length() > 0) paramList.append(", "); paramList.append("Object ").append(p); }

        // Strip the original indentation from the body text, then re-indent at bodyIndent.
        int minIndent = stmtBody.lines()
                .filter(l -> !l.isBlank())
                .mapToInt(l -> { int i = 0; while (i < l.length() && (l.charAt(i) == ' ' || l.charAt(i) == '\t')) i++; return i; })
                .min().orElse(0);
        String bodyText = stmtBody.lines()
                .map(l -> l.length() >= minIndent ? bodyIndent + l.substring(minIndent) : l)
                .collect(java.util.stream.Collectors.joining("\n"));
        String newMethod = "\n\n" + methodIndent + "private " + (isStatic ? "static " : "") + "void extracted("
                + paramList + ") {\n" + bodyText + "\n" + methodIndent + "}";

        StringBuilder callArgs = new StringBuilder();
        for (String p : params) { if (callArgs.length() > 0) callArgs.append(", "); callArgs.append(p); }
        String callLine = indentOf(source, cu.getLineNumber(firstStart) - 1)
                + "extracted(" + callArgs + ");\n";

        int methodEnd = mLoc.found.getStartPosition() + mLoc.found.getLength();

        int stmtEnd = lastEnd;
        if (stmtEnd < source.length() && source.charAt(stmtEnd) == '\n') stmtEnd++;

        int[] callSL = CompilationService.offsetToLineCol(source, stmtLineStart);
        int[] callEL = CompilationService.offsetToLineCol(source, stmtEnd);
        BridgeTextEdit callEdit = new BridgeTextEdit();
        callEdit.startLine = callSL[0]; callEdit.startChar = callSL[1];
        callEdit.endLine   = callEL[0]; callEdit.endChar   = callEL[1];
        callEdit.newText   = callLine;

        BridgeTextEdit insertEdit = insertionEdit(source, methodEnd, newMethod);

        // Ascending order (callEdit first, insertEdit after) so VS Code's
        // WorkspaceEdit validator doesn't flag the same-line boundary as overlap.
        List<BridgeTextEdit> edits = new ArrayList<>(List.of(callEdit, insertEdit));
        edits.sort((a, b) -> {
            int c = Integer.compare(a.startLine, b.startLine);
            return c != 0 ? c : Integer.compare(a.startChar, b.startChar);
        });

        BridgeAction a = new BridgeAction();
        a.title = "Extract method";
        a.kind  = "refactor.extract.function";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits;
        a.edits = List.of(fe);
        return a;
    }

    private static List<org.eclipse.jdt.core.dom.Statement> findStatementsInRange(
            org.eclipse.jdt.core.dom.CompilationUnit cu, int start, int end) {
        // Find the innermost Block that fully contains [start, end], then return
        // only its direct-child statements within the range.  This prevents
        // descending into nested blocks and collecting statements from multiple
        // scope levels (e.g. the for-statement AND the statement inside it).
        final org.eclipse.jdt.core.dom.Block[] innermost = {null};
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.Block b) {
                int bs = b.getStartPosition(), be = bs + b.getLength();
                if (bs <= start && be >= end) {
                    if (innermost[0] == null || b.getLength() < innermost[0].getLength())
                        innermost[0] = b;
                }
                return true;
            }
        });
        if (innermost[0] == null) return List.of();
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Statement> stmts = innermost[0].statements();
        List<org.eclipse.jdt.core.dom.Statement> result = new ArrayList<>();
        for (org.eclipse.jdt.core.dom.Statement s : stmts) {
            int ss = s.getStartPosition(), se = ss + s.getLength();
            if (ss >= start && se <= end) result.add(s);
        }
        return result;
    }

    private static void collectDeclaredNames(org.eclipse.jdt.core.dom.ASTNode node, Set<String> out) {
        node.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.VariableDeclarationFragment n) { out.add(n.getName().getIdentifier()); return true; }
            public boolean visit(org.eclipse.jdt.core.dom.SingleVariableDeclaration n)  { out.add(n.getName().getIdentifier()); return true; }
        });
    }

    private static void collectReferencedNames(org.eclipse.jdt.core.dom.ASTNode node, Set<String> out) {
        node.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.SimpleName n) {
                org.eclipse.jdt.core.dom.ASTNode p = n.getParent();
                // Skip variable declaration positions.
                if (p instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment vdf && vdf.getName() == n) return true;
                if (p instanceof org.eclipse.jdt.core.dom.SingleVariableDeclaration svd && svd.getName() == n) return true;
                // Skip method names in invocations (println in System.out.println(...)).
                if (p instanceof org.eclipse.jdt.core.dom.MethodInvocation mi && mi.getName() == n) return true;
                // Skip type references (String in "for (String item : ...)").
                if (p instanceof org.eclipse.jdt.core.dom.SimpleType) return true;
                // Skip qualified name components (System.out, java.util.List etc.).
                if (p instanceof org.eclipse.jdt.core.dom.QualifiedName) return true;
                // Skip field access names (fa.name, not the expression side).
                if (p instanceof org.eclipse.jdt.core.dom.FieldAccess fa && fa.getName() == n) return true;
                // Skip method/type declaration names.
                if (p instanceof org.eclipse.jdt.core.dom.MethodDeclaration md && md.getName() == n) return true;
                if (p instanceof org.eclipse.jdt.core.dom.AbstractTypeDeclaration atd && atd.getName() == n) return true;
                out.add(n.getIdentifier());
                return true;
            }
        });
    }

    private static BridgeAction makeRemoveUnusedThrownExceptionAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null) return null;
        org.eclipse.jdt.core.dom.Type ex = findThrownExceptionType(loc.found, offset);
        if (ex == null) return null;
        int s = ex.getStartPosition(), e = s + ex.getLength();
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Type> thrown = loc.found.thrownExceptionTypes();
        int idx = thrown.indexOf(ex);
        if (thrown.size() > 1) {
            if (idx == 0) e = thrown.get(1).getStartPosition();
            else {
                s = thrown.get(idx - 1).getStartPosition() + thrown.get(idx - 1).getLength();
                while (s < ex.getStartPosition() && source.charAt(s) != ',') s++;
                if (s < ex.getStartPosition()) s++;
            }
        } else {
            int prev = s; while (prev > 0 && !source.substring(prev, s).contains("throws")) prev--;
            s = prev;
        }
        return makeRemoveTextAction("Remove unused thrown exception", uri, source, s, e);
    }

    private static BridgeAction makeInitializeLocalVariableAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(offset); cu.accept(loc);
        if (loc.found == null || loc.found.fragments().isEmpty()) return null;
        org.eclipse.jdt.core.dom.VariableDeclarationFragment frag = (org.eclipse.jdt.core.dom.VariableDeclarationFragment)loc.found.fragments().get(0);
        if (frag.getInitializer() != null) return null;
        return singleInsertionAction("Initialize variable", uri, source, frag.getStartPosition() + frag.getLength(), " = " + defaultInitializerFor(loc.found.getType()));
    }

    private static BridgeAction makeAddMissingJavadocTagAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, int problemId) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null || loc.found.getJavadoc() == null) return null;
        String tag = switch (problemId) {
            case org.eclipse.jdt.core.compiler.IProblem.JavadocMissingParamTag -> "@param " + firstMissingParamTag(loc.found);
            case org.eclipse.jdt.core.compiler.IProblem.JavadocMissingReturnTag -> "@return";
            case org.eclipse.jdt.core.compiler.IProblem.JavadocMissingThrowsTag -> "@throws " + firstMissingThrowsTag(loc.found);
            default -> null;
        };
        if (tag == null) return null;
        return singleInsertionAction("Add Javadoc " + tag, uri, source, javadocClosingOffset(loc.found.getJavadoc()), " * " + tag + "\n" + indentOf(source, cu.getLineNumber(loc.found.getStartPosition()) - 1));
    }

    private static BridgeAction makeRemoveInvalidJavadocTagAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        if (loc.found == null || loc.found.getJavadoc() == null) return null;
        org.eclipse.jdt.core.dom.TagElement tag = null;
        for (Object t : loc.found.getJavadoc().tags()) {
            org.eclipse.jdt.core.dom.TagElement te = (org.eclipse.jdt.core.dom.TagElement)t;
            if (offset >= te.getStartPosition() && offset <= te.getStartPosition() + te.getLength()) { tag = te; break; }
        }
        if (tag == null) return null;
        int s = lineStart(source, cu.getLineNumber(tag.getStartPosition()) - 1), e = lineStart(source, cu.getLineNumber(tag.getStartPosition() + tag.getLength()));
        return makeRemoveTextAction("Remove invalid Javadoc tag", uri, source, s, e);
    }

    private static BridgeAction makeRenameToExistingTypeAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String missing) {
        String best = nearestTypeName(cu, missing); if (best == null) return null;
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, off, 1);
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.Name)) node = node.getParent();
        if (node == null) return null;
        return makeReplaceNodeAction("Change to '" + best + "'", uri, source, node, best);
    }

    /**
     * "Change to 'SimpleName' (pkg)" — rename the type at the error location to {@code fqn}'s
     * simple name AND add an import statement for the FQN.
     */
    private static BridgeAction makeChangeToTypeAction(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String fqn) {
        int dot = fqn.lastIndexOf('.');
        String simpleName = dot >= 0 ? fqn.substring(dot + 1) : fqn;
        String pkg        = dot >= 0 ? fqn.substring(0, dot)  : "";

        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        org.eclipse.jdt.core.dom.ASTNode node = org.eclipse.jdt.core.dom.NodeFinder.perform(cu, off, 1);
        while (node != null && !(node instanceof org.eclipse.jdt.core.dom.Name)) node = node.getParent();
        if (node == null) return null;

        // Edit 1: replace the type name in the source
        int nStart = node.getStartPosition(), nEnd = nStart + node.getLength();
        int[] sLC = CompilationService.offsetToLineCol(source, nStart);
        int[] eLC = CompilationService.offsetToLineCol(source, nEnd);
        BridgeTextEdit rename = new BridgeTextEdit();
        rename.startLine = sLC[0]; rename.startChar = sLC[1];
        rename.endLine   = eLC[0]; rename.endChar   = eLC[1];
        rename.newText = simpleName;

        // Edit 2: insert the import (skip if already imported or same package)
        List<BridgeTextEdit> edits = new ArrayList<>();
        edits.add(rename);
        boolean alreadyImported = cu.imports().stream().anyMatch(i ->
                fqn.equals(((org.eclipse.jdt.core.dom.ImportDeclaration) i).getName().getFullyQualifiedName()));
        if (!alreadyImported && !pkg.isEmpty() && !"java.lang".equals(pkg)) {
            int insertOff = findSortedImportInsertPoint(source, cu, fqn);
            edits.add(insertionEdit(source, insertOff, "import " + fqn + ";\n"));
        }

        BridgeAction a = new BridgeAction();
        a.title = "Change to '" + simpleName + "' (" + fqn + ")";
        a.kind  = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = edits;
        a.edits = List.of(fe);
        return a;
    }

    private static BridgeAction makeAddTypeParameterAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String typeName) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(off); cu.accept(loc);
        if (loc.found == null) return null;
        int insert = loc.found.getName().getStartPosition() + loc.found.getName().getLength();
        return singleInsertionAction("Add type parameter '" + typeName + "'", uri, source, insert, "<" + typeName + ">");
    }

    private static BridgeAction makeCreateTopLevelTypeAction(String title, String uri, String source, String name, String kind) {
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri;
        // Insert at end of file
        int[] lc = CompilationService.offsetToLineCol(source, source.length());
        BridgeTextEdit te = new BridgeTextEdit();
        te.startLine = lc[0]; te.startChar = lc[1]; te.endLine = lc[0]; te.endChar = lc[1];
        te.newText = "\n\n" + kind + " " + name + " {\n}\n";
        fe.edits = List.of(te); a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeCreateLocalVariableAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String name) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        StatementLocator loc = new StatementLocator(off); cu.accept(loc);
        if (loc.found == null) return null;
        String indent = indentOf(source, cu.getLineNumber(loc.found.getStartPosition()) - 1);
        return singleInsertionAction("Create local variable '" + name + "'", uri, source, lineStart(source, cu.getLineNumber(loc.found.getStartPosition()) - 1), indent + "Object " + name + " = null;\n");
    }

    private static BridgeAction makeCreateFieldAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String name, boolean isStatic) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        // Use the enclosing class, not always the first class in the file
        TypeDeclLocator tdl = new TypeDeclLocator(off); cu.accept(tdl);
        org.eclipse.jdt.core.dom.TypeDeclaration td = tdl.found;
        if (td == null) {
            if (cu.types().isEmpty()) return null;
            Object first = cu.types().get(0);
            if (!(first instanceof org.eclipse.jdt.core.dom.TypeDeclaration)) return null;
            td = (org.eclipse.jdt.core.dom.TypeDeclaration) first;
        }
        int insert = bodyInsertOffset(td, source);
        if (insert < 0) return null;
        String memberIndent = indentOf(source, cu.getLineNumber(td.getStartPosition()) - 1) + "    ";
        String title = isStatic ? "Create constant '" + name + "'" : "Create field '" + name + "'";
        String decl = isStatic ? "private static final Object " + name + " = null;\n" : "private Object " + name + ";\n";
        return singleInsertionAction(title, uri, source, insert, memberIndent + decl);
    }

    private static BridgeAction makeCreateParameterAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String name) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(off); cu.accept(loc);
        if (loc.found == null) return null;
        int insert = loc.found.getName().getStartPosition() + loc.found.getName().getLength();
        while (source.charAt(insert) != '(') insert++; insert++;
        return singleInsertionAction("Create parameter '" + name + "'", uri, source, insert, "Object " + name + (loc.found.parameters().isEmpty() ? "" : ", "));
    }

    private static BridgeAction makeAddFinalAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String name) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        VarDeclLocator loc = new VarDeclLocator(off); cu.accept(loc);
        if (loc.found == null) return null;
        if ((loc.found.getModifiers() & org.eclipse.jdt.core.dom.Modifier.FINAL) != 0) return null;
        return singleInsertionAction("Make '" + name + "' final", uri, source, loc.found.getStartPosition(), "final ");
    }

    /**
     * Scans the entire compilation unit for local variable declarations and method parameters
     * that are not already {@code final} and could be made so (i.e. not reassigned).
     * Returns a single {@code source.generate.finalModifiers} workspace-edit action that
     * inserts {@code final } before each qualifying declaration, or {@code null} if there
     * is nothing to do.
     */
    private static BridgeAction makeFinalModifiersAction(
            String uri,
            String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu) {

        List<BridgeTextEdit> edits = new ArrayList<>();

        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            // Collect all assignment targets so we can skip reassigned variables.
            private final Set<String> reassigned = collectReassignedNames(cu);

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.VariableDeclarationStatement node) {
                if ((node.getModifiers() & org.eclipse.jdt.core.dom.Modifier.FINAL) != 0) return true;
                // Check if any declared name is reassigned later — skip the entire statement if so.
                for (Object frag : node.fragments()) {
                    if (frag instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment vdf) {
                        if (reassigned.contains(vdf.getName().getIdentifier())) return true;
                    }
                }
                int insertAt = node.getStartPosition();
                BridgeTextEdit edit = offsetToEdit(source, cu, insertAt, "final ");
                if (edit != null) edits.add(edit);
                return true;
            }

            @Override
            public boolean visit(org.eclipse.jdt.core.dom.SingleVariableDeclaration node) {
                if ((node.getModifiers() & org.eclipse.jdt.core.dom.Modifier.FINAL) != 0) return true;
                if (reassigned.contains(node.getName().getIdentifier())) return true;
                // Only handle method parameters (not catch / forEach — those are fine too but keep simple).
                if (!(node.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration)) return true;
                int insertAt = node.getStartPosition();
                BridgeTextEdit edit = offsetToEdit(source, cu, insertAt, "final ");
                if (edit != null) edits.add(edit);
                return true;
            }
        });

        if (edits.isEmpty()) return null;

        BridgeFileEdit fe = new BridgeFileEdit();
        fe.uri = uri;
        fe.edits = edits;

        BridgeAction action = new BridgeAction();
        action.title = "Change modifiers to final where possible";
        action.kind = "source.generate.finalModifiers";
        action.edits = List.of(fe);
        return action;
    }

    /** Returns the set of local variable / parameter names that appear as the LHS of an assignment. */
    private static Set<String> collectReassignedNames(org.eclipse.jdt.core.dom.CompilationUnit cu) {
        Set<String> names = new HashSet<>();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.Assignment node) {
                org.eclipse.jdt.core.dom.Expression lhs = node.getLeftHandSide();
                if (lhs instanceof org.eclipse.jdt.core.dom.SimpleName sn) {
                    names.add(sn.getIdentifier());
                }
                return true;
            }
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.PostfixExpression node) {
                if (node.getOperand() instanceof org.eclipse.jdt.core.dom.SimpleName sn) {
                    names.add(sn.getIdentifier());
                }
                return true;
            }
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.PrefixExpression node) {
                org.eclipse.jdt.core.dom.PrefixExpression.Operator op = node.getOperator();
                if ((op == org.eclipse.jdt.core.dom.PrefixExpression.Operator.INCREMENT
                        || op == org.eclipse.jdt.core.dom.PrefixExpression.Operator.DECREMENT)
                        && node.getOperand() instanceof org.eclipse.jdt.core.dom.SimpleName sn) {
                    names.add(sn.getIdentifier());
                }
                return true;
            }
        });
        return names;
    }

    /** Converts a source byte offset to a BridgeTextEdit that inserts {@code text} at that position. */
    private static BridgeTextEdit offsetToEdit(String source, org.eclipse.jdt.core.dom.CompilationUnit cu, int offset, String text) {
        if (offset < 0 || offset > source.length()) return null;
        int line = cu.getLineNumber(offset) - 1; // 0-based
        int lineStart = cu.getColumnNumber(offset) >= 0
                ? offset - cu.getColumnNumber(offset)
                : 0;
        int col = offset - lineStart;
        BridgeTextEdit e = new BridgeTextEdit();
        e.startLine = line;
        e.startChar = col;
        e.endLine = line;
        e.endChar = col;
        e.newText = text;
        return e;
    }

    private static BridgeAction makeAddAllMissingJavadocTagsAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d) {
        int off = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        MethodLocator loc = new MethodLocator(off); cu.accept(loc);
        if (loc.found == null || loc.found.getJavadoc() == null) return null;
        StringBuilder sb = new StringBuilder();
        String p = firstMissingParamTag(loc.found); if (p != null) sb.append(" * @param ").append(p).append("\n");
        if (!hasReturnTag(loc.found.getJavadoc()) && loc.found.getReturnType2() != null && !"void".equals(loc.found.getReturnType2().toString())) sb.append(" * @return\n");
        String t = firstMissingThrowsTag(loc.found); if (t != null) sb.append(" * @throws ").append(t).append("\n");
        if (sb.length() == 0) return null;
        return singleInsertionAction("Add all missing Javadoc tags", uri, source, javadocClosingOffset(loc.found.getJavadoc()), sb.toString() + indentOf(source, cu.getLineNumber(loc.found.getStartPosition()) - 1));
    }

    private static BridgeAction makeCreateMethodAction(String uri, String source, org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeDiagnostic d, String methodName) {
        int offset = CompilationService.lineColToOffset(source, d.startLine, d.startChar);
        ExpressionLocator loc = new ExpressionLocator(offset); cu.accept(loc);
        org.eclipse.jdt.core.dom.TypeDeclaration targetType = null; boolean isStatic = false;
        if (loc.found instanceof org.eclipse.jdt.core.dom.MethodInvocation mi) {
            if (mi.getExpression() == null) {
                org.eclipse.jdt.core.dom.ASTNode p = mi;
                while (p != null && !(p instanceof org.eclipse.jdt.core.dom.TypeDeclaration)) {
                    if (p instanceof org.eclipse.jdt.core.dom.MethodDeclaration md) isStatic = (md.getModifiers() & org.eclipse.jdt.core.dom.Modifier.STATIC) != 0;
                    p = p.getParent();
                }
                if (p instanceof org.eclipse.jdt.core.dom.TypeDeclaration td) targetType = td;
            } else {
                org.eclipse.jdt.core.dom.ITypeBinding tb = mi.getExpression().resolveTypeBinding();
                if (tb != null && tb.isFromSource()) {
                    for (Object o : cu.types()) if (o instanceof org.eclipse.jdt.core.dom.TypeDeclaration td && td.getName().getIdentifier().equals(tb.getName())) { targetType = td; break; }
                }
            }
        }
        if (targetType == null && !cu.types().isEmpty() && cu.types().get(0) instanceof org.eclipse.jdt.core.dom.TypeDeclaration td) targetType = td;
        if (targetType == null) return null;
        int ins = targetType.getStartPosition() + targetType.getLength() - 1;
        while (ins > 0 && source.charAt(ins) != '}') ins--;
        String ind = indentOf(source, cu.getLineNumber(ins) - 1);
        String text = "\n\n" + ind + "    " + (isStatic ? "static " : "") + "void " + methodName + "() {\n" + ind + "        // TODO Auto-generated method stub\n" + ind + "    }\n";
        return singleInsertionAction("Create method '" + methodName + "()'", uri, source, ins, text);
    }

    private static String getEnclosingMethodName(org.eclipse.jdt.core.dom.CompilationUnit cu, int offset) {
        MethodLocator loc = new MethodLocator(offset); cu.accept(loc);
        return loc.found != null ? loc.found.getName().getIdentifier() : null;
    }

    private static String chooseFieldName(org.eclipse.jdt.core.dom.AbstractTypeDeclaration type, String pref) {
        Set<String> ex = new HashSet<>();
        for (Object o : type.bodyDeclarations()) if (o instanceof org.eclipse.jdt.core.dom.FieldDeclaration f) for (Object fr : f.fragments()) ex.add(((org.eclipse.jdt.core.dom.VariableDeclarationFragment)fr).getName().getIdentifier());
        if (!ex.contains(pref)) return pref;
        int i = 2; while (ex.contains(pref + i)) i++; return pref + i;
    }

    private static int bodyInsertOffset(org.eclipse.jdt.core.dom.AbstractTypeDeclaration type, String source) {
        int b = source.indexOf('{', type.getStartPosition()); return b < 0 ? -1 : (source.indexOf('\n', b) >= 0 ? source.indexOf('\n', b) + 1 : b + 1);
    }

    private static int assignmentInsertOffset(org.eclipse.jdt.core.dom.MethodDeclaration method, String source) {
        if (method.getBody() == null) return method.getStartPosition();
        @SuppressWarnings("unchecked")
        List<org.eclipse.jdt.core.dom.Statement> stmts = method.getBody().statements();
        if (method.isConstructor() && !stmts.isEmpty()) {
            org.eclipse.jdt.core.dom.Statement f = stmts.get(0);
            if (f instanceof org.eclipse.jdt.core.dom.ConstructorInvocation || f instanceof org.eclipse.jdt.core.dom.SuperConstructorInvocation) {
                int e = f.getStartPosition() + f.getLength(); return e < source.length() && source.charAt(e) == '\n' ? e + 1 : e;
            }
        }
        int b = method.getBody().getStartPosition(); return source.indexOf('\n', b) >= 0 ? source.indexOf('\n', b) + 1 : b + 1;
    }

    private static boolean hasParamTag(org.eclipse.jdt.core.dom.Javadoc j, String name) {
        if (j == null || name == null) return false;
        for (Object o : j.tags()) {
            org.eclipse.jdt.core.dom.TagElement t = (org.eclipse.jdt.core.dom.TagElement)o;
            if ("@param".equals(t.getTagName()) && !t.fragments().isEmpty() && t.fragments().get(0) instanceof org.eclipse.jdt.core.dom.SimpleName n && name.equals(n.getIdentifier())) return true;
        }
        return false;
    }

    private static boolean hasReturnTag(org.eclipse.jdt.core.dom.Javadoc j) {
        if (j == null) return false;
        for (Object o : j.tags()) if (((org.eclipse.jdt.core.dom.TagElement)o).getTagName().equals("@return")) return true;
        return false;
    }

    private static boolean hasThrowsTag(org.eclipse.jdt.core.dom.Javadoc j, String name) {
        if (j == null || name == null) return false;
        String s = simpleTypeName(name);
        for (Object o : j.tags()) {
            org.eclipse.jdt.core.dom.TagElement t = (org.eclipse.jdt.core.dom.TagElement)o;
            if ("@throws".equals(t.getTagName())) for (Object f : t.fragments()) {
                if (f instanceof org.eclipse.jdt.core.dom.Name n && (name.equals(n.getFullyQualifiedName()) || s.equals(n.getFullyQualifiedName()))) return true;
                if (f instanceof org.eclipse.jdt.core.dom.SimpleName n && s.equals(n.getIdentifier())) return true;
            }
        }
        return false;
    }

    private static String buildParameterDocumentationStub(org.eclipse.jdt.core.dom.MethodDeclaration m, String p, String ind) {
        StringBuilder sb = new StringBuilder(); sb.append(ind).append("/**\n").append(ind).append(" * \n").append(ind).append(" * @param ").append(p).append("\n");
        if (!m.isConstructor() && m.getReturnType2() != null && !"void".equals(m.getReturnType2().toString())) sb.append(ind).append(" * @return\n");
        sb.append(ind).append(" */\n"); return sb.toString();
    }

    private static String nearestTypeName(org.eclipse.jdt.core.dom.CompilationUnit cu, String missing) {
        Set<String> names = new LinkedHashSet<>();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration n) { names.add(n.getName().getIdentifier()); return true; }
            public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration n) { names.add(n.getName().getIdentifier()); return true; }
        });
        String best = null; int bestS = Integer.MAX_VALUE;
        for (String n : names) { int s = levenshteinDistance(missing, n); if (s < bestS) { best = n; bestS = s; } }
        // Only suggest if genuinely similar: at most 3 edits AND within 40% of the name length.
        int threshold = Math.min(3, missing.length() / 3 + 1);
        return bestS <= threshold ? best : null;
    }

    private static int levenshteinDistance(String a, String b) {
        int[][] dp = new int[a.length() + 1][b.length() + 1];
        for (int i = 0; i <= a.length(); i++) dp[i][0] = i;
        for (int j = 0; j <= b.length(); j++) dp[0][j] = j;
        for (int i = 1; i <= a.length(); i++) for (int j = 1; j <= b.length(); j++) dp[i][j] = Math.min(Math.min(dp[i-1][j]+1, dp[i][j-1]+1), dp[i-1][j-1] + (Character.toLowerCase(a.charAt(i-1)) == Character.toLowerCase(b.charAt(j-1)) ? 0 : 1));
        return dp[a.length()][b.length()];
    }

    private static BridgeAction singleInsertionAction(String t, String u, String s, int o, String n) {
        BridgeAction a = new BridgeAction(); a.title = t; a.kind = "quickfix";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = u; fe.edits = List.of(insertionEdit(s, o, n)); a.edits = List.of(fe); return a;
    }

    private static int javadocClosingOffset(org.eclipse.jdt.core.dom.Javadoc j) {
        return Math.max(j.getStartPosition(), j.getStartPosition() + j.getLength() - 3);
    }

    private static String firstMissingParamTag(org.eclipse.jdt.core.dom.MethodDeclaration m) {
        for (Object p : m.parameters()) if (!hasParamTag(m.getJavadoc(), ((org.eclipse.jdt.core.dom.SingleVariableDeclaration)p).getName().getIdentifier())) return ((org.eclipse.jdt.core.dom.SingleVariableDeclaration)p).getName().getIdentifier();
        return null;
    }

    private static String firstMissingThrowsTag(org.eclipse.jdt.core.dom.MethodDeclaration m) {
        for (Object t : m.thrownExceptionTypes()) if (!hasThrowsTag(m.getJavadoc(), t.toString())) return t.toString();
        return null;
    }

    private static String simpleTypeName(String n) { int d = n.lastIndexOf('.'); return d >= 0 ? n.substring(d + 1) : n; }

    private static String defaultInitializerFor(org.eclipse.jdt.core.dom.Type t) {
        if (t.isPrimitiveType()) {
            String s = t.toString();
            if ("boolean".equals(s)) return "false";
            if ("char".equals(s)) return "'\\0'";
            if ("long".equals(s)) return "0L";
            if ("float".equals(s)) return "0.0f";
            if ("double".equals(s)) return "0.0d";
            return "0";
        }
        return "null";
    }

    private static BridgeTextEdit insertionEdit(String s, int o, String n) {
        int[] lc = CompilationService.offsetToLineCol(s, o);
        BridgeTextEdit e = new BridgeTextEdit(); e.startLine = lc[0]; e.startChar = lc[1]; e.endLine = lc[0]; e.endChar = lc[1]; e.newText = n; return e;
    }

    private static BridgeTextEdit removalEditFromLine(String source, int line) {
        int start = lineStart(source, line);
        int end   = lineStart(source, line + 1);
        return removalEditFromOffsets(source, start, end);
    }

    /**
     * Generates a {@code toString()} method for the enclosing type at the cursor.
     * Returns {@code null} if the type already has a {@code toString()} override,
     * has no fields to include, or the cursor is not inside a type body.
     * Matches jdtls by offering the action as both {@code quickassist} and
     * {@code source.generate.toString}.
     */
    private static BridgeAction makeToStringAction(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        if (range == null) return null;

        final int cursorLine = range.startLine;

        // Locate the innermost type containing the cursor.
        final org.eclipse.jdt.core.dom.AbstractTypeDeclaration[] holder = {null};
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration n) { return check(n); }
            @Override public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration n) { return check(n); }
            private boolean check(org.eclipse.jdt.core.dom.AbstractTypeDeclaration n) {
                int s = cu.getLineNumber(n.getStartPosition()) - 1;
                int e = cu.getLineNumber(n.getStartPosition() + n.getLength() - 1) - 1;
                if (cursorLine >= s && cursorLine <= e) holder[0] = n;
                return true;
            }
        });
        org.eclipse.jdt.core.dom.AbstractTypeDeclaration atd = holder[0];
        if (!(atd instanceof org.eclipse.jdt.core.dom.TypeDeclaration td) || td.isInterface()) return null;

        // Don't offer if toString() already exists.
        for (org.eclipse.jdt.core.dom.MethodDeclaration md : td.getMethods()) {
            if ("toString".equals(md.getName().getIdentifier()) && md.parameters().isEmpty()) return null;
        }

        // Collect instance fields.
        List<String[]> fields = new ArrayList<>(); // [typeName, fieldName]
        for (Object bd : td.bodyDeclarations()) {
            if (!(bd instanceof org.eclipse.jdt.core.dom.FieldDeclaration fd)) continue;
            if ((fd.getModifiers() & org.eclipse.jdt.core.dom.Modifier.STATIC) != 0) continue;
            for (Object frag : fd.fragments()) {
                if (frag instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment vdf) {
                    fields.add(new String[]{fd.getType().toString(), vdf.getName().getIdentifier()});
                }
            }
        }

        String className = td.getName().getIdentifier();
        String memberIndent = indentOf(source, cu.getLineNumber(atd.getStartPosition()) - 1) + "    ";

        // Build the method body: "ClassName [field1=" + field1 + ", field2=" + field2 + "]"
        StringBuilder body = new StringBuilder();
        body.append("\"").append(className).append(" [");
        for (int i = 0; i < fields.size(); i++) {
            if (i > 0) body.append(" + \", ");
            else body.append("\" + \"");
            body.append(fields.get(i)[1]).append("=\" + ").append(fields.get(i)[1]);
        }
        if (fields.isEmpty()) {
            body.append("]\"");
        } else {
            body.append(" + \"]\"");
        }

        int classEndOffset = atd.getStartPosition() + atd.getLength() - 1;
        while (classEndOffset > 0 && source.charAt(classEndOffset) != '}') classEndOffset--;
        int[] insLC = CompilationService.offsetToLineCol(source, classEndOffset);

        String methodText = "\n"
                + memberIndent + "@Override\n"
                + memberIndent + "public String toString() {\n"
                + memberIndent + "    return " + body + ";\n"
                + memberIndent + "}\n";

        BridgeTextEdit edit = new BridgeTextEdit();
        edit.startLine = insLC[0]; edit.startChar = insLC[1];
        edit.endLine   = insLC[0]; edit.endChar   = insLC[1];
        edit.newText   = methodText;

        // jdtls offers toString as both quickassist (lightbulb) and source.generate.toString (source menu).
        // Return the quickassist variant here; the source.generate.toString variant is added as a second action.
        BridgeAction quickassist = new BridgeAction();
        quickassist.title = "Generate toString()";
        quickassist.kind  = "quickassist";
        BridgeFileEdit fe1 = new BridgeFileEdit(); fe1.uri = uri; fe1.edits = List.of(edit);
        quickassist.edits = List.of(fe1);

        return quickassist;
    }

    /** Generate Getter / Setter / both for fields in the enclosing type at the cursor. */
    private static List<BridgeAction> makeGetterSetterActions(String uri, String source,
            org.eclipse.jdt.core.dom.CompilationUnit cu, BridgeRange range) {
        if (range == null) return List.of();

        final int cursorLine = range.startLine; // 0-based

        // Find the innermost AbstractTypeDeclaration whose body contains the cursor line.
        // This works regardless of whether the cursor is on a field, blank line, or brace.
        final org.eclipse.jdt.core.dom.AbstractTypeDeclaration[] atdHolder = {null};
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration node) { return checkType(node); }
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.EnumDeclaration node) { return checkType(node); }
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.AnnotationTypeDeclaration node) { return checkType(node); }
            @Override
            public boolean visit(org.eclipse.jdt.core.dom.RecordDeclaration node) {
                int bodyStart = cu.getLineNumber(node.getStartPosition()) - 1;
                int bodyEnd   = cu.getLineNumber(node.getStartPosition() + node.getLength() - 1) - 1;
                if (cursorLine >= bodyStart && cursorLine <= bodyEnd) atdHolder[0] = node;
                return true; // descend to find inner types
            }
            private boolean checkType(org.eclipse.jdt.core.dom.AbstractTypeDeclaration node) {
                int bodyStart = cu.getLineNumber(node.getStartPosition()) - 1;
                int bodyEnd   = cu.getLineNumber(node.getStartPosition() + node.getLength() - 1) - 1;
                if (cursorLine >= bodyStart && cursorLine <= bodyEnd) atdHolder[0] = node;
                return true; // descend to find inner types (innermost wins due to overwrite)
            }
        });
        org.eclipse.jdt.core.dom.AbstractTypeDeclaration atd = atdHolder[0];
        if (atd == null) return List.of();

        // Collect all instance FieldDeclarations directly in this type.
        List<org.eclipse.jdt.core.dom.FieldDeclaration> fields = new ArrayList<>();
        for (Object bd : atd.bodyDeclarations()) {
            if (bd instanceof org.eclipse.jdt.core.dom.FieldDeclaration fd) {
                boolean isStatic = (fd.getModifiers() & org.eclipse.jdt.core.dom.Modifier.STATIC) != 0;
                if (!isStatic) fields.add(fd);
            }
        }
        if (fields.isEmpty()) return List.of();

        // Collect existing method names so we don't offer to generate what's already there.
        // Key: "get<Name>" or "set<Name>", value: parameter count.
        java.util.Map<String, Integer> existingMethods = new java.util.HashMap<>();
        for (Object bd : atd.bodyDeclarations()) {
            if (bd instanceof org.eclipse.jdt.core.dom.MethodDeclaration md) {
                existingMethods.put(md.getName().getIdentifier(), md.parameters().size());
            }
        }

        String memberIndent = indentOf(source, cu.getLineNumber(atd.getStartPosition()) - 1) + "    ";

        // Insert point: just before the closing '}' of the class body.
        int classEndOffset = atd.getStartPosition() + atd.getLength() - 1;
        while (classEndOffset > 0 && source.charAt(classEndOffset) != '}') classEndOffset--;
        int[] insLC = CompilationService.offsetToLineCol(source, classEndOffset);

        // Per-field actions use "quickassist" kind — VS Code shows these directly
        // under "More Actions..." in the lightbulb popup (matching jdtls behaviour).
        List<BridgeAction> result = new ArrayList<>();
        List<BridgeTextEdit> allGetterEdits = new ArrayList<>();
        List<BridgeTextEdit> allSetterEdits = new ArrayList<>();
        for (org.eclipse.jdt.core.dom.FieldDeclaration fd : fields) {
            String typeName = fd.getType().toString();
            for (Object fragObj : fd.fragments()) {
                if (!(fragObj instanceof org.eclipse.jdt.core.dom.VariableDeclarationFragment vdf)) continue;
                String fieldName = vdf.getName().getIdentifier();
                String capName   = Character.toUpperCase(fieldName.charAt(0)) + fieldName.substring(1);
                String getterName = "get" + capName;
                String setterName = "set" + capName;

                // Skip if already exists (0-param getter or 1-param setter).
                boolean hasGetter = existingMethods.containsKey(getterName)
                        && existingMethods.get(getterName) == 0;
                boolean hasSetter = existingMethods.containsKey(setterName)
                        && existingMethods.get(setterName) == 1;

                // Leading blank line keeps the generated method visually separated.
                String getter = "\n" + memberIndent + "public " + typeName + " " + getterName + "() {\n"
                              + memberIndent + "    return " + fieldName + ";\n"
                              + memberIndent + "}\n";
                String setter = "\n" + memberIndent + "public void " + setterName + "(" + typeName + " " + fieldName + ") {\n"
                              + memberIndent + "    this." + fieldName + " = " + fieldName + ";\n"
                              + memberIndent + "}\n";

                BridgeTextEdit getterEdit = new BridgeTextEdit();
                getterEdit.startLine = insLC[0]; getterEdit.startChar = insLC[1];
                getterEdit.endLine   = insLC[0]; getterEdit.endChar   = insLC[1];
                getterEdit.newText   = getter;

                BridgeTextEdit setterEdit = new BridgeTextEdit();
                setterEdit.startLine = insLC[0]; setterEdit.startChar = insLC[1];
                setterEdit.endLine   = insLC[0]; setterEdit.endChar   = insLC[1];
                setterEdit.newText   = setter;

                if (!hasGetter) allGetterEdits.add(getterEdit);
                if (!hasSetter) allSetterEdits.add(setterEdit);

                if (!hasGetter)
                    result.add(makeQuickAssistAction("Generate Getter for '" + fieldName + "'", uri, getterEdit));
                if (!hasSetter)
                    result.add(makeQuickAssistAction("Generate Setter for '" + fieldName + "'", uri, setterEdit));

                if (!hasGetter || !hasSetter) {
                    BridgeAction both = new BridgeAction();
                    both.title = "Generate Getter and Setter for '" + fieldName + "'";
                    both.kind  = "quickassist";
                    BridgeFileEdit fe = new BridgeFileEdit();
                    fe.uri = uri;
                    List<BridgeTextEdit> bothEdits = new ArrayList<>();
                    if (!hasGetter) bothEdits.add(getterEdit);
                    if (!hasSetter) bothEdits.add(setterEdit);
                    fe.edits = bothEdits;
                    both.edits = List.of(fe);
                    result.add(both);
                }
            }
        }

        // Bulk actions: "source.generate.accessors" — shown in Source Actions menu.
        // Only offered when at least one getter/setter is still missing.
        if (!allGetterEdits.isEmpty() || !allSetterEdits.isEmpty()) {
            if (!allGetterEdits.isEmpty() && !allSetterEdits.isEmpty()) {
                BridgeAction bulkBoth = new BridgeAction();
                bulkBoth.title = "Generate Getters and Setters";
                bulkBoth.kind  = "source.generate.accessors";
                BridgeFileEdit fe = new BridgeFileEdit();
                fe.uri = uri;
                List<BridgeTextEdit> all = new ArrayList<>();
                all.addAll(allGetterEdits);
                all.addAll(allSetterEdits);
                fe.edits = all;
                bulkBoth.edits = List.of(fe);
                result.add(bulkBoth);
            }
            if (!allGetterEdits.isEmpty()) {
                BridgeAction bulkGet = new BridgeAction();
                bulkGet.title = "Generate Getters";
                bulkGet.kind  = "source.generate.accessors";
                BridgeFileEdit feG = new BridgeFileEdit(); feG.uri = uri; feG.edits = allGetterEdits;
                bulkGet.edits = List.of(feG);
                result.add(bulkGet);
            }
            if (!allSetterEdits.isEmpty()) {
                BridgeAction bulkSet = new BridgeAction();
                bulkSet.title = "Generate Setters";
                bulkSet.kind  = "source.generate.accessors";
                BridgeFileEdit feS = new BridgeFileEdit(); feS.uri = uri; feS.edits = allSetterEdits;
                bulkSet.edits = List.of(feS);
                result.add(bulkSet);
            }
        }

        return result;
    }

    private static BridgeAction makeEditAction(String title, String uri, BridgeTextEdit edit) {
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "source";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(edit);
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeQuickAssistAction(String title, String uri, BridgeTextEdit edit) {
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "quickassist";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(edit);
        a.edits = List.of(fe); return a;
    }

    private static BridgeAction makeAccessorAction(String title, String uri, BridgeTextEdit edit) {
        BridgeAction a = new BridgeAction(); a.title = title; a.kind = "source.generate.accessors";
        BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = uri; fe.edits = List.of(edit);
        a.edits = List.of(fe); return a;
    }

    private static BridgeTextEdit removalEditFromOffsets(String s, int start, int end) {
        int[] slc = CompilationService.offsetToLineCol(s, start), elc = CompilationService.offsetToLineCol(s, end);
        BridgeTextEdit e = new BridgeTextEdit(); e.startLine = slc[0]; e.startChar = slc[1]; e.endLine = elc[0]; e.endChar = elc[1]; e.newText = ""; return e;
    }

    private static List<BridgeAction> dedupeActions(List<BridgeAction> a) {
        Map<String, BridgeAction> d = new LinkedHashMap<>();
        for (var act : a) {
            StringBuilder k = new StringBuilder().append(act.title).append(act.kind);
            if (act.edits != null) for (var fe : act.edits) { k.append(fe.uri); if (fe.edits != null) for (var ed : fe.edits) k.append(ed.startLine).append(ed.startChar).append(ed.newText); }
            d.putIfAbsent(k.toString(), act);
        }
        return new ArrayList<>(d.values());
    }

    private static List<org.eclipse.jdt.core.dom.ASTNode> castAstNodes(List<?> l) {
        List<org.eclipse.jdt.core.dom.ASTNode> r = new ArrayList<>();
        for (Object o : l) if (o instanceof org.eclipse.jdt.core.dom.ASTNode n) r.add(n);
        return r;
    }

    private static BridgeTextEdit deletionEditForNodes(String s, List<? extends org.eclipse.jdt.core.dom.ASTNode> n, int i) {
        if (i < 0 || i >= n.size()) return null;
        int start = n.get(i).getStartPosition(), end = start + n.get(i).getLength();
        if (n.size() > 1) {
            if (i == 0) end = n.get(1).getStartPosition();
            else {
                start = n.get(i-1).getStartPosition() + n.get(i-1).getLength();
                while (start < n.get(i).getStartPosition() && s.charAt(start) != ',') start++;
                if (start < n.get(i).getStartPosition()) start++;
            }
        }
        return removalEditFromOffsets(s, start, end);
    }

    private static org.eclipse.jdt.core.dom.Type findThrownExceptionType(org.eclipse.jdt.core.dom.MethodDeclaration m, int o) {
        for (Object t : m.thrownExceptionTypes()) {
            org.eclipse.jdt.core.dom.Type ty = (org.eclipse.jdt.core.dom.Type)t;
            if (o >= ty.getStartPosition() && o <= ty.getStartPosition() + ty.getLength()) return ty;
        }
        return null;
    }

    private static int findImportInsertPoint(String s) {
        Matcher m = Pattern.compile("^\\s*(import|package)\\s+[\\w.*]+\\s*;", Pattern.MULTILINE).matcher(s);
        int e = 0; while (m.find()) e = m.end();
        if (e > 0) { while (e < s.length() && s.charAt(e) == '\n') e++; return e; }
        return 0;
    }

    private static int lineStart(String s, int l) {
        int c = 0; for (int i = 0; i < l && c < s.length(); i++) {
            int n = s.indexOf('\n', c); if (n < 0) return s.length(); c = n + 1;
        }
        return c;
    }

    private static String indentOf(String s, int l) {
        int start = lineStart(s, l), i = start;
        while (i < s.length() && (s.charAt(i) == ' ' || s.charAt(i) == '\t')) i++;
        return s.substring(start, i);
    }

    private static List<BridgeFileEdit> rename(Request r, CompilationService c) {
        if (r.files == null || r.uri == null || r.newName == null) return List.of();
        AstNavigationService nav = new AstNavigationService(); List<BridgeFileEdit> res = new ArrayList<>();
        List<BridgeLocation> allRefs = nav.findReferences(r.files, orDefault(r.sourceLevel), r.uri, r.offset);
        for (String furi : r.files.keySet()) {
            List<BridgeLocation> refs = allRefs.stream().filter(l -> furi.equals(l.uri)).toList();
            if (refs.isEmpty()) continue;
            List<BridgeTextEdit> eds = new ArrayList<>();
            for (var loc : refs) {
                BridgeTextEdit e = new BridgeTextEdit(); e.startLine = loc.startLine; e.startChar = loc.startChar; e.endLine = loc.endLine; e.endChar = loc.endChar; e.newText = r.newName; eds.add(e);
            }
            BridgeFileEdit fe = new BridgeFileEdit(); fe.uri = furi; fe.edits = eds; res.add(fe);
        }
        return res;
    }

    private static List<BridgeTextEdit> organizeImports(Request r, CompilationService c) {
        if (r.files == null || r.uri == null) return List.of();
        String s = r.files.get(r.uri); if (s == null) return List.of();
        CompletionService.ensureJrtIndex();
        org.eclipse.jdt.core.dom.ASTParser p = org.eclipse.jdt.core.dom.ASTParser.newParser(org.eclipse.jdt.core.dom.AST.getJLSLatest());
        p.setSource(s.toCharArray()); p.setKind(org.eclipse.jdt.core.dom.ASTParser.K_COMPILATION_UNIT);
        org.eclipse.jdt.core.dom.CompilationUnit cu = (org.eclipse.jdt.core.dom.CompilationUnit)p.createAST(null);
        Set<String> used = new TreeSet<>();
        cu.accept(new org.eclipse.jdt.core.dom.ASTVisitor() {
            public boolean visit(org.eclipse.jdt.core.dom.SimpleType n) {
                String id = n.getName().toString(); if (Character.isUpperCase(id.charAt(0))) used.add(id); return true;
            }
        });
        StringBuilder sb = new StringBuilder();
        for (String t : used) {
            List<String> m = CompletionService.searchBySimpleName(t);
            if (m.size() == 1) sb.append("import ").append(m.get(0)).append(";\n");
        }
        if (sb.length() == 0) return List.of();
        int is = -1, ie = -1; if (!cu.imports().isEmpty()) {
            is = ((org.eclipse.jdt.core.dom.ImportDeclaration)cu.imports().get(0)).getStartPosition();
            org.eclipse.jdt.core.dom.ImportDeclaration l = (org.eclipse.jdt.core.dom.ImportDeclaration)cu.imports().get(cu.imports().size()-1);
            ie = l.getStartPosition() + l.getLength();
        }
        BridgeTextEdit e = new BridgeTextEdit();
        if (is >= 0) { int[] slc = CompilationService.offsetToLineCol(s, is), elc = CompilationService.offsetToLineCol(s, ie); e.startLine = slc[0]; e.startChar = slc[1]; e.endLine = elc[0]; e.endChar = elc[1]; }
        else { int ip = findImportInsertPoint(s); int[] lc = CompilationService.offsetToLineCol(s, ip); e.startLine = lc[0]; e.startChar = lc[1]; e.endLine = lc[0]; e.endChar = lc[1]; }
        e.newText = sb.toString(); return List.of(e);
    }

    private static List<String> orEmpty(List<String> l) { return l != null ? l : Collections.emptyList(); }
    private static String orDefault(String l) { return (l != null && !l.isEmpty()) ? l : "21"; }
    private static long extractId(String j) {
        try { int i = j.indexOf("\"id\""); int c = j.indexOf(':', i); int s = c + 1; while (j.charAt(s) == ' ') s++; int e = s; while (Character.isDigit(j.charAt(e))) e++; return Long.parseLong(j.substring(s, e)); }
        catch (Exception e) { return 0; }
    }

    static class AnnotatableLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.BodyDeclaration bestNode; AnnotatableLocator(int o) { this.o = o; }
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode n) { if (n instanceof org.eclipse.jdt.core.dom.BodyDeclaration bd) { int s = bd.getStartPosition(), e = s + bd.getLength(); if (s <= o && o <= e) if (bestNode == null || s >= bestNode.getStartPosition()) bestNode = bd; } }
    }

    static class JavadocTargetLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        org.eclipse.jdt.core.dom.CompilationUnit cu; int sl, el; org.eclipse.jdt.core.dom.BodyDeclaration best; int bestP = Integer.MAX_VALUE;
        JavadocTargetLocator(org.eclipse.jdt.core.dom.CompilationUnit c, int s, int e) { cu = c; sl = s; el = e; }
        public boolean visit(org.eclipse.jdt.core.dom.MethodDeclaration n) { consider(n); return true; }
        public boolean visit(org.eclipse.jdt.core.dom.FieldDeclaration n) { consider(n); return true; }
        public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration n) { consider(n); return true; }
        private void consider(org.eclipse.jdt.core.dom.BodyDeclaration d) {
            int s = d.getStartPosition(), e = s + d.getLength();
            int dsl = cu.getLineNumber(s)-1, del = cu.getLineNumber(Math.max(s,e-1))-1;
            if (dsl < 0 || del < 0 || dsl > el || del < sl) return;
            int p = (dsl <= sl && sl <= del) ? Math.max(0, sl - dsl) : 1000 + Math.abs(dsl - sl);
            if (best == null || p < bestP) { best = d; bestP = p; }
        }
    }

    static class VarDeclLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.VariableDeclarationStatement found; VarDeclLocator(int o) { this.o = o; }
        public boolean visit(org.eclipse.jdt.core.dom.VariableDeclarationStatement n) { int s = n.getStartPosition(), e = s + n.getLength(); if (s <= o && o <= e) found = n; return true; }
    }

    static class ParameterLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; String n; org.eclipse.jdt.core.dom.SingleVariableDeclaration found; ParameterLocator(int o, String n) { this.o = o; this.n = n; }
        public boolean visit(org.eclipse.jdt.core.dom.SingleVariableDeclaration node) { if (node.getParent() instanceof org.eclipse.jdt.core.dom.MethodDeclaration && (n == null || n.equals(node.getName().getIdentifier()))) { int s = node.getStartPosition(), e = s + node.getLength(); if (s <= o && o <= e) { found = node; return false; } if (found == null) found = node; } return true; }
    }

    static class MethodLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.MethodDeclaration found; MethodLocator(int o) { this.o = o; }
        public boolean visit(org.eclipse.jdt.core.dom.MethodDeclaration n) { int s = n.getStartPosition(), e = s + n.getLength(); if (s <= o && o <= e) found = n; return true; }
    }

    static class AllAssignmentsLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        String n; List<org.eclipse.jdt.core.dom.ExpressionStatement> pureAssignments = new ArrayList<>(); AllAssignmentsLocator(String n) { this.n = n; }
        public boolean visit(org.eclipse.jdt.core.dom.ExpressionStatement node) { if (node.getExpression() instanceof org.eclipse.jdt.core.dom.Assignment a && a.getLeftHandSide() instanceof org.eclipse.jdt.core.dom.SimpleName sn && n.equals(sn.getIdentifier())) pureAssignments.add(node); return true; }
    }

    static class StatementLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.Statement found; StatementLocator(int o) { this.o = o; }
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode n) { if (n instanceof org.eclipse.jdt.core.dom.Statement stmt && !(n instanceof org.eclipse.jdt.core.dom.Block)) { int s = stmt.getStartPosition(), e = s + stmt.getLength(); if (s <= o && o <= e) if (found == null || stmt.getStartPosition() >= found.getStartPosition()) found = stmt; } }
    }

    static class ExpressionLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.Expression found; ExpressionLocator(int o) { this.o = o; }
        public void preVisit(org.eclipse.jdt.core.dom.ASTNode n) { if (n instanceof org.eclipse.jdt.core.dom.Expression expr) { int s = expr.getStartPosition(), e = s + expr.getLength(); if (s <= o && o <= e) if (found == null || expr.getLength() < found.getLength()) found = expr; } }
    }

    static class TypeDeclLocator extends org.eclipse.jdt.core.dom.ASTVisitor {
        int o; org.eclipse.jdt.core.dom.TypeDeclaration found; TypeDeclLocator(int o) { this.o = o; }
        public boolean visit(org.eclipse.jdt.core.dom.TypeDeclaration n) { int s = n.getStartPosition(), e = s + n.getLength(); if (s <= o && o <= e) found = n; return true; }
    }
}
