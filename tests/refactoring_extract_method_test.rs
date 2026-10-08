//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.ExtractMethodTest`.

mod common;

use std::path::{Path, PathBuf};

use common::jdtls::{range, test_default_options};
use common::quickfix::{Expected, QuickFixTest};
use serde_json::Value;

/// `JavaCodeActionKind.REFACTOR_EXTRACT_METHOD`.
const REFACTOR_EXTRACT_METHOD: &str = "refactor.extract.function";
/// `JavaCodeActionKind.QUICK_ASSIST`.
const QUICK_ASSIST: &str = "quickassist";

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor", "quickfix"]);
    (t, root)
}

/// `options = fJProject1.getOptions(true); JavaCore.setComplianceOptions(version, options);
/// fJProject1.setOptions(options)`.
fn set_compliance_options(t: &mut QuickFixTest, root: &Path, version: &str) {
    let mut options = test_default_options();
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), version.to_owned());
    }
    options.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    t.ws.set_project_options(root, &options);
}

#[test]
fn test_extract_method_branch() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public volatile boolean flag;\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        /*[*/for (int i = 0; i < 10; i++) {\n");
    buf.push_str("            if (flag)\n");
    buf.push_str("                continue;\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public volatile boolean flag;\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        extracted();\n");
    buf.push_str("    }\n");
    buf.push_str("    private void extracted() {\n");
    buf.push_str("        /*[*/for (int i = 0; i < 10; i++) {\n");
    buf.push_str("            if (flag)\n");
    buf.push_str("                continue;\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_exception() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        try {\n");
    buf.push_str("            /*[*/g();/*]*/\n");
    buf.push_str("        } catch (java.io.IOException e) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("    public void g() throws java.io.IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        try {\n");
    buf.push_str("            extracted();\n");
    buf.push_str("        } catch (java.io.IOException e) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("    private void extracted() throws IOException {\n");
    buf.push_str("        /*[*/g();/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("    public void g() throws java.io.IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_exception1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        /*[*/try {\n");
    buf.push_str("            g();\n");
    buf.push_str("        } catch (java.io.IOException e) {\n");
    buf.push_str("        } /*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void g() throws java.io.IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        extracted();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void extracted() {\n");
    buf.push_str("        /*[*/try {\n");
    buf.push_str("            g();\n");
    buf.push_str("        } catch (java.io.IOException e) {\n");
    buf.push_str("        } /*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void g() throws java.io.IOException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_return() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public interface I {\n");
    buf.push_str("        public boolean run();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        /*[*/bar(this, new I() {\n");
    buf.push_str("            public boolean run() {\n");
    buf.push_str("                return true;\n");
    buf.push_str("            }\n");
    buf.push_str("        });/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void bar(E a, I i) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public interface I {\n");
    buf.push_str("        public boolean run();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        extracted();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void extracted() {\n");
    buf.push_str("        /*[*/bar(this, new I() {\n");
    buf.push_str("            public boolean run() {\n");
    buf.push_str("                return true;\n");
    buf.push_str("            }\n");
    buf.push_str("        });/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void bar(E a, I i) {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_parameter() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        String x = \"x\";\n");
    buf.push_str("        /*[*/String y = \"a\" + x;\n");
    buf.push_str("        System.out.println(x);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        String x = \"x\";\n");
    buf.push_str("        getY(x);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void getY(String x) {\n");
    buf.push_str("        /*[*/String y = \"a\" + x;\n");
    buf.push_str("        System.out.println(x);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_local() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo(boolean b1, boolean b2) {\n");
    buf.push_str("        int n = 0;\n");
    buf.push_str("        int i = 0;\n");
    buf.push_str("        /*[*/\n");
    buf.push_str("        if (b1)\n");
    buf.push_str("            i = 1;\n");
    buf.push_str("        if (b2)\n");
    buf.push_str("            n = n + i;\n");
    buf.push_str("        /*]*/\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public int foo(boolean b1, boolean b2) {\n");
    buf.push_str("        int n = 0;\n");
    buf.push_str("        int i = 0;\n");
    buf.push_str("        n = extracted(b1, b2, n, i);\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private int extracted(boolean b1, boolean b2, int n, int i) {\n");
    buf.push_str("        /*[*/\n");
    buf.push_str("        if (b1)\n");
    buf.push_str("            i = 1;\n");
    buf.push_str("        if (b2)\n");
    buf.push_str("            n = n + i;\n");
    buf.push_str("        /*]*/\n");
    buf.push_str("        return n;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_lambda_expression() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("@FunctionalInterface\n");
    buf.push_str("interface E {\n");
    buf.push_str("    int foo(int i) throws IOException;\n");
    buf.push_str("\n");
    buf.push_str("    default E method(E i1) throws InterruptedException {\n");
    buf.push_str("        /*[*/if (i1 == null)\n");
    buf.push_str("            throw new InterruptedException();\n");
    buf.push_str("        return x -> {\n");
    buf.push_str("            throw new IOException();\n");
    buf.push_str("        };/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("@FunctionalInterface\n");
    buf.push_str("interface E {\n");
    buf.push_str("    int foo(int i) throws IOException;\n");
    buf.push_str("\n");
    buf.push_str("    default E method(E i1) throws InterruptedException {\n");
    buf.push_str("        return extracted(i1);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    default E extracted(E i1) throws InterruptedException {\n");
    buf.push_str("        /*[*/if (i1 == null)\n");
    buf.push_str("            throw new InterruptedException();\n");
    buf.push_str("        return x -> {\n");
    buf.push_str("            throw new IOException();\n");
    buf.push_str("        };/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_lambda_body_to_method() {
    let (mut t, root) = setup();
    t.set_only(&[QUICK_ASSIST]);
    let contents = concat!("package test1;\r\n", "interface F1 {\r\n", "    int foo1(int a);\r\n", "}\r\n", "public class E {\r\n", "    public void foo(int a) {\r\n", "        F1 k = (e) -> {\r\n", "            int x = e + 3;\r\n", "            if (x > 3) {\r\n", "                return a;\r\n", "            }\r\n", "            return x;\r\n", "        };\r\n", "        k.foo1(4);\r\n", "    }\r\n", "}");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", contents);
    let expected = concat!("package test1;\r\n", "interface F1 {\r\n", "    int foo1(int a);\r\n", "}\r\n", "public class E {\r\n", "    public void foo(int a) {\r\n", "        F1 k = (e) -> extracted(a, e);\r\n", "        k.foo1(4);\r\n", "    }\r\n", "\r\n", "    private int extracted(int a, int e) {\r\n", "        int x = e + 3;\r\n", "        if (x > 3) {\r\n", "            return a;\r\n", "        }\r\n", "        return x;\r\n", "    }\r\n", "}");
    let range = range(7, 26, 7, 26);
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    let e1 = Expected::with_kind("Extract lambda body to method", expected, QUICK_ASSIST);
    t.assert_code_actions_list(&code_actions, &[e1]);
}

// https://github.com/redhat-developer/vscode-java/issues/2370
#[test]
#[ignore = "counts 5 refactor actions; \"Surround with try/catch\" (RefactorProcessor.getSurroundWithTryCatchProposal) is not ported yet"]
fn test_extract_method_in_static_block() {
    let (mut t, root) = setup();
    t.set_only(&["refactor"]);
    let contents = concat!("package test1;\r\n", "public class E {\r\n", "    public static String STR;\r\n", "    static {\r\n", "        STR = new String(\"test\").strip();\r\n", "    }\r\n", "}");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", contents);
    let expected = concat!("package test1;\r\n", "public class E {\r\n", "    public static String STR;\r\n", "    static {\r\n", "        STR = extracted().strip();\r\n", "    }\r\n", "    private static String extracted() {\r\n", "        return new String(\"test\");\r\n", "    }\r\n", "}");
    let range = range(4, 14, 4, 32);
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    assert_eq!(5, code_actions.len());
    let extract_method: Vec<Value> = code_actions.iter().filter(|c| c["title"] == "Extract to method").cloned().collect();
    let e1 = Expected::with_kind("Extract to method", expected, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions_list(&extract_method, &[e1]);
}

#[test]
fn test_extract_method_generic() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public <E> void foo(E param) {\n");
    buf.push_str("        /*[*/List<E> list = new ArrayList<E>();\n");
    buf.push_str("        foo(param);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public <E> void foo(E param) {\n");
    buf.push_str("        getList(param);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private <E> void getList(E param) {\n");
    buf.push_str("        /*[*/List<E> list = new ArrayList<E>();\n");
    buf.push_str("        foo(param);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_generic1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    <T extends Comparable<? super T>> void method(List<T> list) {\n");
    buf.push_str("        /*[*/toExtract(list);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    static <T extends Comparable<? super T>> void toExtract(List<T> list) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    <T extends Comparable<? super T>> void method(List<T> list) {\n");
    buf.push_str("        extracted(list);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private <T extends Comparable<? super T>> void extracted(List<T> list) {\n");
    buf.push_str("        /*[*/toExtract(list);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    static <T extends Comparable<? super T>> void toExtract(List<T> list) {\n");
    buf.push_str("        return;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_field_initializer() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    String fS = \"foo\";\n");
    buf.push_str("\n");
    buf.push_str("    void m() {\n");
    buf.push_str("        new Thread() {\n");
    buf.push_str("            String fSub = /*]*/fS.substring(1)/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("                System.out.println(fS.substring(1));\n");
    buf.push_str("            };\n");
    buf.push_str("        }.start();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    String fS = \"foo\";\n");
    buf.push_str("\n");
    buf.push_str("    void m() {\n");
    buf.push_str("        new Thread() {\n");
    buf.push_str("            String fSub = /*]*/extracted()/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("            private String extracted() {\n");
    buf.push_str("                return fS.substring(1);\n");
    buf.push_str("            }\n");
    buf.push_str("\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("                System.out.println(extracted());\n");
    buf.push_str("            };\n");
    buf.push_str("        }.start();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_expression() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        int i = 1 - (/*[*/2 + 3/*]*/);\n");
    buf.push_str("        int j = 1 - (2 + 3);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        int i = 1 - extracted();\n");
    buf.push_str("        int j = 1 - extracted();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private int extracted() {\n");
    buf.push_str("        return /*[*/2 + 3/*]*/;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_var() {
    let (mut t, root) = setup();
    set_compliance_options(&mut t, &root, "11");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        var test = new String(\"blah\");\n");
    buf.push_str("        test = test + test;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        var test = new String(\"blah\");\n");
    buf.push_str("        getTest(test);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void getTest(String test) {\n");
    buf.push_str("        test = test + test;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    let range = range(6, 8, 6, 27);
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    t.assert_code_actions_list(&code_actions, &[e1]);
}

#[test]
fn test_extract_method_reference() {
    let (mut t, root) = setup();
    set_compliance_options(&mut t, &root, "11");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.Arrays;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        var test = new String(\"blah\");\n");
    buf.push_str("        List<String> list = new ArrayList<>();\n");
    buf.push_str("        Arrays.asList(\"a\", \"b\").forEach(list::add);\n");
    buf.push_str("        test = test + test;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.Arrays;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        var test = new String(\"blah\");\n");
    buf.push_str("        List<String> list = new ArrayList<>();\n");
    buf.push_str("        extracted(test, list);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void extracted(String test, List<String> list) {\n");
    buf.push_str("        Arrays.asList(\"a\", \"b\").forEach(list::add);\n");
    buf.push_str("        test = test + test;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    let range = range(10, 8, 11, 27);
    let code_actions = t.evaluate_code_actions_range(&cu, range);
    t.assert_code_actions_list(&code_actions, &[e1]);
}

#[test]
fn test_extract_method_enum() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    A;\n");
    buf.push_str("\n");
    buf.push_str("    static {\n");
    buf.push_str("        /*[*/foo();/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    A;\n");
    buf.push_str("\n");
    buf.push_str("    static {\n");
    buf.push_str("        extracted();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static void extracted() {\n");
    buf.push_str("        /*[*/foo();/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_duplicate() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public volatile boolean flag;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        /*[*/do {\n");
    buf.push_str("            if (flag)\n");
    buf.push_str("                continue;\n");
    buf.push_str("        } while (flag);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void extracted() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public volatile boolean flag;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        extracted2();\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private void extracted2() {\n");
    buf.push_str("        /*[*/do {\n");
    buf.push_str("            if (flag)\n");
    buf.push_str("                continue;\n");
    buf.push_str("        } while (flag);/*]*/\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public void extracted() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "needs the \"Convert to lambda expression\" refactor proposal (RefactorProcessor.getConvertAnonymousClassCreationsToLambdaProposals), not ported yet"]
fn test_extract_method_anonymous_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.concurrent.ExecutorService;\n");
    buf.push_str("import java.util.concurrent.Executors;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        ExecutorService executor = Executors.newSingleThreadExecutor();\n");
    buf.push_str("        executor.execute(new Runnable() {\n");
    buf.push_str("            @Override\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("                String inLocalThread = \"SecondThreadValue\";\n");
    buf.push_str("                /*[*/System.out.println(inLocalThread);/*]*/\n");
    buf.push_str("            }\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public static void extracted() {}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.concurrent.ExecutorService;\n");
    buf.push_str("import java.util.concurrent.Executors;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        ExecutorService executor = Executors.newSingleThreadExecutor();\n");
    buf.push_str("        executor.execute(() -> {\n");
    buf.push_str("            String inLocalThread = \"SecondThreadValue\";\n");
    buf.push_str("            /*[*/System.out.println(inLocalThread);/*]*/\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public static void extracted() {}\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Convert to lambda expression", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.concurrent.ExecutorService;\n");
    buf.push_str("import java.util.concurrent.Executors;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        ExecutorService executor = Executors.newSingleThreadExecutor();\n");
    buf.push_str("        executor.execute(new Runnable() {\n");
    buf.push_str("            @Override\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("                String inLocalThread = \"SecondThreadValue\";\n");
    buf.push_str("                extracted(inLocalThread);\n");
    buf.push_str("            }\n");
    buf.push_str("\n");
    buf.push_str("            private void extracted(String inLocalThread) {\n");
    buf.push_str("                /*[*/System.out.println(inLocalThread);/*]*/\n");
    buf.push_str("            }\n");
    buf.push_str("        });\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    public static void extracted() {}\n");
    buf.push_str("}\n");
    let e2 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1, e2]);
}

//https://github.com/redhat-developer/vscode-java/issues/2011
#[test]
fn test_extract_method_infer_name_in_context() {
    let (mut t, root) = setup();
    set_compliance_options(&mut t, &root, "11");
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main() {\n");
    buf.push_str("        var parts = new String[]{\"Hello\", \"Java\", \"16\"};\n");
    buf.push_str("        /*[*/var greeting = new StringBuilder();\n");
    buf.push_str("        for(int i = 0; i < parts.length; i++) {\n");
    buf.push_str("            var part = parts[i];\n");
    buf.push_str("            if (i >0) {\n");
    buf.push_str("                greeting.append(\" \");\n");
    buf.push_str("            }\n");
    buf.push_str("            greeting.append(part);\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("        System.out.println(greeting);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main() {\n");
    buf.push_str("        var parts = new String[]{\"Hello\", \"Java\", \"16\"};\n");
    buf.push_str("        var greeting = getGreeting(parts);\n");
    buf.push_str("        System.out.println(greeting);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static StringBuilder getGreeting(String[] parts) {\n");
    buf.push_str("        /*[*/var greeting = new StringBuilder();\n");
    buf.push_str("        for(int i = 0; i < parts.length; i++) {\n");
    buf.push_str("            var part = parts[i];\n");
    buf.push_str("            if (i >0) {\n");
    buf.push_str("                greeting.append(\" \");\n");
    buf.push_str("            }\n");
    buf.push_str("            greeting.append(part);\n");
    buf.push_str("        }/*]*/\n");
    buf.push_str("        return greeting;\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_infer_name_no_var_selected() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        int numArgs = /*[*/args == null ? 0 : args.length/*]*/;\n");
    buf.push_str("        System.out.println(numArgs);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        int numArgs = getNumArgs(args);\n");
    buf.push_str("        System.out.println(numArgs);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static int getNumArgs(String[] args) {\n");
    buf.push_str("        return /*[*/args == null ? 0 : args.length/*]*/;\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_extract_method_infer_name_no_var_selected_assign() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        int numArgs;\n");
    buf.push_str("        numArgs = /*[*/args == null ? 0 : args.length/*]*/;\n");
    buf.push_str("        System.out.println(numArgs);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String[] args) {\n");
    buf.push_str("        int numArgs;\n");
    buf.push_str("        numArgs = getNumArgs(args);\n");
    buf.push_str("        System.out.println(numArgs);\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("    private static int getNumArgs(String[] args) {\n");
    buf.push_str("        return /*[*/args == null ? 0 : args.length/*]*/;\n");
    buf.push_str("    }\n");
    buf.push_str("\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to method", &buf, REFACTOR_EXTRACT_METHOD);
    t.assert_code_actions(&cu, &[e1]);
}
