//! `GradleUtils`: Gradle/Java compatibility, JDK selection for the Gradle
//! daemon and init-script file management.

use super::sha256;
use crate::project::runtime::{RuntimeEnvironment, RuntimeRegistry};
use crate::project::{compare_java_versions, normalize_java_version};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const INVALID_TYPE_FIXED_VERSION: &str = "7.2";
pub const JPMS_SUPPORTED_VERSION: &str = "7.0.1";

/// `org.gradle.util.GradleVersion` restricted to what jdt.ls compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleVersion {
    version: String,
    base: Vec<u32>,
}

impl GradleVersion {
    /// `GradleVersion.version(text)`; `None` where Gradle throws `IllegalArgumentException`.
    pub fn version(text: &str) -> Option<Self> {
        let numeric: String = text
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let base: Vec<u32> = numeric
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        if base.len() < 2 {
            return None;
        }
        let rest = &text[numeric.len()..];
        if !rest.is_empty() && !rest.starts_with('-') {
            return None;
        }
        Some(Self {
            version: text.to_owned(),
            base,
        })
    }

    pub fn get_version(&self) -> &str {
        &self.version
    }

    /// `getBaseVersion()`: the version without stage and snapshot qualifiers.
    pub fn base_version(&self) -> GradleVersion {
        let text = self
            .base
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        GradleVersion {
            version: text,
            base: self.base.clone(),
        }
    }
}

impl PartialOrd for GradleVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GradleVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let n = self.base.len().max(other.base.len());
        for i in 0..n {
            let a = self.base.get(i).copied().unwrap_or(0);
            let b = other.base.get(i).copied().unwrap_or(0);
            if a != b {
                return a.cmp(&b);
            }
        }
        std::cmp::Ordering::Equal
    }
}

/// `GradleUtils.isIncompatible(gradleVersion, javaVersion)`.
pub fn is_incompatible(gradle_version: Option<&GradleVersion>, java_version: Option<&str>) -> bool {
    let (Some(gradle), Some(java)) = (gradle_version, java_version) else {
        return false;
    };
    if java.is_empty() {
        return false;
    }
    compare_java_versions(java, get_highest_supported_java(gradle)) == std::cmp::Ordering::Greater
}

/// `GradleUtils.getHighestSupportedJava(gradleVersion)`, see
/// <https://docs.gradle.org/current/userguide/compatibility.html>.
pub fn get_highest_supported_java(gradle_version: &GradleVersion) -> &'static str {
    let base = gradle_version.base_version();
    let at_least = |v: &str| base >= GradleVersion::version(v).unwrap();
    const TABLE: [(&str, &str); 16] = [
        ("9.1", "25"),
        ("8.14", "24"),
        ("8.10", "23"),
        ("8.8", "22"),
        ("8.5", "21"),
        ("8.3", "20"),
        ("7.6", "19"),
        ("7.5", "18"),
        ("7.3", "17"),
        ("7.0", "16"),
        ("6.7", "15"),
        ("6.3", "14"),
        ("6.0", "13"),
        ("5.4", "12"),
        ("5.0", "11"),
        ("4.7", "10"),
    ];
    TABLE
        .iter()
        .find(|(gradle, _)| at_least(gradle))
        .map(|(_, java)| *java)
        .or_else(|| at_least("4.3").then_some("9"))
        .unwrap_or("1.8")
}

/// `GradleUtils.getMajorJavaVersion(version)`.
pub fn get_major_java_version(version: &str) -> String {
    normalize_java_version(version).unwrap_or_default()
}

/// `java.lang.Runtime.Version` ordering of the major versions used as keys.
fn runtime_version(v: &str) -> Vec<u32> {
    v.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// `GradleUtils.getJdkToLaunchDaemon(highestJavaVersion)`: the latest JDK that
/// is not newer than `highest_java_version`.
pub fn get_jdk_to_launch_daemon(
    all_installs: &BTreeMap<String, PathBuf>,
    highest_java_version: &str,
) -> Option<PathBuf> {
    if highest_java_version.trim().is_empty() {
        return None;
    }
    let highest = runtime_version(highest_java_version);
    let mut selected: Option<(&String, &PathBuf)> = None;
    for (version, home) in all_installs {
        let v = runtime_version(version);
        if v <= highest
            && selected.is_none_or(|(s, _)| runtime_version(s) < v)
        {
            selected = Some((version, home));
        }
    }
    selected.map(|(_, home)| home.clone())
}

/// `GradleUtils.getAllVmInstalls()`: the major version of every installed VM
/// (the first install wins) overridden by the `java.configuration.runtimes`.
pub fn get_all_vm_installs(
    registry: Option<&RuntimeRegistry>,
    runtimes: &[RuntimeEnvironment],
) -> BTreeMap<String, PathBuf> {
    let mut installs: BTreeMap<String, PathBuf> = BTreeMap::new();
    if let Some(registry) = registry {
        for vm in &registry.installs {
            let Some(version) = vm.version.as_deref().map(get_major_java_version) else {
                continue;
            };
            if version.is_empty() {
                continue;
            }
            installs.entry(version).or_insert_with(|| vm.home.clone());
        }
    }
    for runtime in runtimes {
        let Some(path) = runtime.path.as_deref().filter(|p| !p.trim().is_empty()) else {
            continue;
        };
        let home = PathBuf::from(path);
        if installs.values().any(|h| *h == home) {
            continue;
        }
        if let Some(version) = crate::project::vm_version(&home) {
            installs.insert(get_major_java_version(&version), home);
        }
    }
    installs
}

/// `GradleUtils.needReplaceContent(initScript, checksum)`.
pub fn need_replace_content(init_script: &Path, checksum: &[u8]) -> std::io::Result<bool> {
    let Ok(meta) = std::fs::metadata(init_script) else {
        return Ok(true);
    };
    if meta.len() == 0 {
        return Ok(true);
    }
    Ok(sha256::digest(&std::fs::read(init_script)?) != checksum)
}

/// `GradleUtils.getGradleInitScript(scriptPath)`: the script as a file under
/// `dir`, rewritten when its content differs. As Buildship cannot pass
/// arguments with spaces, a location with spaces falls back to a temp file
/// named after the content digest.
pub fn get_gradle_init_script(dir: &Path, script_path: &str, content: &str) -> Option<PathBuf> {
    let bytes = content.as_bytes();
    let checksum = sha256::digest(bytes);
    let target = dir.join(script_path.trim_start_matches('/'));
    if !target.to_string_lossy().contains(' ') {
        let write = || -> std::io::Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if need_replace_content(&target, &checksum)? {
                std::fs::write(&target, bytes)?;
            }
            Ok(())
        };
        if write().is_ok() {
            return Some(target);
        }
    }
    let temp = std::env::temp_dir().join(format!("{}.gradle", sha256::hex(&checksum)));
    if need_replace_content(&temp, &checksum).ok()? {
        std::fs::write(&temp, bytes).ok()?;
    }
    Some(temp)
}
