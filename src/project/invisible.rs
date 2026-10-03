//! Invisible projects: jdt.ls wraps a plain folder of Java files (no build
//! descriptor) in a hidden project whose source roots are inferred from the
//! package declarations of the files found.

use super::{project_prefs, source_attachment, ImportSettings, Library, Project, ProjectKind, SourceFolder};
use std::path::{Path, PathBuf};

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

pub fn import(root: &Path, settings: &ImportSettings) -> Option<Project> {
    let root = super::canonicalize_lenient(root);
    let mut java_files = Vec::new();
    for e in walkdir::WalkDir::new(&root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !(e.file_type().is_dir() && (n.starts_with('.') && e.depth() > 0 || n == "node_modules" || n == "bin"))
        })
        .flatten()
    {
        if e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "java") {
            java_files.push(e.path().to_path_buf());
        }
    }
    if java_files.is_empty() {
        return None;
    }

    let mut source_folders: Vec<SourceFolder> = Vec::new();
    let mut add = |p: PathBuf| {
        if !source_folders.iter().any(|s| s.path == p) {
            source_folders.push(SourceFolder { path: p, is_test: false });
        }
    };
    if !settings.source_paths.is_empty() {
        for sp in &settings.source_paths {
            add(root.join(sp));
        }
    } else {
        for f in &java_files {
            if let Some(dir) = infer_source_directory(f) {
                if dir.starts_with(&root) {
                    add(dir);
                }
            }
        }
    }
    source_folders.sort_by(|a, b| a.path.cmp(&b.path));

    let mut libraries = Vec::new();
    for pattern in &settings.referenced_libraries {
        let full = root.join(pattern).to_string_lossy().replace('\\', "/");
        if let Some(re) = super::detect::glob_to_regex(&full) {
            for e in walkdir::WalkDir::new(&root).into_iter().flatten() {
                let s = e.path().to_string_lossy().replace('\\', "/");
                if e.file_type().is_file() && re.is_match(&s) && !s.ends_with("-sources.jar") {
                    let source = source_attachment(e.path());
                    libraries.push(Library { path: e.path().to_path_buf(), source, is_test: false });
                }
            }
        }
    }

    Some(Project {
        name: project_name(&root),
        root: root.clone(),
        kind: ProjectKind::Invisible,
        source_folders,
        libraries,
        project_deps: Vec::new(),
        options: project_prefs(&root),
    })
}

/// Source root of `file`: its directory minus the package path, if the
/// directory layout matches the declared package.
pub fn infer_source_directory(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file).ok()?;
    let pkg = declared_package(&text);
    let dir = file.parent()?.to_path_buf();
    if pkg.is_empty() {
        return Some(dir);
    }
    let mut cur = dir.clone();
    for seg in pkg.split('.').rev() {
        if cur.file_name()?.to_string_lossy() != seg {
            return Some(dir);
        }
        cur.pop();
    }
    Some(cur)
}

pub fn declared_package(text: &str) -> String {
    let re = regex::Regex::new(r"(?m)^\s*package\s+([\w.]+)\s*;").unwrap();
    // Skip comments before the package declaration.
    let stripped = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").unwrap().replace_all(text, "");
    re.captures(&stripped).map(|c| c[1].to_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_hash_matches() {
        assert_eq!(java_string_hash("hello"), 99162322);
        assert_eq!(java_string_hash(""), 0);
    }
}
