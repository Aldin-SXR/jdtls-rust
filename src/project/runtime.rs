//! Rust policy for `RuntimeEnvironment` and `JVMConfigurator`. VM installation
//! metadata is data; selecting installations, environments and defaults is Rust.
use super::{normalize_java_version, ClasspathEntry, EntryKind, Project, ProjectKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SETTING: &str = "java.configuration.runtimes";
pub const STANDARD_VM_TYPE: &str = "org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType";

pub enum ActionableNotification {}
impl tower_lsp::lsp_types::notification::Notification for ActionableNotification {
    type Params = serde_json::Value;
    const METHOD: &'static str = "language/actionableNotification";
}

pub fn notice(message: &str, actionable: bool) -> serde_json::Value {
    if actionable {
        serde_json::json!({"method":"language/actionableNotification","params":{
            "severity":1,"message":message,"commands":[{"title":"Open Settings","command":"java.runtimeValidation.open"}]
        }})
    } else {
        serde_json::json!({"method":"window/showMessage","params":{"type":1,"message":message}})
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeEnvironment {
    pub name: Option<String>,
    pub path: Option<String>,
    pub javadoc: Option<String>,
    pub sources: Option<String>,
    #[serde(rename = "default")]
    pub is_default: bool,
}
impl RuntimeEnvironment {
    pub fn is_valid(&self) -> bool {
        self.name.as_ref().is_some_and(|s| !s.is_empty())
            && self.path.as_ref().is_some_and(|s| !s.is_empty())
    }
    pub fn installation_file(&self) -> Option<PathBuf> {
        self.is_valid()
            .then(|| PathBuf::from(self.path.as_ref().unwrap()))
    }
    pub fn javadoc_url(&self) -> Option<String> {
        let value = self.javadoc.as_deref().filter(|s| !s.is_empty())?;
        if let Ok(url) = url::Url::parse(value) {
            if matches!(
                url.scheme(),
                "file" | "http" | "https" | "ftp" | "jar" | "jrt" | "mailto"
            ) {
                return Some(if url.scheme() == "file" {
                    value.replacen("file:///", "file:/", 1)
                } else {
                    value.to_owned()
                });
            }
        }
        let path = Path::new(value);
        if path.is_absolute() && path.exists() {
            return Some(super::java_file_uri(path, path.is_dir()));
        }
        tracing::info!("Invalid javadoc: {value}");
        None
    }
    pub fn source_path(&self) -> Option<PathBuf> {
        self.sources
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmLibrary {
    pub path: PathBuf,
    pub source: Option<PathBuf>,
    pub javadoc: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmInstall {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub home: PathBuf,
    pub version: Option<String>,
    pub libraries: Vec<VmLibrary>,
}
impl VmInstall {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }
    pub fn major_version(&self) -> Option<String> {
        normalize_java_version(self.version.as_deref()?)
    }
    pub fn from_home(home: &Path, id: String, name: String) -> Self {
        let version = super::prefs::read_properties(&home.join("release")).and_then(|p| {
            p.get("JAVA_VERSION")
                .map(|v| v.trim_matches('"').to_owned())
        });
        let source = [home.join("lib/src.zip"), home.join("src.zip")]
            .into_iter()
            .find(|p| p.is_file());
        let libraries = [
            home.join("lib/jrt-fs.jar"),
            home.join("jre/lib/rt.jar"),
            home.join("lib/rt.jar"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .into_iter()
        .map(|path| VmLibrary {
            path,
            source: source.clone(),
            javadoc: None,
        })
        .collect();
        Self {
            id,
            kind: STANDARD_VM_TYPE.into(),
            name,
            home: home.into(),
            version,
            libraries,
        }
    }
    pub fn classpath_entries(&self) -> Vec<ClasspathEntry> {
        self.libraries
            .iter()
            .map(|lib| {
                let mut entry = ClasspathEntry::new(EntryKind::Library, lib.path.to_string_lossy());
                entry.location = Some(lib.path.clone());
                entry.source_attachment = lib.source.clone();
                if let Some(url) = &lib.javadoc {
                    entry.set_attribute("javadoc_location", url);
                }
                entry
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeRegistry {
    pub installs: Vec<VmInstall>,
    pub default_vm: Option<String>,
    pub environments: BTreeMap<String, String>,
}
#[derive(Debug, Default)]
pub struct ConfigurationResult {
    pub changed: bool,
    pub notices: Vec<String>,
}
impl RuntimeRegistry {
    pub fn apply_to_workspace(&self, workspace: &mut super::Workspace) {
        workspace.runtime_registry = Some(self.clone());
        workspace.vm_version = self.default_install().and_then(VmInstall::major_version);
        for project in &mut workspace.projects {
            project.runtime = self.vm_for_project(project).cloned();
            if let Some(vm) = &project.runtime.clone() {
                for entry in project
                    .classpath
                    .iter_mut()
                    .filter(|e| e.is_jre_container())
                {
                    entry.children = vm.classpath_entries();
                }
                if matches!(project.kind, ProjectKind::Invisible | ProjectKind::Default) {
                    configure_project_preview(project, vm);
                }
            }
            project.derive_views();
        }
        workspace.finish();
    }
    pub fn with_default_home(home: &Path) -> Self {
        let vm = VmInstall::from_home(
            home,
            "running".into(),
            home.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        );
        Self {
            default_vm: Some(vm.key()),
            installs: vec![vm],
            ..Default::default()
        }
    }
    pub fn default_install(&self) -> Option<&VmInstall> {
        self.default_vm
            .as_ref()
            .and_then(|key| self.installs.iter().find(|vm| vm.key() == *key))
    }
    pub fn find_vm(&self, home: Option<&Path>, name: Option<&str>) -> Option<&VmInstall> {
        self.installs
            .iter()
            .find(|vm| name.is_some_and(|n| n == vm.name) || home.is_some_and(|p| p == vm.home))
    }
    fn unique_id(&self) -> String {
        let mut id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        while self
            .installs
            .iter()
            .any(|vm| vm.kind == STANDARD_VM_TYPE && vm.id == id.to_string())
        {
            id += 1;
        }
        id.to_string()
    }
    /// Existing installations (including contributed VM types) are reused by
    /// location. A whitespace-only or nonexistent home doesn't change the VM.
    pub fn configure_default_vm(&mut self, java_home: Option<&str>) -> bool {
        let Some(home) = java_home
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
        else {
            return false;
        };
        if !home.is_dir() || self.default_install().is_some_and(|vm| vm.home == home) {
            return false;
        }
        let key = if let Some(vm) = self.find_vm(Some(&home), None) {
            vm.key()
        } else {
            let vm = VmInstall::from_home(
                &home,
                self.unique_id(),
                home.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
            );
            let key = vm.key();
            self.installs.push(vm);
            key
        };
        self.default_vm = Some(key);
        true
    }
    pub fn configure(
        &mut self,
        runtimes: &[RuntimeEnvironment],
        java_home: Option<&str>,
    ) -> ConfigurationResult {
        let mut result = ConfigurationResult::default();
        let mut default_set = false;
        for runtime in runtimes.iter().filter(|r| r.is_valid()) {
            let home = runtime.installation_file().unwrap();
            let name = runtime.name.as_deref().unwrap();
            let path = runtime.path.as_deref().unwrap();
            if !home.is_dir() {
                result.notices.push(format!("Invalid runtime for {name}: The path points to a missing or inaccessible folder ({path})."));
                continue;
            }
            let existing = self.find_vm(Some(&home), Some(name)).cloned();
            result.changed |= existing
                .as_ref()
                .map_or(true, |vm| vm.name != name || vm.home != home);
            if !valid_installation(&home) {
                result.notices.push(if home.file_name().is_some_and(|n| n == "bin") {
                    format!("Invalid runtime for {name}: 'bin' should be removed from the path ({path}).")
                } else { format!("Invalid runtime for {name}: The path ({path}) does not point to a JDK.") });
                continue;
            }
            let mut vm = VmInstall::from_home(
                &home,
                existing
                    .as_ref()
                    .map(|v| v.id.clone())
                    .unwrap_or_else(|| self.unique_id()),
                name.into(),
            );
            if let Some(old) = &existing {
                vm.kind = old.kind.clone();
                if !old.libraries.is_empty() {
                    vm.libraries = old.libraries.clone();
                }
            }
            let source = runtime.source_path();
            let javadoc = runtime.javadoc_url();
            for lib in &mut vm.libraries {
                let old = lib.clone();
                if source.is_some() {
                    lib.source = source.clone();
                }
                if javadoc.is_some() {
                    lib.javadoc = javadoc.clone();
                }
                result.changed |= *lib != old;
            }
            let key = vm.key();
            if let Some(index) = self.installs.iter().position(|v| v.key() == key) {
                self.installs[index] = vm.clone();
            } else {
                self.installs.push(vm.clone());
            }
            if runtime.is_default {
                default_set = true;
                result.changed |= self.default_vm.as_ref() != Some(&key);
                self.default_vm = Some(key.clone());
            }
            if self.environments.get(name) == Some(&key)
                || environment_version(name).is_some_and(|required| {
                    vm.major_version().is_some_and(|version| {
                        super::compare_java_versions(&version, &required)
                            != std::cmp::Ordering::Less
                    })
                })
            {
                self.environments.insert(name.into(), key);
            } else {
                result.notices.push(format!("Invalid runtime for {name}: Runtime at '{path}' is not compatible with the '{name}' environment."));
            }
        }
        if !default_set {
            result.changed |= self.configure_default_vm(java_home);
        }
        result
    }
    pub fn vm_for_project(&self, project: &Project) -> Option<&VmInstall> {
        let jre = project.classpath.iter().find(|e| e.is_jre_container())?;
        let environment = jre
            .path
            .rsplit_once(&format!("{STANDARD_VM_TYPE}/"))
            .map(|(_, name)| name)
            .unwrap_or(&jre.path);
        if let Some(ee) = environment_version(environment) {
            if let Some(key) = self.environments.get(environment) {
                if let Some(vm) = self.installs.iter().find(|vm| vm.key() == *key) {
                    return Some(vm);
                }
            }
            if let Some(vm) = self
                .installs
                .iter()
                .find(|vm| vm.major_version().as_deref() == Some(&ee))
            {
                return Some(vm);
            }
        } else if let Some((kind, name)) = jre
            .path
            .strip_prefix(super::JRE_CONTAINER)
            .and_then(|p| p.strip_prefix('/'))
            .and_then(|p| p.split_once('/'))
        {
            return self
                .installs
                .iter()
                .find(|vm| vm.kind == kind && vm.name == name);
        }
        self.default_install()
    }
}

pub fn valid_installation(home: &Path) -> bool {
    (home.join("bin/java").is_file() || home.join("bin/java.exe").is_file())
        && ["lib/jrt-fs.jar", "lib/rt.jar", "jre/lib/rt.jar"]
            .iter()
            .any(|p| home.join(p).is_file())
}
pub fn environment_supported(name: &str) -> bool {
    environment_version(name).is_some()
}
pub fn environment_version(name: &str) -> Option<String> {
    match name {
        "JRE-1.1" | "OSGi/Minimum-1.0" | "OSGi/Minimum-1.1" => Some("1.1".into()),
        "OSGi/Minimum-1.2" => Some("1.2".into()),
        "CDC-1.0/Foundation-1.0" => Some("1.3".into()),
        "CDC-1.1/Foundation-1.1" => Some("1.4".into()),
        _ => {
            let (prefix, version) = name.split_once('-')?;
            let normalized = normalize_java_version(version)?;
            if version != normalized {
                return None;
            }
            let supported = match prefix {
                "J2SE" => ["1.2", "1.3", "1.4", "1.5"].contains(&version),
                "JavaSE" => {
                    super::compare_java_versions(version, "1.6") != std::cmp::Ordering::Less
                        && super::compare_java_versions(version, "26")
                            != std::cmp::Ordering::Greater
                }
                _ => false,
            };
            supported.then_some(normalized)
        }
    }
}
pub fn configure_project_preview(project: &mut Project, vm: &VmInstall) {
    if project.kind == ProjectKind::Invisible && project.root.join(".settings").exists() {
        return;
    }
    let latest = vm.major_version().as_deref() == Some("26");
    project.options.insert(
        super::ENABLE_PREVIEW.into(),
        if latest { "enabled" } else { "disabled" }.into(),
    );
    if latest {
        project
            .options
            .insert(super::REPORT_PREVIEW.into(), "ignore".into());
    }
}

pub fn parse_runtimes(settings: &serde_json::Value) -> Option<Vec<RuntimeEnvironment>> {
    let value = setting_value(settings, SETTING)?;
    let Some(entries) = value.as_array() else {
        return Some(Vec::new());
    };
    let mut defaults_seen = false;
    let mut runtimes: Vec<RuntimeEnvironment> = Vec::new();
    for value in entries {
        let Some(map) = value.as_object() else {
            continue;
        };
        let mut runtime = RuntimeEnvironment::default();
        for (key, value) in map {
            match key.as_str() {
                "name" => runtime.name = value.as_str().map(str::to_owned),
                "path" => runtime.path = value.as_str().map(super::invisible::expand_path),
                "javadoc" => runtime.javadoc = value.as_str().map(super::invisible::expand_path),
                "sources" => runtime.sources = value.as_str().map(super::invisible::expand_path),
                "default" if !defaults_seen => {
                    runtime.is_default = value.as_bool().unwrap_or(false);
                    defaults_seen = true;
                }
                _ => {}
            }
        }
        if runtime.is_valid() && !runtimes.iter().any(|r| r.name == runtime.name) {
            runtimes.push(runtime);
        }
    }
    Some(runtimes)
}

/// Unlike optional scalar preferences, an explicit null runtime list clears
/// the list. Preserve key presence before applying the preference's default.
pub fn setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    settings
        .get(key)
        .or_else(|| key.split('.').try_fold(settings, |v, k| v.get(k)))
}
