//! Class files of libraries and the JDK: the jdt.ls `jdt://contents/...`
//! URI scheme (`JDTUtils.toUri(IClassFile)`) and its inverse
//! (`JDTUtils.resolveClassFile`).
//!
//! A class-file URI is
//!
//! ```text
//! jdt://contents/<root name>/<package>/<source file name>?<handle identifier>
//! ```
//!
//! where the root name is the jar's file name (or the JDK module name), the
//! file name is the class file's `SourceFile` attribute (falling back to the
//! class file name), and the query is the JDT handle identifier of the class
//! file — `=<project>/<root path>[`<module>][=/<attr>=/<value>=/]*<<package>(<Name>.class`
//! with JDT memento escaping — quoted like `java.net.URI`'s multi-argument
//! constructor and then passed through `JDTUtils.cleanupURL`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::Url;

pub const JDT_SCHEME: &str = "jdt";

/// Bridge-side description of a class file (`ClassFileService.ClassFileDesc`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassFileDesc {
    /// Absolute jar/directory path, or the JDK's `lib/jrt-fs.jar`.
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default)]
    pub package_name: String,
    pub class_file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file_name: Option<String>,
}

/// A class file as addressed by a jdt.ls handle identifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassFileRef {
    pub project: String,
    /// Package fragment root path as it appears in the memento: an absolute
    /// path for external jars and the JDK, project-relative for jars inside
    /// the project, `/<project>/...` for jars of another workspace project.
    pub root_path: String,
    pub module: Option<String>,
    /// Classpath entry extra attributes (e.g. m2e's `maven.pomderived`).
    pub attributes: Vec<(String, String)>,
    pub package: String,
    pub class_file: String,
    /// `SourceFile` attribute, used for the URI path.
    pub source_file_name: Option<String>,
}

// ── Memento escaping (`MementoTokenizer.escape`) ─────────────────────────────

const MEMENTO_SPECIAL: &[char] = &[
    '!', '#', '%', '\'', '(', '/', '<', '=', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~',
];

fn escape_memento(out: &mut String, s: &str) {
    for c in s.chars() {
        if MEMENTO_SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
}

impl ClassFileRef {
    /// `IClassFile.getHandleIdentifier()`.
    pub fn handle_identifier(&self) -> String {
        let mut s = String::from("=");
        escape_memento(&mut s, &self.project);
        s.push('/');
        escape_memento(&mut s, &self.root_path);
        if let Some(m) = &self.module {
            s.push('`');
            escape_memento(&mut s, m);
        }
        for (k, v) in &self.attributes {
            s.push_str("=/");
            escape_memento(&mut s, k);
            s.push_str("=/");
            escape_memento(&mut s, v);
            s.push_str("=/");
        }
        s.push('<');
        escape_memento(&mut s, &self.package);
        s.push('(');
        escape_memento(&mut s, &self.class_file);
        s
    }

    /// Element name of the package fragment root (`jarPath.lastSegment()`,
    /// or the module name for the JDK).
    pub fn root_name(&self) -> String {
        if let Some(m) = &self.module {
            return m.clone();
        }
        self.root_path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned()
    }

    /// `JDTUtils.toUri(IClassFile)`.
    pub fn to_uri(&self) -> String {
        let file_name = self.source_file_name.clone().unwrap_or_else(|| self.class_file.clone());
        let mut path = format!("/{}", self.root_name());
        if !self.package.trim().is_empty() {
            path.push('/');
            path.push_str(&self.package);
        }
        path.push('/');
        path.push_str(&file_name);
        let handle = self.handle_identifier();
        let mut query = handle.clone();
        if !handle.contains(&self.class_file) {
            query.push_str("&element=");
            query.push_str(&self.class_file);
        }
        let uri = format!("{JDT_SCHEME}://contents{}?{}", quote(&path, is_path_char), quote(&query, is_query_char));
        cleanup_url(uri)
    }

    /// `JDTUtils.resolveClassFile(uri)`: parse a `jdt://contents/...` URI.
    pub fn parse(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("jdt://contents")?;
        let (path, query) = rest.split_once('?')?;
        let query = query.split('#').next().unwrap_or("");
        let mut handle = percent_decode(query);
        if let Some(i) = handle.find("&element=") {
            handle.truncate(i);
        }
        let mut r = parse_handle(&handle)?;
        let path = percent_decode(path);
        let file = path.rsplit('/').next().unwrap_or("");
        if !file.is_empty() && file != r.class_file {
            r.source_file_name = Some(file.to_owned());
        }
        Some(r)
    }
}

/// Tokenize a class-file handle identifier.
fn parse_handle(h: &str) -> Option<ClassFileRef> {
    let chars: Vec<char> = h.chars().collect();
    let mut i = 0;
    if chars.first() != Some(&'=') {
        return None;
    }
    i += 1;
    let read = |i: &mut usize, stops: &[char]| -> String {
        let mut s = String::new();
        while *i < chars.len() {
            let c = chars[*i];
            if c == '\\' && *i + 1 < chars.len() {
                s.push(chars[*i + 1]);
                *i += 2;
                continue;
            }
            if stops.contains(&c) {
                break;
            }
            // `=/` starts a classpath attribute.
            if c == '=' && chars.get(*i + 1) == Some(&'/') && stops.contains(&'=') {
                break;
            }
            s.push(c);
            *i += 1;
        }
        s
    };
    let project = read(&mut i, &['/']);
    if chars.get(i) != Some(&'/') {
        return None;
    }
    i += 1;
    let root_path = read(&mut i, &['`', '<', '=']);
    let mut module = None;
    if chars.get(i) == Some(&'`') {
        i += 1;
        module = Some(read(&mut i, &['<', '=']));
    }
    let mut attributes = Vec::new();
    while chars.get(i) == Some(&'=') && chars.get(i + 1) == Some(&'/') {
        i += 2;
        let name = read(&mut i, &['=']);
        if !(chars.get(i) == Some(&'=') && chars.get(i + 1) == Some(&'/')) {
            return None;
        }
        i += 2;
        let value = read(&mut i, &['=']);
        if !(chars.get(i) == Some(&'=') && chars.get(i + 1) == Some(&'/')) {
            return None;
        }
        i += 2;
        attributes.push((name, value));
    }
    if chars.get(i) != Some(&'<') {
        return None;
    }
    i += 1;
    let package = read(&mut i, &['(']);
    if chars.get(i) != Some(&'(') {
        return None;
    }
    i += 1;
    let class_file = read(&mut i, &[]);
    Some(ClassFileRef { project, root_path, module, attributes, package, class_file, source_file_name: None })
}

// ── java.net.URI quoting ─────────────────────────────────────────────────────

fn is_unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c)
}

fn is_path_char(c: u8) -> bool {
    is_unreserved(c) || b",;:$&+=@/".contains(&c)
}

fn is_query_char(c: u8) -> bool {
    is_unreserved(c) || b";/?:@&=+$,[]".contains(&c)
}

/// Quote illegal characters (and every non-ASCII character, like
/// `toASCIIString`) as `%XX` of their UTF-8 bytes.
fn quote(s: &str, legal: fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b < 0x80 && legal(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `JDTUtils.cleanupURL`.
fn cleanup_url(url: String) -> String {
    if url.contains('(') {
        return url.replace('(', "%28");
    }
    if url.contains(')') {
        return url.replace(')', "%29");
    }
    url
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn is_class_file_uri(uri: &Url) -> bool {
    uri.scheme() == JDT_SCHEME && uri.host_str() == Some("contents")
}

// ── Project mapping ──────────────────────────────────────────────────────────

/// The memento path of a package fragment root (`PackageFragmentRoot.getHandleMemento`):
/// project-relative for jars inside the project, `/<project>/...` for jars
/// inside another workspace project, absolute otherwise.
pub fn memento_root_path(root: &Path, project_root: Option<&Path>, workspace: &[(String, PathBuf)]) -> String {
    if let Some(pr) = project_root {
        if let Ok(rel) = root.strip_prefix(pr) {
            return rel.to_string_lossy().into_owned();
        }
    }
    for (name, r) in workspace {
        if let Ok(rel) = root.strip_prefix(r) {
            return format!("/{name}/{}", rel.to_string_lossy());
        }
    }
    root.to_string_lossy().into_owned()
}

/// Resolve a memento root path back to an absolute path.
pub fn resolve_root_path(root_path: &str, project_root: Option<&Path>, workspace: &[(String, PathBuf)]) -> PathBuf {
    let p = PathBuf::from(root_path);
    if p.is_absolute() {
        if p.exists() {
            return p;
        }
        // `/<project>/<path>` (workspace full path).
        let mut comps = root_path.trim_start_matches('/').splitn(2, '/');
        if let (Some(first), Some(rest)) = (comps.next(), comps.next()) {
            if let Some((_, r)) = workspace.iter().find(|(n, _)| n == first) {
                return r.join(rest);
            }
        }
        return p;
    }
    match project_root {
        Some(pr) => pr.join(p),
        None => p,
    }
}

/// m2e classpath attributes of a Maven dependency jar in the local repository.
pub fn maven_attributes(jar: &Path, is_test: bool, local_repo: &Path) -> Vec<(String, String)> {
    let Ok(rel) = jar.strip_prefix(local_repo) else { return Vec::new() };
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.len() < 4 {
        return Vec::new();
    }
    let n = parts.len();
    let version = parts[n - 2].clone();
    let artifact = parts[n - 3].clone();
    let group = parts[..n - 3].join(".");
    let mut attrs = vec![("maven.pomderived".to_owned(), "true".to_owned())];
    if is_test {
        attrs.push(("test".to_owned(), "true".to_owned()));
    }
    let javadoc = jar.with_file_name(format!("{artifact}-{version}-javadoc.jar"));
    if javadoc.is_file() {
        attrs.push(("javadoc_location".to_owned(), format!("jar:file:{}!/", javadoc.to_string_lossy())));
    }
    attrs.push(("maven.groupId".to_owned(), group));
    attrs.push(("maven.artifactId".to_owned(), artifact));
    attrs.push(("maven.version".to_owned(), version));
    attrs.push(("maven.scope".to_owned(), if is_test { "test" } else { "compile" }.to_owned()));
    attrs.push(("maven.pomderived".to_owned(), "true".to_owned()));
    attrs
}

/// The JDK's default Javadoc location (`JavaRuntime`'s
/// `JavadocLocations` for the execution environment), from the `release`
/// file of the JDK owning `jrt_fs` (`<java.home>/lib/jrt-fs.jar`).
pub fn jdk_javadoc_location(jrt_fs: &Path) -> Option<String> {
    let home = jrt_fs.parent()?.parent()?;
    let release = std::fs::read_to_string(home.join("release")).ok()?;
    let version = release
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))?
        .trim()
        .trim_matches('"')
        .to_owned();
    let mut parts = version.split(|c: char| c == '.' || c == '_' || c == '-');
    let first: u32 = parts.next()?.parse().ok()?;
    let major = if first == 1 { parts.next()?.parse().ok()? } else { first };
    Some(if major >= 11 {
        format!("https://docs.oracle.com/en/java/javase/{major}/docs/api/")
    } else {
        format!("https://docs.oracle.com/javase/{major}/docs/api/")
    })
}

fn classpath_entries(project_root: &Path) -> Vec<(String, String, Vec<(String, String)>)> {
    let Ok(text) = std::fs::read_to_string(project_root.join(".classpath")) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&text) else { return Vec::new() };
    doc.descendants()
        .filter(|n| n.has_tag_name("classpathentry"))
        .map(|n| {
            let attrs = n
                .descendants()
                .filter(|a| a.has_tag_name("attribute"))
                .filter_map(|a| Some((a.attribute("name")?.to_owned(), a.attribute("value")?.to_owned())))
                .collect();
            (n.attribute("kind").unwrap_or("").to_owned(), n.attribute("path").unwrap_or("").to_owned(), attrs)
        })
        .collect()
}

/// Extra attributes of an Eclipse project's JRE container entry (`.classpath`).
pub fn eclipse_container_attributes(project_root: &Path) -> Vec<(String, String)> {
    classpath_entries(project_root)
        .into_iter()
        .find(|(kind, path, _)| kind == "con" && path.starts_with("org.eclipse.jdt.launching.JRE_CONTAINER"))
        .map(|(_, _, a)| a)
        .unwrap_or_default()
}

/// Extra attributes of an Eclipse project's library entry for `jar`.
pub fn eclipse_library_attributes(project_root: &Path, jar: &Path) -> Vec<(String, String)> {
    classpath_entries(project_root)
        .into_iter()
        .find(|(kind, path, _)| kind == "lib" && (project_root.join(path) == jar || Path::new(path) == jar))
        .map(|(_, _, a)| a)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ClassFileRef {
        ClassFileRef {
            project: "javadoctest".into(),
            root_path: "/home/nkomonen/.m2/repository/io/projectreactor/reactor-core/3.2.10.RELEASE/reactor-core-3.2.10.RELEASE.jar".into(),
            module: None,
            attributes: vec![],
            package: "reactor.core.publisher".into(),
            class_file: "Mono.class".into(),
            source_file_name: None,
        }
    }

    #[test]
    fn builds_jdtls_uri() {
        // JavaDocImageExtractionTest (no SourceFile attribute name).
        assert_eq!(
            sample().to_uri(),
            "jdt://contents/reactor-core-3.2.10.RELEASE.jar/reactor.core.publisher/Mono.class?=javadoctest/%5C/home%5C/nkomonen%5C/.m2%5C/repository%5C/io%5C/projectreactor%5C/reactor-core%5C/3.2.10.RELEASE%5C/reactor-core-3.2.10.RELEASE.jar%3Creactor.core.publisher%28Mono.class"
        );
    }

    #[test]
    fn builds_maven_attributes() {
        let r = ClassFileRef {
            project: "quickstart2".into(),
            root_path: "/r/junit/junit/4.13/junit-4.13.jar".into(),
            attributes: maven_attributes(Path::new("/r/junit/junit/4.13/junit-4.13.jar"), true, Path::new("/r")),
            package: "org.junit".into(),
            class_file: "Assert.class".into(),
            source_file_name: Some("Assert.java".into()),
            ..Default::default()
        };
        let uri = r.to_uri();
        assert!(uri.starts_with("jdt://contents/junit-4.13.jar/org.junit/Assert.java"), "{uri}");
        assert!(uri.contains("junit%5C/junit%5C/4.13%5C/junit-4.13.jar=/maven.pomderived=/true=/=/test=/true=/=/maven.groupId=/junit=/=/maven.artifactId=/junit=/=/maven.version=/4.13=/=/maven.scope=/test=/=/maven.pomderived=/true=/%3Corg.junit%28Assert.class"), "{uri}");
        assert_eq!(ClassFileRef::parse(&uri).unwrap(), r);
    }

    #[test]
    fn round_trips_jrt() {
        let r = ClassFileRef {
            project: "salut".into(),
            root_path: "/opt/jdk/lib/jrt-fs.jar".into(),
            module: Some("java.base".into()),
            package: "java.util".into(),
            class_file: "Map$Entry.class".into(),
            source_file_name: Some("Map.java".into()),
            ..Default::default()
        };
        let uri = r.to_uri();
        assert!(uri.starts_with("jdt://contents/java.base/java.util/Map.java?=salut/%5C/opt%5C/jdk%5C/lib%5C/jrt-fs.jar%60java.base%3Cjava.util%28Map$Entry.class"), "{uri}");
        assert_eq!(ClassFileRef::parse(&uri).unwrap(), r);
        assert!(Url::parse(&uri).is_ok());
    }
}
