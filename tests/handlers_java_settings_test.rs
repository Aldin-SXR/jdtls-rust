//! Port of `org.eclipse.jdt.ls.core.internal.handlers.JavaSettingsTest`.
//!
//! `preferences.setSettingsUrl(url)` / `setFormatterUrl(url)` followed by
//! `StandardProjectsManager.configureSettings`, `registerListeners` or
//! `projectsManager.fileChanged` become `workspace/didChangeConfiguration`
//! (and `workspace/didChangeWatchedFiles`) notifications. `JavaCore.getOption`
//! and `javaProject.getOption(key, true)` are both read with
//! `java.project.getSettings` on the `hello` project, which sets neither
//! option itself. Relative URLs resolve against the root path as upstream
//! (`target/workingProjects`, two levels below the `formatter` folder): the
//! root path is the `eclipse/hello` folder and `formatter` is copied two
//! levels above it.

mod common;
use common::jdtls::*;
use serde_json::{json, Value};
use std::time::Duration;

const MISSING_SERIAL_VERSION: &str = "org.eclipse.jdt.core.compiler.problem.missingSerialVersion";
const STATIC_ACCESS_RECEIVER: &str = "org.eclipse.jdt.core.compiler.problem.staticAccessReceiver";
const FORMATTER_BRACE_POSITION_FOR_BLOCK: &str = "org.eclipse.jdt.core.formatter.brace_position_for_block";
const END_OF_LINE: &str = "end_of_line";
const NEXT_LINE: &str = "next_line";

struct Fixture {
    ws: Workspace,
    /// The latest marker count published for `TestSerial.java`.
    test_serial_markers: usize,
}

/// `setUp`: `importProjects("eclipse/hello")`.
fn set_up() -> Fixture {
    let mut ws = Workspace::new();
    copy_dir(&fixtures_dir().join("formatter"), &ws.dir.join("formatter"));
    ws.import_projects(&["eclipse/hello"]);
    let root = url::Url::from_file_path(ws.project_root("hello")).unwrap().to_string();
    ws.init_options["workspaceFolders"] = json!([root]);
    ws.client();
    Fixture { ws, test_serial_markers: 0 }
}

impl Fixture {
    fn option(&mut self, key: &str) -> Value {
        let uri = self.ws.project_uri("hello");
        let result = self.ws.request(
            "workspace/executeCommand",
            json!({ "command": "java.project.getSettings", "arguments": [uri, [key]] }),
        );
        result[key].clone()
    }

    /// `assertEquals(value, JavaCore.getOption(key)); assertEquals(value, javaProject.getOption(key, true))`.
    fn assert_option(&mut self, value: &str, key: &str) {
        assert_eq!(json!(value), self.option(key), "{key}");
    }

    fn configure(&mut self, settings_url: Option<&str>, formatter_url: Option<&str>) {
        self.ws.client().notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": { "java": {
                "settings": { "url": settings_url },
                "format": { "settings": { "url": formatter_url } }
            } } }),
        );
        self.wait_for_background_jobs();
    }

    fn wait_for_background_jobs(&mut self) {
        self.ws.wait_idle();
        let uri = self.ws.class_uri("hello", "org.sample.TestSerial");
        let c = self.ws.client();
        c.settle(Duration::from_secs(2), Duration::from_secs(60));
        for report in c.take_notifications("textDocument/publishDiagnostics") {
            if report["params"]["uri"] == uri.as_str() {
                self.test_serial_markers = report["params"]["diagnostics"].as_array().unwrap().len();
            }
        }
    }

    /// `testMarkers(count)`: the markers of `org.sample.TestSerial`.
    fn test_markers(&mut self, count: usize) {
        self.wait_for_background_jobs();
        assert_eq!(count, self.test_serial_markers);
    }

    /// `projectsManager.fileChanged(uri, CHANGE_TYPE.CHANGED)`.
    fn file_changed(&mut self, relative: &str) {
        let path = self.ws.project_root("hello").join(relative);
        let path = path.canonicalize().unwrap();
        self.ws.notify_file_changed(&path, 2);
        self.wait_for_background_jobs();
    }
}

#[test]
fn test_file_path() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.test_markers(0);
    let file = f.ws.dir.join("formatter/settings.prefs");
    assert!(file.exists());
    // `preferences.getSettingsAsURI().isAbsolute()`: the absolute path is used as is.
    f.configure(Some(file.to_str().unwrap()), None);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.test_markers(1);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.test_markers(0);
}

#[test]
fn test_relative_file_path() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    let settings_url = "../../formatter/settings.prefs";
    f.configure(Some(settings_url), None);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
}

#[test]
fn test_file_changed() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    let settings_url = "../../formatter/settings.prefs";
    f.configure(Some(settings_url), None);
    f.file_changed(settings_url);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
}

// https://github.com/redhat-developer/vscode-java/issues/1944
#[test]
fn test_file_changed_on_windows() {
    if cfg!(windows) {
        let mut f = set_up();
        f.assert_option(END_OF_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
        let formatter_url = "..\\\\..\\\\formatter\\\\test.xml";
        f.configure(None, Some(formatter_url));
        f.file_changed("../../formatter/test.xml");
        f.assert_option(NEXT_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
        f.configure(None, None);
        f.assert_option(END_OF_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
    }
}

#[test]
fn test_formatter() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option(END_OF_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
    let settings_url = "../../formatter/settings.prefs";
    let formatter_url = "../../formatter/test.xml";
    f.configure(Some(settings_url), Some(formatter_url));
    f.file_changed(formatter_url);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.assert_option(NEXT_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option(END_OF_LINE, FORMATTER_BRACE_POSITION_FOR_BLOCK);
}

// https://github.com/redhat-developer/vscode-java/issues/1939
#[test]
fn test_settings_v3() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option("warning", STATIC_ACCESS_RECEIVER);
    let settings_url = "../../formatter/settings2.prefs";
    f.configure(Some(settings_url), None);
    f.file_changed(settings_url);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.assert_option("ignore", STATIC_ACCESS_RECEIVER);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option("warning", STATIC_ACCESS_RECEIVER);
}

#[test]
fn test_settings() {
    let mut f = set_up();
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option("warning", STATIC_ACCESS_RECEIVER);
    let settings_url = "../../formatter/settings2.prefs";
    // `projectsManager.registerListeners()` configures the settings.
    f.configure(Some(settings_url), None);
    f.assert_option("warning", MISSING_SERIAL_VERSION);
    f.assert_option("ignore", STATIC_ACCESS_RECEIVER);
    f.configure(None, None);
    f.assert_option("ignore", MISSING_SERIAL_VERSION);
    f.assert_option("warning", STATIC_ACCESS_RECEIVER);
}

// https://github.com/redhat-developer/vscode-java/issues/2222
#[test]
#[ignore = "observes the Java builder's output: the Rust build does not write class files to the project's output folder"]
fn test_configure_settings() {
    let mut f = set_up();
    let file = f.ws.project_root("hello").join("bin/org/sample/Test.class");
    let last_modified = std::fs::metadata(&file).and_then(|m| m.modified()).unwrap();
    f.wait_for_background_jobs();
    assert_eq!(last_modified, std::fs::metadata(&file).and_then(|m| m.modified()).unwrap());
}
