//! Maven project import: a Rust replacement for the m2e model resolution
//! jdt.ls relies on.  Builds the effective POM (parent chain, properties,
//! dependencyManagement incl. BOM imports) and resolves dependencies
//! transitively against the local repository — no Maven process needed.

use super::detect::FileDetector;
use super::{
    compliance_options, normalize_java_version, project_prefs, source_attachment, ClasspathEntry,
    EntryKind, ImportSettings, Marker, Project, ProjectKind, Workspace,
};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Dep {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub scope: Option<String>,
    pub classifier: Option<String>,
    pub typ: Option<String>,
    pub optional: bool,
    pub exclusions: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
struct ParentRef {
    group: String,
    artifact: String,
    version: String,
    relative_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RawPom {
    group: Option<String>,
    artifact: String,
    version: Option<String>,
    packaging: Option<String>,
    parent: Option<ParentRef>,
    properties: BTreeMap<String, String>,
    deps: Vec<Dep>,
    dep_mgmt: Vec<Dep>,
    modules: Vec<String>,
    source_dir: Option<String>,
    test_source_dir: Option<String>,
    compiler: BTreeMap<String, String>,
    compiler_args: Vec<String>,
    extra_sources: Vec<(String, bool)>,
}

/// Effective (inherited + interpolated) model.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub packaging: String,
    pub properties: BTreeMap<String, String>,
    pub deps: Vec<Dep>,
    pub dep_mgmt: HashMap<(String, String), Dep>,
    pub modules: Vec<String>,
    pub source_dir: String,
    pub test_source_dir: String,
    pub compiler: BTreeMap<String, String>,
    pub compiler_args: Vec<String>,
    pub extra_sources: Vec<(String, bool)>,
    /// `group:artifact:version` of a parent POM that cannot be resolved.
    pub unresolved_parent: Option<String>,
}

fn child_text(node: roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_owned())
}

fn parse_dep(node: roxmltree::Node) -> Dep {
    Dep {
        group: child_text(node, "groupId").unwrap_or_default(),
        artifact: child_text(node, "artifactId").unwrap_or_default(),
        version: child_text(node, "version"),
        scope: child_text(node, "scope"),
        classifier: child_text(node, "classifier"),
        typ: child_text(node, "type"),
        optional: child_text(node, "optional").is_some_and(|s| s == "true"),
        exclusions: node
            .children()
            .filter(|n| n.has_tag_name("exclusions"))
            .flat_map(|n| n.children().filter(|e| e.has_tag_name("exclusion")))
            .map(|e| {
                (
                    child_text(e, "groupId").unwrap_or_default(),
                    child_text(e, "artifactId").unwrap_or_default(),
                )
            })
            .collect(),
    }
}

/// Maven's pull parser accepts attributes that aren't separated by
/// whitespace (`a="1"b="2"`), which XML forbids: the repaired text of a
/// document that doesn't parse as it is.
fn repaired_xml(text: &str) -> Option<String> {
    static MISSING_SPACE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"(="[^"<>]*")([A-Za-z_][\w:.-]*=)"#).unwrap()
    });
    let mut fixed = text.to_owned();
    loop {
        let next = MISSING_SPACE.replace_all(&fixed, "$1 $2").into_owned();
        if next == fixed {
            break;
        }
        fixed = next;
    }
    (fixed != text).then_some(fixed)
}

pub fn parse_pom(text: &str) -> Option<RawPom> {
    let repaired;
    let doc = match roxmltree::Document::parse(text) {
        Ok(doc) => doc,
        Err(_) => {
            repaired = repaired_xml(text)?;
            roxmltree::Document::parse(&repaired).ok()?
        }
    };
    let root = doc.root_element();
    let mut pom = RawPom {
        group: child_text(root, "groupId"),
        artifact: child_text(root, "artifactId").unwrap_or_default(),
        version: child_text(root, "version"),
        packaging: child_text(root, "packaging"),
        ..Default::default()
    };
    for n in root.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "parent" => {
                pom.parent = Some(ParentRef {
                    group: child_text(n, "groupId").unwrap_or_default(),
                    artifact: child_text(n, "artifactId").unwrap_or_default(),
                    version: child_text(n, "version").unwrap_or_default(),
                    relative_path: child_text(n, "relativePath"),
                })
            }
            "properties" => {
                for p in n.children().filter(|c| c.is_element()) {
                    pom.properties.insert(
                        p.tag_name().name().to_owned(),
                        p.text().unwrap_or("").trim().to_owned(),
                    );
                }
            }
            "dependencies" => {
                pom.deps = n
                    .children()
                    .filter(|c| c.has_tag_name("dependency"))
                    .map(parse_dep)
                    .collect()
            }
            "dependencyManagement" => {
                pom.dep_mgmt = n
                    .descendants()
                    .filter(|c| c.has_tag_name("dependency"))
                    .map(parse_dep)
                    .collect()
            }
            "modules" => {
                pom.modules = n
                    .children()
                    .filter(|c| c.has_tag_name("module"))
                    .filter_map(|c| c.text().map(|t| t.trim().to_owned()))
                    .collect()
            }
            "build" => parse_build(n, &mut pom),
            _ => {}
        }
    }
    // m2e LocalProjectScanner visits modules from every profile, including
    // inactive profiles, when importing the module tree.
    for module in root
        .children()
        .filter(|n| n.has_tag_name("profiles"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("profile")))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("modules")))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("module")))
    {
        if let Some(module) = module.text().map(str::trim) {
            if !pom.modules.iter().any(|m| m == module) {
                pom.modules.push(module.to_owned());
            }
        }
    }
    Some(pom)
}

fn parse_build(build: roxmltree::Node, pom: &mut RawPom) {
    pom.source_dir = child_text(build, "sourceDirectory");
    pom.test_source_dir = child_text(build, "testSourceDirectory");
    let plugins = build
        .children()
        .filter(|n| n.has_tag_name("plugins") || n.has_tag_name("pluginManagement"))
        .flat_map(|n| n.descendants().filter(|p| p.has_tag_name("plugin")));
    for plugin in plugins {
        let aid = child_text(plugin, "artifactId").unwrap_or_default();
        if aid == "maven-compiler-plugin" {
            if let Some(cfg) = plugin.children().find(|c| c.has_tag_name("configuration")) {
                for key in [
                    "source",
                    "target",
                    "release",
                    "compilerArgument",
                    "enablePreview",
                    "parameters",
                    "compilerId",
                ] {
                    if let Some(v) = child_text(cfg, key) {
                        pom.compiler.entry(key.to_owned()).or_insert(v);
                    }
                }
                if let Some(args) = cfg.children().find(|c| c.has_tag_name("compilerArgs")) {
                    if pom.compiler_args.is_empty() {
                        pom.compiler_args = args
                            .children()
                            .filter(|c| c.is_element())
                            .filter_map(|c| c.text().map(|t| t.trim().to_owned()))
                            .collect();
                    }
                }
            }
        } else if aid == "build-helper-maven-plugin" {
            for exec in plugin.descendants().filter(|e| e.has_tag_name("execution")) {
                let goal_test = exec.descendants().any(|g| {
                    g.has_tag_name("goal")
                        && g.text().is_some_and(|t| t.trim() == "add-test-source")
                });
                for src in exec
                    .descendants()
                    .filter(|s| s.has_tag_name("sources"))
                    .flat_map(|s| s.children().filter(|c| c.has_tag_name("source")))
                {
                    if let Some(t) = src.text() {
                        pom.extra_sources.push((t.trim().to_owned(), goal_test));
                    }
                }
            }
        }
    }
}

/// Resolves POMs from the workspace and the local repository.
pub struct Resolver {
    pub local_repo: PathBuf,
    pub repositories: Vec<super::download::Repository>,
    cache: HashMap<PathBuf, Option<Model>>,
    settings: MavenSettings,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            local_repo: local_repository(),
            repositories: super::download::default_repositories(),
            cache: HashMap::new(),
            settings: MavenSettings::default(),
        }
    }

    pub fn with_settings(settings: &MavenSettings) -> Self {
        Self {
            local_repo: local_repository(),
            repositories: super::download::default_repositories(),
            cache: HashMap::new(),
            settings: settings.clone(),
        }
    }

    /// Download `group:artifact:version[:classifier]@ext` into the local
    /// repository (see `download`).
    pub fn download_artifact(
        &self,
        g: &str,
        a: &str,
        v: &str,
        classifier: Option<&str>,
        ext: &str,
    ) -> Option<PathBuf> {
        if self.settings.offline {
            return None;
        }
        super::download::fetch(
            &self.local_repo,
            &self.repositories,
            g,
            a,
            v,
            classifier,
            ext,
        )
    }

    pub fn repo_pom(&self, g: &str, a: &str, v: &str) -> PathBuf {
        self.repo_dir(g, a, v).join(format!("{a}-{v}.pom"))
    }

    fn artifact_model(&mut self, g: &str, a: &str, v: &str) -> Option<Model> {
        let pom = self.repo_pom(g, a, v);
        if !pom.is_file() {
            self.download_artifact(g, a, v, None, "pom")?;
        }
        self.model(&pom)
    }

    fn repo_dir(&self, g: &str, a: &str, v: &str) -> PathBuf {
        let mut p = self.local_repo.clone();
        for seg in g.split('.') {
            p.push(seg);
        }
        p.push(a);
        p.push(v);
        p
    }

    pub fn artifact_jar(
        &self,
        g: &str,
        a: &str,
        v: &str,
        classifier: Option<&str>,
    ) -> Option<PathBuf> {
        let dir = self.repo_dir(g, a, v);
        let name = match classifier {
            Some(c) if !c.is_empty() => format!("{a}-{v}-{c}.jar"),
            _ => format!("{a}-{v}.jar"),
        };
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
        // Timestamped SNAPSHOT jars.
        if v.ends_with("-SNAPSHOT") {
            let base = v.trim_end_matches("-SNAPSHOT");
            let entries = std::fs::read_dir(&dir).ok()?;
            let mut cands: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().unwrap_or_default().to_string_lossy();
                    n.starts_with(&format!("{a}-{base}-"))
                        && n.ends_with(".jar")
                        && !n.ends_with("-sources.jar")
                })
                .collect();
            cands.sort();
            return cands.pop();
        }
        None
    }

    /// Effective model of the POM at `path`.
    pub fn model(&mut self, path: &Path) -> Option<Model> {
        let key = path.to_path_buf();
        if let Some(m) = self.cache.get(&key) {
            return m.clone();
        }
        // Guard against parent cycles.
        self.cache.insert(key.clone(), None);
        let m = self.build_model(path);
        self.cache.insert(key, m.clone());
        m
    }

    fn build_model(&mut self, path: &Path) -> Option<Model> {
        let raw = parse_pom(&std::fs::read_to_string(path).ok()?)?;
        let parent_model = raw.parent.as_ref().and_then(|p| {
            let dir = path.parent()?;
            let rel = p
                .relative_path
                .clone()
                .unwrap_or_else(|| "../pom.xml".to_owned());
            let mut candidate = dir.join(&rel);
            if candidate.is_dir() {
                candidate = candidate.join("pom.xml");
            }
            if !rel.is_empty() && candidate.is_file() {
                if let Some(m) = self.model(&super::canonicalize_lenient(&candidate)) {
                    if m.artifact == p.artifact && m.group == p.group {
                        return Some(m);
                    }
                }
            }
            self.artifact_model(&p.group, &p.artifact, &p.version)
        });

        let mut m = parent_model.clone().unwrap_or_default();
        if m.unresolved_parent.is_none() && parent_model.is_none() {
            m.unresolved_parent = raw
                .parent
                .as_ref()
                .map(|p| format!("{}:{}:{}", p.group, p.artifact, p.version));
        }
        // Modules and build directories are not inherited.
        m.modules = raw.modules.clone();
        m.extra_sources = raw.extra_sources.clone();
        m.artifact = raw.artifact.clone();
        m.group = raw
            .group
            .clone()
            .or_else(|| raw.parent.as_ref().map(|p| p.group.clone()))
            .unwrap_or_default();
        m.version = raw
            .version
            .clone()
            .or_else(|| raw.parent.as_ref().map(|p| p.version.clone()))
            .unwrap_or_default();
        m.packaging = raw.packaging.clone().unwrap_or_else(|| "jar".to_owned());
        m.source_dir = raw
            .source_dir
            .clone()
            .unwrap_or_else(|| "src/main/java".to_owned());
        m.test_source_dir = raw
            .test_source_dir
            .clone()
            .unwrap_or_else(|| "src/test/java".to_owned());
        for (k, v) in &raw.properties {
            m.properties.insert(k.clone(), v.clone());
        }
        for (k, v) in &raw.compiler {
            m.compiler.insert(k.clone(), v.clone());
        }
        if !raw.compiler_args.is_empty() {
            m.compiler_args = raw.compiler_args.clone();
        }
        if let Some(p) = &raw.parent {
            m.properties
                .insert("project.parent.version".into(), p.version.clone());
            m.properties
                .insert("project.parent.groupId".into(), p.group.clone());
        }
        let builtins = [
            ("project.version", m.version.clone()),
            ("pom.version", m.version.clone()),
            ("version", m.version.clone()),
            ("project.groupId", m.group.clone()),
            ("pom.groupId", m.group.clone()),
            ("project.artifactId", m.artifact.clone()),
            (
                "project.basedir",
                path.parent()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "basedir",
                path.parent()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .into_owned(),
            ),
        ];
        for (k, v) in builtins {
            m.properties.insert(k.to_owned(), v);
        }

        let props = m.properties.clone();
        let interp = |s: &str| interpolate(s, &props);
        let interp_dep = |d: &Dep| Dep {
            group: interp(&d.group),
            artifact: interp(&d.artifact),
            version: d.version.as_deref().map(interp),
            scope: d.scope.as_deref().map(interp),
            classifier: d.classifier.as_deref().map(interp),
            typ: d.typ.clone(),
            optional: d.optional,
            exclusions: d.exclusions.clone(),
        };
        // dependencyManagement: child entries override parent's; BOM imports
        // contribute entries that do not override explicit ones.
        let mut boms = Vec::new();
        for d in raw.dep_mgmt.iter().map(interp_dep) {
            if d.scope.as_deref() == Some("import") && d.typ.as_deref() == Some("pom") {
                boms.push(d);
            } else {
                m.dep_mgmt.insert((d.group.clone(), d.artifact.clone()), d);
            }
        }
        for bom in boms {
            if let Some(v) = &bom.version {
                if let Some(bm) = self.artifact_model(&bom.group, &bom.artifact, v) {
                    for (k, d) in bm.dep_mgmt {
                        m.dep_mgmt.entry(k).or_insert(d);
                    }
                }
            }
        }
        let mut deps: Vec<Dep> = raw.deps.iter().map(interp_dep).collect();
        for d in deps.iter_mut() {
            apply_management(d, &m.dep_mgmt);
        }
        // Inherited dependencies come after the child's own.
        let inherited: Vec<Dep> = parent_model.map(|p| p.deps).unwrap_or_default();
        for d in inherited {
            if !deps
                .iter()
                .any(|x| x.group == d.group && x.artifact == d.artifact)
            {
                deps.push(d);
            }
        }
        m.deps = deps;
        m.source_dir = interp(&m.source_dir);
        m.test_source_dir = interp(&m.test_source_dir);
        m.compiler = m
            .compiler
            .iter()
            .map(|(k, v)| (k.clone(), interp(v)))
            .collect();
        m.compiler_args = m.compiler_args.iter().map(|a| interp(a)).collect();
        m.extra_sources = m
            .extra_sources
            .iter()
            .map(|(s, t)| (interp(s), *t))
            .collect();
        Some(m)
    }

    /// Transitively resolve dependencies of `model` (Maven "nearest wins").
    /// Returns `(dep, effective scope)` for every resolved artifact.
    pub fn resolve(&mut self, model: &Model) -> Vec<(Dep, String)> {
        let mut out: Vec<(Dep, String)> = Vec::new();
        let mut seen: HashSet<(String, String, Option<String>)> = HashSet::new();
        let mut queue: VecDeque<(Dep, String, Vec<(String, String)>)> = VecDeque::new();
        for d in &model.deps {
            let scope = d.scope.clone().unwrap_or_else(|| "compile".into());
            queue.push_back((d.clone(), scope, d.exclusions.clone()));
        }
        while let Some((dep, scope, exclusions)) = queue.pop_front() {
            if !seen.insert((
                dep.group.clone(),
                dep.artifact.clone(),
                dep.classifier.clone(),
            )) {
                continue;
            }
            let Some(version) = dep.version.clone() else {
                continue;
            };
            out.push((dep.clone(), scope.clone()));
            if scope == "system" {
                continue;
            }
            let Some(dm) = self.artifact_model(&dep.group, &dep.artifact, &version) else { continue };
            for td in dm.deps {
                let ts = td.scope.clone().unwrap_or_else(|| "compile".into());
                if td.optional || ts == "test" || ts == "provided" || ts == "system" {
                    continue;
                }
                if exclusions
                    .iter()
                    .any(|(g, a)| (g == "*" || *g == td.group) && (a == "*" || *a == td.artifact))
                {
                    continue;
                }
                let mut td = td;
                // The root's dependencyManagement wins for transitive deps.
                if let Some(managed) = model.dep_mgmt.get(&(td.group.clone(), td.artifact.clone()))
                {
                    if managed.version.is_some() {
                        td.version = managed.version.clone();
                    }
                }
                let eff = match (scope.as_str(), ts.as_str()) {
                    ("compile", s) => s.to_owned(),
                    ("provided", _) => "provided".into(),
                    ("runtime", _) => "runtime".into(),
                    ("test", _) => "test".into(),
                    (s, _) => s.to_owned(),
                };
                let mut ex = exclusions.clone();
                ex.extend(td.exclusions.iter().cloned());
                queue.push_back((td, eff, ex));
            }
        }
        out
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_management(d: &mut Dep, mgmt: &HashMap<(String, String), Dep>) {
    if let Some(m) = mgmt.get(&(d.group.clone(), d.artifact.clone())) {
        if d.version.is_none() {
            d.version = m.version.clone();
        }
        if d.scope.is_none() {
            d.scope = m.scope.clone();
        }
        if d.exclusions.is_empty() {
            d.exclusions = m.exclusions.clone();
        }
    }
}

pub fn interpolate(s: &str, props: &BTreeMap<String, String>) -> String {
    let mut cur = s.to_owned();
    for _ in 0..10 {
        let mut out = String::with_capacity(cur.len());
        let mut rest = cur.as_str();
        let mut changed = false;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            if let Some(end) = after.find('}') {
                let key = &after[..end];
                if let Some(v) = props.get(key) {
                    out.push_str(v);
                    changed = true;
                } else {
                    out.push_str(&rest[start..start + 3 + end]);
                }
                rest = &after[end + 1..];
            } else {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
        out.push_str(rest);
        cur = out;
        if !changed {
            break;
        }
    }
    cur
}

pub fn local_repository() -> PathBuf {
    let home = super::eclipse::dirs_home().unwrap_or_default();
    if let Ok(s) = std::fs::read_to_string(home.join(".m2").join("settings.xml")) {
        if let Ok(doc) = roxmltree::Document::parse(&s) {
            if let Some(repo) = doc
                .descendants()
                .find(|n| n.has_tag_name("localRepository"))
                .and_then(|n| n.text())
            {
                let repo = repo.trim().replace("${user.home}", &home.to_string_lossy());
                if !repo.is_empty() {
                    return PathBuf::from(repo);
                }
            }
        }
    }
    home.join(".m2").join("repository")
}

/// `sanitizeJavaVersion`: "5".."8" become "1.5".."1.8"; "1.9"+ lose the "1.".
pub fn sanitize_java_version(v: &str) -> String {
    match v {
        "5" | "6" | "7" | "8" => format!("1.{v}"),
        _ => {
            if let Some(sub) = v.strip_prefix("1.") {
                if sub.parse::<u32>().is_ok_and(|n| n > 8) {
                    return sub.to_owned();
                }
            }
            v.to_owned()
        }
    }
}

/// Highest Java version JDT 3.44 knows (`JavaCore.getAllVersions()`).
pub const HIGHEST_JAVA_VERSION: u32 = 26;

/// `getCompilerLevel` with the supported versions: a version above the
/// highest known one becomes the highest; an unknown one is dropped.
fn supported_level(v: &str) -> Option<String> {
    let v = v.trim();
    let major: Option<u32> = match v.strip_prefix("1.") {
        Some(m) => m.parse().ok(),
        None => v.parse().ok(),
    };
    match major {
        Some(m) if m > HIGHEST_JAVA_VERSION => Some(HIGHEST_JAVA_VERSION.to_string()),
        Some(m) if (1..=HIGHEST_JAVA_VERSION).contains(&m) => Some(v.to_owned()),
        _ => None,
    }
}

/// `(release, source, target)` as m2e reads them from the
/// maven-compiler-plugin execution.
pub fn compiler_parameters(model: &Model) -> (Option<String>, Option<String>, Option<String>) {
    let param = |k: &str, prop: &str| -> Option<String> {
        model
            .compiler
            .get(k)
            .cloned()
            .or_else(|| model.properties.get(prop).cloned())
            .map(|v| interpolate(&v, &model.properties))
            .filter(|v| !v.trim().is_empty() && !v.contains("${"))
    };
    let release = param("release", "maven.compiler.release").and_then(|r| supported_level(&r));
    let source = param("source", "maven.compiler.source")
        .map(|v| sanitize_java_version(v.trim()))
        .and_then(|v| supported_level(&v));
    let target = param("target", "maven.compiler.target")
        .map(|v| sanitize_java_version(v.trim()))
        .and_then(|v| supported_level(&v));
    (release, source, target)
}

/// m2e's effective `(source, target)` (the release wins; the default level is 1.8).
pub fn compiler_levels(model: &Model) -> (String, String) {
    let (release, source, target) = compiler_parameters(model);
    match release {
        Some(r) => {
            let r = sanitize_java_version(&r);
            (r.clone(), r)
        }
        None => {
            let source = sanitize_java_version(&source.unwrap_or_else(|| "1.8".to_owned()));
            let target = sanitize_java_version(&target.unwrap_or_else(|| "1.8".to_owned()));
            (source, target)
        }
    }
}

/// `AbstractJavaProjectConfigurator.addJavaProjectOptions`, on top of the
/// project's existing `.settings` options.
pub fn compiler_options(model: &Model, dir: &Path, name: &str, _vm: Option<&str>) -> BTreeMap<String, String> {
    let mut options = super::project_prefs_of(dir, name);
    let (release, _, _) = compiler_parameters(model);
    let (source, target) = compiler_levels(model);
    let args = &model.compiler_args;
    let argument = model
        .compiler
        .get("compilerArgument")
        .cloned()
        .unwrap_or_default();
    let enable_preview = args.iter().any(|a| a == "--enable-preview")
        || argument.contains("--enable-preview")
        || model
            .compiler
            .get("enablePreview")
            .or_else(|| model.properties.get("maven.compiler.enablePreview"))
            .is_some_and(|v| v.trim() == "true");
    let parameters = model
        .compiler
        .get("parameters")
        .or_else(|| model.properties.get("maven.compiler.parameters"))
        .is_some_and(|v| v.trim() == "true")
        || args.iter().any(|a| a == "-parameters")
        || argument.contains("-parameters");
    for a in args {
        let (err, settings) = if let Some(s) = a.strip_prefix("-warn:") {
            (false, s)
        } else if let Some(s) = a.strip_prefix("-err:") {
            (true, s)
        } else {
            continue;
        };
        for cli in settings.split(',') {
            if cli.len() < 2 {
                continue;
            }
            let severity = if cli.starts_with('-') {
                "ignore"
            } else if err {
                "error"
            } else {
                "warning"
            };
            let name = if cli.chars().next().is_some_and(|c| c.is_alphabetic()) {
                cli
            } else {
                &cli[1..]
            };
            if name == "serial" {
                options.insert(
                    "org.eclipse.jdt.core.compiler.problem.missingSerialVersion".to_owned(),
                    severity.to_owned(),
                );
            }
        }
    }
    options.insert(super::SOURCE.to_owned(), source.clone());
    options.insert(super::COMPLIANCE.to_owned(), source);
    options.insert(super::TARGET.to_owned(), target);
    options.insert(
        super::RELEASE.to_owned(),
        if release.is_some() {
            "enabled"
        } else {
            "disabled"
        }
        .to_owned(),
    );
    if parameters {
        options.insert(
            "org.eclipse.jdt.core.compiler.codegen.methodParameters".to_owned(),
            "generate".to_owned(),
        );
    }
    options
        .entry("org.eclipse.jdt.core.compiler.problem.forbiddenReference".to_owned())
        .or_insert_with(|| "warning".to_owned());
    options.insert(
        super::ENABLE_PREVIEW.to_owned(),
        if enable_preview {
            "enabled"
        } else {
            "disabled"
        }
        .to_owned(),
    );
    options
        .entry(super::REPORT_PREVIEW.to_owned())
        .or_insert_with(|| "ignore".to_owned());
    options
}

/// Maven preferences (`java.import.maven.*`, `java.maven.*`).
#[derive(Debug, Clone, Default)]
pub struct MavenSettings {
    /// `java.import.maven.offline.enabled`.
    pub offline: bool,
    /// `java.maven.downloadSources`.
    pub download_sources: bool,
    /// `java.maven.updateSnapshots`.
    pub update_snapshots: bool,
    /// `java.configuration.maven.userSettings`.
    pub user_settings: Option<PathBuf>,
    /// `java.configuration.maven.globalSettings`.
    pub global_settings: Option<PathBuf>,
}

/// The modules of the effective model of `pom`, `<modules>` plus those of
/// the active profiles (`IMavenProjectFacade.getMavenProjectModules()`).
pub fn active_modules(pom: &Path, selected_profiles: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pom) else {
        return Vec::new();
    };
    let Ok(doc) = roxmltree::Document::parse(&text) else {
        return Vec::new();
    };
    let selected: Vec<&str> = selected_profiles
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let modules_of = |node: roxmltree::Node| -> Vec<String> {
        node.children()
            .filter(|n| n.has_tag_name("modules"))
            .flat_map(|n| n.children().filter(|c| c.has_tag_name("module")))
            .filter_map(|c| c.text().map(|t| t.trim().to_owned()))
            .collect()
    };
    let root = doc.root_element();
    let mut modules = modules_of(root);
    let profiles: Vec<roxmltree::Node> = root
        .children()
        .filter(|n| n.has_tag_name("profiles"))
        .flat_map(|n| n.children().filter(|c| c.has_tag_name("profile")))
        .collect();
    let id_of = |p: &roxmltree::Node| child_text(*p, "id").unwrap_or_default();
    let explicitly_active = profiles.iter().any(|p| selected.contains(&id_of(p).as_str()));
    for profile in &profiles {
        let id = id_of(profile);
        let active_by_default = profile
            .children()
            .find(|n| n.has_tag_name("activation"))
            .and_then(|a| child_text(a, "activeByDefault"))
            .is_some_and(|v| v == "true");
        let active = if selected.iter().any(|s| s.strip_prefix('!') == Some(id.as_str())) {
            false
        } else {
            selected.contains(&id.as_str()) || (active_by_default && !explicitly_active)
        };
        if active {
            for module in modules_of(*profile) {
                if !modules.contains(&module) {
                    modules.push(module);
                }
            }
        }
    }
    modules
}

/// `MavenBuildSupport.collectProjects`: the names of `project` and, for a
/// `pom` packaging, of the open Maven projects named like its modules.
pub fn collect_projects(ws: &Workspace, project: &Project) -> Vec<String> {
    let mut out = Vec::new();
    collect_into(ws, project, &mut Resolver::with_settings(&Default::default()), &mut out);
    out
}

fn collect_into(ws: &Workspace, project: &Project, resolver: &mut Resolver, out: &mut Vec<String>) {
    if !project.has_nature(super::MAVEN_NATURE) {
        return;
    }
    if !out.contains(&project.name) {
        out.push(project.name.clone());
    }
    let pom = project.location.join(POM_FILE);
    let Some(model) = resolver
        .model(&super::canonicalize_lenient(&pom))
        .filter(|m| m.unresolved_parent.is_none())
    else {
        return;
    };
    if model.packaging != "pom" {
        return;
    }
    for module in active_modules(&pom, &project.selected_profiles) {
        if let Some(p) = ws
            .projects
            .iter()
            .find(|p| p.name == module && p.location.join(POM_FILE).exists())
        {
            if p.has_nature(super::MAVEN_NATURE) && !out.contains(&p.name) {
                collect_into(ws, p, resolver, out);
            }
        }
    }
}

/// `pom.xml`.
pub const POM_FILE: &str = "pom.xml";

/// `MavenProjectImporter.applies` + `importToWorkspace` for `root`.
pub fn import(
    root: &Path,
    settings: &ImportSettings,
    ws: &Workspace,
    configs: Option<&[PathBuf]>,
) -> Vec<Project> {
    let non_maven: Vec<&Path> = ws
        .projects
        .iter()
        .filter(|p| !p.has_nature(super::MAVEN_NATURE))
        .map(|p| p.location.as_path())
        .collect();
    let roots: Vec<PathBuf> = match configs {
        Some(files) => files
            .iter()
            .filter(|f| f.file_name().is_some_and(|n| n == POM_FILE))
            .filter_map(|f| f.parent().map(Path::to_path_buf))
            .filter(|d| !non_maven.contains(&d.as_path()))
            .collect(),
        None => {
            let mut detector = FileDetector::new(root, &[POM_FILE])
                .include_nested(false)
                .add_exclusions(["**/target"])
                .add_exclusions(&settings.exclusions);
            for p in &non_maven {
                detector = detector.add_exclusions([super::detect::path_pattern(p)]);
            }
            detector.scan()
        }
    };
    // Already-imported Maven projects are kept as they are.
    let existing: Vec<&Path> = ws
        .projects
        .iter()
        .filter(|p| p.has_nature(super::MAVEN_NATURE))
        .map(|p| p.location.as_path())
        .collect();

    let mut resolver = Resolver::with_settings(&settings.maven);
    // LocalProjectScanner: every root pom plus its module tree.
    let mut pom_dirs: Vec<(PathBuf, Model)> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<PathBuf> = roots
        .iter()
        .rev()
        .map(|d| super::canonicalize_lenient(d))
        .collect();
    while let Some(dir) = stack.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let pom = dir.join(POM_FILE);
        let Some(model) = resolver.model(&pom) else {
            continue;
        };
        for module in model.modules.iter().rev() {
            let mdir = dir.join(module);
            let mdir = if mdir.is_file() {
                mdir.parent().map(Path::to_path_buf).unwrap_or(mdir)
            } else {
                mdir
            };
            stack.push(super::canonicalize_lenient(&mdir));
        }
        if existing.contains(&dir.as_path()) {
            continue;
        }
        pom_dirs.push((dir, model));
    }

    let mut artifact_ids = HashSet::new();
    for (_, m) in &pom_dirs {
        artifact_ids.insert(m.artifact.clone());
    }
    let duplicate_template = pom_dirs.len() > artifact_ids.len();

    let mut workspace_gas: HashMap<(String, String), String> = HashMap::new();
    for p in ws
        .projects
        .iter()
        .filter(|p| p.has_nature(super::MAVEN_NATURE))
    {
        if let Some(m) = resolver.model(&p.location.join(POM_FILE)) {
            workspace_gas.insert((m.group.clone(), m.artifact.clone()), p.name.clone());
        }
    }
    for (_, m) in &pom_dirs {
        let name = if duplicate_template {
            format!("{}-{}", m.group, m.artifact)
        } else {
            m.artifact.clone()
        };
        workspace_gas.insert((m.group.clone(), m.artifact.clone()), name);
    }

    pom_dirs
        .into_iter()
        .map(|(dir, model)| {
            let name = workspace_gas[&(model.group.clone(), model.artifact.clone())].clone();
            to_project(&dir, &name, &model, &mut resolver, &workspace_gas, settings)
        })
        .collect()
}

fn source_entry(
    project: &Project,
    location: &Path,
    output: Option<&Path>,
    test: bool,
    resources: bool,
) -> ClasspathEntry {
    let mut e = ClasspathEntry::new(EntryKind::Source, project.full_path(location));
    e.location = Some(location.to_path_buf());
    e.output = output.map(Path::to_path_buf);
    if resources {
        e.exclusions = vec!["**".to_owned()];
    }
    e.attributes
        .push(("maven.pomderived".into(), "true".into()));
    e.attributes.push(("optional".into(), "true".into()));
    if test {
        e.attributes.push(("test".into(), "true".into()));
    }
    e
}

fn apt_entry(
    project: &Project,
    location: &Path,
    output: Option<&Path>,
    test: bool,
) -> ClasspathEntry {
    let mut e = ClasspathEntry::new(EntryKind::Source, project.full_path(location));
    e.location = Some(location.to_path_buf());
    e.output = output.map(Path::to_path_buf);
    e.attributes = vec![
        ("ignore_optional_problems".into(), "true".into()),
        ("m2e-apt".into(), "true".into()),
        ("maven.pomderived".into(), "true".into()),
        ("optional".into(), "true".into()),
    ];
    if test {
        e.attributes.push(("test".into(), "true".into()));
    }
    e
}

/// m2e's project preferences (`ResolverConfiguration`).
const M2E_PREFS: &str = ".settings/org.eclipse.m2e.core.prefs";

/// The selected profiles of m2e's persisted `ResolverConfiguration`.
fn resolver_configuration(dir: &Path, name: &str) -> String {
    super::prefs::read_properties(&super::metadata::resolve(dir, name, M2E_PREFS))
        .and_then(|p| p.get("activeProfiles").cloned())
        .unwrap_or_default()
}

/// `IProjectConfigurationManager.setResolverConfiguration`: persist the
/// selected profiles (with `resolveWorkspaceProjects`).
pub fn write_resolver_configuration(dir: &Path, name: &str, selected_profiles: &str) -> std::io::Result<()> {
    let path = super::metadata::resolve(dir, name, M2E_PREFS);
    let mut prefs = super::prefs::read_properties(&path).unwrap_or_default();
    prefs.insert("activeProfiles".into(), selected_profiles.to_owned());
    prefs.insert("eclipse.preferences.version".into(), "1".into());
    prefs.insert("resolveWorkspaceProjects".into(), "true".into());
    prefs.insert("version".into(), "1".into());
    super::prefs::write_properties(&path, &prefs)
}

const MISSING_VERSION_MOJO: &str = include_str!("m2e_missing_version.txt");

/// The `<dependency>` of `group:artifact` in the pom `text`: the 0-based line
/// and 1-based column of its start tag.
fn dependency_position(text: &str, group: &str, artifact: &str) -> Option<(u32, u32)> {
    let repaired;
    let doc = match roxmltree::Document::parse(text) {
        Ok(doc) => doc,
        Err(_) => {
            repaired = repaired_xml(text)?;
            roxmltree::Document::parse(&repaired).ok()?
        }
    };
    let node = doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("dependencies"))
        .flat_map(|n| n.children().filter(|c| c.has_tag_name("dependency")))
        .find(|n| {
            child_text(*n, "groupId").as_deref() == Some(group)
                && child_text(*n, "artifactId").as_deref() == Some(artifact)
        })?;
    let pos = doc.text_pos_at(node.range().start);
    Some((pos.row - 1, pos.col))
}

/// The line holding the end of the `<project>` start tag and its length.
fn root_tag_end_line(text: &str) -> (u32, u32) {
    let repaired;
    let text = match roxmltree::Document::parse(text) {
        Ok(_) => text,
        Err(_) => {
            repaired = repaired_xml(text).unwrap_or_else(|| text.to_owned());
            repaired.as_str()
        }
    };
    let start = text.find("<project").unwrap_or(0);
    let end = text[start..].find('>').map_or(start, |i| start + i);
    let line = text[..end].matches('\n').count();
    let line_text = text.lines().nth(line).unwrap_or("");
    (line as u32, line_text.chars().count() as u32)
}

/// The markers m2e puts on the `pom.xml` of a project whose dependencies
/// have no version or cannot be resolved.
fn pom_markers(pom: &Path, model: &Model, missing_artifacts: &[Dep]) -> Vec<Marker> {
    let Ok(text) = std::fs::read_to_string(pom) else {
        return Vec::new();
    };
    let marker = |message: String, range: (u32, u32, u32)| Marker {
        resource: Some(pom.to_path_buf()),
        message,
        severity: 1,
        code: "0".to_owned(),
        range: Some(range),
        derived: false,
    };
    let mut out = Vec::new();
    let missing_version: Vec<&Dep> = model
        .deps
        .iter()
        .filter(|d| d.version.is_none() && d.scope.as_deref() != Some("import"))
        .collect();
    for dep in &missing_version {
        let Some((line, column)) = dependency_position(&text, &dep.group, &dep.artifact) else {
            continue;
        };
        out.push(marker(
            format!(
                "Project build error: 'dependencies.dependency.version' for {}:{}:{} is missing.",
                dep.group,
                dep.artifact,
                dep.typ.as_deref().unwrap_or("jar")
            ),
            (line, 1, column + 11),
        ));
        let (root_line, root_length) = root_tag_end_line(&text);
        for (goal, execution, phase) in [
            ("resources", "default-resources", "process-resources"),
            ("testResources", "default-testResources", "process-test-resources"),
        ] {
            out.push(marker(
                MISSING_VERSION_MOJO
                    .replace("{GOAL}", goal)
                    .replace("{EXECUTION}", execution)
                    .replace("{PHASE}", phase)
                    .replace("{GROUP}", &dep.group)
                    .replace("{ARTIFACT}", &dep.artifact),
                (root_line, root_length.saturating_sub(8), root_length),
            ));
        }
    }
    for dep in missing_artifacts {
        let Some((line, column)) = dependency_position(&text, &dep.group, &dep.artifact) else {
            continue;
        };
        let classifier = match (dep.classifier.as_deref(), dep.typ.as_deref()) {
            (Some(c), _) => format!(":{c}"),
            (None, Some("test-jar")) => ":tests".to_owned(),
            _ => String::new(),
        };
        out.push(marker(
            format!(
                "Missing artifact {}:{}:jar{classifier}:{}",
                dep.group,
                dep.artifact,
                dep.version.as_deref().unwrap_or_default()
            ),
            (line, column, column + 11),
        ));
    }
    out
}

fn to_project(
    dir: &Path,
    name: &str,
    model: &Model,
    resolver: &mut Resolver,
    workspace: &HashMap<(String, String), String>,
    settings: &ImportSettings,
) -> Project {
    let mut project = Project::new(name, dir, ProjectKind::Maven);
    project.build_files = vec![dir.join(POM_FILE)];
    project.selected_profiles = resolver_configuration(dir, name);
    // m2e sets the project encoding from `project.build.sourceEncoding`.
    project.encoding = model.properties.get("project.build.sourceEncoding").cloned();
    if model.packaging == "pom" {
        project.natures = vec![super::MAVEN_NATURE.to_owned()];
        return project;
    }
    project.natures = vec![
        super::JAVA_NATURE.to_owned(),
        super::MAVEN_NATURE.to_owned(),
    ];
    let abs = |rel: &str| {
        if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            dir.join(rel)
        }
    };
    let classes = dir.join("target/classes");
    let test_classes = dir.join("target/test-classes");
    project.output = Some(classes.clone());

    let mut sources: Vec<ClasspathEntry> = Vec::new();
    let mut push = |e: ClasspathEntry| {
        if !sources.iter().any(|s| s.path == e.path) {
            sources.push(e);
        }
    };
    push(source_entry(
        &project,
        &abs(&model.source_dir),
        Some(&classes),
        false,
        false,
    ));
    for (s, t) in model.extra_sources.iter().filter(|(_, t)| !t) {
        push(source_entry(&project, &abs(s), Some(&classes), *t, false));
    }
    push(source_entry(
        &project,
        &abs("src/main/resources"),
        Some(&classes),
        false,
        true,
    ));
    push(source_entry(
        &project,
        &abs(&model.test_source_dir),
        Some(&test_classes),
        true,
        false,
    ));
    for (s, t) in model.extra_sources.iter().filter(|(_, t)| *t) {
        push(source_entry(
            &project,
            &abs(s),
            Some(&test_classes),
            *t,
            false,
        ));
    }
    push(source_entry(
        &project,
        &abs("src/test/resources"),
        Some(&test_classes),
        true,
        true,
    ));
    push(apt_entry(
        &project,
        &abs("target/generated-sources/annotations"),
        None,
        false,
    ));
    push(apt_entry(
        &project,
        &abs("target/generated-test-sources/test-annotations"),
        Some(&test_classes),
        true,
    ));
    project.classpath = sources;

    let target = compiler_levels(model).1;
    // `addJREClasspathContainer`: the execution environment of the target
    // level when a compatible VM exists, else the workspace default JRE.
    let compatible = settings
        .vm_version
        .as_deref()
        .is_none_or(|vm| super::compare_java_versions(&target, vm) != std::cmp::Ordering::Greater);
    let jre_path = if compatible {
        format!(
            "{}/org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType/JavaSE-{target}",
            super::JRE_CONTAINER
        )
    } else {
        super::JRE_CONTAINER.to_owned()
    };
    let mut jre = ClasspathEntry::new(EntryKind::Container, jre_path);
    jre.attributes
        .push(("maven.pomderived".into(), "true".into()));
    project.classpath.push(jre);

    let mut container = ClasspathEntry::new(EntryKind::Container, super::MAVEN_CONTAINER);
    container
        .attributes
        .push(("maven.pomderived".into(), "true".into()));
    let mut missing_artifacts: Vec<Dep> = Vec::new();
    for (dep, scope) in resolver.resolve(model) {
        if let Some(pname) = workspace.get(&(dep.group.clone(), dep.artifact.clone())) {
            let path = format!("/{pname}");
            if pname != name
                && !container
                    .children
                    .iter()
                    .any(|c| c.kind == EntryKind::Project && c.path == path)
            {
                let mut e = ClasspathEntry::new(EntryKind::Project, path);
                if scope == "test" {
                    e.attributes.push(("test".into(), "true".into()));
                }
                e.attributes
                    .push(("maven.pomderived".into(), "true".into()));
                container.children.push(e);
            }
            continue;
        }
        if dep.typ.as_deref().is_some_and(|t| t == "pom") {
            continue;
        }
        let Some(v) = dep.version.as_deref() else {
            continue;
        };
        let classifier = dep
            .classifier
            .as_deref()
            .or(if dep.typ.as_deref() == Some("test-jar") {
                Some("tests")
            } else {
                None
            });
        let jar = match resolver.artifact_jar(&dep.group, &dep.artifact, v, classifier) {
            Some(j) => Some(j),
            None => resolver.download_artifact(&dep.group, &dep.artifact, v, classifier, "jar"),
        };
        let jar = match jar {
            Some(jar) => jar,
            None => {
                let expected = resolver
                    .local_repo
                    .join(super::download::artifact_path(&dep.group, &dep.artifact, v, classifier, "jar"));
                if let Some(direct) = model
                    .deps
                    .iter()
                    .find(|d| d.group == dep.group && d.artifact == dep.artifact)
                {
                    missing_artifacts.push(direct.clone());
                }
                let mut e = ClasspathEntry::new(EntryKind::Library, expected.to_string_lossy().into_owned());
                e.attributes.push(("maven.groupId".into(), dep.group.clone()));
                e.attributes.push(("maven.artifactId".into(), dep.artifact.clone()));
                e.attributes.push(("maven.version".into(), v.to_owned()));
                e.attributes.push(("maven.scope".into(), scope.clone()));
                if scope == "test" {
                    e.attributes.push(("test".into(), "true".into()));
                }
                e.attributes.push(("maven.pomderived".into(), "true".into()));
                e.location = Some(expected);
                container.children.push(e);
                continue;
            }
        };
        let mut source = source_attachment(&jar);
        if source.is_none() && settings.maven.download_sources {
            source =
                resolver.download_artifact(&dep.group, &dep.artifact, v, Some("sources"), "jar");
        }
        let mut e = ClasspathEntry::new(EntryKind::Library, jar.to_string_lossy().into_owned());
        let javadoc = jar.with_file_name(format!("{}-{v}-javadoc.jar", dep.artifact));
        if javadoc.is_file() {
            e.attributes.push((
                "javadoc_location".into(),
                format!("jar:file:{}!/", javadoc.to_string_lossy()),
            ));
        }
        e.attributes
            .push(("maven.groupId".into(), dep.group.clone()));
        e.attributes
            .push(("maven.artifactId".into(), dep.artifact.clone()));
        e.attributes.push(("maven.version".into(), v.to_owned()));
        if let Some(c) = classifier {
            e.attributes.push(("maven.classifier".into(), c.to_owned()));
        }
        e.attributes.push(("maven.scope".into(), scope.clone()));
        if scope == "test" {
            e.attributes.push(("test".into(), "true".into()));
        }
        e.attributes
            .push(("maven.pomderived".into(), "true".into()));
        e.location = Some(jar);
        e.source_attachment = source;
        container.children.push(e);
    }
    project.classpath.push(container);
    project
        .markers
        .extend(pom_markers(&dir.join(POM_FILE), model, &missing_artifacts));

    let options = compiler_options(model, dir, name, settings.vm_version.as_deref());
    project.options = options;
    // Explicit raw libraries retained by m2e have authoritative attachments.
    if let Ok(xml) = std::fs::read_to_string(super::metadata::resolve(dir, name, ".classpath")) {
        let mut raw = project.clone();
        raw.classpath.clear();
        super::eclipse::apply_classpath(&mut raw, &xml);
        for entry in raw
            .classpath
            .into_iter()
            .filter(|e| e.kind == EntryKind::Library)
        {
            for container in &mut project.classpath {
                container.children.retain(|e| e.location != entry.location);
            }
            project.classpath.push(entry);
        }
    }
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_nested_properties() {
        let mut p = BTreeMap::new();
        p.insert("a".into(), "${b}-x".into());
        p.insert("b".into(), "1".into());
        assert_eq!(interpolate("v${a}", &p), "v1-x");
        assert_eq!(interpolate("${missing}", &p), "${missing}");
    }

    #[test]
    fn parses_compiler_config() {
        let pom = parse_pom(
            r#"<project><artifactId>x</artifactId><build><plugins><plugin>
            <artifactId>maven-compiler-plugin</artifactId>
            <configuration><source>1.8</source></configuration></plugin></plugins></build></project>"#,
        )
        .unwrap();
        assert_eq!(pom.compiler["source"], "1.8");
    }
}
