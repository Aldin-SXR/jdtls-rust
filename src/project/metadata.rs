//! Project metadata files (`.project`, `.classpath`, `.settings/*.prefs`) as
//! jdt.ls' file system writes them: next to the project, or in the
//! workspace's metadata area (`java.import.generatesMetadataFilesAtProjectRoot`).

use super::{ClasspathEntry, EntryKind, Project};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataSettings {
    /// `JLSFsUtils.generatesMetadataFilesAtProjectRoot()`.
    pub at_project_root: bool,
    /// `<workspace>/.metadata/.plugins/org.eclipse.core.resources/.projects`.
    pub area: Option<PathBuf>,
}

impl Default for MetadataSettings {
    fn default() -> Self {
        Self {
            at_project_root: true,
            area: None,
        }
    }
}

pub const PROJECT_FILE: &str = ".project";
pub const CLASSPATH_FILE: &str = ".classpath";
pub const SETTINGS_DIR: &str = ".settings";
pub const JDT_CORE_PREFS: &str = "org.eclipse.jdt.core.prefs";

/// The metadata area of a jdt.ls workspace (`-data`) directory.
pub fn metadata_area(data_dir: &Path) -> PathBuf {
    data_dir.join(".metadata/.plugins/org.eclipse.core.resources/.projects")
}

impl MetadataSettings {
    /// Where the metadata file `relative` (`.project`, `.settings/x.prefs`) of
    /// `project` lives (`JLSFsUtils.shouldStoreInMetadataArea`).
    pub fn location(&self, project: &Project, relative: &str) -> PathBuf {
        let at_root = project.location.join(relative);
        let Some(area) = self.area.as_ref().filter(|_| !self.at_project_root) else {
            return at_root;
        };
        if at_root.exists() {
            return at_root;
        }
        if relative.ends_with(".prefs") && project.location.join(SETTINGS_DIR).exists() {
            return at_root;
        }
        area.join(&project.name).join(relative)
    }
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `.project` of a Buildship-managed project.
pub fn project_description(project: &Project, comment: &str, builders: &[&str], filter: &str) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription>\n");
    s.push_str(&format!("\t<name>{}</name>\n", escape_xml(&project.name)));
    s.push_str(&format!("\t<comment>{}</comment>\n", escape_xml(comment)));
    s.push_str("\t<projects>\n\t</projects>\n\t<buildSpec>\n");
    for b in builders {
        s.push_str(&format!(
            "\t\t<buildCommand>\n\t\t\t<name>{b}</name>\n\t\t\t<arguments>\n\t\t\t</arguments>\n\t\t</buildCommand>\n"
        ));
    }
    s.push_str("\t</buildSpec>\n\t<natures>\n");
    for n in &project.natures {
        s.push_str(&format!("\t\t<nature>{n}</nature>\n"));
    }
    s.push_str("\t</natures>\n");
    if !filter.is_empty() {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        s.push_str(&format!(
            "\t<filteredResources>\n\t\t<filter>\n\t\t\t<id>{id}</id>\n\t\t\t<name></name>\n\t\t\t<type>30</type>\n\t\t\t<matcher>\n\t\t\t\t<id>org.eclipse.core.resources.regexFilterMatcher</id>\n\t\t\t\t<arguments>{}</arguments>\n\t\t\t</matcher>\n\t\t</filter>\n\t</filteredResources>\n",
            escape_xml(filter)
        ));
    }
    s.push_str("</projectDescription>\n");
    s
}

fn relative(project: &Project, location: &Path) -> String {
    location
        .strip_prefix(&project.location)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| location.to_string_lossy().into_owned())
}

fn classpath_entry(project: &Project, e: &ClasspathEntry, out: &mut String) {
    let mut attrs: Vec<(&str, String)> = Vec::new();
    let kind = match e.kind {
        EntryKind::Source => "src",
        EntryKind::Library => "lib",
        EntryKind::Project => "src",
        EntryKind::Variable => "var",
        EntryKind::Container => "con",
    };
    attrs.push(("kind", kind.to_owned()));
    if !e.exclusions.is_empty() {
        attrs.push(("excluding", e.exclusions.join("|")));
    }
    if e.exported {
        attrs.push(("exported", "true".to_owned()));
    }
    if !e.inclusions.is_empty() {
        attrs.push(("including", e.inclusions.join("|")));
    }
    if let Some(o) = &e.output {
        attrs.push(("output", relative(project, o)));
    }
    let path = match (e.kind, &e.location) {
        (EntryKind::Source, Some(l)) => relative(project, l),
        _ => e.path.clone(),
    };
    attrs.push(("path", path));
    attrs.sort_by(|a, b| a.0.cmp(b.0));
    out.push_str("\t<classpathentry");
    for (k, v) in &attrs {
        out.push_str(&format!(" {k}=\"{}\"", escape_xml(v)));
    }
    if e.attributes.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n\t\t<attributes>\n");
    for (k, v) in &e.attributes {
        out.push_str(&format!(
            "\t\t\t<attribute name=\"{}\" value=\"{}\"/>\n",
            escape_xml(k),
            escape_xml(v)
        ));
    }
    out.push_str("\t\t</attributes>\n\t</classpathentry>\n");
}

/// `.classpath` of a Java project: the raw classpath and the default output folder.
pub fn classpath_file(project: &Project) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<classpath>\n");
    for e in &project.classpath {
        let mut e = e.clone();
        e.children.clear();
        classpath_entry(project, &e, &mut s);
    }
    if let Some(output) = &project.output {
        s.push_str(&format!(
            "\t<classpathentry kind=\"output\" path=\"{}\"/>\n",
            escape_xml(&relative(project, output))
        ));
    }
    s.push_str("</classpath>\n");
    s
}

fn escape_property(s: &str, key: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' if key || i == 0 => out.push_str("\\ "),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// An Eclipse preferences file (`EclipsePreferences.save`): sorted keys and `eclipse.preferences.version`.
pub fn preferences_file(values: &BTreeMap<String, String>) -> String {
    let mut all = values.clone();
    all.insert("eclipse.preferences.version".to_owned(), "1".to_owned());
    let mut s = String::new();
    for (k, v) in &all {
        s.push_str(&format!("{}={}\n", escape_property(k, true), escape_property(v, false)));
    }
    s
}

pub fn write_project_file(settings: &MetadataSettings, project: &Project, content: &str) -> std::io::Result<()> {
    write_if_changed(&settings.location(project, PROJECT_FILE), content)
}

pub fn write_classpath_file(settings: &MetadataSettings, project: &Project) -> std::io::Result<()> {
    write_if_changed(&settings.location(project, CLASSPATH_FILE), &classpath_file(project))
}

pub fn write_preferences(
    settings: &MetadataSettings,
    project: &Project,
    file_name: &str,
    values: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let relative = format!("{SETTINGS_DIR}/{file_name}");
    write_if_changed(&settings.location(project, &relative), &preferences_file(values))
}

/// The preferences stored in the project's `.settings/<file_name>`.
pub fn read_preferences(
    settings: &MetadataSettings,
    project: &Project,
    file_name: &str,
) -> BTreeMap<String, String> {
    let relative = format!("{SETTINGS_DIR}/{file_name}");
    let mut values = super::prefs::read_properties(&settings.location(project, &relative))
        .unwrap_or_default();
    values.remove("eclipse.preferences.version");
    values
}
