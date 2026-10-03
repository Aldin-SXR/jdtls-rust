package com.jdtls.ecjbridge;

import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import org.eclipse.jdt.internal.compiler.batch.ClasspathLocation;
import org.eclipse.jdt.internal.compiler.batch.CompilationUnit;
import org.eclipse.jdt.internal.compiler.batch.FileSystem;
import org.eclipse.jdt.internal.compiler.env.IModule;
import org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer;

/**
 * A source classpath entry for DOM binding resolution that serves the
 * request's in-memory documents, so other compilation units of the project
 * (open or not, on disk or virtual) resolve from their current contents.
 *
 * Types are indexed by the package declared in each document plus every
 * top-level type it declares (primary and secondary types).
 */
final class InMemorySourceClasspath extends ClasspathLocation {

    private static final Pattern PACKAGE = Pattern.compile("^\\s*package\\s+([\\w.$\\s]+?)\\s*;", Pattern.MULTILINE);

    /** "pkg/sub/Type" → uri */
    private final Map<String, String> types = new HashMap<>();
    /** uri → contents */
    private final Map<String, String> sources;
    private final Set<String> packages = new HashSet<>();

    InMemorySourceClasspath(Map<String, String> sources, String excludeUri) {
        super(null, null);
        this.sources = sources;
        for (Map.Entry<String, String> e : sources.entrySet()) {
            if (e.getKey().equals(excludeUri)) {
                continue;
            }
            String text = e.getValue();
            String pkg = packageOf(text);
            String pkgPath = pkg.replace('.', '/');
            for (String type : topLevelTypes(text)) {
                types.putIfAbsent(pkgPath.isEmpty() ? type : pkgPath + "/" + type, e.getKey());
            }
            String p = pkgPath;
            while (!p.isEmpty()) {
                packages.add(p);
                int slash = p.lastIndexOf('/');
                p = slash < 0 ? "" : p.substring(0, slash);
            }
        }
    }

    /** uri of the document declaring the top-level type `qualifiedName` ("a.b.C"), or null. */
    String uriOf(String qualifiedName) {
        return types.get(qualifiedName.replace('.', '/'));
    }

    String source(String uri) {
        return sources.get(uri);
    }

    static String packageOf(String text) {
        String stripped = stripComments(text);
        Matcher m = PACKAGE.matcher(stripped);
        if (m.find()) {
            return m.group(1).replaceAll("\\s+", "");
        }
        return "";
    }

    /** Names of the types declared at brace depth 0 (comments and literals skipped). */
    static List<String> topLevelTypes(String text) {
        String s = stripComments(text);
        java.util.ArrayList<String> out = new java.util.ArrayList<>();
        int depth = 0;
        int n = s.length();
        for (int i = 0; i < n; i++) {
            char c = s.charAt(i);
            if (c == '{') {
                depth++;
            } else if (c == '}') {
                depth = Math.max(0, depth - 1);
            } else if (depth == 0 && Character.isJavaIdentifierStart(c) && (i == 0 || !Character.isJavaIdentifierPart(s.charAt(i - 1)))) {
                int j = i;
                while (j < n && Character.isJavaIdentifierPart(s.charAt(j))) j++;
                String word = s.substring(i, j);
                if (word.equals("class") || word.equals("interface") || word.equals("enum") || word.equals("record")) {
                    int k = j;
                    while (k < n && Character.isWhitespace(s.charAt(k))) k++;
                    int e = k;
                    while (e < n && Character.isJavaIdentifierPart(s.charAt(e))) e++;
                    if (e > k) {
                        out.add(s.substring(k, e));
                    }
                }
                i = j - 1;
            }
        }
        return out;
    }

    /** Replace comments, string and char literals with spaces (keeps offsets). */
    static String stripComments(String text) {
        StringBuilder sb = new StringBuilder(text);
        int n = text.length();
        int i = 0;
        while (i < n) {
            char c = text.charAt(i);
            if (c == '/' && i + 1 < n && text.charAt(i + 1) == '/') {
                while (i < n && text.charAt(i) != '\n') { sb.setCharAt(i, ' '); i++; }
            } else if (c == '/' && i + 1 < n && text.charAt(i + 1) == '*') {
                int end = text.indexOf("*/", i + 2);
                end = end < 0 ? n : end + 2;
                for (int k = i; k < end; k++) if (text.charAt(k) != '\n') sb.setCharAt(k, ' ');
                i = end;
            } else if (c == '"' || c == '\'') {
                int k = i + 1;
                while (k < n && text.charAt(k) != c && text.charAt(k) != '\n') {
                    if (text.charAt(k) == '\\') k++;
                    k++;
                }
                for (int m = i + 1; m < Math.min(k, n); m++) sb.setCharAt(m, ' ');
                i = k + 1;
            } else {
                i++;
            }
        }
        return sb.toString();
    }

    // ── FileSystem.Classpath ────────────────────────────────────────────────

    @Override
    public char[][][] findTypeNames(String qualifiedPackageName, String moduleName) {
        return null;
    }

    @Override
    public NameEnvironmentAnswer findClass(char[] typeName, String qualifiedPackageName, String moduleName,
            String qualifiedBinaryFileName) {
        return findClass(typeName, qualifiedPackageName, moduleName, qualifiedBinaryFileName, false);
    }

    @Override
    public NameEnvironmentAnswer findClass(char[] typeName, String qualifiedPackageName, String moduleName,
            String qualifiedBinaryFileName, boolean asBinaryOnly) {
        if (asBinaryOnly || qualifiedBinaryFileName == null) {
            return null;
        }
        String key = qualifiedBinaryFileName.endsWith(".class")
                ? qualifiedBinaryFileName.substring(0, qualifiedBinaryFileName.length() - 6)
                : qualifiedBinaryFileName;
        String uri = types.get(key);
        if (uri == null) {
            return null;
        }
        String text = sources.get(uri);
        if (text == null) {
            return null;
        }
        return new NameEnvironmentAnswer(new CompilationUnit(text.toCharArray(), fileName(uri), null), null);
    }

    static String fileName(String uri) {
        try {
            String path = new java.net.URI(uri).getPath();
            if (path != null && !path.isEmpty()) {
                return path;
            }
        } catch (Exception ignored) {
        }
        return uri;
    }

    @Override
    public boolean isPackage(String qualifiedPackageName, String moduleName) {
        return packages.contains(qualifiedPackageName);
    }

    @Override
    public char[][] getModulesDeclaringPackage(String qualifiedPackageName, String moduleName) {
        return singletonModuleNameIf(isPackage(qualifiedPackageName, moduleName));
    }

    @Override
    public boolean hasCompilationUnit(String qualifiedPackageName, String moduleName) {
        return isPackage(qualifiedPackageName, moduleName);
    }

    @Override
    public List<FileSystem.Classpath> fetchLinkedJars(FileSystem.ClasspathSectionProblemReporter problemReporter) {
        return Collections.emptyList();
    }

    @Override
    public void reset() {
    }

    @Override
    public char[] normalizedPath() {
        return "<in-memory-sources>".toCharArray();
    }

    @Override
    public String getPath() {
        return "<in-memory-sources>";
    }

    @Override
    public void initialize() {
    }

    @Override
    public boolean hasAnnotationFileFor(String qualifiedTypeName) {
        return false;
    }

    @Override
    public IModule getModule() {
        return null;
    }

    @Override
    public int getMode() {
        return SOURCE;
    }
}
