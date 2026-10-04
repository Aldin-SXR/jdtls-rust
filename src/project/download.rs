//! Maven artifact download (what m2e's embedded Maven resolver does for
//! jdt.ls): fetch missing POMs, jars and `-sources.jar`s from the remote
//! repositories into the local repository, in Maven's layout
//! (`<repo>/<group path>/<artifact>/<version>/<artifact>-<version>[-<classifier>].<ext>`),
//! recording the origin in `_remote.repositories` as the Maven resolver does.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// A remote repository (`<repository>` / settings.xml mirror).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub id: String,
    pub url: String,
}

/// Maven Central, the super-POM repository.
pub fn central() -> Repository {
    Repository {
        id: "central".into(),
        url: "https://repo.maven.apache.org/maven2".into(),
    }
}

pub fn default_repositories() -> Vec<Repository> {
    vec![central()]
}

/// URLs that already failed in this session (Maven's `.lastUpdated`
/// bookkeeping, kept in memory).
static FAILED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Whether network downloads are disabled for this process
/// (`JDTLS_RUST_OFFLINE=1`, used by hermetic test runs).
fn globally_offline() -> bool {
    std::env::var("JDTLS_RUST_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Relative repository path of an artifact.
pub fn artifact_path(g: &str, a: &str, v: &str, classifier: Option<&str>, ext: &str) -> PathBuf {
    let mut p = PathBuf::new();
    for seg in g.split('.') {
        p.push(seg);
    }
    p.push(a);
    p.push(v);
    let file = match classifier {
        Some(c) if !c.is_empty() => format!("{a}-{v}-{c}.{ext}"),
        _ => format!("{a}-{v}.{ext}"),
    };
    p.push(file);
    p
}

/// Download an artifact into `local_repo` unless present; returns its path.
pub fn fetch(
    local_repo: &Path,
    repos: &[Repository],
    g: &str,
    a: &str,
    v: &str,
    classifier: Option<&str>,
    ext: &str,
) -> Option<PathBuf> {
    if g.is_empty()
        || a.is_empty()
        || v.is_empty()
        || v.contains("${")
        || v.starts_with('[')
        || v.starts_with('(')
    {
        return None;
    }
    let rel = artifact_path(g, a, v, classifier, ext);
    let target = local_repo.join(&rel);
    if target.is_file() {
        return Some(target);
    }
    if globally_offline() {
        return None;
    }
    let rel_url = rel.to_string_lossy().replace('\\', "/");
    for repo in repos {
        let url = format!("{}/{}", repo.url.trim_end_matches('/'), rel_url);
        {
            let mut failed = FAILED.lock().unwrap();
            if failed.get_or_insert_with(HashSet::new).contains(&url) {
                continue;
            }
        }
        match get(&url) {
            Some(bytes) => {
                if write_atomically(&target, &bytes).is_ok() {
                    record_origin(&target, &repo.id);
                    return Some(target);
                }
            }
            None => {
                FAILED
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashSet::new)
                    .insert(url);
            }
        }
    }
    None
}

static AGENT: once_cell::sync::Lazy<ureq::Agent> = once_cell::sync::Lazy::new(|| {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
});

fn get(url: &str) -> Option<Vec<u8>> {
    let resp = AGENT.get(url).call().ok()?;
    if resp.status() != 200 {
        return None;
    }
    let mut bytes = Vec::new();
    const MAX_BYTES: u64 = 512 * 1024 * 1024;
    resp.into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_BYTES).then_some(bytes)
}

fn write_atomically(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(bytes)?;
    file.persist(target).map_err(|e| e.error)?;
    Ok(())
}

/// `_remote.repositories`: `<file>>repoId=` lines.
static ORIGIN_LOCK: Mutex<()> = Mutex::new(());

fn record_origin(file: &Path, repo_id: &str) {
    let _guard = ORIGIN_LOCK.lock().unwrap();
    let Some(dir) = file.parent() else { return };
    let path = dir.join("_remote.repositories");
    let name = file.file_name().unwrap().to_string_lossy();
    let line = format!("{name}>{repo_id}=");
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        "#NOTE: This is a Maven Resolver internal implementation file, its format can be changed without prior notice.\n".to_owned()
    });
    if text.lines().any(|l| l == line) {
        return;
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
    let _ = write_atomically(&path, text.as_bytes());
}
