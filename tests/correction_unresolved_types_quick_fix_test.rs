//! Port of `org.eclipse.jdt.ls.core.internal.correction.UnresolvedTypesQuickFixTest`.
#![allow(unused_variables, unused_mut)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::jdtls::{range, test_default_options};
use common::quickfix::{get_title, Expected, QuickFixTest};
use serde_json::json;

/// `setup`: `newEmptyProject()` with `TestOptions.getDefaultOptions()` plus
/// `COMPILER_PB_NO_EFFECT_ASSIGNMENT = ignore`.
fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    // Preserve the code-template store of the upstream mocked
    // PreferenceManager; a real configuration update clears it.
    t.ws.settings["java"]["templates"] = json!({ "typeComment": ["/**", " * ${type_name}", " * ${tags}", " */"] });
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.noEffectAssignment".into(), "ignore".into());
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

fn lines(l: &[&str]) -> String {
    l.concat()
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_field_decl() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    Vector1 vec;\n", "}\n"]));
    let e1 = Expected::new(
        "Change to 'Vector' (java.util)",
        &lines(&["package test1;\n", "\n", "import java.util.Vector;\n", "\n", "public class E {\n", "    Vector vec;\n", "}\n"]),
    );
    let e5 = Expected::new("Add type parameter 'Vector1' to 'E'", &lines(&["package test1;\n", "public class E<Vector1> {\n", "    Vector1 vec;\n", "}\n"]));
    t.assert_code_actions(&cu, &[e1, e5]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_method_arguments() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    void foo(Vect1or[] vec) {\n", "    }\n", "}\n"]));
    let e1 = Expected::new(
        "Change to 'Vector' (java.util)",
        &lines(&["package test1;\n", "\n", "import java.util.Vector;\n", "\n", "public class E {\n", "    void foo(Vector[] vec) {\n", "    }\n", "}\n"]),
    );
    let e5 = Expected::new("Add type parameter 'Vect1or' to 'E'", &lines(&["package test1;\n", "public class E<Vect1or> {\n", "    void foo(Vect1or[] vec) {\n", "    }\n", "}\n"]));
    let e6 = Expected::new("Add type parameter 'Vect1or' to 'foo(Vect1or[])'", &lines(&["package test1;\n", "public class E {\n", "    <Vect1or> void foo(Vect1or[] vec) {\n", "    }\n", "}\n"]));
    t.assert_code_actions(&cu, &[e1, e5, e6]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_method_return_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    Vect1or[] foo() {\n", "        return null;\n", "    }\n", "}\n"]));
    let e1 = Expected::new(
        "Change to 'Vector' (java.util)",
        &lines(&["package test1;\n", "\n", "import java.util.Vector;\n", "\n", "public class E {\n", "    Vector[] foo() {\n", "        return null;\n", "    }\n", "}\n"]),
    );
    let e5 = Expected::new("Add type parameter 'Vect1or' to 'E'", &lines(&["package test1;\n", "public class E<Vect1or> {\n", "    Vect1or[] foo() {\n", "        return null;\n", "    }\n", "}\n"]));
    let e6 = Expected::new("Add type parameter 'Vect1or' to 'foo()'", &lines(&["package test1;\n", "public class E {\n", "    <Vect1or> Vect1or[] foo() {\n", "        return null;\n", "    }\n", "}\n"]));
    t.assert_code_actions(&cu, &[e1, e5, e6]);
}

#[test]
#[ignore = "@Disabled upstream: Created class doesn't extend Exception."]
fn test_type_in_exception_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    void foo() throws IOExcpetion {\n    }\n}\n");
    let e1 = Expected::new("Change to 'IOException' (java.io)", "package test1;\n\nimport java.io.IOException;\n\npublic class E {\n    void foo() throws IOException {\n    }\n}\n");
    let e2 = Expected::new("Create class 'IOExcpetion'", "package test1;\n\n/**\n * IOExcpetion\n */\npublic class IOExcpetion extends Exception {\n\n}\n");
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_var_decl_with_wildcard() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E {\n", "    void foo(ArrayList<? extends Runnable> a) {\n", "        XY v= a.get(0);\n", "    }\n", "}\n"]),
    );
    let e1 = Expected::new(
        "Change to 'Runnable' (java.lang)",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E {\n", "    void foo(ArrayList<? extends Runnable> a) {\n", "        Runnable v= a.get(0);\n", "    }\n", "}\n"]),
    );
    let e2 = Expected::new(
        "Add type parameter 'XY' to 'E'",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E<XY> {\n", "    void foo(ArrayList<? extends Runnable> a) {\n", "        XY v= a.get(0);\n", "    }\n", "}\n"]),
    );
    let e3 = Expected::new(
        "Add type parameter 'XY' to 'foo(ArrayList<? extends Runnable>)'",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E {\n", "    <XY> void foo(ArrayList<? extends Runnable> a) {\n", "        XY v= a.get(0);\n", "    }\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_type_in_statement() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E {\n", "    void foo() {\n", "        ArrayList v= new ArrayListist();\n", "    }\n", "}\n"]),
    );
    let e1 = Expected::new(
        "Change to 'ArrayList' (java.util)",
        &lines(&["package test1;\n", "import java.util.ArrayList;\n", "public class E {\n", "    void foo() {\n", "        ArrayList v= new ArrayList();\n", "    }\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_array_type_in_statement() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&["package test1;\n", "import java.io.*;\n", "public class E {\n", "    void foo() {\n", "        Serializable[] v= new ArrayListExtra[10];\n", "    }\n", "}\n"]),
    );
    let mut expected = Vec::new();
    expected.push(Expected::new(
        "Change to 'Serializable' (java.io)",
        &lines(&["package test1;\n", "import java.io.*;\n", "public class E {\n", "    void foo() {\n", "        Serializable[] v= new Serializable[10];\n", "    }\n", "}\n"]),
    ));
    expected.push(Expected::new(
        "Change to 'ArrayList' (java.util)",
        &lines(&["package test1;\n", "import java.io.*;\n", "import java.util.ArrayList;\n", "public class E {\n", "    void foo() {\n", "        Serializable[] v= new ArrayList[10];\n", "    }\n", "}\n"]),
    ));
    expected.push(Expected::new(
        "Add type parameter 'ArrayListExtra' to 'E'",
        &lines(&["package test1;\n", "import java.io.*;\n", "public class E<ArrayListExtra> {\n", "    void foo() {\n", "        Serializable[] v= new ArrayListExtra[10];\n", "    }\n", "}\n"]),
    ));
    expected.push(Expected::new(
        "Add type parameter 'ArrayListExtra' to 'foo()'",
        &lines(&["package test1;\n", "import java.io.*;\n", "public class E {\n", "    <ArrayListExtra> void foo() {\n", "        Serializable[] v= new ArrayListExtra[10];\n", "    }\n", "}\n"]),
    ));
    t.assert_code_actions(&cu, &expected);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_qualified_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    void foo() {\n", "        test2.Test t= null;\n", "    }\n", "}\n"]));
    let e1 = Expected::new("Create class 'Test' in package 'test2'", &lines(&["package test2;\n", "\n", "/**\n", " * Test\n", " */\n", "public class Test {\n", "\n", "}\n"]));
    let e2 = Expected::new("Create interface 'Test' in package 'test2'", &lines(&["package test2;\n", "\n", "/**\n", " * Test\n", " */\n", "public interface Test {\n", "\n", "}\n"]));
    let e3 = Expected::new("Create enum 'Test' in package 'test2'", &lines(&["package test2;\n", "\n", "/**\n", " * Test\n", " */\n", "public enum Test {\n", "\n", "}\n"]));
    t.assert_code_actions(&cu, &[e1, e2, e3]);
}

#[test]
fn test_inner_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    void foo() {\n", "        Object object= new F.Inner() {\n", "        };\n", "    }\n", "}\n"]));
    t.ws.create_cu(&root, "src", "test1", "F.java", &lines(&["package test1;\n", "public class F {\n", "}\n"]));
    let e1 = Expected::new(
        "Change to 'Object' (java.lang)",
        &lines(&["package test1;\n", "public class E {\n", "    void foo() {\n", "        Object object= new Object() {\n", "        };\n", "    }\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "@Disabled upstream: Created class doesn't extend Exception."]
fn test_type_in_catch_block() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    void foo() {\n        try {\n        } catch (XXX x) {\n        }\n    }\n}\n");
    let e1 = Expected::new("Create class 'XXX'", "package test1;\n\npublic class XXX extends Exception {\n\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_type_in_super_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E extends XXX {\n", "}\n"]));
    let e1 = Expected::new("Create class 'XXX'", &lines(&["package test1;\n", "\n", "/**\n", " * XXX\n", " */\n", "public class XXX {\n", "\n", "}\n"]));
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_new_cu_no_leading_blank_line() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    MyNewClass obj;\n", "}\n"]));
    let code_actions = t.evaluate_code_actions(&cu);
    for code_action in &code_actions {
        if !common::quickfix::is_command(code_action) && get_title(code_action) == "Create class 'MyNewClass'" {
            let actual = t.evaluate_code_action_command(code_action);
            assert!(!actual.starts_with('\n'), "Generated class file should not start with a blank line");
            assert!(actual.starts_with("package "), "Generated class file should start with package declaration");
            return;
        }
    }
    panic!("Expected code action 'Create class \\'MyNewClass\\'' not found");
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_type_creation() {
    let (mut t, root) = setup();
    // `clientPreferences.isResourceOperationSupported()` returns true.
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] = json!(["create", "rename", "delete"]);
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E extends XXX {\n", "}\n"]));
    let e1 = Expected::new("Create class 'XXX'", &lines(&["package test1;\n", "\n", "/**\n", " * XXX\n", " */\n", "public class XXX {\n", "\n", "}\n"]));
    t.set_only(&["quickfix"]);
    t.assert_code_action_exists_expected(&cu, &e1);
    let code_actions = t.evaluate_code_actions(&cu);
    let edit = &code_actions[0]["edit"];
    assert!(edit["documentChanges"].is_array());
    let resource_operation = &edit["documentChanges"][0];
    assert!(resource_operation.get("kind").is_some(), "{resource_operation}");
    assert_eq!("create", resource_operation["kind"]);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_type_in_super_interface() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public interface E extends XXX {\n", "}\n"]));
    let e1 = Expected::new("Create interface 'XXX'", &lines(&["package test1;\n", "\n", "/**\n", " * XXX\n", " */\n", "public interface XXX {\n", "\n", "}\n"]));
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_type_in_annotation() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "@Xyz\n", "public interface E {\n", "}\n"]));
    let e1 = Expected::new("Create annotation 'Xyz'", &lines(&["package test1;\n", "\n", "/**\n", " * Xyz\n", " */\n", "public @interface Xyz {\n", "\n", "}\n"]));
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_type_in_annotation_bug153881() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "a", "SomeClass.java", &lines(&["package a;\n", "public class SomeClass {\n", "        @scratch.Unimportant void foo() {}\n", "}\n"]));
    let e1 = Expected::new(
        "Create annotation 'Unimportant' in package 'scratch'",
        &lines(&["package scratch;\n", "\n", "/**\n", " * Unimportant\n", " */\n", "public @interface Unimportant {\n", "\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_primitive_type_in_field_decl() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public class E {\n", "    floot vec= 1.0;\n", "}\n"]));
    let mut expected = Vec::new();
    expected.push(Expected::new("Change to 'double'", &lines(&["package test1;\n", "public class E {\n", "    double vec= 1.0;\n", "}\n"])));
    expected.push(Expected::new("Change to 'Float' (java.lang)", &lines(&["package test1;\n", "public class E {\n", "    Float vec= 1.0;\n", "}\n"])));
    expected.push(Expected::new("Add type parameter 'floot' to 'E'", &lines(&["package test1;\n", "public class E<floot> {\n", "    floot vec= 1.0;\n", "}\n"])));
    expected.push(Expected::new("Change to 'float'", &lines(&["package test1;\n", "public class E {\n", "    float vec= 1.0;\n", "}\n"])));
    t.assert_code_actions(&cu, &expected);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_type_arguments1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&["package test1;\n", "public class E<T> {\n", "    class SomeType { }\n", "    void foo() {\n", "        E<XYX> list= new E<SomeType>();\n", "    }\n", "}\n"]),
    );
    let e1 = Expected::new(
        "Change to 'SomeType' (test1.E)",
        &lines(&["package test1;\n", "public class E<T> {\n", "    class SomeType { }\n", "    void foo() {\n", "        E<SomeType> list= new E<SomeType>();\n", "    }\n", "}\n"]),
    );
    let e5 = Expected::new(
        "Add type parameter 'XYX' to 'E<T>'",
        &lines(&["package test1;\n", "public class E<T, XYX> {\n", "    class SomeType { }\n", "    void foo() {\n", "        E<XYX> list= new E<SomeType>();\n", "    }\n", "}\n"]),
    );
    let e6 = Expected::new(
        "Add type parameter 'XYX' to 'foo()'",
        &lines(&["package test1;\n", "public class E<T> {\n", "    class SomeType { }\n", "    <XYX> void foo() {\n", "        E<XYX> list= new E<SomeType>();\n", "    }\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1, e5, e6]);
}

#[test]
#[ignore = "needs the type half of getTypeProposals (AddTypeParameterProposal, import-only proposals) in Rust: not ported yet"]
fn test_type_in_type_arguments2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType { }\n",
            "    void foo() {\n",
            "        E<Map<String, ? extends XYX>> list= new E<Map<String, ? extends SomeType>>() {\n",
            "        };\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e1 = Expected::new(
        "Change to 'SomeType' (test1.E)",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType { }\n",
            "    void foo() {\n",
            "        E<Map<String, ? extends SomeType>> list= new E<Map<String, ? extends SomeType>>() {\n",
            "        };\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e5 = Expected::new(
        "Add type parameter 'XYX' to 'E<T>'",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T, XYX> {\n",
            "    static class SomeType { }\n",
            "    void foo() {\n",
            "        E<Map<String, ? extends XYX>> list= new E<Map<String, ? extends SomeType>>() {\n",
            "        };\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e6 = Expected::new(
        "Add type parameter 'XYX' to 'foo()'",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType { }\n",
            "    <XYX> void foo() {\n",
            "        E<Map<String, ? extends XYX>> list= new E<Map<String, ? extends SomeType>>() {\n",
            "        };\n",
            "    }\n",
            "}\n",
        ]),
    );
    t.assert_code_actions(&cu, &[e1, e5, e6]);
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet"]
fn test_parameterized_type1() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "\n", "public class E {\n", "    void foo(XXY<String> b) {\n", "        b.foo();\n", "    }\n", "}\n"]));
    let e1 = Expected::new("Create class 'XXY<T>'", &lines(&["package test1;\n", "\n", "/**\n", " * XXY\n", " */\n", "public class XXY<T> {\n", "\n", "}\n"]));
    let e2 = Expected::new("Create interface 'XXY<T>'", &lines(&["package test1;\n", "\n", "/**\n", " * XXY\n", " */\n", "public interface XXY<T> {\n", "\n", "}\n"]));
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
fn test_parameterized_type2() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType<S1, S2> { }\n",
            "    void foo() {\n",
            "        SomeType<String, String> list= new XXY<String, String>() { };\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e1 = Expected::new(
        "Change type of 'list' to 'XXY<String, String>'",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType<S1, S2> { }\n",
            "    void foo() {\n",
            "        XXY<String, String> list= new XXY<String, String>() { };\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e2 = Expected::new(
        "Change to 'SomeType' (test1.E)",
        &lines(&[
            "package test1;\n",
            "import java.util.Map;\n",
            "public class E<T> {\n",
            "    static class SomeType<S1, S2> { }\n",
            "    void foo() {\n",
            "        SomeType<String, String> list= new SomeType<String, String>() { };\n",
            "    }\n",
            "}\n",
        ]),
    );
    t.assert_code_actions(&cu, &[e1, e2]);
}

/// `createSomeAmbiguity(ifc, isException)`.
fn create_some_ambiguity(t: &mut QuickFixTest, root: &Path, ifc: bool, is_exception: bool) {
    let kind = if ifc { "interface" } else { "class" };
    let ext = if is_exception { "extends Exception " } else { "" };
    t.ws.create_cu(root, "src", "test3", "A.java", &format!("package test3;\npublic {kind} A {ext}{{\n}}\n"));
    t.ws.create_cu(root, "src", "test3", "B.java", "package test3;\npublic class B {\n}\n");
    t.ws.create_cu(root, "src", "test2", "A.java", &format!("package test2;\npublic {kind} A {ext}{{\n}}\n"));
    t.ws.create_cu(root, "src", "test2", "C.java", "package test2;\npublic class C {\n}\n");
}

fn ambiguity_test(ifc: bool, is_exception: bool, body: &[&str]) {
    let (mut t, root) = setup();
    create_some_ambiguity(&mut t, &root, ifc, is_exception);
    let mut src = vec!["package test1;\n", "import test2.*;\n", "import test3.*;\n"];
    src.extend_from_slice(body);
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&src));
    let mut e1 = vec!["package test1;\n", "import test2.*;\n", "import test2.A;\n", "import test3.*;\n"];
    e1.extend_from_slice(body);
    let e1 = Expected::new("Explicitly import 'test2.A'", &lines(&e1));
    let mut e2 = vec!["package test1;\n", "import test2.*;\n", "import test3.*;\n", "import test3.A;\n"];
    e2.extend_from_slice(body);
    let e2 = Expected::new("Explicitly import 'test3.A'", &lines(&e2));
    t.assert_code_actions(&cu, &[e1, e2]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_super_class() {
    ambiguity_test(false, false, &["public class E extends A {\n", "    B b;\n", "    C c;\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_interface() {
    ambiguity_test(true, false, &["public class E implements A {\n", "    B b;\n", "    C c;\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_field() {
    ambiguity_test(true, false, &["public class E {\n", "    A a;\n", "    B b;\n", "    C c;\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_argument() {
    ambiguity_test(true, false, &["public class E {\n", "    B b;\n", "    C c;\n", "    public void foo(A a) {", "    }\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_return_type() {
    ambiguity_test(false, false, &["public class E {\n", "    B b;\n", "    C c;\n", "    public A foo() {", "        return null;\n", "    }\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_exception_type() {
    ambiguity_test(false, true, &["public class E {\n", "    B b;\n", "    C c;\n", "    public void foo() throws A {", "    }\n", "}\n"]);
}

#[test]
#[ignore = "needs UnresolvedElementsSubProcessor.getAmbiguousTypeReferenceProposals (AmbiguousType 'Explicitly import') in Rust: not ported yet"]
fn test_ambiguous_type_in_catch_block() {
    ambiguity_test(false, true, &["public class E {\n", "    B b;\n", "    C c;\n", "    public void foo() {", "        try {\n", "        } catch (A e) {\n", "        }\n", "    }\n", "}\n"]);
}

/// Offers to raise visibility of method instead of class.
/// https://bugs.eclipse.org/bugs/show_bug.cgi?id=94755
#[test]
fn test_indirect_ref_default_class() {
    let (mut t, root) = setup();
    t.ws.create_cu(&root, "src", "test1", "B.java", "package test1;\nclass B {\n    public Object get(Object c) {\n    \treturn null;\n    }\n}\n");
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    B b = new B();\n    public B getB() {\n    \treturn b;\n    }\n}\n");
    let cu = t.ws.create_cu(&root, "src", "test2", "C.java", "package test2;\nimport test1.A;\npublic class C {\n    public Object getSide(A a) {\n    \treturn a.getB().get(this);\n    }\n}\n");
    let e1 = Expected::new("Change visibility of 'B' to 'public'", "package test1;\npublic class B {\n    public Object get(Object c) {\n    \treturn null;\n    }\n}\n");
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_for_each_missing_type() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(
        &root,
        "src",
        "pack",
        "E.java",
        &lines(&[
            "package pack;\n",
            "\n",
            "import java.util.*;\n",
            "\n",
            "public class E {\n",
            "    public void foo(ArrayList<? extends HashSet<? super Integer>> list) {\n",
            "        for (element: list) {\n",
            "        }\n",
            "    }\n",
            "}\n",
        ]),
    );
    let e1 = Expected::new(
        "Create loop variable 'element'",
        &lines(&[
            "package pack;\n",
            "\n",
            "import java.util.*;\n",
            "\n",
            "public class E {\n",
            "    public void foo(ArrayList<? extends HashSet<? super Integer>> list) {\n",
            "        for (HashSet<? super Integer> element: list) {\n",
            "        }\n",
            "    }\n",
            "}\n",
        ]),
    );
    t.assert_code_action_exists_expected(&cu, &e1);
}

#[test]
fn test_dont_import_test_classes_in_main_code() {
    let (mut t, root) = setup();
    // `JavaProjectHelper.addSourceContainer(fJProject1, "src-tests", new Path[0],
    // new Path[0], "bin-tests", { test=true })`.
    std::fs::create_dir_all(root.join("src-tests")).unwrap();
    std::fs::write(
        root.join(".classpath"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<classpath>\n\t<classpathentry kind=\"src\" path=\"src\"/>\n\t<classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/>\n\t<classpathentry kind=\"src\" output=\"bin-tests\" path=\"src-tests\">\n\t\t<attributes>\n\t\t\t<attribute name=\"test\" value=\"true\"/>\n\t\t</attributes>\n\t</classpathentry>\n\t<classpathentry kind=\"output\" path=\"bin\"/>\n</classpath>\n",
    )
    .unwrap();
    let cu1 = t.ws.create_cu(&root, "src", "pp", "C1.java", &lines(&["package pp;\n", "public class C1 {\n", "    Tests at=new Tests();\n", "}\n"]));
    t.ws.create_cu(&root, "src-tests", "pt", "Tests.java", &lines(&["package pt;\n", "public class Tests {\n", "}\n"]));
    t.assert_code_action_not_exists(&cu1, "Import 'Tests' (pt)");
}

#[test]
#[ignore = "needs NewCUProposal (Create class/interface/enum/annotation) in Rust: not ported yet (oracle differs too: it indents the added method with spaces)"]
fn test_type_in_sealed_type_declaration() {
    let (mut t, root) = setup();
    let mut options17 = BTreeMap::new();
    // `JavaModelUtil.setComplianceOptions(options17, VERSION_17)`.
    for key in ["compliance", "source", "codegen.targetPlatform"] {
        options17.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "17".to_owned());
    }
    options17.insert("org.eclipse.jdt.core.compiler.problem.assertIdentifier".into(), "error".into());
    options17.insert("org.eclipse.jdt.core.compiler.problem.enumIdentifier".into(), "error".into());
    options17.insert("org.eclipse.jdt.core.compiler.codegen.inlineJsrBytecode".into(), "enabled".into());
    t.ws.set_project_options(&root, &options17);
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &lines(&["package test1;\n", "public sealed interface E permits F {\n", "void methodE();\n", "}"]));
    let e1 = Expected::new(
        "Create class 'F'",
        &lines(&[
            "package test1;\n",
            "\n",
            "/**\n",
            " * F\n",
            " */\n",
            "public final class F implements E {\n",
            "\n",
            "\t@Override\n",
            "\tpublic void methodE() {\n",
            "\t\t// TODO Auto-generated method stub\n",
            "\t\tthrow new UnsupportedOperationException(\"Unimplemented method 'methodE'\");\n",
            "\t}\n",
            "\n",
            "}\n",
        ]),
    );
    t.assert_code_actions(&cu, &[e1]);
}

#[test]
#[ignore = "needs AddImportCorrectionProposal + QuickFixProcessor.addAddAllMissingImportsProposal in Rust: not ported yet (also fails on the oracle: no 'Add all missing imports' quick fix offered)"]
fn test_add_all_missing_imports() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &lines(&["package test1;\n", "\n", "public class E {\n", "    Vector vec;\n", "    List<String> b;\n", "}\n"]));
    // assert quick fix exists
    t.assert_code_action_exists(&cu, "Add all missing imports");

    // use source action to test the return TextEdit
    t.set_ignored_kind(&["quickfix"]);
    let e1 = Expected::new(
        "Add all missing imports",
        &lines(&["package test1;\n", "\n", "import java.util.List;\n", "import java.util.Vector;\n", "\n", "public class E {\n", "    Vector vec;\n", "    List<String> b;\n", "}\n"]),
    );
    t.assert_code_actions(&cu, &[e1]);

    // restore the ignored kind
    t.set_ignored_kind(&["source.*"]);
}

#[test]
#[ignore = "needs AddImportCorrectionProposal + QuickFixProcessor.addAddAllMissingImportsProposal in Rust: not ported yet"]
fn test_multiple_add_all_missing_imports() {
    let (mut t, root) = setup();
    let cu = t.ws.create_cu(&root, "src", "pack", "E.java", &lines(&["package test1;\n", "\n", "public class E {\n", "    @Retention(RetentionPolicy.RUNTIME)\n", "    private void test() {\n", "    }\n", "}\n"]));
    // assert quick fix exists
    let code_actions = t.evaluate_code_actions_range(&cu, range(3, 4, 3, 4));
    let add_all_missing_imports_actions: Vec<_> = code_actions
        .iter()
        .filter(|a| !common::quickfix::is_command(a))
        .filter(|a| a["kind"] == "quickfix" && a["title"] == "Add all missing imports")
        .collect();
    assert_eq!(1, add_all_missing_imports_actions.len());
}

#[test]
fn test_ignore_type_filter() {
    let (mut t, root) = setup();
    let before = "package test1;\nimport java.util.ArrayList;\npublic class E {\n\tvoid foo() {\n\t\tList v= new ArrayList();\n\t}\n}";
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", before);
    let after = "package test1;\nimport java.util.ArrayList;\nimport java.util.List;\npublic class E {\n\tvoid foo() {\n\t\tList v= new ArrayList();\n\t}\n}";
    let e1 = Expected::new("Import 'List' (java.util)", after);
    t.assert_code_actions(&cu, &[e1]);
}
