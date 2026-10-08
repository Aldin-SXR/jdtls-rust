//! Port of `org.eclipse.jdt.ls.core.internal.correction.NullAnnotationsQuickFix1d8MixTest`.

mod common;

use common::jdtls::{fixtures_dir, test_default_options};
use common::quickfix::{Expected, QuickFixTest};
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.ws.settings["java"]["compile"] = json!({
        "nullAnalysis": {
            "mode": "automatic",
            "nonnull": ["org.eclipse.jdt.annotation.NonNull"],
            "nullable": ["org.eclipse.jdt.annotation.Nullable"],
            "nonnullbydefault": ["org.eclipse.jdt.annotation.NonNullByDefault"]
        }
    });
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "space".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.size".into(), "4".into());
    options.insert("org.eclipse.jdt.core.formatter.number_of_empty_lines_to_preserve".into(), "99".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.staticAccessReceiver".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.uncheckedTypeOperation".into(), "ignore".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.missingHashCodeMethod".into(), "warning".into());
    options.insert("org.eclipse.jdt.core.compiler.annotation.nullanalysis".into(), "enabled".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.nullSpecViolation".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.nullReference".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.potentialNullReference".into(), "warning".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.nullAnnotationInferenceConflict".into(), "warning".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.nullUncheckedConversion".into(), "warning".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.redundantNullCheck".into(), "warning".into());
    let root = t.ws.new_empty_project(&options);
    let jar = fixtures_dir().join("testresources/org.eclipse.jdt.annotation_2.4.100.v20251017-1955.jar");
    t.ws.add_library(&root, &jar);
    (t, root)
}

#[test]
fn test_bug473068_elided() {
    let (mut t, root) = setup();
    let str = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\tpublic void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @NonNull Object data) {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "testNullAnnotations", "Snippet.java", &str);
    let str1 = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\tpublic void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @Nullable Object data) {\n",
        "    }\n",
        "}\n",
    );
    let str2 = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\t@SuppressWarnings(\"null\")\n",
        "    public void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @NonNull Object data) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter of 'updateSelectionData(..)' to '@Nullable'", &str1);
    let e2 = Expected::new("Add @SuppressWarnings 'null' to 'select()'", &str2);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_bug473068_explicit_type() {
    let (mut t, root) = setup();
    let str = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\tpublic void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (Object data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @NonNull Object data) {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "testNullAnnotations", "Snippet.java", &str);
    let str1 = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\tpublic void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (@NonNull Object data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @NonNull Object data) {\n",
        "    }\n",
        "}\n",
    );
    let str2 = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\tpublic void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (Object data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @Nullable Object data) {\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package testNullAnnotations;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "interface Consumer<T> {\n",
        "    void accept(T t);\n",
        "}\n",
        "public class Snippet {\n",
        "\t\n",
        "\t@SuppressWarnings(\"null\")\n",
        "    public void select(final double min, final double max) {\n",
        "\t    doStuff(0, 1, min, max, (Object data) -> updateSelectionData(data));\n",
        "\t}\n",
        "\t\n",
        "\tprivate void doStuff(int a, int b, final double min, final double max, Consumer<Object> postAction) {\n",
        "\n",
        "\t}\n",
        "    private void updateSelectionData(final @NonNull Object data) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'data' to '@NonNull'", &str1);
    let e2 = Expected::new("Change parameter of 'updateSelectionData(..)' to '@Nullable'", &str2);
    let e3 = Expected::new("Add @SuppressWarnings 'null' to 'select()'", &str3);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}
