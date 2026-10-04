package com.jdtls.ecjbridge;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.jar.JarFile;

import org.eclipse.jdt.core.dom.*;

import com.jdtls.ecjbridge.BridgeProtocol.Request;

/**
 * Binding data for hover (port support for jdt.ls {@code HoverInfoProvider}).
 *
 * The bridge only answers "what is the element at this position" with data:
 * the element's signature parts, its Javadoc as a serialized JDT Javadoc DOM
 * (with every reference already resolved to a location), constant values and
 * the supertype hierarchy needed for {@code {@inheritDoc}}.  All label
 * composition, Javadoc → HTML and HTML → Markdown is done in Rust.
 */
final class HoverService {
    private static final boolean DEBUG = System.getenv("HOVER_DEBUG") != null;

    /** Per-request state. */
    private static final class Ctx {
        final Map<String, String> files;
        final List<String> classpath;
        final Map<String, String> options;
        final SourceIndexNameEnvironment env;
        final Map<String, CompilationUnit> units = new HashMap<>();
        final String targetUri;
        AST ast;

        Ctx(Request req) {
            this.files = req.files == null ? Map.of() : req.files;
            this.classpath = req.classpath == null ? List.of() : req.classpath;
            Map<String, String> o = BridgeOptions.map(req.sourceLevel == null ? "21" : req.sourceLevel);
            o.put("org.eclipse.jdt.core.compiler.doc.comment.support", "enabled");
            this.options = o;
            this.env = new SourceIndexNameEnvironment(files, classpath);
            this.targetUri = req.uri;
        }

        CompilationUnit unit(String uri) {
            if (units.containsKey(uri)) {
                return units.get(uri);
            }
            String src = files.get(uri);
            CompilationUnit cu = null;
            if (src != null) {
                try {
                    cu = BridgeDomResolver.resolve(env, new InMemoryCompilationUnit(uri, src), options);
                } catch (RuntimeException e) {
                    if (DEBUG) e.printStackTrace();
                    cu = null;
                }
                if (DEBUG) System.err.println("resolved " + uri + " -> " + (cu != null));
                if (cu == null) {
                    ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
                    parser.setSource(src.toCharArray());
                    parser.setKind(ASTParser.K_COMPILATION_UNIT);
                    parser.setCompilerOptions(options);
                    parser.setStatementsRecovery(true);
                    cu = (CompilationUnit) parser.createAST(null);
                }
            }
            units.put(uri, cu);
            return cu;
        }
    }

    /** Source declaration of a binding. */
    private static final class Decl {
        final String uri;
        final CompilationUnit cu;
        final String source;
        final ASTNode node;

        Decl(String uri, CompilationUnit cu, String source, ASTNode node) {
            this.uri = uri;
            this.cu = cu;
            this.source = source;
            this.node = node;
        }
    }

    // ─── Entry point ─────────────────────────────────────────────────────────

    static Map<String, Object> hoverInfo(Request req, int line, int character) {
        Map<String, Object> result = new LinkedHashMap<>();
        String source = req.files == null ? null : req.files.get(req.uri);
        if (source == null) {
            result.put("status", "noUnit");
            return result;
        }
        Ctx ctx = new Ctx(req);
        CompilationUnit cu = ctx.unit(req.uri);
        if (cu == null) {
            result.put("status", "none");
            return result;
        }
        ctx.ast = cu.getAST();
        int offset = CompilationService.lineColToOffset(source, line, character);
        Name name = nameAt(cu, source, offset);
        if (DEBUG) System.err.println("hover: offset=" + offset + " name=" + name + " bindings=" + (cu.getAST().hasResolvedBindings()) + " problems=" + java.util.Arrays.toString(cu.getProblems()));
        if (name == null) {
            result.put("status", "none");
            return result;
        }
        IBinding binding = selectBinding(name);
        if (binding == null) {
            result.put("status", "none");
            return result;
        }
        if (binding instanceof ITypeBinding t && (t.isRecovered() || isProblemType(t))) {
            result.put("status", "unresolved");
            return result;
        }
        Map<String, Object> element = element(ctx, binding);
        if (element == null) {
            result.put("status", "none");
            return result;
        }
        result.put("status", "ok");
        result.put("element", element);
        return result;
    }

    private static boolean isProblemType(ITypeBinding t) {
        ITypeBinding e = t.getElementType() != null ? t.getElementType() : t;
        return e.isRecovered();
    }

    // ─── Selection (JDT codeSelect semantics) ────────────────────────────────

    private static Name nameAt(CompilationUnit cu, String source, int offset) {
        int start = offset;
        if (start >= source.length() || !Character.isJavaIdentifierPart(source.charAt(start))) {
            if (start > 0 && start <= source.length() && Character.isJavaIdentifierPart(source.charAt(start - 1))) {
                start = start - 1;
            } else {
                return null;
            }
        }
        NodeFinder finder = new NodeFinder(cu, start, 0);
        ASTNode node = finder.getCoveringNode();
        if (node instanceof SimpleName sn) {
            return sn;
        }
        // In Javadoc, names may only be covered by their parent ref
        final Name[] found = new Name[1];
        final int pos = start;
        if (node != null) {
            node.accept(new ASTVisitor(true) {
                @Override
                public boolean preVisit2(ASTNode n) {
                    if (found[0] != null) return false;
                    int s = n.getStartPosition();
                    if (pos < s || pos >= s + n.getLength()) return false;
                    if (n instanceof SimpleName sn2) {
                        found[0] = sn2;
                        return false;
                    }
                    return true;
                }
            });
        }
        return found[0];
    }

    private static IBinding selectBinding(Name name) {
        // `var` → the inferred type
        ASTNode parent = name.getParent();
        if (parent instanceof SimpleType st && st.isVar()) {
            ITypeBinding t = st.resolveBinding();
            if (t != null) return t;
        }
        // `new Foo()` → the constructor
        ASTNode typeNode = name;
        while (typeNode.getParent() instanceof Name || typeNode.getParent() instanceof Type) {
            ASTNode p = typeNode.getParent();
            if (p instanceof QualifiedName qn && qn.getName() != typeNode && typeNode != name) {
                break;
            }
            typeNode = p;
            if (p instanceof ParameterizedType) {
                break;
            }
        }
        if (typeNode instanceof Type && typeNode.getParent() instanceof ClassInstanceCreation cic
                && cic.getType() == typeNode && cic.getAnonymousClassDeclaration() == null
                && isLastSegment(name, (Type) typeNode)) {
            IMethodBinding ctor = cic.resolveConstructorBinding();
            if (ctor != null) return ctor;
        }
        IBinding b = name.resolveBinding();
        if (b == null && name.getParent() instanceof MemberRef mr) {
            b = mr.resolveBinding();
        } else if (b == null && name.getParent() instanceof MethodRef mr) {
            b = mr.resolveBinding();
        }
        return b;
    }

    private static boolean isLastSegment(Name name, Type type) {
        Type t = type instanceof ParameterizedType pt ? pt.getType() : type;
        if (t instanceof SimpleType st) {
            Name n = st.getName();
            return n == name || (n instanceof QualifiedName qn && qn.getName() == name);
        }
        if (t instanceof QualifiedType qt) {
            return qt.getName() == name;
        }
        if (t instanceof NameQualifiedType nqt) {
            return nqt.getName() == name;
        }
        return false;
    }

    // ─── Element data ────────────────────────────────────────────────────────

    private static Map<String, Object> element(Ctx ctx, IBinding binding) {
        Map<String, Object> e = new LinkedHashMap<>();
        if (binding instanceof IPackageBinding pkg) {
            e.put("kind", "package");
            e.put("name", pkg.getName());
            packageData(ctx, pkg, e);
            return e;
        }
        if (binding instanceof ITypeBinding type) {
            if (type.isTypeVariable()) {
                e.put("kind", "typeParameter");
                e.put("name", type.getName());
                e.put("bounds", bounds(type));
                IBinding member = declaringMemberOfTypeVariable(type);
                if (member != null) {
                    e.put("declaringMember", memberLabel(member));
                    memberDoc(ctx, member, e);
                }
            } else {
                ITypeBinding t = type.isArray() ? type.getElementType() : type;
                if (t.isPrimitive() || t.isNullType()) {
                    return null;
                }
                e.put("kind", "type");
                e.put("type", typeLabel(t, true));
                memberDoc(ctx, t.getTypeDeclaration(), e);
            }
        } else if (binding instanceof IMethodBinding method) {
            e.put("kind", "method");
            e.put("method", methodLabel(method));
            memberDoc(ctx, method.getMethodDeclaration(), e);
            IMethodBinding decl = method.getMethodDeclaration();
            if (decl.getDeclaringClass() != null && decl.getDeclaringClass().isAnnotation()) {
                Object dv = decl.getDefaultValue();
                if (dv != null) {
                    e.put("defaultValue", annotationValue(dv));
                }
            }
        } else if (binding instanceof IVariableBinding var) {
            if (var.isField() || var.isEnumConstant()) {
                IVariableBinding decl = var.getVariableDeclaration();
                e.put("kind", "field");
                e.put("field", fieldLabel(decl));
                memberDoc(ctx, decl, e);
            } else {
                e.put("kind", "localVariable");
                e.put("name", var.getName());
                e.put("type", typeRef(var.getType()));
                IMethodBinding m = var.getDeclaringMethod();
                if (m != null) {
                    e.put("declaringMember", memberLabel(m));
                } else {
                    // initializer / lambda in field: no declaring method
                    e.put("declaringMember", null);
                }
                e.put("isParameter", var.isParameter());
                locationAndRoot(ctx, var, e);
            }
        } else {
            return null;
        }
        return e;
    }

    private static IBinding declaringMemberOfTypeVariable(ITypeBinding tv) {
        if (tv.getDeclaringMethod() != null) return tv.getDeclaringMethod().getMethodDeclaration();
        if (tv.getDeclaringClass() != null) return tv.getDeclaringClass().getTypeDeclaration();
        return null;
    }

    /** Label data of a member, as needed for post-qualification. */
    private static Map<String, Object> memberLabel(IBinding member) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (member instanceof IMethodBinding mb) {
            m.put("kind", "method");
            m.put("method", methodLabel(mb));
        } else if (member instanceof ITypeBinding tb) {
            m.put("kind", "type");
            m.put("type", typeLabel(tb, false));
        } else if (member instanceof IVariableBinding vb) {
            m.put("kind", "field");
            m.put("field", fieldLabel(vb));
        }
        return m;
    }

    // ─── Labels (JavaElementLabelComposerCore inputs) ────────────────────────

    static Map<String, Object> typeRef(ITypeBinding t) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (t == null) {
            m.put("k", "class");
            m.put("n", "Object");
            return m;
        }
        if (t.isPrimitive() || t.isNullType()) {
            m.put("k", "base");
            m.put("n", t.getName());
        } else if (t.isArray()) {
            m.put("k", "array");
            m.put("e", typeRef(t.getElementType()));
            m.put("d", t.getDimensions());
        } else if (t.isCapture()) {
            ITypeBinding w = t.getWildcard();
            return w != null ? typeRef(w) : typeRef(t.getErasure());
        } else if (t.isWildcardType()) {
            m.put("k", "wild");
            if (t.getBound() != null) {
                m.put("b", typeRef(t.getBound()));
                m.put("up", t.isUpperbound());
            }
        } else if (t.isTypeVariable()) {
            m.put("k", "tv");
            m.put("n", t.getName());
        } else if (t.isIntersectionType()) {
            m.put("k", "inter");
            List<Object> bs = new ArrayList<>();
            for (ITypeBinding b : t.getTypeBounds()) bs.add(typeRef(b));
            m.put("b", bs);
        } else {
            m.put("k", "class");
            ITypeBinding erasure = t.getErasure();
            String name = erasure.getName();
            if (name.isEmpty()) {
                name = erasure.getBinaryName() != null ? erasure.getBinaryName() : "";
                int dot = Math.max(name.lastIndexOf('.'), name.lastIndexOf('$'));
                name = name.substring(dot + 1);
            }
            m.put("n", name);
            if (t.isParameterizedType()) {
                List<Object> args = new ArrayList<>();
                for (ITypeBinding a : t.getTypeArguments()) args.add(typeRef(a));
                m.put("a", args);
            }
        }
        return m;
    }

    private static List<Object> bounds(ITypeBinding tv) {
        List<Object> out = new ArrayList<>();
        for (ITypeBinding b : tv.getTypeBounds()) {
            out.add(typeRef(b));
        }
        return out;
    }

    /**
     * Fully qualified label parts of a type: package, enclosing types /
     * methods, name, and (when {@code withTypeParams}) type parameters or
     * arguments.
     */
    static Map<String, Object> typeLabel(ITypeBinding t, boolean withTypeParams) {
        Map<String, Object> m = new LinkedHashMap<>();
        ITypeBinding decl = t.getTypeDeclaration();
        IPackageBinding pkg = decl.getPackage();
        m.put("package", pkg == null || pkg.isUnnamed() ? "" : pkg.getName());
        List<Object> containers = new ArrayList<>();
        collectContainers(decl, containers);
        m.put("containers", containers);
        if (decl.isAnonymous()) {
            m.put("name", "");
            ITypeBinding[] itfs = decl.getInterfaces();
            ITypeBinding sup = itfs.length > 0 ? itfs[0] : decl.getSuperclass();
            if (sup != null) {
                m.put("anonymousSuper", sup.getErasure().getName());
            }
        } else {
            m.put("name", decl.getName());
        }
        if (withTypeParams) {
            if (t.isParameterizedType()) {
                List<Object> args = new ArrayList<>();
                for (ITypeBinding a : t.getTypeArguments()) args.add(typeRef(a));
                m.put("typeArguments", args);
            } else {
                List<Object> tps = new ArrayList<>();
                for (ITypeBinding tp : decl.getTypeParameters()) tps.add(tp.getName());
                m.put("typeParameters", tps);
            }
        }
        return m;
    }

    private static void collectContainers(ITypeBinding t, List<Object> out) {
        ITypeBinding declaring = t.getDeclaringClass();
        IMethodBinding declaringMethod = t.getDeclaringMethod();
        if (declaring == null && declaringMethod != null) {
            declaring = declaringMethod.getDeclaringClass();
        }
        if (declaring != null) {
            collectContainers(declaring.getTypeDeclaration(), out);
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("kind", "type");
            c.put("name", declaring.getTypeDeclaration().isAnonymous() ? "" : declaring.getTypeDeclaration().getName());
            out.add(c);
            if (declaringMethod != null) {
                Map<String, Object> mm = new LinkedHashMap<>();
                mm.put("kind", "method");
                mm.put("name", declaringMethod.isConstructor() ? declaringMethod.getDeclaringClass().getName() : declaringMethod.getName());
                mm.put("hasParams", declaringMethod.getParameterTypes().length > 0);
                out.add(mm);
            }
        }
    }

    static Map<String, Object> methodLabel(IMethodBinding method) {
        Map<String, Object> m = new LinkedHashMap<>();
        IMethodBinding decl = method.getMethodDeclaration();
        m.put("name", decl.isConstructor() ? decl.getDeclaringClass().getName() : decl.getName());
        m.put("isConstructor", decl.isConstructor());
        m.put("declaringType", typeLabel(decl.getDeclaringClass(), false));
        if (method.isParameterizedMethod()) {
            List<Object> args = new ArrayList<>();
            for (ITypeBinding a : method.getTypeArguments()) args.add(typeRef(a));
            m.put("typeArguments", args);
        } else {
            List<Object> tps = new ArrayList<>();
            for (ITypeBinding tp : decl.getTypeParameters()) tps.add(tp.getName());
            m.put("typeParameters", tps);
        }
        IMethodBinding shown = method.isParameterizedMethod() ? method : decl;
        if (!decl.isConstructor()) {
            m.put("returnType", typeRef(shown.getReturnType()));
        }
        List<Object> params = new ArrayList<>();
        ITypeBinding[] pts = shown.getParameterTypes();
        String[] names = parameterNames(decl);
        for (int i = 0; i < pts.length; i++) {
            Map<String, Object> p = new LinkedHashMap<>();
            p.put("type", typeRef(pts[i]));
            p.put("name", names != null && i < names.length ? names[i] : null);
            params.add(p);
        }
        m.put("parameters", params);
        m.put("varargs", decl.isVarargs());
        List<Object> exc = new ArrayList<>();
        for (ITypeBinding x : shown.getExceptionTypes()) exc.add(typeRef(x));
        m.put("exceptions", exc);
        return m;
    }

    private static String[] parameterNames(IMethodBinding decl) {
        try {
            return decl.getParameterNames();
        } catch (RuntimeException e) {
            return null;
        }
    }

    static Map<String, Object> fieldLabel(IVariableBinding f) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("name", f.getName());
        m.put("isEnumConstant", f.isEnumConstant());
        m.put("type", typeRef(f.getType()));
        if (f.getDeclaringClass() != null) {
            m.put("declaringType", typeLabel(f.getDeclaringClass(), false));
        }
        int mods = f.getModifiers();
        boolean staticFinal = Modifier.isStatic(mods) && Modifier.isFinal(mods);
        if (f.getDeclaringClass() != null && f.getDeclaringClass().isInterface()) {
            staticFinal = true;
        }
        m.put("staticFinal", staticFinal);
        if (staticFinal && !f.isEnumConstant()) {
            Object cv = f.getConstantValue();
            if (cv != null) {
                m.put("constant", constant(cv));
            }
        }
        return m;
    }

    static Map<String, Object> constant(Object cv) {
        Map<String, Object> c = new LinkedHashMap<>();
        if (cv instanceof String s) {
            c.put("kind", "string");
            c.put("value", s);
        } else if (cv instanceof Character ch) {
            c.put("kind", "char");
            c.put("value", String.valueOf(ch));
        } else {
            c.put("kind", "other");
            c.put("value", String.valueOf(cv));
        }
        return c;
    }

    static Object annotationValue(Object v) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (v instanceof ITypeBinding t) {
            m.put("kind", "type");
            m.put("value", t.getName());
        } else if (v instanceof IVariableBinding var) {
            m.put("kind", "enum");
            m.put("value", var.getName());
        } else if (v instanceof IAnnotationBinding a) {
            m.put("kind", "annotation");
            m.put("value", a.getName());
            List<Object> pairs = new ArrayList<>();
            for (IMemberValuePairBinding p : a.getDeclaredMemberValuePairs()) {
                Map<String, Object> pm = new LinkedHashMap<>();
                pm.put("name", p.getName());
                pm.put("value", annotationValue(p.getValue()));
                pairs.add(pm);
            }
            m.put("pairs", pairs);
        } else if (v instanceof Object[] arr) {
            m.put("kind", "array");
            List<Object> items = new ArrayList<>();
            for (Object o : arr) items.add(annotationValue(o));
            m.put("items", items);
        } else if (v instanceof String s) {
            m.put("kind", "string");
            m.put("value", s);
        } else if (v instanceof Character ch) {
            m.put("kind", "char");
            m.put("value", String.valueOf(ch));
        } else {
            m.put("kind", "other");
            m.put("value", String.valueOf(v));
        }
        return m;
    }

    // ─── Declarations and locations ──────────────────────────────────────────

    private static String declKey(IBinding b) {
        if (b instanceof IMethodBinding m) return m.getMethodDeclaration().getKey();
        if (b instanceof IVariableBinding v && v.isField()) return v.getVariableDeclaration().getKey();
        if (b instanceof ITypeBinding t && !t.isTypeVariable()) return t.getTypeDeclaration().getKey();
        return b.getKey();
    }

    private static ITypeBinding topLevel(IBinding b) {
        ITypeBinding t;
        if (b instanceof ITypeBinding tb) {
            if (tb.isTypeVariable()) {
                t = tb.getDeclaringClass();
                if (t == null && tb.getDeclaringMethod() != null) t = tb.getDeclaringMethod().getDeclaringClass();
            } else {
                t = tb;
            }
        } else if (b instanceof IMethodBinding mb) {
            t = mb.getDeclaringClass();
        } else if (b instanceof IVariableBinding vb) {
            t = vb.getDeclaringClass();
            if (t == null && vb.getDeclaringMethod() != null) t = vb.getDeclaringMethod().getDeclaringClass();
        } else {
            return null;
        }
        while (t != null) {
            t = t.getTypeDeclaration();
            ITypeBinding next = t.getDeclaringClass();
            if (next == null && t.getDeclaringMethod() != null) next = t.getDeclaringMethod().getDeclaringClass();
            if (next == null) break;
            t = next;
        }
        return t;
    }

    private static Decl decl(Ctx ctx, IBinding b) {
        ITypeBinding top = topLevel(b);
        String key = declKey(b);
        CompilationUnit target = ctx.unit(ctx.targetUri);
        if (top == null) {
            ASTNode n = target.findDeclaringNode(b);
            return n == null ? null : new Decl(ctx.targetUri, target, ctx.files.get(ctx.targetUri), n);
        }
        if (!top.isFromSource()) {
            return null;
        }
        // Same unit first
        ASTNode node = target.findDeclaringNode(key);
        if (node == null) node = target.findDeclaringNode(b);
        if (node != null) {
            return new Decl(ctx.targetUri, target, ctx.files.get(ctx.targetUri), node);
        }
        String qn = top.getQualifiedName();
        String binary = qn.replace('.', '/');
        String uri = ctx.env.sourceUriOf(binary);
        if (uri == null || uri.equals(ctx.targetUri)) {
            return null;
        }
        CompilationUnit cu = ctx.unit(uri);
        if (cu == null) return null;
        node = cu.findDeclaringNode(key);
        if (node == null) return null;
        return new Decl(uri, cu, ctx.files.get(uri), node);
    }

    private static SimpleName declName(ASTNode n) {
        if (n instanceof AbstractTypeDeclaration t) return t.getName();
        if (n instanceof MethodDeclaration m) return m.getName();
        if (n instanceof VariableDeclaration v) return v.getName();
        if (n instanceof EnumConstantDeclaration ec) return ec.getName();
        if (n instanceof AnnotationTypeMemberDeclaration am) return am.getName();
        if (n instanceof TypeParameter tp) return tp.getName();
        return null;
    }

    private static Javadoc javadocOf(ASTNode n) {
        if (n instanceof BodyDeclaration bd) return bd.getJavadoc();
        if (n instanceof VariableDeclarationFragment f && f.getParent() instanceof FieldDeclaration fd) return fd.getJavadoc();
        return null;
    }

    /** {@code JDTUtils.toLocation(element)}: name-range location, as data. */
    private static Map<String, Object> location(Ctx ctx, IBinding b) {
        if (b == null || b instanceof IPackageBinding) return null;
        Decl d = decl(ctx, b);
        if (d != null) {
            SimpleName name = declName(d.node);
            int pos = name != null ? name.getStartPosition() : d.node.getStartPosition();
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("uri", d.uri);
            m.put("line", d.cu.getLineNumber(pos) - 1);
            return m;
        }
        ITypeBinding top = topLevel(b);
        if (top != null && !top.isFromSource()) {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("classFile", classFile(ctx, b));
            m.put("line", 0);
            return m;
        }
        return null;
    }

    /** The class file (and its root) declaring a binary binding. */
    private static Map<String, Object> classFile(Ctx ctx, IBinding b) {
        ITypeBinding t = b instanceof ITypeBinding tb && !tb.isTypeVariable() ? tb.getTypeDeclaration() : null;
        if (t == null) {
            if (b instanceof IMethodBinding mb) t = mb.getDeclaringClass();
            else if (b instanceof IVariableBinding vb) t = vb.getDeclaringClass();
            else if (b instanceof ITypeBinding tv) t = tv.getDeclaringClass();
        }
        if (t == null) return null;
        t = t.getTypeDeclaration();
        String binaryName = t.getBinaryName();
        if (binaryName == null) return null;
        IPackageBinding pkg = t.getPackage();
        String pkgName = pkg == null || pkg.isUnnamed() ? "" : pkg.getName();
        String simpleBinary = pkgName.isEmpty() ? binaryName : binaryName.substring(pkgName.length() + 1);
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("packageName", pkgName);
        m.put("classFileName", simpleBinary + ".class");
        String path = binaryName.replace('.', '/');
        if (!pkgName.isEmpty()) {
            path = pkgName.replace('.', '/') + "/" + simpleBinary;
        }
        locateRoot(ctx, path + ".class", m);
        return m;
    }

    private static void locateRoot(Ctx ctx, String entry, Map<String, Object> m) {
        for (String cp : ctx.classpath) {
            File f = new File(cp);
            if (f.isDirectory()) {
                if (new File(f, entry).isFile()) {
                    m.put("root", cp);
                    m.put("rootKind", "folder");
                    return;
                }
            } else if (f.isFile()) {
                try (JarFile jar = new JarFile(f)) {
                    if (jar.getEntry(entry) != null) {
                        m.put("root", cp);
                        m.put("rootKind", "archive");
                        return;
                    }
                } catch (IOException e) {
                    // ignore
                }
            }
        }
        try {
            java.nio.file.FileSystem jrt = java.nio.file.FileSystems.getFileSystem(java.net.URI.create("jrt:/"));
            try (var mods = Files.list(jrt.getPath("/modules"))) {
                for (java.nio.file.Path mod : (Iterable<java.nio.file.Path>) mods::iterator) {
                    if (Files.exists(mod.resolve(entry))) {
                        m.put("root", System.getProperty("java.home"));
                        m.put("rootKind", "jrt");
                        m.put("module", mod.getFileName().toString());
                        return;
                    }
                }
            }
        } catch (Exception e) {
            // ignore
        }
    }

    private static void locationAndRoot(Ctx ctx, IBinding b, Map<String, Object> e) {
        Map<String, Object> loc = location(ctx, b);
        if (loc != null) {
            e.put("location", loc);
        }
    }

    // ─── Javadoc ─────────────────────────────────────────────────────────────

    /** Location, Javadoc and (for methods) {@code @inheritDoc} data of a member. */
    private static void memberDoc(Ctx ctx, IBinding member, Map<String, Object> e) {
        locationAndRoot(ctx, member, e);
        Decl d = decl(ctx, member);
        e.put("hasSource", d != null);
        if (d == null) {
            return;
        }
        Javadoc jd = javadocOf(d.node);
        if (jd != null) {
            e.put("javadoc", docSource(ctx, d, jd, member));
        }
        e.put("docContext", docContext(ctx, member));
        if (member instanceof IMethodBinding mb && !mb.isConstructor()) {
            e.put("inherit", inheritData(ctx, mb));
        }
    }

    /** What CoreJavadocAccessImpl needs to know about the documented element. */
    private static Map<String, Object> docContext(Ctx ctx, IBinding member) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (member instanceof IMethodBinding mb) {
            m.put("kind", "method");
            m.put("isConstructor", mb.isConstructor());
            List<Object> tps = new ArrayList<>();
            for (ITypeBinding tp : mb.getTypeParameters()) tps.add(tp.getName());
            m.put("typeParameterNames", tps);
            String[] names = parameterNames(mb);
            m.put("parameterNames", names == null ? List.of() : List.of(names));
            List<Object> exc = new ArrayList<>();
            Map<String, Object> excLinks = new LinkedHashMap<>();
            for (ITypeBinding x : mb.getExceptionTypes()) {
                String simple = x.getErasure().getName();
                exc.add(simple);
                // handleSingleException links the simple name relative to the method
                Map<String, Object> loc = resolveRef(ctx, member, simple, null, null, null, null);
                if (loc != null) excLinks.put(simple, loc);
            }
            m.put("exceptionNames", exc);
            m.put("exceptionLinks", excLinks);
            m.put("returnsVoid", mb.getReturnType() == null || "void".equals(mb.getReturnType().getName()));
        } else if (member instanceof IVariableBinding vb) {
            m.put("kind", "field");
            int mods = vb.getModifiers();
            boolean sf = Modifier.isStatic(mods) && Modifier.isFinal(mods)
                    || vb.getDeclaringClass() != null && vb.getDeclaringClass().isInterface();
            m.put("staticFinal", sf);
            if (sf) {
                Object cv = vb.getConstantValue();
                if (cv != null) m.put("constant", constant(cv));
            }
        } else if (member instanceof ITypeBinding) {
            m.put("kind", "type");
        } else if (member instanceof IPackageBinding) {
            m.put("kind", "package");
        }
        return m;
    }

    private static void packageData(Ctx ctx, IPackageBinding pkg, Map<String, Object> e) {
        // package-info.java of a source package
        String name = pkg.getName();
        String best = null;
        for (String uri : ctx.files.keySet()) {
            if (uri.endsWith("/package-info.java") && name.equals(ctx.env.packageOfUri(uri))) {
                best = uri;
                break;
            }
        }
        boolean source = false;
        for (String uri : ctx.files.keySet()) {
            if (uri.endsWith(".java") && name.equals(ctx.env.packageOfUri(uri))) {
                source = true;
                if (e.get("sourceUri") == null) e.put("sourceUri", uri);
            }
        }
        e.put("isSource", source);
        if (best != null) {
            CompilationUnit cu = ctx.unit(best);
            if (cu != null && cu.getPackage() != null && cu.getPackage().getJavadoc() != null) {
                Decl d = new Decl(best, cu, ctx.files.get(best), cu.getPackage());
                IPackageBinding pb = cu.getPackage().resolveBinding();
                e.put("javadoc", docSource(ctx, d, cu.getPackage().getJavadoc(), pb != null ? pb : pkg));
                e.put("packageInfoUri", best);
            }
        }
        if (!source) {
            Map<String, Object> m = new LinkedHashMap<>();
            locateRoot(ctx, name.replace('.', '/'), m);
            if (m.get("root") == null) {
                // a package with only sub-packages, e.g. `javax`: find any class below it
                for (String cp : ctx.classpath) {
                    File f = new File(cp);
                    if (f.isFile()) {
                        try (JarFile jar = new JarFile(f)) {
                            if (jar.getEntry(name.replace('.', '/') + "/") != null) {
                                m.put("root", cp);
                                m.put("rootKind", "archive");
                                break;
                            }
                        } catch (IOException ex) {
                            // ignore
                        }
                    }
                }
            }
            e.put("classFile", m);
        }
    }

    // ─── Javadoc DOM serialization ───────────────────────────────────────────

    private static final class DocWriter {
        final Ctx ctx;
        final IBinding element;
        final int base;
        final List<Map<String, Object>> nodes = new ArrayList<>();
        final IdentityHashMap<ASTNode, Integer> ids = new IdentityHashMap<>();
        final List<Object[]> pendingRefs = new ArrayList<>();

        DocWriter(Ctx ctx, IBinding element, int base) {
            this.ctx = ctx;
            this.element = element;
            this.base = base;
        }

        int write(ASTNode n, int parent) {
            Map<String, Object> m = new LinkedHashMap<>();
            int id = nodes.size();
            nodes.add(m);
            ids.put(n, id);
            m.put("id", id);
            if (parent >= 0) m.put("p", parent);
            m.put("s", n.getStartPosition() - base);
            m.put("l", n.getLength());
            if ((n.getFlags() & ASTNode.MALFORMED) != 0) m.put("malformed", true);
            if (n instanceof TagElement te) {
                m.put("t", "tag");
                m.put("tagName", te.getTagName());
                List<Object> frags = new ArrayList<>();
                for (Object f : te.fragments()) frags.add(write((ASTNode) f, id));
                m.put("fragments", frags);
                List<Object> props = new ArrayList<>();
                try {
                    for (Object p : te.tagProperties()) props.add(write((ASTNode) p, id));
                } catch (UnsupportedOperationException ex) {
                    // below JLS18
                }
                if (!props.isEmpty()) m.put("tagProperties", props);
                snippetProps(te, m);
                if (isLinkTag(te.getTagName()) && !te.fragments().isEmpty()
                        && te.fragments().get(0) instanceof TextElement first) {
                    pendingRefs.add(new Object[] { first, first.getText(), null, null });
                }
            } else if (n instanceof TextElement t) {
                m.put("t", "text");
                m.put("text", t.getText());
            } else if (n instanceof JavaDocTextElement t) {
                m.put("t", "doctext");
                m.put("text", t.getText());
            } else if (n instanceof JavaDocRegion r) {
                m.put("t", "region");
                m.put("tagName", r.getTagName());
                List<Object> frags = new ArrayList<>();
                for (Object f : r.fragments()) frags.add(write((ASTNode) f, id));
                m.put("fragments", frags);
                List<Object> tags = new ArrayList<>();
                for (Object f : r.tags()) tags.add(write((ASTNode) f, id));
                m.put("tags", tags);
                m.put("dummy", r.isDummyRegion());
                snippetProps(r, m);
            } else if (n instanceof TagProperty tp) {
                m.put("t", "tagProperty");
                m.put("name", tp.getName());
                m.put("stringValue", tp.getStringValue());
                if (tp.getNodeValue() != null) m.put("nodeValue", write(tp.getNodeValue(), id));
            } else if (n instanceof SimpleName sn) {
                m.put("t", "name");
                m.put("fqn", sn.getFullyQualifiedName());
                m.put("identifier", sn.getIdentifier());
                if (!(n.getParent() instanceof MemberRef) && !(n.getParent() instanceof MethodRef)
                        && !(n.getParent() instanceof MethodRefParameter) && !(n.getParent() instanceof QualifiedName)
                        && !(n.getParent() instanceof Type)) {
                    pendingRefs.add(new Object[] { n, sn.getFullyQualifiedName(), null, null });
                }
            } else if (n instanceof QualifiedName qn) {
                m.put("t", "name");
                m.put("fqn", qn.getFullyQualifiedName());
                m.put("identifier", qn.getName().getIdentifier());
                if (!(n.getParent() instanceof MemberRef) && !(n.getParent() instanceof MethodRef)
                        && !(n.getParent() instanceof QualifiedName) && !(n.getParent() instanceof Type)) {
                    pendingRefs.add(new Object[] { n, qn.getFullyQualifiedName(), null, null });
                }
            } else if (n instanceof MemberRef mr) {
                m.put("t", "memberRef");
                String q = mr.getQualifier() == null ? "" : mr.getQualifier().getFullyQualifiedName();
                m.put("qualifier", mr.getQualifier() == null ? null : q);
                m.put("name", mr.getName().getIdentifier());
                pendingRefs.add(new Object[] { n, q, mr.getName().getIdentifier(), null });
                TagElement parentTag = n.getParent() instanceof TagElement pt ? pt : null;
                if (parentTag != null && TagElement.TAG_VALUE.equals(parentTag.getTagName())) {
                    Map<String, Object> v = valueRef(mr);
                    if (v != null) m.put("value", v);
                }
            } else if (n instanceof MethodRef mr) {
                m.put("t", "methodRef");
                String q = mr.getQualifier() == null ? "" : mr.getQualifier().getFullyQualifiedName();
                m.put("qualifier", mr.getQualifier() == null ? null : q);
                m.put("name", mr.getName().getIdentifier());
                List<Object> params = new ArrayList<>();
                List<String> types = new ArrayList<>();
                for (Object o : mr.parameters()) {
                    MethodRefParameter p = (MethodRefParameter) o;
                    Map<String, Object> pm = new LinkedHashMap<>();
                    String ts = asString(p.getType());
                    pm.put("type", ts);
                    pm.put("name", p.getName() == null ? null : p.getName().getIdentifier());
                    params.add(pm);
                    types.add(ts);
                }
                m.put("params", params);
                pendingRefs.add(new Object[] { n, q, mr.getName().getIdentifier(), types });
            } else {
                m.put("t", "other");
            }
            return id;
        }

        private void snippetProps(ASTNode n, Map<String, Object> m) {
            Map<String, Object> props = new LinkedHashMap<>();
            Object valid = n.getProperty(TagProperty.TAG_PROPERTY_SNIPPET_IS_VALID);
            if (valid != null) props.put("valid", valid);
            Object err = n.getProperty(TagProperty.TAG_PROPERTY_SNIPPET_ERROR);
            if (err != null) props.put("error", String.valueOf(err));
            Object sid = n.getProperty(TagProperty.TAG_PROPERTY_SNIPPET_ID);
            if (sid != null) props.put("id", String.valueOf(sid));
            Object cnt = n.getProperty(TagProperty.TAG_PROPERTY_SNIPPET_INLINE_TAG_COUNT);
            if (cnt instanceof Integer i) props.put("inlineTagCount", i);
            Object rt = n.getProperty(TagProperty.TAG_PROPERTY_SNIPPET_REGION_TEXT);
            if (rt instanceof ASTNode rn) props.put("regionTextNode", rn);
            if (!props.isEmpty()) m.put("props", props);
        }

        /** {@code handleValueTag} for a member reference: constant + field link. */
        private Map<String, Object> valueRef(MemberRef mr) {
            IBinding el = element;
            ITypeBinding type;
            if (el instanceof ITypeBinding tb && !tb.isTypeVariable()) {
                type = tb;
            } else if (el instanceof IMethodBinding mb) {
                type = mb.getDeclaringClass();
            } else if (el instanceof IVariableBinding vb && vb.isField()) {
                type = vb.getDeclaringClass();
            } else {
                return null;
            }
            if (mr.getQualifier() != null) {
                IBinding qb = mr.getQualifier().resolveBinding();
                if (qb instanceof ITypeBinding qt) {
                    type = qt;
                }
            }
            String fieldName = mr.getName().getIdentifier();
            while (type != null) {
                for (IVariableBinding f : type.getTypeDeclaration().getDeclaredFields()) {
                    if (f.getName().equals(fieldName)) {
                        int mods = f.getModifiers();
                        boolean sf = Modifier.isStatic(mods) && Modifier.isFinal(mods) || type.isInterface();
                        if (!sf) return null;
                        Object cv = f.getConstantValue();
                        if (cv == null) return null;
                        Map<String, Object> v = new LinkedHashMap<>();
                        v.put("constant", constant(cv));
                        Map<String, Object> loc = location(ctx, f);
                        if (loc != null) v.put("location", loc);
                        return v;
                    }
                }
                type = type.getDeclaringClass();
            }
            return null;
        }

        void finish() {
            for (Map<String, Object> m : nodes) {
                @SuppressWarnings("unchecked")
                Map<String, Object> props = (Map<String, Object>) m.get("props");
                if (props != null && props.get("regionTextNode") instanceof ASTNode rn) {
                    Integer rid = ids.get(rn);
                    props.remove("regionTextNode");
                    if (rid != null) props.put("regionText", rid);
                }
            }
            for (Object[] ref : pendingRefs) {
                ASTNode n = (ASTNode) ref[0];
                Integer id = ids.get(n);
                if (id == null) continue;
                @SuppressWarnings("unchecked")
                List<String> params = (List<String>) ref[3];
                Map<String, Object> loc = resolveRef(ctx, element, (String) ref[1], (String) ref[2], params, n, null);
                nodes.get(id).put("link", loc == null ? Map.of() : loc);
            }
        }
    }

    private static boolean isLinkTag(String name) {
        return TagElement.TAG_LINK.equals(name) || TagElement.TAG_LINKPLAIN.equals(name);
    }

    private static String asString(Type t) {
        return t == null ? "" : t.toString();
    }

    private static Map<String, Object> docSource(Ctx ctx, Decl d, Javadoc jd, IBinding element) {
        int base = jd.getStartPosition();
        DocWriter w = new DocWriter(ctx, element, base);
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("raw", d.source.substring(base, base + jd.getLength()));
        m.put("uri", d.uri);
        List<Object> tags = new ArrayList<>();
        for (Object t : jd.tags()) tags.add(w.write((ASTNode) t, -1));
        w.finish();
        m.put("tags", tags);
        m.put("nodes", w.nodes);
        m.put("markdown", jd.isMarkdown());
        return m;
    }

    // ─── Link resolution (CoreJavaElementLinks.parseURI + JDTUtils.toLocation) ─

    /**
     * Resolves a Javadoc reference the way jdt.ls {@code createLinkURIHelper}
     * does and returns the target location, or null when it has none.
     */
    static Map<String, Object> resolveRef(Ctx ctx, IBinding element, String refTypeName, String refMemberName,
            List<String> paramTypes, ASTNode node, Object unused) {
        if (refTypeName == null) return null;
        if (!refTypeName.isEmpty() && refTypeName.indexOf('/') == -1 && refTypeName.indexOf('.') == -1) {
            ITypeBinding tv = resolveTypeVariable(element, refTypeName);
            if (tv != null) {
                return location(ctx, tv);
            }
        }
        if (element instanceof IPackageBinding) {
            ITypeBinding t = typeOfRefNode(node);
            if (t == null) return null;
            return member(ctx, t, refMemberName, paramTypes, node);
        }
        ITypeBinding base;
        if (element instanceof ITypeBinding tb && !tb.isTypeVariable()) {
            base = tb;
        } else if (element instanceof ITypeBinding tv) {
            IBinding m = declaringMemberOfTypeVariable(tv);
            base = m instanceof ITypeBinding mt ? mt : m instanceof IMethodBinding mm ? mm.getDeclaringClass() : null;
        } else if (element instanceof IMethodBinding mb) {
            base = mb.getDeclaringClass();
        } else if (element instanceof IVariableBinding vb) {
            base = vb.getDeclaringClass();
            if (base == null && vb.getDeclaringMethod() != null) base = vb.getDeclaringMethod().getDeclaringClass();
        } else {
            return null;
        }
        if (base == null) return null;
        ITypeBinding type = base;
        if (!refTypeName.isEmpty()) {
            type = typeOfRefNode(node);
            if (type == null) {
                return null; // package (no location) or module or unresolved
            }
        }
        return member(ctx, type, refMemberName, paramTypes, node);
    }

    private static Map<String, Object> member(Ctx ctx, ITypeBinding type, String refMemberName, List<String> paramTypes, ASTNode node) {
        type = type.getTypeDeclaration();
        if (refMemberName == null) {
            return location(ctx, type);
        }
        if (paramTypes != null) {
            if (node instanceof MethodRef mr) {
                IBinding b = mr.resolveBinding();
                if (b instanceof IMethodBinding mb && mb.getMethodDeclaration().getDeclaringClass().getTypeDeclaration().isEqualTo(type)) {
                    return location(ctx, mb.getMethodDeclaration());
                }
            }
            for (IMethodBinding m : declaredMethodsInOrder(ctx, type)) {
                if (methodName(m).equals(refMemberName) && m.getParameterTypes().length == paramTypes.size()) {
                    return location(ctx, m);
                }
            }
            IMethodBinding inHierarchy = findMethodInHierarchy(type, refMemberName, paramTypes);
            if (inHierarchy != null) {
                return location(ctx, inHierarchy.getMethodDeclaration());
            }
        } else {
            for (IVariableBinding f : type.getDeclaredFields()) {
                if (f.getName().equals(refMemberName)) {
                    return location(ctx, f);
                }
            }
            for (IMethodBinding m : declaredMethodsInOrder(ctx, type)) {
                if (methodName(m).equals(refMemberName)) {
                    return location(ctx, m);
                }
            }
        }
        return location(ctx, type);
    }

    private static String methodName(IMethodBinding m) {
        return m.isConstructor() ? m.getDeclaringClass().getName() : m.getName();
    }

    private static IMethodBinding findMethodInHierarchy(ITypeBinding type, String name, List<String> paramTypes) {
        List<ITypeBinding> queue = new ArrayList<>();
        if (type.getSuperclass() != null) queue.add(type.getSuperclass());
        Collections.addAll(queue, type.getInterfaces());
        for (int i = 0; i < queue.size(); i++) {
            ITypeBinding t = queue.get(i).getTypeDeclaration();
            for (IMethodBinding m : t.getDeclaredMethods()) {
                if (m.getName().equals(name) && m.getParameterTypes().length == paramTypes.size()) {
                    boolean same = true;
                    for (int k = 0; k < paramTypes.size(); k++) {
                        String want = simple(paramTypes.get(k));
                        String have = m.getParameterTypes()[k].getErasure().getName();
                        if (!want.equals(have)) {
                            same = false;
                            break;
                        }
                    }
                    if (same) return m;
                }
            }
            if (t.getSuperclass() != null) queue.add(t.getSuperclass());
            Collections.addAll(queue, t.getInterfaces());
        }
        return null;
    }

    private static String simple(String typeName) {
        String s = typeName;
        int lt = s.indexOf('<');
        if (lt >= 0) s = s.substring(0, lt);
        int dot = s.lastIndexOf('.');
        return s.substring(dot + 1);
    }

    /** Declared methods in declaration order (source order for source types). */
    private static List<IMethodBinding> declaredMethodsInOrder(Ctx ctx, ITypeBinding type) {
        List<IMethodBinding> out = new ArrayList<>();
        if (type.isFromSource()) {
            Decl d = decl(ctx, type);
            if (d != null && d.node instanceof AbstractTypeDeclaration td) {
                for (Object o : td.bodyDeclarations()) {
                    if (o instanceof MethodDeclaration md) {
                        IMethodBinding mb = md.resolveBinding();
                        if (mb != null) out.add(mb);
                    }
                }
                if (!out.isEmpty()) return out;
            }
        }
        Collections.addAll(out, type.getDeclaredMethods());
        return out;
    }

    private static ITypeBinding typeOfRefNode(ASTNode node) {
        Name n = null;
        if (node instanceof Name name) n = name;
        else if (node instanceof MemberRef mr) n = mr.getQualifier();
        else if (node instanceof MethodRef mr) n = mr.getQualifier();
        if (n == null) return null;
        IBinding b = n.resolveBinding();
        if (b instanceof ITypeBinding t && !t.isRecovered()) {
            return t;
        }
        return null;
    }

    private static ITypeBinding resolveTypeVariable(IBinding element, String name) {
        IBinding cur = element;
        if (cur instanceof IVariableBinding v && !v.isField()) {
            cur = v.getDeclaringMethod();
        }
        while (cur != null) {
            if (cur instanceof IMethodBinding m) {
                for (ITypeBinding tp : m.getMethodDeclaration().getTypeParameters()) {
                    if (tp.getName().equals(name)) return tp;
                }
                cur = m.getDeclaringClass();
            } else if (cur instanceof ITypeBinding t) {
                if (t.isTypeVariable()) {
                    cur = declaringMemberOfTypeVariable(t);
                    continue;
                }
                for (ITypeBinding tp : t.getTypeDeclaration().getTypeParameters()) {
                    if (tp.getName().equals(name)) return tp;
                }
                cur = t.getDeclaringMethod() != null ? t.getDeclaringMethod() : t.getDeclaringClass();
            } else if (cur instanceof IVariableBinding v) {
                cur = v.getDeclaringClass();
            } else {
                return null;
            }
        }
        return null;
    }

    // ─── @inheritDoc data (InheritDocVisitor / MethodOverrideTester inputs) ──

    private static Map<String, Object> inheritData(Ctx ctx, IMethodBinding method) {
        Map<String, Object> m = new LinkedHashMap<>();
        ITypeBinding start = method.getDeclaringClass();
        Map<String, Map<String, Object>> types = new LinkedHashMap<>();
        List<ITypeBinding> queue = new ArrayList<>();
        queue.add(start);
        ITypeBinding object = ctx.ast.resolveWellKnownType("java.lang.Object");
        if (start.isInterface() && object != null) {
            queue.add(object);
        }
        for (int i = 0; i < queue.size(); i++) {
            ITypeBinding t = queue.get(i);
            String key = t.getTypeDeclaration().getKey();
            if (types.containsKey(key)) continue;
            Map<String, Object> tm = new LinkedHashMap<>();
            types.put(key, tm);
            tm.put("key", key);
            tm.put("name", t.getTypeDeclaration().getName());
            tm.put("isInterface", t.isInterface());
            ITypeBinding sup = t.getSuperclass();
            if (sup != null) {
                tm.put("superclass", sup.getTypeDeclaration().getKey());
                queue.add(sup);
            }
            List<Object> itfs = new ArrayList<>();
            for (ITypeBinding itf : t.getInterfaces()) {
                itfs.add(itf.getTypeDeclaration().getKey());
                queue.add(itf);
            }
            tm.put("interfaces", itfs);
            if (i > 0 || (start.isInterface() && t == object)) {
                IMethodBinding overridden = overriddenIn(method, t);
                if (overridden != null) {
                    tm.put("overridden", overriddenDoc(ctx, overridden.getMethodDeclaration()));
                }
            }
        }
        m.put("start", start.getTypeDeclaration().getKey());
        m.put("object", object == null ? null : object.getTypeDeclaration().getKey());
        m.put("types", new ArrayList<>(types.values()));
        return m;
    }

    private static IMethodBinding overriddenIn(IMethodBinding method, ITypeBinding type) {
        for (IMethodBinding candidate : type.getDeclaredMethods()) {
            if (candidate.isConstructor() || !candidate.getName().equals(method.getName())) continue;
            if (candidate.getParameterTypes().length != method.getParameterTypes().length) continue;
            if (method.overrides(candidate) || method.isSubsignature(candidate)) {
                return candidate;
            }
        }
        return null;
    }

    private static Map<String, Object> overriddenDoc(Ctx ctx, IMethodBinding m) {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("name", m.getName());
        o.put("hasParameters", m.getParameterTypes().length > 0);
        o.put("declaringTypeName", m.getDeclaringClass().getTypeDeclaration().getName());
        Decl d = decl(ctx, m);
        o.put("hasSource", d != null);
        o.put("docContext", docContext(ctx, m));
        if (d != null) {
            Javadoc jd = javadocOf(d.node);
            if (jd != null) {
                o.put("javadoc", docSource(ctx, d, jd, m));
            }
        }
        return o;
    }
}
