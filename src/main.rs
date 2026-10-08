mod analysis;
mod classfile;
mod config;
mod correction;
mod document_store;
mod embedded_jar;
mod features;
mod handlers;
mod index;
mod javadoc;
mod lenient_uri;
mod ordering;
mod project;
mod refactoring;
mod rewrite;
mod semantic_ast;
mod server;

use server::JavaLanguageServer;
use tower_lsp::{LspService, Server};
use tracing_subscriber::{fmt, EnvFilter};

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
        .custom_method("java/organizeImports", JavaLanguageServer::organize_imports)
        .custom_method("java/cleanup", JavaLanguageServer::cleanup)
        .custom_method("java/getRefactorEdit", JavaLanguageServer::get_refactor_edit)
        .custom_method("java/inferSelection", JavaLanguageServer::infer_selection)
        .custom_method(
            "java/classFileContents",
            JavaLanguageServer::class_file_contents,
        )
        .custom_method(
            "java/resolveUnimplementedAccessors",
            JavaLanguageServer::resolve_unimplemented_accessors,
        )
        .custom_method(
            "java/generateAccessors",
            JavaLanguageServer::generate_accessors,
        )
        .custom_method(
            "java/checkConstructorsStatus",
            JavaLanguageServer::check_constructors_status,
        )
        .custom_method(
            "java/generateConstructors",
            JavaLanguageServer::generate_constructors,
        )
        .custom_method(
            "java/checkToStringStatus",
            JavaLanguageServer::check_to_string_status,
        )
        .custom_method(
            "java/generateToString",
            JavaLanguageServer::generate_to_string,
        )
        .custom_method(
            "java/checkHashCodeEqualsStatus",
            JavaLanguageServer::check_hash_code_equals_status,
        )
        .custom_method(
            "java/generateHashCodeEquals",
            JavaLanguageServer::generate_hash_code_equals,
        )
        .custom_method(
            "java/checkDelegateMethodsStatus",
            JavaLanguageServer::check_delegate_methods_status,
        )
        .custom_method(
            "java/generateDelegateMethods",
            JavaLanguageServer::generate_delegate_methods,
        )
        .custom_method(
            "java/listOverridableMethods",
            JavaLanguageServer::list_overridable_methods,
        )
        .custom_method(
            "java/addOverridableMethods",
            JavaLanguageServer::add_overridable_methods,
        )
        .custom_method("java/searchSymbols", JavaLanguageServer::search_symbols)
        .custom_method("java/buildWorkspace", JavaLanguageServer::build_workspace)
        .custom_method("java/buildProjects", JavaLanguageServer::build_projects)
        .custom_method(
            "java/projectConfigurationUpdate",
            JavaLanguageServer::project_configuration_update,
        )
        .custom_method(
            "java/projectConfigurationsUpdate",
            JavaLanguageServer::project_configurations_update,
        )
        .finish();
    Server::new(stdin, stdout, socket)
        .serve(ordering::Ordered::new(lenient_uri::LenientUri::new(
            features::init::InitializeResultRewrite::new(
                features::completion::CompletionService::new(service),
            ),
        )))
        .await;
}
