package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.eclipse.jdt.core.Signature;
import org.eclipse.jdt.core.dom.ASTNode;
import org.eclipse.jdt.core.dom.AbstractTypeDeclaration;
import org.eclipse.jdt.core.dom.AnonymousClassDeclaration;
import org.eclipse.jdt.core.dom.CompilationUnit;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.ImportDeclaration;
import org.eclipse.jdt.core.dom.Modifier;
import org.eclipse.jdt.core.dom.NodeFinder;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/**
 * Override method stubs for {@code METHOD_DECLARATION} proposals, following
 * jdt.ls {@code OverrideCompletionProposal} and JDT's
 * {@code StubUtility2Core.createImplementationStubCore} (with jdt.ls'
 * default code templates).  Returns the stub text (relative indentation,
 * '\n' delimiters later converted by Rust) and the qualified names of the
 * types the stub refers to, in the order JDT adds their imports.
 */
final class OverrideStubs {

    private OverrideStubs() {}

    static final class Stub {
        public String text;
        public List<String> imports = new ArrayList<>();
    }

    static Object compute(JsonObject q, Map<String, String> files, List<String> classpath, String level, String uri) {
        Map<String, Stub> out = new LinkedHashMap<>();
        String source = files.get(uri);
        if (source == null || !q.has("methods")) return out;
        boolean snippets = q.has("snippets") && q.get("snippets").getAsBoolean();
        String indent = indentUnit();
        CompilationUnit cu = CodeAssistOps.parse(source, uri, files, classpath, level, true);
        if (cu == null) return out;
        for (JsonElement e : q.getAsJsonArray("methods")) {
            JsonObject m = e.getAsJsonObject();
            String key = m.get("key").getAsString();
            try {
                Stub s = stub(cu, m.get("name").getAsString(), m.get("signature").getAsString(),
                        m.get("replaceStart").getAsInt(), snippets, indent, level);
                if (s != null) out.put(key, s);
            } catch (RuntimeException ex) {
                // no stub: jdt.ls falls back to "<completion> {};"
            }
        }
        return out;
    }

    private static String indentUnit() {
        Map<String, String> o = BridgeOptions.map("21");
        String ch = o.getOrDefault("org.eclipse.jdt.core.formatter.tabulation.char", "tab");
        int size = 4;
        try {
            size = Integer.parseInt(o.getOrDefault("org.eclipse.jdt.core.formatter.tabulation.size", "4"));
        } catch (NumberFormatException e) {
            size = 4;
        }
        if ("space".equals(ch)) return " ".repeat(size);
        if ("mixed".equals(ch)) {
            int indentSize = 4;
            try {
                indentSize = Integer.parseInt(o.getOrDefault("org.eclipse.jdt.core.formatter.indentation.size", "4"));
            } catch (NumberFormatException ex) {
                indentSize = 4;
            }
            return indentSize >= size ? "\t" + " ".repeat(indentSize - size) : " ".repeat(indentSize);
        }
        return "\t";
    }

    private static Stub stub(CompilationUnit cu, String name, String signature, int offset, boolean snippets,
            String indent, String level) {
        ASTNode node = NodeFinder.perform(cu, offset, 1);
        while (node != null && !(node instanceof AbstractTypeDeclaration) && !(node instanceof AnonymousClassDeclaration)) {
            node = node.getParent();
        }
        ITypeBinding declaringType = null;
        if (node instanceof AnonymousClassDeclaration a) declaringType = a.resolveBinding();
        else if (node instanceof AbstractTypeDeclaration t) declaringType = t.resolveBinding();
        if (declaringType == null) return null;
        String[] paramTypes = Signature.getParameterTypes(signature);
        for (int i = 0; i < paramTypes.length; i++) paramTypes[i] = Signature.toString(paramTypes[i]);
        IMethodBinding toOverride = findMethodInHierarchy(declaringType, name, paramTypes);
        if (toOverride == null && declaringType.isInterface()) {
            ITypeBinding object = cu.getAST().resolveWellKnownType("java.lang.Object");
            toOverride = findMethodInType(object, name, paramTypes);
        }
        if (toOverride == null) return null;

        boolean inInterface = declaringType.isInterface();
        boolean useAlternativeMethodBody = !declaringType.isInterface();
        Imports imports = new Imports(cu);
        Stub s = new Stub();
        StringBuilder decl = new StringBuilder();

        int modifiers = toOverride.getModifiers();
        ITypeBinding methodDeclaring = toOverride.getDeclaringClass();
        ITypeBinding object = cu.getAST().resolveWellKnownType("java.lang.Object");
        boolean skipOverride = inInterface && methodDeclaring == object && !Modifier.isPublic(modifiers);
        if (!skipOverride && overrideAnnotation(methodDeclaring.isInterface())) {
            decl.append("@Override\n");
        }
        int mods = modifiers;
        if (inInterface) {
            mods = mods & ~Modifier.PROTECTED & ~Modifier.PUBLIC;
            if (Modifier.isAbstract(mods)) mods = mods | Modifier.DEFAULT;
        } else {
            mods = mods & ~Modifier.DEFAULT;
        }
        mods = mods & ~Modifier.ABSTRACT & ~Modifier.NATIVE & ~Modifier.PRIVATE;
        if (!inInterface && methodDeclaring.isInterface() && !Modifier.isPublic(mods)) {
            // interface methods are implicitly public
            mods |= Modifier.PUBLIC;
        }
        String modString = modifiers(mods);
        if (!modString.isEmpty()) decl.append(modString).append(' ');
        ITypeBinding[] typeParams = toOverride.getTypeParameters();
        if (typeParams.length > 0) {
            decl.append('<');
            for (int i = 0; i < typeParams.length; i++) {
                if (i > 0) decl.append(", ");
                decl.append(typeParams[i].getName());
                ITypeBinding[] bounds = typeParams[i].getTypeBounds();
                if (bounds.length != 1 || !"java.lang.Object".equals(bounds[0].getQualifiedName())) {
                    for (int b = 0; b < bounds.length; b++) {
                        decl.append(b == 0 ? " extends " : " & ").append(imports.add(bounds[b], s));
                    }
                }
            }
            decl.append("> ");
        }
        ITypeBinding returnType = toOverride.getReturnType();
        String returnTypeName = imports.add(returnType, s);
        decl.append(returnTypeName).append(' ').append(toOverride.getName()).append('(');
        ITypeBinding[] params = toOverride.getParameterTypes();
        String[] names = parameterNames(toOverride);
        for (int i = 0; i < params.length; i++) {
            if (i > 0) decl.append(", ");
            ITypeBinding t = params[i];
            if (toOverride.isVarargs() && t.isArray() && i == params.length - 1) {
                decl.append(imports.add(t.getComponentType(), s)).append("...");
            } else {
                decl.append(imports.add(t, s));
            }
            decl.append(' ').append(names[i]);
        }
        decl.append(')');
        ITypeBinding[] exceptions = toOverride.getExceptionTypes();
        if (exceptions.length > 0) {
            decl.append(" throws ");
            for (int i = 0; i < exceptions.length; i++) {
                if (i > 0) decl.append(", ");
                decl.append(imports.add(exceptions[i], s));
            }
        }
        decl.append(" {\n");
        // body
        if (!inInterface || methodDeclaring != object) {
            String bodyStatement = "";
            if (Modifier.isAbstract(modifiers)) {
                String def = defaultValue(returnType);
                if (def != null) bodyStatement = "return " + def + ";";
            } else {
                StringBuilder inv = new StringBuilder();
                if (methodDeclaring.isInterface()) {
                    ITypeBinding supertype = findImmediateSuperTypeInHierarchy(declaringType, methodDeclaring.getTypeDeclaration().getQualifiedName());
                    if (supertype == null) supertype = methodDeclaring;
                    if (supertype.isInterface()) {
                        inv.append(imports.add(supertype.getTypeDeclaration(), s)).append('.');
                    }
                }
                inv.append("super.").append(toOverride.getName()).append('(');
                for (int i = 0; i < names.length; i++) {
                    if (i > 0) inv.append(", ");
                    inv.append(names[i]);
                }
                inv.append(')');
                boolean isVoid = returnType.isPrimitive() && "void".equals(returnType.getName());
                bodyStatement = isVoid ? inv + ";" : "return " + inv + ";";
            }
            if (snippets) bodyStatement = bodyStatement.replace("$", "\\$");
            String bodyContent = methodBodyContent(useAlternativeMethodBody, bodyStatement);
            StringBuilder placeHolder = new StringBuilder();
            if (snippets) {
                placeHolder.append("${0");
                if (bodyContent != null) placeHolder.append(':').append(bodyContent);
                placeHolder.append('}');
            } else if (bodyContent != null) {
                placeHolder.append(bodyContent);
            }
            if (bodyContent != null || snippets) {
                String[] lines = placeHolder.toString().split("\n", -1);
                for (int i = 0; i < lines.length; i++) {
                    decl.append(indent).append(lines[i]);
                    decl.append('\n');
                }
            }
        }
        decl.append('}');
        s.text = decl.toString();
        return s;
    }

    /** jdt.ls default code templates: METHODBODY / METHODBODY_SUPER. */
    private static String methodBodyContent(boolean alternative, String bodyStatement) {
        String template = alternative
                ? "// TODO Auto-generated method stub\n${body_statement}"
                : "// TODO Auto-generated method stub\nthrow new UnsupportedOperationException(\"Unimplemented method '${enclosing_method}'\");";
        // full line variable body_statement: drop the line when empty
        String result;
        if (bodyStatement.isEmpty()) {
            result = template.replace("${body_statement}", "");
        } else {
            result = template.replace("${body_statement}", bodyStatement);
        }
        return result;
    }

    private static boolean overrideAnnotation(boolean declaringTypeIsInterface) {
        Map<String, String> o = BridgeOptions.map("21");
        if (declaringTypeIsInterface && "disabled".equals(o.get("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation"))) {
            return false;
        }
        return true; // CodeGenerationSettings.overrideAnnotation
    }

    private static String modifiers(int mods) {
        StringBuilder sb = new StringBuilder();
        int[] order = { Modifier.PUBLIC, Modifier.PROTECTED, Modifier.PRIVATE, Modifier.ABSTRACT, Modifier.DEFAULT,
                Modifier.STATIC, Modifier.FINAL, Modifier.SYNCHRONIZED, Modifier.NATIVE, Modifier.STRICTFP };
        String[] names = { "public", "protected", "private", "abstract", "default", "static", "final", "synchronized",
                "native", "strictfp" };
        for (int i = 0; i < order.length; i++) {
            if ((mods & order[i]) != 0) {
                if (sb.length() > 0) sb.append(' ');
                sb.append(names[i]);
            }
        }
        return sb.toString();
    }

    private static String defaultValue(ITypeBinding t) {
        if (!t.isPrimitive()) return "null";
        switch (t.getName()) {
            case "void":
                return null;
            case "boolean":
                return "false";
            default:
                return "0";
        }
    }

    private static String[] parameterNames(IMethodBinding m) {
        ITypeBinding[] params = m.getParameterTypes();
        String[] names = new String[params.length];
        String[] fromElement = null;
        try {
            org.eclipse.jdt.core.dom.IMethodBinding decl = m.getMethodDeclaration();
            if (decl != null) {
                java.lang.reflect.Method pn = decl.getClass().getMethod("getParameterNames");
                Object r = pn.invoke(decl);
                if (r instanceof String[] arr && arr.length == params.length) fromElement = arr;
            }
        } catch (ReflectiveOperationException | RuntimeException e) {
            fromElement = null;
        }
        for (int i = 0; i < names.length; i++) {
            names[i] = fromElement != null && fromElement[i] != null && !fromElement[i].isEmpty() ? fromElement[i] : "arg" + i;
        }
        return names;
    }

    private static IMethodBinding findMethodInHierarchy(ITypeBinding type, String name, String[] paramTypes) {
        Set<String> seen = new HashSet<>();
        List<ITypeBinding> todo = new ArrayList<>();
        todo.add(type);
        while (!todo.isEmpty()) {
            ITypeBinding t = todo.remove(0);
            if (t == null || !seen.add(t.getKey())) continue;
            IMethodBinding m = findMethodInType(t, name, paramTypes);
            if (m != null) return m;
            if (t.getSuperclass() != null) todo.add(t.getSuperclass());
            for (ITypeBinding i : t.getInterfaces()) todo.add(i);
        }
        return null;
    }

    private static IMethodBinding findMethodInType(ITypeBinding type, String name, String[] paramTypes) {
        if (type == null) return null;
        for (IMethodBinding m : type.getDeclaredMethods()) {
            if (!m.getName().equals(name)) continue;
            ITypeBinding[] ps = m.getParameterTypes();
            if (ps.length != paramTypes.length) continue;
            boolean ok = true;
            for (int i = 0; i < ps.length; i++) {
                String q = ps[i].getErasure().getQualifiedName();
                String want = paramTypes[i];
                int lt = want.indexOf('<');
                if (lt >= 0) want = want.substring(0, lt) + want.substring(want.lastIndexOf('>') + 1);
                if (!q.equals(want) && !ps[i].getErasure().getName().equals(want)
                        && !(ps[i].isTypeVariable() || ps[i].getErasure().isTypeVariable())) {
                    ok = false;
                    break;
                }
            }
            if (ok) return m;
        }
        return null;
    }

    private static ITypeBinding findImmediateSuperTypeInHierarchy(ITypeBinding type, String qualifiedName) {
        if (type.getSuperclass() != null && qualifiedName.equals(type.getSuperclass().getTypeDeclaration().getQualifiedName())) {
            return type.getSuperclass();
        }
        for (ITypeBinding i : type.getInterfaces()) {
            if (qualifiedName.equals(i.getTypeDeclaration().getQualifiedName())) return i;
        }
        ITypeBinding sup = type.getSuperclass();
        if (sup != null) {
            ITypeBinding r = findImmediateSuperTypeInHierarchy(sup, qualifiedName);
            if (r != null) return r;
        }
        for (ITypeBinding i : type.getInterfaces()) {
            ITypeBinding r = findImmediateSuperTypeInHierarchy(i, qualifiedName);
            if (r != null) return r;
        }
        return null;
    }

    /** Minimal {@code ImportRewrite.addImport(ITypeBinding)}: simple names, imports recorded. */
    private static final class Imports {
        private final Map<String, String> simpleToQualified = new HashMap<>();
        private final String packageName;

        Imports(CompilationUnit cu) {
            this.packageName = cu.getPackage() == null ? "" : cu.getPackage().getName().getFullyQualifiedName();
            for (Object o : cu.imports()) {
                ImportDeclaration d = (ImportDeclaration) o;
                if (d.isStatic() || d.isOnDemand()) continue;
                String q = d.getName().getFullyQualifiedName();
                simpleToQualified.put(q.substring(q.lastIndexOf('.') + 1), q);
            }
        }

        String add(ITypeBinding t, Stub s) {
            if (t.isPrimitive() || t.isTypeVariable() || t.isNullType()) return t.getName();
            if (t.isArray()) {
                StringBuilder sb = new StringBuilder(add(t.getElementType(), s));
                for (int i = 0; i < t.getDimensions(); i++) sb.append("[]");
                return sb.toString();
            }
            if (t.isWildcardType()) {
                ITypeBinding b = t.getBound();
                if (b == null) return "?";
                return (t.isUpperbound() ? "? extends " : "? super ") + add(b, s);
            }
            if (t.isCapture()) return add(t.getWildcard(), s);
            ITypeBinding decl = t.getTypeDeclaration();
            String qualified = decl.getQualifiedName();
            String simple = decl.getName();
            String name;
            String existing = simpleToQualified.get(simple);
            if (existing != null && !existing.equals(qualified)) {
                name = qualified;
            } else {
                simpleToQualified.put(simple, qualified);
                if (!s.imports.contains(qualified)) s.imports.add(qualified);
                name = decl.isMember() ? memberName(decl) : simple;
            }
            if (t.isParameterizedType()) {
                StringBuilder sb = new StringBuilder(name).append('<');
                ITypeBinding[] args = t.getTypeArguments();
                for (int i = 0; i < args.length; i++) {
                    if (i > 0) sb.append(", ");
                    sb.append(add(args[i], s));
                }
                return sb.append('>').toString();
            }
            return name;
        }

        private String memberName(ITypeBinding decl) {
            return decl.getName();
        }

        @SuppressWarnings("unused")
        String packageName() {
            return packageName;
        }
    }
}
