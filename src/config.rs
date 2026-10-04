use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

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

    /// Formatter profile: "google" | "eclipse" (default: "eclipse").
    pub formatter_profile: String,

    /// Maximum number of completion items to return.
    pub max_completions: usize,

    /// Extra JDT core compiler options (`org.eclipse.jdt.core.*` keys)
    /// applied to every project, e.g. problem severities.
    pub compiler_options: BTreeMap<String, String>,

    /// jdt.ls-style `settings` object (`{ "java": { ... } }`).
    pub settings: Option<serde_json::Value>,

    /// `initializationOptions.extendedClientCapabilities`.
    pub extended_client_capabilities: Option<serde_json::Value>,

    /// Client supports Markdown completion documentation
    /// (`ClientPreferences.isSupportsCompletionDocumentationMarkdown`).
    #[serde(skip)]
    pub completion_documentation_markdown: bool,
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

    /// Deep-merges a `workspace/didChangeConfiguration` settings object.
    pub fn merge_settings(&mut self, settings: &serde_json::Value) {
        fn merge(target: &mut serde_json::Value, src: &serde_json::Value) {
            match (target, src) {
                (serde_json::Value::Object(t), serde_json::Value::Object(s)) => {
                    for (k, v) in s {
                        merge(t.entry(k.clone()).or_insert(serde_json::Value::Null), v);
                    }
                }
                (t, s) => *t = s.clone(),
            }
        }
        if !settings.is_object() {
            return;
        }
        let target = self.settings.get_or_insert_with(|| serde_json::json!({}));
        merge(target, settings);
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
        if self.formatter_profile.is_empty() {
            self.formatter_profile = "eclipse".to_owned();
        }
        if self.max_completions == 0 {
            self.max_completions = 50;
        }
        self
    }

    /// Resolve the `java` binary to use when spawning ecj-bridge.
    pub fn java_binary(&self) -> String {
        if let Some(bin) = self
            .java_home
            .as_deref()
            .and_then(java_binary_from_home)
        {
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
