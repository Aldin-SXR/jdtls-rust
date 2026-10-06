package org.eclipse.jdt.ls.core.internal;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.eclipse.core.internal.resources.Resource;
import org.eclipse.core.resources.IProject;
import org.eclipse.core.resources.ResourcesPlugin;
import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.ls.core.internal.managers.ProjectsManager;
import org.eclipse.jdt.ls.core.internal.managers.StandardProjectsManager;
import org.eclipse.jdt.ls.core.internal.preferences.PreferenceManager;
import org.eclipse.jdt.ls.core.internal.preferences.Preferences;

/** Calls the real manager and resource model; no replacement policy. */
public final class ProjectsManagerTestCommand implements IDelegateCommandHandler {
    @Override
    @SuppressWarnings("unchecked")
    public Object executeCommand(String command, List<Object> arguments, IProgressMonitor ignored) throws Exception {
        Map<String, Object> input = (Map<String, Object>) arguments.get(0);
        Preferences preferences = new Preferences();
        PreferenceManager preferenceManager = new PreferenceManager() {
            @Override public Preferences getPreferences() { return preferences; }
            @Override public org.eclipse.jdt.ls.core.internal.preferences.ClientPreferences getClientPreferences() {
                return JavaLanguageServerPlugin.getPreferencesManager().getClientPreferences();
            }
        };
        ProjectsManager manager = new StandardProjectsManager(preferenceManager);
        NullProgressMonitor monitor = new NullProgressMonitor();
        if ("initializeEmpty".equals(input.get("api"))) {
            manager.initializeProjects(Collections.emptyList(), monitor);
            JobHelpers.waitForJobsToComplete();
            List<Map<String, Object>> projects = new ArrayList<>();
            for (IProject project : ResourcesPlugin.getWorkspace().getRoot().getProjects()) {
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("name", project.getName());
                result.put("location", project.getLocation().toOSString());
                result.put("exists", project.exists());
                result.put("isDefault", project.equals(ProjectsManager.getDefaultProject()));
                projects.add(result);
            }
            return projects;
        }
        IProject project = ResourcesPlugin.getWorkspace().getRoot().getProject((String) input.get("project"));
        if (!project.exists() || !project.hasNature(JavaCore.NATURE_ID)) {
            throw new IllegalArgumentException("Fixture is not a Java project: " + project.getName());
        }
        List<String> original = JavaLanguageServerPlugin.getPreferencesManager().getPreferences().getResourceFilters();
        try {
            // Upstream imports directly before configureFilters is called.
            // LSP initialize has already installed filters; restore that initial
            // resource state using the actual manager API before the test steps.
            preferences.setResourceFilters(null);
            manager.configureFilters(monitor);
            JobHelpers.waitForJobsToComplete();
            preferences.setResourceFilters(original);
            List<Map<String, Object>> results = new ArrayList<>();
            for (Map<String, Object> operation : (List<Map<String, Object>>) input.get("operations")) {
                if (operation.containsKey("patterns")) {
                    preferences.setResourceFilters((List<String>) operation.get("patterns"));
                    manager.configureFilters(monitor);
                    JobHelpers.waitForJobsToComplete();
                }
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("patterns", preferences.getResourceFilters());
                List<Boolean> filtered = new ArrayList<>();
                for (String path : (List<String>) input.get("paths")) {
                    Resource resource = (Resource) project.getFolder(path);
                    filtered.add(resource.isFiltered());
                }
                result.put("filtered", filtered);
                results.add(result);
            }
            return results;
        } finally {
            preferences.setResourceFilters(original);
            manager.configureFilters(monitor);
            JobHelpers.waitForJobsToComplete();
        }
    }
}
