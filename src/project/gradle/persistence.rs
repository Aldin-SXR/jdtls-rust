//! Buildship's persisted project models: a build whose scripts did not change
//! since the models were saved need not be synchronized again.

use crate::project::metadata::{self, MetadataSettings};
use crate::project::{Project, Workspace, GRADLE_NATURE};
use std::path::Path;
use std::sync::OnceLock;

fn model_file(state: &Path, project: &Project) -> std::path::PathBuf {
    state.join("project-preferences").join(&project.name)
}

fn is_gradle_file(name: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^.*\.gradle(\.kts)?$").unwrap())
        .is_match(name)
}

/// `GradleBuildSupport.saveModels()`: persist the models of the Gradle projects.
pub fn save_models(ws: &Workspace, state: &Path) {
    for p in ws.projects.iter().filter(|p| p.has_nature(GRADLE_NATURE)) {
        let file = model_file(state, p);
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&file, p.name.as_bytes());
    }
}

/// `GradleProjectImporter.shouldSynchronize(location)`.
pub fn should_synchronize(
    ws: &Workspace,
    location: &Path,
    state: &Path,
    metadata: &MetadataSettings,
) -> bool {
    for p in ws.projects.iter().filter(|p| p.has_nature(GRADLE_NATURE)) {
        if p.location != location {
            continue;
        }
        return check_persistence(p, state, metadata);
    }
    tracing::info!("No previous Gradle project at {}, it must be synchronized", location.display());
    true
}

fn check_persistence(project: &Project, state: &Path, metadata: &MetadataSettings) -> bool {
    if project.is_java() && !metadata.location(project, metadata::CLASSPATH_FILE).exists() {
        return true;
    }
    let Ok(persisted) = std::fs::metadata(model_file(state, project)).and_then(|m| m.modified())
    else {
        return true;
    };
    let Ok(entries) = std::fs::read_dir(&project.location) else {
        return true;
    };
    let modified_since = entries.flatten().any(|e| {
        is_gradle_file(&e.file_name().to_string_lossy())
            && e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > persisted)
    });
    if modified_since {
        tracing::info!(
            "{} was modified since last time the workspace was opened, must be synchronized",
            project.name
        );
    }
    modified_since
}
