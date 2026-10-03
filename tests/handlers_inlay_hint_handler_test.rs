//! Port of `org.eclipse.jdt.ls.core.internal.handlers.InlayHintHandlerTest`.
//!
//! `preferences.setInlayHints*` become `java.inlayHints.*` settings sent with
//! `initialize`; `getWorkingCopy(path, source)` opens `path` (relative to the
//! current test project) with `source` as its content.

mod common;
use common::jdtls::{range, Workspace};
use serde_json::{json, Value};

/// `@BeforeEach initPreferences`: `setInlayHintsSuppressedWhenSameNameNumberedParameter(true)`.
fn init_preferences(ws: &mut Workspace) {
    set_preference(ws, "parameterNames", "suppressWhenSameNameNumbered", json!(true));
}

/// `preferences.setInlayHints...(value)`: `java.inlayHints.<group>.<key>`.
fn set_preference(ws: &mut Workspace, group: &str, key: &str, value: Value) {
    ws.settings["java"]["inlayHints"][group][key] = value;
}

fn new_workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.settings = json!({ "java": { "inlayHints": {} } });
    init_preferences(&mut ws);
    ws
}

/// `AbstractCompilationUnitBasedTest.getWorkingCopy(path, source)`.
fn get_working_copy(ws: &mut Workspace, project: &str, path: &str, source: &str) -> String {
    let file = ws.project_root(project).join(path);
    let uri = tower_lsp::lsp_types::Url::from_file_path(file).unwrap().to_string();
    ws.open_with(&uri, source);
    uri
}

/// `new InlayHintsHandler(preferenceManager).inlayHint(params, monitor)`.
fn inlay_hint(ws: &mut Workspace, uri: &str, range: Value) -> Vec<Value> {
    let result = ws.request("textDocument/inlayHint", json!({ "textDocument": { "uri": uri }, "range": range }));
    result.as_array().cloned().unwrap_or_else(|| panic!("inlay hints: {result}"))
}

/// `hint.getLabel().getLeft()`.
fn label(hint: &Value) -> &str {
    hint["label"].as_str().unwrap_or_else(|| panic!("label of {hint}"))
}

#[test]
fn test_none_mode() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("none"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(int i) {}\n\tvoid bar() {\n\t\tfoo(123);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert!(inlay_hints.is_empty());
}

#[test]
fn test_out_of_range() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(int i) {}\n\tvoid bar() {\n\t\tfoo(123);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 1, 0));
    assert!(inlay_hints.is_empty());
}

#[test]
fn test_boolean_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(bool b) {}\n\tvoid bar() {\n\t\tfoo(true);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("b:", label(&inlay_hints[0]));
}

#[test]
fn test_character_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(char c) {}\n\tvoid bar() {\n\t\tfoo('c');\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("c:", label(&inlay_hints[0]));
}

#[test]
fn test_null_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(char c) {}\n\tvoid bar() {\n\t\tfoo(null);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("c:", label(&inlay_hints[0]));
}

#[test]
fn test_number_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(int i) {}\n\tvoid bar() {\n\t\tfoo(123);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("i:", label(&inlay_hints[0]));
}

#[test]
fn test_string_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(String s) {}\n\tvoid bar() {\n\t\tfoo(\"s\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("s:", label(&inlay_hints[0]));
}

#[test]
fn test_type_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(Class<T> clazz) {}\n\tvoid bar() {\n\t\tfoo(Foo.class);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("clazz:", label(&inlay_hints[0]));
}

#[test]
fn test_no_inlay_hint_for_non_literal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("literals"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(Double d) {}\n\tvoid bar() {\n\t\tDouble d = 0.0;\n\t\tfoo(d);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert!(inlay_hints.is_empty());
}

#[test]
fn test_all_mode() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(Double doubleParam) {}\n\tvoid foo(Integer intParam) {}\n\tvoid bar() {\n\t\tDouble d = 0.0;\n\t\tInteger i = 0;\n\t\tfoo(d);\n\t\tfoo(i);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 9, 0));
    assert_eq!(2, inlay_hints.len());
    assert_eq!("doubleParam:", label(&inlay_hints[0]));
    assert_eq!("intParam:", label(&inlay_hints[1]));
}

#[test]
fn test_varargs() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(String... args) {}\n\tvoid bar() {\n\t\tfoo(\"1\", \"2\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("...args:", label(&inlay_hints[0]));
}

#[test]
fn test_varargs2() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo2(Integer i, String... args) {}\n\tvoid bar() {\n\t\tfoo2(1);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("i:", label(&inlay_hints[0]));
}

#[test]
fn test_no_inlay_hints_when_names_are_equal() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(Double d) {}\n\tvoid bar() {\n\t\tDouble d = 0.0;\n\t\tfoo(d);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert!(inlay_hints.is_empty());
}

#[test]
fn test_no_inlay_hints_for_cast_expression() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(String str) {}\n\tvoid bar() {\n\t\tString str = \"\";\n\t\tfoo((CharSequence) str);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert!(inlay_hints.is_empty());
}

#[test]
fn test_new_expression() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tFoo(String foo) {}\n\tvoid bar() {\n\t\tFoo foo = new Foo(\"foo\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("foo:", label(&inlay_hints[0]));
}

#[test]
fn test_enum_constant() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public enum Foo {\n\tI(\"i\"), J(\"j\");\n\tString id;\n\tFoo(String id) {\n\t\tthis.id = id;\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(2, inlay_hints.len());
    assert_eq!("id:", label(&inlay_hints[0]));
    assert_eq!("id:", label(&inlay_hints[1]));
}

#[test]
fn test_record() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java16"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    // javaProject.findElement("foo/bar/Bar.java").becomeWorkingCopy(null)
    let uri = ws.class_uri("java16", "foo.bar.Bar");
    ws.open(&uri);
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 12, 0));
    assert_eq!(6, inlay_hints.len());
    assert_eq!("fromNodeId:", label(&inlay_hints[0]));
    assert_eq!("toNodeId:", label(&inlay_hints[1]));
    assert_eq!("fromPoint:", label(&inlay_hints[2]));
    assert_eq!("toPoint:", label(&inlay_hints[3]));
    assert_eq!("length:", label(&inlay_hints[4]));
    assert_eq!("profile:", label(&inlay_hints[5]));
}

#[test]
fn test_complex_expressions() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid foo(String s) {}\n\tvoid bar(int i) {\n\t\tfoo(switch (i) {\n\t\t\tcase 1:\n\t\t\t\tyield \"foo\";\n\t\t\tdefault:\n\t\t\t\tyield \"unknown\"\n\t\t});\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 10, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("s:", label(&inlay_hints[0]));
    assert_eq!(3, inlay_hints[0]["position"]["line"]);
    assert_eq!(6, inlay_hints[0]["position"]["character"]);
}

#[test]
fn test_constructor_invocation() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tpublic Foo(Integer a) {\n\t\tthis(1, 2);\n\t}\n\tpublic Foo(Integer a, Integer b) {\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(2, inlay_hints.len());
    assert_eq!("a:", label(&inlay_hints[0]));
    assert_eq!(2, inlay_hints[0]["position"]["line"]);
    assert_eq!(7, inlay_hints[0]["position"]["character"]);
    assert_eq!("b:", label(&inlay_hints[1]));
    assert_eq!(2, inlay_hints[1]["position"]["line"]);
    assert_eq!(10, inlay_hints[1]["position"]["character"]);
}

#[test]
fn test_super_constructor_invocation() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tpublic Foo(Integer a, Integer b) {}\n}\nclass Bar extends Foo {\n\tpublic Bar() {\n\t\tsuper(1, 2);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 7, 0));
    assert_eq!(2, inlay_hints.len());
    assert_eq!("a:", label(&inlay_hints[0]));
    assert_eq!(5, inlay_hints[0]["position"]["line"]);
    assert_eq!(8, inlay_hints[0]["position"]["character"]);
    assert_eq!("b:", label(&inlay_hints[1]));
    assert_eq!(5, inlay_hints[1]["position"]["line"]);
    assert_eq!(11, inlay_hints[1]["position"]["character"]);
}

#[test]
fn test_super_method_invocation() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tpublic void foo(Object obj){}\n}\nclass Bar extends Foo {\n\tpublic void bar() {\n\t\tBar.super.foo(1);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 7, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("obj:", label(&inlay_hints[0]));
    assert_eq!(5, inlay_hints[0]["position"]["line"]);
    assert_eq!(16, inlay_hints[0]["position"]["character"]);
}

#[test]
fn test_disabled_lambda_parameter_type_hints() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java11"]);
    set_preference(&mut ws, "parameterTypes", "enabled", json!(false));
    let uri = get_working_copy(&mut ws, "java11", "src/Foo.java", "import java.util.stream.Stream;\npublic class Foo {\n\tvoid bar() {\n\t\tStream.of(2, 3, 5, 7).map(n -> n * 2);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(0, inlay_hints.len(), "{inlay_hints:?}");
}

#[test]
fn test_lambda_parameter_type_hints() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java11"]);
    set_preference(&mut ws, "parameterTypes", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "java11", "src/Foo.java", "import java.util.Map;\nimport java.util.HashMap;\npublic class Foo {\n\tvoid bar() {\n\t\tMap<String, Integer> map = new HashMap<>();\n\t\tmap.forEach((key, value) -> System.out.println(key + value));\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 8, 0));
    assert_eq!(2, inlay_hints.len(), "{inlay_hints:?}");
    assert_eq!("String", label(&inlay_hints[0]));
    assert_eq!("Integer", label(&inlay_hints[1]));
}

#[test]
fn test_lambda_explicit_type_no_hints() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java11"]);
    set_preference(&mut ws, "parameterTypes", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "java11", "src/Foo.java", "import java.util.stream.Stream;\npublic class Foo {\n\tvoid bar() {\n\t\t// Explicit type - should not show inlay hint\n\t\tStream.of(2, 3, 5, 7).map((Integer n) -> n * 2);\n\t\t// Implicit type - should show inlay hint\n\t\tStream.of(2, 3, 5, 7).map(x -> x * 2);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 9, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("Integer", label(&inlay_hints[0]));
    assert_eq!(6, inlay_hints[0]["position"]["line"]);
}

#[test]
fn test_variable_type_hints() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java11"]);
    set_preference(&mut ws, "variableTypes", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "java11", "src/Foo.java", "public class Foo {\n\n\t@SuppressWarnings(\"unused\")\n    public void varImplicitTypes() {\n        //Should show var inlayhints\n        var _greeting= getGreeting();\n        var _string = \"foo\" + \"1\";\n\n        //Should NOT show var inlayhints\n        var _int = 1;\n        var _boolean = true;\n        var _double = 1.0;\n        var _float = 1.0f;\n        var _char = 'a';\n        var _long = 1L;\n        var _object = new Object();\n        var _exception= new RuntimeException(\"Foo\");\n        var _castString= (String) getGreeting();\n        var _byte= (byte) 1;\n        var _short = (short) 1;\n        var _array = new int[1];\n        var _arrayInit = new int[] {1, 2, 3};\n\t}\n\n    private String getGreeting() {\n        return \"Hello\";\n    }\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 28, 0));
    assert_eq!(2, inlay_hints.len());
    let mut found_greeting = false;
    let mut found_string = false;
    for hint in &inlay_hints {
        let label = label(hint);
        let line = &hint["position"]["line"];
        if label.contains(": String") {
            // _greeting or _string
            if *line == 5 {
                found_greeting = true; // var _greeting= getGreeting();
            }
            if *line == 6 {
                found_string = true; // var _string = "foo" + "1";
            }
        }
    }
    assert!(found_greeting, "Should find inlay hint for _greeting");
    assert!(found_string, "Should find inlay hint for _string");
}

#[test]
fn test_same_prefix_parameter_filter() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "import java.util.stream.Stream;\nclass Foo {\n\tpublic static void process(String s1, String s2, String s3) {}\n\tpublic static void main(String[] args) {\n\t\tprint(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert!(inlay_hints.is_empty(), "Should not show inlay hints for methods with same prefix+number parameters");
}

#[test]
fn test_same_prefix_parameter_filter_longer_prefix() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid process(String param1, String param2, String param3) {}\n\tvoid bar() {\n\t\tprocess(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert!(inlay_hints.is_empty(), "Should not show inlay hints for methods with same prefix+number parameters");
}

#[test]
fn test_same_prefix_parameter_filter_mixed() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid mixedParams(String s1, String description, String s2) {}\n\tvoid descriptiveParams(String name, String description, String value) {}\n\tvoid bar() {\n\t\tmixedParams(\"first\", \"desc\", \"second\");\n\t\tdescriptiveParams(\"name\", \"desc\", \"value\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 8, 0));
    assert_eq!(6, inlay_hints.len());
    assert_eq!("s1:", label(&inlay_hints[0]));
    assert_eq!("description:", label(&inlay_hints[1]));
    assert_eq!("s2:", label(&inlay_hints[2]));
    assert_eq!("name:", label(&inlay_hints[3]));
    assert_eq!("description:", label(&inlay_hints[4]));
    assert_eq!("value:", label(&inlay_hints[5]));
}

#[test]
fn test_same_prefix_parameter_filter_single_param() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid singleParam(String s1) {}\n\tvoid bar() {\n\t\tsingleParam(\"test\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(1, inlay_hints.len());
    assert_eq!("s1:", label(&inlay_hints[0]));
}

#[test]
fn test_same_prefix_parameter_filter_different_prefixes() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid differentPrefixes(String s1, String t1, String u1) {}\n\tvoid bar() {\n\t\tdifferentPrefixes(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(3, inlay_hints.len());
    assert_eq!("s1:", label(&inlay_hints[0]));
    assert_eq!("t1:", label(&inlay_hints[1]));
    assert_eq!("u1:", label(&inlay_hints[2]));
}

#[test]
fn test_same_prefix_parameter_filter_not_starting_with_one() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid startingWithZero(String s0, String s1, String s2) {}\n\tvoid startingWithTwo(String s2, String s3, String s4) {}\n\tvoid bar() {\n\t\tstartingWithZero(\"first\", \"second\", \"third\");\n\t\tstartingWithTwo(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 8, 0));
    assert_eq!(6, inlay_hints.len());
    assert_eq!("s0:", label(&inlay_hints[0]));
    assert_eq!("s1:", label(&inlay_hints[1]));
    assert_eq!("s2:", label(&inlay_hints[2]));
    assert_eq!("s2:", label(&inlay_hints[3]));
    assert_eq!("s3:", label(&inlay_hints[4]));
    assert_eq!("s4:", label(&inlay_hints[5]));
}

#[test]
fn test_same_prefix_parameter_filter_non_consecutive() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid nonConsecutive(String s1, String s3, String s5) {}\n\tvoid bar() {\n\t\tnonConsecutive(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(3, inlay_hints.len());
    assert_eq!("s1:", label(&inlay_hints[0]));
    assert_eq!("s3:", label(&inlay_hints[1]));
    assert_eq!("s5:", label(&inlay_hints[2]));
}

#[test]
fn test_format_specifier_single_arg() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString result = String.format(\"Hello %s\", name);\n\t}\n\tString name = \"World\";\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_multiple_args() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString name = \"Alice\";\n\t\tint age = 30;\n\t\tString result = String.format(\"Hello %s, you are %d years old\", name, age);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(2, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
    assert_eq!(":age", label(&format_hints[1]));
}

#[test]
fn test_format_specifier_static_format_with_locale() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "import java.util.Locale;\npublic class Foo {\n\tvoid bar() {\n\t\tString name = \"Alice\";\n\t\tString result = String.format(Locale.US, \"Hello %s\", name);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_escaped_percent() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString result = \"done\";\n\t\tString s = String.format(\"100%% done: %s\", result);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":result", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_explicit_arg_index() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString a = \"first\";\n\t\tString b = \"second\";\n\t\tString result = String.format(\"%2$s before %1$s\", a, b);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(2, format_hints.len());
    assert_eq!(":b", label(&format_hints[0]));
    assert_eq!(":a", label(&format_hints[1]));
}

#[test]
fn test_format_specifier_newline_does_not_consume_arg() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString value = \"test\";\n\t\tString result = String.format(\"line%n%s\", value);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":value", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_disabled() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(false));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString name = \"World\";\n\t\tString result = String.format(\"Hello %s\", name);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert!(format_hints.is_empty());
}

#[test]
fn test_format_specifier_non_string_format() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tString format(String pattern, Object... args) { return pattern; }\n\tvoid bar() {\n\t\tString result = format(\"Hello %s\", \"World\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert!(format_hints.is_empty());
}

#[test]
fn test_format_specifier_printf_print_stream() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString name = \"World\";\n\t\tSystem.out.printf(\"Hello %s\\n\", name);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_print_stream_format() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tdouble value = 3.14;\n\t\tSystem.out.format(\"Pi is %.2f\\n\", value);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":value", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_formatter_format() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "import java.util.Formatter;\npublic class Foo {\n\tvoid bar() {\n\t\tString name = \"World\";\n\t\tFormatter fmt = new Formatter();\n\t\tfmt.format(\"Hello %s\", name);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 7, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_formatted() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java15"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "java15", "src/main/java/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString name = \"World\";\n\t\tString result = \"Hello %s\".formatted(name);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 5, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(1, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
}

#[test]
fn test_format_specifier_text_block() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello", "eclipse/java15"]);
    set_preference(&mut ws, "formatParameters", "enabled", json!(true));
    let uri = get_working_copy(&mut ws, "java15", "src/main/java/Foo.java", "public class Foo {\n\tvoid bar() {\n\t\tString name = \"World\";\n\t\tint count = 42;\n\t\tString result = \"\"\"\n\t\t\t\t\t    Hello %s, count is %d\n\t\t\t\t\t\"\"\".formatted(name, count);\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 8, 0));
    // Filter to only format specifier hints (those starting with ':')
    let format_hints: Vec<&Value> = inlay_hints.iter().filter(|h| label(h).starts_with(':')).collect();
    assert_eq!(2, format_hints.len());
    assert_eq!(":name", label(&format_hints[0]));
    assert_eq!(":count", label(&format_hints[1]));
}

#[test]
fn test_disable_same_named_numbered_parameter_filter() {
    let mut ws = new_workspace();
    ws.import_projects(&["eclipse/hello"]);
    set_preference(&mut ws, "parameterNames", "enabled", json!("all"));
    set_preference(&mut ws, "parameterNames", "suppressWhenSameNameNumbered", json!(false));
    let uri = get_working_copy(&mut ws, "hello", "src/Foo.java", "public class Foo {\n\tvoid samePrefixParameter(String s1, String s2, String s3) {}\n\tvoid bar() {\n\t\tsamePrefixParameter(\"first\", \"second\", \"third\");\n\t}\n}\n");
    let inlay_hints = inlay_hint(&mut ws, &uri, range(0, 0, 6, 0));
    assert_eq!(3, inlay_hints.len());
    assert_eq!("s1:", label(&inlay_hints[0]));
    assert_eq!("s2:", label(&inlay_hints[1]));
    assert_eq!("s3:", label(&inlay_hints[2]));
}
