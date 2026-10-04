mod common;
use common::jdtls::*;
use common::projects::*;
use serde_json::json;

#[test]
fn probe() {
    let mut ws = Workspace::new();
    let spec = std::env::var("PROBE").unwrap_or_else(|_| "maven/salut".into());
    let trigger = std::env::var("PROBE_TRIGGER").ok();
    for p in spec.split(',') {
        if let Some(t) = &trigger {
            ws.copy_and_import_folder(p, Some(t));
        } else {
            ws.import_projects(&[p]);
        }
    }
    if let Ok(s) = std::env::var("PROBE_SETTINGS") {
        ws.settings = serde_json::from_str(&s).unwrap();
    }
    if std::env::var("PROBE_PROGRESS").is_ok() {
        ws.init_options["extendedClientCapabilities"]["progressReportProvider"] = json!(true);
    }
    let all = ws.all_projects(true);
    if let Ok(f) = std::env::var("PROBE_KEYS_FILE") {
        let keys: Vec<String> = std::fs::read_to_string(f).unwrap().lines().map(str::to_owned).collect();
        for uri in &all {
            let v = ws.execute("java.project.getSettings", vec![json!(uri), json!(keys)]);
            println!("KEYS {uri} {}", serde_json::to_string(&v).unwrap());
        }
        return;
    }
    eprintln!("ALL(nonjava) = {all:#?}");
    eprintln!("ALL(java) = {:#?}", ws.all_projects(false));
    let keys = [NATURE_IDS, VM_LOCATION, SOURCE_PATHS, OUTPUT_PATH, CLASSPATH_ENTRIES, REFERENCED_LIBRARIES,
        "org.eclipse.jdt.core.compiler.compliance", "org.eclipse.jdt.core.compiler.source",
        "org.eclipse.jdt.core.compiler.codegen.targetPlatform", "org.eclipse.jdt.core.compiler.release",
        "org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures", "org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures"];
    for uri in &all {
        eprintln!("SETTINGS {uri} = {}", serde_json::to_string_pretty(&ws.try_execute("java.project.getSettings", vec![json!(uri), json!(keys)])).unwrap());
        if let Ok(extra) = std::env::var("PROBE_CP") {
            let _ = extra;
            eprintln!("CP runtime {uri} = {:?}", ws.try_execute("java.project.getClasspaths", vec![json!(uri), json!(json!({"scope":"runtime"}).to_string())]));
            eprintln!("CP test {uri} = {:?}", ws.try_execute("java.project.getClasspaths", vec![json!(uri), json!(json!({"scope":"test"}).to_string())]));
        }
    }
    eprintln!("SOURCEPATHS = {}", serde_json::to_string_pretty(&ws.list_source_paths()).unwrap());
    eprintln!("BUILD = {}", ws.build_workspace(false));
    for (uri, d) in ws.published_diagnostics() {
        eprintln!("DIAG {uri}: {}", serde_json::to_string(&d).unwrap());
    }
    eprintln!("WATCHERS = {:#?}", ws.watcher_glob_patterns());
    if std::env::var("PROBE_RAW").is_ok() {
        for r in ws.server_requests("client/registerCapability") {
            eprintln!("REG {}", r);
        }
    }
    for n in &ws.client().notifications {
        if n["method"] == "language/status" || n["method"] == "language/progressReport" || n["method"] == "window/logMessage" && std::env::var("PROBE_LOG").is_ok() {
            eprintln!("NOTE {}", n);
        }
    }
}
