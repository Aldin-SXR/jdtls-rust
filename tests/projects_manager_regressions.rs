//! Additional resource-filter coverage, checked against Eclipse's real resource
//! model. These are regressions, not upstream ports.
mod common;
#[path = "common/projects_manager_fixture.rs"]
mod manager_fixture;
#[path = "../src/project/mod.rs"]
#[allow(dead_code, unused_imports)]
mod project;

use serde_json::{json, Value};

fn matches(patterns: &[&str], paths: &[&str]) -> Value {
    let mut ws = manager_fixture::workspace();
    ws.import_projects(&["maven/salut"]);
    let results =
        manager_fixture::filters(&mut ws, "salut", paths, vec![json!({"patterns":patterns})]);
    assert_eq!(
        json!(patterns),
        results[0]["patterns"],
        "valid expressions must not be dropped"
    );
    results[0]["filtered"].clone()
}

#[test]
fn resource_names_match_completely_and_filters_inherit() {
    assert_eq!(
        json!([true, true, false, false, false, false, true]),
        matches(
            &["node_modules"],
            &[
                "node_modules",
                "src/node_modules/vendor",
                "node_modules_backup",
                "my_node_modules",
                "src",
                "src/node_modulesX",
                "__CREATED_BY_JAVA_LANGUAGE_SERVER__"
            ]
        )
    );
}
#[test]
fn quoted_literals_are_java_regexes() {
    assert_eq!(
        json!([true, false, false]),
        matches(&[r"\Qcache[1]\E"], &["cache[1]", "cache1", "xcache[1]"])
    );
}
#[test]
fn java_character_class_intersections() {
    assert_eq!(
        json!([true, false, true, false]),
        matches(&[r"[a-z&&[^aeiou]]+"], &["bcd", "abc", "xyz", "xyz123"])
    );
}
#[test]
fn negative_lookahead_preserves_exceptions() {
    assert_eq!(
        json!([true, false, true]),
        matches(
            &[r"cache(?!_keep).*"],
            &["cache", "cache_keep", "cache_tmp"]
        )
    );
}
#[test]
fn positive_lookbehind_is_supported() {
    assert_eq!(
        json!([true, true, false]),
        matches(
            &[r".*(?<=cache)_tmp"],
            &["cache_tmp", "xcache_tmp", "cache_keep"]
        )
    );
}
#[test]
fn numeric_backreferences_are_supported() {
    assert_eq!(
        json!([true, false, false]),
        matches(
            &[r"(cache)_\1"],
            &["cache_cache", "cache_tmp", "cache_cachex"]
        )
    );
}
#[test]
fn named_backreferences_are_supported() {
    assert_eq!(
        json!([true, false]),
        matches(&[r"(?<dir>cache)_\k<dir>"], &["cache_cache", "cache_tmp"])
    );
}
#[test]
fn possessive_quantifiers_do_not_backtrack() {
    assert_eq!(
        json!([false]),
        matches(&[r"cache_.*+_tmp"], &["cache_a_tmp"])
    );
    assert_eq!(json!([true]), matches(&[r"cache_.*_tmp"], &["cache_a_tmp"]));
}
#[test]
fn atomic_groups_preserve_alternative_order() {
    assert_eq!(
        json!([true, false]),
        matches(&[r"(?>cache|cache_tmp)"], &["cache", "cache_tmp"])
    );
    assert_eq!(
        json!([true, true]),
        matches(&[r"cache|cache_tmp"], &["cache", "cache_tmp"])
    );
}
#[test]
fn predefined_classes_use_java_ascii_defaults() {
    assert_eq!(
        json!([true, false]),
        matches(&[r"cache_\d+"], &["cache_123", "cache_١٢٣"])
    );
    assert_eq!(
        json!([true, false]),
        matches(&[r"cache_\w+"], &["cache_1", "cache_café"])
    );
}
#[test]
fn project_name_filters_inherit_to_resources() {
    assert_eq!(
        json!([true, true]),
        matches(&["salut"], &["cache", "src/cache"])
    );
}
#[test]
fn java_unicode_escapes_match_utf8_names() {
    assert_eq!(
        json!([true, false]),
        matches(&[r"caf\u00E9"], &["café", "cafe"])
    );
}
#[test]
fn invalid_patterns_are_removed_without_losing_valid_filters() {
    let mut ws = manager_fixture::workspace();
    ws.import_projects(&["maven/salut"]);
    let results = manager_fixture::filters(
        &mut ws,
        "salut",
        &["node_modules", ".git", "src"],
        vec![
            json!({"patterns":["**/node_modules/**", "node_modules", "[", "\\p{notreal}", "\\.git"]}),
            json!({"patterns":null}),
        ],
    );
    assert_eq!(json!(["node_modules", "\\.git"]), results[0]["patterns"]);
    assert_eq!(json!([true, true, false]), results[0]["filtered"]);
    assert_eq!(json!([]), results[1]["patterns"]);
    assert_eq!(json!([false, false, false]), results[1]["filtered"]);
}

#[test]
fn configuration_filters_saved_sources_and_clears_their_diagnostics() {
    let mut ws = common::jdtls::Workspace::new();
    ws.import_projects(&["maven/salut"]);
    let root = ws.project_root("salut");
    let hidden = ws.create_cu(
        &root,
        "src/main/java",
        "cache",
        "Hidden.java",
        "package cache; public class Hidden { int x = \"bad\"; }\n",
    );
    let visible = ws.create_cu(
        &root,
        "src/main/java",
        "sample",
        "Visible.java",
        "package sample; public class Visible { int x = \"bad\"; }\n",
    );
    let before = ws.project_published_diagnostics();
    for uri in [&hidden, &visible] {
        assert!(
            before[uri].iter().any(|d| d["severity"] == 1
                && d["message"] == "Type mismatch: cannot convert from String to int"),
            "{before:#?}"
        );
    }
    ws.settings = json!({"java":{"project":{"resourceFilters":["cache"]}}});
    let settings = ws.settings.clone();
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({"settings":settings}),
    );
    ws.wait_idle();
    let filtered = ws.project_published_diagnostics();
    assert!(
        filtered[&hidden].is_empty(),
        "filtered files must clear their old diagnostics: {filtered:#?}"
    );
    assert!(
        filtered[&visible].iter().any(|d| d["severity"] == 1),
        "{filtered:#?}"
    );

    // Preferences.updateFrom keeps the old list for missing/null values. The
    // direct manager setter's null-clears behavior is tested separately.
    for settings in [
        json!({"java":{"project":{"resourceFilters":null}}}),
        json!({"java":{"format":{"enabled":true}}}),
    ] {
        ws.settings = settings.clone();
        ws.client().notify(
            "workspace/didChangeConfiguration",
            json!({"settings":settings}),
        );
        ws.wait_idle();
        let retained = ws.project_published_diagnostics();
        assert!(retained[&hidden].is_empty(), "{retained:#?}");
        assert!(
            retained[&visible].iter().any(|d| d["severity"] == 1),
            "{retained:#?}"
        );
    }
    ws.settings = json!({"java.project.resourceFilters":[]});
    let settings = ws.settings.clone();
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({"settings":settings}),
    );
    ws.wait_idle();
    ws.build_workspace(true);
    let restored = ws.project_published_diagnostics();
    assert!(
        restored[&hidden].iter().any(|d| d["severity"] == 1),
        "{restored:#?}"
    );
    assert!(
        restored[&visible].iter().any(|d| d["severity"] == 1),
        "{restored:#?}"
    );
}

#[test]
fn cold_maven_resolution_downloads_dependency_parent_and_bom_poms() {
    use std::io::{Read, Write};
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path().join("root.pom");
    std::fs::write(&root,"<project><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version><dependencies><dependency><groupId>org.example</groupId><artifactId>direct</artifactId><version>1</version></dependency></dependencies></project>").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let responses = [
            ("direct/1/direct-1.pom","<project><parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version><relativePath/></parent><artifactId>direct</artifactId><dependencies><dependency><groupId>org.example</groupId><artifactId>leaf</artifactId></dependency></dependencies></project>"),
            ("parent/1/parent-1.pom","<project><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version><packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>bom</artifactId><version>1</version><scope>import</scope><type>pom</type></dependency></dependencies></dependencyManagement></project>"),
            ("bom/1/bom-1.pom","<project><groupId>org.example</groupId><artifactId>bom</artifactId><version>1</version><packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>leaf</artifactId><version>2</version></dependency></dependencies></dependencyManagement></project>"),
            ("leaf/2/leaf-2.pom","<project><groupId>org.example</groupId><artifactId>leaf</artifactId><version>2</version></project>")];
        for (path, body) in responses {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(5))
                    }
                    Err(e) => panic!("missing POM request: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            assert_eq!(
                format!("GET /org/example/{path} HTTP/1.1"),
                String::from_utf8(request).unwrap().lines().next().unwrap()
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let mut resolver = project::maven::Resolver::new();
    resolver.local_repo = repository.path().join("cache");
    resolver.repositories = vec![project::download::Repository {
        id: "cold-test".into(),
        url,
    }];
    let model = resolver.model(&root).unwrap();
    let dependencies = resolver.resolve(&model);
    assert_eq!(
        vec![("direct", Some("1")), ("leaf", Some("2"))],
        dependencies
            .iter()
            .map(|(d, _)| (d.artifact.as_str(), d.version.as_deref()))
            .collect::<Vec<_>>()
    );
    server.join().unwrap();
    // The HTTP server has stopped. Fresh model loading still uses cached POMs.
    let mut resolver = project::maven::Resolver::with_settings(&project::maven::MavenSettings {
        offline: true,
        ..Default::default()
    });
    resolver.local_repo = repository.path().join("cache");
    let model = resolver.model(&root).unwrap();
    assert_eq!(2, resolver.resolve(&model).len());
}

#[test]
fn offline_maven_resolution_keeps_direct_dependencies_without_fetching_missing_poms() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path().join("root.pom");
    std::fs::write(&root,"<project><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version><dependencies><dependency><groupId>org.example</groupId><artifactId>direct</artifactId><version>1</version></dependency></dependencies></project>").unwrap();
    let mut resolver = project::maven::Resolver::with_settings(&project::maven::MavenSettings {
        offline: true,
        ..Default::default()
    });
    resolver.local_repo = repository.path().join("cache");
    resolver.repositories = vec![project::download::Repository {
        id: "unreachable".into(),
        url: "http://127.0.0.1:1".into(),
    }];
    let model = resolver.model(&root).unwrap();
    let dependencies = resolver.resolve(&model);
    assert_eq!(1, dependencies.len());
    assert_eq!("direct", dependencies[0].0.artifact);
    assert!(!resolver.local_repo.exists());
}
