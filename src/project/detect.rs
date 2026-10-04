//! Port of jdt.ls `BasicFileDetector`: finds directories containing build
//! descriptor files under a root, honouring glob exclusions.

use regex::Regex;
use std::path::{Path, PathBuf};

/// Default `java.import.exclusions` from jdt.ls `Preferences`.
pub const DEFAULT_IMPORT_EXCLUSIONS: &[&str] = &[
    "**/node_modules/**",
    "**/.metadata/**",
    "**/archetype-resources/**",
    "**/META-INF/maven/**",
];

pub struct FileDetector {
    root: PathBuf,
    file_names: Vec<String>,
    max_depth: usize,
    include_nested: bool,
    exclusions: Vec<String>,
}

impl FileDetector {
    pub fn new(root: &Path, file_names: &[&str]) -> Self {
        Self {
            // `Paths.get` drops trailing separators.
            root: root.components().collect(),
            file_names: file_names.iter().map(|s| s.to_string()).collect(),
            max_depth: 5,
            include_nested: true,
            exclusions: vec!["**/.metadata".to_owned()],
        }
    }

    pub fn include_nested(mut self, v: bool) -> Self {
        self.include_nested = v;
        self
    }

    /// `maxDepth`: directories deeper than this are not visited.
    pub fn max_depth(mut self, depth: usize) -> Self {
        assert!(depth > 0, "maxDepth must be > 0");
        self.max_depth = depth;
        self
    }

    pub fn add_exclusions<S: AsRef<str>>(mut self, ex: impl IntoIterator<Item = S>) -> Self {
        self.exclusions
            .extend(ex.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    pub fn scan(&self) -> Vec<PathBuf> {
        let matchers: Vec<(bool, Regex)> = self
            .exclusions
            .iter()
            .filter_map(|p| {
                let (include, pat) = match p.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, p.as_str()),
                };
                glob_to_regex(pat).map(|r| (include, r))
            })
            .collect();
        let has_inclusion = self.exclusions.iter().any(|e| e.starts_with('!'));
        let mut found = Vec::new();
        if self.root.is_dir() {
            let mut ancestors = Vec::new();
            self.walk(
                &self.root,
                0,
                &matchers,
                has_inclusion,
                &mut ancestors,
                &mut found,
            );
        }
        found
    }

    /// `Files.walkFileTree(dir, FOLLOW_LINKS, maxDepth, visitor)`: directories
    /// at depth < maxDepth are pre-visited; a link back to an ancestor is a
    /// `FileSystemLoopException`, which the visitor skips.
    fn walk(
        &self,
        dir: &Path,
        depth: usize,
        matchers: &[(bool, Regex)],
        has_inclusion: bool,
        ancestors: &mut Vec<(u64, u64)>,
        found: &mut Vec<PathBuf>,
    ) {
        if depth >= self.max_depth {
            return;
        }
        let key = file_key(dir);
        if let Some(k) = key {
            if ancestors.contains(&k) {
                return;
            }
        }
        if is_excluded(dir, matchers) {
            if !has_inclusion {
                return;
            }
        } else if self.file_names.iter().any(|f| dir.join(f).is_file()) {
            found.push(dir.to_path_buf());
            if !self.include_nested {
                return;
            }
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        if let Some(k) = key {
            ancestors.push(k);
        }
        for sub in subdirs {
            self.walk(&sub, depth + 1, matchers, has_inclusion, ancestors, found);
        }
        if key.is_some() {
            ancestors.pop();
        }
    }
}

#[cfg(unix)]
fn file_key(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_key(_p: &Path) -> Option<(u64, u64)> {
    None
}

fn is_excluded(dir: &Path, matchers: &[(bool, Regex)]) -> bool {
    if dir.file_name().is_none() {
        return true;
    }
    let s = dir.to_string_lossy().replace('\\', "/");
    let mut excluded = false;
    for (include, re) in matchers {
        if re.is_match(&s) {
            excluded = !include;
        }
    }
    excluded
}

/// Convert a Java NIO `glob:` pattern to an anchored regex.
pub fn glob_to_regex(glob: &str) -> Option<Regex> {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut in_group = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                re.push_str(".*");
                i += 1;
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '{' => {
                re.push_str("(?:");
                in_group = true;
            }
            '}' if in_group => {
                re.push(')');
                in_group = false;
            }
            ',' if in_group => re.push('|'),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                re.push_str(&regex::escape(&chars[i].to_string()));
            }
            '[' => {
                // Bracket expression: copy through, translating `!` negation.
                let mut j = i + 1;
                let mut class = String::from("[");
                if chars.get(j) == Some(&'!') {
                    class.push('^');
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    if chars[j] == '\\' || chars[j] == '[' {
                        class.push('\\');
                    }
                    class.push(chars[j]);
                    j += 1;
                }
                class.push(']');
                re.push_str(&class);
                i = j;
            }
            other => re.push_str(&regex::escape(&other.to_string())),
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).ok()
}

#[cfg(test)]
mod tests {
    use super::glob_to_regex;

    #[test]
    fn glob_semantics_match_java_nio() {
        let r = glob_to_regex("**/bin").unwrap();
        assert!(r.is_match("/a/b/bin"));
        assert!(!r.is_match("/a/bin/x"));
        let r = glob_to_regex("**/node_modules/**").unwrap();
        assert!(r.is_match("/a/node_modules/x"));
        assert!(!r.is_match("/a/node_modules"));
    }
}

/// A directory path used as an exclusion pattern (jdt.ls escapes `\` only).
pub fn path_pattern(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}
