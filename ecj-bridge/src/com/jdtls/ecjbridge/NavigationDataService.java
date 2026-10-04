package com.jdtls.ecjbridge;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import org.eclipse.jdt.core.dom.*;

import com.jdtls.ecjbridge.ClassFileService.ClassFileDesc;

/**
 * Binding-resolution data for navigation (jdt.ls
 * {@code NavigateToDefinitionHandler}, {@code NavigateToTypeDefinitionHandler},
 * {@code NavigateToDeclarationHandler}, {@code ImplementationsHandler},
 * {@code ReferencesHandler}, {@code DocumentHighlightHandler}).
 *
 * All project units are resolved together in one batch (shared bindings),
 * so cross-file bindings are exact.  Results are raw locations — a source
 * URI or a class-file descriptor plus a range — and the Rust server turns
 * them into LSP locations (jdt:// URIs, null/empty semantics, preferences).
 */
final class NavigationDataService {

    // ── Protocol ──────────────────────────────────────────────────────────

    public static class RawLocation {
        public String uri;
        public ClassFileDesc classFile;
        public int startLine, startChar, endLine, endChar;
        /** Highlight kind: 1=Text 2=Read 3=Write (0 for plain locations). */
        public int kind;
    }

    public static class NavDataResponse extends BridgeProtocol.Response {
        public List<RawLocation> locations;
        /** {@code true} when the handler's answer is {@code null} rather than empty. */
        public boolean nullResult;
        /** references: the searched elements ("<includeDeclaration>|<key>"). */
        public List<String> searchKeys;
        /** references: library roots searched. */
        public List<String> scannedLibraries;
        /** references: every searched element is declared in this project's sources. */
        public boolean sourceElements;

        NavDataResponse(long id, List<RawLocation> locations, boolean nullResult) {
            this.id = id;
            this.method = "navData";
            this.locations = locations;
            this.nullResult = nullResult;
        }
    }

    public static class ClassFileContentsResponse extends BridgeProtocol.Response {
        public String contents;

        ClassFileContentsResponse(long id, String contents) {
            this.id = id;
            this.method = "classFileContents";
            this.contents = contents;
        }
    }

    public static class ClassFileInfoResponse extends BridgeProtocol.Response {
        public ClassFileDesc classFile;
        public String sourceUri;

        ClassFileInfoResponse(long id, ClassFileDesc classFile, String sourceUri) {
            this.id = id;
            this.method = "classFileInfo";
            this.classFile = classFile;
            this.sourceUri = sourceUri;
        }
    }

    /** Request-scoped options. */
    static final class Options {
        boolean includeClassFiles;
        boolean includeDecompiled = true;
        boolean includeDeclaration;
        boolean includeAccessors = true;
        Map<String, String> attachments = Map.of();
        List<String> classpath = List.of();
        /** Library roots searched for references, in order ("jrt" = the JDK). */
        List<String> libraries;
        /** Library roots already searched (by another project). */
        Set<String> skipLibraries = Set.of();
        String sourceLevel;
    }

    // ── Units ─────────────────────────────────────────────────────────────

    static final class Unit {
        String uri;
        ClassFileDesc classFile;
        String source;
        CompilationUnit cu;
        int[] lineStarts;

        boolean isClassFile() {
            return classFile != null;
        }

        RawLocation location(int offset, int length) {
            RawLocation l = new RawLocation();
            if (classFile != null) {
                l.classFile = classFile;
            } else {
                l.uri = uri;
            }
            int[] s = lineCol(offset);
            int[] e = lineCol(offset + length);
            l.startLine = s[0];
            l.startChar = s[1];
            l.endLine = e[0];
            l.endChar = e[1];
            return l;
        }

        /** {@code JDTUtils.toRange}: offset 0 and length 0 give the empty range at 0:0. */
        int[] lineCol(int offset) {
            offset = Math.max(0, Math.min(offset, source.length()));
            int line = Arrays.binarySearch(lineStarts, offset);
            if (line < 0) {
                line = -line - 2;
            }
            return new int[] { line, offset - lineStarts[line] };
        }

        int offset(int line, int character) {
            if (line < 0 || line >= lineStarts.length) {
                return -1;
            }
            int start = lineStarts[line];
            int end = line + 1 < lineStarts.length ? lineStarts[line + 1] : source.length();
            int off = start + character;
            return off > end ? -1 : off;
        }
    }

    static int[] lineStarts(String s) {
        List<Integer> starts = new ArrayList<>();
        starts.add(0);
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '\r') {
                if (i + 1 < s.length() && s.charAt(i + 1) == '\n') {
                    i++;
                }
                starts.add(i + 1);
            } else if (c == '\n') {
                starts.add(i + 1);
            }
        }
        int[] out = new int[starts.size()];
        for (int i = 0; i < out.length; i++) {
            out[i] = starts.get(i);
        }
        return out;
    }

    static final class Units {
        final List<Unit> list = new ArrayList<>();
        Unit target;
        Options options;
        /** Top-level type key → unit declaring it. */
        final Map<String, Unit> typeOwners = new HashMap<>();
        /** Elements searched by `references` ("<includeDeclaration>|<key>"). */
        final List<String> searched = new ArrayList<>();
        /** Library roots searched by `references`. */
        final Set<String> scannedLibraries = new LinkedHashSet<>();
        boolean searchedSourceOnly = true;
        /** Bindings requested by key. */
        final Map<String, IBinding> keyBindings = new HashMap<>();

        List<Unit> sourceUnits() {
            List<Unit> out = new ArrayList<>();
            for (Unit u : list) {
                if (!u.isClassFile()) {
                    out.add(u);
                }
            }
            return out;
        }
    }

    private static final Pattern TYPE_NAME = Pattern.compile("\\b(?:class|interface|enum|record)\\s+([A-Za-z_$][\\w$]*)");

    private static String fileNameFor(String uri, String source) {
        String path = uri;
        int q = path.indexOf('?');
        if (q >= 0) {
            path = path.substring(0, q);
        }
        String last = path.substring(path.lastIndexOf('/') + 1);
        try {
            last = java.net.URLDecoder.decode(last, StandardCharsets.UTF_8);
        } catch (Exception e) {
            // keep raw
        }
        if (last.endsWith(".java") && last.length() > 5) {
            return last;
        }
        Matcher m = TYPE_NAME.matcher(source);
        return m.find() ? m.group(1) + ".java" : "Unnamed.java";
    }

    /**
     * Resolve {@code files} (plus the class-file contents of {@code classFile},
     * when the request targets a class file) in one batch.
     */
    static Units parse(Map<String, String> files, Options options, String targetUri, ClassFileDesc classFile) {
        return parse(files, options, targetUri, classFile, List.of());
    }

    static Units parse(Map<String, String> files, Options options, String targetUri, ClassFileDesc classFile, List<String> bindingKeys) {
        Units units = new Units();
        units.options = options;
        Path tmp = null;
        try {
            tmp = Files.createTempDirectory("jdtls-nav");
            List<String> paths = new ArrayList<>();
            Map<String, Unit> byPath = new HashMap<>();
            int i = 0;
            List<String> uris = new ArrayList<>(files == null ? List.of() : files.keySet());
            Collections.sort(uris);
            for (String uri : uris) {
                String src = files.get(uri);
                if (src == null || !looksLikeJava(uri)) {
                    continue;
                }
                Unit u = new Unit();
                u.uri = uri;
                u.source = src;
                Path dir = tmp.resolve(Integer.toString(i++));
                Files.createDirectories(dir);
                Path p = dir.resolve(fileNameFor(uri, src));
                Files.writeString(p, src, StandardCharsets.UTF_8);
                paths.add(p.toString());
                byPath.put(p.toString(), u);
                units.list.add(u);
                if (uri.equals(targetUri)) {
                    units.target = u;
                }
            }
            if (classFile != null) {
                String contents = ClassFileService.contents(classFile, options.attachments);
                if (contents != null && !contents.isBlank()) {
                    Unit u = new Unit();
                    u.classFile = classFile;
                    u.source = contents;
                    Path p = tmp.resolve("classfile" + ClassFileService.unitName(classFile));
                    Files.createDirectories(p.getParent());
                    Files.writeString(p, contents, StandardCharsets.UTF_8);
                    paths.add(p.toString());
                    byPath.put(p.toString(), u);
                    units.list.add(u);
                    units.target = u;
                }
            }
            if (paths.isEmpty()) {
                return units;
            }
            ASTParser parser = newParser(options);
            String[] encodings = new String[paths.size()];
            Arrays.fill(encodings, "UTF-8");
            parser.createASTs(paths.toArray(new String[0]), encodings, bindingKeys.toArray(new String[0]), new FileASTRequestor() {
                @Override
                public void acceptAST(String sourceFilePath, CompilationUnit ast) {
                    Unit u = byPath.get(sourceFilePath);
                    if (u == null) {
                        u = byPath.get(Path.of(sourceFilePath).toString());
                    }
                    if (u != null) {
                        u.cu = ast;
                    }
                }

                @Override
                public void acceptBinding(String bindingKey, IBinding binding) {
                    if (binding != null) {
                        units.keyBindings.put(bindingKey, binding);
                    }
                }
            }, null);
        } catch (IOException e) {
            throw new RuntimeException(e);
        } finally {
            if (tmp != null) {
                deleteRecursively(tmp);
            }
        }
        units.list.removeIf(u -> u.cu == null);
        for (Unit u : units.list) {
            u.lineStarts = lineStarts(u.source);
            for (Object t : u.cu.types()) {
                ITypeBinding b = ((AbstractTypeDeclaration) t).resolveBinding();
                if (b != null) {
                    units.typeOwners.putIfAbsent(b.getTypeDeclaration().getKey(), u);
                }
            }
        }
        if (units.target != null && units.target.cu == null) {
            units.target = null;
        }
        return units;
    }

    private static boolean looksLikeJava(String uri) {
        String u = uri.toLowerCase();
        int q = u.indexOf('?');
        if (q >= 0) {
            u = u.substring(0, q);
        }
        return u.endsWith(".java") || !u.substring(u.lastIndexOf('/') + 1).contains(".");
    }

    static ASTParser newParser(Options options) {
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(BridgeOptions.map(options.sourceLevel));
        BridgeOptions.configureEnvironment(parser, options.classpath.toArray(new String[0]));
        return parser;
    }

    /** Parse a class file's contents on its own (no project sources). */
    static Unit parseClassFile(ClassFileDesc desc, Options options) {
        String contents = ClassFileService.contents(desc, options.attachments);
        if (contents == null || contents.isBlank()) {
            return null;
        }
        ASTParser parser = newParser(options);
        parser.setSource(contents.toCharArray());
        parser.setUnitName(ClassFileService.unitName(desc));
        Unit u = new Unit();
        u.classFile = desc;
        u.source = contents;
        u.cu = (CompilationUnit) parser.createAST(null);
        u.lineStarts = lineStarts(contents);
        return u;
    }

    private static void deleteRecursively(Path p) {
        try (var s = Files.walk(p)) {
            s.sorted(Comparator.reverseOrder()).forEach(x -> {
                try {
                    Files.deleteIfExists(x);
                } catch (IOException e) {
                    // ignore
                }
            });
        } catch (IOException e) {
            // ignore
        }
    }

    // ── Bindings ──────────────────────────────────────────────────────────

    static IBinding normalize(IBinding b) {
        if (b instanceof IMethodBinding m) {
            return m.getMethodDeclaration();
        }
        if (b instanceof ITypeBinding t) {
            if (t.isCapture() || t.isWildcardType()) {
                ITypeBinding erasure = t.getErasure();
                return erasure == null ? t : erasure;
            }
            return t.getTypeDeclaration();
        }
        if (b instanceof IVariableBinding v) {
            return v.getVariableDeclaration();
        }
        return b;
    }

    static String key(IBinding b) {
        IBinding n = normalize(b);
        return n == null ? null : n.getKey();
    }

    /** {@code ICodeAssist.codeSelect(offset, 0)} for the name at {@code offset}. */
    static IBinding elementAt(Unit u, int offset) {
        if (u == null || offset < 0) {
            return null;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        if (!(node instanceof SimpleName name)) {
            return null;
        }
        IBinding b = name.resolveBinding();
        if (b == null) {
            return null;
        }
        // `new Foo()` selects the constructor when one is declared.
        ASTNode p = name.getParent();
        if (p instanceof SimpleType st) {
            ASTNode type = st.getParent() instanceof ParameterizedType pt ? pt : st;
            if (type.getParent() instanceof ClassInstanceCreation cic && cic.getType() == type
                    && cic.getAnonymousClassDeclaration() == null) {
                IMethodBinding ctor = cic.resolveConstructorBinding();
                if (ctor != null && !ctor.isDefaultConstructor() && ctor.getDeclaringClass() != null
                        && !ctor.getDeclaringClass().isEnum()) {
                    IMethodBinding decl = ctor.getMethodDeclaration();
                    if (declaredConstructor(decl)) {
                        b = decl;
                    }
                }
            }
        }
        return normalize(b);
    }

    private static boolean declaredConstructor(IMethodBinding ctor) {
        // Binary types never have "default" constructors in the source sense,
        // so only treat source constructors with a declaration as selectable.
        return !ctor.isDefaultConstructor();
    }

    /** {@code ITypeRoot.getElementAt(offset)}: the innermost member containing the offset. */
    static IBinding memberAt(Unit u, int offset) {
        if (u == null || offset < 0) {
            return null;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        while (node != null) {
            if (node instanceof MethodDeclaration md) {
                return normalize(md.resolveBinding());
            }
            if (node instanceof VariableDeclarationFragment vdf && vdf.getParent() instanceof FieldDeclaration) {
                return normalize(vdf.resolveBinding());
            }
            if (node instanceof FieldDeclaration fd && !fd.fragments().isEmpty()) {
                return normalize(((VariableDeclarationFragment) fd.fragments().get(0)).resolveBinding());
            }
            if (node instanceof EnumConstantDeclaration ecd) {
                return normalize(ecd.resolveVariable());
            }
            if (node instanceof AbstractTypeDeclaration td) {
                return normalize(td.resolveBinding());
            }
            if (node instanceof AnnotationTypeMemberDeclaration am) {
                return normalize(am.resolveBinding());
            }
            node = node.getParent();
        }
        return null;
    }

    static ITypeBinding declaringType(IBinding b) {
        if (b instanceof ITypeBinding t) {
            if (t.isTypeVariable()) {
                if (t.getDeclaringClass() != null) {
                    return t.getDeclaringClass();
                }
                return t.getDeclaringMethod() == null ? null : t.getDeclaringMethod().getDeclaringClass();
            }
            return t;
        }
        if (b instanceof IMethodBinding m) {
            return m.getDeclaringClass();
        }
        if (b instanceof IVariableBinding v) {
            if (v.isField()) {
                return v.getDeclaringClass();
            }
            IMethodBinding m = v.getDeclaringMethod();
            return m == null ? null : m.getDeclaringClass();
        }
        return null;
    }

    static ITypeBinding topLevel(ITypeBinding t) {
        if (t == null) {
            return null;
        }
        t = t.getTypeDeclaration();
        while (true) {
            ITypeBinding outer = t.getDeclaringClass();
            if (outer == null && t.getDeclaringMethod() != null) {
                outer = t.getDeclaringMethod().getDeclaringClass();
            }
            if (outer == null) {
                return t;
            }
            t = outer.getTypeDeclaration();
        }
    }

    /** The class file a binary element lives in (its own for types, the declaring type's otherwise). */
    static ITypeBinding classFileType(IBinding b) {
        ITypeBinding t = declaringType(b);
        if (t == null) {
            return null;
        }
        t = t.getTypeDeclaration();
        if (t.isArray()) {
            t = t.getElementType().getTypeDeclaration();
        }
        return t;
    }

    // ── Declarations ──────────────────────────────────────────────────────

    /** Declaration node of {@code key} in {@code cu}. */
    static ASTNode findDeclaration(CompilationUnit cu, String key) {
        if (cu == null || key == null) {
            return null;
        }
        ASTNode[] found = new ASTNode[1];
        cu.accept(new ASTVisitor(true) {
            private boolean check(ASTNode node, IBinding b) {
                if (found[0] != null) {
                    return false;
                }
                if (b != null && key.equals(key(b))) {
                    found[0] = node;
                    return false;
                }
                return true;
            }

            @Override public boolean visit(TypeDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(EnumDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(AnnotationTypeDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(RecordDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(AnonymousClassDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(MethodDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(AnnotationTypeMemberDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(VariableDeclarationFragment node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(SingleVariableDeclaration node) { return check(node, node.resolveBinding()); }
            @Override public boolean visit(EnumConstantDeclaration node) { return check(node, node.resolveVariable()); }
            @Override public boolean visit(TypeParameter node) { return check(node, node.resolveBinding()); }
        });
        return found[0];
    }

    /** Name-based fallback when keys differ between binary and source models. */
    static ASTNode findDeclarationByName(CompilationUnit cu, IBinding b) {
        if (cu == null) {
            return null;
        }
        String name = b.getName();
        ITypeBinding owner = declaringType(b);
        String ownerName = owner == null ? null : owner.getTypeDeclaration().getQualifiedName();
        ASTNode[] found = new ASTNode[1];
        cu.accept(new ASTVisitor() {
            @Override
            public boolean visit(MethodDeclaration node) {
                if (found[0] == null && b instanceof IMethodBinding m && node.getName().getIdentifier().equals(m.isConstructor() ? node.getName().getIdentifier() : name)
                        && m.isConstructor() == node.isConstructor() && node.parameters().size() == m.getParameterTypes().length
                        && ownerMatches(node)) {
                    IMethodBinding nb = node.resolveBinding();
                    if (nb == null || sameErasures(nb, m)) {
                        found[0] = node;
                    }
                }
                return found[0] == null;
            }

            @Override
            public boolean visit(VariableDeclarationFragment node) {
                if (found[0] == null && b instanceof IVariableBinding && node.getName().getIdentifier().equals(name)
                        && node.getParent() instanceof FieldDeclaration && ownerMatches(node)) {
                    found[0] = node;
                }
                return found[0] == null;
            }

            @Override
            public boolean visit(EnumConstantDeclaration node) {
                if (found[0] == null && b instanceof IVariableBinding && node.getName().getIdentifier().equals(name) && ownerMatches(node)) {
                    found[0] = node;
                }
                return found[0] == null;
            }

            @Override
            public boolean visit(TypeDeclaration node) {
                return typeVisit(node);
            }

            @Override
            public boolean visit(EnumDeclaration node) {
                return typeVisit(node);
            }

            @Override
            public boolean visit(AnnotationTypeDeclaration node) {
                return typeVisit(node);
            }

            @Override
            public boolean visit(RecordDeclaration node) {
                return typeVisit(node);
            }

            private boolean typeVisit(AbstractTypeDeclaration node) {
                if (found[0] == null && b instanceof ITypeBinding t && node.getName().getIdentifier().equals(t.getErasure().getName())) {
                    ITypeBinding nb = node.resolveBinding();
                    if (nb == null || nb.getQualifiedName().equals(t.getErasure().getQualifiedName())) {
                        found[0] = node;
                    }
                }
                return found[0] == null;
            }

            private boolean ownerMatches(ASTNode node) {
                if (ownerName == null) {
                    return true;
                }
                ASTNode p = node.getParent();
                while (p != null && !(p instanceof AbstractTypeDeclaration) && !(p instanceof AnonymousClassDeclaration)) {
                    p = p.getParent();
                }
                if (p instanceof AbstractTypeDeclaration td) {
                    ITypeBinding tb = td.resolveBinding();
                    return tb == null || tb.getQualifiedName().equals(ownerName);
                }
                return false;
            }
        });
        return found[0];
    }

    static boolean sameErasures(IMethodBinding a, IMethodBinding b) {
        ITypeBinding[] pa = a.getParameterTypes();
        ITypeBinding[] pb = b.getParameterTypes();
        if (pa.length != pb.length) {
            return false;
        }
        for (int i = 0; i < pa.length; i++) {
            if (!pa[i].getErasure().getQualifiedName().equals(pb[i].getErasure().getQualifiedName())) {
                return false;
            }
        }
        return true;
    }

    /** {@code JDTUtils.getNameRange}: the name node of a declaration. */
    static ASTNode nameNode(ASTNode decl) {
        if (decl instanceof AbstractTypeDeclaration td) {
            return td.getName();
        }
        if (decl instanceof AnonymousClassDeclaration acd && acd.getParent() instanceof ClassInstanceCreation cic) {
            return cic.getType();
        }
        if (decl instanceof MethodDeclaration md) {
            return md.getName();
        }
        if (decl instanceof AnnotationTypeMemberDeclaration am) {
            return am.getName();
        }
        if (decl instanceof VariableDeclaration vd) {
            return vd.getName();
        }
        if (decl instanceof EnumConstantDeclaration ecd) {
            return ecd.getName();
        }
        if (decl instanceof TypeParameter tp) {
            return tp.getName();
        }
        return decl;
    }

    static final class Located {
        Unit unit;
        ASTNode decl;
    }

    /** The source unit and declaration node of {@code b} among the parsed units. */
    static Located sourceDeclaration(Units units, IBinding b) {
        String key = key(b);
        if (key == null) {
            return null;
        }
        ITypeBinding top = topLevel(declaringType(b));
        List<Unit> candidates = new ArrayList<>();
        if (top != null) {
            Unit owner = units.typeOwners.get(top.getKey());
            if (owner != null) {
                candidates.add(owner);
            }
        }
        if (candidates.isEmpty() && (b instanceof IVariableBinding v && !v.isField() || b instanceof ITypeBinding t && (t.isLocal() || t.isAnonymous() || t.isTypeVariable()))) {
            candidates.addAll(units.list);
        }
        for (Unit u : candidates) {
            ASTNode decl = findDeclaration(u.cu, key);
            if (decl == null && top != null) {
                // Implicit members (default constructors, enum values()): the declaring type.
                ITypeBinding dt = declaringType(b);
                if (dt != null && !(b instanceof ITypeBinding)) {
                    decl = findDeclaration(u.cu, dt.getTypeDeclaration().getKey());
                }
            }
            if (decl != null) {
                Located l = new Located();
                l.unit = u;
                l.decl = decl;
                return l;
            }
        }
        return null;
    }

    /**
     * {@code NavigateToDefinitionHandler.computeDefinitionNavigation(element)}
     * (and the type-definition/declaration variants): the location of the
     * element's declaration in a source unit, in attached source, or in the
     * decompiled class file.
     */
    static RawLocation elementLocation(Units units, IBinding b, boolean decompiledWholeNode) {
        if (b == null || b instanceof IPackageBinding) {
            return null;
        }
        Located src = sourceDeclaration(units, b);
        if (src != null) {
            ASTNode name = nameNode(src.decl);
            return src.unit.location(name.getStartPosition(), name.getLength());
        }
        if (b instanceof IVariableBinding v && !v.isField() || b instanceof ITypeBinding t && t.isTypeVariable()) {
            return null;
        }
        ITypeBinding cfType = classFileType(b);
        if (cfType == null || cfType.isFromSource() && !isFromClassFileUnit(units, cfType) || cfType.isPrimitive() || cfType.isNullType()) {
            return null;
        }
        return binaryLocation(units.options, cfType, b, decompiledWholeNode);
    }

    private static boolean isFromClassFileUnit(Units units, ITypeBinding t) {
        Unit owner = units.typeOwners.get(topLevel(t).getKey());
        return owner != null && owner.isClassFile();
    }

    static ClassFileDesc classFileOf(Options options, ITypeBinding t) {
        String binary = t.getTypeDeclaration().getBinaryName();
        if (binary == null) {
            return null;
        }
        return ClassFileService.locate(options.classpath, binary);
    }

    /** Location of a binary element (requires class file content support). */
    static RawLocation binaryLocation(Options options, ITypeBinding cfType, IBinding b, boolean decompiledWholeNode) {
        if (!options.includeClassFiles) {
            return null;
        }
        ClassFileDesc desc = classFileOf(options, cfType);
        if (desc == null) {
            return null;
        }
        boolean attached = ClassFileService.hasAttachedSource(desc, options.attachments);
        if (!attached && !options.includeDecompiled) {
            return emptyLocation(desc);
        }
        Unit u = parseClassFile(desc, options);
        if (u == null) {
            return emptyLocation(desc);
        }
        ASTNode decl = findDeclaration(u.cu, key(b));
        if (decl == null) {
            decl = findDeclarationByName(u.cu, b);
        }
        if (decl == null) {
            return emptyLocation(desc);
        }
        if (!attached && decompiledWholeNode && decl instanceof MethodDeclaration) {
            return u.location(decl.getStartPosition(), decl.getLength());
        }
        ASTNode name = nameNode(decl);
        return u.location(name.getStartPosition(), name.getLength());
    }

    static RawLocation emptyLocation(ClassFileDesc desc) {
        RawLocation l = new RawLocation();
        l.classFile = desc;
        return l;
    }

    // ── Definition ────────────────────────────────────────────────────────

    static List<RawLocation> definition(Units units, int line, int character) {
        Unit u = units.target;
        if (u == null) {
            return List.of();
        }
        int offset = u.offset(line, character);
        IBinding element = elementAt(u, offset);
        RawLocation loc;
        if (element == null) {
            loc = breakContinue(u, offset);
        } else {
            loc = elementLocation(units, element, true);
        }
        return loc == null ? List.of() : List.of(loc);
    }

    private static RawLocation breakContinue(Unit u, int offset) {
        if (offset < 0) {
            return null;
        }
        ASTNode selected = NodeFinder.perform(u.cu, offset, 0);
        ASTNode node = null;
        SimpleName label = null;
        if (selected instanceof BreakStatement bs) {
            node = bs;
            label = bs.getLabel();
        } else if (selected instanceof ContinueStatement cs) {
            node = cs;
            label = cs.getLabel();
        } else if (selected instanceof SimpleName && selected.getParent() instanceof BreakStatement bs) {
            node = bs;
            label = bs.getLabel();
        } else if (selected instanceof SimpleName && selected.getParent() instanceof ContinueStatement cs) {
            node = cs;
            label = cs.getLabel();
        }
        if (node == null) {
            return null;
        }
        ASTNode parent = node.getParent();
        ASTNode target = null;
        while (parent != null) {
            if (parent instanceof MethodDeclaration || parent instanceof Initializer) {
                break;
            }
            if (label == null) {
                if (parent instanceof ForStatement || parent instanceof EnhancedForStatement || parent instanceof WhileStatement || parent instanceof DoStatement) {
                    target = parent;
                    break;
                }
                if (node instanceof BreakStatement && (parent instanceof SwitchStatement || parent instanceof SwitchExpression)) {
                    target = parent;
                    break;
                }
            } else if (parent instanceof LabeledStatement ls && ls.getLabel().getIdentifier().equals(label.getIdentifier())) {
                target = ls;
                break;
            }
            parent = parent.getParent();
        }
        if (target == null) {
            return null;
        }
        int start = target.getStartPosition();
        // TokenScanner.getNextEndOffset(node.getStartPosition(), true): end of the break/continue keyword.
        int end = keywordEnd(u.source, node.getStartPosition()) - start;
        if (start < 0 || end < 0) {
            return null;
        }
        return u.location(start, end);
    }

    private static int keywordEnd(String source, int pos) {
        int i = pos;
        while (i < source.length() && Character.isJavaIdentifierPart(source.charAt(i))) {
            i++;
        }
        return i;
    }

    // ── Type definition ───────────────────────────────────────────────────

    static List<RawLocation> typeDefinition(Units units, int line, int character) {
        Unit u = units.target;
        if (u == null) {
            return null;
        }
        int offset = u.offset(line, character);
        if (offset < 0) {
            return null;
        }
        ASTNode covering = new NodeFinder(u.cu, offset, 0).getCoveringNode();
        if (!(covering instanceof SimpleName name)) {
            return null;
        }
        IBinding b = name.resolveBinding();
        ITypeBinding type = null;
        if (b instanceof IVariableBinding v) {
            type = v.getType();
        } else if (b instanceof ITypeBinding t) {
            type = t;
        }
        if (type == null) {
            return null;
        }
        if (type.isArray()) {
            type = type.getElementType();
        }
        if (type.isPrimitive() || type.isNullType() || type.isRecovered()) {
            return null;
        }
        if (type.isCapture() || type.isWildcardType()) {
            type = type.getErasure();
        }
        RawLocation loc = elementLocation(units, type.isTypeVariable() ? type : type.getTypeDeclaration(), true);
        return loc == null ? null : List.of(loc);
    }

    // ── Declaration ───────────────────────────────────────────────────────

    static List<RawLocation> declaration(Units units, int line, int character) {
        Unit u = units.target;
        if (u == null) {
            return List.of();
        }
        IBinding element = elementAt(u, u.offset(line, character));
        if (!(element instanceof IMethodBinding method)) {
            return List.of();
        }
        IMethodBinding decl = findDeclaringMethod(method);
        if (decl == null) {
            return List.of();
        }
        RawLocation loc = elementLocation(units, decl.getMethodDeclaration(), true);
        return loc == null ? List.of() : List.of(loc);
    }

    /** {@code MethodOverrideTester.findDeclaringMethod(method, false)}. */
    static IMethodBinding findDeclaringMethod(IMethodBinding method) {
        IMethodBinding result = null;
        IMethodBinding overridden = findOverriddenMethod(method);
        Set<String> seen = new LinkedHashSet<>();
        while (overridden != null && seen.add(overridden.getKey())) {
            result = overridden;
            overridden = findOverriddenMethod(result);
        }
        return result;
    }

    static IMethodBinding findOverriddenMethod(IMethodBinding overriding) {
        int mods = overriding.getModifiers();
        if (Modifier.isPrivate(mods) || Modifier.isStatic(mods) || overriding.isConstructor()) {
            return null;
        }
        ITypeBinding type = overriding.getDeclaringClass();
        if (type == null) {
            return null;
        }
        ITypeBinding superClass = type.getSuperclass();
        if (superClass != null) {
            IMethodBinding res = findOverriddenMethodInHierarchy(superClass, overriding);
            if (res != null) {
                return res;
            }
        }
        for (ITypeBinding intf : type.getInterfaces()) {
            IMethodBinding res = findOverriddenMethodInHierarchy(intf, overriding);
            if (res != null) {
                return res;
            }
        }
        return null;
    }

    static IMethodBinding findOverriddenMethodInHierarchy(ITypeBinding type, IMethodBinding overriding) {
        IMethodBinding method = findOverriddenMethodInType(type, overriding);
        if (method != null) {
            return method;
        }
        ITypeBinding superClass = type.getSuperclass();
        if (superClass != null) {
            IMethodBinding res = findOverriddenMethodInHierarchy(superClass, overriding);
            if (res != null) {
                return res;
            }
        }
        for (ITypeBinding intf : type.getInterfaces()) {
            IMethodBinding res = findOverriddenMethodInHierarchy(intf, overriding);
            if (res != null) {
                return res;
            }
        }
        return null;
    }

    /** A method of {@code type} that {@code overriding} overrides (or has the same signature as). */
    static IMethodBinding findOverriddenMethodInType(ITypeBinding type, IMethodBinding overriding) {
        for (IMethodBinding candidate : type.getDeclaredMethods()) {
            if (candidate.isConstructor() || !candidate.getName().equals(overriding.getName())) {
                continue;
            }
            int mods = candidate.getModifiers();
            if (Modifier.isPrivate(mods) || Modifier.isStatic(mods)) {
                continue;
            }
            if (overriding.overrides(candidate) || overriding.isSubsignature(candidate)
                    || candidate.getMethodDeclaration().getKey().equals(overriding.getMethodDeclaration().getKey())) {
                return candidate.getMethodDeclaration();
            }
        }
        return null;
    }

    // ── Implementations ───────────────────────────────────────────────────

    static final class TypeInfo {
        Unit unit;
        ASTNode decl;
        ITypeBinding binding;
    }

    static List<TypeInfo> sourceTypes(Units units) {
        List<TypeInfo> out = new ArrayList<>();
        for (Unit u : units.sourceUnits()) {
            u.cu.accept(new ASTVisitor() {
                private boolean add(ASTNode node, ITypeBinding b) {
                    if (b != null) {
                        TypeInfo t = new TypeInfo();
                        t.unit = u;
                        t.decl = node;
                        t.binding = b.getTypeDeclaration();
                        out.add(t);
                    }
                    return true;
                }

                @Override public boolean visit(TypeDeclaration node) { return add(node, node.resolveBinding()); }
                @Override public boolean visit(EnumDeclaration node) { return add(node, node.resolveBinding()); }
                @Override public boolean visit(RecordDeclaration node) { return add(node, node.resolveBinding()); }
                @Override public boolean visit(AnnotationTypeDeclaration node) { return add(node, node.resolveBinding()); }
                @Override public boolean visit(AnonymousClassDeclaration node) { return add(node, node.resolveBinding()); }
            });
        }
        return out;
    }

    static List<ITypeBinding> directSupertypes(ITypeBinding t) {
        List<ITypeBinding> out = new ArrayList<>();
        if (t.getSuperclass() != null) {
            out.add(t.getSuperclass().getTypeDeclaration());
        }
        for (ITypeBinding i : t.getInterfaces()) {
            out.add(i.getTypeDeclaration());
        }
        return out;
    }

    /** All (transitive) source subtypes of {@code type}, breadth first. */
    static List<TypeInfo> allSubtypes(List<TypeInfo> types, ITypeBinding type) {
        List<TypeInfo> out = new ArrayList<>();
        Set<String> seen = new LinkedHashSet<>();
        List<String> frontier = new ArrayList<>(List.of(type.getTypeDeclaration().getKey()));
        seen.add(frontier.get(0));
        while (!frontier.isEmpty()) {
            List<String> next = new ArrayList<>();
            for (String k : frontier) {
                for (TypeInfo ti : types) {
                    if (seen.contains(ti.binding.getKey())) {
                        continue;
                    }
                    for (ITypeBinding s : directSupertypes(ti.binding)) {
                        if (s.getKey().equals(k)) {
                            seen.add(ti.binding.getKey());
                            out.add(ti);
                            next.add(ti.binding.getKey());
                            break;
                        }
                    }
                }
            }
            frontier = next;
        }
        return out;
    }

    static Set<String> allSupertypeKeys(ITypeBinding t) {
        Set<String> out = new LinkedHashSet<>();
        List<ITypeBinding> work = new ArrayList<>(directSupertypes(t));
        while (!work.isEmpty()) {
            ITypeBinding s = work.remove(0);
            if (out.add(s.getKey())) {
                work.addAll(directSupertypes(s));
            }
        }
        return out;
    }

    static RawLocation typeLocation(TypeInfo ti) {
        ASTNode name = nameNode(ti.decl);
        return ti.unit.location(name.getStartPosition(), name.getLength());
    }

    static ITypeBinding parentType(ASTNode node) {
        ASTNode p = node.getParent();
        while (p != null) {
            if (p instanceof AbstractTypeDeclaration td) {
                return td.resolveBinding();
            }
            if (p instanceof AnonymousClassDeclaration acd) {
                return acd.resolveBinding();
            }
            p = p.getParent();
        }
        return null;
    }

    static List<RawLocation> implementations(Units units, int line, int character) {
        Unit u = units.target;
        if (u == null) {
            return List.of();
        }
        int offset = u.offset(line, character);
        IBinding element = elementAt(u, offset);
        if (!(element instanceof ITypeBinding t && !t.isTypeVariable() || element instanceof IMethodBinding)) {
            return List.of();
        }
        List<RawLocation> locations;
        if (element instanceof ITypeBinding type) {
            locations = new ArrayList<>();
            for (TypeInfo ti : allSubtypes(sourceTypes(units), type)) {
                locations.add(typeLocation(ti));
            }
        } else {
            List<RawLocation> found = methodImplementations(units, u, offset, (IMethodBinding) element);
            locations = found == null ? new ArrayList<>() : new ArrayList<>(found);
        }
        if (shouldIncludeDefinition(u, offset, element, locations)) {
            RawLocation def = elementLocation(units, element, true);
            if (def != null) {
                locations.add(0, def);
            }
        }
        return locations;
    }

    private static boolean isUnimplemented(IBinding element) {
        if (element instanceof IMethodBinding m) {
            return Modifier.isAbstract(m.getModifiers()) || m.getDeclaringClass() != null && m.getDeclaringClass().isInterface();
        }
        if (element instanceof ITypeBinding t) {
            return Modifier.isAbstract(t.getModifiers()) || t.isInterface();
        }
        return false;
    }

    private static boolean shouldIncludeDefinition(Unit u, int offset, IBinding element, List<RawLocation> implementations) {
        if (isUnimplemented(element) && !implementations.isEmpty()) {
            return false;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        return node instanceof SimpleName && !(node.getParent() instanceof MethodDeclaration
                || node.getParent() instanceof SuperMethodInvocation || node.getParent() instanceof AbstractTypeDeclaration);
    }

    private static List<RawLocation> methodImplementations(Units units, Unit u, int offset, IMethodBinding method) {
        int mods = method.getModifiers();
        ITypeBinding declaring = method.getDeclaringClass();
        if (Modifier.isPrivate(mods) || Modifier.isFinal(mods) || Modifier.isStatic(mods) || method.isConstructor()
                || declaring != null && Modifier.isFinal(declaring.getModifiers())) {
            return null;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        ITypeBinding parentTypeBinding = null;
        if (node instanceof SimpleName) {
            ASTNode parent = node.getParent();
            if (parent instanceof MethodInvocation mi) {
                Expression expression = mi.getExpression();
                parentTypeBinding = expression == null ? parentType(node) : expression.resolveTypeBinding();
            } else if (parent instanceof SuperMethodInvocation) {
                RawLocation l = elementLocation(units, method, false);
                return l == null ? List.of() : List.of(l);
            } else if (parent instanceof MethodDeclaration) {
                parentTypeBinding = parentType(node);
            }
        }
        if (parentTypeBinding != null && parentTypeBinding.isTypeVariable()) {
            ITypeBinding[] bounds = parentTypeBinding.getTypeBounds();
            parentTypeBinding = bounds.length > 0 ? bounds[0].getTypeDeclaration() : null;
        }
        if (parentTypeBinding == null) {
            return null;
        }
        ITypeBinding receiver = parentTypeBinding.getTypeDeclaration();
        List<TypeInfo> types = sourceTypes(units);
        Set<String> scope = new LinkedHashSet<>();
        if (receiver.isInterface()) {
            ITypeBinding focus = declaring.getTypeDeclaration();
            scope.add(focus.getKey());
            scope.addAll(allSupertypeKeys(focus));
            for (TypeInfo ti : allSubtypes(types, focus)) {
                scope.add(ti.binding.getKey());
            }
        } else if (findOverriddenMethodInType(receiver, method) == null) {
            scope.add(receiver.getKey());
            scope.addAll(allSupertypeKeys(receiver));
            for (TypeInfo ti : allSubtypes(types, receiver)) {
                scope.add(ti.binding.getKey());
            }
        } else {
            if (Modifier.isAbstract(method.getModifiers())) {
                scope.add(receiver.getKey());
            }
            for (TypeInfo ti : allSubtypes(types, receiver)) {
                scope.add(ti.binding.getKey());
            }
        }
        List<RawLocation> results = new ArrayList<>();
        for (TypeInfo ti : types) {
            if (!scope.contains(ti.binding.getKey())) {
                continue;
            }
            List<?> bodies = ti.decl instanceof AbstractTypeDeclaration td ? td.bodyDeclarations()
                    : ((AnonymousClassDeclaration) ti.decl).bodyDeclarations();
            for (Object o : bodies) {
                if (!(o instanceof MethodDeclaration md) || md.isConstructor()) {
                    continue;
                }
                IMethodBinding mb = md.resolveBinding();
                if (mb == null || !mb.getName().equals(method.getName()) || !matchesSignature(mb, method)) {
                    continue;
                }
                if (Modifier.isAbstract(mb.getModifiers())) {
                    continue;
                }
                results.add(ti.unit.location(md.getName().getStartPosition(), md.getName().getLength()));
            }
        }
        return results;
    }

    private static boolean matchesSignature(IMethodBinding candidate, IMethodBinding method) {
        return candidate.getMethodDeclaration().getKey().equals(method.getKey()) || candidate.overrides(method)
                || candidate.isSubsignature(method) || sameErasures(candidate, method);
    }

    // ── References ────────────────────────────────────────────────────────

    static List<RawLocation> references(Units units, int line, int character) {
        Unit u = units.target;
        List<RawLocation> locations = new ArrayList<>();
        if (u == null) {
            return locations;
        }
        int offset = u.offset(line, character);
        IBinding element = elementAt(u, offset);
        if (element == null) {
            element = memberAt(u, offset);
        }
        if (element == null) {
            return locations;
        }
        Options o = units.options;
        search(units, element, o.includeDeclaration, locations);
        if (o.includeAccessors && element instanceof IVariableBinding field && field.isField()) {
            ITypeBinding owner = field.getDeclaringClass();
            if (owner != null) {
                IMethodBinding getter = getter(field, owner);
                if (getter != null) {
                    search(units, getter, false, locations);
                }
                IMethodBinding setter = setter(field, owner);
                if (setter != null) {
                    search(units, setter, false, locations);
                }
                ITypeBinding builder = builderType(field, owner);
                if (builder != null) {
                    for (IMethodBinding m : builder.getDeclaredMethods()) {
                        ITypeBinding[] params = m.getParameterTypes();
                        if (params.length == 1 && m.getName().equals(field.getName())
                                && params[0].getErasure().getQualifiedName().equals(field.getType().getErasure().getQualifiedName())) {
                            search(units, m.getMethodDeclaration(), false, locations);
                        }
                    }
                }
            }
        }
        return locations;
    }

    /** References of elements given by binding key (another project's search). */
    static List<RawLocation> referencesByKeys(Units units, List<String> searchKeys) {
        List<RawLocation> locations = new ArrayList<>();
        for (String sk : searchKeys) {
            int bar = sk.indexOf('|');
            boolean includeDeclaration = Boolean.parseBoolean(sk.substring(0, bar));
            IBinding b = units.keyBindings.get(sk.substring(bar + 1));
            if (b != null) {
                search(units, normalize(b), includeDeclaration, locations);
            }
        }
        return locations;
    }

    private static String capitalize(String s) {
        return s.isEmpty() ? s : Character.toUpperCase(s.charAt(0)) + s.substring(1);
    }

    /** {@code GetterSetterUtil.getGetter}. */
    static IMethodBinding getter(IVariableBinding field, ITypeBinding owner) {
        boolean bool = "boolean".equals(field.getType().getQualifiedName());
        String name = field.getName();
        List<String> names = new ArrayList<>();
        if (bool) {
            names.add(name.startsWith("is") && name.length() > 2 && Character.isUpperCase(name.charAt(2)) ? name : "is" + capitalize(name));
        }
        names.add("get" + capitalize(name));
        for (String n : names) {
            for (IMethodBinding m : owner.getDeclaredMethods()) {
                if (m.getName().equals(n) && m.getParameterTypes().length == 0) {
                    return m.getMethodDeclaration();
                }
            }
            if (bool) {
                break;
            }
        }
        return null;
    }

    /** {@code GetterSetterUtil.getSetter}. */
    static IMethodBinding setter(IVariableBinding field, ITypeBinding owner) {
        String name = field.getName();
        boolean bool = "boolean".equals(field.getType().getQualifiedName());
        String base = bool && name.startsWith("is") && name.length() > 2 && Character.isUpperCase(name.charAt(2)) ? name.substring(2) : name;
        String n = "set" + capitalize(base);
        for (IMethodBinding m : owner.getDeclaredMethods()) {
            ITypeBinding[] p = m.getParameterTypes();
            if (m.getName().equals(n) && p.length == 1
                    && p[0].getErasure().getQualifiedName().equals(field.getType().getErasure().getQualifiedName())) {
                return m.getMethodDeclaration();
            }
        }
        return null;
    }

    /** {@code ReferencesHandler.getBuilderName}: {@code Owner.OwnerBuilder} or {@code @Builder(builderClassName)}. */
    static ITypeBinding builderType(IVariableBinding field, ITypeBinding owner) {
        String builderName = owner.getName() + "Builder";
        for (IAnnotationBinding a : owner.getAnnotations()) {
            String an = a.getAnnotationType() == null ? "" : a.getAnnotationType().getQualifiedName();
            if (an.equals("Builder") || an.equals("lombok.Builder") || an.endsWith(".Builder")) {
                for (IMemberValuePairBinding pair : a.getDeclaredMemberValuePairs()) {
                    if ("builderClassName".equals(pair.getName()) && pair.getValue() instanceof String s && !s.isEmpty()) {
                        builderName = s;
                    }
                }
            }
        }
        for (ITypeBinding member : owner.getDeclaredTypes()) {
            if (member.getName().equals(builderName)) {
                return member;
            }
        }
        return null;
    }

    /** {@code ReferencesHandler.search}: references (and optionally the declaration) of {@code element}. */
    static void search(Units units, IBinding element, boolean includeDeclaration, List<RawLocation> out) {
        String key = key(element);
        if (key == null) {
            return;
        }
        units.searched.add(includeDeclaration + "|" + key);
        if (!(sourceDeclaration(units, element) != null && !sourceDeclaration(units, element).unit.isClassFile())) {
            units.searchedSourceOnly = false;
        }
        boolean local = element instanceof IVariableBinding v && !v.isField() || element instanceof ITypeBinding t && t.isTypeVariable();
        for (Unit u : units.sourceUnits()) {
            List<int[]> matches = matches(u, element, key, includeDeclaration, false);
            for (int[] m : matches) {
                out.add(u.location(m[0], m[1]));
            }
        }
        if (local || !units.options.includeClassFiles) {
            return;
        }
        // Library class files (application libraries) referencing a binary element.
        ITypeBinding cfType = classFileType(element);
        if (cfType == null || cfType.isFromSource() && !isFromClassFileUnit(units, cfType)) {
            return;
        }
        // Candidates grouped by their top-level class file (one source/decompiled unit each).
        Map<String, ClassFileDesc> tops = new java.util.LinkedHashMap<>();
        Map<String, ClassFileDesc> firstCandidate = new java.util.HashMap<>();
        ClassFileDesc home = classFileOf(units.options, topLevel(cfType));
        boolean insideJre = home == null || home.isJrt();
        List<String> libraries = units.options.libraries != null ? units.options.libraries : new ArrayList<>(units.options.classpath);
        if (units.options.libraries == null) {
            libraries.add("jrt");
        }
        for (String lib : libraries) {
            if (units.options.skipLibraries.contains(lib) || lib.equals("jrt") && !insideJre) {
                continue;
            }
            units.scannedLibraries.add(lib);
            List<ClassFileDesc> candidates = lib.equals("jrt") ? LibraryReferences.jrtCandidates(element, cfType)
                    : LibraryReferences.candidates(lib, element, cfType);
            for (ClassFileDesc cf : candidates) {
                ClassFileDesc top = topDesc(cf);
                if (tops.putIfAbsent(top.key(), top) == null) {
                    firstCandidate.put(top.key(), cf);
                }
            }
        }
        List<ClassFileDesc> toParse = new ArrayList<>();
        Map<String, Unit> parsed = new java.util.HashMap<>();
        for (ClassFileDesc top : tops.values()) {
            if (units.target != null && units.target.isClassFile() && topDesc(units.target.classFile).key().equals(top.key())) {
                parsed.put(top.key(), units.target);
                continue;
            }
            boolean attached = ClassFileService.hasAttachedSource(top, units.options.attachments);
            if (attached || units.options.includeDecompiled) {
                toParse.add(top);
            }
        }
        for (Unit cu : parseClassFiles(toParse, units.options)) {
            parsed.put(cu.classFile.key(), cu);
        }
        for (ClassFileDesc top : tops.values()) {
            Unit cu = parsed.get(top.key());
            if (cu == null) {
                continue;
            }
            boolean attached = ClassFileService.hasAttachedSource(top, units.options.attachments);
            // Attached source: precise matches, each in the class file of its
            // enclosing type; decompiled: every occurrence in the decompiled
            // unit (JDTUtils.searchDecompiledSources with the OccurrencesFinder).
            List<int[]> matches = matches(cu, element, key, includeDeclaration || !attached, !attached);
            for (int[] m : matches) {
                RawLocation l = cu.location(m[0], m[1]);
                l.uri = null;
                l.classFile = attached ? enclosingClassFile(cu, top, m[0]) : ClassFileService.complete(firstCandidate.get(top.key()));
                out.add(l);
            }
        }
    }

    /** The class file of the innermost type enclosing {@code offset} in a class file's source. */
    private static ClassFileDesc enclosingClassFile(Unit cu, ClassFileDesc top, int offset) {
        ASTNode node = NodeFinder.perform(cu.cu, offset, 0);
        while (node != null && !(node instanceof AbstractTypeDeclaration) && !(node instanceof AnonymousClassDeclaration)) {
            node = node.getParent();
        }
        ITypeBinding b = node instanceof AbstractTypeDeclaration td ? td.resolveBinding()
                : node instanceof AnonymousClassDeclaration acd ? acd.resolveBinding() : null;
        String binary = b == null ? null : b.getBinaryName();
        if (binary != null) {
            ClassFileDesc d = new ClassFileDesc();
            d.root = top.root;
            d.module = top.module;
            d.packageName = top.packageName;
            d.classFileName = binary.substring(binary.lastIndexOf('.') + 1) + ".class";
            if (ClassFileService.bytes(d) != null) {
                return ClassFileService.complete(d);
            }
        }
        return ClassFileService.complete(top);
    }

    /** Resolve the contents of several class files in one batch. */
    static List<Unit> parseClassFiles(List<ClassFileDesc> descs, Options options) {
        List<Unit> out = new ArrayList<>();
        if (descs.isEmpty()) {
            return out;
        }
        Path tmp = null;
        try {
            tmp = Files.createTempDirectory("jdtls-cf");
            List<String> paths = new ArrayList<>();
            Map<String, Unit> byPath = new java.util.HashMap<>();
            int i = 0;
            for (ClassFileDesc d : descs) {
                String contents = ClassFileService.contents(d, options.attachments);
                if (contents == null || contents.isBlank()) {
                    continue;
                }
                Unit u = new Unit();
                u.classFile = ClassFileService.complete(d);
                u.source = contents;
                Path p = tmp.resolve(Integer.toString(i++) + ClassFileService.unitName(d));
                Files.createDirectories(p.getParent());
                Files.writeString(p, contents, StandardCharsets.UTF_8);
                paths.add(p.toString());
                byPath.put(p.toString(), u);
            }
            if (paths.isEmpty()) {
                return out;
            }
            ASTParser parser = newParser(options);
            String[] encodings = new String[paths.size()];
            Arrays.fill(encodings, "UTF-8");
            parser.createASTs(paths.toArray(new String[0]), encodings, new String[0], new FileASTRequestor() {
                @Override
                public void acceptAST(String sourceFilePath, CompilationUnit ast) {
                    Unit u = byPath.get(sourceFilePath);
                    if (u != null) {
                        u.cu = ast;
                        u.lineStarts = lineStarts(u.source);
                        out.add(u);
                    }
                }
            }, null);
        } catch (IOException e) {
            throw new RuntimeException(e);
        } finally {
            if (tmp != null) {
                deleteRecursively(tmp);
            }
        }
        return out;
    }

    private static ClassFileDesc topDesc(ClassFileDesc cf) {
        String top = ClassFileService.topLevelBinaryName(cf);
        ClassFileDesc d = new ClassFileDesc();
        d.root = cf.root;
        d.module = cf.module;
        d.packageName = cf.packageName;
        d.classFileName = top.substring(top.lastIndexOf('.') + 1) + ".class";
        return d;
    }

    static boolean isDeclarationName(SimpleName n) {
        StructuralPropertyDescriptor loc = n.getLocationInParent();
        return loc == TypeDeclaration.NAME_PROPERTY || loc == EnumDeclaration.NAME_PROPERTY || loc == AnnotationTypeDeclaration.NAME_PROPERTY
                || loc == RecordDeclaration.NAME_PROPERTY || loc == MethodDeclaration.NAME_PROPERTY
                || loc == VariableDeclarationFragment.NAME_PROPERTY || loc == SingleVariableDeclaration.NAME_PROPERTY
                || loc == EnumConstantDeclaration.NAME_PROPERTY || loc == TypeParameter.NAME_PROPERTY
                || loc == AnnotationTypeMemberDeclaration.NAME_PROPERTY;
    }

    /** Offsets/lengths of the matches of {@code element} in {@code u}, in document order. */
    static List<int[]> matches(Unit u, IBinding element, String key, boolean includeDeclaration, boolean allOccurrences) {
        List<int[]> out = new ArrayList<>();
        boolean isType = element instanceof ITypeBinding t && !t.isTypeVariable();
        boolean isCtor = element instanceof IMethodBinding m && m.isConstructor();
        u.cu.accept(new ASTVisitor(true) {
            @Override
            public boolean visit(SimpleName node) {
                IBinding b = node.resolveBinding();
                if (b == null) {
                    return false;
                }
                if (b instanceof IPackageBinding && !(element instanceof IPackageBinding)) {
                    return false;
                }
                if (!key.equals(key(b))) {
                    return false;
                }
                boolean decl = isDeclarationName(node);
                if (decl && !includeDeclaration) {
                    return false;
                }
                ASTNode range = node;
                if (isType && node.getParent() instanceof QualifiedName qn && qn.getName() == node
                        && qn.getQualifier().resolveBinding() instanceof IPackageBinding) {
                    range = qn;
                }
                int start = range.getStartPosition();
                int end = start + range.getLength();
                // MethodReferenceMatch spans the selector through the argument list.
                if (node.getParent() instanceof MethodInvocation mi && mi.getName() == node
                        || node.getParent() instanceof SuperMethodInvocation smi && smi.getName() == node) {
                    ASTNode inv = node.getParent();
                    end = inv.getStartPosition() + inv.getLength();
                }
                out.add(new int[] { start, end - start });
                return false;
            }

            @Override
            public boolean visit(ClassInstanceCreation node) {
                if (isCtor) {
                    IMethodBinding c = node.resolveConstructorBinding();
                    if (c != null && key.equals(key(c))) {
                        Type t = node.getType();
                        out.add(new int[] { t.getStartPosition(), t.getLength() });
                    }
                }
                return true;
            }

            @Override
            public boolean visit(SuperConstructorInvocation node) {
                if (isCtor) {
                    IMethodBinding c = node.resolveConstructorBinding();
                    if (c != null && key.equals(key(c))) {
                        out.add(new int[] { node.getStartPosition(), node.getLength() });
                    }
                }
                return true;
            }

            @Override
            public boolean visit(ConstructorInvocation node) {
                if (isCtor) {
                    IMethodBinding c = node.resolveConstructorBinding();
                    if (c != null && key.equals(key(c))) {
                        out.add(new int[] { node.getStartPosition(), node.getLength() });
                    }
                }
                return true;
            }
        });
        out.sort(Comparator.comparingInt(a -> a[0]));
        return out;
    }

    // ── Class file lookup (ClassFileUtil.getURI) ──────────────────────────

    static ClassFileInfoResponse classFileInfo(long id, Map<String, String> files, Options options, String fqn) {
        String search = fqn;
        String inner = null;
        int dollar = fqn.indexOf('$');
        if (dollar > 0) {
            search = fqn.substring(0, dollar);
            inner = fqn.substring(dollar + 1);
        }
        // Source types first.
        if (files != null) {
            List<String> uris = new ArrayList<>(files.keySet());
            Collections.sort(uris);
            for (String uri : uris) {
                String src = files.get(uri);
                if (src == null || !looksLikeJava(uri)) {
                    continue;
                }
                ASTParser p = ASTParser.newParser(AST.getJLSLatest());
                p.setKind(ASTParser.K_COMPILATION_UNIT);
                p.setSource(src.toCharArray());
                p.setCompilerOptions(BridgeOptions.map(options.sourceLevel));
                CompilationUnit cu = (CompilationUnit) p.createAST(null);
                String pkg = cu.getPackage() == null ? "" : cu.getPackage().getName().getFullyQualifiedName();
                for (Object t : cu.types()) {
                    AbstractTypeDeclaration td = (AbstractTypeDeclaration) t;
                    String name = pkg.isEmpty() ? td.getName().getIdentifier() : pkg + "." + td.getName().getIdentifier();
                    if (name.equalsIgnoreCase(search)) {
                        if (inner == null || hasMember(td, inner.split("\\$"), 0)) {
                            return new ClassFileInfoResponse(id, null, uri);
                        }
                    }
                }
            }
        }
        ClassFileDesc d = ClassFileService.locateIgnoreCase(options.classpath, fqn);
        if (d == null && inner != null) {
            return new ClassFileInfoResponse(id, null, null);
        }
        return new ClassFileInfoResponse(id, d, null);
    }

    private static boolean hasMember(AbstractTypeDeclaration td, String[] parts, int i) {
        if (i >= parts.length) {
            return true;
        }
        for (Object o : td.bodyDeclarations()) {
            if (o instanceof AbstractTypeDeclaration m && m.getName().getIdentifier().equals(parts[i])) {
                return hasMember(m, parts, i + 1);
            }
        }
        return false;
    }

    static Options options(BridgeProtocol.Request req) {
        Options o = new Options();
        o.classpath = req.classpath == null ? List.of() : req.classpath;
        o.sourceLevel = req.sourceLevel == null ? "21" : req.sourceLevel;
        o.includeClassFiles = req.includeClassFiles;
        o.includeDecompiled = req.includeDecompiled == null || req.includeDecompiled;
        o.includeDeclaration = req.includeDeclaration;
        o.includeAccessors = req.includeAccessors == null || req.includeAccessors;
        o.attachments = req.sourceAttachments == null ? Map.of() : req.sourceAttachments;
        o.libraries = req.libraries;
        o.skipLibraries = req.skipLibraries == null ? Set.of() : new java.util.HashSet<>(req.skipLibraries);
        return o;
    }

    /** Entry point for {@code navData}. */
    static Object navData(BridgeProtocol.Request req) {
        Options o = options(req);
        ClassFileDesc cf = ClassFileService.complete(req.classFile);
        String op = req.op == null ? "" : req.op;
        Map<String, String> files = req.files;
        if (cf != null && (op.equals("definition") || op.equals("typeDefinition") || op.equals("declaration") || op.equals("highlight"))) {
            // A class file only references library types.
            files = Map.of();
        }
        if (cf != null && op.equals("highlight") && !ClassFileService.hasAttachedSource(cf, o.attachments)) {
            return new NavDataResponse(req.id, List.of(), false);
        }
        List<String> keys = new ArrayList<>();
        if (req.searchKeys != null) {
            for (String sk : req.searchKeys) {
                keys.add(sk.substring(sk.indexOf('|') + 1));
            }
        }
        Units units = parse(files, o, req.uri, cf, keys);
        if (op.equals("referencesByKeys")) {
            List<RawLocation> locs = referencesByKeys(units, req.searchKeys == null ? List.of() : req.searchKeys);
            NavDataResponse r = new NavDataResponse(req.id, locs, false);
            r.scannedLibraries = new ArrayList<>(units.scannedLibraries);
            return r;
        }
        if (!op.equals("highlight") && unresolvedSelection(units, req.line, req.character)) {
            // SelectionEngine (codeSelect) resolves the selected name even where
            // the compiler's recovery loses it (e.g. a Java 9 construct in a
            // 1.8 project); retry at the latest source level.
            Options latest = options(req);
            latest.sourceLevel = "99";
            Map<String, String> opts = new java.util.HashMap<>(req.options == null ? Map.of() : req.options);
            opts.remove("org.eclipse.jdt.core.compiler.source");
            opts.remove("org.eclipse.jdt.core.compiler.compliance");
            opts.remove("org.eclipse.jdt.core.compiler.codegen.targetPlatform");
            Units retry;
            BridgeOptions.setCurrent(opts);
            try {
                retry = parse(files, latest, req.uri, cf);
            } finally {
                BridgeOptions.setCurrent(req.options);
            }
            if (!unresolvedSelection(retry, req.line, req.character)) {
                retry.options = o;
                units = retry;
            }
        }
        List<RawLocation> result = switch (op) {
            case "definition" -> definition(units, req.line, req.character);
            case "typeDefinition" -> typeDefinition(units, req.line, req.character);
            case "declaration" -> declaration(units, req.line, req.character);
            case "implementation" -> implementations(units, req.line, req.character);
            case "references" -> references(units, req.line, req.character);
            case "highlight" -> OccurrencesFinders.highlights(units.target, req.line, req.character);
            default -> List.of();
        };
        NavDataResponse r = new NavDataResponse(req.id, result == null ? List.of() : result, result == null);
        r.searchKeys = units.searched;
        r.sourceElements = !units.searched.isEmpty() && units.searchedSourceOnly;
        r.scannedLibraries = new ArrayList<>(units.scannedLibraries);
        return r;
    }

    private static boolean unresolvedSelection(Units units, int line, int character) {
        Unit u = units.target;
        if (u == null) {
            return false;
        }
        int offset = u.offset(line, character);
        if (offset < 0) {
            return false;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        if (!(node instanceof SimpleName sn) || sn.getParent() instanceof LabeledStatement
                || sn.getParent() instanceof BreakStatement || sn.getParent() instanceof ContinueStatement) {
            return false;
        }
        return sn.resolveBinding() == null;
    }

    static Object classFileContents(BridgeProtocol.Request req) {
        ClassFileDesc cf = ClassFileService.complete(req.classFile);
        Map<String, String> attachments = req.sourceAttachments == null ? Map.of() : req.sourceAttachments;
        return new ClassFileContentsResponse(req.id, cf == null ? "" : ClassFileService.contents(cf, attachments));
    }

    static Object classFileInfo(BridgeProtocol.Request req) {
        return classFileInfo(req.id, req.files, options(req), req.fqn);
    }
}
