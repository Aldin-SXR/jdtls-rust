package com.jdtls.ecjbridge;

import org.eclipse.jdt.core.dom.*;

import java.io.IOException;
import java.net.URI;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.logging.Logger;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * Provides code completion using ECJ's DOM/AST API plus jrt:/ package index.
 */
public class CompletionService {

    private static final Logger LOG = Logger.getLogger(CompletionService.class.getName());

    /** Lazily-built map: dot-separated package name → simple class names in that package. */
    private static final Map<String, List<String>> JRT_PKG_INDEX = new ConcurrentHashMap<>();
    /** All top-level package names available in jrt:/ (e.g. "java", "javax", "sun", …). */
    private static final List<String> JRT_TOP_PACKAGES = new ArrayList<>();
    private static volatile boolean jrtIndexBuilt = false;

    public List<BridgeCompletion> complete(
            Map<String, String> sourceFiles,
            List<String> classpath,
            String sourceLevel,
            String targetUri,
            int offset) {

        String source = sourceFiles.get(targetUri);
        if (source == null) return Collections.emptyList();

        List<BridgeCompletion> results = new ArrayList<>();

        try {
            // ── Check if we're inside an import statement ─────────────────────
            String importPrefix = importPrefixAt(source, offset);
            if (importPrefix != null) {
                ensureJrtIndex();
                addImportCompletions(importPrefix, results);
                return results;
            }

            // ── Build DOM AST ─────────────────────────────────────────────────
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            parser.setSource(source.toCharArray());
            parser.setKind(ASTParser.K_COMPILATION_UNIT);
            parser.setResolveBindings(false);
            parser.setStatementsRecovery(true);
            parser.setBindingsRecovery(true);

            Map<String, String> opts = new HashMap<>();
            String ver = resolveVersion(sourceLevel);
            opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_SOURCE, ver);
            opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_COMPLIANCE, ver);
            parser.setCompilerOptions(opts);

            CompilationUnit cu = (CompilationUnit) parser.createAST(null);

            // ── Always collect every type member in the file ──────────────────
            for (Object decl : cu.types()) {
                if (decl instanceof AbstractTypeDeclaration atd) {
                    collectTypeMembers(atd, results);
                }
            }

            // ── Context-sensitive locals/params from the enclosing method ─────
            NodeLocator locator = new NodeLocator(offset);
            cu.accept(locator);
            ASTNode node = locator.found;
            if (node != null) {
                extractLocals(node, results);
            }

            // ── Imported type simple names ────────────────────────────────────
            for (Object imp : cu.imports()) {
                if (imp instanceof ImportDeclaration id) {
                    String name = id.getName().getFullyQualifiedName();
                    int dot = name.lastIndexOf('.');
                    String simpleName = dot >= 0 ? name.substring(dot + 1) : name;
                    if (!simpleName.equals("*")) {
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = simpleName;
                        c.kind = 7; // Class
                        c.detail = name;
                        c.sortText = "2" + simpleName;
                        results.add(c);
                    }
                }
            }

        } catch (Exception e) {
            LOG.warning("Completion error: " + e.getMessage());
        }

        return results;
    }

    // ── Type member collection ────────────────────────────────────────────────

    private void collectTypeMembers(AbstractTypeDeclaration atd, List<BridgeCompletion> results) {
        if (atd instanceof TypeDeclaration td) {
            for (FieldDeclaration fd : td.getFields()) {
                for (Object frag : fd.fragments()) {
                    if (frag instanceof VariableDeclarationFragment vdf) {
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = vdf.getName().getIdentifier();
                        c.kind = 5; // Field
                        c.sortText = "1" + c.label;
                        results.add(c);
                    }
                }
            }
            for (MethodDeclaration md : td.getMethods()) {
                BridgeCompletion c = new BridgeCompletion();
                c.label = md.getName().getIdentifier();
                c.kind = 2; // Method
                c.detail = buildMethodSignature(md);
                c.sortText = "1" + c.label;
                results.add(c);
            }
            // Recurse into nested types
            for (Object member : td.bodyDeclarations()) {
                if (member instanceof AbstractTypeDeclaration nested) {
                    collectTypeMembers(nested, results);
                }
            }
        } else if (atd instanceof EnumDeclaration ed) {
            for (Object ec : ed.enumConstants()) {
                if (ec instanceof EnumConstantDeclaration ecd) {
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = ecd.getName().getIdentifier();
                    c.kind = 20; // EnumMember
                    c.sortText = "1" + c.label;
                    results.add(c);
                }
            }
            for (Object bd : ed.bodyDeclarations()) {
                if (bd instanceof MethodDeclaration md) {
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = md.getName().getIdentifier();
                    c.kind = 2; // Method
                    c.detail = buildMethodSignature(md);
                    c.sortText = "1" + c.label;
                    results.add(c);
                }
            }
        }
    }

    private String buildMethodSignature(MethodDeclaration md) {
        StringBuilder sb = new StringBuilder();
        if (md.getReturnType2() != null) sb.append(md.getReturnType2()).append(" ");
        sb.append(md.getName().getIdentifier()).append("(");
        boolean first = true;
        for (Object p : md.parameters()) {
            if (!first) sb.append(", ");
            first = false;
            if (p instanceof SingleVariableDeclaration svd) {
                sb.append(svd.getType()).append(" ").append(svd.getName().getIdentifier());
            }
        }
        sb.append(")");
        return sb.toString();
    }

    // ── Local variable / parameter extraction ────────────────────────────────

    private void extractLocals(ASTNode node, List<BridgeCompletion> results) {
        ASTNode context = node;
        while (context != null) {
            if (context instanceof MethodDeclaration md && md.getBody() != null) {
                LocalVarVisitor visitor = new LocalVarVisitor();
                md.getBody().accept(visitor);
                for (String var : visitor.vars) {
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = var;
                    c.kind = 6; // Variable
                    c.sortText = "0" + var;
                    results.add(c);
                }
                for (Object param : md.parameters()) {
                    if (param instanceof SingleVariableDeclaration svd) {
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = svd.getName().getIdentifier();
                        c.kind = 6; // Variable (parameter)
                        c.sortText = "0" + c.label;
                        results.add(c);
                    }
                }
            }
            context = context.getParent();
        }
    }

    // ── Import completions ────────────────────────────────────────────────────

    /**
     * If the text before {@code offset} looks like an import statement in progress,
     * return the package prefix typed so far (e.g. "" for "import ", "java." for
     * "import java.", "java.util." for "import java.util.").
     * Returns null if not in an import context.
     */
    private static String importPrefixAt(String source, int offset) {
        // Look backwards from offset for the beginning of the line
        int lineStart = offset;
        while (lineStart > 0 && source.charAt(lineStart - 1) != '\n') lineStart--;
        String line = source.substring(lineStart, Math.min(offset, source.length()));
        Matcher m = Pattern.compile("^\\s*import\\s+([\\w.]*?)$").matcher(line);
        if (m.matches()) return m.group(1); // may be empty string ""
        return null;
    }

    private void addImportCompletions(String prefix, List<BridgeCompletion> results) {
        int lastDot = prefix.lastIndexOf('.');
        if (lastDot < 0) {
            // Typing the top-level package (e.g. "jav")
            for (String pkg : JRT_TOP_PACKAGES) {
                if (pkg.startsWith(prefix)) {
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = pkg;
                    c.kind = 9; // Module
                    c.sortText = pkg;
                    results.add(c);
                }
            }
        } else {
            // Typing inside a package (e.g. "java.util.")
            String parentPkg = prefix.substring(0, lastDot);    // "java.util"
            String partial   = prefix.substring(lastDot + 1);   // ""  or "Arr"

            List<String> children = JRT_PKG_INDEX.getOrDefault(parentPkg, List.of());
            for (String name : children) {
                if (name.startsWith(partial)) {
                    boolean isClass = Character.isUpperCase(name.charAt(0));
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = name;
                    c.kind = isClass ? 7 : 9; // Class or Module/package
                    c.detail = parentPkg + "." + name;
                    c.sortText = (isClass ? "2" : "1") + name;
                    results.add(c);
                }
            }
        }
    }

    // ── jrt:/ index ──────────────────────────────────────────────────────────

    private static void ensureJrtIndex() {
        if (jrtIndexBuilt) return;
        synchronized (JRT_PKG_INDEX) {
            if (jrtIndexBuilt) return;
            try {
                java.nio.file.FileSystem jrtFs =
                        FileSystems.getFileSystem(URI.create("jrt:/"));
                Path modules = jrtFs.getPath("/modules");
                // Walk all module directories and index packages + top-level classes
                Files.walk(modules, Integer.MAX_VALUE)
                    .filter(p -> {
                        // We want directories (packages) and .class files
                        try { return Files.isDirectory(p) || p.toString().endsWith(".class"); }
                        catch (Exception e) { return false; }
                    })
                    .forEach(p -> {
                        // Path looks like /modules/java.base/java/util/ArrayList.class
                        // Strip /modules/<module>/
                        String s = p.toString();
                        int slash2 = s.indexOf('/', 1);
                        if (slash2 < 0) return;
                        int slash3 = s.indexOf('/', slash2 + 1);
                        if (slash3 < 0) return;
                        String rel = s.substring(slash3 + 1); // e.g. java/util/ArrayList.class

                        if (Files.isDirectory(p)) {
                            // This is a package directory
                            String dotPkg = rel.replace('/', '.');
                            // Register as top-level if single segment
                            if (!rel.contains("/")) {
                                if (!JRT_TOP_PACKAGES.contains(rel)) {
                                    JRT_TOP_PACKAGES.add(rel);
                                }
                            }
                            // Register as child in its parent
                            int last = rel.lastIndexOf('/');
                            if (last >= 0) {
                                String parentDot = rel.substring(0, last).replace('/', '.');
                                String childName = rel.substring(last + 1);
                                JRT_PKG_INDEX.computeIfAbsent(parentDot, k -> new ArrayList<>())
                                    .add(childName);
                            }
                        } else if (rel.endsWith(".class") && !rel.contains("$")) {
                            // Public class — register in its package
                            int last = rel.lastIndexOf('/');
                            if (last >= 0) {
                                String parentDot = rel.substring(0, last).replace('/', '.');
                                String className = rel.substring(last + 1, rel.length() - 6);
                                JRT_PKG_INDEX.computeIfAbsent(parentDot, k -> new ArrayList<>())
                                    .add(className);
                            }
                        }
                    });
                // Deduplicate
                JRT_PKG_INDEX.forEach((k, v) -> {
                    Set<String> seen = new LinkedHashSet<>(v);
                    v.clear();
                    v.addAll(seen);
                    Collections.sort(v);
                });
                Collections.sort(JRT_TOP_PACKAGES);
                LOG.info("jrt:/ index built: " + JRT_TOP_PACKAGES.size() + " top-level packages");
            } catch (Exception e) {
                LOG.warning("Cannot build jrt:/ index: " + e.getMessage());
            }
            jrtIndexBuilt = true;
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private String resolveVersion(String level) {
        return switch (level.trim()) {
            case "8", "1.8" -> "1.8";
            case "11" -> "11";
            case "17" -> "17";
            case "21" -> "21";
            default -> "21";
        };
    }

    /** Finds the deepest AST node containing the given offset. */
    static class NodeLocator extends ASTVisitor {
        final int offset;
        ASTNode found;

        NodeLocator(int offset) { this.offset = offset; }

        @Override
        public void preVisit(ASTNode node) {
            int start = node.getStartPosition();
            int end = start + node.getLength();
            if (start <= offset && offset <= end) {
                found = node;
            }
        }
    }

    /** Collects simple names of all local variable declarations in a method body. */
    static class LocalVarVisitor extends ASTVisitor {
        final List<String> vars = new ArrayList<>();

        @Override
        public boolean visit(VariableDeclarationStatement node) {
            for (Object frag : node.fragments()) {
                if (frag instanceof VariableDeclarationFragment vdf) {
                    vars.add(vdf.getName().getIdentifier());
                }
            }
            return true;
        }
    }
}
