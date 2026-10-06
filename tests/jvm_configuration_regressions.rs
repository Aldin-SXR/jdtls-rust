//! Exercise runtime settings through the public LSP configuration flow.
mod common;
use common::jdtls::{apply_edits, java_home, Workspace};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const VM: &str = "org.eclipse.jdt.ls.core.vm.location";
const SOURCE: &str = "org.eclipse.jdt.core.compiler.source";
fn runtime(home: &Path, default: bool) -> Value {
    let release = std::fs::read_to_string(home.join("release")).unwrap();
    let version = release
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))
        .unwrap()
        .trim_matches('"')
        .split('.')
        .next()
        .unwrap();
    json!({"name":format!("JavaSE-{version}"),"path":home,"default":default})
}
fn selected_home(ws: &mut Workspace, root: &Path) -> PathBuf {
    let uri = tower_lsp::lsp_types::Url::from_file_path(&root)
        .unwrap()
        .to_string();
    PathBuf::from(ws.project_settings(&uri, &[VM])[VM].as_str().unwrap())
}

#[test]
fn execution_environments_select_separate_project_vms() {
    let mut ws = Workspace::new();
    let native = PathBuf::from(java_home());
    let alternate = ws.dir.join("environment-jdk");
    std::fs::create_dir_all(&alternate).unwrap();
    for name in ["bin", "lib", "release"] {
        #[cfg(unix)]
        std::os::unix::fs::symlink(native.join(name), alternate.join(name)).unwrap();
    }
    let current = runtime(&native, true);
    let current_environment = current["name"].as_str().unwrap().to_owned();
    let mut roots = Vec::new();
    for (name, environment) in [("Java21", "JavaSE-21"), ("Current", &current_environment)] {
        let options = [(
            SOURCE.to_owned(),
            environment.strip_prefix("JavaSE-").unwrap().to_owned(),
        )]
        .into_iter()
        .collect();
        let root = ws.new_project(name, &options);
        let path = root.join(".classpath");
        let classpath = std::fs::read_to_string(&path).unwrap().replace(
            "path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"",
            &format!("path=\"org.eclipse.jdt.launching.JRE_CONTAINER/org.eclipse.jdt.internal.debug.ui.launcher.StandardVMType/{environment}\""),
        );
        std::fs::write(path, classpath).unwrap();
        roots.push(root);
    }
    ws.settings =
        json!({"java.configuration.runtimes":[current,{"name":"JavaSE-21","path":alternate}]});
    assert_eq!(alternate, selected_home(&mut ws, &roots[0]));
    assert_eq!(native, selected_home(&mut ws, &roots[1]));
    for root in roots {
        let uri = ws.create_cu(
            &root,
            "src",
            "",
            "Main.java",
            "public class Main { String value = 7; }",
        );
        ws.open(&uri);
        assert!(ws
            .diagnostics(&uri)
            .iter()
            .any(|d| d["message"] == "Type mismatch: cannot convert from int to String"));
    }
}

#[test]
fn selected_native_runtime_compiles_and_offers_imports_in_virtual_documents() {
    let home = PathBuf::from(java_home());
    let source = "import java.util.List;\npublic class Main { void run() { List<String> items = new ArrayList<>(); int x = \"bad\"; } }";
    for scheme in ["untitled", "inmemory", "file"] {
        let mut ws = Workspace::new();
        ws.settings = json!({"java":{"configuration":{"runtimes":[runtime(&home,true)]}}});
        let path = ws.dir.join("Main.java");
        let uri = match scheme {
            "untitled" => "untitled:Main.java".to_owned(),
            "inmemory" => "inmemory:///Main.java".to_owned(),
            _ => tower_lsp::lsp_types::Url::from_file_path(&path)
                .unwrap()
                .to_string(),
        };
        ws.open_with(&uri, source);
        let diagnostics = ws.diagnostics(&uri);
        assert!(
            diagnostics
                .iter()
                .any(|d| d["message"] == "Type mismatch: cannot convert from String to int"),
            "{scheme}: {diagnostics:#?}"
        );
        let missing = diagnostics
            .iter()
            .find(|d| d["message"] == "ArrayList cannot be resolved to a type")
            .unwrap();
        let actions = ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":missing["range"],"context":{"diagnostics":[missing],"only":["quickfix"]}}));
        let action = actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == "Import 'java.util.ArrayList'")
            .unwrap_or_else(|| panic!("{scheme}: {actions:#?}"));
        let corrected = apply_edits(source, action["edit"]["changes"][&uri].as_array().unwrap());
        assert!(
            corrected.contains("import java.util.ArrayList;"),
            "{corrected}"
        );
        let symbols = ws.request("workspace/symbol", json!({"query":"java.lang.Object"}));
        assert_eq!(
            1,
            symbols
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["name"] == "Object" && s["containerName"] == "java.lang")
                .count(),
            "{symbols:#?}"
        );
        assert!(!path.exists());
    }
}

#[test]
fn runtime_changes_update_project_vm_without_restarting_the_bridge_on_that_vm() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&Default::default());
    let original = PathBuf::from(java_home());
    let alternative = ws.dir.join("alternate-jdk");
    std::fs::create_dir_all(&alternative).unwrap();
    for name in ["bin", "lib", "release"] {
        #[cfg(unix)]
        std::os::unix::fs::symlink(original.join(name), alternative.join(name)).unwrap();
    }
    ws.settings = json!({"java.configuration.runtimes":[runtime(&alternative,true)]});
    assert_eq!(alternative, selected_home(&mut ws, &root));
    let uri = ws.create_cu(
        &root,
        "src",
        "",
        "Main.java",
        "public class Main { String value = 7; }",
    );
    ws.open(&uri);
    assert!(ws
        .diagnostics(&uri)
        .iter()
        .any(|d| d["message"] == "Type mismatch: cannot convert from int to String"));
    ws.update_settings(json!({"java":{"configuration":{"runtimes":[runtime(&original,true)]}}}));
    assert_eq!(original, selected_home(&mut ws, &root));
    assert!(ws
        .diagnostics(&uri)
        .iter()
        .any(|d| d["message"] == "Type mismatch: cannot convert from int to String"));
    let invalid = common::jdtls::fixtures_dir().join("fakejdk2/21a");
    ws.update_settings(json!({"java.home":invalid,"java.configuration.runtimes":null}));
    assert_eq!(invalid, selected_home(&mut ws, &root));
    // The fake VM has an empty executable. A project-VM change must leave the
    // already running compiler process available for subsequent configuration.
    ws.update_settings(json!({"java.home":original}));
    assert_eq!(original, selected_home(&mut ws, &root));
    assert!(ws
        .diagnostics(&uri)
        .iter()
        .any(|d| d["message"] == "Type mismatch: cannot convert from int to String"));
}

#[test]
fn invalid_runtime_notifications_use_the_negotiated_client_capability() {
    for actionable in [false, true] {
        let mut ws = Workspace::new();
        ws.init_options["extendedClientCapabilities"]["actionableRuntimeNotificationSupport"] =
            json!(actionable);
        let path = ws.dir.join("missing-jdk");
        ws.settings = json!({"java.configuration.runtimes":[{"name":"JavaSE-21","path":path}]});
        ws.wait_idle();
        assert!(!ws.client().notifications.iter().any(|n| {
            n["params"]["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("Invalid runtime for JavaSE-21:"))
                && ["language/actionableNotification", "window/showMessage"]
                    .contains(&n["method"].as_str().unwrap_or_default())
        }));
        ws.update_settings(
            json!({"java.configuration.runtimes":[{"name":"JavaSE-21","path":path}]}),
        );
        let method = if actionable {
            "language/actionableNotification"
        } else {
            "window/showMessage"
        };
        let notifications: Vec<_> = ws
            .client()
            .notifications
            .iter()
            .filter(|n| {
                n["method"] == method
                    && n["params"]["message"]
                        .as_str()
                        .is_some_and(|m| m.starts_with("Invalid runtime for JavaSE-21:"))
            })
            .collect();
        assert_eq!(1, notifications.len(), "{notifications:#?}");
        let params = &notifications[0]["params"];
        assert_eq!(format!("Invalid runtime for JavaSE-21: The path points to a missing or inaccessible folder ({}).",path.display()), params["message"]);
        if actionable {
            assert_eq!(1, params["severity"]);
            assert_eq!(
                "java.runtimeValidation.open",
                params["commands"][0]["command"]
            );
        } else {
            assert_eq!(1, params["type"]);
        }
    }
}

#[test]
fn runtime_source_and_javadoc_attachments_reach_project_classpaths() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&Default::default());
    let home = PathBuf::from(java_home());
    let source = ws.dir.join("sources.zip");
    let output = std::process::Command::new("python3").arg("-c").arg(
        "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); s=z.read('java.base/java/lang/String.java'); o=zipfile.ZipFile(sys.argv[2],'w'); o.writestr('java.base/java/lang/String.java',b'// CONFIGURED_RUNTIME_SOURCE\\n'+s); o.close()"
    ).arg(home.join("lib/src.zip")).arg(&source).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut configured = runtime(&home, true);
    configured["sources"] = json!(source);
    configured["javadoc"] = json!("https://example.com/javadoc/");
    ws.settings = json!({"java.configuration.runtimes":[configured]});
    let uri = tower_lsp::lsp_types::Url::from_file_path(&root)
        .unwrap()
        .to_string();
    let cu = ws.create_cu(
        &root,
        "src",
        "",
        "Main.java",
        "public class Main { String value; }",
    );
    ws.open(&cu);
    let definition = ws.request(
        "textDocument/definition",
        json!({"textDocument":{"uri":cu},"position":{"line":0,"character":22}}),
    );
    let class_uri = definition[0]["uri"]
        .as_str()
        .unwrap_or_else(|| panic!("{definition:#?}"));
    assert!(class_uri.contains("example.com"), "{class_uri}");
    let contents = ws.request("java/classFileContents", json!({"uri":class_uri}));
    assert!(
        contents
            .as_str()
            .unwrap()
            .contains("CONFIGURED_RUNTIME_SOURCE"),
        "{contents:#?}"
    );
    assert!(ws.project_settings(&uri, &[SOURCE])[SOURCE].is_string());
}
