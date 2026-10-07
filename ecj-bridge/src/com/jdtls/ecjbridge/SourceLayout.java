package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Package fragments of the request's source files. JDT resolves a type by
 * the package fragment (the folder below its source folder) that contains its
 * compilation unit, not by the unit's package declaration: a unit
 * {@code src/test2/E.java} declaring {@code package test1} still defines
 * {@code test2.E} (and reports the mismatched declaration).
 *
 * <p>Source folders are inferred from the units whose package declaration
 * matches their folder; units below a known source folder get the folder
 * relative package.
 */
final class SourceLayout {

    private SourceLayout() {}

    /** Source URI to its package fragment name, for units below a known source folder. */
    static Map<String, String> fragmentPackages(Map<String, String> files) {
        Map<String, String> result = new HashMap<>();
        if (files == null) {
            return result;
        }
        List<String> roots = new ArrayList<>();
        for (Map.Entry<String, String> e : files.entrySet()) {
            String uri = e.getKey();
            if (!uri.endsWith(".java")) continue;
            String pkg = InMemorySourceClasspath.packageOf(e.getValue());
            if (pkg.isEmpty()) continue;
            String dir = directory(uri);
            String suffix = "/" + pkg.replace('.', '/');
            if (dir.endsWith(suffix)) {
                String root = dir.substring(0, dir.length() - suffix.length());
                if (!roots.contains(root)) roots.add(root);
            }
        }
        for (String uri : files.keySet()) {
            if (!uri.endsWith(".java")) continue;
            String dir = directory(uri);
            String best = null;
            for (String root : roots) {
                if ((dir.equals(root) || dir.startsWith(root + "/")) && (best == null || root.length() > best.length())) {
                    best = root;
                }
            }
            if (best == null) continue;
            String relative = dir.equals(best) ? "" : dir.substring(best.length() + 1);
            if (isPackagePath(relative)) {
                result.put(uri, relative.replace('/', '.'));
            }
        }
        return result;
    }

    private static String directory(String uri) {
        int slash = uri.lastIndexOf('/');
        return slash < 0 ? "" : uri.substring(0, slash);
    }

    private static boolean isPackagePath(String relative) {
        if (relative.isEmpty()) return true;
        for (String segment : relative.split("/", -1)) {
            if (segment.isEmpty() || !Character.isJavaIdentifierStart(segment.charAt(0))) return false;
            for (int i = 1; i < segment.length(); i++) {
                if (!Character.isJavaIdentifierPart(segment.charAt(i))) return false;
            }
        }
        return true;
    }
}
