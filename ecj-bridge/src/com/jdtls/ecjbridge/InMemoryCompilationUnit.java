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

    /** A unit with an explicit file name (e.g. the attached source of a class file). */
    public InMemoryCompilationUnit(String uri, String source, String fileName) {
        this.uri = uri;
        this.source = source;
        this.fileName = fileName.toCharArray();
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

    /** The expected package (Java model: the package fragment of the unit), or null. */
    private char[][] packageName;

    /** Set the package the unit belongs to (dotted, "" for the default package). */
    public InMemoryCompilationUnit withPackage(String dotted) {
        if (dotted != null) {
            this.packageName = dotted.isEmpty() ? new char[0][]
                    : org.eclipse.jdt.core.compiler.CharOperation.splitOn('.', dotted.toCharArray());
        }
        return this;
    }

    @Override
    public char[][] getPackageName() {
        // null: the package declaration decides.
        return packageName;
    }

    @Override
    public org.eclipse.jdt.internal.compiler.lookup.ModuleBinding module(
            org.eclipse.jdt.internal.compiler.lookup.LookupEnvironment environment) {
        // Workspace sources live in the unnamed module (ECJ >= 3.40 reports
        // "module not found" for a null module name otherwise).
        return environment.UnNamedModule;
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
