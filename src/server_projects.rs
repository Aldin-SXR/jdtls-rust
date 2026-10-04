//! Project-manager behaviour of the server (jdt.ls `StandardProjectsManager`,
//! `InvisibleProjectBuildSupport`, `InvisibleProjectPreferenceChangeListener`
//! and the document life-cycle hooks that create or extend invisible
//! projects).  A child module of `server` so it can reach the server state.

use super::*;
use crate::project::{invisible, ImportSettings, ProjectKind, Workspace};
use std::path::{Path, PathBuf};

impl JavaLanguageServer {
    /// The current workspace model.
    pub(crate) fn workspace_snapshot(&self) -> Workspace {
        self.dispatcher.workspace.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Install `ws` as the workspace model: register its source files, the
    /// file watchers, and rebuild.
    pub(crate) async fn install_workspace(&self, ws: Workspace) {
        let files: Vec<Url> = ws.java_files().into_keys().filter_map(|p| Url::from_file_path(p).ok()).collect();
        self.store.set_workspace_files(files);
        *self.dispatcher.workspace.write().unwrap_or_else(|e| e.into_inner()) = ws;
        if WATCHERS.lock().unwrap().is_some() {
            register_watchers(&self.client, &self.dispatcher, &self.config).await;
        }
        self.request_compile();
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
        let Some(path) = crate::project::uri_to_path(uri) else { return };
        if !path.is_file() || path.extension().is_none_or(|e| e != "java") {
            return;
        }
        let roots = self.roots.read().await.clone();
        let Some(root) = roots.iter().map(|r| crate::project::canonicalize_lenient(r)).find(|r| path.starts_with(r)) else { return };
        let mut ws = self.workspace_snapshot();
        let settings = self.current_import_settings().await;
        let owner = ws.project_for_path(&path).map(|p| (p.name.clone(), p.kind));
        match owner {
            None => {
                if ws.projects.iter().any(|p| p.kind == ProjectKind::Invisible && p.root == root) {
                    return;
                }
                if let Some(p) = invisible::load_invisible_project(&path, &root, &settings, &ws) {
                    ws.default_project.get_or_insert_with(|| settings.workspace_location(crate::project::DEFAULT_PROJECT_NAME));
                    ws.add(p);
                    ws.finish();
                    self.extra_triggers.write().await.push(path.clone());
                    self.install_workspace(ws).await;
                }
            }
            Some((name, ProjectKind::Invisible)) if settings.source_paths.is_none() => {
                let Some(project) = ws.projects.iter_mut().find(|p| p.name == name) else { return };
                if invisible::needs_source_root_inference(project, &path) && invisible::infer_source_root(project, &path, &roots, &settings) {
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
        for p in ws.projects.iter_mut().filter(|p| p.kind == ProjectKind::Invisible) {
            let real = p.root.clone();
            let matches = |pattern: &str, path: &Path| -> bool {
                let glob = invisible::resolve_glob_path(&real, pattern);
                let g = glob.to_string_lossy().replace('\\', "/");
                invisible::ant_pattern(g.trim_start_matches('/')).is_some_and(|re| re.is_match(path.to_string_lossy().trim_start_matches('/')))
            };
            let relevant = paths.iter().any(|path| {
                path.starts_with(&real)
                    && !libs.exclude.iter().any(|x| matches(&invisible::expand_path(x), path))
                    && libs.include.iter().any(|i| matches(&invisible::expand_path(i), path))
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
                    if e.kind == crate::project::EntryKind::Library && e.source_attachment.as_deref().is_some_and(|s| !s.exists()) {
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
    pub(crate) async fn on_import_settings_changed(&self, old: &ImportSettings, new: &ImportSettings) {
        let mut ws = self.workspace_snapshot();
        let roots = self.roots.read().await.clone();
        let mut changed = false;
        let mut error: Option<String> = None;
        if old.source_paths != new.source_paths {
            for p in ws.projects.iter_mut().filter(|p| p.kind == ProjectKind::Invisible) {
                if !roots.iter().any(|r| p.root.starts_with(crate::project::canonicalize_lenient(r))) {
                    continue;
                }
                match invisible::apply_source_paths_preference(p, new.source_paths.as_deref(), new.output_path.as_deref()) {
                    Ok(()) => changed = true,
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
        } else if old.output_path != new.output_path {
            for p in ws.projects.iter_mut().filter(|p| p.kind == ProjectKind::Invisible) {
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
            for p in ws.projects.iter_mut().filter(|p| p.kind == ProjectKind::Invisible) {
                invisible::update_referenced_libraries(p, &new.referenced_libraries);
                changed = true;
            }
        }
        if old.null_analysis != new.null_analysis {
            let vm = ws.vm_version.clone();
            for p in ws.projects.iter_mut() {
                changed |= crate::project::null_analysis::update_project(p, &new.null_analysis, vm.as_deref());
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
