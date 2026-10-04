package com.jdtls.ecjbridge;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.jdt.core.CompletionContext;
import org.eclipse.jdt.core.CompletionProposal;
import org.eclipse.jdt.core.CompletionRequestor;
import org.eclipse.jdt.core.IJavaProject;
import org.eclipse.jdt.core.IPackageFragmentRoot;
import org.eclipse.jdt.core.Signature;
import org.eclipse.jdt.core.compiler.CharOperation;
import org.eclipse.jdt.internal.codeassist.CompletionEngine;
import org.eclipse.jdt.internal.codeassist.InternalCompletionContext;
import org.eclipse.jdt.internal.codeassist.InternalCompletionProposal;
import org.eclipse.jdt.internal.codeassist.InternalExtendedCompletionContext;
import org.eclipse.jdt.internal.compiler.ast.ASTNode;
import org.eclipse.jdt.internal.compiler.ast.AbstractMethodDeclaration;
import org.eclipse.jdt.internal.compiler.ast.AbstractVariableDeclaration;
import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.ast.FieldDeclaration;
import org.eclipse.jdt.internal.compiler.ast.Initializer;
import org.eclipse.jdt.internal.compiler.ast.TypeDeclaration;
import org.eclipse.jdt.internal.compiler.lookup.Binding;
import org.eclipse.jdt.internal.compiler.lookup.FieldBinding;
import org.eclipse.jdt.internal.compiler.lookup.LocalVariableBinding;
import org.eclipse.jdt.internal.compiler.lookup.LookupEnvironment;
import org.eclipse.jdt.internal.compiler.lookup.MethodBinding;
import org.eclipse.jdt.internal.compiler.lookup.ParameterizedTypeBinding;
import org.eclipse.jdt.internal.compiler.lookup.ReferenceBinding;
import org.eclipse.jdt.internal.compiler.lookup.Scope;
import org.eclipse.jdt.internal.compiler.lookup.SourceTypeBinding;
import org.eclipse.jdt.internal.compiler.lookup.TypeBinding;
import org.eclipse.jdt.internal.compiler.lookup.TypeVariableBinding;
import org.eclipse.jdt.internal.compiler.lookup.WildcardBinding;
import org.eclipse.jdt.internal.compiler.util.ObjectVector;
import org.eclipse.jdt.internal.codeassist.impl.Engine;

/**
 * Code completion through JDT's own {@link CompletionEngine}, without the Java
 * model.  Returns the raw {@link CompletionProposal} data (and the bits of
 * semantic information jdt.ls reads from the Java model while converting them)
 * so the Rust server can port jdt.ls' conversion to LSP items.
 */
final class CodeAssistService {

    // ── Result data (serialised with Gson; Rust reads camelCase fields) ───────

    static final class Proposal {
        public int kind;
        public String completion;
        public String name;
        public String signature;
        public String originalSignature;
        public String declarationSignature;
        public String declarationKey;
        public String key;
        public String declarationPackageName;
        public String declarationTypeName;
        public String packageName;
        public String typeName;
        public List<String> parameterNames;
        public int flags;
        public int additionalFlags;
        public int relevance;
        public int replaceStart;
        public int replaceEnd;
        public int tokenStart;
        public int tokenEnd;
        public int completionLocation;
        public String receiverSignature;
        public int receiverStart;
        public int receiverEnd;
        public boolean constructor;
        public int arrayDimensions;
        public int accessibility;
        public boolean canUseDiamond;
        public boolean compatible;
        /** {@code Engine.getSignature(binding.original())} for method bindings. */
        public String bindingSignature;
        public List<String> declarationTypeVariables;
        public List<Proposal> requiredProposals;
        /** jdt.ls computeTypeArgumentProposals(), for required TYPE_REF proposals of constructors. */
        public List<String> typeArguments;
    }

    static final class Context {
        public int offset;
        public String token;
        public int tokenStart;
        public int tokenEnd;
        public int tokenKind;
        public int tokenLocation;
        public boolean inJavadoc;
        public boolean inJavadocText;
        public boolean inJavadocFormalReference;
        public boolean extended;
        public List<String> expectedTypesSignatures;
        public List<String> expectedTypesKeys;
        /** CU-model-like enclosing element: "method", "field", "initializer", "type", or "unit". */
        public String enclosingKind;
        public String enclosingTypeName;
        public boolean enclosingStatic;
        public boolean enclosingInterface;
        public String enclosingMethodName;
        /** Simple class name of the engine's completion node (CompletionOnKeyword2, ...). */
        public String completionNode;
        public String completionNodeParent;
        /** IType.getFields() of the enclosing type (source model form). */
        public List<EnclosingField> enclosingFields = new ArrayList<>();
        /** IType.getMethods() names of the enclosing type. */
        public List<String> enclosingMethods = new ArrayList<>();
    }

    static final class EnclosingField {
        public String name;
        public String typeSignature;
        public int flags;
        public boolean isEnumConstant;
    }

    static final class VisibleElement {
        public int kind;            // 0 local, 1 field, 2 method
        public String name;
        public String typeSignature; // as the Java model reports it
        public int parameterCount;
        public String returnType;    // methods: return type signature (model form)
        public boolean inherited;    // declared outside the enclosing type
    }

    static final class Result {
        public Context context;
        public List<Proposal> proposals = new ArrayList<>();
        /** parameter type signature → assignable visible elements (jdt.ls getAssignableElements). */
        public Map<String, List<VisibleElement>> visibleElements = new LinkedHashMap<>();
        /** Proposal kinds the engine asked about through {@code isIgnored(kind)}. */
        public java.util.SortedSet<Integer> completionKinds = new java.util.TreeSet<>();
    }

    // ── Entry point ──────────────────────────────────────────────────────────

    /** {@code codeAssist} request: {@code query.op} selects the operation. */
    static Object handle(BridgeProtocol.Request req) {
        com.google.gson.JsonObject q = req.query == null ? new com.google.gson.JsonObject() : req.query;
        String op = q.has("op") ? q.get("op").getAsString() : "complete";
        Map<String, String> files = req.files == null ? Map.of() : req.files;
        List<String> classpath = req.classpath == null ? List.of() : req.classpath;
        String level = req.sourceLevel == null ? "21" : req.sourceLevel;
        switch (op) {
            case "complete":
                return new CodeAssistService().complete(files, classpath, level, req.uri, req.offset,
                        new java.util.HashSet<>(jsonStrings(q, "testUris")), jsonStrings(q, "favorites"),
                        jsonStrings(q, "typeFilters"), q.has("visibleElements") && q.get("visibleElements").getAsBoolean(),
                        q.has("unitPackage") && !q.get("unitPackage").isJsonNull() ? q.get("unitPackage").getAsString() : null);
            default:
                return CodeAssistOps.handle(op, q, files, classpath, level, req.uri, req.offset);
        }
    }

    static List<String> jsonStrings(com.google.gson.JsonObject q, String key) {
        List<String> out = new ArrayList<>();
        if (q.has(key) && q.get(key).isJsonArray()) {
            for (com.google.gson.JsonElement e : q.getAsJsonArray(key)) out.add(e.getAsString());
        }
        return out;
    }

    Result complete(Map<String, String> files, List<String> classpath, String sourceLevel, String uri, int offset,
            Set<String> testUris, List<String> favorites, List<String> typeFilters, boolean visibleElements) {
        return complete(files, classpath, sourceLevel, uri, offset, testUris, favorites, typeFilters, visibleElements, null);
    }

    Result complete(Map<String, String> files, List<String> classpath, String sourceLevel, String uri, int offset,
            Set<String> testUris, List<String> favorites, List<String> typeFilters, boolean visibleElements, String unitPackage) {
        String source = files.get(uri);
        if (source == null) source = "";
        boolean isTest = testUris.contains(uri);
        CodeAssistEnvironment env = CodeAssistEnvironment.create(files, testUris, classpath, sourceLevel, uri, !isTest);
        Map<String, String> options = assistOptions(sourceLevel);
        IJavaProject project = javaProjectProxy(options);
        Requestor requestor = new Requestor(!isTest, typeFilters, visibleElements);
        requestor.setAllowsRequiredProposals(CompletionProposal.FIELD_REF, CompletionProposal.TYPE_REF, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.FIELD_REF, CompletionProposal.TYPE_IMPORT, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.FIELD_REF, CompletionProposal.FIELD_IMPORT, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.METHOD_REF, CompletionProposal.TYPE_REF, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.METHOD_REF, CompletionProposal.TYPE_IMPORT, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.METHOD_REF, CompletionProposal.METHOD_IMPORT, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.CONSTRUCTOR_INVOCATION, CompletionProposal.TYPE_REF, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION, CompletionProposal.TYPE_REF, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.ANONYMOUS_CLASS_DECLARATION, CompletionProposal.TYPE_REF, true);
        requestor.setAllowsRequiredProposals(CompletionProposal.TYPE_REF, CompletionProposal.TYPE_REF, true);
        requestor.setFavoriteReferences(favorites == null ? new String[0] : favorites.toArray(new String[0]));
        requestor.setRequireExtendedContext(true);

        CompletionEngine engine = new CompletionEngine(env, requestor, options, project, null, new NullProgressMonitor());
        requestor.engine = engine;
        requestor.env = env;
        requestor.source = source;
        // getResolvedSignature() resolves source constructor signatures through the
        // "no cache" environment; reuse ours instead of a model-based one.
        setField(CompletionEngine.class, engine, "noCacheNameEnvironment", env);
        engine.complete(new InMemoryCompilationUnit(uri, source).withPackage(unitPackage), offset, 0, null);
        return requestor.result;
    }

    // ── Template scope (CompilationUnitCompletion) ──────────────────────────

    static final class ScopeVariable {
        public String name;
        public String signature;
        public boolean isArray;
        public boolean isIterable;
        public List<String> memberTypeNames = new ArrayList<>();
        public List<String> supertypes = new ArrayList<>();
    }

    static final class TemplateScope {
        public List<ScopeVariable> locals = new ArrayList<>();
        public List<ScopeVariable> fields = new ArrayList<>();
        public String enclosingType;
        public String enclosingMethod;
    }

    /**
     * The variables JDT's {@code CompilationUnitCompletion} collects by code
     * completion at {@code offset} (the template start), with the type facts
     * the template resolvers need.
     */
    TemplateScope templateScope(Map<String, String> files, List<String> classpath, String sourceLevel, String uri, int offset,
            Set<String> testUris, int contextOffset, String unitPackage) {
        String source = files.get(uri);
        if (source == null) source = "";
        boolean isTest = testUris.contains(uri);
        CodeAssistEnvironment env = CodeAssistEnvironment.create(files, testUris, classpath, sourceLevel, uri, !isTest);
        Map<String, String> options = assistOptions(sourceLevel);
        IJavaProject project = javaProjectProxy(options);
        TemplateScope scope = new TemplateScope();
        List<String[]> locals = new ArrayList<>();
        List<String[]> fields = new ArrayList<>();
        CompletionEngine[] engineRef = new CompletionEngine[1];
        CompletionContext[] contextRef = new CompletionContext[1];
        CompletionRequestor requestor = new CompletionRequestor() {
            {
                setIgnored(CompletionProposal.ANONYMOUS_CLASS_DECLARATION, true);
                setIgnored(CompletionProposal.ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION, true);
                setIgnored(CompletionProposal.KEYWORD, true);
                setIgnored(CompletionProposal.LABEL_REF, true);
                setIgnored(CompletionProposal.METHOD_DECLARATION, true);
                setIgnored(CompletionProposal.METHOD_NAME_REFERENCE, true);
                setIgnored(CompletionProposal.METHOD_REF, true);
                setIgnored(CompletionProposal.CONSTRUCTOR_INVOCATION, true);
                setIgnored(CompletionProposal.METHOD_REF_WITH_CASTED_RECEIVER, true);
                setIgnored(CompletionProposal.PACKAGE_REF, true);
                setIgnored(CompletionProposal.MODULE_REF, true);
                setIgnored(CompletionProposal.MODULE_DECLARATION, true);
                setIgnored(CompletionProposal.POTENTIAL_METHOD_DECLARATION, true);
                setIgnored(CompletionProposal.VARIABLE_DECLARATION, true);
                setIgnored(CompletionProposal.TYPE_REF, true);
                setRequireExtendedContext(true);
            }

            @Override
            public boolean isTestCodeExcluded() {
                return !isTest;
            }

            @Override
            public void acceptContext(CompletionContext context) {
                contextRef[0] = context;
            }

            @Override
            public void accept(CompletionProposal proposal) {
                String name = String.valueOf(proposal.getCompletion());
                String signature = String.valueOf(proposal.getSignature());
                if (proposal.getKind() == CompletionProposal.LOCAL_VARIABLE_REF) {
                    locals.add(new String[] { name, signature });
                } else if (proposal.getKind() == CompletionProposal.FIELD_REF) {
                    fields.add(new String[] { name, signature });
                }
            }

            @Override
            public void endReporting() {
                try {
                    describe(scope.locals, locals);
                    describe(scope.fields, fields);
                } catch (RuntimeException e) {
                    // best effort
                }
            }

            private void describe(List<ScopeVariable> out, List<String[]> vars) {
                LookupEnvironment lookup = getField(CompletionEngine.class, engineRef[0], "lookupEnvironment");
                InternalExtendedCompletionContext ext = contextRef[0] instanceof InternalCompletionContext ic
                        ? getField(InternalCompletionContext.class, ic, "extendedContext") : null;
                Scope assistScope = ext == null ? null : getField(InternalExtendedCompletionContext.class, ext, "assistScope");
                for (String[] v : vars) {
                    ScopeVariable sv = new ScopeVariable();
                    sv.name = v[0];
                    sv.signature = v[1];
                    sv.isArray = Signature.getTypeSignatureKind(v[1]) == Signature.ARRAY_TYPE_SIGNATURE;
                    TypeBinding binding = null;
                    if (ext != null && assistScope != null) {
                        binding = typeFromSignature(ext, v[1], assistScope);
                    }
                    if (binding != null && lookup != null) {
                        collectSupertypes(binding, sv.supertypes, new java.util.HashSet<>());
                        if (!sv.isArray) {
                            ReferenceBinding iterable = lookup.getType(new char[][] { "java".toCharArray(), "lang".toCharArray(), "Iterable".toCharArray() });
                            TypeBinding sup = iterable == null ? null : binding.findSuperTypeOriginatingFrom(iterable);
                            if (sup != null) {
                                sv.isIterable = true;
                                TypeBinding member = null;
                                if (sup instanceof ParameterizedTypeBinding ptb && ptb.arguments != null && ptb.arguments.length > 0) {
                                    member = ptb.arguments[0];
                                    if (member instanceof WildcardBinding w) {
                                        member = w.boundKind == org.eclipse.jdt.internal.compiler.ast.Wildcard.EXTENDS ? w.bound : null;
                                    } else if (member instanceof TypeVariableBinding tv) {
                                        member = tv.firstBound;
                                    }
                                }
                                if (member == null) {
                                    sv.memberTypeNames.add("Object");
                                } else {
                                    sv.memberTypeNames.add(memberTypeName(member, contextRef[0]));
                                }
                            } else {
                                sv.memberTypeNames.add("Object");
                            }
                        }
                    }
                    if (sv.isArray) {
                        String element = Signature.createArraySignature(Signature.getElementType(v[1]), Signature.getArrayCount(v[1]) - 1);
                        sv.memberTypeNames.clear();
                        sv.memberTypeNames.add(Signature.getSimpleName(Signature.getSignatureSimpleName(element)));
                    }
                    if (sv.memberTypeNames.isEmpty()) sv.memberTypeNames.add("Object");
                    out.add(sv);
                }
            }

            private String memberTypeName(TypeBinding member, CompletionContext ctx) {
                String sig = new String(member.genericTypeSignature()).replace('/', '.');
                try {
                    return Signature.getSimpleName(Signature.getSignatureSimpleName(sig));
                } catch (RuntimeException e) {
                    return new String(member.sourceName());
                }
            }

            private void collectSupertypes(TypeBinding t, List<String> out, java.util.Set<TypeBinding> seen) {
                if (!(t instanceof ReferenceBinding rb) || !seen.add(t.erasure())) return;
                out.add(new String(t.erasure().readableName()));
                if (rb.superclass() != null) collectSupertypes(rb.superclass(), out, seen);
                ReferenceBinding[] ifs = rb.superInterfaces();
                if (ifs != null) for (ReferenceBinding i : ifs) collectSupertypes(i, out, seen);
                if (rb.isInterface()) out.add("java.lang.Object");
            }

            private TypeBinding typeFromSignature(InternalExtendedCompletionContext ext, String signature, Scope scope) {
                try {
                    Method m = InternalExtendedCompletionContext.class.getDeclaredMethod("getTypeFromSignature", String.class, Scope.class);
                    m.setAccessible(true);
                    return (TypeBinding) m.invoke(ext, signature, scope);
                } catch (ReflectiveOperationException e) {
                    return null;
                }
            }
        };
        CompletionEngine engine = new CompletionEngine(env, requestor, options, project, null, new NullProgressMonitor());
        engineRef[0] = engine;
        setField(CompletionEngine.class, engine, "noCacheNameEnvironment", env);
        engine.complete(new InMemoryCompilationUnit(uri, source).withPackage(unitPackage), offset, 0, null);
        // enclosing type / method (CompilationUnitContext.findEnclosingElement at the template start)
        Result main = complete(files, classpath, sourceLevel, uri, contextOffset, testUris, List.of(), List.of(), false, unitPackage);
        if (main.context != null) {
            scope.enclosingType = main.context.enclosingTypeName;
            scope.enclosingMethod = main.context.enclosingMethodName;
        }
        return scope;
    }

    // ── Requestor ────────────────────────────────────────────────────────────

    private static final class Requestor extends CompletionRequestor {
        final Result result = new Result();
        final boolean excludeTest;
        final List<String> typeFilters;
        final boolean computeVisibleElements;
        CompletionEngine engine;
        CodeAssistEnvironment env;
        String source;
        CompletionContext context;
        final List<CompletionProposal> raw = new ArrayList<>();

        Requestor(boolean excludeTest, List<String> typeFilters, boolean visibleElements) {
            this.excludeTest = excludeTest;
            this.typeFilters = typeFilters == null ? List.of() : typeFilters;
            this.computeVisibleElements = visibleElements;
        }

        @Override
        public boolean isTestCodeExcluded() {
            return excludeTest;
        }

        @Override
        public boolean isIgnored(int completionProposalKind) {
            result.completionKinds.add(completionProposalKind);
            return super.isIgnored(completionProposalKind);
        }

        @Override
        public boolean isIgnored(char[] fullTypeName) {
            if (fullTypeName == null) return false;
            String name = new String(fullTypeName);
            for (String f : typeFilters) {
                if (StringMatcher.match(f, name)) return true;
            }
            return false;
        }

        @Override
        public void acceptContext(CompletionContext context) {
            this.context = context;
            Context c = new Context();
            c.offset = context.getOffset();
            c.token = context.getToken() == null ? null : new String(context.getToken());
            c.tokenStart = context.getTokenStart();
            c.tokenEnd = context.getTokenEnd();
            c.tokenKind = context.getTokenKind();
            c.tokenLocation = context.getTokenLocation();
            c.inJavadoc = context.isInJavadoc();
            c.inJavadocText = context.isInJavadocText();
            c.inJavadocFormalReference = context.isInJavadocFormalReference();
            c.extended = context.isExtended();
            c.expectedTypesSignatures = strings(context.getExpectedTypesSignatures());
            c.expectedTypesKeys = strings(context.getExpectedTypesKeys());
            computeEnclosing(c);
            result.context = c;
        }

        @Override
        public void accept(CompletionProposal proposal) {
            raw.add(proposal);
            result.proposals.add(convert(proposal, true));
        }

        @Override
        public void endReporting() {
            if (computeVisibleElements) {
                try {
                    computeVisibleElements();
                } catch (RuntimeException e) {
                    // best effort: guessing falls back to parameter names
                }
            }
        }

        private Proposal convert(CompletionProposal p, boolean top) {
            ensureParameterNames(p);
            Proposal r = new Proposal();
            r.kind = p.getKind();
            r.completion = str(p.getCompletion());
            r.name = str(p.getName());
            r.signature = str(p.getSignature());
            r.declarationSignature = str(p.getDeclarationSignature());
            r.declarationKey = str(p.getDeclarationKey());
            r.key = str(p.getKey());
            r.flags = p.getFlags();
            r.additionalFlags = p.getAdditionalFlags();
            r.relevance = p.getRelevance();
            r.replaceStart = p.getReplaceStart();
            r.replaceEnd = p.getReplaceEnd();
            r.tokenStart = p.getTokenStart();
            r.tokenEnd = p.getTokenEnd();
            r.completionLocation = p.getCompletionLocation();
            r.receiverSignature = str(p.getReceiverSignature());
            r.receiverStart = p.getReceiverStart();
            r.receiverEnd = p.getReceiverEnd();
            r.constructor = p.isConstructor();
            r.arrayDimensions = p.getArrayDimensions();
            r.accessibility = p.getAccessibility();
            char[][] names = getField(InternalCompletionProposal.class, p, "parameterNames");
            r.parameterNames = strings(names);
            if (p instanceof InternalCompletionProposal ip) {
                r.compatible = ip.isCompatibleProposal();
                r.originalSignature = str(getField(InternalCompletionProposal.class, p, "originalSignature"));
                r.declarationPackageName = str(getField(InternalCompletionProposal.class, p, "declarationPackageName"));
                r.declarationTypeName = str(getField(InternalCompletionProposal.class, p, "declarationTypeName"));
                r.packageName = str(getField(InternalCompletionProposal.class, p, "packageName"));
                r.typeName = str(getField(InternalCompletionProposal.class, p, "typeName"));
                r.declarationTypeVariables = strings(ip.getDeclarationTypeVariables());
                Binding b = ip.getBinding();
                if (b instanceof MethodBinding mb) {
                    MethodBinding original = mb.original();
                    char[] sig = Engine.getSignature(original != mb ? original : mb);
                    r.bindingSignature = str(sig);
                }
            }
            if (p.getKind() == CompletionProposal.CONSTRUCTOR_INVOCATION && context != null) {
                try {
                    r.canUseDiamond = p.canUseDiamond(context);
                } catch (RuntimeException e) {
                    r.canUseDiamond = false;
                }
            }
            CompletionProposal[] req = p.getRequiredProposals();
            if (req != null) {
                r.requiredProposals = new ArrayList<>();
                for (CompletionProposal q : req) {
                    Proposal rq = convert(q, false);
                    if (q.getKind() == CompletionProposal.TYPE_REF
                            && (p.getKind() == CompletionProposal.CONSTRUCTOR_INVOCATION
                                    || p.getKind() == CompletionProposal.ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION
                                    || p.getKind() == CompletionProposal.ANONYMOUS_CLASS_DECLARATION)) {
                        try {
                            rq.typeArguments = computeTypeArgumentProposals(q);
                        } catch (RuntimeException e) {
                            rq.typeArguments = List.of();
                        }
                    }
                    r.requiredProposals.add(rq);
                }
            }
            return r;
        }

        /**
         * The Java model computes missing parameter names lazily
         * ({@code findParameterNames}); without it, read them from the class
         * file (debug attributes) or fall back to {@code arg0..n} like
         * {@code BinaryMethod.getParameterNames()} does without attached source.
         */
        private void ensureParameterNames(CompletionProposal p) {
            if (!(p instanceof InternalCompletionProposal)) return;
            Boolean computed = getField(InternalCompletionProposal.class, p, "parameterNamesComputed");
            char[][] names = getField(InternalCompletionProposal.class, p, "parameterNames");
            if (Boolean.TRUE.equals(computed)) return;
            switch (p.getKind()) {
                case CompletionProposal.METHOD_REF, CompletionProposal.METHOD_REF_WITH_CASTED_RECEIVER,
                        CompletionProposal.METHOD_DECLARATION, CompletionProposal.CONSTRUCTOR_INVOCATION,
                        CompletionProposal.ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION,
                        CompletionProposal.ANONYMOUS_CLASS_DECLARATION, CompletionProposal.LAMBDA_EXPRESSION,
                        CompletionProposal.METHOD_NAME_REFERENCE, CompletionProposal.JAVADOC_METHOD_REF,
                        CompletionProposal.POTENTIAL_METHOD_DECLARATION -> {
                    if (names == null) {
                        names = binaryParameterNames(p);
                    }
                    if (names == null && p.getSignature() != null) {
                        try {
                            char[] sig = p.getSignature();
                            int count = Signature.getParameterCount(fix83600(sig));
                            names = CompletionEngine.createDefaultParameterNames(count);
                        } catch (IllegalArgumentException e) {
                            names = null;
                        }
                    }
                    if (names != null) {
                        p.setParameterNames(names);
                    }
                    setField(InternalCompletionProposal.class, p, "parameterNamesComputed", Boolean.TRUE);
                }
                default -> { }
            }
        }

        private char[][] binaryParameterNames(CompletionProposal p) {
            char[] declSig = p.getDeclarationSignature();
            if (declSig == null) return null;
            char[] sig = getField(InternalCompletionProposal.class, p, "originalSignature");
            if (sig == null) sig = p.getSignature();
            if (sig == null) return null;
            String declType = new String(Signature.toCharArray(Signature.getTypeErasure(declSig)));
            char[] selector = p.isConstructor() || p.getKind() == CompletionProposal.CONSTRUCTOR_INVOCATION
                    || p.getKind() == CompletionProposal.ANONYMOUS_CLASS_CONSTRUCTOR_INVOCATION
                            ? "<init>".toCharArray() : p.getName();
            if (p.getKind() == CompletionProposal.ANONYMOUS_CLASS_DECLARATION) selector = "<init>".toCharArray();
            return BinaryNames.find(env, declType, selector, sig);
        }

        // ── Enclosing element ────────────────────────────────────────────────

        private void computeEnclosing(Context c) {
            c.enclosingKind = "unit";
            InternalExtendedCompletionContext ext = extended();
            if (ext == null) return;
            ASTNode node = ext.getCompletionNode();
            ASTNode parent = ext.getCompletionNodeParent();
            c.completionNode = node == null ? null : node.getClass().getSimpleName();
            c.completionNodeParent = parent == null ? null : parent.getClass().getSimpleName();
            CompilationUnitDeclaration unit = getField(InternalExtendedCompletionContext.class, ext, "compilationUnitDeclaration");
            if (unit == null || unit.types == null) return;
            int offset = c.offset;
            for (TypeDeclaration t : unit.types) {
                if (enclosing(t, offset, c)) return;
            }
        }

        private boolean enclosing(TypeDeclaration t, int offset, Context c) {
            if (offset < t.declarationSourceStart || offset > t.declarationSourceEnd) return false;
            c.enclosingKind = "type";
            c.enclosingTypeName = new String(t.name);
            c.enclosingStatic = false;
            c.enclosingMethodName = null;
            c.enclosingInterface = TypeDeclaration.kind(t.modifiers) == TypeDeclaration.INTERFACE_DECL
                    || TypeDeclaration.kind(t.modifiers) == TypeDeclaration.ANNOTATION_TYPE_DECL;
            c.enclosingFields = new ArrayList<>();
            c.enclosingMethods = new ArrayList<>();
            if (t.fields != null) {
                for (FieldDeclaration f : t.fields) {
                    if (f instanceof Initializer || f.name == null) continue;
                    EnclosingField ef = new EnclosingField();
                    ef.name = new String(f.name);
                    ef.isEnumConstant = f.getKind() == AbstractVariableDeclaration.ENUM_CONSTANT;
                    ef.flags = f.modifiers & 0xFFFF;
                    if (ef.isEnumConstant) {
                        ef.flags |= org.eclipse.jdt.internal.compiler.classfmt.ClassFileConstants.AccEnum;
                        ef.typeSignature = Signature.createTypeSignature(t.name, false);
                    } else if (f.type != null) {
                        ef.typeSignature = org.eclipse.jdt.internal.core.util.Util.typeSignature(f.type);
                    }
                    c.enclosingFields.add(ef);
                }
            }
            if (t.methods != null) {
                for (AbstractMethodDeclaration m : t.methods) {
                    if (m.isDefaultConstructor() || m.isClinit()) continue;
                    c.enclosingMethods.add(new String(m.selector));
                }
            }
            if (t.memberTypes != null) {
                for (TypeDeclaration m : t.memberTypes) {
                    if (enclosing(m, offset, c)) return true;
                }
            }
            if (t.methods != null) {
                for (AbstractMethodDeclaration m : t.methods) {
                    if (m.isDefaultConstructor() || m.isClinit()) continue;
                    if (offset >= m.declarationSourceStart && offset <= m.declarationSourceEnd) {
                        c.enclosingKind = "method";
                        c.enclosingStatic = m.isStatic();
                        c.enclosingMethodName = new String(m.selector);
                        return true;
                    }
                }
            }
            if (t.fields != null) {
                ASTNode completionNode = extended() == null ? null : extended().getCompletionNode();
                for (FieldDeclaration f : t.fields) {
                    // The field the completion parser makes of a member-start
                    // token is not a Java model element: the type encloses it.
                    if (f == completionNode || (f.type != null && f.type == completionNode)
                            || f instanceof org.eclipse.jdt.internal.codeassist.complete.CompletionOnFieldType) continue;
                    if (offset >= f.declarationSourceStart && offset <= f.declarationSourceEnd) {
                        c.enclosingKind = f instanceof Initializer ? "initializer" : "field";
                        c.enclosingStatic = f.isStatic();
                        return true;
                    }
                }
            }
            return true;
        }

        private InternalExtendedCompletionContext extended() {
            if (!(context instanceof InternalCompletionContext ic)) return null;
            return getField(InternalCompletionContext.class, ic, "extendedContext");
        }

        // ── Visible elements (getVisibleElements without the Java model) ────

        private void computeVisibleElements() {
            InternalExtendedCompletionContext ext = extended();
            if (ext == null || !(context instanceof InternalCompletionContext ic)) return;
            Scope scope = getField(InternalExtendedCompletionContext.class, ext, "assistScope");
            if (scope == null) return;
            ObjectVector locals = ic.getVisibleLocalVariables();
            ObjectVector fields = ic.getVisibleFields();
            ObjectVector methods = ic.getVisibleMethods();
            if (locals == null || fields == null || methods == null) return;
            for (CompletionProposal p : raw) {
                int kind = p.getKind();
                if (kind != CompletionProposal.METHOD_REF && kind != CompletionProposal.CONSTRUCTOR_INVOCATION
                        && kind != CompletionProposal.METHOD_REF_WITH_CASTED_RECEIVER) continue;
                if (p.getSignature() == null) continue;
                char[][] types;
                try {
                    types = Signature.getParameterTypes(fix83600(p.getSignature()));
                } catch (IllegalArgumentException e) {
                    continue;
                }
                for (char[] t : types) {
                    String sig = new String(t);
                    if (result.visibleElements.containsKey(sig)) continue;
                    result.visibleElements.put(sig, assignable(ext, scope, sig, locals, fields, methods));
                }
            }
        }

        private List<VisibleElement> assignable(InternalExtendedCompletionContext ext, Scope scope, String typeSignature,
                ObjectVector locals, ObjectVector fields, ObjectVector methods) {
            List<VisibleElement> out = new ArrayList<>();
            TypeBinding assignable = typeFromSignature(ext, typeSignature, scope);
            if (assignable == null) return out;
            for (int i = 0; i < locals.size(); i++) {
                LocalVariableBinding b = (LocalVariableBinding) locals.elementAt(i);
                if (b.type == null || !b.type.isCompatibleWith(assignable)) continue;
                AbstractVariableDeclaration local = b.declaration;
                if (local == null) continue;
                VisibleElement e = new VisibleElement();
                e.kind = 0;
                e.name = new String(local.name);
                if (local.type == null || local.type.isTypeNameVar(b.declaringScope)) {
                    e.typeSignature = Signature.createTypeSignature(b.type.signableName(), true);
                } else {
                    e.typeSignature = org.eclipse.jdt.internal.core.util.Util.typeSignature(local.type);
                }
                out.add(e);
            }
            for (int i = 0; i < fields.size(); i++) {
                FieldBinding b = (FieldBinding) fields.elementAt(i);
                if (!b.type.isCompatibleWith(assignable)) continue;
                VisibleElement e = new VisibleElement();
                e.kind = 1;
                e.name = new String(b.name);
                e.inherited = !isEnclosingType(scope, b.declaringClass);
                if (scope.isDefinedInSameUnit(b.declaringClass)) {
                    FieldDeclaration decl = b.sourceField();
                    e.typeSignature = decl != null && decl.type != null
                            ? Signature.createTypeSignature(CharOperation.concatWith(decl.type.getParameterizedTypeName(), '.'), false)
                            : resolvedSignature(b.type);
                } else {
                    e.typeSignature = resolvedSignature(b.type);
                }
                out.add(e);
            }
            for (int i = 0; i < methods.size(); i++) {
                MethodBinding b = (MethodBinding) methods.elementAt(i);
                if (!b.returnType.isCompatibleWith(assignable)) continue;
                VisibleElement e = new VisibleElement();
                e.kind = 2;
                e.name = new String(b.selector);
                e.inherited = !isEnclosingType(scope, b.declaringClass);
                e.parameterCount = b.parameters.length;
                if (scope.isDefinedInSameUnit(b.declaringClass)) {
                    AbstractMethodDeclaration decl = b.sourceMethod();
                    if (decl instanceof org.eclipse.jdt.internal.compiler.ast.MethodDeclaration md && md.returnType != null) {
                        e.returnType = Signature.createTypeSignature(
                                CharOperation.concatWith(md.returnType.getParameterizedTypeName(), '.'), false);
                    } else {
                        e.returnType = resolvedSignature(b.returnType);
                    }
                } else {
                    e.returnType = resolvedSignature(b.returnType);
                }
                e.typeSignature = e.returnType;
                out.add(e);
            }
            return out;
        }

        private static boolean isEnclosingType(Scope scope, ReferenceBinding declaring) {
            org.eclipse.jdt.internal.compiler.lookup.SourceTypeBinding enclosing = scope.enclosingSourceType();
            return enclosing != null && declaring != null && TypeBinding.equalsEquals(enclosing, declaring.erasure());
        }

        private static String resolvedSignature(TypeBinding type) {
            char[] sig = type.genericTypeSignature();
            return new String(CharOperation.replaceOnCopy(sig, '/', '.'));
        }

        private TypeBinding typeFromSignature(InternalExtendedCompletionContext ext, String signature, Scope scope) {
            try {
                Method m = InternalExtendedCompletionContext.class.getDeclaredMethod("getTypeFromSignature", String.class,
                        Scope.class);
                m.setAccessible(true);
                return (TypeBinding) m.invoke(ext, signature, scope);
            } catch (ReflectiveOperationException e) {
                return null;
            }
        }

        // ── Type arguments for constructor proposals ─────────────────────────

        /** Port of jdt.ls {@code CompletionProposalReplacementProvider.computeTypeArgumentProposals}. */
        private List<String> computeTypeArgumentProposals(CompletionProposal proposal) {
            LookupEnvironment lookup = getField(CompletionEngine.class, engine, "lookupEnvironment");
            if (lookup == null || proposal.getSignature() == null) return List.of();
            String fqn = stripSignatureToFQN(new String(proposal.getSignature()));
            ReferenceBinding type = lookup.getType(CharOperation.splitOn('.', fqn.toCharArray()));
            if (type == null) {
                // member types: try progressively shorter package prefixes
                type = findMemberType(lookup, fqn);
            }
            if (type == null || !type.isValidBinding()) return List.of();
            TypeVariableBinding[] parameters = type.typeVariables();
            if (parameters == null || parameters.length == 0) return List.of();
            String[] arguments = new String[parameters.length];
            TypeBinding expected = expectedTypeForGenericParameters();
            if (expected != null && expected.isParameterizedType()) {
                if (proposal instanceof InternalCompletionProposal icp && !icp.isCompatibleProposal()) {
                    return List.of();
                }
                ReferenceBinding expectedType = (ReferenceBinding) expected.erasure();
                List<ReferenceBinding> path = computeInheritancePath(type, expectedType);
                if (path == null) return List.of();
                int[] indices = new int[parameters.length];
                for (int i = 0; i < parameters.length; i++) {
                    indices[i] = mapTypeParameterIndex(path, path.size() - 1, i);
                }
                TypeBinding[] typeArguments = ((ParameterizedTypeBinding) expected).arguments;
                for (int i = 0; i < parameters.length; i++) {
                    if (indices[i] != -1 && typeArguments != null && indices[i] < typeArguments.length) {
                        arguments[i] = computeTypeProposal(typeArguments[indices[i]], parameters[i]);
                    }
                }
            }
            for (int i = 0; i < arguments.length; i++) {
                if (arguments[i] == null) arguments[i] = computeTypeProposal(parameters[i]);
            }
            return List.of(arguments);
        }

        private ReferenceBinding findMemberType(LookupEnvironment lookup, String fqn) {
            String[] parts = fqn.split("\\.");
            for (int split = parts.length - 1; split >= 1; split--) {
                char[][] top = new char[split][];
                for (int i = 0; i < split; i++) top[i] = parts[i].toCharArray();
                ReferenceBinding t = lookup.getType(top);
                if (t == null || !t.isValidBinding()) continue;
                for (int i = split; i < parts.length && t != null; i++) {
                    t = t.getMemberType(parts[i].toCharArray());
                }
                if (t != null) return t;
            }
            return null;
        }

        private TypeBinding expectedTypeForGenericParameters() {
            char[][] keys = context == null ? null : context.getExpectedTypesKeys();
            if (keys == null || keys.length == 0) return null;
            TypeBinding[] expected = getField(CompletionEngine.class, engine, "expectedTypes");
            Integer ptr = getField(CompletionEngine.class, engine, "expectedTypesPtr");
            if (expected == null || ptr == null || ptr < 0) return null;
            return expected[0];
        }

        private static String computeTypeProposal(TypeVariableBinding parameter) {
            TypeBinding[] bounds = parameter.allUpperBounds();
            // ITypeParameter.getBounds(): declared bounds only (no implicit Object)
            List<TypeBinding> declared = new ArrayList<>();
            if (parameter.firstBound != null) {
                if (parameter.superclass != null && TypeBinding.equalsEquals(parameter.firstBound, parameter.superclass)) {
                    declared.add(parameter.superclass);
                }
                if (parameter.superInterfaces != null) {
                    for (ReferenceBinding i : parameter.superInterfaces) declared.add(i);
                }
            }
            if (bounds != null && declared.size() == 1
                    && !"java.lang.Object".equals(new String(declared.get(0).erasure().readableName()))) {
                return Signature.getSimpleName(new String(declared.get(0).erasure().readableName()));
            }
            return new String(parameter.sourceName);
        }

        private static String computeTypeProposal(TypeBinding binding, TypeVariableBinding parameter) {
            String name = typeQualifiedName(binding);
            if (binding instanceof WildcardBinding w) {
                if (w.boundKind == org.eclipse.jdt.internal.compiler.ast.Wildcard.EXTENDS && w.bound != null) {
                    return typeName(w.bound);
                }
                return computeTypeProposal(parameter);
            }
            return name;
        }

        /** DOM {@code ITypeBinding.getName()}. */
        private static String typeName(TypeBinding b) {
            return new String(b.sourceName());
        }

        /** jdt.ls {@code TypeProposalUtils.getTypeQualifiedName}. */
        private static String typeQualifiedName(TypeBinding type) {
            List<String> list = new ArrayList<>();
            createName(type, list);
            return String.join(".", list);
        }

        private static void createName(TypeBinding type, List<String> list) {
            TypeBinding base = type.isArrayType() ? type.leafComponentType() : type;
            if (!base.isBaseType() && base.id != TypeBinding.NULL.id && base instanceof ReferenceBinding rb) {
                ReferenceBinding declaring = rb.enclosingType();
                if (declaring != null && !(rb instanceof TypeVariableBinding)) {
                    createName(declaring, list);
                }
            }
            if (base instanceof ReferenceBinding rb && rb.isAnonymousType()) {
                list.add("$local$");
            } else {
                list.add(typeName(type));
            }
        }

        private static List<ReferenceBinding> computeInheritancePath(ReferenceBinding sub, ReferenceBinding sup) {
            if (sup == null) return null;
            if (TypeBinding.equalsEquals(sub.erasure(), sup.erasure())) {
                List<ReferenceBinding> l = new ArrayList<>();
                l.add((ReferenceBinding) sub.erasure());
                return l;
            }
            List<ReferenceBinding> chain = new ArrayList<>();
            if (!findChain((ReferenceBinding) sub.erasure(), (ReferenceBinding) sup.erasure(), chain, new java.util.HashSet<>())) {
                return null;
            }
            // chain is sub → ... → sup; path is sup → ... → sub
            java.util.Collections.reverse(chain);
            return chain;
        }

        private static boolean findChain(ReferenceBinding current, ReferenceBinding target, List<ReferenceBinding> chain,
                java.util.Set<ReferenceBinding> seen) {
            if (!seen.add(current)) return false;
            chain.add(current);
            if (TypeBinding.equalsEquals(current, target)) return true;
            List<ReferenceBinding> supers = new ArrayList<>();
            if (current.superclass() != null) supers.add(current.superclass());
            ReferenceBinding[] ifs = current.superInterfaces();
            if (ifs != null) for (ReferenceBinding i : ifs) supers.add(i);
            for (ReferenceBinding s : supers) {
                if (findChain((ReferenceBinding) s.erasure(), target, chain, seen)) return true;
            }
            chain.remove(chain.size() - 1);
            return false;
        }

        private static int mapTypeParameterIndex(List<ReferenceBinding> path, int pathIndex, int paramIndex) {
            if (pathIndex == 0) return paramIndex;
            ReferenceBinding subType = path.get(pathIndex);
            ReferenceBinding superType = path.get(pathIndex - 1);
            ReferenceBinding superRef = null;
            if (superType.isInterface()) {
                ReferenceBinding[] ifs = subType.superInterfaces();
                if (ifs != null) {
                    for (ReferenceBinding i : ifs) {
                        if (TypeBinding.equalsEquals(i.erasure(), superType)) superRef = i;
                    }
                }
            } else if (subType.superclass() != null && TypeBinding.equalsEquals(subType.superclass().erasure(), superType)) {
                superRef = subType.superclass();
            }
            if (superRef == null) return -1;
            TypeVariableBinding[] params = subType.typeVariables();
            if (paramIndex >= params.length) return -1;
            String paramName = new String(params[paramIndex].sourceName);
            int index = -1;
            if (superRef instanceof ParameterizedTypeBinding ptb && ptb.arguments != null) {
                for (int i = 0; i < ptb.arguments.length; i++) {
                    TypeBinding a = ptb.arguments[i];
                    String simple = a instanceof TypeVariableBinding tv ? new String(tv.sourceName) : new String(a.sourceName());
                    if (simple.equals(paramName)) {
                        index = i;
                        break;
                    }
                }
            }
            if (index == -1) return -1;
            return mapTypeParameterIndex(path, pathIndex - 1, index);
        }
    }

    // ── Signature helpers (ports of jdt.ls SignatureUtil) ────────────────────

    static char[] fix83600(char[] signature) {
        if (signature == null || signature.length < 2) return signature;
        return Signature.removeCapture(signature);
    }

    static String stripSignatureToFQN(String signature) {
        signature = Signature.getTypeErasure(signature);
        return Signature.toString(signature);
    }

    // ── Java project stand-in ────────────────────────────────────────────────

    /** The bridge options plus the code assist options jdt.ls sets in
     * {@code PreferenceManager.initializeJavaCoreOptions}. */
    static Map<String, String> assistOptions(String sourceLevel) {
        Map<String, String> options = new java.util.HashMap<>(BridgeOptions.map(sourceLevel));
        options.put(org.eclipse.jdt.core.JavaCore.CODEASSIST_VISIBILITY_CHECK, org.eclipse.jdt.core.JavaCore.ENABLED);
        options.put(org.eclipse.jdt.core.JavaCore.CODEASSIST_SUBWORD_MATCH, org.eclipse.jdt.core.JavaCore.DISABLED);
        return options;
    }

    static IJavaProject javaProjectProxy(Map<String, String> options) {
        InvocationHandler h = (proxy, method, args) -> {
            switch (method.getName()) {
                case "getOption":
                    return options.get((String) args[0]);
                case "getOptions":
                    return new HashMap<>(options);
                case "getElementName":
                    return "jdtls-rust";
                case "getAllPackageFragmentRoots":
                case "getPackageFragmentRoots":
                    return new IPackageFragmentRoot[0];
                case "exists":
                    return Boolean.TRUE;
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                case "toString":
                    return "IJavaProject(jdtls-rust)";
                default:
                    Class<?> rt = method.getReturnType();
                    if (rt == boolean.class) return Boolean.FALSE;
                    if (rt == int.class) return 0;
                    if (rt == long.class) return 0L;
                    if (rt.isArray()) return java.lang.reflect.Array.newInstance(rt.getComponentType(), 0);
                    return null;
            }
        };
        return (IJavaProject) Proxy.newProxyInstance(CodeAssistService.class.getClassLoader(), new Class<?>[] { IJavaProject.class }, h);
    }

    // ── Reflection & conversion helpers ──────────────────────────────────────

    @SuppressWarnings("unchecked")
    static <T> T getField(Class<?> owner, Object target, String name) {
        try {
            Field f = owner.getDeclaredField(name);
            f.setAccessible(true);
            return (T) f.get(target);
        } catch (ReflectiveOperationException e) {
            return null;
        }
    }

    static void setField(Class<?> owner, Object target, String name, Object value) {
        try {
            Field f = owner.getDeclaredField(name);
            f.setAccessible(true);
            f.set(target, value);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Cannot set " + name, e);
        }
    }

    static String str(char[] c) {
        return c == null ? null : new String(c);
    }

    static List<String> strings(char[][] c) {
        if (c == null) return null;
        List<String> l = new ArrayList<>(c.length);
        for (char[] x : c) l.add(x == null ? null : new String(x));
        return l;
    }

    /** Eclipse {@code StringMatcher(pattern, ignoreCase=false, ignoreWildCards=false).match(text)}. */
    static final class StringMatcher {
        static boolean match(String pattern, String text) {
            return CharOperation.match(pattern.toCharArray(), text.toCharArray(), true);
        }
    }

    /** Parameter names from class file debug attributes. */
    static final class BinaryNames {
        static char[][] find(CodeAssistEnvironment env, String declaringType, char[] selector, char[] signature) {
            org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer answer = env.findType(
                    CharOperation.splitOn('.', declaringType.toCharArray()), (char[]) null);
            if (answer == null) {
                // member type: Outer.Inner → Outer$Inner
                int dot = declaringType.lastIndexOf('.');
                while (answer == null && dot > 0) {
                    String candidate = declaringType.substring(0, dot) + "$" + declaringType.substring(dot + 1);
                    answer = env.findType(CharOperation.splitOn('.', candidate.toCharArray()), (char[]) null);
                    dot = declaringType.lastIndexOf('.', dot - 1);
                    if (answer == null) declaringType = candidate;
                }
            }
            if (answer == null || !answer.isBinaryType()) return null;
            org.eclipse.jdt.internal.compiler.env.IBinaryMethod[] methods = answer.getBinaryType().getMethods();
            if (methods == null) return null;
            int count;
            try {
                count = Signature.getParameterCount(signature);
            } catch (IllegalArgumentException e) {
                return null;
            }
            String[] wanted = erasedParams(signature);
            org.eclipse.jdt.internal.compiler.env.IBinaryMethod match = null;
            for (org.eclipse.jdt.internal.compiler.env.IBinaryMethod m : methods) {
                if (!CharOperation.equals(m.getSelector(), selector)) continue;
                char[] desc = m.getGenericSignature() != null ? m.getGenericSignature() : m.getMethodDescriptor();
                String[] params = erasedParams(CharOperation.replaceOnCopy(desc, '/', '.'));
                if (params == null || params.length != count) continue;
                if (wanted != null && java.util.Arrays.equals(wanted, params)) {
                    match = m;
                    break;
                }
                if (match == null) match = m;
            }
            if (match == null) return null;
            char[][] names = match.getArgumentNames();
            if (names != null && names.length == count) return names;
            return null;
        }

        private static String[] erasedParams(char[] signature) {
            try {
                char[][] p = Signature.getParameterTypes(signature);
                String[] out = new String[p.length];
                for (int i = 0; i < p.length; i++) out[i] = new String(Signature.getTypeErasure(p[i]));
                return out;
            } catch (IllegalArgumentException e) {
                return null;
            }
        }
    }

    @SuppressWarnings("unused")
    private static boolean isSourceType(TypeBinding b) {
        return b instanceof SourceTypeBinding;
    }

    @SuppressWarnings("unused")
    private static ASTNode none() {
        return null;
    }
}
