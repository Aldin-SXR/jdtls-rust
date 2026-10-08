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
        return compile(sourceFiles, classpath, sourceLevel, null, null);
    }

    /**
     * @param roots            units to compile (all {@code sourceFiles} when null);
     *                         the others are looked up on demand
     * @param expectedPackages package each unit must declare (dotted), checked
     *                         like JDT's package fragments
     */
    public List<BridgeDiagnostic> compile(
            Map<String, String> sourceFiles,
            List<String> classpath,
            String sourceLevel,
            List<String> roots,
            Map<String, String> expectedPackages) {

        return compile(sourceFiles, classpath, sourceLevel, roots, expectedPackages, null);
    }

    public List<BridgeDiagnostic> compile(Map<String, String> sourceFiles, List<String> classpath,
            String sourceLevel, List<String> roots, Map<String, String> expectedPackages,
            Map<String, String> generatedSourceOutput) {
        InMemoryNameEnvironment nameEnv = new InMemoryNameEnvironment(sourceFiles, classpath);
        nameEnv.setExpectedPackages(expectedPackages);
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
                d.problemId = problem.getID();
                d.sourceStart = problem.getSourceStart();
                d.sourceEnd = problem.getSourceEnd();
                d.sourceLine = problem.getSourceLineNumber();
                String[] args = problem.getArguments();
                d.arguments = args == null ? List.of() : Arrays.asList(args);

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

        // The Java builder compiles the source files in the order it visits
        // them (the caller's root order); a type defined twice is reported on
        // the later unit.
        ICompilationUnit[] units = (roots == null
                ? sourceFiles.keySet().stream().filter(k -> k.endsWith(".java"))
                : roots.stream().distinct().filter(sourceFiles::containsKey))
                .map(k -> (ICompilationUnit) new InMemoryCompilationUnit(k, sourceFiles.get(k),
                        nameEnv.expectedPackage(k)))
                .toArray(ICompilationUnit[]::new);

        try {
            if (units.length > 0) {
                compiler.compile(units);
            }
        } finally {
            nameEnv.cleanup();
            if (aptSession != null) {
                if (generatedSourceOutput != null) {
                    try (Stream<Path> files = Files.walk(aptSession.generatedSources)) {
                        for (Path path : files.filter(p -> p.toString().endsWith(".java")).toList()) {
                            generatedSourceOutput.put(aptSession.generatedSources.relativize(path).toString().replace('\\', '/'), Files.readString(path));
                        }
                    } catch (IOException e) {
                        LOG.warning("Cannot read annotation processor output: " + e.getMessage());
                    }
                }
                aptSession.cleanup();
            }
        }
        return diagnostics;
    }

    /**
     * Compile the sources and return the class files named in {@code names}
     * (binary names with '/' separators), base64 encoded.  Class files are
     * kept even when the unit has errors (like the Java builder).
     */
    public Map<String, String> compiledClasses(Map<String, String> sourceFiles, List<String> classpath,
            String sourceLevel, List<String> names) {
        Map<String, String> out = new HashMap<>();
        Set<String> wanted = new HashSet<>(names == null ? List.of() : names);
        InMemoryNameEnvironment nameEnv = new InMemoryNameEnvironment(sourceFiles, classpath);
        ICompilerRequestor requestor = result -> {
            ClassFile[] classFiles = result.getClassFiles();
            if (classFiles == null) {
                return;
            }
            for (ClassFile cf : classFiles) {
                String name = new String(cf.fileName()).replace('\\', '/');
                if (name.endsWith(".class")) name = name.substring(0, name.length() - 6);
                if (!result.hasErrors()) {
                    nameEnv.addCompiledClass(name, cf.getBytes());
                }
                if (wanted.contains(name)) {
                    out.put(name, Base64.getEncoder().encodeToString(cf.getBytes()));
                }
            }
        };
        CompilerOptions compilerOptions = buildOptions(sourceLevel);
        compilerOptions.processAnnotations = false;
        Compiler compiler = new Compiler(nameEnv, DefaultErrorHandlingPolicies.proceedWithAllProblems(),
                compilerOptions, requestor, new DefaultProblemFactory(Locale.ENGLISH));
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
        }
        return out;
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    private CompilerOptions buildOptions(String sourceLevel) {
        Map<String, String> opts = BridgeOptions.map(sourceLevel);
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
        return BridgeOptions.version(level);
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
