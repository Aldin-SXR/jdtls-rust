package com.jdtls.ecjbridge;

import org.eclipse.jdt.internal.compiler.*;
import org.eclipse.jdt.internal.compiler.Compiler;
import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.env.ICompilationUnit;
import org.eclipse.jdt.internal.compiler.impl.CompilerOptions;
import org.eclipse.jdt.internal.compiler.problem.DefaultProblemFactory;

import java.util.*;
import java.util.logging.Logger;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * Compiles Java source units using ECJ entirely in memory.
 * Produces diagnostics without writing .class files to disk.
 */
public class CompilationService {

    private static final Logger LOG = Logger.getLogger(CompilationService.class.getName());

    /**
     * Compile all provided source files and return diagnostics for every file.
     *
     * @param sourceFiles  map of URI → source code
     * @param classpath    list of JAR/directory paths
     * @param sourceLevel  "8", "11", "17", "21", etc.
     */
    public List<BridgeDiagnostic> compile(
            Map<String, String> sourceFiles,
            List<String> classpath,
            String sourceLevel) {

        InMemoryNameEnvironment nameEnv = new InMemoryNameEnvironment(sourceFiles, classpath);
        List<BridgeDiagnostic> diagnostics = new ArrayList<>();

        ICompilerRequestor requestor = result -> {
            // Collect compiled bytecode into nameEnv for cross-file resolution
            if (!result.hasErrors()) {
                for (ClassFile cf : result.getClassFiles()) {
                    String name = new String(cf.fileName()).replace('\\', '/');
                    if (name.endsWith(".class")) name = name.substring(0, name.length() - 6);
                    nameEnv.addCompiledClass(name, cf.getBytes());
                }
            }
            // Collect problems
            for (var problem : result.getAllProblems()) {
                BridgeDiagnostic d = new BridgeDiagnostic();
                d.uri = originatingUri(problem.getOriginatingFileName(), sourceFiles);
                d.startLine = problem.getSourceLineNumber() - 1; // LSP is 0-based
                d.startChar = 0; // ECJ gives column for some problems
                d.endLine = d.startLine;
                d.endChar = 999; // ECJ doesn't always give end position; clients will clamp
                d.severity = problem.isError() ? 1 : problem.isWarning() ? 2 : 3;
                d.message = problem.getMessage();
                d.code = String.valueOf(problem.getID());

                // ECJ problem source start/end
                int start = problem.getSourceStart();
                int end = problem.getSourceEnd();
                if (start >= 0 && end >= start) {
                    // Convert byte offsets → line/column
                    String src = sourceForFile(problem.getOriginatingFileName(), sourceFiles);
                    if (src != null) {
                        int[] startLC = offsetToLineCol(src, start);
                        int[] endLC = offsetToLineCol(src, end + 1);
                        d.startLine = startLC[0];
                        d.startChar = startLC[1];
                        d.endLine = endLC[0];
                        d.endChar = endLC[1];
                    }
                }

                // Deprecation tag
                if (problem.getID() == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedType
                        || problem.getID() == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedMethod
                        || problem.getID() == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedField) {
                    d.tags = List.of(2); // DiagnosticTag.Deprecated
                }

                diagnostics.add(d);
            }
        };

        CompilerOptions options = buildOptions(sourceLevel);

        Compiler compiler = new Compiler(
                nameEnv,
                DefaultErrorHandlingPolicies.proceedWithAllProblems(),
                options,
                requestor,
                new DefaultProblemFactory(Locale.ENGLISH));

        ICompilationUnit[] units = sourceFiles.entrySet().stream()
                .filter(e -> e.getKey().endsWith(".java"))
                .map(e -> (ICompilationUnit) new InMemoryCompilationUnit(e.getKey(), e.getValue()))
                .toArray(ICompilationUnit[]::new);

        if (units.length > 0) {
            compiler.compile(units);
        }

        nameEnv.cleanup();
        return diagnostics;
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    private CompilerOptions buildOptions(String sourceLevel) {
        Map<String, String> opts = new HashMap<>();
        String ver = resolveVersion(sourceLevel);
        opts.put(CompilerOptions.OPTION_Source, ver);
        opts.put(CompilerOptions.OPTION_Compliance, ver);
        opts.put(CompilerOptions.OPTION_TargetPlatform, ver);
        opts.put(CompilerOptions.OPTION_ReportDeprecation, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedImport, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedLocal, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportNullReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportPotentialNullReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUncheckedTypeOperation, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportRawTypeReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_SuppressWarnings, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_Process_Annotations, CompilerOptions.DISABLED);
        return new CompilerOptions(opts);
    }

    private String resolveVersion(String level) {
        return switch (level.trim()) {
            case "8", "1.8" -> CompilerOptions.VERSION_1_8;
            case "11" -> CompilerOptions.VERSION_11;
            case "17" -> CompilerOptions.VERSION_17;
            case "21" -> CompilerOptions.VERSION_21;
            case "22" -> CompilerOptions.VERSION_22;
            default -> CompilerOptions.VERSION_21;
        };
    }

    private String originatingUri(char[] fileName, Map<String, String> sourceFiles) {
        if (fileName == null) return "unknown";
        String name = new String(fileName).replace('\\', '/');
        for (String uri : sourceFiles.keySet()) {
            if (uri.replace('\\', '/').endsWith(name) || name.endsWith(uriPath(uri))) {
                return uri;
            }
        }
        return "file://" + name;
    }

    private String sourceForFile(char[] fileName, Map<String, String> sourceFiles) {
        String uri = originatingUri(fileName, sourceFiles);
        return sourceFiles.get(uri);
    }

    private String uriPath(String uri) {
        try {
            return new java.net.URI(uri).getPath().replace('\\', '/');
        } catch (Exception e) {
            return uri;
        }
    }

    /** Convert a 0-based char offset in source to [line, col] (both 0-based). */
    static int[] offsetToLineCol(String source, int offset) {
        offset = Math.min(offset, source.length());
        int line = 0, col = 0;
        for (int i = 0; i < offset; i++) {
            if (source.charAt(i) == '\n') { line++; col = 0; }
            else { col++; }
        }
        return new int[]{line, col};
    }
}
