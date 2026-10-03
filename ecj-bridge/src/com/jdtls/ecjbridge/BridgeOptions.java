package com.jdtls.ecjbridge;

import java.util.HashMap;
import java.util.Map;

import org.eclipse.jdt.internal.compiler.impl.CompilerOptions;

/**
 * Compiler options for the request being served.  The Rust server computes
 * the effective option map (jdt.ls defaults + project settings) and sends it
 * with every request; this class only layers it over the JDT defaults.
 */
final class BridgeOptions {
    private static final ThreadLocal<Map<String, String>> CURRENT = ThreadLocal.withInitial(Map::of);

    private BridgeOptions() {}

    static void setCurrent(Map<String, String> options) {
        CURRENT.set(options == null ? Map.of() : options);
    }

    /** Normalise a Java version ("8" → "1.8") and clamp to what ECJ supports. */
    static String version(String level) {
        String v = level == null ? "" : level.trim();
        if (v.startsWith("1.")) {
            v = v.substring(2);
        }
        int n;
        try {
            n = Integer.parseInt(v);
        } catch (NumberFormatException e) {
            n = 21;
        }
        long latest = CompilerOptions.versionToJdkLevel(CompilerOptions.getLatestVersion());
        String candidate = n <= 8 ? "1." + n : Integer.toString(n);
        long jdk = CompilerOptions.versionToJdkLevel(candidate);
        if (jdk == 0 || jdk > latest) {
            return CompilerOptions.getLatestVersion();
        }
        return candidate;
    }

    /** JavaCore defaults (`JavaCorePreferenceInitializer`) + request options. */
    static Map<String, String> map(String sourceLevel) {
        Map<String, String> m = new HashMap<>();
        new CompilerOptions().getMap().forEach((k, v) -> {
            if (v != null) {
                m.put(k, v);
            }
        });
        String ver = version(sourceLevel);
        m.put(CompilerOptions.OPTION_Source, ver);
        m.put(CompilerOptions.OPTION_Compliance, ver);
        m.put(CompilerOptions.OPTION_TargetPlatform, ver);
        m.put(CompilerOptions.OPTION_LocalVariableAttribute, CompilerOptions.GENERATE);
        m.put(CompilerOptions.OPTION_PreserveUnusedLocal, CompilerOptions.PRESERVE);
        m.put(CompilerOptions.OPTION_TaskTags, "TODO,FIXME,XXX");
        m.put(CompilerOptions.OPTION_TaskPriorities, "NORMAL,HIGH,NORMAL");
        m.put(CompilerOptions.OPTION_DocCommentSupport, CompilerOptions.ENABLED);
        m.put(CompilerOptions.OPTION_SuppressWarnings, CompilerOptions.ENABLED);
        m.putAll(CURRENT.get());
        // The request's compliance keys must agree with the clamped version.
        for (String key : new String[] { CompilerOptions.OPTION_Source, CompilerOptions.OPTION_Compliance,
                CompilerOptions.OPTION_TargetPlatform }) {
            m.put(key, version(m.get(key)));
        }
        return m;
    }

    static CompilerOptions compilerOptions(String sourceLevel) {
        return new CompilerOptions(map(sourceLevel));
    }

    /**
     * Classpath + JDK system library for binding resolution.  ECJ >= 3.40
     * requires a system library to be present when resolving bindings.
     */
    static void configureEnvironment(org.eclipse.jdt.core.dom.ASTParser parser, String[] classpath) {
        parser.setEnvironment(classpath, null, null, /* includeRunningVMBootclasspath */ true);
    }
}
