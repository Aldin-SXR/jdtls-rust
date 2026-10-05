package com.jdtls.ecjbridge;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.lang.module.ModuleDescriptor;
import java.lang.module.ModuleFinder;
import java.lang.module.ModuleReference;
import java.net.URI;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.stream.Stream;

import org.eclipse.jdt.core.compiler.CharOperation;
import org.eclipse.jdt.internal.compiler.CompilationResult;
import org.eclipse.jdt.internal.compiler.DefaultErrorHandlingPolicies;
import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.ast.ConstructorDeclaration;
import org.eclipse.jdt.internal.compiler.ast.AbstractMethodDeclaration;
import org.eclipse.jdt.internal.compiler.ast.Argument;
import org.eclipse.jdt.internal.compiler.ast.TypeDeclaration;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileConstants;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileReader;
import org.eclipse.jdt.internal.compiler.env.IBinaryMethod;
import org.eclipse.jdt.internal.compiler.impl.CompilerOptions;
import org.eclipse.jdt.internal.compiler.lookup.ExtraCompilerModifiers;
import org.eclipse.jdt.internal.compiler.parser.Parser;
import org.eclipse.jdt.internal.compiler.problem.DefaultProblemFactory;
import org.eclipse.jdt.internal.compiler.problem.ProblemReporter;

/**
 * Type-name index over the classpath, the JDK (jrt) and in-memory sources.
 * Plays the role of the JDT search index for code assist: prefix / camel-case
 * type-name search, package search and constructor search for types that are
 * not imported yet.  Binary roots are listed once per JVM; class file headers
 * are only read for names that match a query.
 */
final class TypeIndex {

    /** One indexed type. Names are in readable (dotted) form. */
    static final class TypeInfo {
        final String packageName;      // "java.util" or ""
        final String simpleName;       // "Entry"
        final String[] enclosingNames; // {"Map"} for java.util.Map.Entry
        final int modifiers;
        final String path;             // source URI, or "<root>|<binaryName>"
        final boolean isSource;
        final boolean isTest;
        final List<CtorInfo> constructors; // source types only (binary read lazily)

        TypeInfo(String pkg, String simple, String[] enclosing, int modifiers, String path, boolean isSource,
                boolean isTest, List<CtorInfo> ctors) {
            this.packageName = pkg;
            this.simpleName = simple;
            this.enclosingNames = enclosing;
            this.modifiers = modifiers;
            this.path = path;
            this.isSource = isSource;
            this.isTest = isTest;
            this.constructors = ctors;
        }

        String qualifiedName() {
            StringBuilder sb = new StringBuilder();
            if (!packageName.isEmpty()) sb.append(packageName).append('.');
            for (String e : enclosingNames) sb.append(e).append('.');
            return sb.append(simpleName).toString();
        }
    }

    static final class CtorInfo {
        final int modifiers;
        final char[] signature;     // may be null for source constructors
        final char[][] parameterTypes;
        final char[][] parameterNames;
        CtorInfo(int modifiers, char[] signature, char[][] parameterTypes, char[][] parameterNames) {
            this.modifiers = modifiers;
            this.signature = signature;
            this.parameterTypes = parameterTypes;
            this.parameterNames = parameterNames;
        }
    }

    // ── Binary roots ─────────────────────────────────────────────────────────

    /** A jar, class folder or the JDK: binary names ("java/util/Map$Entry") of its class files. */
    static final class BinaryRoot {
        final String id;
        final List<String> binaryNames = new ArrayList<>();
        final Set<String> packages = new HashSet<>();   // dotted
        final Map<String, TypeInfo> infos = new ConcurrentHashMap<>();
        final Map<String, Boolean> skipped = new ConcurrentHashMap<>();
        // jrt: binary name → module path
        final Map<String, Path> jrtPaths = new HashMap<>();
        JarFile jar;
        File dir;

        BinaryRoot(String id) { this.id = id; }

        byte[] read(String binaryName) {
            try {
                if (jar != null) {
                    JarEntry e = jar.getJarEntry(binaryName + ".class");
                    if (e == null) return null;
                    try (InputStream is = jar.getInputStream(e)) {
                        return is.readAllBytes();
                    }
                }
                if (dir != null) {
                    return Files.readAllBytes(new File(dir, binaryName + ".class").toPath());
                }
                Path p = jrtPaths.get(binaryName);
                return p == null ? null : Files.readAllBytes(p);
            } catch (IOException e) {
                return null;
            }
        }

        /** Header info for {@code binaryName}, or null for local/anonymous/synthetic types. */
        TypeInfo info(String binaryName) {
            TypeInfo cached = infos.get(binaryName);
            if (cached != null) return cached;
            if (skipped.containsKey(binaryName)) return null;
            TypeInfo info = null;
            byte[] bytes = read(binaryName);
            if (bytes != null) {
                try {
                    ClassFileReader r = new ClassFileReader(bytes, binaryName.toCharArray());
                    if (!r.isAnonymous() && !r.isLocal() && (r.getModifiers() & ClassFileConstants.AccSynthetic) == 0) {
                        String[] parts = enclosingAndSimple(binaryName, r);
                        int slash = binaryName.lastIndexOf('/');
                        String pkg = slash < 0 ? "" : binaryName.substring(0, slash).replace('/', '.');
                        String[] enclosing = new String[parts.length - 1];
                        System.arraycopy(parts, 0, enclosing, 0, enclosing.length);
                        List<CtorInfo> ctors = new ArrayList<>();
                        IBinaryMethod[] methods = r.getMethods();
                        if (methods != null) {
                            for (IBinaryMethod m : methods) {
                                if (!m.isConstructor() || (m.getModifiers() & ClassFileConstants.AccSynthetic) != 0) continue;
                                char[] sig = m.getGenericSignature() != null ? m.getGenericSignature() : m.getMethodDescriptor();
                                ctors.add(new CtorInfo(m.getModifiers(), CharOperation.replaceOnCopy(sig, '/', '.'), null,
                                        m.getArgumentNames()));
                            }
                        }
                        info = new TypeInfo(pkg, parts[parts.length - 1], enclosing, r.getModifiers(),
                                id + "|" + binaryName, false, false, ctors);
                    }
                } catch (Exception e) {
                    info = null;
                }
            }
            if (info == null) {
                skipped.put(binaryName, Boolean.TRUE);
            } else {
                infos.put(binaryName, info);
            }
            return info;
        }

        private static String[] enclosingAndSimple(String binaryName, ClassFileReader r) {
            int slash = binaryName.lastIndexOf('/');
            String local = binaryName.substring(slash + 1);
            if (!r.isMember()) return new String[] { local };
            // Walk the enclosing chain via the InnerClasses attribute.
            List<String> names = new ArrayList<>();
            char[] simple = r.getSourceName();
            names.add(simple == null ? local.substring(local.lastIndexOf('$') + 1) : new String(simple));
            char[] enclosing = r.getEnclosingTypeName();
            String rest = enclosing == null ? null : new String(enclosing);
            if (rest != null) {
                String encLocal = rest.substring(rest.lastIndexOf('/') + 1);
                // Approximation for nested chains: split the enclosing binary name on '$'.
                String[] segs = encLocal.split("\\$");
                for (int i = segs.length - 1; i >= 0; i--) names.add(0, segs[i]);
            }
            return names.toArray(new String[0]);
        }
    }

    private static final Map<String, BinaryRoot> ROOTS = new ConcurrentHashMap<>();
    private static volatile BinaryRoot JRT;
    private static volatile BinaryRoot JRT_ALL;

    /** The JDK image. Below Java 9 JDT reads the image as a flat classpath
     * (every package of every system module, as jdt.ls proposes e.g.
     * {@code com.sun.org.apache.bcel.internal.classfile.Method} in a 1.8
     * project); from 9 on only unqualified exports are visible to the
     * unnamed module. */
    static BinaryRoot jrt(boolean allPackages) {
        BinaryRoot r = allPackages ? JRT_ALL : JRT;
        if (r != null) return r;
        synchronized (TypeIndex.class) {
            r = allPackages ? JRT_ALL : JRT;
            if (r != null) return r;
            BinaryRoot root = new BinaryRoot("jrt");
            try {
                FileSystem fs = FileSystems.getFileSystem(URI.create("jrt:/"));
                // Only packages exported (unqualified) by system modules are visible to unnamed-module code.
                Map<String, Set<String>> exported = new HashMap<>();
                for (ModuleReference ref : ModuleFinder.ofSystem().findAll()) {
                    ModuleDescriptor d = ref.descriptor();
                    Set<String> pk = new HashSet<>();
                    for (ModuleDescriptor.Exports ex : d.exports()) {
                        if (!ex.isQualified()) pk.add(ex.source());
                    }
                    exported.put(d.name(), pk);
                }
                // JavaProject.defaultRootModules + filterLimitedModules: the
                // modules exporting a package unqualified, and what they require.
                Set<String> rootModules = new HashSet<>();
                java.util.ArrayDeque<String> todo = new java.util.ArrayDeque<>();
                for (Map.Entry<String, Set<String>> e : exported.entrySet()) {
                    if (!e.getValue().isEmpty()) todo.add(e.getKey());
                }
                while (!todo.isEmpty()) {
                    String m = todo.poll();
                    if (!rootModules.add(m)) continue;
                    ModuleFinder.ofSystem().find(m).ifPresent(ref -> {
                        for (ModuleDescriptor.Requires req : ref.descriptor().requires()) todo.add(req.name());
                    });
                }
                Path modules = fs.getPath("/modules");
                try (Stream<Path> mods = Files.list(modules)) {
                    for (Path mod : (Iterable<Path>) mods::iterator) {
                        String modName = mod.getFileName().toString().replace("/", "");
                        Set<String> pk = exported.getOrDefault(modName, Collections.emptySet());
                        if (allPackages && !rootModules.contains(modName)) continue;
                        try (Stream<Path> files = Files.walk(mod)) {
                            for (Path p : (Iterable<Path>) files::iterator) {
                                String s = p.toString();
                                if (!s.endsWith(".class")) continue;
                                String rel = mod.relativize(p).toString();
                                if (rel.equals("module-info.class")) continue;
                                String bin = rel.substring(0, rel.length() - 6);
                                int slash = bin.lastIndexOf('/');
                                String pkg = slash < 0 ? "" : bin.substring(0, slash).replace('/', '.');
                                if (!allPackages && !pk.contains(pkg)) continue;
                                root.binaryNames.add(bin);
                                root.packages.add(pkg);
                                root.jrtPaths.put(bin, p);
                            }
                        }
                    }
                }
            } catch (Exception e) {
                // no jrt: empty JDK index
            }
            if (allPackages) JRT_ALL = root; else JRT = root;
            return root;
        }
    }

    static BinaryRoot root(String path) {
        return ROOTS.computeIfAbsent(path, p -> {
            BinaryRoot root = new BinaryRoot(p);
            File f = new File(p);
            try {
                if (f.isFile()) {
                    root.jar = new JarFile(f);
                    root.jar.stream().forEach(e -> addEntry(root, e.getName()));
                } else if (f.isDirectory()) {
                    root.dir = f;
                    Path base = f.toPath();
                    try (Stream<Path> files = Files.walk(base)) {
                        files.forEach(x -> addEntry(root, base.relativize(x).toString().replace(File.separatorChar, '/')));
                    }
                }
            } catch (IOException e) {
                // unreadable root: empty
            }
            return root;
        });
    }

    private static void addEntry(BinaryRoot root, String name) {
        if (!name.endsWith(".class") || name.endsWith("module-info.class") || name.startsWith("META-INF/")) return;
        String bin = name.substring(0, name.length() - 6);
        root.binaryNames.add(bin);
        int slash = bin.lastIndexOf('/');
        root.packages.add(slash < 0 ? "" : bin.substring(0, slash).replace('/', '.'));
        // parent packages exist too
    }

    // ── Source units ─────────────────────────────────────────────────────────

    private static final class SourceEntry {
        final int hash;
        final String packageName;
        final List<TypeInfo> types;
        SourceEntry(int hash, String pkg, List<TypeInfo> types) { this.hash = hash; this.packageName = pkg; this.types = types; }
    }

    private static final Map<String, SourceEntry> SOURCES = new ConcurrentHashMap<>();

    private static SourceEntry source(String uri, String content, boolean isTest, String sourceLevel) {
        int hash = content.hashCode() * 31 + (isTest ? 1 : 0);
        SourceEntry cached = SOURCES.get(uri);
        if (cached != null && cached.hash == hash) return cached;
        List<TypeInfo> types = new ArrayList<>();
        String pkg = "";
        try {
            CompilerOptions opts = BridgeOptions.compilerOptions(sourceLevel);
            ProblemReporter reporter = new ProblemReporter(DefaultErrorHandlingPolicies.proceedWithAllProblems(), opts,
                    new DefaultProblemFactory());
            Parser parser = new Parser(reporter, false);
            InMemoryCompilationUnit unit = new InMemoryCompilationUnit(uri, content);
            CompilationUnitDeclaration cud = parser.dietParse(unit, new CompilationResult(unit, 0, 0, opts.maxProblemsPerUnit));
            if (cud.currentPackage != null) {
                pkg = CharOperation.toString(cud.currentPackage.tokens);
            }
            if (cud.types != null) {
                for (TypeDeclaration t : cud.types) {
                    addSourceType(types, pkg, new String[0], t, uri, isTest);
                }
            }
        } catch (Exception e) {
            // unparsable: no types
        }
        SourceEntry entry = new SourceEntry(hash, pkg, types);
        SOURCES.put(uri, entry);
        return entry;
    }

    private static void addSourceType(List<TypeInfo> out, String pkg, String[] enclosing, TypeDeclaration t, String uri,
            boolean isTest) {
        if (t.name == null || t.name.length == 0) return;
        int mods = t.modifiers & (ExtraCompilerModifiers.AccJustFlag | ClassFileConstants.AccDeprecated
                | ExtraCompilerModifiers.AccRecord);
        switch (TypeDeclaration.kind(t.modifiers)) {
            case TypeDeclaration.INTERFACE_DECL -> mods |= ClassFileConstants.AccInterface;
            case TypeDeclaration.ANNOTATION_TYPE_DECL -> mods |= ClassFileConstants.AccInterface | ClassFileConstants.AccAnnotation;
            case TypeDeclaration.ENUM_DECL -> mods |= ClassFileConstants.AccEnum;
            case TypeDeclaration.RECORD_DECL -> mods |= ExtraCompilerModifiers.AccRecord;
            default -> { }
        }
        if (t.javadoc != null && isDeprecatedJavadoc(t)) mods |= ClassFileConstants.AccDeprecated;
        List<CtorInfo> ctors = new ArrayList<>();
        if (t.methods != null) {
            for (AbstractMethodDeclaration m : t.methods) {
                if (!(m instanceof ConstructorDeclaration) || m.isDefaultConstructor()) continue;
                Argument[] args = m.arguments;
                int n = args == null ? 0 : args.length;
                char[][] types = new char[n][];
                char[][] names = new char[n][];
                for (int i = 0; i < n; i++) {
                    types[i] = CharOperation.concatWith(args[i].type.getParameterizedTypeName(), '.');
                    names[i] = args[i].name;
                }
                ctors.add(new CtorInfo(m.modifiers & ExtraCompilerModifiers.AccJustFlag, null, types, names));
            }
        }
        out.add(new TypeInfo(pkg, new String(t.name), enclosing, mods, uri, true, isTest, ctors));
        if (t.memberTypes != null) {
            String[] inner = new String[enclosing.length + 1];
            System.arraycopy(enclosing, 0, inner, 0, enclosing.length);
            inner[enclosing.length] = new String(t.name);
            for (TypeDeclaration m : t.memberTypes) {
                addSourceType(out, pkg, inner, m, uri, isTest);
            }
        }
    }

    private static boolean isDeprecatedJavadoc(TypeDeclaration t) {
        return (t.modifiers & ClassFileConstants.AccDeprecated) != 0;
    }

    // ── Per-request view ─────────────────────────────────────────────────────

    final List<BinaryRoot> binaryRoots = new ArrayList<>();
    final List<TypeInfo> sourceTypes = new ArrayList<>();
    final Set<String> sourcePackages = new HashSet<>();
    /** FQN (dotted, member types with '.') → source URI of the defining file. */
    final Map<String, String> sourceTypeUris = new HashMap<>();

    TypeIndex(Map<String, String> files, Set<String> testUris, List<String> classpath, String sourceLevel, String skipUri,
            boolean excludeTestCode) {
        for (String cp : classpath) {
            binaryRoots.add(root(cp));
        }
        if (BridgeOptions.includeRunningVM()) binaryRoots.add(jrt(BridgeOptions.version(sourceLevel).startsWith("1.")));
        for (Map.Entry<String, String> f : files.entrySet()) {
            String uri = f.getKey();
            boolean isTest = testUris.contains(uri);
            if (excludeTestCode && isTest) continue;
            SourceEntry e = source(uri, f.getValue(), isTest, sourceLevel);
            addPackageAndParents(sourcePackages, e.packageName);
            for (TypeInfo t : e.types) {
                if (t.enclosingNames.length == 0) sourceTypeUris.putIfAbsent(t.qualifiedName(), uri);
                if (!uri.equals(skipUri)) sourceTypes.add(t);
            }
        }
    }

    private static void addPackageAndParents(Set<String> set, String pkg) {
        if (pkg.isEmpty()) return;
        String p = pkg;
        while (true) {
            set.add(p);
            int dot = p.lastIndexOf('.');
            if (dot < 0) break;
            p = p.substring(0, dot);
        }
    }

    boolean isPackage(String dotted) {
        if (sourcePackages.contains(dotted)) return true;
        for (BinaryRoot r : binaryRoots) {
            if (r.packages.contains(dotted)) return true;
            String prefix = dotted + ".";
            for (String p : r.packages) {
                if (p.startsWith(prefix)) return true;
            }
        }
        return false;
    }

    /** All packages (including parents of packages that contain types). */
    Set<String> allPackages() {
        Set<String> all = new HashSet<>(sourcePackages);
        for (BinaryRoot r : binaryRoots) {
            for (String p : r.packages) addPackageAndParents(all, p);
        }
        return all;
    }

    interface TypeVisitor {
        void accept(TypeInfo info);
    }

    /**
     * Visit every type whose simple name matches {@code simplePattern} under
     * {@code matchRule} (SearchPattern rules), optionally restricted to an exact
     * qualification (package or package+enclosing types).
     */
    void searchTypes(char[] qualification, char[] simplePattern, int matchRule, boolean findMembers, TypeVisitor visitor) {
        String qual = qualification == null ? null : new String(qualification);
        for (TypeInfo t : sourceTypes) {
            if (!findMembers && t.enclosingNames.length > 0) continue;
            if (qual != null && !qual.equals(qualificationOf(t))) continue;
            if (Matching.matches(simplePattern, t.simpleName.toCharArray(), matchRule)) visitor.accept(t);
        }
        for (BinaryRoot r : binaryRoots) {
            for (String bin : r.binaryNames) {
                int slash = bin.lastIndexOf('/');
                String local = bin.substring(slash + 1);
                int dollar = local.lastIndexOf('$');
                if (dollar >= 0) {
                    if (!findMembers) continue;
                    String last = local.substring(dollar + 1);
                    if (last.isEmpty() || Character.isDigit(last.charAt(0))) continue; // anonymous / local
                }
                String candidate = dollar >= 0 ? local.substring(dollar + 1) : local;
                if (!Matching.matches(simplePattern, candidate.toCharArray(), matchRule)) continue;
                if (qual != null) {
                    String pkg = slash < 0 ? "" : bin.substring(0, slash).replace('/', '.');
                    if (!qual.equals(pkg) && !qual.startsWith(pkg)) continue;
                }
                TypeInfo info = r.info(bin);
                if (info == null) continue;
                if (!findMembers && info.enclosingNames.length > 0) continue;
                if (qual != null && !qual.equals(qualificationOf(info))) continue;
                visitor.accept(info);
            }
        }
    }

    static String qualificationOf(TypeInfo t) {
        StringBuilder sb = new StringBuilder(t.packageName);
        for (String e : t.enclosingNames) {
            if (sb.length() > 0) sb.append('.');
            sb.append(e);
        }
        return sb.toString();
    }

    /** SearchPattern-compatible name matching (subset used by code assist). */
    static final class Matching {
        static final int R_EXACT_MATCH = 0;
        static final int R_PREFIX_MATCH = 0x0001;
        static final int R_PATTERN_MATCH = 0x0002;
        static final int R_CASE_SENSITIVE = 0x0008;
        static final int R_CAMELCASE_MATCH = 0x0080;
        static final int R_CAMELCASE_SAME_PART_COUNT_MATCH = 0x0100;
        static final int R_SUBSTRING_MATCH = 0x0200;
        static final int R_SUBWORD_MATCH = 0x0400;

        static boolean matches(char[] pattern, char[] name, int rule) {
            if (pattern == null || pattern.length == 0) return true;
            if (name == null) return false;
            boolean caseSensitive = (rule & R_CASE_SENSITIVE) != 0;
            if ((rule & R_CAMELCASE_MATCH) != 0) {
                if (CharOperation.camelCaseMatch(pattern, name, false)) return true;
                if (CharOperation.prefixEquals(pattern, name, false)) return true;
            }
            if ((rule & R_CAMELCASE_SAME_PART_COUNT_MATCH) != 0) {
                if (CharOperation.camelCaseMatch(pattern, name, true)) return true;
            }
            if ((rule & R_SUBWORD_MATCH) != 0) {
                if (CharOperation.subWordMatch(pattern, name)) return true;
            }
            if ((rule & R_SUBSTRING_MATCH) != 0) {
                if (CharOperation.substringMatch(pattern, name)) return true;
            }
            if ((rule & R_PATTERN_MATCH) != 0) {
                return CharOperation.match(pattern, name, caseSensitive);
            }
            if ((rule & R_PREFIX_MATCH) != 0) {
                return CharOperation.prefixEquals(pattern, name, caseSensitive);
            }
            if ((rule & (R_CAMELCASE_MATCH | R_CAMELCASE_SAME_PART_COUNT_MATCH | R_SUBWORD_MATCH | R_SUBSTRING_MATCH)) != 0) {
                return false;
            }
            return CharOperation.equals(pattern, name, caseSensitive);
        }
    }
}
