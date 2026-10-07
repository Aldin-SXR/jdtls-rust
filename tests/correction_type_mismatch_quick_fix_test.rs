//! Port of `org.eclipse.jdt.ls.core.internal.correction.TypeMismatchQuickFixTest`.

mod common;

use std::path::PathBuf;

use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};

/// `setup()`: `TestOptions.getDefaultOptions()` plus 99 preserved empty
/// lines, static access receiver = error and unchecked type operation =
/// ignore; `extra` are a test's `fJProject1.setOptions(tempOptions)`.
fn setup(extra: &[(&str, &str)]) -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.formatter.number_of_empty_lines_to_preserve".into(), "99".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.staticAccessReceiver".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.uncheckedTypeOperation".into(), "ignore".into());
    for (k, v) in extra {
        options.insert((*k).into(), (*v).into());
    }
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

#[test]
fn test_type_mismatch_in_var_decl() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        Thread th= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        Thread th= (Thread) o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'Thread'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Object o) {\n");
    buf.push_str("        Object th= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'th' to 'Object'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Thread o) {\n");
    buf.push_str("        Thread th= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change type of 'o' to 'Thread'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_mismatch_in_var_decl2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public List[] getLists() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Container.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container c) {\n");
    buf.push_str("         ArrayList[] lists= c.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container c) {\n");
    buf.push_str("         ArrayList[] lists= (ArrayList[]) c.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'ArrayList[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container c) {\n");
    buf.push_str("         List[] lists= c.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'lists' to 'List[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public ArrayList[] getLists() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'getLists(..)' to 'ArrayList[]'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_mismatch_in_var_decl3() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        Thread th= foo();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Thread foo() {\n");
    buf.push_str("        Thread th= foo();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'foo(..)' to 'Thread'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_in_var_decl4() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public List getLists()[] {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Container.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E extends Container {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         ArrayList[] lists= super.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E extends Container {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         ArrayList[] lists= (ArrayList[]) super.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'ArrayList[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class E extends Container {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         List[] lists= super.getLists();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'lists' to 'List[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public ArrayList[] getLists() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'getLists(..)' to 'ArrayList[]'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_mismatch_for_interface1() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test0;\n");
    buf.push_str("public interface PrimaryContainer {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test0", "PrimaryContainer.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public static Container getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Container.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         PrimaryContainer list= Container.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         PrimaryContainer list= (PrimaryContainer) Container.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'PrimaryContainer'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("         Container list= Container.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'list' to 'Container'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("\n");
    buf.push_str("public class Container {\n");
    buf.push_str("    public static PrimaryContainer getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'getContainer(..)' to 'PrimaryContainer'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("\n");
    buf.push_str("public class Container implements PrimaryContainer {\n");
    buf.push_str("    public static Container getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e4 = Expected::new("Let 'Container' implement 'PrimaryContainer'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3, e4]);
}

#[test]
fn test_type_mismatch_for_interface2() {
    let (mut t, root) = setup(&[]);
    let primary_container_code = "package test0;\npublic interface PrimaryContainer {\n    PrimaryContainer duplicate(PrimaryContainer container);\n}\n";
    t.ws.create_cu(&root, "src", "test0", "PrimaryContainer.java", primary_container_code);
    let container_code = "package test1;\npublic class Container {\n    public static Container getContainer() {\n        return null;\n    }\n}\n";
    t.ws.create_cu(&root, "src", "test1", "Container.java", container_code);
    let e_code = "package test1;\nimport test0.PrimaryContainer;\npublic class E {\n    public void foo(PrimaryContainer primary) {\n         primary.duplicate(Container.getContainer());\n    }\n}\n";
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", e_code);
    let e1_code = "package test1;\nimport test0.PrimaryContainer;\npublic class E {\n    public void foo(PrimaryContainer primary) {\n         primary.duplicate((PrimaryContainer) Container.getContainer());\n    }\n}\n";
    let e1 = Expected::new("Cast argument 1 to 'PrimaryContainer'", e1_code);
    let e2_code = "package test1;\n\nimport test0.PrimaryContainer;\n\npublic class Container {\n    public static PrimaryContainer getContainer() {\n        return null;\n    }\n}\n";
    let e2 = Expected::new("Change return type of 'getContainer(..)' to 'PrimaryContainer'", e2_code);
    let e3_code = "package test1;\n\nimport test0.PrimaryContainer;\n\npublic class Container implements PrimaryContainer {\n    public static Container getContainer() {\n        return null;\n    }\n}\n";
    let e3 = Expected::new("Let 'Container' implement 'PrimaryContainer'", e3_code);
    let e4_code = "package test0;\n\nimport test1.Container;\n\npublic interface PrimaryContainer {\n    PrimaryContainer duplicate(Container container);\n}\n";
    let e4 = Expected::new("Change method 'duplicate(PrimaryContainer)' to 'duplicate(Container)'", e4_code);
    let e5_code = "package test0;\n\nimport test1.Container;\n\npublic interface PrimaryContainer {\n    PrimaryContainer duplicate(PrimaryContainer container);\n\n    void duplicate(Container container);\n}\n";
    let e5 = Expected::new("Create method 'duplicate(Container)' in type 'PrimaryContainer'", e5_code);
    t.assert_code_actions(&cu, &[e1, e2, e3, e4, e5]);
}

#[test]
fn test_type_mismatch_for_interface_in_generic() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test0;\n");
    buf.push_str("public interface PrimaryContainer<A> {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test0", "PrimaryContainer.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Container<A> {\n");
    buf.push_str("    public Container<A> getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Container.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<String> c) {\n");
    buf.push_str("         PrimaryContainer<String> list= c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<String> c) {\n");
    buf.push_str("         PrimaryContainer<String> list= (PrimaryContainer<String>) c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'PrimaryContainer<String>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<String> c) {\n");
    buf.push_str("         Container<String> list= c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'list' to 'Container<String>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("\n");
    buf.push_str("public class Container<A> {\n");
    buf.push_str("    public PrimaryContainer<String> getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'getContainer(..)' to 'PrimaryContainer<String>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("\n");
    buf.push_str("public class Container<A> implements PrimaryContainer<String> {\n");
    buf.push_str("    public Container<A> getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e4 = Expected::new("Let 'Container' implement 'PrimaryContainer'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3, e4]);
}

#[test]
fn test_type_mismatch_for_interface_in_generic2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test0;\n");
    buf.push_str("public interface PrimaryContainer<A> {\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test0", "PrimaryContainer.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Container<A> {\n");
    buf.push_str("    public Container<A> getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Container.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<List<?>> c) {\n");
    buf.push_str("         PrimaryContainer<?> list= c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<List<?>> c) {\n");
    buf.push_str("         PrimaryContainer<?> list= (PrimaryContainer<?>) c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'PrimaryContainer<?>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Container<List<?>> c) {\n");
    buf.push_str("         Container<List<?>> list= c.getContainer();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'list' to 'Container<List<?>>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import test0.PrimaryContainer;\n");
    buf.push_str("\n");
    buf.push_str("public class Container<A> {\n");
    buf.push_str("    public PrimaryContainer<?> getContainer() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'getContainer(..)' to 'PrimaryContainer<?>'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
#[ignore = "needs Java50Fix raw type reference fix (InferTypeArguments constraint solver), not ported"]
fn test_type_mismatch_for_parameterized_type() {
    let (mut t, root) = setup(&[("org.eclipse.jdt.core.compiler.problem.uncheckedTypeOperation", "warning"), ("org.eclipse.jdt.core.compiler.problem.rawTypeReference", "warning")]);
    let e_code = "package test1;\nimport java.util.*;\npublic class E {\n    public void foo() {\n        List list= new ArrayList<Integer>();\n    }\n}\n";
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", e_code);
    let e1_code = "package test1;\nimport java.util.*;\npublic class E {\n    public void foo() {\n        List<Integer> list= new ArrayList<Integer>();\n    }\n}\n";
    let e1 = Expected::new("Add type arguments to 'List'", e1_code);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_type_mismatch_for_parameterized_type2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.*;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<Integer> list= new ArrayList<Number>();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.*;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<Number> list= new ArrayList<Number>();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'list' to 'List<Number>'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_in_field_decl() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    int time= System.currentTimeMillis();\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    int time= (int) System.currentTimeMillis();\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'int'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    long time= System.currentTimeMillis();\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'time' to 'long'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_in_field_decl_no_import() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private class StringBuilder { }\n");
    buf.push_str("    private final StringBuilder sb;\n");
    buf.push_str("    public E() {\n");
    buf.push_str("        sb= new java.lang.StringBuilder();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private class StringBuilder { }\n");
    buf.push_str("    private final java.lang.StringBuilder sb;\n");
    buf.push_str("    public E() {\n");
    buf.push_str("        sb= new java.lang.StringBuilder();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'sb' to 'StringBuilder'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    private class StringBuilder { }\n");
    buf.push_str("    private final StringBuilder sb;\n");
    buf.push_str("    public E() {\n");
    buf.push_str("        sb= new StringBuilder();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change 'StringBuilder' to compatible type", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_in_assignment() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        String str;\n");
    buf.push_str("        str= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        String str;\n");
    buf.push_str("        str= (String) iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'String'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        Object str;\n");
    buf.push_str("        str= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'str' to 'Object'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_in_assignment2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        String str, str2;\n");
    buf.push_str("        str= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        String str, str2;\n");
    buf.push_str("        str= (String) iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'String'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        Object str;\n");
    buf.push_str("        String str2;\n");
    buf.push_str("        str= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'str' to 'Object'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_in_assignment3() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    A, B;\n");
    buf.push_str("    String str, str2;\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        str2= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    A, B;\n");
    buf.push_str("    String str, str2;\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        str2= (String) iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'String'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Iterator;\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    A, B;\n");
    buf.push_str("    String str;\n");
    buf.push_str("    Object str2;\n");
    buf.push_str("    public void foo(Iterator iter) {\n");
    buf.push_str("        str2= iter.next();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'str2' to 'Object'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_in_expression() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test0;\n");
    buf.push_str("public class Other {\n");
    buf.push_str("    public Object[] toArray() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test0", "Other.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.Other;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public String[] foo(Other other) {\n");
    buf.push_str("        return other.toArray();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.Other;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public String[] foo(Other other) {\n");
    buf.push_str("        return (String[]) other.toArray();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'String[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import test0.Other;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object[] foo(Other other) {\n");
    buf.push_str("        return other.toArray();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change method return type to 'Object[]'", &buf);
    let mut buf = String::new();
    buf.push_str("package test0;\n");
    buf.push_str("public class Other {\n");
    buf.push_str("    public String[] toArray() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change return type of 'toArray(..)' to 'String[]'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_cast_on_cast_expression() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(List list) {\n");
    buf.push_str("        ArrayList a= (Cloneable) list;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(List list) {\n");
    buf.push_str("        ArrayList a= (ArrayList) list;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change cast to 'ArrayList'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(List list) {\n");
    buf.push_str("        Cloneable a= (Cloneable) list;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'a' to 'Cloneable'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_return_type1() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    public String getName() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    public char[] getName() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    public String getName() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getName(..)' to 'String'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    public char[] getName() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change return type of overridden 'getName(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_return_type2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public interface IBase {\n");
    buf.push_str("    List getCollection();\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "IBase.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E implements IBase {\n");
    buf.push_str("    public String[] getCollection() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E implements IBase {\n");
    buf.push_str("    public List getCollection() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getCollection(..)' to 'List<E>'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("public interface IBase {\n");
    buf.push_str("    String[] getCollection();\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change return type of implemented 'getCollection(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_return_type_on_generic() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base<T extends Number> {\n");
    buf.push_str("    public String getName(T... t) {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends Base<Integer> {\n");
    buf.push_str("    public char[] getName(Integer... i) {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E extends Base<Integer> {\n");
    buf.push_str("    public String getName(Integer... i) {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getName(..)' to 'String'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base<T extends Number> {\n");
    buf.push_str("    public char[] getName(T... t) {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change return type of overridden 'getName(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_return_type_on_generic2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    public Number getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E<T> extends Base {\n");
    buf.push_str("    public T getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E<T> extends Base {\n");
    buf.push_str("    public Number getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getVal(..)' to 'Number'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_mismatching_return_type_on_generic_method() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.lang.annotation.Annotation;\n");
    buf.push_str("import java.lang.reflect.AccessibleObject;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void m() {\n");
    buf.push_str("        new AccessibleObject() {\n");
    buf.push_str("            public <T extends Annotation> void getAnnotation(Class<T> annotationClass) {\n");
    buf.push_str("            }\n");
    buf.push_str("        };\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.lang.annotation.Annotation;\n");
    buf.push_str("import java.lang.reflect.AccessibleObject;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    void m() {\n");
    buf.push_str("        new AccessibleObject() {\n");
    buf.push_str("            public <T extends Annotation> T getAnnotation(Class<T> annotationClass) {\n");
    buf.push_str("            }\n");
    buf.push_str("        };\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getAnnotation(..)' to 'T'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_mismatching_return_type_parameterized() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    public Number getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E<T> extends Base {\n");
    buf.push_str("    public E<T> getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E<T> extends Base {\n");
    buf.push_str("    public Number getVal() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'getVal(..)' to 'Number'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_mismatching_return_type_on_wildcard_extends() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Integer getIt(ArrayList<? extends Number> b) {\n");
    buf.push_str("        return b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Integer getIt(ArrayList<? extends Number> b) {\n");
    buf.push_str("        return (Integer) b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'Integer'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Number getIt(ArrayList<? extends Number> b) {\n");
    buf.push_str("        return b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change method return type to 'Number'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_return_type_on_wildcard_super() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Integer getIt(ArrayList<? super Number> b) {\n");
    buf.push_str("        return b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Integer getIt(ArrayList<? super Number> b) {\n");
    buf.push_str("        return (Integer) b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'Integer'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.ArrayList;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public Object getIt(ArrayList<? super Number> b) {\n");
    buf.push_str("        return b.get(0);\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change method return type to 'Object'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_exceptions1() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public interface IBase {\n");
    buf.push_str("    String[] getValues();\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "IBase.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("public class E implements IBase {\n");
    buf.push_str("    public String[] getValues() throws IOException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E implements IBase {\n");
    buf.push_str("    public String[] getValues() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove exceptions from 'getValues(..)'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("public interface IBase {\n");
    buf.push_str("    String[] getValues() throws IOException;\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add exceptions to 'IBase.getValues(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_exceptions2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    String[] getValues() throws IOException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.EOFException;\n");
    buf.push_str("import java.text.ParseException;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    public String[] getValues() throws EOFException, ParseException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.EOFException;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    public String[] getValues() throws EOFException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove exceptions from 'getValues(..)'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("import java.text.ParseException;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    String[] getValues() throws IOException, ParseException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add exceptions to 'Base.getValues(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_exceptions3() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param i The parameter\n");
    buf.push_str("     *                  More about the parameter\n");
    buf.push_str("     * @return The returned argument\n");
    buf.push_str("     * @throws IOException IO problems\n");
    buf.push_str("     * @since 3.0\n");
    buf.push_str("     */\n");
    buf.push_str("    String[] getValues(int i) throws IOException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "Base.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.EOFException;\n");
    buf.push_str("import java.text.ParseException;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param i The parameter\n");
    buf.push_str("     *                  More about the parameter\n");
    buf.push_str("     * @return The returned argument\n");
    buf.push_str("     * @throws EOFException EOF problems\n");
    buf.push_str("     * @throws ParseException Parse problems\n");
    buf.push_str("     */\n");
    buf.push_str("    public String[] getValues(int i) throws EOFException, ParseException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.EOFException;\n");
    buf.push_str("public class E extends Base {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param i The parameter\n");
    buf.push_str("     *                  More about the parameter\n");
    buf.push_str("     * @return The returned argument\n");
    buf.push_str("     * @throws EOFException EOF problems\n");
    buf.push_str("     */\n");
    buf.push_str("    public String[] getValues(int i) throws EOFException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove exceptions from 'getValues(..)'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("import java.text.ParseException;\n");
    buf.push_str("public class Base {\n");
    buf.push_str("    /**\n");
    buf.push_str("     * @param i The parameter\n");
    buf.push_str("     *                  More about the parameter\n");
    buf.push_str("     * @return The returned argument\n");
    buf.push_str("     * @throws IOException IO problems\n");
    buf.push_str("     * @throws ParseException \n");
    buf.push_str("     * @since 3.0\n");
    buf.push_str("     */\n");
    buf.push_str("    String[] getValues(int i) throws IOException, ParseException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add exceptions to 'Base.getValues(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_exceptions_on_generic() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public interface IBase<T> {\n");
    buf.push_str("    T[] getValues();\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "test1", "IBase.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("public class E implements IBase<String> {\n");
    buf.push_str("    public String[] getValues() throws IOException {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("public class E implements IBase<String> {\n");
    buf.push_str("    public String[] getValues() {\n");
    buf.push_str("        return null;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove exceptions from 'getValues(..)'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("\n");
    buf.push_str("import java.io.IOException;\n");
    buf.push_str("\n");
    buf.push_str("public interface IBase<T> {\n");
    buf.push_str("    T[] getValues() throws IOException;\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Add exceptions to 'IBase<String>.getValues(..)'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_mismatching_exceptions_on_binary_parent() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E implements Runnable {\n");
    buf.push_str("    public void run() throws ClassNotFoundException {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E implements Runnable {\n");
    buf.push_str("    public void run() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Remove exceptions from 'run(..)'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_in_annotation_values1() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        String newAttrib();\n");
    buf.push_str("    }\n");
    buf.push_str("    @Annot(newAttrib= 1)\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        int newAttrib();\n");
    buf.push_str("    }\n");
    buf.push_str("    @Annot(newAttrib= 1)\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'newAttrib(..)' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_in_annotation_values2() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class Other<T> {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        String newAttrib();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    t.ws.create_cu(&root, "src", "pack", "Other.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    @Other.Annot(newAttrib= 1)\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class Other<T> {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        int newAttrib();\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'newAttrib(..)' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_in_single_member_annotation() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        String value();\n");
    buf.push_str("    }\n");
    buf.push_str("    @Annot(1)\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public @interface Annot {\n");
    buf.push_str("        int value();\n");
    buf.push_str("    }\n");
    buf.push_str("    @Annot(1)\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change return type of 'value(..)' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_with_enum_constant() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    ONE;\n");
    buf.push_str("    int m(int i) {\n");
    buf.push_str("            return ONE;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public enum E {\n");
    buf.push_str("    ONE;\n");
    buf.push_str("    E m(int i) {\n");
    buf.push_str("            return ONE;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change method return type to 'E'", &buf);
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
fn test_type_mismatch_with_array_length() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class TestShort {\n");
    buf.push_str("        public static void main(String[] args) {\n");
    buf.push_str("                short test=args.length;\n");
    buf.push_str("        }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "TestShort.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class TestShort {\n");
    buf.push_str("        public static void main(String[] args) {\n");
    buf.push_str("                short test=(short) args.length;\n");
    buf.push_str("        }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'short'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("public class TestShort {\n");
    buf.push_str("        public static void main(String[] args) {\n");
    buf.push_str("                int test=args.length;\n");
    buf.push_str("        }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'test' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "needs folder-based type lookup: test2/E.java declares package test1, which JDT still resolves as test2.E through its package fragment; the bridge name environment resolves types by declared package"]
fn test_type_mismatch_with_type_in_same_package() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {}\n");
    t.ws.create_cu(&root, "src", "test2", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class E {}\n");
    t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    test2.E e2= new Object();\n");
    buf.push_str("    E e1;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "Test.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    test2.E e2= (test2.E) new Object();\n");
    buf.push_str("    E e1;\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'E'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    Object e2= new Object();\n");
    buf.push_str("    E e1;\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'e2' to 'Object'", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("public class Test {\n");
    buf.push_str("    test2.E e2= new test2.E();\n");
    buf.push_str("    E e1;\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change 'Object' to compatible type", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_mismatch_in_for_each_proposals_list() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<String> l= null;    \n");
    buf.push_str("        for (Number e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<String> l= null;    \n");
    buf.push_str("        for (String e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'e' to 'String'", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_type_mismatch_in_for_each_proposals_list_extends() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<? extends String> l= null;    \n");
    buf.push_str("        for (Number e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<? extends String> l= null;    \n");
    buf.push_str("        for (String e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'e' to 'String'", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_type_mismatch_in_for_each_proposals_list_super() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<? super String> l= null;    \n");
    buf.push_str("        for (Number e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        List<? super String> l= null;    \n");
    buf.push_str("        for (Object e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'e' to 'Object'", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_type_mismatch_in_for_each_proposals_arrays() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        String[] l= null;\n");
    buf.push_str("        for (Number e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("import java.util.List;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        String[] l= null;\n");
    buf.push_str("        for (String e : l) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Change type of 'e' to 'String'", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_type_mismatch_in_for_each_missing_type() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(String[] strings) {\n");
    buf.push_str("        for (s: strings) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(String[] strings) {\n");
    buf.push_str("        for (String s: strings) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Create loop variable 's'", &buf);
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_null_check() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String arg) {\n");
    buf.push_str("        while (arg) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    buf.push_str("\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(boolean arg) {\n");
    buf.push_str("        while (arg) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    buf.push_str("\n");
    let e1 = Expected::new("Change type of 'arg' to 'boolean'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public static void main(String arg) {\n");
    buf.push_str("        while (arg != null) {\n");
    buf.push_str("        }\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    buf.push_str("\n");
    let e2 = Expected::new("Insert '!= null' check", &buf);
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_type_mismatch_object_and_primitive_type() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        Object o= new Object();\n");
    buf.push_str("        int i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        Object o= new Object();\n");
    buf.push_str("        int i= (int) o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'int'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        Object o= new Object();\n");
    buf.push_str("        Object i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'i' to 'Object'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo() {\n");
    buf.push_str("        int o= new Object();\n");
    buf.push_str("        int i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change type of 'o' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_mismatch_primitive_types() {
    let (mut t, root) = setup(&[]);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(long o) {\n");
    buf.push_str("        int i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(long o) {\n");
    buf.push_str("        int i= (int) o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Add cast to 'int'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(long o) {\n");
    buf.push_str("        long i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e2 = Expected::new("Change type of 'i' to 'long'", &buf);
    let mut buf = String::new();
    buf.push_str("package pack;\n");
    buf.push_str("\n");
    buf.push_str("\n");
    buf.push_str("public class E {\n");
    buf.push_str("    public void foo(int o) {\n");
    buf.push_str("        int i= o;\n");
    buf.push_str("    }\n");
    buf.push_str("}\n");
    let e3 = Expected::new("Change type of 'o' to 'int'", &buf);
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

