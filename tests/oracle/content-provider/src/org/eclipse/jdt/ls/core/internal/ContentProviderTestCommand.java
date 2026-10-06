package org.eclipse.jdt.ls.core.internal;

import java.io.PrintWriter;
import java.io.StringWriter;
import java.net.URI;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.eclipse.core.runtime.ILogListener;
import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.core.runtime.IStatus;
import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.core.runtime.Platform;
import org.eclipse.jdt.core.IClassFile;
import org.eclipse.jdt.ls.core.internal.managers.ContentProviderManager;
import org.eclipse.jdt.ls.core.internal.preferences.PreferenceManager;
import org.eclipse.jdt.ls.core.internal.preferences.Preferences;

/** Test adapter only: invokes the unmodified real manager, registry and providers. */
public final class ContentProviderTestCommand implements IDelegateCommandHandler {
    @Override
    @SuppressWarnings("unchecked")
    public Object executeCommand(String command, List<Object> arguments, IProgressMonitor ignored) throws Exception {
        Map<String, Object> input = (Map<String, Object>) arguments.get(0);
        Preferences preferences = Preferences.createFrom(Map.of(
            "java.contentProvider.preferred", input.getOrDefault("preferred", List.of())));
        PreferenceManager preferenceManager = new PreferenceManager() {
            @Override public Preferences getPreferences() { return preferences; }
        };
        ContentProviderManager manager = new ContentProviderManager(preferenceManager);
        NullProgressMonitor monitor = new NullProgressMonitor();
        FakeContentProvider.preferences = null;
        FakeContentProvider.returnValue = null;
        List<Map<String, Object>> results = new ArrayList<>();
        for (Map<String, Object> operation : (List<Map<String, Object>>) input.get("operations")) {
            monitor.setCanceled(false);
            String fakeKind = (String) operation.get("fakeKind");
            FakeContentProvider.returnValue = switch (fakeKind) {
                case "text" -> operation.get("fakeValue");
                case "exception" -> new Exception((String) operation.get("fakeValue"));
                case "cancel" -> monitor;
                default -> null;
            };
            List<String> errors = new ArrayList<>();
            List<String> infos = new ArrayList<>();
            ILogListener listener = (status, plugin) -> {
                StringWriter trace = new StringWriter();
                if (status.getException() != null) {
                    status.getException().printStackTrace(new PrintWriter(trace));
                }
                String text = status.getMessage() + "\n" + trace;
                if (status.matches(IStatus.ERROR)) errors.add(text);
                if (status.matches(IStatus.INFO)) infos.add(text);
            };
            Map<String, Object> result = new LinkedHashMap<>();
            Platform.addLogListener(listener);
            try {
                String uriText = (String) operation.get("uri");
                URI uri = uriText == null ? null : URI.create(uriText);
                String api = (String) operation.get("api");
                if ("content".equals(api)) {
                    result.put("content", manager.getContent(uri, monitor));
                } else {
                    IClassFile classFile = uri == null ? null : JDTUtils.resolveClassFile(uri);
                    if (uri != null && classFile == null) {
                        throw new IllegalArgumentException("Fixture class file did not resolve: " + uri);
                    }
                    if ("source".equals(api)) {
                        result.put("content", manager.getSource(classFile, monitor));
                    } else {
                        DecompilerResult decompiled = manager.getSourceResult(classFile, monitor);
                        result.put("content", decompiled == null ? null : decompiled.getContent());
                        result.put("originalLineMappings", decompiled == null ? null : decompiled.getOriginalLineMappings());
                        result.put("decompiledLineMappings", decompiled == null ? null : decompiled.getDecompiledLineMappings());
                    }
                }
            } finally {
                Platform.removeLogListener(listener);
            }
            result.put("errors", errors);
            result.put("infos", infos);
            result.put("canceled", monitor.isCanceled());
            result.put("preferencesMatch", FakeContentProvider.preferences == preferences);
            results.add(result);
        }
        FakeContentProvider.preferences = null;
        FakeContentProvider.returnValue = null;
        return results;
    }
}
