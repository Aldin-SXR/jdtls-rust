//! Port of `org.eclipse.jdt.ls.core.internal.correction.JavadocQuickFixTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    // Preserve the code-template store of the upstream mocked
    // PreferenceManager; a real configuration update clears it.
    t.ws.settings["java"]["templates"] = json!({ "typeComment": ["/**", " * ${type_name}", " * ${tags}", " */"] });
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.invalidJavadoc".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.invalidJavadocTags".into(), "enabled".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.missingJavadocTagsMethodTypeParameters".into(), "enabled".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.missingJavadocTags".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.missingJavadocComments".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.missingJavadocCommentsOverriding".into(), "enabled".into());
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

#[test]
fn test_missing_param1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a \n");
    buf.push_str("     * @param b\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_param2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param b \n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_param3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     * @param c \n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int b, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_param4() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param <A>\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     */\n");
    buf.push_str("    public <A, B> void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param <A>\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param <B> \n");
    buf.push_str("     * @param a\n");
    buf.push_str("     */\n");
    buf.push_str("    public <A, B> void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_param5() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * @param <B> Hello\n");
    buf.push_str(" */\n");
    buf.push_str("public class E<A, B> {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * @param <A> \n");
    buf.push_str(" * @param <B> Hello\n");
    buf.push_str(" */\n");
    buf.push_str("public class E<A, B> {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_param6() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * @author ae\n");
    buf.push_str(" */\n");
    buf.push_str("public class E<A> {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * @author ae\n");
    buf.push_str(" * @param <A> \n");
    buf.push_str(" */\n");
    buf.push_str("public class E<A> {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@param' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_return1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int b, int c) {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     * @return \n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int b, int c) {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@return' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_return2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @return \n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo() {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@return' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_missing_throws() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @return Returns an Int\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo() throws Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @return Returns an Int\n");
    buf.push_str("     * @throws Exception \n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo() throws Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    let e2 = Expected::new("Add '@throws' tag", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_insert_all_missing1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @throws Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int a, int b) throws NullPointerException, Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a \n");
    buf.push_str("     * @param b \n");
    buf.push_str("     * @return \n");
    buf.push_str("     * @throws NullPointerException \n");
    buf.push_str("     * @throws Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int a, int b) throws NullPointerException, Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_insert_all_missing2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     * @return a number\n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int a, int b, int c) throws NullPointerException, Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a \n");
    buf.push_str("     * @param b\n");
    buf.push_str("     * @param c \n");
    buf.push_str("     * @return a number\n");
    buf.push_str("     * @throws NullPointerException \n");
    buf.push_str("     * @throws Exception \n");
    buf.push_str("     */\n");
    buf.push_str("    public int foo(int a, int b, int c) throws NullPointerException, Exception {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_insert_all_missing3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E<S, T> {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * @param <S> \n");
    buf.push_str(" * @param <T> \n");
    buf.push_str(" */\n");
    buf.push_str("public class E<S, T> {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_insert_all_missing4() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param <B> test\n");
    buf.push_str("     * @param b\n");
    buf.push_str("     * @return a number\n");
    buf.push_str("     */\n");
    buf.push_str("    public <A, B> int foo(int a, int b) throws NullPointerException {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param <A> \n");
    buf.push_str("     * @param <B> test\n");
    buf.push_str("     * @param a \n");
    buf.push_str("     * @param b\n");
    buf.push_str("     * @return a number\n");
    buf.push_str("     * @throws NullPointerException \n");
    buf.push_str("     */\n");
    buf.push_str("    public <A, B> int foo(int a, int b) throws NullPointerException {\n");
    buf.push_str("        return 1;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add all missing tags", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_remove_param_tag1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_param_tag2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_throws_tag1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     * @throws Exception Thrown by surprise.\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @param c\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a, int c) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_throws_tag2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_throws_tag3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     * @exception java.io.IOException\n");
    buf.push_str("     * @exception NullPointerException\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception java.io.IOException\n");
    buf.push_str("     * @exception NullPointerException\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_return_tag1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @return Returns the result.\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws Exception {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws Exception {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_remove_unknown_tag1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @return Returns the result.\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws Exception {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     *      comment on second line.\n");
    buf.push_str("     * @exception Exception\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(int a) throws Exception {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove tag", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_method_comment1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public <A> void foo(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param <A>\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     * @throws IOException\n");
    buf.push_str("     */\n");
    buf.push_str("    public <A> void foo(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add Javadoc comment", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_override_method_comment1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" *\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public String toString() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    t.assert_code_action_not_exists(&cu, "Add Javadoc for 'toString'");
}

#[test]
fn test_missing_method_comment3() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * Some comment\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void empty() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * Some comment\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * \n");
    buf.push_str("     */\n");
    buf.push_str("    public void empty() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add Javadoc comment", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_override_method_comment2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class B extends A<Integer> {\n");
    buf.push_str("    public void foo(Integer x) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    buf.push_str("class A<T extends Number> {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param x\n");
    buf.push_str("     */\n");
    buf.push_str("    public void foo(T x) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "B.java", &buf);
    t.assert_code_action_not_exists(&cu, "Add Javadoc for 'foo'");
}

#[test]
fn test_all_javadoc_present() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * Some comment\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("   /**\n");
    buf.push_str("    * Some comment2\n");
    buf.push_str("    */\n");
    buf.push_str("    public void empty() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let r = json!({"start": {"line": 8, "character": 18}, "end": {"line": 8, "character": 18}});
    t.assert_code_action_not_exists_range(&cu, r, "Add Javadoc comment");
}

#[test]
fn test_missing_constructor_comment() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public E(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param a\n");
    buf.push_str("     * @throws IOException\n");
    buf.push_str("     */\n");
    buf.push_str("    public E(int a) throws IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add Javadoc comment", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_type_comment() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E<A, B> {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" * E\n");
    buf.push_str(" * @param <A>\n");
    buf.push_str(" * @param <B>\n");
    buf.push_str(" */\n");
    buf.push_str("public class E<A, B> {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add Javadoc comment", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_missing_field_comment() {
    let (mut t, root) = setup();
    // The upstream test re-applies the unchanged project options.
    t.set_ignored_commands(&["Extract.*"]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static final int COLOR= 1;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("/**\n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("    /**\n");
    buf.push_str("     *\n");
    buf.push_str("     */\n");
    buf.push_str("    public static final int COLOR= 1;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add Javadoc comment", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_qualification1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class A {\n");
    buf.push_str("    public static class B {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "pack", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack2;\n");
    buf.push_str("\n");
    buf.push_str("import pack.A;\n");
    buf.push_str("\n");
    buf.push_str("/**\n");
    buf.push_str(" * {@link A.B} \n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack2", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack2;\n");
    buf.push_str("\n");
    buf.push_str("import pack.A;\n");
    buf.push_str("\n");
    buf.push_str("/**\n");
    buf.push_str(" * {@link pack.A.B} \n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Qualify inner type name", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_invalid_qualification2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class A {\n");
    buf.push_str("    public interface B {\n");
    buf.push_str("        void foo();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "pack", "A.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack2;\n");
    buf.push_str("\n");
    buf.push_str("import pack.A;\n");
    buf.push_str("\n");
    buf.push_str("/**\n");
    buf.push_str(" * {@link A.B#foo()} \n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack2", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack2;\n");
    buf.push_str("\n");
    buf.push_str("import pack.A;\n");
    buf.push_str("\n");
    buf.push_str("/**\n");
    buf.push_str(" * {@link pack.A.B#foo()} \n");
    buf.push_str(" */\n");
    buf.push_str("public class E {\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Qualify inner type name", &buf);
    t.assert_code_actions(&cu, &[e1]);
}
