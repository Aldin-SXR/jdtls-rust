//! Port of `org.eclipse.jdt.ls.core.internal.correction.ConvertToRecordQuickAssistTest`.

mod common;

use common::quickfix::{get_range, to_range, Expected, QuickFixTest};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    // The upstream test preferences indent with tabs.
    t.ws.settings["java"]["format"] = serde_json::json!({ "insertSpaces": false });
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    // `JavaModelUtil.setComplianceOptions(options17, VERSION_17)`.
    let mut options17 = BTreeMap::new();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options17.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "17".to_owned());
    }
    options17.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options17.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options17.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    t.ws.set_project_options(&root, &options17);
    (t, root)
}

#[test]
fn test_convert_to_record1() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Cls(int a, String b) {\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record2() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let str2 = concat!(
        "package test;\n",
        "\n",
        "public class Cls2 {\n",
        "\tpublic void foo() {\n",
        "\t\tCls cls= new Cls(3, \"abc\");\n",
        "\t\tSystem.out.println(cls.getA());\n",
        "\t\tSystem.out.println(cls.getB());\n",
        "\t}\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test", "Cls2.java", &str2);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Cls(int a, String b) {\n",
        "}\n",
    );
    let expected2 = concat!(
        "package test;\n",
        "\n",
        "public class Cls2 {\n",
        "\tpublic void foo() {\n",
        "\t\tCls cls= new Cls(3, \"abc\");\n",
        "\t\tSystem.out.println(cls.a());\n",
        "\t\tSystem.out.println(cls.b());\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let e2 = Expected::new("Convert to record", &expected2);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_multi_file(&cu, selection, &[e, e2]);
}

#[test]
fn test_convert_to_record3() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t// Inner class\n",
        "\tpublic static class Inner {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\n",
        "\t\tpublic Inner(int a, String b) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let str2 = concat!(
        "package test;\n",
        "\n",
        "public class Cls2 {\n",
        "\tpublic void foo() {\n",
        "\t\tCls.Inner cls= new Cls.Inner(3, \"abc\");\n",
        "\t\tSystem.out.println(cls.getA());\n",
        "\t\tSystem.out.println(cls.getB());\n",
        "\t}\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "test", "Cls2.java", &str2);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t// Inner class\n",
        "\tpublic static record Inner(int a, String b) {\n",
        "\t}\n",
        "}\n",
    );
    let expected2 = concat!(
        "package test;\n",
        "\n",
        "public class Cls2 {\n",
        "\tpublic void foo() {\n",
        "\t\tCls.Inner cls= new Cls.Inner(3, \"abc\");\n",
        "\t\tSystem.out.println(cls.a());\n",
        "\t\tSystem.out.println(cls.b());\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let e2 = Expected::new("Convert to record", &expected2);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_multi_file(&cu, selection, &[e, e2]);
}

#[test]
fn test_convert_to_record4() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t/**\n",
        "\t * Class Inner\n",
        "\t */\n",
        "\tprivate final class Inner {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;\n",
        "\n",
        "\t\tpublic Inner(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
        "\tpublic void foo() {\n",
        "\t\tInner inner= new Inner(1, \"comment\", 4.3);\n",
        "\t\tSystem.out.println(inner.getA());\n",
        "\t\tSystem.out.println(inner.getB());\n",
        "\t\tSystem.out.println(inner.getC());\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t/**\n",
        "\t * Class Inner\n",
        "\t */\n",
        "\tprivate record Inner(int a, String b, double c) {\n",
        "\t}\n",
        "\tpublic void foo() {\n",
        "\t\tInner inner= new Inner(1, \"comment\", 4.3);\n",
        "\t\tSystem.out.println(inner.a());\n",
        "\t\tSystem.out.println(inner.b());\n",
        "\t\tSystem.out.println(inner.c());\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let source = t.ws.read(&cu);
    let start = source.find("Inner").map_or(-1, |b| source[..b].encode_utf16().count() as i64);
    let selection = to_range(&source, start, 5);
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record5() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "/* Class Cls */\n",
        "public class Cls extends Object {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "/* Class Cls */\n",
        "public record Cls(int a, String b) {\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record6() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "import java.util.Objects;\n",
        "\n",
        "/* Class Cls */\n",
        "public class Cls extends Object {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic String toString() {\n",
        "\t\treturn \"A [a=\" + a + \", b=\" + b + \"]\";\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic int hashCode() {\n",
        "\t\treturn Objects.hash(a, b);\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic boolean equals(Object obj) {\n",
        "\t\tif (this == obj)\n",
        "\t\t\treturn true;\n",
        "\t\tif (obj == null)\n",
        "\t\t\treturn false;\n",
        "\t\tif (getClass() != obj.getClass())\n",
        "\t\t\treturn false;\n",
        "\t\tCls other = (Cls) obj;\n",
        "\t\treturn a == other.a && Objects.equals(b, other.b);\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "import java.util.Objects;\n",
        "\n",
        "/* Class Cls */\n",
        "public record Cls(int a, String b) {\n",
        "\t@Override\n",
        "\tpublic String toString() {\n",
        "\t\treturn \"A [a=\" + a + \", b=\" + b + \"]\";\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic int hashCode() {\n",
        "\t\treturn Objects.hash(a, b);\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic boolean equals(Object obj) {\n",
        "\t\tif (this == obj)\n",
        "\t\t\treturn true;\n",
        "\t\tif (obj == null)\n",
        "\t\t\treturn false;\n",
        "\t\tif (getClass() != obj.getClass())\n",
        "\t\t\treturn false;\n",
        "\t\tCls other = (Cls) obj;\n",
        "\t\treturn a == other.a && Objects.equals(b, other.b);\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record7() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic Cls(int a) {\n",
        "\t\tthis(a, \"abc\");\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Cls(int a, String b) {\n",
        "\tpublic Cls(int a) {\n",
        "\t\tthis(a, \"abc\");\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record8() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\tpublic static int c;\n",
        "\n",
        "\tstatic {\n",
        "\t\tc = 3;\n",
        "\t}\n",
        "\n",
        "\tpublic static int getC() {\n",
        "\t\treturn c;\n",
        "\t}\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Cls(int a, String b) {\n",
        "\tstatic {\n",
        "\t\tc = 3;\n",
        "\t}\n",
        "\tpublic static int c;\n",
        "\n",
        "\tpublic static int getC() {\n",
        "\t\treturn c;\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record9() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "interface Blah {\n",
        "\tvoid printSomething();\n",
        "}\n",
        "\n",
        "public class Cls implements Blah {\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\tpublic static int c;\n",
        "\n",
        "\tstatic {\n",
        "\t\tc = 3;\n",
        "\t}\n",
        "\n",
        "\tpublic static int getC() {\n",
        "\t\treturn c;\n",
        "\t}\n",
        "\n",
        "\tpublic Cls(int a, String b) {\n",
        "\t\tthis.a= a;\n",
        "\t\tthis.b= b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic void printSomething() {\n",
        "\t\tSystem.out.println(\"here\");\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "interface Blah {\n",
        "\tvoid printSomething();\n",
        "}\n",
        "\n",
        "public record Cls(int a, String b) implements Blah {\n",
        "\tstatic {\n",
        "\t\tc = 3;\n",
        "\t}\n",
        "\tpublic static int c;\n",
        "\n",
        "\tpublic static int getC() {\n",
        "\t\treturn c;\n",
        "\t}\n",
        "\n",
        "\t@Override\n",
        "\tpublic void printSomething() {\n",
        "\t\tSystem.out.println(\"here\");\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getA");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record10() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Pair<T, U> {\n",
        "\tprivate final T first;\n",
        "\tprivate final U second;\n",
        "\n",
        "\tpublic Pair(T first, U second) {\n",
        "\t\tthis.first = first;\n",
        "\t\tthis.second = second;\n",
        "\t}\n",
        "\n",
        "\tpublic T getFirst() {\n",
        "\t\treturn first;\n",
        "\t}\n",
        "\n",
        "\tpublic U getSecond() {\n",
        "\t\treturn second;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Pair.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Pair<T, U>(T first, U second) {\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getFirst");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record11() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "import java.lang.annotation.Retention;\n",
        "import java.lang.annotation.RetentionPolicy;\n",
        "\n",
        "@Deprecated\n",
        "public class User {\n",
        "\t@NotNull\n",
        "\tprivate final String name;\n",
        "\n",
        "\t@Range(min = 0, max = 150)\n",
        "\tprivate final int age;\n",
        "\n",
        "\tpublic User(@NotNull String name, int age) {\n",
        "\t\tthis.name = name;\n",
        "\t\tthis.age = age;\n",
        "\t}\n",
        "\n",
        "\t@NotNull\n",
        "\tpublic String getName() {\n",
        "\t\treturn name;\n",
        "\t}\n",
        "\n",
        "\tpublic int getAge() {\n",
        "\t\treturn age;\n",
        "\t}\n",
        "}\n",
        "\n",
        "@Retention(RetentionPolicy.RUNTIME)\n",
        "@interface NotNull {}\n",
        "\n",
        "@Retention(RetentionPolicy.RUNTIME)\n",
        "@interface Range {\n",
        "\tint min();\n",
        "\tint max();\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "User.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "import java.lang.annotation.Retention;\n",
        "import java.lang.annotation.RetentionPolicy;\n",
        "\n",
        "@Deprecated\n",
        "public record User(@NotNull String name, @Range(min = 0, max = 150) int age) {\n",
        "}\n",
        "\n",
        "@Retention(RetentionPolicy.RUNTIME)\n",
        "@interface NotNull {}\n",
        "\n",
        "@Retention(RetentionPolicy.RUNTIME)\n",
        "@interface Range {\n",
        "\tint min();\n",
        "\tint max();\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getName");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record12() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Identifier {\n",
        "\tprivate final String id;\n",
        "\n",
        "\tpublic Identifier(String id) {\n",
        "\t\tthis.id = id;\n",
        "\t}\n",
        "\n",
        "\tpublic String getId() {\n",
        "\t\treturn id;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Identifier.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record Identifier(String id) {\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getId");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record13() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "class PackagePrivateCls {\n",
        "\tprivate final int value;\n",
        "\n",
        "\tPackagePrivateCls(int value) {\n",
        "\t\tthis.value = value;\n",
        "\t}\n",
        "\n",
        "\tpublic int getValue() {\n",
        "\t\treturn value;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "PackagePrivateCls.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "record PackagePrivateCls(int value) {\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "getValue");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_convert_to_record14() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public final class A {\n",
        "\n",
        "\tprivate final int a;\n",
        "\tprivate final String b;\n",
        "\tprivate int c;\n",
        "\n",
        "\tpublic A(int a, String b, int c) {\n",
        "\t\tclass K {\n",
        "\t\t\tpublic static int doublex(int x) {\n",
        "\t\t\t\treturn x * 2;\n",
        "\t\t\t}\n",
        "\t\t}\n",
        "\t\tthis.a= K.doublex(a);\n",
        "\t\tif (a < 0) {\n",
        "\t\t\tthis.b = massage(b);\n",
        "\t\t} else {\n",
        "\t\t\tthis.b = b;\n",
        "\t\t}\n",
        "\t\tthis.c= c;\n",
        "\t}\n",
        "\n",
        "\tprivate String massage(String s) {\n",
        "\t\treturn s.toLowerCase();\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic String getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "\n",
        "\tpublic int getC() {\n",
        "\t\treturn c;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &str1);
    let expected = concat!(
        "package test;\n",
        "\n",
        "public record A(int a, String b, int c) {\n",
        "\tpublic A(int a, String b, int c) {\n",
        "\t\tclass K {\n",
        "\t\t\tpublic static int doublex(int x) {\n",
        "\t\t\t\treturn x * 2;\n",
        "\t\t\t}\n",
        "\t\t}\n",
        "\t\tthis.a= K.doublex(a);\n",
        "\t\tif (a < 0) {\n",
        "\t\t\tthis.b = massage(b);\n",
        "\t\t} else {\n",
        "\t\t\tthis.b = b;\n",
        "\t\t}\n",
        "\t\tthis.c= c;\n",
        "\t}\n",
        "\n",
        "\tprivate String massage(String s) {\n",
        "\t\treturn s.toLowerCase();\n",
        "\t}\n",
        "}\n",
    );
    let e = Expected::new("Convert to record", &expected);
    let selection = get_range(&t.ws.read(&cu), "massage");
    t.assert_code_actions_range(&cu, selection, &[e]);
}

#[test]
fn test_no_convert_to_record1() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tpublic class Cls {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;\n",
        "\n",
        "\t\tpublic Cls(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record2() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tpublic class Cls {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c = 2.4;;\n",
        "\n",
        "\t\tpublic Cls(int a, String b) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record3() {
    let (mut t, root) = setup();
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tpublic abstract class Cls {\n",
        "\t\tprivate final static int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;;\n",
        "\n",
        "\t\tpublic Cls(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getAValue() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record4() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tpublic class Cls {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;;\n",
        "\n",
        "\t\tpublic Cls(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\n",
        "\t\tprivate int getSum() {\n",
        "\t\t\tc = 4.0;\n",
        "\t\t\treturn a + b.length();\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record5() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tpublic class Cls {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tpublic double c;;\n",
        "\n",
        "\t\tpublic Cls(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record6() {
    let (mut t, root) = setup();
    // https://github.com/eclipse-jdt/eclipse.jdt.ui/issues/2681
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "\tpackage test;\n",
        "\n",
        "\tclass K {\n",
        "\t}\n",
        "\tpublic class Cls extends K {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;;\n",
        "\n",
        "\t\tpublic Cls(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record7() {
    let (mut t, root) = setup();
    // class extended
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t/**\n",
        "\t * Class Inner\n",
        "\t */\n",
        "\tprivate class Inner {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;\n",
        "\n",
        "\t\tpublic Inner(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
        "\tprivate class Inner2 extends Inner {\n",
        "\t\tpublic Inner2() {\n",
        "\t\t\tsuper(2, \"blah\", 5.2);\n",
        "\t\t}\n",
        "\t}\n",
        "\tpublic void foo() {\n",
        "\t\tInner inner= new Inner(1, \"comment\", 4.3);\n",
        "\t\tSystem.out.println(inner.getA());\n",
        "\t\tSystem.out.println(inner.getB());\n",
        "\t\tSystem.out.println(inner.getC());\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Inner");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record8() {
    let (mut t, root) = setup();
    // not all fields initialized
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t/**\n",
        "\t * Class Inner\n",
        "\t */\n",
        "\tprivate class Inner {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;\n",
        "\n",
        "\t\tpublic Inner(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
        "\tpublic void foo() {\n",
        "\t\tInner inner= new Inner(1, \"comment\", 4.3);\n",
        "\t\tSystem.out.println(inner.getA());\n",
        "\t\tSystem.out.println(inner.getB());\n",
        "\t\tSystem.out.println(inner.getC());\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), " b");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record9() {
    let (mut t, root) = setup();
    // second constructor, non-chaining
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\t/**\n",
        "\t * Class Inner\n",
        "\t */\n",
        "\tprivate class Inner {\n",
        "\t\tprivate final int a;\n",
        "\t\tprivate final String b;\n",
        "\t\tprivate double c;\n",
        "\n",
        "\t\tpublic Inner(int a, String b, double c) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= c;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic Inner(int a, String b) {\n",
        "\t\t\tthis.a= a;\n",
        "\t\t\tthis.b= b;\n",
        "\t\t\tthis.c= 2.0;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic double getC() {\n",
        "\t\t\treturn c;\n",
        "\t\t}\n",
        "\t}\n",
        "\tpublic void foo() {\n",
        "\t\tInner inner= new Inner(1, \"comment\", 4.3);\n",
        "\t\tSystem.out.println(inner.getA());\n",
        "\t\tSystem.out.println(inner.getB());\n",
        "\t\tSystem.out.println(inner.getC());\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Inner");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record10() {
    let (mut t, root) = setup();
    // wrong type returned from getter
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class WrongTypeCls {\n",
        "\tprivate final int a;\n",
        "\n",
        "\tpublic WrongTypeCls(int a) {\n",
        "\t\tthis.a = a;\n",
        "\t}\n",
        "\n",
        "\tpublic long getA() {\n",
        "\t\treturn (long) a;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "WrongTypeCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "WrongTypeCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record11() {
    let (mut t, root) = setup();
    // instance initializer
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class InitializerCls {\n",
        "\tprivate final int value;\n",
        "\tprivate final String name;\n",
        "\n",
        "\t{\n",
        "\t\tSystem.out.println(\"Instance initializer\");\n",
        "\t}\n",
        "\n",
        "\tpublic InitializerCls(int value, String name) {\n",
        "\t\tthis.value = value;\n",
        "\t\tthis.name = name;\n",
        "\t}\n",
        "\n",
        "\tpublic int getValue() {\n",
        "\t\treturn value;\n",
        "\t}\n",
        "\n",
        "\tpublic String getName() {\n",
        "\t\treturn name;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "InitializerCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "InitializerCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record12() {
    let (mut t, root) = setup();
    // complex constructor
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class CalculatedCls {\n",
        "\tprivate final int value;\n",
        "\tprivate final int doubled;\n",
        "\n",
        "\tpublic CalculatedCls(int value) {\n",
        "\t\tthis.value = value;\n",
        "\t\tthis.doubled = value * 2;\n",
        "\t}\n",
        "\n",
        "\tpublic int getValue() {\n",
        "\t\treturn value;\n",
        "\t}\n",
        "\n",
        "\tpublic int getDoubled() {\n",
        "\t\treturn doubled;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "CalculatedCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "CalculatedCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record13() {
    let (mut t, root) = setup();
    // native method
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class NativeCls {\n",
        "\tprivate final int value;\n",
        "\n",
        "\tpublic NativeCls(int value) {\n",
        "\t\tthis.value = value;\n",
        "\t}\n",
        "\n",
        "\tpublic int getValue() {\n",
        "\t\treturn value;\n",
        "\t}\n",
        "\n",
        "\tpublic native void nativeMethod();\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "NativeCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "NativeCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record14() {
    let (mut t, root) = setup();
    // protected method finalize
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class NoFieldsCls {\n",
        "\n",
        "\tpublic NoFieldsCls() {\n",
        "\t}\n",
        "\n",
        "\tpublic class Inner {\n",
        "\t\tprivate int a;\n",
        "\t\tprivate String b;\n",
        "\n",
        "\t\tpublic Inner(int a, String b) {\n",
        "\t\t\tthis.a = a;\n",
        "\t\t\tthis.b = b;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic int getA() {\n",
        "\t\t\treturn a;\n",
        "\t\t}\n",
        "\n",
        "\t\tpublic String getB() {\n",
        "\t\t\treturn b;\n",
        "\t\t}\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "NoFieldsCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "NoFieldsCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record15() {
    let (mut t, root) = setup();
    // all fields not initialized
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class PartialInitCls {\n",
        "\tprivate final int a;\n",
        "\tprivate final int b = 10;\n",
        "\n",
        "\tpublic PartialInitCls(int a) {\n",
        "\t\tthis.a = a;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic int getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "PartialInitCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "PartialInitCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record16() {
    let (mut t, root) = setup();
    // sealed class
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public sealed class SealedCls permits SubCls {\n",
        "\tprivate final int a;\n",
        "\tprivate final int b = 10;\n",
        "\n",
        "\tpublic SealedCls(int a) {\n",
        "\t\tthis.a = a;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic int getB() {\n",
        "\t\treturn b;\n",
        "\t}\n",
        "}\n",
        "\n",
        "final class SubCls extends SealedCls {\n",
        "\tpublic SubCls() {\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "SubCls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "SealedCls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record17() {
    let (mut t, root) = setup();
    // no fields
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tpublic Cls() {}\n",
        "\n",
        "\tpublic void printSomething() {\n",
        "\t\tSystem.out.println(\"something\");\n",
        "\t}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

#[test]
fn test_no_convert_to_record18() {
    let (mut t, root) = setup();
    // member class
    let str = concat!(
        "module test {\n",
        "}\n",
    );
    t.ws.create_cu(&root, "src", "", "module-info.java", &str);
    let str1 = concat!(
        "package test;\n",
        "\n",
        "public class Cls {\n",
        "\tprivate int a;\n",
        "\n",
        "\tpublic Cls(int a) {\n",
        "\t\tthis.a = a;\n",
        "\t}\n",
        "\n",
        "\tpublic int getA() {\n",
        "\t\treturn a;\n",
        "\t}\n",
        "\n",
        "\tpublic class K {}\n",
        "}\n",
    );
    let cu = t.ws.create_cu(&root, "src", "test", "Cls.java", &str1);
    let selection = get_range(&t.ws.read(&cu), "Cls");
    t.assert_code_action_not_exists_range(&cu, selection, "Convert to record");
}

