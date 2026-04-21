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

    /** Java reserved words that should never be used as completion prefixes. */
    private static final Set<String> JAVA_KEYWORDS = Set.of(
        "abstract", "assert", "boolean", "break", "byte", "case", "catch",
        "char", "class", "const", "continue", "default", "do", "double",
        "else", "enum", "extends", "final", "finally", "float", "for",
        "goto", "if", "implements", "import", "instanceof", "int", "interface",
        "long", "native", "new", "package", "private", "protected", "public",
        "return", "short", "static", "strictfp", "super", "switch",
        "synchronized", "this", "throw", "throws", "transient", "try",
        "void", "volatile", "while", "var", "record", "sealed", "permits"
    );

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

            // ── Determine context before collecting members ───────────────────
            boolean inDeclName = isVariableNamePosition(cu, offset);
            if (inDeclName) {
                // Cursor is in the name slot of a variable declaration (e.g. `int |a`).
                // No meaningful completions exist here.
                return Collections.emptyList();
            }

            String prefix = typedPrefix(source, offset);

            // ── Classify position context ─────────────────────────────────────
            PositionContext ctx = classifyPosition(source, cu, clampedOffset, node);

            switch (ctx) {

                case TOP_LEVEL -> {
                    // After a type-declaration keyword with no name typed yet
                    // (e.g. `class |`, `public class |`) the user must type a
                    // name — no completion is meaningful here.
                    if (isRightAfterTypeKeyword(source, clampedOffset, prefix)) break;
                    // Otherwise: offer type-declaration keywords and snippets.
                    addTopLevelKeywordsAndSnippets(prefix, cu, source, clampedOffset, results);
                }

                case TYPE_HEADER -> {
                    // Cursor is after the class/interface name but before the '{'.
                    // The only valid next tokens are `extends` and `implements`.
                    if (JAVA_KEYWORDS.contains(prefix)) break; // user finished typing the keyword
                    for (String kw : new String[]{"extends", "implements"}) {
                        if (kw.startsWith(prefix)) {
                            BridgeCompletion c = new BridgeCompletion();
                            c.label = kw; c.kind = 14; c.sortText = "0" + kw;
                            results.add(c);
                        }
                    }
                }

                case EXTENDS_TYPE -> {
                    // After `extends` / `implements`: only type names are valid.
                    // Do not offer keywords, snippets, or member completions.
                    addSameFileTypeCompletions(prefix, cu, results, false);
                    if (prefix.length() >= 2) {
                        addJrtTypeCompletions(prefix, cu, source, results, false);
                    }
                }

                case CLASS_BODY -> {
                    // Inside a class body but not inside a method.
                    // Offer: ctor snippet, access/type modifier keywords, type names.
                    if (JAVA_KEYWORDS.contains(prefix)) break;

                    // ctor snippet
                    AbstractTypeDeclaration encForCtor = node != null
                            ? enclosingType(node) : enclosingTypeAt(cu, clampedOffset);
                    if (encForCtor instanceof TypeDeclaration td && !td.isInterface()
                            && "ctor".startsWith(prefix)) {
                        String className = td.getName().getIdentifier();
                        BridgeCompletion ctor = new BridgeCompletion();
                        ctor.label = "ctor";
                        ctor.kind = 15; ctor.detail = "constructor";
                        ctor.filterText = "ctor"; ctor.sortText = "0ctor";
                        ctor.insertText = "${1|public,protected,private|} " + className
                                + "(${2}) {\n\t${3:super();}${0}\n}";
                        ctor.insertTextFormat = 2;
                        results.add(ctor);
                    }

                    // Member modifier and type keywords
                    if (!prefix.isEmpty()) {
                        for (String kw : new String[]{
                            "public", "private", "protected", "static", "final",
                            "abstract", "synchronized", "transient", "volatile",
                            "void", "int", "long", "double", "float", "boolean",
                            "char", "byte", "short", "class", "interface", "enum", "record"
                        }) {
                            if (kw.startsWith(prefix)) {
                                BridgeCompletion c = new BridgeCompletion();
                                c.label = kw; c.kind = 14; c.sortText = "5" + kw;
                                results.add(c);
                            }
                        }
                    }

                    // Type completions (for field/method return-type declarations).
                    addSameFileTypeCompletions(prefix, cu, results, false);
                    if (prefix.length() >= 2) {
                        addJrtTypeCompletions(prefix, cu, source, results, false);
                    }

                    // Own members (for potential overrides / field references).
                    AbstractTypeDeclaration enclosing = node != null
                            ? enclosingType(node) : enclosingTypeAt(cu, clampedOffset);
                    if (enclosing != null) {
                        Set<String> seen = new LinkedHashSet<>();
                        collectTypeMembersWithHierarchy(enclosing, cu, results, seen, 0, prefix);
                    }
                }

                case PARAM_DECLARATION -> {
                    // Inside a method/constructor parameter list.
                    // Valid: 'final' modifier, primitive types, reference types.
                    // But if the cursor is in the parameter-name slot (type already written),
                    // suppress completions entirely.
                    if (isInParamNameSlot(source, clampedOffset, prefix)) break;
                    if (JAVA_KEYWORDS.contains(prefix)) break;
                    // 'final' modifier
                    if (!prefix.isEmpty() && "final".startsWith(prefix)) {
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = "final"; c.kind = 14; c.sortText = "0final";
                        results.add(c);
                    }
                    // Primitive types (always relevant in param position)
                    for (String prim : new String[]{
                            "int", "long", "double", "float", "boolean",
                            "char", "byte", "short"}) {
                        if (prim.startsWith(prefix)) {
                            BridgeCompletion c = new BridgeCompletion();
                            c.label = prim; c.kind = 14; c.sortText = "1" + prim;
                            results.add(c);
                        }
                    }
                    // Types from the same file and JDK (threshold: 1 char)
                    addSameFileTypeCompletions(prefix, cu, results, false);
                    if (prefix.length() >= 1) {
                        addJrtTypeCompletions(prefix, cu, source, results, false);
                    }
                }

                case METHOD_BODY -> {
                    // Full completions inside a method/constructor body.
                    if (JAVA_KEYWORDS.contains(prefix)) break;

                    boolean afterNew = isAfterNewKeyword(source, clampedOffset);

                    // Local variables and parameters.
                    if (node != null) extractLocals(node, prefix, results);

                    // Members of enclosing class.
                    AbstractTypeDeclaration enclosing = node != null
                            ? enclosingType(node) : enclosingTypeAt(cu, clampedOffset);
                    if (enclosing != null) {
                        Set<String> seen = new LinkedHashSet<>();
                        collectTypeMembersWithHierarchy(enclosing, cu, results, seen, 0, prefix);
                    }

                    // super() constructor completions.
                    if (!prefix.isEmpty() && "super".startsWith(prefix)
                            && isInsideConstructor(node)) {
                        addSuperConstructorCompletions(cu, node, sourceFiles, results);
                    }

                    // Type completions.
                    addSameFileTypeCompletions(prefix, cu, results, afterNew);
                    if ((afterNew && prefix.length() >= 1) || prefix.length() >= 2) {
                        addJrtTypeCompletions(prefix, cu, source, results, afterNew);
                    } else if (prefix.isEmpty()) {
                        // Empty prefix: imported simple names only.
                        for (Object imp : cu.imports()) {
                            if (imp instanceof ImportDeclaration id) {
                                String name = id.getName().getFullyQualifiedName();
                                int dot = name.lastIndexOf('.');
                                String simpleName = dot >= 0 ? name.substring(dot + 1) : name;
                                if (!simpleName.equals("*")) {
                                    BridgeCompletion c = new BridgeCompletion();
                                    c.label = simpleName; c.kind = 7;
                                    c.detail = name; c.sortText = "2" + simpleName;
                                    results.add(c);
                                }
                            }
                        }
                    }

                    // Statement keywords.
                    if (!prefix.isEmpty()) {
                        for (String kw : new String[]{
                            "return", "if", "else", "for", "while", "do", "switch",
                            "case", "break", "continue", "try", "catch", "finally",
                            "throw", "new", "instanceof", "null", "true", "false",
                            "this", "super", "var",
                            "int", "long", "double", "float", "boolean",
                            "char", "byte", "short", "void"
                        }) {
                            if (kw.startsWith(prefix)) {
                                BridgeCompletion c = new BridgeCompletion();
                                c.label = kw; c.kind = 14; c.sortText = "6" + kw;
                                results.add(c);
                            }
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

    // ── Super constructor completions ─────────────────────────────────────────

    private boolean isInsideConstructor(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof MethodDeclaration md && md.isConstructor()) return true;
            current = current.getParent();
        }
        return false;
    }

    private void addSuperConstructorCompletions(
            CompilationUnit cu,
            ASTNode node,
            Map<String, String> sourceFiles,
            List<BridgeCompletion> results) {

        // Don't offer super() if one already exists in this constructor
        ASTNode cur = node;
        while (cur != null) {
            if (cur instanceof MethodDeclaration md && md.isConstructor()) {
                if (md.getBody() != null) {
                    for (Object stmt : md.getBody().statements()) {
                        if (stmt instanceof SuperConstructorInvocation) return;
                    }
                }
                break;
            }
            cur = cur.getParent();
        }

        AbstractTypeDeclaration atd = enclosingType(node);
        if (!(atd instanceof TypeDeclaration td)) return;

        Type superType = td.getSuperclassType();
        String superName = superType != null ? simpleTypeName(superType.toString()) : null;

        List<MethodDeclaration> superCtors = new ArrayList<>();

        // 1. Same CU first
        if (superName != null) {
            for (Object t : cu.types()) {
                if (t instanceof TypeDeclaration std
                        && std.getName().getIdentifier().equals(superName)) {
                    for (MethodDeclaration m : std.getMethods()) {
                        if (m.isConstructor()) superCtors.add(m);
                    }
                    break;
                }
            }
        }

        // 2. Other open files
        if (superCtors.isEmpty() && superName != null && sourceFiles != null) {
            for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
                ASTParser p2 = ASTParser.newParser(AST.getJLSLatest());
                p2.setSource(entry.getValue().toCharArray());
                p2.setKind(ASTParser.K_COMPILATION_UNIT);
                p2.setStatementsRecovery(true);
                CompilationUnit cu2 = (CompilationUnit) p2.createAST(null);
                for (Object t : cu2.types()) {
                    if (t instanceof TypeDeclaration std
                            && std.getName().getIdentifier().equals(superName)) {
                        for (MethodDeclaration m : std.getMethods()) {
                            if (m.isConstructor()) superCtors.add(m);
                        }
                        break;
                    }
                }
                if (!superCtors.isEmpty()) break;
            }
        }

        String displaySuper = superName != null ? superName : "super";

        if (superCtors.isEmpty()) {
            // Implicit default constructor
            BridgeCompletion c = new BridgeCompletion();
            c.label = "super()";
            c.kind = 4; // Constructor
            c.detail = displaySuper + "()";
            c.insertText = "super()";
            c.insertTextFormat = 1;
            c.sortText = "00super";
            results.add(c);
            return;
        }

        for (MethodDeclaration ctor : superCtors) {
            String sig = buildMethodSignature(ctor); // e.g. "Alda(int a)"
            String paramsPart = sig.substring(sig.indexOf('(')); // e.g. "(int a)"
            BridgeCompletion c = new BridgeCompletion();
            c.label = "super" + paramsPart;
            c.kind = 4; // Constructor
            c.detail = displaySuper + paramsPart;
            c.insertText = buildInsertTextFromParams("super", ctor.parameters());
            c.insertTextFormat = ctor.parameters().isEmpty() ? 1 : 2;
            c.sortText = "00super";
            results.add(c);
        }
    }

    // ── Type member collection ────────────────────────────────────────────────

    /**
     * Collects members (fields + methods) of {@code atd} and its superclass chain
     * within the same compilation unit. Used for non-member-access completions so
     * only the enclosing class (not sibling classes) is offered.
     *
     * @param depth 0 = enclosing class (sort "1"), 1+ = superclass (sort "2"), so
     *              directly-declared members always precede inherited ones.
     */
    private void collectTypeMembersWithHierarchy(
            AbstractTypeDeclaration atd,
            CompilationUnit cu,
            List<BridgeCompletion> results,
            Set<String> seen,
            int depth,
            String namePrefix) {
        String sortPfx = depth == 0 ? "1" : "2"; // own members sort before inherited
        if (atd instanceof TypeDeclaration td) {
            for (FieldDeclaration fd : td.getFields()) {
                for (Object frag : fd.fragments()) {
                    if (!(frag instanceof VariableDeclarationFragment vdf)) continue;
                    String fname = vdf.getName().getIdentifier();
                    if (!isValidJavaIdentifier(fname) || !seen.add("F:" + fname)) continue;
                    if (!fname.startsWith(namePrefix)) continue;
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = fname;
                    c.kind = 5;
                    c.sortText = sortPfx + fname;
                    results.add(c);
                }
            }
            for (MethodDeclaration md : td.getMethods()) {
                if (md.isConstructor()) continue;
                String mname = md.getName().getIdentifier();
                if (!isValidJavaIdentifier(mname)
                        || !seen.add("M:" + mname + "/" + md.parameters().size())) continue;
                if (!mname.startsWith(namePrefix)) continue;
                BridgeCompletion c = new BridgeCompletion();
                c.label = buildMethodLabel(md);
                c.filterText = mname;
                c.kind = 2;
                c.sortText = sortPfx + mname;
                c.insertText = buildInsertTextFromParams(mname, md.parameters());
                c.insertTextFormat = md.parameters().isEmpty() ? 1 : 2;
                results.add(c);
            }
            // Traverse superclass chain (same CU only)
            if (td.getSuperclassType() != null) {
                AbstractTypeDeclaration parent = findTypeDeclaration(
                        cu, simpleTypeName(td.getSuperclassType().toString()));
                if (parent != null) {
                    collectTypeMembersWithHierarchy(parent, cu, results, seen, depth + 1, namePrefix);
                }
            }
        } else if (atd instanceof EnumDeclaration ed) {
            for (Object ec : ed.enumConstants()) {
                if (ec instanceof EnumConstantDeclaration ecd) {
                    String ename = ecd.getName().getIdentifier();
                    if (!seen.add("E:" + ename)) continue;
                    if (!ename.startsWith(namePrefix)) continue;
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = ename;
                    c.kind = 20;
                    c.sortText = sortPfx + ename;
                    results.add(c);
                }
            }
            for (Object bd : ed.bodyDeclarations()) {
                if (!(bd instanceof MethodDeclaration md) || md.isConstructor()) continue;
                String mname = md.getName().getIdentifier();
                if (!seen.add("M:" + mname + "/" + md.parameters().size())) continue;
                if (!mname.startsWith(namePrefix)) continue;
                BridgeCompletion c = new BridgeCompletion();
                c.label = buildMethodLabel(md);
                c.filterText = mname;
                c.kind = 2;
                c.sortText = sortPfx + mname;
                c.insertText = buildInsertTextFromParams(mname, md.parameters());
                c.insertTextFormat = md.parameters().isEmpty() ? 1 : 2;
                results.add(c);
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
                if (!isValidJavaIdentifier(name)) continue;
                if (!name.startsWith(prefix) || !seen.add("M:" + name + "/" + md.parameters().size())) continue;
                BridgeCompletion c = new BridgeCompletion();
                c.label = buildMethodLabel(md);
                c.filterText = name;
                c.kind = 2;
                c.sortText = "0" + name;
                c.insertText = buildInsertTextFromParams(name, md.parameters());
                c.insertTextFormat = md.parameters().isEmpty() ? 1 : 2;
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
                c.label = buildMethodLabel(md);
                c.filterText = name;
                c.kind = 2;
                c.sortText = "0" + name;
                c.insertText = buildInsertTextFromParams(name, md.parameters());
                c.insertTextFormat = md.parameters().isEmpty() ? 1 : 2;
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
        // "9" sort prefix ensures Object methods appear after directly-declared and inherited members
        addSyntheticMethod(results, seen, prefix, "9", "clone", "Object clone()");
        addSyntheticMethod(results, seen, prefix, "9", "equals", "boolean equals(Object obj)");
        addSyntheticMethod(results, seen, prefix, "9", "finalize", "void finalize()");
        addSyntheticMethod(results, seen, prefix, "9", "getClass", "Class<?> getClass()");
        addSyntheticMethod(results, seen, prefix, "9", "hashCode", "int hashCode()");
        addSyntheticMethod(results, seen, prefix, "9", "notify", "void notify()");
        addSyntheticMethod(results, seen, prefix, "9", "notifyAll", "void notifyAll()");
        addSyntheticMethod(results, seen, prefix, "9", "toString", "String toString()");
        addSyntheticMethod(results, seen, prefix, "9", "wait", "void wait()");
        addSyntheticMethod(results, seen, prefix, "9", "wait", "void wait(long timeoutMillis)");
        addSyntheticMethod(results, seen, prefix, "9", "wait", "void wait(long timeoutMillis, int nanos)");
    }

    private void addSyntheticMethod(
            List<BridgeCompletion> results,
            Set<String> seen,
            String prefix,
            String sortPrefix,
            String name,
            String signature) {
        // signature format: "ReturnType name(ParamType param, ...)"  e.g. "void wait(long ms)"
        if (!name.startsWith(prefix)) {
            return;
        }
        if (!seen.add("M:" + name + "/" + signature)) {
            return;
        }
        // Convert "void wait(long ms)" → "wait(long ms) : void"
        String label = syntheticMethodLabel(name, signature);
        BridgeCompletion c = new BridgeCompletion();
        c.label = label;
        c.filterText = name;
        c.kind = 2;
        c.sortText = sortPrefix + name;
        c.insertText = buildInsertTextFromSignature(name, signature);
        c.insertTextFormat = c.insertText.contains("${") ? 2 : 1;
        results.add(c);
    }

    /** Converts a classic-style signature {@code "void wait(long ms)"} to jdtls label {@code "wait(long ms) : void"}. */
    private static String syntheticMethodLabel(String name, String signature) {
        int lp = signature.indexOf('(');
        if (lp < 0) return name + "()";
        String paramsPart = signature.substring(lp); // "(long ms)"
        // Return type is everything before the method name
        int nameIdx = signature.lastIndexOf(name, lp);
        String returnType = nameIdx > 0 ? signature.substring(0, nameIdx).trim() : "void";
        return name + paramsPart + " : " + returnType;
    }

    /** Parses a signature like "void wait(long ms, int nanos)" and builds a snippet. */
    private static String buildInsertTextFromSignature(String name, String detail) {
        int lp = detail.indexOf('(');
        int rp = detail.lastIndexOf(')');
        if (lp < 0 || rp <= lp + 1) return name + "()"; // no params
        String paramStr = detail.substring(lp + 1, rp).trim();
        if (paramStr.isEmpty()) return name + "()";
        String[] parts = paramStr.split(",");
        StringBuilder sb = new StringBuilder(name).append("(");
        for (int i = 0; i < parts.length; i++) {
            String part = parts[i].trim();
            // Last token is the param name
            int sp = part.lastIndexOf(' ');
            String pname = sp >= 0 ? part.substring(sp + 1) : part;
            if (i > 0) sb.append(", ");
            sb.append("${").append(i + 1).append(":").append(pname).append("}");
        }
        sb.append(")");
        return sb.toString();
    }

    /** Builds a snippet insertText from actual AST parameter nodes. */
    private static String buildInsertTextFromParams(String name, List<?> params) {
        if (params.isEmpty()) return name + "()";
        StringBuilder sb = new StringBuilder(name).append("(");
        int idx = 1;
        for (Object p : params) {
            if (p instanceof SingleVariableDeclaration svd) {
                String pname = svd.getName().getIdentifier();
                if (idx > 1) sb.append(", ");
                sb.append("${").append(idx).append(":").append(pname).append("}");
                idx++;
            }
        }
        sb.append(")");
        return sb.toString();
    }

    /** Returns true only for proper Java identifier strings (no ECJ placeholders). */
    /**
     * Returns true only when the cursor is inside the *body block* of a method,
     * constructor, or initializer — NOT when it is in the parameter list.
     * The path from {@code node} up to the {@code MethodDeclaration} must pass
     * through at least one {@link Block} node.
     */
    private static boolean isInsideMethodBody(ASTNode node) {
        boolean seenBlock = false;
        ASTNode current = node;
        while (current != null) {
            if (current instanceof Block) seenBlock = true;
            if (current instanceof MethodDeclaration) return seenBlock;
            if (current instanceof Initializer) return true;
            current = current.getParent();
        }
        return false;
    }

    /**
     * Returns true when the cursor is in the formal parameter list of a method
     * or constructor (i.e. inside {@code MethodDeclaration} but before the body
     * {@code Block}).
     */
    private static boolean isInMethodParameterList(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof Block) return false;          // entered a body
            if (current instanceof MethodDeclaration) return true;
            if (current instanceof AbstractTypeDeclaration) return false; // class level
            current = current.getParent();
        }
        return false;
    }

    /**
     * Returns true when the cursor is in the parameter-name slot, i.e. the current
     * parameter segment (text after the last unmatched {@code ,} or {@code (}) already
     * contains a type token *before* the current prefix.
     *
     * <p>Examples:
     * <ul>
     *   <li>{@code String |} → true (type present, cursor at name slot)</li>
     *   <li>{@code String fo|} → true (type present, partial name being typed)</li>
     *   <li>{@code Str|} → false (cursor is still in the type token)</li>
     *   <li>{@code final Str|} → false (only modifier before prefix)</li>
     *   <li>{@code |} → false (nothing before prefix, this is the type slot)</li>
     * </ul>
     * </p>
     */
    private static boolean isInParamNameSlot(String source, int offset, String prefix) {
        // Walk backward to find the start of this parameter segment,
        // skipping past balanced angle-bracket generics.
        int i = offset - prefix.length() - 1; // position just before the prefix
        int angleDepth = 0;
        int segStart = 0;
        while (i >= 0) {
            char c = source.charAt(i);
            if (c == '>') { angleDepth++; i--; continue; }
            if (c == '<') { angleDepth = Math.max(0, angleDepth - 1); i--; continue; }
            if (angleDepth > 0) { i--; continue; }
            if (c == ',' || c == '(') { segStart = i + 1; break; }
            i--;
        }

        // Extract the segment text BEFORE the current prefix.
        String beforePrefix = source.substring(segStart, offset - prefix.length()).trim();
        if (beforePrefix.isEmpty()) return false;

        // Tokenize: split on whitespace and angle-bracket/array punctuation.
        // We want to find non-modifier, non-annotation identifier tokens.
        String[] tokens = beforePrefix.split("[\\s<>\\[\\]]+");
        boolean prevWasAt = false;
        int typeTokenCount = 0;
        for (String tok : tokens) {
            if (tok.isEmpty()) continue;
            if (tok.equals("@")) { prevWasAt = true; continue; }
            if (prevWasAt) { prevWasAt = false; continue; } // skip annotation name
            prevWasAt = false;
            if (tok.startsWith("@")) continue; // "@Annotation" fused
            if (tok.equals("final")) continue;  // modifier
            if (Character.isLetter(tok.charAt(0)) || tok.charAt(0) == '_') {
                typeTokenCount++;
            }
        }
        return typeTokenCount >= 1;
    }

    private static boolean isValidJavaIdentifier(String s) {
        if (s == null || s.isEmpty() || s.startsWith("$")) return false;
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (!Character.isLetterOrDigit(c) && c != '_') return false;
        }
        return true;
    }

    /**
     * Builds a jdtls-style method label: {@code "name(ParamType param) : ReturnType"}.
     * e.g. {@code "setAge(int age) : void"}, {@code "getAge() : int"}.
     */
    private String buildMethodLabel(MethodDeclaration md) {
        String mname = md.getName().getIdentifier();
        StringBuilder sb = new StringBuilder(mname).append("(");
        boolean first = true;
        for (Object p : md.parameters()) {
            if (!first) sb.append(", ");
            first = false;
            if (p instanceof SingleVariableDeclaration svd) {
                sb.append(svd.getType()).append(" ").append(svd.getName().getIdentifier());
            }
        }
        sb.append(")");
        if (md.getReturnType2() != null) {
            sb.append(" : ").append(md.getReturnType2());
        }
        return sb.toString();
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

    private void extractLocals(ASTNode node, String namePrefix, List<BridgeCompletion> results) {
        ASTNode context = node;
        while (context != null) {
            if (context instanceof MethodDeclaration md && md.getBody() != null) {
                LocalVarVisitor visitor = new LocalVarVisitor();
                md.getBody().accept(visitor);
                for (String var : visitor.vars) {
                    if (!isValidJavaIdentifier(var) || !var.startsWith(namePrefix)) continue;
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = var;
                    c.kind = 6; // Variable
                    c.sortText = "0" + var;
                    results.add(c);
                }
                for (Object param : md.parameters()) {
                    if (param instanceof SingleVariableDeclaration svd) {
                        String pname = svd.getName().getIdentifier();
                        if (!isValidJavaIdentifier(pname) || !pname.startsWith(namePrefix)) continue;
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = pname;
                        c.kind = 6; // Variable (parameter)
                        c.sortText = "0" + pname;
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

    /**
     * Finds JRT types whose simple name is similar (but not identical) to {@code typeName}.
     * Used for "Change to 'X' (pkg)" code actions — rename + import alternatives.
     * Matches on case-insensitive prefix, sorted by edit distance, limited to {@code maxResults}.
     */
    public static List<String> searchSimilarTypeNames(String typeName, int maxResults) {
        ensureJrtIndex();
        String lower = typeName.toLowerCase();
        int prefixLen = Math.min(4, lower.length());
        String prefix = lower.substring(0, prefixLen);

        List<long[]> distAndIdx = new ArrayList<>();
        List<String> fqns = new ArrayList<>();

        JRT_PKG_INDEX.forEach((pkg, children) -> {
            // Skip internal/private JDK packages — not useful as "Change to" suggestions.
            if (isInternalPackage(pkg)) return;
            for (String child : children) {
                if (child.equalsIgnoreCase(typeName)) continue; // exact match → handled by "Import"
                if (!child.toLowerCase().startsWith(prefix)) continue;
                int dist = levenshteinDistance(lower, child.toLowerCase());
                int idx = fqns.size();
                fqns.add(pkg + "." + child);
                distAndIdx.add(new long[]{dist, idx});
            }
        });

        distAndIdx.sort(Comparator.comparingLong((long[] a) -> a[0]).thenComparingLong(a -> a[1]));
        List<String> results = new ArrayList<>();
        for (int i = 0; i < Math.min(maxResults, distAndIdx.size()); i++) {
            results.add(fqns.get((int) distAndIdx.get(i)[1]));
        }
        return results;
    }

    private static boolean isInternalPackage(String pkg) {
        return pkg.startsWith("sun.")
            || pkg.startsWith("com.sun.")
            || pkg.startsWith("jdk.internal.")
            || pkg.startsWith("jdk.jfr.")
            || pkg.startsWith("com.oracle.")
            || pkg.startsWith("jdk.nashorn.")
            || pkg.startsWith("netscape.")
            || pkg.equals("sun")
            || pkg.equals("jdk.internal");
    }

    private static int levenshteinDistance(String a, String b) {
        int[][] dp = new int[a.length() + 1][b.length() + 1];
        for (int i = 0; i <= a.length(); i++) dp[i][0] = i;
        for (int j = 0; j <= b.length(); j++) dp[0][j] = j;
        for (int i = 1; i <= a.length(); i++)
            for (int j = 1; j <= b.length(); j++)
                dp[i][j] = Math.min(Math.min(dp[i-1][j]+1, dp[i][j-1]+1),
                        dp[i-1][j-1] + (a.charAt(i-1) == b.charAt(j-1) ? 0 : 1));
        return dp[a.length()][b.length()];
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
    /**
     * Returns true if the last non-whitespace, non-identifier token before the prefix
     * at {@code offset} is the {@code new} keyword.
     * e.g. {@code "new Lin|"} → true, {@code "String s = Lin|"} → false.
     */
    /**
     * Returns true when the cursor (after the current prefix) is in the supertype
     * list of a class declaration, i.e. the preceding keyword is {@code extends} or
     * {@code implements} (possibly separated by comma-delimited type names).
     * <p>
     * Handles: {@code class Foo extends Ba|}, {@code class Foo implements A, Ba|}
     */
    private static boolean isAfterExtendsOrImplements(String source, int offset, String prefix) {
        // Start just before the current prefix.
        int i = offset - prefix.length() - 1;
        // Skip leading whitespace before the prefix.
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;

        // Skip comma-separated type names that may appear before our prefix
        // (e.g. "implements A, B, " — skip ", A, " going backwards).
        while (i >= 0 && source.charAt(i) == ',') {
            i--; // skip comma
            while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
            // Skip one type name (possibly qualified / with generics).
            while (i >= 0) {
                char c = source.charAt(i);
                if (Character.isLetterOrDigit(c) || c == '_' || c == '.' || c == '<' || c == '>') i--;
                else break;
            }
            while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        }

        // i now points to the last character of the keyword ('s' of "extends"/"implements").
        if (i < 0) return false;
        int wordEnd = i + 1;
        while (i >= 0 && Character.isLetter(source.charAt(i))) i--;
        if (wordEnd <= i + 1) return false;
        String word = source.substring(i + 1, wordEnd);
        return "extends".equals(word) || "implements".equals(word);
    }

    private static boolean isAfterNewKeyword(String source, int offset) {
        // Step back past the current identifier prefix.
        int i = Math.min(offset, source.length()) - 1;
        while (i >= 0 && (Character.isLetterOrDigit(source.charAt(i)) || source.charAt(i) == '_')) i--;
        // Step back past whitespace.
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        // Check if the three characters before are "new" preceded by a word boundary.
        if (i < 2) return false;
        if (source.charAt(i) != 'w' || source.charAt(i - 1) != 'e' || source.charAt(i - 2) != 'n') return false;
        // Ensure "new" is not part of a longer identifier.
        return i - 3 < 0 || !Character.isLetterOrDigit(source.charAt(i - 3));
    }

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
     * Adds type declarations from the current compilation unit to {@code results}.
     * When {@code afterNew} is true, constructors are offered instead of the bare class name.
     */
    private void addSameFileTypeCompletions(String prefix, CompilationUnit cu,
                                             List<BridgeCompletion> results, boolean afterNew) {
        String lowerPrefix = prefix.toLowerCase();
        AllTypesCollector collector = new AllTypesCollector();
        cu.accept(collector);
        for (AbstractTypeDeclaration atd : collector.types) {
            String name = atd.getName().getIdentifier();
            if (!lowerPrefix.isEmpty() && !name.toLowerCase().startsWith(lowerPrefix)) continue;

            if (afterNew && atd instanceof TypeDeclaration td && !td.isInterface()) {
                // Offer each constructor as a separate item.
                List<MethodDeclaration> ctors = new ArrayList<>();
                for (MethodDeclaration md : td.getMethods()) {
                    if (md.isConstructor()) ctors.add(md);
                }
                if (ctors.isEmpty()) {
                    // Implicit no-arg constructor.
                    BridgeCompletion c = new BridgeCompletion();
                    c.label = name + "()";
                    c.kind = 4; // Constructor
                    c.detail = name;
                    c.filterText = name;
                    c.insertText = name + "()";
                    c.insertTextFormat = 1;
                    c.sortText = "1-" + name;
                    results.add(c);
                } else {
                    for (MethodDeclaration ctor : ctors) {
                        String paramsPart = buildParamsPart(ctor.parameters());
                        BridgeCompletion c = new BridgeCompletion();
                        c.label = name + paramsPart;
                        c.kind = 4; // Constructor
                        c.detail = name;
                        c.filterText = name;
                        c.insertText = buildInsertTextFromParams(name, ctor.parameters());
                        c.insertTextFormat = ctor.parameters().isEmpty() ? 1 : 2;
                        c.sortText = "1-" + name;
                        results.add(c);
                    }
                }
            } else {
                int kind = (atd instanceof TypeDeclaration td && td.isInterface()) ? 8  // Interface
                         : (atd instanceof EnumDeclaration)                         ? 10 // Enum
                         : 7; // Class
                BridgeCompletion c = new BridgeCompletion();
                c.label = name;
                c.kind = kind;
                c.detail = "(this file)";
                c.sortText = "1-" + name;
                results.add(c);
            }
        }
    }

    /** Builds the params display string like {@code "(int age, String name)"}. */
    private static String buildParamsPart(List<?> params) {
        StringBuilder sb = new StringBuilder("(");
        boolean first = true;
        for (Object p : params) {
            if (!first) sb.append(", ");
            first = false;
            if (p instanceof SingleVariableDeclaration svd) {
                sb.append(svd.getType()).append(" ").append(svd.getName().getIdentifier());
            }
        }
        sb.append(")");
        return sb.toString();
    }

    /**
     * Adds JDK types whose simple name starts with {@code prefix} to {@code results},
     * including an auto-import text edit for types not already imported.
     * When {@code afterNew} is true, items get constructor-style insertText {@code "Name()"}.
     */
    private void addJrtTypeCompletions(String prefix, CompilationUnit cu, String source,
                                       List<BridgeCompletion> results, boolean afterNew) {
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

        String lowerPrefix = prefix.toLowerCase();
        JRT_PKG_INDEX.forEach((pkg, children) -> {
            for (String name : children) {
                if (!Character.isUpperCase(name.charAt(0))) continue; // skip packages
                if (name.contains("$")) continue;                      // skip inner classes
                if (!name.toLowerCase().startsWith(lowerPrefix)) continue;

                String fqn = pkg + "." + name;
                BridgeCompletion c = new BridgeCompletion();
                c.kind = afterNew ? 4 : 7; // Constructor vs Class
                c.detail = fqn;
                c.sortText = "3" + name;
                c.filterText = name;

                if (afterNew) {
                    // We don't know the JDK constructors, so offer a single generic form.
                    // Use diamond operator for java.util / java.util.concurrent generic classes,
                    // plain parens otherwise.
                    boolean likelyGeneric = pkg.startsWith("java.util")
                            || pkg.startsWith("java.lang.ref")
                            || pkg.startsWith("java.util.concurrent")
                            || pkg.startsWith("java.util.function");
                    String insertSuffix = likelyGeneric ? "<>()" : "()";
                    c.label = name + insertSuffix;
                    c.insertText = name + insertSuffix;
                    c.insertTextFormat = 1;
                } else {
                    c.label = name;
                }

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
    /**
     * Returns true only when the cursor is inside the opening '{' ... '}' of this type body.
     * Uses brace-depth counting from the end of the type name to the cursor, so it cannot
     * be fooled by '{' characters belonging to other (sibling or parent) classes.
     */
    private static boolean isCursorInsideBody(String source, AbstractTypeDeclaration atd, int offset) {
        int nameEnd = atd.getName().getStartPosition() + atd.getName().getLength();
        if (offset <= nameEnd) return false;
        int depth = 0;
        boolean seenOpen = false;
        int limit = Math.min(offset, source.length());
        for (int i = nameEnd; i < limit; i++) {
            char c = source.charAt(i);
            if (c == '{') { depth++; seenOpen = true; }
            else if (c == '}') { if (--depth < 0) return false; }
        }
        return seenOpen && depth > 0;
    }

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

    // ── Position context ──────────────────────────────────────────────────────

    enum PositionContext {
        /** Outside any class body — typing a top-level declaration. */
        TOP_LEVEL,
        /** After a type name but before the opening '{' — e.g. `class Foo |`. */
        TYPE_HEADER,
        /** After `extends` or `implements` — expecting a type name. */
        EXTENDS_TYPE,
        /** Inside a class body but not inside a method/initializer. */
        CLASS_BODY,
        /** Inside a method or initializer body. */
        METHOD_BODY,
        /** Inside the formal parameter list of a method or constructor. */
        PARAM_DECLARATION
    }

    private PositionContext classifyPosition(String source, CompilationUnit cu, int offset, ASTNode node) {
        // Use raw brace-depth counting as the primary discriminator.
        // The recovered AST is unreliable for top-level positions because parser
        // recovery can place a cursor node inside a sibling class's span.
        int depth = braceDepthAt(source, offset);

        if (depth == 0) {
            // Cursor is at file top-level (before any open '{' that isn't closed).
            // The only valid contexts here are TOP_LEVEL, TYPE_HEADER, and EXTENDS_TYPE.
            String prefix = typedPrefix(source, offset);
            if (isAfterExtendsOrImplements(source, offset, prefix)) return PositionContext.EXTENDS_TYPE;
            if (isInTypeHeader(source, offset, prefix)) return PositionContext.TYPE_HEADER;
            return PositionContext.TOP_LEVEL;
        }

        // depth >= 1: cursor is inside at least one '{...}' pair.
        if (isInsideMethodBody(node)) return PositionContext.METHOD_BODY;
        if (isInMethodParameterList(node)) return PositionContext.PARAM_DECLARATION;
        return PositionContext.CLASS_BODY;
    }

    /**
     * Counts the net brace depth at {@code offset} by scanning raw source,
     * skipping string literals, character literals, and comments so that
     * braces inside them do not affect the count.
     */
    private static int braceDepthAt(String source, int offset) {
        int limit = Math.min(offset, source.length());
        int depth = 0;
        int i = 0;
        while (i < limit) {
            char c = source.charAt(i);
            // Line comment
            if (c == '/' && i + 1 < limit && source.charAt(i + 1) == '/') {
                i += 2;
                while (i < limit && source.charAt(i) != '\n') i++;
                continue;
            }
            // Block comment
            if (c == '/' && i + 1 < limit && source.charAt(i + 1) == '*') {
                i += 2;
                while (i + 1 < limit && !(source.charAt(i) == '*' && source.charAt(i + 1) == '/')) i++;
                i += 2; // skip */
                continue;
            }
            // String literal (including text blocks)
            if (c == '"') {
                boolean textBlock = i + 2 < limit && source.charAt(i + 1) == '"' && source.charAt(i + 2) == '"';
                if (textBlock) {
                    i += 3;
                    while (i + 2 < limit &&
                           !(source.charAt(i) == '"' && source.charAt(i+1) == '"' && source.charAt(i+2) == '"')) i++;
                    i += 3;
                } else {
                    i++;
                    while (i < limit && source.charAt(i) != '"') {
                        if (source.charAt(i) == '\\') i++;
                        i++;
                    }
                    i++; // skip closing "
                }
                continue;
            }
            // Char literal
            if (c == '\'') {
                i++;
                while (i < limit && source.charAt(i) != '\'') {
                    if (source.charAt(i) == '\\') i++;
                    i++;
                }
                i++; // skip closing '
                continue;
            }
            if (c == '{') depth++;
            else if (c == '}' && depth > 0) depth--;
            i++;
        }
        return depth;
    }

    /**
     * Returns true when the cursor is in the "header" of a type declaration —
     * i.e. the type keyword and name have been written but the opening '{' has not
     * been emitted yet.  This covers both:
     * <ul>
     *   <li>{@code class Foo|} — prefix IS the type name</li>
     *   <li>{@code class Foo |} / {@code class Foo ex|} — cursor after the name</li>
     * </ul>
     */
    private static boolean isInTypeHeader(String source, int offset, String prefix) {
        int i = offset - prefix.length() - 1;
        // Skip whitespace before prefix
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        if (i < 0) return false;
        if (!Character.isLetterOrDigit(source.charAt(i)) && source.charAt(i) != '_') return false;

        // Extract the identifier immediately before the prefix
        int id1End = i + 1;
        while (i >= 0 && (Character.isLetterOrDigit(source.charAt(i)) || source.charAt(i) == '_')) i--;
        String id1 = source.substring(i + 1, id1End);

        // Case A: id1 itself is the type-declaration keyword and prefix is a non-empty,
        // non-keyword identifier — meaning the prefix IS the type name being typed.
        if (isTypeDeclarationKeyword(id1) && !prefix.isEmpty() && !JAVA_KEYWORDS.contains(prefix)) return true;

        // Case B: id1 is the type name — check that what precedes it is the keyword
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        if (i < 0) return false;
        if (!Character.isLetter(source.charAt(i))) return false;
        int kwEnd = i + 1;
        while (i >= 0 && Character.isLetter(source.charAt(i))) i--;
        String kw = source.substring(i + 1, kwEnd);
        return isTypeDeclarationKeyword(kw);
    }

    private static boolean isTypeDeclarationKeyword(String s) {
        return "class".equals(s) || "interface".equals(s) || "enum".equals(s) || "record".equals(s);
    }

    /**
     * Returns true when the cursor (with empty prefix) is positioned immediately
     * after a type-declaration keyword such as {@code class}, {@code interface},
     * {@code enum}, or {@code record} — including when modifiers precede it
     * (e.g. {@code public class |}).  In this slot the user must type a name;
     * no meaningful completion exists yet.
     */
    private static boolean isRightAfterTypeKeyword(String source, int offset, String prefix) {
        if (!prefix.isEmpty()) return false;
        int i = offset - 1;
        while (i >= 0 && Character.isWhitespace(source.charAt(i))) i--;
        if (i < 0 || !Character.isLetter(source.charAt(i))) return false;
        int kwEnd = i + 1;
        while (i >= 0 && Character.isLetter(source.charAt(i))) i--;
        String kw = source.substring(i + 1, kwEnd);
        return isTypeDeclarationKeyword(kw);
    }

    /** Top-level declaration keywords + class/interface/enum/record snippets. */
    private static void addTopLevelKeywordsAndSnippets(String prefix, CompilationUnit cu,
            String source, int offset, List<BridgeCompletion> results) {
        // Access/modifier keywords valid at file level.
        for (String kw : new String[]{
            "public", "protected", "abstract", "final", "import", "package",
            "class", "interface", "enum", "record"
        }) {
            if (kw.startsWith(prefix) && !prefix.isEmpty()) {
                BridgeCompletion c = new BridgeCompletion();
                c.label = kw; c.kind = 14;
                c.sortText = "1" + kw; // snippets ("0") sort before keywords ("1")
                results.add(c);
            }
        }

        // Type-declaration snippets (class, interface, enum, record).
        // Mirror eclipse.jdt.ls SnippetCompletionProposal behaviour.
        record Snip(String keyword, String template, String detail) {}
        List<Snip> snips = List.of(
            new Snip("class",
                "class ${1:ClassName} {\n\t${0}\n}",
                "class declaration"),
            new Snip("interface",
                "interface ${1:InterfaceName} {\n\t${0}\n}",
                "interface declaration"),
            new Snip("enum",
                "enum ${1:EnumName} {\n\t${0}\n}",
                "enum declaration"),
            new Snip("record",
                "record ${1:RecordName}(${2}) {\n\t${0}\n}",
                "record declaration"),
            new Snip("abstract class",
                "abstract class ${1:ClassName} {\n\t${0}\n}",
                "abstract class declaration")
        );
        for (Snip s : snips) {
            // Match on the first word of the label ("abstract" for "abstract class").
            String firstWord = s.keyword().split(" ")[0];
            boolean matches = prefix.isEmpty()
                    || firstWord.startsWith(prefix);
            if (!matches) continue;
            BridgeCompletion c = new BridgeCompletion();
            c.label      = s.keyword();
            c.kind       = 15; // Snippet
            c.detail     = s.detail();
            // filterText uses only the first word so VS Code's client-side fuzzy
            // match doesn't surface "abstract class" when the user types "class".
            c.filterText = firstWord;
            c.sortText   = "0" + firstWord; // snippets ("0") sort before keywords ("1")
            c.insertText = s.template();
            c.insertTextFormat = 2;
            results.add(c);
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

    /** Collects all AbstractTypeDeclaration nodes in a CompilationUnit. */
    static class AllTypesCollector extends ASTVisitor {
        final List<AbstractTypeDeclaration> types = new ArrayList<>();

        @Override public boolean visit(TypeDeclaration node)           { types.add(node); return true; }
        @Override public boolean visit(EnumDeclaration node)           { types.add(node); return true; }
        @Override public boolean visit(AnnotationTypeDeclaration node) { types.add(node); return true; }
        @Override public boolean visit(RecordDeclaration node)         { types.add(node); return true; }
    }
}
