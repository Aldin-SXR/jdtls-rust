package com.jdtls.ecjbridge;

import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeSet;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import org.eclipse.jdt.core.dom.AST;
import org.eclipse.jdt.core.dom.ASTNode;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.ASTVisitor;
import org.eclipse.jdt.core.dom.ClassInstanceCreation;
import org.eclipse.jdt.core.dom.Comment;
import org.eclipse.jdt.core.dom.CompilationUnit;
import org.eclipse.jdt.core.dom.IBinding;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.IVariableBinding;
import org.eclipse.jdt.core.dom.ImportDeclaration;
import org.eclipse.jdt.core.dom.Javadoc;
import org.eclipse.jdt.core.dom.Name;
import org.eclipse.jdt.core.dom.RecordDeclaration;
import org.eclipse.jdt.core.dom.TagElement;
import org.eclipse.jdt.core.dom.TypeDeclaration;

/**
 * Data-only request: the resolved JDT DOM of one compilation unit, flattened
 * in visit order, with the bindings of its names.  Rust turns this into LSP
 * results (semantic tokens).
 *
 * <p>Node layout ({@code int[]}): type, start, length, parent, location,
 * binding, typeBinding, constructorBinding, restrictedIdentifierStart, flags
 * (1 = nested tag, 2 = unattached comment root), tagName — string-valued
 * entries index {@code strings}, -1 means absent.  Binding layout: kind,
 * modifiers, deprecated (0/1), flags, declaringClass.
 */
final class AstBindingsService {

    static final class AstBindingsResponse extends BridgeProtocol.Response {
        public List<String> strings;
        public List<int[]> nodes;
        public List<int[]> bindings;

        AstBindingsResponse(long id, List<String> strings, List<int[]> nodes, List<int[]> bindings) {
            this.id = id;
            this.method = "astBindings";
            this.strings = strings;
            this.nodes = nodes;
            this.bindings = bindings;
        }
    }

    // Binding flags
    static final int ENUM_CONSTANT = 1, RECORD_COMPONENT = 2, FIELD = 4, PARAMETER = 8;
    static final int CONSTRUCTOR = 16, ANNOTATION_MEMBER = 32, GENERIC_METHOD = 64, PARAMETERIZED_METHOD = 128;
    static final int TYPE_VARIABLE = 256, ANNOTATION = 512, RECORD = 1024, INTERFACE = 2048, ENUM = 4096, CLASS = 8192,
            GENERIC_TYPE = 16384, PARAMETERIZED_TYPE = 32768;

    private static final Pattern PACKAGE = Pattern.compile("^\\s*package\\s+([\\w.\\s]+?)\\s*;", Pattern.MULTILINE);
    /** Mirror roots kept for reuse; least recently used ones are deleted. */
    private static final int MAX_MIRRORS = 16;
    private static final Map<String, Map<String, String>> MIRRORS =
        new java.util.LinkedHashMap<>(16, 0.75f, true) {
            @Override
            protected boolean removeEldestEntry(Map.Entry<String, Map<String, String>> eldest) {
                if (size() <= MAX_MIRRORS) {
                    return false;
                }
                deleteTree(mirrorRoot(eldest.getKey()));
                return true;
            }
        };

    static {
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            synchronized (MIRRORS) {
                for (String key : MIRRORS.keySet()) {
                    deleteTree(mirrorRoot(key));
                }
            }
        }));
    }

    private static Path mirrorRoot(String key) {
        return Path.of(System.getProperty("java.io.tmpdir"), "jdtls-rust-src-" + key);
    }

    private static void deleteTree(Path root) {
        try (java.util.stream.Stream<Path> walk = Files.walk(root)) {
            walk.sorted(java.util.Comparator.reverseOrder()).forEach(p -> {
                try {
                    Files.deleteIfExists(p);
                } catch (IOException ignored) {
                    // best effort
                }
            });
        } catch (IOException ignored) {
            // already gone
        }
    }

    private AstBindingsService() {}

    static AstBindingsResponse handle(BridgeProtocol.Request req) {
        CompilationUnit cu = parse(req);
        if (cu == null) {
            return new AstBindingsResponse(req.id, List.of(), List.of(), List.of());
        }
        return new Collector(req.id).collect(cu);
    }

    /**
     * The resolved DOM of {@code req.uri}: the other request files are
     * mirrored into a source folder so bindings resolve across units, virtual
     * documents included.  {@code null} when the unit is not in the request.
     */
    static CompilationUnit parse(BridgeProtocol.Request req) {
        ClassFileService.ClassFileDesc classFile = req.classFile == null ? null : ClassFileService.complete(req.classFile);
        String source = req.files == null ? null : req.files.get(req.uri);
        if (classFile != null) {
            source = ClassFileService.attachedSource(classFile, req.sourceAttachments == null ? Map.of() : req.sourceAttachments);
        }
        if (source == null) {
            return null;
        }
        String[] sourcepath = new String[0];
        try {
            sourcepath = new String[] { mirror(req.files).toString() };
        } catch (IOException e) {
            // resolve against the classpath only
        }
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        String[] cp = req.classpath != null ? req.classpath.toArray(new String[0]) : new String[0];
        Map<String, String> parserOptions = BridgeOptions.map(req.sourceLevel);
        // Standalone ASTParser requires the running VM's system library. An
        // explicit rt.jar/rtstubs.jar otherwise makes java.util imports ambiguous
        // against java.base. Limit the workaround to supplied boot classes.
        if (hasBootClasses(cp)) {
            parserOptions.put("org.eclipse.jdt.core.compiler.ignoreUnnamedModuleForSplitPackage", "enabled");
        }
        parser.setCompilerOptions(parserOptions);
        parser.setUnitName(classFile == null ? unitName(req.uri) : ClassFileService.unitName(classFile));
        BridgeOptions.configureEnvironment(parser, cp, sourcepath);
        return (CompilationUnit) parser.createAST(null);
    }

    private static boolean hasBootClasses(String[] classpath) {
        for (String entry : classpath) {
            Path path = Path.of(entry);
            if (Files.isDirectory(path)) {
                if (Files.exists(path.resolve("java/lang/Object.class"))) {
                    return true;
                }
            } else {
                try (java.util.zip.ZipFile jar = new java.util.zip.ZipFile(entry)) {
                    if (jar.getEntry("java/lang/Object.class") != null) {
                        return true;
                    }
                } catch (IOException ignored) {
                    // Missing or ordinary classpath entries supply no boot classes.
                }
            }
        }
        return false;
    }

    private static String unitName(String uri) {
        String path = null;
        try {
            path = new URI(uri).getPath();
        } catch (Exception e) {
            // fall through
        }
        if (path == null || path.isEmpty()) {
            // Opaque virtual document URIs (`untitled:Foo`).
            path = "/" + uri.substring(Math.max(uri.lastIndexOf('/'), uri.lastIndexOf(':')) + 1);
        }
        return path.endsWith(".java") ? path : path + ".java";
    }

    /**
     * Mirrors the request's source files into a source folder (by package) so
     * the DOM resolver can see the other compilation units, virtual ones too.
     */
    private static Path mirror(Map<String, String> files) throws IOException {
        TreeSet<String> uris = new TreeSet<>(files.keySet());
        String key = hash(String.join("\n", uris));
        Path root = mirrorRoot(key);
        synchronized (MIRRORS) {
            Map<String, String> written = MIRRORS.computeIfAbsent(key, k -> new HashMap<>());
            Files.createDirectories(root);
            for (Map.Entry<String, String> e : files.entrySet()) {
                String uri = e.getKey();
                String content = e.getValue();
                String name = uri.substring(uri.lastIndexOf('/') + 1);
                if (!name.endsWith(".java") || name.equals("module-info.java") || name.contains("%") || name.contains(":")) {
                    continue;
                }
                Matcher m = PACKAGE.matcher(stripComments(content));
                String pkg = m.find() ? m.group(1).replaceAll("\\s", "") : "";
                Path dir = pkg.isEmpty() ? root : root.resolve(pkg.replace('.', '/'));
                Path file = dir.resolve(name);
                String stamp = file + "\u0000" + content.hashCode() + ":" + content.length();
                if (stamp.equals(written.get(uri)) && Files.exists(file)) {
                    continue;
                }
                String previous = written.get(uri);
                if (previous != null) {
                    Files.deleteIfExists(Path.of(previous.substring(0, previous.indexOf('\u0000'))));
                }
                Files.createDirectories(dir);
                Files.writeString(file, content, StandardCharsets.UTF_8);
                written.put(uri, stamp);
            }
        }
        return root;
    }

    private static String stripComments(String s) {
        return s.replaceAll("(?s)/\\*.*?\\*/", " ").replaceAll("//[^\\n]*", " ");
    }

    private static String hash(String s) {
        try {
            byte[] d = MessageDigest.getInstance("SHA-1").digest(s.getBytes(StandardCharsets.UTF_8));
            StringBuilder sb = new StringBuilder();
            for (int i = 0; i < 8; i++) {
                sb.append(String.format("%02x", d[i]));
            }
            return sb.toString();
        } catch (Exception e) {
            return Integer.toHexString(s.hashCode());
        }
    }

    private static final class Collector {
        private final long id;
        private final List<String> strings = new ArrayList<>();
        private final Map<String, Integer> stringIndex = new HashMap<>();
        private final List<int[]> nodes = new ArrayList<>();
        private final List<int[]> bindings = new ArrayList<>();
        private final IdentityHashMap<IBinding, Integer> bindingIndex = new IdentityHashMap<>();

        Collector(long id) {
            this.id = id;
        }

        AstBindingsResponse collect(CompilationUnit cu) {
            walk(cu, false);
            for (Object o : cu.getCommentList()) {
                Comment c = (Comment) o;
                if (c instanceof Javadoc && c.getParent() == null) {
                    walk(c, true);
                }
            }
            return new AstBindingsResponse(id, strings, nodes, bindings);
        }

        private void walk(ASTNode root, boolean commentRoot) {
            final int[] stack = new int[4096];
            final int[] depth = { 0 };
            root.accept(new ASTVisitor(true) {
                @Override
                public boolean preVisit2(ASTNode node) {
                    int parent = depth[0] == 0 ? -1 : stack[depth[0] - 1];
                    int idx = add(node, parent, commentRoot && depth[0] == 0);
                    if (depth[0] < stack.length) {
                        stack[depth[0]] = idx;
                    }
                    depth[0]++;
                    return true;
                }

                @Override
                public void postVisit(ASTNode node) {
                    depth[0]--;
                }
            });
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

        private int add(ASTNode node, int parent, boolean commentRoot) {
            int b = -1, tb = -1, cb = -1, rs = -1, flags = 0, tag = -1;
            try {
                if (node instanceof Name name) {
                    b = binding(name.resolveBinding());
                } else if (node instanceof ImportDeclaration imp) {
                    // An import that does not resolve has no binding (the
                    // recovered binding is an artifact of bindings recovery).
                    IBinding ib = imp.resolveBinding();
                    b = ib == null || ib.isRecovered() ? -1 : binding(ib);
                } else if (node instanceof ClassInstanceCreation cic) {
                    tb = binding(cic.resolveTypeBinding());
                    cb = binding(cic.resolveConstructorBinding());
                } else if (node instanceof TagElement te) {
                    tag = str(te.getTagName());
                    if (te.isNested()) {
                        flags |= 1;
                    }
                } else if (node instanceof TypeDeclaration td) {
                    rs = td.getRestrictedIdentifierStartPosition();
                } else if (node instanceof RecordDeclaration rd) {
                    rs = rd.getRestrictedIdentifierStartPosition();
                }
            } catch (RuntimeException e) {
                // recovered nodes may fail to resolve
            }
            if (commentRoot) {
                flags |= 2;
            }
            String loc = node.getLocationInParent() == null ? null : node.getLocationInParent().getId();
            nodes.add(new int[] { str(node.getClass().getSimpleName()), node.getStartPosition(), node.getLength(), parent,
                    str(loc), b, tb, cb, rs, flags, tag });
            return nodes.size() - 1;
        }

        private int binding(IBinding binding) {
            if (binding == null) {
                return -1;
            }
            Integer known = bindingIndex.get(binding);
            if (known != null) {
                return known;
            }
            int[] data = new int[] { binding.getKind(), 0, 0, 0, -1 };
            int idx = bindings.size();
            bindings.add(data);
            bindingIndex.put(binding, idx);
            try {
                data[1] = binding.getModifiers();
                data[2] = binding.isDeprecated() ? 1 : 0;
                int f = 0;
                if (binding instanceof IVariableBinding v) {
                    if (v.isEnumConstant()) f |= ENUM_CONSTANT;
                    if (v.isRecordComponent()) f |= RECORD_COMPONENT;
                    if (v.isField()) f |= FIELD;
                    if (v.isParameter()) f |= PARAMETER;
                } else if (binding instanceof IMethodBinding m) {
                    if (m.isConstructor()) f |= CONSTRUCTOR;
                    if (m.isAnnotationMember()) f |= ANNOTATION_MEMBER;
                    if (m.isGenericMethod()) f |= GENERIC_METHOD;
                    if (m.isParameterizedMethod()) f |= PARAMETERIZED_METHOD;
                    data[4] = binding(m.getDeclaringClass());
                } else if (binding instanceof ITypeBinding t) {
                    if (t.isTypeVariable()) f |= TYPE_VARIABLE;
                    if (t.isAnnotation()) f |= ANNOTATION;
                    if (t.isRecord()) f |= RECORD;
                    if (t.isInterface()) f |= INTERFACE;
                    if (t.isEnum()) f |= ENUM;
                    if (t.isClass()) f |= CLASS;
                    if (t.isGenericType()) f |= GENERIC_TYPE;
                    if (t.isParameterizedType()) f |= PARAMETERIZED_TYPE;
                }
                data[3] = f;
            } catch (RuntimeException e) {
                // keep what we have
            }
            return idx;
        }
    }
}
