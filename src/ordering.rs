//! lsp4j hands jdt.ls the client's messages in order: a request sent after
//! `didOpen`/`didChange` sees the updated document, and a request sent after
//! `didChangeWatchedFiles` sees the refreshed workspace.  tower-lsp runs
//! handlers concurrently, so this wrapper makes every message wait until the
//! document- and workspace-synchronization notifications received before it
//! have been handled.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::sync::watch;
use tower_lsp::jsonrpc::{Request, Response};
use tower_service::Service;

/// Notifications whose effects later messages depend on.
fn is_sync_notification(method: &str) -> bool {
    matches!(
        method,
        "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didClose"
            | "textDocument/didSave"
            | "workspace/didChangeWatchedFiles"
            | "workspace/didChangeConfiguration"
            | "workspace/didChangeWorkspaceFolders"
            | "workspace/didCreateFiles"
            | "workspace/didRenameFiles"
            | "workspace/didDeleteFiles"
    )
}

pub struct Ordered<S> {
    inner: S,
    /// Completion flags of the synchronization notifications still running.
    pending: Arc<Mutex<Vec<watch::Receiver<bool>>>>,
}

impl<S> Ordered<S> {
    pub fn new(inner: S) -> Self {
        Self { inner, pending: Arc::new(Mutex::new(Vec::new())) }
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S> Service<Request> for Ordered<S>
where
    S: Service<Request, Response = Option<Response>>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Option<Response>;
    type Error = S::Error;
    type Future = BoxFuture<Result<Option<Response>, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let sync = req.id().is_none() && is_sync_notification(req.method());
        let earlier: Vec<watch::Receiver<bool>> = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.retain(|rx| !*rx.borrow() && rx.has_changed().is_ok());
            pending.clone()
        };
        let done = if sync {
            let (tx, rx) = watch::channel(false);
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).push(rx);
            Some(tx)
        } else {
            None
        };
        let fut = self.inner.call(req);
        Box::pin(async move {
            for mut rx in earlier {
                while !*rx.borrow() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            }
            let result = fut.await;
            if let Some(tx) = done {
                let _ = tx.send(true);
            }
            result
        })
    }
}
