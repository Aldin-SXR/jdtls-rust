//! Project-manager test support: the parts of
//! `AbstractProjectsManagerBasedTest` (and its Maven/Gradle/invisible
//! subclasses) that inspect the workspace model.  Everything goes through
//! commands the real jdt.ls exposes over LSP (`java.project.getAll`,
//! `java.project.getSettings`, `java.project.listSourcePaths`,
//! `java/buildWorkspace` and the published diagnostics), so the same tests
//! run against the oracle.

use super::jdtls::Workspace;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tower_lsp::lsp_types::Url;

pub const NATURE_IDS: &str = "org.eclipse.jdt.ls.core.natureIds";
pub const VM_LOCATION: &str = "org.eclipse.jdt.ls.core.vm.location";
pub const SOURCE_PATHS: &str = "org.eclipse.jdt.ls.core.sourcePaths";
pub const OUTPUT_PATH: &str = "org.eclipse.jdt.ls.core.outputPath";
pub const CLASSPATH_ENTRIES: &str = "org.eclipse.jdt.ls.core.classpathEntries";
pub const REFERENCED_LIBRARIES: &str = "org.eclipse.jdt.ls.core.referencedLibraries";
pub const M2E_SELECTED_PROFILES: &str = "org.eclipse.m2e.core.selectedProfiles";

pub const JAVA_NATURE: &str = "org.eclipse.jdt.core.javanature";
pub const MAVEN_NATURE: &str = "org.eclipse.m2e.core.maven2Nature";
pub const GRADLE_NATURE: &str = "org.eclipse.buildship.core.gradleprojectnature";
pub const UNMANAGED_FOLDER_NATURE: &str = "org.eclipse.jdt.ls.core.unmanagedFolder";

/// `IClasspathEntry.CPE_*`.
pub const CPE_LIBRARY: u64 = 1;
pub const CPE_PROJECT: u64 = 2;
pub const CPE_SOURCE: u64 = 3;
pub const CPE_VARIABLE: u64 = 4;
pub const CPE_CONTAINER: u64 = 5;

/// `ProjectUtils.WORKSPACE_LINK`.
pub const WORKSPACE_LINK: &str = "_";

impl Workspace {
    /// `workspace/executeCommand` (arguments are passed as given).
    pub fn execute(&mut self, command: &str, arguments: Vec<Value>) -> Value {
        self.request("workspace/executeCommand", json!({ "command": command, "arguments": arguments }))
    }

    /// Like [`Workspace::execute`] but returns the JSON-RPC error message
    /// instead of panicking.
    pub fn try_execute(&mut self, command: &str, arguments: Vec<Value>) -> Result<Value, String> {
        let c = self.client();
        let id = c.next_id;
        c.next_id += 1;
        c.send(&json!({ "jsonrpc": "2.0", "id": id, "method": "workspace/executeCommand",
            "params": { "command": command, "arguments": arguments } }));
        let resp = c
            .recv_until(Duration::from_secs(180), |m| m["id"] == json!(id) && m.get("method").is_none())
            .unwrap_or_else(|| panic!("timed out waiting for {command}"));
        match resp.get("error") {
            Some(err) => Err(err["message"].as_str().unwrap_or("").to_owned()),
            None => Ok(resp["result"].clone()),
        }
    }

    /// `AbstractProjectsManagerBasedTest.copyFiles(path, true)`: copy
    /// `projects/<path>` into the working directory without importing it.
    pub fn copy_files(&mut self, path: &str) -> PathBuf {
        let from = super::jdtls::fixtures_dir().join("projects").join(path);
        let to = self.dir.join(path);
        if to.is_dir() {
            std::fs::remove_dir_all(&to).ok();
        } else if to.exists() {
            std::fs::remove_file(&to).ok();
        }
        super::jdtls::copy_dir(&from, &to);
        to
    }

    /// Import `root` as a workspace folder (`initializeProjects(roots)` before
    /// the server starts, `updateWorkspaceFolders(added, ∅)` after).
    pub fn import_root(&mut self, root: &Path) {
        self.add_root(root.to_path_buf());
    }

    /// `updateWorkspaceFolders(∅, removed)`.
    pub fn remove_root(&mut self, root: &Path) {
        let root = root.to_path_buf();
        self.roots.retain(|r| r != &root);
        if self.client.is_some() {
            let uri = Url::from_file_path(&root).unwrap().to_string();
            let name = root.file_name().unwrap().to_string_lossy().into_owned();
            self.client().notify(
                "workspace/didChangeWorkspaceFolders",
                json!({ "event": { "added": [], "removed": [{ "uri": uri, "name": name }] } }),
            );
            self.wait_idle();
        }
    }

    /// `importRootFolder(rootPath, triggerFile)`: `triggerFile` (relative to
    /// `root`) becomes the `triggerFiles` initialization option, which is how
    /// vscode-java passes `Preferences.setTriggerFiles`.
    pub fn import_root_folder(&mut self, root: &Path, trigger_file: Option<&str>) {
        if let Some(t) = trigger_file.filter(|t| !t.trim().is_empty()) {
            let uri = Url::from_file_path(root.join(t)).unwrap().to_string();
            let mut files = self.init_options.get("triggerFiles").and_then(Value::as_array).cloned().unwrap_or_default();
            files.push(json!(uri));
            self.init_options["triggerFiles"] = Value::Array(files);
        }
        self.add_root(root.to_path_buf());
    }

    /// `copyAndImportFolder(folder, triggerFile)`.
    pub fn copy_and_import_folder(&mut self, folder: &str, trigger_file: Option<&str>) -> PathBuf {
        let root = self.copy_files(folder);
        self.import_root_folder(&root, trigger_file);
        root
    }

    /// The jdt.ls workspace (`-data`) directory of the server under test.
    pub fn server_workspace_dir(&self) -> PathBuf {
        self.dir.parent().unwrap().join("oracle-data").join("workspace")
    }

    /// `ResourcesPlugin.getWorkspace().getRoot().getProject(name).getLocation()`
    /// for projects jdt.ls creates in its own workspace (invisible projects).
    pub fn workspace_project_location(&self, name: &str) -> PathBuf {
        self.server_workspace_dir().join(name)
    }

    /// `java.project.getAll` (`includeNonJava` → `ProjectCommand.getAllProjects`).
    pub fn all_projects(&mut self, include_non_java: bool) -> Vec<String> {
        let args = if include_non_java { vec![json!(json!({ "includeNonJava": true }).to_string())] } else { vec![] };
        let v = self.execute("java.project.getAll", args);
        v.as_array().cloned().unwrap_or_default().iter().filter_map(|u| u.as_str().map(str::to_owned)).collect()
    }

    /// `WorkspaceHelper.getAllProjects()` minus the default project: the
    /// locations of every workspace project.
    pub fn project_locations(&mut self, include_non_java: bool) -> Vec<PathBuf> {
        let default = self.workspace_project_location(DEFAULT_PROJECT_NAME);
        self.all_projects(include_non_java)
            .iter()
            .filter_map(|u| Url::parse(u).ok().and_then(|u| u.to_file_path().ok()))
            .map(|p| canonical(&p))
            .filter(|p| *p != canonical(&default))
            .collect()
    }

    /// `getProject(name) != null` for a project located at `root`.
    pub fn has_project_at(&mut self, root: &Path, include_non_java: bool) -> bool {
        let root = canonical(root);
        self.project_locations(include_non_java).contains(&root)
    }

    /// `ProjectCommand.getProjectSettings(uri, keys)`.
    pub fn project_settings(&mut self, uri: &str, keys: &[&str]) -> Value {
        self.execute("java.project.getSettings", vec![json!(uri), json!(keys)])
    }

    /// `ProjectCommand.getProjectSettings(uri, [key]).get(key)`.
    pub fn project_setting(&mut self, uri: &str, key: &str) -> Value {
        self.project_settings(uri, &[key])[key].clone()
    }

    /// Nature ids of the project at `root`.
    pub fn natures(&mut self, root: &Path) -> Vec<String> {
        let uri = dir_uri(root);
        self.project_setting(&uri, NATURE_IDS)
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }

    /// `assertIsJavaProject(project)`.
    pub fn assert_is_java_project(&mut self, root: &Path) {
        assert!(self.has_project_at(root, true), "{} is not a project", root.display());
        let natures = self.natures(root);
        assert!(natures.iter().any(|n| n == JAVA_NATURE), "{} is missing the Java nature", root.display());
    }

    /// `assertIsMavenProject(project)`.
    pub fn assert_is_maven_project(&mut self, root: &Path) {
        assert!(self.has_project_at(root, true), "{} is not a project", root.display());
        let natures = self.natures(root);
        assert!(natures.iter().any(|n| n == MAVEN_NATURE), "{} is missing the Maven nature", root.display());
    }

    /// `ProjectUtils.getJavaSourceLevel(project)`.
    pub fn java_source_level(&mut self, root: &Path) -> String {
        let uri = dir_uri(root);
        self.project_setting(&uri, "org.eclipse.jdt.core.compiler.source").as_str().unwrap_or("").to_owned()
    }

    /// `IJavaProject.getOption(key, true)` for the project at `root`.
    pub fn java_option(&mut self, root: &Path, key: &str) -> Value {
        let uri = dir_uri(root);
        self.project_setting(&uri, key)
    }

    /// Classpath entries of the project at `root`
    /// (`org.eclipse.jdt.ls.core.classpathEntries`: the raw classpath with
    /// non-JRE containers expanded and the JRE container left out).
    pub fn classpath_entries(&mut self, root: &Path) -> Vec<Value> {
        let uri = dir_uri(root);
        self.project_setting(&uri, CLASSPATH_ENTRIES).as_array().cloned().unwrap_or_default()
    }

    /// Source paths of the project at `root` (`org.eclipse.jdt.ls.core.sourcePaths`).
    pub fn source_paths(&mut self, root: &Path) -> Vec<String> {
        let uri = dir_uri(root);
        self.project_setting(&uri, SOURCE_PATHS)
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }

    /// `java.project.listSourcePaths` (`BuildPathCommand.listSourcePaths`).
    pub fn list_source_paths(&mut self) -> Value {
        self.execute("java.project.listSourcePaths", vec![])
    }

    /// Names of the projects that own a source path
    /// (`listSourcePaths().data[*].projectName`).
    pub fn source_path_project_names(&mut self) -> Vec<String> {
        let v = self.list_source_paths();
        let mut names: Vec<String> = v["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|e| e["projectName"].as_str().map(str::to_owned))
            .collect();
        names.dedup();
        names
    }

    /// `java/buildWorkspace` (`BuildWorkspaceHandler.buildWorkspace`):
    /// 0 FAILED, 1 SUCCEED, 2 WITH_ERROR, 3 CANCELLED.
    pub fn build_workspace(&mut self, force_rebuild: bool) -> Value {
        self.request("java/buildWorkspace", json!(force_rebuild))
    }

    /// Latest published diagnostics per URI after a workspace build: the
    /// markers `WorkspaceDiagnosticsHandler` reports to the client.
    pub fn published_diagnostics(&mut self) -> BTreeMap<String, Vec<Value>> {
        self.build_workspace(false);
        self.wait_idle();
        let c = self.client();
        let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for m in &c.notifications {
            if m["method"] == "textDocument/publishDiagnostics" {
                let uri = m["params"]["uri"].as_str().unwrap_or("").to_owned();
                out.insert(uri, m["params"]["diagnostics"].as_array().cloned().unwrap_or_default());
            }
        }
        out
    }

    /// `ResourceUtils.getErrorMarkers(project)` over LSP: error diagnostics
    /// of every resource under `root`, as `(uri, diagnostic)`.
    pub fn error_markers(&mut self, root: &Path) -> Vec<(String, Value)> {
        self.markers(root, Some(1))
    }

    /// `ResourceUtils.getWarningMarkers(project)` over LSP.
    pub fn warning_markers(&mut self, root: &Path) -> Vec<(String, Value)> {
        self.markers(root, Some(2))
    }

    /// `project.findMarkers(null, true, DEPTH_INFINITE)` over LSP.
    pub fn all_markers(&mut self, root: &Path) -> Vec<(String, Value)> {
        self.markers(root, None)
    }

    fn markers(&mut self, root: &Path, severity: Option<u64>) -> Vec<(String, Value)> {
        let root = canonical(root);
        let mut out = Vec::new();
        for (uri, diags) in self.published_diagnostics() {
            let Some(path) = Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()) else { continue };
            if !canonical(&path).starts_with(&root) {
                continue;
            }
            for d in diags {
                if severity.is_none() || d["severity"].as_u64() == severity {
                    out.push((uri.clone(), d));
                }
            }
        }
        out
    }

    /// `assertNoErrors(project)`.
    pub fn assert_no_errors(&mut self, root: &Path) {
        let errors = self.error_markers(root);
        assert!(errors.is_empty(), "{} has errors: \n{}", root.display(), markers_to_string(&errors));
    }

    /// `assertHasErrors(project)` / `assertHasErrors(project, expectedErrorsLike...)`.
    pub fn assert_has_errors(&mut self, root: &Path, expected: &[&str]) {
        let errors = self.error_markers(root);
        assert!(!errors.is_empty() || !expected.is_empty(), "{} has no errors", root.display());
        let all = markers_to_string(&errors);
        for e in expected {
            assert!(
                errors.iter().any(|(_, d)| d["message"].as_str().is_some_and(|m| m.contains(e))),
                "{e} was not found in: \n{all}"
            );
        }
    }

    /// `projectsManager.fileChanged(uri, type)` for each change
    /// (1 CREATED, 2 CHANGED, 3 DELETED), sent as `didChangeWatchedFiles`.
    pub fn files_changed(&mut self, changes: &[(&Path, u32)]) {
        let changes: Vec<Value> = changes
            .iter()
            .map(|(p, t)| json!({ "uri": Url::from_file_path(p).unwrap().to_string(), "type": t }))
            .collect();
        self.client().notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }));
        self.wait_idle();
    }

    /// Server→client requests received so far for `method`
    /// (e.g. `client/registerCapability`).
    pub fn server_requests(&mut self, method: &str) -> Vec<Value> {
        self.client().server_requests.iter().filter(|m| m["method"] == method).cloned().collect()
    }

    /// The glob patterns of the latest `workspace/didChangeWatchedFiles`
    /// registration (`ProjectsManager.registerWatchers()`).
    pub fn watcher_glob_patterns(&mut self) -> Vec<String> {
        self.wait_idle();
        let regs = self.server_requests("client/registerCapability");
        let mut latest = Vec::new();
        for r in regs {
            for reg in r["params"]["registrations"].as_array().cloned().unwrap_or_default() {
                if reg["method"] == "workspace/didChangeWatchedFiles" {
                    latest = reg["registerOptions"]["watchers"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|w| match &w["globPattern"] {
                            Value::String(s) => s.clone(),
                            other => other["pattern"].as_str().unwrap_or("").to_owned(),
                        })
                        .collect();
                }
            }
        }
        latest
    }
}

/// `ProjectsManager.DEFAULT_PROJECT_NAME`.
pub const DEFAULT_PROJECT_NAME: &str = "jdt.ls-java-project";

/// `ProjectUtils.getWorkspaceInvisibleProjectName(root)`.
pub fn invisible_project_name(root: &Path) -> String {
    let file_name = root.file_name().unwrap().to_string_lossy();
    let portable = root.to_string_lossy().replace('\\', "/");
    let hash = portable.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    format!("{file_name}_{:x}", hash as u32)
}

/// `file:` URI of a directory (`IResource.getLocationURI()`).
pub fn dir_uri(p: &Path) -> String {
    Url::from_directory_path(p).unwrap().to_string()
}

/// `file:` URI of a file.
pub fn file_uri(p: &Path) -> String {
    Url::from_file_path(p).unwrap().to_string()
}

pub fn canonical(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let trimmed = if s.len() > 1 { s.trim_end_matches('/') } else { &s };
    let p = Path::new(trimmed);
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `ResourceUtils.toString(markers)`.
pub fn markers_to_string(markers: &[(String, Value)]) -> String {
    markers
        .iter()
        .map(|(uri, d)| format!("{uri}:{} {}", d["range"]["start"]["line"], d["message"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
}
