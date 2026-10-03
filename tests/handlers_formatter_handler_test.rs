//! Port of `org.eclipse.jdt.ls.core.internal.handlers.FormatterHandlerTest`.
//!
//! Upstream drives `FormatterHandler` and `Preferences` directly; here every
//! test goes through the protocol:
//! * `preferences.setXxx(..)` + `StandardProjectsManager.configureSettings`
//!   become `java.format.*` settings (initialization options, or
//!   `workspace/didChangeConfiguration` when a test changes them midway);
//! * `JavaCore.getOption(key)` is read with `java.project.getSettings` on the
//!   `hello` project (which does not override the formatter keys checked);
//! * `FormatterHandler.stringFormatting` is the `java.edit.stringFormatting`
//!   command it backs;
//! * `FormatterHandler.formatJavaCode` is reached by formatting a notebook
//!   cell URI registered through the `nonStandardJavaFormatting` extended
//!   client capability, the client returning the cell content.

mod common;
use common::jdtls::{apply_edits, copy_dir, fixtures_dir, pos, range, Workspace};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

const FORMATTER_TAB_CHAR: &str = "org.eclipse.jdt.core.formatter.tabulation.char";
const FORMATTER_INDENT_SWITCHSTATEMENTS_COMPARE_TO_SWITCH: &str = "org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_switch";
const FORMATTER_BRACE_POSITION_FOR_BLOCK: &str = "org.eclipse.jdt.core.formatter.brace_position_for_block";
const FORMATTER_JOIN_WRAPPED_LINES: &str = "org.eclipse.jdt.core.formatter.join_wrapped_lines";
const FORMATTER_JOIN_LINES_IN_COMMENTS: &str = "org.eclipse.jdt.core.formatter.join_lines_in_comments";
const FORMATTER_USE_ON_OFF_TAGS: &str = "org.eclipse.jdt.core.formatter.use_on_off_tags";
const NOTEBOOK_CALLBACK: &str = "java.notebook.getContent";

/// `AbstractCompilationUnitBasedTest.setup` (`eclipse/hello`) + `setUp`
/// (`javaProject.setOption(FORMATTER_TAB_CHAR, TAB)`).
fn setup() -> (Workspace, PathBuf) {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    let root = ws.project_root("hello");
    set_project_option(&root, FORMATTER_TAB_CHAR, "tab");
    (ws, root)
}

/// `IJavaProject.setOption(key, value)`: update the project's JDT core prefs.
fn set_project_option(root: &Path, key: &str, value: &str) {
    let prefs = root.join(".settings").join("org.eclipse.jdt.core.prefs");
    let text = std::fs::read_to_string(&prefs).unwrap_or_else(|_| "eclipse.preferences.version=1\n".to_owned());
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&format!("{key}="))).map(str::to_owned).collect();
    lines.push(format!("{key}={value}"));
    std::fs::create_dir_all(prefs.parent().unwrap()).unwrap();
    std::fs::write(&prefs, lines.join("\n") + "\n").unwrap();
}

/// Set a `java.*` preference (dotted key below `java`) before the server starts.
fn set_pref(ws: &mut Workspace, path: &[&str], value: Value) {
    let mut cur = &mut ws.settings["java"];
    for p in &path[..path.len() - 1] {
        if cur.get(*p).is_none() {
            cur[*p] = json!({});
        }
        cur = &mut cur[*p];
    }
    cur[path[path.len() - 1]] = value;
}

/// `getWorkingCopy(path, source)`: the compilation unit with `source` as its buffer.
fn get_working_copy(ws: &mut Workspace, root: &Path, path: &str, source: &str) -> String {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, source).unwrap();
    let uri = Url::from_file_path(&file).unwrap().to_string();
    ws.open_with(&uri, source);
    uri
}

fn options(tab_size: u32, insert_spaces: bool) -> Value {
    json!({ "tabSize": tab_size, "insertSpaces": insert_spaces })
}

fn formatting(ws: &mut Workspace, uri: &str, options: Value) -> Vec<Value> {
    let result = ws.request("textDocument/formatting", json!({ "textDocument": { "uri": uri }, "options": options }));
    assert!(!result.is_null(), "edits must not be null");
    result.as_array().cloned().unwrap()
}

fn range_formatting(ws: &mut Workspace, uri: &str, options: Value, range: Value) -> Vec<Value> {
    let result = ws.request(
        "textDocument/rangeFormatting",
        json!({ "textDocument": { "uri": uri }, "options": options, "range": range }),
    );
    assert!(!result.is_null(), "edits must not be null");
    result.as_array().cloned().unwrap()
}

fn on_type_formatting(ws: &mut Workspace, uri: &str, options: Value, position: Value, ch: &str) -> Vec<Value> {
    let result = ws.request(
        "textDocument/onTypeFormatting",
        json!({ "textDocument": { "uri": uri }, "options": options, "position": position, "ch": ch }),
    );
    assert!(!result.is_null(), "edits must not be null");
    result.as_array().cloned().unwrap()
}

/// `JavaCore.getOption(key)`.
fn java_core_option(ws: &mut Workspace, key: &str) -> Value {
    let uri = ws.project_uri("hello");
    let result = ws.request(
        "workspace/executeCommand",
        json!({ "command": "java.project.getSettings", "arguments": [uri, [key]] }),
    );
    result[key].clone()
}

/// `preferences.setFormatterUrl(url); preferences.setFormatterProfileName(profile);
/// StandardProjectsManager.configureSettings(preferences)` on a running server.
fn configure_formatter(ws: &mut Workspace, url: Option<&str>, profile: Option<&str>) {
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "java": { "format": { "settings": { "url": url, "profile": profile } } } } }),
    );
    ws.wait_idle();
}

/// `FormatterHandler.stringFormatting(content, options, version)`.
fn string_formatting(ws: &mut Workspace, content: &str, options: Value, version: i32) -> String {
    let result = ws.request(
        "workspace/executeCommand",
        json!({ "command": "java.edit.stringFormatting", "arguments": [content, options, version.to_string()] }),
    );
    result.as_str().unwrap_or_else(|| panic!("unexpected stringFormatting result {result}")).to_owned()
}

/// `ProfileVersionerCore.getCurrentVersion()`.
const CURRENT_VERSION: i32 = 23;

/// `FormatterHandler.getCombinedDefaultFormatterSettings()` as sent to
/// `stringFormatting`: there it goes through `ProfileVersionerCore.updateAndComplete`,
/// which starts from the Eclipse defaults, so the Eclipse-default half of the
/// map does not change the result; the jdt.ls defaults are the part that
/// matters (`getJavaLSDefaultFormatterSettings()`).
fn combined_default_formatter_settings() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(FORMATTER_JOIN_WRAPPED_LINES.to_owned(), json!("false"));
    map.insert(FORMATTER_JOIN_LINES_IN_COMMENTS.to_owned(), json!("false"));
    map.insert(FORMATTER_INDENT_SWITCHSTATEMENTS_COMPARE_TO_SWITCH.to_owned(), json!("true"));
    map.insert(FORMATTER_USE_ON_OFF_TAGS.to_owned(), json!("true"));
    map
}

/// Upstream resolves `../../formatter/test.xml` against its root path
/// `target/workingProjects`, two levels below the test bundle's `formatter`
/// folder.  Here the root path (`initializationOptions.workspaceFolders`) is
/// the `eclipse/hello` folder, two levels below the working directory, and
/// the `formatter` folder is copied there.
fn install_formatter_fixtures(ws: &mut Workspace) {
    copy_dir(&fixtures_dir().join("formatter"), &ws.dir.join("formatter"));
    let root = Url::from_file_path(ws.project_root("hello")).unwrap().to_string();
    ws.init_options["workspaceFolders"] = json!([root]);
}

fn formatter_resource(name: &str) -> PathBuf {
    fixtures_dir().join("formatter resources").join(name)
}

/// Format a notebook cell whose content the client provides
/// (`FormatterHandler.formatJavaCode(text, options, range, monitor)`).
fn format_java_code(text: &str, options: Value, range: Option<Value>) -> Vec<Value> {
    let (mut ws, _root) = setup();
    ws.init_options["extendedClientCapabilities"] = json!({
        "nonStandardJavaFormatting": {
            "schemes": ["vscode-notebook-cell"],
            "extensions": [".java"],
            "getContentCallback": NOTEBOOK_CALLBACK
        }
    });
    ws.client().request_results.insert("workspace/executeClientCommand".to_owned(), json!(text));
    let uri = "vscode-notebook-cell:/notebook/test.ipynb#W0sZmlsZQ%3D%3D";
    let result = match range {
        None => ws.request("textDocument/formatting", json!({ "textDocument": { "uri": uri }, "options": options })),
        Some(r) => ws.request("textDocument/rangeFormatting", json!({ "textDocument": { "uri": uri }, "options": options, "range": r })),
    };
    assert!(!result.is_null(), "edits must not be null");
    result.as_array().cloned().unwrap()
}

#[test]
fn test_document_formatting() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample   ;\n\n", "      public class Baz {  String name;}\n");
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = formatting(&mut ws, &uri, options(4, true)); // ident == 4 spaces
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "    String name;\n", "}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test]
fn test_java_format_enable() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample   ;\n\n", "      public class Baz {  String name;}\n");
    set_pref(&mut ws, &["format", "enabled"], json!(false));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = formatting(&mut ws, &uri, options(4, true)); // ident == 4 spaces
    assert_eq!(text, apply_edits(text, &edits));
}

#[test]
fn test_document_formatting_with_tabs() {
    let (mut ws, root) = setup();
    set_project_option(&root, FORMATTER_TAB_CHAR, "space");
    let text = concat!("package org.sample;\n\n", "public class Baz {\n", "    void foo(){\n", "}\n", "}\n");
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = formatting(&mut ws, &uri, options(2, false)); // ident == tab
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "\tvoid foo() {\n", "\t}\n", "}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test]
fn test_formatting_on_off_tags() {
    let (mut ws, root) = setup();
    let text = concat!(
        "package org.sample;\n\n",
        "      public class Baz {\n",
        "// @formatter:off\n",
        "\tvoid foo(){\n",
        "    }\n",
        "// @formatter:on\n",
        "}\n"
    );
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = formatting(&mut ws, &uri, options(4, false)); // ident == tab
    let expected_text = concat!(
        "package org.sample;\n\n",
        "public class Baz {\n",
        "// @formatter:off\n",
        "\tvoid foo(){\n",
        "    }\n",
        "// @formatter:on\n",
        "}\n"
    );
    assert_eq!(expected_text, apply_edits(text, &edits));
}

const SWITCH_SOURCE: &str = concat!(
    "package org.sample;\n",
    "\n",
    "public class Baz {\n",
    "    private enum Numbers {One, Two};\n",
    "    public void foo() {\n",
    "        Numbers n = Numbers.One;\n",
    "        switch (n) {\n",
    "        case One:\n",
    "        return;\n",
    "        case Two:\n",
    "        return;\n",
    "        default:\n",
    "        break;\n",
    "        }\n",
    "    }\n",
    "}"
);

#[test]
fn test_formatting_indent_switchstatements_default() {
    let (mut ws, root) = setup();
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", SWITCH_SOURCE);
    let edits = formatting(&mut ws, &uri, options(4, true));
    let expected_text = concat!(
        "package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "    private enum Numbers {\n",
        "        One, Two\n",
        "    };\n",
        "\n",
        "    public void foo() {\n",
        "        Numbers n = Numbers.One;\n",
        "        switch (n) {\n",
        "            case One:\n",
        "                return;\n",
        "            case Two:\n",
        "                return;\n",
        "            default:\n",
        "                break;\n",
        "        }\n",
        "    }\n",
        "}"
    );
    assert_eq!(expected_text, apply_edits(SWITCH_SOURCE, &edits));
}

#[test]
fn test_formatting_indent_switchstatements_false() {
    let (mut ws, root) = setup();
    set_project_option(&root, FORMATTER_INDENT_SWITCHSTATEMENTS_COMPARE_TO_SWITCH, "false");
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", SWITCH_SOURCE);
    let edits = formatting(&mut ws, &uri, options(4, true));
    let expected_text = concat!(
        "package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "    private enum Numbers {\n",
        "        One, Two\n",
        "    };\n",
        "\n",
        "    public void foo() {\n",
        "        Numbers n = Numbers.One;\n",
        "        switch (n) {\n",
        "        case One:\n",
        "            return;\n",
        "        case Two:\n",
        "            return;\n",
        "        default:\n",
        "            break;\n",
        "        }\n",
        "    }\n",
        "}"
    );
    assert_eq!(expected_text, apply_edits(SWITCH_SOURCE, &edits));
}

#[test]
fn test_range_formatting() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "      public class Baz {\n", "\tvoid foo(){\n", "    }\n", "\t}\n");
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    // range around foo(), ident == 3 spaces
    let edits = range_formatting(&mut ws, &uri, options(3, true), range(2, 0, 3, 5));
    let expected_text = concat!("package org.sample;\n", "      public class Baz {\n", "         void foo() {\n", "         }\n", "\t}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test]
fn test_document_formatting_with_custom_option() {
    let (mut ws, root) = setup();
    let text = concat!(
        "@Deprecated package org.sample;\n\n",
        "public class Baz {\n",
        "    /**Java doc @param a some parameter*/\n",
        "\tvoid foo(int a){;;\n",
        "}\n",
        "}\n"
    );
    set_pref(&mut ws, &["format", "comments", "enabled"], json!(false));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let options = json!({
        "tabSize": 2,
        "insertSpaces": true,
        "org.eclipse.jdt.core.formatter.blank_lines_before_package": 2,
        "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_package": "do not insert",
        "org.eclipse.jdt.core.formatter.put_empty_statement_on_new_line": true
    });
    let edits = formatting(&mut ws, &uri, options);
    let expected_text = concat!(
        "\n",
        "\n",
        "@Deprecated package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "  /**Java doc @param a some parameter*/\n",
        "  void foo(int a) {\n",
        "    ;\n",
        "    ;\n",
        "  }\n",
        "}\n"
    );
    assert_eq!(expected_text, apply_edits(text, &edits));
}

const GOOGLE_TEXT: &str = concat!("package org.sample;\n\n", "public class Baz {\n", "  String name;\n", "}\n");

#[test]
fn test_google_formatter() {
    let (mut ws, root) = setup();
    let url = Url::from_file_path(formatter_resource("eclipse-java-google-style.xml")).unwrap().to_string();
    set_pref(&mut ws, &["format", "settings", "url"], json!(url));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", GOOGLE_TEXT);
    let edits = formatting(&mut ws, &uri, options(2, true)); // ident == 2 spaces
    assert_eq!(GOOGLE_TEXT, apply_edits(GOOGLE_TEXT, &edits));
}

#[test]
fn test_google_formatter_file_path() {
    let (mut ws, root) = setup();
    let path = formatter_resource("eclipse-java-google-style.xml").to_string_lossy().into_owned();
    set_pref(&mut ws, &["format", "settings", "url"], json!(path));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", GOOGLE_TEXT);
    let edits = formatting(&mut ws, &uri, options(2, true)); // ident == 2 spaces
    assert_eq!(GOOGLE_TEXT, apply_edits(GOOGLE_TEXT, &edits));
}

/// `assertTrue(preferences.getFormatterAsURI().isAbsolute())` is observed
/// as the profile of `formatter/test.xml` (`brace_position_for_block=next_line`)
/// being in effect, which requires the path to resolve to an absolute URI.
#[test]
fn test_file_path() {
    let (mut ws, _root) = setup();
    let file = fixtures_dir().join("formatter/test.xml");
    assert!(file.exists());
    ws.client();
    configure_formatter(&mut ws, Some(&file.to_string_lossy()), None);
    assert_eq!(json!("next_line"), java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    configure_formatter(&mut ws, None, None);
}

/// See `test_file_path` for how `getFormatterAsURI().isAbsolute()` is observed.
#[test]
fn test_relative_file_path() {
    let (mut ws, _root) = setup();
    install_formatter_fixtures(&mut ws);
    let formatter_url = "../../formatter/test.xml";
    ws.client();
    configure_formatter(&mut ws, Some(formatter_url), None);
    assert_eq!(json!("next_line"), java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    configure_formatter(&mut ws, None, None);
}

#[test] // typing ; should format the current line
fn test_formatting_on_type_semi_column() {
    let (mut ws, root) = setup();
    set_project_option(&root, FORMATTER_TAB_CHAR, "space");
    let text = concat!("package org.sample;\n\n", "public class Baz {  \n", "String          name       ;\n", "}\n");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, false), pos(3, 27), ";"); // ident == tab
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {  \n", "\tString name;\n", "}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing new_line should format the current line if previous character doesn't close a block
fn test_formatting_on_type_new_line() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {  \n", "String          name       ;\n", "}\n");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(3, 28), "\n");
    let expected_text = concat!(
        "package org.sample;\n",
        "\n",
        "    public      class     Baz {  \n", //this part won't be formatted
        "        String name;\n",
        "}\n"
    );
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing } should format the previous block
fn test_formatting_on_type_close_block() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {  \n", "String          name       ;\n", "}  ");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(4, 0), "}");
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "    String name;\n", "}");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing new_line after opening a block should only format the current line
fn test_formatting_on_type_return_after_opening_block() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {  \n", "String          name       ;\n", "}  \n");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(2, 33), "\n");
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "String          name       ;\n", "}  \n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing new_line after closing a block should format the that block
fn test_formatting_on_type_return_after_closed_block() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {  \n", "String          name       ;\n", "}  \n");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(4, 3), "\n");
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "    String name;\n", "}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing new_line after inserting a new line should format the previous block if previous non-whitespace char is }
fn test_formatting_on_type_return_after_empty_line() {
    let (mut ws, root) = setup();
    let text = concat!(
        "package org.sample;\n",
        "\n",
        "    public      class     Baz {  \n",
        "String          name       ;\n",
        "}  \n",
        "   \n"
    );
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(5, 3), "\n");
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "    String name;\n", "}\n", "   \n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // typing new_line after an empty block on a single line should format that block
fn test_formatting_on_type_return_after_empty_block() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {}  \n");
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(2, 34), "\n");
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "}\n");
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test] // https://github.com/redhat-developer/vscode-java/issues/3396
fn test_formatting_on_type_return_midline() {
    let (mut ws, root) = setup();
    let text = concat!(
        "package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "    public String print() {\n",
        "        int a = 1;\n",
        "        return String.format(\"Value: {}\",\n",
        "        a);\n",
        "    }\n",
        "}"
    );
    set_pref(&mut ws, &["format", "onType", "enabled"], json!(true));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(5, 41), "\n");
    let expected_text = concat!(
        "package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "    public String print() {\n",
        "        int a = 1;\n",
        "        return String.format(\"Value: {}\",\n",
        "                a);\n",
        "    }\n",
        "}"
    );
    assert_eq!(expected_text, apply_edits(text, &edits));
}

#[test]
fn test_disable_formatting_on_type() {
    let (mut ws, root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {  \n", "String          name       ;\n", "}\n");
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    //Check it's disabled by default
    let edits = on_type_formatting(&mut ws, &uri, options(4, true), pos(3, 28), "\n");
    assert_eq!(text, apply_edits(text, &edits));
}

#[test]
fn test_update_formatter_version() {
    // see: https://github.com/redhat-developer/vscode-java/issues/1640
    let (mut ws, root) = setup();
    let text = concat!(
        "package org.sample;\n\n",
        "public class Baz {\n",
        "\tpublic void test1() {\n",
        "\t\tObject o = new Object() {};\n",
        "\t}\n",
        "}\n"
    );
    let file = formatter_resource("version13.xml");
    set_pref(&mut ws, &["format", "settings", "url"], json!(file.to_string_lossy()));
    let uri = get_working_copy(&mut ws, &root, "src/org/sample/Baz.java", text);
    let edits = formatting(&mut ws, &uri, options(2, true)); // ident == 2 spaces
    let text_result = concat!(
        "package org.sample;\n\n",
        "public class Baz {\n",
        "  public void test1() {\n",
        "    Object o = new Object() {};\n",
        "  }\n",
        "}\n"
    );
    assert_eq!(text_result, apply_edits(text, &edits));
}

#[test]
fn test_string_formatting_with_updating() {
    let (mut ws, _root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {public void test1() {Object o = new Object() {};}}  \n");
    let options = json!({ "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_anonymous_type_declaration": "do not insert" });
    let formatted_text = string_formatting(&mut ws, text, options, 13);
    let expected_text = concat!(
        "package org.sample;\n",
        "\n",
        "public class Baz {\n",
        "\tpublic void test1() {\n",
        "\t\tObject o = new Object() {};\n",
        "\t}\n",
        "}\n"
    );
    assert_eq!(formatted_text, expected_text);
}

#[test]
fn test_string_formatting() {
    let (mut ws, _root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {}  \n");
    let options = json!({ "org.eclipse.jdt.core.formatter.blank_lines_after_package": "3" });
    let formatted_text = string_formatting(&mut ws, text, options, CURRENT_VERSION);
    let expected_text = concat!("package org.sample;\n", "\n", "\n", "\n", "public class Baz {\n", "}\n");
    assert_eq!(formatted_text, expected_text);
}

#[test]
fn test_string_formatting_with_default_settings() {
    let (mut ws, _root) = setup();
    let text = concat!("package org.sample;\n", "\n", "    public      class     Baz {}  \n");
    let formatted_text = string_formatting(&mut ws, text, Value::Null, CURRENT_VERSION);
    let expected_text = concat!("package org.sample;\n", "\n", "public class Baz {\n", "}\n");
    assert_eq!(formatted_text, expected_text);
}

#[test]
fn test_default_formatter_preferences() {
    let (mut ws, _root) = setup();
    assert_eq!(json!("false"), java_core_option(&mut ws, FORMATTER_JOIN_WRAPPED_LINES));
    assert_eq!(json!("false"), java_core_option(&mut ws, FORMATTER_JOIN_LINES_IN_COMMENTS));
    assert_eq!(json!("true"), java_core_option(&mut ws, FORMATTER_INDENT_SWITCHSTATEMENTS_COMPARE_TO_SWITCH));
    assert_eq!(json!("true"), java_core_option(&mut ws, FORMATTER_USE_ON_OFF_TAGS));
}

#[test]
fn test_join_wrapped_lines() {
    let (mut ws, _root) = setup();
    let text = concat!(
        "/**\n",
        " * line 1\n",
        " * line 2\n",
        " */\n",
        "public enum X {\n",
        "       ONE,\n",
        "       TWO,\n",
        "       THREE;\n",
        "}\n"
    );
    let formatted_text = string_formatting(&mut ws, text, Value::Null, CURRENT_VERSION);
    let expected_text = concat!("/**\n", " * line 1\n", " * line 2\n", " */\n", "public enum X {\n", "\tONE,\n", "\tTWO,\n", "\tTHREE;\n", "}\n");
    assert_eq!(formatted_text, expected_text);
    let mut options = combined_default_formatter_settings();
    options.insert(FORMATTER_JOIN_WRAPPED_LINES.to_owned(), json!("true"));
    options.insert(FORMATTER_JOIN_LINES_IN_COMMENTS.to_owned(), json!("true"));
    let formatted_text = string_formatting(&mut ws, text, Value::Object(options), CURRENT_VERSION);
    let expected_text = concat!("/**\n", " * line 1 line 2\n", " */\n", "public enum X {\n", "\tONE, TWO, THREE;\n", "}\n");
    assert_eq!(formatted_text, expected_text);
}

#[test]
fn test_indent_switch_statements() {
    let (mut ws, _root) = setup();
    let text = concat!(
        "public class Hello {\n",
        "    public static void main(String[] args) {\n",
        "        switch (args.length) {\n",
        "        case 0:\n",
        "            System.err.println(\"none\");\n",
        "            break;\n",
        "        case 1:\n",
        "            System.err.println(\"one\");\n",
        "            break;\n",
        "        default:\n",
        "            System.err.println(\"many\");\n",
        "        }\n",
        "    }\n",
        "}\n"
    );
    let formatted_text = string_formatting(&mut ws, text, Value::Null, CURRENT_VERSION);
    let expected_text = concat!(
        "public class Hello {\n",
        "\tpublic static void main(String[] args) {\n",
        "\t\tswitch (args.length) {\n",
        "\t\t\tcase 0:\n",
        "\t\t\t\tSystem.err.println(\"none\");\n",
        "\t\t\t\tbreak;\n",
        "\t\t\tcase 1:\n",
        "\t\t\t\tSystem.err.println(\"one\");\n",
        "\t\t\t\tbreak;\n",
        "\t\t\tdefault:\n",
        "\t\t\t\tSystem.err.println(\"many\");\n",
        "\t\t}\n",
        "\t}\n",
        "}\n"
    );
    assert_eq!(formatted_text, expected_text);
}

#[test]
fn test_profile_settings() {
    let (mut ws, _root) = setup();
    install_formatter_fixtures(&mut ws);
    let end_of_line = json!("end_of_line");
    let next_line = json!("next_line");
    assert_eq!(end_of_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    let formatter_url = "../../formatter/test.xml";
    // valid profile
    configure_formatter(&mut ws, Some(formatter_url), Some("GoogleStyle"));
    assert_eq!(next_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    // reset
    configure_formatter(&mut ws, None, None);
    assert_eq!(end_of_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    // invalid profile
    configure_formatter(&mut ws, Some(formatter_url), Some("Invalid"));
    assert_eq!(end_of_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    // empty profile (valid)
    configure_formatter(&mut ws, Some(formatter_url), Some(""));
    assert_eq!(next_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
    configure_formatter(&mut ws, None, None);
    assert_eq!(end_of_line, java_core_option(&mut ws, FORMATTER_BRACE_POSITION_FOR_BLOCK));
}

#[test]
fn test_notebook() {
    let text = "import java.math.  BigDecimal;\n\nBigDecimal   n1 = new\n   BigDecimal(\"0\");\nSystem.  out.  println(n1);\n";
    let edits = format_java_code(text, options(4, true), None); // ident == 4 spaces
    assert_eq!(1, edits.len());
    let new_text = "import java.math.BigDecimal;\n\nBigDecimal n1 = new BigDecimal(\"0\");\nSystem.out.println(n1);\n";
    assert_eq!(new_text, edits[0]["newText"]);
}

#[test]
fn test_notebook2() {
    let text = concat!(
        "import static smile.plot.vega.Predicate.*;\n",
        "\n",
        "var bar = new View(\"2D Histogram Heatmap\").width(300).height(200);\n",
        "    bar.mark(\"rect\");\n",
        "bar.viewConfig()\n",
        "        .stroke(\"transparent\");\n",
        "bar.data().values(new DatasetLoader().loadAsJson(\"movies\").toPrettyString());\n",
        "bar.encode(\"x\", \"IMDB Rating\")\n",
        "            .type(\"quantitative\")\n",
        "        .title(\"IMDB Rating\")\n",
        "            .bin(new BinParams().maxBins(60));\n",
        "bar.encode(\"y\", \"Rotten Tomatoes Rating\")\n",
        "        .type(\"quantitative\")\n",
        "        .bin(new BinParams().maxBins(40));\n",
        "bar.encode(\"color\", null)\n",
        "        .type(\"quantitative\")\n",
        "        .aggregate(\"count\");\n",
        "bar.transform()\n",
        "        .filter(and(valid(\"IMDB Rating\"), valid(\"Rotten Tomatoes Rating\")));\n",
        "\n",
        "display(bar.toPrettyString(), \"application/vnd.vegalite.v5+json\");\n",
        "\n"
    );
    let edits = format_java_code(text, options(4, true), None); // ident == 4 spaces
    assert_eq!(1, edits.len());
    let new_text = concat!(
        "import static smile.plot.vega.Predicate.*;\n",
        "\n",
        "var bar = new View(\"2D Histogram Heatmap\").width(300).height(200);\n",
        "bar.mark(\"rect\");\n",
        "bar.viewConfig()\n",
        "        .stroke(\"transparent\");\n",
        "bar.data().values(new DatasetLoader().loadAsJson(\"movies\").toPrettyString());\n",
        "bar.encode(\"x\", \"IMDB Rating\")\n",
        "        .type(\"quantitative\")\n",
        "        .title(\"IMDB Rating\")\n",
        "        .bin(new BinParams().maxBins(60));\n",
        "bar.encode(\"y\", \"Rotten Tomatoes Rating\")\n",
        "        .type(\"quantitative\")\n",
        "        .bin(new BinParams().maxBins(40));\n",
        "bar.encode(\"color\", null)\n",
        "        .type(\"quantitative\")\n",
        "        .aggregate(\"count\");\n",
        "bar.transform()\n",
        "        .filter(and(valid(\"IMDB Rating\"), valid(\"Rotten Tomatoes Rating\")));\n",
        "\n",
        "display(bar.toPrettyString(), \"application/vnd.vegalite.v5+json\");\n"
    );
    assert_eq!(new_text, edits[0]["newText"]);
}

#[test]
fn test_notebook_range() {
    let text = concat!(
        "import java.math.BigDecimal;\n",
        "\n",
        "BigDecimal n1\n",
        "= new BigDecimal(\"0\");\n",
        "System.out.println(n1);\n",
        "\n",
        "System.out.  println();\n",
        "\n"
    );
    let edits = format_java_code(text, options(4, true), Some(range(2, 0, 4, 23))); // ident == 4 spaces
    assert_eq!(1, edits.len());
    let new_text = concat!(
        "import java.math.BigDecimal;\n",
        "\n",
        "BigDecimal n1 = new BigDecimal(\"0\");\n",
        "System.out.println(n1);\n",
        "\n",
        "System.out.  println();\n",
        "\n"
    );
    assert_eq!(new_text, edits[0]["newText"]);
}

#[test]
fn test_notebook_class() {
    let text = concat!(
        "import java.math.BigDecimal;\n",
        "\n",
        "public class Test {\n",
        "\tpublic static void main(String[] args) {\n",
        "\t\tBigDecimal n1 =\n",
        "\t\tnew BigDecimal(\"0\");\n",
        "\t\tSystem.out  .println(n1);\n",
        "\t}\n",
        "}\n"
    );
    let edits = format_java_code(text, options(4, true), None); // ident == 4 spaces
    assert_eq!(1, edits.len());
    let new_text = concat!(
        "import java.math.BigDecimal;\n",
        "\n",
        "public class Test {\n",
        "    public static void main(String[] args) {\n",
        "        BigDecimal n1 = new BigDecimal(\"0\");\n",
        "        System.out.println(n1);\n",
        "    }\n",
        "}\n"
    );
    assert_eq!(new_text, edits[0]["newText"]);
}

#[test]
fn test_notebook_malformed() {
    let text = concat!("void test() {\n", "System.out.  println(\"test\");\n", "\n", "test();\n");
    let edits = format_java_code(text, options(4, true), None); // ident == 4 spaces
    assert_eq!(1, edits.len());
    let new_text = concat!("\n    void test() {\n", "        System.out.println(\"test\");\n", "\n", "        test();\n");
    assert_eq!(new_text, edits[0]["newText"]);
}
