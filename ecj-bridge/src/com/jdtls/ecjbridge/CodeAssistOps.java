package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.logging.Level;
import java.util.logging.Logger;

import org.eclipse.jdt.core.Signature;
import org.eclipse.jdt.core.dom.AST;
import org.eclipse.jdt.core.dom.ASTNode;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.ASTVisitor;
import org.eclipse.jdt.core.dom.AbstractTypeDeclaration;
import org.eclipse.jdt.core.dom.AnnotationTypeMemberDeclaration;
import org.eclipse.jdt.core.dom.BodyDeclaration;
import org.eclipse.jdt.core.dom.CompilationUnit;
import org.eclipse.jdt.core.dom.EnumConstantDeclaration;
import org.eclipse.jdt.core.dom.Expression;
import org.eclipse.jdt.core.dom.FieldDeclaration;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.IVariableBinding;
import org.eclipse.jdt.core.dom.Javadoc;
import org.eclipse.jdt.core.dom.MethodDeclaration;
import org.eclipse.jdt.core.dom.Modifier;
import org.eclipse.jdt.core.dom.PrimitiveType;
import org.eclipse.jdt.core.dom.RecordDeclaration;
import org.eclipse.jdt.core.dom.SingleVariableDeclaration;
import org.eclipse.jdt.core.dom.StringLiteral;
import org.eclipse.jdt.core.dom.Type;
import org.eclipse.jdt.core.dom.TypeDeclaration;
import org.eclipse.jdt.core.dom.TypeParameter;
import org.eclipse.jdt.core.dom.VariableDeclarationFragment;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/**
 * Secondary code-assist operations: data jdt.ls reads from the Java model
 * (or computes with JDT DOM rewrites) while converting completion
 * proposals.  All LSP shaping happens in Rust.
 */
final class CodeAssistOps {

    private static final Logger LOG = Logger.getLogger(CodeAssistOps.class.getName());

    private CodeAssistOps() {}

    static Object handle(String op, JsonObject q, Map<String, String> files, List<String> classpath, String level,
            String uri, int offset) {
        switch (op) {
            case "packageTypes":
                return packageTypes(q, files, classpath, level, uri);
            case "templateScope":
                return new CodeAssistService().templateScope(files, classpath, level, uri, offset,
                        new HashSet<>(CodeAssistService.jsonStrings(q, "testUris")),
                        q.has("contextOffset") ? q.get("contextOffset").getAsInt() : offset,
                        q.has("unitPackage") && !q.get("unitPackage").isJsonNull() ? q.get("unitPackage").getAsString() : null);
            case "javadocTarget":
                return javadocTarget(files, classpath, level, uri, offset);
            case "memberDoc":
                return memberDoc(q, files, classpath, level, uri);
            case "overrideStubs":
                return OverrideStubs.compute(q, files, classpath, level, uri);
            default:
                throw new IllegalArgumentException("Unknown codeAssist op: " + op);
        }
    }

    // ── packageTypes: simple names of the types in packages ─────────────────

    private static Object packageTypes(JsonObject q, Map<String, String> files, List<String> classpath, String level,
            String uri) {
        Map<String, Set<String>> out = new LinkedHashMap<>();
        Set<String> wanted = new HashSet<>(CodeAssistService.jsonStrings(q, "packages"));
        for (String p : wanted) out.put(p, new TreeSet<>());
        TypeIndex index = new TypeIndex(files, Set.of(), classpath, level, null, false);
        for (TypeIndex.TypeInfo t : index.sourceTypes) {
            if (wanted.contains(t.packageName) && t.enclosingNames.length == 0) {
                out.get(t.packageName).add(t.simpleName);
            }
        }
        for (TypeIndex.BinaryRoot r : index.binaryRoots) {
            for (String bin : r.binaryNames) {
                int slash = bin.lastIndexOf('/');
                String pkg = slash < 0 ? "" : bin.substring(0, slash).replace('/', '.');
                if (!wanted.contains(pkg)) continue;
                String local = bin.substring(slash + 1);
                if (local.indexOf('$') >= 0) continue;
                out.get(pkg).add(local);
            }
        }
        return out;
    }

    // ── DOM parsing ──────────────────────────────────────────────────────────

    static CompilationUnit parse(String source, String uri, Map<String, String> files, List<String> classpath,
            String level, boolean bindings) {
        try {
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            parser.setSource(source.toCharArray());
            parser.setKind(ASTParser.K_COMPILATION_UNIT);
            parser.setCompilerOptions(BridgeOptions.map(level));
            parser.setStatementsRecovery(true);
            if (bindings) {
                parser.setResolveBindings(true);
                parser.setBindingsRecovery(true);
                parser.setUnitName(InMemorySourceClasspath.fileName(uri));
                BridgeOptions.configureEnvironment(parser, classpath == null ? new String[0] : classpath.toArray(new String[0]));
                SignatureHelpService.injectSources(parser, new InMemorySourceClasspath(files, uri));
            }
            return (CompilationUnit) parser.createAST(null);
        } catch (RuntimeException e) {
            LOG.log(Level.WARNING, "code assist parse failed", e);
            return null;
        }
    }

    // ── javadocTarget: the member a "/**" comment documents ──────────────────

    static final class JavadocTarget {
        public String kind;
        public int nameOffset = -1;
        public boolean inherited;
        public boolean constructor;
        public boolean returnVoid;
        public List<String> typeParams = new ArrayList<>();
        public List<String> params = new ArrayList<>();
        public List<String> exceptions = new ArrayList<>();
        public String typeQualifiedName;
        public boolean record;
        public List<String> recordComponents = new ArrayList<>();
    }

    private static Object javadocTarget(Map<String, String> files, List<String> classpath, String level, String uri,
            int offset) {
        JavadocTarget r = new JavadocTarget();
        String source = files.get(uri);
        if (source == null) return r;
        CompilationUnit cu = parse(source, uri, files, classpath, level, true);
        if (cu == null) return r;
        // ICompilationUnit.getElementAt(offset): the innermost member containing offset.
        BodyDeclaration[] found = new BodyDeclaration[1];
        cu.accept(new ASTVisitor() {
            @Override
            public void preVisit(ASTNode node) {
                if (node instanceof BodyDeclaration bd && contains(bd, offset)) {
                    found[0] = bd;
                }
            }
        });
        BodyDeclaration el = found[0];
        if (el instanceof AbstractTypeDeclaration t) {
            r.kind = "type";
            r.nameOffset = t.getName().getStartPosition();
            r.typeQualifiedName = typeQualifiedName(t);
            if (t instanceof RecordDeclaration rd && !versionLess(level, "14")) {
                r.record = true;
                for (Object o : rd.recordComponents()) {
                    if (o instanceof SingleVariableDeclaration v) r.recordComponents.add(v.getName().getIdentifier());
                }
            } else if (t instanceof TypeDeclaration td) {
                for (Object o : td.typeParameters()) r.typeParams.add(((TypeParameter) o).getName().getIdentifier());
            }
        } else if (el instanceof MethodDeclaration m) {
            r.kind = "method";
            r.nameOffset = m.getName().getStartPosition();
            r.constructor = m.isConstructor();
            Type rt = m.getReturnType2();
            r.returnVoid = rt instanceof PrimitiveType p && p.getPrimitiveTypeCode() == PrimitiveType.VOID;
            for (Object o : m.typeParameters()) r.typeParams.add(((TypeParameter) o).getName().getIdentifier());
            for (Object o : m.parameters()) r.params.add(((SingleVariableDeclaration) o).getName().getIdentifier());
            for (Object o : m.thrownExceptionTypes()) r.exceptions.add(o.toString());
            IMethodBinding mb = m.resolveBinding();
            r.inherited = mb != null && overridesSomething(mb);
        }
        return r;
    }

    private static boolean versionLess(String level, String v) {
        String a = BridgeOptions.version(level);
        String b = BridgeOptions.version(v);
        return org.eclipse.jdt.internal.compiler.impl.CompilerOptions.versionToJdkLevel(a)
                < org.eclipse.jdt.internal.compiler.impl.CompilerOptions.versionToJdkLevel(b);
    }

    private static boolean contains(ASTNode n, int offset) {
        return n.getStartPosition() <= offset && offset < n.getStartPosition() + n.getLength();
    }

    private static String typeQualifiedName(AbstractTypeDeclaration t) {
        StringBuilder sb = new StringBuilder(t.getName().getIdentifier());
        ASTNode p = t.getParent();
        while (p != null) {
            if (p instanceof AbstractTypeDeclaration pt) sb.insert(0, pt.getName().getIdentifier() + ".");
            p = p.getParent();
        }
        return sb.toString();
    }

    /** {@code MethodOverrideTester.findOverriddenMethod(method, true) != null}. */
    private static boolean overridesSomething(IMethodBinding m) {
        ITypeBinding declaring = m.getDeclaringClass();
        if (declaring == null || m.isConstructor() || Modifier.isPrivate(m.getModifiers()) || Modifier.isStatic(m.getModifiers())) {
            return false;
        }
        Set<String> seen = new HashSet<>();
        List<ITypeBinding> todo = new ArrayList<>();
        if (declaring.getSuperclass() != null) todo.add(declaring.getSuperclass());
        for (ITypeBinding i : declaring.getInterfaces()) todo.add(i);
        while (!todo.isEmpty()) {
            ITypeBinding t = todo.remove(0);
            if (!seen.add(t.getErasure().getKey())) continue;
            for (IMethodBinding cand : t.getDeclaredMethods()) {
                if (m.overrides(cand)) return true;
            }
            if (t.getSuperclass() != null) todo.add(t.getSuperclass());
            for (ITypeBinding i : t.getInterfaces()) todo.add(i);
        }
        return false;
    }

    // ── memberDoc: Javadoc / constant / default value of a member ────────────

    static final class MemberDoc {
        public boolean found;
        public String javadoc;
        public String constantValue;
        public String defaultValue;
        public String uri;
    }

    private static Object memberDoc(JsonObject q, Map<String, String> files, List<String> classpath, String level,
            String uri) {
        MemberDoc r = new MemberDoc();
        String type = q.has("type") ? q.get("type").getAsString() : null;
        if (type == null) return r;
        InMemorySourceClasspath sources = new InMemorySourceClasspath(files, null);
        // Top-level type, possibly followed by member type names.
        String[] parts = type.split("\\.");
        String typeUri = null;
        int top = -1;
        for (int i = parts.length; i >= 1 && typeUri == null; i--) {
            String candidate = String.join(".", java.util.Arrays.copyOf(parts, i));
            typeUri = sources.uriOf(candidate);
            if (typeUri != null) top = i;
        }
        if (typeUri == null) {
            // Binary member: exists, but has no source Javadoc without an attachment.
            r.found = binaryMemberExists(q, files, classpath, level, uri, type);
            return r;
        }
        String text = files.get(typeUri);
        CompilationUnit cu = parse(text, typeUri, files, classpath, level, true);
        if (cu == null) return r;
        AbstractTypeDeclaration td = null;
        for (Object o : cu.types()) {
            AbstractTypeDeclaration t = (AbstractTypeDeclaration) o;
            if (t.getName().getIdentifier().equals(parts[top - 1])) td = t;
        }
        for (int i = top; i < parts.length && td != null; i++) {
            AbstractTypeDeclaration next = null;
            for (Object o : td.bodyDeclarations()) {
                if (o instanceof AbstractTypeDeclaration m && m.getName().getIdentifier().equals(parts[i])) next = m;
            }
            td = next;
        }
        if (td == null) return r;
        r.uri = typeUri;
        if (q.has("module")) return r;
        if (!q.has("name")) {
            r.found = true;
            r.javadoc = javadocText(text, td.getJavadoc());
            return r;
        }
        String name = q.get("name").getAsString();
        List<String> params = CodeAssistService.jsonStrings(q, "params");
        boolean hasBinding = q.has("hasBinding") && q.get("hasBinding").getAsBoolean();
        // IType.getMethod(name, params) + findMethods, else IType.getField(name)
        MethodDeclaration method = null;
        AnnotationTypeMemberDeclaration annotationMember = null;
        for (Object o : td.bodyDeclarations()) {
            if (o instanceof MethodDeclaration m && m.getName().getIdentifier().equals(name)
                    && sameParameters(m, params)) {
                method = m;
                break;
            }
            if (o instanceof AnnotationTypeMemberDeclaration a && a.getName().getIdentifier().equals(name) && params.isEmpty()) {
                annotationMember = a;
                break;
            }
        }
        if (method != null) {
            r.found = true;
            r.javadoc = javadocText(text, method.getJavadoc());
            return r;
        }
        if (annotationMember != null) {
            r.found = true;
            r.javadoc = javadocText(text, annotationMember.getJavadoc());
            Expression d = annotationMember.getDefault();
            if (d != null) r.defaultValue = annotationValue(d);
            return r;
        }
        for (Object o : td.bodyDeclarations()) {
            if (o instanceof FieldDeclaration f) {
                for (Object fo : f.fragments()) {
                    VariableDeclarationFragment frag = (VariableDeclarationFragment) fo;
                    if (frag.getName().getIdentifier().equals(name)) {
                        r.found = true;
                        r.javadoc = javadocText(text, f.getJavadoc());
                        int mods = f.getModifiers();
                        boolean interfaceField = td instanceof TypeDeclaration t2 && t2.isInterface();
                        if ((Modifier.isStatic(mods) && Modifier.isFinal(mods)) || interfaceField) {
                            IVariableBinding vb = frag.resolveBinding();
                            Object c = vb == null ? null : vb.getConstantValue();
                            if (c != null) r.constantValue = formatConstant(c);
                        }
                        return r;
                    }
                }
            }
            if (o instanceof org.eclipse.jdt.core.dom.EnumDeclaration) continue;
        }
        if (td instanceof org.eclipse.jdt.core.dom.EnumDeclaration ed) {
            for (Object o : ed.enumConstants()) {
                EnumConstantDeclaration ec = (EnumConstantDeclaration) o;
                if (ec.getName().getIdentifier().equals(name)) {
                    r.found = true;
                    r.javadoc = javadocText(text, ec.getJavadoc());
                    return r;
                }
            }
        }
        if (td instanceof RecordDeclaration rd) {
            for (Object o : rd.recordComponents()) {
                SingleVariableDeclaration v = (SingleVariableDeclaration) o;
                if (v.getName().getIdentifier().equals(name)) {
                    r.found = true;
                    return r;
                }
            }
        }
        if (!hasBinding) {
            // nothing else
        }
        return r;
    }

    private static boolean binaryMemberExists(JsonObject q, Map<String, String> files, List<String> classpath,
            String level, String uri, String type) {
        CodeAssistEnvironment env = CodeAssistEnvironment.create(files, Set.of(), classpath, level, null, false);
        org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer a = env.findType(
                org.eclipse.jdt.core.compiler.CharOperation.splitOn('.', type.toCharArray()), (char[]) null);
        return a != null;
    }

    private static boolean sameParameters(MethodDeclaration m, List<String> params) {
        List<?> ps = m.parameters();
        if (ps.size() != params.size()) return false;
        for (int i = 0; i < ps.size(); i++) {
            SingleVariableDeclaration v = (SingleVariableDeclaration) ps.get(i);
            String declared = v.getType().toString();
            int lt = declared.indexOf('<');
            if (lt >= 0) declared = declared.substring(0, lt);
            if (v.isVarargs()) declared = declared + "[]";
            declared = simple(declared);
            String wanted;
            try {
                wanted = simple(Signature.toString(Signature.getTypeErasure(params.get(i))));
            } catch (IllegalArgumentException e) {
                return false;
            }
            if (!declared.equals(wanted)) return false;
        }
        return true;
    }

    private static String simple(String name) {
        int dims = 0;
        while (name.endsWith("[]")) {
            dims++;
            name = name.substring(0, name.length() - 2);
        }
        name = name.trim();
        int dot = name.lastIndexOf('.');
        StringBuilder sb = new StringBuilder(dot < 0 ? name : name.substring(dot + 1));
        for (int i = 0; i < dims; i++) sb.append("[]");
        return sb.toString();
    }

    private static String javadocText(String source, Javadoc doc) {
        if (doc == null) return null;
        return source.substring(doc.getStartPosition(), doc.getStartPosition() + doc.getLength());
    }

    private static String formatConstant(Object c) {
        if (c instanceof String s) {
            StringLiteral l = AST.newAST(AST.getJLSLatest(), false).newStringLiteral();
            l.setLiteralValue(s);
            return l.getEscapedValue();
        } else if (c instanceof Character) {
            return "'" + c + "'";
        }
        return c.toString();
    }

    private static String annotationValue(Expression e) {
        if (e instanceof StringLiteral s) {
            return "\"" + s.getLiteralValue() + "\"";
        }
        return e.toString();
    }

    @SuppressWarnings("unused")
    private static List<String> strings(JsonArray a) {
        List<String> out = new ArrayList<>();
        if (a != null) for (JsonElement e : a) out.add(e.getAsString());
        return out;
    }
}
