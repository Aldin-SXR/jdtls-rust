package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.eclipse.jdt.core.compiler.CategorizedProblem;
import org.eclipse.jdt.core.compiler.IProblem;
import org.eclipse.jdt.core.dom.*;

/**
 * Data-only request ({@code semanticAst}): the complete JDT DOM of one
 * compilation unit, annotated with bindings, plus the AST's problems.  Rust
 * builds its {@code semantic_ast} model over this and does all correction /
 * code action logic itself.
 *
 * <p>The DOM is exported generically from the structural property
 * descriptors, so every node type and property is covered:
 * <ul>
 * <li>{@code nodes}: preorder (the {@code ASTVisitor} order), attached
 * Javadoc included; unattached comments ({@code getCommentList()}) follow as
 * extra roots.  Each node: type name, start, length, parent, location in
 * parent, extended start/length ({@code getExtendedStartPosition}), binding,
 * type binding, method/constructor binding, flags, its properties as
 * {@code [property, kind, value]} triples in descriptor order (kind 0 =
 * child node index or -1, 1 = index into the node's {@code lists}, 2 = simple
 * value as a string index).</li>
 * <li>{@code bindings}: one entry per distinct binding, referencing others by
 * index (supertypes, erasure, declaring class, parameter types, ...).
 * Declared members cover source types, owner hierarchies and declared field
 * type hierarchies. Member source ranges and names include available source attachments.</li>
 * <li>{@code problems}: {@code CompilationUnit.getProblems()}.</li>
 * </ul>
 * String-valued entries index {@code strings}; -1 means absent.
 */
final class SemanticAstService {

    static final class NodeOut {
        public int t, s, l, p, loc, es, el, b, tb, mb, f;
        public int[] pr;
        public int[][] ls;
        public AnnotationOut annotation;
        public boolean constantExpression;
    }

    static final class BindingOut {
        public int k;          // IBinding.getKind()
        public int key, n;     // key, name
        public int m;          // modifiers
        public long f;         // flags (see constants)
        // type
        public int qn = -1, bn = -1, pkg = -1;
        public int er = -1, td = -1, dc = -1, dm = -1, sc = -1, el = -1, cmp = -1, bound = -1, gt = -1, wc = -1;
        public int dim;
        public int[] it, ta, tp, tbs, dmeth, dfld, dtyp, ctors, assign;
        public int[] cast;     // ITypeBinding.isCastCompatible targets (unresolved invocations only)
        public int fim = -1;
        public int module = -1;
        // variable
        public int type = -1, vid = -1, cv = -1, vd = -1;
        // method
        public int rt = -1, md = -1;
        public int[] pt, et, pn, ss, ov;
        public int[] sm;
        public AnnotationOut[] ann, tann;
        public AnnotationOut[][] pann;
        public int nameOffset = -1, sourceOffset = -1;
    }

    static final class AnnotationOut {
        public int annotationType;
        public MemberValueOut[] members, allMembers;
    }
    static final class MemberValueOut {
        public int name;
        public AnnotationValueOut value;
    }
    static final class AnnotationValueOut {
        public int kind;
        public int text = -1, binding = -1;
        public AnnotationOut annotation;
        public AnnotationValueOut[] values;
    }

    static final class ProblemOut {
        public int id, s, e, line, sev, msg, cat;
        public int[] args;
    }

    static final class SemanticAstResponse extends BridgeProtocol.Response {
        public List<String> strings;
        public List<NodeOut> nodes;
        public List<BindingOut> bindings;
        public List<ProblemOut> problems;
        public int[] comments;
        public String cacheKey;

        SemanticAstResponse(long id) {
            this.id = id;
            this.method = "semanticAst";
        }
    }

    // Binding flags (BindingOut.f)
    static final long DEPRECATED = 1L, RECOVERED = 1L << 1, SYNTHETIC = 1L << 2, FROM_SOURCE = 1L << 3;
    static final long PRIMITIVE = 1L << 4, ARRAY = 1L << 5, CLASS = 1L << 6, INTERFACE = 1L << 7, ENUM = 1L << 8,
            RECORD = 1L << 9, ANNOTATION = 1L << 10, TYPE_VARIABLE = 1L << 11, WILDCARD = 1L << 12,
            CAPTURE = 1L << 13, PARAMETERIZED = 1L << 14, RAW = 1L << 15, GENERIC = 1L << 16, NULL_TYPE = 1L << 17,
            ANONYMOUS = 1L << 18, LOCAL = 1L << 19, MEMBER = 1L << 20, NESTED = 1L << 21, TOP_LEVEL = 1L << 22,
            INTERSECTION = 1L << 23, UPPERBOUND = 1L << 24;
    static final long FIELD = 1L << 25, ENUM_CONSTANT = 1L << 26, PARAMETER = 1L << 27, RECORD_COMPONENT = 1L << 28,
            EFFECTIVELY_FINAL = 1L << 29;
    static final long CONSTRUCTOR = 1L << 30, DEFAULT_CONSTRUCTOR = 1L << 31, VARARGS = 1L << 32,
            ANNOTATION_MEMBER = 1L << 33, GENERIC_METHOD = 1L << 34, PARAMETERIZED_METHOD = 1L << 35,
            RAW_METHOD = 1L << 36, COMPACT_CONSTRUCTOR = 1L << 37, CANONICAL_CONSTRUCTOR = 1L << 38, SYNTHETIC_RECORD_METHOD = 1L << 39;

    // Node flags (NodeOut.f) beyond ASTNode.getFlags() (MALFORMED 1, ORIGINAL 2, PROTECT 4, RECOVERED 8)
    static final int BOXING = 1 << 8, UNBOXING = 1 << 9, COMMENT_ROOT = 1 << 10;

    /** Recently built ASTs by cache key, for follow-up queries. */
    private static final int MAX_CACHED = 8;
    private static final Map<String, CompilationUnit> CACHE = new LinkedHashMap<>(16, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, CompilationUnit> eldest) {
            return size() > MAX_CACHED;
        }
    };

    private record MemberSourceData(String key, int offset, int nameOffset, List<String> parameters, List<String> modifiers) {}
    private record SourceDataKey(String unit, String source, String environment) {}
    private static final Map<SourceDataKey, List<MemberSourceData>> MEMBER_SOURCE_CACHE =
        new LinkedHashMap<>(64, 0.75f, true) {
            @Override protected boolean removeEldestEntry(Map.Entry<SourceDataKey, List<MemberSourceData>> entry) {
                return size() > 64;
            }
        };

    private SemanticAstService() {}

    static SemanticAstResponse handle(BridgeProtocol.Request req) {
        CompilationUnit cu = AstBindingsService.parse(req);
        SemanticAstResponse res = new SemanticAstResponse(req.id);
        if (cu == null) {
            res.strings = List.of();
            res.nodes = List.of();
            res.bindings = List.of();
            res.problems = List.of();
            res.comments = new int[0];
            return res;
        }
        if (req.data != null) {
            synchronized (CACHE) {
                CACHE.put(req.data, cu);
            }
            res.cacheKey = req.data;
        }
        new Collector(cu, res, req).collect();
        return res;
    }

    private static final class Collector {
        private final CompilationUnit cu;
        private final SemanticAstResponse res;
        private final BridgeProtocol.Request request;
        private final List<String> strings = new ArrayList<>();
        private final Map<String, Integer> stringIndex = new HashMap<>();
        private final List<NodeOut> nodes = new ArrayList<>();
        private final IdentityHashMap<ASTNode, Integer> nodeIndex = new IdentityHashMap<>();
        private final List<ASTNode> order = new ArrayList<>();
        private final List<BindingOut> bindings = new ArrayList<>();
        private final Map<String, Integer> bindingIndex = new HashMap<>();
        private final IdentityHashMap<IBinding, Integer> bindingIdentity = new IdentityHashMap<>();
        private final Map<Integer, IMethodBinding> methodBindings = new HashMap<>();
        private final Map<Integer, ITypeBinding> typeBindings = new HashMap<>();

        Collector(CompilationUnit cu, SemanticAstResponse res, BridgeProtocol.Request request) {
            this.cu = cu;
            this.res = res;
            this.request = request;
        }

        void collect() {
            List<ASTNode> roots = new ArrayList<>();
            roots.add(cu);
            for (Object o : cu.getCommentList()) {
                Comment c = (Comment) o;
                if (c.getParent() == null) {
                    roots.add(c);
                }
            }
            // Pass 1: number every node in preorder.
            for (ASTNode root : roots) {
                root.accept(new ASTVisitor(true) {
                    @Override
                    public boolean preVisit2(ASTNode node) {
                        nodeIndex.put(node, order.size());
                        order.add(node);
                        return true;
                    }
                });
            }
            // Pass 2: serialize.
            for (ASTNode node : order) {
                nodes.add(node(node, node != cu && node.getParent() == null));
            }
            // Export member data for owner and declared-field type hierarchies.
            // Rust applies visibility and builds operations; parameter and return
            // types do not recursively expand every library's member graph.
            for (ASTNode node : order) {
                ITypeBinding type = node instanceof AbstractTypeDeclaration d ? d.resolveBinding()
                        : node instanceof AnonymousClassDeclaration d ? d.resolveBinding() : null;
                if (type != null) {
                    constructorMembers(type);
                    if (type.getSuperclass() != null) constructorMembers(type.getSuperclass());
                    hierarchyGraph(type, new java.util.HashSet<>());
                    for (IVariableBinding field : type.getDeclaredFields()) {
                        hierarchyGraph(field.getType(), new java.util.HashSet<>());
                    }
                }
            }
            // Local declaration types expose method headers for resource lifetime
            // and invocation analysis. Rust decides which members matter.
            java.util.Set<String> localTypes = new java.util.HashSet<>();
            for (ASTNode node : order) {
                Type declared = node instanceof VariableDeclarationStatement d ? d.getType()
                        : node instanceof VariableDeclarationExpression d ? d.getType() : null;
                if (declared != null) localTypeMethods(declared.resolveBinding(), localTypes);
                if (node instanceof ExpressionStatement statement) localTypeMethods(statement.getExpression().resolveTypeBinding(), localTypes);
                ITypeBinding functional = node instanceof LambdaExpression e ? e.resolveTypeBinding()
                        : node instanceof MethodReference e ? e.resolveTypeBinding() : null;
                if (functional != null) bindings.get(binding(functional)).fim = binding(functional.getFunctionalInterfaceMethod());
            }
            hierarchyGraph(cu.getAST().resolveWellKnownType("java.lang.Object"), new java.util.HashSet<>());
            constructorMembers(cu.getAST().resolveWellKnownType("java.lang.Object"));
            boolean conditional = nodeIndex.keySet().stream().anyMatch(n -> n instanceof ConditionalExpression);
            if (conditional) {
                java.util.Set<String> seen = new java.util.HashSet<>();
                for (IMethodBinding method : new ArrayList<>(methodBindings.values())) {
                    hierarchyGraph(method.getDeclaringClass(), seen);
                }
                for (String name : new String[] {"boolean", "byte", "char", "short", "int", "long", "float", "double"}) {
                    binding(cu.getAST().resolveWellKnownType(name));
                }
            }
            boolean unresolved = unresolvedInvocations();
            namespaceAnnotations();
            memberSourceData();
            if (unresolved || conditional || order.stream().anyMatch(n -> n instanceof ExpressionStatement)) typeRelations(unresolved);
            methodRelations();
            List<Integer> comments = new ArrayList<>();
            for (Object o : cu.getCommentList()) {
                Integer idx = nodeIndex.get(o);
                if (idx != null) {
                    comments.add(idx);
                }
            }
            List<ProblemOut> problems = new ArrayList<>();
            for (IProblem p : cu.getProblems()) {
                ProblemOut po = new ProblemOut();
                po.id = p.getID();
                po.s = p.getSourceStart();
                po.e = p.getSourceEnd();
                po.line = p.getSourceLineNumber();
                po.sev = p.isError() ? 0 : p.isWarning() ? 1 : 2;
                po.msg = str(p.getMessage());
                po.cat = p instanceof CategorizedProblem cp ? cp.getCategoryID() : 0;
                String[] args = p.getArguments();
                po.args = new int[args == null ? 0 : args.length];
                for (int i = 0; i < po.args.length; i++) {
                    po.args[i] = str(args[i]);
                }
                problems.add(po);
            }
            res.strings = strings;
            res.nodes = nodes;
            res.bindings = bindings;
            res.problems = problems;
            res.comments = comments.stream().mapToInt(Integer::intValue).toArray();
        }

        private int str(String s) {
            if (s == null) {
                return -1;
            }
            return stringIndex.computeIfAbsent(s, k -> {
                strings.add(k);
                return strings.size() - 1;
            });
        }

        private NodeOut node(ASTNode node, boolean commentRoot) {
            NodeOut out = new NodeOut();
            out.t = str(node.getClass().getSimpleName());
            out.s = node.getStartPosition();
            out.l = node.getLength();
            ASTNode parent = node.getParent();
            out.p = parent == null ? -1 : nodeIndex.getOrDefault(parent, -1);
            out.loc = node.getLocationInParent() == null ? -1 : str(node.getLocationInParent().getId());
            int es = cu.getExtendedStartPosition(node);
            int el = cu.getExtendedLength(node);
            if (es != out.s || el != out.l) {
                out.es = es;
                out.el = el;
            } else {
                out.es = -1;
                out.el = -1;
            }
            out.b = -1;
            out.tb = -1;
            out.mb = -1;
            int flags = node.getFlags();
            try {
                resolve(node, out);
                if (node instanceof Expression e) {
                    if (e.resolveBoxing()) {
                        flags |= BOXING;
                    }
                    if (e.resolveUnboxing()) {
                        flags |= UNBOXING;
                    }
                }
            } catch (RuntimeException e) {
                // recovered nodes may fail to resolve
            }
            if (commentRoot) {
                flags |= COMMENT_ROOT;
            }
            out.f = flags;
            List<?> props = node.structuralPropertiesForType();
            int[] pr = new int[props.size() * 3];
            List<int[]> lists = new ArrayList<>();
            int i = 0;
            for (Object o : props) {
                StructuralPropertyDescriptor d = (StructuralPropertyDescriptor) o;
                pr[i++] = str(d.getId());
                Object v = node.getStructuralProperty(d);
                if (d.isChildProperty()) {
                    pr[i++] = 0;
                    pr[i++] = v == null ? -1 : nodeIndex.getOrDefault(v, -1);
                } else if (d.isChildListProperty()) {
                    pr[i++] = 1;
                    List<?> l = (List<?>) v;
                    int[] ids = new int[l.size()];
                    for (int j = 0; j < ids.length; j++) {
                        ids[j] = nodeIndex.getOrDefault(l.get(j), -1);
                    }
                    pr[i++] = lists.size();
                    lists.add(ids);
                } else {
                    pr[i++] = 2;
                    pr[i++] = v == null ? -1 : str(String.valueOf(v));
                }
            }
            out.pr = pr;
            out.ls = lists.toArray(new int[0][]);
            return out;
        }

        private void resolve(ASTNode node, NodeOut out) {
            if (node instanceof Expression e) {
                out.constantExpression = e.resolveConstantExpressionValue() != null;
            }
            if (node instanceof Name n) {
                out.b = binding(n.resolveBinding());
                out.tb = binding(n.resolveTypeBinding());
                return;
            }
            if (node instanceof Expression e) {
                out.tb = binding(e.resolveTypeBinding());
            }
            if (node instanceof AbstractTypeDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof AnonymousClassDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof MethodDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof VariableDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof EnumConstantDeclaration d) {
                out.b = binding(d.resolveVariable());
                out.mb = binding(d.resolveConstructorBinding());
            } else if (node instanceof ImportDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof PackageDeclaration d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof TypeParameter d) {
                out.b = binding(d.resolveBinding());
            } else if (node instanceof Type t) {
                out.b = binding(t.resolveBinding());
            } else if (node instanceof MemberValuePair d) {
                out.mb = binding(d.resolveMemberValuePairBinding() == null ? null : d.resolveMemberValuePairBinding().getMethodBinding());
            } else if (node instanceof MethodInvocation d) {
                out.mb = binding(d.resolveMethodBinding());
            } else if (node instanceof SuperMethodInvocation d) {
                out.mb = binding(d.resolveMethodBinding());
            } else if (node instanceof ClassInstanceCreation d) {
                out.mb = binding(d.resolveConstructorBinding());
            } else if (node instanceof ConstructorInvocation d) {
                out.mb = binding(d.resolveConstructorBinding());
            } else if (node instanceof SuperConstructorInvocation d) {
                out.mb = binding(d.resolveConstructorBinding());
            } else if (node instanceof MethodReference d) {
                out.mb = binding(d.resolveMethodBinding());
            } else if (node instanceof LambdaExpression d) {
                out.mb = binding(d.resolveMethodBinding());
            } else if (node instanceof FieldAccess d) {
                out.b = binding(d.resolveFieldBinding());
            } else if (node instanceof SuperFieldAccess d) {
                out.b = binding(d.resolveFieldBinding());
            } else if (node instanceof Annotation a) {
                IAnnotationBinding ab = a.resolveAnnotationBinding();
                out.b = binding(ab == null ? null : ab.getAnnotationType());
                out.annotation = ab == null ? null : annotation(ab);
            } else if (node instanceof ModuleDeclaration d) {
                out.b = binding(d.resolveBinding());
            }
        }

        private int[] bindings(IBinding[] bs) {
            if (bs == null) {
                return new int[0];
            }
            int[] out = new int[bs.length];
            for (int i = 0; i < bs.length; i++) {
                out[i] = binding(bs[i]);
            }
            return out;
        }

        private int binding(IBinding binding) {
            if (binding == null) {
                return -1;
            }
            Integer known = bindingIdentity.get(binding);
            if (known != null) {
                return known;
            }
            String key = null;
            try {
                key = binding.getKey();
            } catch (RuntimeException e) {
                // no key
            }
            String lookup = key == null ? null : binding instanceof ITypeBinding type ? typeKey(type, key) : key;
            if (lookup != null) {
                known = bindingIndex.get(lookup);
                if (known != null) {
                    bindingIdentity.put(binding, known);
                    return known;
                }
            }
            BindingOut b = new BindingOut();
            int idx = bindings.size();
            bindings.add(b);
            bindingIdentity.put(binding, idx);
            if (lookup != null) {
                bindingIndex.put(lookup, idx);
            }
            b.k = binding.getKind();
            b.key = str(key);
            try {
                b.n = str(binding.getName());
                b.m = binding.getModifiers();
                b.ann = annotations(binding.getAnnotations());
                try {
                    if (binding.getJavaElement() instanceof org.eclipse.jdt.core.IMember member) {
                        org.eclipse.jdt.core.ISourceRange nameRange = member.getNameRange();
                        if (nameRange != null) b.nameOffset = nameRange.getOffset();
                        org.eclipse.jdt.core.ISourceRange sourceRange = member.getSourceRange();
                        if (sourceRange != null) b.sourceOffset = sourceRange.getOffset();
                    }
                } catch (org.eclipse.jdt.core.JavaModelException | RuntimeException e) {
                    // Binary members and standalone ASTs can lack source ranges.
                }
                long f = 0;
                if (binding.isDeprecated()) f |= DEPRECATED;
                if (binding.isRecovered()) f |= RECOVERED;
                if (binding.isSynthetic()) f |= SYNTHETIC;
                if (binding instanceof ITypeBinding t) {
                    typeBindings.put(idx, t);
                    f |= typeFlags(t);
                    b.f = f;
                    type(t, b);
                } else if (binding instanceof IVariableBinding v) {
                    if (v.isField()) f |= FIELD;
                    if (v.isEnumConstant()) f |= ENUM_CONSTANT;
                    if (v.isParameter()) f |= PARAMETER;
                    if (v.isRecordComponent()) f |= RECORD_COMPONENT;
                    if (v.isEffectivelyFinal()) f |= EFFECTIVELY_FINAL;
                    b.f = f;
                    b.vid = v.getVariableId();
                    Object cv = v.getConstantValue();
                    b.cv = cv == null ? -1 : str(String.valueOf(cv));
                    b.type = binding(v.getType());
                    b.dc = binding(v.getDeclaringClass());
                    b.dm = binding(v.getDeclaringMethod());
                    IVariableBinding decl = v.getVariableDeclaration();
                    b.vd = decl == v ? idx : binding(decl);
                } else if (binding instanceof IMethodBinding mb) {
                    methodBindings.put(idx, mb);
                    if (mb.isSyntheticRecordMethod()) f |= SYNTHETIC_RECORD_METHOD;
                    if (mb.isConstructor()) f |= CONSTRUCTOR;
                    if (mb.isDefaultConstructor()) f |= DEFAULT_CONSTRUCTOR;
                    if (mb.isVarargs()) f |= VARARGS;
                    if (mb.isAnnotationMember()) f |= ANNOTATION_MEMBER;
                    if (mb.isGenericMethod()) f |= GENERIC_METHOD;
                    if (mb.isParameterizedMethod()) f |= PARAMETERIZED_METHOD;
                    if (mb.isRawMethod()) f |= RAW_METHOD;
                    if (mb.isCompactConstructor()) f |= COMPACT_CONSTRUCTOR;
                    if (mb.isCanonicalConstructor()) f |= CANONICAL_CONSTRUCTOR;
                    b.f = f;
                    b.dc = binding(mb.getDeclaringClass());
                    b.rt = binding(mb.getReturnType());
                    b.pt = bindings(mb.getParameterTypes());
                    b.et = bindings(mb.getExceptionTypes());
                    b.tp = bindings(mb.getTypeParameters());
                    b.ta = bindings(mb.getTypeArguments());
                    b.pann = new AnnotationOut[mb.getParameterTypes().length][];
                    for (int i = 0; i < b.pann.length; i++) b.pann[i] = annotations(mb.getParameterAnnotations(i));
                    IMethodBinding decl = mb.getMethodDeclaration();
                    b.md = decl == mb ? idx : binding(decl);
                    // Substituted bindings can expose arg0/arg1 even when the
                    // declaration's Java element has the original source names.
                    try {
                        String[] names = decl.getJavaElement() instanceof org.eclipse.jdt.core.IMethod method
                                ? method.getParameterNames() : decl.getParameterNames();
                        b.pn = new int[names == null ? 0 : names.length];
                        for (int i = 0; i < b.pn.length; i++) b.pn[i] = str(names[i]);
                    } catch (org.eclipse.jdt.core.JavaModelException | RuntimeException e) {
                        // Absence of parameter names must not truncate binding data.
                    }
                } else if (binding instanceof IPackageBinding p) {
                    b.f = f;
                    b.qn = str(p.getName());
                    b.module = binding(p.getModule());
                } else {
                    b.f = f;
                }
            } catch (RuntimeException e) {
                // keep what we have
            }
            return idx;
        }

        // Assignment relations and functional-method bindings are compiler data.
        // Export expression, parameter and return types for conditional expressions
        // and standalone-expression corrections, excluding unrelated member types.
        /**
         * Unresolved method / constructor invocations (the unresolved-element
         * quick fixes): member graphs of receivers and created types, the
         * well-known types those processors resolve, and (later) assignment and
         * cast relations between the involved types.
         */
        private boolean unresolvedInvocations() {
            boolean found = false;
            java.util.Set<Integer> starts = new java.util.HashSet<>();
            for (IProblem p : cu.getProblems()) {
                int id = p.getID();
                if (id == IProblem.UndefinedMethod || id == IProblem.ParameterMismatch || id == IProblem.UndefinedConstructor
                        || id == IProblem.UndefinedAnnotationMember || id == IProblem.NoMessageSendOnArrayType) {
                    found = true;
                    starts.add(p.getSourceStart());
                }
            }
            for (ASTNode node : order) {
                ITypeBinding receiver = null;
                boolean unresolved = false;
                boolean atProblem = starts.contains(node.getStartPosition());
                try {
                    if (node instanceof MethodInvocation m && (m.resolveMethodBinding() == null || starts.contains(m.getName().getStartPosition()))) {
                        unresolved = true;
                        if (m.getExpression() != null) receiver = m.getExpression().resolveTypeBinding();
                    } else if (node instanceof SuperMethodInvocation m && (m.resolveMethodBinding() == null || starts.contains(m.getName().getStartPosition()))) {
                        unresolved = true;
                    } else if (node instanceof ClassInstanceCreation c && (c.resolveConstructorBinding() == null || atProblem)) {
                        unresolved = true;
                        receiver = c.getType().resolveBinding();
                        constructorMembers(receiver);
                    } else if (node instanceof ConstructorInvocation c && (c.resolveConstructorBinding() == null || atProblem)) {
                        unresolved = true;
                    } else if (node instanceof SuperConstructorInvocation c && (c.resolveConstructorBinding() == null || atProblem)) {
                        unresolved = true;
                    }
                } catch (RuntimeException e) {
                    // recovered nodes
                }
                if (receiver != null) {
                    java.util.Set<String> seen = new java.util.HashSet<>();
                    hierarchyGraph(receiver, seen);
                    hierarchyGraph(receiver.getTypeDeclaration(), seen);
                    if (receiver.isArray()) hierarchyGraph(cu.getAST().resolveWellKnownType("java.lang.Object"), seen);
                }
                found |= unresolved;
            }
            if (found) {
                for (String name : new String[] {"boolean", "byte", "char", "short", "int", "long", "float", "double", "void",
                        "java.lang.Boolean", "java.lang.Byte", "java.lang.Character", "java.lang.Short", "java.lang.Integer",
                        "java.lang.Long", "java.lang.Float", "java.lang.Double", "java.lang.Object", "java.lang.String",
                        "java.lang.Exception", "java.io.Serializable", "java.lang.Cloneable"}) {
                    binding(cu.getAST().resolveWellKnownType(name));
                }
            }
            return found;
        }

        private void typeRelations() {
            typeRelations(false);
        }

        private void typeRelations(boolean cast) {
            java.util.Set<Integer> types = new java.util.LinkedHashSet<>();
            for (NodeOut node : nodes) if (node.tb >= 0) types.add(node.tb);
            if (cast) {
                for (NodeOut node : nodes) if (node.b >= 0 && typeBindings.containsKey(node.b)) types.add(node.b);
                for (BindingOut b : new ArrayList<>(bindings)) {
                    if (b.k == IBinding.VARIABLE && b.type >= 0) types.add(b.type);
                }
                for (String name : new String[] {"boolean", "byte", "char", "short", "int", "long", "float", "double",
                        "java.lang.Boolean", "java.lang.Byte", "java.lang.Character", "java.lang.Short", "java.lang.Integer",
                        "java.lang.Long", "java.lang.Float", "java.lang.Double", "java.lang.Object", "java.lang.String"}) {
                    types.add(binding(cu.getAST().resolveWellKnownType(name)));
                }
                types.remove(-1);
            }
            for (IMethodBinding method : new ArrayList<>(methodBindings.values())) {
                for (ITypeBinding parameter : method.getParameterTypes()) types.add(binding(parameter));
                if (!method.isConstructor()) types.add(binding(method.getReturnType()));
            }
            for (int id : new ArrayList<>(types)) {
                ITypeBinding type = typeBindings.get(id);
                if (type != null) {
                    try { bindings.get(id).fim = binding(type.getFunctionalInterfaceMethod()); }
                    catch (RuntimeException e) { /* recovered type */ }
                }
            }
            for (int first : types) {
                ITypeBinding source = typeBindings.get(first);
                if (source == null) continue;
                List<Integer> targets = new ArrayList<>();
                for (int second : types) {
                    ITypeBinding target = typeBindings.get(second);
                    if (target == null) continue;
                    try {
                        if (source.isAssignmentCompatible(target)) targets.add(second);
                    } catch (RuntimeException e) {
                        // Recovered bindings may not have a valid conversion.
                    }
                }
                bindings.get(first).assign = targets.stream().mapToInt(Integer::intValue).toArray();
                if (cast) {
                    List<Integer> casts = new ArrayList<>();
                    for (int second : types) {
                        ITypeBinding target = typeBindings.get(second);
                        if (target == null) continue;
                        try {
                            if (source.isCastCompatible(target)) casts.add(second);
                        } catch (RuntimeException e) {
                            // Recovered bindings may not have a valid conversion.
                        }
                    }
                    bindings.get(first).cast = casts.stream().mapToInt(Integer::intValue).toArray();
                }
            }
        }

        // Compiler predicates are semantic data. Rust chooses methods and builds
        // corrections; only the compiler can reliably compare substituted signatures.
        private void methodRelations() {
            Map<String, List<Integer>> byName = new HashMap<>();
            for (var entry : methodBindings.entrySet()) {
                byName.computeIfAbsent(entry.getValue().getName(), name -> new ArrayList<>()).add(entry.getKey());
            }
            for (List<Integer> group : byName.values()) {
                for (int first : group) {
                    IMethodBinding method = methodBindings.get(first);
                    List<Integer> subsignatures = new ArrayList<>(), overrides = new ArrayList<>();
                    for (int second : group) {
                        IMethodBinding other = methodBindings.get(second);
                        try {
                            if (method.isSubsignature(other)) subsignatures.add(second);
                            if (method.overrides(other)) overrides.add(second);
                        } catch (RuntimeException e) {
                            // Recovered or incomplete bindings need not have valid relations.
                        }
                    }
                    bindings.get(first).ss = subsignatures.stream().mapToInt(Integer::intValue).toArray();
                    bindings.get(first).ov = overrides.stream().mapToInt(Integer::intValue).toArray();
                }
            }
        }

        // Export a bounded member graph for declared fields and owner hierarchies.
        // Rust owns visibility, signature checks, ordering and generation.
        private void hierarchyGraph(ITypeBinding type, java.util.Set<String> seen) {
            if (type == null || type.isPrimitive() || type.isArray() || !seen.add(type.getKey())) return;
            hierarchyMembers(type);
            hierarchyGraph(type.getSuperclass(), seen);
            for (ITypeBinding parent : type.getInterfaces()) hierarchyGraph(parent, seen);
            for (ITypeBinding bound : type.getTypeBounds()) hierarchyGraph(bound, seen);
        }

        private void localTypeMethods(ITypeBinding type, java.util.Set<String> seen) {
            if (type == null || type.isPrimitive() || type.isArray() || !seen.add(type.getKey())) return;
            bindings.get(binding(type)).dmeth = bindings(type.getDeclaredMethods());
            localTypeMethods(type.getSuperclass(), seen);
            for (ITypeBinding parent : type.getInterfaces()) localTypeMethods(parent, seen);
            for (ITypeBinding bound : type.getTypeBounds()) localTypeMethods(bound, seen);
        }

        /** Ranges and parameter names are binding metadata, never generated code. */
        private void memberSourceData() {
            applySourceMembers(sourceMembers(cu));
            Map<String, String> types = new LinkedHashMap<>();
            for (BindingOut b : bindings) {
                if (b.k == IBinding.TYPE && b.dmeth != null && b.bn >= 0) {
                    String name = strings.get(b.bn);
                    int nested = name.indexOf('$');
                    String top = nested < 0 ? name : name.substring(0, nested);
                    String path = top.replace('.', '/') + ".java";
                    if ((b.f & FROM_SOURCE) != 0 && b.key >= 0) {
                        String key = strings.get(b.key);
                        if (key.startsWith("L")) {
                            int end = key.length();
                            for (char separator : new char[] {';', '<', '$', '~'}) {
                                int index = key.indexOf(separator);
                                if (index > 0) end = Math.min(end, index);
                            }
                            path = key.substring(1, end) + ".java";
                        }
                    }
                    types.put(top, path);
                }
            }
            java.util.Set<String> parsed = new java.util.HashSet<>();
            parsed.add(request.uri);
            for (Map.Entry<String, String> typeEntry : types.entrySet()) {
                String type = typeEntry.getKey();
                String path = typeEntry.getValue();
                // Secondary source types encode their compilation unit in the key.
                String fileUri = null;
                if (request.files != null) {
                    for (String uri : request.files.keySet()) {
                        if (uri.endsWith("/" + path)) { fileUri = uri; break; }
                    }
                }
                if (fileUri != null) {
                    if (!parsed.add(fileUri)) continue;
                    BridgeProtocol.Request other = new BridgeProtocol.Request();
                    other.uri = fileUri; other.files = request.files;
                    other.classpath = request.classpath; other.sourceLevel = request.sourceLevel;
                    other.options = request.options;
                    SourceDataKey cacheKey = new SourceDataKey(fileUri, request.files.get(fileUri),
                        BridgeOptions.map(request.sourceLevel).toString() + request.classpath + request.files.hashCode());
                    List<MemberSourceData> data;
                    synchronized (MEMBER_SOURCE_CACHE) { data = MEMBER_SOURCE_CACHE.get(cacheKey); }
                    if (data == null) {
                        CompilationUnit unit = AstBindingsService.parse(other);
                        data = unit == null ? List.of() : sourceMembers(unit);
                        synchronized (MEMBER_SOURCE_CACHE) { MEMBER_SOURCE_CACHE.put(cacheKey, data); }
                    }
                    applySourceMembers(data);
                } else {
                    ClassFileService.ClassFileDesc binary = ClassFileService.locate(request.classpath, type);
                    String source = binary == null ? null : ClassFileService.attachedSource(binary, request.sourceAttachments);
                    if (source == null || !parsed.add(binary.root + "|" + path)) continue;
                    SourceDataKey cacheKey = new SourceDataKey(binary.root + "|" + path, source,
                        BridgeOptions.map(request.sourceLevel).toString() + request.classpath);
                    List<MemberSourceData> data;
                    synchronized (MEMBER_SOURCE_CACHE) { data = MEMBER_SOURCE_CACHE.get(cacheKey); }
                    if (data != null) { applySourceMembers(data); continue; }
                    ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
                    parser.setKind(ASTParser.K_COMPILATION_UNIT);
                    parser.setSource(source.toCharArray());
                    parser.setUnitName(path);
                    parser.setResolveBindings(true);
                    parser.setBindingsRecovery(true);
                    Map<String, String> options = BridgeOptions.map(request.sourceLevel);
                    options.put("org.eclipse.jdt.core.compiler.ignoreUnnamedModuleForSplitPackage", "enabled");
                    parser.setCompilerOptions(options);
                    BridgeOptions.configureEnvironment(parser,
                        request.classpath == null ? new String[0] : request.classpath.toArray(new String[0]), new String[0]);
                    data = sourceMembers((CompilationUnit) parser.createAST(null));
                    synchronized (MEMBER_SOURCE_CACHE) { MEMBER_SOURCE_CACHE.put(cacheKey, data); }
                    applySourceMembers(data);
                }
            }
            // Substituted methods keep declaration source ranges and names.
            for (BindingOut b : bindings) {
                if (b.k == IBinding.METHOD && b.md >= 0) {
                    BindingOut declaration = bindings.get(b.md);
                    if (declaration.sourceOffset >= 0) b.sourceOffset = declaration.sourceOffset;
                    if (declaration.pn != null) b.pn = declaration.pn;
                    if (declaration.sm != null) b.sm = declaration.sm;
                }
            }
        }

        // Standalone DOM PackageBinding.getAnnotations requires a workspace
        // SearchableEnvironment. Obtain the same compiler annotation facts
        // directly from package-info source instead.
        private void namespaceAnnotations() {
            PackageDeclaration declaration = cu.getPackage();
            if (declaration == null || request.files == null) return;
            IPackageBinding packageBinding = declaration.resolveBinding();
            if (packageBinding == null) return;
            String name = declaration.getName().getFullyQualifiedName();
            for (String uri : request.files.keySet()) {
                if (!uri.endsWith("/package-info.java")) continue;
                BridgeProtocol.Request other = new BridgeProtocol.Request();
                other.uri = uri; other.files = request.files;
                other.classpath = request.classpath; other.sourceLevel = request.sourceLevel;
                other.options = request.options;
                CompilationUnit unit = uri.equals(request.uri) ? cu : AstBindingsService.parse(other);
                PackageDeclaration info = unit == null ? null : unit.getPackage();
                if (info == null || !info.getName().getFullyQualifiedName().equals(name)) continue;
                List<AnnotationOut> data = new ArrayList<>();
                for (Object value : info.annotations()) {
                    IAnnotationBinding annotation = ((Annotation) value).resolveAnnotationBinding();
                    if (annotation != null) data.add(annotation(annotation));
                }
                bindings.get(binding(packageBinding)).ann = data.toArray(new AnnotationOut[0]);
                break;
            }
        }

        private static List<MemberSourceData> sourceMembers(CompilationUnit unit) {
            List<MemberSourceData> data = new ArrayList<>();
            unit.accept(new ASTVisitor() {
                @Override public boolean visit(MethodDeclaration node) {
                    List<String> params = new ArrayList<>();
                    for (Object parameter : node.parameters()) {
                        params.add(((SingleVariableDeclaration) parameter).getName().getIdentifier());
                    }
                    sourceMember(node.resolveBinding(), node, node.getName(), params);
                    return true;
                }
                @Override public boolean visit(VariableDeclarationFragment node) {
                    if (node.getParent() instanceof FieldDeclaration) sourceMember(node.resolveBinding(), node.getParent(), node.getName(), null);
                    return true;
                }
                private void sourceMember(IBinding binding, ASTNode node, SimpleName name, List<String> params) {
                    List<String> modifiers = new ArrayList<>();
                    if (node instanceof BodyDeclaration declaration) {
                        for (Object modifier : declaration.modifiers()) {
                            if (modifier instanceof Modifier keyword) modifiers.add(keyword.getKeyword().toString());
                            else if (modifier instanceof Annotation annotation) modifiers.add("@" + annotation.getTypeName().getFullyQualifiedName());
                        }
                    }
                    if (binding != null) data.add(new MemberSourceData(binding.getKey(), node.getStartPosition(), name.getStartPosition(), params, modifiers));
                }
            });
            return data;
        }

        private void applySourceMembers(List<MemberSourceData> data) {
            for (MemberSourceData member : data) {
                Integer index = bindingIndex.get(member.key());
                if (index == null) continue;
                BindingOut out = bindings.get(index);
                out.sourceOffset = member.offset();
                out.nameOffset = member.nameOffset();
                out.sm = member.modifiers().stream().mapToInt(this::str).toArray();
                if (member.parameters() != null) {
                    out.pn = member.parameters().stream().mapToInt(this::str).toArray();
                }
            }
        }

        private void hierarchyMembers(ITypeBinding type) {
            BindingOut out = bindings.get(binding(type));
            if (out.dmeth == null) out.dmeth = bindings(type.getDeclaredMethods());
            if (out.dfld == null) out.dfld = bindings(type.getDeclaredFields());
            if (out.dtyp == null) out.dtyp = bindings(type.getDeclaredTypes());
        }

        private void constructorMembers(ITypeBinding type) {
            if (type == null) return;
            int id = binding(type);
            BindingOut out = bindings.get(id);
            if (out.ctors != null) return;
            List<IMethodBinding> constructors = new ArrayList<>();
            for (IMethodBinding method : type.getDeclaredMethods()) {
                if (method.isConstructor()) constructors.add(method);
            }
            out.ctors = bindings(constructors.toArray(new IMethodBinding[0]));
        }

        private static long typeFlags(ITypeBinding t) {
            long f = 0;
            if (t.isPrimitive()) f |= PRIMITIVE;
            if (t.isArray()) f |= ARRAY;
            if (t.isClass()) f |= CLASS;
            if (t.isInterface()) f |= INTERFACE;
            if (t.isEnum()) f |= ENUM;
            if (t.isRecord()) f |= RECORD;
            if (t.isAnnotation()) f |= ANNOTATION;
            if (t.isTypeVariable()) f |= TYPE_VARIABLE;
            if (t.isWildcardType()) f |= WILDCARD;
            if (t.isCapture()) f |= CAPTURE;
            if (t.isParameterizedType()) f |= PARAMETERIZED;
            if (t.isRawType()) f |= RAW;
            if (t.isGenericType()) f |= GENERIC;
            if (t.isNullType()) f |= NULL_TYPE;
            if (t.isAnonymous()) f |= ANONYMOUS;
            if (t.isLocal()) f |= LOCAL;
            if (t.isMember()) f |= MEMBER;
            if (t.isNested()) f |= NESTED;
            if (t.isTopLevel()) f |= TOP_LEVEL;
            if (t.isIntersectionType()) f |= INTERSECTION;
            if (t.isUpperbound()) f |= UPPERBOUND;
            if (t.isFromSource()) f |= FROM_SOURCE;
            return f;
        }

        private void type(ITypeBinding t, BindingOut b) {
            b.tann = annotations(t.getTypeAnnotations());
            b.qn = str(t.getQualifiedName());
            b.bn = str(t.getBinaryName());
            IPackageBinding pkg = t.getPackage();
            b.pkg = pkg == null ? -1 : str(pkg.getName());
            b.dim = t.getDimensions();
            b.er = binding(t.getErasure());
            b.td = binding(t.getTypeDeclaration());
            b.dc = binding(t.getDeclaringClass());
            b.dm = binding(t.getDeclaringMethod());
            b.sc = binding(t.getSuperclass());
            b.it = bindings(t.getInterfaces());
            b.ta = bindings(t.getTypeArguments());
            b.tp = bindings(t.getTypeParameters());
            b.tbs = bindings(t.getTypeBounds());
            b.el = binding(t.getElementType());
            b.cmp = binding(t.getComponentType());
            b.bound = binding(t.getBound());
            b.wc = binding(t.getWildcard());
            b.gt = binding(t.getGenericTypeOfWildcardType());
            if (t.isFromSource() && !t.isParameterizedType() && !t.isRawType() && !t.isCapture()
                    && !t.isWildcardType() && !t.isTypeVariable() && !t.isArray()) {
                b.dmeth = bindings(t.getDeclaredMethods());
                b.dfld = bindings(t.getDeclaredFields());
                b.dtyp = bindings(t.getDeclaredTypes());
            }
        }

        // Type keys deliberately ignore type annotations. Preserve annotated
        // variants, including annotations on arguments, owners and dimensions,
        // while retaining the original compiler key for Rust binding equality.
        private String typeKey(ITypeBinding type, String key) {
            StringBuilder shape = new StringBuilder();
            typeAnnotations(type, shape, new java.util.HashSet<>());
            return shape.length() == 0 ? key : key + '\0' + shape;
        }
        private void typeAnnotations(ITypeBinding type, StringBuilder shape, java.util.Set<String> path) {
            if (type == null) return;
            String key = type.getKey();
            if (!path.add(key)) return;
            StringBuilder own = new StringBuilder();
            for (IAnnotationBinding annotation : type.getTypeAnnotations()) {
                annotationShape(annotation, own);
            }
            if (own.length() > 0) shape.append(key).append('{').append(own).append('}');
            StringBuilder child = new StringBuilder();
            typeAnnotations(type.getComponentType(), child, path);
            if (child.length() > 0) shape.append("array(").append(child).append(')');
            ITypeBinding[] arguments = type.getTypeArguments();
            for (int i = 0; i < arguments.length; i++) {
                child.setLength(0);
                typeAnnotations(arguments[i], child, path);
                if (child.length() > 0) shape.append("arg").append(i).append('(').append(child).append(')');
            }
            child.setLength(0);
            typeAnnotations(type.getBound(), child, path);
            if (child.length() > 0) shape.append("bound(").append(child).append(')');
            child.setLength(0);
            typeAnnotations(type.getDeclaringClass(), child, path);
            if (child.length() > 0) shape.append("owner(").append(child).append(')');
            path.remove(key);
        }
        private void shapeText(StringBuilder shape, String text) {
            if (text == null) shape.append("-1:");
            else shape.append(text.length()).append(':').append(text);
        }
        // AnnotationBinding.toString uses a simple name, so it cannot identify
        // equally named annotations (or values) from different packages.
        private void annotationShape(IAnnotationBinding annotation, StringBuilder shape) {
            shape.append('@');
            shapeText(shape, annotation.getAnnotationType().getKey());
            IMemberValuePairBinding[] pairs = annotation.getDeclaredMemberValuePairs();
            shape.append(pairs.length).append('(');
            for (IMemberValuePairBinding pair : pairs) {
                shapeText(shape, pair.getName());
                annotationValueShape(pair.getValue(), shape);
            }
            shape.append(')');
        }
        private void annotationValueShape(Object value, StringBuilder shape) {
            if (value instanceof IAnnotationBinding annotation) annotationShape(annotation, shape);
            else if (value instanceof IBinding binding) {
                shape.append('K'); shapeText(shape, binding.getKey());
            } else if (value instanceof Object[] values) {
                shape.append(values.length).append('[');
                for (Object element : values) annotationValueShape(element, shape);
                shape.append(']');
            } else {
                shapeText(shape, value == null ? null : value.getClass().getName());
                shapeText(shape, value == null ? null : value.toString());
            }
        }
        private AnnotationOut[] annotations(IAnnotationBinding[] input) {
            AnnotationOut[] output = new AnnotationOut[input.length];
            for (int i = 0; i < input.length; i++) output[i] = annotation(input[i]);
            return output;
        }
        private AnnotationOut annotation(IAnnotationBinding annotation) {
            AnnotationOut output = new AnnotationOut();
            output.annotationType = binding(annotation.getAnnotationType());
            output.members = memberValues(annotation.getDeclaredMemberValuePairs());
            output.allMembers = memberValues(annotation.getAllMemberValuePairs());
            return output;
        }
        private MemberValueOut[] memberValues(IMemberValuePairBinding[] pairs) {
            MemberValueOut[] output = new MemberValueOut[pairs.length];
            for (int i = 0; i < pairs.length; i++) {
                MemberValueOut member = new MemberValueOut();
                member.name = str(pairs[i].getName());
                member.value = annotationValue(pairs[i].getValue());
                output[i] = member;
            }
            return output;
        }
        private AnnotationValueOut annotationValue(Object value) {
            AnnotationValueOut output = new AnnotationValueOut();
            if (value instanceof Boolean) { output.kind = 1; output.text = str(value.toString()); }
            else if (value instanceof Number) { output.kind = 2; output.text = str(value.toString()); }
            else if (value instanceof Character character) { output.kind = 3; output.text = str(Integer.toString(character)); }
            else if (value instanceof String text) { output.kind = 4; output.text = str(text); }
            else if (value instanceof ITypeBinding type) { output.kind = 5; output.binding = binding(type); }
            else if (value instanceof IVariableBinding variable) { output.kind = 6; output.binding = binding(variable); }
            else if (value instanceof IAnnotationBinding annotation) { output.kind = 7; output.annotation = annotation(annotation); }
            else if (value instanceof Object[] values) {
                output.kind = 8;
                output.values = new AnnotationValueOut[values.length];
                for (int i = 0; i < values.length; i++) output.values[i] = annotationValue(values[i]);
            }
            return output;
        }
    }
}
