//! `WrapperValidator`: checks the SHA-256 of a project's `gradle-wrapper.jar`
//! against the checksums Gradle publishes.

use super::sha256;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const GRADLE_WRAPPER_JAR: &str = "gradle/wrapper/gradle-wrapper.jar";
const QUEUE_LENGTH: usize = 20;
const WRAPPER_CHECKSUM_URL: &str = "wrapperChecksumUrl";
const INTERNAL_CHECKSUMS: &str = include_str!("checksums.json");

#[derive(Default)]
struct State {
    allowed: BTreeSet<String>,
    disallowed: BTreeSet<String>,
    wrapper_checksum_urls: BTreeSet<String>,
    downloaded: bool,
    cache_dir: Option<PathBuf>,
}

static STATE: Mutex<State> = Mutex::new(State {
    allowed: BTreeSet::new(),
    disallowed: BTreeSet::new(),
    wrapper_checksum_urls: BTreeSet::new(),
    downloaded: false,
    cache_dir: None,
});

/// `ValidationResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub wrapper_jar: String,
    pub checksum: String,
    pub valid: bool,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

pub struct WrapperValidator {
    queue_length: usize,
}

impl Default for WrapperValidator {
    fn default() -> Self {
        Self::new(QUEUE_LENGTH)
    }
}

fn xdg_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

fn version_cache_file() -> PathBuf {
    xdg_cache().join("tooling/gradle/versions.json")
}

/// The `gradle.checksum.cacheDir` system property.
pub fn set_checksum_cache_dir(dir: Option<&Path>) {
    STATE.lock().unwrap().cache_dir = dir.map(Path::to_path_buf);
}

/// `WrapperValidator.getSha256CacheFile()`.
pub fn get_sha256_cache_file() -> PathBuf {
    let dir = STATE
        .lock()
        .unwrap()
        .cache_dir
        .clone()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| xdg_cache().join("tooling/gradle/checksums"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn clear() {
    let mut s = STATE.lock().unwrap();
    s.allowed.clear();
    s.disallowed.clear();
    s.wrapper_checksum_urls.clear();
}

pub fn allow<I: IntoIterator<Item = String>>(checksums: I) {
    STATE.lock().unwrap().allowed.extend(checksums);
}

pub fn disallow<I: IntoIterator<Item = String>>(checksums: I) {
    STATE.lock().unwrap().disallowed.extend(checksums);
}

pub fn contains(checksum: &str) -> bool {
    STATE.lock().unwrap().disallowed.contains(checksum)
}

pub fn size() -> usize {
    let s = STATE.lock().unwrap();
    s.allowed.len() + s.disallowed.len()
}

/// `Collections.unmodifiableSet(allowed)`: a live view of the allowed set.
#[derive(Debug, Clone, Copy)]
pub struct AllowedView;

impl AllowedView {
    pub fn snapshot(&self) -> Vec<String> {
        STATE.lock().unwrap().allowed.iter().cloned().collect()
    }
}

pub fn get_allowed() -> AllowedView {
    AllowedView
}

/// Mirrors upstream, which returns a view of the allowed set here.
pub fn get_disallowed() -> AllowedView {
    AllowedView
}

/// `WrapperValidator.putSha256(gradleWrapperList)`.
pub fn put_sha256(list: &[Value]) {
    let mut allowed = Vec::new();
    let mut disallowed = Vec::new();
    for object in list {
        let Some(map) = object.as_object() else {
            continue;
        };
        let mut checksum: Option<String> = None;
        let mut is_allowed = true;
        for (key, value) in map {
            match (key.as_str(), value) {
                ("sha256", Value::String(s)) => checksum = Some(s.clone()),
                ("allowed", Value::Bool(b)) => is_allowed = *b,
                _ => {}
            }
        }
        if let Some(checksum) = checksum {
            if is_allowed {
                allowed.push(checksum);
            } else {
                disallowed.push(checksum);
            }
        }
    }
    clear();
    allow(allowed);
    disallow(disallowed);
}

/// `WrapperValidator.getFileName(url)`.
pub fn get_file_name(url: &str) -> Option<&str> {
    match url.rfind('/') {
        Some(i) if url.len() > i => Some(&url[i + 1..]),
        _ => {
            tracing::info!("Invalid wrapper URL {url}");
            None
        }
    }
}

/// The wrapper checksum list bundled with jdt.ls.
pub fn internal_checksums() -> Vec<Value> {
    serde_json::from_str::<Value>(INTERNAL_CHECKSUMS)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn load_internal_checksums() {
    let mut s = STATE.lock().unwrap();
    for entry in internal_checksums() {
        if let Some(sha) = entry["sha256"].as_str() {
            s.allowed.insert(sha.to_owned());
        }
        if let Some(url) = entry[WRAPPER_CHECKSUM_URL].as_str() {
            s.wrapper_checksum_urls.insert(url.to_owned());
        }
    }
}

fn http_get(url: &str) -> Option<String> {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
        .get(url)
        .call()
        .ok()?
        .into_string()
        .ok()
}

fn update_gradle_versions_file() {
    let file = version_cache_file();
    if file.is_file() || std::env::var("JDTLS_RUST_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    if let Some(body) = http_get("https://services.gradle.org/versions/all") {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&file, body);
    }
}

fn read_first_line(path: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path).ok()?.read_to_string(&mut text).ok()?;
    text.lines().next().map(str::to_owned)
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

impl WrapperValidator {
    pub fn new(queue_length: usize) -> Self {
        Self { queue_length }
    }

    /// `checkWrapper(baseDir)`.
    pub fn check_wrapper(&self, base_dir: &str) -> Result<ValidationResult, String> {
        let wrapper_jar = Path::new(base_dir).join(GRADLE_WRAPPER_JAR);
        if !wrapper_jar.exists() {
            return Err(format!("{} doesn't exist.", wrapper_jar.display()));
        }
        let needs_load = {
            let s = STATE.lock().unwrap();
            !s.downloaded || s.allowed.is_empty()
        };
        if needs_load {
            load_internal_checksums();
            let version_file = version_cache_file();
            if !version_file.exists() {
                update_gradle_versions_file();
            }
            if version_file.exists() {
                self.load_published_checksums(&version_file);
            }
        }
        let mut data = Vec::new();
        std::fs::File::open(&wrapper_jar)
            .and_then(|mut f| f.read_to_end(&mut data))
            .map_err(|e| e.to_string())?;
        let checksum = sha256::hex(&sha256::digest(&data));
        let valid = STATE.lock().unwrap().allowed.contains(&checksum);
        Ok(ValidationResult {
            wrapper_jar: wrapper_jar.display().to_string(),
            checksum,
            valid,
        })
    }

    fn load_published_checksums(&self, version_file: &Path) {
        let Ok(text) = std::fs::read_to_string(version_file) else {
            return;
        };
        let versions: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
        let cache_dir = get_sha256_cache_file();
        let version_time = modified(version_file);
        let mut to_download: Vec<String> = Vec::new();
        for url in versions.iter().filter_map(|v| v[WRAPPER_CHECKSUM_URL].as_str()) {
            if STATE.lock().unwrap().wrapper_checksum_urls.contains(url) {
                continue;
            }
            let Some(name) = get_file_name(url) else {
                continue;
            };
            let sha_file = cache_dir.join(name);
            if !sha_file.exists() || modified(&sha_file) < version_time {
                to_download.push(url.to_owned());
            } else if let Some(sha) = read_first_line(&sha_file) {
                STATE.lock().unwrap().allowed.insert(sha);
            }
        }
        for batch in to_download.chunks(self.queue_length.max(1)) {
            let handles: Vec<_> = batch
                .iter()
                .cloned()
                .map(|url| {
                    let cache_dir = cache_dir.clone();
                    std::thread::spawn(move || {
                        let Some(sha) = http_get(&url).map(|s| s.trim().to_owned()) else {
                            tracing::warn!("Cannot download Gradle sha256 checksum: {url}");
                            return;
                        };
                        if let Some(name) = get_file_name(&url) {
                            let _ = std::fs::write(cache_dir.join(name), &sha);
                        }
                        STATE.lock().unwrap().allowed.insert(sha);
                    })
                })
                .collect();
            for h in handles {
                let _ = h.join();
            }
        }
        STATE.lock().unwrap().downloaded = true;
    }
}
