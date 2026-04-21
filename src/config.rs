use serde::Deserialize;
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
