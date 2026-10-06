package org.eclipse.jdt.ls.core.internal;

import java.io.File;
import java.net.URL;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.eclipse.core.runtime.FileLocator;
import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.core.runtime.Platform;
import org.eclipse.core.runtime.URIUtil;
import org.eclipse.core.resources.ResourcesPlugin;
import org.eclipse.jdt.core.IJavaProject;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.internal.launching.StandardVMType;
import org.eclipse.jdt.launching.IVMInstall;
import org.eclipse.jdt.launching.IVMInstall2;
import org.eclipse.jdt.launching.JavaRuntime;
import org.eclipse.jdt.launching.LibraryLocation;
import org.eclipse.jdt.launching.environments.IExecutionEnvironment;
import org.eclipse.jdt.ls.core.internal.managers.ProjectsManager;
import org.eclipse.jdt.ls.core.internal.preferences.Preferences;

/** Calls actual core and launching APIs; does not implement VM policy. */
public final class JvmConfigurationTestCommand implements IDelegateCommandHandler {
    @Override
    @SuppressWarnings("unchecked")
    public Object executeCommand(String command, List<Object> arguments, IProgressMonitor ignored) throws Exception {
        Map<String, Object> input = (Map<String, Object>) arguments.get(0);
        String api = (String) input.get("api");
        if ("setup".equals(api)) {
            TestVMType.setTestJREAsDefault("21");
            JobHelpers.waitForJobsToComplete();
            URL url = FileLocator.toFileURL(Platform.getBundle(JavaLanguageServerTestPlugin.PLUGIN_ID).getEntry("/fakejdk2/21a"));
            return Map.of("fake", TestVMType.getFakeJDKsLocation().getAbsolutePath(), "native", URIUtil.toFile(URIUtil.toURI(url)).getAbsolutePath());
        }
        IVMInstall original = JavaRuntime.getDefaultVMInstall();
        if ("default".equals(api)) {
            boolean changed = JVMConfigurator.configureDefaultVM((String) input.get("home"));
            IVMInstall current = JavaRuntime.getDefaultVMInstall();
            return Map.of("changed", changed, "different", !original.equals(current), "id", current.getId());
        }
        if ("preview".equals(api)) {
            ProjectsManager.createJavaProject(ProjectsManager.getDefaultProject(), new NullProgressMonitor());
            IJavaProject defaultProject = JavaCore.create(ProjectsManager.getDefaultProject());
            IJavaProject invisibleProject = JavaCore.create(ResourcesPlugin.getWorkspace().getRoot().getProject((String) input.get("project")));
            JVMConfigurator listener = new JVMConfigurator();
            JavaRuntime.addVMInstallChangedListener(listener);
            try {
                List<Object> results = new ArrayList<>();
                for (String version : (List<String>) input.get("versions")) {
                    TestVMType.setTestJREAsDefault(version);
                    JobHelpers.waitForJobsToComplete();
                    List<Object> projects = new ArrayList<>();
                    for (IJavaProject project : List.of(defaultProject, invisibleProject)) {
                        projects.add(Map.of("compliance", project.getOption(JavaCore.COMPILER_COMPLIANCE, true),
                            "preview", project.getOption(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, true)));
                    }
                    results.add(projects);
                }
                return results;
            } finally { JavaRuntime.removeVMInstallChangedListener(listener); }
        }
        RuntimeEnvironment runtime = new RuntimeEnvironment();
        runtime.setName((String) input.get("name"));
        runtime.setPath((String) input.get("path"));
        runtime.setJavadoc((String) input.get("javadoc"));
        runtime.setSources((String) input.get("sources"));
        runtime.setDefault(Boolean.TRUE.equals(input.get("default")));
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("valid", runtime.isValid());
        result.put("javadoc", runtime.getJavadocURL());
        if ("javadoc".equals(api)) { return result; }
        File file = runtime.getInstallationFile();
        result.put("directory", file != null && file.isDirectory());
        result.put("validated", file != null && JavaRuntime.getVMInstallType(StandardVMType.ID_STANDARD_VM_TYPE).validateInstallLocation(file).isOK());
        Preferences preferences = new Preferences();
        preferences.setRuntimes(new HashSet<>(Set.of(runtime)));
        result.put("changed", JVMConfigurator.configureJVMs(preferences, JavaLanguageServerPlugin.getInstance().getClientConnection()));
        JobHelpers.waitForJobsToComplete();
        IVMInstall vm = JVMConfigurator.findVM(file, runtime.getName());
        result.put("present", vm != null);
        if (vm != null) {
            result.put("vm2", vm instanceof IVMInstall2);
            result.put("version", ((IVMInstall2) vm).getJavaVersion());
            LibraryLocation[] libraries = vm.getLibraryLocations();
            result.put("librariesNotNull", libraries != null);
            List<Object> libs = new ArrayList<>();
            if (libraries != null) for (LibraryLocation lib : libraries) {
                Map<String, Object> item = new LinkedHashMap<>();
                item.put("path", lib.getSystemLibraryPath().toOSString());
                item.put("javadoc", lib.getJavadocLocation());
                libs.add(item);
            }
            result.put("libraries", libs);
            result.put("different", !original.equals(JavaRuntime.getDefaultVMInstall()));
            result.put("defaultSame", vm.equals(JavaRuntime.getDefaultVMInstall()));
            IExecutionEnvironment environment = JVMConfigurator.getExecutionEnvironment(runtime.getName());
            result.put("environmentPresent", environment != null);
            result.put("environmentSame", environment != null && vm.equals(environment.getDefaultVM()));
            if (Boolean.TRUE.equals(input.get("dispose"))) {
                vm.getVMInstallType().disposeVMInstall(vm.getId());
                result.put("disposedAbsent", JVMConfigurator.findVM(null, runtime.getName()) == null);
            }
        }
        return result;
    }
}
