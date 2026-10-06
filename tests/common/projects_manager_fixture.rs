//! Direct manager access, including the actual Eclipse API in oracle mode.
use crate::common::jdtls::{is_oracle, Workspace};
use crate::project::{self, resource_filters::ResourceFilters, ImportSettings};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    if is_oracle() {
        static PRODUCT: OnceLock<PathBuf> = OnceLock::new();
        ws.oracle_home = Some(
            PRODUCT
                .get_or_init(|| {
                    let output = Command::new("python3")
                        .arg(
                            Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("scripts/prepare-oracle-fixture.py"),
                        )
                        .arg("projects-manager")
                        .output()
                        .expect("build project-manager oracle fixture");
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
                })
                .clone(),
        );
    }
    ws
}

pub fn initialize_empty(ws: &mut Workspace) -> Vec<Value> {
    if is_oracle() {
        return command(ws, json!({"api":"initializeEmpty"}));
    }
    let mut settings = ImportSettings::jdtls_defaults();
    settings.data_dir = Some(ws.server_workspace_dir());
    let model = project::Workspace::import(&[], &settings);
    model.ensure_default_project().unwrap();
    model
        .all_projects()
        .iter()
        .map(|p| {
            json!({
                "name":p.name, "location":p.location, "exists":p.location.exists(),
                "isDefault": model.default_project.as_ref() == Some(&p.location)
            })
        })
        .collect()
}

pub fn filters(
    ws: &mut Workspace,
    project_name: &str,
    paths: &[&str],
    operations: Vec<Value>,
) -> Vec<Value> {
    if is_oracle() {
        return command(
            ws,
            json!({"api":"filters", "project":project_name, "paths":paths, "operations":operations}),
        );
    }
    let root = ws.project_root(project_name);
    let settings = ImportSettings::jdtls_defaults();
    let mut model = project::Workspace::import(&[root.clone()], &settings);
    assert!(model.project(project_name).is_some_and(|p| p.is_java()));
    let mut filters = ResourceFilters::jdtls_default();
    let mut results = Vec::new();
    for operation in operations {
        if let Some(patterns) = operation.get("patterns") {
            let values: Option<Vec<String>> = serde_json::from_value(patterns.clone()).unwrap();
            filters = ResourceFilters::new(values.as_deref());
            model.configure_filters(&filters);
        }
        let project = model.project(project_name).unwrap();
        results.push(json!({"patterns":filters.patterns(),
            "filtered":paths.iter().map(|p| project.is_filtered(&root.join(p.trim_start_matches('/')))).collect::<Vec<_>>()
        }));
    }
    results
}

fn command(ws: &mut Workspace, input: Value) -> Vec<Value> {
    ws.request(
        "workspace/executeCommand",
        json!({"command":"jdtls.test.projectsManager", "arguments":[input]}),
    )
    .as_array()
    .expect("project-manager test results")
    .clone()
}
