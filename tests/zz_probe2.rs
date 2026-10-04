mod common;
use common::jdtls::*;
use common::projects::*;
use serde_json::json;

#[test]
fn probe2() {
    let mut ws = Workspace::new();
    let project_folder = ws.dir.parent().unwrap().join("dynamicLibDetection1");
    std::fs::create_dir_all(&project_folder).unwrap();
    copy_dir(&fixtures_dir().join("projects/eclipse/source-attachment/src"), &project_folder);
    let project_folder = canonical(&project_folder);
    ws.import_root_folder(&project_folder, Some("Test.java"));
    eprintln!("ALL {:?}", ws.all_projects(true));
    eprintln!("SETTINGS {}", ws.project_settings(&dir_uri(&project_folder), &[CLASSPATH_ENTRIES, OUTPUT_PATH]));
    for (u, d) in ws.published_diagnostics() {
        eprintln!("DIAG {u} {}", json!(d));
    }
    let uri = file_uri(&project_folder.join("Test.java"));
    ws.open(&uri);
    eprintln!("OPEN DIAG {:?}", ws.diagnostics(&uri));
    for (u, d) in ws.published_diagnostics() {
        eprintln!("DIAG2 {u} {}", json!(d));
    }
}
