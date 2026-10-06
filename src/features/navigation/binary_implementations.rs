//! Binary type implementations: Rust owns the transitive subtype search over
//! raw class-file headers, matching ImplementationCollector's type hierarchy.

use super::{class_file_uri, workspace};
use crate::analysis::dispatcher::Dispatcher;
use crate::classfile::ClassFileDesc;
use crate::features::semantic::Semantic;
use crate::index::type_index::{self, TypeOrigin};
use crate::project::{DEFAULT_PROJECT_NAME, INCLUDE_RUNNING_VM};
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

pub(super) async fn append(
    d: &Dispatcher,
    uri: &Url,
    pos: Position,
    locations: &mut Vec<Location>,
) {
    let semantic = Semantic::new(d).await;
    let Some((selected, project)) = semantic.select(uri, pos).await else {
        return;
    };
    let Some(target) = selected.select.first().filter(|e| e.is_type()) else {
        return;
    };
    let Some(fqn) = target.fqn.as_deref() else {
        return;
    };
    let mut archives = Vec::new();
    let mut include_jdk = false;
    for context in semantic
        .contexts()
        .iter()
        .filter(|c| c.project.is_some() || project.is_none())
    {
        for archive in &context.ctx.classpath {
            if !archives.contains(archive) {
                archives.push(archive.clone());
            }
        }
        include_jdk |= context
            .ctx
            .options
            .get(INCLUDE_RUNNING_VM)
            .is_none_or(|v| v != "false");
    }
    let missing: Vec<String> = archives
        .iter()
        .filter(|a| type_index::cached_archive(a).is_none())
        .cloned()
        .collect();
    let need_jdk = include_jdk && type_index::cached_jdk().is_none();
    if !missing.is_empty() || need_jdk {
        let Some(listing) = semantic.list_types(&missing, need_jdk).await else {
            return;
        };
        type_index::store_listing(&listing);
    }
    let mut types = Vec::new();
    for archive in archives {
        types.extend(type_index::cached_archive(&archive).unwrap_or_default());
    }
    if include_jdk {
        types.extend(type_index::cached_jdk().unwrap_or_default());
    }
    let name = |t: &type_index::TypeEntry| {
        let mut chain = t.enclosing.clone();
        chain.push(t.name.clone());
        if t.package.is_empty() {
            chain.join("$")
        } else {
            format!("{}.{}", t.package, chain.join("$"))
        }
    };
    // Keep the first root's definition when the build path shadows a type.
    let mut graph = HashMap::new();
    for t in &types {
        graph.entry(name(t)).or_insert(&t.super_types);
    }
    let mut subtypes = HashSet::new();
    let mut frontier = vec![fqn.to_owned()];
    while let Some(parent) = frontier.pop() {
        for (child, parents) in &graph {
            if child != fqn && parents.contains(&parent) && subtypes.insert(child.clone()) {
                frontier.push(child.clone());
            }
        }
    }
    let ws = workspace(d);
    let project = project.as_deref().unwrap_or(DEFAULT_PROJECT_NAME);
    let mut emitted = HashSet::new();
    for t in types {
        let fqn = name(&t);
        if !subtypes.contains(&fqn) || !emitted.insert(fqn) {
            continue;
        }
        let TypeOrigin::Binary {
            archive,
            module,
            class_file,
            source_file_name,
        } = t.origin
        else {
            continue;
        };
        let desc = ClassFileDesc {
            root: archive,
            module,
            package_name: t.package,
            class_file_name: class_file,
            source_file_name,
        };
        let Ok(uri) = Url::parse(&class_file_uri(&ws, project, &desc)) else {
            continue;
        };
        let location = Location {
            uri,
            range: Range::default(),
        };
        if !locations.contains(&location) {
            locations.push(location);
        }
    }
}
