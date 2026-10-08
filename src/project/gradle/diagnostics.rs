//! Diagnostics of the Kotlin, Groovy, AspectJ and Scala compilers, read from
//! the standard error of the Gradle compile tasks
//! (`GradleBuildSupport.publishDiagnostics`).

use crate::project::Marker;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    pub kotlin: bool,
    pub groovy: bool,
    pub aspectj: bool,
    pub scala: bool,
}

impl ParseOptions {
    pub fn for_tasks(tasks: &[String]) -> Self {
        let has = |a: &str, b: &str| tasks.iter().any(|t| t == a || t == b);
        Self {
            kotlin: has("compileKotlin", "compileTestKotlin"),
            groovy: has("compileGroovy", "compileTestGroovy"),
            aspectj: has("compileAspectj", "compileTestAspectj"),
            scala: has("compileScala", "compileTestScala"),
        }
    }
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

fn kotlin() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    regex(&R, r"(?m)^(?:(?P<type>[ew]): )?(?:file:///)?(?P<file>.*\.(?:kt|kts|aj)):(?P<line>\d+):(?P<col>\d+)?\s*(?P<message>.*)$")
}

fn groovy() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    regex(&R, r"(?m)^(?P<file>.*\.groovy):\s*(?P<line>\d+):\s*(?P<message>.*)\s+@\s+line\s+\d+,\s+column\s+(?P<col>\d+)\.")
}

fn aspectj() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    regex(&R, r"(?m)^(?P<file>.*\.aj):(?P<line>\d+)\s+\[(?P<type>error|warning)\]\s+(?P<message>.*)(?:\r\n|[\n\r])(?P<source>.*)(?:\r\n|[\n\r])(?P<indent>\s*)\^")
}

fn scala() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    regex(&R, r"(?m)^(?:(?:\[(?P<type>\w+)\]\s+)?(?P<file>/?[^:]+(?:\.scala|\.java)):(?P<line>\d+):(?:(?P<col>\d+):)?\s*(?P<message>.*))$")
}

/// A file diagnostic as a marker on the file.
pub fn parse(stderr: &str, options: ParseOptions, project_dir: &Path) -> Vec<Marker> {
    let mut out = Vec::new();
    let mut collect = |pattern: &Regex| {
        for c in pattern.captures_iter(stderr) {
            let group = |name: &str| c.name(name).map(|m| m.as_str().trim().to_owned());
            let Some(file) = group("file") else { continue };
            let message = group("message").unwrap_or_else(|| "Unknown".to_owned());
            let ty = group("type").unwrap_or_else(|| "e".to_owned());
            let severity = if ["w", "warning", "warn"].contains(&ty.to_lowercase().as_str()) {
                2
            } else {
                1
            };
            let line = group("line")
                .and_then(|l| l.parse::<i64>().ok())
                .map_or(0, |l| (l - 1).max(0)) as u32;
            let (start, end) = if file.ends_with(".aj") {
                (0, 500)
            } else {
                let mut start = group("col").and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
                if file.ends_with(".kt") || file.ends_with(".scala") {
                    start -= 1;
                }
                let start = start.max(0) as u32;
                (start, start)
            };
            let path = PathBuf::from(&file);
            let path = if path.is_absolute() { path } else { project_dir.join(path) };
            let mut marker = Marker::project(message, severity, "-1");
            marker.resource = Some(path);
            marker.range = Some((line, start, end));
            out.push(marker);
        }
    };
    if options.kotlin {
        collect(kotlin());
    }
    if options.groovy {
        collect(groovy());
    }
    if options.aspectj {
        collect(aspectj());
    }
    if options.scala {
        collect(scala());
    }
    out
}
