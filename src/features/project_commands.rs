//! jdt.ls project commands (`ProjectCommand`, `BuildPathCommand`) over the
//! Rust project model: `java.project.getAll`, `java.project.getSettings`,
//! `java.project.getClasspaths`, `java.project.isTestFile`,
//! `java.project.listSourcePaths`, plus the project-level parts of
//! `WorkspaceDiagnosticsHandler` and `ProjectsManager.registerWatchers`.

use crate::project::runtime::RuntimeRegistry;
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
    // JavaCore.create(project) of the last container is returned even when it
    // doesn't exist.
    let fallback = containers.last().map(|(_, p)| *p);
    containers
        .iter()
        .map(|(_, p)| *p)
        .find(|p| p.is_java())
        .or(fallback)
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
    let modular = has_module_description(&project);
    if modular && test {
        // JDT launching patches the test output folders into the module
        // (`--patch-module`) instead of listing them.
        let test_outputs: Vec<PathBuf> = project
            .classpath
            .iter()
            .filter(|e| e.kind == EntryKind::Source && e.is_test())
            .filter_map(|e| e.output.clone())
            .collect();
        entries.retain(|p| !test_outputs.contains(p));
    }
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
    if project.has_nature(crate::project::GRADLE_NATURE) {
        // Buildship's `GradleClasspathProvider`: a launch configuration
        // without mapped resources includes every source set's output, in
        // classpath order; only the libraries are filtered by test scope.
        for (o, _) in outputs {
            push(o, out);
        }
    } else {
        // The test output folders come first.
        outputs.sort_by_key(|(_, t)| !*t);
        for (o, t) in outputs {
            if t && !test {
                continue;
            }
            push(o, out);
        }
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
    let encoding_warnings = crate::features::preferences::current().get_project_encoding()
        == crate::features::preferences::model::ProjectEncodingMode::Warning;
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
        let mut project_markers: Vec<Value> = project_markers.into_iter().map(to_diag).collect();
        // Core resources' ValidateProjectEncoding marker, which
        // WorkspaceDiagnosticsHandler ignores unless `java.project.encoding`
        // is `warning` (with `setDefault` every project gets the default).
        if encoding_warnings && p.kind != crate::project::ProjectKind::Default && p.explicit_encoding().is_none()
        {
            project_markers.push(to_diag(&crate::project::Marker::project(
                format!("Project '{}' has no explicit encoding set", p.name),
                2,
                "0",
            )));
        }
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

/// `IClasspathAttribute.SOURCE_ATTACHMENT_ENCODING`.
const SOURCE_ATTACHMENT_ENCODING: &str = "source_encoding";

/// `SourceAttachmentCommand.updateSourceAttachment([{classFileUri, attributes}])`
/// for raw library and variable entries: the new attachment path (blank
/// removes it) and encoding replace the entry's, and the `.classpath` is
/// rewritten. Entries of containers are not editable here.
pub fn update_source_attachment(ws: &mut Workspace, class_file_uri: &str, attributes: &Value) -> Value {
    let error = |m: String| json!({ "errorMessage": m });
    let Some(r) = crate::classfile::ClassFileRef::parse(class_file_uri) else {
        return error(format!("Cannot find the class file {class_file_uri}"));
    };
    let roots: Vec<(String, PathBuf)> = ws.projects.iter().map(|p| (p.name.clone(), p.root.clone())).collect();
    let Some(index) = ws.projects.iter().position(|p| p.name == r.project) else {
        return error(format!("Cannot find the class file {class_file_uri}"));
    };
    let jar = crate::classfile::resolve_root_path(&r.root_path, Some(ws.projects[index].root.as_path()), &roots);
    let project = &mut ws.projects[index];
    let blank = |v: Option<&str>| v.is_none_or(|s| s.trim().is_empty());
    let source_path = attributes.get("sourceAttachmentPath").and_then(Value::as_str);
    let encoding = attributes.get("sourceAttachmentEncoding").and_then(Value::as_str);
    let entry = project.classpath.iter_mut().find(|e| {
        matches!(e.kind, EntryKind::Library | EntryKind::Variable) && e.location.as_deref() == Some(jar.as_path())
    });
    let Some(entry) = entry else {
        let in_container = project.classpath.iter().any(|e| {
            e.kind == EntryKind::Container && e.children.iter().any(|c| c.location.as_deref() == Some(jar.as_path()))
        });
        return error(if in_container {
            "The JAR of this class file belongs to a container which does not allow modifications to source attachments on its entries.".to_owned()
        } else {
            format!("Cannot find the ClasspathEntry for the JAR '{}' of this class file", location_string(&jar))
        });
    };
    entry.source_attachment = (!blank(source_path)).then(|| PathBuf::from(source_path.unwrap()));
    // `updateElements`: replace the encoding attribute in place, append a
    // new one, or drop it when blank.
    let new_encoding = (!blank(encoding)).then(|| (SOURCE_ATTACHMENT_ENCODING.to_owned(), encoding.unwrap().to_owned()));
    match (entry.attributes.iter().position(|(n, _)| n == SOURCE_ATTACHMENT_ENCODING), new_encoding) {
        (Some(i), Some(a)) => entry.attributes[i] = a,
        (Some(i), None) => {
            entry.attributes.remove(i);
        }
        (None, Some(a)) => entry.attributes.push(a),
        (None, None) => {}
    }
    project.derive_views();
    if let Err(e) = crate::project::classpath::persist_raw_classpath(project) {
        return error(format!("Update the ClasspathEntry to the project failure. Reason: \"{e}\""));
    }
    json!({})
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

/// `IJavaProject.getOwnModuleDescription() != null`.
pub fn has_module_description(project: &Project) -> bool {
    project
        .source_folders
        .iter()
        .any(|sf| sf.path.join("module-info.java").is_file())
}

// ─── Updating the project (`ProjectCommand.update*`) ─────────────────────────

/// A `ProjectClasspathEntry` sent by the client.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectClasspathEntry {
    pub kind: i32,
    pub path: Option<String>,
    pub output: Option<String>,
    pub attributes: Option<BTreeMap<String, String>>,
}

/// `ProjectClasspathEntries`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectClasspathEntries {
    pub classpath_entries: Vec<ProjectClasspathEntry>,
}

/// The segments of `IPath.fromOSString(path)` (canonicalized: `.` dropped,
/// `..` resolved).
fn segments(path: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in path.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if s == ".." && out.last().is_some_and(|l| l != "..") {
            out.pop();
        } else {
            out.push(s.to_owned());
        }
    }
    out
}

/// `project.getFolder(path).getFullPath()`: the segments of `path` appended
/// to the project's full path, whether `path` is relative or not.
fn folder_full_path(project: &Project, path: &str) -> String {
    let mut full = format!("/{}", project.name);
    for s in segments(path) {
        full.push('/');
        full.push_str(&s);
    }
    full
}

/// The output full path of a client source entry (`"."` is the project).
fn output_full_path(project: &Project, output: Option<&str>) -> Option<String> {
    output.map(|o| {
        if o == "." {
            format!("/{}", project.name)
        } else {
            folder_full_path(project, o)
        }
    })
}

/// `IPath.isPrefixOf`.
fn is_prefix_of(prefix: &str, path: &str) -> bool {
    path == prefix || path.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
}

/// `path.makeRelativeTo(base).addTrailingSeparator()`.
fn relative_with_separator(path: &str, base: &str) -> String {
    let rel = path[base.trim_end_matches('/').len()..].trim_start_matches('/');
    format!("{rel}/")
}

/// `ProjectUtils.resolveSourceClasspathEntries`: new source entries for
/// `source_and_output` (full paths), child folders first, nested folders and
/// the output location excluded from their parents.
pub fn resolve_source_classpath_entries(
    project: &Project,
    source_and_output: &[(String, Option<String>)],
    excluding_paths: &[String],
    output_path: Option<&str>,
) -> Vec<ClasspathEntry> {
    let original: Vec<&ClasspathEntry> = project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Source)
        .collect();
    let mut source_paths: Vec<&String> = source_and_output.iter().map(|(s, _)| s).collect();
    // Sort the source paths to make the child folders come first.
    source_paths.sort_by(|a, b| b.cmp(a));
    let default_output = project.output.as_deref().map(|o| project.full_path(o));
    let output_path = output_path.map(str::to_owned).or(default_output);
    let mut entries: Vec<ClasspathEntry> = Vec::new();
    for current in source_paths {
        let mut can_add = true;
        let mut exclusions = Vec::new();
        for e in &entries {
            if e.path == *current {
                tracing::error!("Skip duplicated source path: {current}");
                can_add = false;
                break;
            }
            if is_prefix_of(current, &e.path) {
                exclusions.push(relative_with_separator(&e.path, current));
            }
        }
        if let Some(output) = &output_path {
            if is_prefix_of(current, output) {
                exclusions.push(relative_with_separator(output, current));
            }
        }
        if !can_add {
            continue;
        }
        for exclusion in excluding_paths {
            if is_prefix_of(current, exclusion) && exclusion != current {
                exclusions.push(relative_with_separator(exclusion, current));
            }
        }
        let specific_output = source_and_output
            .iter()
            .find(|(s, _)| s == current)
            .and_then(|(_, o)| o.clone());
        let mut entry = ClasspathEntry::new(EntryKind::Source, current.clone());
        entry.location = project.location_of(current);
        entry.output = specific_output.and_then(|o| project.location_of(&o));
        if let Some(orig) = original.iter().find(|e| e.path == *current) {
            entry.inclusions = orig.inclusions.clone();
            entry.attributes = orig.attributes.clone();
        }
        entry.exclusions = exclusions;
        entries.push(entry);
    }
    entries
}

/// `ClasspathEntry.validateClasspath`, for duplicate entries and nested
/// output folders.
fn validate_classpath(project: &Project, classpath: &[ClasspathEntry]) -> Result<(), String> {
    // Output locations: the project's default output first, then the
    // distinct custom outputs of the source entries.
    let mut outputs: Vec<String> = vec![project
        .output
        .as_deref()
        .map(|o| project.full_path(o))
        .unwrap_or_else(|| format!("/{}/bin", project.name))];
    let mut all_sources_have_custom_output = true;
    for e in classpath.iter().filter(|e| e.kind == EntryKind::Source) {
        match &e.output {
            Some(o) => {
                let full = project.full_path(o);
                if !outputs.contains(&full) {
                    outputs.push(full);
                }
            }
            None => all_sources_have_custom_output = false,
        }
    }
    let mut potential_nested_output = None;
    for (i, custom) in outputs.iter().enumerate().skip(1) {
        if let Some(index) = outputs.iter().position(|o| is_prefix_of(o, custom)) {
            if index == 0 {
                potential_nested_output.get_or_insert(custom.clone());
            } else if index != i {
                return Err(format!(
                    "Cannot nest output folder '{}' inside output folder '{}'",
                    custom.trim_start_matches('/'),
                    outputs[index].trim_start_matches('/')
                ));
            }
        }
    }
    if let Some(nested) = potential_nested_output.filter(|_| !all_sources_have_custom_output) {
        return Err(format!(
            "Cannot nest output folder '{}' inside output folder '{}'",
            nested.trim_start_matches('/'),
            outputs[0].trim_start_matches('/')
        ));
    }
    for (i, e) in classpath.iter().enumerate() {
        if classpath[..i].iter().any(|o| o.path == e.path) {
            let segments: Vec<&str> = e.path.trim_start_matches('/').split('/').collect();
            let message = if segments.first() == Some(&project.name.as_str()) {
                segments[1..].join("/")
            } else {
                e.path.trim_start_matches('/').to_owned()
            };
            return Err(format!(
                "Build path contains duplicate entry: '{message}' for project '{}'",
                project.name
            ));
        }
    }
    Ok(())
}

/// `IJavaProject.setRawClasspath`: the model and `.classpath`.
fn set_raw_classpath(project: &mut Project, classpath: Vec<ClasspathEntry>) -> Result<(), String> {
    project.classpath = classpath;
    project.derive_views();
    crate::project::classpath::persist_raw_classpath(project).map_err(|e| e.to_string())
}

/// `ProjectCommand.updateSourcePaths(uri, sourceAndOutput)` on the project.
pub fn update_source_paths(
    project: &mut Project,
    source_and_output: &[(String, Option<String>)],
) -> Result<(), String> {
    let full: Vec<(String, Option<String>)> = source_and_output
        .iter()
        .map(|(s, o)| (folder_full_path(project, s), output_full_path(project, o.as_deref())))
        .collect();
    // `ProjectUtils.resolveClassPathEntries(javaProject, map, [], null)`.
    let mut entries: Vec<ClasspathEntry> = project
        .classpath
        .iter()
        .filter(|e| e.kind != EntryKind::Source)
        .cloned()
        .collect();
    entries.extend(resolve_source_classpath_entries(project, &full, &[], None));
    validate_classpath(project, &entries)?;
    set_raw_classpath(project, entries)
}

/// `ProjectCommand.convertClasspathEntry`.
fn convert_classpath_entry(
    project: &Project,
    ws: &Workspace,
    entry: &ProjectClasspathEntry,
) -> Option<ClasspathEntry> {
    let kind = match entry.kind {
        5 => EntryKind::Container,
        1 => EntryKind::Library,
        2 => EntryKind::Project,
        _ => return None,
    };
    let os_path = entry.path.clone().unwrap_or_default();
    let mut path = String::new();
    if os_path.starts_with('/') || os_path.starts_with('\\') {
        path.push('/');
    }
    path.push_str(&segments(&os_path).join("/"));
    let mut e = ClasspathEntry::new(kind, path.clone());
    if let Some(attrs) = &entry.attributes {
        e.attributes = attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    if kind == EntryKind::Library {
        let p = Path::new(&path);
        e.location = Some(if p.exists() {
            p.to_path_buf()
        } else {
            // A workspace path: `/project/lib/a.jar`.
            path.trim_start_matches('/')
                .split_once('/')
                .and_then(|(name, rest)| {
                    let owner = if name == project.name {
                        Some(project)
                    } else {
                        ws.project(name)
                    };
                    owner.map(|o| o.location.join(rest))
                })
                .filter(|l| l.exists())
                .unwrap_or_else(|| p.to_path_buf())
        });
        e.source_attachment = e
            .location
            .as_deref()
            .and_then(crate::project::source_attachment);
    }
    Some(e)
}

/// `ProjectCommand.resolveDependencyEntries`: keep the current dependency
/// entries when the new ones resolve to the same paths.
fn resolve_dependency_entries(
    project: &Project,
    new_entries: Vec<ClasspathEntry>,
) -> Vec<ClasspathEntry> {
    let current: Vec<ClasspathEntry> = project
        .classpath
        .iter()
        .filter(|e| {
            e.kind != EntryKind::Source && !e.path.starts_with(crate::project::JRE_CONTAINER)
        })
        .cloned()
        .collect();
    fn expand<'a>(e: &'a ClasspathEntry, out: &mut Vec<&'a str>) {
        if e.kind == EntryKind::Container {
            for c in &e.children {
                expand(c, out);
            }
        } else if !out.contains(&e.path.as_str()) {
            out.push(&e.path);
        }
    }
    let mut mapping: Vec<&str> = Vec::new();
    for e in &current {
        expand(e, &mut mapping);
    }
    if new_entries.len() != mapping.len()
        || new_entries
            .iter()
            .any(|e| !mapping.contains(&e.path.as_str()))
    {
        return new_entries;
    }
    current
}

/// `ProjectCommand.getNewJdkEntry`.
fn new_jdk_entry(
    project: &Project,
    registry: &mut RuntimeRegistry,
    jdk_path: &str,
) -> Result<ClasspathEntry, String> {
    let Some(vm) = registry.vm_install_by_path(jdk_path) else {
        return Err("The select JDK path is not valid.".to_owned());
    };
    let mut e = ClasspathEntry::new(
        EntryKind::Container,
        crate::project::runtime::jre_container_path(&vm),
    );
    if has_module_description(project) {
        e.attributes.push(("module".into(), "true".into()));
    }
    Ok(e)
}

/// The project `uri` belongs to, mutably.
fn project_mut<'a>(ws: &'a mut Workspace, uri: &str) -> Result<&'a mut Project, String> {
    let name = java_project_from_uri(ws, uri)?.name;
    ws.projects
        .iter_mut()
        .find(|p| p.name == name)
        .ok_or_else(|| "Given URI does not belong to any Java project.".to_owned())
}

/// `ProjectCommand.updateClasspaths(uri, entries)`.
pub fn update_classpaths(
    ws: &mut Workspace,
    registry: &mut RuntimeRegistry,
    uri: &str,
    entries: &[ProjectClasspathEntry],
) -> Result<(), String> {
    let snapshot = ws.clone();
    let project = project_mut(ws, uri)?;
    let mut source_and_output: Vec<(String, Option<String>)> = Vec::new();
    let mut new_entries = Vec::new();
    let mut dependencies = Vec::new();
    for entry in entries {
        match entry.kind {
            3 => {
                let path = folder_full_path(project, entry.path.as_deref().unwrap_or(""));
                let output = output_full_path(project, entry.output.as_deref());
                source_and_output.retain(|(s, _)| *s != path);
                source_and_output.push((path, output));
            }
            5 => {
                let path = entry.path.clone().unwrap_or_default();
                if let Some(jdk_path) = path.strip_prefix(crate::project::JRE_CONTAINER) {
                    new_entries.push(new_jdk_entry(project, registry, jdk_path)?);
                } else {
                    tracing::info!("The container entry {path} is not supported to be updated.");
                }
            }
            _ => match convert_classpath_entry(project, &snapshot, entry) {
                Some(e) => dependencies.push(e),
                None => return Err("Invalid classpath entry".to_owned()),
            },
        }
    }
    new_entries.extend(resolve_source_classpath_entries(
        project,
        &source_and_output,
        &[],
        None,
    ));
    new_entries.extend(resolve_dependency_entries(project, dependencies));
    validate_classpath(project, &new_entries)?;
    set_raw_classpath(project, new_entries)
}

/// `ProjectCommand.updateProjectJdk(uri, jdkPath)`: a `JdkUpdateResult`.
pub fn update_project_jdk(
    ws: &mut Workspace,
    registry: &mut RuntimeRegistry,
    uri: &str,
    jdk_path: &str,
) -> Result<Value, String> {
    let project = project_mut(ws, uri)?;
    let mut classpath = Vec::new();
    for e in &project.classpath {
        if e.kind == EntryKind::Container && e.path.starts_with(crate::project::JRE_CONTAINER) {
            match new_jdk_entry(project, registry, jdk_path) {
                Ok(e) => classpath.push(e),
                Err(message) => return Ok(json!({ "success": false, "message": message })),
            }
        } else {
            classpath.push(e.clone());
        }
    }
    if let Err(message) = set_raw_classpath(project, classpath) {
        return Ok(json!({ "success": false, "message": message }));
    }
    Ok(json!({ "success": true, "message": jdk_path }))
}

/// What `ProjectCommand.updateProjectSettings` changed.
#[derive(Debug, Default)]
pub struct SettingsUpdate {
    /// `javaProject.setOptions`: the project-specific options were written.
    pub options_changed: bool,
    /// The Maven resolver configuration changed: update this project.
    pub update_project: Option<String>,
}

/// `ProjectCommand.updateProjectSettings(uri, options)`; `current` resolves
/// the project's effective options (`javaProject.getOptions(true)`).
pub fn update_project_settings(
    ws: &mut Workspace,
    uri: &str,
    options: &Map<String, Value>,
    current: impl Fn(&Project, &str) -> Option<String>,
) -> Result<SettingsUpdate, String> {
    let project = project_mut(ws, uri)?;
    let mut update = SettingsUpdate::default();
    let mut new_options: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in options {
        let text = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if key == M2E_SELECTED_PROFILES {
            let selected = text
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            if project.selected_profiles == selected {
                continue;
            }
            crate::project::maven::write_resolver_configuration(&project.location, &project.name, &selected)
                .map_err(|e| e.to_string())?;
            project.selected_profiles = selected;
            update.update_project = Some(project.name.clone());
            continue;
        }
        // Only valid keys whose values differ are updated.
        if let Some(setting) = current(project, key) {
            if setting != text.trim() {
                new_options.insert(key.clone(), text);
            }
        }
    }
    if !new_options.is_empty() {
        let prefs = crate::project::metadata::resolve(
            &project.location,
            &project.name,
            ".settings/org.eclipse.jdt.core.prefs",
        );
        let mut specific = crate::project::prefs::read_properties(&prefs).unwrap_or_default();
        specific.extend(new_options.clone());
        specific.insert("eclipse.preferences.version".into(), "1".into());
        crate::project::prefs::write_properties(&prefs, &specific).map_err(|e| e.to_string())?;
        project.options.extend(new_options);
        update.options_changed = true;
    }
    Ok(update)
}

/// `VmCommand.getAllVmInstalls()`.
pub fn get_all_vm_installs(registry: &RuntimeRegistry) -> Value {
    Value::Array(registry.all_vm_installs())
}

#[cfg(test)]
mod project_command_test {
    use super::*;
    use crate::project::{ImportSettings, Workspace};

    fn copy_dir(from: &Path, to: &Path) {
        for entry in walkdir::WalkDir::new(from).into_iter().flatten() {
            let target = to.join(entry.path().strip_prefix(from).unwrap());
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target).unwrap();
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    #[test]
    fn test_update_source_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("salut2");
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects/maven/salut2"), &root);
        let mut ws = Workspace::import(&[root], &ImportSettings::jdtls_defaults());
        let project = ws.projects.iter_mut().find(|p| p.name == "salut2").unwrap();

        let source_and_output = [
            ("src/main/java".to_owned(), Some("bin".to_owned())),
            ("src/main/java/aaa".to_owned(), Some("bin".to_owned())),
        ];
        update_source_paths(project, &source_and_output).unwrap();

        let new_source_paths = project.classpath.iter().filter(|e| e.kind == EntryKind::Source).count();
        assert_eq!(2, new_source_paths);
    }
}
