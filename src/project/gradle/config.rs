//! Build configuration of a Gradle import (`GradleProjectImporter` static
//! helpers and Buildship's `BuildConfiguration`).

use super::util::{self, GradleVersion};
use crate::project::runtime::RuntimeEnvironment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const GRADLE_HOME: &str = "GRADLE_HOME";
pub const GRADLE_USER_HOME: &str = "GRADLE_USER_HOME";
pub const GRADLE_WRAPPER_PROPERTIES_DESCRIPTOR: &str = "gradle/wrapper/gradle-wrapper.properties";

/// `GradleVersion.current()` of the Gradle Tooling API jdt.ls ships.
pub const DEFAULT_GRADLE_VERSION: &str = "8.9";

/// A Buildship `GradleDistribution`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradleDistribution {
    /// `GradleDistribution.fromBuild()` (`WrapperGradleDistribution`).
    Wrapper,
    /// `GradleDistribution.forVersion(version)` (`FixedVersionGradleDistribution`).
    FixedVersion(String),
    /// `GradleDistribution.forLocalInstallation(dir)` (`LocalGradleDistribution`).
    Local(PathBuf),
}

impl GradleDistribution {
    pub fn is_wrapper(&self) -> bool {
        matches!(self, GradleDistribution::Wrapper)
    }
}

/// `GradleProjectImporter.DEFAULT_DISTRIBUTION`.
pub fn default_distribution() -> GradleDistribution {
    GradleDistribution::FixedVersion(DEFAULT_GRADLE_VERSION.to_owned())
}

/// The Gradle related `Preferences`.
#[derive(Debug, Clone, PartialEq)]
pub struct GradleSettings {
    pub wrapper_enabled: bool,
    pub version: Option<String>,
    pub home: Option<String>,
    pub user_home: Option<String>,
    pub java_home: Option<String>,
    pub arguments: Vec<String>,
    pub jvm_arguments: Vec<String>,
    pub offline: bool,
    /// `java.configuration.updateBuildConfiguration`.
    pub update_build_configuration: String,
    pub protobuf_support: bool,
    pub android_support: bool,
    pub aspectj_support: bool,
    pub kotlin_support: bool,
    pub groovy_support: bool,
    pub scala_support: bool,
    pub annotation_processing: bool,
    /// `java.home`.
    pub java_home_preference: Option<String>,
    /// `java.imports.gradle.wrapper.checksums`.
    pub wrapper_checksums: Vec<serde_json::Value>,
    /// `java.configuration.runtimes`.
    pub runtimes: Vec<RuntimeEnvironment>,
    /// The default VM install location (`JavaRuntime.getDefaultVMInstall()`).
    pub default_vm: Option<PathBuf>,
    /// Where init scripts are materialized.
    pub scripts_dir: Option<PathBuf>,
    /// The JDK that runs the Gradle model helper.
    pub launcher_java: Option<PathBuf>,
}

impl Default for GradleSettings {
    fn default() -> Self {
        Self {
            wrapper_enabled: true,
            version: None,
            home: None,
            user_home: None,
            java_home: None,
            arguments: Vec::new(),
            jvm_arguments: Vec::new(),
            offline: false,
            update_build_configuration: "interactive".to_owned(),
            protobuf_support: false,
            android_support: false,
            aspectj_support: false,
            kotlin_support: false,
            groovy_support: false,
            scala_support: false,
            annotation_processing: true,
            java_home_preference: None,
            wrapper_checksums: Vec::new(),
            runtimes: Vec::new(),
            default_vm: None,
            scripts_dir: None,
            launcher_java: None,
        }
    }
}

impl GradleSettings {
    pub fn from_settings(c: &serde_json::Value) -> Self {
        use crate::project::{pref_bool, pref_list, pref_string, pref_value};
        let mut s = Self::default();
        let text = |key: &str| pref_string(c, key);
        if let Some(b) = pref_bool(c, "java.import.gradle.wrapper.enabled") {
            s.wrapper_enabled = b;
        }
        s.version = text("java.import.gradle.version");
        s.home = text("java.import.gradle.home");
        s.user_home = text("java.import.gradle.user.home");
        s.java_home = text("java.import.gradle.java.home");
        if let Some(a) = pref_list(c, "java.import.gradle.arguments") {
            s.arguments = a;
        }
        if let Some(a) = pref_list(c, "java.import.gradle.jvmArguments") {
            s.jvm_arguments = a;
        }
        if let Some(b) = pref_bool(c, "java.import.gradle.offline.enabled") {
            s.offline = b;
        }
        if let Some(v) = text("java.configuration.updateBuildConfiguration") {
            if ["disabled", "interactive", "automatic"].contains(&v.as_str()) {
                s.update_build_configuration = v;
            }
        }
        for (key, field) in [
            ("java.jdt.ls.protobufSupport.enabled", &mut s.protobuf_support),
            ("java.jdt.ls.androidSupport.enabled", &mut s.android_support),
            ("java.jdt.ls.aspectjSupport.enabled", &mut s.aspectj_support),
            ("java.jdt.ls.kotlinSupport.enabled", &mut s.kotlin_support),
            ("java.jdt.ls.groovySupport.enabled", &mut s.groovy_support),
            ("java.jdt.ls.scalaSupport.enabled", &mut s.scala_support),
            (
                "java.import.gradle.annotationProcessing.enabled",
                &mut s.annotation_processing,
            ),
        ] {
            if let Some(b) = pref_bool(c, key) {
                *field = b;
            }
        }
        s.java_home_preference = text("java.home");
        if let Some(list) = pref_value(c, "java.imports.gradle.wrapper.checksums")
            .and_then(serde_json::Value::as_array)
        {
            s.wrapper_checksums = list.clone();
        }
        if let Some(runtimes) = crate::project::runtime::parse_runtimes(c) {
            s.runtimes = runtimes;
        }
        s
    }

    pub fn auto_sync(&self) -> bool {
        self.update_build_configuration == "automatic"
    }
}

fn not_blank(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.trim().is_empty())
}

/// Buildship's `BuildConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildConfiguration {
    pub root_project_directory: PathBuf,
    pub override_workspace_settings: bool,
    pub distribution: GradleDistribution,
    pub java_home: Option<PathBuf>,
    pub arguments: Vec<String>,
    pub gradle_user_home: Option<PathBuf>,
    pub jvm_arguments: Vec<String>,
    pub offline_mode: bool,
    pub auto_sync: bool,
}

static PUBLISHED_VERSIONS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// `CorePlugin.publishedGradleVersions().getVersions()`.
pub fn published_gradle_versions() -> Vec<String> {
    let mut cache = PUBLISHED_VERSIONS.lock().unwrap();
    if let Some(v) = cache.as_ref() {
        return v.clone();
    }
    let fetched = fetch_published_versions().unwrap_or_default();
    if !fetched.is_empty() {
        *cache = Some(fetched.clone());
    }
    fetched
}

fn fetch_published_versions() -> Option<Vec<String>> {
    if std::env::var("JDTLS_RUST_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    let body = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .get("https://services.gradle.org/versions/all")
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let list: serde_json::Value = serde_json::from_str(&body).ok()?;
    Some(
        list.as_array()?
            .iter()
            .filter_map(|v| v["version"].as_str().map(str::to_owned))
            .collect(),
    )
}

fn is_published(version: &GradleVersion) -> bool {
    let versions = published_gradle_versions();
    if versions.is_empty() {
        // The list could not be loaded: accept versions Gradle itself knows locally.
        return true;
    }
    versions
        .iter()
        .filter_map(|v| GradleVersion::version(v))
        .any(|v| v.cmp(version) == std::cmp::Ordering::Equal)
}

/// `GradleProjectImporter.getGradleDistribution(rootFolder)`.
pub fn get_gradle_distribution(root: &Path, settings: &GradleSettings) -> GradleDistribution {
    if settings.wrapper_enabled && root.join(GRADLE_WRAPPER_PROPERTIES_DESCRIPTOR).exists() {
        return GradleDistribution::Wrapper;
    }
    if let Some(version) = not_blank(&settings.version) {
        match GradleVersion::version(version) {
            Some(required) if is_published(&required) => {
                return GradleDistribution::FixedVersion(required.get_version().to_owned())
            }
            _ => tracing::info!("Invalid gradle version{version}"),
        }
    }
    if let Some(home) = get_gradle_home_file_default(settings) {
        return GradleDistribution::Local(home);
    }
    default_distribution()
}

pub fn get_gradle_home_file_default(settings: &GradleSettings) -> Option<PathBuf> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let sysprops = HashMap::new();
    get_gradle_home_file(settings, &env, &sysprops)
}

/// `GradleProjectImporter.getGradleHomeFile(env, sysprops)`.
pub fn get_gradle_home_file(
    settings: &GradleSettings,
    env: &HashMap<String, String>,
    sysprops: &HashMap<String, String>,
) -> Option<PathBuf> {
    if let Some(home) = not_blank(&settings.home) {
        return Some(PathBuf::from(home));
    }
    let mut gradle_home = env.get(GRADLE_HOME).cloned();
    if gradle_home.as_deref().is_none_or(|h| !Path::new(h).is_dir()) {
        gradle_home = sysprops.get(GRADLE_HOME).cloned();
    }
    gradle_home
        .map(PathBuf::from)
        .filter(|h| h.is_dir())
}

/// `GradleProjectImporter.getGradleUserHomeFile()`.
pub fn get_gradle_user_home_file(settings: &GradleSettings) -> Option<PathBuf> {
    if let Some(home) = not_blank(&settings.user_home) {
        return Some(PathBuf::from(home));
    }
    std::env::var(GRADLE_USER_HOME)
        .ok()
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `GradleProjectImporter.getGradleJavaHomeFile()`.
pub fn get_gradle_java_home_file(settings: &GradleSettings) -> Option<PathBuf> {
    let home = PathBuf::from(not_blank(&settings.java_home)?);
    home.is_dir().then_some(home)
}

/// `GradleProjectImporter.getJavaHome(preferences)`.
pub fn get_java_home(settings: &GradleSettings) -> Option<PathBuf> {
    get_gradle_java_home_file(settings)
        .or_else(|| settings.default_vm.clone())
        .or_else(|| settings.java_home_preference.as_deref().map(PathBuf::from))
}

const INIT_SCRIPTS: [(&str, &str); 8] = [
    ("/gradle/init/init.gradle", include_str!("init/init.gradle")),
    (
        "/gradle/protobuf/init.gradle",
        include_str!("protobuf/init.gradle"),
    ),
    (
        "/gradle/android/init.gradle",
        include_str!("android/init.gradle"),
    ),
    (
        "/gradle/aspectj/init.gradle",
        include_str!("aspectj/init.gradle"),
    ),
    (
        "/gradle/kotlin/init.gradle",
        include_str!("kotlin/init.gradle"),
    ),
    (
        "/gradle/groovy/init.gradle",
        include_str!("groovy/init.gradle"),
    ),
    ("/gradle/scala/init.gradle", include_str!("scala/init.gradle")),
    ("/gradle/apt/init.gradle", include_str!("apt/init.gradle")),
];

pub const SCALA_JAVALS_SCRIPT: (&str, &str) =
    ("/gradle/scala/javals.gradle", include_str!("scala/javals.gradle"));

fn scripts_dir(settings: &GradleSettings) -> PathBuf {
    settings
        .scripts_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("jdtls-rust-gradle"))
}

/// `GradleUtils.getGradleInitScript(path)` for the bundled scripts.
pub fn gradle_init_script(settings: &GradleSettings, script_path: &str) -> Option<PathBuf> {
    let content = INIT_SCRIPTS
        .iter()
        .chain(std::iter::once(&SCALA_JAVALS_SCRIPT))
        .find(|(p, _)| *p == script_path)
        .map(|(_, c)| *c)?;
    util::get_gradle_init_script(&scripts_dir(settings), script_path, content)
}

/// `GradleProjectImporter.getGradleInitScriptArgs()`.
pub fn gradle_init_script_args(settings: &GradleSettings) -> Vec<String> {
    let mut args = Vec::new();
    let mut add = |path: &str| {
        if let Some(script) = gradle_init_script(settings, path) {
            if std::fs::metadata(&script).is_ok_and(|m| m.len() > 0) {
                args.push("--init-script".to_owned());
                args.push(script.to_string_lossy().into_owned());
            }
        }
    };
    add("/gradle/init/init.gradle");
    for (enabled, path) in [
        (settings.protobuf_support, "/gradle/protobuf/init.gradle"),
        (settings.android_support, "/gradle/android/init.gradle"),
        (settings.aspectj_support, "/gradle/aspectj/init.gradle"),
        (settings.kotlin_support, "/gradle/kotlin/init.gradle"),
        (settings.groovy_support, "/gradle/groovy/init.gradle"),
        (settings.scala_support, "/gradle/scala/init.gradle"),
    ] {
        if enabled {
            add(path);
        }
    }
    args
}

/// `GradleProjectImporter.getBuildConfiguration(rootFolder)`.
pub fn get_build_configuration(root: &Path, settings: &GradleSettings) -> BuildConfiguration {
    let distribution = get_gradle_distribution(root, settings);
    let java_home = get_java_home(settings);
    let gradle_user_home = get_gradle_user_home_file(settings);
    let mut arguments = gradle_init_script_args(settings);
    arguments.extend(settings.arguments.iter().cloned());
    let jvm_arguments = settings.jvm_arguments.clone();
    let offline_mode = settings.offline;
    let auto_sync = settings.auto_sync();
    let override_workspace_settings = !distribution.is_wrapper()
        || offline_mode
        || !arguments.is_empty()
        || !jvm_arguments.is_empty()
        || gradle_user_home.is_some()
        || java_home.is_some()
        || auto_sync;
    BuildConfiguration {
        root_project_directory: root.to_path_buf(),
        override_workspace_settings,
        distribution,
        java_home,
        arguments,
        gradle_user_home,
        jvm_arguments,
        offline_mode,
        auto_sync,
    }
}
