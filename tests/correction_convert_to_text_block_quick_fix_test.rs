//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConvertToTextBlockQuickFixTest`.

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
    "  String html = \"<html>\\n\"\n",
    "      + \"    <body>\\n\"\n",
    "      + \"        <span>example text</span>\\n\"\n",
    "      + \"    </body>\\n\"\n",
    "      + \"</html>\";\n",
    "}\n",
);

#[test]
fn test_convert_to_text_block() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let expected = concat!(
        "package test;\n",
        "public class Test {\n",
        "  String html = \"\"\"\n",
        "\t<html>\n",
        "\t    <body>\n",
        "\t        <span>example text</span>\n",
        "\t    </body>\n",
        "\t</html>\"\"\";\n",
        "}\n",
    );
    let e = Expected::new("Convert String concatenation to Text Block", expected);
    let selection = get_range(&t.ws.read(&cu), "<body>");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_no_convert_to_text_block() {
    let (mut t, root) = setup();
    t.ws.set_project_options(&root, &compliance_options("11"));
    let cu = t.ws.create_cu(&root, "src", "test", "Test.java", CONTENT);
    let selection = get_range(&t.ws.read(&cu), "<body>");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert String concatenation to Text Block");
}
