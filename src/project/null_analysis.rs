//! Annotation-based null analysis configuration
//! (`Preferences.updateAnnotationNullAnalysisOptions`): with
//! `java.compile.nullAnalysis.mode` `automatic`, every Java project whose
//! classpath holds one of the configured nonnull/nullable annotation types
//! gets JDT's null analysis enabled with those types.

use super::{EntryKind, Project, ProjectKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// `java.compile.nullAnalysis.*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NullAnalysisSettings {
    pub nonnull: Vec<String>,
    pub nullable: Vec<String>,
    pub nonnullbydefault: Vec<String>,
    /// `automatic`, `interactive` or `disabled` (the jdt.ls default).
    pub mode: String,
}

pub const ANNOTATION_NULL_ANALYSIS: &str = "org.eclipse.jdt.core.compiler.annotation.nullanalysis";
const NONNULL_NAME: &str = "org.eclipse.jdt.core.compiler.annotation.nonnull";
const NULLABLE_NAME: &str = "org.eclipse.jdt.core.compiler.annotation.nullable";
const NONNULL_BY_DEFAULT_NAME: &str = "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault";
const PB_NULL_REFERENCE: &str = "org.eclipse.jdt.core.compiler.problem.nullReference";
const PB_POTENTIAL_NULL_REFERENCE: &str =
    "org.eclipse.jdt.core.compiler.problem.potentialNullReference";
const PB_NULL_SPECIFICATION_VIOLATION: &str =
    "org.eclipse.jdt.core.compiler.problem.nullSpecViolation";
const PB_NULL_ANNOTATION_INFERENCE_CONFLICT: &str =
    "org.eclipse.jdt.core.compiler.problem.nullAnnotationInferenceConflict";
const PB_MISSING_NONNULL_BY_DEFAULT: &str =
    "org.eclipse.jdt.core.compiler.problem.missingNonNullByDefaultAnnotation";
const PB_SYNTACTIC_NULL_ANALYSIS_FOR_FIELDS: &str =
    "org.eclipse.jdt.core.compiler.problem.syntacticNullAnalysisForFields";

/// `initializeNullAnalysisClasspathStorage`: known annotation types and the
/// artifact (`group:artifact`) that provides them.
const KNOWN: &[(&str, &str)] = &[
    (
        "javax.annotation.Nonnull",
        "com.google.code.findbugs:jsr305",
    ),
    (
        "javax.annotation.Nullable",
        "com.google.code.findbugs:jsr305",
    ),
    (
        "javax.annotation.ParametersAreNonnullByDefault",
        "com.google.code.findbugs:jsr305",
    ),
    (
        "org.eclipse.jdt.annotation.NonNull",
        "org.eclipse.jdt:org.eclipse.jdt.annotation",
    ),
    (
        "org.eclipse.jdt.annotation.Nullable",
        "org.eclipse.jdt:org.eclipse.jdt.annotation",
    ),
    (
        "org.eclipse.jdt.annotation.NonNullByDefault",
        "org.eclipse.jdt:org.eclipse.jdt.annotation",
    ),
    (
        "org.springframework.lang.NonNull",
        "org.springframework:spring-core",
    ),
    (
        "org.springframework.lang.Nullable",
        "org.springframework:spring-core",
    ),
    (
        "org.springframework.lang.NonNullApi",
        "org.springframework:spring-core",
    ),
    (
        "io.micrometer.core.lang.NonNull",
        "io.micrometer:micrometer-core",
    ),
    (
        "io.micrometer.core.lang.Nullable",
        "io.micrometer:micrometer-core",
    ),
    (
        "io.micrometer.core.lang.NonNullApi",
        "io.micrometer:micrometer-core",
    ),
    (
        "org.jetbrains.annotations.NotNull",
        "org.jetbrains:annotations",
    ),
    (
        "org.jetbrains.annotations.Nullable",
        "org.jetbrains:annotations",
    ),
    ("org.jspecify.annotations.NonNull", "org.jspecify:jspecify"),
    ("org.jspecify.annotations.Nullable", "org.jspecify:jspecify"),
    (
        "org.jspecify.annotations.NullMarked",
        "org.jspecify:jspecify",
    ),
];

/// `getClasspathSubStringFromArtifact`: Gradle (`group/artifact`) and Maven
/// (`g/r/o/u/p/artifact`) style path fragments.
fn classpath_substrings(artifact: &str) -> Vec<String> {
    let Some((group, id)) = artifact.split_once(':') else {
        return Vec::new();
    };
    vec![
        format!("{group}/{id}"),
        format!("{}/{id}", group.replace('.', "/")),
    ]
}

/// The non-test runtime classpath entries of `project` (jars and source folders).
fn runtime_classpath(project: &Project) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    fn walk(entries: &[super::ClasspathEntry], out: &mut Vec<(PathBuf, bool)>) {
        for e in entries {
            match e.kind {
                EntryKind::Library | EntryKind::Variable => {
                    if e.attribute("maven.scope") == Some("runtime") {
                        continue;
                    }
                    if let Some(l) = &e.location {
                        out.push((l.clone(), e.is_test()));
                    }
                }
                EntryKind::Container => walk(&e.children, out),
                _ => {}
            }
        }
    }
    walk(&project.classpath, &mut out);
    out
}

/// `findTypeInProject`: the type in a source folder or a library that is not a test entry.
fn find_type_in_project(project: &Project, fqn: &str) -> bool {
    let rel = format!("{}.java", fqn.replace('.', "/"));
    if project
        .source_folders
        .iter()
        .any(|sf| !sf.is_test && sf.path.join(&rel).is_file())
    {
        return true;
    }
    runtime_classpath(project)
        .iter()
        .any(|(jar, test)| !test && jar.is_file() && super::jar::has_class(jar, fqn))
}

/// `getAnnotationType`: the first configured type available to `project`.
fn annotation_type(project: &Project, types: &[String]) -> Option<String> {
    let classpath: Vec<String> = runtime_classpath(project)
        .into_iter()
        .filter(|(_, test)| !test)
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect();
    for t in types {
        if let Some((_, artifact)) = KNOWN.iter().find(|(k, _)| k == t) {
            let subs = classpath_substrings(artifact);
            if classpath
                .iter()
                .any(|cp| subs.iter().any(|s| cp.contains(s.as_str())))
            {
                return Some(t.clone());
            }
        }
        if find_type_in_project(project, t) {
            return Some(t.clone());
        }
    }
    None
}

/// `generateProjectNullAnalysisOptions`.
fn options(
    nonnull: Option<String>,
    nullable: Option<String>,
    nonnullbydefault: Option<String>,
) -> BTreeMap<String, String> {
    let mut o = BTreeMap::new();
    match (nonnull, nullable, nonnullbydefault) {
        (Some(nn), Some(nl), Some(nd)) => {
            o.insert(ANNOTATION_NULL_ANALYSIS.into(), "enabled".into());
            o.insert(NONNULL_NAME.into(), nn);
            o.insert(NULLABLE_NAME.into(), nl);
            o.insert(NONNULL_BY_DEFAULT_NAME.into(), nd);
            o.insert(PB_NULL_REFERENCE.into(), "warning".into());
            o.insert(PB_POTENTIAL_NULL_REFERENCE.into(), "warning".into());
            o.insert(PB_NULL_SPECIFICATION_VIOLATION.into(), "warning".into());
            o.insert(
                PB_NULL_ANNOTATION_INFERENCE_CONFLICT.into(),
                "warning".into(),
            );
            o.insert(PB_MISSING_NONNULL_BY_DEFAULT.into(), "ignore".into());
            o.insert(
                PB_SYNTACTIC_NULL_ANALYSIS_FOR_FIELDS.into(),
                "enabled".into(),
            );
        }
        _ => {
            o.insert(ANNOTATION_NULL_ANALYSIS.into(), "disabled".into());
            for k in [
                NONNULL_NAME,
                NULLABLE_NAME,
                NONNULL_BY_DEFAULT_NAME,
                PB_NULL_REFERENCE,
                PB_POTENTIAL_NULL_REFERENCE,
                PB_NULL_SPECIFICATION_VIOLATION,
                PB_NULL_ANNOTATION_INFERENCE_CONFLICT,
                PB_SYNTACTIC_NULL_ANALYSIS_FOR_FIELDS,
            ] {
                if let Some((_, v)) = super::jdt_defaults::WORKSPACE_DEFAULTS
                    .iter()
                    .find(|(key, _)| *key == k)
                {
                    o.insert(k.to_owned(), (*v).to_owned());
                }
            }
        }
    }
    o
}

/// `updateAnnotationNullAnalysisOptions(javaProject, enabled)`: whether the
/// project's options changed.
pub fn update_project(
    project: &mut Project,
    settings: &NullAnalysisSettings,
    vm: Option<&str>,
) -> bool {
    if project.kind == ProjectKind::Default || !project.is_java() {
        return false;
    }
    let enabled = settings.mode == "automatic";
    let wanted = if enabled
        && !(settings.nonnull.is_empty()
            && settings.nullable.is_empty()
            && settings.nonnullbydefault.is_empty())
    {
        let nonnull = annotation_type(project, &settings.nonnull);
        let nullable = annotation_type(project, &settings.nullable);
        let mut nonnullbydefault = annotation_type(project, &settings.nonnullbydefault);
        if nonnullbydefault.is_none() && nonnull.is_some() && nullable.is_some() {
            // there is not NonNullByDefault in org.jetbrains:annotations
            nonnullbydefault = Some("org.eclipse.jdt.annotation.NonNullByDefault".to_owned());
        }
        options(nonnull, nullable, nonnullbydefault)
    } else {
        options(None, None, None)
    };
    let should_update = !wanted
        .iter()
        .all(|(k, v)| super::effective_option(project, k, vm).as_deref() == Some(v.as_str()));
    if should_update {
        project.options.extend(wanted);
    }
    should_update
}
