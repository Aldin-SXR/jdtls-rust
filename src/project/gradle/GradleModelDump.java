import java.io.File;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.gradle.tooling.GradleConnector;
import org.gradle.tooling.ModelBuilder;
import org.gradle.tooling.ProjectConnection;
import org.gradle.tooling.model.GradleModuleVersion;
import org.gradle.tooling.model.eclipse.ClasspathAttribute;
import org.gradle.tooling.model.eclipse.EclipseBuildCommand;
import org.gradle.tooling.model.eclipse.EclipseClasspathContainer;
import org.gradle.tooling.model.eclipse.EclipseExternalDependency;
import org.gradle.tooling.model.eclipse.EclipseJavaSourceSettings;
import org.gradle.tooling.model.eclipse.EclipseLinkedResource;
import org.gradle.tooling.model.eclipse.EclipseProject;
import org.gradle.tooling.model.eclipse.EclipseProjectDependency;
import org.gradle.tooling.model.eclipse.EclipseProjectNature;
import org.gradle.tooling.model.eclipse.EclipseSourceDirectory;
import org.gradle.tooling.model.build.BuildEnvironment;

/**
 * Fetches the Gradle Tooling API models Buildship consumes when it imports a
 * build and writes them as JSON. Request lines are `key=value`.
 */
public class GradleModelDump {

    public static void main(String[] args) throws Exception {
        List<String> lines = Files.readAllLines(new File(args[0]).toPath(), StandardCharsets.UTF_8);
        String dir = null, out = null, dist = "default", distValue = "", javaHome = "", userHome = "";
        boolean offline = false;
        List<String> jvmArgs = new ArrayList<>();
        List<String> buildArgs = new ArrayList<>();
        List<String> aptScripts = new ArrayList<>();
        for (String line : lines) {
            int eq = line.indexOf('=');
            if (eq < 0) {
                continue;
            }
            String k = line.substring(0, eq), v = line.substring(eq + 1);
            switch (k) {
                case "dir" -> dir = v;
                case "out" -> out = v;
                case "dist" -> dist = v;
                case "distvalue" -> distValue = v;
                case "javahome" -> javaHome = v;
                case "userhome" -> userHome = v;
                case "offline" -> offline = v.equals("1");
                case "jvmarg" -> jvmArgs.add(v);
                case "arg" -> buildArgs.add(v);
                default -> {
                }
            }
        }
        // the jdt.ls init scripts only act inside an Eclipse application
        System.setProperty("eclipse.application", "org.eclipse.jdt.ls.core.id1");
        StringBuilder sb = new StringBuilder();
        try {
            GradleConnector connector = GradleConnector.newConnector().forProjectDirectory(new File(dir));
            switch (dist) {
                case "wrapper" -> connector.useBuildDistribution();
                case "version" -> connector.useGradleVersion(distValue);
                case "local" -> connector.useInstallation(new File(distValue));
                default -> connector.useGradleVersion(distValue);
            }
            if (!userHome.isEmpty()) {
                connector.useGradleUserHomeDir(new File(userHome));
            }
            try (ProjectConnection connection = connector.connect()) {
                BuildEnvironment env = configure(connection.model(BuildEnvironment.class), javaHome, jvmArgs, buildArgs, offline).get();
                EclipseProject root = configure(connection.model(EclipseProject.class), javaHome, jvmArgs, buildArgs, offline).get();
                sb.append("{\"gradleVersion\":");
                str(sb, env.getGradle().getGradleVersion());
                sb.append(",\"javaHome\":");
                str(sb, env.getJava().getJavaHome().getAbsolutePath());
                sb.append(",\"project\":");
                project(sb, root);
                sb.append('}');
            }
        } catch (Throwable t) {
            sb.setLength(0);
            sb.append("{\"error\":");
            str(sb, String.valueOf(t.getMessage() == null ? t.toString() : t.getMessage()));
            sb.append(",\"causes\":[");
            Throwable c = t;
            boolean first = true;
            while (c != null) {
                if (!first) {
                    sb.append(',');
                }
                first = false;
                str(sb, c.getClass().getName() + ": " + c.getMessage());
                c = c.getCause() == c ? null : c.getCause();
            }
            sb.append("],\"stack\":");
            StringWriter sw = new StringWriter();
            t.printStackTrace(new PrintWriter(sw));
            str(sb, sw.toString());
            sb.append('}');
        }
        Files.writeString(new File(out).toPath(), sb.toString(), StandardCharsets.UTF_8);
        System.exit(0);
    }

    private static <T> ModelBuilder<T> configure(ModelBuilder<T> builder, String javaHome, List<String> jvmArgs, List<String> buildArgs, boolean offline) {
        if (!javaHome.isEmpty()) {
            builder.setJavaHome(new File(javaHome));
        }
        List<String> all = new ArrayList<>(buildArgs);
        if (offline) {
            all.add("--offline");
        }
        builder.withArguments(all);
        builder.setJvmArguments(jvmArgs);
        return builder;
    }

    private static void project(StringBuilder sb, EclipseProject p) {
        sb.append("{\"name\":");
        str(sb, p.getName());
        sb.append(",\"dir\":");
        file(sb, p.getProjectDirectory());
        sb.append(",\"gradlePath\":");
        str(sb, p.getGradleProject().getPath());
        sb.append(",\"buildDir\":");
        try {
            file(sb, p.getGradleProject().getBuildDirectory());
        } catch (RuntimeException e) {
            sb.append("null");
        }
        sb.append(",\"output\":");
        try {
            str(sb, p.getOutputLocation() == null ? null : p.getOutputLocation().getPath());
        } catch (RuntimeException e) {
            sb.append("null");
        }
        sb.append(",\"sources\":[");
        boolean first = true;
        for (EclipseSourceDirectory s : p.getSourceDirectories()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            sb.append("{\"path\":");
            str(sb, s.getPath());
            sb.append(",\"dir\":");
            file(sb, s.getDirectory());
            sb.append(",\"output\":");
            try {
                str(sb, s.getOutput());
            } catch (RuntimeException e) {
                sb.append("null");
            }
            sb.append(",\"includes\":");
            try {
                list(sb, s.getIncludes());
            } catch (RuntimeException e) {
                sb.append("[]");
            }
            sb.append(",\"excludes\":");
            try {
                list(sb, s.getExcludes());
            } catch (RuntimeException e) {
                sb.append("[]");
            }
            sb.append(",\"attributes\":");
            try {
                attributes(sb, s.getClasspathAttributes());
            } catch (RuntimeException e) {
                sb.append("{}");
            }
            sb.append('}');
        }
        sb.append("],\"classpath\":[");
        first = true;
        for (EclipseExternalDependency d : p.getClasspath()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            sb.append("{\"file\":");
            file(sb, d.getFile());
            sb.append(",\"source\":");
            file(sb, d.getSource());
            sb.append(",\"javadoc\":");
            file(sb, d.getJavadoc());
            sb.append(",\"exported\":").append(d.isExported());
            GradleModuleVersion gav = d.getGradleModuleVersion();
            sb.append(",\"group\":");
            str(sb, gav == null ? null : gav.getGroup());
            sb.append(",\"module\":");
            str(sb, gav == null ? null : gav.getName());
            sb.append(",\"version\":");
            str(sb, gav == null ? null : gav.getVersion());
            sb.append(",\"attributes\":");
            try {
                attributes(sb, d.getClasspathAttributes());
            } catch (RuntimeException e) {
                sb.append("{}");
            }
            sb.append('}');
        }
        sb.append("],\"projectDependencies\":[");
        first = true;
        for (EclipseProjectDependency d : p.getProjectDependencies()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            sb.append("{\"path\":");
            str(sb, d.getPath());
            sb.append(",\"exported\":").append(d.isExported());
            sb.append(",\"attributes\":");
            try {
                attributes(sb, d.getClasspathAttributes());
            } catch (RuntimeException e) {
                sb.append("{}");
            }
            sb.append('}');
        }
        sb.append("],\"natures\":[");
        first = true;
        try {
            for (EclipseProjectNature n : p.getProjectNatures()) {
                if (!first) {
                    sb.append(',');
                }
                first = false;
                str(sb, n.getId());
            }
        } catch (RuntimeException e) {
            // older Gradle versions
        }
        sb.append("],\"buildCommands\":[");
        first = true;
        try {
            for (EclipseBuildCommand c : p.getBuildCommands()) {
                if (!first) {
                    sb.append(',');
                }
                first = false;
                sb.append("{\"name\":");
                str(sb, c.getName());
                sb.append(",\"arguments\":");
                Map<String, String> m = new LinkedHashMap<>(c.getArguments());
                map(sb, m);
                sb.append('}');
            }
        } catch (RuntimeException e) {
            // older Gradle versions
        }
        sb.append("],\"containers\":[");
        first = true;
        try {
            for (EclipseClasspathContainer c : p.getClasspathContainers()) {
                if (!first) {
                    sb.append(',');
                }
                first = false;
                sb.append("{\"path\":");
                str(sb, c.getPath());
                sb.append(",\"exported\":").append(c.isExported());
                sb.append('}');
            }
        } catch (RuntimeException e) {
            // older Gradle versions
        }
        sb.append("],\"linkedResources\":[");
        first = true;
        for (EclipseLinkedResource r : p.getLinkedResources()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            sb.append("{\"name\":");
            str(sb, r.getName());
            sb.append(",\"type\":");
            str(sb, r.getType());
            sb.append(",\"location\":");
            str(sb, r.getLocation());
            sb.append(",\"locationUri\":");
            str(sb, r.getLocationUri());
            sb.append('}');
        }
        sb.append("],\"java\":");
        try {
            EclipseJavaSourceSettings j = p.getJavaSourceSettings();
            if (j == null) {
                sb.append("null");
            } else {
                sb.append("{\"source\":");
                str(sb, j.getSourceLanguageLevel() == null ? null : j.getSourceLanguageLevel().toString());
                sb.append(",\"target\":");
                try {
                    str(sb, j.getTargetBytecodeVersion() == null ? null : j.getTargetBytecodeVersion().toString());
                } catch (RuntimeException e) {
                    sb.append("null");
                }
                sb.append(",\"jdkHome\":");
                try {
                    file(sb, j.getJdk() == null ? null : j.getJdk().getJavaHome());
                } catch (RuntimeException e) {
                    sb.append("null");
                }
                sb.append('}');
            }
        } catch (RuntimeException e) {
            sb.append("null");
        }
        sb.append(",\"children\":[");
        first = true;
        for (EclipseProject c : p.getChildren()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            project(sb, c);
        }
        sb.append("]}");
    }

    private static void attributes(StringBuilder sb, Iterable<? extends ClasspathAttribute> attributes) {
        Map<String, String> m = new LinkedHashMap<>();
        for (ClasspathAttribute a : attributes) {
            m.put(a.getName(), a.getValue());
        }
        map(sb, m);
    }

    private static void map(StringBuilder sb, Map<String, String> m) {
        sb.append('{');
        boolean first = true;
        for (Map.Entry<String, String> e : m.entrySet()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            str(sb, e.getKey());
            sb.append(':');
            str(sb, e.getValue());
        }
        sb.append('}');
    }

    private static void list(StringBuilder sb, List<String> l) {
        sb.append('[');
        boolean first = true;
        for (String s : l) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            str(sb, s);
        }
        sb.append(']');
    }

    private static void file(StringBuilder sb, File f) {
        str(sb, f == null ? null : f.getAbsolutePath());
    }

    private static void str(StringBuilder sb, String s) {
        if (s == null) {
            sb.append("null");
            return;
        }
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> {
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
                }
            }
        }
        sb.append('"');
    }
}
