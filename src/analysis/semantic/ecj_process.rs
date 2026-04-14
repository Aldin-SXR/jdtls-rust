//! Manages the long-lived ecj-bridge Java subprocess.
//! The bridge communicates via newline-delimited JSON on stdin/stdout.

use super::protocol::{BridgeRequest, BridgeResponse};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, warn};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A pending request waiting for a response from the bridge.
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<BridgeResponse>>>>;

/// Handle to the ecj-bridge subprocess.
/// All send operations are serialized through an internal mutex so this
/// can be cheaply cloned and shared across async tasks.
#[derive(Clone)]
pub struct EcjProcess {
    inner: Arc<Mutex<EcjInner>>,
}

struct EcjInner {
    child: Child,
    writer: BufWriter<ChildStdin>,
    pending: PendingMap,
}

impl Drop for EcjInner {
    fn drop(&mut self) {
        // Kill the bridge process if it is still running when we are dropped.
        let _ = self.child.kill();
    }
}

impl EcjProcess {
    /// Spawn the ecj-bridge process using the given JAR path and java binary.
    pub async fn spawn(jar_path: &Path, java_binary: &str) -> Result<Self> {
        info!("Spawning ecj-bridge: {} -jar {}", java_binary, jar_path.display());

        let mut child = Command::new(java_binary)
            .args([
                "-jar",
                jar_path.to_str().context("JAR path not valid UTF-8")?,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // bridge logs go to stderr → our stderr
            .spawn()
            .context("failed to spawn ecj-bridge; is java in PATH?")?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        // Spawn a background thread to read responses from the bridge.
        // We use std::thread here because BufReader::lines() blocks.
        std::thread::spawn(move || {
            reader_loop(stdout, pending_clone);
        });

        Ok(Self {
            inner: Arc::new(Mutex::new(EcjInner {
                child,
                writer: BufWriter::new(stdin),
                pending,
            })),
        })
    }

    /// Send a request to the bridge and await its response.
    pub async fn send(&self, req: BridgeRequest) -> Result<BridgeResponse> {
        let id = match &req {
            BridgeRequest::Compile { id, .. }
            | BridgeRequest::Complete { id, .. }
            | BridgeRequest::Hover { id, .. }
            | BridgeRequest::Navigate { id, .. }
            | BridgeRequest::FindReferences { id, .. }
            | BridgeRequest::CodeAction { id, .. }
            | BridgeRequest::SignatureHelp { id, .. }
            | BridgeRequest::Rename { id, .. }
            | BridgeRequest::OrganizeImports { id, .. }
            | BridgeRequest::Format { id, .. }
            | BridgeRequest::InlayHints { id, .. }
            | BridgeRequest::CodeLens { id, .. }
            | BridgeRequest::Shutdown { id } => *id,
        };

        let (tx, rx) = oneshot::channel();

        {
            let mut inner = self.inner.lock().await;
            inner.pending.lock().await.insert(id, tx);

            let line = serde_json::to_string(&req).context("serialize bridge request")?;
            debug!(id, "→ ecj-bridge: {}", &line[..line.len().min(200)]);
            if let Err(err) = inner.writer.write_all(line.as_bytes())
                .and_then(|_| inner.writer.write_all(b"\n"))
                .and_then(|_| inner.writer.flush())
            {
                inner.pending.lock().await.remove(&id);
                return Err(err.into());
            }
        }

        rx.await.map_err(|_| anyhow!("ecj-bridge process died before responding to id={id}"))
    }

    /// Gracefully shut down the bridge.
    pub async fn shutdown(&self) {
        let id = next_id();
        let _ = self.send(BridgeRequest::Shutdown { id }).await;
    }
}

fn reader_loop(stdout: ChildStdout, pending: PendingMap) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        match line {
            Err(e) => {
                error!("ecj-bridge stdout read error: {e}");
                break;
            }
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => {
                debug!("← ecj-bridge: {}", &line[..line.len().min(200)]);
                match serde_json::from_str::<BridgeResponse>(&line) {
                    Err(e) => warn!("Failed to parse ecj-bridge response: {e}\n{line}"),
                    Ok(resp) => {
                        let id = resp.id();
                        // tokio::sync::Mutex can't be awaited from a std thread.
                        // Use try_lock in a spin — pending map is rarely contended.
                        loop {
                            if let Ok(mut map) = pending.try_lock() {
                                if let Some(tx) = map.remove(&id) {
                                    let _ = tx.send(resp);
                                } else {
                                    warn!("ecj-bridge response id={id} has no pending waiter");
                                }
                                break;
                            }
                            std::thread::yield_now();
                        }
                    }
                }
            }
        }
    }
    loop {
        if let Ok(mut map) = pending.try_lock() {
            map.clear();
            break;
        }
        std::thread::yield_now();
    }
    warn!("ecj-bridge stdout closed");
}
