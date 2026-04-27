package com.jdtls.ecjbridge;

import org.eclipse.jdt.internal.compiler.env.*;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileReader;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFormatException;

import java.io.*;
import java.net.URI;
import java.net.URISyntaxException;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.logging.Logger;

/**
 * INameEnvironment implementation that:
 * 1. Serves source units from an in-memory map of URI → source code.
 * 2. Serves compiled class files from previously compiled units (also in memory).
 * 3. Falls back to JDK boot classpath and user-supplied JARs for library classes.
 *
 * No files on disk are required for source resolution.
 */
public class InMemoryNameEnvironment implements INameEnvironment {

    private static final Logger LOG = Logger.getLogger(InMemoryNameEnvironment.class.getName());

    /**
     * Shared classpath cache: fingerprint → pre-built, immutable list of ClasspathEntry.
     * Entries are never closed — they are reused across all requests with the same classpath.
     * Keyed by the sorted, joined classpath string so order differences don't create duplicates.
     */
    private static final ConcurrentHashMap<String, List<ClasspathEntry>> CLASSPATH_CACHE =
        new ConcurrentHashMap<>();

    /** URI string → Java source code for all open files */
    private final Map<String, String> sourceFiles;

    /** Binary class name (e.g. "com/example/Foo") → bytecode, built as we compile */
    private final Map<String, byte[]> compiledClasses = new ConcurrentHashMap<>();

    /** Classpath entries sourced from the shared cache — never closed by this instance. */
    private final List<ClasspathEntry> classpathEntries;

    public InMemoryNameEnvironment(Map<String, String> sourceFiles, List<String> classpath) {
        this.sourceFiles = sourceFiles;
        this.classpathEntries = cachedClasspath(classpath);
    }

    /**
     * Return the cached ClasspathEntry list for the given classpath, building it on first use.
     * Building is expensive (opens JarFiles, enumerates jrt:/ modules) so we do it exactly once
     * per unique classpath regardless of how many concurrent requests share it.
     */
    private static List<ClasspathEntry> cachedClasspath(List<String> classpath) {
        // Normalise order so ["a","b"] and ["b","a"] share the same cache entry.
        String key = classpath.stream().sorted().collect(java.util.stream.Collectors.joining("|"));
        return CLASSPATH_CACHE.computeIfAbsent(key, k -> buildClasspathEntries(classpath));
    }

    private static List<ClasspathEntry> buildClasspathEntries(List<String> classpath) {
        List<ClasspathEntry> entries = new ArrayList<>();
        for (String cp : classpath) {
            File f = new File(cp);
            if (!f.exists()) {
                LOG.warning("Classpath entry not found: " + cp);
                continue;
            }
            if (cp.endsWith(".jar") || cp.endsWith(".zip")) {
                entries.add(new JarClasspathEntry(f));
            } else if (f.isDirectory()) {
                entries.add(new DirClasspathEntry(f));
            }
        }
        // Boot classpath (JDK)
        String javaHome = System.getProperty("java.home");
        if (javaHome != null) {
            File rtJar = new File(javaHome, "lib/rt.jar");
            if (rtJar.exists()) {
                entries.add(new JarClasspathEntry(rtJar));
            } else {
                try {
                    java.nio.file.FileSystem jrtFs =
                        java.nio.file.FileSystems.getFileSystem(java.net.URI.create("jrt:/"));
                    entries.add(new JrtClasspathEntry(jrtFs));
                    LOG.info("Using jrt:/ filesystem for JDK boot classpath");
                } catch (Exception e) {
                    LOG.warning("Cannot open jrt:/ filesystem: " + e.getMessage());
                }
            }
        }
        return Collections.unmodifiableList(entries);
    }

    /** Add compiled bytecode produced by a previous compilation pass. */
    public void addCompiledClass(String binaryName, byte[] bytecode) {
        compiledClasses.put(binaryName, bytecode);
    }

    // ── INameEnvironment ─────────────────────────────────────────────────────

    @Override
    public NameEnvironmentAnswer findType(char[][] compoundTypeName) {
        String binaryName = toBinaryName(compoundTypeName);
        return findByBinaryName(binaryName);
    }

    @Override
    public NameEnvironmentAnswer findType(char[] typeName, char[][] packageName) {
        String binaryName = toBinaryName(packageName) + "/" + new String(typeName);
        return findByBinaryName(binaryName);
    }

    @Override
    public boolean isPackage(char[][] parentPackageName, char[] packageName) {
        String pkg = (parentPackageName != null && parentPackageName.length > 0
                ? toBinaryName(parentPackageName) + "/" : "")
                + new String(packageName);
        // Check compiled classes
        for (String name : compiledClasses.keySet()) {
            if (name.startsWith(pkg + "/")) return true;
        }
        // Check classpath JARs/directories
        for (ClasspathEntry entry : classpathEntries) {
            if (entry.isPackage(pkg)) return true;
        }
        return false;
    }

    @Override
    public void cleanup() {
        // classpathEntries are owned by the shared cache — do not close them.
        // Only discard the per-request compiled bytecode.
        compiledClasses.clear();
    }

    // ── Resolution logic ─────────────────────────────────────────────────────

    private NameEnvironmentAnswer findByBinaryName(String binaryName) {
        // 1. Previously compiled unit (in-memory bytecode)
        byte[] bytecode = compiledClasses.get(binaryName);
        if (bytecode != null) {
            try {
                ClassFileReader reader = new ClassFileReader(bytecode, binaryName.toCharArray());
                return new NameEnvironmentAnswer(reader, null);
            } catch (ClassFormatException e) {
                LOG.warning("Bad class format for " + binaryName + ": " + e.getMessage());
            }
        }

        // 2. Source file (convert binary name to source URI and look up)
        ICompilationUnit srcUnit = findSourceUnit(binaryName);
        if (srcUnit != null) {
            return new NameEnvironmentAnswer(srcUnit, null);
        }

        // 3. Classpath entries (JDK + user JARs)
        for (ClasspathEntry entry : classpathEntries) {
            NameEnvironmentAnswer answer = entry.findClass(binaryName);
            if (answer != null) return answer;
        }

        return null;
    }

    private ICompilationUnit findSourceUnit(String binaryName) {
        // Convert "com/example/Foo" → look for any URI ending in "com/example/Foo.java"
        // or whose content declares this class.
        String suffix = binaryName.replace('/', File.separatorChar) + ".java";
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String uriStr = entry.getKey();
            if (uriStr.replace('\\', '/').endsWith(binaryName + ".java")) {
                return new InMemoryCompilationUnit(uriStr, entry.getValue());
            }
        }
        return null;
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    private static String toBinaryName(char[][] compoundName) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < compoundName.length; i++) {
            if (i > 0) sb.append('/');
            sb.append(compoundName[i]);
        }
        return sb.toString();
    }

    // ── Classpath entries ────────────────────────────────────────────────────

    interface ClasspathEntry {
        NameEnvironmentAnswer findClass(String binaryName);
        boolean isPackage(String packagePath);
        void close();
    }

    static class JarClasspathEntry implements ClasspathEntry {
        private final File jarFile;
        private JarFile jar;
        /** Cached package existence: packagePath → true/false. Avoids O(n) JAR scans. */
        private final ConcurrentHashMap<String, Boolean> packageCache = new ConcurrentHashMap<>();

        JarClasspathEntry(File f) {
            this.jarFile = f;
            try { this.jar = new JarFile(f); } catch (IOException e) {
                LOG.warning("Cannot open JAR: " + f);
            }
        }

        @Override
        public NameEnvironmentAnswer findClass(String binaryName) {
            if (jar == null) return null;
            JarEntry entry = jar.getJarEntry(binaryName + ".class");
            if (entry == null) return null;
            try (InputStream is = jar.getInputStream(entry)) {
                byte[] bytes = is.readAllBytes();
                ClassFileReader reader = new ClassFileReader(bytes, binaryName.toCharArray());
                return new NameEnvironmentAnswer(reader, null);
            } catch (IOException | ClassFormatException e) {
                return null;
            }
        }

        @Override
        public boolean isPackage(String packagePath) {
            if (jar == null) return false;
            return packageCache.computeIfAbsent(packagePath, pkg -> {
                String prefix = pkg + "/";
                return jar.stream().anyMatch(e -> {
                    String name = e.getName();
                    return name.startsWith(prefix) && name.endsWith(".class");
                });
            });
        }

        @Override
        public void close() {
            // Owned by the classpath cache — not closed per-request.
        }
    }

    static class DirClasspathEntry implements ClasspathEntry {
        private final File dir;

        DirClasspathEntry(File dir) { this.dir = dir; }

        @Override
        public NameEnvironmentAnswer findClass(String binaryName) {
            File classFile = new File(dir, binaryName.replace('/', File.separatorChar) + ".class");
            if (!classFile.exists()) return null;
            try {
                byte[] bytes = Files.readAllBytes(classFile.toPath());
                ClassFileReader reader = new ClassFileReader(bytes, binaryName.toCharArray());
                return new NameEnvironmentAnswer(reader, null);
            } catch (IOException | ClassFormatException e) {
                return null;
            }
        }

        @Override
        public boolean isPackage(String packagePath) {
            File pkgDir = new File(dir, packagePath.replace('/', File.separatorChar));
            return pkgDir.isDirectory();
        }

        @Override
        public void close() {}
    }

    /**
     * Classpath entry backed by the Java 9+ jrt:/ virtual filesystem.
     * Searches all modules under /modules/<module>/<binaryName>.class.
     */
    static class JrtClasspathEntry implements ClasspathEntry {
        private final java.nio.file.FileSystem jrtFs;
        /** Shared across all instances — enumerated once per JVM lifetime. */
        private static volatile List<java.nio.file.Path> sharedModuleList;
        private static final Map<String, Boolean> sharedPackageCache = new ConcurrentHashMap<>();

        JrtClasspathEntry(java.nio.file.FileSystem fs) throws IOException {
            this.jrtFs = fs;
            if (sharedModuleList == null) {
                synchronized (JrtClasspathEntry.class) {
                    if (sharedModuleList == null) {
                        java.nio.file.Path modulesRoot = fs.getPath("/modules");
                        try (var stream = Files.list(modulesRoot)) {
                            sharedModuleList = stream.toList();
                        }
                    }
                }
            }
        }

        @Override
        public NameEnvironmentAnswer findClass(String binaryName) {
            String classFile = binaryName + ".class";
            for (java.nio.file.Path module : sharedModuleList) {
                java.nio.file.Path p = module.resolve(classFile);
                if (Files.exists(p)) {
                    try {
                        byte[] bytes = Files.readAllBytes(p);
                        ClassFileReader reader = new ClassFileReader(bytes, binaryName.toCharArray());
                        return new NameEnvironmentAnswer(reader, null);
                    } catch (IOException | ClassFormatException e) {
                        return null;
                    }
                }
            }
            return null;
        }

        @Override
        public boolean isPackage(String packagePath) {
            return sharedPackageCache.computeIfAbsent(packagePath, pkg -> {
                for (java.nio.file.Path module : sharedModuleList) {
                    if (Files.isDirectory(module.resolve(pkg))) return true;
                }
                return false;
            });
        }

        @Override
        public void close() {}
    }
}
