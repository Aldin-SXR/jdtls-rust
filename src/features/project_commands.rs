//! jdt.ls project commands (`ProjectCommand`, `BuildPathCommand`) over the
//! Rust project model: `java.project.getAll`, `java.project.getSettings`,
//! `java.project.getClasspaths`, `java.project.isTestFile`,
//! `java.project.listSourcePaths`, plus the project-level parts of
//! `WorkspaceDiagnosticsHandler` and `ProjectsManager.registerWatchers`.

use crate::project::{
    java_file_uri, ClasspathEntry, EntryKind, Project, ProjectKind, Workspace,
    MAVEN_NATURE, WORKSPACE_LINK,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub const NATURE_IDS: &str = "org.eclipse.jdt.ls.core.natureIds";
pub const VM_LOCATION: &str = "org.eclipse.jdt.ls.core.vm.location";
pub const SOURCE_PATHS: &str = "org.eclipse.jdt.ls.core.sourcePaths";
pub const OUTPUT_PATH: &str = "org.eclipse.jdt.ls.core.outputPath";
pub const CLASSPATH_ENTRIES: &str = "org.eclipse.jdt.ls.core.classpathEntries";
pub const REFERENCED_LIBRARIES: &str = "org.eclipse.jdt.ls.core.referencedLibraries";
pub const M2E_SELECTED_PROFILES: &str = "org.eclipse.m2e.core.selectedProfiles";

/// What the commands need besides the workspace.
pub struct Env<'a> {
    pub ws: &'a Workspace,
    pub vm_home: Option<PathBuf>,
    pub root_paths: &'a [PathBuf],
}

/// The default project (`jdt.ls-java-project`) as a model project.
pub fn default_project(location: &Path) -> Project {
    crate::project::default_java_project(location)
}

/// Every workspace project (`IWorkspaceRoot.getProjects()`, sorted by name),
/// including the default project when it exists.
pub fn all_projects(ws: &Workspace) -> Vec<Project> {
    ws.all_projects()
}

/// `ProjectCommand.getAllJavaProjects` / `getAllProjects`.
pub fn get_all(ws: &Workspace, include_non_java: bool) -> Value {
    let uris: Vec<Value> = all_projects(ws)
        .iter()
        .filter(|p| include_non_java || p.is_java())
        .map(|p| {
            let root = if p.has_nature(crate::project::UNMANAGED_FOLDER_NATURE) {
                &p.root
            } else {
                &p.location
            };
            Value::String(java_file_uri(root, root.is_dir() || !root.exists()))
        })
        .collect();
    Value::Array(uris)
}

/// `ProjectCommand.getJavaProjectFromUri`.
pub fn java_project_from_uri(ws: &Workspace, uri: &str) -> Result<Project, String> {
    let url =
        Url::parse(uri).map_err(|_| "Given URI does not belong to any Java project.".to_owned())?;
    let path = crate::project::uri_to_path(&url)
        .ok_or_else(|| "Given URI does not belong to any Java project.".to_owned())?;
    let projects = all_projects(ws);
    // A type root (source file in a project).
    if path
        .extension()
        .is_some_and(|e| e == "java" || e == "class")
        && path.is_file()
    {
        if let Some(p) = ws.project_for_path(&path) {
            if p.source_folder_for(&path).is_some() {
                return Ok(p.clone());
            }
        }
    }
    // Containers for the location, the shallowest workspace path first.
    let mut containers: Vec<(usize, &Project)> = Vec::new();
    for p in &projects {
        if p.kind == ProjectKind::Invisible {
            if let Ok(rel) = path.strip_prefix(&p.root) {
                containers.push((2 + rel.components().count(), p));
            }
        }
        if let Ok(rel) = path.strip_prefix(&p.location) {
            containers.push((1 + rel.components().count(), p));
        }
    }
    containers.sort_by_key(|(d, _)| *d);
    containers
        .into_iter()
        .map(|(_, p)| p)
        .find(|p| p.is_java())
        .cloned()
        .ok_or_else(|| "Given URI does not belong to any Java project.".to_owned())
}

/// The location of an Eclipse path inside `project` (`IProject.getFolder(..).getLocation()`).
fn location_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `ProjectClasspathEntry` JSON.
fn classpath_entry_json(
    kind: EntryKind,
    path: Option<String>,
    output: Option<String>,
    attributes: &[(String, String)],
) -> Value {
    let mut m = Map::new();
    let attrs: Map<String, Value> = attributes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    m.insert("attributes".into(), Value::Object(attrs));
    m.insert("kind".into(), json!(kind as u8));
    if let Some(o) = output {
        m.insert("output".into(), Value::String(o));
    }
    if let Some(p) = path {
        m.insert("path".into(), Value::String(p));
    }
    Value::Object(m)
}

/// The `org.eclipse.jdt.ls.core.classpathEntries` setting: the raw classpath
/// with non-JRE containers expanded (their entries appended at the end).
pub fn classpath_entries(project: &Project) -> Vec<Value> {
    let mut queue: Vec<ClasspathEntry> = project.classpath.clone();
    let mut out = Vec::new();
    let mut i = 0;
    while i < queue.len() {
        let e = queue[i].clone();
        i += 1;
        match e.kind {
            EntryKind::Source => {
                let rel = e
                    .path
                    .trim_start_matches('/')
                    .split_once('/')
                    .map(|(_, r)| r)
                    .unwrap_or("");
                if rel.is_empty() {
                    continue;
                }
                let loc = e.location.clone().or_else(|| project.location_of(&e.path));
                out.push(classpath_entry_json(
                    e.kind,
                    loc.as_deref().map(location_string),
                    e.output.as_deref().map(location_string),
                    &e.attributes,
                ));
            }
            EntryKind::Container => {
                if !e.is_jre_container() {
                    queue.extend(e.children.iter().cloned());
                }
            }
            EntryKind::Library => {
                let path = match &e.location {
                    Some(l) if l.exists() => location_string(l),
                    _ => e.path.clone(),
                };
                out.push(classpath_entry_json(
                    e.kind,
                    Some(path),
                    e.output.as_deref().map(location_string),
                    &e.attributes,
                ));
            }
            _ => out.push(classpath_entry_json(
                e.kind,
                Some(e.path.clone()),
                e.output.as_deref().map(location_string),
                &e.attributes,
            )),
        }
    }
    out
}

/// `ProjectCommand.getProjectSettings(uri, keys)`; `option` resolves JDT
/// option keys (`IJavaProject.getOption(key, true)`).
pub fn get_settings(
    env: &Env,
    uri: &str,
    keys: &[String],
    option: impl Fn(&Project, &str) -> Option<String>,
) -> Result<Value, String> {
    let project = java_project_from_uri(env.ws, uri)?;
    let mut settings = Map::new();
    for key in keys {
        if settings.contains_key(key) {
            continue;
        }
        let value = match key.as_str() {
            NATURE_IDS => json!(project.natures),
            VM_LOCATION => match project.runtime.as_ref().map(|vm| &vm.home).or(env.vm_home.as_ref()) {
                Some(h) if !project.markers.iter().any(|m| m.code == "963") => {
                    json!(location_string(h))
                }
                _ => continue,
            },
            SOURCE_PATHS => {
                let v: Vec<String> = project
                    .classpath
                    .iter()
                    .filter(|e| e.kind == EntryKind::Source)
                    .filter_map(|e| e.location.clone().or_else(|| project.location_of(&e.path)))
                    .map(|p| location_string(&p))
                    .collect();
                json!(v)
            }
            OUTPUT_PATH => json!(project
                .output
                .as_deref()
                .map(location_string)
                .unwrap_or_default()),
            REFERENCED_LIBRARIES => {
                let v: Vec<String> = project
                    .classpath
                    .iter()
                    .filter(|e| e.kind == EntryKind::Library)
                    .map(|e| e.path.clone())
                    .collect();
                json!(v)
            }
            CLASSPATH_ENTRIES => Value::Array(classpath_entries(&project)),
            M2E_SELECTED_PROFILES => json!(project.selected_profiles),
            // Gson leaves null values out of the map.
            other => match option(&project, other) {
                Some(v) => Value::String(v),
                None => continue,
            },
        };
        settings.insert(key.clone(), value);
    }
    Ok(Value::Object(settings))
}

/// `ProjectCommand.isTestFile(uri)`.
pub fn is_test_file(ws: &Workspace, uri: &str) -> Result<bool, String> {
    const NOT_SOURCE: &str = "Given URI does not belong to an existing Java source file.";
    let url = Url::parse(uri).map_err(|_| NOT_SOURCE.to_owned())?;
    let path = crate::project::uri_to_path(&url).ok_or_else(|| NOT_SOURCE.to_owned())?;
    if !path.extension().is_some_and(|e| e == "java") {
        return Err(NOT_SOURCE.to_owned());
    }
    let Some(project) = ws.project_for_path(&path) else {
        // Files outside every project belong to the default project.
        return Ok(false);
    };
    for e in project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Source)
    {
        let Some(loc) = &e.location else { continue };
        if path.starts_with(loc) && is_test_classpath_entry(e) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `ProjectCommand.isTestClasspathEntry`.
fn is_test_classpath_entry(e: &ClasspathEntry) -> bool {
    if e.kind != EntryKind::Source {
        return false;
    }
    if e.is_test() {
        return true;
    }
    for (name, value) in &e.attributes {
        if name.contains("scope") {
            return value.to_lowercase().contains("test");
        }
    }
    false
}

/// `BuildSupportManager.find(project).buildToolName()`.
fn build_tool_name(p: &Project) -> &'static str {
    crate::project::BuildSupport::of(p).build_tool_name()
}

/// `BuildPathCommand.listSourcePaths()`.
pub fn list_source_paths(env: &Env) -> Value {
    let mut data = Vec::new();
    for p in all_projects(env.ws)
        .iter()
        .filter(|p| p.kind != ProjectKind::Default && p.is_java())
    {
        for e in p.classpath.iter().filter(|e| e.kind == EntryKind::Source) {
            let Some(loc) = e.location.clone().or_else(|| p.location_of(&e.path)) else {
                continue;
            };
            let project_type = if p.kind == ProjectKind::Invisible {
                "Workspace"
            } else {
                build_tool_name(p)
            };
            // `rawLocation.append(<empty relative path>)` keeps a trailing separator.
            let is_root = e.path == format!("/{}", p.name)
                || e.path == format!("/{}/{WORKSPACE_LINK}", p.name);
            let slash = |s: String| {
                if is_root && !s.ends_with('/') {
                    format!("{s}/")
                } else {
                    s
                }
            };
            let display = env
                .root_paths
                .iter()
                .find(|r| loc.starts_with(r))
                .and_then(|r| r.parent().and_then(|parent| loc.strip_prefix(parent).ok()))
                .map(|rel| rel.to_string_lossy().into_owned())
                .unwrap_or_else(|| location_string(&loc));
            data.push(json!({
                "path": slash(location_string(&loc)),
                "displayPath": slash(display),
                "classpathEntry": e.path,
                "projectName": p.name,
                "projectType": project_type,
            }));
        }
    }
    json!({ "status": true, "data": data })
}

/// `ProjectCommand.getClasspaths(uri, {scope})`: the runtime classpath and
/// module path of the project's launch configuration.
pub fn get_classpaths(ws: &Workspace, uri: &str, scope: &str) -> Result<Value, String> {
    let project = java_project_from_uri(ws, uri)?;
    let test = scope == "test";
    let mut entries: Vec<PathBuf> = Vec::new();
    let mut seen_projects = Vec::new();
    collect_runtime(ws, &project, test, true, &mut entries, &mut seen_projects);
    let modular = project
        .source_folders
        .iter()
        .any(|sf| sf.path.join("module-info.java").is_file());
    let paths: Vec<String> = entries.iter().map(|p| location_string(p)).collect();
    let (classpaths, modulepaths) = if modular {
        (Vec::new(), paths)
    } else {
        (paths, Vec::new())
    };
    Ok(json!({
        "projectRoot": format!("file:{}", project.location.to_string_lossy()),
        "classpaths": classpaths,
        "modulepaths": modulepaths,
    }))
}

fn collect_runtime(
    ws: &Workspace,
    project: &Project,
    test: bool,
    root: bool,
    out: &mut Vec<PathBuf>,
    seen: &mut Vec<String>,
) {
    if seen.contains(&project.name) {
        return;
    }
    seen.push(project.name.clone());
    let push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    // Output folders: Maven puts the test output first.
    let mut outputs: Vec<(PathBuf, bool)> = Vec::new();
    for e in project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Source)
    {
        let o = e.output.clone().or_else(|| project.output.clone());
        if let Some(o) = o {
            if !outputs.iter().any(|(p, _)| *p == o) {
                outputs.push((o, e.is_test()));
            }
        }
    }
    if outputs.is_empty() {
        if let Some(o) = &project.output {
            outputs.push((o.clone(), false));
        }
    }
    let maven = project.has_nature(MAVEN_NATURE);
    // The test output folders come first.
    outputs.sort_by_key(|(_, t)| !*t);
    for (o, t) in outputs {
        if t && !test {
            continue;
        }
        push(o, out);
    }
    let mut stack: Vec<&ClasspathEntry> = project.classpath.iter().collect();
    stack.reverse();
    while let Some(e) = stack.pop() {
        if e.is_test() && !test {
            continue;
        }
        match e.kind {
            EntryKind::Library | EntryKind::Variable => {
                if !root && !e.exported && !maven && project.kind == ProjectKind::Eclipse {
                    continue;
                }
                if maven && e.attribute("maven.scope") == Some("provided") && !test {
                    // m2e's runtime classpath leaves provided dependencies out.
                    continue;
                }
                if let Some(l) = e.location.as_ref().filter(|l| l.exists()) {
                    push(l.clone(), out);
                }
            }
            EntryKind::Project => {
                if let Some(dep) = ws.project(e.path.trim_start_matches('/')) {
                    collect_runtime(ws, dep, test, false, out, seen);
                }
            }
            EntryKind::Container if !e.is_jre_container() => {
                for c in e.children.iter().rev() {
                    stack.push(c);
                }
            }
            _ => {}
        }
    }
}

/// A `FileSystemWatcher` glob pattern: a plain glob, or a relative pattern
/// (`{baseUri, pattern}`) with an optional watch kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Watcher {
    Glob(String),
    Relative {
        base_uri: String,
        pattern: String,
        kind: Option<u8>,
    },
}

/// `ResourceUtils.toGlobPattern(path)`: `.jar`/`.zip` files become relative
/// patterns, folders `path/**`.
fn to_glob_pattern(path: &Path) -> Watcher {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if name.ends_with(".jar") || name.ends_with(".zip") {
        let base = path
            .parent()
            .map(|p| java_file_uri(p, true))
            .unwrap_or_default();
        return Watcher::Relative {
            base_uri: base,
            pattern: name,
            kind: None,
        };
    }
    let mut g = path.to_string_lossy().replace('\\', "/");
    if !path.is_file() {
        if !g.ends_with('/') {
            g.push('/');
        }
        g.push_str("**");
    }
    Watcher::Glob(g)
}

/// `ProjectsManager.registerWatchers()`.
pub fn watchers(ws: &Workspace, referenced_libraries: &[String]) -> Vec<Watcher> {
    let mut patterns: Vec<Watcher> = [
        "**/*.java",
        "**/.project",
        "**/.classpath",
        "**/.settings/*.prefs",
        "**/src/**",
        // GradleBuildSupport, then MavenBuildSupport watch patterns.
        "**/*.gradle",
        "**/*.gradle.kts",
        "**/gradle.properties",
        "**/pom.xml",
    ]
    .iter()
    .map(|s| Watcher::Glob(s.to_string()))
    .collect();
    let add = |patterns: &mut Vec<Watcher>, w: Watcher| {
        if !patterns.contains(&w) {
            patterns.push(w);
        }
    };
    let mut sources: Vec<PathBuf> = Vec::new();
    let contained = |loc: &Path, sources: &[PathBuf]| sources.iter().any(|s| loc.starts_with(s));
    for p in ws.sorted_projects() {
        if !p.is_java() {
            continue;
        }
        for e in &p.classpath {
            match e.kind {
                EntryKind::Source => {
                    if e.path.contains("/src/") || e.path.ends_with("/src") {
                        continue;
                    }
                    let loc = if e.path == format!("/{}", p.name) {
                        Some(p.location.clone())
                    } else {
                        e.location.clone().filter(|l| l.is_dir())
                    };
                    if let Some(loc) = loc {
                        if !contained(&loc, &sources) {
                            sources.push(loc);
                        }
                    }
                }
                EntryKind::Library => {
                    // A library inside the workspace (`/project/lib/a.jar`).
                    if let (Some(rest), Some(loc)) = (e.path.strip_prefix('/'), &e.location) {
                        if rest
                            .split('/')
                            .next()
                            .is_some_and(|n| ws.project(n).is_some())
                            && !Path::new(&e.path).exists()
                            && !contained(loc, &sources)
                        {
                            sources.push(loc.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        if p.kind == ProjectKind::Invisible {
            for pattern in referenced_libraries {
                let glob = crate::project::invisible::resolve_glob_path(&p.root, pattern);
                add(
                    &mut patterns,
                    Watcher::Glob(glob.to_string_lossy().replace('\\', "/")),
                );
            }
            add(&mut patterns, Watcher::Glob("**/.settings".to_owned()));
        }
    }
    for s in &sources {
        add(&mut patterns, to_glob_pattern(s));
    }
    // Watch on project root folders.
    for p in ws
        .sorted_projects()
        .into_iter()
        .filter(|p| p.kind != ProjectKind::Invisible)
    {
        let base = p
            .location
            .parent()
            .map(|d| java_file_uri(d, true))
            .unwrap_or_default();
        let name = p
            .location
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        patterns.push(Watcher::Relative {
            base_uri: base,
            pattern: name,
            kind: Some(4),
        });
    }
    patterns
}

/// The registration options of `workspace/didChangeWatchedFiles`.
pub fn watcher_registration(ws: &Workspace, referenced_libraries: &[String]) -> Value {
    let watchers: Vec<Value> = watchers(ws, referenced_libraries)
        .into_iter()
        .map(|w| match w {
            Watcher::Glob(g) => json!({ "globPattern": g }),
            Watcher::Relative {
                base_uri,
                pattern,
                kind,
            } => {
                let mut v = json!({ "globPattern": { "baseUri": base_uri, "pattern": pattern } });
                if let Some(k) = kind {
                    v["kind"] = json!(k);
                }
                v
            }
        })
        .collect();
    json!({ "watchers": watchers })
}

/// Project-level diagnostics (`WorkspaceDiagnosticsHandler.publishMarkers`):
/// `(uri, diagnostics)` for each project with markers, and its build files.
pub fn project_marker_diagnostics(ws: &Workspace) -> Vec<(String, Vec<Value>)> {
    let mut out = Vec::new();
    for p in ws.sorted_projects() {
        let to_diag = |m: &crate::project::Marker| {
            let (line, sc, ec) = m.range.unwrap_or((0, 0, 0));
            json!({
                "range": { "start": { "line": line, "character": sc }, "end": { "line": line, "character": ec } },
                "severity": m.severity,
                "code": m.code,
                "source": "Java",
                "message": m.message,
            })
        };
        // jdt.ls reports the builder's "cannot be built" marker first.
        let mut project_markers: Vec<&crate::project::Marker> =
            p.markers.iter().filter(|m| m.resource.is_none()).collect();
        project_markers.sort_by_key(|m| !m.message.starts_with("The project cannot be built"));
        let project_markers: Vec<Value> = project_markers.into_iter().map(to_diag).collect();
        let mut files: BTreeMap<PathBuf, Vec<Value>> = BTreeMap::new();
        for m in p.markers.iter().filter(|m| m.resource.is_some()) {
            files
                .entry(m.resource.clone().unwrap())
                .or_default()
                .push(to_diag(m));
        }
        if !project_markers.is_empty() {
            out.push((crate::project::resource_uri(&p.location), project_markers));
        }
        let pom = p.location.join("pom.xml");
        if pom.is_file() {
            out.push((
                crate::project::resource_uri(&pom),
                files.remove(&pom).unwrap_or_default(),
            ));
        }
        for (f, d) in files {
            out.push((crate::project::resource_uri(&f), d));
        }
    }
    out
}

/// `ProjectUtils.getProjectRealFolder` of an invisible project's link.
pub fn workspace_link(p: &Project) -> PathBuf {
    p.location.join(WORKSPACE_LINK)
}

/// `SourceAttachmentCommand.resolveSourceAttachment([{classFileUri}])`.
pub fn resolve_source_attachment(ws: &Workspace, class_file_uri: Option<&str>) -> Value {
    let error = |m: String| json!({ "errorMessage": m });
    let Some(uri) = class_file_uri else {
        return error("The parameter is missing.".to_owned());
    };
    let Some(r) = crate::classfile::ClassFileRef::parse(uri) else {
        return error(format!("Cannot find the class file {uri}"));
    };
    let project = ws.project(&r.project);
    let roots: Vec<(String, PathBuf)> = ws
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.root.clone()))
        .collect();
    let jar = crate::classfile::resolve_root_path(
        &r.root_path,
        project.map(|p| p.root.as_path()),
        &roots,
    );
    let Some(project) = project else {
        return error(format!("Cannot find the class file {uri}"));
    };
    // A raw library entry, or an entry of a container.
    for e in &project.classpath {
        match e.kind {
            EntryKind::Library | EntryKind::Variable
                if e.location.as_deref() == Some(jar.as_path()) =>
            {
                return json!({ "attributes": {
                    "jarPath": location_string(&jar),
                    "sourceAttachmentPath": e.source_attachment.as_deref().map(location_string),
                    "canEditEncoding": true,
                } });
            }
            EntryKind::Container
                if e.children
                    .iter()
                    .any(|c| c.location.as_deref() == Some(jar.as_path())) =>
            {
                let name = if e.path.starts_with(crate::project::MAVEN_CONTAINER) {
                    "Maven Dependencies"
                } else if e.path.starts_with(crate::project::GRADLE_CONTAINER) {
                    "Project and External Dependencies"
                } else {
                    "JRE System Library"
                };
                return error(format!(
                    "The JAR of this class file belongs to container '{name}' which does not allow modifications to source attachments on its entries."
                ));
            }
            _ => {}
        }
    }
    error(format!(
        "Cannot find the ClasspathEntry for the JAR '{}' of this class file",
        location_string(&jar)
    ))
}
