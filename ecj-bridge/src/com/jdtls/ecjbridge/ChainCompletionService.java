package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.jdt.core.CompletionContext;
import org.eclipse.jdt.core.CompletionProposal;
import org.eclipse.jdt.core.CompletionRequestor;
import org.eclipse.jdt.core.Flags;
import org.eclipse.jdt.core.IJavaProject;
import org.eclipse.jdt.core.Signature;
import org.eclipse.jdt.core.compiler.CharOperation;
import org.eclipse.jdt.internal.codeassist.CompletionEngine;
import org.eclipse.jdt.internal.codeassist.InternalCompletionContext;
import org.eclipse.jdt.internal.codeassist.InternalExtendedCompletionContext;
import org.eclipse.jdt.internal.compiler.ast.AbstractMethodDeclaration;
import org.eclipse.jdt.internal.compiler.ast.AbstractVariableDeclaration;
import org.eclipse.jdt.internal.compiler.ast.Argument;
import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.ast.FieldDeclaration;
import org.eclipse.jdt.internal.compiler.ast.Initializer;
import org.eclipse.jdt.internal.compiler.ast.MethodDeclaration;
import org.eclipse.jdt.internal.compiler.ast.TypeDeclaration;
import org.eclipse.jdt.internal.compiler.env.IBinaryField;
import org.eclipse.jdt.internal.compiler.env.IBinaryMethod;
import org.eclipse.jdt.internal.compiler.env.IBinaryType;
import org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer;
import org.eclipse.jdt.internal.compiler.lookup.FieldBinding;
import org.eclipse.jdt.internal.compiler.lookup.LocalVariableBinding;
import org.eclipse.jdt.internal.compiler.lookup.LookupEnvironment;
import org.eclipse.jdt.internal.compiler.lookup.MethodBinding;
import org.eclipse.jdt.internal.compiler.lookup.ReferenceBinding;
import org.eclipse.jdt.internal.compiler.lookup.SourceTypeBinding;
import org.eclipse.jdt.internal.compiler.lookup.TypeBinding;
import org.eclipse.jdt.internal.compiler.lookup.TypeVariableBinding;
import org.eclipse.jdt.internal.compiler.util.ObjectVector;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/**
 * The data side of jdt.ls {@code ChainCompletionProposalComputer}: the
 * {@code ChainFinder} search of jdt.core.manipulation ({@code ChainElement},
 * {@code ChainElementAnalyzer}) over the types JDT's completion engine sees,
 * with the Java model's view of members (declaration order, signatures as
 * {@code IField}/{@code IMethod} report them, source supertypes as declared).
 * Rust turns the chains into completion proposals.
 */
final class ChainCompletionService {

    // ── Output ───────────────────────────────────────────────────────────────

    static final class Result {
        /** {@code shouldPerformCompletionOnExpectedType}. */
        public boolean perform;
        /** {@code cu.getTypes()[0].getSourceRange().getOffset()}. */
        public int firstTypeOffset = -1;
        public List<ChainOut> chains = new ArrayList<>();
    }

    static final class ChainOut {
        public int expectedDimensions;
        public List<ElementOut> elements = new ArrayList<>();
    }

    static final class ElementOut {
        /** METHOD, FIELD, LOCAL_VARIABLE or TYPE. */
        public String kind;
        public String name;
        /** {@code getDeclaringType().getFullyQualifiedName()}; the type itself for TYPE. */
        public String declaringType;
        /** Fields and locals: {@code getTypeSignature()}. */
        public String typeSignature;
        /** Methods: {@code getReturnType()}, {@code getParameterTypes()}, {@code getParameterNames()}. */
        public String returnType;
        public List<String> parameterTypes;
        public List<String> parameterNames;
        public int dimension;
        public boolean requiresThis;
    }

    // ── Chain model (ChainType, ChainElement) ────────────────────────────────

    static final int METHOD = 0, FIELD = 1, LOCAL_VARIABLE = 2, TYPE = 3;

    /** {@code ChainType}: a type, or a "primitive" (base type, type variable or unresolved) signature. */
    static final class ChainType {
        final TypeInfo type;
        final String primitiveType;
        final int dimension;

        ChainType(TypeInfo type) {
            this.type = type;
            this.primitiveType = null;
            this.dimension = 0;
        }

        ChainType(String primitiveType, int dimension) {
            this.type = null;
            this.primitiveType = primitiveType;
            this.dimension = dimension;
        }

        @Override
        public String toString() {
            return type != null ? type.fqn : primitiveType;
        }
    }

    /** A member as the Java model reports it ({@code IField}, {@code IMethod}, {@code ILocalVariable}, {@code IType}). */
    static final class Element {
        int kind;
        String name;
        TypeInfo declaring;
        int flags;
        boolean constructor;
        /** Fields/locals: type signature; methods: return type signature. */
        String signature;
        List<String> parameterTypes = List.of();
        List<String> parameterNames = List.of();
        /** The resolved (erased) type of the field/local or the method's return type, if known. */
        TypeBinding resolvedType;
        boolean resolvedTypeKnown;
        String key;

        // ChainElement
        ChainType returnType;
        int dimension;

        String fqnForTypeElement() {
            return declaring.fqn;
        }
    }

    /** {@code IType}. */
    static final class TypeInfo {
        final ReferenceBinding binding;
        final String fqn;
        final String simpleName;
        final String packageName;
        List<Element> methods;
        List<Element> fields;
        List<TypeInfo> supertypes; // superclass (if the model reports one) then interfaces
        TypeInfo superclass;
        boolean superclassComputed;
        boolean hasSuperclassSignature;

        TypeInfo(ReferenceBinding binding) {
            this.binding = binding;
            this.fqn = CharOperation.toString(binding.compoundName);
            this.simpleName = new String(binding.sourceName());
            this.packageName = binding.getPackage() == null ? "" : CharOperation.toString(binding.getPackage().compoundName);
        }
    }

    // ── State ────────────────────────────────────────────────────────────────

    private final CodeAssistEnvironment env;
    private final LookupEnvironment lookup;
    private final Map<ReferenceBinding, TypeInfo> types = new HashMap<>();
    private final Map<String, TypeInfo> typesByName = new HashMap<>();
    private final List<String> excludedTypes;
    private final long deadline;

    private ChainCompletionService(CodeAssistEnvironment env, LookupEnvironment lookup, List<String> excludedTypes, long deadline) {
        this.env = env;
        this.lookup = lookup;
        this.excludedTypes = excludedTypes;
        this.deadline = deadline;
    }

    // ── Entry point ──────────────────────────────────────────────────────────

    static Object compute(JsonObject q, Map<String, String> files, List<String> classpath, String sourceLevel, String uri,
            int offset) {
        Result result = new Result();
        String source = files.get(uri);
        if (source == null) source = "";
        Set<String> testUris = new HashSet<>(CodeAssistService.jsonStrings(q, "testUris"));
        String unitPackage = q.has("unitPackage") && !q.get("unitPackage").isJsonNull() ? q.get("unitPackage").getAsString() : null;
        boolean isTest = testUris.contains(uri);
        CodeAssistEnvironment env = CodeAssistEnvironment.create(files, testUris, classpath, sourceLevel, uri, !isTest);
        Map<String, String> options = CodeAssistService.assistOptions(sourceLevel);
        IJavaProject project = CodeAssistService.javaProjectProxy(options);
        CompletionContext[] contextRef = new CompletionContext[1];
        CompletionRequestor requestor = new CompletionRequestor() {
            {
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
            }
        };
        CodeAssistService.installNoIndexManager();
        CompletionEngine engine = new CompletionEngine(env, requestor, options, project, null, new NullProgressMonitor());
        CodeAssistService.setField(CompletionEngine.class, engine, "noCacheNameEnvironment", env);
        engine.complete(new InMemoryCompilationUnit(uri, source).withPackage(unitPackage), offset, 0, null);
        CompletionContext context = contextRef[0];
        if (!(context instanceof InternalCompletionContext ic)) return result;
        InternalExtendedCompletionContext ext = CodeAssistService.getField(InternalCompletionContext.class, ic, "extendedContext");
        if (ext == null) return result;

        List<String> excluded = new ArrayList<>();
        for (String t : (q.has("ignoreTypes") ? q.get("ignoreTypes").getAsString() : "").split("\\|")) {
            excluded.add("L" + t.replace('.', '/'));
        }
        int maxChains = q.has("maxChains") ? q.get("maxChains").getAsInt() : 20;
        int minDepth = q.has("minDepth") ? q.get("minDepth").getAsInt() : 2;
        int maxDepth = q.has("maxDepth") ? q.get("maxDepth").getAsInt() : 3;
        long timeout = q.has("timeout") ? q.get("timeout").getAsLong() : 1;
        ChainCompletionService service = new ChainCompletionService(env, engine.lookupEnvironment, excluded,
                System.nanoTime() + timeout * 1_000_000_000L);
        try {
            service.run(q, context, ic, ext, result, uri, maxChains, minDepth, maxDepth);
        } catch (RuntimeException e) {
            // best effort, like the computer's swallowed failures
        }
        return result;
    }

    private void run(JsonObject q, CompletionContext context, InternalCompletionContext ic, InternalExtendedCompletionContext ext,
            Result result, String uri, int maxChains, int minDepth, int maxDepth) {
        CompilationUnitDeclaration unit = CodeAssistService.getField(InternalExtendedCompletionContext.class, ext,
                "compilationUnitDeclaration");
        if (unit != null && unit.types != null && unit.types.length > 0) {
            result.firstTypeOffset = unit.types[0].declarationSourceStart;
        }
        if (!shouldPerformCompletionOnExpectedType(context)) return;
        result.perform = true;

        // cu.findPrimaryType()
        TypeInfo invocationType = null;
        String fileName = uri.substring(uri.lastIndexOf('/') + 1);
        String mainName = fileName.endsWith(".java") ? fileName.substring(0, fileName.length() - 5) : fileName;
        if (unit != null && unit.types != null) {
            for (TypeDeclaration t : unit.types) {
                if (t.binding != null && mainName.equals(new String(t.name))) {
                    invocationType = typeInfo(t.binding);
                }
            }
        }

        String token = context.getToken() != null && context.getToken().length > 0 ? new String(context.getToken()) : null;
        List<ChainType> expectedTypes = resolveBindingsForExpectedTypes(context);
        Finder mainFinder = new Finder(expectedTypes, invocationType, token);
        Finder contextFinder = new Finder(expectedTypes, invocationType, token);
        try {
            List<Element> entrypoints = findEntrypoints(q, context, ic);
            if (!entrypoints.isEmpty()) {
                mainFinder.startChainSearch(entrypoints, maxChains, minDepth, maxDepth);
            }
        } catch (RuntimeException e) {
            // the finder's future failed: keep the chains found so far
        }
        try {
            List<Element> contextEntrypoint = computeContextEntrypoint(expectedTypes);
            if (!contextEntrypoint.isEmpty()) {
                contextFinder.startChainSearch(contextEntrypoint, maxChains, 1, 2);
            }
        } catch (RuntimeException e) {
            // ignore
        }
        List<List<Element>> found = new ArrayList<>();
        found.addAll(mainFinder.chains);
        found.addAll(contextFinder.chains);
        List<Integer> dims = new ArrayList<>();
        dims.addAll(mainFinder.chainDimensions);
        dims.addAll(contextFinder.chainDimensions);
        for (int i = 0; i < found.size(); i++) {
            ChainOut out = new ChainOut();
            out.expectedDimensions = dims.get(i);
            for (Element e : found.get(i)) out.elements.add(toOut(e));
            result.chains.add(out);
        }
    }

    private static ElementOut toOut(Element e) {
        ElementOut o = new ElementOut();
        o.kind = switch (e.kind) {
            case METHOD -> "METHOD";
            case FIELD -> "FIELD";
            case LOCAL_VARIABLE -> "LOCAL_VARIABLE";
            default -> "TYPE";
        };
        o.name = e.name;
        o.declaringType = e.declaring == null ? null : e.declaring.fqn;
        if (e.kind == METHOD) {
            o.returnType = e.signature;
            o.parameterTypes = e.parameterTypes;
            o.parameterNames = e.parameterNames;
        } else if (e.kind != TYPE) {
            o.typeSignature = e.signature;
        }
        o.dimension = e.dimension;
        return o;
    }

    // ── ChainCompletionProposalComputer ──────────────────────────────────────

    private boolean shouldPerformCompletionOnExpectedType(CompletionContext context) {
        if (context.getToken() != null && CharOperation.equals(context.getToken(), "new".toCharArray())) return false;
        if (context.getTokenLocation() == CompletionContext.TL_CONSTRUCTOR_START) return false;
        char[][] expected = context.getExpectedTypesSignatures();
        if (expected == null || expected.length == 0) return false;
        String fqn = stripSignatureToFQN(new String(expected[0]));
        // ast.resolveWellKnownType(fqn)
        switch (fqn) {
            case "boolean", "char", "byte", "short", "int", "long", "float", "double", "void",
                    "java.lang.Boolean", "java.lang.Byte", "java.lang.Character", "java.lang.Short", "java.lang.Double",
                    "java.lang.Float", "java.lang.Integer", "java.lang.Long", "java.lang.String", "java.lang.Object":
                return false;
            case "java.lang.StringBuffer", "java.lang.Throwable", "java.lang.Exception", "java.lang.RuntimeException",
                    "java.lang.Error", "java.lang.Class", "java.lang.Cloneable", "java.io.Serializable", "java.lang.Void",
                    "java.lang.AssertionError":
                return true;
            default:
                return findType(fqn) != null;
        }
    }

    private List<ChainType> resolveBindingsForExpectedTypes(CompletionContext context) {
        List<ChainType> out = new LinkedList<>();
        char[][] expected = context.getExpectedTypesSignatures();
        if (expected == null || expected.length == 0) return out;
        for (char[] sig : expected) {
            out.add(new ChainType(findType(stripSignatureToFQN(new String(sig)))));
        }
        return out;
    }

    private List<Element> findEntrypoints(JsonObject q, CompletionContext context, InternalCompletionContext ic) {
        List<Element> entrypoints = new LinkedList<>();
        Set<String> processed = new HashSet<>();
        String prefix = context.getToken() == null ? "null" : new String(context.getToken());
        if (q.has("entrypoints") && q.get("entrypoints").isJsonArray()) {
            for (JsonElement je : q.getAsJsonArray("entrypoints")) {
                JsonObject p = je.getAsJsonObject();
                Element e = resolveJavaElement(p);
                if (e != null && e.name.startsWith(prefix) && !isFromExcludedType(e)) {
                    initializeReturnType(e);
                    entrypoints.add(e);
                    processed.add(e.key);
                }
            }
        }
        ObjectVector locals = ic.getVisibleLocalVariables();
        ObjectVector fields = ic.getVisibleFields();
        ObjectVector methods = ic.getVisibleMethods();
        List<Element> visible = new ArrayList<>();
        if (locals != null) {
            for (int i = 0; i < locals.size(); i++) {
                Element e = localElement((LocalVariableBinding) locals.elementAt(i));
                if (e != null) visible.add(e);
            }
        }
        if (fields != null) {
            for (int i = 0; i < fields.size(); i++) {
                Element e = fieldElement((FieldBinding) fields.elementAt(i));
                if (e != null) visible.add(e);
            }
        }
        if (methods != null) {
            for (int i = 0; i < methods.size(); i++) {
                Element e = methodElement((MethodBinding) methods.elementAt(i));
                if (e != null) visible.add(e);
            }
        }
        for (Element e : visible) {
            if (!processed.contains(e.key) && e.name.startsWith(prefix) && !isFromExcludedType(e)) {
                initializeReturnType(e);
                entrypoints.add(e);
            }
        }
        return entrypoints;
    }

    /** {@code JDTUtils.resolveField} / {@code resolveMethod}. */
    private Element resolveJavaElement(JsonObject p) {
        int kind = p.get("kind").getAsInt();
        String declSig = p.has("declarationSignature") && !p.get("declarationSignature").isJsonNull()
                ? p.get("declarationSignature").getAsString() : null;
        String name = p.has("name") && !p.get("name").isJsonNull() ? p.get("name").getAsString() : null;
        if (declSig == null || name == null) return null;
        TypeInfo type = findType(stripSignatureToFQN(declSig));
        if (type == null) return null;
        if (kind == CompletionProposal.FIELD_REF) {
            for (Element f : fields(type)) {
                if (f.name.equals(name)) return f;
            }
            return null;
        }
        if (kind == CompletionProposal.ANNOTATION_ATTRIBUTE_REF) {
            for (Element m : methods(type)) {
                if (m.name.equals(name) && m.parameterTypes.isEmpty()) return m;
            }
            return null;
        }
        if (kind != CompletionProposal.METHOD_REF) return null;
        String sig = p.has("signature") && !p.get("signature").isJsonNull() ? p.get("signature").getAsString() : null;
        if (sig == null) return null;
        String[] params;
        try {
            params = Signature.getParameterTypes(new String(CodeAssistService.fix83600(sig.toCharArray())));
        } catch (IllegalArgumentException e) {
            return null;
        }
        // JavaModelUtil.findMethod: same name, parameter count and erased simple type names
        for (Element m : methods(type)) {
            if (!m.name.equals(name) || m.constructor || m.parameterTypes.size() != params.length) continue;
            boolean same = true;
            for (int i = 0; i < params.length && same; i++) {
                same = simpleErasure(lowerBound(params[i])).equals(simpleErasure(m.parameterTypes.get(i)));
            }
            if (same) return m;
        }
        return null;
    }

    private static String lowerBound(String sig) {
        if (sig.length() > 0 && (sig.charAt(0) == Signature.C_EXTENDS || sig.charAt(0) == Signature.C_SUPER)) {
            return sig.substring(1);
        }
        if (sig.equals("*")) return "Ljava.lang.Object;";
        return sig;
    }

    private static String simpleErasure(String sig) {
        try {
            return Signature.getSimpleName(Signature.toString(Signature.getTypeErasure(sig)));
        } catch (RuntimeException e) {
            return sig;
        }
    }

    private List<Element> computeContextEntrypoint(List<ChainType> expectedTypes) {
        List<Element> results = new ArrayList<>();
        for (ChainType chainType : expectedTypes) {
            if (chainType.type == null) continue;
            String fqn = chainType.type.fqn;
            if ("java.util.List".equals(fqn) || "java.util.Set".equals(fqn) || "java.util.Map".equals(fqn)) {
                TypeInfo type = findType("java.util.Collections");
                if (type != null) results.add(typeElement(type));
            }
            if ("java.util.stream.Collector".equals(fqn)) {
                TypeInfo type = findType("java.util.stream.Collectors");
                if (type != null) results.add(typeElement(type));
            }
        }
        return results;
    }

    // ── ChainFinder ──────────────────────────────────────────────────────────

    private final class Finder {
        final List<ChainType> expectedTypes;
        final TypeInfo receiverType;
        final String token;
        final List<List<Element>> chains = new LinkedList<>();
        final List<Integer> chainDimensions = new ArrayList<>();
        final Map<String, List<Element>> fieldsAndMethodsCache = new HashMap<>();
        final Map<String, Boolean> assignableCache = new HashMap<>();

        Finder(List<ChainType> expectedTypes, TypeInfo receiverType, String token) {
            this.expectedTypes = expectedTypes;
            this.receiverType = receiverType;
            this.token = token;
        }

        void startChainSearch(List<Element> entrypoints, int maxChains, int minDepth, int maxDepth) {
            for (ChainType expected : expectedTypes) {
                if (expected != null && !isFromExcludedType(expected)) {
                    int expectedDimension = expected.dimension > 0 ? expected.dimension : 0;
                    searchChainsForExpectedType(expected, expectedDimension, entrypoints, maxChains, minDepth, maxDepth);
                }
            }
        }

        private void searchChainsForExpectedType(ChainType expectedType, int expectedDimensions, List<Element> entrypoints,
                int maxChains, int minDepth, int maxDepth) {
            LinkedList<LinkedList<Element>> incompleteChains = new LinkedList<>();
            for (Element entrypoint : entrypoints) {
                LinkedList<Element> chain = new LinkedList<>();
                chain.add(entrypoint);
                incompleteChains.add(chain);
            }
            while (!incompleteChains.isEmpty()) {
                if (System.nanoTime() > deadline) throw new IllegalStateException("chain search timed out");
                LinkedList<Element> chain = incompleteChains.poll();
                Element edge = chain.getLast();
                Element start = chain.getFirst();
                if (isValidEndOfChain(edge, start, expectedType, expectedDimensions)) {
                    if (chain.size() >= minDepth) {
                        chains.add(chain);
                        chainDimensions.add(expectedDimensions);
                        if (chains.size() == maxChains) break;
                    }
                    continue;
                }
                if (chain.size() < maxDepth && incompleteChains.size() <= 50000) {
                    searchDeeper(chain, incompleteChains, edge.returnType);
                }
            }
        }

        private boolean isValidEndOfChain(Element edge, Element start, ChainType expectedType, int expectedDimension) {
            if (edge.kind == TYPE) return false;
            if (token != null && !token.isBlank() && !CharOperation.subWordMatch(token.toCharArray(), edge.name.toCharArray())
                    && !CharOperation.subWordMatch(token.toCharArray(), start.name.toCharArray())) {
                return false;
            }
            if (edge.returnType.primitiveType != null) {
                return edge.returnType.primitiveType.equals(expectedType.primitiveType);
            }
            if (expectedType.primitiveType != null) {
                return expectedType.primitiveType.equals(edge.returnType.primitiveType);
            }
            String cacheKey = edge.key + expectedType;
            Boolean isAssignable = assignableCache.get(cacheKey);
            if (isAssignable == null) {
                isAssignable = isAssignable(edge, expectedType.type, expectedDimension);
                assignableCache.put(cacheKey, isAssignable);
            }
            return isAssignable;
        }

        private void searchDeeper(LinkedList<Element> chain, List<LinkedList<Element>> incompleteChains, ChainType currentlyVisitedType) {
            boolean staticOnly = chain.getLast().kind == TYPE;
            for (Element element : findAllFieldsAndMethods(currentlyVisitedType, staticOnly)) {
                if (!containsElement(chain, element)) {
                    @SuppressWarnings("unchecked")
                    LinkedList<Element> copy = (LinkedList<Element>) chain.clone();
                    copy.add(element);
                    incompleteChains.add(copy);
                }
            }
        }

        private List<Element> findAllFieldsAndMethods(ChainType chainElementType, boolean staticOnly) {
            String key = chainElementType + Boolean.toString(staticOnly);
            List<Element> cached = fieldsAndMethodsCache.get(key);
            if (cached == null) {
                cached = new LinkedList<>();
                for (Element e : findFieldsAndMethods(chainElementType, receiverType, staticOnly)) {
                    if (!isFromExcludedType(e)) {
                        initializeReturnType(e);
                        cached.add(e);
                    }
                }
                fieldsAndMethodsCache.put(key, cached);
            }
            return cached;
        }
    }

    private static boolean containsElement(List<Element> chain, Element e) {
        for (Element c : chain) {
            if (c.key.equals(e.key)) return true;
        }
        return false;
    }

    private boolean isFromExcludedType(Element element) {
        if (element.kind == TYPE) return excludedTypes.contains(element.declaring.fqn);
        return excludedTypes.contains(element.name);
    }

    private boolean isFromExcludedType(ChainType type) {
        if (type.type != null) return excludedTypes.contains(type.type.fqn);
        return excludedTypes.contains(type.primitiveType);
    }

    // ── ChainElementAnalyzer ─────────────────────────────────────────────────

    private Iterable<Element> findFieldsAndMethods(ChainType type, TypeInfo receiverType, boolean staticOnly) {
        Map<String, Element> tmp = new LinkedHashMap<>();
        for (TypeInfo cur : findAllSupertypesIncludingArgument(type)) {
            for (Element method : methods(cur)) {
                boolean isStatic = Flags.isStatic(method.flags);
                boolean relevant = !isVoid(method) && !method.constructor;
                if ((staticOnly ? !isStatic : isStatic) || !relevant) continue;
                if (!methodCanBeSeenBy(method, receiverType)) continue;
                tmp.putIfAbsent(method.key, method);
            }
            for (Element field : fields(cur)) {
                boolean isStatic = Flags.isStatic(field.flags);
                if (staticOnly ? !isStatic : isStatic) continue;
                if (!fieldCanBeSeenBy(field, receiverType)) continue;
                tmp.putIfAbsent(field.key, field);
            }
        }
        return tmp.values();
    }

    private static boolean isVoid(Element m) {
        return "V".equals(m.signature);
    }

    private List<TypeInfo> findAllSupertypesIncludingArgument(ChainType type) {
        if (type.primitiveType != null || type.type == null) return List.of();
        List<TypeInfo> supertypes = new LinkedList<>();
        LinkedList<TypeInfo> queue = new LinkedList<>();
        queue.add(type.type);
        while (!queue.isEmpty()) {
            TypeInfo superType = queue.poll();
            if (superType == null || supertypes.contains(superType)) continue;
            supertypes.add(superType);
            if (hasSuperclassSignature(superType)) queue.add(superclass(superType));
            queue.addAll(superInterfaces(superType));
        }
        return supertypes;
    }

    private boolean isAssignable(Element edge, TypeInfo expectedType, int expectedDimension) {
        if (expectedDimension <= edge.dimension) {
            TypeInfo base = edge.returnType.type;
            if (isAssignmentCompatible(base, expectedType)) return true;
            if (expectedType == null) throw new NullPointerException("expected type");
            LinkedList<TypeInfo> supertypes = new LinkedList<>();
            supertypes.add(base);
            String expectedSignature = expectedType.fqn;
            while (!supertypes.isEmpty()) {
                TypeInfo type = supertypes.poll();
                if (type == null) throw new NullPointerException("supertype");
                if (type.fqn.equals(expectedSignature)) return true;
                if (hasSuperclassSignature(type)) {
                    TypeInfo superclass = superclass(type);
                    if (superclass != null) supertypes.add(superclass);
                    supertypes.addAll(superInterfaces(type));
                }
            }
        }
        return false;
    }

    private boolean isAssignmentCompatible(TypeInfo base, TypeInfo expectedType) {
        LinkedList<TypeInfo> queue = new LinkedList<>();
        queue.add(base);
        while (!queue.isEmpty()) {
            TypeInfo type = queue.poll();
            if (type == null) throw new NullPointerException("supertype");
            if (hasSuperclassSignature(type) && !"java.lang.Object".equals(superclassName(type))) {
                TypeInfo superType = superclass(type);
                if (expectedType == null) throw new NullPointerException("expected type");
                if (expectedType.equals(superType)) return true;
                queue.add(superType);
            }
            List<TypeInfo> interfaces = superInterfaces(type);
            for (TypeInfo iface : interfaces) {
                if (expectedType == null) throw new NullPointerException("expected type");
                if (expectedType.equals(iface)) return true;
                queue.add(iface);
            }
        }
        return false;
    }

    private static boolean methodCanBeSeenBy(Element mb, TypeInfo invocationType) {
        if (Flags.isPublic(mb.flags)) return true;
        if (invocationType == null) throw new NullPointerException("invocation type");
        if (invocationType.equals(mb.declaring)) return true;
        String invocationPackage = invocationType.packageName;
        String methodPackage = mb.declaring.packageName;
        if (Flags.isProtected(mb.flags) && invocationPackage.equals(methodPackage)) return false;
        if (Flags.isPrivate(mb.flags)) return mb.declaring.equals(invocationType);
        return invocationPackage.equals(methodPackage);
    }

    private boolean fieldCanBeSeenBy(Element fb, TypeInfo invocationType) {
        if (Flags.isPublic(fb.flags)) return true;
        if (invocationType == null) throw new NullPointerException("invocation type");
        if (invocationType.equals(fb.declaring)) return true;
        String invocationPackage = invocationType.packageName;
        String fieldPackage = fb.declaring.packageName;
        if (Flags.isProtected(fb.flags)) {
            if (invocationPackage.equals(fieldPackage)) return true;
            TypeInfo currType = invocationType;
            while (hasSuperclassSignature(currType)) {
                currType = superclass(currType);
                if (currType == null) throw new NullPointerException("superclass");
                if (currType.equals(fb.declaring)) return true;
            }
        }
        if (Flags.isPrivate(fb.flags)) return fb.declaring.equals(invocationType);
        return false;
    }

    // ── ChainElement.initializeReturnType ───────────────────────────────────

    private void initializeReturnType(Element e) {
        if (e.returnType != null) return;
        if (e.kind == TYPE) {
            e.returnType = new ChainType(e.declaring);
            e.dimension = 0;
            return;
        }
        String signature = e.signature;
        if (isPrimitive(signature)) {
            e.returnType = new ChainType(signature, 0);
        } else {
            TypeInfo res = null;
            if (e.resolvedTypeKnown) {
                TypeBinding leaf = e.resolvedType == null ? null : e.resolvedType.leafComponentType();
                if (leaf instanceof ReferenceBinding rb && rb.isValidBinding()) res = typeInfo((ReferenceBinding) rb.erasure());
            } else {
                res = findType(stripSignatureToFQN(signature));
            }
            e.returnType = res != null ? new ChainType(res) : new ChainType(signature, 0);
        }
        e.dimension = signature == null ? 0 : Signature.getArrayCount(signature);
    }

    private static boolean isPrimitive(String typeSig) {
        String elementType = Signature.getElementType(typeSig);
        int kind = Signature.getTypeSignatureKind(elementType);
        return kind == Signature.BASE_TYPE_SIGNATURE || kind == Signature.TYPE_VARIABLE_SIGNATURE;
    }

    static String stripSignatureToFQN(String signature) {
        signature = Signature.getTypeErasure(signature);
        signature = Signature.getElementType(signature);
        return Signature.toString(signature);
    }

    // ── The Java model view of types ─────────────────────────────────────────

    /** {@code IJavaProject.findType(fqn)}. */
    private TypeInfo findType(String fqn) {
        if (fqn == null || fqn.isEmpty()) return null;
        if (typesByName.containsKey(fqn)) return typesByName.get(fqn);
        TypeInfo found = null;
        char[][] parts = CharOperation.splitOn('.', fqn.toCharArray());
        for (int top = parts.length; top >= 1 && found == null; top--) {
            try {
                ReferenceBinding t = lookup.getType(Arrays.copyOf(parts, top));
                if (t == null || !t.isValidBinding()) continue;
                for (int i = top; i < parts.length && t != null; i++) {
                    t = t.getMemberType(parts[i]);
                }
                if (t != null && t.isValidBinding()) found = typeInfo(t);
            } catch (RuntimeException e) {
                // keep looking
            }
        }
        typesByName.put(fqn, found);
        return found;
    }

    private TypeInfo typeInfo(ReferenceBinding binding) {
        ReferenceBinding erasure = (ReferenceBinding) binding.erasure();
        return types.computeIfAbsent(erasure, TypeInfo::new);
    }

    private TypeDeclaration sourceDeclaration(TypeInfo t) {
        if (t.binding instanceof SourceTypeBinding stb && stb.scope != null && stb.scope.referenceContext != null) {
            return stb.scope.referenceContext;
        }
        return null;
    }

    private IBinaryType binaryType(TypeInfo t) {
        if (sourceDeclaration(t) != null || t.binding instanceof SourceTypeBinding) return null;
        try {
            NameEnvironmentAnswer answer = env.findType(t.binding.compoundName);
            return answer != null && answer.isBinaryType() ? answer.getBinaryType() : null;
        } catch (RuntimeException e) {
            return null;
        }
    }

    /** {@code getSuperclassTypeSignature() != null}: source types report only an explicit superclass. */
    private boolean hasSuperclassSignature(TypeInfo t) {
        if (!t.superclassComputed) {
            t.superclassComputed = true;
            TypeDeclaration decl = sourceDeclaration(t);
            ReferenceBinding sup = t.binding.superclass();
            if (decl != null) {
                t.hasSuperclassSignature = decl.superclass != null;
            } else if (t.binding instanceof SourceTypeBinding) {
                t.hasSuperclassSignature = sup != null && !t.binding.isInterface() && !t.binding.isEnum() && !t.binding.isRecord()
                        && !CharOperation.equals(sup.compoundName, TypeConstantsHolder.JAVA_LANG_OBJECT);
            } else {
                t.hasSuperclassSignature = sup != null;
            }
            t.superclass = t.hasSuperclassSignature && sup != null && sup.isValidBinding() ? typeInfo(sup) : null;
        }
        return t.hasSuperclassSignature;
    }

    private TypeInfo superclass(TypeInfo t) {
        hasSuperclassSignature(t);
        return t.superclass;
    }

    private String superclassName(TypeInfo t) {
        TypeInfo s = superclass(t);
        return s == null ? null : s.fqn;
    }

    private List<TypeInfo> superInterfaces(TypeInfo t) {
        if (t.supertypes == null) {
            List<TypeInfo> out = new ArrayList<>();
            ReferenceBinding[] ifs = t.binding.superInterfaces();
            if (ifs != null) {
                for (ReferenceBinding i : ifs) {
                    out.add(i != null && i.isValidBinding() ? typeInfo(i) : null);
                }
            }
            t.supertypes = out;
        }
        return t.supertypes;
    }

    private Element typeElement(TypeInfo type) {
        Element e = new Element();
        e.kind = TYPE;
        e.name = type.simpleName;
        e.declaring = type;
        e.key = "T" + type.fqn;
        initializeReturnType(e);
        return e;
    }

    /** {@code IType.getMethods()}: declaration order. */
    private List<Element> methods(TypeInfo t) {
        if (t.methods != null) return t.methods;
        List<Element> out = new ArrayList<>();
        TypeDeclaration decl = sourceDeclaration(t);
        IBinaryType binary = decl == null ? binaryType(t) : null;
        if (decl != null) {
            if (decl.methods != null) {
                for (AbstractMethodDeclaration md : decl.methods) {
                    if (md.isDefaultConstructor() || md.isClinit() || md.binding == null) continue;
                    if (md.isCompactConstructor()) continue;
                    out.add(sourceMethod(t, md));
                }
            }
        } else if (binary != null) {
            IBinaryMethod[] methods = binary.getMethods();
            if (methods != null) {
                for (IBinaryMethod m : methods) {
                    if (m.isClinit()) continue;
                    out.add(binaryMethod(t, m));
                }
            }
        } else {
            for (MethodBinding mb : t.binding.methods()) {
                if (CharOperation.equals(mb.selector, "<clinit>".toCharArray())) continue;
                out.add(methodElementFromBinding(t, mb));
            }
        }
        t.methods = out;
        return out;
    }

    /** {@code IType.getFields()}: declaration order. */
    private List<Element> fields(TypeInfo t) {
        if (t.fields != null) return t.fields;
        List<Element> out = new ArrayList<>();
        TypeDeclaration decl = sourceDeclaration(t);
        IBinaryType binary = decl == null ? binaryType(t) : null;
        if (decl != null) {
            if (decl.fields != null) {
                for (FieldDeclaration f : decl.fields) {
                    if (f instanceof Initializer || f.name == null || f.binding == null) continue;
                    out.add(sourceField(t, decl, f));
                }
            }
        } else if (binary != null) {
            IBinaryField[] fields = binary.getFields();
            if (fields != null) {
                for (IBinaryField f : fields) {
                    if (binary.isRecord() && (f.getModifiers() & ClassFileConstantsHolder.AccStatic) == 0) continue;
                    out.add(binaryField(t, f));
                }
            }
        } else {
            for (FieldBinding fb : t.binding.fields()) {
                out.add(fieldElementFromBinding(t, fb));
            }
        }
        t.fields = out;
        return out;
    }

    private Element sourceMethod(TypeInfo t, AbstractMethodDeclaration md) {
        Element e = new Element();
        e.kind = METHOD;
        e.name = md.isConstructor() ? t.simpleName : new String(md.selector);
        e.declaring = t;
        e.flags = md.binding.modifiers & 0xFFFF;
        e.constructor = md.isConstructor();
        if (md instanceof MethodDeclaration m && m.returnType != null) {
            e.signature = org.eclipse.jdt.internal.core.util.Util.typeSignature(m.returnType);
        } else {
            e.signature = "V";
        }
        List<String> types = new ArrayList<>();
        List<String> names = new ArrayList<>();
        if (md.arguments != null) {
            for (Argument a : md.arguments) {
                types.add(org.eclipse.jdt.internal.core.util.Util.typeSignature(a.type));
                names.add(new String(a.name));
            }
        }
        e.parameterTypes = types;
        e.parameterNames = names;
        e.resolvedType = md.binding.returnType;
        e.resolvedTypeKnown = true;
        e.key = methodKey(t, e);
        return e;
    }

    private Element binaryMethod(TypeInfo t, IBinaryMethod m) {
        Element e = new Element();
        e.kind = METHOD;
        e.constructor = m.isConstructor();
        e.name = e.constructor ? t.simpleName : new String(m.getSelector());
        e.declaring = t;
        e.flags = m.getModifiers();
        char[] sig = m.getGenericSignature() != null ? m.getGenericSignature() : m.getMethodDescriptor();
        String dotted = new String(CharOperation.replaceOnCopy(sig, '/', '.'));
        List<String> types = new ArrayList<>();
        try {
            for (String p : Signature.getParameterTypes(dotted)) types.add(p);
            e.signature = Signature.getReturnType(dotted);
        } catch (IllegalArgumentException ex) {
            String desc = new String(CharOperation.replaceOnCopy(m.getMethodDescriptor(), '/', '.'));
            types.clear();
            for (String p : Signature.getParameterTypes(desc)) types.add(p);
            e.signature = Signature.getReturnType(desc);
        }
        e.parameterTypes = types;
        char[][] argumentNames = m.getArgumentNames();
        List<String> names = new ArrayList<>();
        for (int j = 0; j < types.size(); j++) {
            names.add(argumentNames != null && argumentNames.length >= types.size() ? new String(argumentNames[j]) : "arg" + j);
        }
        e.parameterNames = names;
        e.key = methodKey(t, e);
        return e;
    }

    private Element methodElementFromBinding(TypeInfo t, MethodBinding mb) {
        Element e = new Element();
        e.kind = METHOD;
        e.constructor = mb.isConstructor();
        e.name = e.constructor ? t.simpleName : new String(mb.selector);
        e.declaring = t;
        e.flags = mb.modifiers & 0xFFFF;
        e.signature = resolvedSignature(mb.returnType);
        List<String> types = new ArrayList<>();
        List<String> names = new ArrayList<>();
        for (int i = 0; i < mb.parameters.length; i++) {
            types.add(resolvedSignature(mb.parameters[i]));
            names.add(mb.parameterNames != null && mb.parameterNames.length == mb.parameters.length
                    ? new String(mb.parameterNames[i]) : "arg" + i);
        }
        e.parameterTypes = types;
        e.parameterNames = names;
        e.resolvedType = mb.returnType;
        e.resolvedTypeKnown = true;
        e.key = methodKey(t, e);
        return e;
    }

    private Element sourceField(TypeInfo t, TypeDeclaration decl, FieldDeclaration f) {
        Element e = new Element();
        e.kind = FIELD;
        e.name = new String(f.name);
        e.declaring = t;
        e.flags = f.binding.modifiers & 0xFFFF;
        if (f.getKind() == AbstractVariableDeclaration.ENUM_CONSTANT || f.type == null) {
            e.signature = Signature.createTypeSignature(decl.name, false);
        } else {
            e.signature = org.eclipse.jdt.internal.core.util.Util.typeSignature(f.type);
        }
        e.resolvedType = f.binding.type;
        e.resolvedTypeKnown = true;
        e.key = "F" + t.fqn + "." + e.name;
        return e;
    }

    private Element binaryField(TypeInfo t, IBinaryField f) {
        Element e = new Element();
        e.kind = FIELD;
        e.name = new String(f.getName());
        e.declaring = t;
        e.flags = f.getModifiers();
        char[] sig = f.getGenericSignature() != null ? f.getGenericSignature() : f.getTypeName();
        e.signature = new String(CharOperation.replaceOnCopy(sig, '/', '.'));
        e.key = "F" + t.fqn + "." + e.name;
        return e;
    }

    private Element fieldElementFromBinding(TypeInfo t, FieldBinding fb) {
        Element e = new Element();
        e.kind = FIELD;
        e.name = new String(fb.name);
        e.declaring = t;
        e.flags = fb.modifiers & 0xFFFF;
        e.signature = resolvedSignature(fb.type);
        e.resolvedType = fb.type;
        e.resolvedTypeKnown = true;
        e.key = "F" + t.fqn + "." + e.name;
        return e;
    }

    private static String methodKey(TypeInfo t, Element e) {
        StringBuilder sb = new StringBuilder("M").append(t.fqn).append('.').append(e.name).append('(');
        for (String p : e.parameterTypes) sb.append(simpleErasure(p)).append(',');
        return sb.append(')').toString();
    }

    private static String resolvedSignature(TypeBinding type) {
        return new String(CharOperation.replaceOnCopy(type.genericTypeSignature(), '/', '.'));
    }

    // Visible elements → model elements of their declaring types.

    private Element fieldElement(FieldBinding fb) {
        if (fb.declaringClass == null) return null;
        TypeInfo t = typeInfo(fb.declaringClass);
        for (Element f : fields(t)) {
            if (f.name.equals(new String(fb.name))) return f;
        }
        return fieldElementFromBinding(t, fb);
    }

    private Element methodElement(MethodBinding mb) {
        if (mb.declaringClass == null) return null;
        MethodBinding original = mb.original();
        TypeInfo t = typeInfo(original.declaringClass);
        String selector = new String(original.selector);
        for (Element m : methods(t)) {
            if (!m.name.equals(selector) || m.constructor != original.isConstructor()
                    || m.parameterTypes.size() != original.parameters.length) continue;
            boolean same = true;
            for (int i = 0; i < original.parameters.length && same; i++) {
                same = simpleErasure(m.parameterTypes.get(i)).equals(simpleName(original.parameters[i]));
            }
            if (same) return m;
        }
        return methodElementFromBinding(t, original);
    }

    private static String simpleName(TypeBinding type) {
        if (type.leafComponentType() instanceof TypeVariableBinding) {
            StringBuilder sb = new StringBuilder(new String(type.leafComponentType().sourceName()));
            for (int i = 0; i < type.dimensions(); i++) sb.append("[]");
            return sb.toString();
        }
        return new String(type.erasure().sourceName());
    }

    private Element localElement(LocalVariableBinding b) {
        if (b.type == null || b.declaration == null) return null;
        Element e = new Element();
        e.kind = LOCAL_VARIABLE;
        e.name = new String(b.name);
        AbstractVariableDeclaration local = b.declaration;
        if (local.type == null || local.type.isTypeNameVar(b.declaringScope)) {
            e.signature = Signature.createTypeSignature(b.type.signableName(), true);
        } else {
            e.signature = org.eclipse.jdt.internal.core.util.Util.typeSignature(local.type);
        }
        e.resolvedType = b.type;
        e.resolvedTypeKnown = true;
        e.key = "L" + e.name + "@" + local.sourceStart;
        return e;
    }

    // Constants without pulling extra interfaces into this class.
    private static final class TypeConstantsHolder {
        static final char[][] JAVA_LANG_OBJECT = org.eclipse.jdt.internal.compiler.lookup.TypeConstants.JAVA_LANG_OBJECT;
    }

    private static final class ClassFileConstantsHolder {
        static final int AccStatic = org.eclipse.jdt.internal.compiler.classfmt.ClassFileConstants.AccStatic;
    }
}
