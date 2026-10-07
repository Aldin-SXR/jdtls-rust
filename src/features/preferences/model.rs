//! Port of the jdt.ls `Preferences` model: the typed view of the client's
//! `java.*` settings, built by `Preferences.createFrom` and updated by
//! `Preferences.updateFrom`, which only replaces the settings present in a
//! (partial) configuration.

use super::map_flattener::{contains_key, get_boolean, get_int, get_list, get_string, get_value};
use crate::features::formatting::options::expand_path;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const IMPORT_GRADLE_ENABLED: &str = "java.import.gradle.enabled";
pub const IMPORT_MAVEN_ENABLED: &str = "java.import.maven.enabled";
pub const IMPORT_MAVEN_OFFLINE: &str = "java.import.maven.offline.enabled";
pub const MAVEN_DISABLE_TEST_CLASSPATH_FLAG: &str = "java.import.maven.disableTestClasspathFlag";
pub const JAVA_CONFIGURATION_INSERTSPACES: &str = "java.format.insertSpaces";
pub const JAVA_CONFIGURATION_TABSIZE: &str = "java.format.tabSize";
pub const JAVA_FORMAT_ENABLED_KEY: &str = "java.format.enabled";
pub const JAVA_FORMAT_ON_TYPE_ENABLED_KEY: &str = "java.format.onType.enabled";
pub const AUTOBUILD_ENABLED_KEY: &str = "java.autobuild.enabled";
pub const COMPLETION_ENABLED_KEY: &str = "java.completion.enabled";
pub const JAVA_COMPLETION_OVERWRITE_KEY: &str = "java.completion.overwrite";
pub const JAVA_COMPLETION_GUESS_METHOD_ARGUMENTS_KEY: &str = "java.completion.guessMethodArguments";
pub const JAVA_COMPLETION_FAVORITE_MEMBERS_KEY: &str = "java.completion.favoriteStaticMembers";
pub const JAVA_PROJECT_REFERENCED_LIBRARIES_KEY: &str = "java.project.referencedLibraries";
pub const MAVEN_USER_SETTINGS_KEY: &str = "java.configuration.maven.userSettings";
pub const MAVEN_GLOBAL_SETTINGS_KEY: &str = "java.configuration.maven.globalSettings";
pub const MAVEN_LIFECYCLE_MAPPINGS_KEY: &str = "java.configuration.maven.lifecycleMappings";
pub const MAVEN_NOT_COVERED_PLUGIN_EXECUTION_SEVERITY: &str = "java.configuration.maven.notCoveredPluginExecutionSeverity";
pub const MAVEN_DEFAULT_MOJO_EXECUTION_ACTION: &str = "java.configuration.maven.defaultMojoExecutionAction";
pub const JAVA_SETTINGS_URL: &str = "java.settings.url";
pub const IMPORTS_ONDEMANDTHRESHOLD: &str = "java.sources.organizeImports.starThreshold";
pub const IMPORTS_STATIC_ONDEMANDTHRESHOLD: &str = "java.sources.organizeImports.staticStarThreshold";
pub const JAVA_TEMPLATES_FILEHEADER: &str = "java.templates.fileHeader";
pub const JAVA_TEMPLATES_TYPECOMMENT: &str = "java.templates.typeComment";
pub const JAVA_TELEMETRY_ENABLED_KEY: &str = "java.telemetry.enabled";
pub const JAVA_COMPILE_NULLANALYSIS_NONNULL: &str = "java.compile.nullAnalysis.nonnull";
pub const JAVA_COMPILE_NULLANALYSIS_NULLABLE: &str = "java.compile.nullAnalysis.nullable";
pub const JAVA_COMPILE_NULLANALYSIS_NONNULLBYDEFAULT: &str = "java.compile.nullAnalysis.nonnullbydefault";
pub const JAVA_COMPILE_NULLANALYSIS_MODE: &str = "java.compile.nullAnalysis.mode";

pub const IMPORTS_ONDEMANDTHRESHOLD_DEFAULT: i32 = 99;
pub const IMPORTS_STATIC_ONDEMANDTHRESHOLD_DEFAULT: i32 = 99;
pub const LIFECYCLE_MAPPING_METADATA_SOURCE_NAME: &str = "lifecycle-mapping-metadata.xml";
const DEFAULT_TAB_SIZE: i32 = 4;
const IGNORE: &str = "ignore";

pub(crate) use crate::features::completion::prefs::FAVORITES_DEFAULT as JAVA_COMPLETION_FAVORITE_MEMBERS_DEFAULT;

/// `Preferences.FeatureStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStatus {
    Disabled,
    Interactive,
    Automatic,
}

#[allow(dead_code)]
impl FeatureStatus {
    /// `FeatureStatus.fromString`: case-insensitive, else `default`.
    pub fn from_string(value: Option<&str>, default: FeatureStatus) -> FeatureStatus {
        match value.map(str::to_lowercase).as_deref() {
            Some("disabled") => FeatureStatus::Disabled,
            Some("interactive") => FeatureStatus::Interactive,
            Some("automatic") => FeatureStatus::Automatic,
            _ => default,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FeatureStatus::Disabled => "disabled",
            FeatureStatus::Interactive => "interactive",
            FeatureStatus::Automatic => "automatic",
        }
    }
}

/// `CompletionGuessMethodArgumentsMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionGuessMethodArgumentsMode {
    Off,
    InsertParameterNames,
    InsertBestGuessedArguments,
}

impl CompletionGuessMethodArgumentsMode {
    /// `fromString`: the UpperCamel name (`insertParameterNames`) as the
    /// UPPER_UNDERSCORE constant, else `default`.
    pub fn from_string(value: Option<&str>, default: Self) -> Self {
        let Some(value) = value else { return default };
        let mut upper = String::new();
        for (i, c) in value.chars().enumerate() {
            if i > 0 && c.is_uppercase() {
                upper.push('_');
            }
            upper.extend(c.to_uppercase());
        }
        match upper.as_str() {
            "OFF" => Self::Off,
            "INSERT_PARAMETER_NAMES" => Self::InsertParameterNames,
            "INSERT_BEST_GUESSED_ARGUMENTS" => Self::InsertBestGuessedArguments,
            _ => default,
        }
    }
}

/// `Preferences.ReferencedLibraries`: every path added is expanded
/// (`ResourceUtils.expandPath`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReferencedLibraries {
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
    sources: BTreeMap<String, String>,
}

#[allow(dead_code)]
impl ReferencedLibraries {
    pub fn new<'a>(
        include: impl IntoIterator<Item = &'a str>,
        exclude: impl IntoIterator<Item = &'a str>,
        sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        Self {
            include: include.into_iter().map(expand_path).collect(),
            exclude: exclude.into_iter().map(expand_path).collect(),
            sources: sources.into_iter().map(|(k, v)| (expand_path(k), expand_path(v))).collect(),
        }
    }

    /// `JAVA_PROJECT_REFERENCED_LIBRARIES_DEFAULT`.
    pub fn jdtls_default() -> Self {
        Self::new(["lib/**"], [], [])
    }

    pub fn include(&self) -> &BTreeSet<String> {
        &self.include
    }

    pub fn exclude(&self) -> &BTreeSet<String> {
        &self.exclude
    }

    pub fn sources(&self) -> &BTreeMap<String, String> {
        &self.sources
    }
}

/// jdt.ls `Preferences` (the settings this server reads through the typed
/// model). `Clone` is `Preferences.clone()`: a deep copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Preferences {
    configuration: Option<Map<String, Value>>,
    import_gradle_enabled: bool,
    import_maven_enabled: bool,
    maven_offline: bool,
    maven_disable_test_classpath_flag: bool,
    insert_spaces: bool,
    tab_size: i32,
    java_format_enabled: bool,
    java_format_on_type_enabled: bool,
    autobuild_enabled: bool,
    completion_enabled: bool,
    completion_overwrite: bool,
    guess_method_arguments: CompletionGuessMethodArgumentsMode,
    java_completion_favorite_members: Vec<String>,
    referenced_libraries: ReferencedLibraries,
    maven_user_settings: Option<String>,
    maven_global_settings: Option<String>,
    maven_lifecycle_mappings: Option<String>,
    maven_not_covered_plugin_execution_severity: String,
    maven_default_mojo_execution_action: String,
    settings_url: Option<String>,
    import_on_demand_threshold: i32,
    static_import_on_demand_threshold: i32,
    file_header_template: Option<Vec<String>>,
    type_comment_template: Option<Vec<String>>,
    telemetry_enabled: bool,
    root_paths: Option<Vec<PathBuf>>,
    nonnull_types: Vec<String>,
    nullable_types: Vec<String>,
    nonnullbydefault_types: Vec<String>,
    null_analysis_mode: FeatureStatus,
}

impl Default for Preferences {
    fn default() -> Self {
        Self::new()
    }
}

// Every getter/setter of the ported Java API is kept even where no Rust
// caller reads that setting through the typed model yet.
#[allow(dead_code)]
impl Preferences {
    /// `new Preferences()`: the jdt.ls defaults.
    pub fn new() -> Self {
        Self {
            configuration: None,
            import_gradle_enabled: true,
            import_maven_enabled: true,
            maven_offline: false,
            maven_disable_test_classpath_flag: false,
            insert_spaces: true,
            tab_size: DEFAULT_TAB_SIZE,
            java_format_enabled: true,
            java_format_on_type_enabled: false,
            autobuild_enabled: true,
            completion_enabled: true,
            completion_overwrite: true,
            guess_method_arguments: CompletionGuessMethodArgumentsMode::InsertParameterNames,
            java_completion_favorite_members: JAVA_COMPLETION_FAVORITE_MEMBERS_DEFAULT.iter().map(|s| s.to_string()).collect(),
            referenced_libraries: ReferencedLibraries::jdtls_default(),
            maven_user_settings: None,
            maven_global_settings: None,
            maven_lifecycle_mappings: None,
            maven_not_covered_plugin_execution_severity: IGNORE.to_owned(),
            maven_default_mojo_execution_action: IGNORE.to_owned(),
            settings_url: None,
            import_on_demand_threshold: IMPORTS_ONDEMANDTHRESHOLD_DEFAULT,
            static_import_on_demand_threshold: IMPORTS_STATIC_ONDEMANDTHRESHOLD_DEFAULT,
            file_header_template: None,
            type_comment_template: None,
            telemetry_enabled: false,
            root_paths: None,
            nonnull_types: Vec::new(),
            nullable_types: Vec::new(),
            nonnullbydefault_types: Vec::new(),
            null_analysis_mode: FeatureStatus::Disabled,
        }
    }

    /// `Preferences.createFrom(configuration)`.
    pub fn create_from(configuration: &Value) -> Self {
        Self::update_from(&Self::new(), configuration)
    }

    /// `Preferences.updateFrom(existing, configuration)`: a copy of
    /// `existing` with only the settings present in `configuration`
    /// replaced.
    pub fn update_from(existing: &Preferences, configuration: &Value) -> Self {
        let mut prefs = existing.clone();
        let empty = Map::new();
        let entries = configuration.as_object().unwrap_or(&empty);
        if !entries.is_empty() {
            let merged = prefs.configuration.get_or_insert_with(Map::new);
            for (k, v) in entries {
                merged.insert(k.clone(), v.clone());
            }
        }
        let c = configuration;

        if contains_key(c, IMPORT_GRADLE_ENABLED) {
            prefs.set_import_gradle_enabled(get_boolean(c, IMPORT_GRADLE_ENABLED, existing.import_gradle_enabled));
        }
        if contains_key(c, IMPORT_MAVEN_ENABLED) {
            prefs.import_maven_enabled = get_boolean(c, IMPORT_MAVEN_ENABLED, existing.import_maven_enabled);
        }
        if contains_key(c, JAVA_CONFIGURATION_INSERTSPACES) {
            prefs.set_insert_spaces(get_boolean(c, JAVA_CONFIGURATION_INSERTSPACES, existing.insert_spaces));
        }
        if contains_key(c, JAVA_CONFIGURATION_TABSIZE) {
            prefs.set_tab_size(get_int(c, JAVA_CONFIGURATION_TABSIZE, existing.tab_size));
        }
        if contains_key(c, IMPORT_MAVEN_OFFLINE) {
            prefs.set_maven_offline(get_boolean(c, IMPORT_MAVEN_OFFLINE, existing.maven_offline));
        }
        if contains_key(c, MAVEN_DISABLE_TEST_CLASSPATH_FLAG) {
            prefs.set_maven_disable_test_classpath_flag(get_boolean(
                c,
                MAVEN_DISABLE_TEST_CLASSPATH_FLAG,
                existing.maven_disable_test_classpath_flag,
            ));
        }
        if contains_key(c, JAVA_FORMAT_ENABLED_KEY) {
            prefs.set_java_format_enabled(get_boolean(c, JAVA_FORMAT_ENABLED_KEY, existing.java_format_enabled));
        }
        if contains_key(c, JAVA_FORMAT_ON_TYPE_ENABLED_KEY) {
            prefs.java_format_on_type_enabled =
                get_boolean(c, JAVA_FORMAT_ON_TYPE_ENABLED_KEY, existing.java_format_on_type_enabled);
        }
        if contains_key(c, AUTOBUILD_ENABLED_KEY) {
            prefs.set_autobuild_enabled(get_boolean(c, AUTOBUILD_ENABLED_KEY, existing.autobuild_enabled));
        }
        if contains_key(c, COMPLETION_ENABLED_KEY) {
            prefs.set_completion_enabled(get_boolean(c, COMPLETION_ENABLED_KEY, existing.completion_enabled));
        }
        if contains_key(c, JAVA_COMPLETION_OVERWRITE_KEY) {
            prefs.completion_overwrite = get_boolean(c, JAVA_COMPLETION_OVERWRITE_KEY, existing.completion_overwrite);
        }
        if contains_key(c, JAVA_COMPLETION_GUESS_METHOD_ARGUMENTS_KEY) {
            prefs.guess_method_arguments = match get_value(c, JAVA_COMPLETION_GUESS_METHOD_ARGUMENTS_KEY) {
                Some(Value::Bool(true)) => CompletionGuessMethodArgumentsMode::InsertBestGuessedArguments,
                Some(Value::Bool(false)) => CompletionGuessMethodArgumentsMode::InsertParameterNames,
                _ => CompletionGuessMethodArgumentsMode::from_string(
                    get_string(c, JAVA_COMPLETION_GUESS_METHOD_ARGUMENTS_KEY, None).as_deref(),
                    existing.guess_method_arguments,
                ),
            };
        }
        if contains_key(c, JAVA_PROJECT_REFERENCED_LIBRARIES_KEY) {
            let strings = |v: Option<&Value>| -> Option<Vec<String>> {
                match v {
                    None => Some(Vec::new()),
                    Some(Value::Array(a)) => a.iter().map(|s| s.as_str().map(str::to_owned)).collect(),
                    _ => None,
                }
            };
            let libraries = match get_value(c, JAVA_PROJECT_REFERENCED_LIBRARIES_KEY) {
                Some(Value::Object(config)) => (|| {
                    let include = strings(config.get("include"))?;
                    let exclude = strings(config.get("exclude"))?;
                    let sources: Vec<(String, String)> = match config.get("sources") {
                        None => Vec::new(),
                        Some(Value::Object(m)) => {
                            m.iter().map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect::<Option<_>>()?
                        }
                        _ => return None,
                    };
                    Some(ReferencedLibraries::new(
                        include.iter().map(String::as_str),
                        exclude.iter().map(String::as_str),
                        sources.iter().map(|(k, v)| (k.as_str(), v.as_str())),
                    ))
                })(),
                other => strings(other).map(|include| ReferencedLibraries::new(include.iter().map(String::as_str), [], [])),
            };
            prefs.set_referenced_libraries(libraries.unwrap_or_else(|| existing.referenced_libraries.clone()));
        }
        if contains_key(c, JAVA_COMPLETION_FAVORITE_MEMBERS_KEY) {
            let members = get_list(c, JAVA_COMPLETION_FAVORITE_MEMBERS_KEY, Some(existing.java_completion_favorite_members.clone()));
            prefs.set_java_completion_favorite_members(members);
        }
        if contains_key(c, MAVEN_USER_SETTINGS_KEY) {
            prefs.set_maven_user_settings(get_string(c, MAVEN_USER_SETTINGS_KEY, existing.maven_user_settings.as_deref()));
        }
        if contains_key(c, MAVEN_GLOBAL_SETTINGS_KEY) {
            prefs.set_maven_global_settings(get_string(c, MAVEN_GLOBAL_SETTINGS_KEY, existing.maven_global_settings.as_deref()));
        }
        if contains_key(c, MAVEN_LIFECYCLE_MAPPINGS_KEY) {
            prefs.set_maven_lifecycle_mappings(get_string(
                c,
                MAVEN_LIFECYCLE_MAPPINGS_KEY,
                existing.maven_lifecycle_mappings.as_deref(),
            ));
        }
        if contains_key(c, MAVEN_NOT_COVERED_PLUGIN_EXECUTION_SEVERITY) {
            prefs.maven_not_covered_plugin_execution_severity = get_string(
                c,
                MAVEN_NOT_COVERED_PLUGIN_EXECUTION_SEVERITY,
                Some(&existing.maven_not_covered_plugin_execution_severity),
            )
            .unwrap_or_default();
        }
        if contains_key(c, MAVEN_DEFAULT_MOJO_EXECUTION_ACTION) {
            prefs.set_maven_default_mojo_execution_action(get_string(
                c,
                MAVEN_DEFAULT_MOJO_EXECUTION_ACTION,
                Some(&existing.maven_default_mojo_execution_action),
            ));
        }
        if contains_key(c, JAVA_SETTINGS_URL) {
            prefs.set_settings_url(get_string(c, JAVA_SETTINGS_URL, None));
        }
        if contains_key(c, IMPORTS_ONDEMANDTHRESHOLD) {
            prefs.set_import_on_demand_threshold(get_int(c, IMPORTS_ONDEMANDTHRESHOLD, existing.import_on_demand_threshold));
        }
        if contains_key(c, IMPORTS_STATIC_ONDEMANDTHRESHOLD) {
            prefs.set_static_import_on_demand_threshold(get_int(
                c,
                IMPORTS_STATIC_ONDEMANDTHRESHOLD,
                existing.static_import_on_demand_threshold,
            ));
        }
        if contains_key(c, JAVA_TEMPLATES_FILEHEADER) {
            prefs.set_file_header_template(get_list(c, JAVA_TEMPLATES_FILEHEADER, None));
        }
        if contains_key(c, JAVA_TEMPLATES_TYPECOMMENT) {
            prefs.set_type_comment_template(get_list(c, JAVA_TEMPLATES_TYPECOMMENT, None));
        }
        if contains_key(c, JAVA_COMPILE_NULLANALYSIS_NONNULL) {
            prefs.nonnull_types =
                get_list(c, JAVA_COMPILE_NULLANALYSIS_NONNULL, Some(existing.nonnull_types.clone())).unwrap_or_default();
        }
        if contains_key(c, JAVA_COMPILE_NULLANALYSIS_NULLABLE) {
            prefs.nullable_types =
                get_list(c, JAVA_COMPILE_NULLANALYSIS_NULLABLE, Some(existing.nullable_types.clone())).unwrap_or_default();
        }
        if contains_key(c, JAVA_COMPILE_NULLANALYSIS_NONNULLBYDEFAULT) {
            prefs.nonnullbydefault_types =
                get_list(c, JAVA_COMPILE_NULLANALYSIS_NONNULLBYDEFAULT, Some(existing.nonnullbydefault_types.clone()))
                    .unwrap_or_default();
        }
        if contains_key(c, JAVA_COMPILE_NULLANALYSIS_MODE) {
            prefs.null_analysis_mode = FeatureStatus::from_string(
                get_string(c, JAVA_COMPILE_NULLANALYSIS_MODE, None).as_deref(),
                existing.null_analysis_mode,
            );
        }
        if contains_key(c, JAVA_TELEMETRY_ENABLED_KEY) {
            prefs.set_telemetry_enabled(get_boolean(c, JAVA_TELEMETRY_ENABLED_KEY, existing.telemetry_enabled));
        }
        prefs
    }

    /// `Preferences.asMap()`.
    pub fn as_map(&self) -> Option<&Map<String, Value>> {
        self.configuration.as_ref()
    }

    pub fn set_import_gradle_enabled(&mut self, enabled: bool) -> &mut Self {
        self.import_gradle_enabled = enabled;
        self
    }
    pub fn is_import_gradle_enabled(&self) -> bool {
        self.import_gradle_enabled
    }
    pub fn is_import_maven_enabled(&self) -> bool {
        self.import_maven_enabled
    }

    pub fn set_maven_offline(&mut self, offline: bool) -> &mut Self {
        self.maven_offline = offline;
        self
    }
    pub fn is_maven_offline(&self) -> bool {
        self.maven_offline
    }

    pub fn set_maven_disable_test_classpath_flag(&mut self, flag: bool) -> &mut Self {
        self.maven_disable_test_classpath_flag = flag;
        self
    }
    pub fn is_maven_disable_test_classpath_flag(&self) -> bool {
        self.maven_disable_test_classpath_flag
    }

    pub fn set_insert_spaces(&mut self, insert_spaces: bool) -> &mut Self {
        self.insert_spaces = insert_spaces;
        self
    }
    pub fn is_insert_spaces(&self) -> bool {
        self.insert_spaces
    }

    pub fn set_tab_size(&mut self, tab_size: i32) -> &mut Self {
        self.tab_size = tab_size;
        self
    }
    pub fn get_tab_size(&self) -> i32 {
        self.tab_size
    }

    /// `updateTabSizeInsertSpaces(options)`: the formatter tab options.
    pub fn update_tab_size_insert_spaces(&self, options: &mut BTreeMap<String, String>) {
        use crate::features::formatting::options::{FORMATTER_TAB_CHAR, FORMATTER_TAB_SIZE};
        if self.tab_size > 0 {
            options.insert(FORMATTER_TAB_SIZE.to_owned(), self.tab_size.to_string());
        }
        options.insert(FORMATTER_TAB_CHAR.to_owned(), if self.insert_spaces { "space" } else { "tab" }.to_owned());
    }

    pub fn set_java_format_enabled(&mut self, enabled: bool) -> &mut Self {
        self.java_format_enabled = enabled;
        self
    }
    pub fn is_java_format_enabled(&self) -> bool {
        self.java_format_enabled
    }
    pub fn is_java_format_on_type_enabled(&self) -> bool {
        self.java_format_on_type_enabled
    }

    pub fn set_autobuild_enabled(&mut self, enabled: bool) -> &mut Self {
        self.autobuild_enabled = enabled;
        self
    }
    pub fn is_autobuild_enabled(&self) -> bool {
        self.autobuild_enabled
    }

    pub fn set_completion_enabled(&mut self, enabled: bool) -> &mut Self {
        self.completion_enabled = enabled;
        self
    }
    pub fn is_completion_enabled(&self) -> bool {
        self.completion_enabled
    }
    pub fn is_completion_overwrite(&self) -> bool {
        self.completion_overwrite
    }

    pub fn get_guess_method_arguments_mode(&self) -> CompletionGuessMethodArgumentsMode {
        self.guess_method_arguments
    }

    /// `setJavaCompletionFavoriteMembers`: `null` or empty restores the
    /// defaults.
    pub fn set_java_completion_favorite_members(&mut self, members: Option<Vec<String>>) -> &mut Self {
        self.java_completion_favorite_members = match members {
            Some(m) if !m.is_empty() => m,
            _ => JAVA_COMPLETION_FAVORITE_MEMBERS_DEFAULT.iter().map(|s| s.to_string()).collect(),
        };
        self
    }
    pub fn get_java_completion_favorite_members(&self) -> &[String] {
        &self.java_completion_favorite_members
    }

    pub fn set_referenced_libraries(&mut self, libraries: ReferencedLibraries) -> &mut Self {
        self.referenced_libraries = libraries;
        self
    }
    pub fn get_referenced_libraries(&self) -> &ReferencedLibraries {
        &self.referenced_libraries
    }

    pub fn set_maven_user_settings(&mut self, settings: Option<String>) -> &mut Self {
        self.maven_user_settings = settings.map(|s| expand_path(&s));
        self
    }
    pub fn get_maven_user_settings(&self) -> Option<&str> {
        self.maven_user_settings.as_deref()
    }

    pub fn set_maven_global_settings(&mut self, settings: Option<String>) -> &mut Self {
        self.maven_global_settings = settings.map(|s| expand_path(&s));
        self
    }
    pub fn get_maven_global_settings(&self) -> Option<&str> {
        self.maven_global_settings.as_deref()
    }

    /// `setMavenLifecycleMappings`: a blank value selects the m2e workspace
    /// mapping file in the m2e state location.
    pub fn set_maven_lifecycle_mappings(&mut self, mappings: Option<String>) -> &mut Self {
        let mappings = match mappings {
            Some(m) if !m.trim().is_empty() => m,
            _ => default_lifecycle_mappings(),
        };
        self.maven_lifecycle_mappings = Some(expand_path(&mappings));
        self
    }
    /// `getMavenLifecycleMappings`: initialized to the default on first read.
    pub fn get_maven_lifecycle_mappings(&mut self) -> &str {
        if self.maven_lifecycle_mappings.is_none() {
            self.set_maven_lifecycle_mappings(None);
        }
        self.maven_lifecycle_mappings.as_deref().unwrap_or_default()
    }

    pub fn get_maven_not_covered_plugin_execution_severity(&self) -> &str {
        &self.maven_not_covered_plugin_execution_severity
    }

    /// `setMavenDefaultMojoExecutionAction`: anything but `ignore`,
    /// `execute`, `warn` or `error` falls back to `ignore`.
    pub fn set_maven_default_mojo_execution_action(&mut self, action: Option<String>) -> &mut Self {
        self.maven_default_mojo_execution_action = match action.as_deref() {
            Some(a @ ("ignore" | "execute" | "warn" | "error")) => a.to_owned(),
            _ => IGNORE.to_owned(),
        };
        self
    }
    pub fn get_maven_default_mojo_execution_action(&self) -> &str {
        &self.maven_default_mojo_execution_action
    }

    pub fn set_settings_url(&mut self, url: Option<String>) -> &mut Self {
        self.settings_url = url.map(|u| expand_path(&u));
        self
    }
    pub fn get_settings_url(&self) -> Option<&str> {
        self.settings_url.as_deref()
    }

    /// `setImportOnDemandThreshold`: non-positive values reset to the default.
    pub fn set_import_on_demand_threshold(&mut self, threshold: i32) -> &mut Self {
        self.import_on_demand_threshold = if threshold <= 0 { IMPORTS_ONDEMANDTHRESHOLD_DEFAULT } else { threshold };
        self
    }
    pub fn get_import_on_demand_threshold(&self) -> i32 {
        self.import_on_demand_threshold
    }

    /// `setStaticImportOnDemandThreshold`: non-positive values reset to the
    /// default.
    pub fn set_static_import_on_demand_threshold(&mut self, threshold: i32) -> &mut Self {
        self.static_import_on_demand_threshold =
            if threshold <= 0 { IMPORTS_STATIC_ONDEMANDTHRESHOLD_DEFAULT } else { threshold };
        self
    }
    pub fn get_static_import_on_demand_threshold(&self) -> i32 {
        self.static_import_on_demand_threshold
    }

    pub fn set_file_header_template(&mut self, template: Option<Vec<String>>) -> &mut Self {
        self.file_header_template = template;
        self
    }
    pub fn get_file_header_template(&self) -> Option<&[String]> {
        self.file_header_template.as_deref()
    }

    pub fn set_type_comment_template(&mut self, template: Option<Vec<String>>) -> &mut Self {
        self.type_comment_template = template;
        self
    }
    pub fn get_type_comment_template(&self) -> Option<&[String]> {
        self.type_comment_template.as_deref()
    }

    pub fn set_telemetry_enabled(&mut self, enabled: bool) -> &mut Self {
        self.telemetry_enabled = enabled;
        self
    }
    pub fn is_telemetry_enabled(&self) -> bool {
        self.telemetry_enabled
    }

    pub fn set_root_paths(&mut self, root_paths: Option<Vec<PathBuf>>) -> &mut Self {
        self.root_paths = root_paths;
        self
    }
    pub fn get_root_paths(&self) -> Option<&[PathBuf]> {
        self.root_paths.as_deref()
    }

    pub fn set_nonnull_types(&mut self, types: Vec<String>) {
        self.nonnull_types = types;
    }
    pub fn get_nonnull_types(&self) -> &[String] {
        &self.nonnull_types
    }
    pub fn set_nullable_types(&mut self, types: Vec<String>) {
        self.nullable_types = types;
    }
    pub fn get_nullable_types(&self) -> &[String] {
        &self.nullable_types
    }
    pub fn set_nonnullbydefault_types(&mut self, types: Vec<String>) {
        self.nonnullbydefault_types = types;
    }
    pub fn get_nonnullbydefault_types(&self) -> &[String] {
        &self.nonnullbydefault_types
    }
    pub fn set_null_analysis_mode(&mut self, mode: FeatureStatus) {
        self.null_analysis_mode = mode;
    }
    pub fn get_null_analysis_mode(&self) -> FeatureStatus {
        self.null_analysis_mode
    }
}

/// `Platform.getStateLocation(m2e).append(LIFECYCLE_MAPPING_METADATA_SOURCE_NAME)`.
pub fn default_lifecycle_mappings() -> String {
    m2e_state_location().join(LIFECYCLE_MAPPING_METADATA_SOURCE_NAME).to_string_lossy().into_owned()
}

/// The `org.eclipse.m2e.core` bundle state location in the workspace.
pub fn m2e_state_location() -> PathBuf {
    crate::server::data_dir().join(".metadata").join(".plugins").join("org.eclipse.m2e.core")
}

/// Port of `org.eclipse.jdt.ls.core.internal.preferences.PreferencesTest`.
#[cfg(test)]
mod preferences_test {
    use super::super::manager::{PreferenceManager, M2E_DISABLE_TEST_CLASSPATH_FLAG};
    use super::super::map_flattener::set_value;
    use super::*;
    use serde_json::json;

    /// `new HashMap<>()`.
    fn config() -> Value {
        Value::Object(Map::new())
    }

    #[test]
    fn test_set_import_on_demand_threshold() {
        let mut preferences = Preferences::new();
        preferences.set_import_on_demand_threshold(10);
        assert_eq!(10, preferences.get_import_on_demand_threshold());

        // Zero will fallback to default
        preferences.set_import_on_demand_threshold(0);
        assert_eq!(IMPORTS_ONDEMANDTHRESHOLD_DEFAULT, preferences.get_import_on_demand_threshold());

        // Negative will fallback to default
        preferences.set_import_on_demand_threshold(-1);
        assert_eq!(IMPORTS_ONDEMANDTHRESHOLD_DEFAULT, preferences.get_import_on_demand_threshold());
    }

    #[test]
    fn test_set_static_import_on_demand_threshold() {
        let mut preferences = Preferences::new();
        preferences.set_static_import_on_demand_threshold(10);
        assert_eq!(10, preferences.get_static_import_on_demand_threshold());

        // Zero will fallback to default
        preferences.set_static_import_on_demand_threshold(0);
        assert_eq!(IMPORTS_STATIC_ONDEMANDTHRESHOLD_DEFAULT, preferences.get_static_import_on_demand_threshold());

        // Negative will fallback to default
        preferences.set_static_import_on_demand_threshold(-1);
        assert_eq!(IMPORTS_STATIC_ONDEMANDTHRESHOLD_DEFAULT, preferences.get_static_import_on_demand_threshold());
    }

    #[test]
    fn test_legacy_completion_guess_method_arguments() {
        let mut config = config();
        set_value(&mut config, JAVA_COMPLETION_GUESS_METHOD_ARGUMENTS_KEY, json!(true));

        let preferences = Preferences::create_from(&config);
        assert_eq!(
            CompletionGuessMethodArgumentsMode::InsertBestGuessedArguments,
            preferences.get_guess_method_arguments_mode()
        );
    }

    #[test]
    fn test_partial_update_preserves_existing_values() {
        // Create initial preferences with full configuration
        let mut initial_config = config();
        set_value(&mut initial_config, COMPLETION_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, JAVA_COMPLETION_OVERWRITE_KEY, json!(false));
        set_value(&mut initial_config, AUTOBUILD_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, JAVA_FORMAT_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, JAVA_FORMAT_ON_TYPE_ENABLED_KEY, json!(false));
        set_value(&mut initial_config, IMPORT_GRADLE_ENABLED, json!(true));

        let initial = Preferences::create_from(&initial_config);
        assert!(initial.is_autobuild_enabled(), "Initial autobuild should be enabled");
        assert!(initial.is_completion_enabled(), "Initial completion should be enabled");
        assert!(!initial.is_completion_overwrite(), "Initial completion overwrite should be false");
        assert!(initial.is_java_format_enabled(), "Initial format should be enabled");
        assert!(!initial.is_java_format_on_type_enabled(), "Initial format on type should be disabled");
        assert!(initial.is_import_gradle_enabled(), "Initial Gradle import should be enabled");

        // Now send a partial update that only changes autobuild and completion overwrite
        let mut partial_config = config();
        set_value(&mut partial_config, AUTOBUILD_ENABLED_KEY, json!(false)); // Change this
        set_value(&mut partial_config, JAVA_COMPLETION_OVERWRITE_KEY, json!(true)); // Change this
        // Note: NOT sending format, import, or completion.enabled

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify the updated values changed
        assert!(!updated.is_autobuild_enabled(), "Updated autobuild should be disabled");
        assert!(updated.is_completion_overwrite(), "Updated completion overwrite should be true");

        // Verify the non-updated values were preserved
        assert!(updated.is_completion_enabled(), "Completion enabled should still be true (preserved)");
        assert!(updated.is_java_format_enabled(), "Format enabled should still be true (preserved)");
        assert!(!updated.is_java_format_on_type_enabled(), "Format on type should still be false (preserved)");
        assert!(updated.is_import_gradle_enabled(), "Gradle import should still be enabled (preserved)");
    }

    #[test]
    fn test_update_from_does_not_modify_original() {
        // Create initial preferences
        let mut initial_config = config();
        set_value(&mut initial_config, AUTOBUILD_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, COMPLETION_ENABLED_KEY, json!(true));

        let original = Preferences::create_from(&initial_config);
        assert!(original.is_autobuild_enabled(), "Original autobuild should be enabled");
        assert!(original.is_completion_enabled(), "Original completion should be enabled");

        // Update with partial config
        let mut partial_config = config();
        set_value(&mut partial_config, AUTOBUILD_ENABLED_KEY, json!(false));

        let updated = Preferences::update_from(&original, &partial_config);

        // Verify original wasn't modified
        assert!(original.is_autobuild_enabled(), "Original autobuild should still be enabled");
        assert!(original.is_completion_enabled(), "Original completion should still be enabled");

        // Verify updated has the changes
        assert!(!updated.is_autobuild_enabled(), "Updated autobuild should be disabled");
        assert!(updated.is_completion_enabled(), "Updated completion should be enabled (preserved)");
    }

    #[test]
    fn test_clone_creates_independent_copy() {
        // Create preferences with various settings
        let mut original = Preferences::new();
        original.set_autobuild_enabled(true);
        original.set_completion_enabled(true);
        original.set_java_format_enabled(true);
        original.set_import_gradle_enabled(false);
        original.set_tab_size(4);
        original.set_insert_spaces(true);

        // Clone it
        let mut cloned = original.clone();

        // Verify values are the same
        assert_eq!(original.is_autobuild_enabled(), cloned.is_autobuild_enabled());
        assert_eq!(original.is_completion_enabled(), cloned.is_completion_enabled());
        assert_eq!(original.is_java_format_enabled(), cloned.is_java_format_enabled());
        assert_eq!(original.is_import_gradle_enabled(), cloned.is_import_gradle_enabled());
        assert_eq!(original.get_tab_size(), cloned.get_tab_size());
        assert_eq!(original.is_insert_spaces(), cloned.is_insert_spaces());

        // Verify it's a different instance
        assert!(!std::ptr::eq(&original, &cloned), "Clone should be a different instance");

        // Verify modifying clone doesn't affect original
        cloned.set_autobuild_enabled(false);
        cloned.set_tab_size(2);

        assert!(original.is_autobuild_enabled(), "Original autobuild should still be true");
        assert_eq!(4, original.get_tab_size(), "Original tab size should still be 4");
        assert!(!cloned.is_autobuild_enabled(), "Cloned autobuild should be false");
        assert_eq!(2, cloned.get_tab_size(), "Cloned tab size should be 2");
    }

    #[test]
    fn test_multiple_partial_updates() {
        // Start with default preferences
        let mut prefs = Preferences::new();
        assert!(prefs.is_autobuild_enabled(), "Default autobuild should be enabled");
        assert!(prefs.is_completion_enabled(), "Default completion should be enabled");
        assert!(prefs.is_java_format_enabled(), "Default format should be enabled");

        // First partial update: disable autobuild
        let mut update1 = config();
        set_value(&mut update1, AUTOBUILD_ENABLED_KEY, json!(false));

        prefs = Preferences::update_from(&prefs, &update1);
        assert!(!prefs.is_autobuild_enabled(), "Autobuild should be disabled after update 1");
        assert!(prefs.is_completion_enabled(), "Completion should still be enabled after update 1");
        assert!(prefs.is_java_format_enabled(), "Format should still be enabled after update 1");

        // Second partial update: disable completion
        let mut update2 = config();
        set_value(&mut update2, COMPLETION_ENABLED_KEY, json!(false));

        prefs = Preferences::update_from(&prefs, &update2);
        assert!(!prefs.is_autobuild_enabled(), "Autobuild should still be disabled after update 2");
        assert!(!prefs.is_completion_enabled(), "Completion should be disabled after update 2");
        assert!(prefs.is_java_format_enabled(), "Format should still be enabled after update 2");

        // Third partial update: disable format
        let mut update3 = config();
        set_value(&mut update3, JAVA_FORMAT_ENABLED_KEY, json!(false));

        prefs = Preferences::update_from(&prefs, &update3);
        assert!(!prefs.is_autobuild_enabled(), "Autobuild should still be disabled after update 3");
        assert!(!prefs.is_completion_enabled(), "Completion should still be disabled after update 3");
        assert!(!prefs.is_java_format_enabled(), "Format should be disabled after update 3");
    }

    #[test]
    fn test_partial_update_with_nested_properties() {
        // Create initial preferences
        let mut initial_config = config();
        set_value(&mut initial_config, JAVA_FORMAT_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, JAVA_FORMAT_ON_TYPE_ENABLED_KEY, json!(true));
        set_value(&mut initial_config, COMPLETION_ENABLED_KEY, json!(true));
        set_value(
            &mut initial_config,
            JAVA_COMPLETION_FAVORITE_MEMBERS_KEY,
            json!(["org.junit.Assert.*", "org.mockito.Mockito.*"]),
        );

        let initial = Preferences::create_from(&initial_config);
        assert!(initial.is_java_format_enabled(), "Initial format should be enabled");
        assert!(initial.is_java_format_on_type_enabled(), "Initial format on type should be enabled");
        assert!(initial.is_completion_enabled(), "Initial completion should be enabled");
        assert!(!initial.get_java_completion_favorite_members().is_empty(), "Initial favorite members should not be null");

        // Partial update: only change format.enabled, leave format.onType untouched
        let mut partial_config = config();
        set_value(&mut partial_config, JAVA_FORMAT_ENABLED_KEY, json!(false));
        // Note: NOT sending format.onType or completion

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify the specific nested property changed
        assert!(!updated.is_java_format_enabled(), "Updated format should be disabled");

        // Verify other nested property was preserved
        assert!(updated.is_java_format_on_type_enabled(), "Format on type should still be enabled (preserved)");

        // Verify unrelated properties were preserved
        assert!(updated.is_completion_enabled(), "Completion should still be enabled (preserved)");
        assert!(
            !updated.get_java_completion_favorite_members().is_empty(),
            "Favorite members should still be present (preserved)"
        );
    }

    #[test]
    fn test_empty_partial_update_preserves_all() {
        // Create initial preferences
        let mut initial_config = config();
        set_value(&mut initial_config, AUTOBUILD_ENABLED_KEY, json!(false));
        set_value(&mut initial_config, COMPLETION_ENABLED_KEY, json!(false));

        let initial = Preferences::create_from(&initial_config);
        assert!(!initial.is_autobuild_enabled(), "Initial autobuild should be disabled");
        assert!(!initial.is_completion_enabled(), "Initial completion should be disabled");

        // Send empty partial update
        let empty_config = config();

        let updated = Preferences::update_from(&initial, &empty_config);

        // Verify everything was preserved
        assert!(!updated.is_autobuild_enabled(), "Autobuild should still be disabled");
        assert!(!updated.is_completion_enabled(), "Completion should still be disabled");
    }

    #[test]
    fn test_maven_lifecycle_mappings() {
        // Create initial preferences
        let mut initial_config = config();
        initial_config["java"] = json!({});
        let mut initial = Preferences::create_from(&initial_config);
        assert!(!initial.get_maven_lifecycle_mappings().is_empty());
        // Send empty partial update
        let empty_config = config();
        let mut updated = Preferences::update_from(&initial, &empty_config);
        assert!(!updated.get_maven_lifecycle_mappings().is_empty());
    }

    #[test]
    fn test_maven_disable_test_classpath_flag() {
        let mut preference_manager = PreferenceManager::new();
        let flag = |m: &mut PreferenceManager| {
            m.instance_prefs().get(M2E_DISABLE_TEST_CLASSPATH_FLAG).is_some_and(|v| v == "true")
        };
        let mut maven_disable_test_classpath_flag = flag(&mut preference_manager);
        assert!(!maven_disable_test_classpath_flag);
        let mut config_map = config();
        // java.import.maven.disableTestClasspathFlag
        set_value(&mut config_map, MAVEN_DISABLE_TEST_CLASSPATH_FLAG, json!(true));
        let preferences = Preferences::create_from(&config_map);
        preference_manager.update(preferences.clone());
        assert!(preferences.is_maven_disable_test_classpath_flag());
        maven_disable_test_classpath_flag = flag(&mut preference_manager);
        assert!(maven_disable_test_classpath_flag);
        let mut config_map = config();
        // java.import.maven.disableTestClasspathFlag
        set_value(&mut config_map, MAVEN_DISABLE_TEST_CLASSPATH_FLAG, json!(false));
        let preferences = Preferences::create_from(&config_map);
        preference_manager.update(preferences.clone());
        assert!(!preferences.is_maven_disable_test_classpath_flag());
        maven_disable_test_classpath_flag = flag(&mut preference_manager);
        assert!(!maven_disable_test_classpath_flag);
        // finally
        preference_manager
            .instance_prefs()
            .insert(M2E_DISABLE_TEST_CLASSPATH_FLAG.to_owned(), maven_disable_test_classpath_flag.to_string());
    }

    #[test]
    fn test_settings_url_preservation() {
        let settings_url = "file:///path/to/settings.properties";

        // Create initial preferences with java.settings.url set
        let mut initial_config = config();
        set_value(&mut initial_config, JAVA_SETTINGS_URL, json!(settings_url));

        let initial = Preferences::create_from(&initial_config);
        assert_eq!(Some(settings_url), initial.get_settings_url(), "Initial settings URL should be set");

        // Update with partial config that doesn't include java.settings.url
        let mut partial_config = config();
        set_value(&mut partial_config, AUTOBUILD_ENABLED_KEY, json!(false));
        // Note: NOT sending java.settings.url

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify settings URL was preserved
        assert_eq!(Some(settings_url), updated.get_settings_url(), "Settings URL should be preserved when not in update");
        assert!(!updated.is_autobuild_enabled(), "Autobuild should be updated");
    }

    #[test]
    fn test_settings_url_explicit_null() {
        let settings_url = "file:///path/to/settings.properties";

        // Create initial preferences with java.settings.url set
        let mut initial_config = config();
        set_value(&mut initial_config, JAVA_SETTINGS_URL, json!(settings_url));

        let initial = Preferences::create_from(&initial_config);
        assert_eq!(Some(settings_url), initial.get_settings_url(), "Initial settings URL should be set");

        // Update with java.settings.url explicitly set to null
        let mut partial_config = config();
        set_value(&mut partial_config, JAVA_SETTINGS_URL, Value::Null);

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify settings URL was set to null
        assert_eq!(None, updated.get_settings_url(), "Settings URL should be null when explicitly set to null");
    }

    #[test]
    fn test_settings_url_update() {
        let old_settings_url = "file:///path/to/old.properties";
        let new_settings_url = "file:///path/to/new.properties";

        // Create initial preferences with java.settings.url set
        let mut initial_config = config();
        set_value(&mut initial_config, JAVA_SETTINGS_URL, json!(old_settings_url));

        let initial = Preferences::create_from(&initial_config);
        assert_eq!(Some(old_settings_url), initial.get_settings_url(), "Initial settings URL should be set");

        // Update with java.settings.url set to a new value
        let mut partial_config = config();
        set_value(&mut partial_config, JAVA_SETTINGS_URL, json!(new_settings_url));

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify settings URL was updated
        assert_eq!(Some(new_settings_url), updated.get_settings_url(), "Settings URL should be updated to new value");
    }

    #[test]
    fn test_settings_url_initial_null_then_set() {
        let settings_url = "file:///path/to/settings.properties";

        // Create initial preferences without java.settings.url
        let initial = Preferences::new();
        assert_eq!(None, initial.get_settings_url(), "Initial settings URL should be null");

        // Update with java.settings.url set
        let mut partial_config = config();
        set_value(&mut partial_config, JAVA_SETTINGS_URL, json!(settings_url));

        let updated = Preferences::update_from(&initial, &partial_config);

        // Verify settings URL was set
        assert_eq!(Some(settings_url), updated.get_settings_url(), "Settings URL should be set");
    }

    #[test]
    fn test_settings_url_set_then_unset() {
        let settings_url = "file:///path/to/settings.properties";

        // Create initial preferences with java.settings.url set
        let mut initial_config = config();
        set_value(&mut initial_config, JAVA_SETTINGS_URL, json!(settings_url));

        let initial = Preferences::create_from(&initial_config);
        assert_eq!(Some(settings_url), initial.get_settings_url(), "Initial settings URL should be set");

        // First update: set to null
        let mut partial_config1 = config();
        set_value(&mut partial_config1, JAVA_SETTINGS_URL, Value::Null);

        let updated1 = Preferences::update_from(&initial, &partial_config1);
        assert_eq!(None, updated1.get_settings_url(), "Settings URL should be null after first update");

        // Second update: don't include java.settings.url (should preserve null)
        let mut partial_config2 = config();
        set_value(&mut partial_config2, AUTOBUILD_ENABLED_KEY, json!(false));

        let updated2 = Preferences::update_from(&updated1, &partial_config2);
        assert_eq!(None, updated2.get_settings_url(), "Settings URL should remain null when not in update");
    }
}
