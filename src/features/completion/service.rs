//! Routes `textDocument/completion` and `completionItem/resolve` to this
//! module.  lsp-types 0.94 has no LSP 3.17 `itemDefaults`/`textEditText`,
//! so these two requests are answered here with raw JSON instead of
//! through tower-lsp's typed `LanguageServer` methods.

use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::task::{Context as TaskContext, Poll};
use tower_lsp::jsonrpc::{Request, Response};
use tower_service::Service;

pub struct CompletionService<S> {
    inner: S,
    initialized: Arc<AtomicBool>,
}

impl<S> CompletionService<S> {
    pub fn new(inner: S) -> Self {
        CompletionService {
            inner,
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl<S> Service<Request> for CompletionService<S>
where
    S: Service<Request, Response = Option<Response>> + Send,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Option<Response>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let method = req.method().to_owned();
        if method == "initialize" || method == "shutdown" {
            let initialized = Arc::clone(&self.initialized);
            let future = self.inner.call(req);
            return Box::pin(async move {
                let response = future.await?;
                if response.as_ref().is_some_and(Response::is_ok) {
                    initialized.store(method == "initialize", Ordering::SeqCst);
                }
                Ok(response)
            });
        }
        if self.initialized.load(Ordering::SeqCst)
            && (method == "textDocument/completion" || method == "completionItem/resolve")
        {
            if let Some(env) = super::env() {
                let fallback = if method == "textDocument/completion" {
                    Some(self.inner.call(req.clone()))
                } else {
                    None
                };
                let (_, id, params) = req.into_parts();
                return Box::pin(async move {
                    if let Some(fallback) = fallback {
                        if !env.dispatcher.is_ecj_ready().await {
                            return fallback.await;
                        }
                    }
                    let params = params.unwrap_or(Value::Null);
                    let result = if method == "textDocument/completion" {
                        Ok(completion(&env, params).await)
                    } else {
                        resolve(&env, params).await
                    };
                    Ok(id.map(|id| match result {
                        Ok(value) => Response::from_ok(id, value),
                        Err(error) => Response::from_error(id, error),
                    }))
                });
            }
        }
        Box::pin(self.inner.call(req))
    }
}

async fn completion(env: &super::Env, params: Value) -> Value {
    let uri = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(|u| url::Url::parse(u).ok());
    let position: Option<tower_lsp::lsp_types::Position> = params
        .get("position")
        .and_then(|p| serde_json::from_value(p.clone()).ok());
    let trigger_char = params
        .pointer("/context/triggerCharacter")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let trigger_kind = params
        .pointer("/context/triggerKind")
        .and_then(Value::as_i64);
    let (Some(uri), Some(position)) = (uri, position) else {
        return serde_json::to_value(super::item::List::default()).unwrap_or(Value::Null);
    };
    let list =
        super::handler::completion(env, &uri, position, trigger_char.as_deref(), trigger_kind)
            .await;
    serde_json::to_value(list).unwrap_or(Value::Null)
}

async fn resolve(env: &super::Env, params: Value) -> tower_lsp::jsonrpc::Result<Value> {
    let item: super::item::Item = match serde_json::from_value(params.clone()) {
        Ok(i) => i,
        Err(_) => return Ok(params),
    };
    let resolved = super::resolve::resolve(env, item).await?;
    Ok(serde_json::to_value(resolved).unwrap_or(params))
}
