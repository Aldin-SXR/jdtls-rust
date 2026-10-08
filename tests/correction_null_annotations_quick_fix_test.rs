//! Port of `org.eclipse.jdt.ls.core.internal.correction.NullAnnotationsQuickFixTest`.

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
    let jar = fixtures_dir().join("testresources/org.eclipse.jdt.annotation_1.2.100.v20241001-0914.jar");
    t.ws.add_library(&root, &jar);
    (t, root)
}

#[test]
fn test_extract_nullable_field1() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        System.out.println(f.toUpperCase());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        final String f2 = f;\n",
        "        if (f2 != null) {\n",
        "            System.out.println(f2.toUpperCase());\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field2() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo(boolean b) {\n",
        "        @SuppressWarnings(\"unused\") boolean f2 = false;\n",
        "        if (b)\n",
        "          System.out.println(f.toUpperCase());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo(boolean b) {\n",
        "        @SuppressWarnings(\"unused\") boolean f2 = false;\n",
        "        if (b) {\n",
        "            final String f3 = f;\n",
        "            if (f3 != null) {\n",
        "                System.out.println(f3.toUpperCase());\n",
        "            } else {\n",
        "                // TODO handle null value\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field3() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    int f;\n",
        "    public int foo(E that) {\n",
        "        return that.other.f;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    int f;\n",
        "    public int foo(E that) {\n",
        "        final E other2 = that.other;\n",
        "        if (other2 != null) {\n",
        "            return other2.f;\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "            return 0;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field4() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    @Nullable String f;\n",
        "    public String foo() {\n",
        "        return this.other.f;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    @Nullable String f;\n",
        "    public String foo() {\n",
        "        final E other2 = this.other;\n",
        "        if (other2 != null) {\n",
        "            return other2.f;\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "            return null;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field5() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        String lo;\n",
        "        if ((lo = this.other.f) != null)\n",
        "            System.out.println(lo);\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable E other;\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        String lo;\n",
        "        final E other2 = this.other;\n",
        "        if (other2 != null) {\n",
        "            if ((lo = other2.f) != null)\n",
        "                System.out.println(lo);\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field6() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String[] f1;\n",
        "    @Nullable String[] f2;\n",
        "    public void foo() {\n",
        "        System.out.println(f1[0]);\n",
        "        System.out.println(f2.length);\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String[] f1;\n",
        "    @Nullable String[] f2;\n",
        "    public void foo() {\n",
        "        final String[] f12 = f1;\n",
        "        if (f12 != null) {\n",
        "            System.out.println(f12[0]);\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "        System.out.println(f2.length);\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field7() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "import java.util.List;\n",
        "public class E {\n",
        "    @Nullable List<String> f;\n",
        "    public void foo() {\n",
        "        System.out.println(f.size());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "import java.util.List;\n",
        "public class E {\n",
        "    @Nullable List<String> f;\n",
        "    public void foo() {\n",
        "        final List<String> f2 = f;\n",
        "        if (f2 != null) {\n",
        "            System.out.println(f2.size());\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field8() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable Exception e;\n",
        "    {\n",
        "        e.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable Exception e;\n",
        "    {\n",
        "        final Exception e2 = e;\n",
        "        if (e2 != null) {\n",
        "            e2.printStackTrace();\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_nullable_field9() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public String foo() {\n",
        "        String upper = f.toUpperCase();\n",
        "        System.out.println(upper);\n",
        "        return upper;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public String foo() {\n",
        "        final String f2 = f;\n",
        "        if (f2 != null) {\n",
        "            String upper = f2.toUpperCase();\n",
        "            System.out.println(upper);\n",
        "            return upper;\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_potentially_null_field1() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public @NonNull String foo() {\n",
        "        return this.f;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public @NonNull String foo() {\n",
        "        final String f2 = this.f;\n",
        "        if (f2 != null) {\n",
        "            return f2;\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "            return null;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    // secondary proposal: Change return type of 'foo(..)' to '@Nullable'
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public @Nullable String foo() {\n",
        "        return this.f;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    let e2 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_extract_potentially_null_field2() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        E local = this;\n",
        "        bar(local.f);\n",
        "    }\n",
        "    public void bar(@NonNull String s) { }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable String f;\n",
        "    public void foo() {\n",
        "        E local = this;\n",
        "        final String f2 = local.f;\n",
        "        if (f2 != null) {\n",
        "            bar(f2);\n",
        "        } else {\n",
        "            // TODO handle null value\n",
        "        }\n",
        "    }\n",
        "    public void bar(@NonNull String s) { }\n",
        "}\n",
    );
    let e1 = Expected::new("Extract to checked local variable", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter1a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Exception e1) {\n",
        "        @NonNull Exception e = new Exception();\n",
        "        e = e1;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "        @NonNull Exception e = new Exception();\n",
        "        e = e1;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e1' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter1b() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(Exception e1) {\n",
        "        @NonNull Exception e = new Exception();\n",
        "        e = e1;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "        @NonNull Exception e = new Exception();\n",
        "        e = e1;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e1' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter1c() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo(@Nullable Object o) {\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo(@NonNull Object o) {\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter1d() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo(Object o) {\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo(@NonNull Object o) {\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@NonNull'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter2() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "        e1 = null;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    t.assert_code_action_not_exists(&cu, "Change parameter 'o' to '@NonNull'");
}

#[test]
fn test_change_parameter3a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Exception e1) {\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    void foo(@Nullable Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e1' to '@Nullable'", &str2);
    let e2 = Expected::new("Change parameter in overridden 'foo(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_parameter3b() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Exception e1) {\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "public class E2 extends E {\n",
        "    void foo(Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "public class E2 extends E {\n",
        "    void foo(@Nullable Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e1' to '@Nullable'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter3c() {
    let (mut t, root) = setup();
    // quickfix only offered with this warning enabled, but no need to say, because default is already "warning"
    //		this.fJProject1.setOption(JavaCore.COMPILER_PB_NONNULL_PARAMETER_ANNOTATION_DROPPED, JavaCore.WARNING);
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "public class E2 extends E {\n",
        "    void foo(Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class E2 extends E {\n",
        "    void foo(@NonNull Exception e1) {\n",
        "        e1.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e1' to '@NonNull'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter4() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 {\n",
        "    void test(E e, @Nullable Object in) {\n",
        "        e.foo(in);\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 {\n",
        "    void test(E e, @NonNull Object in) {\n",
        "        e.foo(in);\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'in' to '@NonNull'", &str2);
    let e2 = Expected::new("Change parameter of 'foo(..)' to '@Nullable'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_parameter4a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "public class E2 {\n",
        "    void test(E e, Object in) {\n",
        "        e.foo(in);\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class E2 {\n",
        "    void test(E e, @NonNull Object in) {\n",
        "        e.foo(in);\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'in' to '@NonNull'", &str2);
    let e2 = Expected::new("Change parameter of 'foo(..)' to '@Nullable'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_parameter5() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.nullanalysis", "disabled");
    let str = concat!(
        "package test1;\n",
        "public class E {\n",
        "    void foo(Object o) {\n",
        "        if (o == null) return;\n",
        "        if (o != null) System.out.print(o.toString());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    t.assert_code_action_not_exists(&cu, "Change parameter 'o' to @Nullable");
    t.assert_code_action_not_exists(&cu, "Change parameter 'o' to @NonNull");
}

#[test]
fn test_change_parameter6() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class E {\n",
        "    void foo(Object o) {\n",
        "        if (o == null) return;\n",
        "        if (o != null) System.out.print(o.toString());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    t.assert_code_action_not_exists(&cu, "Change parameter 'o' to @Nullable");
    t.assert_code_action_not_exists(&cu, "Change parameter 'o' to @NonNull");
}

#[test]
fn test_change_parameter7() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Object o) {\n",
        "        if (o != null) System.out.print(o.toString());\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        if (o != null) System.out.print(o.toString());\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@Nullable'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter8() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class E {\n",
        "    void foo(@org.eclipse.jdt.annotation.NonNull Object o) {\n",
        "        if (o == null) System.out.print(\"NOK\");\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "public class E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        if (o == null) System.out.print(\"NOK\");\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@Nullable'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_parameter9() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "enabled");
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E2 extends E {\n",
        "    void foo(Object o) {\n",
        "        System.out.print(\"E2\");\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E2 extends E {\n",
        "    void foo(@Nullable Object o) {\n",
        "        System.out.print(\"E2\");\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(@NonNull Object o) {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'o' to '@Nullable'", &str2);
    let e2 = Expected::new("Change parameter in overridden 'foo(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_return1() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo() {\n",
        "        @Nullable Object o = null;\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable Object foo() {\n",
        "        @Nullable Object o = null;\n",
        "        return o;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_return2a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    @Nullable Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    @NonNull Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@NonNull'", &str2);
    let e2 = Expected::new("Change return type of overridden 'foo(..)' to '@Nullable'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_return2b() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "public class E2 extends E {\n",
        "    Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class E2 extends E {\n",
        "    @NonNull\n",
        "    Object foo() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@NonNull'", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_change_return3() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations", "enabled");
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNull Object foo() {\n",
        "        // nop\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public interface IE {\n",
        "    @Nullable Object foo();\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "IE.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "public class E2 extends E implements IE {\n",
        "    public Object foo() {\n",
        "        return this;\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str2);
    let str3 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "public class E2 extends E implements IE {\n",
        "    public @Nullable Object foo() {\n",
        "        return this;\n",
        "    }\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class E2 extends E implements IE {\n",
        "    public @NonNull Object foo() {\n",
        "        return this;\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str3);
    let e2 = Expected::new("Change return type of 'foo(..)' to '@NonNull'", &str4);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_return4() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    @Nullable Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 {\n",
        "    @NonNull Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 {\n",
        "    @Nullable Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str2);
    let e2 = Expected::new("Change return type of 'bar(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_return5() {
    let (mut t, root) = setup();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.problem.suppressOptionalErrors", "enabled");
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @Nullable Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "public class E2 {\n",
        "    public Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str2);
    let str3 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "public class E2 {\n",
        "    public @Nullable Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let str4 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str3);
    let e2 = Expected::new("Change return type of 'bar(..)' to '@NonNull'", &str4);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_change_return6() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str0 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault(false)\n",
        "public class E {\n",
        "    @Nullable Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str0);
    let str1 = concat!(
        "package test1;\n",
        "public class E2 {\n",
        "    public Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "public class E2 {\n",
        "    public @Nullable Object foo(E e) {\n",
        "        return e.bar();\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault(false)\n",
        "public class E {\n",
        "    @NonNull Object bar() {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change return type of 'foo(..)' to '@Nullable'", &str2);
    let e2 = Expected::new("Change return type of 'bar(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_remove_redundant_annotation1() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    void foo(@NonNull Object o) {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    void foo(Object o) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation2() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    @NonNull\n",
        "    Object foo(Object o) {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    Object foo(Object o) {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation3() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    @NonNull\n",
        "    public Object foo(Object o) {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    public Object foo(Object o) {\n",
        "        return new Object();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation4() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    @NonNullByDefault\n",
        "    void foo(Object o) {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    void foo(Object o) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation5() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    @NonNullByDefault\n",
        "    class E1 {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "    class E1 {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation6() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNullByDefault\n",
        "    void foo(Object o) {\n",
        "        @NonNullByDefault\n",
        "        class E1 {\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNullByDefault\n",
        "    void foo(Object o) {\n",
        "        class E1 {\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation7() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "@NonNullByDefault\n",
        "public class E {\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_redundant_annotation8() {
    let (mut t, root) = setup();
    let str = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    @NonNullByDefault\n",
        "    void foo(Object o) {\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(Object o) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Remove redundant nullness annotation", &str2);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_add_non_null() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "public class E {\n",
        "    public <T extends Number> double foo(boolean b) {\n",
        "        Number n=Integer.valueOf(1);\n",
        "        if(b) {\n",
        "          n = null;\n",
        "        };\n",
        "        return n.doubleValue();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "\n",
        "import org.eclipse.jdt.annotation.NonNull;\n",
        "\n",
        "public class E {\n",
        "    public <T extends Number> double foo(boolean b) {\n",
        "        @NonNull\n",
        "        Number n=Integer.valueOf(1);\n",
        "        if(b) {\n",
        "          n = null;\n",
        "        };\n",
        "        return n.doubleValue();\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Declare 'n' as '@NonNull' to see the root problem", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug506108() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(Exception e, Exception e1, Exception e2) {\n",
        "    }\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    void foo(Exception e1, @NonNull Exception e2, Exception e) {\n",
        "        e2.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E2.java", &str1);
    let str2 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E2 extends E {\n",
        "    void foo(Exception e1, @Nullable Exception e2, Exception e) {\n",
        "        e2.printStackTrace();\n",
        "    }\n",
        "}\n",
    );
    let str3 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.*;\n",
        "public class E {\n",
        "    void foo(Exception e, @NonNull Exception e1, Exception e2) {\n",
        "    }\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 'e2' to '@Nullable'", &str2);
    let e2 = Expected::new("Change parameter in overridden 'foo(..)' to '@NonNull'", &str3);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_bug525428a() {
    let (mut t, root) = setup();
    // the null analysis mode rewrites this option while the project is imported
    t.ws.wait_projects_built();
    t.ws.set_project_option(&root, "org.eclipse.jdt.core.compiler.annotation.missingNonNullByDefaultAnnotation", "error");
    let str = "package test1;\n";
    let cu = t.ws.create_cu(&root, "src", "test1", "package-info.java", &str);
    let str1 = concat!(
        "@org.eclipse.jdt.annotation.NonNullByDefault\n",
        "package test1;\n",
    );
    let e1 = Expected::new("Add '@NonNullByDefault' to the package declaration", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513423a() {
    let (mut t, root) = setup();
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.NonNullByDefault;\n",
        "\n",
        "@NonNullByDefault\n",
        "public class E extends RuntimeException {\n",
        "\tprivate static final long serialVersionUID = 1L;\n",
        "\n",
        "\tpublic void printStackTrace(\n",
        "\t\t// Illegal redefinition of parameter s, inherited method from Throwable\n",
        "\t\t// does not constrain this parameter\n",
        "\t\tjava.io.PrintStream s) {\n",
        "\t\t\tif (s != null) {\n",
        "\t\t\t\tsynchronized (s) {\n",
        "\t\t\t\t\ts.print(getClass().getName() + \": \");\n",
        "\t\t\t\t\ts.print(getStackTrace());\n",
        "\t\t\t\t}\n",
        "\t\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.NonNullByDefault;\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "@NonNullByDefault\n",
        "public class E extends RuntimeException {\n",
        "\tprivate static final long serialVersionUID = 1L;\n",
        "\n",
        "\tpublic void printStackTrace(\n",
        "\t\t// Illegal redefinition of parameter s, inherited method from Throwable\n",
        "\t\t// does not constrain this parameter\n",
        "\t\tjava.io.@Nullable PrintStream s) {\n",
        "\t\t\tif (s != null) {\n",
        "\t\t\t\tsynchronized (s) {\n",
        "\t\t\t\t\ts.print(getClass().getName() + \": \");\n",
        "\t\t\t\t\ts.print(getStackTrace());\n",
        "\t\t\t\t}\n",
        "\t\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 's' to '@Nullable'", &str1);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_bug513423b() {
    let (mut t, root) = setup();
    let str0 = concat!(
        "package test1;\n",
        "\n",
        "import static java.lang.annotation.ElementType.TYPE_USE;\n",
        "\n",
        "import java.lang.annotation.Documented;\n",
        "import java.lang.annotation.ElementType;\n",
        "import java.lang.annotation.Retention;\n",
        "import java.lang.annotation.RetentinPolicy;\n",
        "import java.lang.annotation.Target;\n",
        "\n",
        "@Documented\n",
        "@Retention(RetentionPolicy.CLASS)\n",
        "@Target({ TYPE_USE })\n",
        "public @interface SomeAnnotation {\n",
        "\t// marker annotation with no members\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test1", "SomeAnnotation.java", &str0);
    let str = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.NonNullByDefault;\n",
        "\n",
        "@NonNullByDefault\n",
        "public class E extends RuntimeException {\n",
        "\tprivate static final long serialVersionUID = 1L;\n",
        "\n",
        "\tpublic void printStackTrace(\n",
        "\t\t// Illegal redefinition of parameter s, inherited method from Throwable\n",
        "\t\t// does not constrain this parameter\n",
        "\t\tjava.io.@SomeAnnotation PrintStream s) {\n",
        "\t\t\tif (s != null) {\n",
        "\t\t\t\tsynchronized (s) {\n",
        "\t\t\t\t\ts.print(getClass().getName() + \": \");\n",
        "\t\t\t\t\ts.print(getStackTrace());\n",
        "\t\t\t\t}\n",
        "\t\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &str);
    let str1 = concat!(
        "package test1;\n",
        "import org.eclipse.jdt.annotation.NonNullByDefault;\n",
        "import org.eclipse.jdt.annotation.Nullable;\n",
        "\n",
        "@NonNullByDefault\n",
        "public class E extends RuntimeException {\n",
        "\tprivate static final long serialVersionUID = 1L;\n",
        "\n",
        "\tpublic void printStackTrace(\n",
        "\t\t// Illegal redefinition of parameter s, inherited method from Throwable\n",
        "\t\t// does not constrain this parameter\n",
        "\t\tjava.io.@SomeAnnotation @Nullable PrintStream s) {\n",
        "\t\t\tif (s != null) {\n",
        "\t\t\t\tsynchronized (s) {\n",
        "\t\t\t\t\ts.print(getClass().getName() + \": \");\n",
        "\t\t\t\t\ts.print(getStackTrace());\n",
        "\t\t\t\t}\n",
        "\t\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let e1 = Expected::new("Change parameter 's' to '@Nullable'", &str1);
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
