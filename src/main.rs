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

    // jdt.ls launcher arguments: `-data <workspace>`.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "-data") {
        if let Some(dir) = args.get(i + 1) {
            let _ = config::DATA_DIR.set(std::path::PathBuf::from(dir));
        }
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(JavaLanguageServer::new)
        .custom_method("java/classFileContents", JavaLanguageServer::class_file_contents)
        .custom_method("java/searchSymbols", JavaLanguageServer::search_symbols)
        .custom_method("java/buildWorkspace", JavaLanguageServer::build_workspace)
        .finish();
    Server::new(stdin, stdout, socket).serve(lenient_uri::LenientUri::new(service)).await;
}
