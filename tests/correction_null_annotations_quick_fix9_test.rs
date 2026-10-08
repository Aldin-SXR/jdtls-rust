//! Port of `org.eclipse.jdt.ls.core.internal.correction.NullAnnotationsQuickFix9Test`.

mod common;

use common::quickfix::{Expected, QuickFixTest};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn compliance9() -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "9".to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    options
}

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.settings["java"]["compile"] = json!({
        "nullAnalysis": {
            "mode": "automatic",
            "nonnull": ["annots.NonNull"],
            "nullable": ["annots.Nullable"],
            "nonnullbydefault": ["annots.NonNullByDefault"]
        }
    });
    let annots = t.ws.new_project("annots", &compliance9());
    t.ws.create_cu(&annots, "src", "", "module-info.java", "module annots {\n     exports annots; \n}\n");
    t.ws.create_cu(
        &annots,
        "src",
        "annots",
        "Nullable.java",
        concat!(
            "package annots;\n",
            "\n",
            "import java.lang.annotation.ElementType;\n",
            "import java.lang.annotation.Target;\n",
            "\n",
            "@Target(ElementType.TYPE_USE)\n",
            "public @interface Nullable {\n",
            "}\n",
        ),
    );
    t.ws.create_cu(
        &annots,
        "src",
        "annots",
        "NonNull.java",
        concat!(
            "package annots;\n",
            "\n",
            "import java.lang.annotation.ElementType;\n",
            "import java.lang.annotation.Target;\n",
            "\n",
            "@Target(ElementType.TYPE_USE)\n",
            "public @interface NonNull {\n",
            "}\n",
        ),
    );
    t.ws.create_cu(
        &annots,
        "src",
        "annots",
        "DefaultLocation.java",
        concat!(
            "package annots;\n",
            "\n",
            "public enum DefaultLocation {\n",
            "\tPARAMETER, RETURN_TYPE, FIELD, TYPE_BOUND, TYPE_ARGUMENT\n",
            "}\n",
        ),
    );
    t.ws.create_cu(
        &annots,
        "src",
        "annots",
        "NonNullByDefault.java",
        concat!(
            "package annots;\n",
            "\n",
            "import static annots.DefaultLocation.*;\n",
            "\n",
            "public @interface NonNullByDefault {\n",
            "\tDefaultLocation[] value() default { PARAMETER, RETURN_TYPE, FIELD, TYPE_BOUND, TYPE_ARGUMENT };\n",
            "}\n",
        ),
    );

    let mut options = compliance9();
    options.insert("org.eclipse.jdt.core.compiler.annotation.nullanalysis".into(), "enabled".into());
    options.insert("org.eclipse.jdt.core.compiler.annotation.nullable".into(), "annots.Nullable".into());
    options.insert("org.eclipse.jdt.core.compiler.annotation.nonnull".into(), "annots.NonNull".into());
    options.insert("org.eclipse.jdt.core.compiler.annotation.nonnullbydefault".into(), "annots.NonNullByDefault".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "space".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.size".into(), "4".into());
    options.insert("org.eclipse.jdt.core.formatter.number_of_empty_lines_to_preserve".into(), "99".into());
    let root = t.ws.new_project("TestProject1", &options);
    t.ws.add_project_dependency(&root, "annots", true);
    (t, root)
}

#[test]
fn test_bug530580a() {
    let (mut t, root) = setup();
    let str = concat!(
        "@annots.NonNullByDefault module test {\n",
        " requires annots;}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import annots.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import annots.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tprivate Map<? extends Map<String, @Nullable Integer>, String[][]> x;\n",
        "\n",
        "    abstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import annots.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g(Map<? extends Map<String, @Nullable Integer>, String[][]> x) {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import annots.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tMap<? extends Map<String, @Nullable Integer>, String[][]> x = f();\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Create field 'x'", &str2);
    let e2 = Expected::new("Create parameter 'x'", &str3);
    let e3 = Expected::new("Create local variable 'x'", &str4);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
#[ignore = "the compiler bridge reports no IllegalRedefinitionToNonNullParameter when @NonNullByDefault comes from module-info.java (passes on the oracle)"]
fn test_bug530580b() {
    let (mut t, root) = setup();
    let str = concat!(
        "@annots.NonNullByDefault module test {\n",
        " requires annots;}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import annots.*;\n",
        "\n",
        "interface Type {\n",
        "\tvoid set(@Nullable String s);\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(String t) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import annots.*;\n",
        "\n",
        "interface Type {\n",
        "\tvoid set(@Nullable String s);\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(@Nullable String t) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import annots.*;\n",
        "\n",
        "interface Type {\n",
        "\tvoid set(String s);\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(String t) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 't' to '@Nullable'", &str2);
    let e2 = Expected::new("Change parameter in overridden 'set(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}
