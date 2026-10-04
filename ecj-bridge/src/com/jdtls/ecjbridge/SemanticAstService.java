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
 * Declared members are listed for source types only.</li>
 * <li>{@code problems}: {@code CompilationUnit.getProblems()}.</li>
 * </ul>
 * String-valued entries index {@code strings}; -1 means absent.
 */
final class SemanticAstService {

    static final class NodeOut {
        public int t, s, l, p, loc, es, el, b, tb, mb, f;
        public int[] pr;
        public int[][] ls;
    }

    static final class BindingOut {
        public int k;          // IBinding.getKind()
        public int key, n;     // key, name
        public int m;          // modifiers
        public long f;         // flags (see constants)
        // type
        public int qn = -1, bn = -1, pkg = -1;
        public int er = -1, td = -1, dc = -1, dm = -1, sc = -1, el = -1, cmp = -1, bound = -1, gt = -1;
        public int dim;
        public int[] it, ta, tp, tbs, dmeth, dfld, dtyp;
        // variable
        public int type = -1, vid = -1, cv = -1, vd = -1;
        // method
        public int rt = -1, md = -1;
        public int[] pt, et;
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
            RAW_METHOD = 1L << 36, COMPACT_CONSTRUCTOR = 1L << 37, CANONICAL_CONSTRUCTOR = 1L << 38;

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
        new Collector(cu, res).collect();
        return res;
    }

    private static final class Collector {
        private final CompilationUnit cu;
        private final SemanticAstResponse res;
        private final List<String> strings = new ArrayList<>();
        private final Map<String, Integer> stringIndex = new HashMap<>();
        private final List<NodeOut> nodes = new ArrayList<>();
        private final IdentityHashMap<ASTNode, Integer> nodeIndex = new IdentityHashMap<>();
        private final List<ASTNode> order = new ArrayList<>();
        private final List<BindingOut> bindings = new ArrayList<>();
        private final Map<String, Integer> bindingIndex = new HashMap<>();
        private final IdentityHashMap<IBinding, Integer> bindingIdentity = new IdentityHashMap<>();

        Collector(CompilationUnit cu, SemanticAstResponse res) {
            this.cu = cu;
            this.res = res;
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
            if (key != null) {
                known = bindingIndex.get(key);
                if (known != null) {
                    bindingIdentity.put(binding, known);
                    return known;
                }
            }
            BindingOut b = new BindingOut();
            int idx = bindings.size();
            bindings.add(b);
            bindingIdentity.put(binding, idx);
            if (key != null) {
                bindingIndex.put(key, idx);
            }
            b.k = binding.getKind();
            b.key = str(key);
            try {
                b.n = str(binding.getName());
                b.m = binding.getModifiers();
                long f = 0;
                if (binding.isDeprecated()) f |= DEPRECATED;
                if (binding.isRecovered()) f |= RECOVERED;
                if (binding.isSynthetic()) f |= SYNTHETIC;
                if (binding instanceof ITypeBinding t) {
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
                    IMethodBinding decl = mb.getMethodDeclaration();
                    b.md = decl == mb ? idx : binding(decl);
                } else if (binding instanceof IPackageBinding p) {
                    b.f = f;
                    b.qn = str(p.getName());
                } else {
                    b.f = f;
                }
            } catch (RuntimeException e) {
                // keep what we have
            }
            return idx;
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
            b.gt = binding(t.getGenericTypeOfWildcardType());
            if (t.isFromSource() && !t.isParameterizedType() && !t.isRawType() && !t.isCapture()
                    && !t.isWildcardType() && !t.isTypeVariable() && !t.isArray()) {
                b.dmeth = bindings(t.getDeclaredMethods());
                b.dfld = bindings(t.getDeclaredFields());
                b.dtyp = bindings(t.getDeclaredTypes());
            }
        }
    }
}
