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

/// Direct agent (no proxy), used for `NO_PROXY` hosts and when no proxy is set.
static DIRECT: once_cell::sync::Lazy<ureq::Agent> = once_cell::sync::Lazy::new(|| agent(None));

/// Agent through `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`, like Maven's proxy settings
/// (and the JVM `https.proxyHost` jdt.ls inherits).
static PROXIED: once_cell::sync::Lazy<Option<ureq::Agent>> = once_cell::sync::Lazy::new(|| {
    let url = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))?;
    let proxy = ureq::Proxy::new(&url).ok()?;
    Some(agent(Some(proxy)))
});

fn agent(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(120));
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }
    if let Some(tls) = tls_config() {
        builder = builder.tls_config(tls);
    }
    builder.build()
}

/// The bundled web PKI roots plus any certificates in `SSL_CERT_FILE` (e.g. a corporate
/// or proxy CA), so downloads work wherever the JVM's trust store would.
fn tls_config() -> Option<std::sync::Arc<rustls::ClientConfig>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::CertificateDer;
    let file = std::env::var_os("SSL_CERT_FILE")?;
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    for cert in CertificateDer::pem_file_iter(&file).ok()?.flatten() {
        let _ = roots.add(cert);
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Some(std::sync::Arc::new(config))
}

/// Whether `NO_PROXY` exempts `host` (exact or domain-suffix names, `*`, IPs and CIDRs).
fn no_proxy(host: &str) -> bool {
    let list = ["NO_PROXY", "no_proxy"].iter().find_map(|k| std::env::var(k).ok()).unwrap_or_default();
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    let ip: Option<std::net::IpAddr> = host.parse().ok();
    list.split(',').map(str::trim).filter(|e| !e.is_empty()).any(|entry| {
        let entry = entry.to_ascii_lowercase();
        if entry == "*" {
            return true;
        }
        if let (Some(ip), Some((net, bits))) = (ip, entry.split_once('/')) {
            return cidr_contains(net, bits, ip);
        }
        let name = entry.trim_start_matches("*.").trim_start_matches('.');
        host == name || host.ends_with(&format!(".{name}"))
    })
}

fn cidr_contains(net: &str, bits: &str, ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    let (Ok(net), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(i)) if bits <= 32 => {
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            u32::from(n) & mask == u32::from(i) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(i)) if bits <= 128 => {
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits) };
            u128::from(n) & mask == u128::from(i) & mask
        }
        _ => false,
    }
}

fn agent_for(url: &str) -> &'static ureq::Agent {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned));
    match (&*PROXIED, host) {
        (Some(proxied), Some(host)) if !no_proxy(&host) => proxied,
        _ => &DIRECT,
    }
}

fn get(url: &str) -> Option<Vec<u8>> {
    let resp = agent_for(url).get(url).call().ok()?;
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
