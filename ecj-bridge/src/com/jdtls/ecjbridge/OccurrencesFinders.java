package com.jdtls.ecjbridge;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

import org.eclipse.jdt.core.dom.*;

import com.jdtls.ecjbridge.NavigationDataService.RawLocation;
import com.jdtls.ecjbridge.NavigationDataService.Unit;

/**
 * Occurrence finders used by jdt.ls {@code DocumentHighlightHandler}, in its
 * order: exception occurrences, method exits, break/continue targets,
 * implement occurrences, then plain occurrences (from
 * {@code org.eclipse.jdt.internal.core.manipulation.search}).  Each returns
 * {@code null} when it does not apply to the selection.
 */
final class OccurrencesFinders {

    private OccurrencesFinders() {}

    static final class Occurrence {
        final int offset, length;
        final boolean write;

        Occurrence(int offset, int length, boolean write) {
            this.offset = offset;
            this.length = length;
            this.write = write;
        }

        Occurrence(ASTNode n, boolean write) {
            this(n.getStartPosition(), n.getLength(), write);
        }
    }

    static List<RawLocation> highlights(Unit u, int line, int character) {
        List<RawLocation> out = new ArrayList<>();
        if (u == null) {
            return out;
        }
        int offset = u.offset(line, character);
        if (offset < 0) {
            return out;
        }
        ASTNode node = NodeFinder.perform(u.cu, offset, 0);
        List<Occurrence> occ = exceptionOccurrences(u.cu, node);
        if (occ == null) {
            occ = methodExits(u.cu, node);
        }
        if (occ == null) {
            occ = breakContinueTargets(u, node);
        }
        if (occ == null) {
            occ = implementOccurrences(node);
        }
        if (occ == null) {
            occ = occurrences(u.cu, node);
        }
        if (occ == null) {
            return out;
        }
        for (Occurrence o : occ) {
            RawLocation l = u.location(o.offset, o.length);
            l.kind = o.write ? 3 : 2;
            out.add(l);
        }
        return out;
    }

    // ── helpers ───────────────────────────────────────────────────────────

    /** {@code ASTNodes.getNormalizedNode}: a name inside a type → the outermost type node. */
    static ASTNode normalizedNode(ASTNode node) {
        ASTNode current = node;
        while (current instanceof Name && current.getParent() instanceof QualifiedName qn && qn.getName() == current) {
            current = qn;
        }
        while (current instanceof Name && current.getParent() instanceof QualifiedName qn && qn.getQualifier() == current) {
            current = qn;
        }
        if (current.getParent() instanceof SimpleType || current.getParent() instanceof NameQualifiedType) {
            current = current.getParent();
        }
        while (current.getParent() instanceof ParameterizedType pt && pt.getType() == current) {
            current = pt;
        }
        return current;
    }

    static boolean isSubtype(ITypeBinding sub, ITypeBinding sup) {
        if (sub == null || sup == null) {
            return false;
        }
        return sub.getErasure().isSubTypeCompatible(sup.getErasure());
    }

    // ── ExceptionOccurrencesFinder ────────────────────────────────────────

    static List<Occurrence> exceptionOccurrences(CompilationUnit root, ASTNode node) {
        if (!(node instanceof Name)) {
            return null;
        }
        ASTNode selected = normalizedNode(node);
        ASTNode start;
        ITypeBinding exception;
        if (selected.getLocationInParent() == MethodDeclaration.THROWN_EXCEPTION_TYPES_PROPERTY) {
            exception = ((Type) selected).resolveBinding();
            start = selected.getParent();
        } else if (selected instanceof Type t && t.getParent() instanceof SingleVariableDeclaration svd
                && svd.getParent() instanceof CatchClause cc) {
            exception = t.resolveBinding();
            start = cc.getParent() instanceof TryStatement ts ? ts.getBody() : null;
        } else if (selected instanceof Type t && t.getParent() instanceof UnionType ut && ut.getParent() instanceof SingleVariableDeclaration svd
                && svd.getParent() instanceof CatchClause cc) {
            exception = t.resolveBinding();
            start = cc.getParent() instanceof TryStatement ts ? ts.getBody() : null;
        } else {
            return null;
        }
        if (exception == null || start == null) {
            return null;
        }
        List<Occurrence> result = new ArrayList<>();
        result.add(new Occurrence(selected, false));
        ASTNode body = start instanceof MethodDeclaration md ? md.getBody() : start;
        if (body != null) {
            body.accept(new ExitCollector(exception, result, false));
        }
        return result;
    }

    /**
     * Collects throw statements and invocations throwing {@code exception}
     * (or, with {@code exception == null}, every uncaught exit) — shared by
     * the exception-occurrence and method-exit finders.
     */
    static final class ExitCollector extends ASTVisitor {
        private final ITypeBinding exception;
        private final List<Occurrence> result;
        private final boolean exits;
        private final List<List<ITypeBinding>> caught = new ArrayList<>();

        ExitCollector(ITypeBinding exception, List<Occurrence> result, boolean exits) {
            this.exception = exception;
            this.result = result;
            this.exits = exits;
        }

        private boolean isCaught(ITypeBinding thrown) {
            for (List<ITypeBinding> level : caught) {
                for (ITypeBinding c : level) {
                    if (isSubtype(thrown, c)) {
                        return true;
                    }
                }
            }
            return false;
        }

        private boolean matches(ITypeBinding thrown) {
            if (thrown == null) {
                return false;
            }
            if (exits) {
                return !isCaught(thrown);
            }
            return isSubtype(thrown, exception) && !isCaught(thrown);
        }

        private void handle(ASTNode node, IMethodBinding binding, ASTNode name) {
            if (binding == null) {
                return;
            }
            for (ITypeBinding ex : binding.getExceptionTypes()) {
                if (matches(ex)) {
                    result.add(new Occurrence(name, false));
                    return;
                }
            }
        }

        @Override
        public boolean visit(TryStatement node) {
            List<ITypeBinding> level = new ArrayList<>();
            for (Object o : node.catchClauses()) {
                CatchClause cc = (CatchClause) o;
                Type t = cc.getException().getType();
                if (t instanceof UnionType ut) {
                    for (Object alt : ut.types()) {
                        ITypeBinding b = ((Type) alt).resolveBinding();
                        if (b != null) {
                            level.add(b);
                        }
                    }
                } else if (t.resolveBinding() != null) {
                    level.add(t.resolveBinding());
                }
            }
            caught.add(level);
            for (Object r : node.resources()) {
                ((ASTNode) r).accept(this);
            }
            node.getBody().accept(this);
            caught.remove(caught.size() - 1);
            for (Object o : node.catchClauses()) {
                ((ASTNode) o).accept(this);
            }
            if (node.getFinally() != null) {
                node.getFinally().accept(this);
            }
            return false;
        }

        @Override
        public boolean visit(ThrowStatement node) {
            ITypeBinding t = node.getExpression().resolveTypeBinding();
            if (matches(t)) {
                result.add(new Occurrence(node.getStartPosition(), 5, false));
            }
            return true;
        }

        @Override
        public boolean visit(MethodInvocation node) {
            handle(node, node.resolveMethodBinding(), node.getName());
            return true;
        }

        @Override
        public boolean visit(SuperMethodInvocation node) {
            handle(node, node.resolveMethodBinding(), node.getName());
            return true;
        }

        @Override
        public boolean visit(ClassInstanceCreation node) {
            handle(node, node.resolveConstructorBinding(), node.getType());
            return true;
        }

        @Override
        public boolean visit(ConstructorInvocation node) {
            handle(node, node.resolveConstructorBinding(), node);
            return true;
        }

        @Override
        public boolean visit(SuperConstructorInvocation node) {
            handle(node, node.resolveConstructorBinding(), node);
            return true;
        }

        @Override
        public boolean visit(ReturnStatement node) {
            if (exits) {
                result.add(new Occurrence(node, false));
            }
            return true;
        }

        @Override
        public boolean visit(AnonymousClassDeclaration node) {
            return false;
        }

        @Override
        public boolean visit(LambdaExpression node) {
            return false;
        }

        @Override
        public boolean visit(TypeDeclarationStatement node) {
            return false;
        }
    }

    // ── MethodExitsFinder ─────────────────────────────────────────────────

    static List<Occurrence> methodExits(CompilationUnit root, ASTNode node) {
        MethodDeclaration method = null;
        if (node instanceof ReturnStatement) {
            ASTNode p = node.getParent();
            while (p != null && !(p instanceof MethodDeclaration)) {
                if (p instanceof LambdaExpression || p instanceof AnonymousClassDeclaration) {
                    return null;
                }
                p = p.getParent();
            }
            method = (MethodDeclaration) p;
        } else {
            Type type = null;
            if (node instanceof Type t) {
                type = t;
            } else if (node instanceof Name name) {
                ASTNode top = name;
                while (top.getParent() instanceof QualifiedName) {
                    top = top.getParent();
                }
                if (top.getParent() instanceof Type t) {
                    type = t;
                }
            }
            if (type == null) {
                return null;
            }
            while (type.getParent() instanceof Type t) {
                type = t;
            }
            if (type.getLocationInParent() != MethodDeclaration.RETURN_TYPE2_PROPERTY) {
                return null;
            }
            method = (MethodDeclaration) type.getParent();
        }
        if (method == null) {
            return null;
        }
        List<Occurrence> result = new ArrayList<>();
        Type returnType = method.getReturnType2();
        if (returnType != null) {
            result.add(new Occurrence(returnType, false));
        }
        Block body = method.getBody();
        if (body != null) {
            for (Object s : body.statements()) {
                ((ASTNode) s).accept(new ExitCollector(null, result, true));
            }
            List<?> statements = body.statements();
            boolean voidMethod = returnType == null || returnType.isPrimitiveType() && ((PrimitiveType) returnType).getPrimitiveTypeCode() == PrimitiveType.VOID;
            if (voidMethod && (statements.isEmpty() || !(statements.get(statements.size() - 1) instanceof ReturnStatement
                    || statements.get(statements.size() - 1) instanceof ThrowStatement))) {
                result.add(new Occurrence(body.getStartPosition() + body.getLength() - 1, 1, false));
            }
        }
        return result;
    }

    // ── BreakContinueTargetFinder ─────────────────────────────────────────

    static List<Occurrence> breakContinueTargets(Unit u, ASTNode node) {
        ASTNode selected = node;
        if (selected instanceof SimpleName && (selected.getParent() instanceof BreakStatement || selected.getParent() instanceof ContinueStatement)) {
            selected = selected.getParent();
        }
        SimpleName label;
        boolean isBreak;
        if (selected instanceof BreakStatement bs) {
            label = bs.getLabel();
            isBreak = true;
        } else if (selected instanceof ContinueStatement cs) {
            label = cs.getLabel();
            isBreak = false;
        } else {
            return null;
        }
        ASTNode target = null;
        ASTNode p = selected.getParent();
        while (p != null) {
            if (p instanceof BodyDeclaration || p instanceof LambdaExpression || p instanceof AnonymousClassDeclaration) {
                break;
            }
            if (label == null) {
                if (p instanceof ForStatement || p instanceof EnhancedForStatement || p instanceof WhileStatement || p instanceof DoStatement
                        || isBreak && (p instanceof SwitchStatement || p instanceof SwitchExpression)) {
                    target = p;
                    break;
                }
            } else if (p instanceof LabeledStatement ls && ls.getLabel().getIdentifier().equals(label.getIdentifier())) {
                target = ls;
                break;
            }
            p = p.getParent();
        }
        if (target == null) {
            return null;
        }
        List<Occurrence> result = new ArrayList<>();
        int start = target.getStartPosition();
        int end = start;
        while (end < u.source.length() && Character.isJavaIdentifierPart(u.source.charAt(end))) {
            end++;
        }
        result.add(new Occurrence(start, end - start, false));
        if (isBreak) {
            ASTNode inner = target instanceof LabeledStatement ls ? ls.getBody() : target;
            ASTNode body = null;
            if (inner instanceof ForStatement f) {
                body = f.getBody();
            } else if (inner instanceof EnhancedForStatement f) {
                body = f.getBody();
            } else if (inner instanceof WhileStatement w) {
                body = w.getBody();
            } else if (inner instanceof DoStatement d) {
                body = d.getBody();
            } else if (inner instanceof SwitchStatement || inner instanceof SwitchExpression || inner instanceof Block) {
                body = inner;
            }
            if (body != null) {
                int last = body.getStartPosition() + body.getLength() - 1;
                if (last >= 0 && last < u.source.length() && u.source.charAt(last) == '}') {
                    result.add(new Occurrence(last, 1, false));
                }
            }
        }
        return result;
    }

    // ── ImplementOccurrencesFinder ────────────────────────────────────────

    static List<Occurrence> implementOccurrences(ASTNode node) {
        if (!(node instanceof Name)) {
            return null;
        }
        ASTNode selected = normalizedNode(node);
        if (!(selected instanceof Type type)) {
            return null;
        }
        StructuralPropertyDescriptor loc = selected.getLocationInParent();
        if (loc != TypeDeclaration.SUPERCLASS_TYPE_PROPERTY && loc != TypeDeclaration.SUPER_INTERFACE_TYPES_PROPERTY
                && loc != EnumDeclaration.SUPER_INTERFACE_TYPES_PROPERTY && loc != RecordDeclaration.SUPER_INTERFACE_TYPES_PROPERTY) {
            return null;
        }
        ITypeBinding selectedType = type.resolveBinding();
        if (selectedType == null) {
            return null;
        }
        AbstractTypeDeclaration decl = (AbstractTypeDeclaration) selected.getParent();
        List<Occurrence> result = new ArrayList<>();
        result.add(new Occurrence(selected, false));
        for (Object o : decl.bodyDeclarations()) {
            if (o instanceof MethodDeclaration md) {
                IMethodBinding mb = md.resolveBinding();
                if (mb != null && overridesIn(selectedType, mb, new HashSet<>())) {
                    result.add(new Occurrence(md.getName(), false));
                }
            }
        }
        return result;
    }

    /** {@code Bindings.findOverriddenMethodInHierarchy(type, method) != null}. */
    private static boolean overridesIn(ITypeBinding type, IMethodBinding method, Set<String> seen) {
        if (type == null || !seen.add(type.getKey())) {
            return false;
        }
        for (IMethodBinding m : type.getDeclaredMethods()) {
            if (!m.isConstructor() && m.getName().equals(method.getName()) && (method.overrides(m) || method.isSubsignature(m))) {
                return true;
            }
        }
        if (overridesIn(type.getSuperclass(), method, seen)) {
            return true;
        }
        for (ITypeBinding i : type.getInterfaces()) {
            if (overridesIn(i, method, seen)) {
                return true;
            }
        }
        return false;
    }

    // ── OccurrencesFinder ─────────────────────────────────────────────────

    static List<Occurrence> occurrences(CompilationUnit root, ASTNode node) {
        if (!(node instanceof Name name)) {
            return null;
        }
        IBinding target = name.resolveBinding();
        if (target == null) {
            return null;
        }
        String key = NavigationDataService.key(target);
        if (key == null) {
            return null;
        }
        Set<ASTNode> writes = new HashSet<>();
        List<Occurrence> result = new ArrayList<>();
        root.accept(new ASTVisitor(true) {
            private SimpleName simpleName(Expression e) {
                if (e instanceof SimpleName sn) {
                    return sn;
                }
                if (e instanceof QualifiedName qn) {
                    return qn.getName();
                }
                if (e instanceof FieldAccess fa) {
                    return fa.getName();
                }
                if (e instanceof SuperFieldAccess sfa) {
                    return sfa.getName();
                }
                return null;
            }

            @Override
            public boolean visit(Assignment node) {
                SimpleName n = simpleName(node.getLeftHandSide());
                if (n != null) {
                    writes.add(n);
                }
                return true;
            }

            @Override
            public boolean visit(SingleVariableDeclaration node) {
                if (node.getInitializer() != null) {
                    writes.add(node.getName());
                }
                return true;
            }

            @Override
            public boolean visit(VariableDeclarationFragment node) {
                if (node.getInitializer() != null) {
                    writes.add(node.getName());
                }
                return true;
            }

            @Override
            public boolean visit(PostfixExpression node) {
                SimpleName n = simpleName(node.getOperand());
                if (n != null) {
                    writes.add(n);
                }
                return true;
            }

            @Override
            public boolean visit(PrefixExpression node) {
                PrefixExpression.Operator op = node.getOperator();
                if (op == PrefixExpression.Operator.INCREMENT || op == PrefixExpression.Operator.DECREMENT) {
                    SimpleName n = simpleName(node.getOperand());
                    if (n != null) {
                        writes.add(n);
                    }
                }
                return true;
            }

            @Override
            public boolean visit(SimpleName node) {
                IBinding b = node.resolveBinding();
                if (b != null && key.equals(NavigationDataService.key(b))) {
                    result.add(new Occurrence(node, writes.contains(node)));
                }
                return true;
            }
        });
        return result;
    }
}
