//! The `ProjectCommand`/`VmCommand` delegate commands that change the
//! project model (`java.project.updateClassPaths`, `updateSettings`,
//! `updateJdk`), `java.vm.getAllInstalls` and
//! `java.project.resolveWorkspaceSymbol`.  A child module of `server` so it
//! can reach the server state; the model logic lives in
//! `features::project_commands`.

use super::*;
use crate::features::project_commands as pc;
use crate::project::runtime::RuntimeRegistry;

impl JavaLanguageServer {
    /// `JavaRuntime`'s VM installs: the configured registry, else the
    /// installs detected around the default VM.
    async fn vm_registry(&self) -> RuntimeRegistry {
        let cfg = self.config.read().await;
        cfg.runtime_registry.clone().unwrap_or_else(|| {
            vm_home(&cfg)
                .as_deref()
                .map(RuntimeRegistry::with_default_home)
                .unwrap_or_default()
        })
    }

    /// Install a changed model (and VM registry) like a workspace job would.
    async fn commit_project_change(&self, ws: crate::project::Workspace, registry: Option<RuntimeRegistry>) {
        if let Some(registry) = registry {
            self.config.write().await.runtime_registry = Some(registry);
        }
        self.install_workspace(ws).await;
    }

    /// `IJavaProject.getOption(key, true)` as `java.project.getSettings`
    /// resolves it.
    pub(crate) async fn project_option_resolver(
        &self,
    ) -> impl Fn(&crate::project::Project, &str) -> Option<String> {
        let cfg = self.config.read().await.clone();
        let vm = vm_home(&cfg);
        let vm_version = vm.as_deref().and_then(crate::project::vm_version);
        let formatter = formatting::options::workspace_formatter_options(&cfg.format, &cfg.root_paths);
        let settings = crate::project::prefs::settings_url_options(
            crate::features::preferences::current().get_settings_url(),
            &cfg.root_paths,
        );
        move |p: &crate::project::Project, key: &str| -> Option<String> {
            if let Some(v) = p.options.get(key) {
                return Some(v.clone());
            }
            if let Some(v) = cfg.compiler_options.get(key) {
                return Some(v.clone());
            }
            if let Some(v) = formatter.get(key) {
                return Some(v.clone());
            }
            if let Some(v) = settings.get(key) {
                return Some(v.clone());
            }
            crate::project::effective_option(p, key, vm_version.as_deref())
        }
    }

    /// The project commands handled here, or `None` for other commands.
    pub(crate) async fn project_command(&self, params: &ExecuteCommandParams) -> Option<LspResult<Option<Value>>> {
        let arg = |i: usize| params.arguments.get(i);
        let string = |i: usize| arg(i).and_then(Value::as_str).unwrap_or_default().to_owned();
        Some(match params.command.as_str() {
            "java.vm.getAllInstalls" => Ok(Some(pc::get_all_vm_installs(&self.vm_registry().await))),
            "java.project.updateClassPaths" => {
                let uri = string(0);
                let entries: pc::ProjectClasspathEntries = arg(1)
                    .and_then(json_model)
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let _guard = self.import_lock.lock().await;
                let mut ws = self.workspace_snapshot();
                let mut registry = self.vm_registry().await;
                let before = registry.clone();
                match pc::update_classpaths(&mut ws, &mut registry, &uri, &entries.classpath_entries) {
                    Ok(()) => {
                        let changed = (registry != before).then_some(registry);
                        self.commit_project_change(ws, changed).await;
                        Ok(None)
                    }
                    Err(message) => Err(internal_error(message)),
                }
            }
            "java.project.updateJdk" => {
                let (uri, jdk_path) = (string(0), string(1));
                let _guard = self.import_lock.lock().await;
                let mut ws = self.workspace_snapshot();
                let mut registry = self.vm_registry().await;
                match pc::update_project_jdk(&mut ws, &mut registry, &uri, &jdk_path) {
                    Ok(result) => {
                        if result["success"] == true {
                            self.commit_project_change(ws, Some(registry)).await;
                        }
                        Ok(Some(result))
                    }
                    Err(message) => Err(internal_error(message)),
                }
            }
            "java.project.updateSettings" => {
                let uri = string(0);
                let options = arg(1)
                    .and_then(json_model)
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                let current = self.project_option_resolver().await;
                let update = {
                    let _guard = self.import_lock.lock().await;
                    let mut ws = self.workspace_snapshot();
                    match pc::update_project_settings(&mut ws, &uri, &options, current) {
                        Ok(update) => {
                            if update.options_changed {
                                self.commit_project_change(ws, None).await;
                            }
                            update
                        }
                        Err(message) => return Some(Err(internal_error(message))),
                    }
                };
                if let Some(name) = update.update_project {
                    // `ProjectsManager.updateProject(project, true)`.
                    self.update_projects(&[name]).await;
                }
                Ok(None)
            }
            "java.project.resolveWorkspaceSymbol" => self.resolve_workspace_symbol(arg(0)).await,
            _ => return None,
        })
    }

    /// `ProjectCommand.resolveWorkspaceSymbol(SymbolInformation)`: the
    /// location of the type named like the symbol in its type root, searched
    /// breadth-first through the member types.
    async fn resolve_workspace_symbol(&self, arg: Option<&Value>) -> LspResult<Option<Value>> {
        let Some(request) = arg.and_then(json_model).filter(Value::is_object) else {
            return Err(internal_error(
                "Cannot invoke \"org.eclipse.lsp4j.SymbolInformation.getLocation()\" because \"request\" is null"
                    .to_owned(),
            ));
        };
        let uri = request["location"]["uri"].as_str().unwrap_or_default().to_owned();
        let Ok(url) = Url::parse(&uri) else {
            return Ok(None);
        };
        let Some(text) = crate::features::document_text(&self.dispatcher, &url).await else {
            return Ok(None);
        };
        let name = request["name"].as_str().unwrap_or_default();
        let mut location = request["location"].clone();
        let mut cu = crate::features::java_model::parse(&text);
        let mut location_uri = if crate::classfile::is_class_file_uri(&url) {
            let reference = crate::classfile::ClassFileRef::parse(&uri);
            let chain: Vec<String> = reference
                .as_ref()
                .map(|r| r.class_file.trim_end_matches(".class").split('$').map(str::to_owned).collect())
                .unwrap_or_default();
            let chain: Vec<&str> = chain.iter().map(String::as_str).collect();
            cu.types = crate::features::document_symbol::binary_type(&cu.types, &chain)
                .cloned()
                .into_iter()
                .collect();
            reference.map(|mut r| {
                if r.source_file_name.is_none() {
                    r.source_file_name = chain.first().map(|t| format!("{t}.java"));
                }
                r.to_uri()
            })
        } else {
            url.to_file_path().ok().and_then(|p| Url::from_file_path(p).ok()).map(|u| u.to_string())
        };
        let mut queue: std::collections::VecDeque<&crate::features::java_model::TypeDecl> =
            cu.types.iter().collect();
        while let Some(ty) = queue.pop_front() {
            if ty.name == name {
                let range = crate::features::scanner::LineIndex::new(&text).range(
                    &text,
                    ty.name_range.0,
                    ty.name_range.1,
                );
                location = json!({ "uri": location_uri.take().unwrap_or(uri.clone()), "range": range });
                break;
            }
            for member in &ty.members {
                if let crate::features::java_model::Member::Type(child) = member {
                    queue.push_back(child);
                }
            }
        }
        // `SymbolInformation.kind` is a `SymbolKind` read by a plain Gson,
        // which only knows the enum constant names.
        let Some(kind) = request["kind"].as_str().and_then(symbol_kind) else {
            return Err(internal_error("Property must not be null: kind".to_owned()));
        };
        let mut result = json!({ "name": name, "kind": kind, "location": location });
        if let Some(container) = request.get("containerName").filter(|c| !c.is_null()) {
            result["containerName"] = container.clone();
        }
        Ok(Some(result))
    }
}

/// `org.eclipse.lsp4j.SymbolKind` by constant name.
fn symbol_kind(name: &str) -> Option<u32> {
    const KINDS: [&str; 26] = [
        "File", "Module", "Namespace", "Package", "Class", "Method", "Property", "Field",
        "Constructor", "Enum", "Interface", "Function", "Variable", "Constant", "String",
        "Number", "Boolean", "Array", "Object", "Key", "Null", "EnumMember", "Struct", "Event",
        "Operator", "TypeParameter",
    ];
    KINDS.iter().position(|k| *k == name).map(|i| i as u32 + 1)
}
