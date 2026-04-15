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
            int offset,
            String importPrefix) {

        String source = sourceFiles.get(targetUri);
        if (source == null) return Collections.emptyList();

        List<BridgeCompletion> results = new ArrayList<>();

        try {
            // ── Check if we're inside an import statement ─────────────────────
            // Use the prefix pre-computed by the Rust server (authoritative) if available,
            // otherwise fall back to scanning the source ourselves.
            if (importPrefix == null) importPrefix = importPrefixAt(source, offset);
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
            int clampedOffset = Math.max(0, Math.min(offset, source.length()));
            NodeLocator locator = new NodeLocator(clampedOffset);
            cu.accept(locator);
            ASTNode node = locator.found;

            MemberAccessContext memberAccess = memberAccessAt(source, clampedOffset);
            if (memberAccess != null) {
                addQualifiedMemberCompletions(cu, node, clampedOffset, memberAccess, results);
                return results;
            }

            // ── Always collect every type member in the file ──────────────────
            for (Object decl : cu.types()) {
                if (decl instanceof AbstractTypeDeclaration atd) {
                    collectTypeMembers(atd, results);
                }
            }

            // ── Context-sensitive locals/params from the enclosing method ─────
            if (node != null) {
                extractLocals(node, results);
            }

            // ── Determine context ─────────────────────────────────────────────
            boolean inDeclName = isVariableNamePosition(cu, offset);
            if (inDeclName) {
                // Cursor is in the name slot of a variable declaration (e.g. `int |a`).
                // No meaningful completions exist here.
                return Collections.emptyList();
            }

            boolean inExpression = isExpressionContext(source, offset, cu);
            String prefix = typedPrefix(source, offset);

            // ── Type completions ──────────────────────────────────────────────
            if (!prefix.isEmpty()) {
                // Any context with a typed prefix: search the full JDK for matching
                // types, attaching auto-import edits (same as jdtls via CompletionEngine).
                addJrtTypeCompletions(prefix, cu, source, results);
            } else if (!inExpression) {
                // Empty prefix, non-expression position: offer already-imported simple names.
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
            }

        } catch (Exception e) {
            LOG.warning("Completion error: " + e.getMessage());
        }

        return results;
    }

    private void addQualifiedMemberCompletions(
            CompilationUnit cu,
            ASTNode node,
            int offset,
            MemberAccessContext access,
            List<BridgeCompletion> results) {
        AbstractTypeDeclaration currentType = enclosingType(node);
        if (currentType == null) {
            currentType = enclosingTypeAt(cu, offset);
        }
        if (currentType == null) {
            return;
        }

        Set<String> seen = new LinkedHashSet<>();
        if ("this".equals(access.qualifier)) {
            addInstanceMembers(currentType, cu, access.prefix, true, true, results, seen);
            return;
        }

        if ("super".equals(access.qualifier) && currentType instanceof TypeDeclaration td) {
            Type superType = td.getSuperclassType();
            if (superType == null) {
                return;
            }
            AbstractTypeDeclaration decl = findTypeDeclaration(cu, simpleTypeName(superType.toString()));
            if (decl != null) {
                addInstanceMembers(decl, cu, access.prefix, true, false, results, seen);
            }
        }
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

    private void addInstanceMembers(
            AbstractTypeDeclaration type,
            CompilationUnit cu,
            String prefix,
            boolean includeInherited,
            boolean allowPrivate,
            List<BridgeCompletion> results,
            Set<String> seen) {
        if (type instanceof TypeDeclaration td) {
            for (FieldDeclaration fd : td.getFields()) {
                if (Modifier.isStatic(fd.getModifiers())) continue;
                if (!allowPrivate && Modifier.isPrivate(fd.getModifiers())) continue;
                for (Object frag : fd.fragments()) {
                    if (!(frag instanceof VariableDeclarationFragment vdf)) continue;
                    String name = vdf.getName().getIdentifier();
                    if (!name.startsWith(prefix) || !seen.add("F:" + name)) continue;
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = name;
                    c.kind = 5;
                    c.sortText = "0" + name;
                    results.add(c);
                }
            }
            for (MethodDeclaration md : td.getMethods()) {
                if (md.isConstructor() || Modifier.isStatic(md.getModifiers())) continue;
                if (!allowPrivate && Modifier.isPrivate(md.getModifiers())) continue;
                String name = md.getName().getIdentifier();
                if (!name.startsWith(prefix) || !seen.add("M:" + name + "/" + md.parameters().size())) continue;
                BridgeCompletion c = new BridgeCompletion();
                c.label = name;
                c.kind = 2;
                c.detail = buildMethodSignature(md);
                c.sortText = "0" + name;
                results.add(c);
            }
            if (includeInherited && td.getSuperclassType() != null) {
                AbstractTypeDeclaration parent = findTypeDeclaration(cu, simpleTypeName(td.getSuperclassType().toString()));
                if (parent != null) {
                    addInstanceMembers(parent, cu, prefix, true, false, results, seen);
                } else {
                    addObjectMembers(prefix, results, seen);
                }
            } else if (includeInherited) {
                addObjectMembers(prefix, results, seen);
            }
        } else if (type instanceof EnumDeclaration ed) {
            for (Object bd : ed.bodyDeclarations()) {
                if (!(bd instanceof MethodDeclaration md)) continue;
                if (md.isConstructor() || Modifier.isStatic(md.getModifiers())) continue;
                if (!allowPrivate && Modifier.isPrivate(md.getModifiers())) continue;
                String name = md.getName().getIdentifier();
                if (!name.startsWith(prefix) || !seen.add("M:" + name + "/" + md.parameters().size())) continue;
                BridgeCompletion c = new BridgeCompletion();
                c.label = name;
                c.kind = 2;
                c.detail = buildMethodSignature(md);
                c.sortText = "0" + name;
                results.add(c);
            }
            if (includeInherited) {
                addObjectMembers(prefix, results, seen);
            }
        }
    }

    private void addObjectMembers(
            String prefix,
            List<BridgeCompletion> results,
            Set<String> seen) {
        addSyntheticMethod(results, seen, prefix, "clone", "Object clone()");
        addSyntheticMethod(results, seen, prefix, "equals", "boolean equals(Object obj)");
        addSyntheticMethod(results, seen, prefix, "finalize", "void finalize()");
        addSyntheticMethod(results, seen, prefix, "getClass", "Class<?> getClass()");
        addSyntheticMethod(results, seen, prefix, "hashCode", "int hashCode()");
        addSyntheticMethod(results, seen, prefix, "notify", "void notify()");
        addSyntheticMethod(results, seen, prefix, "notifyAll", "void notifyAll()");
        addSyntheticMethod(results, seen, prefix, "toString", "String toString()");
        addSyntheticMethod(results, seen, prefix, "wait", "void wait()");
        addSyntheticMethod(results, seen, prefix, "wait", "void wait(long timeoutMillis)");
        addSyntheticMethod(results, seen, prefix, "wait", "void wait(long timeoutMillis, int nanos)");
    }

    private void addSyntheticMethod(
            List<BridgeCompletion> results,
            Set<String> seen,
            String prefix,
            String name,
            String detail) {
        if (!name.startsWith(prefix)) {
            return;
        }
        if (!seen.add("M:" + name + "/" + detail)) {
            return;
        }
        BridgeCompletion c = new BridgeCompletion();
        c.label = name;
        c.kind = 2;
        c.detail = detail;
        c.sortText = "0" + name;
        results.add(c);
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

    private AbstractTypeDeclaration enclosingType(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof AbstractTypeDeclaration atd) {
                return atd;
            }
            current = current.getParent();
        }
        return null;
    }

    private AbstractTypeDeclaration enclosingTypeAt(CompilationUnit cu, int offset) {
        EnclosingTypeLocator locator = new EnclosingTypeLocator(offset);
        cu.accept(locator);
        return locator.found;
    }

    private AbstractTypeDeclaration findTypeDeclaration(CompilationUnit cu, String simpleName) {
        TypeFinder finder = new TypeFinder(simpleName);
        cu.accept(finder);
        return finder.found;
    }

    private static String simpleTypeName(String typeName) {
        int generic = typeName.indexOf('<');
        if (generic >= 0) {
            typeName = typeName.substring(0, generic);
        }
        int dot = typeName.lastIndexOf('.');
        return dot >= 0 ? typeName.substring(dot + 1) : typeName;
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
        if (!prefix.contains(".")) {
            // Typing top-level package name (e.g. "jav" → "java", "javax")
            for (String pkg : JRT_TOP_PACKAGES) {
                if (pkg.startsWith(prefix)) {
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = pkg; c.kind = 9;
                    c.detail = "(package)";
                    c.insertText = pkg; c.filterText = pkg;
                    c.sortText = "1" + pkg;
                    results.add(c);
                }
            }
            return;
        }


        // Typing inside a package hierarchy: prefix ends with "." or has partial name.
        // Find parent package (everything up to last dot) and partial typed after last dot.
        int lastDot = prefix.lastIndexOf('.');
        String parentPkg = prefix.substring(0, lastDot);  // e.g. "java" or "java.util"
        String partial   = prefix.substring(lastDot + 1); // e.g. "" or "Arr"

        // label = full FQN (shown in dropdown like jdtls).
        // insertText/filterText = relative suffix after parentPkg (what Monaco inserts/filters on).
        // This way the standard wordRange (covering only what the user typed after the last dot)
        // works correctly without any special range logic.
        // All direct children of parentPkg (sub-packages and classes).
        List<String> children = JRT_PKG_INDEX.getOrDefault(parentPkg, List.of());

        // 1. Direct sub-packages starting with partial.
        for (String name : children) {
            if (Character.isUpperCase(name.charAt(0))) continue; // skip classes
            if (!name.startsWith(partial)) continue;
            String fqn = parentPkg + "." + name;
            BridgeCompletion c = new BridgeCompletion();
            c.label = fqn; c.kind = 9;
            c.detail = "(package)";
            c.insertText = name; c.filterText = name;
            c.sortText = "1" + name;
            results.add(c);
        }

        // 2. Classes directly inside parentPkg starting with partial.
        for (String name : children) {
            if (!Character.isUpperCase(name.charAt(0))) continue;
            if (!name.startsWith(partial)) continue;
            String fqn = parentPkg + "." + name;
            BridgeCompletion c = new BridgeCompletion();
            c.label = fqn; c.kind = 7;
            c.detail = fqn;
            c.insertText = name; c.filterText = name;
            c.sortText = "2" + name;
            results.add(c);
        }
    }

    // ── Public helpers for other services ────────────────────────────────────

    /** Find all fully-qualified class names whose simple name equals {@code simpleName}. */
    public static List<String> searchBySimpleName(String simpleName) {
        ensureJrtIndex();
        List<String> results = new ArrayList<>();
        JRT_PKG_INDEX.forEach((pkg, children) -> {
            if (children.contains(simpleName)) {
                results.add(pkg + "." + simpleName);
            }
        });
        Collections.sort(results);
        return results;
    }

    // ── jrt:/ index ──────────────────────────────────────────────────────────

    static void ensureJrtIndex() {
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

    // ── Context detection ─────────────────────────────────────────────────────

    /**
     * Extracts the Java identifier typed immediately before {@code offset}.
     * E.g. for {@code "int x = Arr|"} returns {@code "Arr"}.
     */
    private String typedPrefix(String source, int offset) {
        int i = Math.min(offset, source.length()) - 1;
        while (i >= 0) {
            char c = source.charAt(i);
            if (!Character.isLetterOrDigit(c) && c != '_') break;
            i--;
        }
        return source.substring(i + 1, Math.min(offset, source.length()));
    }

    private MemberAccessContext memberAccessAt(String source, int offset) {
        int end = Math.max(0, Math.min(offset, source.length()));
        int start = end;
        while (start > 0) {
            char c = source.charAt(start - 1);
            if (!Character.isLetterOrDigit(c) && c != '_' && c != '$') break;
            start--;
        }
        if (start == 0 || source.charAt(start - 1) != '.') {
            return null;
        }

        int qualifierEnd = start - 1;
        int qualifierStart = qualifierEnd;
        while (qualifierStart > 0) {
            char c = source.charAt(qualifierStart - 1);
            if (!Character.isLetterOrDigit(c) && c != '_' && c != '$') break;
            qualifierStart--;
        }
        if (qualifierStart == qualifierEnd) {
            return null;
        }

        MemberAccessContext ctx = new MemberAccessContext();
        ctx.qualifier = source.substring(qualifierStart, qualifierEnd);
        ctx.prefix = source.substring(start, end);
        return ctx;
    }

    /**
     * Adds JDK types whose simple name starts with {@code prefix} to {@code results},
     * including an auto-import text edit for types not already imported.
     * Mirrors what jdtls does via ECJ's CompletionEngine.
     */
    private void addJrtTypeCompletions(String prefix, CompilationUnit cu, String source,
                                       List<BridgeCompletion> results) {
        ensureJrtIndex();

        // Collect already-imported FQNs and locate the end of the import block.
        Set<String> alreadyImported = new HashSet<>();
        int importInsertLine = 0;
        ImportDeclaration lastImport = null;

        for (Object imp : cu.imports()) {
            if (imp instanceof ImportDeclaration id) {
                alreadyImported.add(id.getName().getFullyQualifiedName());
                if (lastImport == null ||
                        id.getStartPosition() > lastImport.getStartPosition()) {
                    lastImport = id;
                }
            }
        }

        if (lastImport != null) {
            // Insert after the last import statement line.
            importInsertLine = cu.getLineNumber(
                    lastImport.getStartPosition() + lastImport.getLength()) - 1;
            // getLineNumber returns 1-based; convert to 0-based row for BridgeTextEdit,
            // then +1 so we insert on the NEXT line.
            importInsertLine += 1;
        } else if (cu.getPackage() != null) {
            importInsertLine = cu.getLineNumber(
                    cu.getPackage().getStartPosition() + cu.getPackage().getLength()) - 1;
            importInsertLine += 1;
        }

        final int insertLine = importInsertLine;

        JRT_PKG_INDEX.forEach((pkg, children) -> {
            for (String name : children) {
                if (!Character.isUpperCase(name.charAt(0))) continue; // skip packages
                if (name.contains("$")) continue;                      // skip inner classes
                if (!name.startsWith(prefix)) continue;

                String fqn = pkg + "." + name;
                BridgeCompletion c = new BridgeCompletion();
                c.label = name;
                c.kind = 7; // Class
                c.detail = fqn;
                c.sortText = "3" + name;
                c.filterText = name;

                if (!alreadyImported.contains(fqn)) {
                    BridgeTextEdit edit = new BridgeTextEdit();
                    edit.startLine = insertLine;
                    edit.startChar = 0;
                    edit.endLine   = insertLine;
                    edit.endChar   = 0;
                    edit.newText   = "import " + fqn + ";\n";
                    c.additionalEdits = List.of(edit);
                }

                results.add(c);
            }
        });
    }

    /**
     * Returns true if the cursor offset is in a variable-declaration name slot
     * (e.g. {@code int |name} before the {@code =}). No completions are useful here.
     */
    private boolean isVariableNamePosition(CompilationUnit cu, int offset) {
        NodeLocator locator = new NodeLocator(offset);
        cu.accept(locator);
        ASTNode node = locator.found;
        if (!(node instanceof SimpleName sn)) return false;
        ASTNode parent = sn.getParent();
        if (parent instanceof VariableDeclarationFragment vdf && vdf.getName() == sn) {
            // Only if there is no initializer yet (before '=')
            return vdf.getInitializer() == null;
        }
        if (parent instanceof SingleVariableDeclaration svd && svd.getName() == sn) {
            return true;
        }
        return false;
    }

    /**
     * Returns true if the cursor is in an expression context: after {@code =},
     * {@code return}, or similar operators where a value is expected rather than a type.
     * In expression context type-name-only completions (imported classes) are suppressed.
     */
    private boolean isExpressionContext(String source, int offset, CompilationUnit cu) {
        // Scan backwards past whitespace to find the last meaningful character
        int i = Math.min(offset, source.length()) - 1;
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        if (i < 0) return false;
        char last = source.charAt(i);
        // Assignment / arithmetic / comparison operators → expression context
        if (last == '=' || last == '+' || last == '-' || last == '*' || last == '/'
                || last == '%' || last == '|' || last == '&' || last == '^'
                || last == '(' || last == ',') {
            return true;
        }
        // Check for 'return' keyword ending just before cursor
        if (i >= 5) {
            String tail = source.substring(i - 5, i + 1).trim();
            if (tail.equals("return")) return true;
        }
        // Check AST: inside a VariableDeclarationFragment initializer or ReturnStatement
        NodeLocator locator = new NodeLocator(offset);
        cu.accept(locator);
        ASTNode node = locator.found;
        if (node == null) return false;
        ASTNode cur = node;
        while (cur != null) {
            if (cur instanceof VariableDeclarationFragment vdf && vdf.getInitializer() != null
                    && vdf.getInitializer().getStartPosition() <= offset) {
                return true;
            }
            if (cur instanceof ReturnStatement rs && rs.getExpression() != null
                    && rs.getExpression().getStartPosition() <= offset) {
                return true;
            }
            if (cur instanceof MethodInvocation || cur instanceof ClassInstanceCreation) {
                return true;
            }
            if (cur instanceof Assignment) return true;
            cur = cur.getParent();
        }
        return false;
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

    static class EnclosingTypeLocator extends ASTVisitor {
        final int offset;
        AbstractTypeDeclaration found;

        EnclosingTypeLocator(int offset) { this.offset = offset; }

        @Override
        public void preVisit(ASTNode node) {
            if (node instanceof AbstractTypeDeclaration atd) {
                int start = atd.getStartPosition();
                int end = start + atd.getLength();
                if (start <= offset && offset <= end) {
                    found = atd;
                }
            }
        }
    }

    static class TypeFinder extends ASTVisitor {
        final String simpleName;
        AbstractTypeDeclaration found;

        TypeFinder(String simpleName) { this.simpleName = simpleName; }

        @Override
        public boolean visit(TypeDeclaration node) {
            if (simpleName.equals(node.getName().getIdentifier())) {
                found = node;
                return false;
            }
            return true;
        }

        @Override
        public boolean visit(EnumDeclaration node) {
            if (simpleName.equals(node.getName().getIdentifier())) {
                found = node;
                return false;
            }
            return true;
        }

        @Override
        public boolean visit(AnnotationTypeDeclaration node) {
            if (simpleName.equals(node.getName().getIdentifier())) {
                found = node;
                return false;
            }
            return true;
        }

        @Override
        public boolean visit(RecordDeclaration node) {
            if (simpleName.equals(node.getName().getIdentifier())) {
                found = node;
                return false;
            }
            return true;
        }
    }

    static class MemberAccessContext {
        String qualifier;
        String prefix;
    }
}
