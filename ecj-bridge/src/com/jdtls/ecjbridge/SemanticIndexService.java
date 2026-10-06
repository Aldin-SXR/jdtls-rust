package com.jdtls.ecjbridge;

import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.*;
import java.util.logging.Logger;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import org.eclipse.jdt.core.Signature;
import org.eclipse.jdt.core.ToolFactory;
import org.eclipse.jdt.core.compiler.IScanner;
import org.eclipse.jdt.core.compiler.ITerminalSymbols;
import org.eclipse.jdt.core.dom.*;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileReader;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/**
 * Binding-resolved semantic index of a project's sources, used by code lens,
 * call hierarchy, type hierarchy and implementation search.  Data only: the
 * Rust server turns the elements and matches into LSP results.
 *
 * <p>The sources of a request are resolved in one batch {@code createASTs}
 * over a temporary mirror (so documents need not exist on disk).  While each
 * unit is still alive the index records its declarations (with their element
 * descriptors), every reference with its resolved target, and the type
 * information needed to replicate the JDT search engine's method-reference
 * matching ({@code MethodLocator}).  The index is cached by the content of the
 * request, so repeated queries (one per code lens) are cheap.
 *
 * <p>Binary type listings ({@code listTypes}) and binary subtype scans read
 * class files with {@link ClassFileReader}, the reader JDT's indexer uses.
 */
final class SemanticIndexService {

    private static final Logger LOG = Logger.getLogger(SemanticIndexService.class.getName());

    // JDT modifier bits not in java.lang.reflect.Modifier
    static final int ACC_INTERFACE = 0x0200;
    static final int ACC_ANNOTATION = 0x2000;
    static final int ACC_ENUM = 0x4000;
    static final int ACC_DEPRECATED = 0x100000;

    // ─── Data ────────────────────────────────────────────────────────────────

    /** Range in LSP coordinates. */
    static final class Rng {
        int sl, sc, el, ec;
    }

    /** Element descriptor (a JDT {@code IMember}). */
    static final class Elem {
        String key;
        String kind;            // type, method, constructor, field, enumConstant, initializer
        String name;
        String typeKind;        // class, interface, enum, annotation, record (types)
        int flags;
        boolean deprecated;
        boolean fromSource;
        boolean anonymous;
        boolean local;
        boolean implicitType;
        boolean defaultConstructor;
        String fqn;             // types: binary-style fully qualified name (Outer$Inner)
        String declaringTypeKey;
        String declaringTypeFqn;
        String packageName;
        List<String> typeChain; // enclosing type names from the top-level type
        List<String> params;    // display types (simple names)
        List<String> paramSigs; // JDT model parameter signatures (handle identifiers)
        boolean varargs;
        String returnType;
        List<String> typeParams;
        int occurrence = 1;
        String uri;
        Rng range;
        Rng nameRange;
        // binary location
        String archive;
        String module;
        String classFile;
        // hierarchy (types)
        String superclassKey;
        List<String> interfaceKeys;
        List<String> overrides; // methods: keys of the methods this one overrides
        List<String> constructorKeys; // types: declared constructors in source order
        transient int start = -1, end = -1;
    }

    static final class Ref {
        String kind;            // type, method, superMethod, methodRef, ctor, field
        String targetKey;
        String uri;
        Rng range;              // search match range
        Rng nodeRange;          // whole node (call hierarchy callees)
        transient int nodeStart;
        boolean javadoc;
        String enclosing;       // key of the enclosing member (null for imports/package)
        String enclosingKind;   // member, import, package
        String selector;
        List<String> paramErasures;
        String receiverKey;
        boolean virtual;
        String declClassKey;
        boolean defaultCtor;
    }

    static final class TypeInfo {
        String key;
        String qualifiedName;
        boolean isInterface;
        boolean isAbstract;
        String superKey;
        List<String> interfaceKeys = new ArrayList<>();
        List<MethodInfo> methods = new ArrayList<>();
    }

    static final class MethodInfo {
        String selector;
        List<String> erasures;
        boolean isAbstract;
        String key;
    }

    static final class NameOcc {
        int start, end;
        String key;
        String kind;            // member kinds, or "local"/"other"
    }

    static final class FileIndex {
        String uri;
        CompilationUnitInfo lines;
        List<NameOcc> names = new ArrayList<>();
        List<Elem> decls = new ArrayList<>();
    }

    /** Line table of a unit, to convert positions without the AST. */
    static final class CompilationUnitInfo {
        int[] lineStarts;
        int length;

        int offset(int line, int character) {
            if (line < 0) return 0;
            if (line >= lineStarts.length) return length;
            return Math.min(lineStarts[line] + character, length);
        }
    }

    static final class Index {
        final Map<String, FileIndex> files = new LinkedHashMap<>();
        final Map<String, Elem> elements = new HashMap<>();   // source declarations + referenced binaries
        final Map<String, TypeInfo> types = new HashMap<>();
        final List<Ref> refs = new ArrayList<>();
        final List<Elem> sourceTypes = new ArrayList<>();
        final List<Elem> sourceMethods = new ArrayList<>();
    }

    // ─── Cache ───────────────────────────────────────────────────────────────

    private static final int CACHE_SIZE = 6;
    private static final LinkedHashMap<String, Index> CACHE = new LinkedHashMap<>(16, 0.75f, true);

    private Index index(Map<String, String> files, List<String> classpath, String sourceLevel, List<String> owned) {
        Map<String, String> all = files == null ? Map.of() : files;
        List<String> own = owned == null ? new ArrayList<>(all.keySet()) : owned;
        String key = cacheKey(all, classpath, sourceLevel, own);
        synchronized (CACHE) {
            Index cached = CACHE.get(key);
            if (cached != null) return cached;
        }
        Index built = build(all, classpath, sourceLevel, own);
        synchronized (CACHE) {
            CACHE.put(key, built);
            while (CACHE.size() > CACHE_SIZE) {
                String eldest = CACHE.keySet().iterator().next();
                CACHE.remove(eldest);
            }
        }
        return built;
    }

    private static String cacheKey(Map<String, String> files, List<String> classpath, String sourceLevel, List<String> owned) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-256");
            for (String uri : new TreeSet<>(files.keySet())) {
                md.update(uri.getBytes(StandardCharsets.UTF_8));
                md.update((byte) 0);
                md.update(files.get(uri).getBytes(StandardCharsets.UTF_8));
                md.update((byte) 1);
            }
            for (String c : classpath == null ? List.<String>of() : classpath) {
                md.update(c.getBytes(StandardCharsets.UTF_8));
                md.update((byte) 2);
            }
            for (String o : new TreeSet<>(owned)) {
                md.update(o.getBytes(StandardCharsets.UTF_8));
                md.update((byte) 3);
            }
            md.update(String.valueOf(sourceLevel).getBytes(StandardCharsets.UTF_8));
            md.update(new TreeMap<>(BridgeOptions.map(sourceLevel)).toString().getBytes(StandardCharsets.UTF_8));
            return HexFormat.of().formatHex(md.digest());
        } catch (Exception e) {
            return UUID.randomUUID().toString();
        }
    }

    // ─── Requests ────────────────────────────────────────────────────────────

    Object handle(Map<String, String> files, List<String> classpath, String sourceLevel, JsonObject query) {
        String op = str(query, "op");
        if (op == null) return Map.of();
        switch (op) {
            case "listTypes":
                return listTypes(query);
            default:
                break;
        }
        List<String> owned = null;
        if (query.has("owned") && query.get("owned").isJsonArray()) {
            owned = new ArrayList<>();
            for (JsonElement e : query.getAsJsonArray("owned")) owned.add(e.getAsString());
        }
        if (query.has("classFile")) {
            com.google.gson.Gson gson = new com.google.gson.Gson();
            ClassFileService.ClassFileDesc cf = ClassFileService.complete(
                gson.fromJson(query.get("classFile"), ClassFileService.ClassFileDesc.class));
            Map<String, String> attachments = new HashMap<>();
            if (query.has("sourceAttachments")) {
                query.getAsJsonObject("sourceAttachments").entrySet().forEach(e -> attachments.put(e.getKey(), e.getValue().getAsString()));
            }
            // A binary Java model AST requires an attached source buffer.
            // Decompiler text is an editor fallback, not a compilation unit.
            String source = cf == null ? null : ClassFileService.attachedSource(cf, attachments);
            if (source == null) return Map.of();
            String uri = str(query, "uri");
            if (uri == null) return Map.of();
            files = new HashMap<>(files == null ? Map.of() : files);
            files.put(uri, source);
            owned = new ArrayList<>(owned == null ? files.keySet() : owned);
            if (!owned.contains(uri)) owned.add(uri);
        }
        Index idx = index(files, classpath, sourceLevel, owned);
        return switch (op) {
            case "select" -> select(idx, str(query, "uri"), intOf(query, "line"), intOf(query, "character"));
            case "references" -> references(idx, str(query, "key"), str(query, "mode"));
            case "callees" -> callees(idx, str(query, "key"));
            case "implementations" -> implementations(idx, str(query, "key"));
            case "element" -> elementResult(idx, str(query, "key"));
            case "supertypes" -> supertypes(idx, str(query, "key"), str(query, "methodKey"), classpath);
            case "subtypes" -> subtypes(idx, str(query, "key"), str(query, "methodKey"), classpath,
                    query.has("libraries") && query.get("libraries").getAsBoolean());
            default -> Map.of("error", "unknown op " + op);
        };
    }

    // ─── select / element ────────────────────────────────────────────────────

    private Map<String, Object> select(Index idx, String uri, int line, int character) {
        Map<String, Object> out = new LinkedHashMap<>();
        FileIndex f = idx.files.get(uri);
        List<Elem> selected = new ArrayList<>();
        Elem enclosing = null;
        if (f != null) {
            int offset = f.lines.offset(line, character);
            NameOcc best = null;
            for (NameOcc n : f.names) {
                if (n.start <= offset && offset <= n.end) {
                    // prefer the identifier that contains the offset over one ending at it
                    if (best == null || (offset < n.end && best.end == offset)) best = n;
                }
            }
            if (best != null) {
                out.put("selectedKind", best.kind);
                Elem e = best.key == null ? null : idx.elements.get(best.key);
                if (e != null) selected.add(e);
            }
            for (Elem d : f.decls) {
                if (d.start <= offset && offset < d.end) {
                    if (enclosing == null || (d.start >= enclosing.start && d.end <= enclosing.end)) enclosing = d;
                }
            }
        }
        out.put("select", selected);
        out.put("enclosing", enclosing);
        if (f != null && !f.decls.isEmpty()) {
            // primary type: the first top-level type
            for (Elem d : f.decls) {
                if ("type".equals(d.kind) && d.typeChain != null && d.typeChain.size() == 1) {
                    out.put("primaryType", d);
                    break;
                }
            }
        }
        return out;
    }

    private Map<String, Object> elementResult(Index idx, String key) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("element", key == null ? null : idx.elements.get(key));
        return out;
    }

    // ─── references ──────────────────────────────────────────────────────────

    private static final int IMPOSSIBLE = 0, INACCURATE = 1, ACCURATE = 2;
    private static final int OVERRIDDEN = 0x100, SUB_INVOCATION = 0x200;

    private Map<String, Object> references(Index idx, String key, String mode) {
        Map<String, Object> out = new LinkedHashMap<>();
        List<Map<String, Object>> matches = new ArrayList<>();
        Map<String, Elem> elements = new LinkedHashMap<>();
        Elem target = key == null ? null : idx.elements.get(key);
        if (target != null) {
            boolean ctorsOf = "constructorsOf".equals(mode);
            Set<String> superNames = null;
            for (Ref r : idx.refs) {
                int level = IMPOSSIBLE;
                if (ctorsOf) {
                    if ("ctor".equals(r.kind) && key.equals(r.declClassKey)) level = ACCURATE;
                } else {
                    switch (target.kind) {
                        case "type" -> {
                            if ("type".equals(r.kind) && key.equals(r.targetKey)) level = ACCURATE;
                        }
                        case "field", "enumConstant" -> {
                            if ("field".equals(r.kind) && key.equals(r.targetKey)) level = ACCURATE;
                        }
                        case "constructor" -> {
                            if ("ctor".equals(r.kind) && key.equals(r.targetKey)) level = ACCURATE;
                        }
                        case "method" -> {
                            if (superNames == null) superNames = allSupertypes(idx, target.declaringTypeKey);
                            level = methodLevel(idx, target, r, superNames);
                        }
                        default -> {
                        }
                    }
                }
                if (level == IMPOSSIBLE) continue;
                Map<String, Object> m = new LinkedHashMap<>();
                m.put("uri", r.uri);
                m.put("range", r.range);
                m.put("accurate", level >= ACCURATE);
                m.put("javadoc", r.javadoc);
                m.put("enclosing", r.enclosing);
                m.put("enclosingKind", r.enclosingKind);
                matches.add(m);
                if (r.enclosing != null) {
                    Elem e = idx.elements.get(r.enclosing);
                    if (e != null) elements.put(e.key, e);
                }
            }
        }
        out.put("matches", matches);
        out.put("elements", elements);
        return out;
    }

    /** {@code MethodLocator.resolveLevel(MessageSend)} over the index. */
    private int methodLevel(Index idx, Elem target, Ref r, Set<String> superNames) {
        if (!("method".equals(r.kind) || "superMethod".equals(r.kind) || "methodRef".equals(r.kind))) return IMPOSSIBLE;
        if (!target.name.equals(r.selector)) return IMPOSSIBLE;
        TypeInfo declaring = idx.types.get(target.declaringTypeKey);
        List<String> targetErasures = null;
        if (declaring != null) {
            for (MethodInfo mi : declaring.methods) {
                if (target.key.equals(mi.key)) targetErasures = mi.erasures;
            }
        }
        if (targetErasures == null) {
            if (!target.key.equals(r.targetKey)) return IMPOSSIBLE;
            return ACCURATE;
        }
        if (!targetErasures.equals(r.paramErasures)) return IMPOSSIBLE;
        if (!r.virtual) {
            return target.declaringTypeKey.equals(r.declClassKey) ? ACCURATE : IMPOSSIBLE;
        }
        int level = levelAsSubtype(idx, target.declaringTypeKey, r.receiverKey, r.selector, r.paramErasures, new HashSet<>());
        if (level == IMPOSSIBLE) {
            if (r.receiverKey != null && superNames.contains(r.receiverKey)) return ACCURATE; // SUPER_INVOCATION_FLAVOR
            return IMPOSSIBLE;
        }
        return level & 0xff;
    }

    private int levelAsSubtype(Index idx, String patternKey, String typeKey, String selector, List<String> args, Set<String> seen) {
        if (typeKey == null) return INACCURATE;
        TypeInfo type = idx.types.get(typeKey);
        if (type == null || !seen.add(typeKey)) return INACCURATE;
        if (patternKey.equals(typeKey)) {
            int level = ACCURATE;
            MethodInfo m = args == null ? null : findMethod(type, selector, args);
            if (((m != null && !m.isAbstract) || !type.isAbstract) && !type.isInterface) level |= OVERRIDDEN;
            return level;
        }
        if (!type.isInterface && !"java.lang.Object".equals(type.qualifiedName) && type.superKey != null) {
            int level = levelAsSubtype(idx, patternKey, type.superKey, selector, args, seen);
            if (level != IMPOSSIBLE) {
                if (args != null) {
                    MethodInfo m = findMethod(type, selector, args);
                    if (m != null) {
                        if ((level & OVERRIDDEN) != 0) return IMPOSSIBLE;
                        if (!m.isAbstract && !type.isInterface) level |= OVERRIDDEN;
                    }
                }
                return level | SUB_INVOCATION;
            }
        }
        for (String i : type.interfaceKeys) {
            int level = levelAsSubtype(idx, patternKey, i, selector, null, seen);
            if (level != IMPOSSIBLE) {
                if (!type.isAbstract && !type.isInterface) level |= OVERRIDDEN;
                return level | SUB_INVOCATION;
            }
        }
        return IMPOSSIBLE;
    }

    private static MethodInfo findMethod(TypeInfo type, String selector, List<String> args) {
        for (MethodInfo m : type.methods) {
            if (m.selector.equals(selector) && m.erasures.equals(args)) return m;
        }
        return null;
    }

    private Set<String> allSupertypes(Index idx, String typeKey) {
        Set<String> out = new LinkedHashSet<>();
        Deque<String> work = new ArrayDeque<>();
        if (typeKey != null) work.add(typeKey);
        while (!work.isEmpty()) {
            TypeInfo t = idx.types.get(work.poll());
            if (t == null) continue;
            if (t.superKey != null && out.add(t.superKey)) work.add(t.superKey);
            for (String i : t.interfaceKeys) {
                if (out.add(i)) work.add(i);
            }
        }
        return out;
    }

    // ─── callees ─────────────────────────────────────────────────────────────

    private Map<String, Object> callees(Index idx, String key) {
        Map<String, Object> out = new LinkedHashMap<>();
        List<Map<String, Object>> calls = new ArrayList<>();
        Map<String, Elem> elements = new LinkedHashMap<>();
        Elem member = key == null ? null : idx.elements.get(key);
        if (member != null && member.uri != null) {
            if ("method".equals(member.kind) && member.declaringTypeKey != null
                    && (Modifier.isAbstract(member.flags) || isInterface(idx, member.declaringTypeKey))) {
                // CalleeAnalyzerVisitor.visit(MethodDeclaration): implementations of an abstract method
                for (Elem impl : implementationsOf(idx, member)) {
                    Map<String, Object> c = new LinkedHashMap<>();
                    c.put("callee", impl.key);
                    c.put("range", null);
                    calls.add(c);
                    elements.put(impl.key, impl);
                }
            }
            List<Ref> own = new ArrayList<>();
            for (Ref r : idx.refs) {
                if (!key.equals(r.enclosing)) continue;
                if (!("method".equals(r.kind) || "superMethod".equals(r.kind) || "methodRef".equals(r.kind) || "ctor".equals(r.kind))) continue;
                if (r.javadoc) continue;
                own.add(r);
            }
            own.sort(Comparator.comparingInt(r -> r.nodeStart));
            for (Ref r : own) {
                String calleeKey = r.targetKey;
                if ("ctor".equals(r.kind) && r.defaultCtor) calleeKey = r.declClassKey;
                Elem callee = calleeKey == null ? null : idx.elements.get(calleeKey);
                if (callee == null) continue;
                Map<String, Object> c = new LinkedHashMap<>();
                c.put("callee", callee.key);
                c.put("range", r.nodeRange);
                calls.add(c);
                elements.put(callee.key, callee);
            }
        }
        out.put("calls", calls);
        out.put("elements", elements);
        return out;
    }

    private boolean isInterface(Index idx, String typeKey) {
        TypeInfo t = idx.types.get(typeKey);
        return t != null && t.isInterface;
    }

    // ─── implementations ─────────────────────────────────────────────────────

    private Map<String, Object> implementations(Index idx, String key) {
        Map<String, Object> out = new LinkedHashMap<>();
        Elem target = key == null ? null : idx.elements.get(key);
        List<Elem> result = new ArrayList<>();
        if (target != null) {
            if ("type".equals(target.kind)) {
                result.addAll(allSourceSubtypes(idx, target.key));
            } else if ("method".equals(target.kind)) {
                result.addAll(implementationsOf(idx, target));
            }
        }
        out.put("elements", result);
        return out;
    }

    /** {@code ImplementationCollector.findMethodImplementations} for a method declaration. */
    private List<Elem> implementationsOf(Index idx, Elem method) {
        List<Elem> out = new ArrayList<>();
        int f = method.flags;
        if (Modifier.isPrivate(f) || Modifier.isStatic(f) || Modifier.isFinal(f)) return out;
        Elem declaring = method.declaringTypeKey == null ? null : idx.elements.get(method.declaringTypeKey);
        if (declaring != null && Modifier.isFinal(declaring.flags)) return out;
        if (declaring != null && "interface".equals(declaring.typeKind) && !Modifier.isAbstract(f)) {
            out.add(method); // default method: the interface itself is in the hierarchy scope
        }
        for (Elem m : idx.sourceMethods) {
            if (m.overrides != null && m.overrides.contains(method.key) && !Modifier.isAbstract(m.flags)) out.add(m);
        }
        return out;
    }

    private List<Elem> allSourceSubtypes(Index idx, String typeKey) {
        List<Elem> out = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        Deque<String> work = new ArrayDeque<>(List.of(typeKey));
        while (!work.isEmpty()) {
            String k = work.poll();
            for (Elem t : idx.sourceTypes) {
                if (isDirectSubtype(t, k) && seen.add(t.key)) {
                    out.add(t);
                    work.add(t.key);
                }
            }
        }
        return out;
    }

    private static boolean isDirectSubtype(Elem t, String superKey) {
        return superKey.equals(t.superclassKey) || (t.interfaceKeys != null && t.interfaceKeys.contains(superKey));
    }

    // ─── type hierarchy ──────────────────────────────────────────────────────

    private Map<String, Object> supertypes(Index idx, String key, String methodKey, List<String> classpath) {
        Map<String, Object> out = new LinkedHashMap<>();
        List<Map<String, Object>> items = new ArrayList<>();
        Elem type = key == null ? null : idx.elements.get(key);
        if (type != null) {
            List<String> supers = new ArrayList<>();
            if (type.interfaceKeys != null) supers.addAll(type.interfaceKeys);
            if (type.superclassKey != null) supers.add(type.superclassKey);
            Elem method = methodKey == null ? null : idx.elements.get(methodKey);
            for (String s : supers) {
                Elem e = idx.elements.get(s);
                if (e == null) continue;
                items.add(hierarchyItem(idx, e, method));
            }
        }
        out.put("items", items);
        return out;
    }

    private Map<String, Object> subtypes(Index idx, String key, String methodKey, List<String> classpath, boolean libraries) {
        Map<String, Object> out = new LinkedHashMap<>();
        List<Map<String, Object>> items = new ArrayList<>();
        Elem type = key == null ? null : idx.elements.get(key);
        Elem method = methodKey == null ? null : idx.elements.get(methodKey);
        if (type != null) {
            for (Elem t : idx.sourceTypes) {
                if (isDirectSubtype(t, key)) items.add(hierarchyItem(idx, t, method));
            }
            if (libraries && type.fqn != null) {
                for (Elem b : binarySubtypes(type.fqn, classpath, !type.fromSource && type.archive == null)) {
                    items.add(hierarchyItem(idx, b, method));
                }
            }
        }
        out.put("items", items);
        return out;
    }

    /** One hierarchy entry: the type plus, for a method hierarchy, {@code IType.findMethods(targetMethod)}. */
    private Map<String, Object> hierarchyItem(Index idx, Elem type, Elem method) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", type);
        if (method != null) {
            Elem found = null;
            for (Elem d : idx.sourceMethods) {
                if (type.key.equals(d.declaringTypeKey) && sameSignature(d, method)) {
                    found = d;
                    break;
                }
            }
            if (found == null) {
                TypeInfo ti = idx.types.get(type.key);
                if (ti != null) {
                    for (MethodInfo mi : ti.methods) {
                        Elem e = idx.elements.get(mi.key);
                        if (e != null && sameSignature(e, method)) {
                            found = e;
                            break;
                        }
                    }
                }
            }
            m.put("method", found);
        }
        return m;
    }

    /** {@code JavaModelUtil.isSameMethodSignature}: name, then simple parameter type names. */
    private static boolean sameSignature(Elem a, Elem b) {
        if (!Objects.equals(a.name, b.name) || !Objects.equals(a.kind, b.kind)) return false;
        List<String> pa = a.params == null ? List.of() : a.params;
        List<String> pb = b.params == null ? List.of() : b.params;
        if (pa.size() != pb.size()) return false;
        for (int i = 0; i < pa.size(); i++) {
            if (!erasedSimple(pa.get(i)).equals(erasedSimple(pb.get(i)))) return false;
        }
        return true;
    }

    private static String erasedSimple(String display) {
        StringBuilder sb = new StringBuilder();
        int depth = 0;
        for (char c : display.toCharArray()) {
            if (c == '<') depth++;
            else if (c == '>') depth--;
            else if (depth == 0) sb.append(c);
        }
        return sb.toString().replace("...", "[]");
    }

    // ─── Building ────────────────────────────────────────────────────────────

    private Index build(Map<String, String> files, List<String> classpath, String sourceLevel, List<String> owned) {
        Index idx = new Index();
        if (files.isEmpty() || owned.isEmpty()) return idx;
        List<String> ownedSorted = new ArrayList<>(owned);
        Collections.sort(ownedSorted);
        try (SourceMirror mirror = new SourceMirror(files)) {
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            parser.setKind(ASTParser.K_COMPILATION_UNIT);
            parser.setResolveBindings(true);
            parser.setBindingsRecovery(true);
            parser.setStatementsRecovery(true);
            parser.setCompilerOptions(BridgeOptions.map(sourceLevel));
            String[] cp = classpath == null ? new String[0] : classpath.toArray(new String[0]);
            BridgeOptions.configureEnvironment(parser, cp, new String[] { mirror.root.toString() });
            Map<String, String> pathToUri = new HashMap<>();
            List<String> paths = new ArrayList<>();
            for (String uri : ownedSorted) {
                Path p = mirror.pathOf(uri);
                if (p == null) continue;
                pathToUri.put(p.toString(), uri);
                paths.add(p.toString());
            }
            String[] encodings = new String[paths.size()];
            Arrays.fill(encodings, "UTF-8");
            Map<String, FileIndex> byUri = new HashMap<>();
            parser.createASTs(paths.toArray(new String[0]), encodings, new String[0], new FileASTRequestor() {
                @Override
                public void acceptAST(String sourceFilePath, CompilationUnit ast) {
                    String uri = pathToUri.get(sourceFilePath);
                    if (uri == null) uri = pathToUri.get(Path.of(sourceFilePath).toString());
                    if (uri == null) return;
                    try {
                        byUri.put(uri, new Indexer(idx, uri, files.get(uri), ast).run());
                    } catch (RuntimeException e) {
                        LOG.warning("semantic index: failed to index " + uri + ": " + e);
                    }
                }
            }, null);
            for (String uri : ownedSorted) {
                FileIndex f = byUri.get(uri);
                if (f != null) idx.files.put(uri, f);
            }
        }
        return idx;
    }

    /** Visits one resolved unit and records its declarations and references. */
    private final class Indexer extends ASTVisitor {
        final Index idx;
        final String uri;
        final String source;
        final CompilationUnit cu;
        final FileIndex file = new FileIndex();
        final Deque<Elem> stack = new ArrayDeque<>();
        final Map<String, Integer> occurrences = new HashMap<>();
        final String packageName;
        boolean inImport;
        boolean inPackage;
        int javadocDepth;

        Indexer(Index idx, String uri, String source, CompilationUnit cu) {
            super(true);
            this.idx = idx;
            this.uri = uri;
            this.source = source;
            this.cu = cu;
            this.packageName = cu.getPackage() == null ? "" : cu.getPackage().getName().getFullyQualifiedName();
        }

        FileIndex run() {
            file.uri = uri;
            file.lines = lineInfo(source);
            cu.accept(this);
            return file;
        }

        // ── positions ──

        Rng rng(int start, int end) {
            Rng r = new Rng();
            int[] s = lc(start), e = lc(end);
            r.sl = s[0]; r.sc = s[1]; r.el = e[0]; r.ec = e[1];
            return r;
        }

        int[] lc(int offset) {
            int[] starts = file.lines.lineStarts;
            int lo = 0, hi = starts.length - 1;
            while (lo < hi) {
                int mid = (lo + hi + 1) >>> 1;
                if (starts[mid] <= offset) lo = mid; else hi = mid - 1;
            }
            return new int[] { lo, offset - starts[lo] };
        }

        // ── declarations ──

        private Elem declare(Elem e, ASTNode node, SimpleName name) {
            e.fromSource = !uri.startsWith("jdt:");
            if (!e.fromSource) locateBinary(e);
            e.uri = uri;
            e.start = node.getStartPosition();
            e.end = node.getStartPosition() + node.getLength();
            e.range = rng(e.start, e.end);
            if (name != null) {
                e.nameRange = rng(name.getStartPosition(), name.getStartPosition() + name.getLength());
            }
            Elem parent = stack.peek();
            List<String> chain = new ArrayList<>();
            Elem t = "type".equals(e.kind) ? parent : parent;
            if (parent != null && parent.typeChain != null) chain.addAll(parent.typeChain);
            if (parent != null && !"type".equals(parent.kind)) {
                // member inside a member (local/anonymous type): keep the enclosing chain
            }
            if ("type".equals(e.kind)) chain.add(e.anonymous ? "" : e.name);
            e.typeChain = chain;
            String occKey = (parent == null ? "" : parent.key) + "|" + e.kind + "|" + e.name + "|" + e.paramSigs;
            int occ = occurrences.merge(occKey, 1, Integer::sum);
            e.occurrence = occ;
            if (e.packageName == null) e.packageName = packageName;
            idx.elements.put(e.key, e);
            file.decls.add(e);
            if ("type".equals(e.kind)) idx.sourceTypes.add(e);
            if ("method".equals(e.kind)) idx.sourceMethods.add(e);
            if (name != null) addName(name, e.key, e.kind);
            return e;
        }

        private void addName(SimpleName name, String key, String kind) {
            NameOcc n = new NameOcc();
            n.start = name.getStartPosition();
            n.end = n.start + name.getLength();
            n.key = key;
            n.kind = kind;
            file.names.add(n);
        }

        private boolean typeDecl(AbstractTypeDeclaration node, ITypeBinding b) {
            if (b == null) return true;
            Elem e = describeType(b);
            e.implicitType = node instanceof ImplicitTypeDeclaration;
            declare(e, node, node instanceof ImplicitTypeDeclaration ? null : node.getName());
            if (e.implicitType) e.nameRange = e.range;
            ensureTypeInfo(b);
            stack.push(e);
            return true;
        }

        @Override public boolean visit(TypeDeclaration node) { return typeDecl(node, node.resolveBinding()); }
        @Override public boolean visit(EnumDeclaration node) { return typeDecl(node, node.resolveBinding()); }
        @Override public boolean visit(AnnotationTypeDeclaration node) { return typeDecl(node, node.resolveBinding()); }
        @Override public boolean visit(RecordDeclaration node) { return typeDecl(node, node.resolveBinding()); }
        @Override public boolean visit(ImplicitTypeDeclaration node) { return typeDecl(node, node.resolveBinding()); }
        @Override public void endVisit(TypeDeclaration node) { popIf(node.resolveBinding()); }
        @Override public void endVisit(EnumDeclaration node) { popIf(node.resolveBinding()); }
        @Override public void endVisit(AnnotationTypeDeclaration node) { popIf(node.resolveBinding()); }
        @Override public void endVisit(RecordDeclaration node) { popIf(node.resolveBinding()); }
        @Override public void endVisit(ImplicitTypeDeclaration node) { popIf(node.resolveBinding()); }

        private void popIf(IBinding b) {
            if (b != null && !stack.isEmpty()) stack.pop();
        }

        @Override
        public boolean visit(AnonymousClassDeclaration node) {
            ITypeBinding b = node.resolveBinding();
            if (b == null) return true;
            Elem e = describeType(b);
            declare(e, node, null);
            e.nameRange = e.range;
            ensureTypeInfo(b);
            stack.push(e);
            return true;
        }

        @Override
        public void endVisit(AnonymousClassDeclaration node) {
            popIf(node.resolveBinding());
        }

        @Override
        public boolean visit(MethodDeclaration node) {
            IMethodBinding b = node.resolveBinding();
            if (b == null) return true;
            Elem e = describeMethod(b);
            List<String> sigs = new ArrayList<>();
            for (Object o : node.parameters()) {
                SingleVariableDeclaration p = (SingleVariableDeclaration) o;
                String text = typeText(p.getType());
                for (int i = 0; i < p.getExtraDimensions(); i++) text += "[]";
                if (p.isVarargs()) text += "[]";
                sigs.add(Signature.createTypeSignature(text, false));
            }
            if (!uri.startsWith("jdt:")) e.paramSigs = sigs;
            declare(e, node, node.getName());
            e.overrides = overridden(b);
            Elem owner = idx.elements.get(e.declaringTypeKey);
            if (b.isConstructor() && owner != null) {
                if (owner.constructorKeys == null) owner.constructorKeys = new ArrayList<>();
                owner.constructorKeys.add(e.key);
            }
            stack.push(e);
            return true;
        }

        @Override
        public void endVisit(MethodDeclaration node) {
            popIf(node.resolveBinding());
        }

        @Override
        public boolean visit(AnnotationTypeMemberDeclaration node) {
            IMethodBinding b = node.resolveBinding();
            if (b == null) return true;
            Elem e = describeMethod(b);
            e.paramSigs = List.of();
            declare(e, node, node.getName());
            stack.push(e);
            return true;
        }

        @Override
        public void endVisit(AnnotationTypeMemberDeclaration node) {
            popIf(node.resolveBinding());
        }

        @Override
        public boolean visit(Initializer node) {
            Elem parent = stack.peek();
            Elem e = new Elem();
            e.kind = "initializer";
            e.name = "";
            e.flags = node.getModifiers();
            if (parent != null) {
                e.declaringTypeKey = parent.key;
                e.declaringTypeFqn = parent.fqn;
            }
            int n = occurrences.merge((parent == null ? "" : parent.key) + "|init", 1, Integer::sum);
            e.key = (parent == null ? uri : parent.key) + "|init#" + n;
            declare(e, node, null);
            e.occurrence = n;
            e.nameRange = e.range;
            stack.push(e);
            return true;
        }

        @Override
        public void endVisit(Initializer node) {
            if (!stack.isEmpty()) stack.pop();
        }

        @Override
        public boolean visit(VariableDeclarationFragment node) {
            if (node.getParent() instanceof FieldDeclaration fd) {
                IVariableBinding b = node.resolveBinding();
                if (b != null) {
                    Elem e = describeField(b);
                    // source range of a field: the declaration for its first fragment, else the fragment
                    declare(e, fd.fragments().get(0) == node ? fd : node, node.getName());
                    if (fd.fragments().get(0) != node) {
                        e.start = node.getStartPosition();
                        e.end = node.getStartPosition() + node.getLength();
                        e.range = rng(e.start, e.end);
                    }
                    stack.push(e);
                    return true;
                }
            } else {
                IVariableBinding b = node.resolveBinding();
                if (b != null) addName(node.getName(), null, "local");
            }
            return true;
        }

        @Override
        public void endVisit(VariableDeclarationFragment node) {
            if (node.getParent() instanceof FieldDeclaration && node.resolveBinding() != null && !stack.isEmpty()) stack.pop();
        }

        @Override
        public boolean visit(EnumConstantDeclaration node) {
            IVariableBinding b = node.resolveVariable();
            if (b == null) return true;
            Elem e = describeField(b);
            declare(e, node, node.getName());
            stack.push(e);
            IMethodBinding ctor = node.resolveConstructorBinding();
            if (ctor != null) {
                addRef("ctor", ctor, node.getName().getStartPosition(), node.getStartPosition() + node.getLength(), node);
            }
            return true;
        }

        @Override
        public void endVisit(EnumConstantDeclaration node) {
            if (node.resolveVariable() != null && !stack.isEmpty()) stack.pop();
        }

        @Override
        public boolean visit(SingleVariableDeclaration node) {
            addName(node.getName(), null, "local");
            return true;
        }

        @Override public boolean visit(ImportDeclaration node) { inImport = true; return true; }
        @Override public void endVisit(ImportDeclaration node) { inImport = false; }
        @Override public boolean visit(PackageDeclaration node) { inPackage = true; return true; }
        @Override public void endVisit(PackageDeclaration node) { inPackage = false; }
        @Override public boolean visit(Javadoc node) { javadocDepth++; return true; }
        @Override public void endVisit(Javadoc node) { javadocDepth--; }

        // ── references ──

        @Override
        public boolean visit(SimpleName node) {
            if (isDeclarationName(node)) return true;
            IBinding b = node.resolveBinding();
            if (b == null) return true;
            switch (b.getKind()) {
                case IBinding.TYPE -> {
                    ITypeBinding t = (ITypeBinding) b;
                    if (t.isTypeVariable() || t.isPrimitive() || t.isNullType()) return true;
                    if (t.isArray()) t = t.getElementType();
                    ITypeBinding decl = t.getTypeDeclaration();
                    if (decl == null) return true;
                    ensureElement(decl);
                    String target = decl.getKey();
                    String selectKey = target;
                    String selectKind = "type";
                    // `new Foo()`: codeSelect on the type name selects the constructor
                    ASTNode p = node.getParent();
                    if (p instanceof SimpleType st && st.getParent() instanceof ClassInstanceCreation cic && cic.getType() == st) {
                        IMethodBinding ctor = cic.resolveConstructorBinding();
                        if (ctor != null && !ctor.isDefaultConstructor() && ctor.getDeclaringClass() != null
                                && !ctor.getDeclaringClass().isAnonymous()) {
                            ensureElement(ctor.getMethodDeclaration());
                            selectKey = ctor.getMethodDeclaration().getKey();
                            selectKind = "constructor";
                        }
                    }
                    addName(node, selectKey, selectKind);
                    Ref r = baseRef("type", node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
                    r.targetKey = target;
                    idx.refs.add(r);
                }
                case IBinding.VARIABLE -> {
                    IVariableBinding v = (IVariableBinding) b;
                    if (!v.isField()) {
                        addName(node, null, "local");
                        return true;
                    }
                    IVariableBinding decl = v.getVariableDeclaration();
                    ensureElement(decl);
                    addName(node, decl.getKey(), v.isEnumConstant() ? "enumConstant" : "field");
                    Ref r = baseRef("field", node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
                    r.targetKey = decl.getKey();
                    idx.refs.add(r);
                }
                case IBinding.METHOD -> {
                    IMethodBinding m = (IMethodBinding) b;
                    IMethodBinding decl = m.getMethodDeclaration();
                    ensureElement(decl);
                    addName(node, decl.getKey(), decl.isConstructor() ? "constructor" : "method");
                }
                default -> addName(node, null, "other");
            }
            return true;
        }

        private boolean isDeclarationName(SimpleName node) {
            ASTNode p = node.getParent();
            if (p instanceof AbstractTypeDeclaration t && t.getName() == node) return true;
            if (p instanceof MethodDeclaration m && m.getName() == node) return true;
            if (p instanceof AnnotationTypeMemberDeclaration a && a.getName() == node) return true;
            if (p instanceof VariableDeclaration v && v.getName() == node) return true;
            if (p instanceof EnumConstantDeclaration e && e.getName() == node) return true;
            return false;
        }

        @Override
        public boolean visit(MethodInvocation node) {
            IMethodBinding b = node.resolveMethodBinding();
            if (b != null) {
                Ref r = addRef("method", b, node.getName().getStartPosition(), node.getStartPosition() + node.getLength(), node);
                ITypeBinding receiver;
                Expression ex = node.getExpression();
                if (ex != null) {
                    receiver = ex.resolveTypeBinding();
                } else {
                    receiver = b.getDeclaringClass();
                    // implicit this: the enclosing type that declares or inherits the method
                    ITypeBinding enclosingType = enclosingTypeBinding(node);
                    while (enclosingType != null && !inherits(enclosingType, b)) enclosingType = enclosingType.getDeclaringClass();
                    if (enclosingType != null) receiver = enclosingType;
                }
                setReceiver(r, b, receiver);
            }
            return true;
        }

        @Override
        public boolean visit(SuperMethodInvocation node) {
            IMethodBinding b = node.resolveMethodBinding();
            if (b != null) {
                Ref r = addRef("superMethod", b, node.getName().getStartPosition(), node.getStartPosition() + node.getLength(), node);
                r.virtual = false;
            }
            return true;
        }

        @Override
        public boolean visit(ExpressionMethodReference node) {
            methodReference(node, node.resolveMethodBinding(), node.getName(), node.getExpression().resolveTypeBinding());
            return true;
        }

        @Override
        public boolean visit(TypeMethodReference node) {
            methodReference(node, node.resolveMethodBinding(), node.getName(), node.getType().resolveBinding());
            return true;
        }

        @Override
        public boolean visit(SuperMethodReference node) {
            IMethodBinding b = node.resolveMethodBinding();
            if (b != null) {
                Ref r = addRef("methodRef", b, node.getName().getStartPosition(), node.getStartPosition() + node.getLength(), node);
                r.virtual = false;
            }
            return true;
        }

        @Override
        public boolean visit(CreationReference node) {
            IMethodBinding b = node.resolveMethodBinding();
            if (b != null && b.isConstructor()) {
                addRef("ctor", b, node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
            }
            return true;
        }

        private void methodReference(ASTNode node, IMethodBinding b, SimpleName name, ITypeBinding receiver) {
            if (b == null) return;
            Ref r = addRef("methodRef", b, name.getStartPosition(), node.getStartPosition() + node.getLength(), node);
            setReceiver(r, b, receiver);
        }

        @Override
        public boolean visit(MethodRef node) {
            IBinding b = node.resolveBinding();
            if (b instanceof IMethodBinding m) {
                Ref r = addRef(m.isConstructor() ? "ctor" : "method", m, node.getName().getStartPosition(), node.getStartPosition() + node.getLength(), node);
                r.virtual = false;
            }
            return true;
        }

        @Override
        public boolean visit(ClassInstanceCreation node) {
            IMethodBinding b = node.resolveConstructorBinding();
            if (b != null) {
                IMethodBinding target = b;
                if (b.getDeclaringClass() != null && b.getDeclaringClass().isAnonymous()) {
                    // the anonymous class' constructor delegates to the super constructor
                    ITypeBinding sup = b.getDeclaringClass().getSuperclass();
                    IMethodBinding superCtor = sup == null ? null : findConstructor(sup, b);
                    if (superCtor != null) target = superCtor;
                }
                addRef("ctor", target, node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
            }
            return true;
        }

        @Override
        public boolean visit(ConstructorInvocation node) {
            IMethodBinding b = node.resolveConstructorBinding();
            if (b != null) addRef("ctor", b, node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
            return true;
        }

        @Override
        public boolean visit(SuperConstructorInvocation node) {
            IMethodBinding b = node.resolveConstructorBinding();
            if (b != null) addRef("ctor", b, node.getStartPosition(), node.getStartPosition() + node.getLength(), node);
            return true;
        }

        private IMethodBinding findConstructor(ITypeBinding type, IMethodBinding like) {
            for (IMethodBinding m : type.getDeclaredMethods()) {
                if (m.isConstructor() && m.getParameterTypes().length == like.getParameterTypes().length) return m;
            }
            return null;
        }

        private Ref baseRef(String kind, int start, int end, ASTNode node) {
            Ref r = new Ref();
            r.kind = kind;
            r.uri = uri;
            r.range = rng(start, end);
            r.nodeStart = node.getStartPosition();
            r.nodeRange = rng(node.getStartPosition(), node.getStartPosition() + node.getLength());
            r.javadoc = javadocDepth > 0;
            if (inImport) {
                r.enclosingKind = "import";
            } else if (inPackage) {
                r.enclosingKind = "package";
            } else {
                Elem e = stack.peek();
                r.enclosing = e == null ? null : e.key;
                r.enclosingKind = e == null ? "unit" : "member";
            }
            return r;
        }

        private Ref addRef(String kind, IMethodBinding b, int start, int end, ASTNode node) {
            IMethodBinding decl = b.getMethodDeclaration();
            ensureElement(decl);
            Ref r = baseRef(kind, start, end, node);
            r.targetKey = decl.getKey();
            r.selector = decl.getName();
            r.paramErasures = erasures(decl);
            ITypeBinding dc = decl.getDeclaringClass();
            r.declClassKey = dc == null ? null : typeKey(dc);
            r.defaultCtor = decl.isDefaultConstructor();
            int mods = decl.getModifiers();
            r.virtual = !decl.isConstructor() && !Modifier.isStatic(mods) && !Modifier.isPrivate(mods);
            if (dc != null) ensureTypeInfo(dc);
            idx.refs.add(r);
            return r;
        }

        private void setReceiver(Ref r, IMethodBinding b, ITypeBinding receiver) {
            if (receiver == null) return;
            if (receiver.isTypeVariable() || receiver.isCapture() || receiver.isWildcardType()) receiver = receiver.getErasure();
            if (receiver.isArray()) return;
            r.receiverKey = typeKey(receiver);
            ensureTypeInfo(receiver);
        }

        private ITypeBinding enclosingTypeBinding(ASTNode node) {
            for (ASTNode n = node.getParent(); n != null; n = n.getParent()) {
                if (n instanceof AbstractTypeDeclaration t) return t.resolveBinding();
                if (n instanceof AnonymousClassDeclaration a) return a.resolveBinding();
            }
            return null;
        }

        private boolean inherits(ITypeBinding type, IMethodBinding m) {
            ITypeBinding declaring = m.getDeclaringClass();
            if (declaring == null) return false;
            return type.getTypeDeclaration().isSubTypeCompatible(declaring.getTypeDeclaration())
                    || type.getErasure().isSubTypeCompatible(declaring.getErasure());
        }

        private String typeText(Type t) {
            return source.substring(t.getStartPosition(), t.getStartPosition() + t.getLength()).replaceAll("\\s+", "");
        }

        // ── element descriptors ──

        Elem ensureElement(IBinding b) {
            if (b == null) return null;
            String key = b instanceof ITypeBinding t ? typeKey(t) : b.getKey();
            if (key == null) return null;
            Elem e = idx.elements.get(key);
            if (e != null) return e;
            if (b instanceof ITypeBinding t) {
                e = describeType(t.getTypeDeclaration());
            } else if (b instanceof IMethodBinding m) {
                e = describeMethod(m);
            } else if (b instanceof IVariableBinding v) {
                e = describeField(v);
            } else {
                return null;
            }
            if (!idx.elements.containsKey(e.key)) {
                locateBinary(e);
                idx.elements.put(e.key, e);
            }
            return idx.elements.get(e.key);
        }

        Elem describeType(ITypeBinding b) {
            ITypeBinding t = b.getTypeDeclaration();
            Elem e = new Elem();
            e.key = typeKey(t);
            e.kind = "type";
            e.name = t.isAnonymous() ? "" : t.getName().replaceAll("<.*", "");
            e.typeKind = t.isAnnotation() ? "annotation" : t.isInterface() ? "interface" : t.isEnum() ? "enum"
                    : t.isRecord() ? "record" : "class";
            e.flags = t.getModifiers() | (t.isInterface() ? ACC_INTERFACE : 0) | (t.isAnnotation() ? ACC_ANNOTATION : 0)
                    | (t.isEnum() ? ACC_ENUM : 0);
            e.deprecated = t.isDeprecated();
            e.anonymous = t.isAnonymous();
            e.local = t.isLocal();
            e.fqn = t.getBinaryName();
            if (e.fqn == null) e.fqn = t.getQualifiedName();
            e.packageName = t.getPackage() == null ? "" : t.getPackage().getName();
            ITypeBinding declaring = t.getDeclaringClass();
            if (declaring != null) {
                e.declaringTypeKey = typeKey(declaring);
                e.declaringTypeFqn = binaryName(declaring);
            }
            List<String> tps = new ArrayList<>();
            for (ITypeBinding tp : t.getTypeParameters()) tps.add(typeParameterLabel(tp));
            e.typeParams = tps;
            if (t.getSuperclass() != null) e.superclassKey = typeKey(t.getSuperclass());
            else if (t.isInterface() && !t.isFromSource()) e.superclassKey = null;
            List<String> ifs = new ArrayList<>();
            for (ITypeBinding i : t.getInterfaces()) ifs.add(typeKey(i));
            e.interfaceKeys = ifs;
            if (t.isInterface() && e.superclassKey == null) {
                // JDT type hierarchies have no superclass for interfaces
                e.superclassKey = null;
            }
            List<String> chain = new ArrayList<>();
            for (ITypeBinding c = t; c != null; c = c.getDeclaringClass()) chain.add(0, c.isAnonymous() ? "" : c.getName().replaceAll("<.*", ""));
            e.typeChain = chain;
            e.fromSource = t.isFromSource();
            for (ITypeBinding s = t.getSuperclass(); s != null; s = null) ensureLight(s);
            for (ITypeBinding i : t.getInterfaces()) ensureLight(i);
            return e;
        }

        /** Make sure supertypes have descriptors (for type hierarchies). */
        private void ensureLight(ITypeBinding s) {
            ITypeBinding d = s.getTypeDeclaration();
            String k = typeKey(d);
            if (idx.elements.containsKey(k)) return;
            Elem e = new Elem();
            e.key = k;
            e.kind = "type";
            e.name = d.getName().replaceAll("<.*", "");
            e.typeKind = d.isAnnotation() ? "annotation" : d.isInterface() ? "interface" : d.isEnum() ? "enum"
                    : d.isRecord() ? "record" : "class";
            e.flags = d.getModifiers() | (d.isInterface() ? ACC_INTERFACE : 0);
            e.deprecated = d.isDeprecated();
            e.fqn = binaryName(d);
            e.packageName = d.getPackage() == null ? "" : d.getPackage().getName();
            List<String> chain = new ArrayList<>();
            for (ITypeBinding c = d; c != null; c = c.getDeclaringClass()) chain.add(0, c.getName().replaceAll("<.*", ""));
            e.typeChain = chain;
            e.fromSource = d.isFromSource();
            if (d.getSuperclass() != null) e.superclassKey = typeKey(d.getSuperclass());
            List<String> ifs = new ArrayList<>();
            for (ITypeBinding i : d.getInterfaces()) ifs.add(typeKey(i));
            e.interfaceKeys = ifs;
            List<String> tps = new ArrayList<>();
            for (ITypeBinding tp : d.getTypeParameters()) tps.add(typeParameterLabel(tp));
            e.typeParams = tps;
            if (d.isFromSource()) {
                // the declaration will be indexed with its source location if it is one of our units
                idx.elements.put(k, e);
            } else {
                locateBinary(e);
                idx.elements.put(k, e);
            }
            ensureTypeInfo(d);
        }

        Elem describeMethod(IMethodBinding b) {
            IMethodBinding m = b.getMethodDeclaration();
            Elem e = new Elem();
            e.key = m.getKey();
            e.kind = m.isConstructor() ? "constructor" : "method";
            e.name = m.getName();
            e.flags = m.getModifiers();
            e.deprecated = m.isDeprecated();
            e.varargs = m.isVarargs();
            e.defaultConstructor = m.isDefaultConstructor();
            ITypeBinding dc = m.getDeclaringClass();
            if (dc != null) {
                e.declaringTypeKey = typeKey(dc);
                e.declaringTypeFqn = binaryName(dc);
                e.packageName = dc.getPackage() == null ? "" : dc.getPackage().getName();
                e.fromSource = dc.isFromSource();
                List<String> chain = new ArrayList<>();
                for (ITypeBinding c = dc.getTypeDeclaration(); c != null; c = c.getDeclaringClass()) chain.add(0, c.isAnonymous() ? "" : c.getName().replaceAll("<.*", ""));
                e.typeChain = chain;
                ensureLight(dc);
            }
            List<String> params = new ArrayList<>();
            List<String> sigs = new ArrayList<>();
            for (ITypeBinding p : m.getParameterTypes()) {
                params.add(p.getName());
                sigs.add(binarySignature(p));
            }
            e.params = params;
            e.paramSigs = sigs;
            e.returnType = m.isConstructor() ? null : m.getReturnType().getName();
            List<String> tps = new ArrayList<>();
            for (ITypeBinding tp : m.getTypeParameters()) tps.add(typeParameterLabel(tp));
            e.typeParams = tps;
            return e;
        }

        Elem describeField(IVariableBinding b) {
            IVariableBinding v = b.getVariableDeclaration();
            Elem e = new Elem();
            e.key = v.getKey();
            e.kind = v.isEnumConstant() ? "enumConstant" : "field";
            e.name = v.getName();
            e.flags = v.getModifiers();
            e.deprecated = v.isDeprecated();
            ITypeBinding dc = v.getDeclaringClass();
            if (dc != null) {
                e.declaringTypeKey = typeKey(dc);
                e.declaringTypeFqn = binaryName(dc);
                e.packageName = dc.getPackage() == null ? "" : dc.getPackage().getName();
                e.fromSource = dc.isFromSource();
                List<String> chain = new ArrayList<>();
                for (ITypeBinding c = dc.getTypeDeclaration(); c != null; c = c.getDeclaringClass()) chain.add(0, c.isAnonymous() ? "" : c.getName().replaceAll("<.*", ""));
                e.typeChain = chain;
            }
            return e;
        }

        private List<String> overridden(IMethodBinding m) {
            List<String> out = new ArrayList<>();
            if (m.isConstructor() || Modifier.isStatic(m.getModifiers()) || Modifier.isPrivate(m.getModifiers())) return out;
            ITypeBinding dc = m.getDeclaringClass();
            if (dc == null) return out;
            Set<String> seen = new HashSet<>();
            Deque<ITypeBinding> work = new ArrayDeque<>();
            if (dc.getSuperclass() != null) work.add(dc.getSuperclass());
            for (ITypeBinding i : dc.getInterfaces()) work.add(i);
            if (dc.isInterface()) {
                // interfaces implicitly declare Object's public methods
            }
            while (!work.isEmpty()) {
                ITypeBinding t = work.poll();
                if (!seen.add(t.getErasure().getKey())) continue;
                for (IMethodBinding sm : t.getDeclaredMethods()) {
                    if (m.overrides(sm)) {
                        IMethodBinding d = sm.getMethodDeclaration();
                        ensureElement(d);
                        out.add(d.getKey());
                    }
                }
                if (t.getSuperclass() != null) work.add(t.getSuperclass());
                for (ITypeBinding i : t.getInterfaces()) work.add(i);
            }
            return out;
        }

        void ensureTypeInfo(ITypeBinding b) {
            Deque<ITypeBinding> work = new ArrayDeque<>(List.of(b));
            while (!work.isEmpty()) {
                ITypeBinding t = work.poll().getTypeDeclaration();
                String k = typeKey(t);
                if (k == null || idx.types.containsKey(k)) continue;
                TypeInfo ti = new TypeInfo();
                ti.key = k;
                ti.qualifiedName = t.getErasure().getQualifiedName();
                ti.isInterface = t.isInterface();
                ti.isAbstract = Modifier.isAbstract(t.getModifiers());
                if (t.getSuperclass() != null) {
                    ti.superKey = typeKey(t.getSuperclass());
                    work.add(t.getSuperclass());
                }
                for (ITypeBinding i : t.getInterfaces()) {
                    ti.interfaceKeys.add(typeKey(i));
                    work.add(i);
                }
                for (IMethodBinding m : t.getDeclaredMethods()) {
                    MethodInfo mi = new MethodInfo();
                    mi.selector = m.getName();
                    mi.erasures = erasures(m);
                    mi.isAbstract = Modifier.isAbstract(m.getModifiers());
                    mi.key = m.getMethodDeclaration().getKey();
                    ti.methods.add(mi);
                }
                idx.types.put(k, ti);
            }
        }
    }

    // ─── binding helpers ─────────────────────────────────────────────────────

    static String typeKey(ITypeBinding t) {
        if (t == null) return null;
        ITypeBinding d = t.getTypeDeclaration();
        if (d == null) d = t;
        if (d.isArray()) d = d.getElementType().getTypeDeclaration();
        return d.getErasure() == null ? d.getKey() : d.getErasure().getTypeDeclaration().getKey();
    }

    static String binaryName(ITypeBinding t) {
        ITypeBinding d = t.getTypeDeclaration();
        String n = d.getBinaryName();
        return n != null ? n : d.getErasure().getQualifiedName();
    }

    static List<String> erasures(IMethodBinding m) {
        List<String> out = new ArrayList<>();
        for (ITypeBinding p : m.getMethodDeclaration().getParameterTypes()) {
            ITypeBinding e = p.getErasure();
            out.add(e == null ? p.getQualifiedName() : e.getQualifiedName());
        }
        return out;
    }

    static String typeParameterLabel(ITypeBinding tp) {
        StringBuilder sb = new StringBuilder(tp.getName());
        ITypeBinding[] bounds = tp.getTypeBounds();
        List<String> names = new ArrayList<>();
        for (ITypeBinding b : bounds) {
            if (!"java.lang.Object".equals(b.getErasure().getQualifiedName())) names.add(b.getName());
        }
        if (!names.isEmpty()) sb.append(" extends ").append(String.join(" & ", names));
        return sb.toString();
    }

    /** Resolved JDT model signature with dots ({@code BinaryMethod} parameter types). */
    static String binarySignature(ITypeBinding p) {
        if (p.isArray()) {
            return "[".repeat(p.getDimensions()) + binarySignature(p.getElementType());
        }
        if (p.isPrimitive()) {
            return switch (p.getName()) {
                case "int" -> "I"; case "long" -> "J"; case "short" -> "S"; case "byte" -> "B";
                case "char" -> "C"; case "float" -> "F"; case "double" -> "D"; case "boolean" -> "Z";
                default -> "V";
            };
        }
        if (p.isTypeVariable()) return "T" + p.getName() + ";";
        ITypeBinding e = p.getErasure();
        String bn = e.getBinaryName() != null ? e.getBinaryName() : e.getQualifiedName();
        return "L" + bn + ";";
    }

    static CompilationUnitInfo lineInfo(String source) {
        List<Integer> starts = new ArrayList<>();
        starts.add(0);
        for (int i = 0; i < source.length(); i++) {
            char c = source.charAt(i);
            if (c == '\r') {
                if (i + 1 < source.length() && source.charAt(i + 1) == '\n') i++;
                starts.add(i + 1);
            } else if (c == '\n') {
                starts.add(i + 1);
            }
        }
        CompilationUnitInfo info = new CompilationUnitInfo();
        info.lineStarts = starts.stream().mapToInt(Integer::intValue).toArray();
        info.length = source.length();
        return info;
    }

    // ─── binary locations ────────────────────────────────────────────────────

    /** Classpath archives seen by the last requests (to locate binary types). */
    private static final ThreadLocal<List<String>> CLASSPATH = ThreadLocal.withInitial(List::of);

    private void locateBinary(Elem e) {
        if (e.fromSource) return;
        String fqn = e.kind.equals("type") ? e.fqn : e.declaringTypeFqn;
        if (fqn == null) return;
        String top = fqn.contains("$") ? fqn.substring(0, fqn.indexOf('$')) : fqn;
        String classFileType = e.kind.equals("type") ? fqn : e.declaringTypeFqn;
        String pkg = e.packageName == null ? "" : e.packageName;
        String simple = classFileType.substring(pkg.isEmpty() ? 0 : pkg.length() + 1);
        e.classFile = simple + ".class";
        String entry = (pkg.isEmpty() ? "" : pkg.replace('.', '/') + "/") + top.substring(pkg.isEmpty() ? 0 : pkg.length() + 1) + ".class";
        for (String archive : CLASSPATH.get()) {
            if (archiveEntries(archive).contains(entry)) {
                e.archive = archive;
                return;
            }
        }
        String module = jdkModuleOf(pkg);
        if (module != null) {
            e.module = module;
            e.archive = jrtFsJar();
        }
    }

    private static final Map<String, Set<String>> ARCHIVE_ENTRIES = new HashMap<>();

    private static Set<String> archiveEntries(String archive) {
        synchronized (ARCHIVE_ENTRIES) {
            Set<String> s = ARCHIVE_ENTRIES.get(archive);
            if (s != null) return s;
        }
        Set<String> entries = new HashSet<>();
        Path p = Path.of(archive);
        if (Files.isDirectory(p)) {
            try (Stream<Path> walk = Files.walk(p)) {
                walk.filter(f -> f.toString().endsWith(".class")).forEach(f -> entries.add(p.relativize(f).toString().replace('\\', '/')));
            } catch (IOException ignored) {
            }
        } else if (Files.isRegularFile(p)) {
            try (ZipFile z = new ZipFile(p.toFile())) {
                z.stream().forEach(en -> entries.add(en.getName()));
            } catch (IOException ignored) {
            }
        }
        synchronized (ARCHIVE_ENTRIES) {
            ARCHIVE_ENTRIES.put(archive, entries);
        }
        return entries;
    }

    private static Map<String, String> PACKAGE_TO_MODULE;

    private static synchronized String jdkModuleOf(String pkg) {
        if (PACKAGE_TO_MODULE == null) {
            PACKAGE_TO_MODULE = new HashMap<>();
            try {
                FileSystem jrt = FileSystems.getFileSystem(URI.create("jrt:/"));
                try (Stream<Path> mods = Files.list(jrt.getPath("/modules"))) {
                    for (Path mod : (Iterable<Path>) mods::iterator) {
                        String module = mod.getFileName().toString().replace("/", "");
                        try (Stream<Path> walk = Files.walk(mod)) {
                            walk.filter(Files::isDirectory).forEach(d -> {
                                String rel = mod.relativize(d).toString().replace('/', '.');
                                if (!rel.isEmpty()) PACKAGE_TO_MODULE.putIfAbsent(rel, module);
                            });
                        }
                    }
                }
            } catch (Exception e) {
                LOG.warning("semantic index: cannot list jrt packages: " + e);
            }
        }
        return PACKAGE_TO_MODULE.get(pkg);
    }

    static String jrtFsJar() {
        return Path.of(System.getProperty("java.home"), "lib", "jrt-fs.jar").toString();
    }

    // ─── listTypes (workspace symbol index of binaries) ──────────────────────

    /** Per-archive cache: archive path → (stamp, types). */
    private static final Map<String, Object[]> TYPE_LISTS = new HashMap<>();
    private static List<List<Object>> JDK_TYPES;
    private static Map<String, List<List<Object>>> JDK_TYPES_BY_MODULE;

    private Object listTypes(JsonObject query) {
        Map<String, Object> out = new LinkedHashMap<>();
        List<Map<String, Object>> archives = new ArrayList<>();
        if (query.has("archives")) {
            for (JsonElement a : query.getAsJsonArray("archives")) {
                String path = a.getAsString();
                Map<String, Object> m = new LinkedHashMap<>();
                m.put("path", path);
                m.put("types", archiveTypes(path));
                archives.add(m);
            }
        }
        out.put("archives", archives);
        if (query.has("jdk") && query.get("jdk").getAsBoolean()) {
            Map<String, Object> jdk = new LinkedHashMap<>();
            jdk.put("home", System.getProperty("java.home"));
            jdk.put("jrtFs", jrtFsJar());
            List<Map<String, Object>> mods = new ArrayList<>();
            for (Map.Entry<String, List<List<Object>>> e : jdkTypes().entrySet()) {
                Map<String, Object> m = new LinkedHashMap<>();
                m.put("module", e.getKey());
                m.put("types", e.getValue());
                mods.add(m);
            }
            jdk.put("modules", mods);
            out.put("jdk", jdk);
        }
        return out;
    }

    /** [packageName, binaryNameWithinPackage, modifiers, superclass, interfaces...] per class. */
    private static List<List<Object>> archiveTypes(String archive) {
        Path p = Path.of(archive);
        long stamp;
        try {
            stamp = Files.getLastModifiedTime(p).toMillis() ^ Files.size(p);
        } catch (IOException e) {
            return List.of();
        }
        synchronized (TYPE_LISTS) {
            Object[] c = TYPE_LISTS.get(archive);
            if (c != null && (long) c[0] == stamp) {
                @SuppressWarnings("unchecked")
                List<List<Object>> l = (List<List<Object>>) c[1];
                return l;
            }
        }
        List<List<Object>> types = new ArrayList<>();
        if (RuntimeImage.isImage(archive)) {
            // A selected VM contributes its module image, not the classes in
            // the jrt-fs provider jar. Return only class-file facts to Rust.
            try {
                FileSystem image = RuntimeImage.open(archive);
                try (Stream<Path> modules = Files.list(image.getPath("/modules"))) {
                    for (Path module : (Iterable<Path>) modules::iterator) {
                        try (Stream<Path> walk = Files.walk(module)) {
                            for (Path file : (Iterable<Path>) walk::iterator) {
                                String name = module.relativize(file).toString();
                                if (!name.endsWith(".class")) continue;
                                try {
                                    List<Object> type = readType(Files.readAllBytes(file), name);
                                    if (type != null) {
                                        type.set(5, module.getFileName().toString());
                                        types.add(type);
                                    }
                                } catch (Exception ignored) {
                                }
                            }
                        }
                    }
                }
            } catch (IOException e) {
                LOG.warning("listTypes: cannot read VM image " + archive + ": " + e);
            }
        } else if (Files.isRegularFile(p)) {
            try (ZipFile z = new ZipFile(p.toFile())) {
                Enumeration<? extends ZipEntry> en = z.entries();
                while (en.hasMoreElements()) {
                    ZipEntry entry = en.nextElement();
                    String name = entry.getName();
                    if (!name.endsWith(".class") || name.startsWith("META-INF/")) continue;
                    try (InputStream in = z.getInputStream(entry)) {
                        List<Object> t = readType(in.readAllBytes(), name);
                        if (t != null) types.add(t);
                    } catch (Exception ignored) {
                    }
                }
            } catch (IOException e) {
                LOG.warning("listTypes: cannot read " + archive + ": " + e);
            }
        } else if (Files.isDirectory(p)) {
            try (Stream<Path> walk = Files.walk(p)) {
                for (Path f : (Iterable<Path>) walk::iterator) {
                    if (!f.toString().endsWith(".class")) continue;
                    try {
                        List<Object> t = readType(Files.readAllBytes(f), p.relativize(f).toString().replace('\\', '/'));
                        if (t != null) types.add(t);
                    } catch (Exception ignored) {
                    }
                }
            } catch (IOException ignored) {
            }
        }
        synchronized (TYPE_LISTS) {
            TYPE_LISTS.put(archive, new Object[] { stamp, types });
        }
        return types;
    }

    private static synchronized Map<String, List<List<Object>>> jdkTypes() {
        if (JDK_TYPES_BY_MODULE != null) return JDK_TYPES_BY_MODULE;
        Map<String, List<List<Object>>> byModule = new TreeMap<>();
        try {
            FileSystem jrt = FileSystems.getFileSystem(URI.create("jrt:/"));
            try (Stream<Path> mods = Files.list(jrt.getPath("/modules"))) {
                for (Path mod : (Iterable<Path>) mods::iterator) {
                    String module = mod.getFileName().toString().replace("/", "");
                    List<List<Object>> types = new ArrayList<>();
                    try (Stream<Path> walk = Files.walk(mod)) {
                        for (Path f : (Iterable<Path>) walk::iterator) {
                            String rel = mod.relativize(f).toString();
                            if (!rel.endsWith(".class")) continue;
                            try {
                                List<Object> t = readType(Files.readAllBytes(f), rel);
                                if (t != null) types.add(t);
                            } catch (Exception ignored) {
                            }
                        }
                    }
                    byModule.put(module, types);
                }
            }
        } catch (Exception e) {
            LOG.warning("listTypes: cannot read jrt: " + e);
        }
        JDK_TYPES_BY_MODULE = byModule;
        return byModule;
    }

    /** Class header, module (filled by VM image listings), and SourceFile attribute. */
    private static List<Object> readType(byte[] bytes, String entryName) throws Exception {
        if (entryName.endsWith("module-info.class") || entryName.endsWith("package-info.class")) return null;
        ClassFileReader r = new ClassFileReader(bytes, entryName.toCharArray());
        if (r.isAnonymous() || r.isLocal()) return null;
        String name = new String(r.getName()); // a/b/C$D
        int slash = name.lastIndexOf('/');
        String pkg = slash < 0 ? "" : name.substring(0, slash).replace('/', '.');
        String simple = name.substring(slash + 1);
        List<Object> t = new ArrayList<>(7);
        t.add(pkg);
        t.add(simple);
        t.add(r.getModifiers());
        t.add(r.getSuperclassName() == null ? null : new String(r.getSuperclassName()).replace('/', '.'));
        List<String> ifs = new ArrayList<>();
        char[][] interfaces = r.getInterfaceNames();
        if (interfaces != null) for (char[] i : interfaces) ifs.add(new String(i).replace('/', '.'));
        t.add(ifs);
        t.add(null);
        t.add(ClassFileService.sourceFileName(bytes));
        return t;
    }

    /** Binary types on the classpath (and the JDK when asked) whose direct supertype is {@code fqn}. */
    private List<Elem> binarySubtypes(String fqn, List<String> classpath, boolean includeJdk) {
        List<Elem> out = new ArrayList<>();
        String dotted = fqn;
        for (String archive : classpath == null ? List.<String>of() : classpath) {
            for (List<Object> t : archiveTypes(archive)) {
                Elem e = binarySubtype(t, dotted, archive, null);
                if (e != null) out.add(e);
            }
        }
        if (includeJdk) {
            for (Map.Entry<String, List<List<Object>>> m : jdkTypes().entrySet()) {
                for (List<Object> t : m.getValue()) {
                    Elem e = binarySubtype(t, dotted, jrtFsJar(), m.getKey());
                    if (e != null) out.add(e);
                }
            }
        }
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Elem binarySubtype(List<Object> t, String superFqn, String archive, String module) {
        String sup = (String) t.get(3);
        List<String> ifs = (List<String>) t.get(4);
        boolean match = superFqn.equals(sup) || ifs.contains(superFqn);
        if (!match) return null;
        String pkg = (String) t.get(0);
        String simple = (String) t.get(1);
        int mods = (Integer) t.get(2);
        Elem e = new Elem();
        e.fqn = pkg.isEmpty() ? simple : pkg + "." + simple;
        e.key = "L" + e.fqn.replace('.', '/') + ";";
        e.kind = "type";
        e.name = simple.substring(simple.lastIndexOf('$') + 1);
        e.typeKind = (mods & ACC_ANNOTATION) != 0 ? "annotation" : (mods & ACC_INTERFACE) != 0 ? "interface"
                : (mods & ACC_ENUM) != 0 ? "enum" : "class";
        e.flags = mods;
        e.deprecated = (mods & ACC_DEPRECATED) != 0;
        e.packageName = pkg;
        e.typeChain = Arrays.asList(simple.split("\\$"));
        e.classFile = simple + ".class";
        e.archive = archive;
        e.module = module != null ? module : t.size() > 5 ? (String) t.get(5) : null;
        e.superclassKey = sup == null ? null : "L" + sup.replace('.', '/') + ";";
        List<String> ik = new ArrayList<>();
        for (String i : ifs) ik.add("L" + i.replace('.', '/') + ";");
        e.interfaceKeys = ik;
        return e;
    }

    // ─── entry ───────────────────────────────────────────────────────────────

    Object request(Map<String, String> files, List<String> classpath, String sourceLevel, JsonObject query) {
        CLASSPATH.set(classpath == null ? List.of() : classpath);
        try {
            return handle(files, classpath, sourceLevel, query == null ? new JsonObject() : query);
        } finally {
            CLASSPATH.remove();
        }
    }

    private static String str(JsonObject o, String k) {
        return o.has(k) && !o.get(k).isJsonNull() ? o.get(k).getAsString() : null;
    }

    private static int intOf(JsonObject o, String k) {
        return o.has(k) && !o.get(k).isJsonNull() ? o.get(k).getAsInt() : 0;
    }

    @SuppressWarnings("unused")
    private static List<String> strings(JsonArray a) {
        List<String> out = new ArrayList<>();
        for (JsonElement e : a) out.add(e.getAsString());
        return out;
    }

    // ─── mirror ──────────────────────────────────────────────────────────────

    /**
     * Temporary copy of the request's sources laid out by package, so that the
     * compiler's source path finds the types of files that are not on disk.
     */
    private static final class SourceMirror implements AutoCloseable {
        final Path root;
        private final Path base;
        private final Map<String, Path> paths = new HashMap<>();

        SourceMirror(Map<String, String> files) {
            Path b;
            try {
                b = Files.createTempDirectory("jdtls-index");
            } catch (IOException e) {
                throw new IllegalStateException("cannot create index mirror", e);
            }
            base = b;
            root = b.resolve("src");
            int dup = 0;
            for (Map.Entry<String, String> entry : new TreeMap<>(files).entrySet()) {
                String source = entry.getValue();
                String pkg = packageOf(source);
                String fileName = fileName(entry.getKey());
                Path dir = root;
                if (!pkg.isEmpty()) {
                    for (String seg : pkg.split("\\.")) dir = dir.resolve(seg);
                }
                Path target = dir.resolve(fileName);
                if (Files.exists(target)) target = b.resolve("dup" + (dup++)).resolve(fileName);
                try {
                    Files.createDirectories(target.getParent());
                    Files.writeString(target, source, StandardCharsets.UTF_8);
                    paths.put(entry.getKey(), target);
                } catch (IOException e) {
                    LOG.warning("semantic index: cannot mirror " + entry.getKey() + ": " + e);
                }
            }
        }

        Path pathOf(String uri) {
            return paths.get(uri);
        }

        private static String fileName(String uri) {
            String path;
            try {
                path = new URI(uri).getPath();
            } catch (Exception e) {
                path = null;
            }
            if (path == null || path.isEmpty()) path = uri;
            String name = path.substring(Math.max(path.lastIndexOf('/'), path.lastIndexOf(':')) + 1);
            if (name.isEmpty()) name = "Untitled";
            if (!name.endsWith(".java")) name = name + ".java";
            return name;
        }

        private static String packageOf(String source) {
            IScanner scanner = ToolFactory.createScanner(false, false, false, false);
            scanner.setSource(source.toCharArray());
            StringBuilder sb = new StringBuilder();
            boolean inPackage = false;
            try {
                int depth = 0;
                while (true) {
                    int tok = scanner.getNextToken();
                    if (tok == ITerminalSymbols.TokenNameEOF) break;
                    if (!inPackage) {
                        if (tok == ITerminalSymbols.TokenNameAT) continue;
                        if (tok == ITerminalSymbols.TokenNameLPAREN) { depth++; continue; }
                        if (tok == ITerminalSymbols.TokenNameRPAREN) { depth--; continue; }
                        if (depth > 0) continue;
                        if (tok == ITerminalSymbols.TokenNamepackage) { inPackage = true; continue; }
                        if (tok == ITerminalSymbols.TokenNameIdentifier || tok == ITerminalSymbols.TokenNameDOT) continue;
                        break;
                    }
                    if (tok == ITerminalSymbols.TokenNameSEMICOLON) return sb.toString();
                    if (tok == ITerminalSymbols.TokenNameIdentifier || tok == ITerminalSymbols.TokenNameDOT) {
                        sb.append(scanner.getCurrentTokenSource());
                    } else {
                        break;
                    }
                }
            } catch (Exception e) {
                // fall through
            }
            return inPackage ? sb.toString() : "";
        }

        @Override
        public void close() {
            try (Stream<Path> walk = Files.walk(base)) {
                walk.sorted(Comparator.reverseOrder()).forEach(p -> {
                    try {
                        Files.deleteIfExists(p);
                    } catch (IOException ignored) {
                    }
                });
            } catch (IOException ignored) {
            }
        }
    }
}
