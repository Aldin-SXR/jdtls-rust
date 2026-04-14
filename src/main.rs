mod analysis;
mod config;
mod document_store;
mod embedded_jar;
mod handlers;
mod server;

use server::JavaLanguageServer;
use tower_lsp::{LspService, Server};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    // Log to stderr so stdout stays clean for LSP JSON-RPC
    fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_env("JDTLS_LOG"))
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(JavaLanguageServer::new).finish();
    Server::new(stdin, stdout, socket).serve(service).await;
}
