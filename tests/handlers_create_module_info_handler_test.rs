//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CreateModuleInfoHandlerTest`.
//!
//! `CreateModuleInfoHandler.createModuleInfo(uri)` is the
//! `java.project.createModuleInfo` delegate command.

mod common;

use common::jdtls::*;
use common::projects::*;
use serde_json::json;
use tower_lsp::lsp_types::Url;

#[test]
fn test_create_module_info() {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/modular-project2"]);
    ws.use_upstream_maven_test_jdk("modular-project2", "11");
    let project = ws.dir.join("maven/modular-project2");

    let module_info_uri = ws.execute("java.project.createModuleInfo", vec![json!(dir_uri(&project))]);

    let module_info_uri = module_info_uri.as_str().expect("module-info.java uri");
    let file = Url::parse(module_info_uri).unwrap().to_file_path().unwrap();
    let content = std::fs::read_to_string(file).unwrap();
    assert!(content.contains("exports com.example;"));
    assert!(content.contains("requires xml.apis;"));
}
