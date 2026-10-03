//! Maven project import: a Rust replacement for the m2e model resolution
//! jdt.ls relies on.  Builds the effective POM (parent chain, properties,
//! dependencyManagement incl. BOM imports) and resolves dependencies
//! transitively against the local repository — no Maven process needed.

use super::detect::FileDetector;
use super::{
    compliance_options, normalize_java_version, project_prefs, source_attachment, ImportSettings, Library, Project,
    ProjectKind, SourceFolder,
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
    pub extra_sources: Vec<(String, bool)>,
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
            .map(|e| (child_text(e, "groupId").unwrap_or_default(), child_text(e, "artifactId").unwrap_or_default()))
            .collect(),
    }
}

pub fn parse_pom(text: &str) -> Option<RawPom> {
    let doc = roxmltree::Document::parse(text).ok()?;
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
                    pom.properties
                        .insert(p.tag_name().name().to_owned(), p.text().unwrap_or("").trim().to_owned());
                }
            }
            "dependencies" => pom.deps = n.children().filter(|c| c.has_tag_name("dependency")).map(parse_dep).collect(),
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
                for key in ["source", "target", "release"] {
                    if let Some(v) = child_text(cfg, key) {
                        pom.compiler.entry(key.to_owned()).or_insert(v);
                    }
                }
            }
        } else if aid == "build-helper-maven-plugin" {
            for exec in plugin.descendants().filter(|e| e.has_tag_name("execution")) {
                let goal_test = exec
                    .descendants()
                    .any(|g| g.has_tag_name("goal") && g.text().is_some_and(|t| t.trim() == "add-test-source"));
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
    cache: HashMap<PathBuf, Option<Model>>,
}

impl Resolver {
    pub fn new() -> Self {
        Self { local_repo: local_repository(), cache: HashMap::new() }
    }

    pub fn repo_pom(&self, g: &str, a: &str, v: &str) -> PathBuf {
        self.repo_dir(g, a, v).join(format!("{a}-{v}.pom"))
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

    pub fn artifact_jar(&self, g: &str, a: &str, v: &str, classifier: Option<&str>) -> Option<PathBuf> {
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
                    n.starts_with(&format!("{a}-{base}-")) && n.ends_with(".jar") && !n.ends_with("-sources.jar")
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
            let rel = p.relative_path.clone().unwrap_or_else(|| "../pom.xml".to_owned());
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
            let repo = self.repo_pom(&p.group, &p.artifact, &p.version);
            self.model(&repo)
        });

        let mut m = parent_model.clone().unwrap_or_default();
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
        m.source_dir = raw.source_dir.clone().unwrap_or_else(|| "src/main/java".to_owned());
        m.test_source_dir = raw.test_source_dir.clone().unwrap_or_else(|| "src/test/java".to_owned());
        for (k, v) in &raw.properties {
            m.properties.insert(k.clone(), v.clone());
        }
        for (k, v) in &raw.compiler {
            m.compiler.insert(k.clone(), v.clone());
        }
        if let Some(p) = &raw.parent {
            m.properties.insert("project.parent.version".into(), p.version.clone());
            m.properties.insert("project.parent.groupId".into(), p.group.clone());
        }
        let builtins = [
            ("project.version", m.version.clone()),
            ("pom.version", m.version.clone()),
            ("version", m.version.clone()),
            ("project.groupId", m.group.clone()),
            ("pom.groupId", m.group.clone()),
            ("project.artifactId", m.artifact.clone()),
            ("project.basedir", path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned()),
            ("basedir", path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned()),
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
                let pom = self.repo_pom(&bom.group, &bom.artifact, v);
                if let Some(bm) = self.model(&pom) {
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
            if !deps.iter().any(|x| x.group == d.group && x.artifact == d.artifact) {
                deps.push(d);
            }
        }
        m.deps = deps;
        m.source_dir = interp(&m.source_dir);
        m.test_source_dir = interp(&m.test_source_dir);
        m.compiler = m.compiler.iter().map(|(k, v)| (k.clone(), interp(v))).collect();
        m.extra_sources = m.extra_sources.iter().map(|(s, t)| (interp(s), *t)).collect();
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
            if !seen.insert((dep.group.clone(), dep.artifact.clone(), dep.classifier.clone())) {
                continue;
            }
            let Some(version) = dep.version.clone() else { continue };
            out.push((dep.clone(), scope.clone()));
            if scope == "system" {
                continue;
            }
            let pom = self.repo_pom(&dep.group, &dep.artifact, &version);
            let Some(dm) = self.model(&pom) else { continue };
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
                if let Some(managed) = model.dep_mgmt.get(&(td.group.clone(), td.artifact.clone())) {
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

pub fn import(root: &Path, settings: &ImportSettings, claimed: &[PathBuf]) -> Vec<Project> {
    let mut detector = FileDetector::new(root, &["pom.xml"])
        .include_nested(false)
        .add_exclusions(["**/target"])
        .add_exclusions(&settings.exclusions);
    for c in claimed {
        detector = detector.add_exclusions([c.to_string_lossy().replace('\\', "\\\\")]);
    }
    let roots: Vec<PathBuf> = detector
        .scan()
        .into_iter()
        .filter(|d| !claimed.iter().any(|c| d.starts_with(c)))
        .collect();

    let mut resolver = Resolver::new();
    // Collect every pom (root poms + their module trees).
    let mut pom_dirs: Vec<(PathBuf, Model)> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<PathBuf> = roots.iter().map(|d| super::canonicalize_lenient(d)).collect();
    while let Some(dir) = stack.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let pom = dir.join("pom.xml");
        let Some(model) = resolver.model(&pom) else { continue };
        for module in model.modules.iter().rev() {
            let mdir = dir.join(module);
            let mdir = if mdir.is_file() { mdir.parent().map(Path::to_path_buf).unwrap_or(mdir) } else { mdir };
            stack.push(super::canonicalize_lenient(&mdir));
        }
        pom_dirs.push((dir, model));
    }
    pom_dirs.sort_by(|a, b| a.0.cmp(&b.0));

    let workspace_gas: HashMap<(String, String), String> = pom_dirs
        .iter()
        .map(|(_, m)| ((m.group.clone(), m.artifact.clone()), m.artifact.clone()))
        .collect();

    pom_dirs
        .into_iter()
        .map(|(dir, model)| to_project(&dir, &model, &mut resolver, &workspace_gas))
        .collect()
}

fn to_project(
    dir: &Path,
    model: &Model,
    resolver: &mut Resolver,
    workspace: &HashMap<(String, String), String>,
) -> Project {
    let mut source_folders = Vec::new();
    let mut push_src = |rel: &str, is_test: bool| {
        let p = if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { dir.join(rel) };
        if p.is_dir() && !source_folders.iter().any(|s: &SourceFolder| s.path == p) {
            source_folders.push(SourceFolder { path: p, is_test });
        }
    };
    if model.packaging != "pom" {
        push_src(&model.source_dir, false);
        for (s, t) in model.extra_sources.iter().filter(|(_, t)| !t) {
            push_src(s, *t);
        }
        push_src(&model.test_source_dir, true);
        for (s, t) in model.extra_sources.iter().filter(|(_, t)| *t) {
            push_src(s, *t);
        }
    }

    let mut libraries = Vec::new();
    let mut project_deps = Vec::new();
    for (dep, scope) in resolver.resolve(model) {
        if let Some(name) = workspace.get(&(dep.group.clone(), dep.artifact.clone())) {
            if name != &model.artifact && !project_deps.contains(name) {
                project_deps.push(name.clone());
            }
            continue;
        }
        if dep.typ.as_deref().is_some_and(|t| t == "pom") {
            continue;
        }
        let Some(v) = dep.version.as_deref() else { continue };
        let classifier = dep.classifier.as_deref().or(if dep.typ.as_deref() == Some("test-jar") { Some("tests") } else { None });
        if let Some(jar) = resolver.artifact_jar(&dep.group, &dep.artifact, v, classifier) {
            let source = source_attachment(&jar);
            libraries.push(Library { path: jar, source, is_test: scope == "test" });
        }
    }

    let version = ["release", "source"]
        .iter()
        .find_map(|k| model.compiler.get(*k).cloned())
        .or_else(|| model.properties.get("maven.compiler.release").cloned())
        .or_else(|| model.properties.get("maven.compiler.source").cloned())
        .and_then(|v| normalize_java_version(&v))
        .unwrap_or_else(|| "1.8".to_owned());
    // m2e overrides any compliance in the prefs file with the POM's.
    let mut options = project_prefs(dir);
    options.extend(compliance_options(&version));

    Project {
        name: model.artifact.clone(),
        root: dir.to_path_buf(),
        kind: ProjectKind::Maven,
        source_folders,
        libraries,
        project_deps,
        options,
    }
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
