//! The Gradle Tooling API models Buildship reads when it imports a build.
//!
//! The Tooling API is a Java library, so a small Java program
//! (`GradleModelDump.java`, run with the source launcher against the Tooling
//! API jar of a Gradle distribution) fetches the models and prints them as
//! JSON; everything else happens in Rust.

use super::config::{BuildConfiguration, GradleDistribution, GradleSettings};
use super::util::GradleVersion;
use serde_json::Value;
use std::collections::BTreeMap;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HELPER_SOURCE: &str = include_str!("GradleModelDump.java");

#[derive(Debug, Clone, Default)]
pub struct ModelSource {
    pub path: String,
    pub dir: PathBuf,
    pub output: Option<String>,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelDependency {
    pub file: PathBuf,
    pub source: Option<PathBuf>,
    pub exported: bool,
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelProjectDependency {
    pub path: String,
    pub exported: bool,
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelJavaSettings {
    pub source: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelProject {
    pub name: String,
    pub dir: PathBuf,
    pub gradle_path: String,
    pub output: Option<String>,
    pub sources: Vec<ModelSource>,
    pub classpath: Vec<ModelDependency>,
    pub project_dependencies: Vec<ModelProjectDependency>,
    pub natures: Vec<String>,
    pub build_commands: Vec<String>,
    pub containers: Vec<String>,
    pub java: Option<ModelJavaSettings>,
    pub children: Vec<ModelProject>,
}

impl ModelProject {
    /// This project followed by all its descendants.
    pub fn flatten(&self) -> Vec<&ModelProject> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.flatten());
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct GradleModel {
    pub gradle_version: String,
    pub java_home: PathBuf,
    pub project: ModelProject,
}

#[derive(Debug, Clone)]
pub enum FetchError {
    /// Gradle could not be run at all (no JDK, no Gradle distribution).
    Unavailable(String),
    /// Gradle ran and reported a failure.
    Failed {
        message: String,
        causes: Vec<String>,
    },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Unavailable(m) => write!(f, "{m}"),
            FetchError::Failed { message, .. } => write!(f, "{message}"),
        }
    }
}

fn gradle_user_home(config: &BuildConfiguration) -> Option<PathBuf> {
    config
        .gradle_user_home
        .clone()
        .or_else(|| std::env::var_os("GRADLE_USER_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gradle")))
}

fn read_properties(path: &Path) -> BTreeMap<String, String> {
    crate::project::prefs::read_properties(path).unwrap_or_default()
}

/// The Gradle version a build asks for, when it can be told without running Gradle.
pub fn requested_version(config: &BuildConfiguration) -> Option<GradleVersion> {
    match &config.distribution {
        GradleDistribution::FixedVersion(v) => GradleVersion::version(v),
        GradleDistribution::Local(home) => home
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("gradle-"))
            .and_then(GradleVersion::version),
        GradleDistribution::Wrapper => {
            let props = read_properties(
                &config
                    .root_project_directory
                    .join(super::config::GRADLE_WRAPPER_PROPERTIES_DESCRIPTOR),
            );
            let url = props.get("distributionUrl")?;
            let file = url.rsplit('/').next()?;
            let rest = file.strip_prefix("gradle-")?;
            let version = rest
                .strip_suffix("-bin.zip")
                .or_else(|| rest.strip_suffix("-all.zip"))?;
            GradleVersion::version(version)
        }
    }
}

/// Tooling API libraries (the `lib` directory of a Gradle distribution).
fn tooling_libs(config: &BuildConfiguration) -> Option<PathBuf> {
    let has_api = |lib: &Path| {
        std::fs::read_dir(lib).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("gradle-tooling-api-")
            })
        })
    };
    if let GradleDistribution::Local(home) = &config.distribution {
        let lib = home.join("lib");
        if has_api(&lib) {
            return Some(lib);
        }
    }
    let mut homes: Vec<PathBuf> = [
        gradle_user_home(config),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gradle")),
    ]
    .into_iter()
    .flatten()
    .collect();
    homes.dedup();
    let mut found: Vec<(GradleVersion, PathBuf)> = Vec::new();
    for dists in homes.iter().map(|h| h.join("wrapper/dists")) {
        for dist in std::fs::read_dir(&dists).into_iter().flatten().flatten() {
            for hash in std::fs::read_dir(dist.path()).into_iter().flatten().flatten() {
                for home in std::fs::read_dir(hash.path()).into_iter().flatten().flatten() {
                    let name = home.file_name().to_string_lossy().into_owned();
                    let Some(version) =
                        name.strip_prefix("gradle-").and_then(GradleVersion::version)
                    else {
                        continue;
                    };
                    let lib = home.path().join("lib");
                    if has_api(&lib) {
                        found.push((version, lib));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let wanted = requested_version(config);
    wanted
        .and_then(|w| found.iter().find(|(v, _)| *v == w))
        .or_else(|| found.last())
        .map(|(_, lib)| lib.clone())
}

fn helper_path(settings: &GradleSettings) -> std::io::Result<PathBuf> {
    let dir = settings
        .scripts_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("jdtls-rust-gradle"))
        .join("model");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("GradleModelDump.java");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(HELPER_SOURCE) {
        std::fs::write(&file, HELPER_SOURCE)?;
    }
    Ok(file)
}

fn launcher(settings: &GradleSettings) -> PathBuf {
    for home in [
        settings.launcher_java.clone(),
        std::env::var_os("JAVA_HOME").map(PathBuf::from),
    ]
    .into_iter()
    .flatten()
    {
        let java = home.join("bin/java");
        if java.is_file() {
            return java;
        }
    }
    PathBuf::from("java")
}

/// Run the helper in `mode` and return its JSON output.
fn run_helper(config: &BuildConfiguration, settings: &GradleSettings, mode: &str) -> Result<String, FetchError> {
    let lib = tooling_libs(config).ok_or_else(|| {
        FetchError::Unavailable("no Gradle distribution with the Tooling API is installed".into())
    })?;
    let helper = helper_path(settings).map_err(|e| FetchError::Unavailable(e.to_string()))?;
    let work = tempfile::tempdir().map_err(|e| FetchError::Unavailable(e.to_string()))?;
    let out = work.path().join("model.json");
    let (dist, dist_value) = match &config.distribution {
        GradleDistribution::Wrapper => ("wrapper", String::new()),
        GradleDistribution::FixedVersion(v) => ("version", v.clone()),
        GradleDistribution::Local(p) => ("local", p.to_string_lossy().into_owned()),
    };
    let mut request = format!(
        "mode={mode}\ndir={}\nout={}\ndist={dist}\ndistvalue={dist_value}\njavahome={}\nuserhome={}\noffline={}\n",
        config.root_project_directory.display(),
        out.display(),
        config.java_home.as_deref().map(|h| h.display().to_string()).unwrap_or_default(),
        config.gradle_user_home.as_deref().map(|h| h.display().to_string()).unwrap_or_default(),
        if config.offline_mode { 1 } else { 0 },
    );
    for a in &config.jvm_arguments {
        request.push_str(&format!("jvmarg={a}\n"));
    }
    if let Some(script) =
        super::config::gradle_init_script(settings, super::config::ECLIPSE_PLUGIN_SCRIPT.0)
    {
        request.push_str(&format!("arg=--init-script\narg={}\n", script.display()));
    }
    for a in &config.arguments {
        request.push_str(&format!("arg={a}\n"));
    }
    let request_file = work.path().join("request.txt");
    std::fs::write(&request_file, request).map_err(|e| FetchError::Unavailable(e.to_string()))?;
    let output = Command::new(launcher(settings))
        .arg("-cp")
        .arg(lib.join("*"))
        .arg(&helper)
        .arg(&request_file)
        .current_dir(work.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| FetchError::Unavailable(e.to_string()))?;
    std::fs::read_to_string(&out).map_err(|_| {
        FetchError::Unavailable(format!(
            "the Gradle model helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    })
}

/// Fetch the `EclipseProject` model of the build in `config.root_project_directory`.
pub fn fetch(config: &BuildConfiguration, settings: &GradleSettings) -> Result<GradleModel, FetchError> {
    parse(&run_helper(config, settings, "model")?)
}

/// The annotation processing configuration of one project.
#[derive(Debug, Clone, Default)]
pub struct AptConfiguration {
    pub processors: Vec<PathBuf>,
    pub compiler_args: Vec<String>,
}

/// `GradleBuildSupport.syncAnnotationProcessingConfiguration`: the custom
/// model of the apt init script, by project directory.
pub fn annotation_processing(
    config: &BuildConfiguration,
    settings: &GradleSettings,
) -> Result<BTreeMap<PathBuf, AptConfiguration>, FetchError> {
    let mut config = config.clone();
    let script = super::config::gradle_init_script(settings, "/gradle/apt/init.gradle")
        .ok_or_else(|| FetchError::Unavailable("the apt init script is missing".into()))?;
    config.arguments.push("--init-script".to_owned());
    config.arguments.push(script.to_string_lossy().into_owned());
    let text = run_helper(&config, settings, "apt")?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| FetchError::Unavailable(format!("invalid Gradle output: {e}")))?;
    if let Some(error) = json["error"].as_str() {
        return Err(FetchError::Failed { message: error.to_owned(), causes: Vec::new() });
    }
    Ok(json["apt"]
        .as_object()
        .map(|projects| {
            projects
                .iter()
                .map(|(dir, info)| {
                    (
                        PathBuf::from(dir),
                        AptConfiguration {
                            processors: strings(&info["processors"]).into_iter().map(PathBuf::from).collect(),
                            compiler_args: strings(&info["compilerArgs"]),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The result of running the compile tasks of the non-Java languages.
#[derive(Debug, Clone, Default)]
pub struct CompileOutput {
    pub tasks: Vec<String>,
    pub stderr: String,
}

/// `GradleBuildSupport.compile`: run the Kotlin, Groovy, AspectJ and Scala
/// compile tasks of the build.
pub fn compile(config: &BuildConfiguration, settings: &GradleSettings) -> Result<CompileOutput, FetchError> {
    let text = run_helper(config, settings, "compile")?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| FetchError::Unavailable(format!("invalid Gradle output: {e}")))?;
    if let Some(error) = json["error"].as_str() {
        return Err(FetchError::Failed { message: error.to_owned(), causes: Vec::new() });
    }
    Ok(CompileOutput {
        tasks: strings(&json["tasks"]),
        stderr: json["stderr"].as_str().unwrap_or("").to_owned(),
    })
}

fn parse(text: &str) -> Result<GradleModel, FetchError> {
    let json: Value = serde_json::from_str(text)
        .map_err(|e| FetchError::Unavailable(format!("invalid Gradle model: {e}")))?;
    if let Some(error) = json["error"].as_str() {
        return Err(FetchError::Failed {
            message: error.to_owned(),
            causes: json["causes"]
                .as_array()
                .map(|c| c.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
        });
    }
    Ok(GradleModel {
        gradle_version: json["gradleVersion"].as_str().unwrap_or("").to_owned(),
        java_home: PathBuf::from(json["javaHome"].as_str().unwrap_or("")),
        project: project(&json["project"]),
    })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn attributes(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn path(v: &Value) -> Option<PathBuf> {
    v.as_str().map(PathBuf::from)
}

fn project(p: &Value) -> ModelProject {
    ModelProject {
        name: p["name"].as_str().unwrap_or("").to_owned(),
        dir: path(&p["dir"]).unwrap_or_default(),
        gradle_path: p["gradlePath"].as_str().unwrap_or(":").to_owned(),
        output: p["output"].as_str().map(str::to_owned),
        sources: p["sources"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| ModelSource {
                        path: s["path"].as_str().unwrap_or("").to_owned(),
                        dir: path(&s["dir"]).unwrap_or_default(),
                        output: s["output"].as_str().map(str::to_owned),
                        includes: strings(&s["includes"]),
                        excludes: strings(&s["excludes"]),
                        attributes: attributes(&s["attributes"]),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        classpath: p["classpath"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| {
                        Some(ModelDependency {
                            file: path(&d["file"])?,
                            source: path(&d["source"]),
                            exported: d["exported"].as_bool().unwrap_or(false),
                            attributes: attributes(&d["attributes"]),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        project_dependencies: p["projectDependencies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|d| ModelProjectDependency {
                        path: d["path"].as_str().unwrap_or("").to_owned(),
                        exported: d["exported"].as_bool().unwrap_or(false),
                        attributes: attributes(&d["attributes"]),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        natures: strings(&p["natures"]),
        build_commands: p["buildCommands"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c["name"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        containers: p["containers"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c["path"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        java: p["java"].as_object().map(|j| ModelJavaSettings {
            source: j.get("source").and_then(Value::as_str).map(str::to_owned),
            target: j.get("target").and_then(Value::as_str).map(str::to_owned),
        }),
        children: p["children"]
            .as_array()
            .map(|a| a.iter().map(project).collect())
            .unwrap_or_default(),
    }
}
