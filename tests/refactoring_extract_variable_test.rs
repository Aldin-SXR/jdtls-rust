//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.ExtractVariableTest`.

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

const REFACTOR_EXTRACT_VARIABLE: &str = "refactor.extract.variable";
const REFACTOR_EXTRACT_CONSTANT: &str = "refactor.extract.constant";

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    t.set_ignored_commands(&["Extract to method"]);
    (t, root)
}

#[test]
#[ignore = "ExtractTemp/ExtractConstant ported in src/refactoring but not yet wired into the code action pipeline"]
fn test_extract_variable() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to local variable (replace all occurrences)", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int j = 0;\n");
    buf.push_str("        int x= /*]*/j/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e2 = Expected::with_kind("Extract to local variable", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	private static final int _0 = /*]*/0/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		int x= _0;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e3 = Expected::with_kind("Extract to constant", &buf, REFACTOR_EXTRACT_CONSTANT);

    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
#[ignore = "ExtractTemp/ExtractConstant ported in src/refactoring but not yet wired into the code action pipeline"]
fn test_extract_variable1() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public void foo() {\n");
    buf.push_str("		ArrayList<? extends Number> nl= new ArrayList<Integer>();\n");
    buf.push_str("		Number n= nl.get(/*]*/0/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public void foo() {\n");
    buf.push_str("		ArrayList<? extends Number> nl= new ArrayList<Integer>();\n");
    buf.push_str("		int i = 0;\n");
    buf.push_str("        Number n= nl.get(/*]*/i/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to local variable (replace all occurrences)", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("	public void foo() {\n");
    buf.push_str("		ArrayList<? extends Number> nl= new ArrayList<Integer>();\n");
    buf.push_str("		int i = 0;\n");
    buf.push_str("        Number n= nl.get(/*]*/i/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e2 = Expected::with_kind("Extract to local variable", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("	private static final int _0 = /*]*/0/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("		ArrayList<? extends Number> nl= new ArrayList<Integer>();\n");
    buf.push_str("		Number n= nl.get(_0);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e3 = Expected::with_kind("Extract to constant", &buf, REFACTOR_EXTRACT_CONSTANT);

    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
#[ignore = "ExtractTemp/ExtractConstant ported in src/refactoring but not yet wired into the code action pipeline"]
fn test_extract_variable2() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	void f(){\n");
    buf.push_str("		try{\n");
    buf.push_str("			int j=0 +0;\n");
    buf.push_str("		} finally {\n");
    buf.push_str("			int j=/*]*/0/*[*/ +0;\n");
    buf.push_str("		}\n");
    buf.push_str("	}	\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	void f(){\n");
    buf.push_str("		int i = 0;\n");
    buf.push_str("        try{\n");
    buf.push_str("			int j=i +i;\n");
    buf.push_str("		} finally {\n");
    buf.push_str("			int j=/*]*/i/*[*/ +i;\n");
    buf.push_str("		}\n");
    buf.push_str("	}	\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to local variable (replace all occurrences)", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	void f(){\n");
    buf.push_str("		try{\n");
    buf.push_str("			int j=0 +0;\n");
    buf.push_str("		} finally {\n");
    buf.push_str("			int i = 0;\n");
    buf.push_str("            int j=/*]*/i/*[*/ +0;\n");
    buf.push_str("		}\n");
    buf.push_str("	}	\n");
    buf.push_str("}\n");
    let e2 = Expected::with_kind("Extract to local variable", &buf, REFACTOR_EXTRACT_VARIABLE);

    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private static final int _0 = 0/*[*/;\n");
    buf.push_str("\n");
    buf.push_str("    void f(){\n");
    buf.push_str("		try{\n");
    buf.push_str("			int j=0 +0;\n");
    buf.push_str("		} finally {\n");
    buf.push_str("			int j=/*]*/_0 +0;\n");
    buf.push_str("		}\n");
    buf.push_str("	}	\n");
    buf.push_str("}\n");
    let e3 = Expected::with_kind("Extract to constant", &buf, REFACTOR_EXTRACT_CONSTANT);

    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_extract_variable_failed() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class A{\n");
    buf.push_str("	int m(int y){\n");
    buf.push_str("		int y= m(/*]*/y/*[*/);\n");
    buf.push_str("	};\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);

    t.assert_code_actions(&cu, &[]);
}
