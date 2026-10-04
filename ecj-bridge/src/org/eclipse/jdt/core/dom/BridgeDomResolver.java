package org.eclipse.jdt.core.dom;

import java.util.Map;

import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.env.INameEnvironment;
import org.eclipse.jdt.internal.compiler.impl.CompilerOptions;
import org.eclipse.jdt.internal.compiler.problem.DefaultProblemFactory;
import org.eclipse.jdt.internal.core.DefaultWorkingCopyOwner;

/**
 * Builds a DOM {@link CompilationUnit} with resolved bindings on top of an
 * arbitrary {@link INameEnvironment} (in-memory sources + classpath), i.e. the
 * equivalent of {@code ASTParser.createAST} with a Java project, but without
 * the Java model.  Lives in the DOM package to reach the package-private
 * pieces of {@link CompilationUnitResolver}.
 */
public final class BridgeDomResolver {
    private BridgeDomResolver() {}

    /**
     * The type named {@code qualifiedName} (dotted; member types after their
     * enclosing type) as seen from {@code cu}'s lookup environment, i.e.
     * {@code IJavaProject.findType} without the Java model. Null when the
     * unit has no bindings or the type does not exist.
     */
    public static ITypeBinding findType(CompilationUnit cu, String qualifiedName) {
        BindingResolver resolver = cu.getAST().getBindingResolver();
        org.eclipse.jdt.internal.compiler.lookup.LookupEnvironment env;
        try {
            env = resolver.lookupEnvironment();
        } catch (RuntimeException e) {
            return null;
        }
        if (env == null || qualifiedName == null || qualifiedName.isEmpty()) return null;
        String[] parts = qualifiedName.split("\\.");
        for (int i = parts.length; i >= 1; i--) {
            char[][] compound = new char[i][];
            for (int j = 0; j < i; j++) compound[j] = parts[j].toCharArray();
            org.eclipse.jdt.internal.compiler.lookup.ReferenceBinding rb;
            try {
                rb = env.getType(compound);
            } catch (RuntimeException e) {
                rb = null;
            }
            if (rb == null || !rb.isValidBinding()) continue;
            for (int j = i; j < parts.length && rb != null; j++) {
                rb = rb.getMemberType(parts[j].toCharArray());
            }
            if (rb == null || !rb.isValidBinding()) return null;
            return resolver.getTypeBinding(rb);
        }
        return null;
    }

    /**
     * Parses and resolves {@code unit}.  Returns {@code null} when the compiler
     * aborted (caller falls back to a binding-less parse).
     */
    public static CompilationUnit resolve(INameEnvironment environment,
            org.eclipse.jdt.internal.compiler.env.ICompilationUnit unit,
            Map<String, String> options) {
        CompilerOptions compilerOptions = CompilationUnitResolver.getCompilerOptions(options, true);
        int flags = org.eclipse.jdt.core.ICompilationUnit.ENABLE_STATEMENTS_RECOVERY
                | org.eclipse.jdt.core.ICompilationUnit.ENABLE_BINDINGS_RECOVERY;
        CompilationUnitResolver resolver = new CompilationUnitResolver(
                environment,
                CompilationUnitResolver.getHandlingPolicy(),
                compilerOptions,
                CompilationUnitResolver.getRequestor(),
                new DefaultProblemFactory(),
                null,
                false);
        CompilationUnitDeclaration declaration = resolver.resolve(unit, true, true, false);
        if (declaration == null || resolver.hasCompilationAborted) {
            return null;
        }
        return CompilationUnitResolver.convert(
                declaration,
                unit.getContents(),
                AST.getJLSLatest(),
                options,
                true,
                DefaultWorkingCopyOwner.PRIMARY,
                new DefaultBindingResolver.BindingTables(),
                flags,
                null,
                false);
    }
}
