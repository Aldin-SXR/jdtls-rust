//! Persist raw source-path changes without discarding XML owned by other tools.

use super::{ClasspathEntry, EntryKind, Project, ProjectKind, WORKSPACE_LINK};
use std::io::{self, Write};
use std::ops::Range;
use std::path::Path;

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn relative(project: &Project, full: &str) -> String {
    let base = format!("/{}", project.name);
    if full == base {
        return String::new();
    }
    full.strip_prefix(&format!("{base}/"))
        .unwrap_or(full)
        .to_owned()
}

fn location(project: &Project, path: &Path) -> String {
    if project.kind == ProjectKind::Invisible {
        if let Ok(relative) = path.strip_prefix(&project.root) {
            return Path::new(WORKSPACE_LINK)
                .join(relative)
                .to_string_lossy()
                .replace('\\', "/");
        }
    }
    path.strip_prefix(&project.location)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn entry_xml(project: &Project, entry: &ClasspathEntry) -> String {
    let kind = match entry.kind {
        EntryKind::Source | EntryKind::Project => "src",
        EntryKind::Library => "lib",
        EntryKind::Variable => "var",
        EntryKind::Container => "con",
    };
    let path = match entry.kind {
        EntryKind::Source | EntryKind::Library => relative(project, &entry.path),
        _ => entry.path.clone(),
    };
    let mut text = format!("<classpathentry kind=\"{kind}\" path=\"{}\"", xml(&path));
    for (name, patterns) in [
        ("including", &entry.inclusions),
        ("excluding", &entry.exclusions),
    ] {
        if !patterns.is_empty() {
            text.push_str(&format!(" {name}=\"{}\"", xml(&patterns.join("|"))));
        }
    }
    if let Some(output) = &entry.output {
        text.push_str(&format!(" output=\"{}\"", xml(&location(project, output))));
    }
    if let Some(source) = &entry.source_attachment {
        text.push_str(&format!(
            " sourcepath=\"{}\"",
            xml(&location(project, source))
        ));
    }
    if entry.exported {
        text.push_str(" exported=\"true\"");
    }
    if entry.attributes.is_empty() {
        text.push_str("/>");
    } else {
        text.push_str("><attributes>");
        for (name, value) in &entry.attributes {
            text.push_str(&format!(
                "<attribute name=\"{}\" value=\"{}\"/>",
                xml(name),
                xml(value)
            ));
        }
        text.push_str("</attributes></classpathentry>");
    }
    text
}

fn initial_classpath(project: &Project) -> String {
    let mut text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<classpath>\n".to_owned();
    for entry in &project.classpath {
        text.push_str(&format!("\t{}\n", entry_xml(project, entry)));
    }
    if let Some(output) = &project.output {
        text.push_str(&format!(
            "\t<classpathentry kind=\"output\" path=\"{}\"/>\n",
            xml(&location(project, output))
        ));
    }
    text.push_str("</classpath>\n");
    text
}

/// Only source entries and their inclusion/exclusion patterns are changed.
/// Libraries, access rules, custom attributes and comments retain their bytes.
fn updated_classpath(text: &str, project: &Project) -> io::Result<String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let root = doc.root_element();
    if !root.has_tag_name("classpath") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected a classpath element",
        ));
    }
    if text[root.range()].ends_with("/>") {
        // A self-closing classpath is a valid empty raw classpath in JDT.
        let mut body = String::new();
        for entry in &project.classpath {
            body.push_str(&format!("\n\t{}", entry_xml(project, entry)));
        }
        if let Some(output) = &project.output {
            body.push_str(&format!(
                "\n\t<classpathentry kind=\"output\" path=\"{}\"/>",
                xml(&location(project, output))
            ));
        }
        let mut result = text.to_owned();
        result.replace_range(
            root.range().end - 2..root.range().end,
            &format!(">{body}\n</classpath>"),
        );
        return Ok(result);
    }
    let entries: Vec<_> = project
        .classpath
        .iter()
        .filter(|e| e.kind == EntryKind::Source)
        .collect();
    let mut seen = Vec::new();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut insertion = text[..root.range().end]
        .rfind("</classpath>")
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Missing classpath closing tag")
        })?;
    for node in root.children().filter(|n| n.has_tag_name("classpathentry")) {
        if node.attribute("kind") == Some("output") {
            insertion = node.range().start;
        }
        let path = node.attribute("path").unwrap_or("");
        if node.attribute("kind") != Some("src") || path.starts_with('/') {
            continue;
        }
        let full = if path.is_empty() || path == "." {
            format!("/{}", project.name)
        } else {
            format!("/{}/{}", project.name, path.trim_end_matches('/'))
        };
        let Some(entry) = entries.iter().find(|e| e.path == full) else {
            edits.push((node.range(), String::new()));
            continue;
        };
        seen.push(full);
        for (name, patterns) in [
            ("including", &entry.inclusions),
            ("excluding", &entry.exclusions),
        ] {
            let value = patterns.join("|");
            if node.attribute(name).unwrap_or("") == value {
                continue;
            }
            if let Some(attr) = node.attributes().find(|a| a.name() == name) {
                if patterns.is_empty() {
                    edits.push((attr.range(), String::new()));
                } else {
                    edits.push((attr.range_value(), xml(&value)));
                }
            } else if !patterns.is_empty() {
                let start = node.range().start;
                let close = text[start..node.range().end].find('>').unwrap() + start;
                let point = if text.as_bytes()[close - 1] == b'/' {
                    close - 1
                } else {
                    close
                };
                edits.push((point..point, format!(" {name}=\"{}\"", xml(&value))));
            }
        }
    }
    let appended: String = entries
        .iter()
        .filter(|e| !seen.contains(&e.path))
        .map(|e| format!("{}\n\t", entry_xml(project, e)))
        .collect();
    if !appended.is_empty() {
        edits.push((insertion..insertion, appended));
    }
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut result = text.to_owned();
    for (range, value) in edits {
        result.replace_range(range, &value);
    }
    Ok(result)
}

fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    if let Ok(metadata) = path.metadata() {
        file.as_file().set_permissions(metadata.permissions())?;
    }
    file.write_all(content.as_bytes())?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// `IJavaProject.setRawClasspath`: JDT rewrites the whole `.classpath`.
pub fn persist_raw_classpath(project: &Project) -> io::Result<()> {
    if project.kind == ProjectKind::Default {
        return Ok(());
    }
    std::fs::create_dir_all(&project.location)?;
    write_atomic(&project.location.join(".classpath"), &initial_classpath(project))
}

/// Write the project metadata JDT would create, then commit its raw classpath.
/// No source file or source folder is created as part of this operation.
pub fn persist_sources(project: &Project) -> io::Result<()> {
    if project.kind == ProjectKind::Invisible {
        std::fs::create_dir_all(&project.location)?;
        std::fs::create_dir_all(project.location.join("bin"))?;
        let path = project.location.join(".project");
        if !path.exists() {
            let uri =
                tower_lsp::lsp_types::Url::from_directory_path(&project.root).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Invalid workspace folder")
                })?;
            let natures: String = project
                .natures
                .iter()
                .map(|n| format!("<nature>{}</nature>", xml(n)))
                .collect();
            let description = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription><name>{}</name><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures>{natures}</natures><linkedResources><link><name>{WORKSPACE_LINK}</name><type>2</type><locationURI>{}</locationURI></link></linkedResources></projectDescription>\n", xml(&project.name), xml(uri.as_str()));
            write_atomic(&path, &description)?;
        }
    }
    let path = project.location.join(".classpath");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => updated_classpath(&text, project)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => initial_classpath(project),
        Err(e) => return Err(e),
    };
    write_atomic(&path, &text)
}
