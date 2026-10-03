package com.jdtls.ecjbridge;

import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import java.util.logging.Logger;

import org.eclipse.jdt.core.ToolFactory;
import org.eclipse.jdt.core.compiler.IScanner;
import org.eclipse.jdt.core.compiler.ITerminalSymbols;
import org.eclipse.jdt.core.dom.*;

/**
 * Binding data for rename (data only; the Rust server computes the edits).
 *
 * <ul>
 * <li>{@link #target}: the element at an offset, the way jdt.ls selects it
 * for {@code textDocument/rename} ({@code codeSelect}) and for
 * {@code textDocument/prepareRename} ({@code OccurrencesFinder}).</li>
 * <li>{@link #occurrences}: for a set of identifiers, every name occurrence
 * in the given files with its binding key, plus the method override relations
 * in the type hierarchies of those files.</li>
 * </ul>
 *
 * Bindings are resolved with one batch {@code createASTs} over the requested
 * units; every other workspace file is visible through a temporary source
 * path mirror, so documents need not exist on disk.
 */
final class RenameBindingService {

    private static final Logger LOG = Logger.getLogger(RenameBindingService.class.getName());

    // ─── Data ────────────────────────────────────────────────────────────────

    static final class Element {
        String kind;              // local, field, enumConstant, method, type, typeParameter, package, other
        String key;
        String name;
        int nameStart = -1;
        int nameLength;
        boolean fromSource;
        boolean recovered;
        boolean anonymous;
        boolean isStatic;
        boolean isPrivate;
        boolean recordComponent;
        boolean topLevel;
        String declaringTypeKey;
        String declaringTypeName;
        String packageName;
        String typeName;          // field type (erasure, qualified) / method return type
        int paramCount = -1;
        List<String> paramTypes;
    }

    static final class Occurrence {
        int start;
        int length;
        String name;
        String kind;
        String key;
        String role;              // decl, ref, constructorName, packageDecl, import
        String declaringTypeKey;
        int paramCount = -1;
    }

    static final class FileOccurrences {
        String uri;
        String packageName;
        List<Occurrence> occurrences = new ArrayList<>();
    }

    static final class MethodInfo {
        String key;
        String name;
        String declaringTypeKey;
        String declaringTypeName;
        boolean fromSource;
        boolean isStatic;
        boolean isPrivate;
        List<String> paramTypes;
    }

    static final class TargetResult {
        Element select;
        Element prepare;
        String packageName;
    }

    static final class OccurrencesResult {
        List<FileOccurrences> files = new ArrayList<>();
        List<MethodInfo> methods = new ArrayList<>();
        List<List<String>> relations = new ArrayList<>();
    }

    // ─── Target ──────────────────────────────────────────────────────────────

    TargetResult target(Map<String, String> files, List<String> classpath, String sourceLevel, String uri, int offset) {
        TargetResult result = new TargetResult();
        String source = files == null ? null : files.get(uri);
        if (source == null) return result;
        try (SourceMirror mirror = new SourceMirror(files)) {
            parse(mirror, List.of(uri), classpath, sourceLevel, (u, cu) -> {
                result.packageName = cu.getPackage() == null ? "" : cu.getPackage().getName().getFullyQualifiedName();
                result.prepare = prepareElement(cu, offset);
                result.select = selectElement(cu, source, offset);
            });
        }
        return result;
    }

    /** {@code OccurrencesFinder.initialize(ast, offset, 0)}: the covering Name and its binding. */
    private Element prepareElement(CompilationUnit cu, int offset) {
        ASTNode node = NodeFinder.perform(cu, offset, 0);
        if (!(node instanceof Name name)) return null;
        SimpleName simple = name instanceof QualifiedName q ? q.getName() : (SimpleName) name;
        IBinding binding = bindingOf(simple);
        if (binding == null) return null;
        Element e = describe(binding);
        if (e == null) return null;
        e.nameStart = simple.getStartPosition();
        e.nameLength = simple.getLength();
        return e;
    }

    /** {@code ICompilationUnit.codeSelect(offset, 0)}. */
    private Element selectElement(CompilationUnit cu, String source, int offset) {
        int start = Math.min(Math.max(offset, 0), source.length());
        while (start > 0 && Character.isJavaIdentifierPart(source.charAt(start - 1))) start--;
        int end = Math.min(Math.max(offset, 0), source.length());
        while (end < source.length() && Character.isJavaIdentifierPart(source.charAt(end))) end++;
        if (end <= start) return null;
        String token = source.substring(start, end);
        if (token.equals("super") || token.equals("this")) {
            ASTNode node = NodeFinder.perform(cu, start, end - start);
            ITypeBinding type = null;
            if (token.equals("super") && (node instanceof SuperMethodInvocation || node instanceof SuperFieldAccess
                    || node instanceof SuperConstructorInvocation || node instanceof SuperMethodReference)) {
                ITypeBinding enclosing = enclosingType(node);
                type = enclosing == null ? null : enclosing.getSuperclass();
            } else if (token.equals("this") && node instanceof ConstructorInvocation) {
                type = enclosingType(node);
            }
            if (type == null) return null;
            Element e = describe(type);
            if (e != null) {
                e.nameStart = start;
                e.nameLength = end - start;
            }
            return e;
        }
        ASTNode node = NodeFinder.perform(cu, start, end - start);
        if (!(node instanceof SimpleName simple)) return null;
        IBinding binding = bindingOf(simple);
        if (binding == null) return null;
        if (binding instanceof IMethodBinding m) {
            IMethodBinding decl = m.getMethodDeclaration();
            ITypeBinding declaring = decl.getDeclaringClass();
            if (decl.isConstructor()) {
                binding = declaring;
            } else if (declaring != null && declaring.isRecord() && decl.getParameterTypes().length == 0) {
                // Implicit record accessor: the record component.
                IVariableBinding component = recordComponent(declaring, decl.getName());
                if (component != null && isImplicitAccessor(decl)) binding = component;
            }
        }
        Element e = describe(binding);
        if (e != null) {
            e.nameStart = simple.getStartPosition();
            e.nameLength = simple.getLength();
        }
        return e;
    }

    private static boolean isImplicitAccessor(IMethodBinding m) {
        try {
            return m.isSyntheticRecordMethod();
        } catch (Throwable t) {
            return false;
        }
    }

    private static IVariableBinding recordComponent(ITypeBinding record, String name) {
        for (IVariableBinding f : record.getTypeDeclaration().getDeclaredFields()) {
            if (f.getName().equals(name) && !java.lang.reflect.Modifier.isStatic(f.getModifiers())) return f;
        }
        return null;
    }

    private static ITypeBinding enclosingType(ASTNode node) {
        for (ASTNode n = node; n != null; n = n.getParent()) {
            if (n instanceof AbstractTypeDeclaration t) return t.resolveBinding();
            if (n instanceof AnonymousClassDeclaration a) return a.resolveBinding();
        }
        return null;
    }

    /** Binding of a name, normalising qualified package names and record components. */
    private static IBinding bindingOf(SimpleName n) {
        ASTNode parent = n.getParent();
        if (parent instanceof SingleVariableDeclaration svd && svd.getName() == n
                && svd.getParent() instanceof RecordDeclaration rd) {
            ITypeBinding rb = rd.resolveBinding();
            IVariableBinding f = rb == null ? null : recordComponent(rb, n.getIdentifier());
            if (f != null) return f;
        }
        IBinding b = n.resolveBinding();
        if (b == null && parent instanceof QualifiedName q && q.getName() == n) {
            b = q.resolveBinding();
        }
        return b;
    }

    private Element describe(IBinding binding) {
        Element e = new Element();
        e.recovered = binding.isRecovered();
        e.name = binding.getName();
        switch (binding.getKind()) {
            case IBinding.VARIABLE -> {
                IVariableBinding v = ((IVariableBinding) binding).getVariableDeclaration();
                e.key = v.getKey();
                e.name = v.getName();
                e.isStatic = java.lang.reflect.Modifier.isStatic(v.getModifiers());
                e.isPrivate = java.lang.reflect.Modifier.isPrivate(v.getModifiers());
                ITypeBinding type = v.getType();
                e.typeName = type == null ? null : type.getErasure().getQualifiedName();
                if (v.isField()) {
                    e.kind = v.isEnumConstant() ? "enumConstant" : "field";
                    ITypeBinding declaring = v.getDeclaringClass();
                    e.fromSource = declaring != null && declaring.isFromSource();
                    if (declaring != null) {
                        e.declaringTypeKey = declaring.getTypeDeclaration().getKey();
                        e.declaringTypeName = declaring.getTypeDeclaration().getQualifiedName();
                        e.recordComponent = declaring.isRecord() && !e.isStatic;
                    }
                } else {
                    e.kind = "local";
                    e.fromSource = true;
                }
            }
            case IBinding.METHOD -> {
                IMethodBinding m = ((IMethodBinding) binding).getMethodDeclaration();
                if (m.isConstructor() && m.getDeclaringClass() != null) {
                    // Renaming a constructor renames its type.
                    return describe(m.getDeclaringClass());
                }
                e.kind = "method";
                e.key = m.getKey();
                e.isStatic = java.lang.reflect.Modifier.isStatic(m.getModifiers());
                e.isPrivate = java.lang.reflect.Modifier.isPrivate(m.getModifiers());
                ITypeBinding declaring = m.getDeclaringClass();
                e.fromSource = declaring != null && declaring.isFromSource();
                if (declaring != null) {
                    e.declaringTypeKey = declaring.getTypeDeclaration().getKey();
                    e.declaringTypeName = declaring.getTypeDeclaration().getQualifiedName();
                }
                e.paramCount = m.getParameterTypes().length;
                e.paramTypes = paramTypes(m);
                e.typeName = m.getReturnType() == null ? null : m.getReturnType().getErasure().getQualifiedName();
            }
            case IBinding.TYPE -> {
                ITypeBinding t = (ITypeBinding) binding;
                if (t.isTypeVariable()) {
                    e.kind = "typeParameter";
                    e.key = t.getKey();
                    IMethodBinding dm = t.getDeclaringMethod();
                    ITypeBinding dc = dm != null ? dm.getDeclaringClass() : t.getDeclaringClass();
                    e.fromSource = dc != null && dc.isFromSource();
                } else {
                    t = t.getTypeDeclaration();
                    if (t.isArray() || t.isPrimitive() || t.isNullType() || t.isWildcardType() || t.isCapture()) {
                        e.kind = "other";
                        e.key = t.getKey();
                        return e;
                    }
                    e.kind = "type";
                    e.key = t.getKey();
                    e.name = t.getName();
                    e.fromSource = t.isFromSource();
                    e.anonymous = t.isAnonymous();
                    e.topLevel = t.isTopLevel();
                    e.packageName = t.getPackage() == null ? "" : t.getPackage().getName();
                    e.declaringTypeName = t.getQualifiedName();
                }
            }
            case IBinding.PACKAGE -> {
                e.kind = "package";
                e.key = binding.getName();
                e.packageName = binding.getName();
            }
            default -> {
                e.kind = "other";
                e.key = binding.getKey();
            }
        }
        return e;
    }

    private static List<String> paramTypes(IMethodBinding m) {
        List<String> out = new ArrayList<>();
        for (ITypeBinding p : m.getParameterTypes()) out.add(p.getErasure().getQualifiedName());
        return out;
    }

    // ─── Occurrences ─────────────────────────────────────────────────────────

    OccurrencesResult occurrences(Map<String, String> files, List<String> classpath, String sourceLevel,
                                  List<String> uris, List<String> names, String packageName) {
        OccurrencesResult result = new OccurrencesResult();
        if (files == null || uris == null || uris.isEmpty()) return result;
        Set<String> nameSet = new HashSet<>(names == null ? List.of() : names);
        Map<String, MethodInfo> methods = new LinkedHashMap<>();
        Set<String> relationKeys = new HashSet<>();
        List<String> units = uris.stream().filter(files::containsKey).toList();
        try (SourceMirror mirror = new SourceMirror(files)) {
            parse(mirror, units, classpath, sourceLevel, (uri, cu) -> {
                FileOccurrences fo = new FileOccurrences();
                fo.uri = uri;
                fo.packageName = cu.getPackage() == null ? "" : cu.getPackage().getName().getFullyQualifiedName();
                cu.accept(new ASTVisitor(true) {
                    @Override
                    public boolean visit(SimpleName n) {
                        if (nameSet.contains(n.getIdentifier())) {
                            Occurrence o = classify(n);
                            if (o != null) fo.occurrences.add(o);
                        }
                        if (packageName != null) packageOccurrence(n, packageName, fo);
                        return true;
                    }

                    @Override
                    public boolean visit(QualifiedName n) {
                        if (packageName != null) packageOccurrence(n, packageName, fo);
                        return true;
                    }

                    @Override
                    public boolean visit(TypeDeclaration n) {
                        hierarchy(n.resolveBinding(), nameSet, methods, relationKeys, result.relations);
                        return true;
                    }

                    @Override
                    public boolean visit(EnumDeclaration n) {
                        hierarchy(n.resolveBinding(), nameSet, methods, relationKeys, result.relations);
                        return true;
                    }

                    @Override
                    public boolean visit(RecordDeclaration n) {
                        hierarchy(n.resolveBinding(), nameSet, methods, relationKeys, result.relations);
                        return true;
                    }

                    @Override
                    public boolean visit(AnnotationTypeDeclaration n) {
                        hierarchy(n.resolveBinding(), nameSet, methods, relationKeys, result.relations);
                        return true;
                    }

                    @Override
                    public boolean visit(AnonymousClassDeclaration n) {
                        hierarchy(n.resolveBinding(), nameSet, methods, relationKeys, result.relations);
                        return true;
                    }
                });
                result.files.add(fo);
            });
        }
        result.methods.addAll(methods.values());
        return result;
    }

    private Occurrence classify(SimpleName n) {
        IBinding b = bindingOf(n);
        if (b == null) return null;
        Occurrence o = new Occurrence();
        o.start = n.getStartPosition();
        o.length = n.getLength();
        o.name = n.getIdentifier();
        o.role = n.isDeclaration() ? "decl" : "ref";
        switch (b.getKind()) {
            case IBinding.VARIABLE -> {
                IVariableBinding v = ((IVariableBinding) b).getVariableDeclaration();
                o.kind = v.isField() ? (v.isEnumConstant() ? "enumConstant" : "field") : "local";
                o.key = v.getKey();
                if (v.getDeclaringClass() != null) o.declaringTypeKey = v.getDeclaringClass().getTypeDeclaration().getKey();
            }
            case IBinding.METHOD -> {
                IMethodBinding m = ((IMethodBinding) b).getMethodDeclaration();
                ITypeBinding declaring = m.getDeclaringClass();
                if (m.isConstructor()) {
                    if (declaring == null) return null;
                    o.kind = "type";
                    o.key = declaring.getTypeDeclaration().getKey();
                    o.role = n.getParent() instanceof MethodDeclaration md && md.getName() == n ? "constructorName" : "ref";
                } else {
                    o.kind = "method";
                    o.key = m.getKey();
                    o.paramCount = m.getParameterTypes().length;
                    if (declaring != null) o.declaringTypeKey = declaring.getTypeDeclaration().getKey();
                }
            }
            case IBinding.TYPE -> {
                ITypeBinding t = (ITypeBinding) b;
                if (t.isTypeVariable()) {
                    o.kind = "typeParameter";
                    o.key = t.getKey();
                } else {
                    o.kind = "type";
                    o.key = t.getTypeDeclaration().getKey();
                }
            }
            default -> {
                return null;
            }
        }
        return o;
    }

    /** Maximal names bound to the package {@code pkg}. */
    private static void packageOccurrence(Name n, String pkg, FileOccurrences fo) {
        IBinding b = n.resolveBinding();
        if (b == null && n instanceof SimpleName s && s.getParent() instanceof QualifiedName q && q.getName() == s) {
            return; // covered by the qualified name itself
        }
        if (!(b instanceof IPackageBinding pb) || !pkg.equals(pb.getName())) return;
        if (n.getParent() instanceof QualifiedName q && q.getName() == n) return;
        Occurrence o = new Occurrence();
        o.start = n.getStartPosition();
        o.length = n.getLength();
        o.name = n.getFullyQualifiedName();
        o.kind = "package";
        o.key = pkg;
        o.role = "ref";
        for (ASTNode a = n.getParent(); a != null; a = a.getParent()) {
            if (a instanceof PackageDeclaration) { o.role = "packageDecl"; break; }
            if (a instanceof ImportDeclaration) { o.role = "import"; break; }
        }
        fo.occurrences.add(o);
    }

    /** Methods named in {@code names} across the hierarchy of {@code type}, and their override relations. */
    private static void hierarchy(ITypeBinding type, Set<String> names, Map<String, MethodInfo> methods,
                                  Set<String> relationKeys, List<List<String>> relations) {
        if (type == null || names.isEmpty()) return;
        List<IMethodBinding> found = new ArrayList<>();
        collectMethods(type, names, found, new HashSet<>());
        for (IMethodBinding m : found) {
            IMethodBinding d = m.getMethodDeclaration();
            methods.computeIfAbsent(d.getKey(), k -> methodInfo(d));
        }
        for (int i = 0; i < found.size(); i++) {
            IMethodBinding a = found.get(i);
            if (!virtual(a)) continue;
            for (int j = i + 1; j < found.size(); j++) {
                IMethodBinding b = found.get(j);
                if (!virtual(b) || !a.getName().equals(b.getName())) continue;
                if (a.overrides(b) || b.overrides(a) || a.isSubsignature(b) || b.isSubsignature(a)) {
                    String ka = a.getMethodDeclaration().getKey();
                    String kb = b.getMethodDeclaration().getKey();
                    if (ka.equals(kb)) continue;
                    String rk = ka.compareTo(kb) < 0 ? ka + "\u0000" + kb : kb + "\u0000" + ka;
                    if (relationKeys.add(rk)) relations.add(List.of(ka, kb));
                }
            }
        }
    }

    private static boolean virtual(IMethodBinding m) {
        int mod = m.getModifiers();
        return !m.isConstructor() && !java.lang.reflect.Modifier.isStatic(mod) && !java.lang.reflect.Modifier.isPrivate(mod);
    }

    private static void collectMethods(ITypeBinding t, Set<String> names, List<IMethodBinding> out, Set<String> seen) {
        if (t == null || !seen.add(t.getKey())) return;
        for (IMethodBinding m : t.getDeclaredMethods()) {
            if (!m.isConstructor() && names.contains(m.getName())) out.add(m);
        }
        collectMethods(t.getSuperclass(), names, out, seen);
        for (ITypeBinding i : t.getInterfaces()) collectMethods(i, names, out, seen);
    }

    private static MethodInfo methodInfo(IMethodBinding d) {
        MethodInfo mi = new MethodInfo();
        mi.key = d.getKey();
        mi.name = d.getName();
        ITypeBinding declaring = d.getDeclaringClass();
        if (declaring != null) {
            mi.declaringTypeKey = declaring.getTypeDeclaration().getKey();
            mi.declaringTypeName = declaring.getTypeDeclaration().getQualifiedName();
            mi.fromSource = declaring.isFromSource();
        }
        mi.isStatic = java.lang.reflect.Modifier.isStatic(d.getModifiers());
        mi.isPrivate = java.lang.reflect.Modifier.isPrivate(d.getModifiers());
        mi.paramTypes = paramTypes(d);
        return mi;
    }

    // ─── Parsing ─────────────────────────────────────────────────────────────

    private interface UnitConsumer {
        void accept(String uri, CompilationUnit cu);
    }

    /** Resolve {@code uris} as one batch; bindings are only valid inside the callback. */
    private void parse(SourceMirror mirror, List<String> uris, List<String> classpath, String sourceLevel, UnitConsumer consumer) {
        if (uris.isEmpty()) return;
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(BridgeOptions.map(sourceLevel));
        String[] cp = classpath == null ? new String[0] : classpath.toArray(new String[0]);
        BridgeOptions.configureEnvironment(parser, cp, new String[] { mirror.root.toString() });
        Map<String, String> pathToUri = new HashMap<>();
        List<String> paths = new ArrayList<>();
        for (String uri : uris) {
            Path p = mirror.pathOf(uri);
            if (p == null) continue;
            pathToUri.put(p.toString(), uri);
            paths.add(p.toString());
        }
        String[] encodings = new String[paths.size()];
        Arrays.fill(encodings, "UTF-8");
        parser.createASTs(paths.toArray(new String[0]), encodings, new String[0], new FileASTRequestor() {
            @Override
            public void acceptAST(String sourceFilePath, CompilationUnit ast) {
                String uri = pathToUri.get(sourceFilePath);
                if (uri == null) uri = pathToUri.get(Path.of(sourceFilePath).toString());
                if (uri == null) return;
                try {
                    consumer.accept(uri, ast);
                } catch (RuntimeException e) {
                    LOG.warning("rename: failed to process " + uri + ": " + e);
                }
            }
        }, null);
    }

    /**
     * Temporary copy of the request's sources, laid out by package so that
     * the compiler's source path finds types of files not being resolved.
     */
    private static final class SourceMirror implements AutoCloseable {
        final Path root;
        private final Path base;
        private final Map<String, Path> paths = new HashMap<>();

        SourceMirror(Map<String, String> files) {
            Path b;
            try {
                b = Files.createTempDirectory("jdtls-rename");
            } catch (IOException e) {
                throw new IllegalStateException("cannot create rename mirror", e);
            }
            base = b;
            root = b.resolve("src");
            int dup = 0;
            for (Map.Entry<String, String> entry : files.entrySet()) {
                String source = entry.getValue();
                String pkg = packageOf(source);
                String fileName = fileName(entry.getKey());
                Path dir = root;
                if (!pkg.isEmpty()) {
                    for (String seg : pkg.split("\\.")) dir = dir.resolve(seg);
                }
                Path target = dir.resolve(fileName);
                if (Files.exists(target)) {
                    target = b.resolve("dup" + (dup++)).resolve(fileName);
                }
                try {
                    Files.createDirectories(target.getParent());
                    Files.writeString(target, source, StandardCharsets.UTF_8);
                    paths.put(entry.getKey(), target);
                } catch (IOException e) {
                    LOG.warning("rename: cannot mirror " + entry.getKey() + ": " + e);
                }
            }
        }

        Path pathOf(String uri) {
            return paths.get(uri);
        }

        private static String fileName(String uri) {
            String path;
            try {
                path = new URI(uri).getPath();
            } catch (Exception e) {
                path = null;
            }
            if (path == null || path.isEmpty()) path = uri;
            String name = path.substring(Math.max(path.lastIndexOf('/'), path.lastIndexOf(':')) + 1);
            if (name.isEmpty()) name = "Untitled";
            if (!name.endsWith(".java")) name = name + ".java";
            return name;
        }

        private static String packageOf(String source) {
            IScanner scanner = ToolFactory.createScanner(false, false, false, false);
            scanner.setSource(source.toCharArray());
            StringBuilder sb = new StringBuilder();
            boolean inPackage = false;
            try {
                int depth = 0;
                while (true) {
                    int tok = scanner.getNextToken();
                    if (tok == ITerminalSymbols.TokenNameEOF) break;
                    if (!inPackage) {
                        if (tok == ITerminalSymbols.TokenNameAT) {
                            // annotations on the package declaration: skip "@Name(...)"
                            continue;
                        }
                        if (tok == ITerminalSymbols.TokenNameLPAREN) { depth++; continue; }
                        if (tok == ITerminalSymbols.TokenNameRPAREN) { depth--; continue; }
                        if (depth > 0) continue;
                        if (tok == ITerminalSymbols.TokenNamepackage) { inPackage = true; continue; }
                        if (tok == ITerminalSymbols.TokenNameIdentifier || tok == ITerminalSymbols.TokenNameDOT) continue;
                        break;
                    }
                    if (tok == ITerminalSymbols.TokenNameSEMICOLON) return sb.toString();
                    if (tok == ITerminalSymbols.TokenNameIdentifier || tok == ITerminalSymbols.TokenNameDOT) {
                        sb.append(scanner.getCurrentTokenSource());
                    } else {
                        break;
                    }
                }
            } catch (Exception e) {
                // fall through
            }
            return inPackage ? sb.toString() : "";
        }

        @Override
        public void close() {
            try (var walk = Files.walk(base)) {
                walk.sorted(Comparator.reverseOrder()).forEach(p -> {
                    try {
                        Files.deleteIfExists(p);
                    } catch (IOException ignored) {
                    }
                });
            } catch (IOException ignored) {
            }
        }
    }
}
