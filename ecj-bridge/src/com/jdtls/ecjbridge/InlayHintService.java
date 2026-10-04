package com.jdtls.ecjbridge;

import java.net.URI;
import java.util.ArrayList;
import java.util.List;

import org.eclipse.jdt.core.dom.AST;
import org.eclipse.jdt.core.dom.ASTNode;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.ASTVisitor;
import org.eclipse.jdt.core.dom.CastExpression;
import org.eclipse.jdt.core.dom.ClassInstanceCreation;
import org.eclipse.jdt.core.dom.CompilationUnit;
import org.eclipse.jdt.core.dom.ConstructorInvocation;
import org.eclipse.jdt.core.dom.EnumConstantDeclaration;
import org.eclipse.jdt.core.dom.Expression;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.IVariableBinding;
import org.eclipse.jdt.core.dom.LambdaExpression;
import org.eclipse.jdt.core.dom.MethodInvocation;
import org.eclipse.jdt.core.dom.SimpleName;
import org.eclipse.jdt.core.dom.StringLiteral;
import org.eclipse.jdt.core.dom.SuperConstructorInvocation;
import org.eclipse.jdt.core.dom.SuperMethodInvocation;
import org.eclipse.jdt.core.dom.TextBlock;
import org.eclipse.jdt.core.dom.VariableDeclaration;
import org.eclipse.jdt.core.dom.VariableDeclarationFragment;
import org.eclipse.jdt.core.dom.VariableDeclarationStatement;

import com.jdtls.ecjbridge.BridgeProtocol.Request;

/**
 * Binding data for inlay hints (jdt.ls {@code InlayHintVisitor}).  This only
 * reports, in AST pre-order, the nodes the jdt.ls visitor looks at together
 * with their resolved bindings; all hint logic (ranges, modes, filters,
 * exclusions, format specifiers) lives in Rust ({@code features::inlay_hints}).
 */
final class InlayHintService {

    /** One expression (argument or invocation receiver). */
    static final class Expr {
        int start, length;
        /** DOM node class simple name, e.g. {@code StringLiteral}. */
        String node;
        /** Identifier of a {@code SimpleName}. */
        String identifier;
        /** {@code getLiteralValue()} of a {@code StringLiteral} / {@code TextBlock}. */
        String literalValue;
        /** {@code toString()} (only when format-parameter data is requested). */
        String text;
        /** The casted expression of a {@code CastExpression}. */
        Expr inner;
    }

    /** A resolved method or constructor binding. */
    static final class Method {
        String name;
        /** {@code getDeclaringClass().getQualifiedName()}. */
        String declaringType;
        String declaringPackage;
        /** {@code IType.getTypeQualifiedName()} ('$'-separated, no package). */
        String declaringTypeQualifiedName;
        /** The declaring type comes from source (jdt.ls: has a source buffer). */
        boolean fromSource;
        /** The declaring type is declared in the requested compilation unit. */
        boolean inTargetUnit;
        /** No Java element (synthetic, e.g. an implicit record canonical constructor). */
        boolean synthetic;
        boolean record;
        boolean varargs;
        boolean constructor;
        List<String> parameterNames;
        /** {@code getParameterTypes()[i].getQualifiedName()}. */
        List<String> parameterTypes;
    }

    static final class LambdaParameter {
        String node;
        int nameStart;
    }

    static final class Fragment {
        boolean resolved;
        String initializer;
        String typeName;
        int nameStart, nameLength;
    }

    static final class Node {
        /** DOM node class simple name of the visited node. */
        String kind;
        int start, length;
        Method method;
        List<Expr> arguments;
        Expr expression;
        List<String> lambdaParameterTypes;
        List<LambdaParameter> lambdaParameters;
        boolean isVar;
        List<Fragment> fragments;
    }

    List<Node> collect(Request req) {
        List<Node> nodes = new ArrayList<>();
        String source = req.files == null ? null : req.files.get(req.uri);
        if (source == null) {
            return nodes;
        }
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(BridgeOptions.map(req.sourceLevel));
        parser.setUnitName(unitName(req.uri));
        String[] cp = req.classpath != null ? req.classpath.toArray(new String[0]) : new String[0];
        String[] sp = req.sourcepath != null && !req.sourcepath.isEmpty() ? req.sourcepath.toArray(new String[0]) : null;
        parser.setEnvironment(cp, sp, null, /* includeRunningVMBootclasspath */ true);
        CompilationUnit cu = (CompilationUnit) parser.createAST(null);
        boolean withText = req.formatParameters;

        cu.accept(new ASTVisitor() {
            private Node add(ASTNode node) {
                Node n = new Node();
                n.kind = node.getClass().getSimpleName();
                n.start = node.getStartPosition();
                n.length = node.getLength();
                nodes.add(n);
                return n;
            }

            private void invocation(ASTNode node, IMethodBinding binding, List<?> args, Expression receiver) {
                Node n = add(node);
                n.method = method(cu, binding);
                n.arguments = new ArrayList<>();
                for (Object a : args) {
                    n.arguments.add(expr((Expression) a, withText, true));
                }
                if (receiver != null) {
                    boolean literal = receiver instanceof StringLiteral || receiver instanceof TextBlock;
                    n.expression = expr(receiver, withText && literal, false);
                }
            }

            @Override
            public boolean visit(EnumConstantDeclaration node) {
                invocation(node, node.resolveConstructorBinding(), node.arguments(), null);
                return true;
            }

            @Override
            public boolean visit(ClassInstanceCreation node) {
                invocation(node, node.resolveConstructorBinding(), node.arguments(), null);
                return true;
            }

            @Override
            public boolean visit(MethodInvocation node) {
                invocation(node, node.resolveMethodBinding(), node.arguments(), node.getExpression());
                return true;
            }

            @Override
            public boolean visit(SuperMethodInvocation node) {
                invocation(node, node.resolveMethodBinding(), node.arguments(), null);
                return true;
            }

            @Override
            public boolean visit(ConstructorInvocation node) {
                invocation(node, node.resolveConstructorBinding(), node.arguments(), null);
                return true;
            }

            @Override
            public boolean visit(SuperConstructorInvocation node) {
                invocation(node, node.resolveConstructorBinding(), node.arguments(), null);
                return true;
            }

            @Override
            public boolean visit(LambdaExpression node) {
                Node n = add(node);
                IMethodBinding binding = node.resolveMethodBinding();
                if (binding != null) {
                    n.lambdaParameterTypes = new ArrayList<>();
                    for (ITypeBinding t : binding.getParameterTypes()) {
                        n.lambdaParameterTypes.add(t.getName());
                    }
                }
                n.lambdaParameters = new ArrayList<>();
                for (Object p : node.parameters()) {
                    LambdaParameter lp = new LambdaParameter();
                    lp.node = p.getClass().getSimpleName();
                    SimpleName name = p instanceof VariableDeclaration vd ? vd.getName()
                            : p instanceof SimpleName sn ? sn : null;
                    lp.nameStart = name != null ? name.getStartPosition() : -1;
                    n.lambdaParameters.add(lp);
                }
                return true;
            }

            @Override
            public boolean visit(VariableDeclarationStatement node) {
                Node n = add(node);
                n.isVar = node.getType().isVar();
                n.fragments = new ArrayList<>();
                for (Object o : node.fragments()) {
                    VariableDeclarationFragment fragment = (VariableDeclarationFragment) o;
                    Fragment f = new Fragment();
                    IVariableBinding binding = fragment.resolveBinding();
                    f.resolved = binding != null;
                    if (binding != null && binding.getType() != null) {
                        f.typeName = binding.getType().getName();
                    }
                    Expression init = fragment.getInitializer();
                    f.initializer = init != null ? init.getClass().getSimpleName() : null;
                    f.nameStart = fragment.getName().getStartPosition();
                    f.nameLength = fragment.getName().getLength();
                    n.fragments.add(f);
                }
                return true;
            }
        });
        return nodes;
    }

    private static Method method(CompilationUnit cu, IMethodBinding binding) {
        if (binding == null) {
            return null;
        }
        Method m = new Method();
        ITypeBinding declaring = binding.getDeclaringClass();
        m.name = binding.getName();
        m.constructor = binding.isConstructor();
        m.varargs = binding.isVarargs();
        m.synthetic = binding.isSynthetic() || binding.isSyntheticRecordMethod();
        if (declaring != null) {
            ITypeBinding erasure = declaring.getErasure();
            m.declaringType = declaring.getQualifiedName();
            m.declaringPackage = declaring.getPackage() != null ? declaring.getPackage().getName() : "";
            m.declaringTypeQualifiedName = typeQualifiedName(erasure);
            m.fromSource = erasure.isFromSource();
            m.inTargetUnit = cu.findDeclaringNode(erasure) != null;
            m.record = declaring.isRecord();
        }
        m.parameterNames = new ArrayList<>();
        try {
            for (String name : binding.getParameterNames()) {
                m.parameterNames.add(name);
            }
        } catch (RuntimeException e) {
            m.parameterNames = null;
        }
        m.parameterTypes = new ArrayList<>();
        for (ITypeBinding t : binding.getParameterTypes()) {
            m.parameterTypes.add(t.getQualifiedName());
        }
        return m;
    }

    private static String typeQualifiedName(ITypeBinding type) {
        String binary = type.getBinaryName();
        if (binary != null) {
            String pkg = type.getPackage() != null ? type.getPackage().getName() : "";
            return pkg.isEmpty() || !binary.startsWith(pkg + ".") ? binary : binary.substring(pkg.length() + 1);
        }
        return type.getName();
    }

    private static Expr expr(Expression e, boolean withText, boolean unwrapCast) {
        Expr x = new Expr();
        x.start = e.getStartPosition();
        x.length = e.getLength();
        x.node = e.getClass().getSimpleName();
        if (e instanceof SimpleName name) {
            x.identifier = name.getIdentifier();
        } else if (e instanceof StringLiteral literal) {
            try {
                x.literalValue = literal.getLiteralValue();
            } catch (IllegalArgumentException ex) {
                // malformed literal
            }
        } else if (e instanceof TextBlock block) {
            try {
                x.literalValue = block.getLiteralValue();
            } catch (IllegalArgumentException ex) {
                // malformed text block
            }
        } else if (unwrapCast && e instanceof CastExpression cast && cast.getExpression() != null) {
            x.inner = expr(cast.getExpression(), false, false);
        }
        if (withText) {
            x.text = e.toString();
        }
        return x;
    }

    private static String unitName(String uri) {
        try {
            String path = new URI(uri).getPath();
            return path != null ? path : uri;
        } catch (Exception e) {
            return uri;
        }
    }
}
