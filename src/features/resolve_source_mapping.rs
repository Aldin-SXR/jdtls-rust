//! Port of jdt.ls `ResolveSourceMappingHandler` (`java.project.resolveStackTraceLocation`).

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analysis::dispatcher::Dispatcher;
use crate::features::navigation;
use crate::project::Workspace;

static SOURCE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"([0-9A-Za-z_$.]+/)?(([0-9A-Za-z_$]+\.)+[<0-9A-Za-z_$>]+)\(([0-9A-Za-z_$\-]+\.(?:java|kt|groovy|clj|scala)(?::\d+)?)+\)")
        .expect("stack trace pattern")
});

/// The type and source path a stack trace line points to.
fn parse_line(line: &str) -> Option<(String, String)> {
    let captures = SOURCE_PATTERN.captures(line)?;
    let method_field = captures.get(2)?.as_str();
    let location_field = captures.get(captures.len() - 1)?.as_str();
    let fully_qualified_name = &method_field[..method_field.rfind('.')?];
    let package_name = fully_qualified_name.rfind('.').map_or("", |i| &fully_qualified_name[..i]);
    let source_name = location_field.split(':').next().unwrap_or_default();
    let source_path = if package_name.trim().is_empty() {
        source_name.to_owned()
    } else {
        format!("{}/{}", package_name.replace('.', "/"), source_name)
    };
    Some((fully_qualified_name.to_owned(), source_path))
}

/// `ResolveSourceMappingHandler.resolveStackTraceLocation`: the URI of the
/// source file or class file a stack trace line refers to.
pub async fn resolve_stack_trace_location(
    d: &Dispatcher,
    ws: &Workspace,
    line: Option<&str>,
    project_names: Option<&[String]>,
) -> Option<String> {
    let (fully_qualified_name, source_path) = parse_line(line?)?;
    let known = ws.all_projects();
    let projects: Vec<_> = match project_names {
        Some(names) if !names.is_empty() => names.iter().filter_map(|name| known.iter().find(|p| &p.name == name)).collect(),
        _ => known.iter().collect(),
    };
    for project in &projects {
        for folder in &project.source_folders {
            let file = folder.path.join(&source_path);
            if file.is_file() {
                return tower_lsp::lsp_types::Url::from_file_path(&file).ok().map(|u| u.to_string());
            }
        }
    }
    let (package, file) = source_path.rsplit_once('/').unwrap_or(("", source_path.as_str()));
    let base = file.rsplit_once('.').map_or(file, |(base, _)| base);
    let sibling = if package.is_empty() { base.to_owned() } else { format!("{}.{base}", package.replace('/', ".")) };
    for project in &projects {
        for candidate in [&sibling, &fully_qualified_name] {
            if let Some(uri) = navigation::type_uri(d, &project.name, candidate).await {
                return Some(uri);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stack_trace_lines() {
        assert_eq!(
            Some(("quickstart.AppTest".to_owned(), "quickstart/AppTest.java".to_owned())),
            parse_line("at quickstart.AppTest.shouldAnswerWithTrue(AppTest.java:10)")
        );
        assert_eq!(
            Some(("okhttp3.OkHttpClient".to_owned(), "okhttp3/OkHttpClient.kt".to_owned())),
            parse_line("at okhttp3.OkHttpClient.<init>(OkHttpClient.kt)")
        );
        assert_eq!(
            Some(("akka.actor.Actor".to_owned(), "akka/actor/Actor.scala".to_owned())),
            parse_line("at akka.actor.Actor.$init$(Actor.scala:492)")
        );
        assert_eq!(None, parse_line("no frame here"));
    }
}
