package com.jdtls.ecjbridge;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.logging.Level;
import java.util.logging.Logger;

import org.eclipse.jdt.core.dom.*;
import org.eclipse.jdt.internal.core.dom.ICompilationUnitResolver;

import com.jdtls.ecjbridge.BridgeProtocol.SigCandidate;
import com.jdtls.ecjbridge.BridgeProtocol.SigNode;
import com.jdtls.ecjbridge.BridgeProtocol.SignatureHelpDataResponse;

/**
 * Data provider for signature help.
 *
 * jdt.ls drives signature help with the JDT completion engine over the Java
 * model.  The bridge has no Java model, so this service answers with plain
 * data instead: the method-like AST nodes around the request offsets (as the
 * JDT DOM with statement and binding recovery sees them) and, for each, the
 * method bindings the completion engine would propose.  Selection of the
 * active signature/parameter, labels and LSP shaping happen in Rust
 * (`src/features/signature_help.rs`).
 */
final class SignatureHelpService {

    private static final Logger LOG = Logger.getLogger(SignatureHelpService.class.getName());

    SignatureHelpDataResponse compute(long id, Map<String, String> files, List<String> classpath, String sourceLevel,
            String uri, int searchOffset, int contextOffset, String fallbackName, boolean description) {
        SignatureHelpDataResponse response = new SignatureHelpDataResponse(id);
        String source = files == null ? null : files.get(uri);
        if (source == null) {
            return response;
        }
        InMemorySourceClasspath sources = new InMemorySourceClasspath(files, uri);
        CompilationUnit cu = parse(source, uri, classpath, sourceLevel, sources);
        if (cu == null) {
            return response;
        }
        Context ctx = new Context(cu, source, sources, description);
        if (searchOffset >= 0) {
            ASTNode node = NodeFinder.perform(cu, searchOffset, 0);
            for (ASTNode n = node; n != null && !(n instanceof Block); n = n.getParent()) {
                if (isMethodLike(n)) {
                    response.chain.add(describe(ctx, n));
                }
            }
        }
        if (contextOffset >= 0) {
            ASTNode node = fallbackNode(cu, contextOffset);
            if (node != null) {
                SigNode info = describe(ctx, node);
                info.boundMethod = boundMethod(ctx, node);
                String name = info.boundMethod != null ? info.boundMethod.name
                        : (node instanceof Block ? fallbackName : null);
                if (name != null) {
                    info.scopeCandidates = toCandidates(ctx, scopeMethods(ctx, node, name), null);
                    if (node instanceof Block) {
                        info.methodName = fallbackName;
                        info.candidates = info.scopeCandidates;
                    }
                }
                response.fallback = info;
            }
        }
        return response;
    }

    // ── Parsing ─────────────────────────────────────────────────────────────

    private CompilationUnit parse(String source, String uri, List<String> classpath, String sourceLevel,
            InMemorySourceClasspath sources) {
        try {
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            parser.setSource(source.toCharArray());
            parser.setKind(ASTParser.K_COMPILATION_UNIT);
            parser.setResolveBindings(true);
            parser.setBindingsRecovery(true);
            parser.setStatementsRecovery(true);
            parser.setCompilerOptions(BridgeOptions.map(sourceLevel));
            parser.setUnitName(InMemorySourceClasspath.fileName(uri));
            BridgeOptions.configureEnvironment(parser, classpath == null ? new String[0] : classpath.toArray(new String[0]));
            injectSources(parser, sources);
            return (CompilationUnit) parser.createAST(null);
        } catch (RuntimeException e) {
            LOG.log(Level.WARNING, "signature help parse failed", e);
            return null;
        }
    }

    /**
     * Put the in-memory documents in front of the parser's classpath, so types
     * of other compilation units resolve from source (DOM only reads source
     * folders from disk otherwise).
     */
    @SuppressWarnings("unchecked")
    static void injectSources(ASTParser parser, InMemorySourceClasspath sources) {
        try {
            Field f = ASTParser.class.getDeclaredField("unitResolver");
            f.setAccessible(true);
            ICompilationUnitResolver delegate = (ICompilationUnitResolver) f.get(parser);
            InvocationHandler handler = (proxy, method, args) -> {
                if (method.getName().equals("toCompilationUnit") && args != null && args.length > 3
                        && args[3] instanceof List<?> list) {
                    List<Object> cp = new ArrayList<>();
                    cp.add(sources);
                    cp.addAll((List<Object>) list);
                    args[3] = cp;
                }
                try {
                    return method.invoke(delegate, args);
                } catch (InvocationTargetException e) {
                    throw e.getCause();
                }
            };
            Object proxy = Proxy.newProxyInstance(SignatureHelpService.class.getClassLoader(),
                    new Class<?>[] { ICompilationUnitResolver.class }, handler);
            f.set(parser, proxy);
        } catch (ReflectiveOperationException | RuntimeException e) {
            LOG.log(Level.WARNING, "cannot inject in-memory sources; other compilation units will not resolve", e);
        }
    }

    // ── Nodes ───────────────────────────────────────────────────────────────

    private static boolean isMethodLike(ASTNode node) {
        return node instanceof MethodInvocation || node instanceof ClassInstanceCreation
                || node instanceof SuperConstructorInvocation || node instanceof ConstructorInvocation
                || node instanceof SuperMethodInvocation || node instanceof MethodRef;
    }

    /** `SignatureHelpHandler.getNode` without the method-name check of blocks (done in Rust). */
    private static ASTNode fallbackNode(CompilationUnit cu, int offset) {
        ASTNode node = NodeFinder.perform(cu, offset, 1);
        if (node instanceof MethodInvocation || node instanceof ClassInstanceCreation || node instanceof MethodRef) {
            return node;
        }
        if (node instanceof Block) {
            return node;
        }
        if (node instanceof Expression) {
            node = node.getParent();
            if (node instanceof MethodInvocation || node instanceof ClassInstanceCreation || node instanceof MethodRef) {
                return node;
            }
        }
        return null;
    }

    private SigNode describe(Context ctx, ASTNode node) {
        SigNode info = new SigNode();
        info.kind = node.getClass().getSimpleName();
        info.start = node.getStartPosition();
        info.length = node.getLength();
        ASTNode nameNode = null;
        Expression optional = null;
        List<?> arguments = null;
        IBinding binding = null;
        if (node instanceof MethodInvocation mi) {
            nameNode = mi.getName();
            optional = mi.getExpression();
            arguments = mi.arguments();
            info.methodName = mi.getName().getIdentifier();
            binding = mi.resolveMethodBinding();
        } else if (node instanceof ClassInstanceCreation cic) {
            nameNode = cic.getType();
            optional = cic.getExpression();
            arguments = cic.arguments();
            ITypeBinding type = cic.getType().resolveBinding();
            if (type != null) {
                info.methodName = type.getErasure().getName();
            }
            binding = cic.resolveConstructorBinding();
        } else if (node instanceof SuperMethodInvocation smi) {
            nameNode = smi.getName();
            arguments = smi.arguments();
            info.methodName = smi.getName().getIdentifier();
            binding = smi.resolveMethodBinding();
        } else if (node instanceof MethodRef ref) {
            nameNode = ref.getName();
            arguments = ref.parameters();
            info.methodName = ref.getName().getIdentifier();
            binding = ref.resolveBinding();
        } else if (node instanceof ConstructorInvocation ci) {
            nameNode = ci;
            arguments = ci.arguments();
            IMethodBinding b = ci.resolveConstructorBinding();
            if (b != null) {
                info.methodName = b.getDeclaringClass().getName();
            }
            binding = b;
        } else if (node instanceof SuperConstructorInvocation sci) {
            nameNode = sci;
            optional = sci.getExpression();
            arguments = sci.arguments();
            IMethodBinding b = sci.resolveConstructorBinding();
            if (b != null) {
                info.methodName = b.getDeclaringClass().getName();
            }
            binding = b;
        }
        if (nameNode != null) {
            info.nameEnd = nameNode.getStartPosition() + nameNode.getLength();
        }
        info.optionalExpressionLength = optional == null ? 0 : optional.getLength();
        if (arguments != null) {
            info.arguments = new ArrayList<>();
            for (Object a : arguments) {
                ASTNode arg = (ASTNode) a;
                info.arguments.add(new int[] { arg.getStartPosition(), arg.getLength() });
            }
        }
        if (binding instanceof IMethodBinding mb) {
            if (mb.isDefaultConstructor()) {
                info.parameterTypes = new ArrayList<>();
            } else {
                info.parameterTypesFromBinding = new ArrayList<>();
                for (ITypeBinding t : mb.getParameterTypes()) {
                    info.parameterTypesFromBinding.add(t.getErasure().getName().replace(";", ""));
                }
                info.parameterTypes = new ArrayList<>();
                for (ITypeBinding t : mb.getMethodDeclaration().getParameterTypes()) {
                    info.parameterTypes.add(simpleTypeName(display(t)));
                }
            }
        }
        if (info.methodName != null) {
            info.candidates = toCandidates(ctx, primaryMethods(ctx, node, info.methodName), info.methodName);
            if ((node instanceof MethodInvocation mi && mi.getExpression() != null)
                    || node instanceof SuperMethodInvocation) {
                info.secondaryCandidates = toCandidates(ctx, scopeMethods(ctx, node, info.methodName), null);
            }
        }
        if (node instanceof ClassInstanceCreation cic) {
            info.declaredConstructors = declaredConstructors(ctx, cic);
        }
        return info;
    }

    /** `SignatureHelpHandler.getMethod`: the method the node resolves to, or null. */
    private SigCandidate boundMethod(Context ctx, ASTNode node) {
        IBinding binding = null;
        if (node instanceof MethodInvocation mi) {
            binding = mi.resolveMethodBinding();
        } else if (node instanceof MethodRef ref) {
            binding = ref.resolveBinding();
        } else if (node instanceof ClassInstanceCreation cic) {
            binding = cic.resolveConstructorBinding();
        }
        if (binding instanceof IMethodBinding mb && !mb.isDefaultConstructor()) {
            return toCandidate(ctx, mb, null);
        }
        return null;
    }

    // ── Candidate collection (what the completion engine proposes) ─────────

    private static final class Context {
        final CompilationUnit cu;
        final String source;
        final InMemorySourceClasspath sources;
        final boolean description;
        final String packageName;

        Context(CompilationUnit cu, String source, InMemorySourceClasspath sources, boolean description) {
            this.cu = cu;
            this.source = source;
            this.sources = sources;
            this.description = description;
            PackageDeclaration pkg = cu.getPackage();
            this.packageName = pkg == null ? "" : pkg.getName().getFullyQualifiedName();
        }
    }

    private List<IMethodBinding> primaryMethods(Context ctx, ASTNode node, String name) {
        List<IMethodBinding> out = new ArrayList<>();
        ITypeBinding site = enclosingType(node);
        if (node instanceof MethodInvocation mi) {
            Expression expr = mi.getExpression();
            if (expr == null) {
                return scopeMethods(ctx, node, name);
            }
            ITypeBinding receiver = expr.resolveTypeBinding();
            boolean staticOnly = expr instanceof Name n && n.resolveBinding() instanceof ITypeBinding;
            if (receiver != null) {
                collectMethods(ctx, receiver, name, staticOnly, site, out);
            }
        } else if (node instanceof SuperMethodInvocation smi) {
            ITypeBinding type = null;
            if (smi.getQualifier() != null) {
                IBinding q = smi.getQualifier().resolveBinding();
                if (q instanceof ITypeBinding qt) {
                    type = qt.isInterface() ? qt : qt.getSuperclass();
                }
            } else if (site != null) {
                type = site.getSuperclass();
            }
            if (type != null) {
                collectMethods(ctx, type, name, false, site, out);
            }
        } else if (node instanceof MethodRef ref) {
            ITypeBinding type = null;
            if (ref.getQualifier() != null) {
                IBinding q = ref.getQualifier().resolveBinding();
                if (q instanceof ITypeBinding qt) {
                    type = qt;
                }
            } else {
                type = site;
            }
            if (type != null) {
                collectMethods(ctx, type, name, false, site, out);
            }
        } else if (node instanceof ClassInstanceCreation cic) {
            ITypeBinding type = cic.getType().resolveBinding();
            if (type != null && !type.isInterface()) {
                collectConstructors(type, site, false, out);
            }
        } else if (node instanceof ConstructorInvocation) {
            if (site != null) {
                collectConstructors(site, site, true, out);
            }
        } else if (node instanceof SuperConstructorInvocation) {
            if (site != null && site.getSuperclass() != null) {
                collectConstructors(site.getSuperclass(), site, true, out);
            }
        }
        return out;
    }

    /** Methods named `name` visible without qualification at `node` (enclosing types, static imports). */
    private List<IMethodBinding> scopeMethods(Context ctx, ASTNode node, String name) {
        List<IMethodBinding> out = new ArrayList<>();
        ITypeBinding site = enclosingType(node);
        boolean staticsOnly = false;
        for (ASTNode n = node; n != null; n = n.getParent()) {
            if (n instanceof BodyDeclaration bd && !(n instanceof AbstractTypeDeclaration)
                    && Modifier.isStatic(bd.getModifiers())) {
                staticsOnly = true;
            }
            ITypeBinding type = null;
            if (n instanceof AbstractTypeDeclaration td) {
                type = td.resolveBinding();
            } else if (n instanceof AnonymousClassDeclaration acd) {
                type = acd.resolveBinding();
            }
            if (type != null) {
                collectMethods(ctx, type, name, staticsOnly, site, out);
                if (type.isInterface() || type.isEnum() || type.isRecord()
                        || (type.isMember() && Modifier.isStatic(type.getModifiers()))) {
                    staticsOnly = true;
                }
            }
        }
        for (Object o : ctx.cu.imports()) {
            ImportDeclaration imp = (ImportDeclaration) o;
            if (!imp.isStatic()) {
                continue;
            }
            ITypeBinding type = null;
            Name importName = imp.getName();
            if (imp.isOnDemand()) {
                IBinding b = importName.resolveBinding();
                if (b instanceof ITypeBinding tb) {
                    type = tb;
                }
            } else if (importName instanceof QualifiedName qn && qn.getName().getIdentifier().equals(name)) {
                IBinding b = qn.getQualifier().resolveBinding();
                if (b instanceof ITypeBinding tb) {
                    type = tb;
                }
            }
            if (type != null) {
                collectMethods(ctx, type, name, true, site, out);
            }
        }
        return out;
    }

    /** `CompletionEngine.findMethods`: the type's methods, then its supertypes', hidden ones filtered. */
    private void collectMethods(Context ctx, ITypeBinding receiver, String name, boolean staticsOnly,
            ITypeBinding site, List<IMethodBinding> out) {
        if (receiver.isCapture() || receiver.isWildcardType() || receiver.isTypeVariable()) {
            ITypeBinding[] bounds = receiver.getTypeBounds();
            ITypeBinding bound = receiver.isCapture() ? receiver.getWildcard().getBound() : null;
            if (bounds != null && bounds.length > 0) {
                for (ITypeBinding b : bounds) {
                    collectMethods(ctx, b, name, staticsOnly, site, out);
                }
                return;
            }
            if (bound != null && receiver.getWildcard().isUpperbound()) {
                collectMethods(ctx, bound, name, staticsOnly, site, out);
                return;
            }
            ITypeBinding object = ctx.cu.getAST().resolveWellKnownType("java.lang.Object");
            if (object != null) {
                collectMethods(ctx, object, name, staticsOnly, site, out);
            }
            return;
        }
        if (receiver.isArray()) {
            receiver = ctx.cu.getAST().resolveWellKnownType("java.lang.Object");
            if (receiver == null) return;
        }
        List<ITypeBinding> order = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        if (receiver.isInterface()) {
            addInterfaces(receiver, order, seen);
            ITypeBinding object = ctx.cu.getAST().resolveWellKnownType("java.lang.Object");
            if (object != null && seen.add(object.getErasure().getKey())) {
                order.add(object);
            }
        } else {
            List<ITypeBinding> classes = new ArrayList<>();
            for (ITypeBinding t = receiver; t != null; t = t.getSuperclass()) {
                if (seen.add(t.getErasure().getKey())) {
                    classes.add(t);
                    order.add(t);
                }
            }
            for (ITypeBinding c : classes) {
                for (ITypeBinding i : c.getInterfaces()) {
                    addInterfaces(i, order, seen);
                }
            }
        }
        for (ITypeBinding t : order) {
            IMethodBinding[] methods = t.getDeclaredMethods();
            for (int i = methods.length - 1; i >= 0; i--) {
                IMethodBinding m = methods[i];
                if (m.isConstructor() || !name.equals(m.getName())) continue;
                boolean isStatic = Modifier.isStatic(m.getModifiers());
                if (staticsOnly && !isStatic) continue;
                if (isStatic && t.isInterface() && t != receiver
                        && !t.getErasure().isEqualTo(receiver.getErasure())) continue;
                if (!isAccessible(m, site, ctx, false)) continue;
                if (isHidden(m, out)) continue;
                out.add(m);
            }
        }
    }

    private static void addInterfaces(ITypeBinding type, List<ITypeBinding> order, Set<String> seen) {
        if (!seen.add(type.getErasure().getKey())) return;
        order.add(type);
        for (ITypeBinding i : type.getInterfaces()) {
            addInterfaces(i, order, seen);
        }
    }

    private static boolean isHidden(IMethodBinding m, List<IMethodBinding> found) {
        for (IMethodBinding f : found) {
            if (!f.getName().equals(m.getName())) continue;
            if (f.isSubsignature(m) || m.isSubsignature(f) || sameErasedParameters(f, m)) {
                return true;
            }
        }
        return false;
    }

    private static boolean sameErasedParameters(IMethodBinding a, IMethodBinding b) {
        ITypeBinding[] pa = a.getParameterTypes();
        ITypeBinding[] pb = b.getParameterTypes();
        if (pa.length != pb.length) return false;
        for (int i = 0; i < pa.length; i++) {
            if (!pa[i].getErasure().getQualifiedName().equals(pb[i].getErasure().getQualifiedName())) {
                return false;
            }
        }
        return true;
    }

    private void collectConstructors(ITypeBinding type, ITypeBinding site, boolean explicitInvocation,
            List<IMethodBinding> out) {
        IMethodBinding[] methods = type.getDeclaredMethods();
        for (int i = methods.length - 1; i >= 0; i--) {
            IMethodBinding m = methods[i];
            if (!m.isConstructor()) continue;
            if (!isAccessible(m, site, null, !explicitInvocation)) continue;
            out.add(m);
        }
    }

    private List<SigCandidate> declaredConstructors(Context ctx, ClassInstanceCreation cic) {
        IMethodBinding binding = cic.resolveConstructorBinding();
        if (binding == null) return null;
        ITypeBinding type = binding.getDeclaringClass();
        if (type == null || type.isAnonymous()) return null;
        List<SigCandidate> out = new ArrayList<>();
        if (binding.isDefaultConstructor()) {
            SigCandidate c = toCandidate(ctx, binding, binding.getName());
            c.parameterTypes = new ArrayList<>();
            c.parameterNames = new ArrayList<>();
            c.matchTypes = new ArrayList<>();
            c.declaredTypes = new ArrayList<>();
            c.varargs = false;
            out.add(c);
            return out;
        }
        for (IMethodBinding m : type.getErasure().getDeclaredMethods()) {
            if (m.isConstructor() && !m.isDefaultConstructor()) {
                out.add(toCandidate(ctx, m, null));
            }
        }
        return out;
    }

    private static ITypeBinding enclosingType(ASTNode node) {
        for (ASTNode n = node; n != null; n = n.getParent()) {
            if (n instanceof AbstractTypeDeclaration td) {
                return td.resolveBinding();
            }
            if (n instanceof AnonymousClassDeclaration acd) {
                return acd.resolveBinding();
            }
        }
        return null;
    }

    private static ITypeBinding outermost(ITypeBinding t) {
        ITypeBinding cur = t.getErasure();
        while (cur.getDeclaringClass() != null) {
            cur = cur.getDeclaringClass().getErasure();
        }
        return cur;
    }

    private static String packageOf(ITypeBinding t) {
        IPackageBinding p = t.getErasure().getPackage();
        return p == null ? "" : p.getName();
    }

    private static boolean isAccessible(IMethodBinding m, ITypeBinding site, Context ctx, boolean allocation) {
        ITypeBinding declaring = m.getDeclaringClass();
        if (declaring == null) return true;
        int mod = m.getModifiers();
        if (declaring.isInterface() && !Modifier.isPrivate(mod)) return true;
        if (Modifier.isPublic(mod)) return true;
        if (site == null) return false;
        boolean samePackage = packageOf(declaring).equals(packageOf(site));
        if (Modifier.isPrivate(mod)) {
            return outermost(declaring).isEqualTo(outermost(site));
        }
        if (Modifier.isProtected(mod)) {
            if (samePackage) return true;
            if (allocation) return false;
            for (ITypeBinding s = site; s != null; s = s.getDeclaringClass()) {
                if (s.getErasure().isSubTypeCompatible(declaring.getErasure())) return true;
            }
            return false;
        }
        return samePackage;
    }

    // ── Candidate data ──────────────────────────────────────────────────────

    private List<SigCandidate> toCandidates(Context ctx, List<IMethodBinding> methods, String nameOverride) {
        List<SigCandidate> out = new ArrayList<>();
        for (IMethodBinding m : methods) {
            out.add(toCandidate(ctx, m, m.isConstructor() ? nameOverride : null));
        }
        return out;
    }

    private SigCandidate toCandidate(Context ctx, IMethodBinding m, String nameOverride) {
        SigCandidate c = new SigCandidate();
        c.constructor = m.isConstructor();
        c.name = nameOverride != null ? nameOverride
                : (c.constructor ? m.getDeclaringClass().getErasure().getName() : m.getName());
        c.varargs = m.isVarargs();
        ITypeBinding[] params = m.getParameterTypes();
        c.parameterTypes = new ArrayList<>();
        c.matchTypes = new ArrayList<>();
        StringBuilder key = new StringBuilder("(");
        for (ITypeBinding p : params) {
            c.parameterTypes.add(lowerBound(p));
            c.matchTypes.add(simpleTypeName(display(p)));
            key.append(p.getQualifiedName()).append(';');
        }
        key.append(')');
        if (!c.constructor) {
            c.returnType = upperBound(m.getReturnType());
            key.append(m.getReturnType().getQualifiedName());
        } else {
            key.append('V');
        }
        c.key = key.toString();
        c.declaredTypes = new ArrayList<>();
        for (ITypeBinding p : m.getMethodDeclaration().getParameterTypes()) {
            c.declaredTypes.add(display(p));
        }
        MethodDeclaration decl = declaration(ctx, m);
        c.parameterNames = parameterNames(m, decl, params.length);
        if (ctx.description && decl != null && decl.getJavadoc() != null) {
            Javadoc doc = decl.getJavadoc();
            String text = sourceOf(decl);
            if (text != null) {
                c.javadoc = text.substring(doc.getStartPosition(), doc.getStartPosition() + doc.getLength());
            }
        }
        return c;
    }

    private static String sourceOf(ASTNode node) {
        Object text = node.getRoot().getProperty(SOURCE_PROPERTY);
        return text instanceof String s ? s : null;
    }

    private static final String SOURCE_PROPERTY = "jdtls.source";

    private static List<String> parameterNames(IMethodBinding m, MethodDeclaration decl, int count) {
        List<String> names = new ArrayList<>();
        if (decl != null && decl.parameters().size() == count) {
            for (Object o : decl.parameters()) {
                names.add(((SingleVariableDeclaration) o).getName().getIdentifier());
            }
            return names;
        }
        String[] fromBinding = null;
        try {
            fromBinding = m.getParameterNames();
        } catch (RuntimeException ignored) {
        }
        for (int i = 0; i < count; i++) {
            names.add(fromBinding != null && fromBinding.length == count && fromBinding[i] != null
                    && !fromBinding[i].isEmpty() ? fromBinding[i] : "arg" + i);
        }
        return names;
    }

    /** The source declaration of `m` (this unit or another in-memory document), or null. */
    private MethodDeclaration declaration(Context ctx, IMethodBinding m) {
        IMethodBinding decl = m.getMethodDeclaration();
        if (decl.isDefaultConstructor()) return null;
        ctx.cu.setProperty(SOURCE_PROPERTY, ctx.source);
        ASTNode node = ctx.cu.findDeclaringNode(decl);
        if (node == null) node = ctx.cu.findDeclaringNode(decl.getKey());
        if (node instanceof MethodDeclaration md) return md;
        ITypeBinding declaring = decl.getDeclaringClass();
        if (declaring == null || !declaring.isFromSource()) return null;
        ITypeBinding top = outermost(declaring);
        String uri = ctx.sources.uriOf(top.getQualifiedName());
        if (uri == null) return null;
        String text = ctx.sources.source(uri);
        CompilationUnit other = parsePlain(text);
        if (other == null) return null;
        other.setProperty(SOURCE_PROPERTY, text);
        List<String> path = new ArrayList<>();
        for (ITypeBinding t = declaring.getErasure(); t != null; t = t.getDeclaringClass()) {
            path.add(0, t.getName());
        }
        List<?> types = other.types();
        AbstractTypeDeclaration current = null;
        for (String segment : path) {
            AbstractTypeDeclaration next = null;
            for (Object o : types) {
                if (o instanceof AbstractTypeDeclaration td && td.getName().getIdentifier().equals(segment)) {
                    next = td;
                    break;
                }
            }
            if (next == null) return null;
            current = next;
            types = current.bodyDeclarations();
        }
        if (current == null) return null;
        ITypeBinding[] params = decl.getParameterTypes();
        for (Object o : current.bodyDeclarations()) {
            if (!(o instanceof MethodDeclaration md)) continue;
            if (md.isConstructor() != decl.isConstructor()) continue;
            if (!decl.isConstructor() && !md.getName().getIdentifier().equals(decl.getName())) continue;
            if (md.parameters().size() != params.length) continue;
            boolean same = true;
            for (int i = 0; i < params.length && same; i++) {
                SingleVariableDeclaration svd = (SingleVariableDeclaration) md.parameters().get(i);
                String written = writtenSimpleErasure(svd);
                String expected = params[i].getErasure().getName();
                if (params[i].isTypeVariable() || params[i].isArray() && params[i].getElementType().isTypeVariable()) {
                    continue;
                }
                same = written.equals(expected);
            }
            if (same) return md;
        }
        return null;
    }

    private static String writtenSimpleErasure(SingleVariableDeclaration svd) {
        Type t = svd.getType();
        int dims = svd.getExtraDimensions() + (svd.isVarargs() ? 1 : 0);
        while (t instanceof ArrayType at) {
            dims += at.getDimensions();
            t = at.getElementType();
        }
        if (t instanceof ParameterizedType pt) t = pt.getType();
        String name;
        if (t instanceof SimpleType st) {
            Name n = st.getName();
            name = n instanceof QualifiedName qn ? qn.getName().getIdentifier() : n.getFullyQualifiedName();
        } else if (t instanceof QualifiedType qt) {
            name = qt.getName().getIdentifier();
        } else if (t instanceof NameQualifiedType nqt) {
            name = nqt.getName().getIdentifier();
        } else {
            name = t.toString();
        }
        return name + "[]".repeat(dims);
    }

    private static CompilationUnit parsePlain(String text) {
        if (text == null) return null;
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(text.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(false);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(BridgeOptions.map(null));
        return (CompilationUnit) parser.createAST(null);
    }

    // ── Type display (Signature.toString + Signature.getSimpleName) ────────

    static String display(ITypeBinding t) {
        if (t == null) return "Object";
        if (t.isCapture()) {
            ITypeBinding w = t.getWildcard();
            return w != null ? display(w) : "?";
        }
        if (t.isWildcardType()) {
            ITypeBinding bound = t.getBound();
            if (bound == null) return "?";
            return (t.isUpperbound() ? "? extends " : "? super ") + display(bound);
        }
        if (t.isArray()) {
            return display(t.getElementType()) + "[]".repeat(t.getDimensions());
        }
        if (t.isPrimitive() || t.isTypeVariable()) {
            return t.getName();
        }
        String base = t.getErasure().getName();
        int lt = base.indexOf('<');
        if (lt >= 0) base = base.substring(0, lt);
        if (t.isParameterizedType()) {
            StringBuilder sb = new StringBuilder(base).append('<');
            ITypeBinding[] args = t.getTypeArguments();
            for (int i = 0; i < args.length; i++) {
                if (i > 0) sb.append(',');
                sb.append(display(args[i]));
            }
            return sb.append('>').toString();
        }
        return base;
    }

    /** `SignatureUtil.getLowerBound` on a top-level type, then display. */
    static String lowerBound(ITypeBinding t) {
        ITypeBinding w = t.isCapture() ? t.getWildcard() : t;
        if (w != null && w.isWildcardType()) {
            ITypeBinding bound = w.getBound();
            if (bound == null) return "?";
            return w.isUpperbound() ? "null" : display(bound);
        }
        return display(t);
    }

    /** `SignatureUtil.getUpperBound` on a top-level type, then display. */
    static String upperBound(ITypeBinding t) {
        ITypeBinding w = t.isCapture() ? t.getWildcard() : t;
        if (w != null && w.isWildcardType()) {
            ITypeBinding bound = w.getBound();
            if (bound == null || !w.isUpperbound()) return "Object";
            return display(bound);
        }
        return display(t);
    }

    /** `SignatureHelpUtils.getSimpleTypeName`. */
    static String simpleTypeName(String display) {
        return display.replaceAll("<.*>", "").replace(";", "");
    }
}
