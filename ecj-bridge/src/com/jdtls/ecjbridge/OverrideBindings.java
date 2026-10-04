package com.jdtls.ecjbridge;

import java.util.ArrayList;
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
import org.eclipse.jdt.core.dom.NodeFinder;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/** Binding data for Rust's override completion replacement generator. */
final class OverrideBindings {
    private OverrideBindings() {}

    static final class TypeData {
        public String kind, name;
        public String qualified = "";
        public int dimensions;
        public boolean upper;
        public TypeData element, bound;
        public List<TypeData> arguments = new ArrayList<>();
    }
    static final class TypeParameterData {
        public String name;
        public List<TypeData> bounds = new ArrayList<>();
    }
    static final class MethodData {
        public String name;
        public int modifiers;
        public boolean inInterface, declaringInterface, declaringObject, varargs;
        public TypeData returnType, interfaceSuper;
        public List<TypeData> parameters = new ArrayList<>(), exceptions = new ArrayList<>();
        public List<TypeParameterData> typeParameters = new ArrayList<>();
        public String[] parameterNames;
    }

    static Object compute(JsonObject q, Map<String, String> files, List<String> classpath, String level, String uri) {
        Map<String, MethodData> out = new LinkedHashMap<>();
        String source = files.get(uri);
        if (source == null || !q.has("methods")) return out;
        CompilationUnit cu = org.eclipse.jdt.core.dom.BridgeDomResolver.resolve(
                new SourceIndexNameEnvironment(files, classpath),
                new InMemoryCompilationUnit(uri, source), BridgeOptions.map(level));
        if (cu == null) return out;
        for (JsonElement e : q.getAsJsonArray("methods")) {
            JsonObject m = e.getAsJsonObject();
            try {
                MethodData data = method(cu, m.get("name").getAsString(), m.get("signature").getAsString(),
                        m.get("replaceStart").getAsInt());
                if (data != null) out.put(m.get("key").getAsString(), data);
            } catch (RuntimeException ex) {
                // Unresolved bindings: Rust uses the proposal's fallback text.
            }
        }
        return out;
    }

    private static MethodData method(CompilationUnit cu, String name, String signature, int offset) {
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
        IMethodBinding method = findMethodInHierarchy(declaringType, name, paramTypes);
        ITypeBinding object = cu.getAST().resolveWellKnownType("java.lang.Object");
        if (method == null && declaringType.isInterface()) method = findMethodInType(object, name, paramTypes);
        if (method == null) return null;
        MethodData data = new MethodData();
        data.name = method.getName();
        data.modifiers = method.getModifiers();
        data.inInterface = declaringType.isInterface();
        ITypeBinding owner = method.getDeclaringClass();
        data.declaringInterface = owner.isInterface();
        data.declaringObject = owner == object;
        data.varargs = method.isVarargs();
        data.returnType = type(method.getReturnType());
        data.parameterNames = parameterNames(method);
        for (ITypeBinding p : method.getParameterTypes()) data.parameters.add(type(p));
        for (ITypeBinding t : method.getExceptionTypes()) data.exceptions.add(type(t));
        for (ITypeBinding t : method.getTypeParameters()) {
            TypeParameterData p = new TypeParameterData();
            p.name = t.getName();
            for (ITypeBinding b : t.getTypeBounds()) p.bounds.add(type(b));
            data.typeParameters.add(p);
        }
        if (owner.isInterface()) {
            ITypeBinding immediate = findImmediateSuperTypeInHierarchy(declaringType, owner.getTypeDeclaration().getQualifiedName());
            if (immediate == null) immediate = owner;
            if (immediate.isInterface()) data.interfaceSuper = type(immediate.getTypeDeclaration());
        }
        return data;
    }

    private static TypeData type(ITypeBinding t) {
        if (t.isCapture()) return type(t.getWildcard());
        TypeData data = new TypeData();
        data.name = t.getName();
        if (t.isPrimitive() || t.isTypeVariable() || t.isNullType()) {
            data.kind = "simple";
        } else if (t.isArray()) {
            data.kind = "array";
            data.dimensions = t.getDimensions();
            data.element = type(t.getElementType());
        } else if (t.isWildcardType()) {
            data.kind = "wildcard";
            data.upper = t.isUpperbound();
            if (t.getBound() != null) data.bound = type(t.getBound());
        } else {
            data.kind = "class";
            data.name = t.getTypeDeclaration().getName();
            data.qualified = t.getTypeDeclaration().getQualifiedName();
            if (t.isParameterizedType()) {
                for (ITypeBinding a : t.getTypeArguments()) data.arguments.add(type(a));
            }
        }
        return data;
    }

    private static String[] parameterNames(IMethodBinding m) {
        ITypeBinding[] params = m.getParameterTypes();
        String[] names = new String[params.length];
        String[] fromElement = null;
        try {
            String[] declared = m.getMethodDeclaration().getParameterNames();
            if (declared != null && declared.length == params.length) fromElement = declared;
        } catch (RuntimeException e) {
            // Parameter metadata is optional in binary methods.
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

}
