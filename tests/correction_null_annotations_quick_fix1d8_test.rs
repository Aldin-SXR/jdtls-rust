//! Port of `org.eclipse.jdt.ls.core.internal.correction.NullAnnotationsQuickFix1d8Test`.

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
fn test_bug499716_a() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "enabled");
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str0 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\t@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "\tK get();\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic String get() { // <-- error \"The default '@NonNull' conflicts...\"\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str0);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\t@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "\tK get();\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic @Nullable String get() { // <-- error \"The default '@NonNull' conflicts...\"\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\t@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "\tK get();\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic @NonNull String get() { // <-- error \"The default '@NonNull' conflicts...\"\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\t@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "    @NonNull\n",
        "\tK get();\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic String get() { // <-- error \"The default '@NonNull' conflicts...\"\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'get(..)' to '@Nullable'", &str1);
    let e2 = Expected::new("Change return type of 'get(..)' to '@NonNull'", &str2);
    let e3 = Expected::new("Change return type of overridden 'get(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_bug499716_b() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "enabled");
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\tvoid set(int i, K arg);\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(int i, String arg) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\tvoid set(int i, K arg);\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(int i, @Nullable String arg) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "interface Type<@Nullable K> {\n",
        "\tvoid set(int i, @NonNull K arg);\n",
        "\n",
        "\tclass U implements Type<@Nullable String> {\n",
        "\t\t@Override\n",
        "\t\tpublic void set(int i, String arg) {\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'arg' to '@Nullable'", &str2);
    let e2 = Expected::new("Change parameter in overridden 'set(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_bug499716_c() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "enabled");
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "interface Type {\n",
        "\tString get();\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic @Nullable String get() {\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "interface Type {\n",
        "\tString get();\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic String get() {\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault(DefaultLocation.RETURN_TYPE)\n",
        "interface Type {\n",
        "\t@Nullable\n",
        "    String get();\n",
        "\n",
        "\tclass U implements Type {\n",
        "\t\t@Override\n",
        "\t\tpublic @Nullable String get() {\n",
        "\t\t\treturn \"\";\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'get(..)' to '@NonNull'", &str1);
    let e2 = Expected::new("Change return type of overridden 'get(..)' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_bug499716_d() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
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

#[test]
fn test443146a() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
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
fn test443146b() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract @Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tprivate @Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> x;\n",
        "\n",
        "    abstract @Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract @Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g(@Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> x) {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\tabstract @Nullable Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
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
fn test443146c() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault({})\n",
        "abstract class Test {\n",
        "\t@NonNullByDefault\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault({})\n",
        "abstract class Test {\n",
        "\tprivate @NonNull Map<? extends @NonNull Map<@NonNull String, @Nullable Integer>, String @NonNull [][]> x;\n",
        "\n",
        "    @NonNullByDefault\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault({})\n",
        "abstract class Test {\n",
        "\t@NonNullByDefault\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g(@NonNull Map<? extends @NonNull Map<@NonNull String, @Nullable Integer>, String @NonNull [][]> x) {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "@NonNullByDefault({})\n",
        "abstract class Test {\n",
        "\t@NonNullByDefault\n",
        "\tabstract Map<? extends Map<String, @Nullable Integer>, String[][]> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tMap<? extends @NonNull Map<@NonNull String, @Nullable Integer>, String @NonNull [][]> x = f();\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Create field 'x'", &str2);
    let e2 = Expected::new("Create parameter 'x'", &str3);
    let e3 = Expected::new("Create local variable 'x'", &str4);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test443146d() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\t@NonNull Map<@NonNull String, @Nullable Integer> f(Object o) {\n",
        "\t\treturn o;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test {\n",
        "\t@NonNull Map<@NonNull String, @Nullable Integer> f(Object o) {\n",
        "\t\treturn (Map<@NonNull String, @Nullable Integer>) o;\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Add cast to 'Map<String, Integer>'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test443146e() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract @NonNull T f();\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tprivate @NonNull T x;\n",
        "\n",
        "    abstract @NonNull T f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract @NonNull T f();\n",
        "\n",
        "\tpublic void g(@NonNull T x) {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract @NonNull T f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\t@NonNull\n",
        "        T x = f();\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Create field 'x'", &str2);
    let e2 = Expected::new("Create parameter 'x'", &str3);
    let e3 = Expected::new("Create local variable 'x'", &str4);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test443146f() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract Map<Map<@NonNull ?, Integer>, @NonNull T> f();\n",
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
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tprivate Map<Map<@NonNull ?, Integer>, @NonNull T> x;\n",
        "\n",
        "    abstract Map<Map<@NonNull ?, Integer>, @NonNull T> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract Map<Map<@NonNull ?, Integer>, @NonNull T> f();\n",
        "\n",
        "\tpublic void g(Map<Map<@NonNull ?, Integer>, @NonNull T> x) {\n",
        "\t\tx=f();\n",
        "\t}\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "\n",
        "abstract class Test<T> {\n",
        "\tabstract Map<Map<@NonNull ?, Integer>, @NonNull T> f();\n",
        "\n",
        "\tpublic void g() {\n",
        "\t\tMap<Map<@NonNull ?, Integer>, @NonNull T> x = f();\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Create field 'x'", &str2);
    let e2 = Expected::new("Create parameter 'x'", &str3);
    let e3 = Expected::new("Create local variable 'x'", &str4);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_bug513682() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class Test {\n",
        "    void foo(Object o) {\n",
        "      if(o != null) {\n",
        "          o.hashCode();\n",
        "      }\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class Test {\n",
        "    void foo(@Nullable Object o) {\n",
        "      if(o != null) {\n",
        "          o.hashCode();\n",
        "      }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@Nullable'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513209a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class A {\n",
        "   public void SomeMethod(\n",
        "      String[] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "A.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String[] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "B.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String @Nullable [] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'a' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513209b() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class A {\n",
        "   public void SomeMethod(\n",
        "      int[][] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "A.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      int[][] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "B.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      int @Nullable [][] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'a' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513209c() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class A {\n",
        "   public void SomeMethod(\n",
        "      String[] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "A.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String @NonNull [] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "B.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String @Nullable [] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'a' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513209d() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class A {\n",
        "   public String[][][] SomeMethod()\n",
        "   {\n",
        "\t\treturn null;\n",
        "   }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "A.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public String[][][] SomeMethod()\n",
        "   {\n",
        "\t\treturn new String[0][][];\n",
        "   }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "B.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public String @NonNull [][][] SomeMethod()\n",
        "   {\n",
        "\t\treturn new String[0][][];\n",
        "   }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'SomeMethod(..)' to '@NonNull'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug525424() {
    let (mut t, root) = setup();
    t.ws.settings["java"]["compile"]["nullAnalysis"] = json!({
        "mode": "automatic",
        "nonnull": ["my.NonNull"],
        "nullable": ["my.Nullable"],
        "nonnullbydefault": ["my.NonNullByDefault"]
    });
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.nullable", "my.Nullable");
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.nonnull", "my.NonNull");
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault", "my.NonNullByDefault");
    let str = concat!(
        "package my;\n",
        "\n",
        "import java.lang.annotation.ElementType;\n",
        "import java.lang.annotation.Target;\n",
        "\n",
        "\n",
        "@Target(ElementType.TYPE_USE)\n",
        "public @interface Nullable {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "my", "Nullable.java", &str);
    let str1 = concat!(
        "package my;\n",
        "\n",
        "import java.lang.annotation.ElementType;\n",
        "import java.lang.annotation.Target;\n",
        "\n",
        "@Target(ElementType.TYPE_USE)\n",
        "public @interface NonNull {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "my", "NonNull.java", &str1);
    let str2 = concat!(
        "package my;\n",
        "\n",
        "public enum DefaultLocation {\n",
        "\tPARAMETER, RETURN_TYPE, FIELD, TYPE_BOUND, TYPE_ARGUMENT\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "my", "DefaultLocation.java", &str2);
    let str3 = concat!(
        "package my;\n",
        "\n",
        "import static my.DefaultLocation.*;\n",
        "\n",
        "public @interface NonNullByDefault {\n",
        "\tDefaultLocation[] value() default { PARAMETER, RETURN_TYPE, FIELD, TYPE_BOUND, TYPE_ARGUMENT };\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "my", "NonNullByDefault.java", &str3);
    let str4 = concat!(
        "package test1;\n",
        "public class A {\n",
        "   public void SomeMethod(\n",
        "      String[] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "A.java", &str4);
    let str5 = concat!(
        "package test1;\n",
        "import my.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String[] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "B.java", &str5);
    let str6 = concat!(
        "package test1;\n",
        "import my.*;\n",
        "@NonNullByDefault\n",
        "public class B extends A {\n",
        "   @Override\n",
        "   public void SomeMethod(\n",
        "      String @Nullable [] a)\n",
        "   {\n",
        "\n",
        "   }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'a' to '@Nullable'", &str6);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_gh1294() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "my", "Test.java", concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn new IInputValidator() {\n",
        "\t\t\t@Override\n",
        "\t\t\tpublic String isValid(String newText) {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, this,\n",
        "\t\t\t\t\t\trefPrefix, errorOnEmptyName);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String refNameInput,\n",
        "\t\t\t@NonNull Object repo, @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    ));
    let str1 = concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn new IInputValidator() {\n",
        "\t\t\t@Override\n",
        "\t\t\tpublic String isValid(String newText) {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, this,\n",
        "\t\t\t\t\t\trefPrefix, errorOnEmptyName);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String refNameInput,\n",
        "\t\t\t@NonNull Object repo, @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'refPrefix' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_gh1294_no_quickfix() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "my", "Test.java", concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "import java.util.List;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final List<String> refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn new IInputValidator() {\n",
        "\t\t\t@Override\n",
        "\t\t\tpublic String isValid(String newText) {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, this,\n",
        "\t\t\t\t\t\trefPrefix, errorOnEmptyName);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String refNameInput,\n",
        "\t\t\t@NonNull Object repo, List<@NonNull String> refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    ));
    t.assert_code_actions_exist(&cu, &["Add @SuppressWarnings 'null' to 'isValid()'", "Add @SuppressWarnings 'null' to 'validationStatus'"]);
}

#[test]
fn test_gh1294_lambda() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "my", "Test.java", concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn (String newText) -> {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, new Object(),\n",
        "\t\t\t\t\t\trefPrefix, errorOnEmptyName);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String refNameInput,\n",
        "\t\t\t@NonNull Object repo, @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    ));
    let str1 = concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn (String newText) -> {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, new Object(),\n",
        "\t\t\t\t\t\trefPrefix, errorOnEmptyName);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String refNameInput,\n",
        "\t\t\t@NonNull Object repo, @NonNull String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'refPrefix' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_gh1294_varargs() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "my", "Test.java", concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(@NonNull String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn new IInputValidator() {\n",
        "\t\t\tpublic String isValid(@NonNull String newText) {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, refPrefix);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(@NonNull String... refPrefix) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    ));
    t.assert_code_actions_exist(&cu, &["Add @SuppressWarnings 'null' to 'isValid()'", "Add @SuppressWarnings 'null' to 'validationStatus'", "Change parameter 'refPrefix' to '@NonNull'"]);
}

#[test]
fn test_gh1294_varargs_ok() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "my", "Test.java", concat!(
        "package my;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "interface IInputValidator {\n",
        "\tpublic String isValid(@NonNull String newText);\n",
        "}\n",
        "public class Test {\n",
        "\tpublic static IInputValidator getRefNameInputValidator(\n",
        "\t\t\tfinal Object repo, final String refPrefix,\n",
        "\t\t\tfinal boolean errorOnEmptyName) {\n",
        "\t\treturn new IInputValidator() {\n",
        "\t\t\tpublic String isValid(@NonNull String newText) {\n",
        "\t\t\t\tString validationStatus = validateNewRefName(newText, refPrefix);\n",
        "\t\t\t\treturn validationStatus;\n",
        "\t\t\t}\n",
        "\t\t};\n",
        "\t}\n",
        "\t@NonNull\n",
        "\tpublic static String validateNewRefName(String s1, @NonNull String s2, @NonNull String... refPrefix) {\n",
        "\t\treturn \"\";\n",
        "\t}\n",
        "}\n",
    ));
    t.assert_code_actions_exist(&cu, &["Add @SuppressWarnings 'null' to 'isValid()'", "Add @SuppressWarnings 'null' to 'validationStatus'", "Change parameter 'refPrefix' to '@NonNull'", "Change parameter of 'validateNewRefName(..)' to '@Nullable'"]);
}
