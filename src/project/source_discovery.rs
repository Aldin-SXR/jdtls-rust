//! `MavenSourceDownloader.discoverSource`: identify the Maven artifact of a
//! library without attached sources and download its source and Javadoc jars.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactKey {
    pub group: String,
    pub artifact: String,
    pub version: String,
}

/// `MavenLocalRepositoryIdentifier`: a file `<repo>/<group path>/<artifact>/<version>/<file>`.
pub fn identify_in_local_repository(file: &Path, repository: &Path) -> Option<ArtifactKey> {
    if !file.is_file() {
        return None;
    }
    let relative = file.strip_prefix(repository).ok()?;
    let segments: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if segments.len() < 4 {
        return None;
    }
    let n = segments.len();
    Some(ArtifactKey {
        group: segments[..n - 3].join("."),
        artifact: segments[n - 3].clone(),
        version: segments[n - 2].clone(),
    })
}

/// The jars `discoverSource` requested within the last hour
/// (`downloadRequestsCache`).
static REQUESTS: Mutex<Option<HashMap<PathBuf, Instant>>> = Mutex::new(None);

/// Records a request for `jar`; false when one was made within the hour.
pub fn first_request(jar: &Path) -> bool {
    let mut guard = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    let requests = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    requests.retain(|_, at| now.duration_since(*at) < Duration::from_secs(3600));
    requests.insert(jar.to_path_buf(), now).is_none()
}

/// Finished downloads not yet attached: jar -> (sources, javadoc).
static COMPLETED: Mutex<Vec<(PathBuf, Option<PathBuf>, Option<PathBuf>)>> = Mutex::new(Vec::new());

pub fn complete(jar: PathBuf, sources: Option<PathBuf>, javadoc: Option<PathBuf>) {
    COMPLETED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((jar, sources, javadoc));
}

pub fn take_completed() -> Vec<(PathBuf, Option<PathBuf>, Option<PathBuf>)> {
    std::mem::take(&mut *COMPLETED.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_local_repository_files() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("org/apache/commons/commons-lang3/3.9/commons-lang3-3.9.jar");
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, b"").unwrap();
        assert_eq!(
            Some(ArtifactKey {
                group: "org.apache.commons".into(),
                artifact: "commons-lang3".into(),
                version: "3.9".into()
            }),
            identify_in_local_repository(&jar, dir.path())
        );
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(None, identify_in_local_repository(outside.path(), dir.path()));
    }
}
