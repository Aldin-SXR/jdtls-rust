//! Protocol and editor-buffer regressions for the shared import organizer.
mod common;
#[path = "../src/features/organize_imports/scope.rs"]
mod scope;
use common::jdtls::{apply_edits, range, test_default_options, Workspace};
use common::projects::{dir_uri, file_uri};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower_lsp::lsp_types::Url;

fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    ws.capabilities["workspace"]["applyEdit"] = json!(false);
    let root = ws.new_empty_project(&test_default_options());
    ws.use_upstream_test_jdk("TestProject");
    (ws, root)
}
fn organize(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.execute("java.edit.organizeImports", vec![json!(uri)])
}
fn applied(source: &str, uri: &str, edit: &Value) -> String {
    edit["changes"][uri]
        .as_array()
        .map(|edits| apply_edits(source, edits))
        .unwrap_or_else(|| source.to_owned())
}

#[test]
fn applies_on_client_only_when_negotiated_even_for_empty_edits_or_rejected_edits() {
    for accepted in [true, false] {
        let (mut ws, root) = setup();
        ws.capabilities["workspace"]["applyEdit"] = json!(true);
        let source = "import java.util.Set;\npublic class E {}\n";
        let uri = ws.create_cu(&root, "src", "", "E.java", source);
        ws.open(&uri);
        ws.client()
            .request_results
            .insert("workspace/applyEdit".into(), json!({"applied":accepted}));
        let start = ws.client().server_requests.len();
        assert_eq!(organize(&mut ws, &uri), json!({}));
        let requests: Vec<_> = ws.client().server_requests[start..]
            .iter()
            .filter(|r| r["method"] == "workspace/applyEdit")
            .cloned()
            .collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            applied(source, &uri, &requests[0]["params"]["edit"]),
            "public class E {}\n"
        );
        assert_eq!(ws.read(&uri), source);
        // The client has not changed its editor buffer; a positive acknowledgement
        // alone must not mutate the server's working copy either.
        let start = ws.client().server_requests.len();
        assert_eq!(organize(&mut ws, &uri), json!({}));
        let again = ws.client().server_requests[start..]
            .iter()
            .find(|r| r["method"] == "workspace/applyEdit")
            .unwrap();
        assert_eq!(again["params"]["edit"], requests[0]["params"]["edit"]);
        let start = ws.client().server_requests.len();
        assert_eq!(ws.execute("java.edit.organizeImports", vec![]), json!({}));
        let empty = ws.client().server_requests[start..]
            .iter()
            .find(|r| r["method"] == "workspace/applyEdit")
            .unwrap();
        assert_eq!(empty["params"]["edit"], json!({"changes":{}}));
    }
}

#[test]
fn raw_arguments_and_empty_edits_follow_delegate_contract() {
    let (mut ws, root) = setup();
    let empty = json!({"changes":{}});
    for args in [
        vec![],
        vec![Value::Null],
        vec![json!(12)],
        vec![json!({"uri":"Main.java"})],
    ] {
        assert_eq!(ws.execute("java.edit.organizeImports", args), empty);
    }
    assert_eq!(
        ws.try_execute(
            "java.edit.organizeImports",
            vec![json!("no/such/file.java")]
        )
        .unwrap_err(),
        "URI is not found"
    );
    let uri = file_uri(&root.join("Missing.java"));
    assert_eq!(organize(&mut ws, &uri), empty);
    let source = "public class E {}\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    assert_eq!(organize(&mut ws, &uri), empty);
    assert!(ws
        .try_execute(
            "java.edit.organizeImports",
            vec![json!(json!(uri).to_string())]
        )
        .is_err());
    assert!(ws
        .client()
        .server_requests
        .iter()
        .all(|r| r["method"] != "workspace/applyEdit"));
}

#[test]
fn ambiguity_keeps_unique_imports_without_prompting_the_client() {
    let (mut ws, root) = setup();
    ws.init_options["extendedClientCapabilities"]["executeClientCommandSupport"] = json!(true);
    ws.create_cu(
        &root,
        "src",
        "other",
        "List.java",
        "package other; public class List {}\n",
    );
    let source = "package p;\n\npublic class E { List values; ArrayList other; }\n";
    let uri = ws.create_cu(&root, "src", "p", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "package p;\n\nimport java.util.ArrayList;\n\npublic class E { List values; ArrayList other; }\n");
    assert!(ws
        .client()
        .server_requests
        .iter()
        .all(|r| r["params"]["command"] != "java.action.organizeImports.chooseImports"));
}

#[test]
fn used_imports_survive_filters_and_unresolvable_imports_are_preserved() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java": {"completion":{"filteredTypes":["java.util.*"]}}});
    let source = "import java.util.List;\nimport missing.Ghost;\nimport java.util.Set;\npublic class E { List list; Ghost ghost; }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "import java.util.List;\n\nimport missing.Ghost;\npublic class E { List list; Ghost ghost; }\n");
}

#[test]
fn source_action_and_command_share_sorting_and_wildcard_preferences() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java":{"sources":{"organizeImports":{"starThreshold":2}}}});
    let source = "package p;\n\nimport java.util.HashMap;\nimport java.util.ArrayList;\n\npublic class E { ArrayList list; HashMap map; }\n";
    let uri = ws.create_cu(&root, "src", "p", "E.java", source);
    ws.open(&uri);
    let command = organize(&mut ws, &uri);
    let actions = ws.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":range(0,0,0,0),"context":{"diagnostics":[],"only":["source.organizeImports"]}}));
    let mut action = actions.as_array().unwrap()[0].clone();
    if action["edit"].is_null() {
        action = ws.request("codeAction/resolve", action);
    }
    assert_eq!(action["edit"], command, "{action:#}");
    assert_eq!(
        applied(source, &uri, &command),
        "package p;\n\nimport java.util.*;\n\npublic class E { ArrayList list; HashMap map; }\n"
    );
}

#[test]
fn project_scope_includes_tests_and_all_roots_but_omits_unchanged_and_non_sources() {
    let (mut ws, root) = setup();
    std::fs::write(root.join(".classpath"), "<classpath><classpathentry kind=\"src\" path=\"src\" excluding=\"excluded/\"/><classpathentry kind=\"src\" path=\"test\"><attributes><attribute name=\"test\" value=\"true\"/></attributes></classpathentry><classpathentry kind=\"lib\" path=\"lib/rtstubs.jar\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>").unwrap();
    let unused = "import java.util.List;\npublic class E {}\n";
    let first = ws.create_cu(&root, "src", "", "E.java", unused);
    let second = ws.create_cu(
        &root,
        "test",
        "",
        "F.java",
        "import java.util.Set;\npublic class F {}\n",
    );
    ws.create_cu(&root, "src", "excluded", "E.java", unused);
    ws.create_cu(&root, "outside", "", "E.java", unused);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Unchanged.java",
        "package p; public class Unchanged {}\n",
    );
    let other = ws.new_project("Other", &test_default_options());
    ws.create_cu(&other, "src", "", "E.java", unused);
    let edit = organize(&mut ws, &dir_uri(&root));
    let changes = edit["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 2, "{edit:#}");
    assert_eq!(applied(unused, &first, &edit), "public class E {}\n");
    assert_eq!(
        applied("import java.util.Set;\npublic class F {}\n", &second, &edit),
        "public class F {}\n"
    );
    assert_eq!(ws.read(&first), unused);
}

#[test]
fn directory_arguments_match_the_pinned_resource_routing() {
    let (mut ws, root) = setup();
    ws.create_cu(
        &root,
        "src",
        "p",
        "E.java",
        "import java.util.List;\npublic class E {}\n",
    );
    for folder in [root.join("src"), root.join("src/p"), root.join("lib")] {
        for uri in [file_uri(&folder), dir_uri(&folder)] {
            assert_eq!(organize(&mut ws, &uri), json!({"changes":{}}));
        }
    }
}

#[test]
fn direct_package_collection_matches_substring_matching_and_source_root_scope() {
    let units: Vec<_> = ["p", "p.child", "other.p", "prefix", "q"]
        .iter()
        .map(|p| {
            (
                Url::parse(&format!("file:///src/{}/E.java", p.replace('.', "/"))).unwrap(),
                (*p).into(),
            )
        })
        .collect();
    let selected = scope::collect_compilation_units(&units, Some("p"));
    assert_eq!(
        selected,
        units[..4]
            .iter()
            .map(|(u, _)| u.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(scope::collect_compilation_units(&units, Some(" ")).len(), 5);
}

#[test]
fn unsaved_crlf_and_unicode_buffers_keep_their_comments_and_disk_contents() {
    let (mut ws, root) = setup();
    let saved = "public class E {}\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", saved);
    let source = "// 😀 header\r\nimport java.util.Set; // unused\r\nimport java.util.ArrayList; // values\r\npublic class E { ArrayList values; }\r\n";
    ws.open_with(&uri, source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "// 😀 header\r\nimport java.util.ArrayList; // values\r\npublic class E { ArrayList values; }\r\n");
    assert_eq!(ws.read(&uri), saved);
}

#[test]
fn fully_qualified_and_lexically_declared_references_need_no_imports() {
    let (mut ws, root) = setup();
    let source = "public class E { static class ArrayList {} static int value; java.util.List<String> list; ArrayList local; int get() { return value; } }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    assert_eq!(organize(&mut ws, &uri), json!({"changes":{}}));
}

#[test]
fn static_favorites_follow_the_upstream_configuration_update_sequence() {
    let (mut ws, root) = setup();
    ws.settings = json!({"java":{"completion":{"favoriteStaticMembers":["java.lang.Math.*"]}}});
    let source = "public class E { double get() { return sqrt(4) + pow(2, 2); } }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    assert_eq!(organize(&mut ws, &uri), json!({"changes":{}}));
    // Preferences.updateFrom persists the previous favorite-members value.
    // Sending the same setting again makes Math available to organize imports.
    ws.update_settings(
        json!({"java":{"completion":{"favoriteStaticMembers":["java.lang.Math.*"]}}}),
    );
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "import static java.lang.Math.pow;\nimport static java.lang.Math.sqrt;\n\npublic class E { double get() { return sqrt(4) + pow(2, 2); } }\n");
}

#[test]
fn syntax_recovery_organizes_imports_without_rewriting_the_method() {
    let (mut ws, root) = setup();
    let source = "import java.util.Set;\npublic class E { ArrayList values; void m( { }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(
        applied(source, &uri, &edit),
        "import java.util.ArrayList;\npublic class E { ArrayList values; void m( { }\n"
    );
}

#[test]
fn virtual_documents_organize_without_creating_source_files() {
    if common::jdtls::is_oracle() {
        return;
    }
    let (mut ws, _) = setup();
    for uri in [
        "untitled:Virtual.java".to_owned(),
        "inmemory://editor/Virtual.java".to_owned(),
        ws.path_uri("Virtual.java"),
    ] {
        let source = "import java.util.Set;\npublic class Virtual { ArrayList<String> values; }\n";
        ws.open_with(&uri, source);
        let edit = organize(&mut ws, &uri);
        assert_eq!(
            applied(source, &uri, &edit),
            "import java.util.ArrayList;\npublic class Virtual { ArrayList<String> values; }\n"
        );
        if let Ok(path) = Url::parse(&uri).unwrap().to_file_path() {
            assert!(!path.exists());
        }
    }
}

#[test]
fn unused_unresolvable_normal_static_and_wildcard_imports_are_removed() {
    let (mut ws, root) = setup();
    let source = "import missing.Ghost;\nimport missing.*;\nimport static missing.Type.method;\nimport static missing.Type.*;\npublic class E {}\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "public class E {}\n");
}

#[test]
fn type_search_uses_the_selected_runtime_image_in_each_project() {
    let mut ws = Workspace::new();
    ws.capabilities["workspace"]["applyEdit"] = json!(false);
    let native = PathBuf::from(common::jdtls::java_home());
    let major = std::fs::read_to_string(native.join("release"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION=\""))
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .to_owned();
    let mut options = test_default_options();
    for key in [
        "org.eclipse.jdt.core.compiler.source",
        "org.eclipse.jdt.core.compiler.compliance",
        "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
    ] {
        options.insert(key.into(), major.clone());
    }
    let current = ws.new_project("Current", &options);
    let cp_path = current.join(".classpath");
    let cp = std::fs::read_to_string(&cp_path).unwrap().replace(
        "<classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/>",
        "<classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"><attributes><attribute name=\"module\" value=\"true\"/></attributes></classpathentry>",
    );
    std::fs::write(cp_path, cp).unwrap();
    let legacy = ws.new_project("Legacy", &test_default_options());
    ws.create_cu(&legacy, "src", "", "E.java", "public class E {}\n");
    // Dispatcher's explicit-VM classpath contains jrt-fs.jar. Searching that
    // provider jar cannot find ArrayList; the compiler must list its VM image.
    let source = "public class E { ArrayList list; Gatherer gatherer; }\n";
    let first = ws.create_cu(&current, "src", "", "E.java", source);
    let mut runtimes = vec![json!({"name":format!("JavaSE-{major}"),"path":native,"default":true})];
    let java21 = std::env::var_os("JAVA21_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".oracle/jdks/jdk-21.0.12.1+1/Contents/Home")
        });
    let older = if java21.join("lib/jrt-fs.jar").is_file() && major.parse::<u32>().unwrap() >= 24 {
        for key in [
            "org.eclipse.jdt.core.compiler.source",
            "org.eclipse.jdt.core.compiler.compliance",
            "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
        ] {
            options.insert(key.into(), "21".into());
        }
        let root = ws.new_project("Older", &options);
        let path = root.join(".classpath");
        let cp = std::fs::read_to_string(&path).unwrap().replace("org.eclipse.jdt.launching.JRE_CONTAINER", "org.eclipse.jdt.launching.JRE_CONTAINER/org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType/JavaSE-21");
        let cp = cp.replace("path=\"org.eclipse.jdt.launching.JRE_CONTAINER/org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType/JavaSE-21\"/>", "path=\"org.eclipse.jdt.launching.JRE_CONTAINER/org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType/JavaSE-21\"><attributes><attribute name=\"module\" value=\"true\"/></attributes></classpathentry>");
        std::fs::write(path, cp).unwrap();
        runtimes.push(json!({"name":"JavaSE-21","path":java21}));
        Some(ws.create_cu(&root, "src", "", "E.java", source))
    } else {
        None
    };
    ws.settings = json!({"java":{"configuration":{"runtimes":runtimes}}});
    let edit = organize(&mut ws, &first);
    let imports = if major.parse::<u32>().unwrap() >= 22 {
        "import java.util.ArrayList;\nimport java.util.stream.Gatherer;\n\n"
    } else {
        "import java.util.ArrayList;\n\n"
    };
    assert_eq!(applied(source, &first, &edit), format!("{imports}{source}"));
    let symbols = ws.request(
        "java/searchSymbols",
        json!({"query":"java.lang.Object","projectName":"Current"}),
    );
    let objects: Vec<_> = symbols
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["name"] == "Object" && s["containerName"] == "java.lang")
        .collect();
    assert_eq!(objects.len(), 1, "{symbols:#}");
    assert!(objects[0]["location"]["uri"]
        .as_str()
        .unwrap()
        .contains("/java.base/java.lang/Object.java?"));
    assert!(objects[0]["location"]["uri"]
        .as_str()
        .unwrap()
        .contains("=/module=/true=/"));
    let symbols = ws.request(
        "java/searchSymbols",
        json!({"query":"java.lang.Object","projectName":"Legacy"}),
    );
    let objects: Vec<_> = symbols
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["name"] == "Object" && s["containerName"] == "java.lang")
        .collect();
    assert_eq!(objects.len(), 1, "{symbols:#}");
    assert!(
        objects[0]["location"]["uri"]
            .as_str()
            .unwrap()
            .contains("/java.base/java.lang/Object.java?"),
        "{symbols:#}"
    );
    assert!(!objects[0]["location"]["uri"]
        .as_str()
        .unwrap()
        .contains("=/module=/true=/"));
    if let Some(uri) = older {
        let edit = organize(&mut ws, &uri);
        assert_eq!(
            applied(source, &uri, &edit),
            format!("import java.util.ArrayList;\n\n{source}")
        );
        let symbols = ws.request(
            "java/searchSymbols",
            json!({"query":"java.util.stream.Gatherer","projectName":"Older"}),
        );
        assert!(symbols.as_array().unwrap().is_empty(), "{symbols:#}");
    }
}

#[test]
fn used_unresolvable_static_imports_precede_favorites() {
    let (mut ws, root) = setup();
    let source = "import static missing.Type.method;\nimport static missing.Type.value;\nimport static missing.Type.unused;\npublic class E { int get() { method(); return value; } }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "import static missing.Type.method;\nimport static missing.Type.value;\npublic class E { int get() { method(); return value; } }\n");
}

#[test]
fn unknown_wildcard_imports_are_retained_for_unresolved_references() {
    let (mut ws, root) = setup();
    let source = "import missing.*;\nimport static missing.Type.*;\npublic class E { Ghost ghost; int get() { return method(); } }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "import static missing.Type.*;\n\nimport missing.*;\npublic class E { Ghost ghost; int get() { return method(); } }\n");
}

#[test]
fn existing_wildcards_expand_when_fewer_types_are_used_than_the_threshold() {
    let (mut ws, root) = setup();
    let source = "import java.util.*;\npublic class E { ArrayList list; HashMap map; }\n";
    let uri = ws.create_cu(&root, "src", "", "E.java", source);
    let edit = organize(&mut ws, &uri);
    assert_eq!(applied(source, &uri, &edit), "import java.util.ArrayList;\nimport java.util.HashMap;\npublic class E { ArrayList list; HashMap map; }\n");
}
