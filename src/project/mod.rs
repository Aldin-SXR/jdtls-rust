//! Workspace project model — Rust port of the jdt.ls project importers
//! (`GradleProjectImporter`, `MavenProjectImporter`, `EclipseProjectImporter`,
//! `InvisibleProjectImporter`).
//!
//! The model is purely additive: documents that are not inside any imported
//! project (including virtual documents with no file on disk, e.g.
//! `untitled:` or `inmemory://` URIs) belong to the default project, which
//! uses the configured classpath and compliance only.

pub mod detect;
pub mod eclipse;
pub mod gradle;
pub mod invisible;
pub mod maven;
pub mod prefs;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub const COMPLIANCE: &str = "org.eclipse.jdt.core.compiler.compliance";
pub const SOURCE: &str = "org.eclipse.jdt.core.compiler.source";
pub const TARGET: &str = "org.eclipse.jdt.core.compiler.codegen.targetPlatform";

/// Name jdt.ls gives the project holding files outside any build project.
pub const DEFAULT_PROJECT_NAME: &str = "jdt.ls-java-project";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    Eclipse,
    Maven,
    Gradle,
    Invisible,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFolder {
    pub path: PathBuf,
    pub is_test: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub path: PathBuf,
    pub source: Option<PathBuf>,
    pub is_test: bool,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub root: PathBuf,
    pub kind: ProjectKind,
    pub source_folders: Vec<SourceFolder>,
    pub libraries: Vec<Library>,
    /// Names of workspace projects this project depends on.
    pub project_deps: Vec<String>,
    /// JDT core options specific to this project (compliance, prefs file).
    pub options: BTreeMap<String, String>,
}

impl Project {
    pub fn compliance(&self) -> Option<&str> {
        self.options.get(COMPLIANCE).map(String::as_str)
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    pub fn source_folder_for(&self, path: &Path) -> Option<&SourceFolder> {
        self.source_folders
            .iter()
            .filter(|sf| path.starts_with(&sf.path))
            .max_by_key(|sf| sf.path.components().count())
    }

    /// All `.java` files beneath this project's source folders.
    pub fn java_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for sf in &self.source_folders {
            for entry in walkdir::WalkDir::new(&sf.path).follow_links(true).into_iter().flatten() {
                let p = entry.path();
                if entry.file_type().is_file()
                    && p.extension().is_some_and(|e| e == "java")
                    && seen.insert(p.to_path_buf())
                {
                    out.push(p.to_path_buf());
                }
            }
        }
        out.sort();
        out
    }
}

/// Settings that influence import (subset of jdt.ls `Preferences`).
#[derive(Debug, Clone, Default)]
pub struct ImportSettings {
    pub exclusions: Vec<String>,
    pub maven_enabled: bool,
    pub gradle_enabled: bool,
    /// `java.project.sourcePaths` for invisible projects.
    pub source_paths: Vec<String>,
    /// `java.project.referencedLibraries` include globs.
    pub referenced_libraries: Vec<String>,
}

impl ImportSettings {
    pub fn jdtls_defaults() -> Self {
        Self {
            exclusions: detect::DEFAULT_IMPORT_EXCLUSIONS.iter().map(|s| s.to_string()).collect(),
            maven_enabled: true,
            gradle_enabled: true,
            source_paths: Vec::new(),
            referenced_libraries: vec!["lib/**/*.jar".to_owned()],
        }
    }
}

/// The set of imported projects.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub projects: Vec<Project>,
}

impl Workspace {
    /// Import every project found under `roots`, mirroring jdt.ls
    /// `ProjectsManager.importProjects`: importers run in order
    /// Gradle (300) → Maven (400) → Eclipse (1000) → Invisible (1500), and a
    /// directory claimed by an earlier importer is excluded from later ones.
    pub fn import(roots: &[PathBuf], settings: &ImportSettings) -> Self {
        let mut projects: Vec<Project> = Vec::new();
        for root in roots {
            if !root.is_dir() {
                continue;
            }
            let mut claimed: Vec<PathBuf> = Vec::new();
            if settings.gradle_enabled {
                for p in gradle::import(root, settings, &claimed) {
                    claimed.push(p.root.clone());
                    projects.push(p);
                }
            }
            if settings.maven_enabled {
                for p in maven::import(root, settings, &claimed) {
                    claimed.push(p.root.clone());
                    projects.push(p);
                }
            }
            for p in eclipse::import(root, settings, &claimed) {
                claimed.push(p.root.clone());
                projects.push(p);
            }
            if claimed.is_empty() {
                if let Some(p) = invisible::import(root, settings) {
                    projects.push(p);
                }
            }
        }
        // Deduplicate by name (first wins, like the Eclipse workspace).
        let mut names = HashSet::new();
        projects.retain(|p| names.insert(p.name.clone()));
        Workspace { projects }
    }

    pub fn project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// The project owning `path`: the deepest project root containing it.
    pub fn project_for_path(&self, path: &Path) -> Option<&Project> {
        self.projects
            .iter()
            .filter(|p| p.contains_path(path) && p.kind != ProjectKind::Default)
            .max_by_key(|p| p.root.components().count())
    }

    pub fn project_for_uri(&self, uri: &Url) -> Option<&Project> {
        let path = uri_to_path(uri)?;
        self.project_for_path(&path)
    }

    /// `project` plus the transitive closure of its project dependencies.
    pub fn project_closure<'a>(&'a self, project: &'a Project) -> Vec<&'a Project> {
        let mut out = vec![project];
        let mut seen: HashSet<&str> = HashSet::from([project.name.as_str()]);
        let mut i = 0;
        while i < out.len() {
            for dep in &out[i].project_deps {
                if let Some(p) = self.project(dep) {
                    if seen.insert(p.name.as_str()) {
                        out.push(p);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// Map of every workspace `.java` file to the name of its project.
    pub fn java_files(&self) -> HashMap<PathBuf, String> {
        let mut out = HashMap::new();
        for p in &self.projects {
            for f in p.java_files() {
                out.entry(f).or_insert_with(|| p.name.clone());
            }
        }
        out
    }
}

/// `java.io.File.toURI().toString()`: `file:` + absolute path (trailing `/`
/// for directories), quoting only characters illegal in a URI path — the
/// exact form jdt.ls returns from commands such as `java.project.getAll`.
pub fn java_file_uri(path: &Path, is_dir: bool) -> String {
    let mut p = path.to_string_lossy().replace('\\', "/");
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if is_dir && !p.ends_with('/') {
        p.push('/');
    }
    let mut out = String::from("file:");
    for c in p.chars() {
        let legal = c.is_ascii_alphanumeric() || "-_.!~*'()/:@&=+$,;".contains(c) || (!c.is_ascii() && !c.is_control() && !c.is_whitespace());
        if legal {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

pub fn uri_to_path(uri: &Url) -> Option<PathBuf> {
    if uri.scheme() != "file" {
        return None;
    }
    let path = uri.to_file_path().ok()?;
    Some(canonicalize_lenient(&path))
}

/// Canonicalize the existing prefix of `path` (so `/tmp` vs `/private/tmp`
/// compare equal on macOS) while keeping nonexistent tails intact — virtual
/// `file:` URIs never touch the disk beyond this.
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let mut tail = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(name) = cur.file_name().map(|n| n.to_os_string()) {
        if !cur.pop() {
            break;
        }
        tail.push(name);
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for t in tail.iter().rev() {
                out.push(t);
            }
            return out;
        }
    }
    path.to_path_buf()
}

/// Normalise a Java version string the way JDT does ("8" → "1.8", "1.11" → "11").
pub fn normalize_java_version(v: &str) -> Option<String> {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
    let v = v
        .strip_prefix("JavaVersion.VERSION_")
        .map(|s| s.replace('_', "."))
        .unwrap_or_else(|| v.to_owned());
    let v = v.as_str();
    let n: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if n.is_empty() {
        return None;
    }
    let parts: Vec<&str> = n.split('.').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        ["1", minor, ..] => {
            let m: u32 = minor.parse().ok()?;
            Some(if m <= 8 { format!("1.{m}") } else { m.to_string() })
        }
        [major, ..] => {
            let m: u32 = major.parse().ok()?;
            Some(if m <= 8 { format!("1.{m}") } else { m.to_string() })
        }
        _ => None,
    }
}

pub(crate) fn compliance_options(version: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(COMPLIANCE.to_owned(), version.to_owned());
    m.insert(SOURCE.to_owned(), version.to_owned());
    m.insert(TARGET.to_owned(), version.to_owned());
    m
}

/// Default JDT core options jdt.ls applies on top of `JavaCore.getOptions()`
/// (see `PreferenceManager.initialize`).
pub fn jdtls_default_options() -> BTreeMap<String, String> {
    let pairs: &[(&str, &str)] = &[
        ("org.eclipse.jdt.core.codeComplete.visibilityCheck", "enabled"),
        ("org.eclipse.jdt.core.compiler.release", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.unhandledWarningToken", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.redundantSuperinterface", "warning"),
        ("org.eclipse.jdt.core.codeComplete.subwordMatch", "disabled"),
        ("org.eclipse.jdt.core.compiler.problem.missingSerialVersion", "ignore"),
        ("org.eclipse.jdt.core.circularClasspath", "warning"),
        ("org.eclipse.jdt.core.compiler.ignoreUnnamedModuleForSplitPackage", "enabled"),
        ("org.eclipse.jdt.core.compiler.problem.unusedLambdaParameter", "ignore"),
        ("org.eclipse.jdt.core.compiler.problem.forbiddenReference", "ignore"),
        ("org.eclipse.jdt.core.compiler.doc.comment.support", "enabled"),
    ];
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Read `<root>/.settings/org.eclipse.jdt.core.prefs` if present.
pub(crate) fn project_prefs(root: &Path) -> BTreeMap<String, String> {
    prefs::read_properties(&root.join(".settings").join("org.eclipse.jdt.core.prefs"))
        .map(|mut m| {
            m.remove("eclipse.preferences.version");
            m
        })
        .unwrap_or_default()
}

/// Locate a jar's `-sources.jar` sibling.
pub(crate) fn source_attachment(jar: &Path) -> Option<PathBuf> {
    let stem = jar.file_stem()?.to_string_lossy();
    let src = jar.with_file_name(format!("{stem}-sources.jar"));
    src.is_file().then_some(src)
}

#[cfg(test)]
mod tests {
    use super::normalize_java_version as n;

    #[test]
    fn java_file_uri_matches_file_to_uri() {
        use std::path::Path;
        assert_eq!(super::java_file_uri(Path::new("/a/b c"), true), "file:/a/b%20c/");
        assert_eq!(super::java_file_uri(Path::new("/a/Foo.java"), false), "file:/a/Foo.java");
    }

    #[test]
    fn normalizes_versions() {
        assert_eq!(n("1.8").as_deref(), Some("1.8"));
        assert_eq!(n("8").as_deref(), Some("1.8"));
        assert_eq!(n("17").as_deref(), Some("17"));
        assert_eq!(n("JavaVersion.VERSION_11").as_deref(), Some("11"));
        assert_eq!(n("JavaVersion.VERSION_1_8").as_deref(), Some("1.8"));
        assert_eq!(n("'1.7'").as_deref(), Some("1.7"));
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects").join(rel)
    }

    #[test]
    fn imports_fixture_projects() {
        let s = ImportSettings::jdtls_defaults();
        for rel in ["maven/salut", "eclipse/hello", "gradle/simple-gradle", "maven/multimodule", "singlefile/simple", "eclipse/reference"] {
            let ws = Workspace::import(&[fixture(rel)], &s);
            for p in &ws.projects {
                eprintln!(
                    "{rel}: {} {:?} compliance={:?} src={:?} libs={:?} deps={:?}",
                    p.name, p.kind, p.compliance(),
                    p.source_folders.iter().map(|s| s.path.strip_prefix(&p.root).unwrap_or(&s.path).to_path_buf()).collect::<Vec<_>>(),
                    p.libraries.iter().map(|l| l.path.file_name().unwrap().to_owned()).collect::<Vec<_>>(),
                    p.project_deps
                );
            }
        }
        let ws = Workspace::import(&[fixture("maven/salut")], &s);
        assert_eq!(ws.projects[0].name, "salut");
        assert_eq!(ws.projects[0].compliance(), Some("1.8"));
    }
}
