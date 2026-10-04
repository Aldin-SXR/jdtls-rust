package com.jdtls.ecjbridge;

import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import org.eclipse.jdt.internal.compiler.env.INameEnvironment;
import org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer;

/**
 * Name environment for binding resolution over in-memory sources: indexes
 * every source file by its declared package and top-level type names (so
 * source packages are packages, and types are found even when the file path
 * does not mirror the package), then falls back to {@link InMemoryNameEnvironment}
 * for the classpath and JDK.
 */
final class SourceIndexNameEnvironment implements INameEnvironment {
    private static final Pattern PACKAGE = Pattern.compile("(?m)^\\s*package\\s+([\\w.\\s]+?)\\s*;");
    private static final Pattern TOP_TYPE = Pattern.compile(
            "(?m)^(?:\\s*(?:@[\\w.]+(?:\\([^)]*\\))?\\s*)*(?:public|abstract|final|sealed|non-sealed|strictfp|static|\\s)*)\\b(?:class|interface|enum|record|@interface)\\s+(\\w+)");

    private final Map<String, String> sourceFiles;
    private final InMemoryNameEnvironment delegate;
    /** binary name (a/b/C) → uri */
    private final Map<String, String> typeIndex = new HashMap<>();
    /** uri → package (dotted, "" for default) */
    private final Map<String, String> packageOfUri = new HashMap<>();
    private final Set<String> sourcePackages = new HashSet<>();

    SourceIndexNameEnvironment(Map<String, String> sourceFiles, List<String> classpath) {
        this.sourceFiles = sourceFiles;
        this.delegate = new InMemoryNameEnvironment(sourceFiles, classpath);
        for (Map.Entry<String, String> e : sourceFiles.entrySet()) {
            String uri = e.getKey();
            if (!uri.endsWith(".java")) {
                continue;
            }
            String src = e.getValue();
            String pkg = packageOf(src);
            packageOfUri.put(uri, pkg);
            String prefix = pkg.isEmpty() ? "" : pkg.replace('.', '/') + "/";
            if (!pkg.isEmpty()) {
                String[] parts = pkg.split("\\.");
                StringBuilder sb = new StringBuilder();
                for (String p : parts) {
                    if (sb.length() > 0) sb.append('/');
                    sb.append(p);
                    sourcePackages.add(sb.toString());
                }
            }
            String file = uri.substring(uri.lastIndexOf('/') + 1, uri.length() - 5);
            typeIndex.putIfAbsent(prefix + file, uri);
            Matcher m = TOP_TYPE.matcher(stripComments(src));
            while (m.find()) {
                typeIndex.putIfAbsent(prefix + m.group(1), uri);
            }
        }
    }

    static String packageOf(String src) {
        Matcher m = PACKAGE.matcher(stripComments(src));
        if (m.find()) {
            return m.group(1).replaceAll("\\s+", "");
        }
        return "";
    }

    private static String stripComments(String src) {
        StringBuilder sb = new StringBuilder(src.length());
        int n = src.length();
        int i = 0;
        while (i < n) {
            char c = src.charAt(i);
            if (c == '/' && i + 1 < n && src.charAt(i + 1) == '*') {
                int end = src.indexOf("*/", i + 2);
                end = end < 0 ? n : end + 2;
                for (int k = i; k < end; k++) sb.append(src.charAt(k) == '\n' ? '\n' : ' ');
                i = end;
            } else if (c == '/' && i + 1 < n && src.charAt(i + 1) == '/') {
                while (i < n && src.charAt(i) != '\n') { sb.append(' '); i++; }
            } else if (c == '"') {
                sb.append(c);
                i++;
                while (i < n && src.charAt(i) != '"' && src.charAt(i) != '\n') {
                    if (src.charAt(i) == '\\') { sb.append(' '); i++; }
                    if (i < n) { sb.append(' '); i++; }
                }
            } else {
                sb.append(c);
                i++;
            }
        }
        return sb.toString();
    }

    /** The uri of the source file declaring top-level type {@code binaryName} (a/b/C), or null. */
    String sourceUriOf(String binaryName) {
        return typeIndex.get(binaryName);
    }

    String packageOfUri(String uri) {
        return packageOfUri.getOrDefault(uri, "");
    }

    Map<String, String> sources() {
        return sourceFiles;
    }

    private NameEnvironmentAnswer find(String binaryName) {
        String uri = typeIndex.get(binaryName);
        if (uri != null) {
            return new NameEnvironmentAnswer(new InMemoryCompilationUnit(uri, sourceFiles.get(uri)), null);
        }
        return null;
    }

    @Override
    public NameEnvironmentAnswer findType(char[][] compoundTypeName) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < compoundTypeName.length; i++) {
            if (i > 0) sb.append('/');
            sb.append(compoundTypeName[i]);
        }
        NameEnvironmentAnswer a = find(sb.toString());
        return a != null ? a : delegate.findType(compoundTypeName);
    }

    @Override
    public NameEnvironmentAnswer findType(char[] typeName, char[][] packageName) {
        StringBuilder sb = new StringBuilder();
        for (char[] p : packageName) {
            sb.append(p).append('/');
        }
        sb.append(typeName);
        NameEnvironmentAnswer a = find(sb.toString());
        return a != null ? a : delegate.findType(typeName, packageName);
    }

    @Override
    public boolean isPackage(char[][] parentPackageName, char[] packageName) {
        StringBuilder sb = new StringBuilder();
        if (parentPackageName != null) {
            for (char[] p : parentPackageName) {
                sb.append(p).append('/');
            }
        }
        sb.append(packageName);
        String pkg = sb.toString();
        if (sourcePackages.contains(pkg)) {
            return true;
        }
        if (typeIndex.containsKey(pkg)) {
            return false;
        }
        return delegate.isPackage(parentPackageName, packageName);
    }

    @Override
    public void cleanup() {
        delegate.cleanup();
    }
}
