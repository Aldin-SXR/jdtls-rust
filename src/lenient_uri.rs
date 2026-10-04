//! lsp4j accepts any string as a document URI, so jdt.ls answers requests
//! like `textDocument/definition` on `"/foo/bar"` with an empty result
//! instead of an "invalid params" error.  tower-lsp deserializes URIs into
//! `Url`, which rejects such strings; this service wrapper rewrites a
//! scheme-less absolute path in `params.textDocument.uri` into a `file:` URI
//! before dispatch, so the handlers see a document that simply doesn't exist.

use std::task::{Context, Poll};

use serde_json::Value;
use tower_lsp::jsonrpc::Request;
use tower_service::Service;

pub struct LenientUri<S> {
    inner: S,
}

impl<S> LenientUri<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

fn fix_uri(params: &mut Value) -> bool {
    let Some(uri) = params.get_mut("textDocument").and_then(|t| t.get_mut("uri")) else { return false };
    let Some(s) = uri.as_str() else { return false };
    if url::Url::parse(s).is_ok() || !s.starts_with('/') {
        return false;
    }
    match url::Url::from_file_path(s) {
        Ok(u) => {
            *uri = Value::String(u.to_string());
            true
        }
        Err(_) => false,
    }
}

impl<S> Service<Request> for LenientUri<S>
where
    S: Service<Request>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let needs_fix = req.method().starts_with("textDocument/")
            && req.params().and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(Value::as_str).is_some_and(|s| {
                url::Url::parse(s).is_err() && s.starts_with('/')
            });
        if !needs_fix {
            return self.inner.call(req);
        }
        let (method, id, params) = req.into_parts();
        let mut params = params.unwrap_or(Value::Null);
        fix_uri(&mut params);
        let mut b = Request::build(method).params(params);
        if let Some(id) = id {
            b = b.id(id);
        }
        self.inner.call(b.finish())
    }
}
