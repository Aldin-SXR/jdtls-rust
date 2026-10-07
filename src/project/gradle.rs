//! Gradle project import.  Instead of running the Gradle tooling API (as
//! Buildship does for jdt.ls), the build scripts are read statically: the
//! conventional layout, `sourceSets` overrides, Java version and declared
//! dependencies (resolved from the Gradle and Maven local caches).

use super::detect::FileDetector;
use super::maven::{Dep, Model, Resolver};
use super::{
    compliance_options, normalize_java_version, project_prefs, source_attachment, ClasspathEntry,
    EntryKind, ImportSettings, Library, Project, ProjectKind, SourceFolder, Workspace,
};
use regex::Regex;
use std::path::{Path, PathBuf};

const BUILD_FILES: &[&str] = &[
    "build.gradle",
    "settings.gradle",
    "build.gradle.kts",
    "settings.gradle.kts",
];

pub fn import(
    root: &Path,
    settings: &ImportSettings,
    ws: &Workspace,
    configs: Option<&[PathBuf]>,
) -> Vec<Project> {
    let dirs: Vec<PathBuf> = match configs {
        Some(files) => files
            .iter()
            .filter(|f| {
                f.file_name()
                    .is_some_and(|n| BUILD_FILES.contains(&n.to_string_lossy().as_ref()))
            })
            .filter_map(|f| f.parent().map(Path::to_path_buf))
            .collect(),
        None => {
            let mut detector = FileDetector::new(root, BUILD_FILES)
                .include_nested(false)
                .add_exclusions(["**/build", "**/bin"])
                .add_exclusions(&settings.exclusions);
            for p in ws
                .projects
                .iter()
                .filter(|p| p.kind != ProjectKind::Invisible)
            {
                detector =
                    detector.add_exclusions([p.location.to_string_lossy().replace('\\', "\\\\")]);
            }
            detector.scan()
        }
    };
    let mut out = Vec::new();
    for dir in dirs {
        let dir = super::canonicalize_lenient(&dir);
        out.extend(import_build(&dir));
    }
    out
}

fn read_script(dir: &Path, base: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(base))
        .or_else(|_| std::fs::read_to_string(dir.join(format!("{base}.kts"))))
        .ok()
}

fn strip_comments(s: &str) -> String {
    let block = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let line = Regex::new(r"(?m)^\s*//.*$").unwrap();
    line.replace_all(&block.replace_all(s, ""), "").into_owned()
}

/// Import the root build at `dir` plus subprojects listed in `settings.gradle`.
fn import_build(dir: &Path) -> Vec<Project> {
    let settings = read_script(dir, "settings.gradle")
        .map(|s| strip_comments(&s))
        .unwrap_or_default();
    let root_name = Regex::new(r#"rootProject\.name\s*=\s*['"]([^'"]+)['"]"#)
        .unwrap()
        .captures(&settings)
        .map(|c| c[1].to_owned())
        .unwrap_or_else(|| {
            dir.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });

    let mut subprojects: Vec<(String, PathBuf)> = Vec::new();
    let include_re = Regex::new(r#"(?m)^\s*include\s*\(?([^\n)]*)\)?"#).unwrap();
    let item_re = Regex::new(r#"['"]([^'"]+)['"]"#).unwrap();
    for cap in include_re.captures_iter(&settings) {
        for item in item_re.captures_iter(&cap[1]) {
            let path = item[1].trim_start_matches(':').to_owned();
            let rel: PathBuf = path.split(':').collect();
            let name = path.rsplit(':').next().unwrap_or(&path).to_owned();
            subprojects.push((name, dir.join(rel)));
        }
    }

    let root_script = read_script(dir, "build.gradle")
        .map(|s| strip_comments(&s))
        .unwrap_or_default();
    let mut projects = vec![load(dir, &root_name, &root_script, None)];
    for (name, sub) in subprojects {
        if !sub.is_dir() {
            continue;
        }
        let script = read_script(&sub, "build.gradle")
            .map(|s| strip_comments(&s))
            .unwrap_or_default();
        projects.push(load(&sub, &name, &script, Some(&root_script)));
    }
    projects
}

fn load(dir: &Path, name: &str, script: &str, root_script: Option<&str>) -> Project {
    let mut source_folders = Vec::new();
    let mut add = |p: PathBuf, is_test: bool| {
        if p.is_dir() && !source_folders.iter().any(|s: &SourceFolder| s.path == p) {
            source_folders.push(SourceFolder { path: p, is_test });
        }
    };
    let custom_main = source_set_dirs(script, "main");
    let custom_test = source_set_dirs(script, "test");
    if custom_main.is_empty() {
        add(dir.join("src/main/java"), false);
    }
    for d in custom_main {
        add(dir.join(d), false);
    }
    if custom_test.is_empty() {
        add(dir.join("src/test/java"), true);
    }
    for d in custom_test {
        add(dir.join(d), true);
    }

    let version = java_version(script).or_else(|| root_script.and_then(java_version));
    let mut options = project_prefs(dir);
    if let Some(v) = version {
        options.extend(compliance_options(&v));
    }

    let (libraries, project_deps) = dependencies(script);
    let mut p = Project::new(name, dir, ProjectKind::Gradle);
    p.natures = vec![
        super::JAVA_NATURE.to_owned(),
        super::GRADLE_NATURE.to_owned(),
    ];
    p.options = options;
    p.output = Some(dir.join("bin/default"));
    p.build_files = BUILD_FILES
        .iter()
        .map(|f| dir.join(f))
        .filter(|f| f.is_file())
        .collect();
    for sf in source_folders {
        let mut e = ClasspathEntry::new(EntryKind::Source, p.full_path(&sf.path));
        if sf.is_test {
            e.attributes.push(("gradle_scope".into(), "test".into()));
            e.attributes
                .push(("gradle_used_by_scope".into(), "test".into()));
            e.attributes.push(("test".into(), "true".into()));
            e.output = Some(dir.join("bin/test"));
        } else {
            e.attributes.push(("gradle_scope".into(), "main".into()));
            e.attributes
                .push(("gradle_used_by_scope".into(), "main,test".into()));
            e.output = Some(dir.join("bin/main"));
        }
        e.location = Some(sf.path);
        p.classpath.push(e);
    }
    p.classpath.push(ClasspathEntry::new(
        EntryKind::Container,
        super::JRE_CONTAINER,
    ));
    let mut container = ClasspathEntry::new(EntryKind::Container, super::GRADLE_CONTAINER);
    for lib in libraries {
        let mut e =
            ClasspathEntry::new(EntryKind::Library, lib.path.to_string_lossy().into_owned());
        if lib.is_test {
            e.attributes
                .push(("gradle_used_by_scope".into(), "test".into()));
            e.attributes.push(("test".into(), "true".into()));
        } else {
            e.attributes
                .push(("gradle_used_by_scope".into(), "main,test".into()));
        }
        e.location = Some(lib.path);
        e.source_attachment = lib.source;
        container.children.push(e);
    }
    for dep in project_deps {
        let mut e = ClasspathEntry::new(EntryKind::Project, format!("/{dep}"));
        e.attributes
            .push(("gradle_used_by_scope".into(), "main,test".into()));
        container.children.push(e);
    }
    p.classpath.push(container);
    p
}

fn source_set_dirs(script: &str, set: &str) -> Vec<String> {
    let re = Regex::new(&format!(r#"(?s)\b{set}\s*\{{\s*java\s*\{{([^}}]*)\}}"#)).unwrap();
    let item = Regex::new(r#"['"]([^'"]+)['"]"#).unwrap();
    re.captures_iter(script)
        .flat_map(|c| {
            item.captures_iter(&c[1])
                .map(|i| i[1].to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn java_version(script: &str) -> Option<String> {
    let patterns = [
        r#"languageVersion(?:\.set\()?\s*=?\s*JavaLanguageVersion\.of\(\s*(\d+)\s*\)"#,
        r#"options\.release(?:\.set\()?\s*=?\s*(\d+)"#,
        r#"sourceCompatibility\s*=\s*([^\s\n]+)"#,
    ];
    patterns.iter().find_map(|p| {
        Regex::new(p)
            .unwrap()
            .captures(script)
            .and_then(|c| normalize_java_version(&c[1]))
    })
}

const CONFIGS: &str = "implementation|api|compile|compileOnly|runtimeOnly|runtime|testImplementation|testCompile|testCompileOnly|testRuntimeOnly|annotationProcessor";

fn dependencies(script: &str) -> (Vec<Library>, Vec<String>) {
    let mut libs = Vec::new();
    let mut projects = Vec::new();
    let notation = Regex::new(&format!(
        r#"(?m)^\s*({CONFIGS})\s*\(?\s*['"]([^'":]+):([^'":]+):([^'":@]+)(?::([^'"@]+))?['"]"#
    ))
    .unwrap();
    let map = Regex::new(&format!(r#"(?m)^\s*({CONFIGS})\s*\(?\s*group\s*:\s*['"]([^'"]+)['"]\s*,\s*name\s*:\s*['"]([^'"]+)['"]\s*,\s*version\s*:\s*['"]([^'"]+)['"]"#)).unwrap();
    let project = Regex::new(&format!(
        r#"(?m)^\s*({CONFIGS})\s*\(?\s*project\s*\(\s*(?:path\s*:\s*)?['"]:?([^'"]+)['"]"#
    ))
    .unwrap();
    let files = Regex::new(&format!(
        r#"(?m)^\s*({CONFIGS})\s*\(?\s*files\s*\(([^)]*)\)"#
    ))
    .unwrap();

    let mut declared: Vec<(Dep, bool)> = Vec::new();
    for c in notation.captures_iter(script) {
        declared.push((
            Dep {
                group: c[2].to_owned(),
                artifact: c[3].to_owned(),
                version: Some(c[4].to_owned()),
                classifier: c.get(5).map(|m| m.as_str().to_owned()),
                ..Default::default()
            },
            c[1].starts_with("test"),
        ));
    }
    for c in map.captures_iter(script) {
        declared.push((
            Dep {
                group: c[2].to_owned(),
                artifact: c[3].to_owned(),
                version: Some(c[4].to_owned()),
                ..Default::default()
            },
            c[1].starts_with("test"),
        ));
    }
    for c in project.captures_iter(script) {
        let name = c[2].rsplit(':').next().unwrap_or(&c[2]).to_owned();
        if !projects.contains(&name) {
            projects.push(name);
        }
    }
    let _ = files;

    let mut resolver = Resolver::new();
    for (dep, is_test) in declared {
        let model = Model {
            deps: vec![dep],
            ..Default::default()
        };
        for (d, _) in resolver.resolve(&model) {
            let Some(v) = d.version.as_deref() else {
                continue;
            };
            let jar =
                gradle_cache_jar(&d.group, &d.artifact, v, d.classifier.as_deref())
                    .or_else(|| resolver.artifact_jar(&d.group, &d.artifact, v, d.classifier.as_deref()))
                    // Buildship lets Gradle resolve (download) the declared
                    // dependencies; fetch the ones no local cache holds.
                    .or_else(|| {
                        resolver.download_artifact(&d.group, &d.artifact, v, d.classifier.as_deref(), "jar")
                    });
            if let Some(jar) = jar {
                if !libs.iter().any(|l: &Library| l.path == jar) {
                    let source = source_attachment(&jar)
                        .or_else(|| gradle_cache_sources(&d.group, &d.artifact, v));
                    libs.push(Library {
                        path: jar,
                        source,
                        is_test,
                    });
                }
            }
        }
    }
    (libs, projects)
}

fn gradle_cache_dir(g: &str, a: &str, v: &str) -> Option<PathBuf> {
    let home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| super::eclipse::dirs_home().map(|h| h.join(".gradle")))?;
    let dir = home
        .join("caches/modules-2/files-2.1")
        .join(g)
        .join(a)
        .join(v);
    dir.is_dir().then_some(dir)
}

fn gradle_cache_find(g: &str, a: &str, v: &str, file: &str) -> Option<PathBuf> {
    let dir = gradle_cache_dir(g, a, v)?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path().join(file))
        .find(|p| p.is_file())
}

fn gradle_cache_jar(g: &str, a: &str, v: &str, classifier: Option<&str>) -> Option<PathBuf> {
    let file = match classifier {
        Some(c) => format!("{a}-{v}-{c}.jar"),
        None => format!("{a}-{v}.jar"),
    };
    gradle_cache_find(g, a, v, &file)
}

fn gradle_cache_sources(g: &str, a: &str, v: &str) -> Option<PathBuf> {
    gradle_cache_find(g, a, v, &format!("{a}-{v}-sources.jar"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_java_version() {
        assert_eq!(
            java_version("sourceCompatibility = 1.8").as_deref(),
            Some("1.8")
        );
        assert_eq!(
            java_version("java { toolchain { languageVersion = JavaLanguageVersion.of(17) } }")
                .as_deref(),
            Some("17")
        );
        assert_eq!(
            java_version("sourceCompatibility = JavaVersion.VERSION_11").as_deref(),
            Some("11")
        );
    }
}
