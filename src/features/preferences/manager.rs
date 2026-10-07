//! Port of jdt.ls `PreferenceManager` / `StandardPreferenceManager`: holds
//! the current `Preferences`, notifies preference-change listeners, and
//! pushes the derived state the rest of the server reads: the code
//! templates (`java.templates.*`), the formatter tab options of the
//! JavaCore options, and the Maven (m2e) configuration, preferences and
//! settings profile.

use super::model::Preferences;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const JAVALS_PROFILE: &str = "javals.profile";
pub const M2E_DISABLE_TEST_CLASSPATH_FLAG: &str = "m2e.disableTestClasspathFlag";
pub const MAVEN_MULTI_MODULE_PROJECT_DIRECTORY: &str = "maven.multiModuleProjectDirectory";
/// `MavenPreferenceConstants.P_OFFLINE`.
pub const P_OFFLINE: &str = "eclipse.m2.offline";
/// `MavenPreferenceConstants.P_WORKSPACE_MAPPINGS_LOCATION`.
#[allow(dead_code)]
pub const P_WORKSPACE_MAPPINGS_LOCATION: &str = "eclipse.m2.WorkspacelifecycleMappingsLocation";

/// `CodeTemplateContextType.NEWTYPE_ID` / `CodeTemplatePreferences.CODETEMPLATE_NEWTYPE`.
pub const CODETEMPLATE_NEWTYPE: &str = "org.eclipse.jdt.ui.text.codetemplates.newtype";
/// `CodeTemplateContextType.FILECOMMENT_ID` / `CODETEMPLATE_FILECOMMENT`.
pub const CODETEMPLATE_FILECOMMENT: &str = "org.eclipse.jdt.ui.text.codetemplates.filecomment";
/// `CodeTemplateContextType.TYPECOMMENT_ID` / `CODETEMPLATE_TYPECOMMENT`.
pub const CODETEMPLATE_TYPECOMMENT: &str = "org.eclipse.jdt.ui.text.codetemplates.typecomment";
pub const CODETEMPLATE_NEWTYPE_DEFAULT: &str = "${filecomment}${package_declaration}\n\n${typecomment}\n${type_declaration}";
pub const CODETEMPLATE_TYPECOMMENT_DEFAULT: &str = "/**\n * ${type_name}\n * ${tags}\n */";

/// The `CodeGenerationTemplate`s whose content jdt.ls preferences decide,
/// with their default patterns.
const CODE_GENERATION_TEMPLATES: &[(&str, &str)] = &[
    (CODETEMPLATE_TYPECOMMENT, CODETEMPLATE_TYPECOMMENT_DEFAULT),
    (CODETEMPLATE_NEWTYPE, CODETEMPLATE_NEWTYPE_DEFAULT),
    (CODETEMPLATE_FILECOMMENT, ""),
];

/// m2e's `IMavenConfiguration`, the part `StandardPreferenceManager`
/// updates.
pub trait MavenConfiguration: Send + Sync {
    fn get_user_settings_file(&self) -> Option<String>;
    fn set_user_settings_file(&mut self, file: Option<String>) -> Result<(), String>;
    fn get_global_settings_file(&self) -> Option<String>;
    fn set_global_settings_file(&mut self, file: Option<String>) -> Result<(), String>;
    fn get_workspace_lifecycle_mapping_metadata_file(&self) -> Option<String>;
    fn set_workspace_lifecycle_mapping_metadata_file(&mut self, file: Option<String>) -> Result<(), String>;
    fn get_not_covered_mojo_execution_severity(&self) -> Option<String>;
    fn set_not_covered_mojo_execution_severity(&mut self, severity: Option<String>) -> Result<(), String>;
    /// `getDefaultMojoExecutionAction().name()`.
    fn get_default_mojo_execution_action(&self) -> Option<String>;
    fn set_default_mojo_execution_action(&mut self, action: String);
}

/// The workspace's m2e configuration (`MavenPlugin.getMavenConfiguration()`).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceMavenConfiguration {
    user_settings_file: Option<String>,
    global_settings_file: Option<String>,
    workspace_lifecycle_mapping_metadata_file: Option<String>,
    not_covered_mojo_execution_severity: Option<String>,
    default_mojo_execution_action: Option<String>,
}

impl WorkspaceMavenConfiguration {
    pub fn new() -> Self {
        Self {
            not_covered_mojo_execution_severity: Some("ignore".to_owned()),
            default_mojo_execution_action: Some("ignore".to_owned()),
            ..Default::default()
        }
    }
}

impl MavenConfiguration for WorkspaceMavenConfiguration {
    fn get_user_settings_file(&self) -> Option<String> {
        self.user_settings_file.clone()
    }
    fn set_user_settings_file(&mut self, file: Option<String>) -> Result<(), String> {
        self.user_settings_file = file;
        Ok(())
    }
    fn get_global_settings_file(&self) -> Option<String> {
        self.global_settings_file.clone()
    }
    fn set_global_settings_file(&mut self, file: Option<String>) -> Result<(), String> {
        self.global_settings_file = file;
        Ok(())
    }
    fn get_workspace_lifecycle_mapping_metadata_file(&self) -> Option<String> {
        self.workspace_lifecycle_mapping_metadata_file.clone()
    }
    fn set_workspace_lifecycle_mapping_metadata_file(&mut self, file: Option<String>) -> Result<(), String> {
        self.workspace_lifecycle_mapping_metadata_file = file;
        Ok(())
    }
    fn get_not_covered_mojo_execution_severity(&self) -> Option<String> {
        self.not_covered_mojo_execution_severity.clone()
    }
    fn set_not_covered_mojo_execution_severity(&mut self, severity: Option<String>) -> Result<(), String> {
        self.not_covered_mojo_execution_severity = severity;
        Ok(())
    }
    fn get_default_mojo_execution_action(&self) -> Option<String> {
        self.default_mojo_execution_action.clone()
    }
    fn set_default_mojo_execution_action(&mut self, action: String) {
        self.default_mojo_execution_action = Some(action);
    }
}

/// A Maven settings profile (`org.apache.maven.settings.Profile`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MavenProfile {
    pub id: String,
    pub active_by_default: bool,
    pub properties: BTreeMap<String, String>,
}

/// The effective Maven settings (`MavenPlugin.getMaven().getSettings()`):
/// only the profiles matter here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MavenSettings {
    pub profiles: Vec<MavenProfile>,
    pub active_profiles: Vec<String>,
}

#[allow(dead_code)]
impl MavenSettings {
    /// A property of the first active profile that defines it.
    pub fn active_property(&self, key: &str) -> Option<&str> {
        self.profiles
            .iter()
            .filter(|p| p.active_by_default || self.active_profiles.contains(&p.id))
            .find_map(|p| p.properties.get(key).map(String::as_str))
    }
}

/// A code template (`org.eclipse.jface.text.templates.Template`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub pattern: String,
}

/// `JavaManipulation.getCodeTemplateStore()`.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct CodeTemplateStore {
    templates: BTreeMap<String, Template>,
}

#[allow(dead_code)]
impl CodeTemplateStore {
    pub fn find_template_by_id(&self, id: &str) -> Option<&Template> {
        self.templates.get(id)
    }
}

/// `IPreferencesChangeListener`.
pub type PreferencesChangeListener = Arc<dyn Fn(&Preferences, &Preferences) + Send + Sync>;

/// `StandardPreferenceManager`.
pub struct PreferenceManager {
    preferences: Preferences,
    listeners: Vec<PreferencesChangeListener>,
    maven_config: Box<dyn MavenConfiguration>,
    /// `PreferenceManager.templates` (template id → template).
    templates: BTreeMap<String, Template>,
    template_store: CodeTemplateStore,
    /// `JavaCore.getOptions()`.
    java_core_options: BTreeMap<String, String>,
    /// `InstanceScope` node `org.eclipse.jdt.ls.core`.
    instance_prefs: BTreeMap<String, String>,
    /// `InstanceScope` node `org.eclipse.m2e.core`.
    m2e_instance_prefs: BTreeMap<String, String>,
    /// `DefaultScope` node `org.eclipse.m2e.core`.
    m2e_default_prefs: BTreeMap<String, String>,
    maven_settings: MavenSettings,
}

impl Default for PreferenceManager {
    fn default() -> Self {
        Self::new()
    }
}

// The full manager API is ported; the server only drives part of it so far.
#[allow(dead_code)]
impl PreferenceManager {
    /// `new StandardPreferenceManager()`.
    pub fn new() -> Self {
        let mut manager = Self {
            preferences: Preferences::new(),
            listeners: Vec::new(),
            maven_config: Box::new(WorkspaceMavenConfiguration::new()),
            templates: BTreeMap::new(),
            template_store: CodeTemplateStore::default(),
            java_core_options: BTreeMap::new(),
            instance_prefs: BTreeMap::new(),
            m2e_instance_prefs: BTreeMap::new(),
            m2e_default_prefs: BTreeMap::new(),
            maven_settings: MavenSettings::default(),
        };
        manager.initialize();
        manager
    }

    /// `setMavenConfiguration` (public for testing purposes).
    pub fn set_maven_configuration(&mut self, config: Box<dyn MavenConfiguration>) {
        self.maven_config = config;
    }

    /// `PreferenceManager.initialize()`: the JavaCore options jdt.ls
    /// installs and the default code templates.
    pub fn initialize(&mut self) {
        self.initialize_java_core_options();
        self.templates = CODE_GENERATION_TEMPLATES
            .iter()
            .map(|(id, pattern)| (id.to_string(), Template { id: id.to_string(), pattern: pattern.to_string() }))
            .collect();
        self.reload_template_store();
    }

    /// `initializeJavaCoreOptions()`.
    fn initialize_java_core_options(&mut self) {
        let mut options: BTreeMap<String, String> = crate::project::jdt_defaults::WORKSPACE_DEFAULTS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        options.extend(crate::features::formatting::options::eclipse_defaults());
        options.extend(crate::features::formatting::options::jdtls_default_formatter_settings());
        options.extend(crate::project::jdtls_default_options());
        self.java_core_options = options;
    }

    fn reload_template_store(&mut self) {
        self.template_store = CodeTemplateStore { templates: self.templates.clone() };
    }

    /// `updateTemplate(templateId, content)`.
    fn update_template(&mut self, template_id: &str, content: &str) -> bool {
        let template = self.templates.get(template_id);
        if (content.is_empty() && template.is_none()) || template.is_some_and(|t| t.pattern == content) {
            return false;
        }
        if !CODE_GENERATION_TEMPLATES.iter().any(|(id, _)| *id == template_id) {
            return false;
        }
        self.templates
            .insert(template_id.to_owned(), Template { id: template_id.to_owned(), pattern: content.to_owned() });
        true
    }

    /// `StandardPreferenceManager.update(preferences)`.
    pub fn update(&mut self, mut preferences: Preferences) {
        // PreferenceManager.update
        let old_preferences = std::mem::replace(&mut self.preferences, preferences.clone());
        for listener in &self.listeners {
            listener(&old_preferences, &preferences);
        }
        let mut template_changed = false;
        let content = preferences.get_file_header_template().map(|l| l.join("\n")).unwrap_or_default();
        template_changed |= self.update_template(CODETEMPLATE_FILECOMMENT, &content);
        let content = preferences.get_type_comment_template().map(|l| l.join("\n")).unwrap_or_default();
        template_changed |= self.update_template(CODETEMPLATE_TYPECOMMENT, &content);
        if template_changed {
            self.reload_template_store();
        }
        preferences.update_tab_size_insert_spaces(&mut self.java_core_options);

        // StandardPreferenceManager.update
        let new_maven_settings = preferences.get_maven_user_settings().map(str::to_owned);
        let old_maven_settings = self.maven_config.get_user_settings_file();
        if new_maven_settings != old_maven_settings
            && self.maven_config.set_user_settings_file(new_maven_settings).is_err()
        {
            preferences.set_maven_user_settings(old_maven_settings);
        }
        let new_global_settings = preferences.get_maven_global_settings().map(str::to_owned);
        let old_global_settings = self.maven_config.get_global_settings_file();
        if new_global_settings != old_global_settings
            && self.maven_config.set_global_settings_file(new_global_settings).is_err()
        {
            preferences.set_maven_global_settings(old_global_settings);
        }
        let new_lifecycle_mappings = Some(preferences.get_maven_lifecycle_mappings().to_owned());
        let old_lifecycle_mappings = self.maven_config.get_workspace_lifecycle_mapping_metadata_file();
        if new_lifecycle_mappings != old_lifecycle_mappings
            && self.maven_config.set_workspace_lifecycle_mapping_metadata_file(new_lifecycle_mappings).is_err()
        {
            preferences.set_maven_lifecycle_mappings(old_lifecycle_mappings);
        }

        let old_disable_test = self.instance_prefs.get(M2E_DISABLE_TEST_CLASSPATH_FLAG).is_some_and(|v| v == "true");
        let disable_test = preferences.is_maven_disable_test_classpath_flag();
        let multi_module_project_directory = preferences
            .get_root_paths()
            .unwrap_or_default()
            .iter()
            .find_map(|path| compute_multi_module_project_directory(path))
            .map(|f| std::fs::canonicalize(&f).unwrap_or(f).to_string_lossy().into_owned());
        self.maven_settings.profiles.retain(|p| p.id != JAVALS_PROFILE);
        if disable_test || multi_module_project_directory.is_some() {
            let mut profile =
                MavenProfile { id: JAVALS_PROFILE.to_owned(), active_by_default: true, properties: BTreeMap::new() };
            profile.properties.insert(M2E_DISABLE_TEST_CLASSPATH_FLAG.to_owned(), disable_test.to_string());
            if let Some(dir) = &multi_module_project_directory {
                profile.properties.insert(MAVEN_MULTI_MODULE_PROJECT_DIRECTORY.to_owned(), dir.clone());
            }
            self.maven_settings.profiles.push(profile);
            self.maven_settings.active_profiles.push(JAVALS_PROFILE.to_owned());
            self.instance_prefs.insert(M2E_DISABLE_TEST_CLASSPATH_FLAG.to_owned(), disable_test.to_string());
            match multi_module_project_directory {
                Some(dir) => self.instance_prefs.insert(MAVEN_MULTI_MODULE_PROJECT_DIRECTORY.to_owned(), dir),
                None => self.instance_prefs.remove(MAVEN_MULTI_MODULE_PROJECT_DIRECTORY),
            };
        } else if old_disable_test != disable_test {
            self.instance_prefs.insert(M2E_DISABLE_TEST_CLASSPATH_FLAG.to_owned(), disable_test.to_string());
        }

        let new_severity = Some(preferences.get_maven_not_covered_plugin_execution_severity().to_owned());
        if new_severity != self.maven_config.get_not_covered_mojo_execution_severity() {
            let _ = self.maven_config.set_not_covered_mojo_execution_severity(new_severity);
        }
        let new_action = preferences.get_maven_default_mojo_execution_action().to_owned();
        if Some(&new_action) != self.maven_config.get_default_mojo_execution_action().as_ref() {
            self.maven_config.set_default_mojo_execution_action(new_action);
        }
        self.m2e_default_prefs.insert(P_OFFLINE.to_owned(), preferences.is_maven_offline().to_string());
        self.preferences = preferences;
    }

    /// The workspace-wide preferences.
    pub fn get_preferences(&self) -> &Preferences {
        &self.preferences
    }

    pub fn add_preferences_change_listener(&mut self, listener: PreferencesChangeListener) {
        if !self.listeners.iter().any(|l| Arc::ptr_eq(l, &listener)) {
            self.listeners.push(listener);
        }
    }

    pub fn remove_preferences_change_listener(&mut self, listener: &PreferencesChangeListener) {
        self.listeners.retain(|l| !Arc::ptr_eq(l, listener));
    }

    /// `JavaManipulation.getCodeTemplateStore()`.
    pub fn code_template_store(&self) -> &CodeTemplateStore {
        &self.template_store
    }

    /// `JavaCore.getOptions()`.
    pub fn java_core_options(&self) -> &BTreeMap<String, String> {
        &self.java_core_options
    }

    /// `InstanceScope.INSTANCE.getNode(IConstants.PLUGIN_ID)`.
    pub fn instance_prefs(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.instance_prefs
    }

    /// `InstanceScope.INSTANCE.getNode(IMavenConstants.PLUGIN_ID)`.
    pub fn m2e_instance_prefs(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.m2e_instance_prefs
    }

    /// `DefaultScope.INSTANCE.getNode(IMavenConstants.PLUGIN_ID)`.
    pub fn m2e_default_prefs(&self) -> &BTreeMap<String, String> {
        &self.m2e_default_prefs
    }

    /// `MavenPlugin.getMaven().getSettings()`.
    pub fn maven_settings(&self) -> &MavenSettings {
        &self.maven_settings
    }
}

/// m2e `MavenProperties.computeMultiModuleProjectDirectory(File)`: the
/// nearest directory, up to (excluding) the workspace root, that holds a
/// `.mvn` directory.
pub fn compute_multi_module_project_directory(file: &Path) -> Option<PathBuf> {
    let basedir = if file.is_dir() { file.to_path_buf() } else { file.parent()?.to_path_buf() };
    let workspace_root = crate::server::data_dir();
    let mut current = Some(basedir.as_path());
    while let Some(dir) = current {
        if dir == workspace_root {
            break;
        }
        if dir.join(".mvn").is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Port of `org.eclipse.jdt.ls.core.internal.preferences.PreferenceManagerTest`.
///
/// The Mockito `IMavenConfiguration` mock becomes [`MockMavenConfiguration`]:
/// unstubbed getters return `null`, every setter call is recorded, and
/// `reset` clears both.
#[cfg(test)]
mod preference_manager_test {
    use super::super::model::{ReferencedLibraries, LIFECYCLE_MAPPING_METADATA_SOURCE_NAME, MAVEN_GLOBAL_SETTINGS_KEY,
        MAVEN_LIFECYCLE_MAPPINGS_KEY, MAVEN_USER_SETTINGS_KEY};
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockState {
        stubs: BTreeMap<&'static str, Option<String>>,
        calls: Vec<(&'static str, Option<String>)>,
    }

    #[derive(Clone, Default)]
    struct MockMavenConfiguration(Arc<Mutex<MockState>>);

    impl MockMavenConfiguration {
        fn when(&self, getter: &'static str, value: Option<&str>) {
            self.0.lock().unwrap().stubs.insert(getter, value.map(str::to_owned));
        }
        fn reset(&self) {
            *self.0.lock().unwrap() = MockState::default();
        }
        fn get(&self, getter: &'static str) -> Option<String> {
            self.0.lock().unwrap().stubs.get(getter).cloned().flatten()
        }
        fn record(&self, setter: &'static str, value: Option<String>) {
            self.0.lock().unwrap().calls.push((setter, value));
        }
        /// `verify(mavenConfig).setter(value)`.
        fn verify(&self, setter: &'static str, value: Option<&str>) {
            let calls = &self.0.lock().unwrap().calls;
            let n = calls.iter().filter(|(s, v)| *s == setter && v.as_deref() == value).count();
            assert_eq!(1, n, "expected one call {setter}({value:?}), got {calls:?}");
        }
        /// `verify(mavenConfig, never()).setter(anyString())`.
        fn verify_never_any_string(&self, setter: &'static str) {
            let calls = &self.0.lock().unwrap().calls;
            assert!(!calls.iter().any(|(s, v)| *s == setter && v.is_some()), "unexpected {setter} in {calls:?}");
        }
    }

    impl MavenConfiguration for MockMavenConfiguration {
        fn get_user_settings_file(&self) -> Option<String> {
            self.get("getUserSettingsFile")
        }
        fn set_user_settings_file(&mut self, file: Option<String>) -> Result<(), String> {
            self.record("setUserSettingsFile", file);
            Ok(())
        }
        fn get_global_settings_file(&self) -> Option<String> {
            self.get("getGlobalSettingsFile")
        }
        fn set_global_settings_file(&mut self, file: Option<String>) -> Result<(), String> {
            self.record("setGlobalSettingsFile", file);
            Ok(())
        }
        fn get_workspace_lifecycle_mapping_metadata_file(&self) -> Option<String> {
            self.get("getWorkspaceLifecycleMappingMetadataFile")
        }
        fn set_workspace_lifecycle_mapping_metadata_file(&mut self, file: Option<String>) -> Result<(), String> {
            self.record("setWorkspaceLifecycleMappingMetadataFile", file);
            Ok(())
        }
        fn get_not_covered_mojo_execution_severity(&self) -> Option<String> {
            self.get("getNotCoveredMojoExecutionSeverity")
        }
        fn set_not_covered_mojo_execution_severity(&mut self, severity: Option<String>) -> Result<(), String> {
            self.record("setNotCoveredMojoExecutionSeverity", severity);
            Ok(())
        }
        fn get_default_mojo_execution_action(&self) -> Option<String> {
            self.get("getDefaultMojoExecutionAction")
        }
        fn set_default_mojo_execution_action(&mut self, action: String) {
            self.record("setDefaultMojoExecutionAction", Some(action));
        }
    }

    /// `setUp()`.
    fn set_up() -> (PreferenceManager, MockMavenConfiguration) {
        let maven_config = MockMavenConfiguration::default();
        let mut preference_manager = PreferenceManager::new();
        preference_manager.set_maven_configuration(Box::new(maven_config.clone()));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        (preference_manager, maven_config)
    }

    #[test]
    fn test_update_maven_settings() {
        let (mut preference_manager, maven_config) = set_up();
        let path = "/foo/bar.xml";
        let mut preferences = Preferences::create_from(&json!({ MAVEN_USER_SETTINGS_KEY: path }));
        preference_manager.update(preferences.clone());
        maven_config.verify("setUserSettingsFile", Some(path));

        //check setting the same path doesn't call Maven's config update
        maven_config.reset();
        maven_config.when("getUserSettingsFile", Some(path));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        maven_config.when("getDefaultMojoExecutionAction", Some("ignore"));
        preference_manager.update(preferences.clone());
        maven_config.verify_never_any_string("setUserSettingsFile");

        //check setting null is allowed
        maven_config.reset();
        maven_config.when("getUserSettingsFile", Some(path));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        preferences.set_maven_user_settings(None);
        preference_manager.update(preferences.clone());
        maven_config.verify("setUserSettingsFile", None);
    }

    #[test]
    fn test_update_maven_global_settings() {
        let (mut preference_manager, maven_config) = set_up();
        let path = "/foo/bar.xml";
        let mut preferences = Preferences::create_from(&json!({ MAVEN_GLOBAL_SETTINGS_KEY: path }));
        preference_manager.update(preferences.clone());
        maven_config.verify("setGlobalSettingsFile", Some(path));

        //check setting the same path doesn't call Maven's config update
        maven_config.reset();
        maven_config.when("getGlobalSettingsFile", Some(path));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        preference_manager.update(preferences.clone());
        maven_config.verify_never_any_string("setGlobalSettingsFile");

        //check setting null is allowed
        maven_config.reset();
        maven_config.when("getGlobalSettingsFile", Some(path));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        preferences.set_maven_global_settings(None);
        preference_manager.update(preferences.clone());
        maven_config.verify("setGlobalSettingsFile", None);
    }

    #[test]
    fn test_update_maven_lifecycle_mappings() {
        let (mut preference_manager, maven_config) = set_up();
        let path = "/foo/bar.xml";
        let mut preferences = Preferences::create_from(&json!({ MAVEN_LIFECYCLE_MAPPINGS_KEY: path }));
        preference_manager.update(preferences.clone());
        maven_config.verify("setWorkspaceLifecycleMappingMetadataFile", Some(path));

        //check setting the same path doesn't call Maven's config update
        maven_config.reset();
        maven_config.when("getWorkspaceLifecycleMappingMetadataFile", Some(path));
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        preference_manager.update(preferences.clone());
        maven_config.verify_never_any_string("setGlobalSettingsFile");

        //check setting null is allowed
        maven_config.reset();
        maven_config.when("getWorkspaceLifecycleMappingMetadataFile", Some(path));
        let old_mappings = preference_manager.m2e_instance_prefs().get(P_WORKSPACE_MAPPINGS_LOCATION).cloned();
        preference_manager.m2e_instance_prefs().insert(P_WORKSPACE_MAPPINGS_LOCATION.to_owned(), path.to_owned());
        maven_config.when("getNotCoveredMojoExecutionSeverity", Some("ignore"));
        let default_path = super::super::model::m2e_state_location()
            .join(LIFECYCLE_MAPPING_METADATA_SOURCE_NAME)
            .to_string_lossy()
            .into_owned();
        preferences.set_maven_lifecycle_mappings(None);
        preference_manager.update(preferences.clone());
        maven_config.verify("setWorkspaceLifecycleMappingMetadataFile", Some(&default_path));
        match old_mappings {
            None => preference_manager.m2e_instance_prefs().remove(P_WORKSPACE_MAPPINGS_LOCATION),
            Some(old) => preference_manager.m2e_instance_prefs().insert(P_WORKSPACE_MAPPINGS_LOCATION.to_owned(), old),
        };
    }

    #[test]
    fn test_initialize() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let options = preference_manager.java_core_options();
        assert_eq!(Some("enabled"), options.get("org.eclipse.jdt.core.codeComplete.visibilityCheck").map(String::as_str));
        assert_eq!(
            Some("ignore"),
            options.get("org.eclipse.jdt.core.compiler.problem.unhandledWarningToken").map(String::as_str)
        );
    }

    #[test]
    fn test_preferences_change_listener() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let called = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&called);
        let listener: PreferencesChangeListener = Arc::new(move |_old, _new| *flag.lock().unwrap() = true);
        preference_manager.add_preferences_change_listener(Arc::clone(&listener));
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(*called.lock().unwrap(), "No one listener has been called");
        preference_manager.remove_preferences_change_listener(&listener);
        *called.lock().unwrap() = false;
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(!*called.lock().unwrap(), "A listener has been called");
    }

    #[test]
    fn test_update_file_header_template() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();

        let template = preference_manager.code_template_store().find_template_by_id(CODETEMPLATE_FILECOMMENT);
        assert!(template.is_some());
        assert_eq!("", template.unwrap().pattern);

        let mut preferences = Preferences::new();
        preferences.set_file_header_template(Some(vec!["/** */".to_owned()]));
        preference_manager.update(preferences);

        let template = preference_manager.code_template_store().find_template_by_id(CODETEMPLATE_FILECOMMENT);
        assert!(template.is_some());
        assert_eq!("/** */", template.unwrap().pattern);
    }

    #[test]
    fn test_update_type_comment_template() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();

        let template = preference_manager.code_template_store().find_template_by_id(CODETEMPLATE_TYPECOMMENT);
        assert!(template.is_some());
        assert_eq!(CODETEMPLATE_TYPECOMMENT_DEFAULT, template.unwrap().pattern);

        let mut preferences = Preferences::new();
        preferences.set_type_comment_template(Some(vec!["/** */".to_owned()]));
        preference_manager.update(preferences);

        let template = preference_manager.code_template_store().find_template_by_id(CODETEMPLATE_TYPECOMMENT);
        assert!(template.is_some());
        assert_eq!("/** */", template.unwrap().pattern);
    }

    #[test]
    fn test_update_new_type_template() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();

        let template = preference_manager.code_template_store().find_template_by_id(CODETEMPLATE_NEWTYPE);
        assert!(template.is_some());
        assert_eq!(CODETEMPLATE_NEWTYPE_DEFAULT, template.unwrap().pattern);
    }

    #[test]
    fn test_insert_spaces_tab_size() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(preference_manager.get_preferences().is_insert_spaces());
        assert_eq!(4, preference_manager.get_preferences().get_tab_size());
        let eclipse_options = preference_manager.java_core_options();
        let tab_size = &eclipse_options[crate::features::formatting::options::FORMATTER_TAB_SIZE];
        assert_eq!(4, tab_size.parse::<i32>().unwrap());
        let insert_spaces = &eclipse_options[crate::features::formatting::options::FORMATTER_TAB_CHAR];
        assert_eq!("space", insert_spaces);
    }

    #[test]
    fn test_maven_offline() {
        let (mut preference_manager, _maven_config) = set_up();
        let store = |m: &PreferenceManager| m.m2e_default_prefs().get(P_OFFLINE).is_some_and(|v| v == "true");

        preference_manager.initialize();
        let mut preferences = Preferences::new();
        preference_manager.update(preferences.clone());
        assert!(!preference_manager.get_preferences().is_maven_offline());
        assert!(!store(&preference_manager));
        preferences.set_maven_offline(true);
        preference_manager.update(preferences);
        assert!(preference_manager.get_preferences().is_maven_offline());
        assert!(store(&preference_manager));

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(!store(&preference_manager));
    }

    fn get_disable_test_flag(m: &PreferenceManager) -> bool {
        m.maven_settings().active_property(M2E_DISABLE_TEST_CLASSPATH_FLAG) == Some("true")
    }

    fn get_multiple_module_project_directory(m: &PreferenceManager) -> Option<String> {
        m.maven_settings().active_property(MAVEN_MULTI_MODULE_PROJECT_DIRECTORY).map(str::to_owned)
    }

    #[test]
    fn test_maven_disable_test_flag() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let mut preferences = Preferences::new();
        preference_manager.update(preferences.clone());
        assert!(!preference_manager.get_preferences().is_maven_disable_test_classpath_flag());
        assert!(!get_disable_test_flag(&preference_manager));
        preferences.set_maven_disable_test_classpath_flag(true);
        preference_manager.update(preferences);
        assert!(preference_manager.get_preferences().is_maven_disable_test_classpath_flag());
        assert!(get_disable_test_flag(&preference_manager));

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(!preference_manager.get_preferences().is_maven_disable_test_classpath_flag());
        assert!(!get_disable_test_flag(&preference_manager));
    }

    #[test]
    fn test_maven_multiple_module_project_directory() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert_eq!(None, get_multiple_module_project_directory(&preference_manager));
        let mut preferences = Preferences::new();
        let dir = std::env::current_dir().unwrap().join("target").join("workingProjects").join("test");
        let dot_mvn = dir.join(".mvn");
        std::fs::create_dir_all(&dot_mvn).unwrap();
        preferences.set_root_paths(Some(vec![dir.clone()]));
        preference_manager.update(preferences);
        assert_eq!(Some(dir.to_string_lossy().into_owned()), get_multiple_module_project_directory(&preference_manager));

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert_eq!(None, get_multiple_module_project_directory(&preference_manager));
    }

    // https://github.com/eclipse-jdtls/eclipse.jdt.ls/issues/3495
    #[test]
    fn test_referenced_libraries_sources_get_expanded() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let mut preferences = Preferences::new();
        preferences.set_referenced_libraries(ReferencedLibraries::new(
            ["~/include/path/lib.jar"],
            [],
            [("~/include/path/lib.jar", "~/include/path/lib-src.jar")],
        ));
        preference_manager.update(preferences);
        let entries = preference_manager.get_preferences().get_referenced_libraries().sources();
        for (key, value) in entries {
            assert!(!key.starts_with('~'));
            assert!(!value.starts_with('~'));
        }

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert_eq!(1, preference_manager.get_preferences().get_referenced_libraries().include().len());
        assert!(preference_manager.get_preferences().get_referenced_libraries().include().contains("lib/**"));
    }

    #[test]
    fn test_maven_default_mojo_execution() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let mut preferences = Preferences::new();
        preference_manager.update(preferences.clone());
        assert_eq!("ignore", preference_manager.get_preferences().get_maven_default_mojo_execution_action());
        preferences.set_maven_default_mojo_execution_action(Some("warn".to_owned()));
        preference_manager.update(preferences.clone());
        assert_eq!("warn", preference_manager.get_preferences().get_maven_default_mojo_execution_action());
        preferences.set_maven_default_mojo_execution_action(Some("unknown".to_owned()));
        preference_manager.update(preferences);
        assert_eq!("ignore", preference_manager.get_preferences().get_maven_default_mojo_execution_action());

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert_eq!("ignore", preference_manager.get_preferences().get_maven_default_mojo_execution_action());
    }

    #[test]
    fn test_telemetry_settings() {
        let (mut preference_manager, _maven_config) = set_up();
        preference_manager.initialize();
        let mut preferences = Preferences::new(); // default is disabled
        preference_manager.update(preferences.clone());
        assert!(!preference_manager.get_preferences().is_telemetry_enabled());
        preferences.set_telemetry_enabled(true);
        preference_manager.update(preferences);
        assert!(preference_manager.get_preferences().is_telemetry_enabled());

        // finally
        let preferences = Preferences::new();
        preference_manager.update(preferences);
        assert!(!preference_manager.get_preferences().is_telemetry_enabled());
    }
}
