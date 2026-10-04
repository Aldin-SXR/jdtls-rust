mod analysis;
mod classfile;
mod config;
mod document_store;
mod embedded_jar;
mod features;
mod handlers;
mod lenient_uri;
mod index;
mod project;
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

    let (service, socket) = LspService::build(JavaLanguageServer::new)
        .custom_method("java/classFileContents", JavaLanguageServer::class_file_contents)
        .custom_method("java/searchSymbols", JavaLanguageServer::search_symbols)
        .finish();
    Server::new(stdin, stdout, socket).serve(lenient_uri::LenientUri::new(service)).await;
}
