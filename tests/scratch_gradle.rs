mod common;

use common::jdtls::*;
use common::maven::*;
use common::projects::*;

#[test]
fn scratch() {
    let names: Vec<String> = std::env::var("SCRATCH_PROJECTS")
        .unwrap_or("gradle/simple-gradle".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let mut ws = workspace();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    ws.import_projects(&refs);
    ws.wait_for_background_jobs();
    let base = canonical(&ws.dir).display().to_string();
    let rel = |s: String| s.replace(&base, "<WS>");
    for p in ws.project_locations(true) {
        eprintln!("##### {}", rel(p.display().to_string()));
        let n = ws.natures(&p);
        eprintln!("natures {:?}", n);
        if n.iter().any(|x| x == JAVA_NATURE) {
            for e in ws.classpath_entries(&p) {
                eprintln!("cpe {}", rel(e.to_string()));
            }
            eprintln!("level {}", ws.java_source_level(&p));
            eprintln!("srcpaths {:?}", ws.source_paths(&p).into_iter().map(&rel).collect::<Vec<_>>());
        }
    }
    eprintln!("SRCPATHS {}", rel(ws.list_source_paths().to_string()));
}
