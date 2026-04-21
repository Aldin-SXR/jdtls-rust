package com.jdtls.ecjbridge;

import org.eclipse.jdt.internal.compiler.*;
import org.eclipse.jdt.internal.compiler.Compiler;
import org.eclipse.jdt.internal.compiler.apt.dispatch.BatchAnnotationProcessorManager;
import org.eclipse.jdt.internal.compiler.ast.CompilationUnitDeclaration;
import org.eclipse.jdt.internal.compiler.batch.Main;
import org.eclipse.jdt.internal.compiler.env.ICompilationUnit;
import org.eclipse.jdt.internal.compiler.impl.CompilerOptions;
import org.eclipse.jdt.internal.compiler.problem.DefaultProblemFactory;

import java.io.IOException;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import java.util.logging.Logger;
import java.util.stream.Stream;

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
            ClassFile[] classFiles = result.getClassFiles();
            if (!result.hasErrors() && classFiles != null) {
                for (ClassFile cf : classFiles) {
                    String name = new String(cf.fileName()).replace('\\', '/');
                    if (name.endsWith(".class")) name = name.substring(0, name.length() - 6);
                    nameEnv.addCompiledClass(name, cf.getBytes());
                }
            }
            // Collect problems
            var problems = result.getAllProblems();
            if (problems == null) {
                return;
            }
            for (var problem : problems) {
                BridgeDiagnostic d = new BridgeDiagnostic();
                d.uri = originatingUri(problem.getOriginatingFileName(), sourceFiles);
                d.startLine = problem.getSourceLineNumber() - 1; // LSP is 0-based
                d.startChar = 0; // ECJ gives column for some problems
                d.endLine = d.startLine;
                d.endChar = 999; // ECJ doesn't always give end position; clients will clamp
                d.severity = problem.isError() ? 1 : problem.isWarning() ? 2 : 3;
                d.message = problem.getMessage();
                d.code = String.valueOf(problem.getID());
                d.categoryId = problem.getCategoryID();

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
                int pid = problem.getID();
                if (pid == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedType
                        || pid == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedMethod
                        || pid == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedField
                        || pid == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedConstructor
                        || pid == org.eclipse.jdt.core.compiler.IProblem.UsingDeprecatedModule) {
                    d.tags = List.of(2); // DiagnosticTag.Deprecated
                }
                // Unnecessary tag (faded-out display in editors)
                if (pid == org.eclipse.jdt.core.compiler.IProblem.UnusedImport
                        || pid == org.eclipse.jdt.core.compiler.IProblem.LocalVariableIsNeverUsed
                        || pid == org.eclipse.jdt.core.compiler.IProblem.ArgumentIsNeverUsed
                        || pid == org.eclipse.jdt.core.compiler.IProblem.DeadCode) {
                    d.tags = List.of(1); // DiagnosticTag.Unnecessary
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
        AnnotationProcessingSession aptSession = configureAnnotationProcessing(compiler, classpath, sourceLevel);

        ICompilationUnit[] units = sourceFiles.entrySet().stream()
                .filter(e -> e.getKey().endsWith(".java"))
                .map(e -> (ICompilationUnit) new InMemoryCompilationUnit(e.getKey(), e.getValue()))
                .toArray(ICompilationUnit[]::new);

        try {
            if (units.length > 0) {
                compiler.compile(units);
            }
        } finally {
            nameEnv.cleanup();
            if (aptSession != null) {
                aptSession.cleanup();
            }
        }
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
        opts.put(CompilerOptions.OPTION_ReportUnusedParameter, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedParameterIncludeDocCommentReference, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportNullReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportPotentialNullReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUncheckedTypeOperation, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportRawTypeReference, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedDeclaredThrownException, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedDeclaredThrownExceptionIncludeDocCommentReference, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportUnusedDeclaredThrownExceptionExemptExceptionAndThrowable, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportUnnecessaryTypeCheck, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportDeadCode, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportDeadCodeInTrivialIfStatement, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportNoEffectAssignment, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportUnusedObjectAllocation, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_DocCommentSupport, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportMissingJavadocTags, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportMissingJavadocTagsVisibility, CompilerOptions.PRIVATE);
        opts.put(CompilerOptions.OPTION_ReportInvalidJavadoc, CompilerOptions.WARNING);
        opts.put(CompilerOptions.OPTION_ReportInvalidJavadocTags, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_ReportInvalidJavadocTagsVisibility, CompilerOptions.PRIVATE);
        opts.put(CompilerOptions.OPTION_SuppressWarnings, CompilerOptions.ENABLED);
        opts.put(CompilerOptions.OPTION_Process_Annotations, CompilerOptions.ENABLED);
        return new CompilerOptions(opts);
    }

    private AnnotationProcessingSession configureAnnotationProcessing(
            Compiler compiler, List<String> classpath, String sourceLevel) {
        if (!compiler.options.processAnnotations || classpath.isEmpty()) {
            return null;
        }

        String joinedClasspath = String.join(java.io.File.pathSeparator, classpath);
        String version = resolveVersion(sourceLevel);

        try {
            Path generatedSources = Files.createTempDirectory("jdtls-rust-apt-src");
            Path generatedClasses = Files.createTempDirectory("jdtls-rust-apt-bin");
            String[] args = {
                    "-classpath", joinedClasspath,
                    "-processorpath", joinedClasspath,
                    "-source", version,
                    "-target", version,
                    "-s", generatedSources.toString(),
                    "-d", generatedClasses.toString(),
            };

            PrintWriter out = new PrintWriter(new StringWriter());
            PrintWriter err = new PrintWriter(new StringWriter());
            Main batchMain = new Main(out, err, false, new HashMap<>());
            batchMain.configure(args);
            batchMain.batchCompiler = compiler;

            BatchAnnotationProcessorManager aptManager = new BatchAnnotationProcessorManager();
            aptManager.configure(batchMain, args);
            aptManager.setOut(out);
            aptManager.setErr(err);
            compiler.annotationProcessorManager = aptManager;

            return new AnnotationProcessingSession(generatedSources, generatedClasses);
        } catch (Exception e) {
            LOG.warning("Annotation processing disabled: " + e.getMessage());
            return null;
        }
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

    /** Convert 0-based line/col to a char offset in source. */
    public static int lineColToOffset(String source, int line, int col) {
        int cur = 0;
        for (int i = 0; i < line && cur < source.length(); i++) {
            int nl = source.indexOf('\n', cur);
            if (nl < 0) return source.length();
            cur = nl + 1;
        }
        return Math.min(cur + col, source.length());
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

    private static final class AnnotationProcessingSession {
        private final Path generatedSources;
        private final Path generatedClasses;

        private AnnotationProcessingSession(Path generatedSources, Path generatedClasses) {
            this.generatedSources = generatedSources;
            this.generatedClasses = generatedClasses;
        }

        private void cleanup() {
            deleteRecursively(generatedSources);
            deleteRecursively(generatedClasses);
        }

        private static void deleteRecursively(Path root) {
            if (root == null || !Files.exists(root)) {
                return;
            }
            try (Stream<Path> paths = Files.walk(root)) {
                paths.sorted(Comparator.reverseOrder()).forEach(path -> {
                    try {
                        Files.deleteIfExists(path);
                    } catch (IOException e) {
                        LOG.fine("Failed to delete temp path " + path + ": " + e.getMessage());
                    }
                });
            } catch (IOException e) {
                LOG.fine("Failed to walk temp path " + root + ": " + e.getMessage());
            }
        }
    }
}
