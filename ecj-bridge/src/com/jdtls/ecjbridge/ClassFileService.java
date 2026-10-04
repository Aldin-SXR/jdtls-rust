package com.jdtls.ecjbridge;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.jar.Manifest;
import java.util.logging.Logger;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import org.eclipse.jdt.core.util.IClassFileReader;
import org.eclipse.jdt.core.util.ISourceAttribute;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileReader;
import org.eclipse.jdt.internal.compiler.env.IBinaryNestedType;
import org.jetbrains.java.decompiler.main.decompiler.BaseDecompiler;
import org.jetbrains.java.decompiler.main.extern.IFernflowerLogger;
import org.jetbrains.java.decompiler.main.extern.IFernflowerPreferences;
import org.jetbrains.java.decompiler.main.extern.IResultSaver;

/**
 * Class files of libraries and of the JDK (jrt filesystem of the running
 * JVM): locating the binary for a type, its {@code SourceFile} attribute,
 * attached source and the decompiled fallback.  Mirrors jdt.ls
 * {@code SourceContentProvider} (attached source) followed by
 * {@code FernFlowerDecompiler}; all data only, the {@code jdt://} URI is
 * built by the Rust server.
 */
final class ClassFileService {

    private static final Logger LOG = Logger.getLogger(ClassFileService.class.getName());

    /** jdt.ls {@code FernFlowerDecompiler.DECOMPILER_HEADER}. */
    static final String DECOMPILER_HEADER = "// Source code is decompiled from a .class file using FernFlower decompiler (from Intellij IDEA).\n";

    /** Identifies one class file. */
    public static class ClassFileDesc {
        /** Absolute jar/directory path, or the running JDK's {@code lib/jrt-fs.jar} for jrt classes. */
        public String root;
        /** Module name for jrt classes, else null. */
        public String module;
        /** Dotted package name ("" for the default package). */
        public String packageName;
        /** e.g. {@code Map$Entry.class}. */
        public String classFileName;
        /** {@code SourceFile} attribute (output only). */
        public String sourceFileName;

        String binaryPath() {
            String pkg = packageName == null || packageName.isEmpty() ? "" : packageName.replace('.', '/') + "/";
            return pkg + classFileName;
        }

        String key() {
            return root + "|" + module + "|" + binaryPath();
        }

        boolean isJrt() {
            return module != null && !module.isEmpty();
        }
    }

    private ClassFileService() {}

    // ── JDK ───────────────────────────────────────────────────────────────

    private static volatile FileSystem jrt;

    static FileSystem jrt() {
        if (jrt == null) {
            synchronized (ClassFileService.class) {
                if (jrt == null) {
                    try {
                        jrt = FileSystems.getFileSystem(URI.create("jrt:/"));
                    } catch (Exception e) {
                        LOG.warning("No jrt filesystem: " + e);
                    }
                }
            }
        }
        return jrt;
    }

    static String jrtRoot() {
        return Path.of(System.getProperty("java.home"), "lib", "jrt-fs.jar").toString();
    }

    private static final Map<String, List<String>> PACKAGE_MODULES = new ConcurrentHashMap<>();

    private static List<String> modulesOf(String packageName) {
        return PACKAGE_MODULES.computeIfAbsent(packageName, pkg -> {
            FileSystem fs = jrt();
            if (fs == null) {
                return List.of();
            }
            Path dir = fs.getPath("/packages", pkg);
            if (!Files.isDirectory(dir)) {
                return List.of();
            }
            List<String> out = new ArrayList<>();
            try (Stream<Path> s = Files.list(dir)) {
                s.forEach(p -> out.add(p.getFileName().toString()));
            } catch (IOException e) {
                // ignore
            }
            Collections.sort(out);
            return out;
        });
    }

    // ── Jars ──────────────────────────────────────────────────────────────

    private static final Map<String, Set<String>> JAR_ENTRIES = new ConcurrentHashMap<>();

    static Set<String> jarEntries(String jar) {
        File f = new File(jar);
        String key = jar + "@" + f.lastModified();
        return JAR_ENTRIES.computeIfAbsent(key, k -> {
            Set<String> out = new LinkedHashSet<>();
            try (ZipFile z = new ZipFile(f)) {
                z.stream().forEach(e -> out.add(e.getName()));
            } catch (IOException e) {
                // not a jar
            }
            return out;
        });
    }

    static boolean isArchive(String path) {
        String p = path.toLowerCase();
        return p.endsWith(".jar") || p.endsWith(".zip") || (new File(path).isFile());
    }

    // ── Locating ──────────────────────────────────────────────────────────

    /**
     * Locate the class file of the binary type {@code binaryName}
     * ({@code java.util.Map$Entry}) on {@code classpath}, then in the JDK.
     */
    static ClassFileDesc locate(List<String> classpath, String binaryName) {
        if (binaryName == null || binaryName.isEmpty()) {
            return null;
        }
        String internal = binaryName.replace('.', '/');
        int slash = internal.lastIndexOf('/');
        String pkg = slash < 0 ? "" : internal.substring(0, slash).replace('/', '.');
        String file = (slash < 0 ? internal : internal.substring(slash + 1)) + ".class";
        String entry = internal + ".class";
        if (classpath != null) {
            for (String cp : classpath) {
                File f = new File(cp);
                boolean found = f.isDirectory() ? new File(f, entry).isFile() : f.isFile() && jarEntries(cp).contains(entry);
                if (found) {
                    return desc(cp, null, pkg, file);
                }
            }
        }
        for (String module : modulesOf(pkg)) {
            FileSystem fs = jrt();
            if (fs != null && Files.isRegularFile(fs.getPath("/modules", module, entry))) {
                return desc(jrtRoot(), module, pkg, file);
            }
        }
        return null;
    }

    /**
     * {@code ClassFileUtil.getURI}-style lookup: package and simple name
     * match case-insensitively ({@code SearchPattern.R_EXACT_MATCH}).
     */
    static ClassFileDesc locateIgnoreCase(List<String> classpath, String fqn) {
        ClassFileDesc exact = locate(classpath, fqn);
        if (exact != null) {
            return exact;
        }
        String wanted = fqn.replace('.', '/').toLowerCase() + ".class";
        if (classpath != null) {
            for (String cp : classpath) {
                File f = new File(cp);
                if (f.isFile()) {
                    for (String e : jarEntries(cp)) {
                        if (e.toLowerCase().equals(wanted)) {
                            return locate(List.of(cp), e.substring(0, e.length() - 6));
                        }
                    }
                }
            }
        }
        int dot = fqn.lastIndexOf('.');
        String pkg = dot < 0 ? "" : fqn.substring(0, dot);
        FileSystem fs = jrt();
        if (fs != null) {
            try (Stream<Path> s = Files.list(fs.getPath("/packages"))) {
                for (Path p : (Iterable<Path>) s::iterator) {
                    String name = p.getFileName().toString();
                    if (name.equalsIgnoreCase(pkg)) {
                        for (String module : modulesOf(name)) {
                            Path dir = fs.getPath("/modules", module, name.replace('.', '/'));
                            try (Stream<Path> files = Files.list(dir)) {
                                for (Path cf : (Iterable<Path>) files::iterator) {
                                    String fn = cf.getFileName().toString();
                                    if (fn.equalsIgnoreCase(fqn.substring(dot + 1) + ".class")) {
                                        return desc(jrtRoot(), module, name, fn);
                                    }
                                }
                            }
                        }
                    }
                }
            } catch (IOException e) {
                // ignore
            }
        }
        return null;
    }

    private static ClassFileDesc desc(String root, String module, String pkg, String file) {
        ClassFileDesc d = new ClassFileDesc();
        d.root = root;
        d.module = module;
        d.packageName = pkg;
        d.classFileName = file;
        d.sourceFileName = sourceFileName(bytes(d));
        return d;
    }

    /** Fill {@code sourceFileName} for a descriptor coming from Rust. */
    static ClassFileDesc complete(ClassFileDesc d) {
        if (d != null && d.sourceFileName == null) {
            d.sourceFileName = sourceFileName(bytes(d));
        }
        return d;
    }

    // ── Bytes ─────────────────────────────────────────────────────────────

    static byte[] bytes(ClassFileDesc d) {
        return bytes(d, d.binaryPath());
    }

    static byte[] bytes(ClassFileDesc d, String entry) {
        try {
            if (d.isJrt()) {
                FileSystem fs = jrt();
                Path p = fs == null ? null : fs.getPath("/modules", d.module, entry);
                return p != null && Files.isRegularFile(p) ? Files.readAllBytes(p) : null;
            }
            File f = new File(d.root);
            if (f.isDirectory()) {
                File cf = new File(f, entry);
                return cf.isFile() ? Files.readAllBytes(cf.toPath()) : null;
            }
            if (!f.isFile()) {
                return null;
            }
            try (ZipFile z = new ZipFile(f)) {
                ZipEntry e = z.getEntry(entry);
                if (e == null) {
                    return null;
                }
                try (InputStream in = z.getInputStream(e)) {
                    return in.readAllBytes();
                }
            }
        } catch (IOException e) {
            return null;
        }
    }

    /** jdt.ls {@code SourceFileAttributeReader.getSourceFileName}. */
    static String sourceFileName(byte[] bytes) {
        if (bytes == null || bytes.length == 0) {
            return null;
        }
        try {
            IClassFileReader reader = new org.eclipse.jdt.internal.core.util.ClassFileReader(bytes, IClassFileReader.CLASSFILE_ATTRIBUTES);
            if (reader == null) {
                return null;
            }
            ISourceAttribute attr = reader.getSourceFileAttribute();
            if (attr == null || attr.getSourceFileName() == null || attr.getSourceFileName().length == 0) {
                return null;
            }
            return new String(attr.getSourceFileName());
        } catch (Exception e) {
            return null;
        }
    }

    /** Binary name of the top-level type of a class file ({@code Map$Entry.class} → {@code java.util.Map}). */
    static String topLevelBinaryName(ClassFileDesc d) {
        String simple = d.classFileName.substring(0, d.classFileName.length() - ".class".length());
        byte[] b = bytes(d);
        String top = simple;
        try {
            if (b != null) {
                ClassFileReader r = new ClassFileReader(b, null);
                char[] enclosing = r.getEnclosingTypeName();
                String current = new String(r.getName());
                while (enclosing != null) {
                    current = new String(enclosing);
                    ClassFileDesc outer = new ClassFileDesc();
                    outer.root = d.root;
                    outer.module = d.module;
                    outer.packageName = d.packageName;
                    outer.classFileName = current.substring(current.lastIndexOf('/') + 1) + ".class";
                    byte[] ob = bytes(outer);
                    if (ob == null) {
                        break;
                    }
                    enclosing = new ClassFileReader(ob, null).getEnclosingTypeName();
                }
                top = current.substring(current.lastIndexOf('/') + 1);
            }
        } catch (Exception e) {
            // keep simple name
        }
        return d.packageName == null || d.packageName.isEmpty() ? top : d.packageName + "." + top;
    }

    // ── Contents ──────────────────────────────────────────────────────────

    /** Content shown for a class file: attached source, else decompiled, else "". */
    static String contents(ClassFileDesc d, Map<String, String> attachments) {
        if (d == null || bytes(d) == null) {
            return "";
        }
        String src = attachedSource(d, attachments);
        if (src != null) {
            return src;
        }
        String dec = decompile(d);
        return dec == null ? "" : dec;
    }

    static boolean hasAttachedSource(ClassFileDesc d, Map<String, String> attachments) {
        return attachedSource(d, attachments) != null;
    }

    private static final Map<String, String> SOURCE_CACHE = Collections.synchronizedMap(new LinkedHashMap<>(64, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, String> eldest) {
            return size() > 100;
        }
    });

    /** Source of the class file from its source attachment (jdt.ls {@code SourceContentProvider}). */
    static String attachedSource(ClassFileDesc d, Map<String, String> attachments) {
        String archive;
        String prefix = "";
        if (d.isJrt()) {
            Path srcZip = Path.of(System.getProperty("java.home"), "lib", "src.zip");
            if (!Files.isRegularFile(srcZip)) {
                return null;
            }
            archive = srcZip.toString();
            prefix = d.module + "/";
        } else {
            archive = attachments == null ? null : attachments.get(d.root);
            if (archive == null) {
                return null;
            }
        }
        String top = topLevelBinaryName(d);
        String srcName = d.sourceFileName != null ? d.sourceFileName : sourceFileName(bytes(d));
        if (srcName == null) {
            srcName = top.substring(top.lastIndexOf('.') + 1) + ".java";
        }
        String pkgPath = d.packageName == null || d.packageName.isEmpty() ? "" : d.packageName.replace('.', '/') + "/";
        String entry = prefix + pkgPath + srcName;
        String key = archive + "!" + entry;
        String cached = SOURCE_CACHE.get(key);
        if (cached != null) {
            return cached;
        }
        String result = readSourceEntry(archive, entry, "/" + pkgPath + srcName);
        if (result != null) {
            SOURCE_CACHE.put(key, result);
        }
        return result;
    }

    private static String readSourceEntry(String archive, String entry, String suffix) {
        File f = new File(archive);
        try {
            if (f.isDirectory()) {
                File s = new File(f, entry);
                return s.isFile() ? Files.readString(s.toPath()) : null;
            }
            if (!f.isFile()) {
                return null;
            }
            try (ZipFile z = new ZipFile(f)) {
                ZipEntry e = z.getEntry(entry);
                if (e == null) {
                    // Source roots nested inside the archive (SourceMapper root detection).
                    e = z.stream().filter(x -> ("/" + x.getName()).endsWith(suffix)).findFirst().orElse(null);
                }
                if (e == null) {
                    return null;
                }
                try (InputStream in = z.getInputStream(e)) {
                    return new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8);
                }
            }
        } catch (IOException e) {
            return null;
        }
    }

    private static final Map<String, String> DECOMPILED = Collections.synchronizedMap(new LinkedHashMap<>(64, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, String> eldest) {
            return size() > 100;
        }
    });

    /**
     * jdt.ls {@code FernFlowerDecompiler}: decompile the top-level class of
     * {@code d} together with its (recursively) declared member types.
     */
    static String decompile(ClassFileDesc d) {
        String top = topLevelBinaryName(d);
        ClassFileDesc topDesc = new ClassFileDesc();
        topDesc.root = d.root;
        topDesc.module = d.module;
        topDesc.packageName = d.packageName;
        topDesc.classFileName = top.substring(top.lastIndexOf('.') + 1) + ".class";
        String key = topDesc.key();
        String cached = DECOMPILED.get(key);
        if (cached != null) {
            return cached;
        }
        Map<String, byte[]> bytecode = new LinkedHashMap<>();
        List<File> files = new ArrayList<>();
        collectMembers(topDesc, bytecode, files, new LinkedHashSet<>());
        if (files.isEmpty()) {
            return null;
        }
        Map<String, Object> options = new HashMap<>();
        options.put(IFernflowerPreferences.HIDE_DEFAULT_CONSTRUCTOR, "0");
        options.put(IFernflowerPreferences.IGNORE_INVALID_BYTECODE, "1");
        options.put(IFernflowerPreferences.REMOVE_SYNTHETIC, "1");
        options.put(IFernflowerPreferences.REMOVE_BRIDGE, "1");
        options.put(IFernflowerPreferences.DECOMPILE_GENERIC_SIGNATURES, "1");
        options.put(IFernflowerPreferences.DECOMPILE_INNER, "1");
        options.put(IFernflowerPreferences.DECOMPILE_ENUM, "1");
        options.put(IFernflowerPreferences.LOG_LEVEL, IFernflowerLogger.Severity.ERROR.name());
        options.put(IFernflowerPreferences.ASCII_STRING_CHARACTERS, "0");
        options.put(IFernflowerPreferences.BYTECODE_SOURCE_MAPPING, "1");
        final String[] content = new String[1];
        IResultSaver saver = new IResultSaver() {
            @Override public void saveFolder(String path) {}
            @Override public void copyFile(String source, String path, String entryName) {}
            @Override public void saveClassFile(String path, String qualifiedName, String entryName, String c, int[] mapping) {
                content[0] = c;
            }
            @Override public void createArchive(String path, String archiveName, Manifest manifest) {}
            @Override public void saveDirEntry(String path, String archiveName, String entryName) {}
            @Override public void copyEntry(String source, String path, String archiveName, String entry) {}
            @Override public void saveClassEntry(String path, String archiveName, String qualifiedName, String entryName, String c) {}
            @Override public void closeArchive(String path, String archiveName) {}
        };
        try {
            BaseDecompiler fernflower = new BaseDecompiler((externalPath, internalPath) -> {
                byte[] b = bytecode.get(externalPath);
                if (b == null) {
                    throw new IOException("Class file not found: " + externalPath);
                }
                return b;
            }, saver, options, new IFernflowerLogger() {
                @Override public void writeMessage(String message, Severity severity) {}
                @Override public void writeMessage(String message, Severity severity, Throwable t) {}
            });
            for (File f : files) {
                fernflower.addSource(f);
            }
            fernflower.decompileContext();
        } catch (Throwable t) {
            LOG.warning("FernFlower failed for " + key + ": " + t);
            return null;
        }
        if (content[0] == null) {
            return null;
        }
        String result = DECOMPILER_HEADER + content[0];
        DECOMPILED.put(key, result);
        return result;
    }

    private static void collectMembers(ClassFileDesc d, Map<String, byte[]> bytecode, List<File> files, Set<String> seen) {
        if (!seen.add(d.classFileName)) {
            return;
        }
        byte[] b = bytes(d);
        if (b == null) {
            return;
        }
        File f = new File(d.classFileName).getAbsoluteFile();
        files.add(f);
        bytecode.put(f.getPath(), b);
        try {
            ClassFileReader r = new ClassFileReader(b, null);
            IBinaryNestedType[] members = r.getMemberTypes();
            if (members == null) {
                return;
            }
            for (IBinaryNestedType m : members) {
                String name = new String(m.getName());
                ClassFileDesc md = new ClassFileDesc();
                md.root = d.root;
                md.module = d.module;
                md.packageName = d.packageName;
                md.classFileName = name.substring(name.lastIndexOf('/') + 1) + ".class";
                collectMembers(md, bytecode, files, seen);
            }
        } catch (Exception e) {
            // ignore malformed
        }
    }

    /** Unit name used when parsing the class file's contents ("/java/util/Map.java"). */
    static String unitName(ClassFileDesc d) {
        String top = topLevelBinaryName(d);
        return "/" + top.replace('.', '/') + ".java";
    }
}
