//! Manages the connection to the shared ecj-bridge daemon process.
//!
//! The bridge runs as a singleton daemon and communicates over a Unix socket.
//! Multiple LSP server instances share one JVM by each connecting to the same
//! socket path.  The protocol is unchanged: newline-delimited JSON with numeric
//! request IDs for multiplexing.

use super::protocol::{BridgeRequest, BridgeResponse};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, warn};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A pending request waiting for a response from the bridge.
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<BridgeResponse>>>>;

/// Handle to the ecj-bridge daemon connection.
/// All send operations are serialized through an internal mutex so this
/// can be cheaply cloned and shared across async tasks.
#[derive(Clone)]
pub struct EcjProcess {
    inner: Arc<Mutex<EcjInner>>,
}

struct EcjInner {
    writer: tokio::io::WriteHalf<UnixStream>,
    pending: PendingMap,
}

impl EcjProcess {
    /// Connect to an already-running bridge, or start one if the socket does
    /// not exist yet.  Safe to call from multiple Rust instances concurrently:
    /// if two instances both fail to connect and both spawn a Java process,
    /// one Java process will fail to bind the socket and exit, and both Rust
    /// instances will connect to whichever Java process won the bind race.
    pub async fn ensure_started(
        jar_path: &Path,
        java_binary: &str,
        socket_path: &Path,
    ) -> Result<Self> {
        // 1. Fast path — bridge is already running.
        if let Ok(stream) = UnixStream::connect(socket_path).await {
            info!("Connected to existing ecj-bridge at {}", socket_path.display());
            return Ok(Self::from_stream(stream));
        }

        // 2. Spawn the bridge daemon.  We deliberately do NOT hold a Child
        //    handle — the bridge runs independently and survives this process.
        info!(
            "Starting ecj-bridge daemon: {} -jar {} --socket {}",
            java_binary,
            jar_path.display(),
            socket_path.display()
        );
        let mut child = Command::new(java_binary)
            .args([
                "-jar",
                jar_path.to_str().context("JAR path not valid UTF-8")?,
                "--socket",
                socket_path.to_str().context("socket path not valid UTF-8")?,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!("failed to spawn ecj-bridge with java binary '{java_binary}'")
            })?;

        // Reap the child in a background thread so it never becomes a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        // 3. Poll until the socket is ready (up to 30 s).
        for attempt in 0..300 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(stream) = UnixStream::connect(socket_path).await {
                info!(
                    "ecj-bridge ready after ~{}ms",
                    (attempt + 1) * 100
                );
                return Ok(Self::from_stream(stream));
            }
        }

        Err(anyhow!(
            "ecj-bridge did not create socket at '{}' within 30 s",
            socket_path.display()
        ))
    }

    fn from_stream(stream: UnixStream) -> Self {
        let (read_half, write_half) = tokio::io::split(stream);
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        tokio::spawn(async move {
            reader_loop(read_half, pending_clone).await;
        });

        Self {
            inner: Arc::new(Mutex::new(EcjInner {
                writer: write_half,
                pending,
            })),
        }
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
            | BridgeRequest::TypeHierarchyPrepare { id, .. }
            | BridgeRequest::TypeHierarchySupertypes { id, .. }
            | BridgeRequest::TypeHierarchySubtypes { id, .. }
            | BridgeRequest::CallHierarchyPrepare { id, .. }
            | BridgeRequest::CallHierarchyIncoming { id, .. }
            | BridgeRequest::CallHierarchyOutgoing { id, .. }
            | BridgeRequest::Shutdown { id } => *id,
        };

        let (tx, rx) = oneshot::channel();

        {
            let mut inner = self.inner.lock().await;
            inner.pending.lock().await.insert(id, tx);

            let line = serde_json::to_string(&req).context("serialize bridge request")?;
            debug!(id, "→ ecj-bridge: {}", &line[..line.len().min(200)]);

            let write_result = async {
                inner.writer.write_all(line.as_bytes()).await?;
                inner.writer.write_all(b"\n").await?;
                inner.writer.flush().await
            }
            .await;

            if let Err(err) = write_result {
                inner.pending.lock().await.remove(&id);
                return Err(err.into());
            }
        }

        rx.await
            .map_err(|_| anyhow!("ecj-bridge connection closed before responding to id={id}"))
    }

    /// Send a graceful shutdown to the bridge connection.
    /// The bridge daemon itself remains running for other connected clients.
    pub async fn shutdown(&self) {
        let id = next_id();
        let _ = self.send(BridgeRequest::Shutdown { id }).await;
    }
}

async fn reader_loop(
    read_half: tokio::io::ReadHalf<UnixStream>,
    pending: PendingMap,
) {
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF / connection closed
            Err(e) => {
                error!("ecj-bridge read error: {e}");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                debug!("← ecj-bridge: {}", &trimmed[..trimmed.len().min(200)]);
                match serde_json::from_str::<BridgeResponse>(trimmed) {
                    Err(e) => warn!("Failed to parse ecj-bridge response: {e}\n{trimmed}"),
                    Ok(resp) => {
                        let id = resp.id();
                        if let Some(tx) = pending.lock().await.remove(&id) {
                            let _ = tx.send(resp);
                        } else {
                            warn!("ecj-bridge response id={id} has no pending waiter");
                        }
                    }
                }
            }
        }
    }

    pending.lock().await.clear();
    warn!("ecj-bridge connection closed");
}
