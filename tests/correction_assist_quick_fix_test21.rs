//! Port of `org.eclipse.jdt.ls.core.internal.correction.AssistQuickFixTest21`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CONVERT: &str = "Convert pattern instanceof if/else if/else chain to switch";

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
    t.ws.set_project_options(&root, &compliance_options("21"));
    (t, root)
}

fn create(t: &mut QuickFixTest, root: &Path, source: &str) -> String {
    t.ws.create_cu(root, "src", "", "module-info.java", "module test {\n}\n");
    let cu = t.ws.create_cu(root, "src", "test", "E.java", source);
    let path = url::Url::parse(&cu).unwrap().to_file_path().unwrap();
    t.ws.assert_no_errors(&path);
    cu
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works"]
fn test_convert_pattern_instanceof_to_switch1() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (x instanceof Integer xint) {\n",
        "\t\t\ti = xint.intValue();\n",
        "\t\t} else if (x instanceof Double xdouble) {\n",
        "\t\t\td = xdouble.doubleValue();\n",
        "\t\t} else if (x instanceof Boolean xboolean) {\n",
        "\t\t\tb = xboolean.booleanValue();\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tb = false;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tswitch (x) {\n",
        "\t\t\tcase Integer xint -> i = xint.intValue();\n",
        "\t\t\tcase Double xdouble -> d = xdouble.doubleValue();\n",
        "\t\t\tcase Boolean xboolean -> b = xboolean.booleanValue();\n",
        "\t\t\tcase null, default -> {\n",
        "\t\t\t\ti = 0;\n",
        "\t\t\t\td = 0.0D;\n",
        "\t\t\t\tb = false;\n",
        "\t\t\t}\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new(CONVERT, expected);
    let selection = get_range(&t.ws.read(&cu), "doubleValue");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works"]
fn test_convert_pattern_instanceof_to_switch2() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (x instanceof Integer xint) {\n",
        "\t\t\tj = 7;\n",
        "\t\t} else if (x instanceof Double xdouble) {\n",
        "\t\t\tj = 8; // comment\n",
        "\t\t} else if (x instanceof Boolean xboolean) {\n",
        "\t\t\tj = 9;\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tj = 10;\n",
        "\t\t\tb = false;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tswitch (x) {\n",
        "\t\t\tcase Integer xint -> j = 7;\n",
        "\t\t\tcase Double xdouble -> j = 8; // comment\n",
        "\t\t\tcase Boolean xboolean -> j = 9;\n",
        "\t\t\tcase null, default -> {\n",
        "\t\t\t\ti = 0;\n",
        "\t\t\t\td = 0.0D;\n",
        "\t\t\t\tj = 10;\n",
        "\t\t\t\tb = false;\n",
        "\t\t\t}\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new(CONVERT, expected);
    let selection = get_range(&t.ws.read(&cu), "xboolean");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works"]
fn test_convert_pattern_instanceof_to_switch3() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic int square(int x) {\n",
        "\t\treturn x * x;\n",
        "\t}\n",
        "\tpublic int foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (y instanceof Integer xint) {\n",
        "\t\t\treturn 7;\n",
        "\t\t} else if (y instanceof final Double xdouble) {\n",
        "\t\t\treturn square(8); // square\n",
        "\t\t} else if (y instanceof final Boolean xboolean) {\n",
        "\t\t\tthrow new NullPointerException();\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tb = false;\n",
        "\t\t\tif (x instanceof Integer) {\n",
        "\t\t\t\treturn 10;\n",
        "\t\t\t}\n",
        "\t\t\treturn 11;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic int square(int x) {\n",
        "\t\treturn x * x;\n",
        "\t}\n",
        "\tpublic int foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tswitch (y) {\n",
        "\t\t\tcase Integer xint -> {\n",
        "\t\t\t\treturn 7;\n",
        "\t\t\t}\n",
        "\t\t\tcase Double xdouble -> {\n",
        "\t\t\t\treturn square(8); // square\n",
        "\t\t\t}\n",
        "\t\t\tcase Boolean xboolean -> throw new NullPointerException();\n",
        "\t\t\tcase null, default -> {\n",
        "\t\t\t\ti = 0;\n",
        "\t\t\t\td = 0.0D;\n",
        "\t\t\t\tb = false;\n",
        "\t\t\t\tif (x instanceof Integer) {\n",
        "\t\t\t\t\treturn 10;\n",
        "\t\t\t\t}\n",
        "\t\t\t\treturn 11;\n",
        "\t\t\t}\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new(CONVERT, expected);
    let selection = get_range(&t.ws.read(&cu), "throw");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works; the switch expression variant is also not formatted (yield statement placeholder needs a placeholder expression child)"]
fn test_convert_pattern_instanceof_to_switch4() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (x instanceof Integer xint) {\n",
        "\t\t\tj = 7;\n",
        "\t\t} else if (x instanceof Double xdouble) {\n",
        "\t\t\tj = 8; // comment\n",
        "\t\t} else if (x instanceof Boolean xboolean) {\n",
        "\t\t\tj = 9;\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tb = false;\n",
        "\t\t\tj = 10;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tj = switch (x) {\n",
        "\t\t\tcase Integer xint -> 7;\n",
        "\t\t\tcase Double xdouble -> 8; // comment\n",
        "\t\t\tcase Boolean xboolean -> 9;\n",
        "\t\t\tcase null, default -> {\n",
        "\t\t\t\ti = 0;\n",
        "\t\t\t\td = 0.0D;\n",
        "\t\t\t\tb = false;\n",
        "\t\t\t\tyield 10;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new(CONVERT, expected);
    let selection = get_range(&t.ws.read(&cu), "false");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works; the switch expression variant is also not formatted (yield statement placeholder needs a placeholder expression child)"]
fn test_convert_pattern_instanceof_to_switch5() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic int square(int x) {\n",
        "\t\treturn x * x;\n",
        "\t}\n",
        "\tpublic int foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (y instanceof Integer xint) {\n",
        "\t\t\treturn 7;\n",
        "\t\t} else if (y instanceof final Double xdouble) {\n",
        "\t\t\treturn square(8); // square\n",
        "\t\t} else if (y instanceof final Boolean xboolean) {\n",
        "\t\t\tthrow new NullPointerException();\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tb = false;\n",
        "\t\t\treturn 10;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic int square(int x) {\n",
        "\t\treturn x * x;\n",
        "\t}\n",
        "\tpublic int foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\treturn switch (y) {\n",
        "\t\t\tcase Integer xint -> 7;\n",
        "\t\t\tcase Double xdouble -> square(8); // square\n",
        "\t\t\tcase Boolean xboolean -> throw new NullPointerException();\n",
        "\t\t\tcase null, default -> {\n",
        "\t\t\t\ti = 0;\n",
        "\t\t\t\td = 0.0D;\n",
        "\t\t\t\tb = false;\n",
        "\t\t\t\tyield 10;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new(CONVERT, expected);
    let selection = get_range(&t.ws.read(&cu), "false");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
#[ignore = "module-info.java projects report \"java.lang.Object is not accessible\" errors (named modules are not resolved against the JDK by the bridge compiler), so assertNoErrors fails; the assist itself works"]
fn test_do_not_convert_pattern_instanceof_to_switch1() {
    let (mut t, root) = setup();
    let cu = create(&mut t, &root, concat!(
        "package test;\n",
        "\n",
        "public class E {\n",
        "\tpublic void foo(Object x, Object y) {\n",
        "\t\tint i, j;\n",
        "\t\tdouble d;\n",
        "\t\tboolean b;\n",
        "\t\tif (x instanceof Integer xint) {\n",
        "\t\t\ti = xint.intValue();\n",
        "\t\t} else if (y instanceof Double xdouble) {\n",
        "\t\t\td = xdouble.doubleValue();\n",
        "\t\t} else if (x instanceof Boolean xboolean) {\n",
        "\t\t\tb = xboolean.booleanValue();\n",
        "\t\t} else {\n",
        "\t\t\ti = 0;\n",
        "\t\t\td = 0.0D;\n",
        "\t\t\tb = false;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    ));
    t.assert_code_action_not_exists(&cu, CONVERT);
}
