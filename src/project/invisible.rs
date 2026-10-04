//! Invisible projects (`InvisibleProjectImporter`): jdt.ls wraps a plain
//! folder of Java files (no build descriptor) in a hidden project created in
//! its own workspace, which links the folder as `_` and whose source roots
//! are inferred from the package declarations of a trigger file and of
//! nearby files.

use super::{ClasspathEntry, EntryKind, ImportSettings, Project, ProjectKind, ReferencedLibraries, Workspace, WORKSPACE_LINK};
use std::path::{Path, PathBuf};

/// `InvisibleProjectBuildSupport.LIB_FOLDER`.
pub const LIB_FOLDER: &str = "lib";

/// Build descriptors that make a folder part of a "mature" project.
const BUILD_FILES: &[&str] = &["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts", ".project", ".classpath"];

/// `ProjectUtils.getWorkspaceInvisibleProjectName`.
pub fn project_name(root: &Path) -> String {
    let file_name = root.file_name().unwrap_or_default().to_string_lossy();
    let portable = root.to_string_lossy().replace('\\', "/");
    format!("{file_name}_{:x}", java_string_hash(&portable) as u32)
}

/// `java.lang.String.hashCode` over UTF-16 code units.
pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// Errors `loadInvisibleProject` reports (as `CoreException`s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvisibleError {
    AbsoluteOutputPath,
    AbsoluteSourcePath,
}

/// `InvisibleProjectImporter.loadInvisibleProject(javaFile, rootPath)`.
pub fn load_invisible_project(java_file: &Path, root: &Path, settings: &ImportSettings, ws: &Workspace) -> Option<Project> {
    try_load_invisible_project(java_file, root, settings, ws).ok().flatten()
}

pub fn try_load_invisible_project(
    java_file: &Path,
    root: &Path,
    settings: &ImportSettings,
    ws: &Workspace,
) -> Result<Option<Project>, InvisibleError> {
    if !ws.visible_projects_under(root).is_empty() {
        return Ok(None);
    }
    let package = package_name(java_file, root);
    let Some(source_dir) = infer_source_directory(java_file, &package) else { return Ok(None) };
    if !source_dir.starts_with(root) || is_part_of_mature_project(&source_dir) {
        return Ok(None);
    }
    let name = project_name(root);
    let mut project = Project::new(&name, root, ProjectKind::Invisible);
    project.location = settings.workspace_location(&name);
    project.natures = vec![super::JAVA_NATURE.to_owned(), super::UNMANAGED_FOLDER_NATURE.to_owned()];
    project.options = invisible_options(root);

    // Source paths.
    let mut source_paths: Vec<PathBuf> = Vec::new();
    match &settings.source_paths {
        Some(paths) => {
            let mut seen = Vec::new();
            for p in paths.iter().map(|p| p.trim().to_owned()) {
                if seen.contains(&p) {
                    continue;
                }
                seen.push(p.clone());
                if Path::new(&p).is_absolute() {
                    return Err(InvisibleError::AbsoluteSourcePath);
                }
                let folder = if p.is_empty() { root.to_path_buf() } else { root.join(&p) };
                if folder.is_dir() {
                    source_paths.push(folder);
                }
            }
        }
        None => {
            for s in collect_source_paths(java_file, &source_dir, root, settings, ws) {
                if !source_paths.contains(&s) {
                    source_paths.push(s);
                }
            }
        }
    }

    // Output path.
    let output = output_path(&project, settings.output_path.as_deref())?;
    project.output = Some(output.clone());

    // createJavaProject: the JRE container, then the resolved source entries.
    project.classpath.push(ClasspathEntry::new(EntryKind::Container, super::JRE_CONTAINER));
    let excluding: Vec<PathBuf> = Vec::new();
    project.classpath.extend(resolve_source_entries(&project, &source_paths, &excluding, &output));
    update_referenced_libraries(&mut project, &settings.referenced_libraries);
    Ok(Some(project))
}

/// JDT options of a new invisible project: those of a linked `.settings`
/// folder, with the preview options reset to the JDT defaults
/// (`ProjectUtils.createInvisibleProjectIfNotExist`).
fn invisible_options(root: &Path) -> std::collections::BTreeMap<String, String> {
    let mut options = super::project_prefs(root);
    if root.join(".settings").exists() {
        options.insert(super::ENABLE_PREVIEW.to_owned(), "disabled".to_owned());
        options.insert(super::REPORT_PREVIEW.to_owned(), "warning".to_owned());
    }
    options
}

/// `InvisibleProjectImporter.getOutputPath`.
pub fn output_path(project: &Project, output: Option<&str>) -> Result<PathBuf, InvisibleError> {
    let output = output.map(str::trim).unwrap_or("");
    if Path::new(output).is_absolute() {
        return Err(InvisibleError::AbsoluteOutputPath);
    }
    if output.is_empty() {
        return Ok(project.location.join("bin"));
    }
    Ok(project.root.join(output))
}

/// `ProjectUtils.resolveSourceClasspathEntries` for an invisible project.
pub fn resolve_source_entries(project: &Project, sources: &[PathBuf], excluding: &[PathBuf], output: &Path) -> Vec<ClasspathEntry> {
    let mut paths: Vec<(String, PathBuf)> = sources.iter().map(|s| (project.full_path(s), s.clone())).collect();
    // Child folders first.
    paths.sort_by(|a, b| b.0.cmp(&a.0));
    let output_full = project.full_path(output);
    let mut entries: Vec<ClasspathEntry> = Vec::new();
    for (full, loc) in paths {
        if entries.iter().any(|e| e.path == full) {
            continue;
        }
        let mut exclusions = Vec::new();
        for e in &entries {
            if let Some(rel) = relative_to(&e.path, &full) {
                exclusions.push(format!("{rel}/"));
            }
        }
        if let Some(rel) = relative_to(&output_full, &full) {
            exclusions.push(format!("{rel}/"));
        }
        for ex in excluding {
            let ex_full = project.full_path(ex);
            if let Some(rel) = relative_to(&ex_full, &full) {
                exclusions.push(format!("{rel}/"));
            }
        }
        let mut e = ClasspathEntry::new(EntryKind::Source, full);
        e.location = Some(loc);
        e.exclusions = exclusions;
        entries.push(e);
    }
    entries
}

/// `child` relative to `parent` when `parent` is a proper prefix (segment-wise).
fn relative_to(child: &str, parent: &str) -> Option<String> {
    let rest = child.strip_prefix(parent)?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// `InvisibleProjectImporter.collectSourcePaths`.
fn collect_source_paths(trigger: &Path, source_dir: &Path, root: &Path, settings: &ImportSettings, ws: &Workspace) -> Vec<PathBuf> {
    let mut out = vec![source_dir.to_path_buf()];
    let folders = if source_dir == root {
        // Direct child folders, skipping ancestors of the trigger file.
        child_dirs(root).into_iter().filter(|d| !trigger.starts_with(d)).collect::<Vec<_>>()
    } else {
        match source_dir.parent() {
            Some(parent) if parent.starts_with(root) => {
                child_dirs(parent).into_iter().filter(|d| d.file_name() != source_dir.file_name()).collect()
            }
            _ => Vec::new(),
        }
    };
    let detector = JavaFileDetector::new(settings, ws, &project_name(root));
    for f in detector.scan(&folders) {
        let package = package_name(&f, root);
        let Some(dir) = infer_source_directory(&f, &package) else { continue };
        if !dir.starts_with(root) || is_part_of_mature_project(&dir) {
            continue;
        }
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// `InvisibleProjectImporter.JavaFileDetector`: at most one Java file per
/// searched folder (depth ≤ 3), stopping at folders holding build files.
pub struct JavaFileDetector {
    exclusions: Vec<String>,
    project_paths: Vec<PathBuf>,
}

impl JavaFileDetector {
    pub fn new(settings: &ImportSettings, ws: &Workspace, current: &str) -> Self {
        let mut project_paths = Vec::new();
        for p in &ws.projects {
            if p.name == current {
                continue;
            }
            if p.kind != ProjectKind::Invisible {
                project_paths.push(p.location.clone());
            } else if let Some(parent) = p.root.parent() {
                // The parent of the linked folder of other invisible projects.
                project_paths.push(parent.to_path_buf());
            }
        }
        Self { exclusions: settings.exclusions.clone(), project_paths }
    }

    pub fn with_exclusions(exclusions: Vec<String>, project_paths: Vec<PathBuf>) -> Self {
        Self { exclusions, project_paths }
    }

    /// `Files.walkFileTree(folder, noneOf(FOLLOW_LINKS), 3, detector)` for each folder.
    pub fn scan(&self, folders: &[PathBuf]) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for f in folders {
            let mut stop = false;
            self.visit(f, 0, &mut found, &mut stop);
        }
        found
    }

    fn visit(&self, dir: &Path, depth: usize, found: &mut Vec<PathBuf>, stop: &mut bool) {
        if *stop || depth >= 3 {
            return;
        }
        if std::fs::symlink_metadata(dir).map(|m| m.file_type().is_symlink()).unwrap_or(false) && depth > 0 {
            return;
        }
        if self.is_excluded(dir) {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        let mut java = None;
        for f in entries.iter().filter(|p| p.is_file()) {
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            if BUILD_FILES.contains(&name.as_ref()) {
                *stop = true;
                return;
            }
            if java.is_none() && name.ends_with(".java") {
                java = Some(f.clone());
            }
        }
        if let Some(j) = java {
            found.push(j);
            *stop = true;
            return;
        }
        for d in entries.iter().filter(|p| p.is_dir()) {
            self.visit(d, depth + 1, found, stop);
            if *stop {
                return;
            }
        }
    }

    fn is_excluded(&self, dir: &Path) -> bool {
        if dir.file_name().is_none() {
            return true;
        }
        if self.project_paths.iter().any(|p| dir.starts_with(p)) {
            return true;
        }
        let s = dir.to_string_lossy().replace('\\', "/");
        let mut excluded = false;
        for pattern in &self.exclusions {
            let (include, pat) = match pattern.strip_prefix('!') {
                Some(p) => (true, p),
                None => (false, pattern.as_str()),
            };
            if super::detect::glob_to_regex(pat).is_some_and(|re| re.is_match(&s)) {
                excluded = !include;
            }
        }
        excluded
    }
}

/// `InvisibleProjectImporter.isPartOfMatureProject`.
pub fn is_part_of_mature_project(source: &Path) -> bool {
    let segments: Vec<String> = source.components().filter_map(|c| match c {
        std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    let Some(index) = segments.iter().rposition(|s| s == "src") else { return false };
    if index == 0 {
        return false;
    }
    let mut container = PathBuf::from("/");
    for s in &segments[..index] {
        container.push(s);
    }
    ["pom.xml", "build.gradle", "settings.gradle", "build.gradle.kts", "settings.gradle.kts"]
        .iter()
        .any(|f| container.join(f).exists())
}

/// `InvisibleProjectImporter.getPackageName(javaFile, workspaceRoot)`.
pub fn package_name(java_file: &Path, root: &Path) -> String {
    let mut file = java_file.to_path_buf();
    if let Ok(content) = std::fs::read_to_string(java_file) {
        if content.trim().is_empty() {
            match find_nearby_non_empty_file(java_file) {
                Some(f) => file = f,
                None => return infer_package_name_from_path(java_file, root),
            }
        }
    }
    std::fs::read_to_string(&file).map(|t| declared_package(&t)).unwrap_or_default()
}

fn find_nearby_non_empty_file(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    entries.into_iter().find(|p| {
        p.is_file()
            && p.extension().is_some_and(|e| e == "java")
            && p.file_name() != file.file_name()
            && std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false)
    })
}

/// `inferPackageNameFromPath` with the `src` prefix.
fn infer_package_name_from_path(file: &Path, root: &Path) -> String {
    let parent = file.parent().unwrap_or(file);
    let rel = parent.strip_prefix(root).unwrap_or(parent);
    let segments: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match segments.iter().position(|s| s == "src") {
        Some(i) => segments[i + 1..].join("."),
        None => segments.join("."),
    }
}

/// `InvisibleProjectImporter.inferSourceDirectory`.
pub fn infer_source_directory(file: &Path, package: &str) -> Option<PathBuf> {
    let dir = file.parent()?.to_path_buf();
    if package.trim().is_empty() {
        return Some(dir);
    }
    let segs: Vec<&str> = package.split('.').collect();
    let mut cur = dir.clone();
    for seg in segs.iter().rev() {
        if cur.file_name()?.to_string_lossy() != *seg {
            return None;
        }
        cur.pop();
    }
    Some(cur)
}

/// The package declared by `text` (`""` for the default package).
pub fn declared_package(text: &str) -> String {
    let stripped = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").unwrap().replace_all(text, " ");
    let re = regex::Regex::new(r"^\s*(?:@[\w.]+(?:\([^)]*\))?\s*)*package\s+([\w.\s]+?)\s*;").unwrap();
    re.captures(&stripped).map(|c| c[1].split_whitespace().collect::<String>()).unwrap_or_default()
}

/// Compatibility with the pre-`load_invisible_project` callers: the source
/// root of `file` (its directory minus the package path).
pub fn infer_source_directory_of(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file).ok()?;
    infer_source_directory(file, &declared_package(&text))
}

/// `UpdateClasspathJob.updateClasspath(project, referencedLibraries)`:
/// replace the library entries with the jars matching `libs`.
pub fn update_referenced_libraries(project: &mut Project, libs: &ReferencedLibraries) {
    let real = project.root.clone();
    let binaries = collect_binaries(&real, &libs.include, &libs.exclude);
    let sources: Vec<(PathBuf, PathBuf)> = libs
        .sources
        .iter()
        .map(|(b, s)| (real.join(expand_path(b)), real.join(expand_path(s))))
        .collect();
    // updateBinaries: keep the non-library entries, keep source attachments
    // of existing entries whose source still exists.
    let old: Vec<ClasspathEntry> = project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Library && e.source_attachment.as_deref().is_some_and(|s| s.exists()))
        .cloned()
        .collect();
    project.classpath.retain(|e| e.kind != EntryKind::Library);
    for bin in binaries {
        let mut source = match sources.iter().find(|(b, _)| *b == bin) {
            Some((_, s)) => Some(s.clone()),
            None => super::source_attachment(&bin),
        };
        if source.is_none() {
            source = old.iter().find(|e| e.location.as_deref() == Some(bin.as_path())).and_then(|e| e.source_attachment.clone());
        }
        let mut e = ClasspathEntry::new(EntryKind::Library, bin.to_string_lossy().into_owned());
        e.location = Some(bin);
        e.source_attachment = source;
        project.classpath.push(e);
    }
    project.derive_views();
}

/// `ResourceUtils.expandPath`: a leading `~` is the user home, and
/// `${user.home}` / `${java.home}` are system properties.
pub fn expand_path(path: &str) -> String {
    let home = super::eclipse::dirs_home().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
    let mut p = path.to_owned();
    if p == "~" || p.starts_with("~/") || p.starts_with("~\\") {
        p = format!("{home}{}", &p[1..]);
    }
    p = p.replace("${user.home}", &home);
    if let Some(jh) = java_home_property() {
        p = p.replace("${java.home}", &jh);
    }
    p
}

fn java_home_property() -> Option<String> {
    std::env::var("JDTLS_JAVA_HOME").ok().or_else(|| std::env::var("JAVA_HOME").ok())
}

/// `ProjectUtils.collectBinaries(projectDir, include, exclude)`.
pub fn collect_binaries(project_dir: &Path, include: &[String], exclude: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let excludes: Vec<PathBuf> = exclude.iter().map(|g| resolve_glob_path(project_dir, &expand_path(g))).collect();
    // groupGlobsByPrefix
    let mut groups: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for glob in include {
        let pattern = resolve_glob_path(project_dir, &expand_path(glob));
        let segs: Vec<String> = pattern.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let n = segs.iter().position(|s| s.contains('*') || s.contains('?')).unwrap_or(segs.len());
        let mut prefix = PathBuf::new();
        for s in &segs[..n] {
            prefix.push(s);
        }
        let remain = segs[n..].join("/");
        match groups.iter_mut().find(|(p, _)| *p == prefix) {
            Some((_, v)) => v.push(remain),
            None => groups.push((prefix, vec![remain])),
        }
    }
    for (base, patterns) in groups {
        if base.is_file() {
            if is_binary(&base) && !out.contains(&base) {
                out.push(base);
            }
            continue;
        }
        if !base.is_dir() {
            continue;
        }
        let sub_excludes: Vec<String> = excludes
            .iter()
            .map(|e| e.strip_prefix(&base).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| e.to_string_lossy().into_owned()))
            .collect();
        let includes: Vec<regex::Regex> = patterns.iter().filter_map(|p| ant_pattern(p)).collect();
        let excludes_re: Vec<regex::Regex> = sub_excludes.iter().filter_map(|p| ant_pattern(p)).collect();
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&base)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !is_default_excluded(e.file_name().to_string_lossy().as_ref()))
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect();
        files.sort();
        for f in files {
            let rel = f.strip_prefix(&base).unwrap().to_string_lossy().replace('\\', "/");
            if includes.iter().any(|r| r.is_match(&rel)) && !excludes_re.iter().any(|r| r.is_match(&rel)) && is_binary(&f) && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

fn is_default_excluded(name: &str) -> bool {
    matches!(name, ".git" | ".svn" | "CVS" | ".hg" | ".bzr" | "_darcs" | "SCCS" | "RCS" | "vssver.scc" | ".DS_Store")
}

fn is_binary(p: &Path) -> bool {
    let n = p.file_name().unwrap_or_default().to_string_lossy();
    n.ends_with(".jar") && !n.ends_with("-sources.jar")
}

/// `ProjectUtils.resolveGlobPath`.
pub fn resolve_glob_path(base: &Path, glob: &str) -> PathBuf {
    let p = Path::new(glob);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(glob)
    }
}

/// An Ant/plexus `DirectoryScanner` pattern as a regex over `/` paths.
pub fn ant_pattern(pattern: &str) -> Option<regex::Regex> {
    let mut pat = pattern.replace('\\', "/");
    if pat.ends_with('/') {
        pat.push_str("**");
    }
    let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let mut re = String::from("^");
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        if *seg == "**" {
            if last {
                // `dir/**` also matches `dir` itself.
                if re.ends_with('/') {
                    re.pop();
                    re.push_str("(?:/.*)?");
                } else {
                    re.push_str(".*");
                }
            } else {
                re.push_str("(?:[^/]*/)*");
            }
            continue;
        }
        for c in seg.chars() {
            match c {
                '*' => re.push_str("[^/]*"),
                '?' => re.push_str("[^/]"),
                other => re.push_str(&regex::escape(&other.to_string())),
            }
        }
        if !last {
            re.push('/');
        }
    }
    re.push('$');
    regex::Regex::new(&re).ok()
}

/// Kept for callers that predate `load_invisible_project`.
pub fn workspace_link(project: &Project) -> PathBuf {
    project.location.join(WORKSPACE_LINK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_hash_matches() {
        assert_eq!(java_string_hash("hello"), 99162322);
        assert_eq!(java_string_hash(""), 0);
    }

    #[test]
    fn ant_patterns() {
        let r = ant_pattern("**/*.jar").unwrap();
        assert!(r.is_match("a.jar"));
        assert!(r.is_match("x/y/a.jar"));
        let r = ant_pattern("sources/**").unwrap();
        assert!(r.is_match("sources/a.jar"));
        assert!(!r.is_match("a.jar"));
    }

    #[test]
    fn packages() {
        assert_eq!(declared_package("/* c */\npackage a.b ;\nclass X{}"), "a.b");
        assert_eq!(declared_package("class X{}"), "");
    }
}

// ─── Incremental updates of an existing invisible project ────────────────────

/// `getOutputPath(javaProject, outputPath, isUpdate=true)`.
pub fn update_output_path(project: &Project, output: Option<&str>) -> Result<PathBuf, String> {
    let output = output.map(str::trim).unwrap_or("");
    if Path::new(output).is_absolute() {
        return Err("The output path must be a relative path to the workspace.".to_owned());
    }
    if output.is_empty() {
        return Ok(project.location.join("bin"));
    }
    let full = project.root.join(output);
    if project.output.as_deref() == Some(full.as_path()) {
        return Ok(full);
    }
    if full.is_dir() && std::fs::read_dir(&full).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err("Cannot set the output path to a folder which is not empty, please provide a new path.".to_owned());
    }
    Ok(full)
}

/// `InvisibleProjectImporter.getSourcePaths(sourcePaths, workspaceLinkFolder)`.
pub fn source_paths_from_preferences(project: &Project, paths: Option<&[String]>) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for p in paths.unwrap_or(&[]).iter().map(|p| p.trim().to_owned()) {
        if seen.contains(&p) {
            continue;
        }
        seen.push(p.clone());
        if Path::new(&p).is_absolute() {
            return Err("The source path must be a relative path to the workspace.".to_owned());
        }
        let folder = if p.is_empty() { project.root.clone() } else { project.root.join(&p) };
        if folder.is_dir() {
            out.push(folder);
        }
    }
    Ok(out)
}

/// Replace the source entries (`resolveClassPathEntries` + `setRawClasspath`).
pub fn set_source_paths(project: &mut Project, sources: &[PathBuf], output: &Path) {
    let entries = resolve_source_entries(project, sources, &[], output);
    project.classpath.retain(|e| e.kind != EntryKind::Source);
    // Non-source entries first, then the sources; the libraries added by the
    // classpath update job stay last.
    let libs: Vec<ClasspathEntry> = project.classpath.iter().filter(|e| e.kind == EntryKind::Library).cloned().collect();
    project.classpath.retain(|e| e.kind != EntryKind::Library);
    project.classpath.extend(entries);
    project.classpath.extend(libs);
    project.output = Some(output.to_path_buf());
    project.derive_views();
}

/// `InvisibleProjectPreferenceChangeListener` for a `java.project.sourcePaths` change.
pub fn apply_source_paths_preference(project: &mut Project, paths: Option<&[String]>, output: Option<&str>) -> Result<(), String> {
    let sources = source_paths_from_preferences(project, paths)?;
    let output = update_output_path(project, output)?;
    set_source_paths(project, &sources, &output);
    Ok(())
}

/// `InvisibleProjectPreferenceChangeListener` for a `java.project.outputPath` change.
pub fn apply_output_path_preference(project: &mut Project, output: Option<&str>) -> Result<(), String> {
    let output = update_output_path(project, output)?;
    project.output = Some(output);
    Ok(())
}

/// `BaseDocumentLifeCycleHandler.needInferSourceRoot`: the unit is not on
/// the classpath, or every unit of its package folder declares an
/// unexpected package.
pub fn needs_source_root_inference(project: &Project, unit: &Path) -> bool {
    let Some(sf) = project.source_folder_for(unit) else { return true };
    let Some(dir) = unit.parent() else { return false };
    let expected: String = dir
        .strip_prefix(&sf.path)
        .map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("."))
        .unwrap_or_default();
    let units: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "java")).collect())
        .unwrap_or_default();
    !units.is_empty() && units.iter().all(|u| std::fs::read_to_string(u).map(|t| declared_package(&t) != expected).unwrap_or(false))
}

/// `InvisibleProjectImporter.inferSourceRoot(javaProject, unitPath)`:
/// whether a source root was added.
pub fn infer_source_root(project: &mut Project, unit: &Path, roots: &[PathBuf], settings: &ImportSettings) -> bool {
    let Some(root) = roots.iter().find(|r| project.root.starts_with(r)).cloned() else { return false };
    let package = package_name(unit, &root);
    let Some(source_dir) = infer_source_directory(unit, &package) else { return false };
    if !source_dir.starts_with(&root) || is_part_of_mature_project(&source_dir) {
        return false;
    }
    let mut sources: Vec<PathBuf> = project.classpath.iter().filter(|e| e.kind == EntryKind::Source).filter_map(|e| e.location.clone()).collect();
    if sources.contains(&source_dir) {
        return false;
    }
    sources.insert(0, source_dir);
    let Ok(output) = output_path(project, settings.output_path.as_deref()) else { return false };
    set_source_paths(project, &sources, &output);
    true
}
