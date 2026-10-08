//! Port of `org.eclipse.jdt.ls.core.internal.correction.StringConcatenationQuickFixTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn compliance_options(version: &str) -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), version.to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    options
}

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    // The upstream test preferences indent with tabs.
    t.ws.settings["java"]["format"] = serde_json::json!({ "insertSpaces": false });
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    t.ws.set_project_options(&root, &compliance_options("17"));
    (t, root)
}

const CONTENT: &str = concat!(
    "package test;\n",
    "public class Test {\n",
    "\tprivate void print(String name, int age) {\n",
    "\t  String value = \"User name: \" + name + \", age: \" + age;\n",
    "\t}\n",
    "}\n",
);

const STRING_BUILDER_EXPECTED: &str = concat!(
    "package test;\n",
    "public class Test {\n",
    "\tprivate void print(String name, int age) {\n",
    "\t  StringBuilder stringBuilder = new StringBuilder();\n",
    "\t\tstringBuilder.append(\"User name: \");\n",
    "\t\tstringBuilder.append(name);\n",
    "\t\tstringBuilder.append(\", age: \");\n",
    "\t\tstringBuilder.append(age);\n",
    "\t  String value = stringBuilder.toString();\n",
    "\t}\n",
    "}\n",
);

#[test]
fn test_convert_to_string_format() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let expected = concat!(
        "package test;\n",
        "public class Test {\n",
        "\tprivate void print(String name, int age) {\n",
        "\t  String value = String.format(\"User name: %s, age: %d\", name, age);\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Use 'String.format' for string concatenation", expected);
    let selection = get_range(&t.ws.read(&cu), "User name:");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_string_builder() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let e = Expected::new("Use 'StringBuilder' for string concatenation", STRING_BUILDER_EXPECTED);
    let selection = get_range(&t.ws.read(&cu), "User name:");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_message_format() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let expected = concat!(
        "package test;\n",
        "\n",
        "import java.text.MessageFormat;\n",
        "\n",
        "public class Test {\n",
        "\tprivate void print(String name, int age) {\n",
        "\t  String value = MessageFormat.format(\"User name: {0}, age: {1}\", name, age);\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Use 'MessageFormat' for string concatenation", expected);
    let selection = get_range(&t.ws.read(&cu), "User name:");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_string_buffer() {
    let (mut t, root) = setup();
    t.ws.set_project_options(&root, &compliance_options("1.8"));
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let e = Expected::new("Use 'StringBuilder' for string concatenation", STRING_BUILDER_EXPECTED);
    let selection = get_range(&t.ws.read(&cu), "User name:");
    t.assert_code_actions_range(&cu, selection, &[e]);
}
