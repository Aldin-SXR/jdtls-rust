package com.jdtls.ecjbridge;

import org.eclipse.jdt.internal.compiler.env.ICompilationUnit;

import java.io.File;
import java.net.URI;

/**
 * An ICompilationUnit whose source lives entirely in memory.
 * ECJ uses this to compile source code without touching the filesystem.
 */
public class InMemoryCompilationUnit implements ICompilationUnit {

    private final String uri;
    private final String source;
    private final char[] fileName;

    public InMemoryCompilationUnit(String uri, String source) {
        this.uri = uri;
        this.source = source;
        this.fileName = uriToFileName(uri);
    }

    @Override
    public char[] getContents() {
        return source.toCharArray();
    }

    @Override
    public char[] getFileName() {
        return fileName;
    }

    @Override
    public char[] getMainTypeName() {
        // Extract file name without extension and path
        String name = new String(fileName);
        int sep = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
        String base = name.substring(sep + 1);
        if (base.endsWith(".java")) {
            base = base.substring(0, base.length() - 5);
        }
        return base.toCharArray();
    }

    @Override
    public char[][] getPackageName() {
        // Parse package statement from source to derive package name.
        // ECJ will re-parse it anyway; returning null is fine.
        return null;
    }

    @Override
    public boolean ignoreOptionalProblems() {
        return false;
    }

    private static char[] uriToFileName(String uriStr) {
        try {
            URI uri = new URI(uriStr);
            String path = uri.getPath();
            if (path != null) return path.toCharArray();
        } catch (Exception ignored) {}
        return uriStr.toCharArray();
    }
}
