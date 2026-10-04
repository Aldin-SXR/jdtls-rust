//! `textDocument/completion` and `completionItem/resolve`: a Rust port of
//! jdt.ls `CompletionHandler`, `CompletionProposalRequestor`,
//! `CompletionResolveHandler` and the proposal conversion classes.
//!
//! Proposals come from JDT's own `CompletionEngine`, run in the bridge
//! without the Java model (`CodeAssistService`); everything that turns them
//! into LSP items happens here.

pub mod accessors;
pub mod description;
pub mod doc;
pub mod guesser;
pub mod handler;
pub mod imports;
pub mod item;
pub mod javadoc_proposal;
pub mod javadoc_text;
pub mod naming;
pub mod prefs;
pub mod proposal;
pub mod replacement;
pub mod requestor;
pub mod resolve;
pub mod service;
pub mod signature;
pub mod snippets;
pub mod sort_text;

use std::sync::{Arc, Mutex, OnceLock};

/// Java `String.compareTo(...) < 0` of two version strings (jdt.ls
/// `isVersionLessThan`, `JavaModelUtil.isVersionLessThan` for 1.x/N).
pub fn version_less_than(v1: &str, v2: &str) -> bool {
    fn norm(v: &str) -> (u32, u32) {
        let v = v.trim();
        if let Some(rest) = v.strip_prefix("1.") {
            return (1, rest.parse().unwrap_or(0));
        }
        let major: u32 = v.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        if major <= 8 {
            (1, major)
        } else {
            (major, 0)
        }
    }
    norm(v1) < norm(v2)
}

/// Handles the completion feature needs from the server.
#[derive(Clone)]
pub struct Env {
    pub dispatcher: Arc<crate::analysis::dispatcher::Dispatcher>,
    pub store: Arc<crate::document_store::DocumentStore>,
    pub client: tower_lsp::Client,
    pub config: Arc<tokio::sync::RwLock<crate::config::Config>>,
}

static ENV: OnceLock<Mutex<Option<Env>>> = OnceLock::new();

/// Called once by the server when it is created.
pub fn set_env(env: Env) {
    *ENV.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = Some(env);
}

pub fn env() -> Option<Env> {
    ENV.get()?.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub use service::CompletionService;
