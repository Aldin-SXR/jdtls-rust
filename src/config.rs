use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The jdt.ls workspace directory (`-data <dir>` on the command line).
pub static DATA_DIR: once_cell::sync::OnceCell<PathBuf> = once_cell::sync::OnceCell::new();

/// Parsed from LSP `initializationOptions`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Path to a Java home directory (e.g. `/usr/lib/jvm/java-21`).
    /// Used to locate `java` binary and the standard library.
    pub java_home: Option<String>,

    /// Additional classpath entries (JARs or directories) for ECJ.
    pub classpath: Vec<String>,

    /// Java source/target compatibility level (default: "21").
    pub source_compatibility: String,

    /// `java.format.*` preferences (from `settings`).
    #[serde(skip)]
    pub format: crate::features::formatting::FormatSettings,

    /// jdt.ls `extendedClientCapabilities` (e.g. `nonStandardJavaFormatting`).
    pub extended_client_capabilities: Option<serde_json::Value>,

    /// `initializationOptions.workspaceFolders` (URIs).
    pub workspace_folders: Option<Vec<String>>,

    /// `initializationOptions.triggerFiles` (URIs): files whose folder gets an
    /// invisible project (`Preferences.setTriggerFiles`).
    pub trigger_files: Option<Vec<String>>,

    /// `initializationOptions.projectConfigurations` (URIs of build files to
    /// import instead of scanning the roots).
    pub project_configurations: Option<Vec<String>>,

    /// jdt.ls `Preferences.getRootPaths()`: `workspaceFolders` from the
    /// initialization options, else `rootUri`/`rootPath` (`BaseInitHandler`).
    #[serde(skip)]
    pub root_paths: Vec<std::path::PathBuf>,

    /// Maximum number of completion items to return.
    pub max_completions: usize,

    /// Extra JDT core compiler options (`org.eclipse.jdt.core.*` keys)
    /// applied to every project, e.g. problem severities.
    pub compiler_options: BTreeMap<String, String>,

    /// jdt.ls-style `settings` object (`{ "java": { ... } }`).
    pub settings: Option<serde_json::Value>,

    /// Client supports Markdown completion documentation
    /// (`ClientPreferences.isSupportsCompletionDocumentationMarkdown`).
    #[serde(skip)]
    pub completion_documentation_markdown: bool,
    /// Inlay-hint preferences (`java.inlayHints.*`), from `settings`.
    #[serde(skip)]
    pub inlay_hints: crate::features::inlay_hints::InlayHintPreferences,

    /// Client capability `workspace.inlayHint.refreshSupport`.
    #[serde(skip)]
    pub inlay_hint_refresh_support: bool,
}

impl Config {
    /// A jdt.ls setting by dotted path, e.g. `java.hover.javadoc.enabled`.
    pub fn setting(&self, path: &str) -> Option<&serde_json::Value> {
        let settings = self.settings.as_ref()?;
        if let Some(v) = settings.get(path) {
            return Some(v);
        }
        path.split('.').try_fold(settings, |v, k| v.get(k))
    }

    /// `extendedClientCapabilities.<name>` is `true`.
    pub fn extended_capability(&self, name: &str) -> bool {
        self.extended_client_capabilities
            .as_ref()
            .and_then(|c| c.get(name))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }
}

impl Config {
    pub fn with_defaults(mut self) -> Self {
        if self.source_compatibility.is_empty() {
            self.source_compatibility = "21".to_owned();
        }
        if self.max_completions == 0 {
            self.max_completions = 50;
        }
        self
    }

    /// Resolve the `java` binary to use when spawning ecj-bridge.
    pub fn java_binary(&self) -> String {
        if let Some(bin) = self.java_home.as_deref().and_then(java_binary_from_home) {
            return bin;
        }

        if let Ok(home) = std::env::var("JAVA_HOME") {
            if let Some(bin) = java_binary_from_home(&home) {
                return bin;
            }
        }

        "java".to_owned()
    }
}

fn java_binary_from_home(home: &str) -> Option<String> {
    if home.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(home).join("bin").join("java");
    candidate
        .is_file()
        .then(|| candidate.to_string_lossy().into_owned())
}
