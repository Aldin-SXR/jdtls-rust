//! Project-manager behaviour of the server (jdt.ls `StandardProjectsManager`,
//! `InvisibleProjectBuildSupport`, `InvisibleProjectPreferenceChangeListener`
//! and the document life-cycle hooks that create or extend invisible
//! projects).  A child module of `server` so it can reach the server state.

use super::*;
use crate::project::{invisible, ImportSettings, ProjectKind, Workspace};
use std::path::{Path, PathBuf};

impl JavaLanguageServer {
    /// `MavenSourceDownloader.discoverSource` for the library of the class
    /// file `uri`: download its sources (waiting up to `MAX_TIME_MILLIS`) and
    /// attach them to the projects using the library.
    pub(crate) async fn discover_source(&self, uri: &str) {
        use crate::project::source_discovery as discovery;
        let mut changed = false;
        let mut ws = self.workspace_snapshot();
        for (jar, sources, javadoc) in discovery::take_completed() {
            changed |= attach_downloaded(&mut ws, &jar, sources, javadoc);
        }
        if let Some((desc, class_file)) = crate::features::navigation::class_file_target(&ws, uri) {
            let jar = PathBuf::from(&desc.root);
            let discovers = match ws.project(&class_file.project).map(|p| p.kind) {
                Some(ProjectKind::Gradle) => false,
                Some(ProjectKind::Eclipse) => {
                    crate::features::preferences::get_bool("java.eclipse.downloadSources").unwrap_or(false)
                }
                _ => true,
            };
            let attached = ws.projects.iter().any(|p| {
                p.libraries.iter().any(|l| {
                    l.path == jar && l.source.as_ref().is_some_and(|s| s.exists())
                })
            });
            if discovers && desc.module.is_none() && !attached && jar.is_file() && discovery::first_request(&jar) {
                let settings = self.current_import_settings().await;
                let job = tokio::task::spawn_blocking(move || {
                    let resolver = crate::project::maven::Resolver::with_settings(&settings.maven);
                    let key = discovery::identify_in_local_repository(&jar, &resolver.local_repo)?;
                    let download = |classifier: &str| {
                        resolver.download_artifact(&key.group, &key.artifact, &key.version, Some(classifier), "jar")
                    };
                    let sources = download("sources");
                    let javadoc = download("javadoc");
                    discovery::complete(jar, sources, javadoc);
                    Some(())
                });
                let _ = tokio::time::timeout(std::time::Duration::from_millis(3000), job).await;
                for (jar, sources, javadoc) in discovery::take_completed() {
                    changed |= attach_downloaded(&mut ws, &jar, sources, javadoc);
                }
            }
        }
        if changed {
            let _guard = self.import_lock.lock().await;
            let mut current = self.workspace_snapshot();
            for p in current.projects.iter_mut() {
                if let Some(updated) = ws.project(&p.name) {
                    p.classpath = updated.classpath.clone();
                }
                p.derive_views();
            }
            self.install_workspace(current).await;
        }
    }

    pub(crate) async fn change_source_path(&self, uri: String, add: bool) -> LspResult<Value> {
        let _guard = self.import_lock.lock().await;
        let mut ws = self.workspace_snapshot();
        let roots = self.roots.read().await.clone();
        let settings = self.current_import_settings().await;
        let (ws, change) = tokio::task::spawn_blocking(move || {
            let change = crate::features::build_path::change(&mut ws, &settings, &roots, &uri, add);
            (ws, change)
        }).await.map_err(|e| internal_error(e.to_string()))?;
        if change.changed { self.install_workspace(ws).await; }
        Ok(change.result)
    }

    /// Settings changes report runtime validation failures to the client.
    pub(crate) async fn send_runtime_notices(&self) {
        let (messages, actionable) = {
            let mut config = self.config.write().await;
            let actionable = config.extended_capability("actionableRuntimeNotificationSupport");
            (std::mem::take(&mut config.runtime_notices), actionable)
        };
        for message in messages {
            if actionable {
                let notice = crate::project::runtime::notice(&message, true);
                self.client.send_notification::<crate::project::runtime::ActionableNotification>(notice["params"].clone()).await;
            } else {
                self.client.show_message(MessageType::ERROR, message).await;
            }
        }
    }

    /// The current workspace model.
    pub(crate) fn workspace_snapshot(&self) -> Workspace {
        self.dispatcher
            .workspace
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Install `ws` as the workspace model: register its source files, the
    /// file watchers, and rebuild.
    pub(crate) async fn install_workspace(&self, mut ws: Workspace) {
        let previous = self.workspace_snapshot();
        let settings = self.current_import_settings().await;
        ws.configure_filters(&settings.resource_filters);
        if let Some(registry) = &settings.runtime_registry {
            registry.apply_to_workspace(&mut ws);
        }
        if let Err(error) = ws.ensure_default_project() {
            tracing::error!("Unable to create default Java project: {error}");
        }
        let files: Vec<Url> = ws
            .java_files()
            .into_keys()
            .filter_map(|p| Url::from_file_path(p).ok())
            .collect();
        self.store.set_workspace_files(files);
        *self
            .dispatcher
            .workspace
            .write()
            .unwrap_or_else(|e| e.into_inner()) = ws;
        if self.service_ready.load(std::sync::atomic::Ordering::SeqCst) {
            self.register_watchers().await;
            self.send_classpath_updates(&previous).await;
        }
        if self.legacy_diagnostics().await {
            self.request_compile();
        } else {
            self.lifecycle.build(None).await;
            for uri in self.store.open_uris() {
                if crate::features::lifecycle::is_java_like(&uri) {
                    self.lifecycle.publish_unit(&uri).await;
                }
            }
        }
    }

    /// `ClasspathUpdateHandler.elementChanged`: the projects whose classpath
    /// differs from `previous` are announced to the client.
    pub(crate) async fn send_classpath_updates(&self, previous: &Workspace) {
        let changed: Vec<PathBuf> = self
            .workspace_snapshot()
            .projects
            .iter()
            .filter(|p| previous.project(&p.name).is_some_and(|old| old.classpath != p.classpath))
            .map(|p| p.root.clone())
            .collect();
        for root in changed {
            let Ok(uri) = Url::from_directory_path(&root) else { continue };
            let uri = uri.as_str().replacen("file:///", "file:/", 1);
            self.send_event_notification(EventType::ClasspathUpdated, json!(uri)).await;
        }
    }

    pub(crate) async fn send_event_notification(&self, event_type: EventType, data: Value) {
        self.client
            .send_notification::<EventNotification>(json!({ "eventType": event_type as i32, "data": data }))
            .await;
    }

    /// The import settings with the trigger files opened since startup.
    pub(crate) async fn current_import_settings(&self) -> ImportSettings {
        let mut s = import_settings(&*self.config.read().await);
        for t in self.extra_triggers.read().await.iter() {
            if !s.trigger_files.contains(t) {
                s.trigger_files.push(t.clone());
            }
        }
        s
    }

    /// `DocumentLifeCycleHandler.resolveCompilationUnit` / `handleOpen`: a
    /// standalone file under a root gets an invisible project
    /// (`loadInvisibleProject`); a file of an invisible project that is not
    /// on its classpath gets its source root inferred (`inferSourceRoot`).
    pub(crate) async fn on_document_opened(&self, uri: &Url) {
        // `JDTUtils.getFakeCompilationUnit` creates the default project when
        // opening a standalone unit. This also covers virtual buffers; only
        // the metadata is materialized, never the user's source file.
        if crate::features::lifecycle::is_java_like(uri)
            && crate::features::lifecycle::classify(&self.workspace_snapshot(), uri)
                == crate::features::lifecycle::UnitKind::Default
        {
            let settings = self.current_import_settings().await;
            let mut ws = self.dispatcher.workspace.write().unwrap_or_else(|e| e.into_inner());
            ws.default_project.get_or_insert_with(|| settings.workspace_location(crate::project::DEFAULT_PROJECT_NAME));
            if let Err(error) = ws.ensure_default_project() {
                tracing::error!("Unable to create default Java project: {error}");
            }
        }
        let Some(path) = crate::project::uri_to_path(uri) else {
            return;
        };
        if !path.is_file() || path.extension().is_none_or(|e| e != "java") {
            return;
        }
        let roots = self.roots.read().await.clone();
        let Some(root) = roots
            .iter()
            .map(|r| crate::project::canonicalize_lenient(r))
            .find(|r| path.starts_with(r))
        else {
            return;
        };
        let mut ws = self.workspace_snapshot();
        let settings = self.current_import_settings().await;
        let owner = ws.project_for_path(&path).map(|p| (p.name.clone(), p.kind));
        match owner {
            None => {
                if ws
                    .projects
                    .iter()
                    .any(|p| p.kind == ProjectKind::Invisible && p.root == root)
                {
                    return;
                }
                if let Some(p) = invisible::load_invisible_project(&path, &root, &settings, &ws) {
                    ws.default_project.get_or_insert_with(|| {
                        settings.workspace_location(crate::project::DEFAULT_PROJECT_NAME)
                    });
                    ws.add(p);
                    ws.finish();
                    self.extra_triggers.write().await.push(path.clone());
                    self.install_workspace(ws).await;
                }
            }
            Some((name, ProjectKind::Invisible)) if settings.source_paths.is_none() => {
                let Some(project) = ws.projects.iter_mut().find(|p| p.name == name) else {
                    return;
                };
                if invisible::needs_source_root_inference(project, &path)
                    && invisible::infer_source_root(project, &path, &roots, &settings)
                {
                    ws.finish();
                    self.install_workspace(ws).await;
                }
            }
            _ => {}
        }
    }

    /// `InvisibleProjectBuildSupport.fileChanged` and the library part of
    /// `EclipseBuildSupport.fileChanged`: referenced libraries of invisible
    /// projects follow jar changes; build path markers are re-validated.
    /// Returns whether the workspace changed.
    pub(crate) async fn on_files_changed(&self, paths: &[PathBuf]) -> bool {
        let settings = self.current_import_settings().await;
        let libs = settings.referenced_libraries.clone();
        let mut ws = self.workspace_snapshot();
        let mut changed = false;
        for p in ws
            .projects
            .iter_mut()
            .filter(|p| p.kind == ProjectKind::Invisible)
        {
            let real = p.root.clone();
            let matches = |pattern: &str, path: &Path| -> bool {
                let glob = invisible::resolve_glob_path(&real, pattern);
                let g = glob.to_string_lossy().replace('\\', "/");
                invisible::ant_pattern(g.trim_start_matches('/'))
                    .is_some_and(|re| re.is_match(path.to_string_lossy().trim_start_matches('/')))
            };
            let relevant = paths.iter().any(|path| {
                path.starts_with(&real)
                    && !libs
                        .exclude
                        .iter()
                        .any(|x| matches(&invisible::expand_path(x), path))
                    && libs
                        .include
                        .iter()
                        .any(|i| matches(&invisible::expand_path(i), path))
            });
            if relevant {
                invisible::update_referenced_libraries(p, &libs);
                changed = true;
            }
        }
        let touches_libraries = paths.iter().any(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.ends_with(".jar") || name.ends_with(".zip") || !p.exists() || p.is_dir()
        });
        if changed || touches_libraries {
            let before: Vec<_> = ws.projects.iter().map(|p| p.markers.clone()).collect();
            for p in ws.projects.iter_mut() {
                for e in p.classpath.iter_mut() {
                    if e.kind == crate::project::EntryKind::Library
                        && e.source_attachment.as_deref().is_some_and(|s| !s.exists())
                    {
                        e.source_attachment = None;
                        changed = true;
                    }
                }
                p.derive_views();
            }
            ws.finish();
            let after: Vec<_> = ws.projects.iter().map(|p| p.markers.clone()).collect();
            changed |= before != after;
        }
        if changed {
            self.install_workspace(ws).await;
        }
        changed
    }

    /// `InvisibleProjectPreferenceChangeListener.preferencesChange` and the
    /// other import-preference listeners (referenced libraries, null analysis).
    pub(crate) async fn on_import_settings_changed(
        &self,
        old: &ImportSettings,
        new: &ImportSettings,
    ) {
        let mut ws = self.workspace_snapshot();
        let roots = self.roots.read().await.clone();
        let mut changed = false;
        let mut error: Option<String> = None;
        if old.runtime_registry != new.runtime_registry {
            if let Some(registry) = &new.runtime_registry {
                registry.apply_to_workspace(&mut ws);
                changed = true;
            }
        }
        if old.resource_filters != new.resource_filters {
            ws.configure_filters(&new.resource_filters);
            changed = true;
        }
        if old.source_paths != new.source_paths {
            for p in ws
                .projects
                .iter_mut()
                .filter(|p| p.kind == ProjectKind::Invisible)
            {
                if !roots
                    .iter()
                    .any(|r| p.root.starts_with(crate::project::canonicalize_lenient(r)))
                {
                    continue;
                }
                match invisible::apply_source_paths_preference(
                    p,
                    new.source_paths.as_deref(),
                    new.output_path.as_deref(),
                ) {
                    Ok(()) => changed = true,
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
        } else if old.output_path != new.output_path {
            for p in ws
                .projects
                .iter_mut()
                .filter(|p| p.kind == ProjectKind::Invisible)
            {
                match invisible::apply_output_path_preference(p, new.output_path.as_deref()) {
                    Ok(()) => changed = true,
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
        }
        if old.referenced_libraries != new.referenced_libraries {
            for p in ws
                .projects
                .iter_mut()
                .filter(|p| p.kind == ProjectKind::Invisible)
            {
                invisible::update_referenced_libraries(p, &new.referenced_libraries);
                changed = true;
            }
        }
        if old.null_analysis != new.null_analysis {
            let vm = ws.vm_version.clone();
            for p in ws.projects.iter_mut() {
                changed |= crate::project::null_analysis::update_project(
                    p,
                    &new.null_analysis,
                    vm.as_deref(),
                );
            }
        }
        if let Some(message) = error {
            self.client.show_message(MessageType::ERROR, message).await;
        }
        if changed {
            ws.finish();
            self.install_workspace(ws).await;
        }
    }
}

impl JavaLanguageServer {
    /// `ProjectCommand.changeImportedProjects(toImport, toUpdate, toDelete)`
    /// (`ImportProjectsFromSelectionJob`): delete the projects at
    /// `to_delete`, import the build files `to_import`, update `to_update`.
    pub(crate) async fn change_imported_projects(
        &self,
        to_import: &[String],
        to_update: &[String],
        to_delete: &[String],
    ) {
        let to_paths = |uris: &[String]| -> Vec<PathBuf> {
            uris.iter()
                .filter_map(|u| Url::parse(u).ok())
                .filter_map(|u| crate::project::uri_to_path(&u))
                .collect()
        };
        let delete = to_paths(to_delete);
        let import = to_paths(to_import);
        let _ = to_update;
        let ws = self.workspace_snapshot();
        let mut configs = match self.config.read().await.project_configurations.clone() {
            Some(c) => c
                .iter()
                .filter_map(|u| Url::parse(u).ok())
                .filter_map(|u| crate::project::uri_to_path(&u))
                .collect::<Vec<_>>(),
            None => ws
                .projects
                .iter()
                .filter(|p| p.kind != ProjectKind::Invisible)
                .flat_map(|p| {
                    p.build_files
                        .iter()
                        .filter(|f| f.is_file())
                        .cloned()
                        .take(1)
                })
                .collect(),
        };
        for d in &delete {
            if let Some(p) = ws
                .projects
                .iter()
                .find(|p| p.root == *d || p.location == *d)
            {
                configs.retain(|c| {
                    !p.build_files.contains(c) && c.parent() != Some(p.location.as_path())
                });
            }
        }
        for i in import {
            if !configs.contains(&i) {
                configs.push(i);
            }
        }
        self.config.write().await.project_configurations = Some(
            configs
                .iter()
                .filter_map(|p| Url::from_file_path(p).ok())
                .map(|u| u.to_string())
                .collect(),
        );
        self.reimport_workspace().await;
        self.request_compile();
    }
}

/// `ProjectsManager.BUILD_FILE_MARKER_TYPE` message.
pub(crate) const BUILD_FILE_CHANGED: &str =
    "The build file has been changed and may need reload to make it effective.";

/// `DigestStore`: SHA-256 digests of the imported build files.
pub(crate) static DIGESTS: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<u8>>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn digest(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    // A simple stable digest (FNV-1a over the content, plus the length).
    let mut h: u64 = 0xcbf29ce484222325;
    for b in &bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let mut out = h.to_le_bytes().to_vec();
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    Some(out)
}

/// `DigestStore.updateDigest(path)`: whether the digest changed.
pub(crate) fn update_digest(path: &Path) -> bool {
    let Some(d) = digest(path) else { return false };
    let mut store = DIGESTS.lock().unwrap();
    match store.insert(path.to_path_buf(), d.clone()) {
        Some(old) => old != d,
        None => true,
    }
}

impl JavaLanguageServer {
    /// Record the digests of every imported build file (the importers do it
    /// for the build files they import).
    pub(crate) fn record_build_file_digests(&self) {
        let ws = self.workspace_snapshot();
        for p in &ws.projects {
            for f in &p.build_files {
                update_digest(f);
            }
        }
    }

    /// `StandardProjectsManager.fileChanged` for a build file: when its
    /// content changed, update the project (`automatic`), or mark the file
    /// and ask the client (`interactive`, the default), or only mark it
    /// (`disabled`).  Returns whether it was a build file of a project.
    pub(crate) async fn on_build_file_changed(&self, path: &Path) -> bool {
        let ws = self.workspace_snapshot();
        let Some(project) = ws
            .projects
            .iter()
            .filter(|p| {
                matches!(
                    crate::project::BuildSupport::of(p),
                    crate::project::BuildSupport::Maven | crate::project::BuildSupport::Gradle
                )
            })
            .find(|p| p.build_files.iter().any(|f| f == path))
            .cloned()
        else {
            return false;
        };
        if !update_digest(path) {
            return true;
        }
        let status = {
            let cfg = self.config.read().await;
            cfg.settings
                .as_ref()
                .and_then(|s| {
                    crate::project::pref_value(s, "java.configuration.updateBuildConfiguration")
                })
                .and_then(Value::as_str)
                .unwrap_or("interactive")
                .to_owned()
        };
        match status.as_str() {
            "automatic" => {
                self.update_projects(&[project.name.clone()]).await;
            }
            "disabled" => self.append_build_file_marker(&project.name, path).await,
            _ => {
                let uri = Url::from_file_path(path)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let cmd = "java.projectConfiguration.status";
                let params = json!({
                    "severity": 3,
                    "message": "A build file was modified. Do you want to synchronize the Java classpath/configuration?",
                    "commands": [
                        { "title": "Yes", "command": cmd, "arguments": [{ "uri": uri }, "interactive"] },
                        { "title": "Always", "command": cmd, "arguments": [{ "uri": uri }, "automatic"] },
                        { "title": "Never", "command": cmd, "arguments": [{ "uri": uri }, "disabled"] },
                    ],
                });
                self.client
                    .send_notification::<ActionableNotification>(params)
                    .await;
                self.append_build_file_marker(&project.name, path).await;
            }
        }
        true
    }

    async fn append_build_file_marker(&self, project: &str, path: &Path) {
        let mut ws = self.workspace_snapshot();
        let Some(p) = ws.projects.iter_mut().find(|p| p.name == project) else {
            return;
        };
        if p.markers
            .iter()
            .any(|m| m.resource.as_deref() == Some(path) && m.message == BUILD_FILE_CHANGED)
        {
            return;
        }
        let mut m = crate::project::Marker::project(BUILD_FILE_CHANGED, 3, "0");
        m.resource = Some(path.to_path_buf());
        p.markers.push(m);
        self.install_workspace(ws).await;
    }

    /// `ProjectsManager.updateProjects(projects, force)`: re-import the
    /// workspace (the build supports re-read the build files) and drop the
    /// build file markers of the updated projects.
    pub(crate) async fn update_projects(&self, names: &[String]) {
        self.send_projects_status("Message", "Updating project configurations...")
            .await;
        let before = self.workspace_snapshot();
        self.reimport_workspace().await;
        let mut ws = self.workspace_snapshot();
        for p in ws.projects.iter_mut() {
            let updated = names.contains(&p.name);
            if let Some(old) = before.project(&p.name) {
                // Projects not updated keep their build file markers.
                if !updated {
                    for m in old
                        .markers
                        .iter()
                        .filter(|m| m.message == BUILD_FILE_CHANGED)
                    {
                        if !p.markers.contains(m) {
                            p.markers.push(m.clone());
                        }
                    }
                }
            }
        }
        for name in names {
            if let Some(p) = ws.project(name) {
                for f in p.build_files.clone() {
                    update_digest(&f);
                }
            }
        }
        self.install_workspace(ws).await;
        self.report_projects_status().await;
    }

    /// `ProjectsManager.reportProjectsStatus()`.
    pub(crate) async fn report_projects_status(&self) {
        let ws = self.workspace_snapshot();
        let error = ws.projects.iter().any(|p| {
            p.markers.iter().any(|m| {
                m.severity == 1
                    && (m.resource.is_none()
                        || p.build_files
                            .iter()
                            .any(|f| Some(f.as_path()) == m.resource.as_deref()))
            })
        });
        self.send_projects_status("ProjectStatus", if error { "WARNING" } else { "OK" })
            .await;
    }

    pub(crate) async fn send_projects_status(&self, typ: &str, message: &str) {
        self.client
            .send_notification::<LanguageStatus>(LanguageStatusParams {
                typ: typ.to_owned(),
                message: message.to_owned(),
            })
            .await;
    }

    /// `java/projectConfigurationUpdate` (`ProjectConfigurationUpdateHandler`).
    pub async fn project_configuration_update(&self, params: Value) {
        let identifiers = match params.get("identifiers") {
            Some(ids) => ids.as_array().cloned().unwrap_or_default(),
            None => vec![params],
        };
        let ws = self.workspace_snapshot();
        let mut names = Vec::new();
        for id in identifiers {
            let Some(path) = id
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|u| Url::parse(u).ok())
                .and_then(|u| crate::project::uri_to_path(&u))
            else {
                continue;
            };
            if let Some(p) = ws
                .projects
                .iter()
                .filter(|p| path.starts_with(&p.root) || path.starts_with(&p.location))
                .max_by_key(|p| p.root.components().count())
            {
                if !names.contains(&p.name) {
                    names.push(p.name.clone());
                }
            }
        }
        if !names.is_empty() {
            self.update_projects(&names).await;
        }
    }

    /// `CreateModuleInfoHandler.createModuleInfo`: the URI of the created `module-info.java`.
    pub(crate) async fn create_module_info(&self, project_uri: &str) -> Option<String> {
        use crate::features::create_module_info as module_info;
        let ws = self.workspace_snapshot();
        let root = Url::parse(project_uri).ok().and_then(|u| crate::project::uri_to_path(&u));
        let project = root
            .and_then(|root| ws.all_projects().into_iter().find(|p| p.root == root || p.location == root))
            .filter(|p| p.is_java());
        let Some(project) = project else {
            self.client.show_message(MessageType::ERROR, "The selected project is not a valid Java project.").await;
            return None;
        };
        let project_dir = Url::from_directory_path(&project.root).ok()?;
        let env = self.format_env().await;
        let mut options = env.jdt_options(Some(&project_dir)).await;
        drop(env);
        // The workspace-wide `JavaCore` options carry the client's tab settings.
        let mut tab_options = std::collections::BTreeMap::new();
        crate::features::preferences::current().update_tab_size_insert_spaces(&mut tab_options);
        for (key, value) in tab_options {
            if !project.options.contains_key(&key) {
                options.insert(key, value);
            }
        }
        let compliance = options.get(crate::project::COMPLIANCE).cloned().unwrap_or_default();
        if !module_info::is_9_or_higher(&compliance) {
            let message = "The project source compliance must be 9 or higher to create module-info.java.";
            self.client.show_message(MessageType::ERROR, message).await;
            return None;
        }
        let roots: Vec<_> = project.source_folders.iter().map(|f| f.path.clone()).filter(|p| p.is_dir()).collect();
        if roots.is_empty() {
            self.client.show_message(MessageType::ERROR, "No source folder exists in the project.").await;
            return None;
        }
        for root in &roots {
            if root.join(module_info::MODULE_INFO_JAVA).is_file() {
                let message = format!(
                    "The module-info.java file already exists in the source folder \"{}\"",
                    root.file_name().unwrap_or_default().to_string_lossy()
                );
                self.client.show_message(MessageType::ERROR, message).await;
                return None;
            }
        }
        let target = roots[0].join(module_info::MODULE_INFO_JAVA);

        let packages: Vec<String> = roots.iter().flat_map(|r| module_info::packages_with_units(r)).collect();
        let exported = module_info::java_hash_set_order(&packages);
        let ctx = self.dispatcher.context_for_project_name(&project.name).await;
        let required = match self
            .dispatcher
            .send_request(crate::analysis::semantic::BridgeRequest::ReferencedModules {
                id: crate::analysis::semantic::ecj_process::next_id(),
                files: ctx.files,
                classpath: ctx.classpath,
                source_level: ctx.source_level,
                options: ctx.options,
            })
            .await
        {
            Ok(crate::analysis::semantic::BridgeResponse::ReferencedModules { modules, .. }) => modules,
            _ => Vec::new(),
        };
        let delimiter = if cfg!(windows) { "\r\n" } else { "\n" };
        let text = module_info::module_info_text(&module_info::module_name(&project.name), &exported, &required, delimiter);
        let length = text.encode_utf16().count();
        let formatted = match self
            .dispatcher
            .format_source(&text, crate::rewrite::formatter::K_MODULE_INFO, 0, length, delimiter, options)
            .await
        {
            Ok(Some(edits)) => {
                let mut units: Vec<u16> = text.encode_utf16().collect();
                for edit in edits.iter().rev() {
                    units.splice(edit.offset..edit.offset + edit.length, edit.text.encode_utf16());
                }
                String::from_utf16_lossy(&units)
            }
            _ => text,
        };
        std::fs::write(&target, formatted).ok()?;

        let mut ws = self.workspace_snapshot();
        if let Some(p) = ws.projects.iter_mut().find(|p| p.name == project.name) {
            for entry in p.classpath.iter_mut().filter(|e| !matches!(e.kind, crate::project::EntryKind::Source)) {
                match entry.attributes.iter_mut().find(|(name, _)| name == "module") {
                    Some((_, value)) => *value = "true".to_owned(),
                    None => entry.attributes.push(("module".to_owned(), "true".to_owned())),
                }
            }
        }
        self.install_workspace(ws).await;
        Some(format!("file:{}", target.to_string_lossy()))
    }

    /// `java/projectConfigurationsUpdate`.
    pub async fn project_configurations_update(&self, params: Value) {
        self.project_configuration_update(params).await;
    }
}

/// jdt.ls `language/eventNotification`.
pub(crate) enum EventNotification {}

impl tower_lsp::lsp_types::notification::Notification for EventNotification {
    type Params = Value;
    const METHOD: &'static str = "language/eventNotification";
}

/// jdt.ls `EventType`.
#[derive(Clone, Copy)]
pub(crate) enum EventType {
    ClasspathUpdated = 100,
}

/// jdt.ls `language/actionableNotification`.
pub(crate) enum ActionableNotification {}

impl tower_lsp::lsp_types::notification::Notification for ActionableNotification {
    type Params = Value;
    const METHOD: &'static str = "language/actionableNotification";
}

/// `ProgressReporterManager`'s extended progress reports.
enum ProjectProgress {}
impl tower_lsp::lsp_types::notification::Notification for ProjectProgress {
    type Params = Value;
    const METHOD: &'static str = "language/progressReport";
}
static NEXT_PROGRESS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl JavaLanguageServer {
    async fn project_progress(&self, id: &str, task: &str, complete: bool) {
        self.client
            .send_notification::<ProjectProgress>(json!({
                "id": id, "task": task, "subTask": "", "status": task,
                "totalWork": 100, "workDone": if complete { 100 } else { 0 }, "complete": complete,
            }))
            .await;
    }

    pub(super) async fn begin_maven_import_progress(
        &self,
        roots: &[PathBuf],
        settings: &ImportSettings,
    ) -> Option<String> {
        if !settings.maven_enabled
            || !self
                .config
                .read()
                .await
                .extended_capability("progressReportProvider")
        {
            return None;
        }
        let has_pom = roots.iter().any(|root| {
            !crate::project::detect::FileDetector::new(root, &["pom.xml"])
                .include_nested(false)
                .add_exclusions(&settings.exclusions)
                .scan()
                .is_empty()
        });
        if !has_pom {
            return None;
        }
        let id = format!(
            "{}-{}",
            std::process::id(),
            NEXT_PROGRESS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        self.project_progress(&id, "Importing Maven project(s)", false)
            .await;
        Some(id)
    }

    pub(super) async fn begin_gradle_import_progress(
        &self,
        roots: &[PathBuf],
        settings: &ImportSettings,
    ) -> Option<String> {
        if !settings.gradle_enabled
            || !self
                .config
                .read()
                .await
                .extended_capability("progressReportProvider")
        {
            return None;
        }
        let has_build = roots.iter().any(|root| {
            !crate::project::detect::FileDetector::new(root, crate::project::gradle::BUILD_FILES)
                .include_nested(false)
                .add_exclusions(["**/build", "**/bin"])
                .add_exclusions(&settings.exclusions)
                .scan()
                .is_empty()
        });
        if !has_build {
            return None;
        }
        let id = format!(
            "{}-{}",
            std::process::id(),
            NEXT_PROGRESS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        self.project_progress(&id, "Importing Gradle project(s)", false)
            .await;
        Some(id)
    }

    pub(super) async fn complete_gradle_import_progress(&self, import_id: Option<String>) {
        if let Some(id) = import_id {
            self.project_progress(&id, "Importing Gradle project(s)", true)
                .await;
        }
    }

    pub(super) async fn complete_maven_import_progress(
        &self,
        ws: &Workspace,
        import_id: Option<String>,
    ) {
        // Persist the imported POM stamps in the server's metadata area, so a
        // restart distinguishes unchanged projects from updated configurations.
        let state_path = data_dir().join(".metadata/jdtls-rust/maven-imports.json");
        let previous: std::collections::BTreeMap<String, u64> = std::fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut current = std::collections::BTreeMap::new();
        for p in ws.projects.iter().filter(|p| p.kind == ProjectKind::Maven) {
            let pom = p.root.join("pom.xml");
            if let Some(stamp) = std::fs::metadata(&pom)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|t| t.as_nanos() as u64)
            {
                current.insert(pom.to_string_lossy().into_owned(), stamp);
            }
        }
        let updated = current
            .iter()
            .any(|(path, stamp)| previous.get(path).is_some_and(|old| stamp > old));
        if let Some(id) = import_id {
            self.project_progress(&id, "Importing Maven project(s)", true)
                .await;
            if updated {
                let id = format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT_PROGRESS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                );
                self.project_progress(&id, "Update Maven project configuration", false)
                    .await;
                self.project_progress(&id, "Update Maven project configuration", true)
                    .await;
            }
        }
        if let Some(parent) = state_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent).and_then(|_| {
                let mut file = tempfile::NamedTempFile::new_in(parent)?;
                std::io::Write::write_all(&mut file, &serde_json::to_vec(&current)?)?;
                file.persist(&state_path).map_err(|e| e.error)?;
                Ok(())
            }) {
                tracing::warn!("Maven import metadata: {e}");
            }
        }
    }
}

/// Attaches downloaded sources and Javadoc to every classpath entry of `jar`.
fn attach_downloaded(ws: &mut Workspace, jar: &Path, sources: Option<PathBuf>, javadoc: Option<PathBuf>) -> bool {
    fn visit(entries: &mut [crate::project::ClasspathEntry], jar: &Path, sources: &Option<PathBuf>, javadoc: &Option<PathBuf>) -> bool {
        let mut changed = false;
        for e in entries {
            if e.location.as_deref() == Some(jar) {
                if let Some(s) = sources.as_ref().filter(|_| e.source_attachment.is_none()) {
                    e.source_attachment = Some(s.clone());
                    changed = true;
                }
                if let Some(j) = javadoc.as_ref().filter(|_| e.attribute("javadoc_location").is_none()) {
                    e.set_attribute("javadoc_location", &format!("jar:file:{}!/", j.to_string_lossy()));
                    changed = true;
                }
            }
            changed |= visit(&mut e.children, jar, sources, javadoc);
        }
        changed
    }
    let mut changed = false;
    for p in ws.projects.iter_mut() {
        if visit(&mut p.classpath, jar, &sources, &javadoc) {
            p.derive_views();
            changed = true;
        }
    }
    changed
}
